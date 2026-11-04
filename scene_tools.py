"""Section-based image scenes shared by preview, batch snapshots and export."""
from __future__ import annotations

import math
import re
import uuid
from functools import lru_cache
from pathlib import Path

from PIL import Image

from timing import beat_words


SECTION_NAMES = frozenset(("intro", "verse", "pre-chorus", "chorus", "bridge", "outro", "instrumental"))
SCENE_ID = re.compile(r"^[0-9a-f]{32}$")


def media_name(identifier: str) -> str:
    if not isinstance(identifier, str) or not SCENE_ID.fullmatch(identifier):
        raise ValueError("Mã cảnh không hợp lệ.")
    return f"scene_{identifier}.png"


def _keyframe(value, fallback=None):
    value = value if isinstance(value, dict) else {}
    fallback = fallback or {}
    result = {}
    for key, default, low, high in (
            ("x", 0, -100, 100), ("y", 0, -100, 100),
            ("scale", 100, 100, 200), ("opacity", 100, 0, 100)):
        try:
            number = float(value.get(key, fallback.get(key, default)))
        except (TypeError, ValueError):
            number = default
        if not math.isfinite(number):
            number = default
        result[key] = round(max(low, min(high, number)), 2)
    return result


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
        contrast_mode = scene.get("contrast_mode", "auto")
        if contrast_mode not in ("auto", "always", "off"):
            contrast_mode = "auto"
        keyframes = scene.get("keyframes", [])
        if not isinstance(keyframes, list):
            keyframes = []
        start = _keyframe(keyframes[0] if keyframes else None)
        end = _keyframe(keyframes[1] if len(keyframes) > 1 else None, start)
        output.append({"id": identifier, "section_name": section,
                       "media_name": media_name(identifier),
                       "opacity": round(max(0, min(100, opacity))),
                       "transition": transition,
                       "transition_seconds": round(max(0, min(3, transition_seconds)), 2),
                       "keyframes": [start, end], "contrast_mode": contrast_mode})
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


@lru_cache(maxsize=128)
def _bright_pixel_ratio(image_path: str, modified_ns: int) -> float:
    """Estimate whether the scene's brighter areas need a backing behind lyrics."""
    del modified_ns  # The timestamp is part of the cache key so image replacements invalidate it.
    try:
        with Image.open(image_path) as image:
            image.thumbnail((96, 54))
            gray = image.convert("RGB").resize((48, 27)).convert("L")
            histogram = gray.histogram()
        total = sum(histogram)
        if not total:
            return 0.
        # The 80th percentile responds to broad bright areas without reacting to tiny highlights.
        target = total * .8
        count = 0
        for level, amount in enumerate(histogram):
            count += amount
            if count >= target:
                return level / 255
    except (OSError, ValueError):
        pass
    return 0.


def apply_readability(plan: dict, project: dict, directory: Path) -> list[dict]:
    """Attach one scene-aware lyric backing decision shared by preview and ASS output."""
    duration = (project.get("beat") or {}).get("duration", 0)
    scenes = timeline(project, duration, directory)
    for scene in scenes:
        mode = scene.get("contrast_mode", "auto")
        if mode == "always":
            opacity = 48
        elif mode == "auto":
            try:
                modified_ns = scene["image_path"].stat().st_mtime_ns
            except OSError:
                modified_ns = 0
            brightness = _bright_pixel_ratio(str(scene["image_path"]), modified_ns)
            opacity = 44 if brightness >= .64 else 0
        else:
            opacity = 0
        scene["backing_opacity"] = opacity

    settings = project.get("settings", {})
    for row in plan.get("rows", []):
        if settings.get("display_mode") == "subtitle" and settings.get("subtitle_box"):
            row["readability_backing"] = 0
            continue
        active = [scene for scene in scenes if scene["start"] <= row["start"] <= scene["end"]]
        row["readability_backing"] = max((scene["backing_opacity"] for scene in active), default=0)
    return scenes
