"""Verify and archive a Rust portable stage without packaging user data."""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import zipfile

ROOT = Path(__file__).resolve().parents[1]
CHUNK = 1024 * 1024


def archive_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(CHUNK), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("stage", type=Path, help="Existing portable stage under releases/")
    parser.add_argument(
        "--checksum-output",
        type=Path,
        help="Optional checksum path under releases/ (default: version-based filename)",
    )
    args = parser.parse_args()
    stage = args.stage.resolve()
    releases = (ROOT / "releases").resolve()
    if not stage.is_relative_to(releases) or stage == releases:
        raise SystemExit("Stage must be a child directory of repository releases/.")
    manifest_path = stage / "BUILD-INFO.json"
    if not manifest_path.is_file():
        raise SystemExit(f"Missing package manifest: {manifest_path}")
    if (stage / "data").exists():
        raise SystemExit("Stage contains data/; remove user data before archiving.")
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    expected = {item["path"]: item for item in manifest.get("files", [])}
    archive = stage.with_suffix(".zip")
    sums = (
        args.checksum_output.resolve()
        if args.checksum_output
        else stage.parent / f"SHA256SUMS-{manifest.get('version', 'unknown')}-RustPortable.txt"
    )
    if not sums.is_relative_to(releases) or sums == releases:
        raise SystemExit("Checksum output must be a file under releases/.")
    temporary = archive.with_suffix(archive.suffix + ".partial")
    sums_temporary = sums.with_suffix(sums.suffix + ".partial")
    for path in (archive, sums, temporary, sums_temporary):
        if path.exists():
            raise SystemExit(f"Refusing to overwrite existing artifact: {path}")

    entries = list(stage.rglob("*"))
    if any(path.is_symlink() for path in entries):
        raise SystemExit("Stage contains a symbolic link; refusing to archive external files.")
    # Packaged Python smoke/runtime checks may create bytecode after the stage
    # manifest was written. Match build_rust_installer.py: these files are
    # transient and are excluded from the installer payload as well.
    files = sorted(
        path
        for path in entries
        if path.is_file()
        and "__pycache__" not in path.parts
        and path.suffix not in {".pyc", ".pyo"}
    )
    listed = {path.relative_to(stage).as_posix() for path in files if path != manifest_path}
    if listed != expected.keys():
        missing = sorted(expected.keys() - listed)[:10]
        unlisted = sorted(listed - expected.keys())[:10]
        raise SystemExit(f"Manifest file set differs (missing={missing}, unlisted={unlisted}).")

    try:
        with zipfile.ZipFile(temporary, "x", compression=zipfile.ZIP_DEFLATED,
                             compresslevel=1, allowZip64=True) as output:
            for index, path in enumerate(files, start=1):
                relative = path.relative_to(stage).as_posix()
                digest = hashlib.sha256()
                if path == manifest_path:
                    with path.open("rb") as source, output.open(f"{stage.name}/{relative}", "w") as target:
                        for chunk in iter(lambda: source.read(CHUNK), b""):
                            target.write(chunk)
                    continue
                info = expected[relative]
                if path.stat().st_size != info["size"]:
                    raise SystemExit(f"Size mismatch: {relative}")
                with path.open("rb") as source, output.open(f"{stage.name}/{relative}", "w") as target:
                    for chunk in iter(lambda: source.read(CHUNK), b""):
                        digest.update(chunk)
                        target.write(chunk)
                if digest.hexdigest() != info["sha256"]:
                    raise SystemExit(f"SHA-256 mismatch: {relative}")
                if index % 1000 == 0:
                    print(f"Verified and archived {index}/{len(files)} files", flush=True)
        archive_hash = archive_sha256(temporary)
        temporary.rename(archive)
        sums_temporary.write_text(f"{archive_hash}  {archive.name}\n", encoding="ascii")
        sums_temporary.rename(sums)
    except Exception:
        temporary.unlink(missing_ok=True)
        sums_temporary.unlink(missing_ok=True)
        raise

    print(f"Archive: {archive} ({archive.stat().st_size:,} bytes)")
    print(f"SHA-256: {archive_hash}")
    print(f"Checksums: {sums}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
