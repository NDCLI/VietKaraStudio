"""Build a per-user Rust installer from a verified portable stage."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
RELEASES = ROOT / "releases"


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def verify_stage(stage: Path, manifest_path: Path, manifest: dict) -> None:
    expected = {
        item["path"]: item for item in manifest.get("files", [])
        if "__pycache__" not in Path(item["path"]).parts
        and Path(item["path"]).suffix not in {".pyc", ".pyo"}
    }
    entries = list(stage.rglob("*"))
    if any(path.is_symlink() for path in entries):
        raise SystemExit("Stage contains a symbolic link; refusing to install external files.")
    # Python may create bytecode during packaged-runtime smoke tests. The installer
    # explicitly excludes these transient files, so they are not manifest payload.
    files = [
        path for path in entries
        if path.is_file()
        and path != manifest_path
        and "__pycache__" not in path.parts
        and path.suffix not in {".pyc", ".pyo"}
    ]
    actual_names = {path.relative_to(stage).as_posix() for path in files}
    if actual_names != expected.keys():
        raise SystemExit("Stage contents do not match BUILD-INFO.json.")
    for index, path in enumerate(files, start=1):
        name = path.relative_to(stage).as_posix()
        expected_file = expected[name]
        if path.stat().st_size != expected_file["size"] or sha256(path) != expected_file["sha256"]:
            raise SystemExit(f"Stage file does not match manifest: {name}")
        if index % 1000 == 0:
            print(f"Verified installer input {index}/{len(files)} files", flush=True)


def find_iscc() -> Path | None:
    candidates = [
        os.environ.get("ISCC_EXE"),
        os.environ.get("INNO_SETUP_DIR") and str(Path(os.environ["INNO_SETUP_DIR"]) / "ISCC.exe"),
        str(Path(os.environ.get("ProgramFiles", r"C:\Program Files")) / "Inno Setup 7/ISCC.exe"),
        str(Path(os.environ.get("ProgramFiles(x86)", r"C:\Program Files (x86)")) / "Inno Setup 7/ISCC.exe"),
        str(Path(os.environ.get("ProgramFiles(x86)", r"C:\Program Files (x86)")) / "Inno Setup 6/ISCC.exe"),
        str(Path(os.environ.get("ProgramFiles", r"C:\Program Files")) / "Inno Setup 6/ISCC.exe"),
    ]
    return next((Path(path) for path in candidates if path and Path(path).is_file()), None)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("stage", type=Path, help="Portable stage under repository releases/")
    parser.add_argument("--version", default="0.1.0")
    parser.add_argument(
        "--output",
        type=Path,
        help="Optional Setup output path under releases/; default is the release asset name",
    )
    parser.add_argument("--iscc", type=Path, help="Path to Inno Setup 6 ISCC.exe")
    parser.add_argument("--csc", type=Path, help="Path to .NET Framework 64-bit csc.exe")
    parser.add_argument("--verify-only", action="store_true",
                        help="Validate every stage file against BUILD-INFO.json and exit")
    args = parser.parse_args()
    if not re.fullmatch(r"\d+\.\d+\.\d+", args.version):
        raise SystemExit("Version must use MAJOR.MINOR.PATCH.")

    stage = args.stage.resolve()
    if not stage.is_relative_to(RELEASES.resolve()) or stage == RELEASES.resolve():
        raise SystemExit("Stage must be a child directory of repository releases/.")
    manifest = stage / "BUILD-INFO.json"
    if not manifest.is_file():
        raise SystemExit(f"Missing package manifest: {manifest}")
    info = json.loads(manifest.read_text(encoding="utf-8"))
    if info.get("architecture") != "Windows x64" or info.get("user_data_bundled") is not False:
        raise SystemExit("Stage manifest is not a Windows x64 package without user data.")
    if info.get("version") != args.version:
        raise SystemExit("Installer version does not match stage BUILD-INFO.json.")
    if (stage / "data").exists():
        raise SystemExit("Stage contains data/; refusing to package user data.")
    verify_stage(stage, manifest, info)
    if args.verify_only:
        print(f"Portable stage verified: {stage}")
        return 0

    iscc = (args.iscc or find_iscc())
    if iscc is None or not iscc.is_file():
        raise SystemExit("Inno Setup 6 compiler not found; pass --iscc or set ISCC_EXE.")
    csc = args.csc or Path(os.environ.get("WINDIR", r"C:\Windows")) / "Microsoft.NET/Framework64/v4.0.30319/csc.exe"
    if not csc.is_file():
        raise SystemExit(f".NET Framework C# compiler not found: {csc}")

    release_installer = RELEASES / f"VietKaraStudio-Rust-{args.version}-Setup-x64.exe"
    installer = args.output.resolve() if args.output else release_installer
    if not installer.is_relative_to(RELEASES.resolve()):
        raise SystemExit("Installer output must be a file under repository releases/.")
    checksum = installer.with_suffix(installer.suffix + ".sha256")
    if installer.exists() or checksum.exists():
        raise SystemExit(f"Refusing to overwrite installer/checksum: {installer}")

    RELEASES.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="rust-installer-", dir=RELEASES) as temporary:
        compiler_output = Path(temporary) / "output"
        compiler_output.mkdir()
        launcher = Path(temporary) / "VietKaraStudio.exe"
        updater = Path(temporary) / "VietKaraUpdater.exe"
        subprocess.run([
            str(csc), "/nologo", "/target:winexe", "/optimize+", "/platform:x64",
            "/reference:System.Windows.Forms.dll", f"/out:{launcher}",
            str(ROOT / "packaging/RustLauncher.cs"),
        ], check=True)
        subprocess.run([
            str(csc), "/nologo", "/target:winexe", "/optimize+", "/platform:x64",
            "/reference:System.Windows.Forms.dll", "/reference:System.Web.Extensions.dll",
            f"/out:{updater}", str(ROOT / "packaging/RustUpdateHelper.cs"),
        ], check=True)
        subprocess.run([
            str(iscc), "/Qp", f"/DSourceDir={stage}", f"/DLauncherPath={launcher}",
            f"/DUpdaterPath={updater}",
            f"/DAppVersion={args.version}", f"/DInstallerOutputDir={compiler_output}",
            str(ROOT / "packaging/VietKaraStudio-Rust.iss"),
        ], check=True)

        compiled_installer = compiler_output / release_installer.name
        if not compiled_installer.is_file():
            raise SystemExit(f"Inno Setup did not produce expected installer: {compiled_installer}")
        compiled_installer.replace(installer)

    if not installer.is_file():
        raise SystemExit(f"Inno Setup did not produce expected installer: {installer}")
    digest = sha256(installer)
    with checksum.open("x", encoding="ascii") as stream:
        stream.write(f"{digest}  {installer.name}\n")
    print(f"Installer: {installer} ({installer.stat().st_size:,} bytes)")
    print(f"SHA-256: {digest}")
    print(f"Checksum: {checksum}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
