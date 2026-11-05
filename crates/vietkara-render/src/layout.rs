//! Shared lyric row planning for preview and subtitle rendering.
//!
//! Font discovery and glyph shaping are deliberately supplied by the caller:
//! GPUI and the export process must use the same installed/bundled font face.
//! This module owns the timing, wrapping, lane, alignment and positioning rules.

use std::collections::BTreeMap;

use serde_json::Value;
use vietkara_domain::{Project, Word, timing::map_time};

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
    pub text: String,
    pub section_start: bool,
    pub section_end: bool,
    pub words: Vec<PlannedWord>,
}

#[derive(Debug, Clone)]
pub struct DisplayPlan {
    pub width: u32,
    pub height: u32,
    pub font: String,
    pub rows: Vec<DisplayRow>,
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
    measure.width(text, font, size, italic).max(0.0) * horizontal_scale / 100.0
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
        text_width(measure, &text, font, font_size, italic, horizontal_scale) + extra
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
            first.word.role.clone()
        } else {
            "all".into()
        };
        let text = draft
            .words
            .iter()
            .map(|word| word.word.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let mut size = font_size;
        let width_at_size = text_width(measure, &text, font, size, italic, horizontal_scale);
        let shrink = (f64::from(width) * 0.9 / width_at_size.max(1.0)).min(1.0);
        size *= shrink;
        let row_width = width_at_size * shrink;
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
                start: item.start,
                end: item.end,
                x: x + prefix_width * shrink,
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
            text,
            section_start: index == draft.block_start,
            section_end,
            words: positioned,
        });
    }
    DisplayPlan {
        width,
        height,
        font: font.to_owned(),
        rows,
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
    fn planner_folds_all_duet_tracks_into_two_slots_and_maps_anchors() {
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
        assert_eq!(plan.rows[0].start, 4.0);
    }
}
