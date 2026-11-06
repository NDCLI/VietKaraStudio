//! Shared lyric row planning for preview and subtitle rendering.
//!
//! Font discovery and glyph shaping are deliberately supplied by the caller:
//! GPUI and the export process must use the same installed/bundled font face.
//! This module owns the timing, wrapping, lane, alignment and positioning rules.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde_json::Value;
use unicode_normalization::UnicodeNormalization;
use vietkara_domain::{Project, Word, timing::map_time};

/// Encode Unicode text for the legacy VNI/TCVN3 font families supported by
/// the Python baseline. Unknown families keep Unicode text unchanged.
pub fn font_text(text: &str, family: &str) -> String {
    let normalized = text.nfc().collect::<String>();
    let family = family.trim().to_lowercase();
    let encoding = if family.starts_with("vni") || family == "vinhan" {
        "vni"
    } else if family.starts_with(".vn") {
        "tcvn3"
    } else {
        return normalized;
    };
    static VNI_TABLE: OnceLock<std::collections::HashMap<char, String>> = OnceLock::new();
    static TCVN3_TABLE: OnceLock<std::collections::HashMap<char, String>> = OnceLock::new();
    let table = if encoding == "vni" {
        VNI_TABLE.get_or_init(|| {
            let mut table = std::collections::HashMap::<char, String>::new();
            let rows = [
                ("aáàảãạ", "a|aù|aø|aû|aõ|aï"),
                ("âấầẩẫậ", "aâ|aá|aà|aå|aã|aä"),
                ("ăắằẳẵặ", "aê|aé|aè|aú|aü|aë"),
                ("eéèẻẽẹ", "e|eù|eø|eû|eõ|eï"),
                ("êếềểễệ", "eâ|eá|eà|eå|eã|eä"),
                ("iíìỉĩị", "i|í|ì|æ|ó|ò"),
                ("oóòỏõọ", "o|où|oø|oû|oõ|oï"),
                ("ôốồổỗộ", "oâ|oá|oà|oå|oã|oä"),
                ("ơớờởỡợ", "ô|ôù|ôø|ôû|ôõ|ôï"),
                ("uúùủũụ", "u|uù|uø|uû|uõ|uï"),
                ("ưứừửữự", "ö|öù|öø|öû|öõ|öï"),
                ("yýỳỷỹỵ", "y|yù|yø|yû|yõ|î"),
                ("đ", "ñ"),
            ];
            for (letters, mapped) in rows {
                for (letter, value) in letters.chars().zip(mapped.split('|')) {
                    table.insert(letter, value.to_owned());
                    table.insert(
                        letter.to_uppercase().next().unwrap_or(letter),
                        value.to_uppercase(),
                    );
                }
            }
            table
        })
    } else {
        TCVN3_TABLE.get_or_init(|| {
            let mut table = std::collections::HashMap::<char, String>::new();
            let letters = "áàảãạâấầẩẫậăắằẳẵặéèẻẽẹêếềểễệíìỉĩịóòỏõọôốồổỗộơớờởỡợúùủũụưứừửữựýỳỷỹỵđ";
            let values = "¸µ¶·¹©ÊÇÈÉË¨¾»¼½ÆÐÌÎÏÑªÕÒÓÔÖÝ×ØÜÞãßáâä«èåæçé¬íêëìîóïñòô\u{AD}øõö÷ùýúûüþ®";
            for (letter, value) in letters.chars().zip(values.chars()) {
                table.insert(letter, value.to_string());
                table.insert(
                    letter.to_uppercase().next().unwrap_or(letter),
                    value.to_uppercase().collect(),
                );
            }
            for (letter, value) in [
                ('Â', '¢'),
                ('Ă', '¡'),
                ('Ê', '£'),
                ('Ô', '¤'),
                ('Ơ', '¥'),
                ('Ư', '¦'),
                ('Đ', '§'),
            ] {
                table.insert(letter, value.to_string());
            }
            table
        })
    };
    normalized
        .chars()
        .map(|character| {
            table
                .get(&character)
                .cloned()
                .unwrap_or_else(|| character.to_string())
        })
        .collect()
}

/// Font metrics at the requested logical frame size. Implementations should
/// shape the complete string (including Vietnamese combining marks) and return
/// its unscaled advance in frame pixels. The planner applies `horizontal_scale`.
pub trait TextMeasurer {
    fn width(&self, text: &str, family: &str, size: f64, italic: bool) -> f64;
}

#[derive(Debug, Clone)]
pub struct PlannedWord {
    pub source_index: usize,
    pub word: Word,
    pub display_text: String,
    pub start: f64,
    pub end: f64,
    pub x: f64,
    pub width: f64,
}

#[derive(Debug, Clone)]
pub struct DisplayRow {
    pub index: usize,
    pub line: u32,
    /// Karaoke lane; always 0 or 1. Non-karaoke modes use lane 0.
    pub slot: u8,
    pub start: f64,
    pub end: f64,
    pub show_start: f64,
    pub show_end: f64,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub font_size: f64,
    pub align: String,
    pub role: String,
    pub role_marker: Option<RoleMarker>,
    pub text: String,
    pub section_start: bool,
    pub section_end: bool,
    pub words: Vec<PlannedWord>,
}

#[derive(Debug, Clone)]
pub struct RoleMarker {
    pub role: String,
    pub x: f64,
    pub y: f64,
    pub size: f64,
    pub width: f64,
    pub color: String,
}

#[derive(Debug, Clone, Copy)]
pub struct CountdownLayout {
    pub start: f64,
    pub end: f64,
    pub x: f64,
    pub y: f64,
    pub size: f64,
}

#[derive(Debug, Clone)]
pub struct DisplayPlan {
    pub width: u32,
    pub height: u32,
    pub font: String,
    pub rows: Vec<DisplayRow>,
    pub title: Option<TitleLayout>,
    pub countdowns: Vec<CountdownLayout>,
}

#[derive(Debug, Clone)]
pub struct LyricsMvRow {
    pub row: DisplayRow,
    pub active: bool,
    pub opacity_percent: u8,
}

struct LyricsMvGeometry<'a> {
    line: u32,
    text: &'a str,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    font_size: f64,
    active: bool,
}

fn lyricsmv_display_row(base: &DisplayRow, geometry: LyricsMvGeometry<'_>) -> DisplayRow {
    DisplayRow {
        index: base.index,
        line: geometry.line,
        slot: base.slot,
        start: base.start,
        end: base.end,
        show_start: base.show_start,
        show_end: base.show_end,
        x: geometry.x,
        y: geometry.y,
        width: geometry.width,
        height: geometry.height,
        font_size: geometry.font_size,
        align: base.align.clone(),
        role: base.role.clone(),
        role_marker: if geometry.active {
            base.role_marker.clone()
        } else {
            None
        },
        text: geometry.text.to_owned(),
        section_start: geometry.active && base.section_start,
        section_end: geometry.active && base.section_end,
        words: Vec::new(),
    }
}

/// Build the active lyrics-MV line and its neighboring ghost lines with the
/// same geometry used by preview and ASS export.
pub fn lyricsmv_rows(
    plan: &DisplayPlan,
    active_index: usize,
    style: &str,
    line_height: f64,
) -> Vec<LyricsMvRow> {
    let Some(active) = plan.rows.get(active_index) else {
        return Vec::new();
    };
    let count = match style {
        "single" => 0isize,
        "scroll" => 2,
        _ => 1,
    };
    let line_height = line_height.clamp(1.0, 2.5);
    let mut rows = vec![LyricsMvRow {
        row: lyricsmv_display_row(
            active,
            LyricsMvGeometry {
                line: active.line,
                text: &active.text,
                x: active.x,
                y: active.y,
                width: active.width,
                height: active.height,
                font_size: active.font_size,
                active: true,
            },
        ),
        active: true,
        opacity_percent: 100,
    }];
    for delta in -count..=count {
        if delta == 0 {
            continue;
        }
        let Some(neighbor) = active_index
            .checked_add_signed(delta)
            .and_then(|index| plan.rows.get(index))
        else {
            continue;
        };
        let font_size = neighbor.font_size * 0.7;
        let width = neighbor.width * 0.7;
        let height = neighbor.height * 0.7;
        let x = match active.align.as_str() {
            "left" => active.x,
            "right" => active.x + active.width - width,
            _ => (f64::from(plan.width) - width) / 2.0,
        };
        let y = active.y + delta as f64 * active.font_size * line_height;
        let row = lyricsmv_display_row(
            active,
            LyricsMvGeometry {
                line: neighbor.line,
                text: &neighbor.text,
                x,
                y,
                width,
                height,
                font_size,
                active: false,
            },
        );
        let opacity_percent = (65 - delta.unsigned_abs() as u8 * 15).max(20);
        rows.push(LyricsMvRow {
            row,
            active: false,
            opacity_percent,
        });
    }
    rows
}

#[derive(Debug, Clone)]
pub struct TitleRow {
    pub text: String,
    pub display_text: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub size: f64,
}

#[derive(Debug, Clone)]
pub struct TitleTemplate {
    pub colors: Vec<String>,
    pub opacity: f64,
    pub border: String,
    pub border_width: f64,
    pub radius: f64,
    pub pad_x: f64,
    pub pad_y: f64,
}

#[derive(Debug, Clone)]
pub struct TitleBox {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub template: TitleTemplate,
}

#[derive(Debug, Clone)]
pub struct TitleLayout {
    pub start: f64,
    pub end: f64,
    pub effect_seconds: f64,
    pub enter: String,
    pub exit: String,
    pub rows: Vec<TitleRow>,
    pub color: String,
    pub outline: String,
    pub outline_width: f64,
    pub shadow: f64,
    pub shadow_color: String,
    pub weight: u32,
    pub italic: bool,
    pub font: String,
    pub template: Option<TitleBox>,
}

#[derive(Clone)]
struct TimedWord {
    source_index: usize,
    word: Word,
    start: f64,
    end: f64,
}

#[derive(Clone)]
struct RowDraft {
    words: Vec<TimedWord>,
    slot: u8,
    block_start: usize,
}

fn number(settings: &serde_json::Map<String, Value>, key: &str, default: f64) -> f64 {
    settings
        .get(key)
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .unwrap_or(default)
}

fn string<'a>(
    settings: &'a serde_json::Map<String, Value>,
    key: &str,
    default: &'a str,
) -> &'a str {
    settings.get(key).and_then(Value::as_str).unwrap_or(default)
}

fn boolean(settings: &serde_json::Map<String, Value>, key: &str, default: bool) -> bool {
    settings
        .get(key)
        .and_then(Value::as_bool)
        .unwrap_or(default)
}

fn text_width<M: TextMeasurer>(
    measure: &M,
    text: &str,
    font: &str,
    size: f64,
    italic: bool,
    horizontal_scale: f64,
) -> f64 {
    measure
        .width(&font_text(text, font), font, size, italic)
        .max(0.0)
        * horizontal_scale
        / 100.0
}

fn setting_color(settings: &serde_json::Map<String, Value>, key: &str, default: &str) -> String {
    settings
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| crate::is_hex_color(value))
        .unwrap_or(default)
        .to_owned()
}

fn title_template(name: &str) -> Option<TitleTemplate> {
    let (colors, opacity, border, border_width, radius, pad_x, pad_y) = match name {
        "gradient" => (
            ["#762ee5", "#e83e86", "#ffab51"],
            96.0,
            "#ffe8f2",
            0.0,
            14.0,
            0.42,
            0.24,
        ),
        "midnight" => (
            ["#101a2b", "#1c2b42", ""],
            95.0,
            "#e8c568",
            2.0,
            8.0,
            0.42,
            0.25,
        ),
        "crystal" => (
            ["#183144", "#476d82", ""],
            82.0,
            "#b8e7f5",
            1.5,
            18.0,
            0.40,
            0.28,
        ),
        "neon" => (
            ["#10112b", "#301446", ""],
            96.0,
            "#48e9dc",
            2.0,
            7.0,
            0.42,
            0.25,
        ),
        "retro" => (
            ["#321b18", "#73401f", ""],
            96.0,
            "#e5aa58",
            2.0,
            2.0,
            0.42,
            0.25,
        ),
        "pastel" => (
            ["#71395e", "#554d83", ""],
            94.0,
            "#ffc7e1",
            1.5,
            16.0,
            0.40,
            0.26,
        ),
        _ => return None,
    };
    Some(TitleTemplate {
        colors: colors
            .into_iter()
            .filter(|color| !color.is_empty())
            .map(str::to_owned)
            .collect(),
        opacity,
        border: border.to_owned(),
        border_width,
        radius,
        pad_x,
        pad_y,
    })
}

fn title_layout<M: TextMeasurer>(
    project: &Project,
    measure: &M,
    width: u32,
    height: u32,
) -> Option<TitleLayout> {
    let settings = &project.settings.extra;
    if !boolean(settings, "title_enabled", false) {
        return None;
    }
    let title = string(settings, "title_text", "").trim();
    if title.is_empty() {
        return None;
    }
    let start = number(settings, "title_start", 0.0).max(0.0);
    let configured_end = number(settings, "title_end", 8.0);
    let beat_duration = project
        .beat
        .as_ref()
        .and_then(|beat| beat.get("duration"))
        .and_then(Value::as_f64)
        .filter(|duration| duration.is_finite() && *duration > 0.0)
        .unwrap_or(configured_end);
    let end = configured_end.min(beat_duration);
    if !end.is_finite() || end <= start {
        return None;
    }
    let font = string(settings, "title_font", "Arial").to_owned();
    let italic = boolean(settings, "title_italic", false);
    let title_size = number(settings, "title_size", 105.0).clamp(30.0, 220.0);
    let credit = string(settings, "title_credit", "").trim();
    let mut rows = Vec::new();
    let mut y = f64::from(height) * number(settings, "title_y", 28.0).clamp(5.0, 85.0) / 100.0;
    for (text, factor) in [(title, 1.0), (credit, 0.43)] {
        if text.is_empty() {
            continue;
        }
        let size = title_size * factor;
        let display_text = font_text(text, &font);
        let measured = measure.width(&display_text, &font, size, italic).max(0.0);
        rows.push(TitleRow {
            text: text.to_owned(),
            display_text,
            x: 0.0,
            y,
            width: measured,
            size,
        });
        y += size * 1.45;
    }
    let initial_width = rows.iter().map(|row| row.width).fold(0.0_f64, f64::max);
    let initial_shrink = (f64::from(width) * 0.86 / initial_width.max(1.0)).min(1.0);
    if initial_shrink < 1.0 {
        let first_y = rows[0].y;
        for row in &mut rows {
            row.size *= initial_shrink;
            row.width *= initial_shrink;
            row.y = first_y + (row.y - first_y) * initial_shrink;
        }
    }

    let template = title_template(string(settings, "title_template", "none"));
    let title_x = number(settings, "title_x", 50.0).clamp(5.0, 95.0) / 100.0;
    let title_box = if let Some(mut template) = template {
        let outline = number(settings, "title_outline_width", 3.0).clamp(0.0, 12.0);
        let shadow = number(settings, "title_shadow", 2.0).clamp(0.0, 15.0);
        let paint_x = outline + shadow * 2.5 + template.border_width + 3.0;
        let paint_y = paint_x;
        let mut main_size = rows[0].size;
        let mut pad_x = (main_size * template.pad_x).max(paint_x + main_size * 0.04);
        let mut pad_y = (main_size * template.pad_y).max(paint_y + main_size * 0.04);
        let content_width = rows.iter().map(|row| row.width).fold(0.0_f64, f64::max);
        let shrink =
            ((f64::from(width) * 0.94 - 2.0 * pad_x) / (content_width * 1.02).max(1.0)).min(1.0);
        if shrink < 1.0 {
            let first_y = rows[0].y;
            for row in &mut rows {
                row.size *= shrink;
                row.width *= shrink;
                row.y = first_y + (row.y - first_y) * shrink;
            }
            main_size = rows[0].size;
            pad_x = (main_size * template.pad_x).max(paint_x + main_size * 0.04);
            pad_y = (main_size * template.pad_y).max(paint_y + main_size * 0.04);
        }
        template.pad_x = pad_x;
        template.pad_y = pad_y;
        let first_y = rows[0].y;
        let box_height =
            rows.last().unwrap().y + rows.last().unwrap().size * 1.2 - first_y + 2.0 * pad_y;
        let box_y = (f64::from(height) * 0.02)
            .max((rows[0].y - pad_y).min(f64::from(height) * 0.98 - box_height));
        let content_width = rows.iter().map(|row| row.width).fold(0.0_f64, f64::max);
        let box_width = (content_width + 2.0 * pad_x).min(f64::from(width) * 0.94);
        let box_x = (f64::from(width) * 0.02).max(
            (f64::from(width) * title_x - box_width / 2.0).min(f64::from(width) * 0.98 - box_width),
        );
        // ASS/export rows use width_factor=1 in the Python baseline. The 1.02
        // inset belongs only to its preview rows and must not shift the shared
        // export layout.
        let text_inset = pad_x;
        for row in &mut rows {
            row.x = box_x + text_inset;
            row.y = row.y + box_y + pad_y - first_y;
        }
        Some(TitleBox {
            x: box_x,
            y: box_y,
            width: box_width,
            height: box_height,
            template,
        })
    } else {
        let content_width = rows.iter().map(|row| row.width).fold(0.0_f64, f64::max);
        let x = (f64::from(width) * 0.02).max(
            (f64::from(width) * title_x - content_width / 2.0)
                .min(f64::from(width) * 0.98 - content_width),
        );
        let overflow = (rows.last().unwrap().y + rows.last().unwrap().size * 1.2
            - f64::from(height) * 0.95)
            .max(0.0);
        for row in &mut rows {
            row.x = x;
            row.y -= overflow;
        }
        None
    };
    Some(TitleLayout {
        start,
        end,
        effect_seconds: number(settings, "title_effect_seconds", 0.7)
            .clamp(0.1, 3.0)
            .min((end - start) / 2.0),
        enter: string(settings, "title_enter", "fade").to_owned(),
        exit: string(settings, "title_exit", "fade").to_owned(),
        rows,
        color: setting_color(settings, "title_color", "#ffffff"),
        outline: setting_color(settings, "title_outline", "#15202a"),
        outline_width: number(settings, "title_outline_width", 3.0).clamp(0.0, 12.0),
        shadow: number(settings, "title_shadow", 2.0).clamp(0.0, 15.0),
        shadow_color: setting_color(settings, "title_shadow_color", "#000000"),
        weight: number(settings, "title_weight", 700.0).clamp(300.0, 900.0) as u32,
        italic,
        font,
        template: title_box,
    })
}

/// Build the lyric portion of the display plan from project words and anchors.
/// The API intentionally accepts the same measurer for preview and export so
/// row breaks and word positions cannot drift between the two surfaces.
pub fn display_plan<M: TextMeasurer>(project: &Project, measure: &M) -> DisplayPlan {
    let settings = &project.settings.extra;
    let (width, height) = match project.settings.aspect {
        vietkara_domain::settings::Aspect::Landscape => (1920, 1080),
        vietkara_domain::settings::Aspect::Portrait => (1080, 1920),
    };
    let font = project.settings.font.as_str();
    let font_size = project.settings.font_size.clamp(20.0, 240.0);
    let italic = boolean(settings, "italic", false);
    let line_height = number(settings, "line_height", 1.4).clamp(1.0, 2.5);
    let outline = number(settings, "outline_width", 3.0).clamp(0.0, 12.0);
    let shadow = number(settings, "shadow", 1.0).clamp(0.0, 15.0);
    let horizontal_scale = number(settings, "horizontal_scale", 100.0).clamp(50.0, 150.0);
    let display_mode = string(settings, "display_mode", "karaoke");
    let duet = string(settings, "singing_mode", "solo") == "duet";
    let role_icons = boolean(settings, "role_icons", true) && duet && display_mode == "karaoke";
    let marker_ratio = if role_icons {
        ["male", "female", "all"]
            .into_iter()
            .filter_map(|role| {
                crate::role_icons::role_icon(crate::role_icons::role_icon_setting(role, settings))
            })
            .map(|icon| icon.width)
            .fold(1.0_f64, f64::max)
    } else {
        0.0
    };
    let alignment = string(settings, "align", "center");
    let position = string(settings, "position", "bottom");
    let vertical_offset = number(settings, "vertical_offset", 0.0).clamp(0.0, 75.0);
    let horizontal_offset = number(settings, "horizontal_offset", 0.0).clamp(-40.0, 40.0);
    let duration = project
        .beat
        .as_ref()
        .and_then(|beat| beat.get("duration"))
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && *value >= 0.0)
        .unwrap_or(f64::INFINITY);

    let mut lines: BTreeMap<u32, Vec<TimedWord>> = BTreeMap::new();
    for (source_index, word) in project.words.iter().enumerate() {
        let start = map_time(word.start, &project.anchors, false);
        let end = map_time(word.end, &project.anchors, false);
        if !start.is_finite() || !end.is_finite() || end <= 0.0 || start >= duration {
            continue;
        }
        lines.entry(word.line).or_default().push(TimedWord {
            source_index,
            word: word.clone(),
            start: start.max(0.0),
            end: end.min(duration),
        });
    }

    let mut original_lines = lines.into_values().collect::<Vec<_>>();
    for line in &mut original_lines {
        line.sort_by(|a, b| a.start.total_cmp(&b.start));
    }
    let measure_fits = |words: &[TimedWord]| {
        let text = words
            .iter()
            .map(|word| word.word.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let extra = 2.0 * outline + shadow + if italic { font_size * 0.15 } else { 0.0 };
        // Match layout.py::role_icon_space: reserve the selected widest
        // artwork plus a small gap so lyric text cannot touch the marker.
        let marker_space = if role_icons {
            font_size * (0.72 * marker_ratio + 0.22)
        } else {
            0.0
        };
        text_width(measure, &text, font, font_size, italic, horizontal_scale) + extra + marker_space
            <= f64::from(width) * 0.9
    };

    let mut drafts = Vec::new();
    for line in original_lines {
        let mut chunk = Vec::new();
        for word in line {
            let pause = chunk
                .last()
                .is_some_and(|previous: &TimedWord| word.start - previous.end > 4.0);
            let too_wide = !chunk.is_empty() && {
                let mut candidate = chunk.clone();
                candidate.push(word.clone());
                !measure_fits(&candidate)
            };
            if !chunk.is_empty() && (pause || too_wide) {
                drafts.push(RowDraft {
                    words: std::mem::take(&mut chunk),
                    slot: 0,
                    block_start: 0,
                });
            }
            chunk.push(word);
        }
        if !chunk.is_empty() {
            drafts.push(RowDraft {
                words: chunk,
                slot: 0,
                block_start: 0,
            });
        }
    }
    drafts.sort_by(|a, b| a.words[0].start.total_cmp(&b.words[0].start));

    let role_slot = |role: &str| match role {
        "male" | "voice_c" | "voice_e" => Some(0),
        "female" | "voice_d" | "voice_f" => Some(1),
        _ => None,
    };
    let mut block_row = 0usize;
    let mut block_start = 0usize;
    for index in 0..drafts.len() {
        let first = &drafts[index].words[0];
        let new_block = index == 0
            || first.word.section == Some(true)
            || first.start
                - drafts[index - 1]
                    .words
                    .last()
                    .expect("previous row is nonempty")
                    .end
                > 4.0;
        if new_block {
            block_row = 0;
            block_start = index;
        }
        let fallback = (block_row % 2) as u8;
        let role = first.word.role.as_str();
        drafts[index].slot = if display_mode == "karaoke" && duet {
            first
                .word
                .row
                .map(|row| row % 2)
                .or_else(|| role_slot(role))
                .unwrap_or(fallback)
        } else if display_mode == "karaoke" {
            fallback
        } else {
            0
        };
        drafts[index].block_start = block_start;
        block_row += 1;
    }

    let mut rows: Vec<DisplayRow> = Vec::with_capacity(drafts.len());
    let mut previous_role: Option<String> = None;
    for index in 0..drafts.len() {
        let draft = &drafts[index];
        let first = &draft.words[0];
        let start = first.start;
        let end = draft
            .words
            .iter()
            .map(|word| word.end)
            .fold(start, f64::max);
        let previous = (0..index).rev().find(|&j| drafts[j].slot == draft.slot);
        let following = (index + 1..drafts.len()).find(|&j| drafts[j].slot == draft.slot);
        let mut show_start = (start - 1.5).max(0.0);
        if let Some(previous) = previous {
            show_start =
                start.min(show_start.max(drafts[previous].words.last().unwrap().end + 0.05));
        }
        if index == draft.block_start + 1 && drafts[draft.block_start].slot != draft.slot {
            show_start = show_start.min(rows[draft.block_start].show_start);
        }
        let mut show_end = end + 0.6;
        if let Some(following) = following {
            show_end = show_end.min((drafts[following].words[0].start - 1.55).max(end));
        }
        if display_mode != "karaoke" {
            show_start = start;
            show_end = end + 0.1;
            if let Some(next) = drafts.get(index + 1) {
                show_end = show_end.min(next.words[0].start.max(end));
            }
        }
        let role = if duet && display_mode == "karaoke" {
            match first.word.role.as_str() {
                "voice_c" | "voice_e" => "male".to_owned(),
                "voice_d" | "voice_f" => "female".to_owned(),
                role => role.to_owned(),
            }
        } else {
            "all".into()
        };
        let marker_space = if role_icons
            && matches!(role.as_str(), "male" | "female" | "all")
            && previous_role.as_deref() != Some(role.as_str())
        {
            font_size * (0.72 * marker_ratio + 0.22)
        } else {
            0.0
        };
        previous_role = Some(role.clone());
        let text = draft
            .words
            .iter()
            .map(|word| word.word.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let mut size = font_size;
        let width_at_size = text_width(measure, &text, font, size, italic, horizontal_scale);
        let shrink = (f64::from(width) * 0.9 / (width_at_size + marker_space).max(1.0)).min(1.0);
        size *= shrink;
        let marker_width = marker_space * shrink;
        let row_width = width_at_size * shrink + marker_width;
        let align = if alignment == "justify" {
            if draft.slot == 0 { "left" } else { "right" }
        } else {
            alignment
        };
        let mut x = match align {
            "left" => f64::from(width) * 0.05,
            "right" => f64::from(width) * 0.95 - row_width,
            _ => (f64::from(width) - row_width) / 2.0,
        } + f64::from(width) * horizontal_offset / 100.0;
        x = x.clamp(
            f64::from(width) * 0.02,
            (f64::from(width) * 0.98 - row_width).max(0.0),
        );
        let baseline = match position {
            "top" => size * (line_height + 2.0),
            "center" => f64::from(height) / 2.0 + size * 0.5,
            _ if display_mode == "lyricsmv" => f64::from(height) * 0.5,
            _ => f64::from(height) - size * 1.25,
        } - f64::from(height) * vertical_offset / 100.0;
        let lane_offset = if display_mode == "karaoke" {
            (1 - i32::from(draft.slot)) as f64 * (1.15 + line_height)
        } else {
            0.0
        };
        let y = (baseline - size * (1.15 + lane_offset)).max(0.0);
        let role_color = match role.as_str() {
            "male" => setting_color(settings, "role_a_color", "#62b5ff"),
            "female" => setting_color(settings, "role_b_color", "#ff87b7"),
            "all" => setting_color(settings, "role_all_color", "#f4cf72"),
            // Legacy projects can contain more than two voice identities. Keep
            // their data, but render them with the two supported duet lanes.
            "voice_c" | "voice_e" => setting_color(settings, "role_a_color", "#62b5ff"),
            "voice_d" | "voice_f" => setting_color(settings, "role_b_color", "#ff87b7"),
            _ => setting_color(settings, "role_unknown_color", "#ffffff"),
        };
        let role_marker = (marker_width > 0.0).then(|| RoleMarker {
            role: role.clone(),
            x,
            y: y + size * 0.08,
            size: size * 0.72,
            width: crate::role_icons::role_icon(crate::role_icons::role_icon_setting(
                &role, settings,
            ))
            .map(|icon| size * 0.72 * icon.width / icon.height)
            .unwrap_or(0.0),
            color: role_color,
        });
        let mut positioned = Vec::with_capacity(draft.words.len());
        for (word_index, item) in draft.words.iter().enumerate() {
            let prefix = draft.words[..word_index]
                .iter()
                .map(|word| word.word.text.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            let prefix_width = if prefix.is_empty() {
                0.0
            } else {
                text_width(
                    measure,
                    &format!("{prefix} "),
                    font,
                    font_size,
                    italic,
                    horizontal_scale,
                )
            };
            let word_width = text_width(
                measure,
                &item.word.text,
                font,
                font_size,
                italic,
                horizontal_scale,
            ) * shrink;
            positioned.push(PlannedWord {
                source_index: item.source_index,
                word: item.word.clone(),
                display_text: font_text(&item.word.text, font),
                start: item.start,
                end: item.end,
                x: x + marker_width + prefix_width * shrink,
                width: word_width,
            });
        }
        let section_end = index + 1 == drafts.len()
            || drafts[index + 1].words[0].word.section == Some(true)
            || drafts[index + 1].words[0].start - end > 4.0;
        rows.push(DisplayRow {
            index,
            line: first.word.line,
            slot: draft.slot,
            start,
            end,
            show_start,
            show_end: show_end.max(show_start + 0.01),
            x,
            y,
            width: row_width,
            height: size * 1.2,
            font_size: size,
            align: align.to_owned(),
            role,
            role_marker,
            text,
            section_start: index == draft.block_start,
            section_end,
            words: positioned,
        });
    }
    let mut countdowns = Vec::new();
    let countdown = string(settings, "countdown", "none");
    if display_mode == "karaoke" && countdown != "none" {
        let countdown_seconds = number(settings, "countdown_seconds", 4.0).clamp(1.0, 8.0);
        let mut previous_vocal_end = 0.0_f64;
        for (index, row) in rows.iter().enumerate() {
            let count_in = index == 0 || row.start - previous_vocal_end >= 8.0;
            if count_in {
                let end = (row.start - 0.05).max(0.0);
                let start = 0.0_f64.max(previous_vocal_end).max(end - countdown_seconds);
                if end - start >= 0.4 {
                    countdowns.push(CountdownLayout {
                        start,
                        end,
                        x: f64::from(width) * 0.05,
                        y: (row.y - row.font_size * 0.9).max(0.0),
                        size: row.font_size * 0.7,
                    });
                }
            }
            previous_vocal_end = previous_vocal_end.max(row.end);
        }
    }

    DisplayPlan {
        width,
        height,
        font: font.to_owned(),
        rows,
        title: title_layout(project, measure, width, height),
        countdowns,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vietkara_domain::{
        Word,
        settings::{Aspect, Settings},
    };

    struct FixedWidth;
    impl TextMeasurer for FixedWidth {
        fn width(&self, text: &str, _family: &str, size: f64, _italic: bool) -> f64 {
            text.chars().count() as f64 * size * 0.5
        }
    }

    #[test]
    fn legacy_vietnamese_font_encodings_match_baseline_tables() {
        assert_eq!(font_text("Xin chào", "VNI-Avo"), "Xin chaøo");
        assert_eq!(font_text("Xin chào", ".VnArialH"), "Xin chµo");
        assert_eq!(font_text("Á Â Đ", ".VnArialH"), "¸ ¢ §");
        assert_eq!(font_text("A\u{301}", "VNI-Times"), "AÙ");
        assert_eq!(font_text("Xin chào", "Arial"), "Xin chào");
        assert_eq!(font_text("íìỉĩịóòỏõọ", ".VnArialH"), "Ý×ØÜÞãßáâä");
    }

    fn word(text: &str, line: u32, start: f64, role: &str, row: Option<u8>) -> Word {
        serde_json::from_value(serde_json::json!({
            "id": format!("{line:032x}"), "line_uid": format!("{:032x}", line + 1),
            "text":text, "start":start, "end":start + 0.5, "line":line,
            "role":role, "row":row
        }))
        .unwrap()
    }

    #[test]
    fn planner_wraps_measured_rows_and_keeps_word_positions_inside_row() {
        let mut project = Project {
            schema: 2,
            id: "p".into(),
            name: "test".into(),
            reference: None,
            beat: None,
            anchors: vec![],
            settings: Settings::default(),
            text: String::new(),
            alignment: None,
            scenes: vec![],
            words: vec![
                word("Xin", 0, 1., "all", None),
                word("chào", 0, 1.6, "all", None),
                word("bạn", 0, 2.2, "all", None),
            ],
            extra: Default::default(),
        };
        project.settings.aspect = Aspect::Portrait;
        project.settings.font_size = 240.0;
        let plan = display_plan(&project, &FixedWidth);
        assert_eq!((plan.width, plan.height), (1080, 1920));
        assert!(plan.rows.len() >= 2);
        assert_eq!(plan.rows[0].start, 1.0);
        for row in &plan.rows {
            assert!(row.x >= 0.0 && row.x + row.width <= f64::from(plan.width));
            assert!(
                row.words
                    .iter()
                    .all(|word| word.x >= row.x && word.x + word.width <= row.x + row.width + 0.01)
            );
            assert!(row.show_start <= row.start && row.start < row.show_end);
        }
    }

    #[test]
    fn planner_reserves_space_for_widest_selected_duet_artwork() {
        let mut project = Project::new("duet marker spacing");
        project.settings.font_size = 60.0;
        project
            .settings
            .extra
            .insert("singing_mode".into(), Value::String("duet".into()));
        project
            .settings
            .extra
            .insert("role_icons".into(), Value::Bool(true));
        project.words = vec![word("Xin", 0, 1.0, "male", None)];

        let plan = display_plan(&project, &FixedWidth);
        let row = &plan.rows[0];
        let expected_space = row.font_size * (0.72 * (122.0 / 90.0) + 0.22);
        assert!((row.words[0].x - row.x - expected_space).abs() < 0.01);
        assert!(row.width >= expected_space + row.words[0].width);
    }

    #[test]
    fn lyricsmv_preview_rows_share_measured_geometry_and_opacity() {
        let mut project = Project::new("lyricsmv");
        project
            .settings
            .extra
            .insert("display_mode".into(), Value::String("lyricsmv".into()));
        project.settings.font_size = 60.0;
        project.words = (0..4)
            .map(|line| {
                word(
                    &format!("dòng {line}"),
                    line,
                    line as f64 + 1.0,
                    "all",
                    None,
                )
            })
            .collect();
        let plan = display_plan(&project, &FixedWidth);

        let single = lyricsmv_rows(&plan, 1, "single", 1.4);
        assert_eq!(single.len(), 1);
        assert!(single[0].active);
        assert_eq!(single[0].row.line, plan.rows[1].line);

        let stack = lyricsmv_rows(&plan, 1, "stack", 1.4);
        assert_eq!(stack.len(), 3);
        assert_eq!(stack[1].row.line, plan.rows[0].line);
        assert_eq!(stack[1].opacity_percent, 50);
        assert!((stack[1].row.font_size - plan.rows[0].font_size * 0.7).abs() < 1e-9);
        assert!((stack[1].row.y - (plan.rows[1].y - plan.rows[1].font_size * 1.4)).abs() < 1e-9);

        let scroll = lyricsmv_rows(&plan, 1, "scroll", 1.4);
        assert_eq!(scroll.len(), 4);
        assert_eq!(scroll[3].row.line, plan.rows[3].line);
        assert_eq!(scroll[3].opacity_percent, 35);
        assert!(scroll.iter().filter(|row| row.active).count() == 1);
    }

    #[test]
    fn planner_folds_legacy_duet_tracks_into_two_roles_and_two_slots() {
        let roles = ["male", "female", "voice_c", "voice_d", "voice_e", "voice_f"];
        let mut project = Project {
            schema: 2,
            id: "p".into(),
            name: "test".into(),
            reference: None,
            beat: None,
            anchors: vec![vietkara_domain::Anchor {
                r#ref: 0.,
                beat: 3.,
            }],
            settings: Settings::default(),
            text: String::new(),
            alignment: None,
            scenes: vec![],
            words: roles
                .iter()
                .enumerate()
                .map(|(i, role)| word(role, i as u32, i as f64 + 1., role, Some(i as u8)))
                .collect(),
            extra: Default::default(),
        };
        project
            .settings
            .extra
            .insert("singing_mode".into(), Value::String("duet".into()));
        let plan = display_plan(&project, &FixedWidth);
        assert_eq!(
            plan.rows.iter().map(|row| row.slot).collect::<Vec<_>>(),
            [0, 1, 0, 1, 0, 1]
        );
        let marker_colors: Vec<_> = plan
            .rows
            .iter()
            .filter_map(|row| row.role_marker.as_ref().map(|marker| marker.color.clone()))
            .collect();
        assert_eq!(marker_colors.len(), roles.len());
        assert_eq!(marker_colors[0], marker_colors[2]);
        assert_eq!(marker_colors[2], marker_colors[4]);
        assert_eq!(marker_colors[1], marker_colors[3]);
        assert_eq!(marker_colors[3], marker_colors[5]);
        assert!(plan.rows.iter().all(|row| row.slot <= 1));
        assert_eq!(plan.rows[0].start, 4.0);
    }
}
