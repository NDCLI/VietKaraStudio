"""Persistent cache for deterministic speech recognition and alignment work."""
from __future__ import annotations

from copy import deepcopy
import hashlib
import json
import os
from pathlib import Path
import shutil
import threading
import uuid

import project_archive


CACHE_SCHEMA = 1
SYNC_CACHE_SCHEMA = 1
SYNC_ALGORITHM_REVISION = "audio-sync-v1"
MAX_CACHE_BYTES = 4 * 1024 ** 3
MAX_SYNC_CACHE_BYTES = 256 * 1024 ** 2
_CACHE_LOCK = threading.Lock()


def cache_key(reference: Path, model: str, language: str, lyrics: str,
              preference: str, region: dict | None = None,
              extra_options: dict | None = None) -> str | None:
    """Key results by source contents and every recognition input that affects them."""
    try:
        digest = project_archive.sha256(reference)
    except OSError:
        return None
    parameters = {
        "schema": CACHE_SCHEMA,
        "reference_sha256": digest,
        "demucs_revision": "separation-v1",
        "model": str(model),
        "language": str(language),
        "lyrics": str(lyrics),
        "device_preference": str(preference),
        "region": ({"start": round(float(region["start"]), 3),
                    "end": round(float(region["end"]), 3)} if region else None),
    }
    if extra_options:
        parameters["extra_options"] = extra_options
    encoded = json.dumps(parameters, ensure_ascii=False, sort_keys=True,
                         separators=(",", ":")).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def sync_cache_key(reference: Path, beat: Path,
                   extra_options: dict | None = None) -> str | None:
    """Key advisory sync results by the bytes of both audio inputs."""
    try:
        reference_digest = project_archive.sha256(reference)
        beat_digest = project_archive.sha256(beat)
    except OSError:
        return None
    parameters = {
        "schema": SYNC_CACHE_SCHEMA,
        "algorithm": SYNC_ALGORITHM_REVISION,
        "reference_sha256": reference_digest,
        "beat_sha256": beat_digest,
    }
    if extra_options:
        parameters["extra_options"] = extra_options
    encoded = json.dumps(parameters, ensure_ascii=False, sort_keys=True,
                         separators=(",", ":")).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def load_sync(data_root: Path, key: str) -> dict | None:
    """Load only a complete advisory result from the sync cache namespace."""
    if not _valid_cache_key(key):
        return None
    entry = data_root / "cache" / "audio_sync" / key
    try:
        result = json.loads((entry / "result.json").read_text(encoding="utf-8"))
        if (not isinstance(result, dict)
                or result.get("requires_confirmation") is not True
                or not isinstance(result.get("anchors"), list)):
            return None
        os.utime(entry, None)
        return result
    except (OSError, ValueError, TypeError):
        return None


def store_sync(data_root: Path, key: str, result: dict) -> bool:
    """Atomically cache a validated advisory result; cache errors are optional."""
    if (not _valid_cache_key(key) or not isinstance(result, dict)
            or result.get("requires_confirmation") is not True
            or not isinstance(result.get("anchors"), list)):
        return False
    root = data_root / "cache" / "audio_sync"
    temporary = root / (key + ".tmp-" + uuid.uuid4().hex)
    destination = root / key
    try:
        with _CACHE_LOCK:
            root.mkdir(parents=True, exist_ok=True)
            if destination.is_dir():
                if load_sync(data_root, key) is not None:
                    return True
                shutil.rmtree(destination, ignore_errors=True)
            elif destination.exists():
                destination.unlink(missing_ok=True)
            temporary.mkdir()
            result_path = temporary / "result.json"
            result_path.write_text(
                json.dumps(result, ensure_ascii=False, allow_nan=False, separators=(",", ":")),
                encoding="utf-8",
            )
            temporary.replace(destination)
            _prune_sync(root, keep=destination)
        return True
    except (OSError, ValueError, TypeError):
        shutil.rmtree(temporary, ignore_errors=True)
        return False


def _valid_cache_key(key: str) -> bool:
    return (isinstance(key, str) and len(key) == 64
            and all(character in "0123456789abcdef" for character in key))


def _prune_sync(root: Path, keep: Path) -> None:
    entries = [entry for entry in root.iterdir() if entry.is_dir() and len(entry.name) == 64
               and all(character in "0123456789abcdef" for character in entry.name)]
    sizes = {entry: sum(item.stat().st_size for item in entry.iterdir() if item.is_file())
             for entry in entries}
    total = sum(sizes.values())
    for entry in sorted((item for item in entries if item != keep), key=lambda item: item.stat().st_mtime):
        if total <= MAX_SYNC_CACHE_BYTES:
            break
        shutil.rmtree(entry, ignore_errors=True)
        total -= sizes[entry]


def load(data_root: Path, key: str) -> dict | None:
    if len(key) != 64 or any(character not in "0123456789abcdef" for character in key):
        return None
    entry = data_root / "cache" / "recognition" / key
    try:
        result = json.loads((entry / "result.json").read_text(encoding="utf-8"))
        vocals = entry / "vocals.wav"
        if (not isinstance(result, dict) or not isinstance(result.get("words"), list) or
                not isinstance(result.get("text"), str) or not vocals.is_file()):
            return None
        os.utime(entry, None)
        return {"result": result, "vocals": vocals}
    except (OSError, ValueError, TypeError):
        return None


def store(data_root: Path, key: str, result: dict, vocals: Path) -> bool:
    """Write a completed result atomically; cache failures never fail recognition."""
    if (not vocals.is_file() or not isinstance(result, dict) or
            not isinstance(result.get("words"), list) or not isinstance(result.get("text"), str) or
            len(key) != 64 or any(character not in "0123456789abcdef" for character in key)):
        return False
    root = data_root / "cache" / "recognition"
    temporary = root / (key + ".tmp-" + uuid.uuid4().hex)
    destination = root / key
    try:
        with _CACHE_LOCK:
            root.mkdir(parents=True, exist_ok=True)
            if destination.is_dir():
                if load(data_root, key):
                    return True
                shutil.rmtree(destination, ignore_errors=True)
            elif destination.exists():
                destination.unlink(missing_ok=True)
            temporary.mkdir()
            shutil.copy2(vocals, temporary / "vocals.wav")
            result_path = temporary / "result.json"
            result_path.write_text(json.dumps(result, ensure_ascii=False, separators=(",", ":")),
                                   encoding="utf-8")
            temporary.replace(destination)
            _prune(root, keep=destination)
        return True
    except OSError:
        shutil.rmtree(temporary, ignore_errors=True)
        return False


def _prune(root: Path, keep: Path) -> None:
    entries = [entry for entry in root.iterdir() if entry.is_dir() and len(entry.name) == 64
               and all(character in "0123456789abcdef" for character in entry.name)]
    sizes = {entry: sum(item.stat().st_size for item in entry.iterdir() if item.is_file())
             for entry in entries}
    total = sum(sizes.values())
    for entry in sorted((item for item in entries if item != keep), key=lambda item: item.stat().st_mtime):
        if total <= MAX_CACHE_BYTES:
            break
        shutil.rmtree(entry, ignore_errors=True)
        total -= sizes[entry]
