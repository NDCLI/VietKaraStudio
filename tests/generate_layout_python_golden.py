"""Generate a deterministic layout golden from a Python baseline checkout.

Set VIETKARA_PYTHON_ROOT to a checkout of baseline commit c8dae55 before
running this helper. The Rust test uses the checked-in output and an identical
per-character measurer, so it compares layout rules independently of fonts.
"""

import json
import os
from pathlib import Path
import sys


ROOT = Path(__file__).resolve().parents[1]
PYTHON_ROOT = Path(os.environ["VIETKARA_PYTHON_ROOT"]).resolve()
sys.path.insert(0, str(PYTHON_ROOT))
sys.path.insert(1, str(ROOT))

import layout  # noqa: E402
import subtitles  # noqa: E402


class FixedMeasure:
    def __init__(self, opts):
        self.opts = opts
        self.family = opts["font"]
        self.factor = 1.0
        self.fallback = False
        self.scale = opts["horizontal_scale"] / 100

    def width(self, text):
        return len(layout.font_text(text, self.family)) * self.opts["font_size"] * 0.5 * self.scale

    def fits(self, tokens, width):
        extra = 2 * self.opts["outline_width"] + self.opts["shadow"]
        if self.opts["italic"]:
            extra += self.opts["font_size"] * 0.15
        return self.width(" ".join(tokens)) + extra + layout.role_icon_space(self.opts) <= width


def make_word(text, start, line, index, role, row, section=False):
    return {
        "id": f"{index + 1:032x}",
        "line_uid": f"{line + 100:032x}",
        "text": text,
        "start": start,
        "end": start + 0.3,
        "line": line,
        "role": role,
        "row": row,
        "section": section or None,
    }


words = []
long_lyrics = "Nhiều khi anh mong được một lần nói ra hết tất cả thay vì ngồi lặng im".split()
for index, text in enumerate(long_lyrics):
    start = 0.5 + index * 0.34
    if index >= 7:
        start += 5.0
    words.append(make_word(text, start, 0, len(words), "male", 0, index == 0))
for text in ("Xin", "chào"):
    words.append(make_word(text, 10.0 + (len(words) % 2) * 0.4, 1, len(words), "female", 1, text == "Xin"))
for text in ("Điệp", "khúc"):
    words.append(make_word(text, 18.0 + (len(words) % 2) * 0.4, 2, len(words), "male", 0, text == "Điệp"))

project = {
    "schema": 2,
    "id": "00000000000000000000000000000001",
    "name": "Python layout geometry golden",
    "beat": {"duration": 32.0},
    "anchors": [{"ref": 0.0, "beat": 1.25}, {"ref": 32.0, "beat": 33.25}],
    "settings": {
        "font": "Arial",
        "font_size": 120,
        "aspect": "landscape",
        "display_mode": "karaoke",
        "singing_mode": "duet",
        "role_icons": True,
        "role_a_icon": "man-new-blue.png",
        "role_b_icon": "lady-pink.png",
        "role_all_icon": "men(blue) & lady(pink).png",
        "sung_gradient": False,
        "line_animation": "none",
        "horizontal_scale": 110,
        "horizontal_offset": -3,
        "vertical_offset": 5,
        "line_height": 1.25,
        "outline_width": 3,
        "shadow": 2,
        "align": "justify",
        "position": "bottom",
        "countdown": "mic",
        "countdown_seconds": 4,
        "title_enabled": True,
        "title_text": "Bài hát tiếng Việt để đối chiếu title layout",
        "title_credit": "Trình bày: Ca sĩ thử nghiệm",
        "title_font": "Arial",
        "title_size": 120,
        "title_x": 50,
        "title_y": 28,
        "title_start": 0,
        "title_end": 8,
        "title_effect_seconds": 0.7,
        "title_template": "midnight",
    },
    "words": words,
}

layout.Measure = FixedMeasure
plan = layout.display_plan(project)
ass = subtitles.build_ass(project)
rows = []
for row in plan["rows"]:
    rows.append(
        {
            key: row[key]
            for key in (
                "index", "line", "slot", "start", "end", "show_start", "show_end",
                "x", "y", "width", "height", "font_size", "align", "role", "text",
                "section_start", "section_end",
            )
        }
        | {
            "role_icon": (
                {
                    key: row["role_icon"][key]
                    for key in ("x", "y", "size", "width", "role", "color")
                }
                if row["role_icon"] else None
            ),
            "words": [
                {
                    key: word[key]
                    for key in ("text", "display_text", "start", "end", "x", "width")
                }
                for word in row["words"]
            ]
        }
    )

title = plan["title"]
title_expected = {
    key: title[key]
    for key in (
        "start", "end", "effect_seconds", "enter", "exit", "color", "outline",
        "outline_width", "shadow", "shadow_color", "weight", "italic", "font",
        "template",
    )
}
title_expected["rows"] = [
    {key: row[key] for key in ("text", "display_text", "x", "y", "width", "size")}
    for row in title["rows"]
]
title_expected["template"] = {
    key: title["box"][key]
    for key in ("x", "y", "width", "height", "colors", "opacity", "border",
                "border_width", "radius")
}

output = {
    "baseline_commit": "c8dae557ce941485150aa4e6b525f6b136e8319f",
    "measurer": "font_text character count × size × 0.5 × horizontal scale",
    "project": project,
    "expected": {
        "width": plan["width"], "height": plan["height"], "rows": rows,
        "title": title_expected,
        "countdowns": [
            {key: item[key] for key in ("start", "end", "x", "y", "size")}
            for item in plan["countdowns"]
        ],
    },
}
fixture = ROOT / "crates" / "vietkara-render" / "tests" / "fixtures" / "python-c8dae55-layout-fixed-width.json"
fixture.write_text(json.dumps(output, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
print(f"Wrote {fixture}")
ass_fixture = ROOT / "crates" / "vietkara-render" / "tests" / "fixtures" / "python-c8dae55-layout-fixed-width.ass"
ass_fixture.write_text(ass, encoding="utf-8")
print(f"Wrote {ass_fixture}")
