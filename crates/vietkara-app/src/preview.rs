use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
};

use vietkara_domain::{Anchor, Word, timing::map_time};
use vietkara_render::layout::{DisplayPlan, TextMeasurer};

#[derive(Default)]
pub(super) struct PreviewLayoutCache {
    key: Option<u64>,
    plan: Option<DisplayPlan>,
}

struct GpuiTextMeasurer<'a>(&'a gpui::WindowTextSystem);

impl TextMeasurer for GpuiTextMeasurer<'_> {
    fn width(&self, text: &str, family: &str, size: f64, italic: bool) -> f64 {
        let font = gpui::font(family.to_owned());
        let font = if italic { font.italic() } else { font };
        let run = gpui::TextRun {
            len: text.len(),
            font,
            color: gpui::hsla(0.0, 0.0, 0.0, 1.0),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let shaped = self
            .0
            .shape_line(text.to_owned().into(), gpui::px(size as f32), &[run], None);
        f64::from(shaped.width)
    }
}

fn layout_key(project: &vietkara_domain::Project, words: &[Word]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    project.settings.font.hash(&mut hasher);
    project.settings.font_size.to_bits().hash(&mut hasher);
    serde_json::to_string(&project.settings.extra)
        .unwrap_or_default()
        .hash(&mut hasher);
    for anchor in &project.anchors {
        anchor.r#ref.to_bits().hash(&mut hasher);
        anchor.beat.to_bits().hash(&mut hasher);
    }
    project
        .beat
        .as_ref()
        .and_then(|beat| beat.get("duration"))
        .and_then(serde_json::Value::as_f64)
        .map(f64::to_bits)
        .hash(&mut hasher);
    for word in words {
        word.text.hash(&mut hasher);
        word.start.to_bits().hash(&mut hasher);
        word.end.to_bits().hash(&mut hasher);
        word.line.hash(&mut hasher);
        word.role.hash(&mut hasher);
        word.row.hash(&mut hasher);
        word.section.hash(&mut hasher);
    }
    hasher.finish()
}

pub(super) fn cached_display_plan<'a>(
    cache: &'a mut PreviewLayoutCache,
    project: &vietkara_domain::Project,
    words: &[Word],
    text_system: &gpui::WindowTextSystem,
) -> &'a DisplayPlan {
    let key = layout_key(project, words);
    if cache.key != Some(key) {
        let mut snapshot = project.clone();
        snapshot.words = words.to_vec();
        cache.plan = Some(vietkara_render::layout::display_plan(
            &snapshot,
            &GpuiTextMeasurer(text_system),
        ));
        cache.key = Some(key);
    }
    cache
        .plan
        .as_ref()
        .expect("layout cache was just populated")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct VisibleLine {
    pub(super) line: u32,
    pub(super) slot: u8,
}

/// Select preview rows using the shared measured plan's show windows.
pub(super) fn visible_plan_rows(plan: &DisplayPlan, beat_time: f64, mode: &str) -> Vec<usize> {
    if mode == "karaoke" {
        let mut visible = Vec::new();
        for slot in 0..2 {
            let candidates = plan
                .rows
                .iter()
                .enumerate()
                .filter(|(_, row)| {
                    row.slot == slot && beat_time >= row.show_start && beat_time <= row.show_end
                })
                .collect::<Vec<_>>();
            let singing = candidates
                .iter()
                .copied()
                .filter(|(_, row)| beat_time >= row.start && beat_time <= row.end)
                .max_by(|a, b| a.1.start.total_cmp(&b.1.start));
            if let Some(row) = singing.or_else(|| {
                candidates
                    .into_iter()
                    .max_by(|a, b| a.1.start.total_cmp(&b.1.start))
            }) && !visible
                .iter()
                .any(|&index: &usize| plan.rows[index].line == row.1.line)
            {
                visible.push(row.0);
            }
        }
        visible
    } else {
        plan.rows
            .iter()
            .enumerate()
            .find(|(_, row)| beat_time >= row.show_start && beat_time <= row.show_end)
            .map(|(index, _)| vec![index])
            .unwrap_or_default()
    }
}

struct Row<'a> {
    line: u32,
    words: Vec<&'a Word>,
    start: f64,
    end: f64,
    slot: u8,
    section_start: bool,
    block_start: usize,
    show_start: f64,
    show_end: f64,
}

/// Pick the lyric rows visible at the current beat time using the baseline
/// studio.js show windows and lane arbitration. Word timestamps are in the
/// reference clock; the returned rows are selected in beat-clock time.
pub(super) fn visible_lines(
    words: &[Word],
    anchors: &[Anchor],
    beat_time: f64,
    mode: &str,
    duet: bool,
) -> Vec<VisibleLine> {
    let mut indexes = HashMap::new();
    let mut rows: Vec<Row<'_>> = Vec::new();
    for word in words {
        let index = if let Some(index) = indexes.get(&word.line) {
            *index
        } else {
            let index = rows.len();
            indexes.insert(word.line, index);
            rows.push(Row {
                line: word.line,
                words: Vec::new(),
                start: f64::INFINITY,
                end: 0.,
                slot: 0,
                section_start: false,
                block_start: 0,
                show_start: 0.,
                show_end: 0.,
            });
            index
        };
        let row = &mut rows[index];
        let start = map_time(word.start, anchors, false);
        let end = map_time(word.end, anchors, false);
        row.start = row.start.min(start);
        row.end = row.end.max(end);
        row.words.push(word);
    }
    for row in &mut rows {
        row.words.sort_by(|a, b| {
            map_time(a.start, anchors, false).total_cmp(&map_time(b.start, anchors, false))
        });
        row.section_start = row
            .words
            .first()
            .is_some_and(|word| word.section == Some(true));
    }
    rows.sort_by(|a, b| a.start.total_cmp(&b.start));
    if rows.is_empty() {
        return Vec::new();
    }

    let mut block_row = 0usize;
    let mut block_start = 0usize;
    let role_slots = HashMap::from([
        ("male", 0u8),
        ("female", 1),
        ("voice_c", 2),
        ("voice_d", 3),
        ("voice_e", 4),
        ("voice_f", 5),
    ]);
    for index in 0..rows.len() {
        let new_block =
            index == 0 || rows[index].section_start || rows[index].start - rows[index - 1].end > 4.;
        if new_block {
            block_row = 0;
            block_start = index;
        }
        let first = rows[index].words[0];
        let fallback_slot = (block_row % 2) as u8;
        rows[index].slot = if mode == "karaoke" && duet {
            first
                .row
                .or_else(|| role_slots.get(first.role.as_str()).copied())
                .unwrap_or(fallback_slot)
                % 2
        } else if mode == "karaoke" {
            fallback_slot
        } else {
            0
        };
        rows[index].block_start = block_start;
        block_row += 1;
    }

    for index in 0..rows.len() {
        let start = rows[index].start;
        let end = rows[index].end;
        let slot = rows[index].slot;
        let previous = (0..index).rev().find(|&j| rows[j].slot == slot);
        let following = (index + 1..rows.len()).find(|&j| rows[j].slot == slot);
        let mut show_start = (start - 1.5).max(0.);
        if let Some(previous) = previous {
            show_start = start.min(show_start.max(rows[previous].end + 0.05));
        }
        let block = rows[index].block_start;
        if index == block + 1 && rows[block].slot != slot {
            show_start = show_start.min(rows[block].show_start);
        }
        let mut show_end = end + 0.6;
        if let Some(following) = following {
            show_end = show_end.min((rows[following].start - 1.55).max(end));
        }
        if mode != "karaoke" {
            show_start = start;
            show_end = end + 0.1;
            if let Some(next) = rows.get(index + 1) {
                show_end = show_end.min(next.start.max(end));
            }
        }
        rows[index].show_start = show_start;
        rows[index].show_end = show_end.max(show_start + 0.01);
    }

    if mode == "karaoke" {
        let mut visible = Vec::new();
        for slot in 0..2 {
            let candidates: Vec<_> = rows
                .iter()
                .filter(|row| {
                    row.slot == slot && beat_time >= row.show_start && beat_time <= row.show_end
                })
                .collect();
            let singing: Vec<_> = candidates
                .iter()
                .copied()
                .filter(|row| beat_time >= row.start && beat_time <= row.end)
                .collect();
            if let Some(row) = singing
                .into_iter()
                .max_by(|a, b| a.start.total_cmp(&b.start))
                .or_else(|| {
                    candidates
                        .into_iter()
                        .max_by(|a, b| a.start.total_cmp(&b.start))
                })
            {
                visible.push(VisibleLine {
                    line: row.line,
                    slot,
                });
            }
        }
        visible
    } else {
        rows.iter()
            .find(|row| beat_time >= row.show_start && beat_time <= row.show_end)
            .map(|row| {
                vec![VisibleLine {
                    line: row.line,
                    slot: 0,
                }]
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(text: &str, line: u32, start: f64, end: f64) -> Word {
        serde_json::from_value(serde_json::json!({
            "id": format!("{line:032x}"),
            "line_uid": format!("{:032x}", line + 100),
            "text": text,
            "start": start,
            "end": end,
            "line": line,
            "role": "all"
        }))
        .unwrap()
    }

    #[test]
    fn shared_plan_selects_one_visible_row_per_lane() {
        let row = |index, line, slot, start, end, show_start, show_end| {
            vietkara_render::layout::DisplayRow {
                index,
                line,
                slot,
                start,
                end,
                show_start,
                show_end,
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 24.0,
                font_size: 20.0,
                align: "center".into(),
                role: "all".into(),
                text: "line".into(),
                section_start: false,
                section_end: false,
                words: Vec::new(),
            }
        };
        let plan = DisplayPlan {
            width: 1920,
            height: 1080,
            font: "Arial".into(),
            title: None,
            rows: vec![
                row(0, 0, 0, 1.0, 2.0, 0.0, 2.6),
                row(1, 1, 1, 2.0, 3.0, 0.5, 3.6),
                row(2, 2, 0, 3.0, 4.0, 2.1, 4.6),
            ],
        };
        assert_eq!(visible_plan_rows(&plan, 3.5, "karaoke"), vec![2, 1]);
        assert_eq!(visible_plan_rows(&plan, 3.5, "subtitle"), vec![1]);
    }

    #[test]
    fn karaoke_keeps_one_recently_sung_line_per_lane() {
        let words = vec![
            word("một", 0, 1., 2.),
            word("hai", 1, 2., 3.),
            word("ba", 2, 3., 4.),
        ];
        assert_eq!(
            visible_lines(&words, &[], 1.5, "karaoke", false),
            vec![
                VisibleLine { line: 0, slot: 0 },
                VisibleLine { line: 1, slot: 1 },
            ]
        );
        assert_eq!(
            visible_lines(&words, &[], 3.5, "karaoke", false),
            vec![
                VisibleLine { line: 2, slot: 0 },
                VisibleLine { line: 1, slot: 1 },
            ]
        );
    }

    #[test]
    fn non_karaoke_modes_show_one_active_line_and_duet_roles_select_lanes() {
        let mut first = word("Nam", 0, 1., 2.);
        first.role = "male".into();
        let mut second = word("Nữ", 1, 2., 3.);
        second.role = "female".into();
        assert_eq!(
            visible_lines(
                &[first.clone(), second.clone()],
                &[],
                2.4,
                "subtitle",
                false
            ),
            vec![VisibleLine { line: 1, slot: 0 }]
        );
        assert_eq!(
            visible_lines(&[first, second], &[], 1.5, "karaoke", true),
            vec![
                VisibleLine { line: 0, slot: 0 },
                VisibleLine { line: 1, slot: 1 },
            ]
        );
    }

    #[test]
    fn duet_preview_maps_all_roles_to_two_lanes() {
        let roles = ["male", "female", "voice_c", "voice_d", "voice_e", "voice_f"];
        let words: Vec<_> = roles
            .iter()
            .enumerate()
            .map(|(line, role)| {
                let mut word = word("giọng", line as u32, 1., 2.);
                word.role = (*role).into();
                word
            })
            .collect();

        assert_eq!(
            visible_lines(&words, &[], 1.5, "karaoke", true),
            vec![
                VisibleLine { line: 4, slot: 0 },
                VisibleLine { line: 5, slot: 1 }
            ]
        );
    }
}
