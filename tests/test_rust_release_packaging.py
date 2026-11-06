from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import sys

import pytest


SCRIPT = Path(__file__).resolve().parents[1] / "packaging" / "split_rust_release.py"
SPEC = importlib.util.spec_from_file_location("split_rust_release", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
split_rust_release = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(split_rust_release)


def test_split_records_exact_part_sizes_and_hashes(tmp_path: Path, monkeypatch) -> None:
    source = tmp_path / "sample-setup.exe"
    destination = tmp_path / "parts"
    destination.mkdir()
    payload = b"VietKara Rust release payload"
    source.write_bytes(payload)
    monkeypatch.setattr(split_rust_release, "CHUNK_BYTES", 7)

    manifest = split_rust_release.split(source, destination)

    parts = manifest["parts"]
    assert [part["name"] for part in parts] == [
        "sample-setup.exe.001",
        "sample-setup.exe.002",
        "sample-setup.exe.003",
        "sample-setup.exe.004",
        "sample-setup.exe.005",
    ]
    assert [part["size"] for part in parts] == [7, 7, 7, 7, 1]
    assert sum(part["size"] for part in parts) == len(payload)
    assert b"".join((destination / part["name"]).read_bytes() for part in parts) == payload
    assert manifest["size"] == len(payload)
    assert manifest["sha256"] == split_rust_release.digest_file(source)
    assert all(
        part["sha256"] == split_rust_release.digest_file(destination / part["name"])
        for part in parts
    )


def test_split_rejects_empty_artifact(tmp_path: Path) -> None:
    source = tmp_path / "empty.exe"
    destination = tmp_path / "parts"
    source.touch()
    destination.mkdir()

    with pytest.raises(SystemExit, match="must not be empty"):
        split_rust_release.split(source, destination)


def test_release_output_includes_joiner_and_manifest(tmp_path: Path, monkeypatch) -> None:
    root = tmp_path / "repo"
    releases = root / "releases"
    packaging = root / "packaging"
    releases.mkdir(parents=True)
    packaging.mkdir()
    source = releases / "VietKaraStudio-Rust-0.1.0-Setup-x64.exe"
    source.write_bytes(b"setup bytes")
    for name in ("Ghep-File.ps1", "Ghep-File.cmd"):
        (packaging / name).write_text(name, encoding="utf-8")

    monkeypatch.setattr(split_rust_release, "ROOT", root)
    monkeypatch.setattr(split_rust_release, "CHUNK_BYTES", 5)
    monkeypatch.setattr(
        sys,
        "argv",
        ["split_rust_release.py", str(source)],
    )
    assert split_rust_release.main() == 0

    output = releases / "rust-github"
    manifest = json.loads((output / "downloads.json").read_text(encoding="utf-8"))
    assert manifest[0]["name"] == source.name
    assert (output / "Ghep-File.ps1").read_text(encoding="utf-8") == "Ghep-File.ps1"
    assert (output / "Ghep-File.cmd").read_text(encoding="utf-8") == "Ghep-File.cmd"
    assert (output / "HUONG-DAN.txt").is_file()
    assert (output / "SHA256SUMS.txt").is_file()
