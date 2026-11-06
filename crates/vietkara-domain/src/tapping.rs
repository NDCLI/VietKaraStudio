//! Beat-clock tapping state machine, ported from static/tap_timing.js.
//! `wall_ms` is monotonic wall time, distinct from playback time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tap {
    pub index: usize,
    pub start: f64,
    pub end: f64,
}
#[derive(Debug, Clone)]
pub struct TapTiming {
    count: usize,
    pub index: usize,
    pub history: Vec<Tap>,
    pub armed: Option<f64>,
    pub pressing: bool,
    pressed_at: Option<f64>,
}
impl TapTiming {
    pub fn new(count: usize, index: usize) -> Result<Self, String> {
        if count == 0 {
            return Err("Cấu hình gõ nhịp không hợp lệ".into());
        }
        Ok(Self {
            count,
            index: index.min(count - 1),
            history: Vec::new(),
            armed: None,
            pressing: false,
            pressed_at: None,
        })
    }
    pub fn done(&self) -> bool {
        self.index >= self.count
    }
    pub fn cancel(&mut self) {
        self.armed = None;
        self.pressing = false;
        self.pressed_at = None;
    }
    fn commit(&mut self, end: f64) -> Option<Tap> {
        let start = self.armed?;
        if !end.is_finite() || end - start < 0.04 {
            return None;
        }
        let tap = Tap {
            index: self.index,
            start,
            end,
        };
        self.history.push(tap);
        self.index += 1;
        self.cancel();
        Some(tap)
    }
    pub fn down(&mut self, time: f64, repeat: bool, wall_ms: f64) -> Option<Tap> {
        if repeat
            || self.pressing
            || self.done()
            || !time.is_finite()
            || time < 0.
            || !wall_ms.is_finite()
        {
            return None;
        }
        let committed = self.commit(time);
        if self.armed.is_some() {
            return None;
        }
        if !self.done() {
            self.armed = Some(time);
            self.pressing = true;
            self.pressed_at = Some(wall_ms);
        }
        committed
    }
    pub fn up(&mut self, time: f64, wall_ms: f64) -> Option<Tap> {
        if !self.pressing || !wall_ms.is_finite() {
            return None;
        }
        self.pressing = false;
        if wall_ms - self.pressed_at.unwrap_or(wall_ms) < 180. {
            return None;
        }
        let item = self.commit(time);
        self.cancel();
        item
    }
    pub fn finish(&mut self, time: f64) -> Option<Tap> {
        self.commit(time)
    }
    pub fn back(&mut self) -> Option<Tap> {
        self.cancel();
        let item = self.history.pop()?;
        self.index = item.index;
        Some(item)
    }
}

/// Build a validated reversible command without modifying the live project.
pub fn tap_command(project: &crate::Project, tap: Tap) -> Result<crate::edit::Command, String> {
    let mut word = project
        .words
        .get(tap.index)
        .cloned()
        .ok_or("Không tìm thấy từ đang gõ nhịp")?;
    if word.locked == Some(true) {
        return Err("Từ đang khóa".into());
    }
    word.start = crate::timing::map_time(tap.start, &project.anchors, true);
    word.end = crate::timing::map_time(tap.end, &project.anchors, true);
    if !word.start.is_finite() || !word.end.is_finite() || word.start < 0. || word.end <= word.start
    {
        return Err("Mốc đồng bộ đưa từ ra trước bản hát hoặc tạo thời lượng không hợp lệ".into());
    }
    word.manual = Some(true);
    word.estimated = false;
    Ok(crate::edit::Command::ReplaceWord {
        id: word.id.clone(),
        word: Box::new(word),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Anchor, Word};

    fn word(id: &str, start: f64, end: f64) -> Word {
        Word {
            id: id.into(),
            text: id.into(),
            start,
            end,
            line_uid: "line".into(),
            role: "all".into(),
            line: 0,
            estimated: true,
            locked: None,
            section: None,
            section_name: None,
            manual: None,
            label_color: None,
            row: None,
            extra: Default::default(),
        }
    }

    #[test]
    fn quick_taps_commit_on_the_next_press_and_finish_last_word() {
        let mut clock = TapTiming::new(3, 0).unwrap();
        assert_eq!(clock.down(4.0, false, 0.0), None);
        assert_eq!(clock.up(4.05, 50.0), None);
        assert_eq!(
            clock.down(4.5, false, 500.0),
            Some(Tap {
                index: 0,
                start: 4.0,
                end: 4.5,
            })
        );
        assert_eq!(clock.up(4.55, 550.0), None);
        assert_eq!(
            clock.down(5.4, false, 1400.0),
            Some(Tap {
                index: 1,
                start: 4.5,
                end: 5.4,
            })
        );
        assert_eq!(
            clock.finish(5.8),
            Some(Tap {
                index: 2,
                start: 5.4,
                end: 5.8,
            })
        );
        assert!(clock.done());
        assert_eq!(clock.history.len(), 3);
    }

    #[test]
    fn hold_commits_from_keydown_to_keyup_using_wall_clock_threshold() {
        let mut clock = TapTiming::new(2, 0).unwrap();
        assert_eq!(clock.down(32.4, false, 1000.0), None);
        assert_eq!(clock.down(32.7, true, 1100.0), None);
        assert_eq!(
            clock.up(33.2, 1180.0),
            Some(Tap {
                index: 0,
                start: 32.4,
                end: 33.2,
            })
        );
        assert_eq!(clock.history.len(), 1);
        clock.down(33.4, false, 1400.0);
        assert_eq!(
            clock.up(33.8, 1700.0),
            Some(Tap {
                index: 1,
                start: 33.4,
                end: 33.8,
            })
        );
        assert!(clock.done());
    }

    #[test]
    fn invalid_short_or_backwards_clock_does_not_leave_press_armed() {
        let mut clock = TapTiming::new(2, 0).unwrap();
        clock.down(10.0, false, 1000.0);
        assert_eq!(clock.up(9.0, 1300.0), None);
        assert_eq!(clock.armed, None);
        assert_eq!(clock.index, 0);

        clock.down(10.2, false, 1400.0);
        assert_eq!(clock.up(10.23, 1700.0), None);
        assert_eq!(clock.armed, None);
        assert_eq!(clock.index, 0);
    }

    #[test]
    fn repeat_and_too_close_press_do_not_advance_twice() {
        let mut clock = TapTiming::new(2, 0).unwrap();
        clock.down(1.0, false, 1000.0);
        assert_eq!(clock.down(1.01, true, 1010.0), None);
        clock.up(1.02, 1020.0);
        assert_eq!(clock.down(1.03, false, 1030.0), None);
        assert_eq!(clock.index, 0);
        assert_eq!(
            clock.down(1.3, false, 1300.0),
            Some(Tap {
                index: 0,
                start: 1.0,
                end: 1.3,
            })
        );
        assert_eq!(clock.index, 1);
    }

    #[test]
    fn back_discards_armed_press_and_restores_previous_word_index() {
        let mut clock = TapTiming::new(5, 2).unwrap();
        clock.down(9.0, false, 0.0);
        clock.up(9.05, 50.0);
        assert_eq!(
            clock.down(9.4, false, 400.0),
            Some(Tap {
                index: 2,
                start: 9.0,
                end: 9.4,
            })
        );
        clock.down(10.0, false, 1000.0);

        assert_eq!(
            clock.back(),
            Some(Tap {
                index: 2,
                start: 9.0,
                end: 9.4,
            })
        );
        assert_eq!(clock.index, 2);
        assert_eq!(clock.armed, None);
        assert!(!clock.pressing);
    }

    #[test]
    fn tap_command_maps_beat_times_to_reference_and_marks_manual() {
        let mut project = crate::Project::new("tap");
        project.anchors = vec![Anchor {
            r#ref: 1.0,
            beat: 2.0,
        }];
        project.words.push(word("word", 0.0, 1.0));
        let command = tap_command(
            &project,
            Tap {
                index: 0,
                start: 4.0,
                end: 5.0,
            },
        )
        .unwrap();
        let crate::edit::Command::ReplaceWord { word, .. } = command else {
            panic!("tap creates a reversible single-word command");
        };
        assert_eq!((word.start, word.end), (3.0, 4.0));
        assert_eq!(word.manual, Some(true));
        assert!(!word.estimated);
    }

    #[test]
    fn tap_command_rejects_locked_words_and_negative_reference_times() {
        let mut project = crate::Project::new("tap");
        project.anchors = vec![Anchor {
            r#ref: 1.0,
            beat: 2.0,
        }];
        project.words.push(word("word", 0.0, 1.0));
        let tap = Tap {
            index: 0,
            start: 0.5,
            end: 1.0,
        };
        project.words[0].locked = Some(true);
        assert!(tap_command(&project, tap).unwrap_err().contains("khóa"));
        project.words[0].locked = None;
        assert!(tap_command(&project, tap).is_err());
    }
}
