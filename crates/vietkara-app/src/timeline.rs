use super::*;
use gpui::Div;

fn clamp_seek_position(position: f64, duration: f64) -> f64 {
    if !position.is_finite() || !duration.is_finite() {
        return 0.0;
    }
    position.clamp(0.0, duration.max(0.0))
}

pub(super) fn track_to_timeline_time(
    slot: AudioSlot,
    position: f64,
    anchors: &[vietkara_domain::Anchor],
) -> f64 {
    match slot {
        AudioSlot::Beat => position,
        AudioSlot::Reference => vietkara_domain::timing::map_time(position, anchors, false),
    }
}

pub(super) fn timeline_to_track_time(
    slot: AudioSlot,
    position: f64,
    anchors: &[vietkara_domain::Anchor],
) -> f64 {
    match slot {
        AudioSlot::Beat => position,
        AudioSlot::Reference => vietkara_domain::timing::map_time(position, anchors, true),
    }
}

fn waveform_timeline_interval(
    slot: AudioSlot,
    first_bin: usize,
    end_bin: usize,
    bin_count: usize,
    track_duration: f64,
    anchors: &[vietkara_domain::Anchor],
) -> Option<(f64, f64)> {
    if bin_count == 0 || !track_duration.is_finite() || track_duration <= 0.0 {
        return None;
    }
    let start = first_bin as f64 / bin_count as f64 * track_duration;
    let end = end_bin.min(bin_count) as f64 / bin_count as f64 * track_duration;
    let start = track_to_timeline_time(slot, start, anchors).max(0.0);
    let end = track_to_timeline_time(slot, end, anchors).max(0.0);
    (end > start).then_some((start, end))
}

fn insert_anchor_at_beat(
    beat: f64,
    anchors: &[vietkara_domain::Anchor],
) -> Result<Vec<vietkara_domain::Anchor>, String> {
    if !beat.is_finite() || beat < 0.0 {
        return Err("Playhead không hợp lệ để thêm anchor".into());
    }
    let reference = vietkara_domain::timing::map_time(beat, anchors, true).max(0.0);
    if anchors
        .iter()
        .any(|anchor| (anchor.beat - beat).abs() < 0.02 || (anchor.r#ref - reference).abs() < 0.02)
    {
        return Err("Đã có anchor quá gần playhead".into());
    }
    let mut candidate = anchors.to_vec();
    candidate.push(vietkara_domain::Anchor {
        r#ref: reference,
        beat,
    });
    candidate.sort_by(|left, right| left.r#ref.total_cmp(&right.r#ref));
    Ok(candidate)
}

pub(super) fn project_timeline_duration(
    project: &Project,
    playback_duration: f64,
    anchors: &[vietkara_domain::Anchor],
) -> f64 {
    let media_duration = [
        (AudioSlot::Reference, project.reference.as_ref()),
        (AudioSlot::Beat, project.beat.as_ref()),
    ]
    .into_iter()
    .filter_map(|(slot, metadata)| {
        let duration = metadata?.get("duration")?.as_f64()?;
        Some(track_to_timeline_time(slot, duration, anchors).max(0.0))
    })
    .fold(0.0_f64, f64::max);
    let word_duration = project
        .words
        .iter()
        .map(|word| vietkara_domain::timing::map_time(word.end, anchors, false))
        .fold(0.0_f64, f64::max);
    playback_duration
        .max(media_duration)
        .max(word_duration)
        .max(1.0)
}

pub(super) struct TimelineDrag {
    index: usize,
    edge: vietkara_domain::timeline::Edge,
    anchor_index: Option<usize>,
    anchor_origin: f64,
    origin: f32,
    scroll: f32,
    command: Option<Command>,
    pub(super) words: Vec<Word>,
    pub(super) anchors: Vec<vietkara_domain::Anchor>,
}
impl Studio {
    pub(super) fn add_anchor_at_playhead(&mut self, cx: &mut Context<Self>) {
        if self.saving
            || self.autosaving
            || self.tapping.is_some()
            || self.timeline_drag.is_some()
            || self.recovery.is_some()
            || self.pending_lyrics.is_some()
            || self.pending_audio.is_some()
        {
            return;
        }
        let project = self.editor.project();
        if project.reference.is_none() || project.beat.is_none() {
            self.edit_status = "Cần cả bản hát và beat để thêm anchor".into();
            cx.notify();
            return;
        }
        let candidate = match insert_anchor_at_beat(self.play_position, &project.anchors) {
            Ok(candidate) => candidate,
            Err(error) => {
                self.edit_status = error;
                cx.notify();
                return;
            }
        };
        self.edit_status = match self.editor.execute(Command::SetAnchors(candidate)) {
            Ok(_) => {
                self.dirty = true;
                self.autosave_pending = true;
                "Đã thêm anchor tại playhead • chưa lưu".into()
            }
            Err(error) => error.to_string(),
        };
        cx.notify();
    }

    pub(super) fn seek_timeline(&mut self, position: f64, cx: &mut Context<Self>) {
        if self.tapping.is_some() || self.timeline_drag.is_some() {
            return;
        }
        let project = self.editor.project();
        let duration = project_timeline_duration(project, self.play_duration, &project.anchors);
        let position = clamp_seek_position(position, duration);
        #[cfg(windows)]
        if let Some(worker) = &self.playback
            && let Err(error) = worker.send(PlaybackCommand::Seek(timeline_to_track_time(
                self.listen_track,
                position,
                &self.editor.project().anchors,
            )))
        {
            self.playback_error = Some(error);
        }
        self.play_position = position;
        self.request_background_frame(cx);
        cx.notify();
    }

    pub(super) fn start_resize(
        &mut self,
        index: usize,
        edge: vietkara_domain::timeline::Edge,
        x: f32,
        cx: &mut Context<Self>,
    ) {
        if self.saving
            || self.tapping.is_some()
            || self.recovery.is_some()
            || self.pending_lyrics.is_some()
            || self.pending_audio.is_some()
        {
            return;
        }
        let Some(word) = self.editor.project().words.get(index) else {
            return;
        };
        if word.locked == Some(true) {
            return;
        }
        let word_id = word.id.clone();
        let word_text = word.text.clone();
        self.selected_word = Some(word_id);
        self.set_word_input(&word_text, cx);
        self.timeline_drag = Some(TimelineDrag {
            index,
            edge,
            anchor_index: None,
            anchor_origin: 0.0,
            origin: x,
            scroll: f32::from(self.timeline_scroll.offset().x),
            command: None,
            words: self.editor.project().words.clone(),
            anchors: self.editor.project().anchors.clone(),
        });
        cx.stop_propagation();
        cx.notify();
    }

    pub(super) fn start_anchor_move(&mut self, index: usize, x: f32, cx: &mut Context<Self>) {
        if self.saving
            || self.tapping.is_some()
            || self.recovery.is_some()
            || self.pending_lyrics.is_some()
            || self.pending_audio.is_some()
        {
            return;
        }
        let Some(anchor) = self.editor.project().anchors.get(index) else {
            return;
        };
        self.timeline_drag = Some(TimelineDrag {
            index: 0,
            edge: vietkara_domain::timeline::Edge::Start,
            anchor_index: Some(index),
            anchor_origin: anchor.beat,
            origin: x,
            scroll: f32::from(self.timeline_scroll.offset().x),
            command: None,
            words: self.editor.project().words.clone(),
            anchors: self.editor.project().anchors.clone(),
        });
        cx.stop_propagation();
        cx.notify();
    }

    pub(super) fn move_resize(&mut self, x: f32, cx: &mut Context<Self>) {
        let Some(drag) = &mut self.timeline_drag else {
            return;
        };
        let delta = (x - drag.origin - f32::from(self.timeline_scroll.offset().x) + drag.scroll)
            as f64
            / self.timeline_zoom as f64;
        let project = self.editor.project();
        if let Some(anchor_index) = drag.anchor_index {
            let mut anchors = project.anchors.clone();
            if let Some(anchor) = anchors.get_mut(anchor_index) {
                anchor.beat = (drag.anchor_origin + delta).max(0.0);
                if let Ok(mut preview) = Editor::new(project.clone())
                    && preview.execute(Command::SetAnchors(anchors)).is_ok()
                {
                    drag.anchors = preview.project().anchors.clone();
                    drag.command = Some(Command::SetAnchors(drag.anchors.clone()));
                }
            }
            cx.notify();
            return;
        }
        let duration = project
            .words
            .iter()
            .map(|w| vietkara_domain::timing::map_time(w.end, &project.anchors, false))
            .fold(self.play_duration.max(1.), f64::max);
        let result =
            vietkara_domain::timeline::resize(project, drag.index, drag.edge, delta, duration);
        match result {
            Ok(command) => {
                if let Ok(mut preview) = Editor::new(project.clone())
                    && preview.execute(command.clone()).is_ok()
                {
                    drag.words = preview.project().words.clone();
                    drag.command = Some(command);
                }
            }
            Err(error) => self.edit_status = error,
        }
        cx.notify();
    }
    pub(super) fn finish_resize(&mut self, cx: &mut Context<Self>) {
        let Some(drag) = self.timeline_drag.take() else {
            return;
        };
        if let Some(command) = drag.command {
            let is_anchor = drag.anchor_index.is_some();
            self.edit_status = match self.editor.execute(command) {
                Ok(_) => {
                    self.dirty = true;
                    self.autosave_pending = true;
                    if is_anchor {
                        "Đã căn anchor • chưa lưu".into()
                    } else {
                        "Đã kéo mốc • chưa lưu".into()
                    }
                }
                Err(error) => error.to_string(),
            };
        }
        cx.notify();
    }

    pub(super) fn timeline_view(&self, cx: &mut Context<Self>) -> Div {
        let project = self.editor.project();
        let anchors = self
            .timeline_drag
            .as_ref()
            .map(|drag| drag.anchors.as_slice())
            .unwrap_or(&project.anchors);
        let words = self
            .timeline_drag
            .as_ref()
            .map(|s| s.words.as_slice())
            .or_else(|| self.tapping.as_ref().map(|s| s.words.as_slice()))
            .unwrap_or(&project.words);
        let map = |t| vietkara_domain::timing::map_time(t, anchors, false);
        let duration = project_timeline_duration(project, self.play_duration, anchors);
        let width = (duration as f32 * self.timeline_zoom).max(800.);
        let zoom = self.timeline_zoom;
        let mut content = div().relative().w(px(width)).h(px(178.)).bg(rgb(0x151820));
        let step = (duration / 100.).ceil().max(1.);
        for tick in 0..=(duration / step) as usize {
            let time = tick as f64 * step;
            content = content.child(
                div()
                    .absolute()
                    .left(px(time as f32 * zoom))
                    .top(px(0.))
                    .w(px(
                        ((step as f32 * zoom).min(width - time as f32 * zoom)).max(1.)
                    ))
                    .h(px(18.))
                    .text_size(px(10.))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(0x343b49)))
                    .child(format!("{time:.0}s"))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| this.seek_timeline(time, cx)),
                    ),
            );
        }
        for (slot, metadata, label, top, color) in [
            (
                AudioSlot::Reference,
                &project.reference,
                "Bản hát",
                20.,
                0x397a9c,
            ),
            (AudioSlot::Beat, &project.beat, "Beat", 68., 0x4d9a72),
        ] {
            content = content.child(div().absolute().left(px(0.)).top(px(top)).child(label));
            if let Some(metadata) = metadata {
                let length = metadata
                    .get("duration")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(duration);
                if let Some(peaks) = metadata.get("peaks").and_then(|v| v.as_array()) {
                    let stride = (peaks.len() / 1600).max(1);
                    for index in (0..peaks.len()).step_by(stride) {
                        let peak = peaks[index].as_f64().unwrap_or(0.).clamp(0., 1.) as f32;
                        let Some((start, end)) = waveform_timeline_interval(
                            slot,
                            index,
                            index + stride,
                            peaks.len(),
                            length,
                            anchors,
                        ) else {
                            continue;
                        };
                        content = content.child(
                            div()
                                .absolute()
                                .left(px(start as f32 * zoom))
                                .top(px(top + 22. - peak * 15.))
                                .w(px(((end - start) as f32 * zoom).max(1.)))
                                .h(px((peak * 30.).max(1.)))
                                .bg(rgb(color)),
                        );
                    }
                }
            } else {
                content = content.child(
                    div()
                        .absolute()
                        .left(px(90.))
                        .top(px(top))
                        .child("Chưa nạp media"),
                );
            }
        }
        for (index, anchor) in anchors.iter().enumerate() {
            content = content.child(
                div()
                    .absolute()
                    .left(px(anchor.beat as f32 * zoom - 3.))
                    .top(px(18.))
                    .w(px(6.))
                    .h(px(84.))
                    .rounded_full()
                    .bg(rgb(0xffbe55))
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                            this.start_anchor_move(index, f32::from(event.position.x), cx)
                        }),
                    ),
            );
        }
        for (index, word) in words.iter().enumerate() {
            let id = word.id.clone();
            let start = map(word.start).max(0.);
            let end = map(word.end).max(start);
            let selected = Some(&word.id) == self.selected_word.as_ref();
            content = content.child(
                div()
                    .absolute()
                    .left(px(start as f32 * zoom))
                    .top(px(116. + (word.line % 2) as f32 * 28.))
                    .w(px(((end - start) as f32 * zoom).max(3.)))
                    .h(px(24.))
                    .overflow_hidden()
                    .bg(rgb(if selected { 0x536fa0 } else { 0x314354 }))
                    .text_size(px(11.))
                    .cursor_pointer()
                    .child(word.text.clone())
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| this.select_word(id.clone(), cx)),
                    )
                    .children(
                        [
                            (vietkara_domain::timeline::Edge::Start, true),
                            (vietkara_domain::timeline::Edge::End, false),
                        ]
                        .into_iter()
                        .map(|(edge, start)| {
                            div()
                                .absolute()
                                .top(px(0.))
                                .h_full()
                                .w(px(5.))
                                .bg(rgb(0x91a7bc))
                                .when(start, |handle| handle.left(px(0.)))
                                .when(!start, |handle| handle.right(px(0.)))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(
                                        move |this, event: &gpui::MouseDownEvent, _, cx| {
                                            this.start_resize(
                                                index,
                                                edge,
                                                f32::from(event.position.x),
                                                cx,
                                            )
                                        },
                                    ),
                                )
                        }),
                    ),
            );
        }
        content = content.child(
            div()
                .absolute()
                .left(px(self.play_position as f32 * zoom))
                .top(px(18.))
                .w(px(1.))
                .h(px(158.))
                .bg(rgb(0xffdd66)),
        );
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(div().cursor_pointer().child("Zoom −").on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            if this.timeline_drag.is_none() {
                                this.timeline_zoom = (this.timeline_zoom / 1.25).max(10.);
                            }
                            cx.notify();
                        }),
                    ))
                    .child(format!("{zoom:.0} px/s"))
                    .child(format!("{} anchor", anchors.len()))
                    .child(
                        div()
                            .cursor_pointer()
                            .rounded_sm()
                            .bg(rgb(theme::ACCENT_SURFACE))
                            .px_2()
                            .child("+ Anchor tại playhead")
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| this.add_anchor_at_playhead(cx)),
                            ),
                    )
                    .child(div().cursor_pointer().child("Zoom +").on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            if this.timeline_drag.is_none() {
                                this.timeline_zoom = (this.timeline_zoom * 1.25).min(500.);
                            }
                            cx.notify();
                        }),
                    )),
            )
            .child(
                div()
                    .id("timeline-scroll")
                    .overflow_x_scroll()
                    .track_scroll(&self.timeline_scroll)
                    .h(px(190.))
                    .child(content),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::super::AudioSlot;
    use super::{
        clamp_seek_position, insert_anchor_at_beat, timeline_to_track_time, track_to_timeline_time,
        waveform_timeline_interval,
    };
    use vietkara_domain::Anchor;

    #[test]
    fn timeline_seek_is_clamped_and_rejects_non_finite_values() {
        assert_eq!(clamp_seek_position(-3.0, 20.0), 0.0);
        assert_eq!(clamp_seek_position(8.5, 20.0), 8.5);
        assert_eq!(clamp_seek_position(25.0, 20.0), 20.0);
        assert_eq!(clamp_seek_position(8.5, -1.0), 0.0);
        assert_eq!(clamp_seek_position(f64::NAN, 20.0), 0.0);
        assert_eq!(clamp_seek_position(8.5, f64::INFINITY), 0.0);
    }

    #[test]
    fn reference_track_clock_maps_to_and_from_the_beat_timeline() {
        let anchors = vec![
            Anchor {
                r#ref: 10.0,
                beat: 12.0,
            },
            Anchor {
                r#ref: 20.0,
                beat: 32.0,
            },
        ];
        let timeline = track_to_timeline_time(AudioSlot::Reference, 15.0, &anchors);
        assert!((timeline - 22.0).abs() < 0.001);
        let source = timeline_to_track_time(AudioSlot::Reference, timeline, &anchors);
        assert!((source - 15.0).abs() < 0.001);
        assert_eq!(
            track_to_timeline_time(AudioSlot::Beat, 15.0, &anchors),
            15.0
        );
        assert_eq!(
            timeline_to_track_time(AudioSlot::Beat, 15.0, &anchors),
            15.0
        );
    }

    #[test]
    fn reference_waveform_bins_align_to_beat_time_and_clip_before_zero() {
        let anchors = vec![
            Anchor {
                r#ref: 10.0,
                beat: 12.0,
            },
            Anchor {
                r#ref: 20.0,
                beat: 32.0,
            },
        ];
        assert_eq!(
            waveform_timeline_interval(AudioSlot::Reference, 2, 3, 4, 20.0, &anchors),
            Some((12.0, 22.0))
        );
        assert_eq!(
            waveform_timeline_interval(AudioSlot::Reference, 0, 1, 4, 20.0, &anchors),
            Some((0.0, 2.0))
        );
        assert_eq!(
            waveform_timeline_interval(AudioSlot::Beat, 1, 2, 4, 20.0, &anchors),
            Some((5.0, 10.0))
        );
        assert_eq!(
            waveform_timeline_interval(AudioSlot::Beat, 2, 2, 4, 20.0, &anchors),
            None
        );
    }

    #[test]
    fn anchor_insert_preserves_alignment_and_rejects_near_duplicates() {
        let anchors = vec![
            Anchor {
                r#ref: 10.0,
                beat: 12.0,
            },
            Anchor {
                r#ref: 20.0,
                beat: 32.0,
            },
        ];
        let inserted = insert_anchor_at_beat(22.0, &anchors).unwrap();
        assert_eq!(inserted.len(), 3);
        assert_eq!(inserted[1].r#ref, 15.0);
        assert_eq!(inserted[1].beat, 22.0);
        assert!(insert_anchor_at_beat(22.01, &inserted).is_err());
    }
}
