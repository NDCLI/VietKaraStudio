use serde_json::{Map, Value};
use std::path::Path;

const FORMAT: &str = "original-beat-preset";
const MAX_PRESET_BYTES: u64 = 1_000_000;
const VCL_NOTES: &[&str] =
    &["Áp dụng font, màu, viền, bóng và hiệu ứng của preset VCL; cỡ chữ mặc định 110."];
const BUNDLED_PRESETS: &[&str] = &[
    include_str!("../../../resources/presets/01_default.json"),
    include_str!("../../../resources/presets/02_anime_pop.json"),
    include_str!("../../../resources/presets/03_gaming_neon.json"),
    include_str!("../../../resources/presets/04_chanh_peridot.json"),
    include_str!("../../../resources/presets/05_lam_aquamarine.json"),
    include_str!("../../../resources/presets/06_vang_champagne.json"),
    include_str!("../../../resources/presets/07_hong_ngoc_rose.json"),
    include_str!("../../../resources/presets/08_ngoc_luc_bao.json"),
    include_str!("../../../resources/presets/09_cam_ho_phach.json"),
    include_str!("../../../resources/presets/10_tim_amethyst.json"),
    include_str!("../../../resources/presets/11_lam_sapphire.json"),
    include_str!("../../../resources/presets/12_neon_vien_manh.json"),
    include_str!("../../../resources/presets/13_dien_anh_serif.json"),
    include_str!("../../../resources/presets/14_retro_vien_doi.json"),
    include_str!("../../../resources/presets/15_pastel_mem.json"),
    include_str!("../../../resources/presets/16_chrome_bac.json"),
];
const COLORS: &[&str] = &[
    "sung",
    "unsung",
    "background",
    "outline_unsung",
    "outline_sung",
    "shadow_color",
    "role_a_color",
    "role_b_color",
    "role_c_color",
    "role_d_color",
    "role_e_color",
    "role_f_color",
    "role_unknown_color",
    "role_all_color",
    "gradient_top",
    "gradient_bottom",
    "subtitle_box_color",
    "title_color",
    "title_outline",
    "title_shadow_color",
];
const BOOLS: &[&str] = &[
    "use_background",
    "italic",
    "sung_gradient",
    "subtitle_box",
    "logo_enabled",
    "timeline_linked",
    "role_icons",
    "title_enabled",
    "title_italic",
    "audio_normalize",
];
const STRING_CHOICES: &[(&str, &[&str])] = &[
    ("aspect", &["landscape", "portrait"]),
    ("position", &["bottom", "center", "top"]),
    (
        "line_animation",
        &[
            "none",
            "fadesec",
            "slowfade",
            "zoom",
            "zoomout",
            "flipup",
            "split",
            "slidedown",
            "slideup",
            "popbounce",
            "punchzoom",
        ],
    ),
    ("display_mode", &["karaoke", "lyricsmv", "subtitle"]),
    ("resolution", &["1080p", "4k"]),
    (
        "background_type",
        &["auto", "color", "image", "video", "original"],
    ),
    (
        "countdown",
        &["none", "mic", "star", "heart", "note", "moon", "image"],
    ),
    ("singing_mode", &["solo", "duet"]),
    ("align", &["left", "center", "right", "justify"]),
    ("mv_style", &["single", "stack", "scroll"]),
    ("logo_effect", &["none", "fade", "float", "pulse"]),
    ("render_quality", &["standard", "high", "maximum"]),
    ("render_fps", &["source", "30", "60"]),
    ("audio_output", &["beat", "guide"]),
    ("render_format", &["mp4", "mov", "mkv"]),
    (
        "title_template",
        &[
            "none", "gradient", "midnight", "crystal", "neon", "retro", "pastel",
        ],
    ),
    (
        "title_enter",
        &[
            "none",
            "fade",
            "slide_up",
            "slide_down",
            "slide_left",
            "slide_right",
            "zoom",
            "flip",
        ],
    ),
    (
        "title_exit",
        &[
            "none",
            "fade",
            "slide_up",
            "slide_down",
            "slide_left",
            "slide_right",
            "zoom",
            "flip",
        ],
    ),
];
const NUMBERS: &[(&str, f64, f64)] = &[
    ("font_size", 20.0, 240.0),
    ("outline_width", 0.0, 12.0),
    ("shadow", 0.0, 15.0),
    ("line_height", 1.0, 2.5),
    ("title_start", 0.0, 86400.0),
    ("title_end", 0.1, 86400.0),
    ("title_size", 30.0, 220.0),
    ("title_x", 5.0, 95.0),
    ("title_y", 5.0, 85.0),
    ("title_effect_seconds", 0.1, 3.0),
    ("logo_size", 3.0, 40.0),
    ("logo_x", 0.0, 100.0),
    ("logo_y", 0.0, 100.0),
    ("logo_opacity", 0.0, 100.0),
    ("outline_unsung_opacity", 0.0, 100.0),
    ("outline_sung_opacity", 0.0, 100.0),
    ("countdown_seconds", 1.0, 8.0),
    ("vertical_offset", 0.0, 75.0),
    ("horizontal_offset", -40.0, 40.0),
    ("horizontal_scale", 50.0, 150.0),
    ("sweep_feather", 0.0, 40.0),
    ("sweep_glow", 0.0, 200.0),
    ("gradient_start", 0.0, 99.0),
    ("gradient_end", 1.0, 100.0),
    ("line_max_words", 2.0, 20.0),
    ("subtitle_box_opacity", 0.0, 100.0),
    ("subtitle_box_radius", 0.0, 40.0),
    ("title_outline_width", 0.0, 12.0),
    ("title_shadow", 0.0, 15.0),
    ("title_weight", 300.0, 900.0),
];
const TEXT: &[&str] = &[
    "font",
    "title_font",
    "role_a_name",
    "role_b_name",
    "role_c_name",
    "role_d_name",
    "role_e_name",
    "role_f_name",
    "role_unknown_name",
    "title_text",
    "title_credit",
    "role_a_icon",
    "role_b_icon",
    "role_all_icon",
];
const SETTING_KEYS: &[&str] = &[
    "aspect",
    "font",
    "font_size",
    "sung",
    "unsung",
    "background",
    "use_background",
    "outline_unsung",
    "outline_sung",
    "outline_width",
    "outline_unsung_opacity",
    "outline_sung_opacity",
    "italic",
    "shadow",
    "shadow_color",
    "position",
    "line_height",
    "line_animation",
    "display_mode",
    "resolution",
    "background_type",
    "countdown",
    "countdown_seconds",
    "singing_mode",
    "role_a_name",
    "role_b_name",
    "role_a_color",
    "role_b_color",
    "role_c_name",
    "role_d_name",
    "role_e_name",
    "role_f_name",
    "role_unknown_name",
    "role_c_color",
    "role_d_color",
    "role_e_color",
    "role_f_color",
    "role_unknown_color",
    "role_all_color",
    "role_icons",
    "role_a_icon",
    "role_b_icon",
    "role_all_icon",
    "align",
    "vertical_offset",
    "horizontal_offset",
    "horizontal_scale",
    "sweep_feather",
    "sweep_glow",
    "sung_gradient",
    "gradient_top",
    "gradient_bottom",
    "gradient_start",
    "gradient_end",
    "line_max_words",
    "mv_style",
    "subtitle_box",
    "subtitle_box_color",
    "subtitle_box_opacity",
    "subtitle_box_radius",
    "logo_enabled",
    "logo_size",
    "logo_x",
    "logo_y",
    "logo_opacity",
    "logo_effect",
    "timeline_linked",
    "render_quality",
    "render_fps",
    "audio_normalize",
    "render_format",
    "audio_output",
    "title_enabled",
    "title_text",
    "title_credit",
    "title_font",
    "title_start",
    "title_end",
    "title_size",
    "title_x",
    "title_y",
    "title_color",
    "title_outline",
    "title_outline_width",
    "title_shadow",
    "title_shadow_color",
    "title_weight",
    "title_italic",
    "title_template",
    "title_enter",
    "title_exit",
    "title_effect_seconds",
];

#[derive(Debug, Clone)]
pub(super) struct Preset {
    pub(super) name: String,
    pub(super) description: String,
    pub(super) source: String,
    pub(super) notes: Vec<String>,
    pub(super) settings: Map<String, Value>,
}

#[derive(Debug, Clone)]
pub(super) struct StoredPreset {
    pub(super) id: String,
    pub(super) preset: Preset,
}

pub(super) fn bundled() -> Vec<Preset> {
    BUNDLED_PRESETS
        .iter()
        .filter_map(|source| {
            serde_json::from_str(source)
                .ok()
                .and_then(|value| Preset::decode(value).ok())
        })
        .collect()
}

impl Preset {
    pub(super) fn decode(value: Value) -> Result<Self, String> {
        let object = value.as_object().ok_or("File preset phải là JSON object")?;
        let source = if object.get("format").and_then(Value::as_str) == Some(FORMAT)
            && object.get("version").and_then(Value::as_u64) == Some(1)
        {
            "Riêng".to_owned()
        } else if object
            .get("settings")
            .and_then(Value::as_object)
            .is_some_and(|settings| settings.contains_key("fs"))
        {
            "VCL".to_owned()
        } else {
            return Err("Hỗ trợ preset VietKara Studio phiên bản 1 hoặc preset VCL".into());
        };
        let raw = object
            .get("settings")
            .and_then(Value::as_object)
            .ok_or("Preset thiếu settings")?;
        let (name, description, mut settings) = if source == "VCL" {
            convert_vcl(object, raw)?
        } else {
            let mut settings = Map::new();
            for (key, value) in raw {
                if SETTING_KEYS.contains(&key.as_str()) {
                    settings.insert(key.clone(), value.clone());
                }
            }
            (
                bounded_text(object.get("name"), "Preset riêng", 100),
                bounded_text(object.get("description"), "", 1_500),
                settings,
            )
        };
        normalize_settings(&mut settings)?;
        let mut notes: Vec<String> = object
            .get("notes")
            .and_then(Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .take(10)
                    .filter_map(Value::as_str)
                    .map(|value| value.chars().take(300).collect())
                    .collect()
            })
            .unwrap_or_default();
        if source == "VCL" && notes.is_empty() {
            notes.extend(VCL_NOTES.iter().map(|note| (*note).to_owned()));
        }
        Ok(Self {
            name: if name.trim().is_empty() {
                "Preset riêng".into()
            } else {
                name.trim().to_owned()
            },
            description,
            source,
            notes,
            settings,
        })
    }

    pub(super) fn from_project(name: &str, value: Value) -> Result<Self, String> {
        let settings = value.as_object().ok_or("Thiết lập project không hợp lệ")?;
        let mut object = Map::new();
        for (key, setting) in settings {
            if SETTING_KEYS.contains(&key.as_str()) {
                object.insert(key.clone(), setting.clone());
            }
        }
        normalize_settings(&mut object)?;
        Ok(Self {
            name: name.chars().take(100).collect(),
            description: String::new(),
            source: "Riêng".into(),
            notes: Vec::new(),
            settings: object,
        })
    }

    pub(super) fn write_to(&self, path: &Path) -> Result<(), String> {
        if path.exists() {
            return Err("File preset đã tồn tại; hãy chọn tên file khác để tránh ghi đè".into());
        }
        let payload = serde_json::json!({
            "format": FORMAT,
            "version": 1,
            "name": self.name,
            "description": self.description,
            "settings": self.settings,
            "notes": self.notes,
        });
        let bytes = serde_json::to_vec_pretty(&payload).map_err(|error| error.to_string())?;
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        let mut staged =
            tempfile::NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
        std::io::Write::write_all(staged.as_file_mut(), &bytes)
            .map_err(|error| error.to_string())?;
        staged
            .as_file()
            .sync_all()
            .map_err(|error| error.to_string())?;
        staged
            .persist_noclobber(path)
            .map_err(|error| error.error.to_string())?;
        Ok(())
    }

    pub(super) fn read_from(path: &Path) -> Result<Self, String> {
        let metadata = std::fs::metadata(path).map_err(|error| error.to_string())?;
        if metadata.len() > MAX_PRESET_BYTES {
            return Err("File preset vượt quá giới hạn 1 MB".into());
        }
        let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
        let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes);
        let value: Value = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
        Self::decode(value)
    }
}

pub(super) fn load_custom(data_root: &Path) -> Result<Vec<StoredPreset>, String> {
    let directory = data_root.join("presets");
    let entries = match std::fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.to_string()),
    };
    let mut paths = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let metadata = entry.file_type().ok()?;
            let id = path.file_stem()?.to_str()?;
            (metadata.is_file()
                && path.extension().and_then(|extension| extension.to_str()) == Some("json")
                && valid_id(id))
            .then_some((id.to_owned(), path))
        })
        .collect::<Vec<_>>();
    paths.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(paths
        .into_iter()
        .filter_map(|(id, path)| {
            Preset::read_from(&path)
                .ok()
                .map(|preset| StoredPreset { id, preset })
        })
        .collect())
}

pub(super) fn save_custom(data_root: &Path, preset: &Preset) -> Result<StoredPreset, String> {
    let id = vietkara_domain::new_id();
    let path = data_root.join("presets").join(format!("{id}.json"));
    preset.write_to(&path)?;
    Ok(StoredPreset {
        id,
        preset: preset.clone(),
    })
}

pub(super) fn delete_custom(data_root: &Path, id: &str) -> Result<(), String> {
    if !valid_id(id) {
        return Err("ID preset không hợp lệ".into());
    }
    std::fs::remove_file(data_root.join("presets").join(format!("{id}.json")))
        .map_err(|error| error.to_string())
}

fn valid_id(id: &str) -> bool {
    id.len() == 32
        && id
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

pub(super) fn filename_stem(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|character| {
            !character.is_control()
                && !matches!(
                    character,
                    '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
                )
        })
        .take(80)
        .collect();
    let cleaned = cleaned.trim().trim_matches('.');
    if cleaned.is_empty() {
        "preset".into()
    } else {
        cleaned.into()
    }
}

fn bounded_text(value: Option<&Value>, default: &str, max_chars: usize) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or(default)
        .chars()
        .take(max_chars)
        .collect()
}

fn convert_vcl(
    object: &Map<String, Value>,
    raw: &Map<String, Value>,
) -> Result<(String, String, Map<String, Value>), String> {
    let mut settings = Map::new();
    let copy =
        |_target: &str, source: &str, fallback: Value| raw.get(source).cloned().unwrap_or(fallback);
    settings.insert(
        "font".into(),
        copy("font", "font", Value::String("Tahoma".into())),
    );
    settings.insert("font_size".into(), Value::from(110));
    for (target, source, fallback) in [
        ("sung", "sung", "#52efb9"),
        ("unsung", "unsung", "#ffffff"),
        ("outline_unsung", "ou", "#000000"),
        ("outline_sung", "os", "#000000"),
        ("shadow_color", "shsc", "#000000"),
        ("gradient_top", "sungGradTop", "#ffffff"),
        ("gradient_bottom", "sungGradBot", "#1a3ad6"),
    ] {
        settings.insert(
            target.into(),
            copy(target, source, Value::String(fallback.into())),
        );
    }
    for (target, source, fallback) in [
        ("outline_width", "ow", 8.0),
        ("outline_unsung_opacity", "ouA", 100.0),
        ("outline_sung_opacity", "osA", 100.0),
        ("shadow", "shadow", 0.0),
        ("line_height", "lh", 1.4),
        ("vertical_offset", "pos", 0.0),
        ("sweep_feather", "sweepFx", 0.0),
        ("sweep_glow", "sweepGlow", 0.0),
        ("gradient_start", "sungGradPosA", 0.0),
        ("gradient_end", "sungGradPosB", 100.0),
    ] {
        settings.insert(target.into(), copy(target, source, Value::from(fallback)));
    }
    settings.insert(
        "italic".into(),
        Value::Bool(raw.get("italic").is_some_and(vcl_bool)),
    );
    settings.insert("position".into(), Value::String("bottom".into()));
    settings.insert(
        "align".into(),
        copy("align", "align", Value::String("justify".into())),
    );
    settings.insert(
        "line_animation".into(),
        Value::String(
            match raw
                .get("lineAnim")
                .and_then(Value::as_str)
                .unwrap_or("none")
            {
                "fade" => "slowfade",
                "spin" => "flipup",
                value => value,
            }
            .into(),
        ),
    );
    let countdown = raw.get("cdStyle").and_then(Value::as_str).unwrap_or("none");
    settings.insert(
        "countdown".into(),
        Value::String(
            if countdown == "icon" {
                "mic"
            } else {
                countdown
            }
            .into(),
        ),
    );
    settings.insert(
        "sung_gradient".into(),
        Value::Bool(raw.get("sungGradOn").is_some_and(vcl_bool)),
    );
    Ok((
        bounded_text(object.get("name"), "Preset VCL", 100),
        bounded_text(object.get("desc"), "", 1_500),
        settings,
    ))
}

fn vcl_bool(value: &Value) -> bool {
    value
        .as_bool()
        .unwrap_or_else(|| value.as_i64().is_some_and(|number| number != 0))
}

fn normalize_settings(settings: &mut Map<String, Value>) -> Result<(), String> {
    for key in COLORS {
        if let Some(value) = settings.get_mut(*key) {
            let color = value
                .as_str()
                .ok_or_else(|| format!("Màu {key} không hợp lệ"))?;
            if color.len() != 7
                || !color.starts_with('#')
                || !color[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                return Err(format!("Màu {key} phải có dạng #RRGGBB"));
            }
            *value = Value::String(color.to_ascii_lowercase());
        }
    }
    for key in BOOLS {
        if settings.get(*key).is_some_and(|value| !value.is_boolean()) {
            return Err(format!("Giá trị {key} phải là true hoặc false"));
        }
    }
    for (key, choices) in STRING_CHOICES {
        if let Some(value) = settings.get(*key)
            && !value.as_str().is_some_and(|text| choices.contains(&text))
        {
            return Err(format!("Giá trị {key} không hợp lệ"));
        }
    }
    for (key, low, high) in NUMBERS {
        if let Some(value) = settings.get_mut(*key) {
            let number = value
                .as_f64()
                .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
                .filter(|number: &f64| number.is_finite())
                .ok_or_else(|| format!("Giá trị {key} không hợp lệ"))?;
            let number = number.clamp(*low, *high);
            *value = if [
                "font_size",
                "countdown_seconds",
                "line_max_words",
                "title_weight",
            ]
            .contains(key)
            {
                Value::from(number.round_ties_even() as i64)
            } else {
                serde_json::Number::from_f64((number * 100.0).round_ties_even() / 100.0)
                    .map(Value::Number)
                    .ok_or_else(|| format!("Giá trị {key} không hợp lệ"))?
            };
        }
    }
    for key in TEXT {
        if let Some(value) = settings.get_mut(*key) {
            let text = value
                .as_str()
                .ok_or_else(|| format!("Giá trị {key} phải là chuỗi"))?;
            let cleaned = if ["font", "title_font"].contains(key) {
                let value: String = text
                    .chars()
                    .filter(|ch| !matches!(ch, ',' | '\r' | '\n' | '{' | '}' | '\\'))
                    .take(80)
                    .collect();
                if value.trim().is_empty() {
                    "Arial".into()
                } else {
                    value.trim().into()
                }
            } else if key.ends_with("_name") {
                let value: String = text.trim().chars().take(30).collect();
                if value.is_empty() {
                    match *key {
                        "role_a_name" => "Nam".into(),
                        "role_b_name" => "Nữ".into(),
                        "role_unknown_name" => "Chưa rõ".into(),
                        _ => "Giọng hát".into(),
                    }
                } else {
                    value
                }
            } else if ["title_text", "title_credit"].contains(key) {
                text.replace(['\r', '\n'], " ")
                    .trim()
                    .chars()
                    .take(180)
                    .collect()
            } else {
                text.chars().take(80).collect()
            };
            *value = Value::String(cleaned);
        }
    }
    for (key, choices) in [
        (
            "role_a_icon",
            &["man-new-blue.png", "man-new-green.png"][..],
        ),
        (
            "role_b_icon",
            &["lady-pink.png", "lady-red.png", "lady-darkred.png"][..],
        ),
        (
            "role_all_icon",
            &[
                "men(blue) & lady(pink).png",
                "men(blue) & lady(red).png",
                "men(green) & lady(pink).png",
                "men(green) & lady(red).png",
            ][..],
        ),
    ] {
        if let Some(value) = settings.get(key)
            && !value.as_str().is_some_and(|text| choices.contains(&text))
        {
            return Err(format!("Biểu tượng {key} không hợp lệ"));
        }
    }
    let title_start = settings
        .get("title_start")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let title_end = settings
        .get("title_end")
        .and_then(Value::as_f64)
        .unwrap_or(8.0);
    if title_end <= title_start {
        return Err("Thời gian kết thúc title phải sau thời gian bắt đầu".into());
    }
    let gradient_start = settings
        .get("gradient_start")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let gradient_end = settings
        .get("gradient_end")
        .and_then(Value::as_f64)
        .unwrap_or(100.0);
    if gradient_start >= gradient_end {
        settings.insert(
            "gradient_end".into(),
            Value::from((gradient_start + 1.0).min(100.0)),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_preset_round_trips_known_settings_and_filters_unknown_keys() {
        let preset = Preset::decode(serde_json::json!({
            "format": FORMAT, "version": 1, "name": "  Test  ",
            "settings": {"sung":"#ABCDEF", "font_size": 111, "future_key": true},
            "notes": ["note"],
        }))
        .unwrap();
        assert_eq!(preset.settings["sung"], "#abcdef");
        assert_eq!(preset.settings["font_size"], 111);
        assert!(!preset.settings.contains_key("future_key"));
        assert_eq!(preset.notes, ["note"]);
    }

    #[test]
    fn legacy_vcl_preset_converts_visual_settings() {
        let preset = Preset::decode(serde_json::json!({
            "name": "VCL", "desc": "legacy",
            "settings": {"fs": 210, "font":"Tahoma", "sung":"#AABBCC", "italic":1, "lineAnim":"spin", "cdStyle":"icon"},
        })).unwrap();
        assert_eq!(preset.source, "VCL");
        assert_eq!(preset.settings["font_size"], 110);
        assert_eq!(preset.settings["sung"], "#aabbcc");
        assert_eq!(preset.settings["line_animation"], "flipup");
        assert_eq!(preset.settings["countdown"], "mic");
        assert_eq!(preset.notes.len(), 1);
    }

    #[test]
    fn bundled_vcl_presets_decode_with_expected_color_and_font_defaults() {
        assert_eq!(bundled().len(), BUNDLED_PRESETS.len());
        for source in [
            include_str!("../../../resources/presets/01_default.json"),
            include_str!("../../../resources/presets/03_gaming_neon.json"),
        ] {
            let preset = Preset::decode(serde_json::from_str(source).unwrap()).unwrap();
            assert_eq!(preset.source, "VCL");
            assert_eq!(preset.settings["font_size"], 110);
            assert_eq!(preset.settings["font"], "Tahoma");
            assert!(!preset.settings["sung"].as_str().unwrap().contains(' '));
            assert_eq!(preset.notes.len(), 1);
        }
    }

    #[test]
    fn malformed_colors_and_unknown_preset_versions_are_rejected() {
        assert!(
            Preset::decode(serde_json::json!({
                "format": FORMAT, "version": 1, "settings": {"sung":"red"}
            }))
            .is_err()
        );
        assert!(
            Preset::decode(serde_json::json!({
                "format": FORMAT, "version": 2, "settings": {}
            }))
            .is_err()
        );
    }

    #[test]
    fn export_uses_atomic_no_clobber_publish_and_sanitized_filename() {
        let dir = tempfile::tempdir().unwrap();
        let preset = Preset::from_project(
            "Bài hát / Demo",
            serde_json::json!({
                "font": "Tahoma", "sung": "#52efb9", "font_size": 88
            }),
        )
        .unwrap();
        assert_eq!(filename_stem(&preset.name), "Bài hát  Demo");
        assert_eq!(preset.settings["font_size"], 88);
        let path = dir.path().join("demo.preset.json");
        preset.write_to(&path).unwrap();
        assert!(Preset::read_from(&path).is_ok());
        assert!(preset.write_to(&path).is_err());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn custom_catalog_persists_applies_and_deletes_only_generated_ids() {
        let dir = tempfile::tempdir().unwrap();
        let preset = Preset::from_project(
            "Preset của tôi",
            serde_json::json!({"font":"Tahoma", "font_size": 96, "sung":"#52efb9"}),
        )
        .unwrap();
        let stored = save_custom(dir.path(), &preset).unwrap();
        assert_eq!(stored.id.len(), 32);
        assert_eq!(load_custom(dir.path()).unwrap().len(), 1);
        assert_eq!(
            load_custom(dir.path()).unwrap()[0].preset.settings["font_size"],
            96
        );
        assert!(delete_custom(dir.path(), "../outside").is_err());
        delete_custom(dir.path(), &stored.id).unwrap();
        assert!(load_custom(dir.path()).unwrap().is_empty());
    }
}
