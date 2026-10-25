"""Download and safely stage small GitHub release patches for installed builds."""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import subprocess
import sys
import threading
import time
from urllib.parse import urlparse
from urllib.error import HTTPError
from urllib.request import Request, urlopen
import zipfile

import engine
from app_runtime import VERSION

REPOSITORY = "NDCLI/VietKaraStudio"
RELEASE_API = f"https://api.github.com/repos/{REPOSITORY}/releases/latest"
UPDATE_ROOT = engine.DATA / "updates"
MAX_PATCH_BYTES = 900 * 1024 * 1024
MAX_EXTRACTED_BYTES = 1200 * 1024 * 1024
MAX_FILES = 2500
PATCH_FORMAT = "vietkara-nuitka-patch-v1"
ALLOWED_ROOT_FILES = {"VietKaraStudio.exe", "BUILD-INFO.json"}


def _version_tuple(value: str) -> tuple[int, ...]:
    match = re.fullmatch(r"v?(\d+)\.(\d+)\.(\d+)(?:\.(\d+))?", str(value).strip(), re.I)
    if not match:
        raise ValueError("Phiên bản cập nhật không đúng định dạng.")
    return tuple(int(part or 0) for part in match.groups())


def _installed_info() -> dict:
    path = engine.ROOT / "BUILD-INFO.json"
    if path.is_file():
        try:
            return json.loads(path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            pass
    return {"version": VERSION}


def _runtime_fingerprint(info: dict) -> str:
    runtime = {key: info.get(key) for key in ("python", "nuitka", "libraries")}
    encoded = json.dumps(runtime, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def _installed_version() -> str:
    return str(_installed_info().get("version") or VERSION)


def _helper() -> Path:
    return engine.ROOT / "VietKaraUpdater.exe"


def _request_json(url: str) -> dict:
    request = Request(url, headers={"Accept": "application/vnd.github+json", "User-Agent": "VietKaraStudio-Updater"})
    with urlopen(request, timeout=12) as response:
        if urlparse(response.geturl()).hostname != "api.github.com":
            raise ValueError("Không xác thực được máy chủ cập nhật.")
        return json.loads(response.read(4 * 1024 * 1024).decode("utf-8"))


def _latest_release() -> tuple[dict | None, str, dict | None]:
    try:
        release = _request_json(RELEASE_API)
    except HTTPError as exc:
        if exc.code == 404:
            return None, _installed_version(), None
        raise
    version = str(release.get("tag_name", "")).removeprefix("v")
    _version_tuple(version)
    current = _installed_version()
    if _version_tuple(version) <= _version_tuple(current):
        return release, version, None
    expected = f"VietKaraStudio-{version}-Nuitka-Patch.zip"
    asset = next((item for item in release.get("assets", []) if item.get("name") == expected), None)
    if not asset:
        raise ValueError("Bản mới đã có trên GitHub nhưng chưa có gói vá VietKara. Hãy dùng bộ cài đầy đủ hoặc thử lại sau.")
    if int(asset.get("size", 0)) <= 0 or int(asset["size"]) > MAX_PATCH_BYTES:
        raise ValueError("Gói vá có dung lượng không hợp lệ hoặc vượt giới hạn.")
    return release, version, asset


def _ready_update() -> dict | None:
    state = UPDATE_ROOT / "ready.json"
    if not state.is_file():
        return None
    try:
        value = json.loads(state.read_text(encoding="utf-8"))
        version = str(value["version"])
        folder = (UPDATE_ROOT / version / "payload").resolve()
        if folder.parent.parent != UPDATE_ROOT.resolve() or not (folder / "VietKaraStudio.exe").is_file():
            return None
        if _version_tuple(version) <= _version_tuple(_installed_version()):
            return None
        return {"version": version, "folder": str(folder)}
    except (OSError, ValueError, KeyError, TypeError):
        return None


def status() -> dict:
    info = _installed_info()
    ready = _ready_update()
    return {"supported": _helper().is_file() and os.name == "nt",
            "installed_version": str(info.get("version") or VERSION),
            "ready": bool(ready), "ready_version": ready["version"] if ready else None}


def check() -> dict:
    local = status()
    if not local["supported"]:
        return {**local, "update_available": False,
                "message": "Tự cập nhật chỉ có trong bản Setup đã cài trên Windows."}
    try:
        release, version, asset = _latest_release()
    except Exception as exc:
        return {**local, "update_available": False, "message": "Chưa kiểm tra được GitHub: " + str(exc)}
    if release is None:
        ready = _ready_update()
        if ready:
            return {**local, "update_available": True, "version": ready["version"],
                    "ready": True, "ready_version": ready["version"], "size": 0,
                    "message": f"Bản vá {ready['version']} đã sẵn sàng để cài."}
        return {**local, "update_available": False, "message": "Chưa có bản vá nào trên GitHub Releases."}
    if not asset:
        return {**local, "update_available": False, "message": "Bạn đang dùng bản mới nhất."}
    ready = _ready_update()
    return {**local, "update_available": True, "version": version,
            "title": str(release.get("name") or "")[:160],
            "notes": str(release.get("body") or "")[:500],
            "size": int(asset.get("size", 0)),
            "ready": bool(ready and ready["version"] == version),
            "ready_version": ready["version"] if ready else None,
            "message": f"Có bản vá {version}."}


def _asset_digest(release: dict, patch_asset: dict) -> str:
    digest = patch_asset.get("digest") or ""
    match = re.fullmatch(r"sha256:([0-9a-fA-F]{64})", digest)
    if match:
        return match.group(1).lower()
    sidecar_name = Path(str(patch_asset["name"])).with_suffix(".sha256").name
    sidecar = next((item for item in release.get("assets", []) if item.get("name") == sidecar_name), None)
    if not sidecar:
        raise ValueError("Gói vá chưa có mã SHA-256 trên GitHub.")
    url = sidecar.get("browser_download_url", "")
    if urlparse(url).scheme != "https" or urlparse(url).hostname != "github.com":
        raise ValueError("Đường dẫn mã kiểm tra của gói vá không hợp lệ.")
    request = Request(url, headers={"User-Agent": "VietKaraStudio-Updater"})
    with urlopen(request, timeout=20) as response:
        host = urlparse(response.geturl()).hostname or ""
        if not (host == "github.com" or host == "githubusercontent.com" or host.endswith(".githubusercontent.com")):
            raise ValueError("Không xác thực được mã kiểm tra của gói vá.")
        text = response.read(4096).decode("ascii", "strict")
    found = re.search(r"\b([0-9a-fA-F]{64})\b", text)
    if not found:
        raise ValueError("Không đọc được mã SHA-256 của gói vá.")
    return found.group(1).lower()


def _download_asset(asset: dict, destination: Path) -> str:
    url = asset.get("browser_download_url", "")
    parsed = urlparse(url)
    if parsed.scheme != "https" or parsed.hostname != "github.com":
        raise ValueError("Đường dẫn gói vá không hợp lệ.")
    request = Request(url, headers={"User-Agent": "VietKaraStudio-Updater", "Accept": "application/octet-stream"})
    digest = hashlib.sha256()
    total = 0
    with urlopen(request, timeout=45) as response, destination.open("wb") as stream:
        host = urlparse(response.geturl()).hostname or ""
        if not (host == "github.com" or host == "githubusercontent.com" or host.endswith(".githubusercontent.com")):
            raise ValueError("Không xác thực được máy chủ tải gói vá.")
        while True:
            chunk = response.read(1024 * 1024)
            if not chunk:
                break
            total += len(chunk)
            if total > MAX_PATCH_BYTES:
                raise ValueError("Gói vá vượt giới hạn dung lượng.")
            digest.update(chunk)
            stream.write(chunk)
    if total != int(asset.get("size", total)):
        raise ValueError("Gói vá tải về chưa đầy đủ.")
    return digest.hexdigest()


def _safe_patch_path(name: str) -> bool:
    if not name or "\\" in name or "\x00" in name or name.startswith("/") or ":" in name:
        return False
    path = PurePosixPath(name)
    if any(part in ("", ".", "..") for part in path.parts):
        return False
    if name in ALLOWED_ROOT_FILES:
        return True
    return len(path.parts) >= 2 and path.parts[0] in ("static", "resources")


def _extract_patch(archive_path: Path, version: str, target: Path) -> None:
    info = _installed_info()
    current = _installed_version()
    if not isinstance(info.get("libraries"), list) or not info.get("python") or not info.get("nuitka"):
        raise ValueError("Không đọc được thông tin runtime để kiểm tra tính tương thích.")
    with zipfile.ZipFile(archive_path) as archive:
        entries = archive.infolist()
        if len(entries) > MAX_FILES:
            raise ValueError("Gói vá có quá nhiều tệp.")
        files = [entry for entry in entries if not entry.is_dir()]
        if sum(entry.file_size for entry in files) > MAX_EXTRACTED_BYTES:
            raise ValueError("Dung lượng giải nén gói vá vượt giới hạn.")
        names = [entry.filename for entry in files]
        if len(set(names)) != len(names) or "UPDATE-MANIFEST.json" not in names:
            raise ValueError("Danh sách tệp trong gói vá không hợp lệ.")
        manifest = json.loads(archive.read("UPDATE-MANIFEST.json").decode("utf-8"))
        if manifest.get("format") != PATCH_FORMAT or manifest.get("version") != version:
            raise ValueError("Gói vá không khớp phiên bản GitHub.")
        minimum = str(manifest.get("minimum_version", ""))
        if _version_tuple(current) < _version_tuple(minimum):
            raise ValueError(f"Bản vá này cần VietKara {minimum} trở lên. Hãy cài Setup phù hợp trước.")
        if manifest.get("runtime_fingerprint") != _runtime_fingerprint(info):
            raise ValueError("Bản vá không tương thích với runtime hiện tại. Hãy dùng Setup đầy đủ.")
        expected = manifest.get("files")
        if not isinstance(expected, dict) or not expected or set(names) != set(expected) | {"UPDATE-MANIFEST.json"}:
            raise ValueError("Gói vá có tệp thiếu hoặc thừa.")
        for entry in files:
            name = entry.filename
            if not _safe_patch_path(name):
                raise ValueError("Gói vá chứa đường dẫn không an toàn.")
            mode = (entry.external_attr >> 16) & 0o170000
            if mode == 0o120000:
                raise ValueError("Gói vá không được chứa liên kết tượng trưng.")
            digest = hashlib.sha256()
            extracted = target / PurePosixPath(name)
            extracted.parent.mkdir(parents=True, exist_ok=True)
            with archive.open(entry) as source, extracted.open("wb") as output:
                while True:
                    chunk = source.read(1024 * 1024)
                    if not chunk:
                        break
                    digest.update(chunk)
                    output.write(chunk)
            if digest.hexdigest() != expected[name]:
                raise ValueError("Mã kiểm tra tệp cập nhật không khớp: " + name)
        new_info = json.loads((target / "BUILD-INFO.json").read_text(encoding="utf-8"))
        if str(new_info.get("version")) != version:
            raise ValueError("Thông tin phiên bản trong gói vá không khớp.")


def download() -> dict:
    local = status()
    if not local["supported"]:
        raise ValueError("Tự cập nhật chỉ có trong bản Setup đã cài trên Windows.")
    release, version, asset = _latest_release()
    if release is None:
        raise ValueError("Chưa có bản vá nào trên GitHub Releases.")
    if not asset:
        raise ValueError("Bạn đang dùng bản mới nhất.")
    ready = _ready_update()
    if ready and ready["version"] == version:
        return {"ready": True, "version": version}
    expected_digest = _asset_digest(release, asset)
    UPDATE_ROOT.mkdir(parents=True, exist_ok=True)
    folder = UPDATE_ROOT / version
    partial = UPDATE_ROOT / (version + ".partial")
    shutil.rmtree(partial, ignore_errors=True)
    partial.mkdir(parents=True)
    archive_path = partial / "patch.zip"
    try:
        actual = _download_asset(asset, archive_path)
        if actual != expected_digest:
            raise ValueError("Mã SHA-256 của gói vá không khớp. Không cài đặt bản tải này.")
        payload = partial / "payload"
        payload.mkdir()
        _extract_patch(archive_path, version, payload)
        shutil.rmtree(folder, ignore_errors=True)
        partial.replace(folder)
        marker = UPDATE_ROOT / "ready.json"
        temp = marker.with_suffix(".tmp")
        temp.write_text(json.dumps({"version": version}, indent=2), encoding="utf-8")
        temp.replace(marker)
        return {"ready": True, "version": version}
    except Exception:
        shutil.rmtree(partial, ignore_errors=True)
        raise


def apply() -> dict:
    ready = _ready_update()
    if not ready:
        raise ValueError("Chưa có bản vá đã tải và kiểm tra xong.")
    helper = _helper()
    if not helper.is_file():
        raise ValueError("Không tìm thấy trình cài bản vá. Hãy chạy lại bộ Setup.")
    if not (ready["folder"] and Path(ready["folder"]).is_dir()):
        raise ValueError("Không tìm thấy dữ liệu bản vá.")
    flags = getattr(subprocess, "DETACHED_PROCESS", 0x00000008) | getattr(subprocess, "CREATE_NO_WINDOW", 0x08000000)
    subprocess.Popen([str(helper), str(os.getpid()), ready["folder"], str(engine.ROOT)],
                     cwd=str(engine.ROOT), stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                     stderr=subprocess.DEVNULL, close_fds=True, creationflags=flags)
    threading.Timer(1.5, os._exit, args=(0,)).start()
    return {"applying": True, "version": ready["version"]}
