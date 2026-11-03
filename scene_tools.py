"""Section-based still-image scenes shared by preview, batch snapshots and export."""
from __future__ import annotations

import math
import re
import uuid
from pathlib import Path

from timing import beat_words


SECTION_NAMES = frozenset(("intro", "verse", "pre-chorus", "chorus", "bridge", "outro", "instrumental"))
SCENE_ID = re.compile(r"^[0-9a-f]{32}$")


def media_name(identifier: str) -> str:
    if not isinstance(identifier, str) or not SCENE_ID.fullmatch(identifier):
        raise ValueError("Mã cảnh không hợp lệ.")
    return f"scene_{identifier}.png"


def validate(scenes) -> list[dict]:
    if scenes is None:
        return []
    if not isinstance(scenes, list) or len(scenes) > len(SECTION_NAMES):
        raise ValueError("Danh sách cảnh không hợp lệ.")
    output, seen_sections, seen_ids = [], set(), set()
    for scene in scenes:
        if not isinstance(scene, dict):
            raise ValueError("Thiết lập cảnh không hợp lệ.")
        section = str(scene.get("section_name", "")).strip()
        if section not in SECTION_NAMES or section in seen_sections:
            raise ValueError("Mỗi phân đoạn chỉ được có một cảnh ảnh.")
        identifier = str(scene.get("id", ""))
        if not SCENE_ID.fullmatch(identifier):
            identifier = uuid.uuid4().hex
        if identifier in seen_ids:
            raise ValueError("Mã cảnh bị trùng.")
        seen_sections.add(section)
        seen_ids.add(identifier)
        try:
            opacity = float(scene.get("opacity", 100))
            transition_seconds = float(scene.get("transition_seconds", .6))
        except (TypeError, ValueError):
            raise ValueError("Độ rõ hoặc thời gian chuyển cảnh không hợp lệ.") from None
        if not math.isfinite(opacity) or not math.isfinite(transition_seconds):
            raise ValueError("Độ rõ hoặc thời gian chuyển cảnh không hợp lệ.")
        transition = scene.get("transition", "fade")
        if transition not in ("cut", "fade"):
            raise ValueError("Kiểu chuyển cảnh không hợp lệ.")
        output.append({"id": identifier, "section_name": section,
                       "media_name": media_name(identifier),
                       "opacity": round(max(0, min(100, opacity))),
                       "transition": transition,
                       "transition_seconds": round(max(0, min(3, transition_seconds)), 2)})
    return output


def public(scenes, directory: Path) -> list[dict]:
    output = validate(scenes)
    for scene in output:
        scene["image_ready"] = (directory / scene["media_name"]).is_file()
    return output


def timeline(project: dict, duration: float, directory: Path) -> list[dict]:
    """Resolve each consecutive labeled section to an image interval on the beat clock."""
    scenes = {scene["section_name"]: scene for scene in validate(project.get("scenes", []))
              if (directory / scene["media_name"]).is_file()}
    if not scenes or not project.get("words"):
        return []
    mapped = beat_words(project)
    lines = {}
    for index, word in enumerate(mapped):
        line = lines.setdefault(word["line"], {"order": index, "start": word["start"],
                                                "end": word["end"],
                                                "section_name": word.get("section_name", "")})
        line["order"] = min(line["order"], index)
        line["start"] = min(line["start"], word["start"])
        line["end"] = max(line["end"], word["end"])
        if not line["section_name"] and word.get("section_name"):
            line["section_name"] = word["section_name"]
    ordered = sorted(lines.values(), key=lambda line: (line["start"], line["order"]))
    runs = []
    for line in ordered:
        section = line["section_name"] if line["section_name"] in scenes else ""
        if runs and runs[-1]["section_name"] == section:
            runs[-1]["end"] = max(runs[-1]["end"], line["end"])
        else:
            runs.append({"section_name": section, "start": line["start"], "end": line["end"]})
    output = []
    duration = max(0., float(duration or 0))
    for index, run in enumerate(runs):
        scene = scenes.get(run["section_name"])
        if not scene:
            continue
        image = directory / scene["media_name"]
        previous = scenes.get(runs[index - 1]["section_name"]) if index else None
        next_run = runs[index + 1] if index + 1 < len(runs) else None
        following = scenes.get(next_run["section_name"]) if next_run else None
        transition = scene["transition_seconds"] if scene["transition"] == "fade" else 0.
        if previous:
            transition = min(transition, max(0., (run["start"] - runs[index - 1]["start"]) / 2))
        if following and next_run:
            transition = min(transition, max(0., (next_run["start"] - run["start"]) / 2))
        start = run["start"] - transition / 2 if previous else run["start"]
        end = (next_run["start"] + transition / 2 if following else
               next_run["start"] if next_run else duration)
        start, end = max(0., start), min(duration, end)
        if end - start < .05:
            continue
        output.append({**scene, "start": round(start, 3), "end": round(end, 3),
                       "fade_seconds": min(transition, max(.05, (end - start) / 2)),
                       "image_path": image})
    return output
