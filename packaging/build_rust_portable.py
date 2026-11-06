"""Assemble the Rust/GPUI Windows app and its explicitly supplied runtimes."""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
WORKER_FILES = (
    "ai_worker.py",
    "worker.py",
    "hardware.py",
    "app_runtime.py",
    "duet_roles.py",
    "audio_sync.py",
    "analysis_cache.py",
    "project_archive.py",
)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def require_file(path: Path, description: str) -> Path:
    if not path.is_file():
        raise SystemExit(f"Missing {description}: {path}")
    return path


def copy_runtime(source: Path, destination: Path, site_packages: Path) -> None:
    require_file(source / "python.exe", "Python runtime python.exe")
    license_file = next((path for path in (source / "LICENSE.txt", source / "LICENSE") if path.is_file()), None)
    if license_file is None:
        raise SystemExit(f"Python license not found in runtime: {source}")
    require_file(site_packages / "numpy" / "__init__.py", "AI site-packages numpy")
    require_file(site_packages / "scipy" / "__init__.py", "AI site-packages scipy")
    require_file(site_packages / "torch" / "__init__.py", "AI site-packages torch")
    require_file(site_packages / "stable_whisper" / "__init__.py", "AI site-packages stable_whisper")
    shutil.copytree(source, destination, ignore=lambda folder, names: (
        list(shutil.ignore_patterns(
        "__pycache__", "*.pyc", "*.pyo", "pytest", "_pytest", "pip", "pip-*",
        )(folder, names)) + (["site-packages"] if Path(folder).resolve() == (source / "Lib").resolve() else [])))
    packaged_site = destination / "Lib/site-packages"
    packaged_site.parent.mkdir(parents=True, exist_ok=True)
    shutil.copytree(site_packages, packaged_site, dirs_exist_ok=True,
                    ignore=shutil.ignore_patterns("__pycache__", "*.pyc", "*.pyo", "pytest", "_pytest"))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--python-home", type=Path, required=True,
                        help="Python 3.12 x64 installation root")
    parser.add_argument("--site-packages", type=Path, required=True,
                        help="Provisioned Lib/site-packages directory with AI dependencies")
    parser.add_argument("--ffmpeg-dir", type=Path, required=True,
                        help="Directory containing ffmpeg.exe and ffprobe.exe")
    parser.add_argument("--rust-exe", type=Path, default=ROOT / "target/release/vietkara-app.exe")
    parser.add_argument("--output", type=Path,
                        default=ROOT / "releases/VietKaraStudio-Rust-Portable-x64")
    parser.add_argument("--version", default="0.1.0")
    args = parser.parse_args()

    rust_exe = require_file(args.rust_exe.resolve(), "Rust release executable")
    python_home = args.python_home.resolve()
    site_packages = args.site_packages.resolve()
    ffmpeg_dir = args.ffmpeg_dir.resolve()
    ffmpeg = require_file(ffmpeg_dir / "ffmpeg.exe", "FFmpeg executable")
    ffprobe = require_file(ffmpeg_dir / "ffprobe.exe", "FFprobe executable")
    output = args.output.resolve()
    if output.exists():
        raise SystemExit(f"Output already exists; choose an empty output path: {output}")
    if output in ROOT.parents or output == ROOT:
        raise SystemExit("Refusing to create output outside the releases subtree")
    if not output.is_relative_to((ROOT / "releases").resolve()):
        raise SystemExit("Output must be inside the repository releases/ directory")

    output.mkdir(parents=True)
    try:
        copy_runtime(python_home, output / "runtime", site_packages)
        (output / "app").mkdir()
        shutil.copy2(rust_exe, output / "app/vietkara-app.exe")
        shutil.copy2(ffmpeg, output / "app/ffmpeg.exe")
        shutil.copy2(ffprobe, output / "app/ffprobe.exe")

        worker = output / "worker"
        worker.mkdir()
        for name in WORKER_FILES:
            shutil.copy2(require_file(ROOT / name, f"AI worker module {name}"), worker / name)

        license_result = subprocess.run([str(ffmpeg), "-L"], capture_output=True, check=True)
        python_version = subprocess.run(
            [str(output / "runtime/python.exe"), "--version"],
            capture_output=True, check=True, text=True, timeout=15,
        )
        subprocess.run(
            [str(output / "runtime/python.exe"), "-c",
             "import numpy, scipy, soundfile, stable_whisper, demucs, torch"],
            cwd=output / "worker", capture_output=True, check=True, timeout=180,
        )
        ffmpeg_version = subprocess.run(
            [str(ffmpeg), "-version"], capture_output=True, check=True, text=True, timeout=15,
        )
        ffprobe_version = subprocess.run(
            [str(ffprobe), "-version"], capture_output=True, check=True, text=True, timeout=15,
        )
        (output / "licenses").mkdir()
        (output / "licenses/FFMPEG-LICENSE.txt").write_bytes(
            license_result.stdout + license_result.stderr +
            b"\nBuild/source information: https://ffmpeg.org/\n")
        python_license = next(path for path in (output / "runtime/LICENSE.txt", output / "runtime/LICENSE") if path.is_file())
        shutil.copy2(python_license, output / "licenses/PYTHON-LICENSE.txt")

        (output / "VietKaraStudio.cmd").write_text(
            '@echo off\r\n'
            'setlocal\r\n'
            'cd /d "%~dp0"\r\n'
            'if not exist "%~dp0data" mkdir "%~dp0data"\r\n'
            'set "OBS_DATA_DIR=%~dp0data"\r\n'
            'set "PYTHONUTF8=1"\r\n'
            'set "VIETKARA_PYTHON=%~dp0runtime\\python.exe"\r\n'
            'set "VIETKARA_AI_WORKER=%~dp0worker\\ai_worker.py"\r\n'
            'set "OBS_FFMPEG=%~dp0app\\ffmpeg.exe"\r\n'
            'set "OBS_FFPROBE=%~dp0app\\ffprobe.exe"\r\n'
            'start "" "%~dp0app\\vietkara-app.exe"\r\n',
            encoding="ascii")
        (output / "README-VI.txt").write_text(
            f"""VIETKARA STUDIO {args.version} - RUST/GPUI PORTABLE x64

Chay VietKaraStudio.cmd. Goi nay gom app Rust, Python runtime/AI dependencies,
FFmpeg va worker AI. Khong kem model AI; model duoc tai boi worker vao data/.
Project, preset, cache, model va diagnostics duoc luu trong data/ ben canh goi.
Sao luu data/ truoc khi cap nhat goi portable.

Dieu kien build: app release Windows x64, runtime Python x64 da cai dependency
cua repository, FFmpeg/FFprobe x64. Xem BUILD-INFO.json va licenses/.
""", encoding="utf-8")

        files = sorted(path for path in output.rglob("*") if path.is_file())
        manifest = {
            "app": "VietKaraStudio Rust/GPUI",
            "version": args.version,
            "architecture": "Windows x64",
            "built_utc": datetime.now(timezone.utc).isoformat(),
            "python": (python_version.stdout or python_version.stderr).strip(),
            "ffmpeg": (ffmpeg_version.stdout or ffmpeg_version.stderr).splitlines()[0],
            "ffprobe": (ffprobe_version.stdout or ffprobe_version.stderr).splitlines()[0],
            "ai_models_bundled": False,
            "user_data_bundled": False,
            "files": [{"path": path.relative_to(output).as_posix(),
                       "size": path.stat().st_size, "sha256": sha256(path)} for path in files],
        }
        (output / "BUILD-INFO.json").write_text(json.dumps(manifest, indent=2), encoding="utf-8")
    except Exception:
        # Keep a failed stage for inspection; never recursively delete user data.
        raise

    print(f"Portable Rust stage ready: {output}")
    print(f"Files: {len(list(output.rglob('*')))}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
