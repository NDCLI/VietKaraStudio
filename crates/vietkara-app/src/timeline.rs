use super::*;
use gpui::Div;

fn clamp_seek_position(position: f64, duration: f64) -> f64 {
    if !position.is_finite() || !duration.is_finite() {
        return 0.0;
    }
    position.clamp(0.0, duration.max(0.0))
}

fn visible_timeline_interval(
    scroll_offset: f32,
    viewport_width: f32,
    zoom: f32,
    duration: f64,
) -> (f64, f64) {
    if !scroll_offset.is_finite()
        || !viewport_width.is_finite()
        || viewport_width <= 0.0
        || !zoom.is_finite()
        || zoom <= 0.0
        || !duration.is_finite()
        || duration <= 0.0
    {
        return (0.0, duration.max(0.0));
    }

    // GPUI scroll offsets are zero at the start and negative as content moves
    // left. Render one viewport of overscan on either side to avoid edge pop-in.
    let viewport_seconds = f64::from(viewport_width / zoom);
    let left = f64::from((-scroll_offset).max(0.0) / zoom);
    let overscan = viewport_seconds.max(1.0);
    (
        (left - overscan).max(0.0),
        (left + viewport_seconds + overscan).min(duration),
    )
}

#[derive(Default)]
pub(super) struct TimelineWordIndex {
    words_ptr: usize,
    words_len: usize,
    anchors_ptr: usize,
    anchors_len: usize,
    entries: Vec<(f64, f64, usize)>,
    prefix_max_end: Vec<f64>,
}

impl TimelineWordIndex {
    fn rebuild(&mut self, words: &[Word], anchors: &[vietkara_domain::Anchor]) {
        self.words_ptr = words.as_ptr() as usize;
        self.words_len = words.len();
        self.anchors_ptr = anchors.as_ptr() as usize;
        self.anchors_len = anchors.len();
        self.entries = words
            .iter()
            .enumerate()
            .map(|(index, word)| {
                (
                    vietkara_domain::timing::map_time(word.start, anchors, false),
                    vietkara_domain::timing::map_time(word.end, anchors, false),
                    index,
                )
            })
            .collect();
        self.entries.sort_by(|left, right| {
            left.0
                .total_cmp(&right.0)
                .then_with(|| left.2.cmp(&right.2))
        });
        self.prefix_max_end.clear();
        let mut max_end = f64::NEG_INFINITY;
        for entry in &self.entries {
            max_end = max_end.max(entry.1);
            self.prefix_max_end.push(max_end);
        }
    }

    fn visible_indices(
        &mut self,
        words: &[Word],
        anchors: &[vietkara_domain::Anchor],
        start: f64,
        end: f64,
    ) -> Vec<usize> {
        if self.words_ptr != words.as_ptr() as usize
            || self.words_len != words.len()
            || self.anchors_ptr != anchors.as_ptr() as usize
            || self.anchors_len != anchors.len()
        {
            self.rebuild(words, anchors);
        }

        let first = self
            .prefix_max_end
            .partition_point(|prefix_end| *prefix_end < start);
        let after_last = self.entries.partition_point(|entry| entry.0 <= end);
        let mut visible = self.entries[first..after_last]
            .iter()
            .filter(|entry| entry.1 >= start)
            .map(|entry| entry.2)
            .collect::<Vec<_>>();
        // Preserve the project order for overlapping blocks and hit testing.
        visible.sort_unstable();
        visible
    }
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

fn anchors_valid(anchors: &[vietkara_domain::Anchor]) -> bool {
    anchors.len() <= vietkara_domain::MAX_ANCHORS
        && anchors.iter().all(|anchor| {
            anchor.r#ref.is_finite()
                && anchor.beat.is_finite()
                && anchor.r#ref >= 0.0
                && anchor.beat >= 0.0
        })
        && anchors.windows(2).all(|pair| {
            pair[1].r#ref - pair[0].r#ref >= 0.02 && pair[1].beat - pair[0].beat >= 0.02
        })
}

fn remove_anchor_at(
    anchors: &[vietkara_domain::Anchor],
    index: usize,
) -> Option<Vec<vietkara_domain::Anchor>> {
    if index >= anchors.len() {
        return None;
    }
    let mut candidate = anchors.to_vec();
    candidate.remove(index);
    Some(candidate)
}

fn sample_waveform(peaks: &[f64], duration: f64, time: f64) -> Option<f64> {
    if peaks.len() < 2 || !duration.is_finite() || duration <= 0.0 || time < 0.0 || time > duration
    {
        return None;
    }
    let position = time / duration * (peaks.len() - 1) as f64;
    let left = position.floor() as usize;
    let right = (left + 1).min(peaks.len() - 1);
    let amount = position - left as f64;
    Some(peaks[left] * (1.0 - amount) + peaks[right] * amount)
}

fn pearson_correlation(left: &[f64], right: &[f64]) -> Option<f64> {
    if left.len() != right.len() || left.len() < 20 {
        return None;
    }
    let left_mean = left.iter().sum::<f64>() / left.len() as f64;
    let right_mean = right.iter().sum::<f64>() / right.len() as f64;
    let mut covariance = 0.0;
    let mut left_variance = 0.0;
    let mut right_variance = 0.0;
    for (left, right) in left.iter().zip(right) {
        let left = left - left_mean;
        let right = right - right_mean;
        covariance += left * right;
        left_variance += left * left;
        right_variance += right * right;
    }
    let denominator = (left_variance * right_variance).sqrt();
    (denominator > 1e-8).then_some((covariance / denominator).clamp(-1.0, 1.0))
}

fn suggest_anchor_alignment(
    anchor: &vietkara_domain::Anchor,
    anchors: &[vietkara_domain::Anchor],
    reference_peaks: &[f64],
    reference_duration: f64,
    beat_peaks: &[f64],
    beat_duration: f64,
) -> Option<(f64, f64)> {
    if reference_peaks.len() < 20
        || beat_peaks.len() < 20
        || !reference_duration.is_finite()
        || !beat_duration.is_finite()
        || reference_duration <= 0.0
        || beat_duration <= 0.0
    {
        return None;
    }
    let step = (reference_duration / reference_peaks.len() as f64)
        .min(beat_duration / beat_peaks.len() as f64)
        .clamp(0.005, 0.025);
    let half_window = 0.35;
    let window_steps = (half_window / step).floor() as i32;
    let shift_steps = (0.5 / step).floor() as i32;
    let mut best: Option<(f64, f64)> = None;
    let mut current_score = None;

    for shift in -shift_steps..=shift_steps {
        let offset = shift as f64 * step;
        let mut reference_window = Vec::with_capacity((window_steps * 2 + 1) as usize);
        let mut beat_window = Vec::with_capacity((window_steps * 2 + 1) as usize);
        for sample in -window_steps..=window_steps {
            let reference_time = anchor.r#ref + sample as f64 * step;
            let beat_time =
                vietkara_domain::timing::map_time(reference_time, anchors, false) + offset;
            if let (Some(reference_value), Some(beat_value)) = (
                sample_waveform(reference_peaks, reference_duration, reference_time),
                sample_waveform(beat_peaks, beat_duration, beat_time),
            ) {
                reference_window.push(reference_value);
                beat_window.push(beat_value);
            }
        }
        let Some(score) = pearson_correlation(&reference_window, &beat_window) else {
            continue;
        };
        if shift == 0 {
            current_score = Some(score);
        }
        if best.is_none_or(|(best_offset, best_score)| {
            score > best_score + 1e-6
                || ((score - best_score).abs() <= 1e-6 && offset.abs() < best_offset.abs())
        }) {
            best = Some((offset, score));
        }
    }

    let (offset, score) = best?;
    let current_score = current_score?;
    (offset.abs() >= step * 1.5 && score >= 0.45 && score - current_score >= 0.08)
        .then_some((anchor.beat + offset, score))
}

fn waveform_from_metadata(metadata: Option<&Value>) -> Option<(Vec<f64>, f64)> {
    let metadata = metadata?;
    let duration = metadata.get("duration")?.as_f64()?;
    let peaks = metadata
        .get("peaks")?
        .as_array()?
        .iter()
        .map(Value::as_f64)
        .collect::<Option<Vec<_>>>()?;
    (duration.is_finite() && duration > 0.0 && peaks.iter().all(|peak| peak.is_finite()))
        .then_some((peaks, duration))
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
        let selected_anchor = candidate
            .iter()
            .position(|anchor| (anchor.beat - self.play_position).abs() < 0.0001);
        self.edit_status = match self.editor.execute(Command::SetAnchors(candidate)) {
            Ok(_) => {
                self.selected_anchor = selected_anchor;
                self.dirty = true;
                self.autosave_pending = true;
                "Đã thêm anchor tại playhead • chưa lưu".into()
            }
            Err(error) => error.to_string(),
        };
        cx.notify();
    }

    pub(super) fn remove_selected_anchor(&mut self, cx: &mut Context<Self>) {
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
        let Some(index) = self.selected_anchor else {
            self.edit_status = "Chọn một anchor trên timeline để xóa".into();
            cx.notify();
            return;
        };
        let Some(candidate) = remove_anchor_at(&self.editor.project().anchors, index) else {
            self.selected_anchor = None;
            self.edit_status = "Anchor đã thay đổi, hãy chọn lại".into();
            cx.notify();
            return;
        };
        self.edit_status = match self.editor.execute(Command::SetAnchors(candidate)) {
            Ok(_) => {
                self.selected_anchor = None;
                self.dirty = true;
                self.autosave_pending = true;
                "Đã xóa anchor • chưa lưu".into()
            }
            Err(error) => error.to_string(),
        };
        cx.notify();
    }

    pub(super) fn auto_align_selected_anchor(&mut self, cx: &mut Context<Self>) {
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
        let Some(index) = self.selected_anchor else {
            self.edit_status = "Chọn anchor cần tự căn trước".into();
            cx.notify();
            return;
        };
        let project = self.editor.project();
        let Some(anchor) = project.anchors.get(index).cloned() else {
            self.selected_anchor = None;
            self.edit_status = "Anchor đã thay đổi, hãy chọn lại".into();
            cx.notify();
            return;
        };
        let Some((reference_peaks, reference_duration)) =
            waveform_from_metadata(project.reference.as_ref())
        else {
            self.edit_status = "Chưa có waveform hợp lệ của bản hát".into();
            cx.notify();
            return;
        };
        let Some((beat_peaks, beat_duration)) = waveform_from_metadata(project.beat.as_ref())
        else {
            self.edit_status = "Chưa có waveform hợp lệ của beat".into();
            cx.notify();
            return;
        };
        let anchors = project.anchors.clone();
        let Some((beat, confidence)) = suggest_anchor_alignment(
            &anchor,
            &anchors,
            &reference_peaks,
            reference_duration,
            &beat_peaks,
            beat_duration,
        ) else {
            self.edit_status = "Waveform quanh anchor chưa đủ tương quan để tự căn an toàn".into();
            cx.notify();
            return;
        };
        let offset = beat - anchor.beat;
        let mut candidate = anchors;
        candidate[index].beat = beat.max(0.0);
        if !anchors_valid(&candidate) {
            self.edit_status = "Gợi ý waveform làm đảo thứ tự anchors; chưa áp dụng".into();
            cx.notify();
            return;
        }
        self.edit_status = match self.editor.execute(Command::SetAnchors(candidate)) {
            Ok(_) => {
                self.dirty = true;
                self.autosave_pending = true;
                format!("Đã tự căn anchor (r={confidence:.2}, Δbeat={offset:+.3}s) • chưa lưu")
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
        self.selected_anchor = Some(index);
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
                if anchors_valid(&anchors) {
                    drag.anchors = anchors.clone();
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

    pub(super) fn timeline_view(&self, window: &Window, cx: &mut Context<Self>) -> Div {
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
        let selected_anchor_label = self
            .selected_anchor
            .and_then(|index| anchors.get(index).map(|anchor| (index, anchor)))
            .map(|(index, anchor)| {
                format!(
                    "Anchor {}  ·  Ref {:.2}s  ↔  Beat {:.2}s",
                    index + 1,
                    anchor.r#ref,
                    anchor.beat
                )
            })
            .unwrap_or_else(|| "Chọn vạch anchor để xem và chỉnh mốc sync".into());
        let width = (duration as f32 * self.timeline_zoom).max(800.);
        let zoom = self.timeline_zoom;
        let scroll_offset = f32::from(self.timeline_scroll.offset().x);
        let scroll_bounds_width = f32::from(self.timeline_scroll.bounds().size.width);
        let viewport_width = if scroll_bounds_width > 0.0 {
            scroll_bounds_width
        } else {
            f32::from(window.viewport_size().width)
        };
        let (visible_start, visible_end) =
            visible_timeline_interval(scroll_offset, viewport_width, zoom, duration);
        let visible_word_indices = if self.timeline_drag.is_none() && self.tapping.is_none() {
            self.timeline_word_index.borrow_mut().visible_indices(
                &project.words,
                anchors,
                visible_start,
                visible_end,
            )
        } else {
            words
                .iter()
                .enumerate()
                .filter_map(|(index, word)| {
                    let start = map(word.start).max(0.0);
                    let end = map(word.end).max(start);
                    (end >= visible_start && start <= visible_end).then_some(index)
                })
                .collect()
        };
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
                        if end < visible_start || start > visible_end {
                            continue;
                        }
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
                    .bg(rgb(if self.selected_anchor == Some(index) {
                        0xffe08a
                    } else {
                        0xffbe55
                    }))
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                            this.start_anchor_move(index, f32::from(event.position.x), cx)
                        }),
                    ),
            );
        }
        for index in visible_word_indices {
            let Some(word) = words.get(index) else {
                continue;
            };
            let start = map(word.start).max(0.);
            let end = map(word.end).max(start);
            let id = word.id.clone();
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
                    .child(
                        div()
                            .cursor_pointer()
                            .rounded_sm()
                            .bg(rgb(if self.selected_anchor.is_some() {
                                0x713b3b
                            } else {
                                theme::CANVAS
                            }))
                            .px_2()
                            .child("Xóa anchor")
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| this.remove_selected_anchor(cx)),
                            ),
                    )
                    .child(
                        div()
                            .cursor_pointer()
                            .rounded_sm()
                            .bg(rgb(theme::ACCENT_SURFACE))
                            .px_2()
                            .child("Tự căn anchor")
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| this.auto_align_selected_anchor(cx)),
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
                    .text_size(px(11.))
                    .text_color(rgb(theme::TEXT_MUTED))
                    .child(selected_anchor_label),
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
        TimelineWordIndex, anchors_valid, clamp_seek_position, insert_anchor_at_beat,
        remove_anchor_at, suggest_anchor_alignment, timeline_to_track_time, track_to_timeline_time,
        visible_timeline_interval, waveform_timeline_interval,
    };
    use vietkara_domain::{Anchor, Word};

    fn word(index: usize, start: f64, end: f64) -> Word {
        Word {
            id: format!("{index:032x}"),
            text: format!("Từ{index}"),
            start,
            end,
            line_uid: format!("{:032x}", index + 100),
            role: "all".into(),
            line: index as u32,
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
    fn timeline_seek_is_clamped_and_rejects_non_finite_values() {
        assert_eq!(clamp_seek_position(-3.0, 20.0), 0.0);
        assert_eq!(clamp_seek_position(8.5, 20.0), 8.5);
        assert_eq!(clamp_seek_position(25.0, 20.0), 20.0);
        assert_eq!(clamp_seek_position(8.5, -1.0), 0.0);
        assert_eq!(clamp_seek_position(f64::NAN, 20.0), 0.0);
        assert_eq!(clamp_seek_position(8.5, f64::INFINITY), 0.0);
    }

    #[test]
    fn timeline_culling_uses_negative_scroll_and_one_viewport_overscan() {
        assert_eq!(
            visible_timeline_interval(-1_000.0, 500.0, 100.0, 60.0),
            (5.0, 20.0)
        );
        assert_eq!(
            visible_timeline_interval(0.0, 800.0, 100.0, 10.0),
            (0.0, 10.0)
        );
        assert_eq!(
            visible_timeline_interval(0.0, 0.0, 100.0, 20.0),
            (0.0, 20.0)
        );
    }

    #[test]
    fn timeline_word_index_keeps_long_overlaps_and_source_order() {
        let words = vec![
            word(0, 0.0, 15.0),
            word(1, 8.0, 9.0),
            word(2, 10.0, 11.0),
            word(3, 4.0, 30.0),
            word(4, 12.1, 13.0),
        ];
        let mut index = TimelineWordIndex::default();

        assert_eq!(index.visible_indices(&words, &[], 10.0, 12.0), [0, 2, 3]);
    }

    #[test]
    fn timeline_word_index_rebuilds_when_anchor_mapping_changes() {
        let words = vec![word(0, 0.0, 1.0), word(1, 2.0, 3.0)];
        let mut index = TimelineWordIndex::default();
        assert!(index.visible_indices(&words, &[], 10.0, 11.0).is_empty());

        let anchors = vec![Anchor {
            r#ref: 0.0,
            beat: 10.0,
        }];
        assert_eq!(index.visible_indices(&words, &anchors, 10.0, 11.0), [0]);
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
        assert!(anchors_valid(&inserted));
        let mut crossed = inserted;
        crossed[1].beat = crossed[2].beat + 1.0;
        assert!(!anchors_valid(&crossed));
    }

    #[test]
    fn removing_anchor_preserves_order_and_checks_selection_bounds() {
        let anchors = vec![
            Anchor {
                r#ref: 10.0,
                beat: 12.0,
            },
            Anchor {
                r#ref: 20.0,
                beat: 22.0,
            },
        ];
        let remaining = remove_anchor_at(&anchors, 0).unwrap();
        assert_eq!(remaining, vec![anchors[1].clone()]);
        assert!(remove_anchor_at(&anchors, 2).is_none());
    }

    #[test]
    fn waveform_alignment_suggests_shift_only_for_correlated_envelopes() {
        let mut reference = vec![0.02_f64; 2000];
        let mut beat = vec![0.02_f64; 2000];
        for event in [9.68, 9.82, 9.96, 10.12, 10.29, 10.34] {
            for (index, value) in reference.iter_mut().enumerate() {
                let time = index as f64 * 20.0 / 1999.0;
                let distance = (time - event) / 0.025;
                *value = (*value).max(0.02 + (-distance * distance / 2.0).exp());
            }
            for (index, value) in beat.iter_mut().enumerate() {
                let time = index as f64 * 20.0 / 1999.0;
                let distance = (time - event - 0.15) / 0.025;
                *value = (*value).max(0.02 + (-distance * distance / 2.0).exp());
            }
        }
        let anchor = Anchor {
            r#ref: 10.0,
            beat: 10.0,
        };
        let suggestion = suggest_anchor_alignment(
            &anchor,
            std::slice::from_ref(&anchor),
            &reference,
            20.0,
            &beat,
            20.0,
        )
        .unwrap();
        assert!((suggestion.0 - 10.15).abs() < 0.03);
        assert!(suggestion.1 >= 0.45);

        let flat = vec![0.5_f64; 2000];
        assert!(
            suggest_anchor_alignment(
                &anchor,
                std::slice::from_ref(&anchor),
                &flat,
                20.0,
                &flat,
                20.0,
            )
            .is_none()
        );
    }
}
