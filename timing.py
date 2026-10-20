"""All lyric times are stored on the reference clock and mapped to the original beat."""
from __future__ import annotations

import math
import re


def finite(value, label="Giá trị"):
    try:
        result = float(value)
    except (TypeError, ValueError):
        raise ValueError(f"{label} phải là số.") from None
    if not math.isfinite(result):
        raise ValueError(f"{label} không hợp lệ.")
    return result


def validate_anchors(anchors):
    if not isinstance(anchors, list) or len(anchors) > 1000:
        raise ValueError("Danh sách mốc không hợp lệ.")
    out = sorted([{"ref": finite(a["ref"], "Mốc bản hát"),
                   "beat": finite(a["beat"], "Mốc beat")} for a in anchors], key=lambda a: a["ref"])
    for i, point in enumerate(out):
        if min(point.values()) < 0:
            raise ValueError("Mốc không được âm.")
        if i and (point["ref"] - out[i - 1]["ref"] < .02 or
                  point["beat"] - out[i - 1]["beat"] < .02):
            raise ValueError("Mốc bản hát và beat phải cùng tăng; không được trùng hoặc đảo thứ tự.")
    return out


def map_time(t, anchors, inverse=False):
    """Piecewise linear mapping, including extrapolation outside the outer anchors."""
    t = float(t)
    if not anchors:
        return t
    x, y = ("beat", "ref") if inverse else ("ref", "beat")
    if len(anchors) == 1:
        return t + anchors[0][y] - anchors[0][x]
    pair = anchors[:2]
    for i in range(len(anchors) - 1):
        pair = anchors[i:i + 2]
        if t <= pair[1][x]:
            break
    a, b = pair
    return a[y] + (t - a[x]) * (b[y] - a[y]) / (b[x] - a[x])


def validate_words(words):
    if not isinstance(words, list) or len(words) > 30000:
        raise ValueError("Danh sách từ không hợp lệ.")
    out = []
    for i, word in enumerate(words):
        start, end = finite(word["start"]), finite(word["end"])
        if start < 0 or end <= start:
            raise ValueError(f"Mốc của từ {i + 1} không hợp lệ: kết thúc phải sau bắt đầu.")
        text = str(word.get("text", "")).strip()[:200]
        if not text:
            continue
        role = word.get("role", "all")
        if role not in ("all", "male", "female"):
            role = "all"
        out.append({"text": text, "start": round(start, 4), "end": round(end, 4),
                    "line": max(0, int(word.get("line", 0))), "role": role,
                    "estimated": bool(word.get("estimated", False))})
        if word.get("section"):
            out[-1]["section"] = True
        if word.get("manual"):
            out[-1]["manual"] = True
        if word.get('label_color'):
            color=str(word['label_color'])
            if not re.fullmatch(r'#[0-9a-fA-F]{6}',color):
                raise ValueError('Màu nhãn từ phải có dạng #RRGGBB.')
            out[-1]['label_color']=color.lower()
        if "row" in word:
            if type(word["row"]) is not int or word["row"] not in (0, 1):
                raise ValueError("Hàng lời phải là 0 (trên) hoặc 1 (dưới).")
            out[-1]["row"] = word["row"]
    return out


def beat_words(project):
    duration = (project.get("beat") or {}).get("duration", float("inf"))
    anchors = validate_anchors(project.get("anchors", []))
    out = []
    for word in project.get("words", []):
        start = map_time(word["start"], anchors)
        end = map_time(word["end"], anchors)
        if end <= 0 or start >= duration:
            continue
        out.append({**word, "start": max(0., start), "end": min(duration, end)})
    return out


def lyric_lines(words):
    groups = {}
    for word in words:
        groups.setdefault(word["line"], []).append(word)
    return [sorted(group, key=lambda w: w["start"]) for group in groups.values()]


def estimate_words(text, duration):
    lines = [line.split() for line in text.splitlines() if line.strip()]
    count = sum(map(len, lines))
    if not count:
        raise ValueError("Hãy nhập lời bài hát.")
    start = min(5., duration * .05)
    step = max(.1, (duration - start - 1) / count)
    words = []
    for li, line in enumerate(lines):
        for token in line:
            words.append({"text": token, "start": start, "end": start + step * .9,
                          "line": li, "role": "all", "estimated": True})
            start += step
    return words
