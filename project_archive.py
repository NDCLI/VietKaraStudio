"""Project version snapshots and portable project archives."""
from __future__ import annotations

import hashlib
import json
import os
import re
import shutil
import time
import uuid
import zipfile
from pathlib import Path


FIXED_MEDIA = {
    "reference.wav", "beat.wav", "vocals.wav", "reference_instrumental.wav",
    "background.jpg", "logo.png", "countdown.png",
}
MEDIA_PATTERN = re.compile(
    r"^(?:reference|beat|background)_source\.[a-z0-9]{1,8}$|"
    r"^(?:reference|beat|background)_preview\.mp4$"
)
MAX_ARCHIVE_SIZE = 3 * 1024 ** 3


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def managed_media(path: Path) -> list[Path]:
    return sorted((item for item in path.iterdir() if item.is_file() and
                   (item.name in FIXED_MEDIA or MEDIA_PATTERN.fullmatch(item.name))),
                  key=lambda item: item.name)


def media_manifest(path: Path) -> list[dict]:
    output = []
    for item in managed_media(path):
        output.append({"name": item.name, "size": item.stat().st_size, "sha256": sha256(item)})
    return output


def create_version(path: Path, project: dict, note: str = "") -> dict:
    versions = path / "versions"
    versions.mkdir(parents=True, exist_ok=True)
    identifier = uuid.uuid4().hex
    target = versions / identifier
    assets = target / "media"
    assets.mkdir(parents=True)
    (target / "project.json").write_text(json.dumps(project, ensure_ascii=False, indent=2), encoding="utf-8")
    entries = []
    for item in managed_media(path):
        target_file = assets / item.name
        try:
            os.link(item, target_file)
        except OSError:
            shutil.copy2(item, target_file)
        entries.append({"name": item.name, "size": item.stat().st_size,
                        "sha256": sha256(target_file)})
    record = {"id": identifier, "created_at": time.time(), "name": project.get("name", "Project"),
              "note": str(note).strip()[:200], "media": entries}
    (target / "version.json").write_text(json.dumps(record, ensure_ascii=False, indent=2), encoding="utf-8")
    return record


def list_versions(path: Path) -> list[dict]:
    result = []
    root = path / "versions"
    if not root.is_dir():
        return result
    for item in root.iterdir():
        if not item.is_dir() or not re.fullmatch(r"[0-9a-f]{32}", item.name):
            continue
        try:
            result.append(json.loads((item / "version.json").read_text(encoding="utf-8")))
        except (OSError, ValueError):
            continue
    return sorted(result, key=lambda record: record.get("created_at", 0), reverse=True)


def restore_version(path: Path, identifier: str) -> dict:
    if not re.fullmatch(r"[0-9a-f]{32}", identifier):
        raise ValueError("Mã phiên bản không hợp lệ.")
    source = path / "versions" / identifier
    try:
        project = json.loads((source / "project.json").read_text(encoding="utf-8"))
        record = json.loads((source / "version.json").read_text(encoding="utf-8"))
    except (OSError, ValueError):
        raise FileNotFoundError("Không tìm thấy phiên bản project.") from None
    restored = []
    for entry in record.get("media", []):
        name = entry.get("name", "")
        if name not in FIXED_MEDIA and not MEDIA_PATTERN.fullmatch(name):
            continue
        source_file = source / "media" / name
        if not source_file.is_file():
            continue
        if source_file.stat().st_size != entry.get("size"):
            raise ValueError("File trong phiên bản bị thiếu hoặc đã thay đổi.")
        digest = sha256(source_file)
        if digest != entry.get("sha256"):
            raise ValueError("File trong phiên bản không khớp dấu vân tay.")
        restored.append((source_file, path / name))
    for item in managed_media(path):
        item.unlink(missing_ok=True)
    for source_file, target in restored:
        shutil.copy2(source_file, target)
    return project


def write_bundle(output, project: dict, path: Path, include_media: bool) -> None:
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=2) as archive:
        archive.writestr("project.json", json.dumps(project, ensure_ascii=False, indent=2))
        entries = media_manifest(path)
        archive.writestr("manifest.json", json.dumps({"format": "vietkara-project", "version": 1,
                                                       "includes_media": include_media, "media": entries},
                                                      ensure_ascii=False, indent=2))
        if include_media:
            for item in managed_media(path):
                archive.write(item, "media/" + item.name)


def read_bundle(source, destination: Path) -> tuple[dict, list[str], dict]:
    try:
        archive = zipfile.ZipFile(source)
    except (OSError, zipfile.BadZipFile):
        raise ValueError("File project ZIP không đọc được.") from None
    with archive:
        infos = archive.infolist()
        if len(infos) > 2000 or sum(item.file_size for item in infos) > MAX_ARCHIVE_SIZE:
            raise ValueError("Project ZIP vượt quá giới hạn dung lượng.")
        project_info = next((item for item in infos if item.filename == "project.json"), None)
        if project_info is None or project_info.file_size > 20 * 1024 * 1024:
            raise ValueError("Project ZIP thiếu project.json hợp lệ.")
        try:
            project = json.loads(archive.read(project_info).decode("utf-8-sig"))
        except (UnicodeError, ValueError):
            raise ValueError("Không đọc được dữ liệu project trong ZIP.") from None
        if not isinstance(project, dict) or project.get("schema") not in (1, 2):
            raise ValueError("Định dạng project chưa được hỗ trợ.")
        manifest_info = next((item for item in infos if item.filename == "manifest.json"), None)
        manifest = {"media": []}
        if manifest_info:
            try:
                manifest = json.loads(archive.read(manifest_info).decode("utf-8"))
            except (UnicodeError, ValueError):
                raise ValueError("Project ZIP có manifest không hợp lệ.") from None
        if not isinstance(manifest, dict) or not isinstance(manifest.get("media", []), list):
            raise ValueError("Project ZIP có manifest không hợp lệ.")
        expected = {item.get("name"): item for item in manifest.get("media", []) if isinstance(item, dict)}
        copied = []
        names = set()
        for item in infos:
            if not item.filename.startswith("media/"):
                continue
            name = item.filename[6:]
            if not name or "/" in name or "\\" in name or (name not in FIXED_MEDIA and not MEDIA_PATTERN.fullmatch(name)):
                raise ValueError("Project ZIP chứa tên file media không hợp lệ.")
            if name in names:
                raise ValueError("Project ZIP bị trùng file media.")
            names.add(name)
            target = destination / name
            with archive.open(item) as source_file, target.open("wb") as target_file:
                shutil.copyfileobj(source_file, target_file, length=1024 * 1024)
            record = expected.get(name)
            if record and (target.stat().st_size != record.get("size") or sha256(target) != record.get("sha256")):
                target.unlink(missing_ok=True)
                raise ValueError("File media trong project ZIP không khớp dấu vân tay.")
            copied.append(name)
        return project, copied, expected
