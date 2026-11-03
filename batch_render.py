"""Durable, frozen-snapshot queue for multi-project renders."""
from __future__ import annotations

from copy import deepcopy
import json
import shutil
import time
import uuid
from pathlib import Path

import engine
import project_archive


def _write(path: Path, value: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(".tmp")
    temporary.write_text(json.dumps(value, ensure_ascii=False, indent=2), encoding="utf-8")
    temporary.replace(path)


def record_path(data_root: Path, batch_id: str) -> Path:
    return data_root / "batch-renders" / batch_id / "batch.json"


def load_record(data_root: Path, batch_id: str) -> dict:
    return json.loads(record_path(data_root, batch_id).read_text(encoding="utf-8"))


def save_record(data_root: Path, record: dict) -> None:
    record["updated_at"] = time.time()
    _write(record_path(data_root, record["id"]), record)


def set_status(data_root: Path, batch_id: str, status: str) -> None:
    try:
        record = load_record(data_root, batch_id)
    except (OSError, ValueError):
        return
    record["status"] = status
    save_record(data_root, record)


def first_pending(record: dict) -> int:
    return next((index for index, item in enumerate(record.get("items", []))
                 if item.get("status") != "done"), len(record.get("items", [])))


def required_media(project: dict, source: Path) -> list[Path]:
    available = {item.name: item for item in project_archive.managed_media(source)}
    needed = ["beat.wav"]
    settings = project.get("settings", {})
    background_type = settings.get("background_type", "auto")
    if background_type == "original":
        for slot in ("reference", "beat"):
            name = (project.get(slot) or {}).get("video", {}).get("source_file")
            if name in available:
                needed.append(name)
                break
    elif background_type == "video":
        name = (project.get("background_video") or {}).get("source_file")
        if name in available:
            needed.append(name)
    elif background_type == "image" or (background_type == "auto" and settings.get("use_background", True)):
        if "background.jpg" in available:
            needed.append("background.jpg")
    if settings.get("logo_enabled") and project.get("logo_name"):
        needed.append("logo.png")
    if settings.get("countdown") == "image" and settings.get("display_mode") == "karaoke":
        needed.append("countdown.png")
    return [available[name] for name in dict.fromkeys(needed) if name in available]


def create(data_root: Path, projects: list[tuple[dict, Path]], owner: str) -> dict:
    batch_id, job_id = uuid.uuid4().hex, uuid.uuid4().hex
    root = data_root / "batch-renders" / batch_id
    snapshots = [(project, source, required_media(project, source)) for project, source in projects]
    required_bytes = sum(media.stat().st_size for _, _, media_list in snapshots for media in media_list)
    free_bytes = shutil.disk_usage(data_root).free
    if required_bytes + 256 * 1024 ** 2 > free_bytes:
        raise ValueError(f"Không đủ dung lượng trống để chụp hàng đợi ({required_bytes / 1024**3:.1f} GB cần, "
                         f"{free_bytes / 1024**3:.1f} GB còn trống).")
    root.mkdir(parents=True, exist_ok=False)
    items = []
    try:
        for project, source, media_list in snapshots:
            frozen = root / "items" / project["id"]
            frozen.mkdir(parents=True)
            snapshot = deepcopy(project)
            (frozen / "snapshot.json").write_text(json.dumps(snapshot, ensure_ascii=False, indent=2), encoding="utf-8")
            assets = []
            for media in media_list:
                shutil.copy2(media, frozen / media.name)
                assets.append(media.name)
            items.append({"project_id": project["id"], "name": project["name"],
                          "directory": str(frozen), "assets": assets, "status": "queued", "result": None})
        record = {"id": batch_id, "job_id": job_id, "owner": owner, "created_at": time.time(),
                  "updated_at": time.time(), "status": "queued", "next_index": 0,
                  "items": items, "completed": 0, "failed": 0}
        save_record(data_root, record)
        return record
    except Exception:
        shutil.rmtree(root, ignore_errors=True)
        raise


def run(data_root: Path, batch_id: str, job) -> dict:
    record = load_record(data_root, batch_id)
    record["status"] = "running"
    save_record(data_root, record)
    total = len(record["items"])
    for index in range(int(record.get("next_index", 0)), total):
        job.check()
        item = record["items"][index]
        if item.get("status") == "done":
            record["next_index"] = index + 1
            continue
        frozen = Path(item["directory"])
        project = json.loads((frozen / "snapshot.json").read_text(encoding="utf-8"))
        job.update(5 + int(90 * index / max(1, total)), f"Đang dựng bài {index + 1}/{total}: {item['name']}…")
        if item.get("status") == "error":
            record["failed"] = max(0, int(record.get("failed", 0)) - 1)
        item["status"] = "running"
        item["error"] = None
        save_record(data_root, record)
        try:
            result = engine.render(project, frozen, job)
            job.check()
            generated = frozen / "exports" / job.id
            destination = data_root / "projects" / item["project_id"] / "exports" / job.id
            destination.parent.mkdir(parents=True, exist_ok=True)
            if destination.exists():
                shutil.rmtree(destination)
            if generated.is_dir():
                shutil.move(str(generated), str(destination))
            item["status"] = "done"
            item["result"] = result
            item["error"] = None
            record["completed"] += 1
        except engine.Cancelled:
            item["status"] = "queued"
            record["status"] = "cancelled"
            save_record(data_root, record)
            raise
        except Exception as error:
            item["status"] = "error"
            item["error"] = str(error)[-2000:]
            record["failed"] += 1
        record["next_index"] = index + 1
        save_record(data_root, record)
    record["status"] = "done"
    save_record(data_root, record)
    return {"batch_id": batch_id, "items": record["items"], "completed": record["completed"],
            "failed": record["failed"]}
