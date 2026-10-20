"""Validated settings shared by projects, presets and subtitle rendering."""
import math
import re

DEFAULT_SETTINGS = {
    "aspect": "landscape", "font": "Arial", "font_size": 72,
    "sung": "#52efb9", "unsung": "#ffffff", "background": "#101d2b",
    "use_background": True, "outline_unsung": "#15202a", "outline_sung": "#15202a",
    "outline_width": 3, "outline_unsung_opacity": 100, "outline_sung_opacity": 100,
    "italic": False, "shadow": 1, "shadow_color": "#000000",
    "position": "bottom", "line_height": 1.4, "line_animation": "none",
    "display_mode": "karaoke", "resolution": "1080p", "background_type": "auto",
    "countdown": "none", "countdown_seconds": 4, "singing_mode": "solo",
    "role_a_name": "Nam", "role_b_name": "Nữ", "role_a_color": "#62b5ff", "role_b_color": "#ff87b7",
    "align": "center", "vertical_offset": 0, "horizontal_offset": 0, "horizontal_scale": 100,
    "sweep_feather": 0, "sweep_glow": 0, "sung_gradient": False,
    "gradient_top": "#ffffff", "gradient_bottom": "#1a3ad6", "gradient_start": 0, "gradient_end": 100,
    "line_max_words": 5, "mv_style": "stack", "subtitle_box": True,
    "subtitle_box_color": "#000000", "subtitle_box_opacity": 50, "subtitle_box_radius": 8,
    "logo_enabled": False, "logo_size": 12, "logo_x": 96, "logo_y": 4,
    "logo_opacity": 100, "logo_effect": "none",
    "timeline_linked": True,
    "render_quality": "high", "render_fps": "source",
    "render_format": "mp4",
}
COLOR_KEYS = ("sung", "unsung", "background", "outline_unsung", "outline_sung", "shadow_color",
              "role_a_color", "role_b_color", "gradient_top", "gradient_bottom", "subtitle_box_color")
ANIMATIONS = ("none", "fadesec", "slowfade", "zoom", "zoomout", "flipup", "split", "slidedown", "slideup", "popbounce", "punchzoom")


def validate_settings(values, base=None):
    if not isinstance(values, dict):
        raise ValueError("Thiết lập phải là một đối tượng JSON.")
    result = {**DEFAULT_SETTINGS, **(base or {})}
    result.update({k: v for k, v in values.items() if k in DEFAULT_SETTINGS})
    for key in COLOR_KEYS:
        if not re.fullmatch(r"#[0-9a-fA-F]{6}", str(result[key])):
            raise ValueError(f"Màu {key} không hợp lệ.")
        result[key] = result[key].lower()
    for key, low, high in (("font_size", 20, 240), ("outline_width", 0, 12),
                           ("shadow", 0, 15), ("line_height", 1, 2.5),
                           ("logo_size", 3, 40), ("logo_x", 0, 100), ("logo_y", 0, 100), ("logo_opacity", 0, 100),
                           ("outline_unsung_opacity", 0, 100), ("outline_sung_opacity", 0, 100),
                           ("countdown_seconds", 1, 8), ("vertical_offset", 0, 75),
                           ("horizontal_offset", -40, 40), ("horizontal_scale", 50, 150),
                           ("sweep_feather", 0, 40), ("sweep_glow", 0, 200),
                           ("gradient_start", 0, 99), ("gradient_end", 1, 100),
                           ("line_max_words", 2, 20), ("subtitle_box_opacity", 0, 100), ("subtitle_box_radius", 0, 40)):
        try:
            number = float(result[key])
        except (TypeError, ValueError):
            raise ValueError(f"Giá trị {key} không hợp lệ.") from None
        if not math.isfinite(number):
            raise ValueError(f"Giá trị {key} không hợp lệ.")
        result[key] = round(max(low, min(high, number)), 2)
    result["font_size"] = round(result["font_size"])
    result["font"] = re.sub(r"[,\r\n{}\\]", "", str(result["font"]))[:80].strip() or "Arial"
    for key in ("countdown_seconds", "line_max_words"):
        result[key] = round(result[key])
    if result["gradient_start"] >= result["gradient_end"]:
        result["gradient_end"] = min(100, result["gradient_start"] + 1)
    for key in ("role_a_name", "role_b_name"):
        result[key] = str(result[key]).strip()[:30] or ("Vai A" if key == "role_a_name" else "Vai B")
    for key in ("italic", "use_background", "sung_gradient", "subtitle_box", "logo_enabled", "timeline_linked"):
        if type(result[key]) is not bool:
            raise ValueError(f"Giá trị {key} phải là true hoặc false.")
    for key, choices in (("aspect", ("landscape", "portrait")),
                         ("position", ("bottom", "center", "top")),
                         ("line_animation", ANIMATIONS), ("display_mode", ("karaoke", "lyricsmv", "subtitle")),
                         ("resolution", ("1080p", "4k")), ("background_type", ("auto", "color", "image", "video", "original")),
                         ("countdown", ("none", "mic", "star", "heart", "note", "moon", "image")),
                         ("singing_mode", ("solo", "duet")), ("align", ("left", "center", "right", "justify")),
                         ("mv_style", ("single", "stack", "scroll")), ("logo_effect", ("none", "fade", "float", "pulse")),
                         ("render_quality", ("standard", "high", "maximum")), ("render_fps", ("source", "30", "60")),
                         ("render_format", ("mp4", "mov", "mkv"))):
        if result[key] not in choices:
            raise ValueError(f"Giá trị {key} không hợp lệ.")
    return result
