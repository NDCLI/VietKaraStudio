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
from styles import validate_settings


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
    if settings.get("audio_output", "beat") == "guide":
        if "vocals.wav" in available:
            needed.append("vocals.wav")
        elif "reference.wav" in available:
            needed.append("reference.wav")
        else:
            raise ValueError(f"Project {project.get('name', '')} cần bản hát hoặc vocal đã tách cho bản hướng dẫn.")
    return [available[name] for name in dict.fromkeys(needed) if name in available]


def create(data_root: Path, projects: list[tuple[dict, Path]], owner: str,
           aspects: list[str] | None = None, include_guide: bool = False) -> dict:
    batch_id, job_id = uuid.uuid4().hex, uuid.uuid4().hex
    root = data_root / "batch-renders" / batch_id
    requested_aspects = list(dict.fromkeys(aspects or ["current"]))
    if not requested_aspects or any(value not in ("current", "landscape", "portrait") for value in requested_aspects):
        raise ValueError("Tỉ lệ đầu ra theo lô không hợp lệ.")
    snapshots = []
    media_by_project = {}
    for project, source in projects:
        current_aspect = validate_settings(project.get("settings", {}))["aspect"]
        seen_outputs = set()
        for requested in requested_aspects:
            target_aspect = current_aspect if requested == "current" else requested
            for audio_output in (("beat", "guide") if include_guide else ("beat",)):
                output_key = (target_aspect, audio_output)
                if output_key in seen_outputs:
                    continue
                seen_outputs.add(output_key)
                snapshot = deepcopy(project)
                snapshot["settings"] = validate_settings({"aspect": target_aspect,
                                                          "audio_output": audio_output},
                                                         project.get("settings", {}))
                media_list = required_media(snapshot, source)
                snapshots.append((project, snapshot, source, media_list, requested, audio_output))
                media_by_project.setdefault(project["id"], {})
                media_by_project[project["id"]].update({media.name: media for media in media_list})
    required_by_path = {str(media.resolve()): media for media_list in media_by_project.values()
                        for media in media_list.values()}
    required_bytes = sum(media.stat().st_size for media in required_by_path.values())
    free_bytes = shutil.disk_usage(data_root).free
    if required_bytes + 256 * 1024 ** 2 > free_bytes:
        raise ValueError(f"Không đủ dung lượng trống để chụp hàng đợi ({required_bytes / 1024**3:.1f} GB cần, "
                         f"{free_bytes / 1024**3:.1f} GB còn trống).")
    root.mkdir(parents=True, exist_ok=False)
    items = []
    try:
        media_snapshots = {}
        for project_id, media_list in media_by_project.items():
            frozen_media = root / "media" / project_id
            frozen_media.mkdir(parents=True)
            media_snapshots[project_id] = {}
            for name, media in media_list.items():
                destination = frozen_media / name
                shutil.copy2(media, destination)
                media_snapshots[project_id][name] = destination
        for project, snapshot, source, media_list, requested_aspect, audio_output in snapshots:
            render_id = uuid.uuid4().hex
            frozen = root / "items" / render_id
            frozen.mkdir(parents=True)
            (frozen / "snapshot.json").write_text(json.dumps(snapshot, ensure_ascii=False, indent=2), encoding="utf-8")
            assets = []
            for media in media_list:
                destination = frozen / media.name
                try:
                    destination.hardlink_to(media_snapshots[project["id"]][media.name])
                except OSError:
                    shutil.copy2(media_snapshots[project["id"]][media.name], destination)
                assets.append(media.name)
            aspect_label = "Hiện tại" if requested_aspect == "current" else ("Ngang 16:9" if requested_aspect == "landscape" else "Dọc 9:16")
            audio_label = "Beat gốc" if audio_output == "beat" else "Beat + vocal hướng dẫn"
            items.append({"project_id": project["id"], "render_id": render_id,
                          "name": f"{project['name']} · {aspect_label} · {audio_label}",
                          "variant": {"aspect": snapshot["settings"]["aspect"], "audio": audio_output},
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
            render_id = item.get("render_id") or job.id
            result = engine.render(project, frozen, job, render_id=render_id)
            job.check()
            generated = frozen / "exports" / render_id
            destination = data_root / "projects" / item["project_id"] / "exports" / render_id
            destination.parent.mkdir(parents=True, exist_ok=True)
            if destination.exists():
                shutil.rmtree(destination)
            if generated.is_dir():
                shutil.move(str(generated), str(destination))
                output_path = Path(result.get("output_path", ""))
                try:
                    relative_output = output_path.relative_to(generated)
                except (ValueError, OSError):
                    pass
                else:
                    result["output_path"] = str(destination / relative_output)
            # A guide variant without pre-separated vocals creates them on its own
            # frozen snapshot. Reuse that separation for sibling aspect variants.
            if item.get("variant", {}).get("audio") == "guide":
                vocals = frozen / "vocals.wav"
                if vocals.is_file():
                    for sibling in record["items"][index + 1:]:
                        sibling_dir = Path(sibling.get("directory", ""))
                        if (sibling.get("project_id") != item["project_id"] or
                                sibling.get("variant", {}).get("audio") != "guide" or
                                (sibling_dir / "vocals.wav").exists()):
                            continue
                        sibling_vocals = sibling_dir / "vocals.wav"
                        try:
                            sibling_vocals.hardlink_to(vocals)
                        except OSError:
                            shutil.copy2(vocals, sibling_vocals)
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
