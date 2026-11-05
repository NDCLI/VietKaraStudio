mod preview;
mod text_input;
mod theme;
mod timeline;
use gpui::{
    App, Application, Bounds, Context, Entity, FocusHandle, KeyBinding, KeyDownEvent, KeyUpEvent,
    MouseButton, ObjectFit, PathPromptOptions, Render, ScrollHandle, Task, Timer, Window,
    WindowBounds, WindowOptions, actions, div, img, prelude::*, px, rgb, size,
};
use serde_json::Value;
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
        ImportLyrics,
        TogglePlayback,
        StopPlayback,
        SeekBack,
        SeekForward,
        BeginTapping,
        ImportBeat,
        ImportReference,
        ExportVideo,
        CancelExport
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
    word_input: Entity<text_input::TextInput>,
    title_credit_input: Entity<text_input::TextInput>,
    section_name_input: Entity<text_input::TextInput>,
    focus_handle: FocusHandle,
    edit_status: String,
    playing: bool,
    selected_word: Option<String>,
    timeline_zoom: f32,
    timeline_drag: Option<timeline::TimelineDrag>,
    timeline_scroll: ScrollHandle,
    preview_layout: preview::PreviewLayoutCache,
    tapping: Option<TapSession>,
    play_position: f64,
    play_duration: f64,
    background_video_frame: Option<PathBuf>,
    background_frame_requested_at: f64,
    background_frame_pending: bool,
    playback_error: Option<String>,
    #[cfg(windows)]
    playback: Option<Playback>,
    _playback_task: Task<()>,
    path: Option<PathBuf>,
    recovery: Option<RecoveryCandidate>,
    saving: bool,
    rendering: bool,
    render_progress: f64,
    render_cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    dirty: bool,
    confirm_open: bool,
    confirm_close: bool,
    pending_lyrics: Option<Command>,
    pending_audio: Option<(AudioSlot, PathBuf)>,
    autosave_status: String,
    autosaving: bool,
    autosave_pending: bool,
    _autosave_task: Task<()>,
    left_tab_media: bool,
    inspector_design: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct WorkspaceSize {
    left: f32,
    preview: f32,
    right: f32,
}

fn workspace_size(viewport_width: f32) -> WorkspaceSize {
    if viewport_width < 960. {
        WorkspaceSize {
            left: 176.,
            preview: 320.,
            right: 204.,
        }
    } else if viewport_width < 1280. {
        WorkspaceSize {
            left: 210.,
            preview: 410.,
            right: 236.,
        }
    } else {
        WorkspaceSize {
            left: 240.,
            preview: 520.,
            right: 270.,
        }
    }
}

fn render_project(
    project: Project,
    plan: vietkara_render::layout::DisplayPlan,
    project_path: &std::path::Path,
    target: &std::path::Path,
    cancel: &std::sync::atomic::AtomicBool,
    mut report_progress: impl FnMut(f64),
) -> Result<(), String> {
    use std::process::{Command, Stdio};

    let profile = vietkara_render::RenderProfile::from_project(&project);
    let expected_extension = profile.output_name();
    let expected_extension = std::path::Path::new(&expected_extension)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default();
    if target
        .extension()
        .and_then(|extension| extension.to_str())
        .is_none_or(|extension| !extension.eq_ignore_ascii_case(expected_extension))
    {
        return Err(format!(
            "Định dạng xuất trong project là .{expected_extension}; hãy chọn tên file đúng định dạng"
        ));
    }
    if target.exists() {
        return Err("File đích đã tồn tại; hãy chọn tên khác để tránh ghi đè".into());
    }
    let project_path = std::fs::canonicalize(project_path)
        .map_err(|error| format!("Không mở được project để xuất video: {error}"))?;
    let project_directory = project_path
        .parent()
        .ok_or_else(|| "Không xác định được thư mục project".to_owned())?;
    let beat = project
        .beat
        .as_ref()
        .and_then(|beat| beat.get("filename"))
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .filter(|path| path.file_name().and_then(|name| name.to_str()) == path.to_str())
        .map(|name| project_directory.join(name))
        .filter(|path| path.is_file())
        .ok_or_else(|| "Không tìm thấy file beat cạnh project".to_owned())?;
    let duration = project
        .beat
        .as_ref()
        .and_then(|beat| beat.get("duration"))
        .and_then(Value::as_f64)
        .filter(|duration| duration.is_finite() && *duration > 0.0)
        .ok_or_else(|| "Thời lượng beat không hợp lệ".to_owned())?;
    let ffmpeg = vietkara_media::find_binary("ffmpeg").map_err(|error| error.to_string())?;
    let ffprobe = vietkara_media::find_binary("ffprobe").map_err(|error| error.to_string())?;
    let target = if target.is_absolute() {
        target.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| error.to_string())?
            .join(target)
    };
    let output_directory = target
        .parent()
        .ok_or_else(|| "Không xác định được thư mục xuất video".to_owned())?;
    if !output_directory.is_dir() {
        return Err("Thư mục xuất video không tồn tại".into());
    }
    let file_name = target
        .file_name()
        .ok_or_else(|| "Tên file xuất không hợp lệ".to_owned())?;
    let output_directory = std::fs::canonicalize(output_directory)
        .map_err(|error| format!("Không mở được thư mục xuất: {error}"))?;
    let target = output_directory.join(file_name);
    let temporary_directory =
        output_directory.join(format!(".vietkara-render-{}", vietkara_domain::new_id()));
    std::fs::create_dir(&temporary_directory)
        .map_err(|error| format!("Không tạo được thư mục render tạm: {error}"))?;

    let result = (|| {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Err("Đã hủy xuất video".into());
        }
        std::fs::create_dir(temporary_directory.join("fonts"))
            .map_err(|error| format!("Không tạo được thư mục font tạm: {error}"))?;
        let mut inputs = vietkara_render::RenderInputs::new(beat, duration);
        inputs.ffmpeg = ffmpeg;
        inputs.audio_normalize = project.settings.audio_normalize;
        inputs
            .configure_background_from_project(&project, project_directory)
            .map_err(|error| error.to_string())?;
        inputs.configure_scenes_from_project(&project, project_directory);
        let (width, height) = profile.frame_size();
        inputs
            .configure_overlays_from_project(&project, project_directory, width, height)
            .map_err(|error| error.to_string())?;
        if profile.audio_output == vietkara_render::AudioOutput::Guide {
            inputs.vocals = project
                .reference
                .as_ref()
                .and_then(|reference| reference.get("filename"))
                .and_then(Value::as_str)
                .map(PathBuf::from)
                .filter(|path| path.file_name().and_then(|name| name.to_str()) == path.to_str())
                .map(|name| project_directory.join(name));
        }
        inputs
            .prepare_lyrics_ass(&project, &plan, temporary_directory.join("lyrics.ass"))
            .map_err(|error| error.to_string())?;
        inputs.ass_file = PathBuf::from("lyrics.ass");
        inputs.fonts_dir = PathBuf::from("fonts");

        let arguments = vietkara_render::ffmpeg_command(&profile, &inputs)
            .map_err(|error| error.to_string())?;
        let executable = arguments
            .first()
            .ok_or_else(|| "Lệnh FFmpeg rỗng".to_owned())?;
        let stderr_path = temporary_directory.join("ffmpeg.stderr.log");
        let stderr_file = std::fs::File::create(&stderr_path)
            .map_err(|error| format!("Không tạo được log FFmpeg: {error}"))?;
        let mut child = Command::new(executable)
            .args(&arguments[1..])
            .current_dir(&temporary_directory)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(stderr_file))
            .spawn()
            .map_err(|error| format!("Không chạy được FFmpeg: {error}"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "Không đọc được tiến độ FFmpeg".to_owned())?;
        let mut reader = std::io::BufReader::new(stdout);
        let mut line = String::new();
        let mut was_cancelled = false;
        loop {
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                was_cancelled = true;
                let _ = child.kill();
                break;
            }
            line.clear();
            let bytes = std::io::BufRead::read_line(&mut reader, &mut line)
                .map_err(|error| format!("Không đọc được tiến độ FFmpeg: {error}"))?;
            if bytes == 0 {
                break;
            }
            if let Some(time) = line.strip_prefix("out_time=").and_then(parse_ffmpeg_time) {
                report_progress((time / duration).clamp(0.0, 0.99));
            }
        }
        drop(reader);
        let status = child
            .wait()
            .map_err(|error| format!("Không lấy được trạng thái FFmpeg: {error}"))?;
        if was_cancelled || cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Err("Đã hủy xuất video".into());
        }
        if !status.success() {
            let stderr = std::fs::read_to_string(&stderr_path).unwrap_or_default();
            let detail = stderr
                .chars()
                .rev()
                .take(1200)
                .collect::<String>()
                .chars()
                .rev()
                .collect::<String>();
            return Err(if detail.trim().is_empty() {
                format!("FFmpeg kết thúc với mã {:?}", status.code())
            } else {
                format!("FFmpeg lỗi: {}", detail.trim())
            });
        }
        let rendered = temporary_directory.join(profile.output_name());
        vietkara_render::verify_output_with_ffprobe(&rendered, duration, width, height, &ffprobe)
            .map_err(|error| format!("Video render chưa đạt kiểm tra: {error}"))?;
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Err("Đã hủy xuất video".into());
        }
        // A hard link publishes atomically and fails if another process created
        // the destination after our initial existence check.
        if let Err(link_error) = std::fs::hard_link(&rendered, &target) {
            if link_error.kind() == std::io::ErrorKind::AlreadyExists {
                return Err("File đích đã tồn tại; hãy chọn tên khác để tránh ghi đè".into());
            }
            // Some removable Windows volumes do not support hard links. Since
            // rendering is complete, fall back to an exclusive create + copy.
            let mut destination_created = false;
            let copy_result: Result<(), String> = (|| {
                use std::io::Write;
                let mut destination = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&target)
                    .map_err(|error| {
                        if error.kind() == std::io::ErrorKind::AlreadyExists {
                            "File đích đã tồn tại; hãy chọn tên khác để tránh ghi đè".to_owned()
                        } else {
                            format!("Không thể tạo file đích: {error}")
                        }
                    })?;
                destination_created = true;
                let mut source = std::fs::File::open(&rendered)
                    .map_err(|error| format!("Không mở được video đã render: {error}"))?;
                std::io::copy(&mut source, &mut destination)
                    .map_err(|error| format!("Không chép được video vào file đích: {error}"))?;
                destination
                    .flush()
                    .and_then(|()| destination.sync_all())
                    .map_err(|error| format!("Không hoàn tất được file video: {error}"))?;
                Ok(())
            })();
            if let Err(error) = copy_result {
                if destination_created {
                    let _ = std::fs::remove_file(&target);
                }
                return Err(format!(
                    "Không thể publish file mà không ghi đè ({link_error}); {error}"
                ));
            }
        }
        report_progress(1.0);
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&temporary_directory);
    result
}

fn parse_ffmpeg_time(value: &str) -> Option<f64> {
    let mut parts = value.trim().split(':');
    let hours = parts.next()?.parse::<f64>().ok()?;
    let minutes = parts.next()?.parse::<f64>().ok()?;
    let seconds = parts.next()?.parse::<f64>().ok()?;
    if parts.next().is_some()
        || !hours.is_finite()
        || !minutes.is_finite()
        || !seconds.is_finite()
        || !(0.0..60.0).contains(&minutes)
        || !(0.0..60.0).contains(&seconds)
    {
        return None;
    }
    Some(hours * 3600.0 + minutes * 60.0 + seconds)
}

enum ExportEvent {
    Progress(f64),
    Finished(Result<(), String>),
}

impl Studio {
    pub(crate) fn set_word_input(&mut self, value: &str, cx: &mut Context<Self>) {
        self.word_input.update(cx, |input, cx| {
            input.set_value(value);
            cx.notify();
        });
    }

    fn apply_word_text(&mut self, cx: &mut Context<Self>) {
        if self.saving || self.autosaving || self.tapping.is_some() || self.timeline_drag.is_some()
        {
            return;
        }
        let Some(id) = self.selected_word.clone() else {
            self.edit_status = "Chọn một từ trước khi sửa nội dung".into();
            cx.notify();
            return;
        };
        let Some(mut word) = self
            .editor
            .project()
            .words
            .iter()
            .find(|word| word.id == id)
            .cloned()
        else {
            return;
        };
        let text = self.word_input.read(cx).value().trim().to_owned();
        if text.is_empty() || text.chars().count() > 200 {
            self.edit_status = "Nội dung từ cần có 1–200 ký tự".into();
            cx.notify();
            return;
        }
        if word.locked == Some(true) {
            self.edit_status = "Câu này đang khóa".into();
            cx.notify();
            return;
        }
        if word.text == text {
            return;
        }
        word.text = text;
        let mut words = self.editor.project().words.clone();
        let Some(index) = words.iter().position(|item| item.id == id) else {
            return;
        };
        words[index] = word.clone();
        let project_text = vietkara_domain::lyrics::text_from_words(&words);
        self.edit_status = match self.editor.execute(Command::ReplaceWordText {
            id,
            word: Box::new(word),
            text: project_text,
        }) {
            Ok(_) => {
                self.dirty = true;
                self.autosave_pending = true;
                "Đã sửa từ • chưa lưu".into()
            }
            Err(error) => error.to_string(),
        };
        cx.notify();
    }

    fn cycle_selected_role(&mut self, cx: &mut Context<Self>) {
        if self.saving || self.autosaving || self.tapping.is_some() || self.timeline_drag.is_some()
        {
            return;
        }
        let Some(id) = self.selected_word.clone() else {
            return;
        };
        let Some(selected) = self
            .editor
            .project()
            .words
            .iter()
            .find(|word| word.id == id)
            .cloned()
        else {
            return;
        };
        let words: Vec<_> = self
            .editor
            .project()
            .words
            .iter()
            .filter(|word| word.line_uid == selected.line_uid)
            .collect();
        if words.iter().any(|word| word.locked == Some(true)) {
            self.edit_status = "Câu này đang khóa".into();
            cx.notify();
            return;
        }
        let roles = [
            "all", "male", "female", "voice_c", "voice_d", "voice_e", "voice_f", "unknown",
        ];
        let index = roles
            .iter()
            .position(|role| *role == selected.role)
            .unwrap_or(0);
        let next_role = roles[(index + 1) % roles.len()];
        let label = role_label(next_role);
        let edits = words
            .into_iter()
            .map(|word| {
                let mut updated = word.clone();
                updated.role = next_role.into();
                Command::ReplaceWord {
                    id: word.id.clone(),
                    word: Box::new(updated),
                }
            })
            .collect();
        self.edit_status = match self.editor.execute(Command::Batch(edits)) {
            Ok(_) => {
                self.dirty = true;
                self.autosave_pending = true;
                format!("Đã đổi vai câu: {label} • chưa lưu")
            }
            Err(error) => error.to_string(),
        };
        cx.notify();
    }

    fn update_setting(&mut self, key: &str, value: Value, cx: &mut Context<Self>) {
        if self.saving || self.autosaving || self.recovery.is_some() || self.tapping.is_some() {
            return;
        }
        let mut settings = self.editor.project().settings.clone();
        settings.extra.insert(key.to_owned(), value);
        self.edit_status = match self.editor.execute(Command::SetSettings(settings)) {
            Ok(_) => {
                self.dirty = true;
                self.autosave_pending = true;
                "Đã cập nhật thiết kế • chưa lưu".into()
            }
            Err(error) => error.to_string(),
        };
        cx.notify();
    }
    fn update_typed_settings(
        &mut self,
        update: impl FnOnce(&mut vietkara_domain::settings::Settings),
        cx: &mut Context<Self>,
    ) {
        if self.saving || self.autosaving || self.recovery.is_some() || self.tapping.is_some() {
            return;
        }
        let mut settings = self.editor.project().settings.clone();
        update(&mut settings);
        self.edit_status = match self.editor.execute(Command::SetSettings(settings)) {
            Ok(_) => {
                self.dirty = true;
                self.autosave_pending = true;
                "Đã cập nhật cài đặt xuất • chưa lưu".into()
            }
            Err(error) => error.to_string(),
        };
        cx.notify();
    }
    fn apply_section_name(&mut self, cx: &mut Context<Self>) {
        if self.saving
            || self.autosaving
            || self.recovery.is_some()
            || self.tapping.is_some()
            || self.timeline_drag.is_some()
        {
            return;
        }
        let Some(id) = self.selected_word.as_deref() else {
            self.edit_status = "Chọn một từ trong section trước".into();
            cx.notify();
            return;
        };
        let Some(selected) = self
            .editor
            .project()
            .words
            .iter()
            .find(|word| word.id == id)
        else {
            return;
        };
        let line = selected.line;
        let name = self.section_name_input.read(cx).value();
        let mut words = self.editor.project().words.clone();
        set_section_name_on_line(&mut words, line, &name);
        let project_text = vietkara_domain::lyrics::text_from_words(&words);
        let was_dirty = self.dirty;
        self.edit_status = match self.editor.execute(Command::SetLyrics {
            words,
            text: project_text,
        }) {
            Ok(_) => {
                self.dirty = true;
                self.autosave_pending = true;
                "Đã đặt tên section • chưa lưu".into()
            }
            Err(error) => {
                self.dirty = was_dirty;
                error.to_string()
            }
        };
        cx.notify();
    }
    fn toggle_section_boundary(&mut self, cx: &mut Context<Self>) {
        if self.saving
            || self.autosaving
            || self.recovery.is_some()
            || self.tapping.is_some()
            || self.timeline_drag.is_some()
        {
            return;
        }
        let Some(id) = self.selected_word.as_deref() else {
            self.edit_status = "Chọn một từ để đặt biên section".into();
            cx.notify();
            return;
        };
        let Some(selected) = self
            .editor
            .project()
            .words
            .iter()
            .find(|word| word.id == id)
        else {
            return;
        };
        let line = selected.line;
        let mut words = self.editor.project().words.clone();
        let Some(next) = toggle_section_on_line(&mut words, line) else {
            return;
        };
        let project_text = vietkara_domain::lyrics::text_from_words(&words);
        self.edit_status = match self.editor.execute(Command::SetLyrics {
            words,
            text: project_text,
        }) {
            Ok(_) => {
                self.dirty = true;
                self.autosave_pending = true;
                if !next {
                    "Đã bỏ biên section • chưa lưu".into()
                } else {
                    "Đã đặt biên section • chưa lưu".into()
                }
            }
            Err(error) => error.to_string(),
        };
        cx.notify();
    }
    fn apply_title_text(&mut self, cx: &mut Context<Self>) {
        let text = self.word_input.read(cx).value().trim().to_owned();
        if text.chars().count() > 200 {
            self.edit_status = "Title không được dài quá 200 ký tự".into();
            cx.notify();
            return;
        }
        self.update_setting("title_text", Value::String(text), cx);
    }
    fn apply_title_credit(&mut self, cx: &mut Context<Self>) {
        let text = self.title_credit_input.read(cx).value().trim().to_owned();
        if text.chars().count() > 120 {
            self.edit_status = "Credit không được dài quá 120 ký tự".into();
            cx.notify();
            return;
        }
        self.update_setting("title_credit", Value::String(text), cx);
    }
    fn adjust_title_number(
        &mut self,
        key: &str,
        delta: f64,
        min: f64,
        max: f64,
        cx: &mut Context<Self>,
    ) {
        let current = self
            .editor
            .project()
            .settings
            .extra
            .get(key)
            .and_then(Value::as_f64)
            .unwrap_or(min);
        let next = (current + delta).clamp(min, max);
        if (next - current).abs() < f64::EPSILON {
            return;
        }
        self.update_setting(key, Value::from(next), cx);
    }
    fn cycle_setting(&mut self, key: &str, values: &[&str], cx: &mut Context<Self>) {
        let current = self
            .editor
            .project()
            .settings
            .extra
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or("");
        let next = values
            .iter()
            .position(|value| *value == current)
            .map(|index| (index + 1) % values.len())
            .unwrap_or(0);
        if let Some(value) = values.get(next) {
            self.update_setting(key, Value::String((*value).to_owned()), cx);
        }
    }
    fn choose_audio(&mut self, slot: AudioSlot, cx: &mut Context<Self>) {
        if self.saving
            || self.autosaving
            || self.recovery.is_some()
            || self.tapping.is_some()
            || self.timeline_drag.is_some()
            || self.pending_audio.is_some()
            || self.pending_lyrics.is_some()
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
    fn choose_background_image(&mut self, cx: &mut Context<Self>) {
        if self.saving
            || self.autosaving
            || self.recovery.is_some()
            || self.tapping.is_some()
            || self.timeline_drag.is_some()
            || self.pending_audio.is_some()
            || self.pending_lyrics.is_some()
        {
            return;
        }
        let Some(main) = self.path.clone() else {
            self.edit_status = "Hãy lưu project trước khi thêm ảnh nền".into();
            cx.notify();
            return;
        };
        let project = self.editor.project().clone();
        let selection = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Chọn ảnh nền (PNG, JPG hoặc WebP)".into()),
        });
        self.saving = true;
        self.edit_status = "Đang thêm ảnh nền và tạo phiên bản phục hồi…".into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = match selection.await {
                Ok(Ok(Some(paths))) => {
                    if let Some(source) = paths.into_iter().next() {
                        cx.background_executor()
                            .spawn(async move {
                                vietkara_storage::background_import::import_image_background(
                                    &project, &main, &source,
                                )
                            })
                            .await
                            .map(Some)
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
                this.edit_status = match result {
                    Ok(Some(imported)) => match Editor::new(imported.project) {
                        Ok(editor) => {
                            this.editor = editor;
                            this.dirty = false;
                            this.autosave_pending = false;
                            format!("Đã thêm ảnh nền • phiên bản trước: {}", imported.version_id)
                        }
                        Err(error) => error.to_string(),
                    },
                    Ok(None) => "Đã hủy chọn ảnh nền".into(),
                    Err(error) => error,
                };
                cx.notify();
            });
        })
        .detach();
    }
    fn choose_background_video(&mut self, cx: &mut Context<Self>) {
        if self.saving
            || self.autosaving
            || self.recovery.is_some()
            || self.tapping.is_some()
            || self.timeline_drag.is_some()
            || self.pending_audio.is_some()
            || self.pending_lyrics.is_some()
        {
            return;
        }
        let Some(main) = self.path.clone() else {
            self.edit_status = "Hãy lưu project trước khi thêm video nền".into();
            cx.notify();
            return;
        };
        let project = self.editor.project().clone();
        let selection = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Chọn video nền (MP4, MOV, MKV, WebM, AVI hoặc M4V)".into()),
        });
        self.saving = true;
        self.edit_status = "Đang tạo proxy video nền và phiên bản phục hồi…".into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = match selection.await {
                Ok(Ok(Some(paths))) => {
                    if let Some(source) = paths.into_iter().next() {
                        cx.background_executor()
                            .spawn(async move {
                                vietkara_storage::background_import::import_video_background(
                                    &project, &main, &source,
                                )
                            })
                            .await
                            .map(Some)
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
                this.edit_status = match result {
                    Ok(Some(imported)) => match Editor::new(imported.project) {
                        Ok(editor) => {
                            this.editor = editor;
                            this.dirty = false;
                            this.autosave_pending = false;
                            format!(
                                "Đã thêm nền video • phiên bản trước: {}",
                                imported.version_id
                            )
                        }
                        Err(error) => error.to_string(),
                    },
                    Ok(None) => "Đã hủy chọn video nền".into(),
                    Err(error) => error,
                };
                cx.notify();
            });
        })
        .detach();
    }
    fn choose_logo(&mut self, cx: &mut Context<Self>) {
        if self.saving
            || self.autosaving
            || self.recovery.is_some()
            || self.tapping.is_some()
            || self.timeline_drag.is_some()
        {
            return;
        }
        let Some(main) = self.path.clone() else {
            self.edit_status = "Hãy lưu project trước khi thêm logo".into();
            cx.notify();
            return;
        };
        let project = self.editor.project().clone();
        let selection = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Chọn logo PNG".into()),
        });
        self.saving = true;
        self.edit_status = "Đang thêm logo và tạo phiên bản phục hồi…".into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = match selection.await {
                Ok(Ok(Some(paths))) => {
                    if let Some(source) = paths.into_iter().next() {
                        cx.background_executor()
                            .spawn(async move {
                                vietkara_storage::background_import::import_logo(
                                    &project, &main, &source,
                                )
                            })
                            .await
                            .map(Some)
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
                this.edit_status = match result {
                    Ok(Some(imported)) => match Editor::new(imported.project) {
                        Ok(editor) => {
                            this.editor = editor;
                            this.dirty = false;
                            this.autosave_pending = false;
                            format!("Đã thêm logo • phiên bản trước: {}", imported.version_id)
                        }
                        Err(error) => error.to_string(),
                    },
                    Ok(None) => "Đã hủy chọn logo".into(),
                    Err(error) => error,
                };
                cx.notify();
            });
        })
        .detach();
    }
    fn choose_countdown_image(&mut self, cx: &mut Context<Self>) {
        if self.saving
            || self.autosaving
            || self.recovery.is_some()
            || self.tapping.is_some()
            || self.timeline_drag.is_some()
        {
            return;
        }
        let Some(main) = self.path.clone() else {
            self.edit_status = "Hãy lưu project trước khi thêm ảnh countdown".into();
            cx.notify();
            return;
        };
        let project = self.editor.project().clone();
        let selection = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Chọn ảnh countdown PNG".into()),
        });
        self.saving = true;
        self.edit_status = "Đang thêm ảnh countdown và tạo phiên bản phục hồi…".into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = match selection.await {
                Ok(Ok(Some(paths))) => {
                    if let Some(source) = paths.into_iter().next() {
                        cx.background_executor()
                            .spawn(async move {
                                vietkara_storage::background_import::import_countdown_image(
                                    &project, &main, &source,
                                )
                            })
                            .await
                            .map(Some)
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
                this.edit_status = match result {
                    Ok(Some(imported)) => match Editor::new(imported.project) {
                        Ok(editor) => {
                            this.editor = editor;
                            this.dirty = false;
                            this.autosave_pending = false;
                            format!(
                                "Đã thêm ảnh countdown • phiên bản trước: {}",
                                imported.version_id
                            )
                        }
                        Err(error) => error.to_string(),
                    },
                    Ok(None) => "Đã hủy chọn ảnh countdown".into(),
                    Err(error) => error,
                };
                cx.notify();
            });
        })
        .detach();
    }
    fn set_selected_scene_section(&mut self, cx: &mut Context<Self>) {
        const SECTIONS: &[&str] = &[
            "intro",
            "verse",
            "pre-chorus",
            "chorus",
            "bridge",
            "outro",
            "instrumental",
        ];
        let Some(id) = self.selected_word.as_deref() else {
            return;
        };
        let Some(line) = self
            .editor
            .project()
            .words
            .iter()
            .find(|word| word.id == id)
            .map(|word| word.line)
        else {
            return;
        };
        let current = self
            .editor
            .project()
            .words
            .iter()
            .find(|word| word.id == id)
            .and_then(|word| word.section_name.as_deref())
            .unwrap_or("");
        let next = SECTIONS
            .iter()
            .position(|section| *section == current)
            .map(|index| (index + 1) % SECTIONS.len())
            .unwrap_or(0);
        let mut words = self.editor.project().words.clone();
        set_section_name_on_line(&mut words, line, SECTIONS[next]);
        let text = vietkara_domain::lyrics::text_from_words(&words);
        self.edit_status = match self.editor.execute(Command::SetLyrics { words, text }) {
            Ok(_) => {
                self.dirty = true;
                self.autosave_pending = true;
                "Đã chọn loại section scene • chưa lưu".into()
            }
            Err(error) => error.to_string(),
        };
        self.section_name_input.update(cx, |input, cx| {
            input.set_value(SECTIONS[next]);
            cx.notify();
        });
        cx.notify();
    }
    fn choose_scene_image(&mut self, cx: &mut Context<Self>) {
        if self.saving
            || self.autosaving
            || self.recovery.is_some()
            || self.tapping.is_some()
            || self.timeline_drag.is_some()
        {
            return;
        }
        let Some(main) = self.path.clone() else {
            self.edit_status = "Hãy lưu project trước khi thêm scene".into();
            cx.notify();
            return;
        };
        let Some(id) = self.selected_word.as_deref() else {
            self.edit_status = "Chọn dòng đã gán loại section scene trước".into();
            cx.notify();
            return;
        };
        let Some(section) = self
            .editor
            .project()
            .words
            .iter()
            .find(|word| word.id == id)
            .and_then(|word| word.section_name.clone())
        else {
            self.edit_status = "Hãy gán loại section scene cho dòng đang chọn".into();
            cx.notify();
            return;
        };
        if ![
            "intro",
            "verse",
            "pre-chorus",
            "chorus",
            "bridge",
            "outro",
            "instrumental",
        ]
        .contains(&section.as_str())
        {
            self.edit_status = "Tên section này không thuộc các loại scene được hỗ trợ".into();
            cx.notify();
            return;
        }
        let project = self.editor.project().clone();
        let selection = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Chọn ảnh scene (PNG, JPG hoặc WebP)".into()),
        });
        self.saving = true;
        self.edit_status = "Đang thêm ảnh scene và tạo phiên bản phục hồi…".into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = match selection.await {
                Ok(Ok(Some(paths))) => {
                    if let Some(source) = paths.into_iter().next() {
                        cx.background_executor()
                            .spawn(async move {
                                vietkara_storage::background_import::import_scene_image(
                                    &project, &main, &source, &section,
                                )
                            })
                            .await
                            .map(Some)
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
                this.edit_status = match result {
                    Ok(Some(imported)) => match Editor::new(imported.project) {
                        Ok(editor) => {
                            this.editor = editor;
                            this.dirty = false;
                            this.autosave_pending = false;
                            format!(
                                "Đã thêm ảnh scene • phiên bản trước: {}",
                                imported.version_id
                            )
                        }
                        Err(error) => error.to_string(),
                    },
                    Ok(None) => "Đã hủy chọn ảnh scene".into(),
                    Err(error) => error,
                };
                cx.notify();
            });
        })
        .detach();
    }
    fn choose_scene_decoration(&mut self, cx: &mut Context<Self>) {
        if self.saving
            || self.autosaving
            || self.recovery.is_some()
            || self.tapping.is_some()
            || self.timeline_drag.is_some()
        {
            return;
        }
        let Some(main) = self.path.clone() else {
            self.edit_status = "Hãy lưu project trước khi thêm ảnh trang trí".into();
            cx.notify();
            return;
        };
        let Some(section) = self
            .selected_word
            .as_deref()
            .and_then(|id| {
                self.editor
                    .project()
                    .words
                    .iter()
                    .find(|word| word.id == id)
            })
            .and_then(|word| word.section_name.clone())
        else {
            self.edit_status = "Chọn dòng thuộc section scene trước".into();
            cx.notify();
            return;
        };
        let project = self.editor.project().clone();
        let selection = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Chọn ảnh trang trí (PNG, JPG hoặc WebP)".into()),
        });
        self.saving = true;
        self.edit_status = "Đang thêm ảnh trang trí và tạo phiên bản phục hồi…".into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = match selection.await {
                Ok(Ok(Some(paths))) => {
                    if let Some(source) = paths.into_iter().next() {
                        cx.background_executor()
                            .spawn(async move {
                                vietkara_storage::background_import::import_scene_decoration(
                                    &project, &main, &source, &section,
                                )
                            })
                            .await
                            .map(Some)
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
                this.edit_status = match result {
                    Ok(Some(imported)) => match Editor::new(imported.project) {
                        Ok(editor) => {
                            this.editor = editor;
                            this.dirty = false;
                            this.autosave_pending = false;
                            format!(
                                "Đã thêm ảnh trang trí • phiên bản trước: {}",
                                imported.version_id
                            )
                        }
                        Err(error) => error.to_string(),
                    },
                    Ok(None) => "Đã hủy chọn ảnh trang trí".into(),
                    Err(error) => error,
                };
                cx.notify();
            });
        })
        .detach();
    }
    fn remove_scene_decoration(&mut self, cx: &mut Context<Self>) {
        if self.saving || self.autosaving || self.recovery.is_some() {
            return;
        }
        let (Some(main), Some(section)) = (
            self.path.clone(),
            self.selected_word
                .as_deref()
                .and_then(|id| {
                    self.editor
                        .project()
                        .words
                        .iter()
                        .find(|word| word.id == id)
                })
                .and_then(|word| word.section_name.clone()),
        ) else {
            return;
        };
        let project = self.editor.project().clone();
        self.saving = true;
        self.edit_status = "Đang bỏ ảnh trang trí và tạo phiên bản phục hồi…".into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    vietkara_storage::background_import::remove_scene_decoration(
                        &project, &main, &section,
                    )
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.saving = false;
                this.edit_status = match result {
                    Ok(imported) => match Editor::new(imported.project) {
                        Ok(editor) => {
                            this.editor = editor;
                            this.dirty = false;
                            this.autosave_pending = false;
                            format!(
                                "Đã bỏ ảnh trang trí • phiên bản trước: {}",
                                imported.version_id
                            )
                        }
                        Err(error) => error.to_string(),
                    },
                    Err(error) => error,
                };
                cx.notify();
            });
        })
        .detach();
    }
    fn adjust_selected_scene_number(
        &mut self,
        key: &'static str,
        delta: f64,
        minimum: f64,
        maximum: f64,
        cx: &mut Context<Self>,
    ) {
        if self.saving || self.autosaving || self.recovery.is_some() {
            return;
        }
        let Some(id) = self.selected_word.as_deref() else {
            return;
        };
        let Some(section) = self
            .editor
            .project()
            .words
            .iter()
            .find(|word| word.id == id)
            .and_then(|word| word.section_name.clone())
        else {
            return;
        };
        let mut scenes = self.editor.project().scenes.clone();
        let Some(scene) = scenes
            .iter_mut()
            .find(|scene| scene.get("section_name").and_then(Value::as_str) == Some(&section))
        else {
            return;
        };
        let default = match key {
            "decoration_size" => 28.0,
            "transition_seconds" => 0.6,
            _ => 100.0,
        };
        let current = scene.get(key).and_then(Value::as_f64).unwrap_or(default);
        scene[key] = serde_json::json!((current + delta).clamp(minimum, maximum));
        self.edit_status = match self.editor.execute(Command::SetScenes(scenes)) {
            Ok(_) => {
                self.dirty = true;
                self.autosave_pending = true;
                "Đã cập nhật ảnh scene • chưa lưu".into()
            }
            Err(error) => error.to_string(),
        };
        cx.notify();
    }
    fn cycle_selected_scene_option(
        &mut self,
        key: &'static str,
        options: &[&'static str],
        cx: &mut Context<Self>,
    ) {
        if self.saving || self.autosaving || self.recovery.is_some() {
            return;
        }
        let Some(id) = self.selected_word.as_deref() else {
            return;
        };
        let Some(section) = self
            .editor
            .project()
            .words
            .iter()
            .find(|word| word.id == id)
            .and_then(|word| word.section_name.clone())
        else {
            return;
        };
        let mut scenes = self.editor.project().scenes.clone();
        let Some(scene) = scenes
            .iter_mut()
            .find(|scene| scene.get("section_name").and_then(Value::as_str) == Some(&section))
        else {
            return;
        };
        let current = scene.get(key).and_then(Value::as_str).unwrap_or(options[0]);
        let next = options
            .iter()
            .position(|value| *value == current)
            .map(|index| (index + 1) % options.len())
            .unwrap_or(0);
        scene[key] = serde_json::json!(options[next]);
        if key == "transition" && options[next] == "cut" {
            scene["transition_seconds"] = serde_json::json!(0.0);
        }
        if key == "transition" && options[next] == "fade" {
            scene["transition_seconds"] = serde_json::json!(0.6);
        }
        self.edit_status = match self.editor.execute(Command::SetScenes(scenes)) {
            Ok(_) => {
                self.dirty = true;
                self.autosave_pending = true;
                "Đã cập nhật thiết lập scene • chưa lưu".into()
            }
            Err(error) => error.to_string(),
        };
        cx.notify();
    }
    fn adjust_selected_scene_frame(
        &mut self,
        endpoint: usize,
        key: &'static str,
        delta: f64,
        cx: &mut Context<Self>,
    ) {
        if self.saving || self.autosaving || self.recovery.is_some() {
            return;
        }
        let Some(id) = self.selected_word.as_deref() else {
            return;
        };
        let Some(section) = self
            .editor
            .project()
            .words
            .iter()
            .find(|word| word.id == id)
            .and_then(|word| word.section_name.clone())
        else {
            return;
        };
        let mut scenes = self.editor.project().scenes.clone();
        let Some(scene) = scenes
            .iter_mut()
            .find(|scene| scene.get("section_name").and_then(Value::as_str) == Some(&section))
        else {
            return;
        };
        if !scene.get("keyframes").is_some_and(Value::is_array) {
            scene["keyframes"] = serde_json::json!([]);
        }
        let frames = scene["keyframes"].as_array_mut().unwrap();
        while frames.len() < 2 {
            frames.push(serde_json::json!({"x":0,"y":0,"scale":100,"opacity":100}));
        }
        let (min, max) = match key {
            "x" | "y" => (-100.0, 100.0),
            "scale" => (100.0, 200.0),
            _ => (0.0, 100.0),
        };
        if endpoint > 1 {
            return;
        }
        let frame = &mut frames[endpoint];
        let current = frame
            .get(key)
            .and_then(Value::as_f64)
            .unwrap_or(if key == "x" || key == "y" { 0.0 } else { 100.0 });
        frame[key] = serde_json::json!((current + delta).clamp(min, max));
        self.edit_status = match self.editor.execute(Command::SetScenes(scenes)) {
            Ok(_) => {
                self.dirty = true;
                self.autosave_pending = true;
                "Đã cập nhật chuyển động scene • chưa lưu".into()
            }
            Err(error) => error.to_string(),
        };
        cx.notify();
    }
    fn adjust_selected_decoration_frame(
        &mut self,
        endpoint: usize,
        key: &'static str,
        delta: f64,
        cx: &mut Context<Self>,
    ) {
        if self.saving || self.autosaving || self.recovery.is_some() || endpoint > 1 {
            return;
        }
        let Some(id) = self.selected_word.as_deref() else {
            return;
        };
        let Some(section) = self
            .editor
            .project()
            .words
            .iter()
            .find(|word| word.id == id)
            .and_then(|word| word.section_name.clone())
        else {
            return;
        };
        let mut scenes = self.editor.project().scenes.clone();
        let Some(scene) = scenes
            .iter_mut()
            .find(|scene| scene.get("section_name").and_then(Value::as_str) == Some(&section))
        else {
            return;
        };
        if !scene
            .get("decoration_keyframes")
            .is_some_and(Value::is_array)
        {
            scene["decoration_keyframes"] = serde_json::json!([
                {"x":50,"y":50,"scale":100,"opacity":100}, {"x":50,"y":50,"scale":100,"opacity":100}
            ]);
        }
        let frame = &mut scene["decoration_keyframes"].as_array_mut().unwrap()[endpoint];
        let (min, max, default) = match key {
            "x" | "y" => (0.0, 100.0, 50.0),
            "scale" => (100.0, 200.0, 100.0),
            _ => (0.0, 100.0, 100.0),
        };
        let current = frame.get(key).and_then(Value::as_f64).unwrap_or(default);
        frame[key] = serde_json::json!((current + delta).clamp(min, max));
        self.edit_status = match self.editor.execute(Command::SetScenes(scenes)) {
            Ok(_) => {
                self.dirty = true;
                self.autosave_pending = true;
                "Đã cập nhật chuyển động ảnh trang trí • chưa lưu".into()
            }
            Err(error) => error.to_string(),
        };
        cx.notify();
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
                            this.set_word_input("", cx);
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
        let (tap, id, text) = {
            let Some(session) = &mut self.tapping else {
                return;
            };
            let Some(tap) = session.clock.back() else {
                return;
            };
            session.words[tap.index] = self.editor.project().words[tap.index].clone();
            (
                tap,
                session.words[tap.index].id.clone(),
                session.words[tap.index].text.clone(),
            )
        };
        self.selected_word = Some(id);
        self.set_word_input(&text, cx);
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
            || self.pending_lyrics.is_some()
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
        let outcome = {
            let Some(session) = &mut self.tapping else {
                return;
            };
            let mut project = self.editor.project().clone();
            project.words = session.words.clone();
            match vietkara_domain::tapping::tap_command(&project, tap) {
                Ok(Command::ReplaceWord { word, .. }) => {
                    let id = word.id.clone();
                    let text = word.text.clone();
                    session.words[tap.index] = *word;
                    Ok(Some((id, text)))
                }
                Err(error) => {
                    session.clock.back();
                    Err(error)
                }
                _ => Ok(None),
            }
        };
        match outcome {
            Ok(Some((id, text))) => {
                self.selected_word = Some(id);
                self.set_word_input(&text, cx);
                self.autosave_pending = true;
                cx.notify();
            }
            Err(error) => {
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
            let text = word.text.clone();
            let section_name = word.section_name.clone().unwrap_or_default();
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
            self.set_word_input(&text, cx);
            self.section_name_input.update(cx, |input, cx| {
                input.set_value(&section_name);
                cx.notify();
            });
            cx.notify();
        }
    }
    fn toggle_linked(&mut self, cx: &mut Context<Self>) {
        if self.saving
            || self.tapping.is_some()
            || self.timeline_drag.is_some()
            || self.recovery.is_some()
            || self.pending_lyrics.is_some()
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
            || self.pending_lyrics.is_some()
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
        self.request_background_frame(cx);
    }

    fn request_background_frame(&mut self, cx: &mut Context<Self>) {
        let project = self.editor.project();
        if project
            .settings
            .extra
            .get("background_type")
            .and_then(Value::as_str)
            != Some("video")
            || (self.play_position - self.background_frame_requested_at).abs() < 0.5
            || self.background_frame_pending
        {
            return;
        }
        let Some(main) = self.path.as_ref() else {
            return;
        };
        let Some(metadata) = project.extra.get("background_video") else {
            return;
        };
        let Some(directory) = main.parent() else {
            return;
        };
        let Some(video) = background_video_preview_path(directory, metadata) else {
            return;
        };
        let position = self.play_position.max(0.0);
        let cache_dir = std::env::temp_dir().join(format!("vietkara-preview-{}", project.id));
        let output = cache_dir.join(format!("frame-{}.jpg", (position * 1000.0) as u64));
        self.background_frame_pending = true;
        self.background_frame_requested_at = position;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    std::fs::create_dir_all(&cache_dir).map_err(|error| error.to_string())?;
                    vietkara_storage::background_import::extract_video_frame(
                        &video, position, &output,
                    )?;
                    let mut cached = std::fs::read_dir(&cache_dir)
                        .map_err(|error| error.to_string())?
                        .flatten()
                        .filter_map(|entry| {
                            let path = entry.path();
                            let modified = entry.metadata().ok()?.modified().ok()?;
                            path.extension()
                                .is_some_and(|extension| extension == "jpg")
                                .then_some((path, modified))
                        })
                        .collect::<Vec<_>>();
                    cached.sort_by_key(|(_, modified)| std::cmp::Reverse(*modified));
                    for (stale, _) in cached.into_iter().skip(16) {
                        if stale != output {
                            let _ = std::fs::remove_file(stale);
                        }
                    }
                    Ok::<_, String>(output)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.background_frame_pending = false;
                if let Ok(path) = result {
                    this.background_video_frame = Some(path);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn import_srt(&mut self, cx: &mut Context<Self>) {
        self.import_lyrics_file(true, cx);
    }

    fn import_plain_lyrics(&mut self, cx: &mut Context<Self>) {
        self.import_lyrics_file(false, cx);
    }

    fn import_lyrics_file(&mut self, is_srt: bool, cx: &mut Context<Self>) {
        if self.saving
            || self.autosaving
            || self.recovery.is_some()
            || self.pending_lyrics.is_some()
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
            self.edit_status = "Mở khóa các câu trước khi thay lời".into();
            cx.notify();
            return;
        }
        let duration = self.play_duration;
        if !is_srt && duration <= 0. {
            self.edit_status = "Hãy nạp beat trước khi ước lượng mốc cho lời TXT".into();
            cx.notify();
            return;
        }
        let project = self.editor.project().clone();
        let selection = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(
                if is_srt {
                    "Nhập file SRT"
                } else {
                    "Nhập lời dạng TXT"
                }
                .into(),
            ),
        });
        self.saving = true;
        self.edit_status = if is_srt {
            "Đang đọc file SRT…"
        } else {
            "Đang ước lượng mốc lời…"
        }
        .into();
        cx.notify();
        let is_srt_for_parse = is_srt;
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
                                    return Err("File lời vượt quá 10 MiB".to_owned());
                                }
                                let command = if is_srt_for_parse {
                                    vietkara_domain::lyrics::import_srt(&project, &source)
                                } else {
                                    vietkara_domain::lyrics::estimate_lyrics(
                                        &project, &source, duration,
                                    )
                                };
                                command.map(Some).map_err(|e| e.to_string())
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
                        this.pending_lyrics = Some(command);
                        this.edit_status = if is_srt {
                            "SRT sẵn sàng, chờ xác nhận"
                        } else {
                            "Lời sẵn sàng, chờ xác nhận"
                        }
                        .into();
                    }
                    Ok(None) => this.edit_status = "Đã hủy nhập lời".into(),
                    Err(error) => this.edit_status = error,
                }
                cx.notify();
            });
        })
        .detach();
    }
    fn accept_lyrics(&mut self, cx: &mut Context<Self>) {
        let Some(command) = self.pending_lyrics.take() else {
            return;
        };
        self.edit_status = match self.editor.execute(command) {
            Ok(_) => {
                self.dirty = true;
                self.autosave_pending = true;
                self.selected_word = self
                    .editor
                    .project()
                    .words
                    .first()
                    .map(|word| word.id.clone());
                let first_text = self
                    .editor
                    .project()
                    .words
                    .first()
                    .map(|word| word.text.clone())
                    .unwrap_or_default();
                self.set_word_input(&first_text, cx);
                "Đã thay lời • chưa lưu".into()
            }
            Err(error) => error.to_string(),
        };
        cx.notify();
    }

    fn open(&mut self, cx: &mut Context<Self>) {
        if self.saving
            || self.autosaving
            || self.recovery.is_some()
            || self.pending_lyrics.is_some()
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
            || self.pending_lyrics.is_some()
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
                        this.set_word_input("", cx);
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
            || self.pending_lyrics.is_some()
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
            || self.pending_lyrics.is_some()
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

    fn export_video(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.rendering
            || self.saving
            || self.autosaving
            || self.recovery.is_some()
            || self.pending_lyrics.is_some()
            || self.pending_audio.is_some()
            || self.tapping.is_some()
            || self.timeline_drag.is_some()
        {
            return;
        }
        let Some(project_path) = self.path.clone() else {
            self.edit_status = "Hãy lưu project trước khi xuất video".into();
            cx.notify();
            return;
        };
        let project = self.editor.project().clone();
        let has_valid_duration = project
            .beat
            .as_ref()
            .and_then(|beat| beat.get("duration"))
            .and_then(Value::as_f64)
            .is_some_and(|duration| duration.is_finite() && duration > 0.0);
        if !has_valid_duration {
            self.edit_status = "Project chưa có beat hợp lệ để xuất video".into();
            cx.notify();
            return;
        }
        let beat_name = project
            .beat
            .as_ref()
            .and_then(|beat| beat.get("filename"))
            .and_then(Value::as_str)
            .map(PathBuf::from);
        let Some(beat_name) = beat_name
            .filter(|name| name.file_name().and_then(|value| value.to_str()) == name.to_str())
        else {
            self.edit_status = "Không xác định được file beat trong project".into();
            cx.notify();
            return;
        };
        if !project_path
            .parent()
            .is_some_and(|directory| directory.join(&beat_name).is_file())
        {
            self.edit_status = "Không tìm thấy file beat cạnh project".into();
            cx.notify();
            return;
        }
        let profile = vietkara_render::RenderProfile::from_project(&project);
        let suggested_name = profile.output_name();
        let Some(directory) = project_path.parent().map(PathBuf::from) else {
            self.edit_status = "Không xác định được thư mục project".into();
            cx.notify();
            return;
        };
        let plan = preview::cached_display_plan(
            &mut self.preview_layout,
            &project,
            &project.words,
            window.text_system(),
        )
        .clone();
        let selection = cx.prompt_for_new_path(&directory, Some(&suggested_name));
        self.rendering = true;
        self.edit_status = "Đang xuất video…".into();
        self.render_progress = 0.0;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = match selection.await {
                Ok(Ok(Some(target))) => {
                    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                    let worker_cancel = cancel.clone();
                    if this
                        .update(cx, |this, cx| {
                            this.render_cancel = Some(cancel);
                            cx.notify();
                        })
                        .is_err()
                    {
                        return;
                    }
                    let (sender, receiver) = std::sync::mpsc::channel();
                    let worker_project_path = project_path.clone();
                    let worker_target = target.clone();
                    let thread = std::thread::Builder::new()
                        .name("vietkara-render".into())
                        .spawn(move || {
                            let result = render_project(
                                project,
                                plan,
                                &worker_project_path,
                                &worker_target,
                                &worker_cancel,
                                |progress| {
                                    let _ = sender.send(ExportEvent::Progress(progress));
                                },
                            );
                            let _ = sender.send(ExportEvent::Finished(result));
                        });
                    match thread {
                        Ok(_) => loop {
                            Timer::after(std::time::Duration::from_millis(120)).await;
                            let mut finished = None;
                            loop {
                                match receiver.try_recv() {
                                    Ok(ExportEvent::Progress(progress)) => {
                                        if this
                                            .update(cx, |this, cx| {
                                                this.render_progress = progress;
                                                this.edit_status = format!(
                                                    "Đang xuất video… {:.0}%",
                                                    progress * 100.0
                                                );
                                                cx.notify();
                                            })
                                            .is_err()
                                        {
                                            return;
                                        }
                                    }
                                    Ok(ExportEvent::Finished(result)) => {
                                        finished = Some(result.map(|()| target.clone()));
                                        break;
                                    }
                                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                                        finished =
                                            Some(Err("Tác vụ render dừng bất thường".into()));
                                        break;
                                    }
                                }
                            }
                            if let Some(result) = finished {
                                break result;
                            }
                        },
                        Err(error) => Err(format!("Không khởi chạy được tác vụ render: {error}")),
                    }
                }
                Ok(Ok(None)) => Err("Đã hủy xuất video".to_owned()),
                Ok(Err(error)) => Err(error.to_string()),
                Err(error) => Err(error.to_string()),
            };
            let _ = this.update(cx, |this, cx| {
                this.rendering = false;
                this.render_cancel = None;
                this.edit_status = match result {
                    Ok(path) => format!("Đã xuất video: {}", path.display()),
                    Err(error) => error,
                };
                cx.notify();
            });
        })
        .detach();
    }

    fn cancel_export(&mut self, cx: &mut Context<Self>) {
        if let Some(cancel) = &self.render_cancel {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            self.edit_status = "Đang dừng render…".into();
            cx.notify();
        }
    }
    fn save_to(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if self.saving
            || self.autosaving
            || self.recovery.is_some()
            || self.pending_lyrics.is_some()
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
            || self.pending_lyrics.is_some()
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
            || self.pending_lyrics.is_some()
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
        let changed = matches!(&result, Ok(Some(_)));
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
        if changed {
            let text = self
                .selected_word
                .as_ref()
                .and_then(|id| {
                    self.editor
                        .project()
                        .words
                        .iter()
                        .find(|word| &word.id == id)
                })
                .map(|word| word.text.clone())
                .unwrap_or_default();
            self.set_word_input(&text, cx);
        }
        cx.notify();
    }
}

impl Render for Studio {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let credit_input_value = self
            .editor
            .project()
            .settings
            .extra
            .get("title_credit")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        if self.title_credit_input.read(cx).value() != credit_input_value {
            self.title_credit_input.update(cx, |input, cx| {
                input.set_value(&credit_input_value);
                cx.notify();
            });
        }
        let workspace = workspace_size(f32::from(window.viewport_size().width));
        let portrait_width = (workspace.preview * 9. / 16.).min(210.);
        let preview_height = if self.editor.project().settings.aspect
            == vietkara_domain::settings::Aspect::Portrait
        {
            portrait_width * 16. / 9.
        } else {
            workspace.preview * 9. / 16.
        };
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
        let selected_start = selected.map(|word| word.start).unwrap_or(0.);
        let selected_end = selected.map(|word| word.end).unwrap_or(0.);
        let selected_role = selected.map(|word| role_label(&word.role)).unwrap_or("—");
        let selected_section = selected
            .and_then(|word| word.section_name.as_deref())
            .unwrap_or("")
            .to_owned();
        let selected_is_section_start = selected.is_some_and(|word| {
            self.editor
                .project()
                .words
                .iter()
                .find(|item| item.line == word.line)
                .and_then(|item| item.section)
                .unwrap_or(false)
        });
        let selected_scene_exists = self.editor.project().scenes.iter().any(|scene| {
            scene.get("section_name").and_then(Value::as_str) == Some(selected_section.as_str())
        });
        let selected_scene_contrast = self
            .editor
            .project()
            .scenes
            .iter()
            .find(|scene| {
                scene.get("section_name").and_then(Value::as_str) == Some(selected_section.as_str())
            })
            .and_then(|scene| scene.get("contrast_mode"))
            .and_then(Value::as_str)
            .unwrap_or("auto");
        let selected_scene_transition_seconds = self
            .editor
            .project()
            .scenes
            .iter()
            .find(|scene| {
                scene.get("section_name").and_then(Value::as_str) == Some(selected_section.as_str())
            })
            .and_then(|scene| scene.get("transition_seconds"))
            .and_then(Value::as_f64)
            .unwrap_or(0.6);
        let settings = &self.editor.project().settings;
        let display_mode = settings
            .extra
            .get("display_mode")
            .and_then(Value::as_str)
            .unwrap_or("karaoke");
        let title_enabled = settings
            .extra
            .get("title_enabled")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let title_template = settings
            .extra
            .get("title_template")
            .and_then(Value::as_str)
            .unwrap_or("none");
        let title_text = settings
            .extra
            .get("title_text")
            .and_then(Value::as_str)
            .unwrap_or("");
        let title_size = settings
            .extra
            .get("title_size")
            .and_then(Value::as_f64)
            .unwrap_or(105.0);
        let title_y = settings
            .extra
            .get("title_y")
            .and_then(Value::as_f64)
            .unwrap_or(28.0);
        let title_start = settings
            .extra
            .get("title_start")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let title_end = settings
            .extra
            .get("title_end")
            .and_then(Value::as_f64)
            .unwrap_or(8.0);
        let title_color = settings
            .extra
            .get("title_color")
            .and_then(Value::as_str)
            .unwrap_or("#ffffff");
        let subtitle_box = settings
            .extra
            .get("subtitle_box")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let subtitle_box_color = settings
            .extra
            .get("subtitle_box_color")
            .and_then(Value::as_str)
            .unwrap_or("#000000");
        let subtitle_box_opacity = settings
            .extra
            .get("subtitle_box_opacity")
            .and_then(Value::as_f64)
            .unwrap_or(50.0);
        let subtitle_box_radius = settings
            .extra
            .get("subtitle_box_radius")
            .and_then(Value::as_f64)
            .unwrap_or(8.0);
        let singing_mode = settings
            .extra
            .get("singing_mode")
            .and_then(Value::as_str)
            .unwrap_or("solo");
        let role_icons = settings
            .extra
            .get("role_icons")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let countdown = settings
            .extra
            .get("countdown")
            .and_then(Value::as_str)
            .unwrap_or("none");
        let countdown_seconds = settings
            .extra
            .get("countdown_seconds")
            .and_then(Value::as_f64)
            .unwrap_or(4.0);
        let countdown_path = self
            .path
            .as_ref()
            .and_then(|path| path.parent())
            .map(|directory| directory.join("countdown.png"))
            .filter(|path| path.is_file());
        let background_type = settings
            .extra
            .get("background_type")
            .and_then(Value::as_str)
            .unwrap_or("auto");
        let use_background = settings
            .extra
            .get("use_background")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let background_asset = self
            .path
            .as_ref()
            .and_then(|path| path.parent())
            .map(|directory| {
                if background_type == "video" {
                    self.background_video_frame
                        .clone()
                        .unwrap_or_else(|| directory.join("background.jpg"))
                } else {
                    directory.join("background.jpg")
                }
            })
            .filter(|path| path.is_file());
        let background_image = if background_type == "image"
            || background_type == "video"
            || (background_type == "auto" && use_background)
        {
            background_asset.clone()
        } else {
            None
        };
        let logo_path = self
            .path
            .as_ref()
            .and_then(|path| path.parent())
            .map(|directory| {
                let name = settings
                    .extra
                    .get("logo_name")
                    .and_then(Value::as_str)
                    .unwrap_or("logo.png");
                directory.join(name)
            })
            .filter(|path| path.is_file());
        let logo_enabled = settings
            .extra
            .get("logo_enabled")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let logo_size = settings
            .extra
            .get("logo_size")
            .and_then(Value::as_f64)
            .unwrap_or(12.0);
        let logo_x = settings
            .extra
            .get("logo_x")
            .and_then(Value::as_f64)
            .unwrap_or(96.0);
        let logo_y = settings
            .extra
            .get("logo_y")
            .and_then(Value::as_f64)
            .unwrap_or(4.0);
        let logo_opacity = settings
            .extra
            .get("logo_opacity")
            .and_then(Value::as_f64)
            .unwrap_or(100.0);
        let logo_effect = settings
            .extra
            .get("logo_effect")
            .and_then(Value::as_str)
            .unwrap_or("none");
        let background_color = setting_color(settings, "background", 0x101d2b);
        let display_position = settings
            .extra
            .get("position")
            .and_then(Value::as_str)
            .unwrap_or("bottom");
        let display_align = settings
            .extra
            .get("align")
            .and_then(Value::as_str)
            .unwrap_or("center");
        let export_resolution = match settings.resolution {
            vietkara_domain::settings::Resolution::Hd => "1080p",
            vietkara_domain::settings::Resolution::UltraHd => "4K",
        };
        let export_quality = match settings.render_quality {
            vietkara_domain::settings::RenderQuality::Standard => "Tiêu chuẩn",
            vietkara_domain::settings::RenderQuality::High => "Cao",
            vietkara_domain::settings::RenderQuality::Maximum => "Tối đa",
        };
        let export_format = match settings.render_format {
            vietkara_domain::settings::RenderFormat::Mp4 => "MP4",
            vietkara_domain::settings::RenderFormat::Mov => "MOV",
            vietkara_domain::settings::RenderFormat::Mkv => "MKV",
        };
        let export_fps = match settings.render_fps {
            vietkara_domain::settings::RenderFps::Source => "Theo nguồn",
            vietkara_domain::settings::RenderFps::Thirty => "30 FPS",
            vietkara_domain::settings::RenderFps::Sixty => "60 FPS",
        };
        let export_audio = match settings.audio_output {
            vietkara_domain::settings::AudioOutput::Beat => "Beat",
            vietkara_domain::settings::AudioOutput::Guide => "Beat + vocal guide",
        };
        let position_label = match display_position {
            "top" => "Trên",
            "center" => "Giữa",
            _ => "Dưới",
        };
        let align_label = match display_align {
            "left" => "Trái",
            "right" => "Phải",
            "justify" => "Căn theo lane",
            _ => "Giữa",
        };
        let is_portrait = settings.aspect == vietkara_domain::settings::Aspect::Portrait;
        let italic = settings
            .extra
            .get("italic")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let font_size = settings.font_size;
        let sung_color = setting_color(settings, "sung", 0x52efb9);
        let unsung_color = setting_color(settings, "unsung", 0xffffff);
        let duet = settings.extra.get("singing_mode").and_then(Value::as_str) == Some("duet");
        let role_color = |line: u32| {
            let key = display_words
                .iter()
                .find(|word| word.line == line)
                .map(|word| match word.role.as_str() {
                    "male" => "role_a_color",
                    "female" => "role_b_color",
                    "voice_c" => "role_c_color",
                    "voice_d" => "role_d_color",
                    "voice_e" => "role_e_color",
                    "voice_f" => "role_f_color",
                    "unknown" => "role_unknown_color",
                    _ => "role_all_color",
                });
            if duet && display_mode == "karaoke" {
                key.map(|key| setting_color(settings, key, sung_color))
                    .unwrap_or(sung_color)
            } else {
                sung_color
            }
        };
        let reference_position = vietkara_domain::timing::map_time(
            self.play_position,
            &self.editor.project().anchors,
            true,
        );
        let layout = preview::cached_display_plan(
            &mut self.preview_layout,
            self.editor.project(),
            display_words,
            window.text_system(),
        );
        let active_plan_rows = if display_mode == "karaoke" {
            preview::visible_plan_rows(layout, self.play_position, display_mode)
        } else {
            Vec::new()
        };
        let active_lines = if display_mode == "karaoke" {
            active_plan_rows
                .iter()
                .map(|&index| preview::VisibleLine {
                    line: layout.rows[index].line,
                    slot: layout.rows[index].slot,
                })
                .collect()
        } else {
            preview::visible_lines(
                display_words,
                &self.editor.project().anchors,
                self.play_position,
                display_mode,
                duet,
            )
        };
        let selected_id = self.selected_word.as_deref();
        let preview_line_element = |line: u32, size: f32, subdued: bool, line_color: u32| {
            let line_align = preview_alignment(display_align, line, &active_lines);
            div()
                .w_full()
                .flex()
                .items_center()
                .when(line_align == "left", |row| row.justify_start())
                .when(line_align == "center", |row| row.justify_center())
                .when(line_align == "right", |row| row.justify_end())
                .gap_2()
                .children(
                    display_words
                        .iter()
                        .filter(move |word| word.line == line)
                        .map(|word| {
                            let active =
                                reference_position >= word.start && reference_position < word.end;
                            let sung = reference_position >= word.end;
                            let color = if subdued {
                                0xc4ccd5
                            } else if display_mode == "subtitle" {
                                unsung_color
                            } else if display_mode == "lyricsmv" || active || sung {
                                line_color
                            } else if Some(word.id.as_str()) == selected_id {
                                0xffdf79
                            } else {
                                0xffffff
                            };
                            div()
                                .text_size(px(size))
                                .text_color(rgb(color))
                                .when(italic, |text| text.italic())
                                .child(vietkara_render::layout::font_text(
                                    &word.text,
                                    &settings.font,
                                ))
                        }),
                )
        };
        let mut preview_rows: Vec<(u32, f32, bool, u32)> = if display_mode == "karaoke" {
            active_lines
                .iter()
                .map(|row| {
                    (
                        row.line,
                        (font_size as f32 / 3.).clamp(16., 42.),
                        false,
                        role_color(row.line),
                    )
                })
                .collect()
        } else {
            active_lines
                .first()
                .map(|row| {
                    vec![(
                        row.line,
                        if display_mode == "subtitle" { 28. } else { 34. },
                        false,
                        role_color(row.line),
                    )]
                })
                .unwrap_or_default()
        };
        if self.play_duration <= 0.
            && preview_rows.is_empty()
            && let Some(word) = selected
        {
            preview_rows.push((
                word.line,
                (font_size as f32 / 3.).clamp(16., 42.),
                false,
                role_color(word.line),
            ));
        }
        if display_mode == "lyricsmv"
            && settings.extra.get("mv_style").and_then(Value::as_str) != Some("single")
            && let Some(active) = preview_rows.first().copied()
        {
            let mut lines: Vec<(f64, u32)> =
                display_words.iter().fold(Vec::new(), |mut lines, word| {
                    if !lines.iter().any(|(_, line)| *line == word.line) {
                        lines.push((
                            vietkara_domain::timing::map_time(
                                word.start,
                                &self.editor.project().anchors,
                                false,
                            ),
                            word.line,
                        ));
                    }
                    lines
                });
            lines.sort_by(|a, b| a.0.total_cmp(&b.0));
            if let Some(index) = lines.iter().position(|(_, line)| *line == active.0) {
                let count =
                    if settings.extra.get("mv_style").and_then(Value::as_str) == Some("scroll") {
                        2isize
                    } else {
                        1isize
                    };
                let mut stack = Vec::new();
                for offset in -count..=count {
                    let Some(line) = index.checked_add_signed(offset) else {
                        continue;
                    };
                    let Some((_, line_id)) = lines.get(line) else {
                        continue;
                    };
                    if offset == 0 {
                        stack.push((*line_id, 34., false, role_color(*line_id)));
                    } else {
                        stack.push((*line_id, 22., true, role_color(*line_id)));
                    }
                }
                preview_rows = stack;
            }
        }
        let preview_width = if is_portrait {
            portrait_width
        } else {
            workspace.preview
        };
        let preview_scale = preview_width / layout.width as f32;
        let preview_position = self.play_position;
        let scene_preview = self
            .path
            .as_ref()
            .and_then(|path| path.parent())
            .map(|directory| {
                scene_preview_layers(
                    self.editor.project(),
                    directory,
                    preview_position,
                    self.play_duration,
                )
            })
            .unwrap_or_default();
        let title_visible = layout
            .title
            .as_ref()
            .is_some_and(|title| preview_position >= title.start && preview_position < title.end);
        let active_countdown_window = countdown_window_at(
            &self.editor.project().words,
            &self.editor.project().anchors,
            preview_position,
            countdown_seconds,
        );
        let countdown_preview_visible =
            display_mode == "karaoke" && countdown != "none" && active_countdown_window.is_some();
        let scene_backing = self
            .path
            .as_ref()
            .and_then(|path| path.parent())
            .map(|directory| {
                self.editor
                    .project()
                    .scenes
                    .iter()
                    .filter(|scene| {
                        let section = scene
                            .get("section_name")
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        let scene_words = display_words
                            .iter()
                            .filter(|word| word.section_name.as_deref() == Some(section));
                        let (start, end) = scene_words.fold(
                            (f64::INFINITY, f64::NEG_INFINITY),
                            |(start, end), word| {
                                (
                                    start.min(vietkara_domain::timing::map_time(
                                        word.start,
                                        &self.editor.project().anchors,
                                        false,
                                    )),
                                    end.max(vietkara_domain::timing::map_time(
                                        word.end,
                                        &self.editor.project().anchors,
                                        false,
                                    )),
                                )
                            },
                        );
                        start <= preview_position && preview_position <= end
                    })
                    .map(|scene| vietkara_render::scene_readability_opacity(scene, directory))
                    .max()
                    .unwrap_or(0)
            })
            .unwrap_or(0);
        let show_readability = scene_backing > 0
            && !(display_mode == "subtitle"
                && settings
                    .extra
                    .get("subtitle_box")
                    .and_then(Value::as_bool)
                    .unwrap_or(true));
        let mut current_preview: Vec<_> =
            if display_mode == "karaoke" && !active_plan_rows.is_empty() {
                active_plan_rows
                    .iter()
                    .flat_map(|&row_index| {
                        let row = &layout.rows[row_index];
                        let line_color = role_color(row.line);
                        row.words.iter().map(move |planned| {
                            let active =
                                preview_position >= planned.start && preview_position < planned.end;
                            let sung = preview_position >= planned.end;
                            let color = if active || sung {
                                line_color
                            } else if Some(planned.word.id.as_str()) == selected_id {
                                0xffdf79
                            } else {
                                0xffffff
                            };
                            div()
                                .absolute()
                                .left(px(planned.x as f32 * preview_scale))
                                .top(px(row.y as f32 * preview_scale))
                                .text_size(px(row.font_size as f32 * preview_scale))
                                .text_color(rgb(color))
                                .when(italic, |text| text.italic())
                                .child(planned.display_text.clone())
                        })
                    })
                    .collect()
            } else {
                preview_rows
                    .iter()
                    .map(|(line, size, subdued, color)| {
                        preview_line_element(*line, *size, *subdued, *color)
                    })
                    .collect()
            };
        if display_mode == "subtitle" && subtitle_box && !active_plan_rows.is_empty() {
            let row = &layout.rows[active_plan_rows[0]];
            let color = preview_hex_color(subtitle_box_color);
            current_preview.insert(
                0,
                div()
                    .absolute()
                    .left(px(row.x as f32 * preview_scale - 10.0 * preview_scale))
                    .top(px(row.y as f32 * preview_scale - 6.0 * preview_scale))
                    .w(px(row.width as f32 * preview_scale + 20.0 * preview_scale))
                    .h(px(row.height as f32 * preview_scale + 12.0 * preview_scale))
                    .rounded(px(subtitle_box_radius as f32 * preview_scale))
                    .bg(rgb(color))
                    .opacity((subtitle_box_opacity / 100.0) as f32),
            );
        }
        if show_readability {
            for &row_index in &active_plan_rows {
                let row = &layout.rows[row_index];
                let pad = row.font_size as f32 * 0.22 * preview_scale;
                current_preview.insert(
                    0,
                    div()
                        .absolute()
                        .left(px(row.x as f32 * preview_scale - pad))
                        .top(px(row.y as f32 * preview_scale - pad))
                        .w(px(row.width as f32 * preview_scale + pad * 2.))
                        .h(px(row.height as f32 * preview_scale + pad * 2.))
                        .rounded(px(12. * preview_scale))
                        .bg(rgb(0x000000))
                        .opacity(f32::from(scene_backing) / 100.),
                );
            }
        }
        for row in &layout.rows {
            if let Some(marker) = &row.role_marker {
                let symbol = match marker.role.as_str() {
                    "male" => "♂",
                    "female" => "♀",
                    _ => "♫",
                };
                current_preview.push(
                    div()
                        .absolute()
                        .left(px(marker.x as f32 * preview_scale))
                        .top(px(marker.y as f32 * preview_scale))
                        .text_size(px(marker.size as f32 * preview_scale))
                        .text_color(rgb(preview_hex_color(&marker.color)))
                        .child(symbol),
                );
            }
        }
        if let Some(title) = layout.title.as_ref().filter(|_| title_visible) {
            let (motion_x, motion_y, motion_scale, motion_opacity) =
                title_preview_motion(title, preview_position);
            if let Some(box_layout) = &title.template {
                let template = &box_layout.template;
                let last = template.colors.last().unwrap_or(&template.colors[0]);
                let bands = (0..24).map(|index| {
                    let t = index as f64 / 23.0;
                    let color = vietkara_render::blend(&template.colors[0], last, t);
                    div().flex_1().h_full().bg(rgb(preview_hex_color(&color)))
                });
                current_preview.insert(
                    0,
                    div()
                        .absolute()
                        .left(px((box_layout.x as f32 + motion_x) * preview_scale))
                        .top(px((box_layout.y as f32 + motion_y) * preview_scale))
                        .w(px(box_layout.width as f32 * preview_scale))
                        .h(px(box_layout.height as f32 * preview_scale))
                        .flex()
                        .flex_row()
                        .overflow_hidden()
                        .rounded(px(template.radius as f32 * preview_scale))
                        .bg(rgb(preview_hex_color(&template.colors[0])))
                        .border_1()
                        .border_color(rgb(preview_hex_color(&template.border)))
                        .opacity((template.opacity as f32 / 100.0) * motion_opacity)
                        .children(bands),
                );
            }
            for row in &title.rows {
                current_preview.push(
                    div()
                        .absolute()
                        .left(px((row.x as f32 + motion_x) * preview_scale))
                        .top(px((row.y as f32 + motion_y) * preview_scale))
                        .text_size(px(row.size as f32 * preview_scale * motion_scale))
                        .text_color(rgb(preview_hex_color(&title.color)))
                        .font_weight(gpui::FontWeight::BOLD)
                        .when(title.italic, |text| text.italic())
                        .opacity(motion_opacity)
                        .child(row.display_text.clone()),
                );
            }
        }
        let timeline = self.timeline_view(cx);
        let lyric_rows = display_words.iter().map(|line: &Word| {
            let id = line.id.clone();
            let selected = Some(&line.id) == self.selected_word.as_ref();
            let active =
                self.playing && reference_position >= line.start && reference_position < line.end;
            div()
                .cursor_pointer()
                .bg(rgb(if selected {
                    theme::ACCENT_SURFACE
                } else if active {
                    theme::SUCCESS_ACTIVE
                } else {
                    theme::SURFACE
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
                .border_color(rgb(theme::BORDER))
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
            .on_action(cx.listener(|this, _: &ExportVideo, window, cx| this.export_video(window, cx)))
            .on_action(cx.listener(|this, _: &CancelExport, _, cx| this.cancel_export(cx)))
            .on_action(cx.listener(|this, _: &Open, _, cx| this.open(cx)))
            .on_action(cx.listener(|this, _: &ImportSrt, _, cx| this.import_srt(cx)))
            .on_action(cx.listener(|this, _: &ImportLyrics, _, cx| this.import_plain_lyrics(cx)))
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
            .bg(rgb(theme::CANVAS))
            .text_color(rgb(theme::TEXT))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .px_5()
                    .py_3()
                    .bg(rgb(theme::SURFACE))
                    .child(format!(
                        "VietKara Studio  /  {}{}",
                        self.editor.project().name,
                        if self.dirty { " *" } else { "" }
                    ))
                    .child(div().cursor_pointer().px_3().child("Nhập SRT")
                        .on_mouse_down(MouseButton::Left,cx.listener(|this, _, _, cx|this.import_srt(cx))))
                    .child(div().cursor_pointer().px_3().child("Mở project")
                        .on_mouse_down(MouseButton::Left,cx.listener(|this, _, _, cx|this.open(cx))))
                    .child(div().cursor_pointer().px_3().child(if self.rendering {
                        format!("Hủy xuất ({:.0}%)", self.render_progress * 100.0)
                    } else { "Xuất video".into() })
                        .on_mouse_down(MouseButton::Left,cx.listener(|this, _, window, cx| {
                            if this.rendering { this.cancel_export(cx); }
                            else { this.export_video(window,cx); }
                        })))
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
            .when(self.pending_lyrics.is_some(), |root| root.child(div().flex().gap_3().p_3()
                .child("Thay lời sẽ cập nhật toàn bộ từ và mốc hiện tại.")
                .child(div().cursor_pointer().px_3().child("Thay toàn bộ lời")
                    .on_mouse_down(MouseButton::Left,cx.listener(|this, _, _, cx|this.accept_lyrics(cx))))
                .child(div().cursor_pointer().px_3().child("Hủy")
                    .on_mouse_down(MouseButton::Left,cx.listener(|this, _, _, cx|{this.pending_lyrics=None;this.edit_status="Đã hủy nhập lời".into();cx.notify();})))))
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
                            .w(px(workspace.left))
                            .flex()
                            .flex_col()
                            .overflow_hidden()
                            .bg(rgb(theme::SURFACE))
                            .rounded_md()
                            .p_3()
                            .child(div().flex().justify_between().items_center()
                                .child("LỜI BÀI HÁT")
                                .child(div().cursor_pointer().px_2().child("＋ SRT")
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this, _, _, cx|this.import_srt(cx)))))
                            .child(div().flex().mt_3().mb_2().gap_2()
                                .child(div().cursor_pointer().px_2().py_1().rounded_sm()
                                    .bg(rgb(if !self.left_tab_media {theme::ACCENT_SURFACE}else{theme::CANVAS}))
                                    .child("Danh sách")
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|{this.left_tab_media=false;cx.notify();})))
                                .child(div().cursor_pointer().px_2().py_1().rounded_sm()
                                    .bg(rgb(if self.left_tab_media {theme::ACCENT_SURFACE}else{theme::CANVAS}))
                                    .child("Nguồn media")
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|{this.left_tab_media=true;cx.notify();}))))
                            .when(self.left_tab_media,|panel|panel
                                .child(div().mt_2().text_color(rgb(theme::TEXT_MUTED)).child("AUDIO & VIDEO"))
                                .child(div().mt_3().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2()
                                    .child(if self.editor.project().beat.is_some(){"●  Beat gốc · đã nạp"}else{"＋  Nạp beat gốc"})
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.choose_audio(AudioSlot::Beat,cx))))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2()
                                    .child(if self.editor.project().reference.is_some(){"●  Bản hát · đã nạp"}else{"＋  Nạp bản hát / video"})
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.choose_audio(AudioSlot::Reference,cx))))
                                .child(div().mt_3().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2()
                                    .child(if background_asset.is_some() {"▧  Thay ảnh nền"} else {"＋  Nạp ảnh nền"})
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.choose_background_image(cx))))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2()
                                    .child(if background_type == "video" {"▶  Thay video nền"} else {"＋  Nạp video nền"})
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.choose_background_video(cx))))
                                .child(div().mt_4().text_color(rgb(theme::TEXT_MUTED)).child("Project JSON"))
                                .child(div().mt_2().text_size(px(11.)).child(self.path.as_ref().map(|path|path.display().to_string()).unwrap_or_else(||"Chưa lưu project".into()))))
                            .when(!self.left_tab_media,|panel|panel
                                .child(div().text_size(px(11.)).text_color(rgb(theme::TEXT_MUTED))
                                    .child(format!("{} từ · {} dòng",self.editor.project().words.len(),self.editor.project().words.iter().map(|w|w.line).max().map(|line|line+1).unwrap_or(0))))
                                .child(div().id("lyric-list").flex_1().overflow_y_scroll().mt_2().children(lyric_rows))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2()
                                    .child("✎  Ước lượng mốc từ TXT")
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.import_plain_lyrics(cx))))),
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
                                    .bg(rgb(theme::SURFACE))
                                    .rounded_md()
                                    .p_4()
                                    .child(div().flex().justify_between()
                                        .child("XEM TRƯỚC")
                                        .child(format!("{}  ·  {}  ·  {}",if is_portrait {"9:16 · Dọc"} else {"16:9 · Ngang"},settings.font,display_mode)))
                                    .child(
                                        div()
                                            .flex()
                                            .flex_1()
                                            .justify_center()
                                            .items_center()
                                            .child(div()
                                                .flex().flex_col()
                                                .when(display_position == "top", |frame| frame.justify_start())
                                                .when(display_position == "center", |frame| frame.justify_center())
                                                .when(display_position != "top" && display_position != "center", |frame| frame.justify_end())
                                                .when(display_align == "left", |frame| frame.items_start())
                                                .when(display_align == "center" || display_align == "justify", |frame| frame.items_center())
                                                .when(display_align == "right", |frame| frame.items_end())
                                                .w(if is_portrait {px(portrait_width)} else {px(workspace.preview)})
                                                .h(px(preview_height))
                                                .relative().overflow_hidden().rounded_sm().bg(rgb(background_color))
                                                .when(background_image.is_some(), |frame| {
                                                    frame.child(div().absolute().left(px(0.)).top(px(0.)).w_full().h_full()
                                                        .child(img(background_image.clone().unwrap()).size_full().object_fit(ObjectFit::Cover)))
                                                        .child(div().absolute().left(px(0.)).top(px(0.)).w_full().h_full()
                                                            .bg(rgb(0x05080c)).opacity(0.32))
                                                })
                                                .when(!scene_preview.is_empty(), |frame| frame.children(scene_preview.clone().into_iter().map(|layer| {
                                                    let image_width = preview_width * layer.scale / 100.0;
                                                    let image_height = preview_height * layer.scale / 100.0;
                                                    let image_left = (preview_width-image_width)/2.0 + layer.x*(image_width-preview_width)/100.0;
                                                    let image_top = (preview_height-image_height)/2.0 + layer.y*(image_height-preview_height)/100.0;
                                                    div().absolute().left(px(0.)).top(px(0.)).w_full().h_full()
                                                        .when(layer.image.is_some(), |container| container.child(img(layer.image.clone().unwrap()).absolute()
                                                            .w(px(image_width)).h(px(image_height)).object_fit(ObjectFit::Cover)
                                                            .left(px(image_left)).top(px(image_top)).opacity(layer.image_opacity)))
                                                        .when(layer.decoration.is_some(), |container| {
                                                            let (path,size,x,y,opacity) = layer.decoration.clone().unwrap();
                                                            let width = preview_width * size / 100.0;
                                                            container.child(img(path).absolute().w(px(width)).h(px(width*0.65)).object_fit(ObjectFit::Contain)
                                                                .left(px(preview_width*x/100.0-width/2.0))
                                                                .top(px(preview_height*y/100.0-width*0.325)).opacity(opacity))
                                                        })
                                                })))
                                                .child(div().absolute().left(px(8.)).top(px(6.)).text_size(px(12.)).text_color(rgb(0x8190a3)).child("PREVIEW · KHUNG VIDEO"))
                                                .child(div().absolute().left(px(0.)).top(px(0.)).w_full().h_full()
                                                    .flex().flex_col().gap_2()
                                                    .when(display_position == "top", |rows| rows.justify_start())
                                                    .when(display_position == "center", |rows| rows.justify_center())
                                                    .when(display_position != "top" && display_position != "center", |rows| rows.justify_end())
                                                    .when(display_align == "left", |rows| rows.items_start())
                                                    .when(display_align == "center" || display_align == "justify", |rows| rows.items_center())
                                                    .when(display_align == "right", |rows| rows.items_end())
                                                    .children(current_preview)
                                                    .when(logo_enabled && logo_path.is_some(), |rows| {
                                                        let side = (workspace.preview * (logo_size as f32 / 100.0)).clamp(18.0, workspace.preview * 0.4);
                                                        let float_offset = if logo_effect == "float" { 8.0 * (std::f64::consts::PI * preview_position / 2.0).sin() as f32 } else { 0.0 };
                                                        let pulse = if logo_effect == "pulse" { 0.8 + 0.2 * (std::f64::consts::PI * preview_position / 2.0).sin().abs() as f32 } else { 1.0 };
                                                        let fade = if logo_effect == "fade" { (preview_position / 1.0).clamp(0.0,1.0) as f32 } else { 1.0 };
                                                        rows.child(img(logo_path.clone().unwrap()).absolute()
                                                            .w(px(side)).h(px(side * 0.65)).object_fit(ObjectFit::Contain)
                                                            .left(px((workspace.preview - side) * logo_x as f32 / 100.0))
                                                            .top(px((preview_height * logo_y as f32 / 100.0 + float_offset).clamp(0.0,preview_height - side * 0.65)))
                                                            .opacity((logo_opacity / 100.0) as f32 * pulse * fade))
                                                    })
                                                    .when(countdown_preview_visible, |rows| {
                                                        let glyph = match countdown { "mic"=>"♫", "star"=>"★", "heart"=>"♥", "note"=>"♪", "moon"=>"☾", _=>"" };
                                                        let (_, window_end) = active_countdown_window.unwrap();
                                                        let remaining = (window_end - preview_position).ceil().clamp(1.0, 4.0) as usize;
                                                        rows.child(div().absolute().left(px(preview_width * 0.05)).top(px(preview_height * 0.28)).flex().gap_2()
                                                            .children((0..remaining).map(|_| {
                                                                div().w(px(22.0)).h(px(22.0)).flex().items_center().justify_center()
                                                                    .when(countdown == "image" && countdown_path.is_some(), |box_| box_.child(img(countdown_path.clone().unwrap()).size_full().object_fit(ObjectFit::Contain)))
                                                                    .when(countdown != "image", |box_| box_.text_size(px(22.0)).text_color(rgb(0xf4cf72)).child(glyph))
                                                            })))
                                                    })
                                                .when(preview_rows.is_empty() && !title_visible,|empty|empty
                                                        .child(div().text_size(px(15.)).text_color(rgb(theme::TEXT_MUTED)).child(if display_words.is_empty(){"Nạp lời để xem trước karaoke"}else{"Tua đến câu để xem trước"}))))),
                                    ),
                            )
                            .child(
                                div()
                                    .h(px(390.))
                                    .bg(rgb(theme::SURFACE))
                                    .rounded_md()
                                    .p_3()
                                    .child(timeline)
                                    .child(div().flex().justify_between().items_center().mt_2()
                                        .child("TIMELINE · Kéo khối để chọn, kéo mép để chỉnh mốc")
                                        .child(div().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_1()
                                            .child(if self.editor.project().settings.timeline_linked {"Mốc liên kết"}else{"Mốc độc lập"})
                                            .on_mouse_down(MouseButton::Left,cx.listener(|this, _, _, cx|this.toggle_linked(cx))))),
                            ),
                    )
                    .child(
                        div()
                            .w(px(workspace.right))
                            .flex()
                            .flex_col()
                            .id("design-inspector")
                            .overflow_y_scroll()
                            .bg(rgb(theme::SURFACE))
                            .rounded_md()
                            .p_3()
                            .child(div().flex().gap_2().mb_2()
                                .child(div().cursor_pointer().px_2().py_1().rounded_sm()
                                    .bg(rgb(if !self.inspector_design {theme::ACCENT_SURFACE}else{theme::CANVAS}))
                                    .child("Từ")
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|{
                                        this.inspector_design=false;
                                        let text=this.selected_word.as_ref().and_then(|id|this.editor.project().words.iter().find(|word|&word.id==id)).map(|word|word.text.clone()).unwrap_or_default();
                                        this.set_word_input(&text,cx);
                                        cx.notify();
                                    })))
                                .child(div().cursor_pointer().px_2().py_1().rounded_sm()
                                    .bg(rgb(if self.inspector_design {theme::ACCENT_SURFACE}else{theme::CANVAS}))
                                    .child("Thiết kế")
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|{
                                        this.inspector_design=true;
                                        let text=this.editor.project().settings.extra.get("title_text").and_then(Value::as_str).unwrap_or("").to_owned();
                                        this.set_word_input(&text,cx);
                                        cx.notify();
                                    }))))
                            .when(!self.inspector_design,|panel|panel
                            .child("TỪ ĐANG CHỌN")
                            .child(div().mt_3().child("Lời bài hát"))
                            .child(
                                div()
                                    .mt_2()
                                    .child(self.word_input.clone()),
                            )
                            .child(div().mt_2().flex().gap_2()
                                .child(div().flex_1().cursor_pointer().rounded_sm().bg(rgb(theme::SUCCESS)).px_2().py_2()
                                    .child("Áp dụng chữ")
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.apply_word_text(cx))))
                                .child(div().flex_1().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2()
                                    .child(format!("Vai: {selected_role}"))
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.cycle_selected_role(cx)))))
                            .child(div().mt_4().text_size(px(11.)).text_color(rgb(theme::TEXT_MUTED)).child("SECTION / SCENE"))
                            .child(div().mt_1().text_color(rgb(theme::TEXT_SUBTLE)).child(if selected_section.is_empty() {"Chưa đặt tên section".to_owned()} else {selected_section.clone()}))
                            .child(div().mt_2().child(self.section_name_input.clone()))
                            .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2()
                                .child("Đặt tên cho cả dòng")
                                .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.apply_section_name(cx))))
                            .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2()
                                .child("Chọn loại section scene tiếp theo")
                                .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.set_selected_scene_section(cx))))
                            .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::SUCCESS)).px_2().py_2()
                                .child(if selected_scene_exists {"Thay ảnh scene cho section"} else {"＋  Nạp ảnh scene cho section"})
                                .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.choose_scene_image(cx))))
                            .when(selected_scene_exists, |panel| panel
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2()
                                    .child(if self.editor.project().scenes.iter().find(|scene| scene.get("section_name").and_then(Value::as_str) == Some(selected_section.as_str())).and_then(|scene| scene.get("transition")).and_then(Value::as_str) == Some("cut") {"Chuyển cảnh: cắt thẳng"} else {"Chuyển cảnh: mờ 0,6 giây"})
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.cycle_selected_scene_option("transition", &["fade","cut"],cx))))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2()
                                    .child("Đổi chế độ tương phản lời")
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.cycle_selected_scene_option("contrast_mode", &["auto","always","off"],cx))))
                                .child(div().mt_1().text_size(px(11.)).text_color(rgb(theme::TEXT_MUTED)).child(match selected_scene_contrast {"always"=>"Tương phản: luôn bật", "off"=>"Tương phản: tắt", _=>"Tương phản: tự động theo ảnh"}))
                                .when(self.editor.project().scenes.iter().find(|scene| scene.get("section_name").and_then(Value::as_str) == Some(selected_section.as_str())).and_then(|scene| scene.get("transition")).and_then(Value::as_str) != Some("cut"), |panel| panel
                                    .child(div().mt_2().child(format!("Thời lượng mờ: {:.1}s",selected_scene_transition_seconds))
                                        .child(div().flex().gap_3()
                                            .child(div().cursor_pointer().child("−").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_selected_scene_number("transition_seconds",-0.2,0.0,3.0,cx))))
                                            .child(div().cursor_pointer().child("+").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_selected_scene_number("transition_seconds",0.2,0.0,3.0,cx)))))))
                                .child(div().mt_2().child("Keyframe đầu")
                                    .child(div().flex().gap_2()
                                        .child(div().flex_1().cursor_pointer().child("− Zoom").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_selected_scene_frame(0,"scale",-5.0,cx))))
                                        .child(div().flex_1().cursor_pointer().child("+ Zoom").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_selected_scene_frame(0,"scale",5.0,cx)))))
                                    .child(div().flex().gap_2()
                                        .child(div().flex_1().cursor_pointer().child("←").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_selected_scene_frame(0,"x",-5.0,cx))))
                                        .child(div().flex_1().cursor_pointer().child("→").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_selected_scene_frame(0,"x",5.0,cx))))
                                        .child(div().flex_1().cursor_pointer().child("↑").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_selected_scene_frame(0,"y",-5.0,cx))))
                                        .child(div().flex_1().cursor_pointer().child("↓").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_selected_scene_frame(0,"y",5.0,cx))))))
                                .child(div().mt_2().child("Keyframe cuối")
                                    .child(div().flex().gap_2()
                                        .child(div().flex_1().cursor_pointer().child("− Zoom").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_selected_scene_frame(1,"scale",-5.0,cx))))
                                        .child(div().flex_1().cursor_pointer().child("+ Zoom").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_selected_scene_frame(1,"scale",5.0,cx)))))
                                    .child(div().flex().gap_2()
                                        .child(div().flex_1().cursor_pointer().child("←").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_selected_scene_frame(1,"x",-5.0,cx))))
                                        .child(div().flex_1().cursor_pointer().child("→").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_selected_scene_frame(1,"x",5.0,cx))))
                                        .child(div().flex_1().cursor_pointer().child("↑").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_selected_scene_frame(1,"y",-5.0,cx))))
                                        .child(div().flex_1().cursor_pointer().child("↓").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_selected_scene_frame(1,"y",5.0,cx))))))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::SUCCESS)).px_2().py_2().child("＋  Nạp/thay ảnh trang trí")
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.choose_scene_decoration(cx))))
                                .child(div().mt_2().flex().gap_2()
                                    .child(div().flex_1().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2().child("Trang trí −")
                                        .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_selected_scene_number("decoration_size",-2.0,5.0,80.0,cx))))
                                    .child(div().flex_1().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2().child("Trang trí +")
                                        .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_selected_scene_number("decoration_size",2.0,5.0,80.0,cx)))))
                                .child(div().mt_2().child("Keyframe trang trí đầu")
                                    .child(div().flex().gap_2()
                                        .child(div().flex_1().cursor_pointer().child("←").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_selected_decoration_frame(0,"x",-5.0,cx))))
                                        .child(div().flex_1().cursor_pointer().child("→").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_selected_decoration_frame(0,"x",5.0,cx))))
                                        .child(div().flex_1().cursor_pointer().child("↑").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_selected_decoration_frame(0,"y",-5.0,cx))))
                                        .child(div().flex_1().cursor_pointer().child("↓").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_selected_decoration_frame(0,"y",5.0,cx))))
                                        .child(div().flex_1().cursor_pointer().child("mờ −").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_selected_decoration_frame(0,"opacity",-10.0,cx))))
                                        .child(div().flex_1().cursor_pointer().child("mờ +").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_selected_decoration_frame(0,"opacity",10.0,cx))))))
                                .child(div().mt_2().child("Keyframe trang trí cuối")
                                    .child(div().flex().gap_2()
                                        .child(div().flex_1().cursor_pointer().child("←").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_selected_decoration_frame(1,"x",-5.0,cx))))
                                        .child(div().flex_1().cursor_pointer().child("→").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_selected_decoration_frame(1,"x",5.0,cx))))
                                        .child(div().flex_1().cursor_pointer().child("↑").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_selected_decoration_frame(1,"y",-5.0,cx))))
                                        .child(div().flex_1().cursor_pointer().child("↓").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_selected_decoration_frame(1,"y",5.0,cx))))
                                        .child(div().flex_1().cursor_pointer().child("mờ −").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_selected_decoration_frame(1,"opacity",-10.0,cx))))
                                        .child(div().flex_1().cursor_pointer().child("mờ +").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_selected_decoration_frame(1,"opacity",10.0,cx))))))
                                .child(div().mt_2().flex().gap_2()
                                    .child(div().flex_1().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2().child("Scene mờ −")
                                        .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_selected_scene_number("opacity",-5.0,0.0,100.0,cx))))
                                    .child(div().flex_1().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2().child("Scene mờ +")
                                        .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_selected_scene_number("opacity",5.0,0.0,100.0,cx)))))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(0x302020)).px_2().py_2().child("Bỏ ảnh trang trí")
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.remove_scene_decoration(cx)))))
                            .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2()
                                .child(if selected_is_section_start {"✓  Đây là đầu section"} else {"＋  Bắt đầu section tại dòng này"})
                                .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.toggle_section_boundary(cx))))
                            .child(div().mt_4().child(format!("Bắt đầu {selected_start:.3}s")))
                            .child(div().mt_2().child(format!("Kết thúc {selected_end:.3}s")))
                            .children([(true,-0.05,"Đầu −50ms"),(true,0.05,"Đầu +50ms"),(false,-0.05,"Cuối −50ms"),(false,0.05,"Cuối +50ms")].into_iter().map(|(start,delta,label)|
                                div().mt_2().cursor_pointer().child(label)
                                    .on_mouse_down(MouseButton::Left,cx.listener(move |this, _, _, cx|this.adjust_word(start,delta,cx)))))
                            .child(div().mt_6().child("Space để căn lời theo nhạc")))
                            .when(self.inspector_design,|panel|panel
                                .child("THIẾT KẾ LỜI")
                                .child(div().mt_3().text_size(px(11.)).text_color(rgb(theme::TEXT_MUTED)).child("CHẾ ĐỘ HIỂN THỊ"))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2()
                                    .child(match display_mode {"lyricsmv"=>"Lời nổi bật theo câu", "subtitle"=>"Phụ đề", _=>"Karaoke · quét từng từ"})
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.cycle_setting("display_mode",&["karaoke","lyricsmv","subtitle"],cx))))
                                .when(display_mode == "subtitle",|panel|panel
                                    .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2()
                                        .child(if subtitle_box {"✓  Hộp nền phụ đề"} else {"Hộp nền phụ đề đang tắt"})
                                        .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|{
                                            let enabled=this.editor.project().settings.extra.get("subtitle_box").and_then(Value::as_bool).unwrap_or(true);
                                            this.update_setting("subtitle_box",Value::Bool(!enabled),cx);
                                        })))
                                    .when(subtitle_box,|panel|panel
                                        .child(div().mt_2().child(format!("Độ mờ nền: {subtitle_box_opacity:.0}%"))
                                            .child(div().flex().gap_3()
                                                .child(div().cursor_pointer().child("−").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_title_number("subtitle_box_opacity",-5.0,0.0,100.0,cx))))
                                                .child(div().cursor_pointer().child("+").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_title_number("subtitle_box_opacity",5.0,0.0,100.0,cx))))))
                                        .child(div().mt_2().child(format!("Bo góc: {subtitle_box_radius:.0}px"))
                                            .child(div().flex().gap_3()
                                                .child(div().cursor_pointer().child("−").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_title_number("subtitle_box_radius",-2.0,0.0,40.0,cx))))
                                                .child(div().cursor_pointer().child("+").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_title_number("subtitle_box_radius",2.0,0.0,40.0,cx))))))
                                        .child(div().mt_2().flex().gap_2()
                                            .children([("Đen","#000000"),("Xanh đêm","#101d2b"),("Tím","#221c35")].into_iter().map(|(label,color)|
                                                div().flex_1().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2().child(label)
                                                    .on_mouse_down(MouseButton::Left,cx.listener(move |this,_,_,cx|this.update_setting("subtitle_box_color",Value::String(color.into()),cx))))))))
                                .child(div().mt_3().text_size(px(11.)).text_color(rgb(theme::TEXT_MUTED)).child("KHUNG HÌNH"))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2()
                                    .child(if is_portrait{"9:16 · Dọc"}else{"16:9 · Ngang"})
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|{
                                        let next=if this.editor.project().settings.aspect==vietkara_domain::settings::Aspect::Portrait {vietkara_domain::settings::Aspect::Landscape} else {vietkara_domain::settings::Aspect::Portrait};
                                        let mut settings=this.editor.project().settings.clone();settings.aspect=next;
                                        this.edit_status=match this.editor.execute(Command::SetSettings(settings)){Ok(_)=>{this.dirty=true;this.autosave_pending=true;"Đã đổi khung hình • chưa lưu".into()},Err(error)=>error.to_string()};cx.notify();
                                    })))
                                .child(div().mt_3().text_size(px(11.)).text_color(rgb(theme::TEXT_MUTED)).child("PHÔNG CHỮ"))
                                .child(div().mt_2().flex().justify_between().items_center().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2()
                                    .child(settings.font.clone())
                                    .child(div().flex().gap_3()
                                        .child(div().cursor_pointer().child("−").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.font_size(-1.,cx))))
                                        .child(div().cursor_pointer().child("+").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.font_size(1.,cx))))))
                                .child(div().mt_2().text_size(px(11.)).text_color(rgb(theme::TEXT_MUTED)).child(format!("{} px · Tiếng Việt: Trường xưa, nắng mới",settings.font_size)))
                                .child(div().mt_3().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2()
                                    .child(if italic{"✓  Chữ nghiêng"}else{"Chữ nghiêng"})
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|{let v=!this.editor.project().settings.extra.get("italic").and_then(Value::as_bool).unwrap_or(false);this.update_setting("italic",Value::Bool(v),cx);})))
                                .child(div().mt_3().text_size(px(11.)).text_color(rgb(theme::TEXT_MUTED)).child("NỀN KHUNG HÌNH"))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2()
                                    .child(match background_type {"image"=>"Đang dùng ảnh nền","video"=>"Đang dùng video nền",_=>"Đang dùng nền màu"})
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.cycle_setting("background_type",&["color","image"],cx))))
                                .child(div().mt_2().flex().gap_2()
                                    .children([("Xanh đêm","#101d2b"),("Đen","#090c12"),("Tím","#221c35")].into_iter().map(|(label,color)|
                                        div().flex_1().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2()
                                            .child(div().flex().gap_1().items_center()
                                                .child(div().w(px(10.)).h(px(10.)).rounded_sm().bg(rgb(match color {"#090c12"=>0x090c12,"#221c35"=>0x221c35,_=>0x101d2b})))
                                                .child(label))
                                            .on_mouse_down(MouseButton::Left,cx.listener(move |this,_,_,cx|this.update_setting("background",Value::String(color.into()),cx))))))
                                .child(div().mt_4().text_size(px(11.)).text_color(rgb(theme::TEXT_MUTED)).child("LOGO"))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2()
                                    .child(if logo_path.is_some() {"＋  Thay logo PNG"} else {"＋  Nạp logo PNG"})
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.choose_logo(cx))))
                                .when(logo_path.is_some(),|panel|panel
                                    .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2()
                                        .child(if logo_enabled {"✓  Logo hiển thị"} else {"Logo đang ẩn"})
                                        .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|{
                                            let enabled=this.editor.project().settings.extra.get("logo_enabled").and_then(Value::as_bool).unwrap_or(false);
                                            this.update_setting("logo_enabled",Value::Bool(!enabled),cx);
                                        })))
                                    .child(div().mt_2().flex().justify_between().items_center().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2()
                                        .child(format!("Kích thước: {logo_size:.0}%"))
                                        .child(div().flex().gap_3()
                                            .child(div().cursor_pointer().child("−").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_title_number("logo_size",-1.0,3.0,40.0,cx))))
                                            .child(div().cursor_pointer().child("+").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_title_number("logo_size",1.0,3.0,40.0,cx))))))
                                    .child(div().mt_2().flex().justify_between().items_center().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2()
                                        .child(format!("Vị trí: {logo_x:.0}%, {logo_y:.0}%"))
                                        .child(div().flex().gap_3()
                                            .child(div().cursor_pointer().child("←").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_title_number("logo_x",-2.0,0.0,100.0,cx))))
                                            .child(div().cursor_pointer().child("→").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_title_number("logo_x",2.0,0.0,100.0,cx))))
                                            .child(div().cursor_pointer().child("↑").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_title_number("logo_y",-2.0,0.0,100.0,cx))))
                                            .child(div().cursor_pointer().child("↓").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_title_number("logo_y",2.0,0.0,100.0,cx))))))
                                    .child(div().mt_2().flex().justify_between().items_center().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2()
                                        .child(format!("Độ mờ: {logo_opacity:.0}%"))
                                        .child(div().flex().gap_3()
                                            .child(div().cursor_pointer().child("−").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_title_number("logo_opacity",-10.0,0.0,100.0,cx))))
                                            .child(div().cursor_pointer().child("+").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_title_number("logo_opacity",10.0,0.0,100.0,cx))))))
                                    .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2()
                                        .child(format!("Hiệu ứng: {logo_effect}"))
                                        .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.cycle_setting("logo_effect",&["none","fade","pulse","float"],cx)))))
                                .child(div().mt_3().text_size(px(11.)).text_color(rgb(theme::TEXT_MUTED)).child("VỊ TRÍ LỜI"))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2()
                                    .child(format!("Vị trí: {position_label}"))
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.cycle_setting("position",&["bottom","center","top"],cx))))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2()
                                    .child(format!("Căn lề: {align_label}"))
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.cycle_setting("align",&["center","left","right","justify"],cx))))
                                .child(div().mt_4().text_size(px(11.)).text_color(rgb(theme::TEXT_MUTED)).child("DUET · ROLE MARKER"))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2()
                                    .child(if singing_mode == "duet" {"✓  Hai lane duet"} else {"Solo · một lane"})
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.cycle_setting("singing_mode",&["solo","duet"],cx))))
                                .when(singing_mode == "duet",|panel|panel.child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2()
                                    .child(if role_icons {"✓  Hiện biểu tượng vai hát"} else {"Ẩn biểu tượng vai hát"})
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|{
                                        let enabled=this.editor.project().settings.extra.get("role_icons").and_then(Value::as_bool).unwrap_or(true);
                                        this.update_setting("role_icons",Value::Bool(!enabled),cx);
                                    }))))
                                .child(div().mt_4().text_size(px(11.)).text_color(rgb(theme::TEXT_MUTED)).child("COUNTDOWN TRƯỚC LỜI"))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2()
                                    .child(format!("Biểu tượng: {countdown}"))
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|{
                                        let current=this.editor.project().settings.extra.get("countdown").and_then(Value::as_str).unwrap_or("none");
                                        let values=["none","mic","star","heart","note","moon","image"];
                                        let next=values.iter().position(|value|*value==current).map(|index|(index+1)%values.len()).unwrap_or(0);
                                        this.update_setting("countdown",Value::String(values[next].into()),cx);
                                    })))
                                .when(countdown == "image",|panel|panel.child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2()
                                    .child(if countdown_path.is_some() {"＋  Thay ảnh countdown PNG"} else {"＋  Nạp ảnh countdown PNG"})
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.choose_countdown_image(cx)))))
                                .when(countdown != "none",|panel|panel.child(div().mt_2().flex().justify_between().items_center().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2()
                                    .child(format!("Đếm ngược: {countdown_seconds:.0} giây"))
                                    .child(div().flex().gap_3()
                                        .child(div().cursor_pointer().child("−").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_title_number("countdown_seconds",-1.0,1.0,8.0,cx))))
                                        .child(div().cursor_pointer().child("+").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_title_number("countdown_seconds",1.0,1.0,8.0,cx)))))))
                                .child(div().mt_3().text_size(px(11.)).text_color(rgb(theme::TEXT_MUTED)).child("MÀU CHỮ ĐÃ HÁT"))
                                .child(div().mt_2().flex().gap_2()
                                    .children([("Xanh ngọc","#52efb9"),("Vàng","#f4cf72"),("Hồng","#ff87b7")].into_iter().map(|(label,color)|
                                        div().flex_1().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2()
                                            .child(div().flex().gap_1().items_center()
                                                .child(div().w(px(10.)).h(px(10.)).rounded_sm().bg(rgb(match color {"#f4cf72"=>0xf4cf72,"#ff87b7"=>0xff87b7,_=>0x52efb9})))
                                                .child(label))
                                            .on_mouse_down(MouseButton::Left,cx.listener(move |this,_,_,cx|this.update_setting("sung",Value::String(color.into()),cx))))))
                                .child(div().mt_4().text_size(px(11.)).text_color(rgb(theme::TEXT_MUTED)).child("TITLE CARD"))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2()
                                    .child(if title_enabled {"✓  Title card đang bật"} else {"＋  Dùng tên project làm title"})
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|{
                                        let enabled=this.editor.project().settings.extra.get("title_enabled").and_then(Value::as_bool).unwrap_or(false);
                                        if !enabled && this.editor.project().settings.extra.get("title_text").and_then(Value::as_str).unwrap_or("").trim().is_empty() {
                                            let project_name=this.editor.project().name.clone();
                                            this.set_word_input(&project_name,cx);
                                            this.update_setting("title_text",Value::String(project_name),cx);
                                        } else if !enabled {
                                            let title=this.editor.project().settings.extra.get("title_text").and_then(Value::as_str).unwrap_or("").to_owned();
                                            this.set_word_input(&title,cx);
                                        }
                                        this.update_setting("title_enabled",Value::Bool(!enabled),cx);
                                    })))
                                .when(title_enabled,|panel|panel
                                    .child(div().mt_2().child(self.word_input.clone()))
                                    .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::SUCCESS)).px_2().py_2()
                                        .child("Áp dụng title")
                                        .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.apply_title_text(cx))))
                                    .child(div().mt_1().text_size(px(11.)).text_color(rgb(theme::TEXT_SUBTLE)).child(if title_text.is_empty(){"Chưa có nội dung title".to_owned()}else{title_text.to_owned()}))
                                    .child(div().mt_3().child("Credit / nghệ sĩ"))
                                    .child(div().mt_2().child(self.title_credit_input.clone()))
                                    .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::SUCCESS)).px_2().py_2()
                                        .child("Áp dụng credit")
                                        .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.apply_title_credit(cx))))
                                    .child(div().mt_3().flex().justify_between().items_center().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2()
                                        .child(format!("Cỡ title: {title_size:.0} px"))
                                        .child(div().flex().gap_3()
                                            .child(div().cursor_pointer().child("−").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_title_number("title_size",-5.0,30.0,220.0,cx))))
                                            .child(div().cursor_pointer().child("+").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_title_number("title_size",5.0,30.0,220.0,cx))))))
                                    .child(div().mt_2().flex().justify_between().items_center().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2()
                                        .child(format!("Vị trí dọc: {title_y:.0}%"))
                                        .child(div().flex().gap_3()
                                            .child(div().cursor_pointer().child("−").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_title_number("title_y",-2.0,5.0,85.0,cx))))
                                            .child(div().cursor_pointer().child("+").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_title_number("title_y",2.0,5.0,85.0,cx))))))
                                    .child(div().mt_2().flex().justify_between().items_center().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2()
                                        .child(format!("Thời gian: {title_start:.1}–{title_end:.1}s"))
                                        .child(div().flex().gap_3()
                                            .child(div().cursor_pointer().child("Đầu −").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_title_number("title_start",-0.5,0.0,86399.0,cx))))
                                            .child(div().cursor_pointer().child("Đầu +").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_title_number("title_start",0.5,0.0,86399.0,cx))))
                                            .child(div().cursor_pointer().child("Cuối −").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_title_number("title_end",-0.5,0.1,86400.0,cx))))
                                            .child(div().cursor_pointer().child("Cuối +").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_title_number("title_end",0.5,0.1,86400.0,cx))))))
                                    .child(div().mt_3().child(format!("Màu chữ: {title_color}")))
                                    .child(div().mt_2().flex().gap_2()
                                        .children([("Trắng","#ffffff"),("Vàng","#f4cf72"),("Xanh ngọc","#52efb9"),("Hồng","#ff87b7")].into_iter().map(|(label,color)|
                                            div().flex_1().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2()
                                                .child(div().flex().gap_1().items_center()
                                                    .child(div().w(px(10.)).h(px(10.)).rounded_sm().bg(rgb(match color {"#f4cf72"=>0xf4cf72,"#52efb9"=>0x52efb9,"#ff87b7"=>0xff87b7,_=>0xffffff})))
                                                    .child(label))
                                                .on_mouse_down(MouseButton::Left,cx.listener(move |this,_,_,cx|this.update_setting("title_color",Value::String(color.into()),cx))))))
                                    .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2()
                                        .child(format!("Template: {title_template}"))
                                        .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.cycle_setting("title_template",&["none","gradient","midnight","crystal","neon","retro","pastel"],cx))))
                                    .child(div().mt_2().flex().gap_2()
                                        .children([("Vào","title_enter"),("Ra","title_exit")].into_iter().map(|(label,key)|{
                                            let current=this_setting_label(settings,key);
                                            div().flex_1().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2()
                                                .child(format!("{label}: {current}"))
                                                .on_mouse_down(MouseButton::Left,cx.listener(move |this,_,_,cx|this.cycle_setting(key,&["none","fade","slide_up","slide_down","slide_left","slide_right","zoom","flip"],cx)))
                                        })))
                                )
                                .child(div().mt_4().text_size(px(11.)).text_color(rgb(theme::TEXT_MUTED)).child("XUẤT VIDEO"))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2()
                                    .child(format!("Định dạng: {export_format}"))
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.update_typed_settings(|settings|settings.render_format=match settings.render_format {vietkara_domain::settings::RenderFormat::Mp4=>vietkara_domain::settings::RenderFormat::Mov,vietkara_domain::settings::RenderFormat::Mov=>vietkara_domain::settings::RenderFormat::Mkv,vietkara_domain::settings::RenderFormat::Mkv=>vietkara_domain::settings::RenderFormat::Mp4},cx))))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2()
                                    .child(format!("Độ phân giải: {export_resolution}"))
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.update_typed_settings(|settings|settings.resolution=match settings.resolution {vietkara_domain::settings::Resolution::Hd=>vietkara_domain::settings::Resolution::UltraHd,vietkara_domain::settings::Resolution::UltraHd=>vietkara_domain::settings::Resolution::Hd},cx))))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2()
                                    .child(format!("Chất lượng: {export_quality}"))
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.update_typed_settings(|settings|settings.render_quality=match settings.render_quality {vietkara_domain::settings::RenderQuality::Standard=>vietkara_domain::settings::RenderQuality::High,vietkara_domain::settings::RenderQuality::High=>vietkara_domain::settings::RenderQuality::Maximum,vietkara_domain::settings::RenderQuality::Maximum=>vietkara_domain::settings::RenderQuality::Standard},cx))))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2()
                                    .child(format!("Tốc độ khung hình: {export_fps}"))
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.update_typed_settings(|settings|settings.render_fps=match settings.render_fps {vietkara_domain::settings::RenderFps::Source=>vietkara_domain::settings::RenderFps::Thirty,vietkara_domain::settings::RenderFps::Thirty=>vietkara_domain::settings::RenderFps::Sixty,vietkara_domain::settings::RenderFps::Sixty=>vietkara_domain::settings::RenderFps::Source},cx))))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2()
                                    .child(format!("Âm thanh: {export_audio}"))
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.update_typed_settings(|settings|settings.audio_output=match settings.audio_output {vietkara_domain::settings::AudioOutput::Beat=>vietkara_domain::settings::AudioOutput::Guide,vietkara_domain::settings::AudioOutput::Guide=>vietkara_domain::settings::AudioOutput::Beat},cx))))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2()
                                    .child(if settings.audio_normalize {"✓  Chuẩn hóa âm thanh (-16 LUFS)"} else {"Chuẩn hóa âm thanh: tắt"})
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.update_typed_settings(|settings|settings.audio_normalize = !settings.audio_normalize,cx))))
                                .child(div().mt_3().text_size(px(11.)).text_color(rgb(theme::TEXT_SUBTLE)).child("Cài đặt được lưu cùng project."))),
                    ),
            )
            .child(
                div()
                    .flex()
                    .justify_between()
                    .px_5()
                    .py_3()
                    .bg(rgb(theme::SURFACE))
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

fn setting_color(settings: &vietkara_domain::settings::Settings, key: &str, fallback: u32) -> u32 {
    settings
        .extra
        .get(key)
        .and_then(Value::as_str)
        .and_then(|value| value.strip_prefix('#'))
        .and_then(|value| u32::from_str_radix(value, 16).ok())
        .unwrap_or(fallback)
}

fn this_setting_label(settings: &vietkara_domain::settings::Settings, key: &str) -> &'static str {
    match settings
        .extra
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("fade")
    {
        "none" => "Không",
        "slide_up" => "Trượt lên",
        "slide_down" => "Trượt xuống",
        "slide_left" => "Trượt trái",
        "slide_right" => "Trượt phải",
        "zoom" => "Thu/phóng",
        "flip" => "Lật",
        _ => "Mờ dần",
    }
}

fn preview_alignment(
    align: &str,
    line: u32,
    active_lines: &[preview::VisibleLine],
) -> &'static str {
    match align {
        "left" => "left",
        "right" => "right",
        "justify" => active_lines
            .iter()
            .find(|row| row.line == line)
            .map(|row| if row.slot == 0 { "left" } else { "right" })
            .unwrap_or("left"),
        _ => "center",
    }
}

fn preview_hex_color(value: &str) -> u32 {
    u32::from_str_radix(value.strip_prefix('#').unwrap_or(value), 16).unwrap_or(0xffffff)
}

fn interpolate_scene_keyframes(frames: &Value, key: &str, progress: f64, default: f64) -> f64 {
    let read = |index: usize| {
        frames
            .as_array()
            .and_then(|frames| frames.get(index))
            .and_then(|frame| frame.get(key))
            .and_then(Value::as_f64)
            .filter(|value| value.is_finite())
            .unwrap_or(default)
    };
    read(0) + (read(1) - read(0)) * progress.clamp(0.0, 1.0)
}

fn scene_fade_opacity(progress: f64, duration: f64, fade_seconds: f64) -> f32 {
    if !duration.is_finite() || duration <= 0.0 || !fade_seconds.is_finite() || fade_seconds <= 0.0
    {
        return 1.0;
    }
    let fade = fade_seconds.min(duration / 2.0).max(0.0);
    if fade == 0.0 {
        return 1.0;
    }
    let time = progress.clamp(0.0, 1.0) * duration;
    (time / fade)
        .clamp(0.0, 1.0)
        .min(((duration - time) / fade).clamp(0.0, 1.0)) as f32
}

fn background_video_preview_path(directory: &std::path::Path, metadata: &Value) -> Option<PathBuf> {
    ["preview_file", "source_file"].into_iter().find_map(|key| {
        let name = metadata.get(key)?.as_str()?;
        if PathBuf::from(name).file_name()?.to_str()? != name {
            return None;
        }
        let path = directory.join(name);
        path.is_file().then_some(path)
    })
}

#[derive(Clone)]
struct ScenePreviewLayer {
    image: Option<PathBuf>,
    image_opacity: f32,
    scale: f32,
    x: f32,
    y: f32,
    decoration: Option<(PathBuf, f32, f32, f32, f32)>,
}

fn scene_preview_layers(
    project: &Project,
    directory: &std::path::Path,
    time: f64,
    total_duration: f64,
) -> Vec<ScenePreviewLayer> {
    use std::collections::BTreeMap;
    let total_duration = if total_duration.is_finite() && total_duration > 0.0 {
        total_duration
    } else {
        project
            .words
            .iter()
            .map(|word| vietkara_domain::timing::map_time(word.end, &project.anchors, false))
            .fold(0.0_f64, f64::max)
    };
    let scenes = project
        .scenes
        .iter()
        .filter_map(|scene| {
            let section = scene.get("section_name")?.as_str()?.trim();
            if ![
                "intro",
                "verse",
                "pre-chorus",
                "chorus",
                "bridge",
                "outro",
                "instrumental",
            ]
            .contains(&section)
            {
                return None;
            }
            let id = scene.get("id")?.as_str()?;
            if !vietkara_domain::is_hex32(id) {
                return None;
            }
            let image = directory.join(format!("scene_{id}.png"));
            let image = image.is_file().then_some(image);
            let decoration = directory.join(format!("scene_decoration_{id}.png"));
            let decoration = decoration.is_file().then_some(decoration);
            if image.is_none() && decoration.is_none() {
                return None;
            }
            let transition = if scene
                .get("transition")
                .and_then(Value::as_str)
                .unwrap_or("fade")
                == "fade"
            {
                scene
                    .get("transition_seconds")
                    .and_then(Value::as_f64)
                    .filter(|v| v.is_finite())
                    .unwrap_or(0.6)
                    .clamp(0.0, 3.0)
            } else {
                0.0
            };
            Some((
                section.to_owned(),
                (scene.clone(), image, decoration, transition),
            ))
        })
        .collect::<std::collections::HashMap<_, _>>();
    if scenes.is_empty() || project.words.is_empty() {
        return Vec::new();
    }

    let mut lines: BTreeMap<u32, (usize, f64, f64, String)> = BTreeMap::new();
    for (index, word) in project.words.iter().enumerate() {
        let start = vietkara_domain::timing::map_time(word.start, &project.anchors, false);
        let end = vietkara_domain::timing::map_time(word.end, &project.anchors, false);
        let row = lines
            .entry(word.line)
            .or_insert((index, start, end, String::new()));
        row.0 = row.0.min(index);
        row.1 = row.1.min(start);
        row.2 = row.2.max(end);
        if row.3.is_empty() {
            row.3 = word.section_name.as_deref().unwrap_or("").trim().to_owned();
        }
    }
    let mut ordered = lines.into_values().collect::<Vec<_>>();
    ordered.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
    let mut runs: Vec<(String, f64, f64)> = Vec::new();
    for (_, start, end, section) in ordered {
        let section = if scenes.contains_key(&section) {
            section
        } else {
            String::new()
        };
        if let Some(last) = runs.last_mut().filter(|last| last.0 == section) {
            last.2 = last.2.max(end);
        } else {
            runs.push((section, start, end));
        }
    }

    let mut layers = Vec::new();
    for (index, (section, run_start, _)) in runs.iter().enumerate() {
        let Some((scene, image, decoration, base_transition)) = scenes.get(section) else {
            continue;
        };
        let previous_has_scene = index > 0 && scenes.contains_key(&runs[index - 1].0);
        let next_index = index + 1;
        let following_has_scene =
            next_index < runs.len() && scenes.contains_key(&runs[next_index].0);
        let mut transition = *base_transition;
        if previous_has_scene {
            transition = transition.min(((run_start - runs[index - 1].1) / 2.0).max(0.0));
        }
        if following_has_scene {
            transition = transition.min(((runs[next_index].1 - run_start) / 2.0).max(0.0));
        }
        let start = (if previous_has_scene {
            run_start - transition / 2.0
        } else {
            *run_start
        })
        .max(0.0);
        let end = (if following_has_scene {
            runs[next_index].1 + transition / 2.0
        } else if next_index < runs.len() {
            runs[next_index].1
        } else {
            total_duration
        })
        .min(total_duration);
        if end <= start || time < start || time > end {
            continue;
        }
        let duration = end - start;
        let progress = ((time - start) / duration).clamp(0.0, 1.0);
        let fade = transition.min(duration / 2.0).max(0.05);
        let fade_opacity = scene_fade_opacity(progress, duration, fade);
        let keyframes = scene.get("keyframes").unwrap_or(&Value::Null);
        let scale = interpolate_scene_keyframes(keyframes, "scale", progress, 100.0)
            .clamp(100.0, 200.0) as f32;
        let x =
            interpolate_scene_keyframes(keyframes, "x", progress, 0.0).clamp(-100.0, 100.0) as f32;
        let y =
            interpolate_scene_keyframes(keyframes, "y", progress, 0.0).clamp(-100.0, 100.0) as f32;
        let frame_opacity = interpolate_scene_keyframes(keyframes, "opacity", progress, 100.0)
            .clamp(0.0, 100.0) as f32
            / 100.0;
        let image_opacity = scene
            .get("opacity")
            .and_then(Value::as_f64)
            .filter(|v| v.is_finite())
            .unwrap_or(100.0)
            .clamp(0.0, 100.0) as f32
            / 100.0
            * frame_opacity
            * fade_opacity;
        let decoration = decoration.as_ref().map(|path| {
            let base_size = scene
                .get("decoration_size")
                .and_then(Value::as_f64)
                .filter(|v| v.is_finite())
                .unwrap_or(28.0)
                .clamp(5.0, 80.0) as f32;
            let keyframes = scene.get("decoration_keyframes").unwrap_or(&Value::Null);
            let size = base_size
                * interpolate_scene_keyframes(keyframes, "scale", progress, 100.0)
                    .clamp(100.0, 200.0) as f32
                / 100.0;
            let x = interpolate_scene_keyframes(keyframes, "x", progress, 50.0).clamp(0.0, 100.0)
                as f32;
            let y = interpolate_scene_keyframes(keyframes, "y", progress, 50.0).clamp(0.0, 100.0)
                as f32;
            let opacity = interpolate_scene_keyframes(keyframes, "opacity", progress, 100.0)
                .clamp(0.0, 100.0) as f32
                / 100.0
                * fade_opacity;
            (path.clone(), size, x, y, opacity)
        });
        layers.push(ScenePreviewLayer {
            image: image.clone(),
            image_opacity,
            scale,
            x,
            y,
            decoration,
        });
    }
    layers
}

fn set_section_name_on_line(words: &mut [Word], line: u32, name: &str) {
    let cleaned = name
        .trim()
        .chars()
        .take(vietkara_domain::MAX_SECTION_NAME_CHARS)
        .collect::<String>();
    for word in words.iter_mut().filter(|word| word.line == line) {
        word.section_name = (!cleaned.is_empty()).then(|| cleaned.clone());
    }
}

fn toggle_section_on_line(words: &mut [Word], line: u32) -> Option<bool> {
    let first_id = words.iter().find(|word| word.line == line)?.id.clone();
    let first = words.iter_mut().find(|word| word.id == first_id)?;
    let next = !first.section.unwrap_or(false);
    first.section = Some(next);
    Some(next)
}

fn countdown_window_at(
    words: &[Word],
    anchors: &[vietkara_domain::Anchor],
    time: f64,
    duration: f64,
) -> Option<(f64, f64)> {
    let mut lines = std::collections::BTreeMap::<u32, (f64, f64)>::new();
    for word in words {
        let start = vietkara_domain::timing::map_time(word.start, anchors, false);
        let end = vietkara_domain::timing::map_time(word.end, anchors, false);
        let line = lines.entry(word.line).or_insert((start, end));
        line.0 = line.0.min(start);
        line.1 = line.1.max(end);
    }
    let mut previous_end: f64 = 0.0;
    for (index, (start, end)) in lines.into_values().enumerate() {
        if index == 0 || start - previous_end >= 8.0 {
            let window_end = (start - 0.05).max(0.0);
            let window_start = 0.0_f64.max(previous_end).max(window_end - duration);
            if window_end - window_start >= 0.4 && time >= window_start && time < window_end {
                return Some((window_start, window_end));
            }
        }
        previous_end = previous_end.max(end);
    }
    None
}

fn title_preview_motion(
    title: &vietkara_render::layout::TitleLayout,
    time: f64,
) -> (f32, f32, f32, f32) {
    let duration = title.effect_seconds.max(0.001);
    let elapsed = (time - title.start).clamp(0.0, duration);
    let remaining = (title.end - time).clamp(0.0, duration);
    let mut x = 0.0_f32;
    let mut y = 0.0_f32;
    let mut scale = 1.0_f32;
    let mut opacity = 1.0_f32;
    let entering = match title.enter.as_str() {
        "slide_up" => (0.0, 48.0),
        "slide_down" => (0.0, -48.0),
        "slide_left" => (96.0, 0.0),
        "slide_right" => (-96.0, 0.0),
        _ => (0.0, 0.0),
    };
    if time - title.start < duration {
        let p = (elapsed / duration) as f32;
        x += entering.0 * (1.0 - p);
        y += entering.1 * (1.0 - p);
        match title.enter.as_str() {
            "zoom" => scale *= 0.65 + 0.35 * p,
            "flip" => scale *= 0.65 + 0.35 * p,
            "fade" => opacity *= p,
            _ => {}
        }
    }
    if title.end - time < duration {
        let p = (remaining / duration) as f32;
        let leaving = match title.exit.as_str() {
            "slide_up" => (0.0, -48.0),
            "slide_down" => (0.0, 48.0),
            "slide_left" => (-96.0, 0.0),
            "slide_right" => (96.0, 0.0),
            _ => (0.0, 0.0),
        };
        x += leaving.0 * (1.0 - p);
        y += leaving.1 * (1.0 - p);
        match title.exit.as_str() {
            "zoom" | "flip" => scale *= 0.65 + 0.35 * p,
            "fade" => opacity *= p,
            _ => {}
        }
    }
    (x, y, scale, opacity)
}

fn role_label(role: &str) -> &'static str {
    match role {
        "male" => "Nam",
        "female" => "Nữ",
        "voice_c" => "Giọng 3",
        "voice_d" => "Giọng 4",
        "voice_e" => "Giọng 5",
        "voice_f" => "Giọng 6",
        "unknown" => "Chưa rõ",
        _ => "Cùng hát",
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
        cx.bind_keys(text_input::key_bindings());
        cx.bind_keys([
            KeyBinding::new("ctrl-b", ImportBeat, Some("Studio")),
            KeyBinding::new("ctrl-r", ImportReference, Some("Studio")),
            KeyBinding::new("ctrl-t", BeginTapping, Some("Studio")),
            KeyBinding::new("ctrl-space", TogglePlayback, Some("Studio")),
            KeyBinding::new("escape", StopPlayback, Some("Studio")),
            KeyBinding::new("ctrl-left", SeekBack, Some("Studio")),
            KeyBinding::new("ctrl-right", SeekForward, Some("Studio")),
            KeyBinding::new("ctrl-shift-i", ImportSrt, Some("Studio")),
            KeyBinding::new("ctrl-shift-l", ImportLyrics, Some("Studio")),
            KeyBinding::new("ctrl-o", Open, Some("Studio")),
            KeyBinding::new("ctrl-s", Save, Some("Studio")),
            KeyBinding::new("ctrl-shift-s", SaveAs, Some("Studio")),
            KeyBinding::new("ctrl-shift-e", ExportVideo, Some("Studio")),
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
                let word_input = cx.new(|cx| text_input::TextInput::new(cx, ""));
                let title_credit_input = cx.new(|cx| text_input::TextInput::new(cx, ""));
                let section_name_input = cx.new(|cx| text_input::TextInput::new(cx, ""));
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
                        word_input,
                        title_credit_input,
                        section_name_input,
                        editor: Editor::new(project).expect("Project mới phải hợp lệ"),
                        edit_status: initial_status,
                        path,
                        recovery,
                        saving: false,
                        rendering: false,
                        render_progress: 0.0,
                        render_cancel: None,
                        dirty: false,
                        confirm_open: false,
                        confirm_close: false,
                        pending_lyrics: None,
                        pending_audio: None,
                        autosave_status: String::new(),
                        autosaving: false,
                        autosave_pending: false,
                        _autosave_task: task,
                        left_tab_media: false,
                        inspector_design: false,
                        playing: false,
                        selected_word: None,
                        timeline_zoom: 80.,
                        timeline_drag: None,
                        timeline_scroll: ScrollHandle::new(),
                        preview_layout: preview::PreviewLayoutCache::default(),
                        tapping: None,
                        play_position: 0.,
                        play_duration: 0.,
                        background_video_frame: None,
                        background_frame_requested_at: -1.,
                        background_frame_pending: false,
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

#[cfg(test)]
mod workspace_tests {
    use super::*;

    #[test]
    fn workspace_columns_shrink_with_logical_viewport_width() {
        assert_eq!(
            workspace_size(800.),
            WorkspaceSize {
                left: 176.,
                preview: 320.,
                right: 204.,
            }
        );
        assert_eq!(
            workspace_size(1100.),
            WorkspaceSize {
                left: 210.,
                preview: 410.,
                right: 236.,
            }
        );
        assert_eq!(
            workspace_size(1440.),
            WorkspaceSize {
                left: 240.,
                preview: 520.,
                right: 270.,
            }
        );
    }

    #[test]
    fn justified_karaoke_aligns_the_two_lanes_outward() {
        let rows = [
            preview::VisibleLine { line: 7, slot: 0 },
            preview::VisibleLine { line: 8, slot: 1 },
        ];
        assert_eq!(preview_alignment("justify", 7, &rows), "left");
        assert_eq!(preview_alignment("justify", 8, &rows), "right");
        assert_eq!(preview_alignment("center", 7, &rows), "center");
    }

    #[test]
    fn scene_keyframes_interpolate_between_start_and_end_and_clamp_progress() {
        let frames = serde_json::json!([
            {"x":10,"scale":100,"opacity":20},
            {"x":70,"scale":150,"opacity":80}
        ]);
        assert_eq!(interpolate_scene_keyframes(&frames, "x", 0.0, 50.0), 10.0);
        assert_eq!(interpolate_scene_keyframes(&frames, "x", 0.5, 50.0), 40.0);
        assert_eq!(
            interpolate_scene_keyframes(&frames, "scale", 1.0, 100.0),
            150.0
        );
        assert_eq!(
            interpolate_scene_keyframes(&frames, "opacity", 2.0, 100.0),
            80.0
        );
        assert_eq!(
            interpolate_scene_keyframes(&Value::Null, "x", 0.5, 42.0),
            42.0
        );
    }

    #[test]
    fn scene_fades_in_and_out_over_configured_duration() {
        assert_eq!(scene_fade_opacity(0.0, 10.0, 2.0), 0.0);
        assert_eq!(scene_fade_opacity(0.1, 10.0, 2.0), 0.5);
        assert_eq!(scene_fade_opacity(0.5, 10.0, 2.0), 1.0);
        assert_eq!(scene_fade_opacity(0.9, 10.0, 2.0), 0.5);
        assert_eq!(scene_fade_opacity(1.0, 10.0, 2.0), 0.0);
        assert_eq!(scene_fade_opacity(0.5, 0.0, 2.0), 1.0);
    }

    #[test]
    fn adjacent_scene_sections_are_both_present_during_crossfade() {
        let directory = tempfile::tempdir().unwrap();
        let intro_id = "0123456789abcdef0123456789abcdef";
        let chorus_id = "abcdef0123456789abcdef0123456789";
        std::fs::write(
            directory.path().join(format!("scene_{intro_id}.png")),
            b"intro",
        )
        .unwrap();
        std::fs::write(
            directory.path().join(format!("scene_{chorus_id}.png")),
            b"chorus",
        )
        .unwrap();
        let mut project = Project::new("fixture");
        project.scenes = vec![
            serde_json::json!({"id":intro_id,"section_name":"intro","transition":"fade","transition_seconds":0.8}),
            serde_json::json!({"id":chorus_id,"section_name":"chorus","transition":"fade","transition_seconds":0.8}),
        ];
        project.words = vec![
            serde_json::from_value(serde_json::json!({"id":"w1","text":"Xin","line":0,"start":0.0,"end":2.0,"section_name":"intro"})).unwrap(),
            serde_json::from_value(serde_json::json!({"id":"w2","text":"chào","line":1,"start":2.0,"end":4.0,"section_name":"chorus"})).unwrap(),
        ];
        let layers = scene_preview_layers(&project, directory.path(), 2.0, 4.0);
        assert_eq!(layers.len(), 2);
        assert!((layers[0].image_opacity - 0.5).abs() < 0.001);
        assert!((layers[1].image_opacity - 0.5).abs() < 0.001);
        assert_eq!(
            layers[0].image.as_ref().unwrap().file_name().unwrap(),
            std::ffi::OsStr::new(&format!("scene_{intro_id}.png"))
        );
        assert_eq!(
            layers[1].image.as_ref().unwrap().file_name().unwrap(),
            std::ffi::OsStr::new(&format!("scene_{chorus_id}.png"))
        );
    }

    #[test]
    fn video_preview_prefers_proxy_and_falls_back_to_safe_source() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("source.mp4"), b"source").unwrap();
        std::fs::write(directory.path().join("proxy.mp4"), b"proxy").unwrap();
        let metadata = serde_json::json!({"preview_file":"proxy.mp4","source_file":"source.mp4"});
        assert_eq!(
            background_video_preview_path(directory.path(), &metadata).unwrap(),
            directory.path().join("proxy.mp4")
        );
        std::fs::remove_file(directory.path().join("proxy.mp4")).unwrap();
        assert_eq!(
            background_video_preview_path(directory.path(), &metadata).unwrap(),
            directory.path().join("source.mp4")
        );
        let unsafe_metadata =
            serde_json::json!({"preview_file":"..\\outside.mp4","source_file":"..\\outside.mp4"});
        assert!(background_video_preview_path(directory.path(), &unsafe_metadata).is_none());
    }

    #[test]
    fn section_name_and_boundary_apply_to_selected_line_only() {
        let mut words = vec![
            serde_json::from_value(
                serde_json::json!({"id":"a","text":"Xin","line":0,"role":"all"}),
            )
            .unwrap(),
            serde_json::from_value(
                serde_json::json!({"id":"b","text":"chào","line":0,"role":"all"}),
            )
            .unwrap(),
            serde_json::from_value(
                serde_json::json!({"id":"c","text":"Bạn","line":1,"role":"all"}),
            )
            .unwrap(),
        ];
        set_section_name_on_line(&mut words, 0, "  Điệp khúc  ");
        assert_eq!(words[0].section_name.as_deref(), Some("Điệp khúc"));
        assert_eq!(words[1].section_name.as_deref(), Some("Điệp khúc"));
        assert_eq!(words[2].section_name, None);
        assert_eq!(toggle_section_on_line(&mut words, 0), Some(true));
        assert_eq!(words[0].section, Some(true));
        assert_eq!(words[1].section, None);
        assert_eq!(toggle_section_on_line(&mut words, 0), Some(false));
        assert_eq!(words[0].section, Some(false));
        set_section_name_on_line(&mut words, 0, "");
        assert_eq!(words[0].section_name, None);
        assert_eq!(words[1].section_name, None);
    }

    #[test]
    fn ffmpeg_progress_time_parser_handles_hours_and_rejects_bad_fields() {
        assert_eq!(parse_ffmpeg_time("00:01:02.500000\n"), Some(62.5));
        assert_eq!(parse_ffmpeg_time("01:00:00.000000"), Some(3600.0));
        assert_eq!(parse_ffmpeg_time("N/A"), None);
        assert_eq!(parse_ffmpeg_time("00:61:00.000000"), None);
    }

    #[test]
    fn preview_countdown_matches_initial_and_long_pause_windows() {
        let words = vec![
            serde_json::from_value(serde_json::json!({"id":"00000000000000000000000000000001","line_uid":"00000000000000000000000000000011","text":"A","start":5.0,"end":6.0,"line":0,"role":"all"})).unwrap(),
            serde_json::from_value(serde_json::json!({"id":"00000000000000000000000000000002","line_uid":"00000000000000000000000000000012","text":"B","start":20.0,"end":21.0,"line":1,"role":"all"})).unwrap(),
        ];
        let initial = countdown_window_at(&words, &[], 1.0, 4.0).unwrap();
        assert!((initial.0 - 0.95).abs() < 1e-9);
        assert!((initial.1 - 4.95).abs() < 1e-9);
        let pause = countdown_window_at(&words, &[], 16.0, 4.0).unwrap();
        assert!((pause.0 - 15.95).abs() < 1e-9);
        assert!((pause.1 - 19.95).abs() < 1e-9);
        assert_eq!(countdown_window_at(&words, &[], 8.0, 4.0), None);
        assert_eq!(countdown_window_at(&words, &[], 5.0, 4.0), None);
    }

    #[test]
    fn title_preview_motion_tracks_enter_hold_and_exit_phases() {
        let title = vietkara_render::layout::TitleLayout {
            start: 2.0,
            end: 8.0,
            effect_seconds: 1.0,
            enter: "slide_left".into(),
            exit: "fade".into(),
            rows: vec![],
            color: "#ffffff".into(),
            outline: "#000000".into(),
            outline_width: 2.0,
            shadow: 1.0,
            shadow_color: "#000000".into(),
            weight: 700,
            italic: false,
            font: "Arial".into(),
            template: None,
        };
        assert_eq!(title_preview_motion(&title, 2.0), (96.0, 0.0, 1.0, 1.0));
        assert_eq!(title_preview_motion(&title, 4.0), (0.0, 0.0, 1.0, 1.0));
        assert_eq!(title_preview_motion(&title, 8.0), (0.0, 0.0, 1.0, 0.0));
    }

    #[test]
    fn export_pipeline_renders_measured_lyrics_and_refuses_to_overwrite() {
        let root = tempfile::tempdir().unwrap();
        let project_directory = root.path().join("dự án");
        let output_directory = root.path().join("bản xuất");
        std::fs::create_dir(&project_directory).unwrap();
        std::fs::create_dir(&output_directory).unwrap();
        let ffmpeg = vietkara_media::find_binary("ffmpeg").unwrap();
        let audio = std::process::Command::new(ffmpeg)
            .args([
                "-y",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:sample_rate=44100",
                "-t",
                "1",
                "-c:a",
                "pcm_s16le",
            ])
            .arg(project_directory.join("beat.wav"))
            .output()
            .unwrap();
        assert!(
            audio.status.success(),
            "could not synthesize beat: {}",
            String::from_utf8_lossy(&audio.stderr)
        );

        let mut project = Project::new("Export smoke test");
        project.beat = Some(serde_json::json!({"filename":"beat.wav","duration":1.0}));
        project.settings.audio_normalize = true;
        project.words.push(Word {
            id: "word-1".into(),
            text: "Xin chào".into(),
            start: 0.0,
            end: 0.8,
            line_uid: "line-1".into(),
            role: "all".into(),
            line: 0,
            estimated: false,
            locked: None,
            section: None,
            section_name: None,
            manual: None,
            label_color: None,
            row: None,
            extra: Default::default(),
        });
        let project_path = project_directory.join("project.json");
        std::fs::write(&project_path, serde_json::to_vec(&project).unwrap()).unwrap();

        struct TestMeasurer;
        impl vietkara_render::layout::TextMeasurer for TestMeasurer {
            fn width(&self, text: &str, _: &str, size: f64, _: bool) -> f64 {
                text.chars().count() as f64 * size * 0.55
            }
        }
        let plan = vietkara_render::layout::display_plan(&project, &TestMeasurer);
        let target = output_directory.join("karaoke tiếng Việt.mp4");
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let mut progress: f64 = 0.0;
        render_project(
            project.clone(),
            plan.clone(),
            &project_path,
            &target,
            &cancel,
            |value| progress = progress.max(value),
        )
        .unwrap();
        assert!(target.is_file());
        assert!(progress > 0.0);
        let original_size = std::fs::metadata(&target).unwrap().len();
        let error =
            render_project(project, plan, &project_path, &target, &cancel, |_| {}).unwrap_err();
        assert!(error.contains("đã tồn tại"));
        assert_eq!(std::fs::metadata(&target).unwrap().len(), original_size);
    }
}
