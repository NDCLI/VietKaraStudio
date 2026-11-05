use std::collections::HashMap;

use vietkara_domain::{Anchor, Word, timing::map_time};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct VisibleLine {
    pub(super) line: u32,
    pub(super) slot: u8,
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
