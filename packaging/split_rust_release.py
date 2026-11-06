"""Split Rust Setup/Portable artifacts into GitHub's per-asset size range."""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import shutil

ROOT = Path(__file__).resolve().parents[1]
CHUNK_BYTES = 1800 * 1024 * 1024


def digest_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def split(source: Path, destination: Path) -> dict:
    source_size = source.stat().st_size
    if source_size <= 0:
        raise SystemExit(f"Artifact must not be empty: {source}")
    whole = hashlib.sha256()
    parts = []
    total_size = 0
    with source.open("rb") as stream:
        index = 1
        while True:
            part = destination / f"{source.name}.{index:03}"
            digest = hashlib.sha256()
            size = 0
            with part.open("xb") as output:
                while size < CHUNK_BYTES:
                    chunk = stream.read(min(1024 * 1024, CHUNK_BYTES - size))
                    if not chunk:
                        break
                    output.write(chunk)
                    whole.update(chunk)
                    digest.update(chunk)
                    size += len(chunk)
            if size == 0:
                part.unlink()
                break
            parts.append({"name": part.name, "size": size, "sha256": digest.hexdigest()})
            total_size += size
            print(f"{part.name}: {size:,} bytes", flush=True)
            index += 1
    if total_size != source_size:
        raise SystemExit(f"Artifact changed while splitting: {source}")
    return {"name": source.name, "size": source_size,
            "sha256": whole.hexdigest(), "parts": parts}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("artifacts", nargs="+", type=Path,
                        help="Rust Setup/Portable ZIP files under repository releases/")
    parser.add_argument("--output", type=Path, default=ROOT / "releases/rust-github")
    args = parser.parse_args()
    sources = [path.resolve() for path in args.artifacts]
    releases = (ROOT / "releases").resolve()
    if any(not path.is_relative_to(releases) or not path.is_file() for path in sources):
        raise SystemExit("Every source artifact must be a file under repository releases/.")
    if CHUNK_BYTES <= 0 or any(path.stat().st_size <= 0 for path in sources):
        raise SystemExit("Chunk size and source artifact sizes must be positive.")
    if len({path.name for path in sources}) != len(sources):
        raise SystemExit("Artifact basenames must be unique.")
    output = args.output.resolve()
    if not output.is_relative_to(releases) or output == releases:
        raise SystemExit("Output must be a child directory of repository releases/.")
    if output.exists() and any(output.iterdir()):
        raise SystemExit(f"Refusing to overwrite non-empty split output: {output}")
    output.mkdir(parents=True, exist_ok=True)
    manifest = [split(source, output) for source in sources]
    (output / "downloads.json").write_text(json.dumps(manifest, indent=2), encoding="utf-8")
    sums = "".join(f"{item['sha256']}  {item['name']}\n" for item in manifest)
    (output / "SHA256SUMS.txt").write_text(sums, encoding="ascii")
    shutil.copy2(ROOT / "packaging/Ghep-File.ps1", output / "Ghep-File.ps1")
    shutil.copy2(ROOT / "packaging/Ghep-File.cmd", output / "Ghep-File.cmd")
    (output / "HUONG-DAN.txt").write_text(
        "VietKara Studio Rust/GPUI\n\n"
        "Tai cac part .001, .002... cua Setup hoac Portable, cung voi "
        "downloads.json, Ghep-File.ps1 va Ghep-File.cmd vao cung mot thu muc.\n"
        "Chay Ghep-File.cmd de kiem tra SHA-256 va ghep artifact.\n"
        "Can dung luong trong bang kich thuoc artifact de ghep file.\n"
        "Setup: chay file -Setup-x64.exe sau khi ghep.\n"
        "Portable: giai nen file -Portable-x64.zip sau khi ghep.\n"
        "Khong xoa thu muc data/ hoac cache model khi cap nhat ban portable.\n",
        encoding="utf-8",
    )
    print(f"Release parts ready: {output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
