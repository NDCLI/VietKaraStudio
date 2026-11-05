use super::*;
use gpui::Div;
pub(super) struct TimelineDrag {
    index: usize,
    edge: vietkara_domain::timeline::Edge,
    origin: f32,
    scroll: f32,
    command: Option<Command>,
    pub(super) words: Vec<Word>,
}
impl Studio {
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
            || self.pending_srt.is_some()
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
        self.selected_word = Some(word.id.clone());
        self.timeline_drag = Some(TimelineDrag {
            index,
            edge,
            origin: x,
            scroll: f32::from(self.timeline_scroll.offset().x),
            command: None,
            words: self.editor.project().words.clone(),
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
            self.edit_status = match self.editor.execute(command) {
                Ok(_) => {
                    self.dirty = true;
                    self.autosave_pending = true;
                    "Đã kéo mốc • chưa lưu".into()
                }
                Err(error) => error.to_string(),
            };
        }
        cx.notify();
    }

    pub(super) fn timeline_view(&self, cx: &mut Context<Self>) -> Div {
        let project = self.editor.project();
        let words = self
            .timeline_drag
            .as_ref()
            .map(|s| s.words.as_slice())
            .or_else(|| self.tapping.as_ref().map(|s| s.words.as_slice()))
            .unwrap_or(&project.words);
        let map = |t| vietkara_domain::timing::map_time(t, &project.anchors, false);
        let duration = words
            .iter()
            .map(|w| map(w.end))
            .fold(self.play_duration.max(1.), f64::max);
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
                    .text_size(px(10.))
                    .child(format!("{time:.0}s")),
            );
        }
        for (metadata, label, top, color) in [
            (&project.reference, "Bản hát", 20., 0x397a9c),
            (&project.beat, "Beat", 68., 0x4d9a72),
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
                        let x = index as f64 / peaks.len().max(1) as f64 * length;
                        content = content.child(
                            div()
                                .absolute()
                                .left(px(x as f32 * zoom))
                                .top(px(top + 22. - peak * 15.))
                                .w(px((length as f32 * zoom * stride as f32
                                    / peaks.len().max(1) as f32)
                                    .max(1.)))
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
