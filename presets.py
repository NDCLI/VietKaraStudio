"""Portable presets; packaged VCL sources are read only, custom presets stay in data/."""
from pathlib import Path
import json
import re
import uuid

from styles import validate_settings

BUILTINS = Path(__file__).resolve().parent / "resources" / "presets"
VCL_NOTES = ["Áp dụng font, màu, viền, bóng và hiệu ứng của preset VCL; cỡ chữ mặc định 110."]


def convert_vcl(data):
    s = data["settings"]
    number = lambda key, default: float(s.get(key, default))
    values = {"font": s.get("font", "Tahoma"), "font_size": 110,
              "sung": s.get("sung", "#52efb9"), "unsung": s.get("unsung", "#ffffff"),
              "outline_unsung": s.get("ou", "#000000"), "outline_sung": s.get("os", "#000000"),
              "outline_width": number("ow", 8),
              "outline_unsung_opacity": number("ouA", 100), "outline_sung_opacity": number("osA", 100),
              "italic": bool(s.get("italic", 0)),
              "shadow": number("shadow", 0),
              "shadow_color": s.get("shsc", "#000000"), "position": "bottom",
              "line_height": number("lh", 1.4), "line_animation": s.get("lineAnim", "none"),
              "align":s.get('align','justify'), 'vertical_offset':number('pos',0),
              'sweep_feather':number('sweepFx',0), 'sweep_glow':number('sweepGlow',0),
              'countdown':s.get('cdStyle','none'), 'sung_gradient':bool(s.get('sungGradOn',False)),
              'gradient_top':s.get('sungGradTop','#ffffff'), 'gradient_bottom':s.get('sungGradBot','#1a3ad6'),
              'gradient_start':number('sungGradPosA',0),'gradient_end':number('sungGradPosB',100)}
    if values['countdown']=='icon':values['countdown']='mic'
    values['line_animation']={'fade':'slowfade','spin':'flipup'}.get(values['line_animation'],values['line_animation'])
    checked = validate_settings(values)
    notes = list(VCL_NOTES)
    return {"name": str(data.get("name", "Preset VCL"))[:100],
            "description": str(data.get("desc", ""))[:1500], "source": "VCL",
            "settings": {key: checked[key] for key in values}, "notes": notes}


def decode(data):
    if not isinstance(data, dict) or not isinstance(data.get("settings"), dict):
        raise ValueError("File preset cần có name và settings.")
    if "fs" in data["settings"]:
        return convert_vcl(data)
    if data.get("format") != "original-beat-preset" or data.get("version") != 1:
        raise ValueError("Hỗ trợ preset JSON của VCL hoặc VietKara Studio phiên bản 1.")
    values = validate_settings(data["settings"])
    return {"name": str(data.get("name", "Preset riêng")).strip()[:100] or "Preset riêng",
            "description": str(data.get("description", ""))[:1500],
            "settings": {**{key: values[key] for key in data["settings"] if key in values}, "font_size": 110},
            "source": "Riêng", "notes": [str(x)[:300] for x in data.get("notes", [])[:10]]}


def catalog(root):
    items = []
    for file in sorted(BUILTINS.glob("*.json")):
        data = json.loads(file.read_text(encoding="utf-8-sig"))
        item = convert_vcl(data)
        items.append({**item, "id": "vcl-" + str(data["id"]), "builtin": True})
    for file in sorted((root / "presets").glob("*.json")):
        try:
            item = decode(json.loads(file.read_text(encoding="utf-8")))
            items.append({**item, "id": file.stem, "builtin": False})
        except (ValueError, TypeError, KeyError, OSError):
            continue
    return items


def create(root, data):
    item = decode(data)
    identifier = uuid.uuid4().hex
    folder = root / "presets"
    folder.mkdir(parents=True, exist_ok=True)
    destination = folder / (identifier + ".json")
    payload = export(item)
    temporary = destination.with_suffix(".tmp")
    temporary.write_text(json.dumps(payload, ensure_ascii=False, indent=2), encoding="utf-8")
    temporary.replace(destination)
    return {**item, "id": identifier, "builtin": False}


def remove(root, identifier):
    if not re.fullmatch(r"[0-9a-f]{32}", identifier):
        raise ValueError("Chỉ có thể xóa preset riêng.")
    (root / "presets" / (identifier + ".json")).unlink()


def export(item):
    return {"format": "original-beat-preset", "version": 1,
            **{key: item[key] for key in ("name", "description", "settings", "notes")}}
