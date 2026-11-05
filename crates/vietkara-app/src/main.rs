mod timeline;
use gpui::{
    App, Application, Bounds, Context, FocusHandle, KeyBinding, KeyDownEvent, KeyUpEvent,
    MouseButton, PathPromptOptions, Render, ScrollHandle, Task, Timer, Window, WindowBounds,
    WindowOptions, actions, div, prelude::*, px, rgb, size,
};
use vietkara_domain::{
    Project, Word,
    edit::{Command, Editor},
};

use std::path::PathBuf;
use vietkara_storage::audio_import::AudioSlot;
use vietkara_storage::autosave::{RecoveryCandidate, recovery_candidate};

actions!(
    studio,
    [
        Undo,
        Redo,
        Save,
        SaveAs,
        Open,
        ImportSrt,
        TogglePlayback,
        StopPlayback,
        SeekBack,
        SeekForward,
        BeginTapping,
        ImportBeat,
        ImportReference
    ]
);

#[cfg(windows)]
use vietkara_media::playback::{Playback, PlaybackCommand};

struct TapSession {
    clock: vietkara_domain::tapping::TapTiming,
    words: Vec<Word>,
    wall: std::time::Instant,
}
struct Studio {
    editor: Editor,
    focus_handle: FocusHandle,
    edit_status: String,
    playing: bool,
    selected_word: Option<String>,
    timeline_zoom: f32,
    timeline_drag: Option<timeline::TimelineDrag>,
    timeline_scroll: ScrollHandle,
    tapping: Option<TapSession>,
    play_position: f64,
    play_duration: f64,
    playback_error: Option<String>,
    #[cfg(windows)]
    playback: Option<Playback>,
    _playback_task: Task<()>,
    path: Option<PathBuf>,
    recovery: Option<RecoveryCandidate>,
    saving: bool,
    dirty: bool,
    confirm_open: bool,
    confirm_close: bool,
    pending_srt: Option<Command>,
    pending_audio: Option<(AudioSlot, PathBuf)>,
    autosave_status: String,
    autosaving: bool,
    autosave_pending: bool,
    _autosave_task: Task<()>,
}

impl Studio {
    fn choose_audio(&mut self, slot: AudioSlot, cx: &mut Context<Self>) {
        if self.saving
            || self.autosaving
            || self.recovery.is_some()
            || self.tapping.is_some()
            || self.timeline_drag.is_some()
            || self.pending_audio.is_some()
            || self.pending_srt.is_some()
        {
            return;
        }
        if self.path.is_none() {
            self.edit_status = "Hãy lưu project trước khi nhập media".into();
            cx.notify();
            return;
        }
        let selection = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Chọn audio hoặc video nguồn".into()),
        });
        self.saving = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let selected = selection.await;
            let _ = this.update(cx, |this, cx| {
                this.saving = false;
                match selected {
                    Ok(Ok(Some(paths))) => {
                        if let Some(path) = paths.into_iter().next() {
                            this.pending_audio = Some((slot, path));
                        }
                    }
                    Ok(Ok(None)) => this.edit_status = "Đã hủy nhập audio".into(),
                    Ok(Err(error)) => this.edit_status = error.to_string(),
                    Err(error) => this.edit_status = error.to_string(),
                }
                cx.notify();
            });
        })
        .detach();
    }
    fn accept_audio(&mut self, cx: &mut Context<Self>) {
        if self.saving || self.autosaving || self.tapping.is_some() || self.timeline_drag.is_some()
        {
            return;
        }
        let Some((slot, source)) = self.pending_audio.take() else {
            return;
        };
        let Some(main) = self.path.clone() else {
            return;
        };
        let project = self.editor.project().clone();
        #[cfg(windows)]
        {
            self.playback = None;
        }
        self.playing = false;
        self.saving = true;
        self.edit_status = "Đang nhập audio và tạo phiên bản phục hồi…".into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    vietkara_storage::audio_import::import_audio(&project, &main, &source, slot)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.saving = false;
                match result {
                    Ok(imported) => match Editor::new(imported.project) {
                        Ok(editor) => {
                            this.editor = editor;
                            this.selected_word = None;
                            this.dirty = false;
                            this.autosave_pending = false;
                            this.edit_status = format!(
                                "{} • phiên bản trước: {}",
                                if imported.relinked {
                                    "Đã relink audio, giữ mốc"
                                } else {
                                    "Đã nhập audio"
                                },
                                imported.version_id
                            );
                        }
                        Err(error) => this.edit_status = error.to_string(),
                    },
                    Err(error) => this.edit_status = error,
                }
                this.reset_playback();
                cx.notify();
            });
        })
        .detach();
    }

    fn cancel_tap_hold(&mut self, cx: &mut Context<Self>) {
        if let Some(session) = &mut self.tapping {
            session.clock.cancel();
        }
        #[cfg(windows)]
        if let Some(worker) = &self.playback {
            let _ = worker.send(PlaybackCommand::Pause);
        }
        cx.notify();
    }
    fn previous_tap(&mut self, cx: &mut Context<Self>) {
        let Some(session) = &mut self.tapping else {
            return;
        };
        let Some(tap) = session.clock.back() else {
            return;
        };
        session.words[tap.index] = self.editor.project().words[tap.index].clone();
        self.selected_word = Some(session.words[tap.index].id.clone());
        self.autosave_pending = true;
        #[cfg(windows)]
        if let Some(worker) = &self.playback {
            let _ = worker.send(PlaybackCommand::Seek((tap.start - 1.).max(0.)));
        }
        cx.notify();
    }
    fn begin_tapping(&mut self, cx: &mut Context<Self>) {
        if self.saving
            || self.recovery.is_some()
            || self.pending_srt.is_some()
            || self.pending_audio.is_some()
            || self.tapping.is_some()
            || self.timeline_drag.is_some()
        {
            return;
        }
        if self
            .editor
            .project()
            .words
            .iter()
            .any(|w| w.locked == Some(true))
        {
            self.edit_status = "Mở khóa lời trước khi căn nhịp".into();
            cx.notify();
            return;
        }
        #[cfg(windows)]
        {
            let Some(worker) = &self.playback else {
                return;
            };
            let state = worker.snapshot();
            if !state.loaded {
                return;
            }
            let words = self.editor.project().words.clone();
            let index = words
                .iter()
                .position(|w| Some(&w.id) == self.selected_word.as_ref())
                .unwrap_or(0);
            let Ok(clock) = vietkara_domain::tapping::TapTiming::new(words.len(), index) else {
                return;
            };
            self.tapping = Some(TapSession {
                clock,
                words,
                wall: std::time::Instant::now(),
            });
            let _ = worker.send(PlaybackCommand::Play);
            self.edit_status = "Căn nhịp: Space nhấn/thả, Enter hoàn tất".into();
            cx.notify();
        }
    }
    fn accept_tap(&mut self, tap: Option<vietkara_domain::tapping::Tap>, cx: &mut Context<Self>) {
        let Some(tap) = tap else {
            return;
        };
        let Some(session) = &mut self.tapping else {
            return;
        };
        let mut project = self.editor.project().clone();
        project.words = session.words.clone();
        match vietkara_domain::tapping::tap_command(&project, tap) {
            Ok(Command::ReplaceWord { word, .. }) => {
                self.selected_word = Some(word.id.clone());
                session.words[tap.index] = *word;
                self.autosave_pending = true;
                cx.notify();
            }
            Err(error) => {
                session.clock.back();
                self.edit_status = error;
                cx.notify();
            }
            _ => {}
        }
    }
    fn finish_tapping(&mut self, finalize: bool, cx: &mut Context<Self>) {
        if finalize {
            let tap = self
                .tapping
                .as_mut()
                .and_then(|s| s.clock.finish(self.play_position));
            self.accept_tap(tap, cx);
        }
        let Some(session) = self.tapping.take() else {
            return;
        };
        if !session.clock.history.is_empty() {
            self.edit_status = match self.editor.execute(Command::SetLyrics {
                words: session.words,
                text: self.editor.project().text.clone(),
            }) {
                Ok(_) => {
                    self.dirty = true;
                    self.autosave_pending = true;
                    "Đã căn nhịp • chưa lưu".into()
                }
                Err(error) => error.to_string(),
            };
        }
        cx.notify();
    }
    fn tap_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        if self.tapping.is_none() || event.keystroke.modifiers.control {
            return;
        }
        if event.keystroke.key == "backspace" {
            if !event.is_held {
                self.previous_tap(cx);
            }
            cx.stop_propagation();
            return;
        }
        if event.keystroke.key == "enter" {
            self.finish_tapping(true, cx);
            cx.stop_propagation();
            return;
        }
        if event.keystroke.key != "space" || !self.playing {
            return;
        }
        let time = self.playback_position();
        let tap = self.tapping.as_mut().and_then(|s| {
            s.clock
                .down(time, event.is_held, s.wall.elapsed().as_secs_f64() * 1000.)
        });
        self.accept_tap(tap, cx);
        if self.tapping.as_ref().is_some_and(|s| s.clock.done()) {
            self.finish_tapping(false, cx);
        }
        cx.stop_propagation();
    }
    fn tap_up(&mut self, event: &KeyUpEvent, cx: &mut Context<Self>) {
        if event.keystroke.key != "space" || self.tapping.is_none() {
            return;
        }
        let time = self.playback_position();
        let tap = self
            .tapping
            .as_mut()
            .and_then(|s| s.clock.up(time, s.wall.elapsed().as_secs_f64() * 1000.));
        self.accept_tap(tap, cx);
        if self.tapping.as_ref().is_some_and(|s| s.clock.done()) {
            self.finish_tapping(false, cx);
        }
        cx.stop_propagation();
    }
    fn playback_position(&self) -> f64 {
        #[cfg(windows)]
        if let Some(worker) = &self.playback {
            return worker.snapshot().position;
        }
        self.play_position
    }

    fn select_word(&mut self, id: String, cx: &mut Context<Self>) {
        if self.tapping.is_some() {
            return;
        }
        if let Some(word) = self
            .editor
            .project()
            .words
            .iter()
            .find(|word| word.id == id)
        {
            let beat = vietkara_domain::timing::map_time(
                word.start,
                &self.editor.project().anchors,
                false,
            )
            .max(0.);
            #[cfg(windows)]
            if let Some(worker) = &self.playback
                && let Err(error) = worker.send(PlaybackCommand::Seek(beat))
            {
                self.playback_error = Some(error);
            }
            #[cfg(not(windows))]
            let _ = beat;
            self.selected_word = Some(id);
            cx.notify();
        }
    }
    fn toggle_linked(&mut self, cx: &mut Context<Self>) {
        if self.saving
            || self.tapping.is_some()
            || self.timeline_drag.is_some()
            || self.recovery.is_some()
            || self.pending_srt.is_some()
            || self.pending_audio.is_some()
        {
            return;
        }
        let mut settings = self.editor.project().settings.clone();
        settings.timeline_linked = !settings.timeline_linked;
        match self.editor.execute(Command::SetSettings(settings)) {
            Ok(_) => {
                self.dirty = true;
                self.autosave_pending = true;
            }
            Err(error) => self.edit_status = error.to_string(),
        }
        cx.notify();
    }
    fn adjust_word(&mut self, start: bool, delta: f64, cx: &mut Context<Self>) {
        if self.saving
            || self.recovery.is_some()
            || self.pending_srt.is_some()
            || self.pending_audio.is_some()
            || self.tapping.is_some()
            || self.timeline_drag.is_some()
        {
            return;
        }
        let Some(index) = self
            .editor
            .project()
            .words
            .iter()
            .position(|word| Some(&word.id) == self.selected_word.as_ref())
        else {
            return;
        };
        let edge = if start {
            vietkara_domain::timeline::Edge::Start
        } else {
            vietkara_domain::timeline::Edge::End
        };
        let duration = if self.play_duration > 0. {
            self.play_duration
        } else {
            self.editor
                .project()
                .words
                .iter()
                .map(|w| {
                    vietkara_domain::timing::map_time(w.end, &self.editor.project().anchors, false)
                })
                .fold(0., f64::max)
        };
        let result =
            vietkara_domain::timeline::resize(self.editor.project(), index, edge, delta, duration)
                .and_then(|command| self.editor.execute(command).map_err(|e| e.to_string()));
        self.edit_status = match result {
            Ok(_) => {
                self.dirty = true;
                self.autosave_pending = true;
                "Đã chỉnh mốc • chưa lưu".into()
            }
            Err(error) => error,
        };
        cx.notify();
    }

    fn reset_playback(&mut self) {
        self.playing = false;
        self.play_position = 0.;
        self.play_duration = 0.;
        self.playback_error = None;
        #[cfg(windows)]
        {
            self.playback = None;
            let Some(directory) = self.path.as_ref().and_then(|p| p.parent()) else {
                return;
            };
            let path = directory.join("beat.wav");
            if !path.is_file() {
                self.playback_error = Some("Chưa có beat.wav; cần nhập hoặc relink beat".into());
                return;
            }
            match Playback::new() {
                Ok(worker) => {
                    if let Err(error) = worker.send(PlaybackCommand::Load(path)) {
                        self.playback_error = Some(error);
                    }
                    self.playback = Some(worker);
                }
                Err(error) => self.playback_error = Some(error),
            }
        }
        #[cfg(not(windows))]
        {
            self.playback_error = Some("Backend audio hiện hỗ trợ Windows".into());
        }
    }
    fn transport(&mut self, operation: u8, cx: &mut Context<Self>) {
        if self.tapping.is_some() {
            if operation == 1 {
                self.finish_tapping(false, cx);
            } else if let Some(session) = &mut self.tapping {
                session.clock.cancel();
            }
        }
        #[cfg(windows)]
        if let Some(worker) = &self.playback {
            let state = worker.snapshot();
            let command = match operation {
                0 => {
                    if state.state == vietkara_media::PlayerState::Playing {
                        PlaybackCommand::Pause
                    } else {
                        PlaybackCommand::Play
                    }
                }
                1 => PlaybackCommand::Stop,
                2 => PlaybackCommand::Seek((state.position - 5.).max(0.)),
                _ => PlaybackCommand::Seek((state.position + 5.).min(state.duration)),
            };
            if let Err(error) = worker.send(command) {
                self.playback_error = Some(error);
            }
        }
        #[cfg(not(windows))]
        let _ = operation;
        cx.notify();
    }
    fn poll_playback(&mut self, cx: &mut Context<Self>) {
        #[cfg(windows)]
        if let Some(worker) = &self.playback {
            let state = worker.snapshot();
            let playing = state.state == vietkara_media::PlayerState::Playing;
            let ended = self.playing
                && state.state == vietkara_media::PlayerState::Stopped
                && state.loaded
                && state.position >= state.duration;
            if self.playing != playing
                || (self.play_position - state.position).abs() > 0.005
                || self.play_duration != state.duration
                || self.playback_error != state.error
            {
                self.playing = playing;
                self.play_position = state.position;
                self.play_duration = state.duration;
                self.playback_error = state.error;
                cx.notify();
            }
            if ended && self.tapping.is_some() {
                self.finish_tapping(true, cx);
            }
        }
        #[cfg(not(windows))]
        let _ = cx;
    }

    fn import_srt(&mut self, cx: &mut Context<Self>) {
        if self.saving
            || self.autosaving
            || self.recovery.is_some()
            || self.pending_srt.is_some()
            || self.pending_audio.is_some()
            || self.tapping.is_some()
            || self.timeline_drag.is_some()
        {
            return;
        }
        if self
            .editor
            .project()
            .words
            .iter()
            .any(|w| w.locked == Some(true))
        {
            self.edit_status = "Mở khóa các câu trước khi nhập SRT".into();
            cx.notify();
            return;
        }
        let project = self.editor.project().clone();
        let selection = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Nhập file SRT".into()),
        });
        self.saving = true;
        self.edit_status = "Đang chuẩn bị lời từ SRT…".into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = match selection.await {
                Ok(Ok(Some(paths))) => {
                    if let Some(path) = paths.into_iter().next() {
                        cx.background_executor()
                            .spawn(async move {
                                use std::io::Read;
                                let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
                                let mut source = String::new();
                                file.take(10 * 1024 * 1024 + 1)
                                    .read_to_string(&mut source)
                                    .map_err(|e| e.to_string())?;
                                if source.len() > 10 * 1024 * 1024 {
                                    return Err("SRT vượt quá 10 MiB".to_owned());
                                }
                                vietkara_domain::lyrics::import_srt(&project, &source)
                                    .map(Some)
                                    .map_err(|e| e.to_string())
                            })
                            .await
                    } else {
                        Ok(None)
                    }
                }
                Ok(Ok(None)) => Ok(None),
                Ok(Err(error)) => Err(error.to_string()),
                Err(error) => Err(error.to_string()),
            };
            let _ = this.update(cx, |this, cx| {
                this.saving = false;
                match result {
                    Ok(Some(command)) => {
                        this.pending_srt = Some(command);
                        this.edit_status = "SRT đã sẵn sàng, chờ xác nhận".into();
                    }
                    Ok(None) => this.edit_status = "Đã hủy nhập SRT".into(),
                    Err(error) => this.edit_status = error,
                }
                cx.notify();
            });
        })
        .detach();
    }
    fn accept_srt(&mut self, cx: &mut Context<Self>) {
        let Some(command) = self.pending_srt.take() else {
            return;
        };
        self.edit_status = match self.editor.execute(command) {
            Ok(_) => {
                self.dirty = true;
                self.autosave_pending = true;
                "Đã nhập SRT • chưa lưu".into()
            }
            Err(error) => error.to_string(),
        };
        cx.notify();
    }

    fn open(&mut self, cx: &mut Context<Self>) {
        if self.saving
            || self.autosaving
            || self.recovery.is_some()
            || self.pending_srt.is_some()
            || self.pending_audio.is_some()
            || self.tapping.is_some()
            || self.timeline_drag.is_some()
        {
            return;
        }
        if self.dirty {
            self.confirm_open = true;
            cx.notify();
            return;
        }
        self.choose_open(cx);
    }
    fn choose_open(&mut self, cx: &mut Context<Self>) {
        if self.saving
            || self.autosaving
            || self.recovery.is_some()
            || self.pending_srt.is_some()
            || self.pending_audio.is_some()
            || self.tapping.is_some()
            || self.timeline_drag.is_some()
        {
            return;
        }
        self.confirm_open = false;
        self.saving = true;
        let selection = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Mở project".into()),
        });
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = match selection.await {
                Ok(Ok(Some(paths))) => {
                    if let Some(path) = paths.into_iter().next() {
                        cx.background_executor()
                            .spawn(async move {
                                let project = vietkara_storage::load_project(&path)
                                    .map_err(|e| e.to_string())?;
                                let recovery =
                                    recovery_candidate(&path).map_err(|e| e.to_string())?;
                                let editor = Editor::new(project).map_err(|e| e.to_string())?;
                                Ok(Some((path, editor, recovery)))
                            })
                            .await
                    } else {
                        Ok(None)
                    }
                }
                Ok(Ok(None)) => Ok(None),
                Ok(Err(error)) => Err(error.to_string()),
                Err(error) => Err(error.to_string()),
            };
            let _ = this.update(cx, |this, cx| {
                this.saving = false;
                match result {
                    Ok(Some((path, editor, recovery))) => {
                        this.path = Some(path);
                        this.editor = editor;
                        this.recovery = recovery;
                        this.autosave_pending = false;
                        this.dirty = false;
                        this.edit_status = "Đã mở project".into();
                        this.selected_word = None;
                        this.reset_playback();
                    }
                    Ok(None) => this.edit_status = "Đã hủy mở project".into(),
                    Err(error) => this.edit_status = error,
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn recover(&mut self, accept: bool, cx: &mut Context<Self>) {
        if let Some(candidate) = self.recovery.take() {
            if accept {
                match Editor::new(candidate.accept()) {
                    Ok(editor) => {
                        self.editor = editor;
                        self.dirty = true;
                        self.edit_status = "Đã khôi phục vào bộ nhớ • chưa lưu".into();
                    }
                    Err(error) => self.edit_status = error.to_string(),
                }
            } else {
                self.edit_status = "Đang dùng bản chính; giữ file recovery trên đĩa".into();
            }
            cx.notify();
        }
    }
    fn save(&mut self, cx: &mut Context<Self>) {
        if self.saving
            || self.autosaving
            || self.recovery.is_some()
            || self.pending_srt.is_some()
            || self.pending_audio.is_some()
            || self.tapping.is_some()
            || self.timeline_drag.is_some()
        {
            return;
        }
        if let Some(path) = self.path.clone() {
            self.save_to(path, cx);
        } else {
            self.save_as(cx);
        }
    }
    fn save_as(&mut self, cx: &mut Context<Self>) {
        if self.saving
            || self.autosaving
            || self.recovery.is_some()
            || self.pending_srt.is_some()
            || self.pending_audio.is_some()
            || self.tapping.is_some()
            || self.timeline_drag.is_some()
        {
            return;
        }
        let directory = match self
            .path
            .as_ref()
            .and_then(|p| p.parent())
            .map(PathBuf::from)
            .map(Ok)
            .unwrap_or_else(std::env::current_dir)
        {
            Ok(path) => path,
            Err(error) => {
                self.edit_status = error.to_string();
                cx.notify();
                return;
            }
        };
        let result = cx.prompt_for_new_path(&directory, Some("project.json"));
        self.saving = true;
        self.edit_status = "Chọn nơi lưu project".into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let selection = result.await;
            let _ = this.update(cx, |this, cx| {
                this.saving = false;
                match selection {
                    Ok(Ok(Some(path))) => this.save_to(path, cx),
                    Ok(Ok(None)) => {
                        this.edit_status = "Đã hủy chọn nơi lưu".into();
                        cx.notify();
                    }
                    Ok(Err(error)) => {
                        this.edit_status = error.to_string();
                        cx.notify();
                    }
                    Err(error) => {
                        this.edit_status = error.to_string();
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }
    fn save_to(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if self.saving
            || self.autosaving
            || self.recovery.is_some()
            || self.pending_srt.is_some()
            || self.pending_audio.is_some()
            || self.tapping.is_some()
            || self.timeline_drag.is_some()
        {
            return;
        }
        let source_path = self.path.clone();
        let saved_path = path.clone();
        let project = self.editor.project().clone();
        self.saving = true;
        self.edit_status = "Đang lưu…".into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    vietkara_storage::copy::save_project_copy(
                        &project,
                        source_path.as_deref(),
                        &path,
                    )
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.saving = false;
                this.edit_status = match result {
                    Ok(()) => {
                        this.path = Some(saved_path);
                        this.dirty = false;
                        this.confirm_open = false;
                        this.confirm_close = false;
                        this.autosave_pending = false;
                        "Đã lưu project".into()
                    }
                    Err(error) => error.to_string(),
                };
                cx.notify();
            });
        })
        .detach();
    }

    fn font_size(&mut self, delta: f64, cx: &mut Context<Self>) {
        if self.saving
            || self.recovery.is_some()
            || self.pending_srt.is_some()
            || self.pending_audio.is_some()
            || self.tapping.is_some()
            || self.timeline_drag.is_some()
        {
            return;
        }
        let mut settings = self.editor.project().settings.clone();
        let next = (settings.font_size + delta).clamp(20., 240.);
        if next == settings.font_size {
            return;
        }
        settings.font_size = next;
        self.edit_status = match self.editor.execute(Command::SetSettings(settings)) {
            Ok(_) => {
                self.dirty = true;
                self.autosave_pending = true;
                "Đã đổi cỡ chữ • chưa lưu".into()
            }
            Err(error) => error.to_string(),
        };
        cx.notify();
    }
    fn history(&mut self, redo: bool, cx: &mut Context<Self>) {
        if self.saving
            || self.recovery.is_some()
            || self.pending_srt.is_some()
            || self.pending_audio.is_some()
            || self.tapping.is_some()
            || self.timeline_drag.is_some()
        {
            return;
        }
        let result = if redo {
            self.editor.redo()
        } else {
            self.editor.undo()
        };
        self.edit_status = match result {
            Ok(Some(_)) => {
                self.dirty = true;
                self.autosave_pending = true;
                if redo {
                    "Đã làm lại • chưa lưu"
                } else {
                    "Đã hoàn tác • chưa lưu"
                }
                .into()
            }
            Ok(None) => "Không có thay đổi trong lịch sử".into(),
            Err(error) => error.to_string(),
        };
        cx.notify();
    }
}

impl Render for Studio {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let status = if cfg!(debug_assertions) {
            let viewport = window.viewport_size();
            format!(
                "GPUI spike | {} × {} logical px | scale {:.2}",
                viewport.width,
                viewport.height,
                window.scale_factor()
            )
        } else {
            "Sẵn sàng".to_owned()
        };
        let transport = if self.playing {
            "Ⅱ  Tạm dừng"
        } else {
            "▶  Phát"
        };
        let display_words = self
            .timeline_drag
            .as_ref()
            .map(|s| s.words.as_slice())
            .or_else(|| self.tapping.as_ref().map(|s| s.words.as_slice()))
            .unwrap_or(&self.editor.project().words);
        let selected = display_words
            .iter()
            .find(|word| Some(&word.id) == self.selected_word.as_ref());
        let selected_text = selected
            .map(|word| word.text.clone())
            .unwrap_or_else(|| "Chọn một từ trên timeline".into());
        let selected_start = selected.map(|word| word.start).unwrap_or(0.);
        let selected_end = selected.map(|word| word.end).unwrap_or(0.);
        let reference_position = vietkara_domain::timing::map_time(
            self.play_position,
            &self.editor.project().anchors,
            true,
        );
        let timeline = self.timeline_view(cx);
        let lyric_rows = display_words.iter().map(|line: &Word| {
            let id = line.id.clone();
            let selected = Some(&line.id) == self.selected_word.as_ref();
            let active =
                self.playing && reference_position >= line.start && reference_position < line.end;
            div()
                .cursor_pointer()
                .bg(rgb(if selected {
                    0x34465b
                } else if active {
                    0x255348
                } else {
                    0x20232b
                }))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| this.select_word(id.clone(), cx)),
                )
                .flex()
                .justify_between()
                .px_3()
                .py_2()
                .border_b_1()
                .border_color(rgb(0x292c34))
                .child(format!("{:05.2}  {}", line.start, line.text))
                .child(format!("{:05.2}", line.end))
        });

        div()
            .key_context("Studio")
            .on_mouse_move(cx.listener(|this,event:&gpui::MouseMoveEvent,_,cx|this.move_resize(f32::from(event.position.x),cx)))
            .on_mouse_up(MouseButton::Left,cx.listener(|this,_,_,cx|this.finish_resize(cx)))
            .on_mouse_up_out(MouseButton::Left,cx.listener(|this,_,_,cx|this.finish_resize(cx)))
            .on_key_down(cx.listener(|this,event,_,cx|this.tap_down(event,cx)))
            .on_key_up(cx.listener(|this,event,_,cx|this.tap_up(event,cx)))
            .on_action(cx.listener(|this,_:&BeginTapping,_,cx|this.begin_tapping(cx)))
            .on_action(cx.listener(|this,_:&ImportBeat,_,cx|this.choose_audio(AudioSlot::Beat,cx)))
            .on_action(cx.listener(|this,_:&ImportReference,_,cx|this.choose_audio(AudioSlot::Reference,cx)))
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &Undo, _, cx| this.history(false, cx)))
            .on_action(cx.listener(|this, _: &Redo, _, cx| this.history(true, cx)))
            .on_action(cx.listener(|this, _: &Save, _, cx| this.save(cx)))
            .on_action(cx.listener(|this, _: &SaveAs, _, cx| this.save_as(cx)))
            .on_action(cx.listener(|this, _: &Open, _, cx| this.open(cx)))
            .on_action(cx.listener(|this, _: &ImportSrt, _, cx| this.import_srt(cx)))
            .on_action(cx.listener(|this, _: &TogglePlayback, _, cx| this.transport(0,cx)))
            .on_action(cx.listener(|this, _: &StopPlayback, _, cx| this.transport(1,cx)))
            .on_action(cx.listener(|this, _: &SeekBack, _, cx| this.transport(2,cx)))
            .on_action(cx.listener(|this, _: &SeekForward, _, cx| this.transport(3,cx)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, _| window.focus(&this.focus_handle)),
            )
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x17191f))
            .text_color(rgb(0xe8e9ed))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .px_5()
                    .py_3()
                    .bg(rgb(0x20232b))
                    .child(format!(
                        "VietKara Studio  /  {}{}",
                        self.editor.project().name,
                        if self.dirty { " *" } else { "" }
                    ))
                    .child(div().cursor_pointer().px_3().child("Nhập SRT")
                        .on_mouse_down(MouseButton::Left,cx.listener(|this, _, _, cx|this.import_srt(cx))))
                    .child(div().cursor_pointer().px_3().child("Mở project")
                        .on_mouse_down(MouseButton::Left,cx.listener(|this, _, _, cx|this.open(cx))))
                    .child(
                        div()
                            .cursor_pointer()
                            .px_3()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| this.save(cx)),
                            )
                            .child("Lưu project"),
                    ),
            )
            .child(div().flex().gap_3().px_5().py_2()
                .child(div().cursor_pointer().child("Bắt đầu căn nhịp (Ctrl+T)")
                    .on_mouse_down(MouseButton::Left,cx.listener(|this, _, _, cx|this.begin_tapping(cx))))
                .when(self.tapping.is_some(),|bar|bar
                    .child(div().cursor_pointer().child("Từ trước (Backspace)")
                        .on_mouse_down(MouseButton::Left,cx.listener(|this, _, _, cx|this.previous_tap(cx))))
                    .child(div().cursor_pointer().child("Hoàn tất (Enter)")
                        .on_mouse_down(MouseButton::Left,cx.listener(|this, _, _, cx|this.finish_tapping(true,cx))))
                    .child(format!("Từ {} / {}",self.tapping.as_ref().map(|s|s.clock.index+1).unwrap_or(0),self.editor.project().words.len()))))
            .when(self.pending_audio.is_some(),|root|root.child(div().flex().gap_3().p_3()
                .child("Nhập media sẽ lưu project và tạo phiên bản trước thay đổi. Media khác có thể xóa mốc đồng bộ; thay bản hát có thể xóa lời cũ.")
                .child(div().cursor_pointer().child("Nhập media")
                    .on_mouse_down(MouseButton::Left,cx.listener(|this, _, _, cx|this.accept_audio(cx))))
                .child(div().cursor_pointer().child("Hủy")
                    .on_mouse_down(MouseButton::Left,cx.listener(|this, _, _, cx|{this.pending_audio=None;cx.notify();})))))
            .when(self.pending_srt.is_some(), |root| root.child(div().flex().gap_3().p_3()
                .child("Nhập SRT sẽ thay toàn bộ lời và mốc từ hiện tại.")
                .child(div().cursor_pointer().px_3().child("Thay lời bằng SRT")
                    .on_mouse_down(MouseButton::Left,cx.listener(|this, _, _, cx|this.accept_srt(cx))))
                .child(div().cursor_pointer().px_3().child("Hủy")
                    .on_mouse_down(MouseButton::Left,cx.listener(|this, _, _, cx|{this.pending_srt=None;this.edit_status="Đã hủy nhập SRT".into();cx.notify();})))))
            .when(self.confirm_close, |root| root.child(div().flex().gap_3().p_3()
                .child("Project chưa lưu. Lưu trước khi đóng hoặc chọn đóng không lưu.")
                .child(div().cursor_pointer().px_3().child("Đóng không lưu")
                    .on_mouse_down(MouseButton::Left,cx.listener(|this, _, window, _| {
                        if !this.saving && !this.autosaving { window.remove_window(); }
                    })))
                .child(div().cursor_pointer().px_3().child("Tiếp tục chỉnh sửa")
                    .on_mouse_down(MouseButton::Left,cx.listener(|this, _, _, cx| {this.confirm_close=false;cx.notify();})))))
            .when(self.confirm_open, |root| root.child(div().flex().gap_3().p_3()
                .child("Project chưa lưu. Lưu trước, hoặc bỏ thay đổi để mở project khác.")
                .child(div().cursor_pointer().px_3().child("Bỏ thay đổi và mở")
                    .on_mouse_down(MouseButton::Left,cx.listener(|this, _, _, cx|this.choose_open(cx))))
                .child(div().cursor_pointer().px_3().child("Hủy")
                    .on_mouse_down(MouseButton::Left,cx.listener(|this, _, _, cx|{this.confirm_open=false;cx.notify();})))))
            .when(self.recovery.is_some(), |root| {
                root.child(
                    div()
                        .flex()
                        .gap_3()
                        .p_3()
                        .child("Có bản phục hồi mới hơn. Bạn muốn dùng bản nào?")
                        .child(
                            div()
                                .cursor_pointer()
                                .px_3()
                                .child("Khôi phục")
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, _, _, cx| this.recover(true, cx)),
                                ),
                        )
                        .child(
                            div()
                                .cursor_pointer()
                                .px_3()
                                .child("Dùng bản chính")
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, _, _, cx| this.recover(false, cx)),
                                ),
                        ),
                )
            })
            .child(
                div()
                    .flex()
                    .gap_3()
                    .px_5()
                    .py_2()
                    .child(
                        div()
                            .px_3()
                            .cursor_pointer()
                            .opacity(if self.editor.can_undo() { 1. } else { 0.4 })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| this.history(false, cx)),
                            )
                            .child("Hoàn tác"),
                    )
                    .child(
                        div()
                            .px_3()
                            .cursor_pointer()
                            .opacity(if self.editor.can_redo() { 1. } else { 0.4 })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| this.history(true, cx)),
                            )
                            .child("Làm lại"),
                    )
                    .child(
                        div()
                            .px_3()
                            .cursor_pointer()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| this.font_size(-1., cx)),
                            )
                            .child("Cỡ chữ −"),
                    )
                    .child(format!("{}", self.editor.project().settings.font_size))
                    .child(
                        div()
                            .px_3()
                            .cursor_pointer()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| this.font_size(1., cx)),
                            )
                            .child("Cỡ chữ +"),
                    )
                    .child(self.edit_status.clone())
                    .child(self.autosave_status.clone()),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .gap_3()
                    .p_3()
                    .child(
                        div()
                            .w(px(220.))
                            .bg(rgb(0x20232b))
                            .rounded_md()
                            .p_3()
                            .child("PROJECT")
                            .child(div().mt_3().cursor_pointer().child("Nhập / relink beat")
                                .on_mouse_down(MouseButton::Left,cx.listener(|this, _, _, cx|this.choose_audio(AudioSlot::Beat,cx))))
                            .child(div().mt_3().cursor_pointer().child("Nhập / relink bản hát")
                                .on_mouse_down(MouseButton::Left,cx.listener(|this, _, _, cx|this.choose_audio(AudioSlot::Reference,cx))))
                            .child(div().mt_3().child(if self.editor.project().beat.is_some() {
                                "Beat gốc • đã nạp"
                            } else {
                                "Beat gốc"
                            }))
                            .child(div().mt_2().child(
                                if self.editor.project().reference.is_some() {
                                    "Bản tham chiếu • đã nạp"
                                } else {
                                    "Bản tham chiếu"
                                },
                            ))
                            .child(div().mt_2().child("Video nền"))
                            .child(div().mt_6().child("Preset chữ"))
                            .child(div().mt_2().child("Vai & màu song ca")),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .gap_3()
                            .child(
                                div()
                                    .flex_1()
                                    .bg(rgb(0x20232b))
                                    .rounded_md()
                                    .p_4()
                                    .child("XEM TRƯỚC")
                                    .child(
                                        div()
                                            .flex()
                                            .flex_1()
                                            .justify_center()
                                            .items_center()
                                            .child("Nạp beat để xem trước video karaoke"),
                                    ),
                            )
                            .child(
                                div()
                                    .h(px(390.))
                                    .bg(rgb(0x20232b))
                                    .rounded_md()
                                    .p_3()
                                    .child(timeline)
                                    .child("DANH SÁCH TỪ")
                                    .child(div().cursor_pointer().child(if self.editor.project().settings.timeline_linked {"Mốc: Liên kết"}else{"Mốc: Độc lập"})
                                        .on_mouse_down(MouseButton::Left,cx.listener(|this, _, _, cx|this.toggle_linked(cx))))
                                    .child(div().id("word-list").overflow_y_scroll().h(px(120.)).children(lyric_rows)),
                            ),
                    )
                    .child(
                        div()
                            .w(px(250.))
                            .bg(rgb(0x20232b))
                            .rounded_md()
                            .p_3()
                            .child("CÂU ĐANG CHỌN")
                            .child(div().mt_3().child("Lời bài hát"))
                            .child(
                                div()
                                    .mt_2()
                                    .p_2()
                                    .bg(rgb(0x17191f))
                                    .child(selected_text),
                            )
                            .child(div().mt_4().child(format!("Bắt đầu {selected_start:.3}s")))
                            .child(div().mt_2().child(format!("Kết thúc {selected_end:.3}s")))
                            .children([(true,-0.05,"Đầu −50ms"),(true,0.05,"Đầu +50ms"),(false,-0.05,"Cuối −50ms"),(false,0.05,"Cuối +50ms")].into_iter().map(|(start,delta,label)|
                                div().mt_2().cursor_pointer().child(label)
                                    .on_mouse_down(MouseButton::Left,cx.listener(move |this, _, _, cx|this.adjust_word(start,delta,cx)))))
                            .child(div().mt_6().child("Space để căn lời theo nhạc")),
                    ),
            )
            .child(
                div()
                    .flex()
                    .justify_between()
                    .px_5()
                    .py_3()
                    .bg(rgb(0x20232b))
                    .child(div().cursor_pointer().child(transport)
                        .on_mouse_down(MouseButton::Left,cx.listener(|this, _, _, cx|this.transport(0,cx))))
                    .child(div().cursor_pointer().child("Dừng")
                        .on_mouse_down(MouseButton::Left,cx.listener(|this, _, _, cx|this.transport(1,cx))))
                    .child(div().cursor_pointer().child("−5s")
                        .on_mouse_down(MouseButton::Left,cx.listener(|this, _, _, cx|this.transport(2,cx))))
                    .child(div().cursor_pointer().child("+5s")
                        .on_mouse_down(MouseButton::Left,cx.listener(|this, _, _, cx|this.transport(3,cx))))
                    .child(format!("{:.2} / {:.2} s",self.play_position,self.play_duration))
                    .child(self.playback_error.clone().unwrap_or_default())
                    .child(status),
            )
    }
}

fn main() {
    let path = std::env::args_os().nth(1).map(PathBuf::from);
    let project = match path
        .as_ref()
        .map(|path| vietkara_storage::load_project(path))
        .transpose()
    {
        Ok(project) => project.unwrap_or_else(|| Project::new("Bài hát mới")),
        Err(error) => {
            eprintln!("Không mở được project: {error}");
            return;
        }
    };
    let (recovery, initial_status) = match path
        .as_ref()
        .map(|path| recovery_candidate(path))
        .transpose()
    {
        Ok(candidate) => (candidate.flatten(), "Sẵn sàng".to_owned()),
        Err(error) => (None, format!("Không đọc được recovery: {error}")),
    };
    Application::new().run(move |cx: &mut App| {
        cx.bind_keys([
            KeyBinding::new("ctrl-b", ImportBeat, Some("Studio")),
            KeyBinding::new("ctrl-r", ImportReference, Some("Studio")),
            KeyBinding::new("ctrl-t", BeginTapping, Some("Studio")),
            KeyBinding::new("ctrl-space", TogglePlayback, Some("Studio")),
            KeyBinding::new("escape", StopPlayback, Some("Studio")),
            KeyBinding::new("ctrl-left", SeekBack, Some("Studio")),
            KeyBinding::new("ctrl-right", SeekForward, Some("Studio")),
            KeyBinding::new("ctrl-shift-i", ImportSrt, Some("Studio")),
            KeyBinding::new("ctrl-o", Open, Some("Studio")),
            KeyBinding::new("ctrl-s", Save, Some("Studio")),
            KeyBinding::new("ctrl-shift-s", SaveAs, Some("Studio")),
            KeyBinding::new("ctrl-z", Undo, Some("Studio")),
            KeyBinding::new("ctrl-y", Redo, Some("Studio")),
            KeyBinding::new("ctrl-shift-z", Redo, Some("Studio")),
        ]);
        let bounds = Bounds::centered(None, size(px(1440.), px(900.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                cx.new(|cx: &mut Context<Studio>| {
                    let weak = cx.entity().downgrade();
                    window.on_window_should_close(cx, move |_, cx| {
                        weak.update(cx, |studio: &mut Studio, cx| {
                            if studio.saving || studio.autosaving {
                                studio.edit_status =
                                    "Đang ghi file, vui lòng chờ trước khi đóng".into();
                                cx.notify();
                                return false;
                            }
                            if studio.dirty
                                || studio
                                    .tapping
                                    .as_ref()
                                    .is_some_and(|s| !s.clock.history.is_empty())
                            {
                                studio.confirm_close = true;
                                cx.notify();
                                return false;
                            }
                            true
                        })
                        .unwrap_or(true)
                    });
                    let focus_handle = cx.focus_handle();
                    window.focus(&focus_handle);
                    cx.observe_window_activation(window, |studio, window, cx| {
                        if !window.is_window_active() && studio.tapping.is_some() {
                            studio.cancel_tap_hold(cx);
                        }
                    })
                    .detach();
                    let task = cx.spawn(async move |this, cx| {
                        loop {
                            Timer::after(std::time::Duration::from_secs(30)).await;
                            let snapshot = this.update(cx, |this, _| {
                                if !this.autosave_pending || this.saving || this.recovery.is_some()
                                {
                                    return None;
                                }
                                let path = this.path.clone()?;
                                this.autosave_pending = false;
                                this.autosaving = true;
                                Some((
                                    {
                                        let mut project = this.editor.project().clone();
                                        if let Some(session) = &this.tapping {
                                            project.words = session.words.clone();
                                        }
                                        project
                                    },
                                    path,
                                ))
                            });
                            let Ok(snapshot) = snapshot else {
                                break;
                            };
                            let Some((project, path)) = snapshot else {
                                continue;
                            };
                            let result = cx
                                .background_executor()
                                .spawn(async move {
                                    vietkara_storage::autosave::save_autosave(&project, &path)
                                })
                                .await;
                            if this
                                .update(cx, |this, cx| {
                                    this.autosaving = false;
                                    if result.is_ok() {
                                        this.autosave_status = "Đã cập nhật bản phục hồi".into();
                                        cx.notify();
                                    }
                                    if let Err(error) = result {
                                        this.autosave_pending = true;
                                        this.edit_status = format!("Autosave lỗi: {error}");
                                        cx.notify();
                                    }
                                })
                                .is_err()
                            {
                                break;
                            }
                        }
                    });
                    let playback_task = cx.spawn(async move |this, cx| {
                        loop {
                            Timer::after(std::time::Duration::from_millis(50)).await;
                            if this.update(cx, |this, cx| this.poll_playback(cx)).is_err() {
                                break;
                            }
                        }
                    });
                    let mut studio = Studio {
                        focus_handle,
                        editor: Editor::new(project).expect("Project mới phải hợp lệ"),
                        edit_status: initial_status,
                        path,
                        recovery,
                        saving: false,
                        dirty: false,
                        confirm_open: false,
                        confirm_close: false,
                        pending_srt: None,
                        pending_audio: None,
                        autosave_status: String::new(),
                        autosaving: false,
                        autosave_pending: false,
                        _autosave_task: task,
                        playing: false,
                        selected_word: None,
                        timeline_zoom: 80.,
                        timeline_drag: None,
                        timeline_scroll: ScrollHandle::new(),
                        tapping: None,
                        play_position: 0.,
                        play_duration: 0.,
                        playback_error: None,
                        #[cfg(windows)]
                        playback: None,
                        _playback_task: playback_task,
                    };
                    studio.reset_playback();
                    studio
                })
            },
        )
        .expect("Không mở được cửa sổ VietKara Studio");
        cx.activate(true);
    });
}
