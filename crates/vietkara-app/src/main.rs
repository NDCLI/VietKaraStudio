mod preview;
mod text_input;
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
        let (Some(main), Some(video_name)) = (
            self.path.as_ref(),
            project
                .extra
                .get("background_video")
                .and_then(|video| video.get("source_file"))
                .and_then(Value::as_str),
        ) else {
            return;
        };
        if PathBuf::from(video_name)
            .file_name()
            .and_then(|name| name.to_str())
            != Some(video_name)
        {
            return;
        }
        let Some(directory) = main.parent() else {
            return;
        };
        let video = directory.join(video_name);
        if !video.is_file() {
            return;
        }
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
        let settings = &self.editor.project().settings;
        let display_mode = settings
            .extra
            .get("display_mode")
            .and_then(Value::as_str)
            .unwrap_or("karaoke");
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
                                .child(word.text.clone())
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
        let title_visible = layout
            .title
            .as_ref()
            .is_some_and(|title| preview_position >= title.start && preview_position < title.end);
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
                                .child(planned.word.text.clone())
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
            for row in &title.rows {
                current_preview.push(
                    div()
                        .absolute()
                        .left(px(row.x as f32 * preview_scale))
                        .top(px(row.y as f32 * preview_scale))
                        .text_size(px(row.size as f32 * preview_scale))
                        .text_color(rgb(0xffffff))
                        .font_weight(gpui::FontWeight::BOLD)
                        .when(title.italic, |text| text.italic())
                        .child(row.text.clone()),
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
                            .bg(rgb(0x20232b))
                            .rounded_md()
                            .p_3()
                            .child(div().flex().justify_between().items_center()
                                .child("LỜI BÀI HÁT")
                                .child(div().cursor_pointer().px_2().child("＋ SRT")
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this, _, _, cx|this.import_srt(cx)))))
                            .child(div().flex().mt_3().mb_2().gap_2()
                                .child(div().cursor_pointer().px_2().py_1().rounded_sm()
                                    .bg(rgb(if !self.left_tab_media {0x34465b}else{0x17191f}))
                                    .child("Danh sách")
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|{this.left_tab_media=false;cx.notify();})))
                                .child(div().cursor_pointer().px_2().py_1().rounded_sm()
                                    .bg(rgb(if self.left_tab_media {0x34465b}else{0x17191f}))
                                    .child("Nguồn media")
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|{this.left_tab_media=true;cx.notify();}))))
                            .when(self.left_tab_media,|panel|panel
                                .child(div().mt_2().text_color(rgb(0x9aa7b7)).child("AUDIO & VIDEO"))
                                .child(div().mt_3().cursor_pointer().rounded_sm().bg(rgb(0x17191f)).px_2().py_2()
                                    .child(if self.editor.project().beat.is_some(){"●  Beat gốc · đã nạp"}else{"＋  Nạp beat gốc"})
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.choose_audio(AudioSlot::Beat,cx))))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(0x17191f)).px_2().py_2()
                                    .child(if self.editor.project().reference.is_some(){"●  Bản hát · đã nạp"}else{"＋  Nạp bản hát / video"})
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.choose_audio(AudioSlot::Reference,cx))))
                                .child(div().mt_3().cursor_pointer().rounded_sm().bg(rgb(0x17191f)).px_2().py_2()
                                    .child(if background_asset.is_some() {"▧  Thay ảnh nền"} else {"＋  Nạp ảnh nền"})
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.choose_background_image(cx))))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(0x17191f)).px_2().py_2()
                                    .child(if background_type == "video" {"▶  Thay video nền"} else {"＋  Nạp video nền"})
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.choose_background_video(cx))))
                                .child(div().mt_4().text_color(rgb(0x9aa7b7)).child("Project JSON"))
                                .child(div().mt_2().text_size(px(11.)).child(self.path.as_ref().map(|path|path.display().to_string()).unwrap_or_else(||"Chưa lưu project".into()))))
                            .when(!self.left_tab_media,|panel|panel
                                .child(div().text_size(px(11.)).text_color(rgb(0x9aa7b7))
                                    .child(format!("{} từ · {} dòng",self.editor.project().words.len(),self.editor.project().words.iter().map(|w|w.line).max().map(|line|line+1).unwrap_or(0))))
                                .child(div().id("lyric-list").flex_1().overflow_y_scroll().mt_2().children(lyric_rows))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(0x17191f)).px_2().py_2()
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
                                    .bg(rgb(0x20232b))
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
                                                .when(preview_rows.is_empty() && !title_visible,|empty|empty
                                                        .child(div().text_size(px(15.)).text_color(rgb(0x9aa7b7)).child(if display_words.is_empty(){"Nạp lời để xem trước karaoke"}else{"Tua đến câu để xem trước"}))))),
                                    ),
                            )
                            .child(
                                div()
                                    .h(px(390.))
                                    .bg(rgb(0x20232b))
                                    .rounded_md()
                                    .p_3()
                                    .child(timeline)
                                    .child(div().flex().justify_between().items_center().mt_2()
                                        .child("TIMELINE · Kéo khối để chọn, kéo mép để chỉnh mốc")
                                        .child(div().cursor_pointer().rounded_sm().bg(rgb(0x17191f)).px_2().py_1()
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
                            .bg(rgb(0x20232b))
                            .rounded_md()
                            .p_3()
                            .child(div().flex().gap_2().mb_2()
                                .child(div().cursor_pointer().px_2().py_1().rounded_sm()
                                    .bg(rgb(if !self.inspector_design {0x34465b}else{0x17191f}))
                                    .child("Từ")
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|{this.inspector_design=false;cx.notify();})))
                                .child(div().cursor_pointer().px_2().py_1().rounded_sm()
                                    .bg(rgb(if self.inspector_design {0x34465b}else{0x17191f}))
                                    .child("Thiết kế")
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|{this.inspector_design=true;cx.notify();}))))
                            .when(!self.inspector_design,|panel|panel
                            .child("TỪ ĐANG CHỌN")
                            .child(div().mt_3().child("Lời bài hát"))
                            .child(
                                div()
                                    .mt_2()
                                    .child(self.word_input.clone()),
                            )
                            .child(div().mt_2().flex().gap_2()
                                .child(div().flex_1().cursor_pointer().rounded_sm().bg(rgb(0x183226)).px_2().py_2()
                                    .child("Áp dụng chữ")
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.apply_word_text(cx))))
                                .child(div().flex_1().cursor_pointer().rounded_sm().bg(rgb(0x17191f)).px_2().py_2()
                                    .child(format!("Vai: {selected_role}"))
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.cycle_selected_role(cx)))))
                            .child(div().mt_4().child(format!("Bắt đầu {selected_start:.3}s")))
                            .child(div().mt_2().child(format!("Kết thúc {selected_end:.3}s")))
                            .children([(true,-0.05,"Đầu −50ms"),(true,0.05,"Đầu +50ms"),(false,-0.05,"Cuối −50ms"),(false,0.05,"Cuối +50ms")].into_iter().map(|(start,delta,label)|
                                div().mt_2().cursor_pointer().child(label)
                                    .on_mouse_down(MouseButton::Left,cx.listener(move |this, _, _, cx|this.adjust_word(start,delta,cx)))))
                            .child(div().mt_6().child("Space để căn lời theo nhạc")))
                            .when(self.inspector_design,|panel|panel
                                .child("THIẾT KẾ LỜI")
                                .child(div().mt_3().text_size(px(11.)).text_color(rgb(0x9aa7b7)).child("CHẾ ĐỘ HIỂN THỊ"))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(0x17191f)).px_2().py_2()
                                    .child(match display_mode {"lyricsmv"=>"Lời nổi bật theo câu", "subtitle"=>"Phụ đề", _=>"Karaoke · quét từng từ"})
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.cycle_setting("display_mode",&["karaoke","lyricsmv","subtitle"],cx))))
                                .child(div().mt_3().text_size(px(11.)).text_color(rgb(0x9aa7b7)).child("KHUNG HÌNH"))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(0x17191f)).px_2().py_2()
                                    .child(if is_portrait{"9:16 · Dọc"}else{"16:9 · Ngang"})
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|{
                                        let next=if this.editor.project().settings.aspect==vietkara_domain::settings::Aspect::Portrait {vietkara_domain::settings::Aspect::Landscape} else {vietkara_domain::settings::Aspect::Portrait};
                                        let mut settings=this.editor.project().settings.clone();settings.aspect=next;
                                        this.edit_status=match this.editor.execute(Command::SetSettings(settings)){Ok(_)=>{this.dirty=true;this.autosave_pending=true;"Đã đổi khung hình • chưa lưu".into()},Err(error)=>error.to_string()};cx.notify();
                                    })))
                                .child(div().mt_3().text_size(px(11.)).text_color(rgb(0x9aa7b7)).child("PHÔNG CHỮ"))
                                .child(div().mt_2().flex().justify_between().items_center().rounded_sm().bg(rgb(0x17191f)).px_2().py_2()
                                    .child(settings.font.clone())
                                    .child(div().flex().gap_3()
                                        .child(div().cursor_pointer().child("−").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.font_size(-1.,cx))))
                                        .child(div().cursor_pointer().child("+").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.font_size(1.,cx))))))
                                .child(div().mt_2().text_size(px(11.)).text_color(rgb(0x9aa7b7)).child(format!("{} px · Tiếng Việt: Trường xưa, nắng mới",settings.font_size)))
                                .child(div().mt_3().cursor_pointer().rounded_sm().bg(rgb(0x17191f)).px_2().py_2()
                                    .child(if italic{"✓  Chữ nghiêng"}else{"Chữ nghiêng"})
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|{let v=!this.editor.project().settings.extra.get("italic").and_then(Value::as_bool).unwrap_or(false);this.update_setting("italic",Value::Bool(v),cx);})))
                                .child(div().mt_3().text_size(px(11.)).text_color(rgb(0x9aa7b7)).child("NỀN KHUNG HÌNH"))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(0x17191f)).px_2().py_2()
                                    .child(match background_type {"image"=>"Đang dùng ảnh nền","video"=>"Đang dùng video nền",_=>"Đang dùng nền màu"})
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.cycle_setting("background_type",&["color","image"],cx))))
                                .child(div().mt_2().flex().gap_2()
                                    .children([("Xanh đêm","#101d2b"),("Đen","#090c12"),("Tím","#221c35")].into_iter().map(|(label,color)|
                                        div().flex_1().cursor_pointer().rounded_sm().bg(rgb(0x17191f)).px_2().py_2()
                                            .child(div().flex().gap_1().items_center()
                                                .child(div().w(px(10.)).h(px(10.)).rounded_sm().bg(rgb(match color {"#090c12"=>0x090c12,"#221c35"=>0x221c35,_=>0x101d2b})))
                                                .child(label))
                                            .on_mouse_down(MouseButton::Left,cx.listener(move |this,_,_,cx|this.update_setting("background",Value::String(color.into()),cx))))))
                                .child(div().mt_3().text_size(px(11.)).text_color(rgb(0x9aa7b7)).child("VỊ TRÍ LỜI"))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(0x17191f)).px_2().py_2()
                                    .child(format!("Vị trí: {position_label}"))
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.cycle_setting("position",&["bottom","center","top"],cx))))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(0x17191f)).px_2().py_2()
                                    .child(format!("Căn lề: {align_label}"))
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.cycle_setting("align",&["center","left","right","justify"],cx))))
                                .child(div().mt_3().text_size(px(11.)).text_color(rgb(0x9aa7b7)).child("MÀU CHỮ ĐÃ HÁT"))
                                .child(div().mt_2().flex().gap_2()
                                    .children([("Xanh ngọc","#52efb9"),("Vàng","#f4cf72"),("Hồng","#ff87b7")].into_iter().map(|(label,color)|
                                        div().flex_1().cursor_pointer().rounded_sm().bg(rgb(0x17191f)).px_2().py_2()
                                            .child(div().flex().gap_1().items_center()
                                                .child(div().w(px(10.)).h(px(10.)).rounded_sm().bg(rgb(match color {"#f4cf72"=>0xf4cf72,"#ff87b7"=>0xff87b7,_=>0x52efb9})))
                                                .child(label))
                                            .on_mouse_down(MouseButton::Left,cx.listener(move |this,_,_,cx|this.update_setting("sung",Value::String(color.into()),cx))))))
                                .child(div().mt_4().text_size(px(11.)).text_color(rgb(0x748297)).child("Các tùy chỉnh nâng cao sẽ được bổ sung tiếp theo."))),
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

fn setting_color(settings: &vietkara_domain::settings::Settings, key: &str, fallback: u32) -> u32 {
    settings
        .extra
        .get(key)
        .and_then(Value::as_str)
        .and_then(|value| value.strip_prefix('#'))
        .and_then(|value| u32::from_str_radix(value, 16).ok())
        .unwrap_or(fallback)
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
    fn ffmpeg_progress_time_parser_handles_hours_and_rejects_bad_fields() {
        assert_eq!(parse_ffmpeg_time("00:01:02.500000\n"), Some(62.5));
        assert_eq!(parse_ffmpeg_time("01:00:00.000000"), Some(3600.0));
        assert_eq!(parse_ffmpeg_time("N/A"), None);
        assert_eq!(parse_ffmpeg_time("00:61:00.000000"), None);
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
