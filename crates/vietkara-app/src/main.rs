mod diagnostics;
mod fonts;
mod presets;
mod preview;
mod text_input;
mod theme;
mod timeline;
mod ui_preferences;
use gpui::{
    AnyWindowHandle, App, Application, Bounds, Context, Entity, FocusHandle, KeyBinding,
    KeyDownEvent, KeyUpEvent, MouseButton, ObjectFit, PathPromptOptions, Render, ScrollHandle,
    Task, Timer, Window, WindowBounds, WindowOptions, actions, div, img, prelude::*, px, rgb, size,
    uniform_list,
};
use serde_json::Value;
use vietkara_ai_bridge::{AiKind, AiRequest, Worker, WorkerConfig};
use vietkara_domain::{
    Anchor, Project, Word,
    edit::{Command, Editor},
};
use vietkara_jobs::{BatchItem, BatchRecord, BatchStatus, ItemStatus};

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
        NewProject,
        ImportSrt,
        ImportLyrics,
        ImportPreset,
        ExportPreset,
        SavePreset,
        TogglePlayback,
        StopPlayback,
        SeekBack,
        SeekForward,
        BeginTapping,
        ImportBeat,
        ImportReference,
        ExportVideo,
        CancelExport,
        BatchRender
    ]
);

#[cfg(windows)]
use vietkara_media::playback::{Playback, PlaybackCommand};

struct TapSession {
    clock: vietkara_domain::tapping::TapTiming,
    words: Vec<Word>,
    wall: std::time::Instant,
}

struct PendingSyncSuggestion {
    project_id: String,
    reference_path: PathBuf,
    beat_path: PathBuf,
    base_anchors: Vec<Anchor>,
    result: Value,
}

struct PendingRoleSuggestion {
    project_id: String,
    reference_path: PathBuf,
    reference_sha256: String,
    base_words: Vec<Word>,
    lines: Vec<u32>,
    result: Value,
}

struct PendingLyricsAi {
    project_id: String,
    reference_path: PathBuf,
    reference_sha256: String,
    base_text: String,
    base_words: Vec<Word>,
    text: String,
    words: Vec<Word>,
    changed_words: usize,
    operation: &'static str,
}

enum SyncEvent {
    Started(std::sync::Arc<std::sync::atomic::AtomicBool>),
    Progress(f32, String),
    Finished(Result<Value, String>),
}

enum RoleSuggestionEvent {
    Started(std::sync::Arc<std::sync::atomic::AtomicBool>),
    Progress(f32, String),
    Finished(Result<(Value, String), String>),
}

enum LyricsAiEvent {
    Started(std::sync::Arc<std::sync::atomic::AtomicBool>),
    Progress(f32, String),
    Finished(Result<(Vec<Word>, String, String), String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingNavigation {
    NewProject,
    OpenProject,
}

struct Studio {
    editor: Editor,
    word_input: Entity<text_input::TextInput>,
    title_credit_input: Entity<text_input::TextInput>,
    section_name_input: Entity<text_input::TextInput>,
    font_query_input: Entity<text_input::TextInput>,
    focus_handle: FocusHandle,
    edit_status: String,
    playing: bool,
    selected_word: Option<String>,
    selected_anchor: Option<usize>,
    timeline_zoom: f32,
    timeline_drag: Option<timeline::TimelineDrag>,
    timeline_scroll: ScrollHandle,
    timeline_word_index: std::cell::RefCell<timeline::TimelineWordIndex>,
    preview_layout: preview::PreviewLayoutCache,
    tapping: Option<TapSession>,
    play_position: f64,
    play_duration: f64,
    background_video_frame: Option<PathBuf>,
    background_frame_requested_at: f64,
    background_frame_pending: bool,
    playback_error: Option<String>,
    listen_track: AudioSlot,
    output_device: Option<String>,
    #[cfg(windows)]
    playback: Option<Playback>,
    _playback_task: Task<()>,
    path: Option<PathBuf>,
    recovery: Option<RecoveryCandidate>,
    saving: bool,
    rendering: bool,
    render_progress: f64,
    render_cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    active_batch: bool,
    pending_batch_projects: Vec<PathBuf>,
    batch_record: Option<BatchRecord>,
    pending_preset: Option<presets::Preset>,
    bundled_presets: Vec<presets::Preset>,
    custom_presets: Vec<presets::StoredPreset>,
    show_preset_catalog: bool,
    show_tools_menu: bool,
    pending_preset_delete: Option<String>,
    font_catalog: &'static [fonts::FontFamily],
    show_font_catalog: bool,
    diagnostics_running: bool,
    diagnostics_report: Option<String>,
    ai_device_preference: String,
    ai_model: String,
    dirty: bool,
    confirm_open: bool,
    confirm_new: bool,
    confirm_close: bool,
    close_after_save_window: Option<AnyWindowHandle>,
    navigate_after_save: Option<PendingNavigation>,
    pending_lyrics: Option<Command>,
    pending_audio: Option<(AudioSlot, PathBuf)>,
    autosave_status: String,
    autosaving: bool,
    autosave_pending: bool,
    sync_running: bool,
    sync_progress: f32,
    sync_cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    pending_sync: Option<PendingSyncSuggestion>,
    role_suggest_running: bool,
    role_suggest_cancel_request: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    pending_role_suggestion: Option<PendingRoleSuggestion>,
    lyrics_ai_running: bool,
    lyrics_ai_cancel_request: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    pending_lyrics_ai: Option<PendingLyricsAi>,
    _autosave_task: Task<()>,
    left_tab_media: bool,
    inspector_design: bool,
    workspace_layout: Option<WorkspaceSize>,
    workspace_resize: Option<WorkspaceResize>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct WorkspaceSize {
    left: f32,
    preview: f32,
    right: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkspaceMode {
    Compact,
    Standard,
    Wide,
}

fn workspace_mode(viewport_width: f32) -> WorkspaceMode {
    if viewport_width < 960.0 {
        WorkspaceMode::Compact
    } else if viewport_width < 1280.0 {
        WorkspaceMode::Standard
    } else {
        WorkspaceMode::Wide
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkspaceEdge {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct WorkspaceResize {
    edge: WorkspaceEdge,
    origin_x: f32,
    initial: WorkspaceSize,
    viewport_width: f32,
}

fn resize_workspace(
    initial: WorkspaceSize,
    edge: WorkspaceEdge,
    delta_x: f32,
    viewport_width: f32,
) -> WorkspaceSize {
    let mut next = initial;
    match edge {
        WorkspaceEdge::Left => {
            let max_left = (viewport_width - initial.right - 392.0).clamp(170.0, 420.0);
            next.left = (initial.left + delta_x).clamp(170.0, max_left);
        }
        WorkspaceEdge::Right => {
            let max_right = (viewport_width - initial.left - 392.0).clamp(204.0, 460.0);
            next.right = (initial.right - delta_x).clamp(204.0, max_right);
        }
    }
    fit_workspace(next, viewport_width)
}

fn fit_workspace(mut layout: WorkspaceSize, viewport_width: f32) -> WorkspaceSize {
    let max_panel_width = (viewport_width - 392.0).max(374.0);
    let left_extra = (layout.left - 170.0).max(0.0);
    let right_extra = (layout.right - 204.0).max(0.0);
    let extra_budget = max_panel_width - 374.0;
    let extra_total = left_extra + right_extra;
    if extra_total > extra_budget && extra_total > 0.0 {
        let left_share = left_extra / extra_total;
        layout.left = 170.0 + extra_budget * left_share;
        layout.right = 204.0 + extra_budget * (1.0 - left_share);
    }
    layout.preview = layout
        .preview
        .min((viewport_width - layout.left - layout.right - 112.0).max(240.0));
    layout
}

fn workspace_size(viewport_width: f32) -> WorkspaceSize {
    if workspace_mode(viewport_width) == WorkspaceMode::Compact {
        WorkspaceSize {
            left: 176.,
            preview: 320.,
            right: 260.,
        }
    } else if workspace_mode(viewport_width) == WorkspaceMode::Standard {
        WorkspaceSize {
            left: 190.,
            preview: 410.,
            right: 300.,
        }
    } else {
        WorkspaceSize {
            left: 220.,
            preview: 520.,
            right: 320.,
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
    mut report_fallback: impl FnMut(),
) -> Result<bool, String> {
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
    let ffprobe = vietkara_media::find_binary("ffprobe").ok();
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

    let mut cpu_fallback_used = false;
    let result = (|| {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Err("Đã hủy xuất video".into());
        }
        std::fs::create_dir(temporary_directory.join("fonts"))
            .map_err(|error| format!("Không tạo được thư mục font tạm: {error}"))?;
        fonts::write_family(&project.settings.font, &temporary_directory.join("fonts")).map_err(
            |error| {
                format!(
                    "Không chuẩn bị được font {}: {error}",
                    project.settings.font
                )
            },
        )?;
        if let Some(title_font) = project
            .settings
            .extra
            .get("title_font")
            .and_then(Value::as_str)
        {
            fonts::write_family(title_font, &temporary_directory.join("fonts"))
                .map_err(|error| format!("Không chuẩn bị được font title {title_font}: {error}"))?;
        }
        let mut inputs = vietkara_render::RenderInputs::new(beat, duration);
        inputs.ffmpeg = ffmpeg.clone();
        inputs.encoder = select_video_encoder(&project, &ffmpeg);
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

        let stderr_path = temporary_directory.join("ffmpeg.stderr.log");
        let arguments = vietkara_render::ffmpeg_command(&profile, &inputs)
            .map_err(|error| error.to_string())?;
        let first_encoder = inputs.encoder;
        if let Err(first_error) = execute_ffmpeg(
            &arguments,
            &temporary_directory,
            &stderr_path,
            cancel,
            duration,
            &mut report_progress,
        ) {
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                return Err("Đã hủy xuất video".into());
            }
            if first_encoder != vietkara_render::VideoEncoder::H264Nvenc
                || !is_retryable_nvenc_error(&first_error)
            {
                return Err(first_error);
            }
            report_fallback();
            cpu_fallback_used = true;
            inputs.encoder = vietkara_render::VideoEncoder::Libx264;
            let cpu_arguments = vietkara_render::ffmpeg_command(&profile, &inputs)
                .map_err(|error| error.to_string())?;
            execute_ffmpeg(
                &cpu_arguments,
                &temporary_directory,
                &stderr_path,
                cancel,
                duration,
                &mut report_progress,
            )
            .map_err(|cpu_error| {
                format!(
                    "NVENC thất bại ({first_error}); xuất lại bằng CPU cũng thất bại: {cpu_error}"
                )
            })?;
        }
        let rendered = temporary_directory.join(profile.output_name());
        vietkara_render::verify_output_with_tools(
            &rendered,
            duration,
            width,
            height,
            ffprobe.as_deref(),
            &ffmpeg,
        )
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
        Ok(cpu_fallback_used)
    })();
    let _ = std::fs::remove_dir_all(&temporary_directory);
    result
}

fn select_video_encoder(
    project: &Project,
    ffmpeg: &std::path::Path,
) -> vietkara_render::VideoEncoder {
    use vietkara_render::VideoEncoder;

    match project
        .settings
        .extra
        .get("video_encoder")
        .and_then(Value::as_str)
        .unwrap_or("auto")
    {
        "cpu" => VideoEncoder::Libx264,
        "nvenc" => VideoEncoder::H264Nvenc,
        _ if probe_nvenc(ffmpeg) => VideoEncoder::H264Nvenc,
        _ => VideoEncoder::Libx264,
    }
}

/// Exercise the actual FFmpeg NVENC encoder before automatic selection.
/// A full render still retries on CPU if the selected hardware path fails.
fn probe_nvenc(ffmpeg: &std::path::Path) -> bool {
    use std::process::{Command, Stdio};

    let Ok(mut child) = Command::new(ffmpeg)
        .args([
            "-hide_banner",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=size=256x144:rate=30",
            "-frames:v",
            "3",
            "-c:v",
            "h264_nvenc",
            "-f",
            "null",
            "-",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

fn is_retryable_nvenc_error(error: &str) -> bool {
    let error = error.to_ascii_lowercase();
    [
        "nvenc",
        "unknown encoder",
        "no capable devices",
        "cannot load nvcuda",
        "error while opening encoder",
    ]
    .iter()
    .any(|needle| error.contains(needle))
}

fn execute_ffmpeg(
    arguments: &[String],
    working_directory: &std::path::Path,
    stderr_path: &std::path::Path,
    cancel: &std::sync::atomic::AtomicBool,
    duration: f64,
    report_progress: &mut impl FnMut(f64),
) -> Result<(), String> {
    use std::process::{Command, Stdio};

    let executable = arguments
        .first()
        .ok_or_else(|| "Lệnh FFmpeg rỗng".to_owned())?;
    let stderr_file = std::fs::File::create(stderr_path)
        .map_err(|error| format!("Không tạo được log FFmpeg: {error}"))?;
    let mut child = Command::new(executable)
        .args(&arguments[1..])
        .current_dir(working_directory)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(stderr_file))
        .spawn()
        .map_err(|error| format!("Không chạy được FFmpeg: {error}"))?;
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err("Không đọc được tiến độ FFmpeg".into());
    };
    let mut reader = std::io::BufReader::new(stdout);
    let mut line = String::new();
    loop {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Đã hủy xuất video".into());
        }
        line.clear();
        let bytes = match std::io::BufRead::read_line(&mut reader, &mut line) {
            Ok(bytes) => bytes,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("Không đọc được tiến độ FFmpeg: {error}"));
            }
        };
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
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        return Err("Đã hủy xuất video".into());
    }
    if !status.success() {
        let stderr = std::fs::read_to_string(stderr_path).unwrap_or_default();
        let detail = stderr
            .chars()
            .rev()
            .take(1200)
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>();
        return Err(format_ffmpeg_failure(status.code(), detail.trim()));
    }
    Ok(())
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

fn format_ffmpeg_failure(code: Option<i32>, detail: &str) -> String {
    let lower = detail.to_ascii_lowercase();
    let action = if lower.contains("no such file or directory")
        || lower.contains("could not open file")
        || lower.contains("cannot open")
    {
        "Kiểm tra beat, nền, font và media trong thư mục project; relink file bị thiếu rồi xuất lại."
    } else if lower.contains("permission denied") || lower.contains("access is denied") {
        "Chọn thư mục xuất có quyền ghi và đóng video đích nếu ứng dụng khác đang giữ file."
    } else if lower.contains("no space left") || lower.contains("disk full") {
        "Ổ đĩa đã hết dung lượng; giải phóng dung lượng rồi xuất lại."
    } else if lower.contains("invalid data found")
        || lower.contains("error while decoding")
        || lower.contains("corrupt input")
    {
        "Một file media có thể hỏng hoặc không được hỗ trợ; chuyển đổi hoặc relink file đó rồi thử lại."
    } else if lower.contains("unknown encoder") || lower.contains("error while opening encoder") {
        "FFmpeg không mở được encoder; cài bản FFmpeg đầy đủ hoặc giảm độ phân giải/chất lượng xuất."
    } else if lower.contains("out of memory") || lower.contains("cannot allocate memory") {
        "Thiếu bộ nhớ cho cấu hình hiện tại; giảm độ phân giải/FPS hoặc đóng ứng dụng khác rồi thử lại."
    } else {
        "Kiểm tra file media, cấu hình xuất và quyền ghi thư mục đích; thử giảm độ phân giải nếu lỗi lặp lại."
    };
    if detail.is_empty() {
        format!("FFmpeg kết thúc với mã {code:?}. {action}")
    } else {
        format!("FFmpeg lỗi. {action} Chi tiết: {detail}")
    }
}

enum ExportEvent {
    Progress(f64),
    Fallback,
    Finished(Result<(PathBuf, bool), String>),
}

struct PreparedBatchRender {
    index: usize,
    project: Project,
    project_path: PathBuf,
    target: PathBuf,
    plan: vietkara_render::layout::DisplayPlan,
}

struct BatchProjectSnapshot {
    project_path: PathBuf,
    project_hash: String,
    media_paths: Vec<PathBuf>,
    media_hashes: serde_json::Map<String, Value>,
}

enum BatchRenderEvent {
    ItemStarted(usize, String),
    Progress(usize, f64),
    Fallback(usize),
    ItemFinished(usize, Result<(PathBuf, bool), String>),
    Finished(BatchRecord, bool),
}

impl Studio {
    fn run_updater(&mut self, cx: &mut Context<Self>) {
        if self.dirty || self.saving || self.autosaving || self.rendering || self.active_batch {
            self.edit_status = "Lưu project và dừng xuất/batch trước khi cập nhật".into();
            cx.notify();
            return;
        }
        #[cfg(windows)]
        {
            let helper = std::env::current_exe().ok().and_then(|path| {
                path.parent()
                    .map(|parent| parent.join("VietKaraUpdater.exe"))
            });
            let Some(helper) = helper.filter(|path| path.is_file()) else {
                self.edit_status = "Tự cập nhật chỉ có trong bản Setup đã cài trên Windows".into();
                cx.notify();
                return;
            };
            match std::process::Command::new(helper).spawn() {
                Ok(_) => self.edit_status = "Đã mở trình kiểm tra cập nhật".into(),
                Err(error) => self.edit_status = format!("Không mở được trình cập nhật: {error}"),
            }
        }
        #[cfg(not(windows))]
        {
            self.edit_status = "Tự cập nhật chỉ hỗ trợ bản Setup trên Windows".into();
        }
        cx.notify();
    }

    fn run_diagnostics(&mut self, cx: &mut Context<Self>) {
        if self.diagnostics_running {
            return;
        }
        self.diagnostics_running = true;
        self.diagnostics_report = None;
        self.edit_status = "Đang kiểm tra thành phần cục bộ…".into();
        cx.notify();
        let task = cx
            .background_executor()
            .spawn(async { diagnostics::collect() });
        cx.spawn(async move |this, cx| {
            let report = task.await;
            let _ = this.update(cx, |this, cx| {
                this.diagnostics_running = false;
                this.edit_status = "Đã hoàn tất chẩn đoán cục bộ".into();
                this.diagnostics_report = Some(report);
                cx.notify();
            });
        })
        .detach();
    }

    fn hide_diagnostics(&mut self, cx: &mut Context<Self>) {
        self.diagnostics_report = None;
        cx.notify();
    }

    fn toggle_font_catalog(&mut self, cx: &mut Context<Self>) {
        self.show_font_catalog = !self.show_font_catalog;
        if self.show_font_catalog {
            self.font_query_input.update(cx, |input, cx| {
                input.set_value("");
                cx.notify();
            });
        }
        cx.notify();
    }

    fn select_font(&mut self, family: String, cx: &mut Context<Self>) {
        let mut settings = self.editor.project().settings.clone();
        if settings.font.eq_ignore_ascii_case(&family) {
            self.show_font_catalog = false;
            cx.notify();
            return;
        }
        settings.font = family.clone();
        self.edit_status = match self.editor.execute(Command::SetSettings(settings)) {
            Ok(_) => {
                self.dirty = true;
                self.autosave_pending = true;
                self.show_font_catalog = false;
                format!("Đã chọn font {family} · chưa lưu")
            }
            Err(error) => error.to_string(),
        };
        cx.notify();
    }

    fn toggle_preset_catalog(&mut self, cx: &mut Context<Self>) {
        if self.rendering || self.saving || self.pending_preset.is_some() {
            return;
        }
        self.show_preset_catalog = !self.show_preset_catalog;
        if !self.show_preset_catalog {
            self.pending_preset_delete = None;
        }
        cx.notify();
    }

    fn preview_preset(&mut self, preset: presets::Preset, cx: &mut Context<Self>) {
        if self.rendering || self.saving || self.pending_preset.is_some() {
            return;
        }
        self.show_preset_catalog = false;
        self.pending_preset_delete = None;
        self.edit_status = format!("Đã chọn preset mẫu {} · xác nhận để áp dụng", preset.name);
        self.pending_preset = Some(preset);
        cx.notify();
    }

    fn choose_preset(&mut self, cx: &mut Context<Self>) {
        if self.rendering || self.saving || self.pending_preset.is_some() {
            return;
        }
        self.show_preset_catalog = false;
        let selection = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Chọn preset VietKara Studio hoặc VCL JSON".into()),
        });
        cx.spawn(async move |this, cx| {
            let result = selection.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(Ok(Some(paths))) => {
                        if let Some(path) = paths.into_iter().next() {
                            match presets::Preset::read_from(&path) {
                                Ok(preset) => {
                                    this.edit_status = format!(
                                        "Đã đọc preset {} ({}) · kiểm tra rồi xác nhận để áp dụng",
                                        preset.name, preset.source
                                    );
                                    this.pending_preset = Some(preset);
                                }
                                Err(error) => {
                                    this.edit_status = format!("Không đọc được preset: {error}")
                                }
                            }
                        }
                    }
                    Ok(Ok(None)) => this.edit_status = "Đã hủy nhập preset".into(),
                    Ok(Err(error)) => this.edit_status = error.to_string(),
                    Err(error) => this.edit_status = error.to_string(),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn save_custom_preset(&mut self, cx: &mut Context<Self>) {
        if self.rendering || self.saving || self.pending_preset.is_some() {
            return;
        }
        let result = serde_json::to_value(&self.editor.project().settings)
            .map_err(|error| error.to_string())
            .and_then(|settings| {
                presets::Preset::from_project(&self.editor.project().name, settings)
            })
            .and_then(|preset| presets::save_custom(&batch_data_root(), &preset));
        match result {
            Ok(stored) => {
                self.edit_status =
                    format!("Đã lưu preset {} vào thư viện cá nhân", stored.preset.name);
                self.custom_presets.push(stored);
                self.show_preset_catalog = true;
                self.pending_preset_delete = None;
            }
            Err(error) => self.edit_status = format!("Không lưu được preset: {error}"),
        }
        cx.notify();
    }

    fn request_delete_preset(&mut self, id: String, cx: &mut Context<Self>) {
        if self.rendering || self.saving || self.pending_preset.is_some() {
            return;
        }
        self.pending_preset_delete = Some(id);
        cx.notify();
    }

    fn cancel_delete_preset(&mut self, cx: &mut Context<Self>) {
        self.pending_preset_delete = None;
        cx.notify();
    }

    fn delete_custom_preset(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.pending_preset_delete.take() else {
            return;
        };
        match presets::delete_custom(&batch_data_root(), &id) {
            Ok(()) => {
                self.custom_presets.retain(|stored| stored.id != id);
                self.edit_status = "Đã xóa preset cá nhân".into();
            }
            Err(error) => self.edit_status = format!("Không xóa được preset: {error}"),
        }
        cx.notify();
    }

    fn export_preset(&mut self, cx: &mut Context<Self>) {
        if self.rendering || self.saving || self.pending_preset.is_some() {
            return;
        }
        self.show_preset_catalog = false;
        let project = self.editor.project();
        let settings = match serde_json::to_value(&project.settings) {
            Ok(settings) => settings,
            Err(error) => {
                self.edit_status = format!("Không tạo được preset: {error}");
                cx.notify();
                return;
            }
        };
        let preset = match presets::Preset::from_project(&project.name, settings) {
            Ok(preset) => preset,
            Err(error) => {
                self.edit_status =
                    format!("Thiết lập hiện tại không thể xuất thành preset: {error}");
                cx.notify();
                return;
            }
        };
        let directory = self
            .path
            .as_ref()
            .and_then(|path| path.parent())
            .map(PathBuf::from)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_default();
        let suggested_name = format!("{}.preset.json", presets::filename_stem(&preset.name));
        let selection = cx.prompt_for_new_path(&directory, Some(&suggested_name));
        cx.spawn(async move |this, cx| {
            let result = match selection.await {
                Ok(Ok(Some(path))) => preset.write_to(&path).map(|()| path),
                Ok(Ok(None)) => Err("Đã hủy xuất preset".into()),
                Ok(Err(error)) => Err(error.to_string()),
                Err(error) => Err(error.to_string()),
            };
            let _ = this.update(cx, |this, cx| {
                this.edit_status = match result {
                    Ok(path) => format!("Đã xuất preset: {}", path.display()),
                    Err(error) => error,
                };
                cx.notify();
            });
        })
        .detach();
    }

    fn apply_preset(&mut self, cx: &mut Context<Self>) {
        if self.saving || self.rendering {
            return;
        }
        let Some(preset) = self.pending_preset.take() else {
            return;
        };
        let result = (|| -> Result<(), String> {
            let mut settings = serde_json::to_value(&self.editor.project().settings)
                .map_err(|error| error.to_string())?;
            let object = settings
                .as_object_mut()
                .ok_or("Thiết lập project không hợp lệ")?;
            for (key, value) in preset.settings.clone() {
                object.insert(key, value);
            }
            let settings = vietkara_domain::settings::Settings::from_value(settings)?;
            self.editor
                .execute(Command::SetSettings(settings))
                .map_err(|error| error.to_string())?;
            Ok(())
        })();
        self.edit_status = match result {
            Ok(()) => {
                self.dirty = true;
                self.autosave_pending = true;
                format!("Đã áp dụng preset {} · một bước hoàn tác", preset.name)
            }
            Err(error) => {
                self.pending_preset = Some(preset);
                format!("Không áp dụng được preset: {error}")
            }
        };
        cx.notify();
    }

    fn dismiss_preset(&mut self, cx: &mut Context<Self>) {
        self.pending_preset = None;
        self.edit_status = "Đã bỏ preset; project không thay đổi".into();
        cx.notify();
    }

    fn toggle_ai_device(&mut self, cx: &mut Context<Self>) {
        let preferences = ui_preferences::AiPreferences {
            device: if self.ai_device_preference == "auto" {
                "cpu".into()
            } else {
                "auto".into()
            },
            model: self.ai_model.clone(),
        };
        match ui_preferences::save_ai_preferences(&preferences) {
            Ok(()) => {
                self.ai_device_preference = preferences.device.clone();
                self.edit_status = if preferences.device == "auto" {
                    "AI sẽ tự chọn GPU NVIDIA khả dụng, nếu không có sẽ dùng CPU".into()
                } else {
                    "AI được đặt chạy bằng CPU".into()
                };
            }
            Err(error) => self.edit_status = format!("Không lưu được cấu hình AI: {error}"),
        }
        cx.notify();
    }

    fn cycle_ai_model(&mut self, cx: &mut Context<Self>) {
        let models = ["small", "medium", "large-v3-turbo"];
        let next = models
            .iter()
            .position(|model| *model == self.ai_model)
            .map(|index| (index + 1) % models.len())
            .unwrap_or(0);
        let preferences = ui_preferences::AiPreferences {
            device: self.ai_device_preference.clone(),
            model: models[next].into(),
        };
        match ui_preferences::save_ai_preferences(&preferences) {
            Ok(()) => {
                self.ai_model = preferences.model;
                self.edit_status = format!("Model nhận dạng/căn lời: {}", self.ai_model);
            }
            Err(error) => self.edit_status = format!("Không lưu được cấu hình AI: {error}"),
        }
        cx.notify();
    }

    fn choose_batch_projects(&mut self, cx: &mut Context<Self>) {
        if self.rendering
            || self.saving
            || self
                .batch_record
                .as_ref()
                .is_some_and(|record| record.status != BatchStatus::Done || record.failed > 0)
        {
            return;
        }
        let selection = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Chọn các file project JSON cần render".into()),
        });
        cx.spawn(async move |this, cx| {
            let result = selection.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(Ok(Some(paths))) => {
                        let mut unique = Vec::new();
                        for path in paths {
                            if path.is_file() && !unique.contains(&path) {
                                unique.push(path);
                            }
                        }
                        this.pending_batch_projects = unique;
                        this.edit_status = format!(
                            "Đã chọn {} project; nhấn Bắt đầu batch để kiểm tra và tạo hàng đợi",
                            this.pending_batch_projects.len()
                        );
                    }
                    Ok(Ok(None)) => this.edit_status = "Đã hủy chọn project batch".into(),
                    Ok(Err(error)) => this.edit_status = error.to_string(),
                    Err(error) => this.edit_status = error.to_string(),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn start_pending_batch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.rendering || self.pending_batch_projects.is_empty() {
            return;
        }
        let mut paths = self.pending_batch_projects.clone();
        paths.sort();
        let mut seen = std::collections::HashSet::new();
        paths.retain(|path| {
            std::fs::canonicalize(path)
                .map(|canonical| seen.insert(canonical))
                .unwrap_or(false)
        });
        let data_root = batch_data_root();
        let mut record = match BatchRecord::new(
            "VietKaraStudio",
            "batch",
            &["current".into()],
            &["beat".into()],
        ) {
            Ok(record) => record,
            Err(error) => {
                self.edit_status = format!("Không tạo được hàng đợi: {error}");
                cx.notify();
                return;
            }
        };
        let mut prepared = Vec::new();
        for path in paths {
            match prepare_batch_item(
                path,
                &mut record,
                &data_root,
                &mut self.preview_layout,
                window.text_system(),
            ) {
                Ok(item) => prepared.push(item),
                Err(error) => {
                    let _ = std::fs::remove_dir_all(
                        batch_data_root().join("batch-renders").join(&record.id),
                    );
                    self.edit_status = format!("Không thể đưa project vào batch: {error}");
                    cx.notify();
                    return;
                }
            }
        }
        if prepared.is_empty() {
            self.edit_status = "Không có project hợp lệ để render".into();
            cx.notify();
            return;
        }
        if let Err(error) = record.save(&data_root) {
            let _ = std::fs::remove_dir_all(data_root.join("batch-renders").join(&record.id));
            self.edit_status = format!("Không lưu được hàng đợi batch: {error}");
            cx.notify();
            return;
        }
        self.pending_batch_projects.clear();
        self.batch_record = Some(record.clone());
        self.run_batch(record, prepared, data_root, cx);
    }

    fn resume_batch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.rendering {
            return;
        }
        let Some(mut record) = self.batch_record.clone() else {
            self.edit_status = "Không có hàng đợi batch để tiếp tục".into();
            cx.notify();
            return;
        };
        if record.status == BatchStatus::Done && record.failed == 0 {
            self.edit_status = "Batch gần nhất đã hoàn tất; hãy tạo batch mới".into();
            cx.notify();
            return;
        }
        let data_root = batch_data_root();
        let mut prepared = Vec::new();
        for index in record.first_pending()..record.items.len() {
            if record.items[index].status == ItemStatus::Done {
                continue;
            }
            let item = &record.items[index];
            let project_path = item
                .variant
                .get("source_project_path")
                .and_then(Value::as_str)
                .map(PathBuf::from);
            let result = project_path
                .ok_or_else(|| "Thiếu đường dẫn project trong hàng đợi".to_owned())
                .and_then(|path| {
                    prepare_resumed_batch_item(
                        index,
                        item,
                        path,
                        &mut self.preview_layout,
                        window.text_system(),
                    )
                });
            match result {
                Ok(item) => prepared.push(item),
                Err(error) => {
                    record.mark_item_running(index);
                    record.mark_item_error(index, error);
                }
            }
        }
        record.status = BatchStatus::Queued;
        if let Err(error) = record.save(&data_root) {
            self.edit_status = format!("Không cập nhật được hàng đợi: {error}");
            cx.notify();
            return;
        }
        self.batch_record = Some(record.clone());
        if prepared.is_empty() {
            record.status = BatchStatus::Done;
            let _ = record.save(&data_root);
            self.batch_record = Some(record);
            self.edit_status =
                "Batch đã xử lý xong; kiểm tra các mục lỗi trong trạng thái hàng đợi".into();
            cx.notify();
            return;
        }
        self.run_batch(record, prepared, data_root, cx);
    }

    fn run_batch(
        &mut self,
        record: BatchRecord,
        prepared: Vec<PreparedBatchRender>,
        data_root: PathBuf,
        cx: &mut Context<Self>,
    ) {
        self.rendering = true;
        self.active_batch = true;
        self.render_progress = 0.0;
        self.edit_status = format!("Đang render batch ({} mục)…", prepared.len());
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.render_cancel = Some(cancel.clone());
        self.batch_record = Some(record.clone());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let worker_cancel = cancel.clone();
            let (sender, receiver) = std::sync::mpsc::channel();
            let thread = std::thread::Builder::new().name("vietkara-batch-render".into()).spawn(move || {
                render_batch_items(record, prepared, &data_root, &worker_cancel, |event| {
                    let _ = sender.send(event);
                });
            });
            if let Err(error) = thread {
                let _ = this.update(cx, |this, cx| {
                    this.rendering = false;
                    this.active_batch = false;
                    this.render_cancel = None;
                    this.edit_status = format!("Không khởi chạy được batch render: {error}");
                    cx.notify();
                });
                return;
            }
            loop {
                Timer::after(std::time::Duration::from_millis(150)).await;
                let mut completed = false;
                loop {
                    match receiver.try_recv() {
                        Ok(BatchRenderEvent::ItemStarted(index, name)) => {
                            let _ = this.update(cx, |this, cx| {
                                this.render_progress = 0.0;
                                this.edit_status = format!("Batch · mục {}: {name}", index + 1);
                                cx.notify();
                            });
                        }
                        Ok(BatchRenderEvent::Progress(index, progress)) => {
                            let _ = this.update(cx, |this, cx| {
                                this.render_progress = progress;
                                this.edit_status = format!("Batch · mục {} · {:.0}%", index + 1, progress * 100.0);
                                cx.notify();
                            });
                        }
                        Ok(BatchRenderEvent::Fallback(index)) => {
                            let _ = this.update(cx, |this, cx| {
                                this.edit_status = format!("Batch · mục {}: NVENC lỗi, đang thử CPU…", index + 1);
                                cx.notify();
                            });
                        }
                        Ok(BatchRenderEvent::ItemFinished(index, result)) => {
                            let _ = this.update(cx, |this, cx| {
                                this.edit_status = match result {
                                    Ok((path, fallback)) => format!("Batch · mục {} xong{}: {}", index + 1, if fallback { " (CPU)" } else { "" }, path.display()),
                                    Err(error) => format!("Batch · mục {} lỗi: {error}", index + 1),
                                };
                                cx.notify();
                            });
                        }
                        Ok(BatchRenderEvent::Finished(record, cancelled)) => {
                            let _ = this.update(cx, |this, cx| {
                                let completed_count = record.completed;
                                let failed_count = record.failed;
                                let error_summary = record.items.iter()
                                    .filter(|item| item.status == ItemStatus::Error)
                                    .map(|item| format!("{}: {}", item.name, item.error.as_deref().unwrap_or("Lỗi không rõ")))
                                    .collect::<Vec<_>>().join(" · ");
                                this.batch_record = Some(record);
                                this.rendering = false;
                                this.active_batch = false;
                                this.render_cancel = None;
                                this.render_progress = 0.0;
                                this.edit_status = if cancelled {
                                    format!("Batch đã dừng; hoàn tất {completed_count}, lỗi {failed_count}. Có thể tiếp tục sau.")
                                } else {
                                    let mut summary = format!("Batch hoàn tất; thành công {completed_count}, lỗi {failed_count}.");
                                    if !error_summary.is_empty() {
                                        summary.push_str(" Chi tiết: ");
                                        summary.push_str(&error_summary);
                                    }
                                    summary
                                };
                                cx.notify();
                            });
                            completed = true;
                            break;
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => break,
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            let _ = this.update(cx, |this, cx| {
                                this.rendering = false;
                                this.active_batch = false;
                                this.render_cancel = None;
                                this.edit_status = "Batch worker dừng bất thường; hàng đợi đã lưu và có thể tiếp tục".into();
                                cx.notify();
                            });
                            completed = true;
                            break;
                        }
                    }
                }
                if completed { break; }
            }
        }).detach();
    }

    fn start_workspace_resize(
        &mut self,
        edge: WorkspaceEdge,
        x: f32,
        viewport_width: f32,
        cx: &mut Context<Self>,
    ) {
        if self.timeline_drag.is_some() || self.workspace_resize.is_some() {
            return;
        }
        let initial = self
            .workspace_layout
            .unwrap_or_else(|| workspace_size(viewport_width));
        let initial = fit_workspace(initial, viewport_width);
        self.workspace_resize = Some(WorkspaceResize {
            edge,
            origin_x: x,
            initial,
            viewport_width,
        });
        cx.stop_propagation();
        cx.notify();
    }

    fn move_workspace_resize(&mut self, x: f32, cx: &mut Context<Self>) {
        let Some(drag) = self.workspace_resize else {
            return;
        };
        self.workspace_layout = Some(resize_workspace(
            drag.initial,
            drag.edge,
            x - drag.origin_x,
            drag.viewport_width,
        ));
        cx.notify();
    }

    fn finish_workspace_resize(&mut self, cx: &mut Context<Self>) {
        if self.workspace_resize.take().is_some() {
            if let Some(layout) = self.workspace_layout
                && let Err(error) = ui_preferences::save_workspace_layout(layout)
            {
                self.edit_status = format!("Không lưu được bố cục giao diện: {error}");
            }
            cx.notify();
        }
    }

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
        // The editor exposes only the two duet lanes. Legacy voice_c..voice_f
        // values remain readable, but cycling a role moves into the current
        // two-lane model instead of creating more lane identities.
        let roles = ["all", "male", "female", "unknown"];
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
            || self.sync_running
            || self.role_suggest_running
            || self.recovery.is_some()
            || self.tapping.is_some()
            || self.timeline_drag.is_some()
            || self.pending_audio.is_some()
            || self.pending_lyrics.is_some()
            || self.pending_sync.is_some()
            || self.pending_role_suggestion.is_some()
            || self.pending_lyrics_ai.is_some()
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
            || self.pending_sync.is_some()
            || self.pending_role_suggestion.is_some()
            || self.pending_lyrics_ai.is_some()
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
    fn remove_scene_image(&mut self, cx: &mut Context<Self>) {
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
        self.edit_status = "Đang bỏ ảnh nền scene và tạo phiên bản phục hồi…".into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    vietkara_storage::background_import::remove_scene_image(
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
                                "Đã bỏ ảnh nền scene • phiên bản trước: {}",
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
        let Some(frames) = scene["keyframes"].as_array_mut() else {
            return;
        };
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
        let Some(frames) = scene["decoration_keyframes"].as_array_mut() else {
            return;
        };
        while frames.len() < 2 {
            frames.push(serde_json::json!({"x":50,"y":50,"scale":100,"opacity":100}));
        }
        let (min, max, default) = match key {
            "x" | "y" => (0.0, 100.0, 50.0),
            "scale" => (100.0, 200.0, 100.0),
            _ => (0.0, 100.0, 100.0),
        };
        let Some(frame) = frames.get_mut(endpoint).and_then(Value::as_object_mut) else {
            return;
        };
        let current = frame.get(key).and_then(Value::as_f64).unwrap_or(default);
        frame.insert(
            key.to_owned(),
            serde_json::json!((current + delta).clamp(min, max)),
        );
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
        if self.saving
            || self.autosaving
            || self.sync_running
            || self.role_suggest_running
            || self.tapping.is_some()
            || self.timeline_drag.is_some()
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
                            this.listen_track = slot;
                            this.selected_word = None;
                            this.selected_anchor = None;
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
            let timeline_position = (tap.start - 1.).max(0.);
            let track_position = timeline::timeline_to_track_time(
                self.listen_track,
                timeline_position,
                &self.editor.project().anchors,
            );
            let _ = worker.send(PlaybackCommand::Seek(track_position.max(0.)));
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
            let position = self.playback_position();
            let tap = self.tapping.as_mut().and_then(|s| s.clock.finish(position));
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
        if event.keystroke.key != "space" || !self.playback_is_playing() {
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
    #[cfg(windows)]
    fn playback_position(&self) -> f64 {
        self.playback.as_ref().map_or(self.play_position, |worker| {
            let snapshot = worker.snapshot();
            timeline::track_to_timeline_time(
                self.listen_track,
                snapshot.position,
                &self.editor.project().anchors,
            )
        })
    }

    #[cfg(not(windows))]
    fn playback_position(&self) -> f64 {
        self.play_position
    }

    #[cfg(windows)]
    fn playback_is_playing(&self) -> bool {
        self.playback
            .as_ref()
            .is_some_and(|worker| worker.snapshot().state == vietkara_media::PlayerState::Playing)
    }

    #[cfg(not(windows))]
    fn playback_is_playing(&self) -> bool {
        self.playing
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
            let playback_time = timeline::timeline_to_track_time(
                self.listen_track,
                beat,
                &self.editor.project().anchors,
            );
            #[cfg(windows)]
            if let Some(worker) = &self.playback
                && let Err(error) = worker.send(PlaybackCommand::Seek(playback_time))
            {
                self.playback_error = Some(error);
            }
            #[cfg(not(windows))]
            let _ = playback_time;
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
            let path = self.path.as_ref().and_then(|project_path| {
                let directory = project_path.parent()?;
                Some(
                    project_audio_file(
                        self.editor.project(),
                        Some(project_path),
                        self.listen_track,
                    )
                    .unwrap_or_else(|| directory.join(audio_slot_filename(self.listen_track))),
                )
            });
            match Playback::new() {
                Ok(worker) => {
                    // Keep the worker alive even when the project has no audio
                    // yet: it also owns output-device enumeration for the UI.
                    let Some(path) = path else {
                        self.playback = Some(worker);
                        return;
                    };
                    if !path.is_file() {
                        self.playback_error = Some(format!(
                            "Chưa có {}; hãy nhập hoặc relink track đang nghe",
                            path.file_name()
                                .and_then(|name| name.to_str())
                                .unwrap_or("audio")
                        ));
                        self.playback = Some(worker);
                        return;
                    }
                    let command = self.output_device.clone().map_or_else(
                        || PlaybackCommand::Load(path.clone()),
                        |device| PlaybackCommand::LoadOnDevice {
                            path: path.clone(),
                            device,
                        },
                    );
                    if let Err(error) = worker.send(command) {
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
    fn select_listen_track(&mut self, slot: AudioSlot, cx: &mut Context<Self>) {
        if self.listen_track == slot {
            return;
        }
        self.listen_track = slot;
        self.reset_playback();
        cx.notify();
    }
    fn cycle_output_device(&mut self, cx: &mut Context<Self>) {
        #[cfg(windows)]
        {
            let snapshot = self
                .playback
                .as_ref()
                .map(Playback::snapshot)
                .unwrap_or_default();
            let choices = additional_output_devices(
                &snapshot.output_devices,
                snapshot.default_output_device.as_deref(),
            );
            if choices.is_empty() {
                if snapshot.output_devices.is_empty() {
                    let refresh = self
                        .playback
                        .as_ref()
                        .ok_or_else(|| "Playback worker chưa sẵn sàng".to_owned())
                        .and_then(|worker| worker.send(PlaybackCommand::RefreshDevices));
                    self.edit_status = refresh.map_or_else(
                        |error| error,
                        |_| "Đang làm mới danh sách thiết bị đầu ra audio…".into(),
                    );
                } else {
                    self.edit_status =
                        "Chỉ có thiết bị mặc định; kết nối thêm thiết bị để đổi output".into();
                }
                cx.notify();
                return;
            }
            self.output_device = match self.output_device.as_ref() {
                None => Some(choices[0].clone()),
                Some(current) => choices
                    .iter()
                    .position(|device| device == current)
                    .and_then(|index| choices.get(index + 1))
                    .cloned()
                    .or(None),
            };
            self.reset_playback();
            self.edit_status = self.output_device.as_ref().map_or_else(
                || "Đang dùng thiết bị mặc định của Windows".into(),
                |name| format!("Đang dùng thiết bị: {name}"),
            );
        }
        #[cfg(not(windows))]
        {
            self.edit_status = "Chọn thiết bị audio hiện chỉ hỗ trợ Windows".into();
        }
        cx.notify();
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
            let playback_error = merge_playback_error(state.error, self.playback_error.as_deref());
            let playing = state.state == vietkara_media::PlayerState::Playing;
            let timeline_position = timeline::track_to_timeline_time(
                self.listen_track,
                state.position,
                &self.editor.project().anchors,
            );
            let timeline_duration = timeline::track_to_timeline_time(
                self.listen_track,
                state.duration,
                &self.editor.project().anchors,
            )
            .max(0.0);
            let ended = self.playing
                && state.state == vietkara_media::PlayerState::Stopped
                && state.loaded
                && state.position >= state.duration;
            if self.playing != playing
                || (self.play_position - timeline_position).abs() > 0.005
                || self.play_duration != timeline_duration
                || self.playback_error != playback_error
            {
                self.playing = playing;
                self.play_position = timeline_position;
                self.play_duration = timeline_duration;
                self.playback_error = playback_error;
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

    fn suggest_audio_sync(&mut self, cx: &mut Context<Self>) {
        if self.sync_running {
            if self.role_suggest_running || self.lyrics_ai_running {
                if let Some(cancel) = &self.sync_cancel {
                    cancel.store(true, std::sync::atomic::Ordering::SeqCst);
                }
                if let Some(requested) = &self.role_suggest_cancel_request {
                    requested.store(true, std::sync::atomic::Ordering::SeqCst);
                }
                if let Some(requested) = &self.lyrics_ai_cancel_request {
                    requested.store(true, std::sync::atomic::Ordering::SeqCst);
                }
                self.edit_status = if self.role_suggest_running {
                    "Đang hủy gợi ý phân vai…".into()
                } else {
                    "Đang hủy nhận dạng/căn lời…".into()
                };
                cx.notify();
                return;
            }
            if let Some(cancel) = &self.sync_cancel {
                cancel.store(true, std::sync::atomic::Ordering::SeqCst);
                self.edit_status = "Đang hủy phân tích đồng bộ…".into();
                cx.notify();
            }
            return;
        }
        if self.saving
            || self.autosaving
            || self.rendering
            || self.recovery.is_some()
            || self.pending_audio.is_some()
            || self.pending_lyrics.is_some()
            || self.pending_sync.is_some()
            || self.pending_role_suggestion.is_some()
            || self.pending_lyrics_ai.is_some()
            || self.tapping.is_some()
            || self.timeline_drag.is_some()
            || self.confirm_close
            || self.confirm_open
            || self.confirm_new
        {
            return;
        }
        let Some(project_path) = self.path.as_deref() else {
            self.edit_status = "Hãy lưu project trước khi phân tích đồng bộ audio".into();
            cx.notify();
            return;
        };
        let Some(reference_path) = project_audio_file(
            self.editor.project(),
            Some(project_path),
            AudioSlot::Reference,
        ) else {
            self.edit_status = "Project chưa có bản hát hợp lệ để phân tích".into();
            cx.notify();
            return;
        };
        let Some(beat_path) =
            project_audio_file(self.editor.project(), Some(project_path), AudioSlot::Beat)
        else {
            self.edit_status = "Project chưa có beat hợp lệ để phân tích".into();
            cx.notify();
            return;
        };
        let project_id = self.editor.project().id.clone();
        let base_anchors = self.editor.project().anchors.clone();
        let worker_script = match ai_worker_script() {
            Ok(path) => path,
            Err(error) => {
                self.edit_status = error;
                cx.notify();
                return;
            }
        };
        let python = std::env::var("VIETKARA_PYTHON").unwrap_or_else(|_| {
            if cfg!(windows) {
                "python".into()
            } else {
                "python3".into()
            }
        });
        let data_root = ai_data_root(&worker_script);
        let request = AiRequest::new(
            AiKind::SyncSuggest,
            project_id.clone(),
            serde_json::json!({
                "reference": reference_path,
                "beat": beat_path,
            }),
            serde_json::json!({"data_root": data_root}),
        );
        let config = WorkerConfig::new(python)
            .arg(worker_script.to_string_lossy().into_owned())
            .env("OBS_DATA_DIR", data_root.to_string_lossy().into_owned())
            .timeout(std::time::Duration::from_secs(30 * 60));
        let reference_path = reference_path.to_path_buf();
        let beat_path = beat_path.to_path_buf();
        self.sync_running = true;
        self.sync_progress = 0.0;
        self.sync_cancel = None;
        self.pending_sync = None;
        self.pending_role_suggestion = None;
        self.edit_status = "Đang phân tích tương quan bản hát và beat…".into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let (sender, receiver) = std::sync::mpsc::channel();
            let worker_thread = std::thread::Builder::new()
                .name("vietkara-audio-sync".into())
                .spawn(move || match Worker::spawn(config, request) {
                    Ok(mut worker) => {
                        let _ = sender.send(SyncEvent::Started(worker.cancel_flag()));
                        let result = worker
                            .run(|progress, message| {
                                let _ =
                                    sender.send(SyncEvent::Progress(progress, message.to_owned()));
                            })
                            .map_err(|error| error.to_string());
                        let _ = sender.send(SyncEvent::Finished(result));
                    }
                    Err(error) => {
                        let _ = sender.send(SyncEvent::Finished(Err(error.to_string())));
                    }
                });
            if let Err(error) = worker_thread {
                let _ = this.update(cx, |this, cx| {
                    this.sync_running = false;
                    this.sync_cancel = None;
                    this.edit_status = format!("Không khởi chạy được phân tích đồng bộ: {error}");
                    cx.notify();
                });
                return;
            }
            let mut finished = false;
            while !finished {
                Timer::after(std::time::Duration::from_millis(100)).await;
                loop {
                    match receiver.try_recv() {
                        Ok(SyncEvent::Started(cancel)) => {
                            let _ = this.update(cx, |this, cx| {
                                this.sync_cancel = Some(cancel);
                                cx.notify();
                            });
                        }
                        Ok(SyncEvent::Progress(progress, message)) => {
                            let _ = this.update(cx, |this, cx| {
                                this.sync_progress = progress.clamp(0.0, 100.0);
                                this.edit_status = format!("{message} · {:.0}%", progress);
                                cx.notify();
                            });
                        }
                        Ok(SyncEvent::Finished(result)) => {
                            finished = true;
                            let _ = this.update(cx, |this, cx| {
                                this.sync_running = false;
                                this.sync_cancel = None;
                                match result {
                                    Ok(result)
                                        if this.editor.project().id == project_id
                                            && this.path.as_deref().is_some_and(|path| {
                                                project_audio_file(
                                                    this.editor.project(),
                                                    Some(path),
                                                    AudioSlot::Reference,
                                                ) == Some(reference_path.clone())
                                                    && project_audio_file(
                                                        this.editor.project(),
                                                        Some(path),
                                                        AudioSlot::Beat,
                                                    ) == Some(beat_path.clone())
                                            }) =>
                                    {
                                        match validate_sync_suggestion(
                                            this.editor.project(),
                                            &result,
                                        ) {
                                            Ok(_) => {
                                                this.pending_sync = Some(PendingSyncSuggestion {
                                                    project_id: project_id.clone(),
                                                    reference_path: reference_path.clone(),
                                                    beat_path: beat_path.clone(),
                                                    base_anchors: base_anchors.clone(),
                                                    result,
                                                });
                                                this.edit_status =
                                                    "Có gợi ý đồng bộ; project chưa thay đổi"
                                                        .into();
                                            }
                                            Err(error) => this.edit_status = error,
                                        }
                                    }
                                    Ok(_) => {
                                        this.edit_status =
                                            "Project hoặc file audio đã đổi; bỏ gợi ý cũ".into();
                                    }
                                    Err(error) => this.edit_status = error,
                                }
                                cx.notify();
                            });
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => break,
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            finished = true;
                            let _ = this.update(cx, |this, cx| {
                                this.sync_running = false;
                                this.sync_cancel = None;
                                this.edit_status =
                                    "Worker đồng bộ dừng đột ngột, không trả kết quả".into();
                                cx.notify();
                            });
                            break;
                        }
                    }
                }
            }
        })
        .detach();
    }

    fn accept_sync_suggestion(&mut self, cx: &mut Context<Self>) {
        let Some(suggestion) = self.pending_sync.take() else {
            return;
        };
        let sources_match = self.editor.project().id == suggestion.project_id
            && self.editor.project().anchors == suggestion.base_anchors
            && self.path.as_deref().is_some_and(|path| {
                project_audio_file(self.editor.project(), Some(path), AudioSlot::Reference)
                    == Some(suggestion.reference_path.clone())
                    && project_audio_file(self.editor.project(), Some(path), AudioSlot::Beat)
                        == Some(suggestion.beat_path.clone())
            });
        if !sources_match {
            self.edit_status =
                "Project, audio hoặc mốc đã đổi; hãy phân tích lại trước khi áp dụng".into();
            cx.notify();
            return;
        }
        let anchors = match validate_sync_suggestion(self.editor.project(), &suggestion.result) {
            Ok(anchors) => anchors,
            Err(error) => {
                self.edit_status = error;
                cx.notify();
                return;
            }
        };
        self.edit_status = match self.editor.execute(Command::SetAnchors(anchors)) {
            Ok(_) => {
                self.dirty = true;
                self.autosave_pending = true;
                "Đã áp dụng gợi ý đồng bộ • chưa lưu".into()
            }
            Err(error) => error.to_string(),
        };
        self.selected_anchor = None;
        cx.notify();
    }

    fn dismiss_sync_suggestion(&mut self, cx: &mut Context<Self>) {
        if self.pending_sync.take().is_some() {
            self.edit_status = "Đã bỏ gợi ý đồng bộ; project không thay đổi".into();
            cx.notify();
        }
    }

    fn suggest_roles(&mut self, cx: &mut Context<Self>) {
        if self.role_suggest_running {
            if let Some(requested) = &self.role_suggest_cancel_request {
                requested.store(true, std::sync::atomic::Ordering::SeqCst);
            }
            if let Some(cancel) = &self.sync_cancel {
                cancel.store(true, std::sync::atomic::Ordering::SeqCst);
                self.edit_status = "Đang hủy gợi ý phân vai…".into();
                cx.notify();
            }
            return;
        }
        if self.sync_running {
            self.edit_status = "Hãy chờ phân tích hiện tại hoàn tất trước".into();
            cx.notify();
            return;
        }
        if self.saving
            || self.autosaving
            || self.rendering
            || self.recovery.is_some()
            || self.pending_audio.is_some()
            || self.pending_lyrics.is_some()
            || self.pending_sync.is_some()
            || self.pending_role_suggestion.is_some()
            || self.pending_lyrics_ai.is_some()
            || self.tapping.is_some()
            || self.timeline_drag.is_some()
            || self.confirm_close
            || self.confirm_open
            || self.confirm_new
        {
            return;
        }
        let Some(project_path) = self.path.as_deref() else {
            self.edit_status = "Hãy lưu project trước khi gợi ý phân vai".into();
            cx.notify();
            return;
        };
        if self.editor.project().words.is_empty() {
            self.edit_status = "Hãy nhập lời và mốc thời gian trước khi gợi ý phân vai".into();
            cx.notify();
            return;
        }
        let Some(reference_path) = project_audio_file(
            self.editor.project(),
            Some(project_path),
            AudioSlot::Reference,
        ) else {
            self.edit_status = "Project chưa có bản hát hợp lệ để phân tích vai".into();
            cx.notify();
            return;
        };
        let worker_script = match ai_worker_script() {
            Ok(path) => path,
            Err(error) => {
                self.edit_status = error;
                cx.notify();
                return;
            }
        };
        let python = std::env::var("VIETKARA_PYTHON").unwrap_or_else(|_| {
            if cfg!(windows) {
                "python".into()
            } else {
                "python3".into()
            }
        });
        let data_root = ai_data_root(&worker_script);
        let project_id = self.editor.project().id.clone();
        let base_words = self.editor.project().words.clone();
        let mut line_segments: Vec<(u32, f64, f64)> = Vec::new();
        let mut line_indexes = std::collections::HashMap::new();
        for word in &base_words {
            if let Some(index) = line_indexes.get(&word.line).copied() {
                let segment: &mut (u32, f64, f64) = &mut line_segments[index];
                segment.1 = segment.1.min(word.start);
                segment.2 = segment.2.max(word.end);
            } else {
                line_indexes.insert(word.line, line_segments.len());
                line_segments.push((word.line, word.start, word.end));
            }
        }
        let lines = line_segments
            .iter()
            .map(|(line, _, _)| *line)
            .collect::<Vec<_>>();
        let segments = line_segments
            .iter()
            .map(|(line, start, end)| serde_json::json!({"line":line,"start":start,"end":end}))
            .collect::<Vec<_>>();
        let config = WorkerConfig::new(python)
            .arg(worker_script.to_string_lossy().into_owned())
            .env("OBS_DATA_DIR", data_root.to_string_lossy().into_owned())
            .timeout(std::time::Duration::from_secs(30 * 60));
        let device_preference = self.ai_device_preference.clone();
        let reference_path = reference_path.to_path_buf();
        self.sync_running = true;
        self.role_suggest_running = true;
        self.sync_progress = 0.0;
        self.sync_cancel = None;
        let cancel_requested = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.role_suggest_cancel_request = Some(cancel_requested.clone());
        self.pending_role_suggestion = None;
        self.edit_status = "Đang chuẩn bị vocal để gợi ý hai lane Nam/Nữ…".into();
        cx.notify();

        cx.spawn(async move |this, cx| {
            let (sender, receiver) = std::sync::mpsc::channel();
            let thread_cancel_requested = cancel_requested.clone();
            let worker_project_id = project_id.clone();
            let worker_reference_path = reference_path.clone();
            let worker_thread = std::thread::Builder::new()
                .name("vietkara-role-suggest".into())
                .spawn(move || {
                    let cancel_requested = thread_cancel_requested;
                    let result = (|| -> Result<(Value, String), String> {
                        let (vocal_path, fingerprint) = ensure_cached_vocal(
                            VocalCacheRequest {
                                config: &config,
                                project_id: &worker_project_id,
                                reference_path: &worker_reference_path,
                                data_root: &data_root,
                                device: &device_preference,
                            },
                            &cancel_requested,
                            |cancel| {
                                let _ = sender.send(RoleSuggestionEvent::Started(cancel));
                            },
                            |progress, message| {
                                let _ = sender.send(RoleSuggestionEvent::Progress(
                                    progress,
                                    message.to_owned(),
                                ));
                            },
                        )?;
                        let role_request = AiRequest::new(
                            AiKind::RoleSuggest,
                            worker_project_id.clone(),
                            serde_json::json!({
                                "vocal": vocal_path,
                                "segments": segments,
                            }),
                            serde_json::json!({}),
                        );
                        let mut worker = Worker::spawn(config, role_request)
                            .map_err(|error| error.to_string())?;
                        let cancel = worker.cancel_flag();
                        let _ = sender.send(RoleSuggestionEvent::Started(cancel.clone()));
                        if cancel_requested.load(std::sync::atomic::Ordering::SeqCst) {
                            cancel.store(true, std::sync::atomic::Ordering::SeqCst);
                        }
                        let result = worker
                            .run(|progress, message| {
                                let _ = sender.send(RoleSuggestionEvent::Progress(
                                    40.0 + progress.clamp(0.0, 100.0) * 0.6,
                                    message.to_owned(),
                                ));
                            })
                            .map_err(|error| error.to_string())?;
                        if cancel_requested.load(std::sync::atomic::Ordering::SeqCst) {
                            return Err("Đã hủy gợi ý phân vai".into());
                        }
                        let fingerprint_after = vietkara_storage::sha256_file(&worker_reference_path)
                            .map_err(|error| format!("Không kiểm tra lại được bản hát: {error}"))?;
                        if fingerprint_after != fingerprint {
                            return Err("File bản hát đã đổi trong lúc phân tích; hãy chạy lại".into());
                        }
                        Ok((result, fingerprint))
                    })();
                    let _ = sender.send(RoleSuggestionEvent::Finished(result));
                });
            if let Err(error) = worker_thread {
                let _ = this.update(cx, |this, cx| {
                    this.sync_running = false;
                    this.role_suggest_running = false;
                    this.role_suggest_cancel_request = None;
                    this.sync_cancel = None;
                    this.edit_status = format!("Không khởi chạy được gợi ý phân vai: {error}");
                    cx.notify();
                });
                return;
            }
            let mut finished = false;
            while !finished {
                Timer::after(std::time::Duration::from_millis(100)).await;
                loop {
                    match receiver.try_recv() {
                        Ok(RoleSuggestionEvent::Started(cancel)) => {
                            let _ = this.update(cx, |this, cx| {
                                if this
                                    .role_suggest_cancel_request
                                    .as_ref()
                                    .is_some_and(|requested| {
                                        requested.load(std::sync::atomic::Ordering::SeqCst)
                                    })
                                {
                                    cancel.store(true, std::sync::atomic::Ordering::SeqCst);
                                }
                                this.sync_cancel = Some(cancel);
                                cx.notify();
                            });
                        }
                        Ok(RoleSuggestionEvent::Progress(progress, message)) => {
                            let _ = this.update(cx, |this, cx| {
                                this.sync_progress = progress.clamp(0.0, 100.0);
                                this.edit_status = format!("{message} · {:.0}%", progress);
                                cx.notify();
                            });
                        }
                        Ok(RoleSuggestionEvent::Finished(result)) => {
                            finished = true;
                            let result = if cancel_requested
                                .load(std::sync::atomic::Ordering::SeqCst)
                            {
                                Err("Đã hủy gợi ý phân vai".into())
                            } else {
                                result
                            };
                            let _ = this.update(cx, |this, cx| {
                                this.sync_running = false;
                                this.role_suggest_running = false;
                                this.role_suggest_cancel_request = None;
                                this.sync_cancel = None;
                                match result {
                                    Ok((result, reference_sha256))
                                        if this.editor.project().id == project_id
                                            && words_match_snapshot(
                                                &this.editor.project().words,
                                                &base_words,
                                            )
                                            && this.path.as_deref().is_some_and(|path| {
                                                project_audio_file(
                                                    this.editor.project(),
                                                    Some(path),
                                                    AudioSlot::Reference,
                                                ) == Some(reference_path.clone())
                                            }) =>
                                    {
                                        match role_suggestion_command(&base_words, &lines, &result) {
                                            Ok((Some(_), changed_words)) => {
                                                this.pending_role_suggestion = Some(PendingRoleSuggestion {
                                                    project_id: project_id.clone(),
                                                    reference_path: reference_path.clone(),
                                                    reference_sha256,
                                                    base_words: base_words.clone(),
                                                    lines: lines.clone(),
                                                    result,
                                                });
                                                this.edit_status = format!("Có gợi ý phân vai Nam/Nữ cho {changed_words} từ; project chưa thay đổi");
                                            }
                                            Ok((None, _)) => this.edit_status = "Gợi ý không tạo thay đổi cho các từ hiện tại".into(),
                                            Err(error) => this.edit_status = error,
                                        }
                                    }
                                    Ok(_) => this.edit_status = "Project, lời hoặc file bản hát đã đổi; bỏ gợi ý cũ".into(),
                                    Err(error) => this.edit_status = error,
                                }
                                cx.notify();
                            });
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => break,
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            finished = true;
                            let _ = this.update(cx, |this, cx| {
                                this.sync_running = false;
                                this.role_suggest_running = false;
                                this.role_suggest_cancel_request = None;
                                this.sync_cancel = None;
                                this.edit_status = "Worker phân vai dừng đột ngột, không trả kết quả".into();
                                cx.notify();
                            });
                            break;
                        }
                    }
                }
            }
        })
        .detach();
    }

    fn accept_role_suggestion(&mut self, cx: &mut Context<Self>) {
        let Some(suggestion) = self.pending_role_suggestion.take() else {
            return;
        };
        let source_matches = self.editor.project().id == suggestion.project_id
            && words_match_snapshot(&self.editor.project().words, &suggestion.base_words)
            && self.path.as_deref().is_some_and(|path| {
                project_audio_file(self.editor.project(), Some(path), AudioSlot::Reference)
                    == Some(suggestion.reference_path.clone())
            });
        if !source_matches {
            self.edit_status =
                "Project, lời hoặc bản hát đã đổi; hãy phân tích lại trước khi áp dụng".into();
            cx.notify();
            return;
        }
        let reference_path = suggestion.reference_path.clone();
        let expected_sha256 = suggestion.reference_sha256.clone();
        self.saving = true;
        self.edit_status = "Đang kiểm tra bản hát trước khi áp dụng gợi ý…".into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let actual_sha256 = cx
                .background_executor()
                .spawn(async move {
                    vietkara_storage::sha256_file(&reference_path)
                        .map_err(|error| error.to_string())
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.saving = false;
                match actual_sha256 {
                    Ok(actual_sha256) if actual_sha256 == expected_sha256 => {
                        let still_matches = this.editor.project().id == suggestion.project_id
                            && words_match_snapshot(
                                &this.editor.project().words,
                                &suggestion.base_words,
                            )
                            && this.path.as_deref().is_some_and(|path| {
                                project_audio_file(
                                    this.editor.project(),
                                    Some(path),
                                    AudioSlot::Reference,
                                ) == Some(suggestion.reference_path.clone())
                            });
                        if !still_matches {
                            this.edit_status = "Project, lời hoặc file đã đổi; hãy gợi ý lại trước khi áp dụng".into();
                            cx.notify();
                            return;
                        }
                        let (command, changed_words) = match role_application_command(
                            this.editor.project(),
                            &suggestion.lines,
                            &suggestion.result,
                        ) {
                            Ok((Some(command), changed_words)) => (command, changed_words),
                            Ok((None, _)) => {
                                this.edit_status = "Không có từ nào cần đổi vai".into();
                                cx.notify();
                                return;
                            }
                            Err(error) => {
                                this.edit_status = error;
                                cx.notify();
                                return;
                            }
                        };
                        this.edit_status = match this.editor.execute(command) {
                            Ok(_) => {
                                this.dirty = true;
                                this.autosave_pending = true;
                                format!("Đã áp dụng gợi ý phân vai cho {changed_words} từ • một bước hoàn tác")
                            }
                            Err(error) => error.to_string(),
                        };
                    }
                    Ok(_) => {
                        this.edit_status = "File bản hát đã đổi; hãy chạy lại gợi ý phân vai".into();
                    }
                    Err(error) => this.edit_status = format!("Không xác minh được file bản hát: {error}"),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn dismiss_role_suggestion(&mut self, cx: &mut Context<Self>) {
        if self.pending_role_suggestion.take().is_some() {
            self.edit_status = "Đã bỏ gợi ý phân vai; project không thay đổi".into();
            cx.notify();
        }
    }

    fn run_lyrics_ai(&mut self, selected_line: Option<u32>, cx: &mut Context<Self>) {
        if self.lyrics_ai_running {
            if let Some(requested) = &self.lyrics_ai_cancel_request {
                requested.store(true, std::sync::atomic::Ordering::SeqCst);
            }
            if let Some(cancel) = &self.sync_cancel {
                cancel.store(true, std::sync::atomic::Ordering::SeqCst);
            }
            self.edit_status = "Đang hủy nhận dạng/căn lời…".into();
            cx.notify();
            return;
        }
        if self.sync_running {
            self.edit_status = "Hãy chờ hoặc hủy tác vụ AI hiện tại trước".into();
            cx.notify();
            return;
        }
        if self.saving
            || self.autosaving
            || self.rendering
            || self.recovery.is_some()
            || self.pending_audio.is_some()
            || self.pending_lyrics.is_some()
            || self.pending_sync.is_some()
            || self.pending_role_suggestion.is_some()
            || self.pending_lyrics_ai.is_some()
            || self.tapping.is_some()
            || self.timeline_drag.is_some()
            || self.confirm_close
            || self.confirm_open
            || self.confirm_new
        {
            return;
        }
        let Some(project_path) = self.path.as_deref() else {
            self.edit_status = "Hãy lưu project trước khi nhận dạng hoặc căn lời".into();
            cx.notify();
            return;
        };
        let project = self.editor.project();
        if let Some(line) = selected_line {
            let selected_words = project
                .words
                .iter()
                .filter(|word| word.line == line)
                .collect::<Vec<_>>();
            if selected_words.is_empty() {
                self.edit_status = "Không tìm thấy câu đang chọn để căn lại".into();
                cx.notify();
                return;
            }
            if selected_words.iter().any(|word| word.locked == Some(true)) {
                self.edit_status = "Câu này đang khóa. Mở khóa trước khi căn lại".into();
                cx.notify();
                return;
            }
            if selected_words.len() > 500 {
                self.edit_status = "Câu đang chọn quá dài để căn lại cùng lúc".into();
                cx.notify();
                return;
            }
        } else if project.words.iter().any(|word| word.locked == Some(true)) {
            self.edit_status =
                "Mở khóa các câu đã khóa trước khi nhận lời và căn lại toàn bài".into();
            cx.notify();
            return;
        }
        let Some(reference_path) = project_audio_file(
            self.editor.project(),
            Some(project_path),
            AudioSlot::Reference,
        ) else {
            self.edit_status = "Project chưa có bản hát hợp lệ để nhận dạng".into();
            cx.notify();
            return;
        };
        let worker_script = match ai_worker_script() {
            Ok(path) => path,
            Err(error) => {
                self.edit_status = error;
                cx.notify();
                return;
            }
        };
        let python = std::env::var("VIETKARA_PYTHON").unwrap_or_else(|_| {
            if cfg!(windows) {
                "python".into()
            } else {
                "python3".into()
            }
        });
        let data_root = ai_data_root(&worker_script);
        let project_id = self.editor.project().id.clone();
        let base_project = self.editor.project().clone();
        let base_text = base_project.text.clone();
        let base_words = base_project.words.clone();
        let region = selected_line.map(|line| {
            let start = base_words
                .iter()
                .filter(|word| word.line == line)
                .map(|word| word.start)
                .fold(f64::INFINITY, f64::min);
            let mut end = base_words
                .iter()
                .filter(|word| word.line == line)
                .map(|word| word.end)
                .fold(0.0_f64, f64::max);
            let start = (start - 1.2).max(0.0);
            end += 1.2;
            if let Some(duration) = base_project
                .reference
                .as_ref()
                .and_then(|reference| reference.get("duration"))
                .and_then(Value::as_f64)
                .filter(|duration| *duration > 0.0)
            {
                end = end.min(duration);
            }
            serde_json::json!({"start":start,"end":end})
        });
        if region.as_ref().is_some_and(|region| {
            let start = region.get("start").and_then(Value::as_f64).unwrap_or(0.0);
            let end = region.get("end").and_then(Value::as_f64).unwrap_or(0.0);
            end - start < 1.0 || end - start > 120.0
        }) {
            self.edit_status = "Không đủ âm thanh quanh câu hoặc đoạn căn dài hơn 2 phút".into();
            cx.notify();
            return;
        }
        let region_lines = selected_line.map(|line| vec![line]);
        let supplied_lyrics = if let Some(line) = selected_line {
            base_words
                .iter()
                .filter(|word| word.line == line)
                .map(|word| word.text.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        } else if base_text.trim().is_empty() && !base_words.is_empty() {
            vietkara_domain::lyrics::text_from_words(&base_words)
        } else {
            base_text.clone()
        };
        let align = !supplied_lyrics.trim().is_empty();
        let operation = if selected_line.is_some() {
            "căn lại câu"
        } else if align {
            "căn lời"
        } else {
            "nhận lời"
        };
        let kind = if selected_line.is_some() {
            AiKind::AlignRegion
        } else if align {
            AiKind::Align
        } else {
            AiKind::Recognize
        };
        let config = WorkerConfig::new(python)
            .arg(worker_script.to_string_lossy().into_owned())
            .env("OBS_DATA_DIR", data_root.to_string_lossy().into_owned())
            .timeout(std::time::Duration::from_secs(30 * 60));
        let device_preference = self.ai_device_preference.clone();
        let whisper_model = self.ai_model.clone();
        let reference_path = reference_path.to_path_buf();
        self.sync_running = true;
        self.lyrics_ai_running = true;
        self.sync_progress = 0.0;
        self.sync_cancel = None;
        let cancel_requested = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.lyrics_ai_cancel_request = Some(cancel_requested.clone());
        self.pending_lyrics_ai = None;
        self.edit_status = format!("Đang chuẩn bị vocal để {operation}…");
        cx.notify();

        cx.spawn(async move |this, cx| {
            let (sender, receiver) = std::sync::mpsc::channel();
            let thread_cancel_requested = cancel_requested.clone();
            let worker_project_id = project_id.clone();
            let worker_reference_path = reference_path.clone();
            let worker_base_project = base_project.clone();
            let worker_region_lines = region_lines.clone();
            let worker_thread = std::thread::Builder::new()
                .name("vietkara-lyrics-ai".into())
                .spawn(move || {
                    let cancel_requested = thread_cancel_requested;
                    let result = (|| -> Result<(Vec<Word>, String, String), String> {
                        let (vocal_path, fingerprint) = ensure_cached_vocal(
                            VocalCacheRequest {
                                config: &config,
                                project_id: &worker_project_id,
                                reference_path: &worker_reference_path,
                                data_root: &data_root,
                                device: &device_preference,
                            },
                            &cancel_requested,
                            |cancel| { let _ = sender.send(LyricsAiEvent::Started(cancel)); },
                            |progress, message| {
                                let _ = sender.send(LyricsAiEvent::Progress(progress, message.to_owned()));
                            },
                        )?;
                        if cancel_requested.load(std::sync::atomic::Ordering::SeqCst) {
                            return Err("Đã hủy nhận dạng/căn lời".into());
                        }
                        let mut options = serde_json::json!({
                            "model": whisper_model.clone(),
                            "language": std::env::var("VIETKARA_AI_LANGUAGE").unwrap_or_else(|_| "vi".into()),
                            "device": device_preference,
                            "data_root": data_root,
                        });
                        if align { options["lyrics"] = Value::String(supplied_lyrics); }
                        if let Some(region) = region { options["region"] = region; }
                        let request = AiRequest::new(
                            kind,
                            worker_project_id.clone(),
                            serde_json::json!({"vocals": vocal_path}),
                            options,
                        );
                        let mut worker = Worker::spawn(config, request)
                            .map_err(|error| error.to_string())?;
                        let cancel = worker.cancel_flag();
                        let _ = sender.send(LyricsAiEvent::Started(cancel.clone()));
                        if cancel_requested.load(std::sync::atomic::Ordering::SeqCst) {
                            cancel.store(true, std::sync::atomic::Ordering::SeqCst);
                        }
                        let result = worker.run(|progress, message| {
                            let _ = sender.send(LyricsAiEvent::Progress(
                                40.0 + progress.clamp(0.0, 100.0) * 0.6,
                                message.to_owned(),
                            ));
                        }).map_err(|error| error.to_string())?;
                        if cancel_requested.load(std::sync::atomic::Ordering::SeqCst) {
                            return Err("Đã hủy nhận dạng/căn lời".into());
                        }
                        let fingerprint_after = vietkara_storage::sha256_file(&worker_reference_path)
                            .map_err(|error| format!("Không kiểm tra lại được bản hát: {error}"))?;
                        if fingerprint_after != fingerprint {
                            return Err("File bản hát đã đổi trong lúc phân tích; hãy chạy lại".into());
                        }
                        let (words, text) = build_ai_lyrics_candidate(
                            &result,
                            &worker_base_project,
                            worker_region_lines.as_deref(),
                        )?;
                        Ok((words, text, fingerprint))
                    })();
                    let _ = sender.send(LyricsAiEvent::Finished(result));
                });
            if let Err(error) = worker_thread {
                let _ = this.update(cx, |this, cx| {
                    this.sync_running = false;
                    this.lyrics_ai_running = false;
                    this.lyrics_ai_cancel_request = None;
                    this.sync_cancel = None;
                    this.edit_status = format!("Không khởi chạy được AI nhận dạng/căn lời: {error}");
                    cx.notify();
                });
                return;
            }
            let mut finished = false;
            while !finished {
                Timer::after(std::time::Duration::from_millis(100)).await;
                loop {
                    match receiver.try_recv() {
                        Ok(LyricsAiEvent::Started(cancel)) => {
                            let _ = this.update(cx, |this, cx| {
                                if this.lyrics_ai_cancel_request.as_ref().is_some_and(|requested| {
                                    requested.load(std::sync::atomic::Ordering::SeqCst)
                                }) {
                                    cancel.store(true, std::sync::atomic::Ordering::SeqCst);
                                }
                                this.sync_cancel = Some(cancel);
                                cx.notify();
                            });
                        }
                        Ok(LyricsAiEvent::Progress(progress, message)) => {
                            let _ = this.update(cx, |this, cx| {
                                this.sync_progress = progress.clamp(0.0, 100.0);
                                this.edit_status = format!("{message} · {:.0}%", progress);
                                cx.notify();
                            });
                        }
                        Ok(LyricsAiEvent::Finished(result)) => {
                            finished = true;
                            let result = if cancel_requested
                                .load(std::sync::atomic::Ordering::SeqCst)
                            {
                                Err("Đã hủy nhận dạng/căn lời".into())
                            } else {
                                result
                            };
                            let _ = this.update(cx, |this, cx| {
                                this.sync_running = false;
                                this.lyrics_ai_running = false;
                                this.lyrics_ai_cancel_request = None;
                                this.sync_cancel = None;
                                match result {
                                    Ok((words, text, reference_sha256))
                                        if this.editor.project().id == project_id
                                            && this.editor.project().text == base_text
                                            && words_match_snapshot(&this.editor.project().words, &base_words)
                                            && this.path.as_deref().is_some_and(|path| {
                                                project_audio_file(this.editor.project(), Some(path), AudioSlot::Reference)
                                                    == Some(reference_path.clone())
                                            }) =>
                                    {
                                        let changed_words = region_lines.as_ref().map_or(words.len(), |lines| {
                                            words.iter().filter(|word| lines.contains(&word.line)).count()
                                        });
                                        this.pending_lyrics_ai = Some(PendingLyricsAi {
                                            project_id: project_id.clone(),
                                            reference_path: reference_path.clone(),
                                            reference_sha256,
                                            base_text: base_text.clone(),
                                            base_words: base_words.clone(),
                                            text,
                                            words,
                                            changed_words,
                                            operation,
                                        });
                                        this.edit_status = format!("Đã {operation} {changed_words} từ; xem trước rồi xác nhận để thay project");
                                    }
                                    Ok(_) => this.edit_status = "Project, lời hoặc file bản hát đã đổi; bỏ kết quả AI cũ".into(),
                                    Err(error) => this.edit_status = error,
                                }
                                cx.notify();
                            });
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => break,
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            finished = true;
                            let _ = this.update(cx, |this, cx| {
                                this.sync_running = false;
                                this.lyrics_ai_running = false;
                                this.lyrics_ai_cancel_request = None;
                                this.sync_cancel = None;
                                this.edit_status = "Worker nhận dạng/căn lời dừng đột ngột, không trả kết quả".into();
                                cx.notify();
                            });
                            break;
                        }
                    }
                }
            }
        })
        .detach();
    }

    fn accept_lyrics_ai(&mut self, cx: &mut Context<Self>) {
        let Some(candidate) = self.pending_lyrics_ai.take() else {
            return;
        };
        let source_matches = self.editor.project().id == candidate.project_id
            && self.editor.project().text == candidate.base_text
            && words_match_snapshot(&self.editor.project().words, &candidate.base_words)
            && self.path.as_deref().is_some_and(|path| {
                project_audio_file(self.editor.project(), Some(path), AudioSlot::Reference)
                    == Some(candidate.reference_path.clone())
            });
        if !source_matches {
            self.edit_status =
                "Project, lời hoặc bản hát đã đổi; hãy chạy AI lại trước khi áp dụng".into();
            cx.notify();
            return;
        }
        let reference_path = candidate.reference_path.clone();
        let expected_sha256 = candidate.reference_sha256.clone();
        self.saving = true;
        self.edit_status = "Đang xác minh bản hát trước khi áp dụng kết quả AI…".into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let actual_sha256 = cx
                .background_executor()
                .spawn(async move {
                    vietkara_storage::sha256_file(&reference_path)
                        .map_err(|error| error.to_string())
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.saving = false;
                match actual_sha256 {
                    Ok(actual_sha256) if actual_sha256 == expected_sha256 => {
                        let still_matches = this.editor.project().id == candidate.project_id
                            && this.editor.project().text == candidate.base_text
                            && words_match_snapshot(
                                &this.editor.project().words,
                                &candidate.base_words,
                            )
                            && this.path.as_deref().is_some_and(|path| {
                                project_audio_file(
                                    this.editor.project(),
                                    Some(path),
                                    AudioSlot::Reference,
                                ) == Some(candidate.reference_path.clone())
                            });
                        if !still_matches {
                            this.edit_status =
                                "Project đã đổi trong lúc kiểm tra; hãy chạy AI lại".into();
                            cx.notify();
                            return;
                        }
                        let operation = candidate.operation;
                        let changed_words = candidate.changed_words;
                        this.edit_status = match this.editor.execute(Command::SetLyrics {
                            words: candidate.words,
                            text: candidate.text,
                        }) {
                            Ok(_) => {
                                this.dirty = true;
                                this.autosave_pending = true;
                                this.selected_word = None;
                                format!("Đã áp dụng {operation} cho {changed_words} từ · một bước hoàn tác")
                            }
                            Err(error) => error.to_string(),
                        };
                    }
                    Ok(_) => {
                        this.edit_status =
                            "File bản hát đã đổi; hãy chạy lại nhận dạng/căn lời".into()
                    }
                    Err(error) => {
                        this.edit_status = format!("Không xác minh được bản hát: {error}")
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn dismiss_lyrics_ai(&mut self, cx: &mut Context<Self>) {
        if self.pending_lyrics_ai.take().is_some() {
            self.edit_status = "Đã bỏ kết quả AI; project không thay đổi".into();
            cx.notify();
        }
    }

    fn import_lyrics_file(&mut self, is_srt: bool, cx: &mut Context<Self>) {
        if self.saving
            || self.autosaving
            || self.sync_running
            || self.role_suggest_running
            || self.recovery.is_some()
            || self.pending_lyrics.is_some()
            || self.pending_audio.is_some()
            || self.pending_sync.is_some()
            || self.pending_role_suggestion.is_some()
            || self.pending_lyrics_ai.is_some()
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
        if self.saving || self.autosaving || self.pending_lyrics.is_none() {
            return;
        }
        let project = self.editor.project().clone();
        let path = self.path.clone();
        self.saving = true;
        self.edit_status = "Đang tạo phiên bản trước khi thay lời…".into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let snapshot = match path {
                Some(path) => {
                    let Some(project_dir) = path.parent().map(std::path::Path::to_path_buf) else {
                        let _ = this.update(cx, |this, cx| {
                            this.saving = false;
                            this.edit_status =
                                "Không xác định được thư mục project để tạo phiên bản".into();
                            cx.notify();
                        });
                        return;
                    };
                    cx.background_executor()
                        .spawn(async move {
                            vietkara_storage::create_version(
                                &project_dir,
                                &project,
                                "Trước khi nhập lời/SRT",
                            )
                            .map(|record| Some(record.id))
                            .map_err(|error| error.to_string())
                        })
                        .await
                }
                None => Ok(None),
            };
            let _ = this.update(cx, |this, cx| {
                this.saving = false;
                match snapshot {
                    Ok(version_id) => {
                        let Some(command) = this.pending_lyrics.take() else {
                            cx.notify();
                            return;
                        };
                        this.edit_status = match this.editor.execute(command) {
                            Ok(_) => {
                                this.dirty = true;
                                this.autosave_pending = true;
                                this.selected_word = this
                                    .editor
                                    .project()
                                    .words
                                    .first()
                                    .map(|word| word.id.clone());
                                let first_text = this
                                    .editor
                                    .project()
                                    .words
                                    .first()
                                    .map(|word| word.text.clone())
                                    .unwrap_or_default();
                                this.set_word_input(&first_text, cx);
                                version_id.map_or_else(
                                    || "Đã thay lời • chưa lưu".into(),
                                    |id| format!("Đã thay lời • chưa lưu • phiên bản trước: {id}"),
                                )
                            }
                            Err(error) => error.to_string(),
                        };
                    }
                    Err(error) => {
                        this.edit_status = format!(
                            "Không tạo được phiên bản trước khi thay lời; project chưa đổi: {error}"
                        );
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn open(&mut self, cx: &mut Context<Self>) {
        if self.saving
            || self.autosaving
            || self.rendering
            || self.sync_running
            || self.role_suggest_running
            || self.recovery.is_some()
            || self.pending_lyrics.is_some()
            || self.pending_audio.is_some()
            || self.pending_role_suggestion.is_some()
            || self.pending_lyrics_ai.is_some()
            || self.tapping.is_some()
            || self.timeline_drag.is_some()
            || self.confirm_close
            || self.confirm_open
            || self.confirm_new
        {
            return;
        }
        self.confirm_new = false;
        if self.dirty {
            self.confirm_open = true;
            cx.notify();
            return;
        }
        self.choose_open(cx);
    }

    fn new_project(&mut self, cx: &mut Context<Self>) {
        if self.saving
            || self.autosaving
            || self.rendering
            || self.sync_running
            || self.role_suggest_running
            || self.recovery.is_some()
            || self.pending_lyrics.is_some()
            || self.pending_audio.is_some()
            || self.pending_role_suggestion.is_some()
            || self.pending_lyrics_ai.is_some()
            || self.tapping.is_some()
            || self.timeline_drag.is_some()
            || self.confirm_close
        {
            return;
        }
        self.confirm_open = false;
        if self.dirty {
            self.confirm_new = true;
            cx.notify();
            return;
        }
        self.create_empty_project(cx);
    }

    fn create_empty_project(&mut self, cx: &mut Context<Self>) {
        if self.saving
            || self.autosaving
            || self.rendering
            || self.sync_running
            || self.role_suggest_running
            || self.recovery.is_some()
            || self.pending_lyrics.is_some()
            || self.pending_audio.is_some()
            || self.pending_role_suggestion.is_some()
            || self.pending_lyrics_ai.is_some()
            || self.tapping.is_some()
            || self.timeline_drag.is_some()
            || self.confirm_close
        {
            return;
        }
        match Editor::new(Project::new("Bài hát mới")) {
            Ok(editor) => {
                self.editor = editor;
                self.path = None;
                self.recovery = None;
                self.dirty = true;
                self.autosave_pending = false;
                self.confirm_open = false;
                self.confirm_new = false;
                self.confirm_close = false;
                self.navigate_after_save = None;
                self.selected_word = None;
                self.selected_anchor = None;
                self.timeline_drag = None;
                self.timeline_scroll = ScrollHandle::new();
                self.timeline_word_index = std::cell::RefCell::default();
                self.preview_layout = preview::PreviewLayoutCache::default();
                self.tapping = None;
                self.pending_lyrics = None;
                self.pending_audio = None;
                self.pending_sync = None;
                self.pending_role_suggestion = None;
                self.pending_lyrics_ai = None;
                self.background_video_frame = None;
                self.background_frame_requested_at = -1.0;
                self.background_frame_pending = false;
                self.set_word_input("", cx);
                self.reset_playback();
                self.edit_status = "Đã tạo project mới • chưa lưu".into();
            }
            Err(error) => self.edit_status = error.to_string(),
        }
        cx.notify();
    }

    fn discard_and_navigate(&mut self, cx: &mut Context<Self>) {
        if self.confirm_new {
            self.create_empty_project(cx);
        } else if self.confirm_open {
            self.choose_open(cx);
        }
    }

    fn save_and_navigate(&mut self, cx: &mut Context<Self>) {
        if self.saving || self.autosaving {
            return;
        }
        self.navigate_after_save = if self.confirm_new {
            Some(PendingNavigation::NewProject)
        } else if self.confirm_open {
            Some(PendingNavigation::OpenProject)
        } else {
            None
        };
        if self.navigate_after_save.is_none() {
            return;
        }
        self.save(cx);
        if !self.saving {
            self.navigate_after_save = None;
        }
    }

    fn save_and_close(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.saving
            || self.autosaving
            || !self.confirm_close
            || !self.dirty
            || self.rendering
            || self.recovery.is_some()
            || self.pending_lyrics.is_some()
            || self.pending_audio.is_some()
            || self.tapping.is_some()
            || self.timeline_drag.is_some()
        {
            return;
        }
        self.close_after_save_window = Some(window.window_handle());
        self.save(cx);
    }

    fn choose_open(&mut self, cx: &mut Context<Self>) {
        if self.saving
            || self.autosaving
            || self.sync_running
            || self.role_suggest_running
            || self.recovery.is_some()
            || self.pending_lyrics.is_some()
            || self.pending_audio.is_some()
            || self.tapping.is_some()
            || self.timeline_drag.is_some()
            || self.rendering
            || self.confirm_close
        {
            return;
        }
        self.confirm_open = false;
        self.confirm_new = false;
        self.navigate_after_save = None;
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
                        this.pending_sync = None;
                        this.pending_role_suggestion = None;
                        this.pending_lyrics_ai = None;
                        this.selected_anchor = None;
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
                        self.selected_anchor = None;
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
            || self.pending_role_suggestion.is_some()
            || self.pending_lyrics_ai.is_some()
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
            || self.pending_role_suggestion.is_some()
            || self.pending_lyrics_ai.is_some()
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
                        this.navigate_after_save = None;
                        this.close_after_save_window = None;
                        this.edit_status = "Đã hủy chọn nơi lưu".into();
                        cx.notify();
                    }
                    Ok(Err(error)) => {
                        this.navigate_after_save = None;
                        this.close_after_save_window = None;
                        this.edit_status = error.to_string();
                        cx.notify();
                    }
                    Err(error) => {
                        this.navigate_after_save = None;
                        this.close_after_save_window = None;
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
            || self.pending_role_suggestion.is_some()
            || self.pending_lyrics_ai.is_some()
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
                                || {
                                    let _ = sender.send(ExportEvent::Fallback);
                                },
                            );
                            let _ = sender.send(ExportEvent::Finished(
                                result.map(|fallback| (worker_target, fallback)),
                            ));
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
                                    Ok(ExportEvent::Fallback) => {
                                        if this
                                            .update(cx, |this, cx| {
                                                this.edit_status =
                                                    "NVENC lỗi; đang thử xuất lại bằng CPU…".into();
                                                cx.notify();
                                            })
                                            .is_err()
                                        {
                                            return;
                                        }
                                    }
                                    Ok(ExportEvent::Finished(result)) => {
                                        finished = Some(result);
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
                    Ok((path, true)) => format!(
                        "Đã xuất video bằng CPU sau khi NVENC lỗi: {}",
                        path.display()
                    ),
                    Ok((path, false)) => format!("Đã xuất video: {}", path.display()),
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
            self.edit_status = if self.active_batch {
                "Đang dừng batch sau mục hiện tại…".into()
            } else {
                "Đang dừng render…".into()
            };
            cx.notify();
        }
    }
    fn save_to(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if self.saving
            || self.autosaving
            || self.recovery.is_some()
            || self.pending_lyrics.is_some()
            || self.pending_audio.is_some()
            || self.pending_role_suggestion.is_some()
            || self.pending_lyrics_ai.is_some()
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
            let close_window = this.update(cx, |this, cx| {
                this.saving = false;
                let saved = result.is_ok();
                this.edit_status = match result {
                    Ok(()) => {
                        this.path = Some(saved_path);
                        this.pending_sync = None;
                        this.pending_role_suggestion = None;
                        this.pending_lyrics_ai = None;
                        this.dirty = false;
                        this.confirm_open = false;
                        this.confirm_new = false;
                        this.confirm_close = false;
                        this.autosave_pending = false;
                        "Đã lưu project".into()
                    }
                    Err(error) => error.to_string(),
                };
                cx.notify();
                let navigation = if saved {
                    this.navigate_after_save.take()
                } else {
                    this.navigate_after_save = None;
                    None
                };
                match navigation {
                    Some(PendingNavigation::NewProject) => this.create_empty_project(cx),
                    Some(PendingNavigation::OpenProject) => this.choose_open(cx),
                    None => {}
                }
                if saved {
                    this.close_after_save_window.take()
                } else {
                    this.close_after_save_window = None;
                    None
                }
            });
            if let Ok(Some(window)) = close_window {
                let _ = window.update(cx, |_, window, _| window.remove_window());
            }
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
            self.selected_anchor = None;
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

fn batch_data_root() -> PathBuf {
    if let Some(path) = std::env::var_os("OBS_DATA_DIR") {
        PathBuf::from(path)
    } else if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        PathBuf::from(local_app_data).join("VietKaraStudio")
    } else {
        std::env::current_dir().unwrap_or_default().join("data")
    }
}

fn load_latest_batch_record(data_root: &std::path::Path) -> Option<BatchRecord> {
    let root = data_root.join("batch-renders");
    let entries = std::fs::read_dir(root).ok()?;
    let mut records = entries
        .filter_map(Result::ok)
        .filter_map(|entry| BatchRecord::load(data_root, &entry.file_name().to_string_lossy()).ok())
        .collect::<Vec<_>>();
    records.sort_by(|left, right| right.updated_at.total_cmp(&left.updated_at));
    let mut latest = records
        .into_iter()
        .find(|record| record.status != BatchStatus::Done || record.failed > 0)?;
    if latest.status == BatchStatus::Running {
        for item in &mut latest.items {
            if item.status == ItemStatus::Running {
                item.status = ItemStatus::Queued;
            }
        }
        latest.status = BatchStatus::Cancelled;
        let _ = latest.save(data_root);
    }
    Some(latest)
}

fn render_batch_items(
    record: BatchRecord,
    prepared: Vec<PreparedBatchRender>,
    data_root: &std::path::Path,
    cancel: &std::sync::atomic::AtomicBool,
    emit: impl FnMut(BatchRenderEvent),
) {
    render_batch_items_with(
        record,
        prepared,
        data_root,
        cancel,
        emit,
        |job, cancel, progress, fallback| {
            let worker_target = job.target.clone();
            render_project(
                job.project,
                job.plan,
                &job.project_path,
                &job.target,
                cancel,
                progress,
                fallback,
            )
            .map(|used_cpu_fallback| (worker_target, used_cpu_fallback))
        },
    );
}

fn render_batch_items_with(
    mut record: BatchRecord,
    prepared: Vec<PreparedBatchRender>,
    data_root: &std::path::Path,
    cancel: &std::sync::atomic::AtomicBool,
    emit: impl FnMut(BatchRenderEvent),
    mut render: impl FnMut(
        PreparedBatchRender,
        &std::sync::atomic::AtomicBool,
        &mut dyn FnMut(f64),
        &mut dyn FnMut(),
    ) -> Result<(PathBuf, bool), String>,
) {
    let emit = std::cell::RefCell::new(emit);
    record.status = BatchStatus::Running;
    let _ = record.save(data_root);
    let mut cancelled = false;
    for job in prepared {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            cancelled = true;
            break;
        }
        let index = job.index;
        let name = record
            .items
            .get(index)
            .map(|item| item.name.clone())
            .unwrap_or_default();
        record.mark_item_running(index);
        record.status = BatchStatus::Running;
        let _ = record.save(data_root);
        emit.borrow_mut()(BatchRenderEvent::ItemStarted(index, name));
        let mut on_progress = |progress| {
            emit.borrow_mut()(BatchRenderEvent::Progress(index, progress));
        };
        let mut on_fallback = || emit.borrow_mut()(BatchRenderEvent::Fallback(index));
        let result = render(job, cancel, &mut on_progress, &mut on_fallback);
        match &result {
            Ok((path, fallback)) => record.mark_item_done(
                index,
                serde_json::json!({"output_path": path, "cpu_fallback": fallback}),
            ),
            Err(_error) if cancel.load(std::sync::atomic::Ordering::Relaxed) => {
                if let Some(item) = record.items.get_mut(index) {
                    item.status = ItemStatus::Queued;
                    item.error = None;
                }
                cancelled = true;
            }
            Err(error) => record.mark_item_error(index, error.clone()),
        }
        let _ = record.save(data_root);
        emit.borrow_mut()(BatchRenderEvent::ItemFinished(index, result));
        if cancelled {
            break;
        }
        emit.borrow_mut()(BatchRenderEvent::Progress(index, 1.0));
    }
    record.status = if cancelled {
        BatchStatus::Cancelled
    } else {
        BatchStatus::Done
    };
    let _ = record.save(data_root);
    emit.borrow_mut()(BatchRenderEvent::Finished(record, cancelled));
}

fn batch_beat_path(project: &Project, project_path: &std::path::Path) -> Result<PathBuf, String> {
    let name = project
        .beat
        .as_ref()
        .and_then(|beat| beat.get("filename"))
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .filter(|name| name.file_name().and_then(|value| value.to_str()) == name.to_str())
        .ok_or_else(|| "Không xác định được file beat trong project".to_owned())?;
    let parent = project_path
        .parent()
        .ok_or_else(|| "Không xác định được thư mục project".to_owned())?;
    let path = parent.join(name);
    if !path.is_file() {
        return Err(format!("Không tìm thấy beat: {}", path.display()));
    }
    Ok(path)
}

fn unique_batch_target(directory: &std::path::Path, output_name: &str) -> PathBuf {
    let original = PathBuf::from(output_name);
    let stem = original
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("vietkara");
    let extension = original
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("mp4");
    let initial = directory.join(output_name);
    if !initial.exists() {
        return initial;
    }
    for suffix in 2..100_000 {
        let candidate = directory.join(format!("{stem} ({suffix}).{extension}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    directory.join(format!("{stem} {}.{extension}", vietkara_domain::new_id()))
}

fn unique_batch_target_for_record(
    directory: &std::path::Path,
    output_name: &str,
    items: &[BatchItem],
) -> PathBuf {
    let reserved = |candidate: &std::path::Path| {
        let candidate = candidate.to_string_lossy().to_lowercase();
        items.iter().any(|item| {
            item.variant
                .get("output_path")
                .and_then(Value::as_str)
                .is_some_and(|path| path.to_lowercase() == candidate)
        })
    };
    let first = unique_batch_target(directory, output_name);
    if !reserved(&first) {
        return first;
    }
    let output = PathBuf::from(output_name);
    let stem = output
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("vietkara");
    let extension = output
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("mp4");
    for suffix in 2..100_000 {
        let candidate = directory.join(format!("{stem} ({suffix}).{extension}"));
        if !candidate.exists() && !reserved(&candidate) {
            return candidate;
        }
    }
    directory.join(format!("{stem} {}.{extension}", vietkara_domain::new_id()))
}

fn snapshot_batch_project(
    data_root: &std::path::Path,
    batch_id: &str,
    render_id: &str,
    project: &Project,
    source_project_path: &std::path::Path,
) -> Result<BatchProjectSnapshot, String> {
    let snapshot_directory = data_root
        .join("batch-renders")
        .join(batch_id)
        .join("items")
        .join(render_id);
    let items_directory = snapshot_directory
        .parent()
        .ok_or_else(|| "Không xác định được thư mục snapshot batch".to_owned())?;
    std::fs::create_dir_all(items_directory).map_err(|error| error.to_string())?;
    // A render id is immutable. Never reuse or clean a directory that may
    // already contain a valid snapshot from an earlier attempt.
    std::fs::create_dir(&snapshot_directory).map_err(|error| error.to_string())?;
    let result = (|| {
        let snapshot_path = snapshot_directory.join("project.json");
        vietkara_storage::copy::save_project_copy(
            project,
            Some(source_project_path),
            &snapshot_path,
        )
        .map_err(|error| error.to_string())?;
        let project_hash =
            vietkara_storage::sha256_file(&snapshot_path).map_err(|error| error.to_string())?;
        let media_paths = vietkara_storage::managed_media(&snapshot_directory);
        let mut media_hashes = serde_json::Map::new();
        for path in &media_paths {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| "Tên file media snapshot không hợp lệ".to_owned())?;
            let hash = vietkara_storage::sha256_file(path).map_err(|error| error.to_string())?;
            media_hashes.insert(name.to_owned(), Value::String(hash));
        }
        let beat_name = project
            .beat
            .as_ref()
            .and_then(|beat| beat.get("filename"))
            .and_then(Value::as_str)
            .and_then(|name| PathBuf::from(name).file_name().map(|name| name.to_owned()))
            .and_then(|name| name.to_str().map(str::to_owned))
            .ok_or_else(|| "Không xác định được tên beat trong project snapshot".to_owned())?;
        if !media_hashes.contains_key(&beat_name) {
            return Err("Không tạo được snapshot beat cho hàng đợi".into());
        }
        Ok(BatchProjectSnapshot {
            project_path: snapshot_path,
            project_hash,
            media_paths,
            media_hashes,
        })
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&snapshot_directory);
    }
    result
}

fn prepare_batch_item(
    project_path: PathBuf,
    record: &mut BatchRecord,
    data_root: &std::path::Path,
    cache: &mut preview::PreviewLayoutCache,
    text_system: &gpui::WindowTextSystem,
) -> Result<PreparedBatchRender, String> {
    let project_path = std::fs::canonicalize(&project_path).map_err(|error| error.to_string())?;
    let project =
        vietkara_storage::load_project(&project_path).map_err(|error| error.to_string())?;
    let _duration = project
        .beat
        .as_ref()
        .and_then(|beat| beat.get("duration"))
        .and_then(Value::as_f64)
        .filter(|duration| duration.is_finite() && *duration > 0.0)
        .ok_or_else(|| "Project chưa có beat hợp lệ để xuất video".to_owned())?;
    batch_beat_path(&project, &project_path)?;
    let project_hash =
        vietkara_storage::sha256_file(&project_path).map_err(|error| error.to_string())?;
    let directory = project_path
        .parent()
        .ok_or_else(|| "Không xác định được thư mục project".to_owned())?;
    let target = unique_batch_target_for_record(
        directory,
        &vietkara_render::RenderProfile::from_project(&project).output_name(),
        &record.items,
    );
    let index = record.items.len();
    let render_id = vietkara_domain::new_id();
    let snapshot =
        snapshot_batch_project(data_root, &record.id, &render_id, &project, &project_path)?;
    // Render the exact immutable project/media snapshot persisted in the queue.
    // The source path is retained only for user-facing provenance and output placement.
    let snapshot_project = vietkara_storage::load_project(&snapshot.project_path)
        .map_err(|error| error.to_string())?;
    let plan = preview::cached_display_plan(
        cache,
        &snapshot_project,
        &snapshot_project.words,
        text_system,
    )
    .clone();
    record.items.push(BatchItem {
        project_id: project.id.clone(),
        render_id,
        name: project.name.clone(),
        variant: serde_json::json!({
            "source_project_path": project_path,
            "source_project_sha256": project_hash,
            "snapshot_project_path": snapshot.project_path,
            "snapshot_project_sha256": snapshot.project_hash,
            "snapshot_media": snapshot.media_hashes,
            "output_path": target,
        }),
        directory: directory.to_string_lossy().into_owned(),
        assets: snapshot
            .media_paths
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect(),
        status: ItemStatus::Queued,
        result: None,
        error: None,
    });
    Ok(PreparedBatchRender {
        index,
        project: snapshot_project,
        project_path: snapshot.project_path,
        target,
        plan,
    })
}

fn prepare_resumed_batch_item(
    index: usize,
    item: &BatchItem,
    project_path: PathBuf,
    cache: &mut preview::PreviewLayoutCache,
    text_system: &gpui::WindowTextSystem,
) -> Result<PreparedBatchRender, String> {
    let _source_project_path = project_path;
    let project_path = validate_batch_snapshot(item)?;
    let project =
        vietkara_storage::load_project(&project_path).map_err(|error| error.to_string())?;
    let target = item
        .variant
        .get("output_path")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .ok_or_else(|| "Thiếu đường dẫn đầu ra trong hàng đợi".to_owned())?;
    let plan = preview::cached_display_plan(cache, &project, &project.words, text_system).clone();
    Ok(PreparedBatchRender {
        index,
        project,
        project_path,
        target,
        plan,
    })
}

fn validate_batch_snapshot(item: &BatchItem) -> Result<PathBuf, String> {
    let project_path = item
        .variant
        .get("snapshot_project_path")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .ok_or_else(|| "Thiếu project snapshot trong hàng đợi".to_owned())?;
    let project_path = std::fs::canonicalize(&project_path).map_err(|error| error.to_string())?;
    let expected_project_hash = item
        .variant
        .get("snapshot_project_sha256")
        .and_then(Value::as_str)
        .ok_or_else(|| "Thiếu fingerprint project snapshot".to_owned())?;
    let actual_project_hash =
        vietkara_storage::sha256_file(&project_path).map_err(|error| error.to_string())?;
    if actual_project_hash != expected_project_hash {
        return Err("Project snapshot của batch bị thay đổi".into());
    }
    let snapshot_directory = project_path
        .parent()
        .ok_or_else(|| "Không xác định được thư mục snapshot".to_owned())?;
    let media_fingerprints = item
        .variant
        .get("snapshot_media")
        .and_then(Value::as_object)
        .ok_or_else(|| "Thiếu fingerprint media snapshot".to_owned())?;
    for (name, expected_hash) in media_fingerprints {
        if PathBuf::from(name)
            .file_name()
            .and_then(|value| value.to_str())
            != Some(name.as_str())
        {
            return Err("Tên media snapshot không hợp lệ".into());
        }
        let media_path = snapshot_directory.join(name);
        let actual_hash =
            vietkara_storage::sha256_file(&media_path).map_err(|error| error.to_string())?;
        if actual_hash != expected_hash.as_str().unwrap_or_default() {
            return Err(format!("Media snapshot bị thay đổi: {name}"));
        }
    }
    Ok(project_path)
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
        let font_query = self.font_query_input.read(cx).value().to_lowercase();
        let viewport_width = f32::from(window.viewport_size().width);
        let layout_mode = workspace_mode(viewport_width);
        let workspace = self
            .workspace_layout
            .unwrap_or_else(|| workspace_size(viewport_width));
        let workspace = fit_workspace(workspace, viewport_width);
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
        let output_device_label = self
            .output_device
            .as_deref()
            .map_or("Thiết bị: Mặc định".to_owned(), |name| {
                format!("Thiết bị: {name}")
            });
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
        let missing_audio = [AudioSlot::Beat, AudioSlot::Reference]
            .into_iter()
            .filter(|slot| audio_slot_missing(self.editor.project(), self.path.as_deref(), *slot))
            .collect::<Vec<_>>();
        let pending_audio_relink = self.pending_audio.as_ref().is_some_and(|(slot, _)| {
            audio_slot_missing(self.editor.project(), self.path.as_deref(), *slot)
        });
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
        let selected_scene_image_exists = self
            .path
            .as_ref()
            .and_then(|path| path.parent())
            .and_then(|directory| {
                self.editor
                    .project()
                    .scenes
                    .iter()
                    .find(|scene| {
                        scene.get("section_name").and_then(Value::as_str)
                            == Some(selected_section.as_str())
                    })
                    .and_then(|scene| scene.get("id"))
                    .and_then(Value::as_str)
                    .filter(|id| vietkara_domain::is_hex32(id))
                    .map(|id| directory.join(format!("scene_{id}.png")))
            })
            .is_some_and(|path| path.is_file());
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
        let sweep_feather = settings
            .extra
            .get("sweep_feather")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let sweep_glow = settings
            .extra
            .get("sweep_glow")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
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
        let role_a_icon = settings
            .extra
            .get("role_a_icon")
            .and_then(Value::as_str)
            .unwrap_or("man-new-blue.png");
        let role_b_icon = settings
            .extra
            .get("role_b_icon")
            .and_then(Value::as_str)
            .unwrap_or("lady-pink.png");
        let role_all_icon = settings
            .extra
            .get("role_all_icon")
            .and_then(Value::as_str)
            .unwrap_or("men(blue) & lady(pink).png");
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
        let export_encoder = match settings.extra.get("video_encoder").and_then(Value::as_str) {
            Some("cpu") => "CPU · libx264",
            Some("nvenc") => "NVIDIA · NVENC (có fallback CPU)",
            _ => "Tự động · ưu tiên NVENC, fallback CPU",
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
            let role_setting = display_words
                .iter()
                .find(|word| word.line == line)
                .map(|word| match word.role.as_str() {
                    "male" | "voice_c" | "voice_e" => ("role_a_color", 0x62b5ff),
                    "female" | "voice_d" | "voice_f" => ("role_b_color", 0xff87b7),
                    "unknown" => ("role_unknown_color", 0xffffff),
                    _ => ("role_all_color", 0xf4cf72),
                });
            if duet && display_mode == "karaoke" {
                role_setting
                    .map(|(key, fallback)| setting_color(settings, key, fallback))
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
        let active_plan_rows = preview::visible_plan_rows(layout, self.play_position, display_mode);
        let active_lines: Vec<_> = active_plan_rows
            .iter()
            .map(|&index| preview::VisibleLine {
                line: layout.rows[index].line,
                slot: layout.rows[index].slot,
            })
            .collect();
        let selected_id = self.selected_word.as_deref();
        let preview_line_element = |line: u32, size: f32, subdued: bool, line_color: u32| {
            let line_align = preview_alignment(display_align, line, &active_lines);
            div()
                .w_full()
                .flex()
                .items_center()
                .font_family(settings.font.clone())
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
        let active_countdown = preview::active_countdown_at(&layout.countdowns, preview_position);
        let countdown_preview_visible =
            display_mode == "karaoke" && countdown != "none" && active_countdown.is_some();
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
        let preview_outline_width = settings
            .extra
            .get("outline_width")
            .and_then(Value::as_f64)
            .unwrap_or(3.0)
            .clamp(0.0, 12.0) as f32
            * preview_scale;
        let preview_outline_color = setting_color(settings, "outline_unsung", 0x15202a);
        let preview_outline_opacity = settings
            .extra
            .get("outline_unsung_opacity")
            .and_then(Value::as_f64)
            .unwrap_or(100.0)
            .clamp(0.0, 100.0) as f32
            / 100.0;
        let preview_shadow_color = setting_color(settings, "shadow_color", 0x000000);
        let preview_shadow = settings
            .extra
            .get("shadow")
            .and_then(Value::as_f64)
            .unwrap_or(1.0)
            .clamp(0.0, 15.0) as f32
            * preview_scale;
        let mut current_preview: Vec<_> = if display_mode == "karaoke"
            && !active_plan_rows.is_empty()
        {
            active_plan_rows
                .iter()
                .flat_map(|&row_index| {
                    let row = &layout.rows[row_index];
                    let line_color = role_color(row.line);
                    row.words.iter().flat_map(move |planned| {
                        let active =
                            preview_position >= planned.start && preview_position < planned.end;
                        let sung = preview_position >= planned.end;
                        let color = if sung {
                            line_color
                        } else if active {
                            unsung_color
                        } else if Some(planned.word.id.as_str()) == selected_id {
                            0xffdf79
                        } else {
                            0xffffff
                        };
                        let x = planned.x as f32 * preview_scale;
                        let y = row.y as f32 * preview_scale;
                        let width = planned.width as f32 * preview_scale;
                        let height = row.height as f32 * preview_scale;
                        let font_px = row.font_size as f32 * preview_scale;
                        let text = planned.display_text.clone();
                        let make_text = |color| {
                            div()
                                .text_size(px(font_px))
                                .text_color(rgb(color))
                                .font_family(settings.font.clone())
                                .when(italic, |text| text.italic())
                                .child(text.clone())
                        };
                        let mut elements = vec![
                            div()
                                .absolute()
                                .left(px(x))
                                .top(px(y))
                                .child(make_text(color)),
                        ];
                        if active {
                            let progress = ((preview_position - planned.start)
                                / (planned.end - planned.start).max(0.001))
                            .clamp(0.0, 1.0) as f32;
                            let geometry = sweep_preview_geometry(
                                width,
                                progress,
                                sweep_feather as f32 / 3.0 * preview_scale,
                                sweep_glow as f32,
                                font_px,
                                2.0 * preview_scale,
                            );
                            if geometry.solid_width > 0.0 {
                                elements.push(
                                    div()
                                        .absolute()
                                        .left(px(x))
                                        .top(px(y))
                                        .w(px(geometry.solid_width))
                                        .h(px(height))
                                        .overflow_hidden()
                                        .child(make_text(line_color)),
                                );
                            }
                            for (segment_left, segment_width, opacity) in geometry.feather_segments
                            {
                                elements.push(
                                    div()
                                        .absolute()
                                        .left(px(x + segment_left))
                                        .top(px(y))
                                        .w(px(segment_width))
                                        .h(px(height))
                                        .relative()
                                        .overflow_hidden()
                                        .child(
                                            make_text(line_color)
                                                .absolute()
                                                .left(px(-segment_left))
                                                .top(px(0.))
                                                .opacity(opacity),
                                        ),
                                );
                            }
                            if let Some((left, band_width, opacity)) = geometry.glow_band {
                                let blur_radius =
                                    (1.0 + sweep_feather as f32 / 12.0 + sweep_glow as f32 / 100.0)
                                        * preview_scale;
                                let mut glow = div()
                                    .absolute()
                                    .left(px(x + left))
                                    .top(px(y))
                                    .w(px(band_width))
                                    .h(px(height))
                                    .relative()
                                    .overflow_hidden();
                                for (offset_x, offset_y, alpha) in SWEEP_PREVIEW_GLOW_OFFSETS {
                                    glow = glow.child(
                                        make_text(0xffffff)
                                            .absolute()
                                            .left(px(-left + blur_radius * offset_x))
                                            .top(px(blur_radius * offset_y))
                                            .opacity(opacity * alpha),
                                    );
                                }
                                elements.push(glow);
                            }
                        }
                        elements
                    })
                })
                .collect()
        } else if (display_mode == "lyricsmv" || display_mode == "subtitle")
            && let Some(&row_index) = active_plan_rows.first()
        {
            if display_mode == "lyricsmv" {
                let style = settings
                    .extra
                    .get("mv_style")
                    .and_then(Value::as_str)
                    .unwrap_or("stack");
                let line_height = settings
                    .extra
                    .get("line_height")
                    .and_then(Value::as_f64)
                    .unwrap_or(1.4);
                vietkara_render::layout::lyricsmv_rows(layout, row_index, style, line_height)
                    .into_iter()
                    .flat_map(|item| {
                        let row = item.row;
                        let color = if item.active {
                            sung_color
                        } else {
                            unsung_color
                        };
                        let opacity = item.opacity_percent as f32 / 100.0;
                        preview_text_layers(
                            &row.text,
                            row.x as f32 * preview_scale,
                            row.y as f32 * preview_scale,
                            row.font_size as f32 * preview_scale,
                            PreviewTextStyle {
                                font: &settings.font,
                                italic,
                                fill_color: color,
                                outline_color: preview_outline_color,
                                outline_width: preview_outline_width,
                                outline_opacity: preview_outline_opacity * opacity,
                                shadow_color: preview_shadow_color,
                                shadow: preview_shadow,
                                opacity,
                            },
                        )
                    })
                    .collect()
            } else {
                let row = &layout.rows[row_index];
                preview_text_layers(
                    &row.text,
                    row.x as f32 * preview_scale,
                    row.y as f32 * preview_scale,
                    row.font_size as f32 * preview_scale,
                    PreviewTextStyle {
                        font: &settings.font,
                        italic,
                        fill_color: unsung_color,
                        outline_color: preview_outline_color,
                        outline_width: preview_outline_width,
                        outline_opacity: preview_outline_opacity,
                        shadow_color: preview_shadow_color,
                        shadow: preview_shadow,
                        opacity: 1.0,
                    },
                )
            }
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
        for &row_index in &active_plan_rows {
            let row = &layout.rows[row_index];
            if let Some(marker) = &row.role_marker {
                let filename =
                    vietkara_render::role_icons::role_icon_setting(&marker.role, &settings.extra);
                if let Some(icon) = vietkara_render::role_icons::role_icon(filename) {
                    current_preview.extend(icon.rectangles.iter().map(|rectangle| {
                        div()
                            .absolute()
                            .left(px(
                                (marker.x + rectangle.x * marker.size) as f32 * preview_scale
                            ))
                            .top(px(
                                (marker.y + rectangle.y * marker.size) as f32 * preview_scale
                            ))
                            .w(px((rectangle.width * marker.size) as f32 * preview_scale))
                            .h(px((rectangle.height * marker.size) as f32 * preview_scale))
                            .bg(gpui::rgba(u32::from_be_bytes(rectangle.color)))
                    }));
                } else {
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
        let timeline = self.timeline_view(window, cx);
        let active_reference_position = reference_position;
        let lyric_rows = uniform_list(
            "lyric-list",
            display_words.len(),
            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                let words = this
                    .timeline_drag
                    .as_ref()
                    .map(|session| session.words.as_slice())
                    .or_else(|| {
                        this.tapping
                            .as_ref()
                            .map(|session| session.words.as_slice())
                    })
                    .unwrap_or(&this.editor.project().words);
                range
                    .filter_map(|index| words.get(index).map(|line| (index, line)))
                    .map(|(index, line)| {
                        let id = line.id.clone();
                        let selected = Some(&line.id) == this.selected_word.as_ref();
                        let active = this.playing
                            && active_reference_position >= line.start
                            && active_reference_position < line.end;
                        div()
                            .id(index)
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
                            .child(
                                div()
                                    .flex_1()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(format!("{:05.2}  {}", line.start, line.text)),
                            )
                            .child(format!("{:05.2}", line.end))
                    })
                    .collect::<Vec<_>>()
            }),
        );

        div()
            .key_context("Studio")
            .on_mouse_move(cx.listener(|this,event:&gpui::MouseMoveEvent,_,cx|{
                let x=f32::from(event.position.x);
                this.move_workspace_resize(x,cx);
                this.move_resize(x,cx);
            }))
            .on_mouse_up(MouseButton::Left,cx.listener(|this,_,_,cx|{
                this.finish_workspace_resize(cx);
                this.finish_resize(cx);
            }))
            .on_mouse_up_out(MouseButton::Left,cx.listener(|this,_,_,cx|{
                this.finish_workspace_resize(cx);
                this.finish_resize(cx);
            }))
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
            .on_action(cx.listener(|this, _: &NewProject, _, cx| this.new_project(cx)))
            .on_action(cx.listener(|this, _: &ExportVideo, window, cx| this.export_video(window, cx)))
            .on_action(cx.listener(|this, _: &CancelExport, _, cx| this.cancel_export(cx)))
            .on_action(cx.listener(|this, _: &BatchRender, window, cx| this.start_pending_batch(window, cx)))
            .on_action(cx.listener(|this, _: &Open, _, cx| this.open(cx)))
            .on_action(cx.listener(|this, _: &ImportSrt, _, cx| this.import_srt(cx)))
            .on_action(cx.listener(|this, _: &ImportLyrics, _, cx| this.import_plain_lyrics(cx)))
            .on_action(cx.listener(|this, _: &ImportPreset, _, cx| this.choose_preset(cx)))
            .on_action(cx.listener(|this, _: &ExportPreset, _, cx| this.export_preset(cx)))
            .on_action(cx.listener(|this, _: &SavePreset, _, cx| this.save_custom_preset(cx)))
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
                div().flex().flex_col().gap_2().px_4().py_3().bg(rgb(theme::SURFACE))
                    .child(div().flex().items_center().justify_between().gap_3()
                        .child(div().flex().items_center().gap_3().flex_1()
                            .child(div().w(px(34.)).h(px(34.)).flex().items_center().justify_center().rounded_md().bg(rgb(theme::ACCENT_SURFACE)).text_color(rgb(theme::ACCENT)).text_size(px(15.)).child("VK"))
                            .child(div().flex().flex_col()
                                .child(div().text_size(px(14.)).child("VietKara Studio"))
                                .child(div().text_size(px(11.)).text_color(rgb(theme::TEXT_MUTED)).child(format!("{}{}", self.editor.project().name, if self.dirty { " · Chưa lưu" } else { "" }))))))
                        .child(div().flex().items_center().gap_2()
                        .child(div().cursor_pointer().rounded_md().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_3().py_2().text_size(px(12.)).child("Mới")
                            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.new_project(cx))))
                        .child(div().cursor_pointer().rounded_md().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_3().py_2().text_size(px(12.)).child("Mở")
                            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.open(cx))))
                        .child(div().cursor_pointer().rounded_md().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_3().py_2().text_size(px(12.)).child("Lưu")
                            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.save(cx))))
                        .child(div().cursor_pointer().rounded_md().bg(rgb(theme::ACCENT)).hover(|style| style.bg(rgb(0x4b8bea))).px_3().py_2().text_size(px(12.)).text_color(rgb(0xffffff))
                            .child(if self.rendering { format!("Hủy xuất · {:.0}%", self.render_progress * 100.0) } else { "Xuất video".into() })
                            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| if this.rendering { this.cancel_export(cx) } else { this.export_video(window, cx) })))
                        .child(div().cursor_pointer().rounded_md().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_3().py_2().text_size(px(12.))
                            .child(if self.show_tools_menu { "Công cụ ▴" } else { "Công cụ ▾" })
                            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| { this.show_tools_menu = !this.show_tools_menu; cx.notify(); })))))
                    .child(div().h(px(1.)).bg(rgb(theme::BORDER)))
                    .child(div().flex().items_center().flex_wrap().gap_2()
                        .child(div().text_size(px(11.)).text_color(rgb(theme::TEXT_SUBTLE)).mr_1().child("LÀM VIỆC"))
                        .child(div().cursor_pointer().rounded_md().bg(rgb(theme::ACCENT_SURFACE)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_3().py_2().text_size(px(12.)).child("Nhập lời · SRT")
                            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.import_srt(cx))))
                        .child(div().cursor_pointer().rounded_md().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_3().py_2().text_size(px(12.)).child(format!("Thư viện preset · {}", self.bundled_presets.len() + self.custom_presets.len()))
                            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.toggle_preset_catalog(cx))))
                        .child(div().cursor_pointer().rounded_md().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_3().py_2().text_size(px(12.))
                            .child(if self.pending_batch_projects.is_empty() { "Chọn project batch".to_owned() } else { format!("Batch · {} project", self.pending_batch_projects.len()) })
                            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.choose_batch_projects(cx))))
                        .when(!self.pending_batch_projects.is_empty() && !self.rendering, |bar| bar.child(
                            div().cursor_pointer().rounded_md().bg(rgb(theme::SUCCESS)).px_3().py_2().text_size(px(12.)).child("Bắt đầu batch")
                                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| this.start_pending_batch(window, cx)))
                        ))
                        .when(!self.rendering && self.batch_record.as_ref().is_some_and(|record| record.status != BatchStatus::Done || record.failed > 0), |bar| bar.child(
                            div().cursor_pointer().rounded_md().bg(rgb(theme::CANVAS)).px_3().py_2().text_size(px(12.)).child("Tiếp tục batch")
                                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| this.resume_batch(window, cx)))
                        )))
                    .when(self.show_tools_menu, |bar| bar.child(div().flex().items_center().flex_wrap().gap_2().pt_1()
                        .child(div().text_size(px(11.)).text_color(rgb(theme::TEXT_SUBTLE)).mr_1().child("CÔNG CỤ NÂNG CAO"))
                        .child(div().cursor_pointer().rounded_md().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_3().py_2().text_size(px(12.)).child("Nhập preset")
                            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.choose_preset(cx))))
                        .child(div().cursor_pointer().rounded_md().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_3().py_2().text_size(px(12.)).child("Xuất preset")
                            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.export_preset(cx))))
                        .child(div().cursor_pointer().rounded_md().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_3().py_2().text_size(px(12.)).child("Lưu preset hiện tại")
                            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.save_custom_preset(cx))))
                        .child(div().cursor_pointer().rounded_md().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_3().py_2().text_size(px(12.)).child(if self.diagnostics_running { "Đang chẩn đoán…" } else { "Chẩn đoán" })
                            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.run_diagnostics(cx))))
                        .child(div().cursor_pointer().rounded_md().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_3().py_2().text_size(px(12.)).child("Cập nhật")
                            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.run_updater(cx))))),
            )
            .when(self.show_preset_catalog, |root| root.child(
                div().id("preset-catalog").h(px(260.)).overflow_y_scroll().flex().flex_col().gap_1().p_2()
                    .bg(rgb(theme::SURFACE)).border_b_1().border_color(rgb(theme::BORDER))
                    .child(div().px_2().py_1().text_size(px(11.)).text_color(rgb(theme::TEXT_MUTED)).child("THƯ VIỆN CỦA TÔI · lưu trong dữ liệu ứng dụng"))
                    .children(self.custom_presets.iter().map(|stored| {
                        let id = stored.id.clone();
                        let name = stored.preset.name.clone();
                        let description = stored.preset.description.clone();
                        let preset = stored.preset.clone();
                        let confirm_delete = self.pending_preset_delete.as_deref() == Some(id.as_str());
                        div().flex().items_center().gap_2().rounded_sm().bg(rgb(theme::CANVAS)).px_3().py_2()
                            .child(div().flex_1().child(div().text_size(px(13.)).child(name))
                                .child(div().text_size(px(11.)).text_color(rgb(theme::TEXT_MUTED)).child(description)))
                            .child(div().cursor_pointer().rounded_sm().bg(rgb(theme::ACCENT_SURFACE)).px_2().py_1().child("Dùng")
                                .on_mouse_down(MouseButton::Left, cx.listener(move |this, _, _, cx| this.preview_preset(preset.clone(), cx))))
                            .when(confirm_delete, |row| row
                                .child(div().cursor_pointer().rounded_sm().bg(rgb(theme::DANGER)).px_2().py_1().child("Xác nhận xóa")
                                    .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.delete_custom_preset(cx))))
                                .child(div().cursor_pointer().px_2().py_1().child("Hủy")
                                    .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.cancel_delete_preset(cx)))))
                            .when(!confirm_delete, |row| row
                                .child(div().cursor_pointer().rounded_sm().px_2().py_1().child("Xóa")
                                    .on_mouse_down(MouseButton::Left, cx.listener(move |this, _, _, cx| this.request_delete_preset(id.clone(), cx)))))
                    }))
                    .child(div().px_2().py_1().text_size(px(11.)).text_color(rgb(theme::TEXT_MUTED)).child("PRESET MẪU"))
                    .children(self.bundled_presets.iter().map(|preset| {
                        let preset = preset.clone();
                        let name = preset.name.clone();
                        let description = preset.description.clone();
                        div().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER)))
                            .px_3().py_2()
                            .child(div().text_size(px(13.)).child(name))
                            .child(div().text_size(px(11.)).text_color(rgb(theme::TEXT_MUTED)).child(description))
                            .on_mouse_down(MouseButton::Left, cx.listener(move |this, _, _, cx| this.preview_preset(preset.clone(), cx)))
                    }))
            ))
            .when(self.diagnostics_report.is_some(), |root| root.child(
                div().flex().flex_col().gap_1().p_3().bg(rgb(theme::SURFACE)).border_b_1().border_color(rgb(theme::BORDER))
                    .child(div().flex().justify_between().items_center()
                        .child("Chẩn đoán cục bộ · báo cáo không gửi ra mạng")
                        .child(div().cursor_pointer().px_2().py_1().child("Ẩn")
                            .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.hide_diagnostics(cx)))))
                    .child(div().id("diagnostics-report").h(px(190.)).overflow_y_scroll().flex().flex_col().gap_1()
                        .children(self.diagnostics_report.as_deref().unwrap_or_default().lines().map(|line|
                            div().text_size(px(12.)).child(line.to_owned())
                        )))
            ))
            .when(
                self.batch_record
                    .as_ref()
                    .is_some_and(|record| record.status != BatchStatus::Done || record.failed > 0),
                |bar| {
                    let record = self.batch_record.as_ref().expect("batch record checked");
                    let errors = record
                        .items
                        .iter()
                        .filter(|item| item.status == ItemStatus::Error)
                        .take(3)
                        .map(|item| {
                            format!(
                                "{}: {}",
                                item.name,
                                item.error.as_deref().unwrap_or("Lỗi không rõ")
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(" · ");
                    bar.child(
                        div()
                            .px_5()
                            .py_1()
                            .text_sm()
                            .child(format!(
                                "Batch: {}/{} hoàn tất, {} lỗi{}{}",
                                record.completed,
                                record.items.len(),
                                record.failed,
                                if errors.is_empty() { "" } else { " — " },
                                errors
                            )),
                    )
                },
            )
            .child(div().flex().items_center().gap_2().px_5().py_2().bg(rgb(theme::SURFACE))
                .child(div().cursor_pointer().rounded_md().bg(rgb(theme::ACCENT_SURFACE)).hover(|style|style.bg(rgb(theme::SURFACE_HOVER))).px_3().py_2().text_size(px(12.)).child("Bắt đầu căn nhịp · Ctrl+T")
                    .on_mouse_down(MouseButton::Left,cx.listener(|this, _, _, cx|this.begin_tapping(cx))))
                .when(self.tapping.is_some(),|bar|bar
                    .child(div().cursor_pointer().rounded_md().bg(rgb(theme::CANVAS)).hover(|style|style.bg(rgb(theme::SURFACE_HOVER))).px_3().py_2().text_size(px(12.)).child("Từ trước · Backspace")
                        .on_mouse_down(MouseButton::Left,cx.listener(|this, _, _, cx|this.previous_tap(cx))))
                    .child(div().cursor_pointer().rounded_md().bg(rgb(theme::SUCCESS_ACTIVE)).hover(|style|style.bg(rgb(theme::SUCCESS))).px_3().py_2().text_size(px(12.)).child("Hoàn tất · Enter")
                        .on_mouse_down(MouseButton::Left,cx.listener(|this, _, _, cx|this.finish_tapping(true,cx))))
                    .child(format!("Từ {} / {}",self.tapping.as_ref().map(|s|s.clock.index+1).unwrap_or(0),self.editor.project().words.len()))))
            .child(div().flex().items_center().flex_wrap().gap_2().px_5().py_2()
                .child(div().cursor_pointer().rounded_sm().bg(rgb(theme::ACCENT_SURFACE)).hover(|style|style.bg(rgb(theme::SURFACE_HOVER))).px_3().py_1()
                    .child(if self.sync_running && !self.role_suggest_running && !self.lyrics_ai_running { "Hủy phân tích đồng bộ" } else { "Gợi ý đồng bộ bản hát/beat" })
                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.suggest_audio_sync(cx))))
                .when(self.sync_running && !self.role_suggest_running && !self.lyrics_ai_running,|bar|bar
                    .child(format!("{:.0}%",self.sync_progress))
                    .child(self.edit_status.clone()))
                .child(div().cursor_pointer().rounded_sm().bg(rgb(theme::ACCENT_SURFACE)).hover(|style|style.bg(rgb(theme::SURFACE_HOVER))).px_3().py_1()
                    .child(if self.role_suggest_running { "Hủy gợi ý vai hát" } else { "Gợi ý vai Nam/Nữ" })
                    .opacity(if self.sync_running && !self.role_suggest_running {0.55} else {1.0})
                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.suggest_roles(cx))))
                .when(self.role_suggest_running,|bar|bar
                    .child(format!("{:.0}%",self.sync_progress))
                    .child(self.edit_status.clone()))
                .child(div().cursor_pointer().rounded_sm().bg(rgb(theme::ACCENT_SURFACE)).hover(|style|style.bg(rgb(theme::SURFACE_HOVER))).px_3().py_1()
                    .child(if self.lyrics_ai_running { "Hủy nhận dạng / căn lời" } else { "AI nhận dạng / căn lời" })
                    .opacity(if self.sync_running && !self.lyrics_ai_running {0.55} else {1.0})
                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.run_lyrics_ai(None,cx))))
                .when(self.lyrics_ai_running,|bar|bar
                    .child(format!("{:.0}%",self.sync_progress))
                    .child(self.edit_status.clone()))
                .child(div().cursor_pointer().rounded_sm().bg(rgb(theme::SURFACE)).hover(|style|style.bg(rgb(theme::SURFACE_HOVER))).px_3().py_1()
                    .child(format!("AI: {}", if self.ai_device_preference == "auto" { "Tự động · NVIDIA/CPU" } else { "CPU" }))
                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.toggle_ai_device(cx))))
                .child(div().cursor_pointer().rounded_sm().bg(rgb(theme::SURFACE)).hover(|style|style.bg(rgb(theme::SURFACE_HOVER))).px_3().py_1()
                    .child(format!("Whisper: {}", self.ai_model))
                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.cycle_ai_model(cx)))))
            .when(!missing_audio.is_empty(), |root| root.child(
                div().flex().items_center().gap_3().px_4().py_2().bg(rgb(theme::ACCENT_SURFACE))
                    .child("Thiếu file audio trong thư mục project:")
                    .children(missing_audio.iter().map(|slot| {
                        let slot = *slot;
                        div().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).px_3().py_1()
                            .child(match slot {
                                AudioSlot::Beat => "Relink beat gốc",
                                AudioSlot::Reference => "Relink bản hát",
                            })
                            .on_mouse_down(MouseButton::Left,cx.listener(move |this,_,_,cx|this.choose_audio(slot,cx)))
                    }))
            ))
            .when(self.pending_audio.is_some(),|root|root.child(div().flex().gap_3().p_3()
                .child(if pending_audio_relink {
                    "File audio gốc đang thiếu. Nếu chọn đúng nguồn cũ, app sẽ nhận diện bằng fingerprint và giữ mốc/lời; chọn track khác sẽ thay media và có thể làm mới dữ liệu liên quan."
                } else {
                    "Nhập media sẽ lưu project và tạo phiên bản trước thay đổi. Media khác có thể xóa mốc đồng bộ; thay bản hát có thể xóa lời cũ."
                })
                .child(div().cursor_pointer().child("Nhập media")
                    .on_mouse_down(MouseButton::Left,cx.listener(|this, _, _, cx|this.accept_audio(cx))))
                .child(div().cursor_pointer().child("Hủy")
                    .on_mouse_down(MouseButton::Left,cx.listener(|this, _, _, cx|{this.pending_audio=None;cx.notify();})))))
            .when(self.pending_lyrics.is_some(), |root| root.child(div().flex().gap_3().p_3()
                .child("Thay lời sẽ cập nhật toàn bộ từ và mốc hiện tại. Project đã lưu sẽ tạo phiên bản khôi phục trước khi thay.")
                .child(div().cursor_pointer().px_3().child("Thay toàn bộ lời")
                    .on_mouse_down(MouseButton::Left,cx.listener(|this, _, _, cx|this.accept_lyrics(cx))))
                .child(div().cursor_pointer().px_3().opacity(if self.saving { 0.5 } else { 1. }).child("Hủy")
                    .on_mouse_down(MouseButton::Left,cx.listener(|this, _, _, cx|{
                        if !this.saving {
                            this.pending_lyrics=None;
                            this.edit_status="Đã hủy nhập lời".into();
                            cx.notify();
                        }
                    })))))
            .when(self.pending_sync.is_some(), |root| root.child(div().flex().items_center().gap_3().p_3().bg(rgb(theme::ACCENT_SURFACE))
                .child(format!("Gợi ý đồng bộ · {} · lệch {:.2}s · tỷ lệ {:.3}",
                    self.pending_sync.as_ref().and_then(|item|item.result.get("confidence")).and_then(Value::as_str).unwrap_or("chưa rõ"),
                    self.pending_sync.as_ref().and_then(|item|item.result.get("offset_seconds")).and_then(Value::as_f64).unwrap_or(0.0),
                    self.pending_sync.as_ref().and_then(|item|item.result.get("beat_per_reference")).and_then(Value::as_f64).unwrap_or(1.0)))
                .child(self.pending_sync.as_ref().and_then(|item|item.result.get("structure")).and_then(Value::as_str).unwrap_or("").to_owned())
                .child(div().cursor_pointer().rounded_sm().bg(rgb(theme::SUCCESS_ACTIVE)).px_3().py_1().child("Áp dụng mốc (một bước hoàn tác)")
                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.accept_sync_suggestion(cx))))
                .child(div().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).px_3().py_1().child("Bỏ gợi ý")
                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.dismiss_sync_suggestion(cx))))))
            .when(self.pending_role_suggestion.is_some(), |root| root.child(div().flex().items_center().gap_3().p_3().bg(rgb(theme::ACCENT_SURFACE))
                .child(format!("Gợi ý phân vai · Nam {} · Nữ {} · Cùng hát {} · chưa rõ {} · {}",
                    self.pending_role_suggestion.as_ref().and_then(|item|item.result.get("male")).and_then(Value::as_u64).unwrap_or(0),
                    self.pending_role_suggestion.as_ref().and_then(|item|item.result.get("female")).and_then(Value::as_u64).unwrap_or(0),
                    self.pending_role_suggestion.as_ref().and_then(|item|item.result.get("shared")).and_then(Value::as_u64).unwrap_or(0),
                    self.pending_role_suggestion.as_ref().and_then(|item|item.result.get("uncertain")).and_then(Value::as_u64).unwrap_or(0),
                    self.pending_role_suggestion.as_ref().and_then(|item|item.result.get("method")).and_then(Value::as_str).unwrap_or("cao độ vocal")))
                .child("Chỉ áp dụng sau khi xác nhận; từ đã khóa được giữ nguyên.")
                .child(div().cursor_pointer().rounded_sm().bg(rgb(theme::SUCCESS_ACTIVE)).px_3().py_1().child("Áp dụng vai · một bước hoàn tác")
                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.accept_role_suggestion(cx))))
                .child(div().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).px_3().py_1().child("Bỏ gợi ý")
                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.dismiss_role_suggestion(cx))))))
            .when(self.pending_lyrics_ai.is_some(), |root| root.child(div().flex().items_center().gap_3().p_3().bg(rgb(theme::ACCENT_SURFACE))
                .child(format!("Xem trước {} · {} từ sẽ thay · {} từ cần rà trong project · {} câu",
                    self.pending_lyrics_ai.as_ref().map(|item|item.operation).unwrap_or("AI"),
                    self.pending_lyrics_ai.as_ref().map(|item|item.changed_words).unwrap_or(0),
                    self.pending_lyrics_ai.as_ref().map(|item|item.words.iter().filter(|word|word.estimated).count()).unwrap_or(0),
                    self.pending_lyrics_ai.as_ref().map(|item|item.words.iter().map(|word|word.line).collect::<std::collections::HashSet<_>>().len()).unwrap_or(0)))
                .child(self.pending_lyrics_ai.as_ref().map(|item|item.text.chars().take(240).collect::<String>()).unwrap_or_default())
                .child("Project chưa bị thay; xác nhận mới ghi lời và mốc.")
                .child(div().cursor_pointer().rounded_sm().bg(rgb(theme::SUCCESS_ACTIVE)).px_3().py_1().child(format!("Áp dụng {} · một bước hoàn tác",self.pending_lyrics_ai.as_ref().map(|item|item.operation).unwrap_or("AI")))
                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.accept_lyrics_ai(cx))))
                .child(div().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).px_3().py_1().child("Bỏ kết quả")
                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.dismiss_lyrics_ai(cx))))))
            .when(self.confirm_close, |root| root.child(div().flex().gap_3().p_3()
                .child("Project chưa lưu. Lưu trước khi đóng hoặc chọn đóng không lưu.")
                .child(div().cursor_pointer().px_3().child("Lưu và đóng")
                    .on_mouse_down(MouseButton::Left,cx.listener(|this, _, window, cx| {
                        this.save_and_close(window, cx);
                    })))
                .child(div().cursor_pointer().px_3().child("Đóng không lưu")
                    .on_mouse_down(MouseButton::Left,cx.listener(|this, _, window, _| {
                        if !this.saving && !this.autosaving { window.remove_window(); }
                    })))
                .child(div().cursor_pointer().px_3().child("Tiếp tục chỉnh sửa")
                    .on_mouse_down(MouseButton::Left,cx.listener(|this, _, _, cx| {
                        this.confirm_close=false;
                        this.close_after_save_window=None;
                        cx.notify();
                    })))))
            .when(self.confirm_open || self.confirm_new, |root| root.child(div().flex().gap_3().p_3()
                .child(if self.confirm_new {
                    "Project chưa lưu. Lưu hoặc bỏ thay đổi trước khi tạo project mới."
                } else {
                    "Project chưa lưu. Lưu hoặc bỏ thay đổi trước khi mở project khác."
                })
                .child(div().cursor_pointer().px_3().child(if self.confirm_new { "Bỏ thay đổi và tạo mới" } else { "Bỏ thay đổi và mở" })
                    .on_mouse_down(MouseButton::Left,cx.listener(|this, _, _, cx|this.discard_and_navigate(cx))))
                .child(div().cursor_pointer().px_3().child(if self.confirm_new { "Lưu và tạo mới" } else { "Lưu và mở" })
                    .on_mouse_down(MouseButton::Left,cx.listener(|this, _, _, cx|this.save_and_navigate(cx))))
                .child(div().cursor_pointer().px_3().child("Hủy")
                    .on_mouse_down(MouseButton::Left,cx.listener(|this, _, _, cx|{this.confirm_open=false;this.confirm_new=false;cx.notify();})))))
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
                    .items_center()
                    .flex_wrap()
                    .gap_2()
                    .px_5()
                    .py_2()
                    .bg(rgb(theme::SURFACE))
                    .child(
                        div()
                            .px_3()
                            .py_2()
                            .rounded_md()
                            .bg(rgb(theme::CANVAS))
                            .hover(|style| style.bg(rgb(theme::SURFACE_HOVER)))
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
                            .py_2()
                            .rounded_md()
                            .bg(rgb(theme::CANVAS))
                            .hover(|style| style.bg(rgb(theme::SURFACE_HOVER)))
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
                            .py_2()
                            .rounded_md()
                            .bg(rgb(theme::CANVAS))
                            .hover(|style| style.bg(rgb(theme::SURFACE_HOVER)))
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
                            .px_2()
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
                    .min_h(px(0.))
                    .gap_3()
                    .p_3()
                    .child(
                        div()
                            .w(px(workspace.left))
                            .flex_shrink_0()
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
                                .child(div().mt_3().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2()
                                    .child(audio_slot_label(self.editor.project(),self.path.as_deref(),AudioSlot::Beat))
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.choose_audio(AudioSlot::Beat,cx))))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2()
                                    .child(audio_slot_label(self.editor.project(),self.path.as_deref(),AudioSlot::Reference))
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.choose_audio(AudioSlot::Reference,cx))))
                                .child(div().mt_3().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2()
                                    .child(if background_asset.is_some() {"▧  Thay ảnh nền"} else {"＋  Nạp ảnh nền"})
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.choose_background_image(cx))))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2()
                                    .child(if background_type == "video" {"▶  Thay video nền"} else {"＋  Nạp video nền"})
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.choose_background_video(cx))))
                                .child(div().mt_4().text_color(rgb(theme::TEXT_MUTED)).child("Project JSON"))
                                .child(div().mt_2().text_size(px(11.)).child(self.path.as_ref().map(|path|path.display().to_string()).unwrap_or_else(||"Chưa lưu project".into()))))
                            .when(!self.left_tab_media,|panel|panel
                                .child(div().text_size(px(11.)).text_color(rgb(theme::TEXT_MUTED))
                                    .child(format!("{} từ · {} dòng",self.editor.project().words.len(),self.editor.project().words.iter().map(|w|w.line).max().map(|line|line+1).unwrap_or(0))))
                                .when(display_words.is_empty(),|panel|panel.child(
                                    div().flex_1().flex().flex_col().items_center().justify_center().gap_3().px_3()
                                        .child(div().w(px(44.)).h(px(44.)).flex().items_center().justify_center().rounded_lg().bg(rgb(theme::ACCENT_SURFACE)).text_size(px(23.)).text_color(rgb(theme::ACCENT)).child("♫"))
                                        .child(div().text_size(px(14.)).child("Bắt đầu với lời bài hát"))
                                        .child(div().text_size(px(11.)).text_color(rgb(theme::TEXT_MUTED)).child("Nhập SRT hoặc TXT để tạo các dòng karaoke."))
                                        .child(div().w_full().cursor_pointer().rounded_md().bg(rgb(theme::ACCENT)).hover(|style|style.bg(rgb(0x4b8bea))).px_3().py_2().text_size(px(12.)).text_color(rgb(0xffffff)).text_center().child("Nhập lời · SRT")
                                            .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.import_srt(cx))))
                                        .child(div().cursor_pointer().rounded_md().bg(rgb(theme::CANVAS)).hover(|style|style.bg(rgb(theme::SURFACE_HOVER))).px_3().py_2().text_size(px(12.)).child("Nhập lời · TXT")
                                            .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.import_plain_lyrics(cx))))
                                )))
                                .when(!display_words.is_empty(),|panel|panel
                                    .child(lyric_rows.flex_1().mt_2())
                                    .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2()
                                        .child("✎  Ước lượng mốc từ TXT")
                                        .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.import_plain_lyrics(cx))))),
                    )
                    .child(
                        div()
                            .id("resize-lyrics-divider")
                            .w(px(6.))
                            .flex_shrink_0()
                            .h_full()
                            .rounded_sm()
                            .bg(rgb(theme::BORDER))
                            .hover(|style| style.bg(rgb(theme::ACCENT)))
                            .on_mouse_down(MouseButton::Left,cx.listener(|this,event:&gpui::MouseDownEvent,window,cx|{
                                this.start_workspace_resize(WorkspaceEdge::Left,f32::from(event.position.x),f32::from(window.viewport_size().width),cx);
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_h(px(0.))
                            .overflow_hidden()
                            .gap_3()
                            .child(
                                div()
                                    .flex_1()
                                    .min_h(px(0.))
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
                                                        let countdown_layout = active_countdown.unwrap();
                                                        let remaining = (countdown_layout.end - preview_position).ceil().clamp(1.0, 4.0) as usize;
                                                        let size = countdown_layout.size as f32 * preview_scale;
                                                        rows.child(div().absolute()
                                                            .left(px(countdown_layout.x as f32 * preview_scale))
                                                            .top(px(countdown_layout.y as f32 * preview_scale))
                                                            .flex().gap(px(size * 0.28))
                                                            .children((0..remaining).map(|_| {
                                                                div().w(px(size)).h(px(size)).flex().items_center().justify_center()
                                                                    .when(countdown == "image" && countdown_path.is_some(), |box_| box_.child(img(countdown_path.clone().unwrap()).size_full().object_fit(ObjectFit::Contain)))
                                                                    .when(countdown != "image", |box_| box_.text_size(px(size)).text_color(rgb(0xf4cf72)).child(glyph))
                                                            })))
                                                    })
                                                .when(preview_rows.is_empty() && !title_visible,|empty|empty
                                                    .child(div().absolute().left(px(0.)).top(px(0.)).w_full().h_full().flex().items_center().justify_center()
                                                        .child(div().flex().flex_col().items_center().gap_3().rounded_lg().bg(rgb(0x192431)).px_6().py_5()
                                                            .child(div().w(px(48.)).h(px(48.)).flex().items_center().justify_center().rounded_lg().bg(rgb(theme::ACCENT_SURFACE)).text_size(px(25.)).text_color(rgb(theme::ACCENT)).child("♫"))
                                                            .child(div().text_size(px(15.)).child(if display_words.is_empty(){"Tạo preview karaoke"}else{"Chọn một câu để xem trước"}))
                                                            .child(div().text_size(px(12.)).text_color(rgb(theme::TEXT_MUTED)).child(if display_words.is_empty(){"Nhập lời bài hát để bắt đầu căn nhịp."}else{"Di chuyển playhead đến câu muốn xem."}))
                                                            .when(display_words.is_empty(),|card|card.child(div().cursor_pointer().rounded_md().bg(rgb(theme::ACCENT)).hover(|style|style.bg(rgb(0x4b8bea))).px_4().py_2().text_size(px(12.)).text_color(rgb(0xffffff)).child("Nhập lời · SRT")
                                                                .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.import_srt(cx)))))))))),
                                    ),
                            )
                            .child(
                                div()
                                    .h(px(match layout_mode {
                                        WorkspaceMode::Compact => 260.,
                                        WorkspaceMode::Standard => 320.,
                                        WorkspaceMode::Wide => 390.,
                                    }))
                                    .bg(rgb(theme::SURFACE))
                                    .rounded_md()
                                    .p_3()
                                    .child(timeline)
                                    .child(div().flex().justify_between().items_center().mt_2()
                                        .child("TIMELINE · Kéo khối để chọn, kéo mép để chỉnh mốc")
                                        .child(div().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_1()
                                            .child(if self.editor.project().settings.timeline_linked {"Mốc liên kết"}else{"Mốc độc lập"})
                                            .on_mouse_down(MouseButton::Left,cx.listener(|this, _, _, cx|this.toggle_linked(cx))))),
                            ),
                    )
                    .child(
                        div()
                            .id("resize-inspector-divider")
                            .w(px(6.))
                            .flex_shrink_0()
                            .h_full()
                            .rounded_sm()
                            .bg(rgb(theme::BORDER))
                            .hover(|style| style.bg(rgb(theme::ACCENT)))
                            .on_mouse_down(MouseButton::Left,cx.listener(|this,event:&gpui::MouseDownEvent,window,cx|{
                                this.start_workspace_resize(WorkspaceEdge::Right,f32::from(event.position.x),f32::from(window.viewport_size().width),cx);
                            })),
                    )
                    .child(
                        div()
                            .w(px(workspace.right))
                            .flex_shrink_0()
                            .flex()
                            .flex_col()
                            .id("design-inspector")
                            .overflow_y_scroll()
                            .bg(rgb(theme::SURFACE))
                            .rounded_md()
                            .p_3()
                            .child(div().flex().gap_1().mb_3().p_1().rounded_md().bg(rgb(theme::CANVAS))
                                .child(div().flex_1().text_center().cursor_pointer().px_2().py_2().rounded_sm().text_size(px(12.))
                                    .bg(rgb(if !self.inspector_design {theme::ACCENT_SURFACE}else{theme::CANVAS}))
                                    .child("Từ")
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|{
                                        this.inspector_design=false;
                                        let text=this.selected_word.as_ref().and_then(|id|this.editor.project().words.iter().find(|word|&word.id==id)).map(|word|word.text.clone()).unwrap_or_default();
                                        this.set_word_input(&text,cx);
                                        cx.notify();
                                    })))
                                .child(div().flex_1().text_center().cursor_pointer().px_2().py_2().rounded_sm().text_size(px(12.))
                                    .bg(rgb(if self.inspector_design {theme::ACCENT_SURFACE}else{theme::CANVAS}))
                                    .child("Thiết kế")
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|{
                                        this.inspector_design=true;
                                        let text=this.editor.project().settings.extra.get("title_text").and_then(Value::as_str).unwrap_or("").to_owned();
                                        this.set_word_input(&text,cx);
                                        cx.notify();
                                    }))))
                            .when(!self.inspector_design,|panel|panel
                            .child(div().text_size(px(11.)).text_color(rgb(theme::TEXT_SUBTLE)).child("TỪ ĐANG CHỌN"))
                            .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::ACCENT_SURFACE)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2()
                                .child("AI căn câu này")
                                .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|{
                                    let line=this.selected_word.as_ref().and_then(|id|this.editor.project().words.iter().find(|word|&word.id==id)).map(|word|word.line);
                                    if let Some(line)=line { this.run_lyrics_ai(Some(line),cx); }
                                    else { this.edit_status="Chọn một từ trong câu cần căn lại trước".into();cx.notify(); }
                                })))
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
                                .child(div().flex_1().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2()
                                    .child(format!("Vai: {selected_role}"))
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.cycle_selected_role(cx)))))
                            .child(div().mt_4().text_size(px(11.)).text_color(rgb(theme::TEXT_MUTED)).child("SECTION / SCENE"))
                            .child(div().mt_1().text_color(rgb(theme::TEXT_SUBTLE)).child(if selected_section.is_empty() {"Chưa đặt tên section".to_owned()} else {selected_section.clone()}))
                            .child(div().mt_2().child(self.section_name_input.clone()))
                            .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2()
                                .child("Đặt tên cho cả dòng")
                                .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.apply_section_name(cx))))
                            .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2()
                                .child("Chọn loại section scene tiếp theo")
                                .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.set_selected_scene_section(cx))))
                            .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::SUCCESS)).px_2().py_2()
                                .child(if selected_scene_image_exists {"Thay ảnh scene cho section"} else {"＋  Nạp ảnh scene cho section"})
                                .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.choose_scene_image(cx))))
                            .when(selected_scene_image_exists, |panel| panel.child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2()
                                .child("Bỏ ảnh nền scene (giữ trang trí)")
                                .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.remove_scene_image(cx)))))
                            .when(selected_scene_exists, |panel| panel
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2()
                                    .child(if self.editor.project().scenes.iter().find(|scene| scene.get("section_name").and_then(Value::as_str) == Some(selected_section.as_str())).and_then(|scene| scene.get("transition")).and_then(Value::as_str) == Some("cut") {"Chuyển cảnh: cắt thẳng"} else {"Chuyển cảnh: mờ 0,6 giây"})
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.cycle_selected_scene_option("transition", &["fade","cut"],cx))))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2()
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
                                    .child(div().flex_1().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2().child("Trang trí −")
                                        .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_selected_scene_number("decoration_size",-2.0,5.0,80.0,cx))))
                                    .child(div().flex_1().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2().child("Trang trí +")
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
                                    .child(div().flex_1().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2().child("Scene mờ −")
                                        .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_selected_scene_number("opacity",-5.0,0.0,100.0,cx))))
                                    .child(div().flex_1().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2().child("Scene mờ +")
                                        .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_selected_scene_number("opacity",5.0,0.0,100.0,cx)))))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(0x302020)).px_2().py_2().child("Bỏ ảnh trang trí")
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.remove_scene_decoration(cx)))))
                            .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2()
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
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2()
                                    .child(match display_mode {"lyricsmv"=>"Lời nổi bật theo câu", "subtitle"=>"Phụ đề", _=>"Karaoke · quét từng từ"})
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.cycle_setting("display_mode",&["karaoke","lyricsmv","subtitle"],cx))))
                                .when(display_mode == "subtitle",|panel|panel
                                    .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2()
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
                                                div().flex_1().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2().child(label)
                                                    .on_mouse_down(MouseButton::Left,cx.listener(move |this,_,_,cx|this.update_setting("subtitle_box_color",Value::String(color.into()),cx))))))))
                                .child(div().mt_3().text_size(px(11.)).text_color(rgb(theme::TEXT_MUTED)).child("KHUNG HÌNH"))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2()
                                    .child(if is_portrait{"9:16 · Dọc"}else{"16:9 · Ngang"})
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|{
                                        let next=if this.editor.project().settings.aspect==vietkara_domain::settings::Aspect::Portrait {vietkara_domain::settings::Aspect::Landscape} else {vietkara_domain::settings::Aspect::Portrait};
                                        let mut settings=this.editor.project().settings.clone();settings.aspect=next;
                                        this.edit_status=match this.editor.execute(Command::SetSettings(settings)){Ok(_)=>{this.dirty=true;this.autosave_pending=true;"Đã đổi khung hình • chưa lưu".into()},Err(error)=>error.to_string()};cx.notify();
                                    })))
                                .child(div().mt_3().text_size(px(11.)).text_color(rgb(theme::TEXT_MUTED)).child("PHÔNG CHỮ"))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2()
                                    .child(if self.show_font_catalog {"Ẩn thư viện font".to_owned()} else {format!("Chọn font · {} mẫu", self.font_catalog.len())})
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.toggle_font_catalog(cx))))
                                .when(self.show_font_catalog, |panel| panel
                                    .child(self.font_query_input.clone())
                                    .child(div().id("font-family-list").h(px(230.)).overflow_y_scroll().flex().flex_col().gap_1().mt_1()
                                        .children(self.font_catalog.iter().enumerate()
                                            .filter(|(_, family)| font_query.is_empty() || family.family.to_lowercase().contains(&font_query))
                                            .map(|(index, family)| {
                                                let name = family.family.clone();
                                                let encoding = if family.encoding == "unicode" {"Unicode"} else {"VNI/TCVN3"};
                                                let selected = settings.font.eq_ignore_ascii_case(&name);
                                                div().id(("font-family", index)).cursor_pointer().rounded_sm()
                                                    .bg(rgb(if selected {theme::ACCENT_SURFACE} else {theme::CANVAS}))
                                                    .hover(|style| style.bg(rgb(theme::SURFACE_HOVER)))
                                                    .px_2().py_2()
                                                    .child(div().font_family(name.clone()).text_size(px(14.)).child(name.clone()))
                                                    .child(div().text_size(px(10.)).text_color(rgb(theme::TEXT_MUTED)).child(encoding))
                                                    .on_mouse_down(MouseButton::Left,cx.listener(move |this,_,_,cx|this.select_font(name.clone(),cx)))
                                            }))))
                                .child(div().mt_2().flex().justify_between().items_center().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2()
                                    .child(settings.font.clone())
                                    .child(div().flex().gap_3()
                                        .child(div().cursor_pointer().child("−").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.font_size(-1.,cx))))
                                        .child(div().cursor_pointer().child("+").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.font_size(1.,cx))))))
                                .child(div().mt_2().text_size(px(11.)).text_color(rgb(theme::TEXT_MUTED)).child(format!("{} px · Tiếng Việt: Trường xưa, nắng mới",settings.font_size)))
                                .child(div().mt_3().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2()
                                    .child(if italic{"✓  Chữ nghiêng"}else{"Chữ nghiêng"})
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|{let v=!this.editor.project().settings.extra.get("italic").and_then(Value::as_bool).unwrap_or(false);this.update_setting("italic",Value::Bool(v),cx);})))
                                .child(div().mt_3().text_size(px(11.)).text_color(rgb(theme::TEXT_MUTED)).child("NỀN KHUNG HÌNH"))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2()
                                    .child(match background_type {"image"=>"Đang dùng ảnh nền","video"=>"Đang dùng video nền",_=>"Đang dùng nền màu"})
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.cycle_setting("background_type",&["color","image"],cx))))
                                .child(div().mt_2().flex().gap_2()
                                    .children([("Xanh đêm","#101d2b"),("Đen","#090c12"),("Tím","#221c35")].into_iter().map(|(label,color)|
                                        div().flex_1().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2()
                                            .child(div().flex().gap_1().items_center()
                                                .child(div().w(px(10.)).h(px(10.)).rounded_sm().bg(rgb(match color {"#090c12"=>0x090c12,"#221c35"=>0x221c35,_=>0x101d2b})))
                                                .child(label))
                                            .on_mouse_down(MouseButton::Left,cx.listener(move |this,_,_,cx|this.update_setting("background",Value::String(color.into()),cx))))))
                                .child(div().mt_4().text_size(px(11.)).text_color(rgb(theme::TEXT_MUTED)).child("LOGO"))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2()
                                    .child(if logo_path.is_some() {"＋  Thay logo PNG"} else {"＋  Nạp logo PNG"})
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.choose_logo(cx))))
                                .when(logo_path.is_some(),|panel|panel
                                    .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2()
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
                                    .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2()
                                        .child(format!("Hiệu ứng: {logo_effect}"))
                                        .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.cycle_setting("logo_effect",&["none","fade","pulse","float"],cx)))))
                                .child(div().mt_3().text_size(px(11.)).text_color(rgb(theme::TEXT_MUTED)).child("VỊ TRÍ LỜI"))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2()
                                    .child(format!("Vị trí: {position_label}"))
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.cycle_setting("position",&["bottom","center","top"],cx))))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2()
                                    .child(format!("Căn lề: {align_label}"))
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.cycle_setting("align",&["center","left","right","justify"],cx))))
                                .when(display_mode == "karaoke", |panel| panel
                                    .child(div().mt_4().text_size(px(11.)).text_color(rgb(theme::TEXT_MUTED)).child("HIỆU ỨNG QUÉT CHỮ"))
                                    .child(div().mt_2().flex().justify_between().items_center().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2()
                                        .child(format!("Mềm mép: {sweep_feather:.0}"))
                                        .child(div().flex().gap_3()
                                            .child(div().cursor_pointer().child("−").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_title_number("sweep_feather",-2.0,0.0,40.0,cx))))
                                            .child(div().cursor_pointer().child("+").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_title_number("sweep_feather",2.0,0.0,40.0,cx))))))
                                    .child(div().mt_2().flex().justify_between().items_center().rounded_sm().bg(rgb(theme::CANVAS)).px_2().py_2()
                                        .child(format!("Ánh sáng quét: {sweep_glow:.0}"))
                                        .child(div().flex().gap_3()
                                            .child(div().cursor_pointer().child("−").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_title_number("sweep_glow",-10.0,0.0,200.0,cx))))
                                            .child(div().cursor_pointer().child("+").on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.adjust_title_number("sweep_glow",10.0,0.0,200.0,cx)))))))
                                .child(div().mt_4().text_size(px(11.)).text_color(rgb(theme::TEXT_MUTED)).child("DUET · ROLE MARKER"))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2()
                                    .child(if singing_mode == "duet" {"✓  Hai lane duet"} else {"Solo · một lane"})
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.cycle_setting("singing_mode",&["solo","duet"],cx))))
                                .when(singing_mode == "duet",|panel|panel.child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2()
                                    .child(if role_icons {"✓  Hiện biểu tượng vai hát"} else {"Ẩn biểu tượng vai hát"})
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|{
                                        let enabled=this.editor.project().settings.extra.get("role_icons").and_then(Value::as_bool).unwrap_or(true);
                                        this.update_setting("role_icons",Value::Bool(!enabled),cx);
                                    }))))
            .when(self.pending_preset.is_some(), |root| root.child(div().flex().items_center().gap_3().p_3().bg(rgb(theme::ACCENT_SURFACE))
                .child(format!("Preset {} · {} · {} trường thiết lập",
                    self.pending_preset.as_ref().map(|preset|preset.name.as_str()).unwrap_or(""),
                    self.pending_preset.as_ref().map(|preset|preset.source.as_str()).unwrap_or(""),
                    self.pending_preset.as_ref().map(|preset|preset.settings.len()).unwrap_or(0)))
                .child(self.pending_preset.as_ref().map(|preset|preset.description.clone()).unwrap_or_default())
                .child(div().cursor_pointer().rounded_sm().bg(rgb(theme::SUCCESS_ACTIVE)).px_3().py_1().child("Áp dụng preset · một bước hoàn tác")
                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.apply_preset(cx))))
                .child(div().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).px_3().py_1().child("Hủy")
                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.dismiss_preset(cx))))))
                                .when(singing_mode == "duet" && role_icons, |panel| panel
                                    .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2()
                                        .child(format!("Icon Nam: {}", if role_a_icon.contains("green") {"Xanh lá"} else {"Xanh dương"}))
                                        .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.cycle_setting("role_a_icon", &["man-new-blue.png","man-new-green.png"],cx))))
                                    .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2()
                                        .child(format!("Icon Nữ: {}", if role_b_icon.contains("darkred") {"Đỏ đậm"} else if role_b_icon.contains("red") {"Đỏ"} else {"Hồng"}))
                                        .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.cycle_setting("role_b_icon", &["lady-pink.png","lady-red.png","lady-darkred.png"],cx))))
                                    .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2()
                                        .child(format!("Icon Cùng hát: {}", match (role_all_icon.contains("green"), role_all_icon.contains("red")) {(true,true)=>"Nam xanh lá · Nữ đỏ",(true,false)=>"Nam xanh lá · Nữ hồng",(false,true)=>"Nam xanh dương · Nữ đỏ",_=>"Nam xanh dương · Nữ hồng"}))
                                        .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.cycle_setting("role_all_icon", &["men(blue) & lady(pink).png","men(blue) & lady(red).png","men(green) & lady(pink).png","men(green) & lady(red).png"],cx)))))
                                .child(div().mt_4().text_size(px(11.)).text_color(rgb(theme::TEXT_MUTED)).child("COUNTDOWN TRƯỚC LỜI"))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2()
                                    .child(format!("Biểu tượng: {countdown}"))
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|{
                                        let current=this.editor.project().settings.extra.get("countdown").and_then(Value::as_str).unwrap_or("none");
                                        let values=["none","mic","star","heart","note","moon","image"];
                                        let next=values.iter().position(|value|*value==current).map(|index|(index+1)%values.len()).unwrap_or(0);
                                        this.update_setting("countdown",Value::String(values[next].into()),cx);
                                    })))
                                .when(countdown == "image",|panel|panel.child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2()
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
                                        div().flex_1().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2()
                                            .child(div().flex().gap_1().items_center()
                                                .child(div().w(px(10.)).h(px(10.)).rounded_sm().bg(rgb(match color {"#f4cf72"=>0xf4cf72,"#ff87b7"=>0xff87b7,_=>0x52efb9})))
                                                .child(label))
                                            .on_mouse_down(MouseButton::Left,cx.listener(move |this,_,_,cx|this.update_setting("sung",Value::String(color.into()),cx))))))
                                .child(div().mt_4().text_size(px(11.)).text_color(rgb(theme::TEXT_MUTED)).child("TITLE CARD"))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2()
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
                                            div().flex_1().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2()
                                                .child(div().flex().gap_1().items_center()
                                                    .child(div().w(px(10.)).h(px(10.)).rounded_sm().bg(rgb(match color {"#f4cf72"=>0xf4cf72,"#52efb9"=>0x52efb9,"#ff87b7"=>0xff87b7,_=>0xffffff})))
                                                    .child(label))
                                                .on_mouse_down(MouseButton::Left,cx.listener(move |this,_,_,cx|this.update_setting("title_color",Value::String(color.into()),cx))))))
                                    .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2()
                                        .child(format!("Template: {title_template}"))
                                        .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.cycle_setting("title_template",&["none","gradient","midnight","crystal","neon","retro","pastel"],cx))))
                                    .child(div().mt_2().flex().gap_2()
                                        .children([("Vào","title_enter"),("Ra","title_exit")].into_iter().map(|(label,key)|{
                                            let current=this_setting_label(settings,key);
                                            div().flex_1().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2()
                                                .child(format!("{label}: {current}"))
                                                .on_mouse_down(MouseButton::Left,cx.listener(move |this,_,_,cx|this.cycle_setting(key,&["none","fade","slide_up","slide_down","slide_left","slide_right","zoom","flip"],cx)))
                                        })))
                                )
                                .child(div().mt_4().text_size(px(11.)).text_color(rgb(theme::TEXT_MUTED)).child("XUẤT VIDEO"))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2()
                                    .child(format!("Định dạng: {export_format}"))
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.update_typed_settings(|settings|settings.render_format=match settings.render_format {vietkara_domain::settings::RenderFormat::Mp4=>vietkara_domain::settings::RenderFormat::Mov,vietkara_domain::settings::RenderFormat::Mov=>vietkara_domain::settings::RenderFormat::Mkv,vietkara_domain::settings::RenderFormat::Mkv=>vietkara_domain::settings::RenderFormat::Mp4},cx))))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2()
                                    .child(format!("Độ phân giải: {export_resolution}"))
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.update_typed_settings(|settings|settings.resolution=match settings.resolution {vietkara_domain::settings::Resolution::Hd=>vietkara_domain::settings::Resolution::UltraHd,vietkara_domain::settings::Resolution::UltraHd=>vietkara_domain::settings::Resolution::Hd},cx))))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2()
                                    .child(format!("Chất lượng: {export_quality}"))
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.update_typed_settings(|settings|settings.render_quality=match settings.render_quality {vietkara_domain::settings::RenderQuality::Standard=>vietkara_domain::settings::RenderQuality::High,vietkara_domain::settings::RenderQuality::High=>vietkara_domain::settings::RenderQuality::Maximum,vietkara_domain::settings::RenderQuality::Maximum=>vietkara_domain::settings::RenderQuality::Standard},cx))))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2()
                                    .child(format!("Encoder: {export_encoder}"))
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|{
                                        let current=this.editor.project().settings.extra.get("video_encoder").and_then(Value::as_str).unwrap_or("auto");
                                        let next=match current {"auto"=>"cpu","cpu"=>"nvenc",_=>"auto"};
                                        this.update_setting("video_encoder",Value::String(next.into()),cx);
                                    }))))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2()
                                    .child(format!("Tốc độ khung hình: {export_fps}"))
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.update_typed_settings(|settings|settings.render_fps=match settings.render_fps {vietkara_domain::settings::RenderFps::Source=>vietkara_domain::settings::RenderFps::Thirty,vietkara_domain::settings::RenderFps::Thirty=>vietkara_domain::settings::RenderFps::Sixty,vietkara_domain::settings::RenderFps::Sixty=>vietkara_domain::settings::RenderFps::Source},cx))))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2()
                                    .child(format!("Âm thanh: {export_audio}"))
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.update_typed_settings(|settings|settings.audio_output=match settings.audio_output {vietkara_domain::settings::AudioOutput::Beat=>vietkara_domain::settings::AudioOutput::Guide,vietkara_domain::settings::AudioOutput::Guide=>vietkara_domain::settings::AudioOutput::Beat},cx))))
                                .child(div().mt_2().cursor_pointer().rounded_sm().bg(rgb(theme::CANVAS)).hover(|style| style.bg(rgb(theme::SURFACE_HOVER))).px_2().py_2()
                                    .child(if settings.audio_normalize {"✓  Chuẩn hóa âm thanh (-16 LUFS)"} else {"Chuẩn hóa âm thanh: tắt"})
                                    .on_mouse_down(MouseButton::Left,cx.listener(|this,_,_,cx|this.update_typed_settings(|settings|settings.audio_normalize = !settings.audio_normalize,cx))))
                                .child(div().mt_3().text_size(px(11.)).text_color(rgb(theme::TEXT_SUBTLE)).child("Cài đặt được lưu cùng project.")),
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
                    .child(div().cursor_pointer().px_2().rounded_sm()
                        .bg(rgb(if self.listen_track == AudioSlot::Beat { theme::ACCENT_SURFACE } else { theme::CANVAS }))
                        .child("Nghe beat")
                        .on_mouse_down(MouseButton::Left,cx.listener(|this, _, _, cx|this.select_listen_track(AudioSlot::Beat,cx))))
                    .child(div().cursor_pointer().px_2().rounded_sm()
                        .bg(rgb(if self.listen_track == AudioSlot::Reference { theme::ACCENT_SURFACE } else { theme::CANVAS }))
                        .child("Nghe bản hát")
                        .on_mouse_down(MouseButton::Left,cx.listener(|this, _, _, cx|this.select_listen_track(AudioSlot::Reference,cx))))
                    .when(cfg!(windows), |root| root.child(div().cursor_pointer().px_2().rounded_sm()
                        .child(output_device_label)
                        .on_mouse_down(MouseButton::Left,cx.listener(|this, _, _, cx|this.cycle_output_device(cx)))))
                    .child(format!("{:.2} / {:.2} s",self.play_position,self.play_duration))
                    .child(self.playback_error.clone().unwrap_or_default())
                    .child(status),
            )
    }
}

fn audio_slot_filename(slot: AudioSlot) -> &'static str {
    match slot {
        AudioSlot::Beat => "beat.wav",
        AudioSlot::Reference => "reference.wav",
    }
}

fn additional_output_devices(devices: &[String], default_device: Option<&str>) -> Vec<String> {
    devices
        .iter()
        .filter(|device| Some(device.as_str()) != default_device)
        .cloned()
        .collect()
}

fn merge_playback_error(
    worker_error: Option<String>,
    current_error: Option<&str>,
) -> Option<String> {
    worker_error.or_else(|| {
        current_error
            .filter(|error| error.starts_with("Chưa có "))
            .map(str::to_owned)
    })
}

fn audio_metadata(project: &Project, slot: AudioSlot) -> Option<&Value> {
    match slot {
        AudioSlot::Beat => project.beat.as_ref(),
        AudioSlot::Reference => project.reference.as_ref(),
    }
}

fn project_audio_file(
    project: &Project,
    project_path: Option<&std::path::Path>,
    slot: AudioSlot,
) -> Option<PathBuf> {
    let metadata = audio_metadata(project, slot)?;
    let filename = metadata
        .get("filename")
        .and_then(Value::as_str)
        .unwrap_or_else(|| audio_slot_filename(slot));
    let relative = std::path::Path::new(filename);
    if relative.file_name().and_then(|name| name.to_str()) != Some(filename)
        || relative.components().count() != 1
    {
        return None;
    }
    Some(project_path?.parent()?.join(relative))
}

fn ai_worker_script() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("VIETKARA_AI_WORKER").map(PathBuf::from) {
        return path
            .is_file()
            .then_some(path.clone())
            .ok_or_else(|| format!("Không tìm thấy AI worker: {}", path.display()));
    }
    let mut roots = Vec::new();
    if let Ok(executable) = std::env::current_exe()
        && let Some(parent) = executable.parent()
    {
        roots.extend(parent.ancestors().map(PathBuf::from));
    }
    if let Ok(directory) = std::env::current_dir() {
        roots.extend(directory.ancestors().map(PathBuf::from));
    }
    roots.dedup();
    roots
        .into_iter()
        .map(|root| root.join("ai_worker.py"))
        .find(|path| path.is_file())
        .ok_or_else(|| {
            "Không tìm thấy ai_worker.py; đặt VIETKARA_AI_WORKER trỏ tới worker đã cài đặt".into()
        })
}

fn ai_data_root(worker_script: &std::path::Path) -> PathBuf {
    if let Some(path) = std::env::var_os("OBS_DATA_DIR") {
        return PathBuf::from(path);
    }
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(local_app_data).join("VietKaraStudio");
    }
    worker_script
        .parent()
        .map(|directory| directory.join("data"))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default().join("data"))
}

fn publish_cache_file(
    source: &std::path::Path,
    destination: &std::path::Path,
) -> std::io::Result<()> {
    if destination.is_file() {
        return Ok(());
    }
    match std::fs::hard_link(source, destination) {
        Ok(()) => return Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => return Ok(()),
        Err(_) => {}
    }
    let mut input = std::fs::File::open(source)?;
    let parent = destination
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    std::fs::create_dir_all(parent)?;
    let mut staged_path = None;
    let mut output = None;
    for attempt in 0..8u8 {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let candidate = parent.join(format!(
            ".vietkara-cache-{}-{nonce}-{attempt}.tmp",
            std::process::id()
        ));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => {
                staged_path = Some(candidate);
                output = Some(file);
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    let staged_path = staged_path.ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "could not reserve a unique cache staging file",
        )
    })?;
    let mut output = output.expect("staging path is set with its open file");
    if let Err(error) = std::io::copy(&mut input, &mut output) {
        let _ = std::fs::remove_file(&staged_path);
        return Err(error);
    }
    if let Err(error) = output.sync_all() {
        let _ = std::fs::remove_file(&staged_path);
        return Err(error);
    }
    drop(output);
    let publish_result = std::fs::hard_link(&staged_path, destination);
    let _ = std::fs::remove_file(&staged_path);
    match publish_result {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(error),
    }
}

struct VocalCacheRequest<'a> {
    config: &'a WorkerConfig,
    project_id: &'a str,
    reference_path: &'a std::path::Path,
    data_root: &'a std::path::Path,
    device: &'a str,
}

fn ensure_cached_vocal(
    cache_request: VocalCacheRequest<'_>,
    cancel_requested: &std::sync::atomic::AtomicBool,
    mut started: impl FnMut(std::sync::Arc<std::sync::atomic::AtomicBool>),
    mut progress: impl FnMut(f32, &str),
) -> Result<(PathBuf, String), String> {
    let fingerprint = vietkara_storage::sha256_file(cache_request.reference_path)
        .map_err(|error| format!("Không đọc được fingerprint bản hát: {error}"))?;
    let cache_directory = cache_request.data_root.join("role-vocals");
    std::fs::create_dir_all(&cache_directory)
        .map_err(|error| format!("Không tạo được cache vocal: {error}"))?;
    let vocal_path = cache_directory.join(format!("{fingerprint}.wav"));
    if !vocal_path.is_file() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let temporary_vocal =
            std::env::temp_dir().join(format!("vietkara-role-{}-{unique}.wav", std::process::id()));
        let separate_request = AiRequest::new(
            AiKind::Separate,
            cache_request.project_id,
            serde_json::json!({
                "reference": cache_request.reference_path,
                "output": temporary_vocal,
            }),
            serde_json::json!({"device": cache_request.device}),
        );
        let mut worker = Worker::spawn(cache_request.config.clone(), separate_request)
            .map_err(|error| error.to_string())?;
        let cancel = worker.cancel_flag();
        started(cancel.clone());
        if cancel_requested.load(std::sync::atomic::Ordering::SeqCst) {
            cancel.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        let separated = worker
            .run(|percent, message| progress(percent.clamp(0.0, 100.0) * 0.4, message))
            .map_err(|error| error.to_string());
        let output_path = match separated {
            Ok(value) => value
                .get("vocals")
                .and_then(Value::as_str)
                .map(PathBuf::from)
                .filter(|path| path.is_file())
                .ok_or_else(|| "Tách vocal không tạo được file đầu ra".to_owned()),
            Err(error) => Err(error),
        };
        let output_path = match output_path {
            Ok(path) => path,
            Err(error) => {
                let _ = std::fs::remove_file(&temporary_vocal);
                return Err(error);
            }
        };
        if output_path != temporary_vocal {
            let _ = std::fs::remove_file(&temporary_vocal);
            return Err("Worker trả đường dẫn vocal khác với file tạm được yêu cầu".into());
        }
        if cancel_requested.load(std::sync::atomic::Ordering::SeqCst) {
            let _ = std::fs::remove_file(&temporary_vocal);
            return Err("Đã hủy tách vocal".into());
        }
        let fingerprint_after_separation =
            match vietkara_storage::sha256_file(cache_request.reference_path) {
                Ok(fingerprint) => fingerprint,
                Err(error) => {
                    let _ = std::fs::remove_file(&temporary_vocal);
                    return Err(format!(
                        "Không kiểm tra lại được bản hát sau khi tách vocal: {error}"
                    ));
                }
            };
        if fingerprint_after_separation != fingerprint {
            let _ = std::fs::remove_file(&temporary_vocal);
            return Err("File bản hát đã đổi trong lúc tách vocal; hãy chạy lại".into());
        }
        if let Err(error) = publish_cache_file(&output_path, &vocal_path) {
            let _ = std::fs::remove_file(&temporary_vocal);
            return Err(format!("Không lưu được vocal đã tách vào cache: {error}"));
        }
        let _ = std::fs::remove_file(&temporary_vocal);
    }
    Ok((vocal_path, fingerprint))
}

fn validate_sync_suggestion(project: &Project, result: &Value) -> Result<Vec<Anchor>, String> {
    if result.get("requires_confirmation").and_then(Value::as_bool) != Some(true) {
        return Err("Worker không đánh dấu gợi ý cần xác nhận".into());
    }
    let anchors: Vec<Anchor> = serde_json::from_value(
        result
            .get("anchors")
            .cloned()
            .ok_or_else(|| "Worker không trả danh sách mốc".to_owned())?,
    )
    .map_err(|error| format!("Danh sách mốc từ worker không hợp lệ: {error}"))?;
    let mut candidate = project.clone();
    candidate.anchors = anchors.clone();
    candidate
        .validate()
        .map_err(|error| format!("Danh sách mốc từ worker không hợp lệ: {error}"))?;
    Ok(anchors)
}

fn role_suggestion_command(
    words: &[Word],
    lines: &[u32],
    result: &Value,
) -> Result<(Option<Command>, usize), String> {
    if result.get("two_registers").and_then(Value::as_bool) != Some(true) {
        let message = match result.get("reason").and_then(Value::as_str) {
            Some("insufficient_voiced_audio") => "Vocal chưa có đủ đoạn giọng rõ để gợi ý phân vai",
            Some("overlapping_registers") => {
                "Cao độ hai vai chưa tách đủ rõ; giữ nguyên phân vai hiện tại"
            }
            _ => "Worker không tìm thấy hai nhóm giọng đủ rõ để gợi ý",
        };
        return Err(message.into());
    }
    let suggested = result
        .get("roles")
        .and_then(Value::as_array)
        .ok_or_else(|| "Kết quả phân vai thiếu danh sách vai".to_owned())?;
    if suggested.len() != lines.len() {
        return Err("Kết quả phân vai không khớp số dòng lời hiện tại".into());
    }
    let mut by_line = std::collections::HashMap::with_capacity(lines.len());
    for (&line, role) in lines.iter().zip(suggested) {
        let role = if role.is_null() {
            None
        } else {
            let role = role
                .as_str()
                .ok_or_else(|| "Kết quả có vai hát sai định dạng".to_owned())?;
            if !matches!(role, "male" | "female" | "all") {
                return Err(format!("Worker trả về vai không hỗ trợ: {role}"));
            }
            Some(role.to_owned())
        };
        by_line.insert(line, role);
    }

    let mut suggestible_lines = std::collections::HashMap::new();
    for word in words {
        suggestible_lines
            .entry(word.line)
            .or_insert(matches!(word.role.as_str(), "all" | "unknown"));
    }
    let mut commands = Vec::new();
    let mut changed_words = 0;
    for word in words {
        if word.locked == Some(true) {
            continue;
        }
        if !suggestible_lines.get(&word.line).copied().unwrap_or(false) {
            continue;
        }
        let Some(Some(role)) = by_line.get(&word.line) else {
            continue;
        };
        if word.role == *role {
            continue;
        }
        let mut candidate = word.clone();
        candidate.role = role.clone();
        commands.push(Command::ReplaceWord {
            id: word.id.clone(),
            word: Box::new(candidate),
        });
        changed_words += 1;
    }
    Ok((
        (!commands.is_empty()).then_some(Command::Batch(commands)),
        changed_words,
    ))
}

fn role_application_command(
    project: &Project,
    lines: &[u32],
    result: &Value,
) -> Result<(Option<Command>, usize), String> {
    let (Some(Command::Batch(mut commands)), changed_words) =
        role_suggestion_command(&project.words, lines, result)?
    else {
        return Ok((None, 0));
    };
    let mut settings = project.settings.clone();
    settings
        .extra
        .insert("singing_mode".into(), Value::String("duet".into()));
    commands.insert(0, Command::SetSettings(settings));
    Ok((Some(Command::Batch(commands)), changed_words))
}

fn words_match_snapshot(current: &[Word], snapshot: &[Word]) -> bool {
    match (
        serde_json::to_value(current),
        serde_json::to_value(snapshot),
    ) {
        (Ok(current), Ok(snapshot)) => current == snapshot,
        _ => false,
    }
}

fn build_ai_lyrics_candidate(
    result: &Value,
    base_project: &Project,
    target_lines: Option<&[u32]>,
) -> Result<(Vec<Word>, String), String> {
    let base_words = &base_project.words;
    let raw_words = result
        .get("words")
        .and_then(Value::as_array)
        .ok_or_else(|| "Kết quả AI thiếu danh sách từ".to_owned())?;
    if raw_words.is_empty() {
        return Err("AI không nhận được từ nào để đưa vào project".into());
    }
    if raw_words.len() > vietkara_domain::MAX_WORDS {
        return Err("Kết quả AI vượt quá giới hạn số từ của project".into());
    }
    let base_lines =
        base_words
            .iter()
            .map(|word| word.line)
            .fold(Vec::<u32>::new(), |mut lines, line| {
                if !lines.contains(&line) {
                    lines.push(line);
                }
                lines
            });
    let result_lines = raw_words
        .iter()
        .map(|word| {
            word.get("line")
                .and_then(Value::as_u64)
                .ok_or_else(|| "Kết quả AI thiếu số dòng hợp lệ".to_owned())
        })
        .collect::<Result<std::collections::HashSet<_>, _>>()?;
    let line_targets = target_lines.map(|lines| lines.to_vec()).unwrap_or_else(|| {
        if base_lines.is_empty() {
            Vec::new()
        } else {
            base_lines.clone()
        }
    });
    if let Some(target_lines) = target_lines {
        let unique = target_lines
            .iter()
            .copied()
            .collect::<std::collections::HashSet<_>>();
        if unique.len() != target_lines.len()
            || target_lines.iter().any(|line| !base_lines.contains(line))
        {
            return Err("Danh sách câu cần căn lại không hợp lệ".into());
        }
    }
    if !result_lines.is_empty() {
        for index in 0..result_lines.len() as u64 {
            if !result_lines.contains(&index) {
                return Err("Kết quả AI có số dòng không liên tục".into());
            }
        }
        if !line_targets.is_empty() && result_lines.len() != line_targets.len() {
            return Err("Kết quả AI không căn đủ số dòng lời hiện tại".into());
        }
    }
    let base_line_first = base_lines
        .iter()
        .map(|line| (*line, base_words.iter().find(|word| word.line == *line)))
        .collect::<std::collections::HashMap<_, _>>();
    let mut generated_line_ids = std::collections::HashMap::<u32, String>::new();
    let mut words = Vec::with_capacity(raw_words.len());
    for raw in raw_words {
        let mut word: Word = serde_json::from_value(raw.clone())
            .map_err(|error| format!("Từ AI sai định dạng: {error}"))?;
        if word.text.trim().is_empty()
            || !word.start.is_finite()
            || !word.end.is_finite()
            || word.start < 0.0
            || word.end <= word.start
        {
            return Err("Kết quả AI có từ hoặc mốc thời gian không hợp lệ".into());
        }
        let line_index = word.line;
        if let Some(old_line) = line_targets.get(line_index as usize)
            && let Some(Some(source)) = base_line_first.get(old_line)
        {
            word.line = *old_line;
            word.role = source.role.clone();
            word.row = source.row;
            word.line_uid = source.line_uid.clone();
            word.section = source.section;
            word.section_name = source.section_name.clone();
            word.label_color = source.label_color.clone();
        }
        if word.line_uid.is_empty() {
            let line_uid = generated_line_ids
                .entry(word.line)
                .or_insert_with(vietkara_domain::new_id);
            word.line_uid = line_uid.clone();
        }
        word.id = vietkara_domain::new_id();
        word.locked = None;
        word.manual = None;
        words.push(word);
    }
    let (words, text) = if let Some(target_lines) = target_lines {
        let mut combined = Vec::with_capacity(base_words.len() + words.len());
        for line in &base_lines {
            if target_lines.contains(line) {
                combined.extend(words.iter().filter(|word| word.line == *line).cloned());
            } else {
                combined.extend(base_words.iter().filter(|word| word.line == *line).cloned());
            }
        }
        let text = vietkara_domain::lyrics::text_from_words(&combined);
        (combined, text)
    } else {
        let text = result
            .get("text")
            .and_then(Value::as_str)
            .filter(|text| !text.trim().is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| vietkara_domain::lyrics::text_from_words(&words));
        (words, text)
    };
    if text.len() > 300_000 {
        return Err("Lời nhận dạng vượt quá giới hạn 300.000 byte".into());
    }
    let mut candidate = base_project.clone();
    candidate.words = words.clone();
    candidate.text = text.clone();
    candidate
        .validate()
        .map_err(|error| format!("Kết quả AI không hợp lệ cho project: {error}"))?;
    Ok((words, text))
}

fn audio_slot_missing(
    project: &Project,
    project_path: Option<&std::path::Path>,
    slot: AudioSlot,
) -> bool {
    audio_metadata(project, slot).is_some()
        && project_audio_file(project, project_path, slot).is_none_or(|path| !path.is_file())
}

fn audio_slot_label(
    project: &Project,
    project_path: Option<&std::path::Path>,
    slot: AudioSlot,
) -> String {
    let has_metadata = audio_metadata(project, slot).is_some();
    match (
        slot,
        has_metadata,
        audio_slot_missing(project, project_path, slot),
    ) {
        (AudioSlot::Beat, false, _) => "＋  Nạp beat gốc".into(),
        (AudioSlot::Beat, true, true) => "⚠  Beat gốc · thiếu file — Relink".into(),
        (AudioSlot::Beat, true, false) => "●  Beat gốc · sẵn sàng".into(),
        (AudioSlot::Reference, false, _) => "＋  Nạp bản hát / video".into(),
        (AudioSlot::Reference, true, true) => "⚠  Bản hát · thiếu file — Relink".into(),
        (AudioSlot::Reference, true, false) => "●  Bản hát · sẵn sàng".into(),
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

struct PreviewTextStyle<'a> {
    font: &'a str,
    italic: bool,
    fill_color: u32,
    outline_color: u32,
    outline_width: f32,
    outline_opacity: f32,
    shadow_color: u32,
    shadow: f32,
    opacity: f32,
}

fn preview_text_layers(
    text: &str,
    x: f32,
    y: f32,
    font_size: f32,
    style: PreviewTextStyle<'_>,
) -> Vec<gpui::Div> {
    let text = vietkara_render::layout::font_text(text, style.font);
    let layer = |offset_x: f32, offset_y: f32, color: u32, alpha: f32| {
        div()
            .absolute()
            .left(px(x + offset_x))
            .top(px(y + offset_y))
            .text_size(px(font_size))
            .text_color(rgb(color))
            .font_family(style.font.to_owned())
            .when(style.italic, |text| text.italic())
            .opacity(alpha.clamp(0.0, 1.0))
            .child(text.clone())
    };
    let mut layers = Vec::with_capacity(10);
    if style.shadow > 0.0 {
        layers.push(layer(
            style.shadow,
            style.shadow,
            style.shadow_color,
            style.opacity,
        ));
    }
    if style.outline_width > 0.0 {
        let diagonal = style.outline_width * std::f32::consts::FRAC_1_SQRT_2;
        for (dx, dy) in [
            (-style.outline_width, 0.0),
            (style.outline_width, 0.0),
            (0.0, -style.outline_width),
            (0.0, style.outline_width),
            (-diagonal, -diagonal),
            (diagonal, -diagonal),
            (-diagonal, diagonal),
            (diagonal, diagonal),
        ] {
            layers.push(layer(dx, dy, style.outline_color, style.outline_opacity));
        }
    }
    layers.push(layer(0.0, 0.0, style.fill_color, style.opacity));
    layers
}

#[derive(Debug, PartialEq)]
struct SweepPreviewGeometry {
    solid_width: f32,
    feather_segments: Vec<(f32, f32, f32)>,
    glow_band: Option<(f32, f32, f32)>,
}

const SWEEP_PREVIEW_GLOW_OFFSETS: [(f32, f32, f32); 9] = [
    (0.0, 0.0, 1.0),
    (-1.0, 0.0, 0.22),
    (1.0, 0.0, 0.22),
    (0.0, -1.0, 0.18),
    (0.0, 1.0, 0.18),
    (-0.7, -0.7, 0.12),
    (0.7, -0.7, 0.12),
    (-0.7, 0.7, 0.12),
    (0.7, 0.7, 0.12),
];

fn sweep_preview_geometry(
    width: f32,
    progress: f32,
    requested_feather: f32,
    glow: f32,
    font_size: f32,
    min_glow_band: f32,
) -> SweepPreviewGeometry {
    let progress = progress.clamp(0.0, 1.0);
    let feather = requested_feather.min(width * 0.25).max(0.0);
    let edge = width * progress;
    let solid_width = (edge - feather * 0.5).clamp(0.0, width);
    let feather_segments = if feather > 0.0 {
        let segment_width = feather / 4.0;
        (0..4)
            .filter_map(|segment| {
                let raw_start = edge - feather * 0.5 + segment_width * segment as f32;
                let start = raw_start.clamp(0.0, width);
                let end = (raw_start + segment_width).clamp(0.0, width);
                (end > start).then_some((start, end - start, 1.0 - (segment as f32 + 0.5) / 4.0))
            })
            .collect()
    } else {
        Vec::new()
    };
    let glow_band = if glow > 0.0 && progress > 0.0 && progress < 1.0 {
        let band = (width * 0.2)
            .min(feather + font_size * 0.07)
            .max(min_glow_band);
        let left = (edge - band).clamp(0.0, width);
        let right = (edge + band).clamp(0.0, width);
        (right > left).then_some((left, right - left, (glow / 150.0).clamp(0.0, 1.0)))
    } else {
        None
    };
    SweepPreviewGeometry {
        solid_width,
        feather_segments,
        glow_band,
    }
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
        if !vietkara_render::is_project_basename(name) {
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
        "male" | "voice_c" | "voice_e" => "Nam",
        "female" | "voice_d" | "voice_f" => "Nữ",
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
    let (recovery, mut initial_status) = match path
        .as_ref()
        .map(|path| recovery_candidate(path))
        .transpose()
    {
        Ok(candidate) => (candidate.flatten(), "Sẵn sàng".to_owned()),
        Err(error) => (None, format!("Không đọc được recovery: {error}")),
    };
    let batch_record = load_latest_batch_record(&batch_data_root());
    let ai_preferences = ui_preferences::load_ai_preferences();
    if let Some(record) = &batch_record
        && record.status != BatchStatus::Done
    {
        initial_status = format!(
            "Có batch cần tiếp tục: {} mục, hoàn tất {}, lỗi {}",
            record.items.len(),
            record.completed,
            record.failed
        );
    }
    Application::new().run(move |cx: &mut App| {
        if let Err(error) = fonts::add_embedded_fonts(cx) {
            eprintln!("Không nạp được font nhúng: {error}");
            initial_status = format!("Không nạp được thư viện font: {error}");
        }
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
            KeyBinding::new("ctrl-n", NewProject, Some("Studio")),
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
                let font_query_input = cx.new(|cx| text_input::TextInput::new(cx, ""));
                font_query_input.update(cx, |input, _| input.set_placeholder("Tìm font theo tên"));
                cx.new(|cx: &mut Context<Studio>| {
                    let weak = cx.entity().downgrade();
                    window.on_window_should_close(cx, move |_, cx| {
                        weak.update(cx, |studio: &mut Studio, cx| {
                            if studio.rendering {
                                studio.cancel_export(cx);
                                studio.edit_status = if studio.active_batch {
                                    "Đang dừng batch và lưu trạng thái trước khi đóng".into()
                                } else {
                                    "Đang dừng xuất video trước khi đóng".into()
                                };
                                cx.notify();
                                return false;
                            }
                            if studio.sync_running {
                                if let Some(cancel) = &studio.sync_cancel {
                                    cancel.store(true, std::sync::atomic::Ordering::SeqCst);
                                }
                                if let Some(requested) = &studio.role_suggest_cancel_request {
                                    requested.store(true, std::sync::atomic::Ordering::SeqCst);
                                }
                                if let Some(requested) = &studio.lyrics_ai_cancel_request {
                                    requested.store(true, std::sync::atomic::Ordering::SeqCst);
                                }
                                studio.edit_status = if studio.role_suggest_running {
                                    "Đang dừng gợi ý phân vai trước khi đóng cửa sổ".into()
                                } else if studio.lyrics_ai_running {
                                    "Đang dừng nhận dạng/căn lời trước khi đóng cửa sổ".into()
                                } else {
                                    "Đang dừng phân tích đồng bộ trước khi đóng cửa sổ".into()
                                };
                                cx.notify();
                                return false;
                            }
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
                        font_query_input,
                        editor: Editor::new(project).expect("Project mới phải hợp lệ"),
                        edit_status: initial_status,
                        path,
                        recovery,
                        saving: false,
                        rendering: false,
                        render_progress: 0.0,
                        render_cancel: None,
                        active_batch: false,
                        pending_batch_projects: Vec::new(),
                        batch_record: batch_record.clone(),
                        pending_preset: None,
                        bundled_presets: presets::bundled(),
                        custom_presets: presets::load_custom(&batch_data_root())
                            .unwrap_or_default(),
                        show_preset_catalog: false,
                        show_tools_menu: false,
                        pending_preset_delete: None,
                        font_catalog: fonts::families(),
                        show_font_catalog: false,
                        diagnostics_running: false,
                        diagnostics_report: None,
                        ai_device_preference: ai_preferences.device.clone(),
                        ai_model: ai_preferences.model.clone(),
                        dirty: false,
                        confirm_open: false,
                        confirm_new: false,
                        confirm_close: false,
                        sync_running: false,
                        sync_progress: 0.0,
                        sync_cancel: None,
                        pending_sync: None,
                        role_suggest_running: false,
                        role_suggest_cancel_request: None,
                        pending_role_suggestion: None,
                        lyrics_ai_running: false,
                        lyrics_ai_cancel_request: None,
                        pending_lyrics_ai: None,
                        close_after_save_window: None,
                        navigate_after_save: None,
                        pending_lyrics: None,
                        pending_audio: None,
                        autosave_status: String::new(),
                        autosaving: false,
                        autosave_pending: false,
                        _autosave_task: task,
                        left_tab_media: false,
                        inspector_design: false,
                        workspace_layout: ui_preferences::load_workspace_layout(),
                        workspace_resize: None,
                        playing: false,
                        selected_word: None,
                        selected_anchor: None,
                        timeline_zoom: 80.,
                        timeline_drag: None,
                        timeline_scroll: ScrollHandle::new(),
                        timeline_word_index: std::cell::RefCell::default(),
                        preview_layout: preview::PreviewLayoutCache::default(),
                        tapping: None,
                        play_position: 0.,
                        play_duration: 0.,
                        background_video_frame: None,
                        background_frame_requested_at: -1.,
                        background_frame_pending: false,
                        playback_error: None,
                        listen_track: AudioSlot::Beat,
                        output_device: None,
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
    fn ffmpeg_failure_messages_suggest_a_specific_recovery() {
        let cases = [
            ("No such file or directory", "relink file bị thiếu"),
            ("Permission denied", "quyền ghi"),
            ("No space left on device", "hết dung lượng"),
            ("Unknown encoder 'h264_nvenc'", "encoder"),
            ("Cannot allocate memory", "Thiếu bộ nhớ"),
        ];
        for (detail, expected) in cases {
            let message = format_ffmpeg_failure(Some(1), detail);
            assert!(message.contains(expected), "{detail} => {message}");
            assert!(message.contains(detail));
        }
        assert!(format_ffmpeg_failure(Some(1), "").contains("mã Some(1)"));
    }

    #[test]
    fn batch_target_avoids_overwriting_existing_exports() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("song.mp4"), b"existing").unwrap();
        assert_eq!(
            unique_batch_target(dir.path(), "song.mp4"),
            dir.path().join("song (2).mp4")
        );
    }

    #[test]
    fn batch_target_avoids_paths_already_reserved_by_the_same_queue() {
        let dir = tempfile::tempdir().unwrap();
        let reserved = BatchItem {
            project_id: "project".into(),
            render_id: "render".into(),
            name: "Song".into(),
            variant: serde_json::json!({ "output_path": dir.path().join("song.mp4") }),
            directory: dir.path().to_string_lossy().into_owned(),
            assets: Vec::new(),
            status: ItemStatus::Queued,
            result: None,
            error: None,
        };
        assert_eq!(
            unique_batch_target_for_record(dir.path(), "song.mp4", &[reserved]),
            dir.path().join("song (2).mp4")
        );
    }

    #[test]
    fn batch_snapshot_copies_managed_media_and_never_clobbers_or_tracks_source_changes() {
        let root = tempfile::tempdir().unwrap();
        let source_dir = root.path().join("Bài hát fixture");
        let data_root = root.path().join("app-data");
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::write(source_dir.join("beat_source.wav"), b"fixture beat bytes").unwrap();
        std::fs::write(
            source_dir.join("reference_source.wav"),
            b"fixture reference bytes",
        )
        .unwrap();
        std::fs::write(source_dir.join("notes.txt"), b"not managed media").unwrap();
        let source_project_path = source_dir.join("project.json");
        let mut project = Project::new("Snapshot fixture");
        project.beat = Some(serde_json::json!({"filename":"beat_source.wav","duration":2.0}));
        project.reference =
            Some(serde_json::json!({"filename":"reference_source.wav","duration":2.0}));
        vietkara_storage::save_project(&project, &source_project_path).unwrap();

        let batch_id = vietkara_domain::new_id();
        let render_id = vietkara_domain::new_id();
        let snapshot = snapshot_batch_project(
            &data_root,
            &batch_id,
            &render_id,
            &project,
            &source_project_path,
        )
        .unwrap();
        let snapshot_dir = snapshot.project_path.parent().unwrap();
        assert!(snapshot.project_path.is_file());
        assert_eq!(snapshot.media_paths.len(), 2);
        assert!(snapshot_dir.join("beat_source.wav").is_file());
        assert!(snapshot_dir.join("reference_source.wav").is_file());
        assert!(!snapshot_dir.join("notes.txt").exists());
        assert_eq!(snapshot.media_hashes.len(), 2);

        let item = BatchItem {
            project_id: project.id.clone(),
            render_id: render_id.clone(),
            name: project.name.clone(),
            variant: serde_json::json!({
                "source_project_path":source_project_path,
                "snapshot_project_path":snapshot.project_path,
                "snapshot_project_sha256":snapshot.project_hash,
                "snapshot_media":snapshot.media_hashes,
                "output_path":root.path().join("karaoke.mp4"),
            }),
            directory: source_dir.to_string_lossy().into_owned(),
            assets: snapshot
                .media_paths
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect(),
            status: ItemStatus::Queued,
            result: None,
            error: None,
        };
        assert!(validate_batch_snapshot(&item).is_ok());

        std::fs::write(
            source_dir.join("beat_source.wav"),
            b"source changed after queueing",
        )
        .unwrap();
        assert!(validate_batch_snapshot(&item).is_ok());
        let original_snapshot_beat = std::fs::read(snapshot_dir.join("beat_source.wav")).unwrap();
        assert_eq!(original_snapshot_beat, b"fixture beat bytes");

        let retry = snapshot_batch_project(
            &data_root,
            &batch_id,
            &render_id,
            &project,
            &source_project_path,
        );
        assert!(retry.is_err(), "an existing render id must not be reused");
        assert_eq!(
            std::fs::read(snapshot_dir.join("beat_source.wav")).unwrap(),
            original_snapshot_beat
        );

        std::fs::write(snapshot_dir.join("beat_source.wav"), b"tampered snapshot").unwrap();
        assert!(
            validate_batch_snapshot(&item)
                .unwrap_err()
                .contains("Media snapshot bị thay đổi: beat_source.wav")
        );
    }

    #[test]
    fn batch_worker_cancel_preserves_queue_and_retry_completes_pending_items() {
        let root = tempfile::tempdir().unwrap();
        let mut record = BatchRecord::new(
            "VietKaraStudio",
            "batch",
            &["current".into()],
            &["beat".into()],
        )
        .unwrap();
        for index in 0..2 {
            record.items.push(BatchItem {
                project_id: format!("project-{index}"),
                render_id: format!("render-{index}"),
                name: format!("Song {index}"),
                variant: serde_json::json!({"output_path":root.path().join(format!("{index}.mp4"))}),
                directory: root.path().to_string_lossy().into_owned(),
                assets: Vec::new(),
                status: ItemStatus::Queued,
                result: None,
                error: None,
            });
        }
        record.save(root.path()).unwrap();

        let make_job = |index| PreparedBatchRender {
            index,
            project: Project::new(format!("Song {index}")),
            project_path: root.path().join(format!("project-{index}.json")),
            target: root.path().join(format!("{index}.mp4")),
            plan: vietkara_render::layout::DisplayPlan {
                width: 1,
                height: 1,
                font: String::new(),
                rows: Vec::new(),
                title: None,
                countdowns: Vec::new(),
            },
        };
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let mut cancelled_record = None;
        render_batch_items_with(
            record,
            vec![make_job(0), make_job(1)],
            root.path(),
            &cancel,
            |event| {
                if let BatchRenderEvent::Finished(record, true) = event {
                    cancelled_record = Some(record);
                }
            },
            |job, cancel, _progress, _fallback| {
                assert_eq!(job.index, 0);
                cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                Err("cancelled during render".into())
            },
        );
        let cancelled_record = cancelled_record.expect("worker reports cancellation");
        assert_eq!(cancelled_record.status, BatchStatus::Cancelled);
        assert_eq!(cancelled_record.completed, 0);
        assert_eq!(cancelled_record.items[0].status, ItemStatus::Queued);
        assert_eq!(cancelled_record.items[1].status, ItemStatus::Queued);

        let restarted = BatchRecord::load(root.path(), &cancelled_record.id).unwrap();
        cancel.store(false, std::sync::atomic::Ordering::Relaxed);
        let mut finished_record = None;
        render_batch_items_with(
            restarted,
            vec![make_job(0), make_job(1)],
            root.path(),
            &cancel,
            |event| {
                if let BatchRenderEvent::Finished(record, false) = event {
                    finished_record = Some(record);
                }
            },
            |job, _cancel, progress, _fallback| {
                progress(0.5);
                Ok((job.target, false))
            },
        );
        let finished_record = finished_record.expect("worker reports completion");
        assert_eq!(finished_record.status, BatchStatus::Done);
        assert_eq!(finished_record.completed, 2);
        assert!(
            finished_record
                .items
                .iter()
                .all(|item| item.status == ItemStatus::Done)
        );
        let saved = BatchRecord::load(root.path(), &finished_record.id).unwrap();
        assert_eq!(saved.completed, 2);
        assert_eq!(saved.items[0].status, ItemStatus::Done);
        assert_eq!(saved.items[1].status, ItemStatus::Done);
    }

    #[test]
    fn startup_recovers_running_batch_item_as_queued() {
        let dir = tempfile::tempdir().unwrap();
        let batch_dir = dir.path().join("batch-renders");
        let source_dir = dir.path().join("source");
        std::fs::create_dir_all(&batch_dir).unwrap();
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::write(source_dir.join("beat.wav"), b"source beat before crash").unwrap();
        let project_path = source_dir.join("project.json");
        let mut project = Project::new("Bài hát nguồn");
        project.beat = Some(serde_json::json!({"filename":"beat.wav","duration":4.0}));
        std::fs::write(&project_path, serde_json::to_vec(&project).unwrap()).unwrap();
        let mut record = BatchRecord::new(
            "VietKaraStudio",
            "batch",
            &["current".into()],
            &["beat".into()],
        )
        .unwrap();
        record.status = BatchStatus::Running;
        let first_id = vietkara_domain::new_id();
        let first_dir = BatchRecord::record_path(dir.path(), &record.id)
            .parent()
            .unwrap()
            .join("items")
            .join(&first_id);
        std::fs::create_dir_all(&first_dir).unwrap();
        let first_snapshot = first_dir.join("project.json");
        std::fs::write(&first_snapshot, serde_json::to_vec(&project).unwrap()).unwrap();
        std::fs::write(first_dir.join("beat.wav"), b"immutable snapshot beat").unwrap();
        let first_snapshot_hash = vietkara_storage::sha256_file(&first_snapshot).unwrap();
        let first_beat_hash = vietkara_storage::sha256_file(&first_dir.join("beat.wav")).unwrap();
        let first_output = dir.path().join("done.mp4");
        let mut done = BatchItem {
            project_id: project.id.clone(),
            render_id: first_id,
            name: "Đã xong".into(),
            variant: serde_json::json!({
                "source_project_path":project_path,
                "snapshot_project_path":first_snapshot,
                "snapshot_project_sha256":first_snapshot_hash,
                "snapshot_media":{"beat.wav":first_beat_hash},
                "output_path":first_output,
            }),
            directory: source_dir.to_string_lossy().into_owned(),
            assets: vec![first_dir.join("beat.wav").to_string_lossy().into_owned()],
            status: ItemStatus::Queued,
            result: None,
            error: None,
        };
        done.status = ItemStatus::Done;
        done.result = Some(serde_json::json!({"output_path":"done.mp4"}));
        record.completed = 1;
        record.items.push(done);
        let second_id = vietkara_domain::new_id();
        let second_dir = BatchRecord::record_path(dir.path(), &record.id)
            .parent()
            .unwrap()
            .join("items")
            .join(&second_id);
        std::fs::create_dir_all(&second_dir).unwrap();
        let second_snapshot = second_dir.join("project.json");
        std::fs::write(&second_snapshot, serde_json::to_vec(&project).unwrap()).unwrap();
        std::fs::write(second_dir.join("beat.wav"), b"beat snapshot").unwrap();
        let second_snapshot_hash = vietkara_storage::sha256_file(&second_snapshot).unwrap();
        let second_beat_hash = vietkara_storage::sha256_file(&second_dir.join("beat.wav")).unwrap();
        record.items.push(BatchItem {
            project_id: project.id,
            render_id: second_id,
            name: "Đang render".into(),
            variant: serde_json::json!({
                "source_project_path":project_path,
                "snapshot_project_path":second_snapshot,
                "snapshot_project_sha256":second_snapshot_hash,
                "snapshot_media":{"beat.wav":second_beat_hash},
                "output_path":dir.path().join("pending.mp4"),
            }),
            directory: source_dir.to_string_lossy().into_owned(),
            assets: vec![second_dir.join("beat.wav").to_string_lossy().into_owned()],
            status: ItemStatus::Running,
            result: None,
            error: None,
        });
        record.next_index = 1;
        record.save(dir.path()).unwrap();

        let recovered = load_latest_batch_record(dir.path()).unwrap();
        assert_eq!(recovered.status, BatchStatus::Cancelled);
        assert_eq!(recovered.completed, 1);
        assert_eq!(recovered.items[0].status, ItemStatus::Done);
        assert_eq!(
            recovered.items[0].result.as_ref().unwrap()["output_path"],
            "done.mp4"
        );
        assert_eq!(recovered.items[1].status, ItemStatus::Queued);
        assert_eq!(recovered.first_pending(), 1);
        std::fs::write(
            source_dir.join("beat.wav"),
            b"source changed after the batch started",
        )
        .unwrap();
        let saved_snapshot_beat = std::fs::read(second_dir.join("beat.wav")).unwrap();
        assert_eq!(saved_snapshot_beat, b"beat snapshot");
        let expected_snapshot_hash = recovered.items[1].variant["snapshot_media"]["beat.wav"]
            .as_str()
            .unwrap();
        assert_eq!(
            vietkara_storage::sha256_file(&second_dir.join("beat.wav")).unwrap(),
            expected_snapshot_hash
        );
        assert!(validate_batch_snapshot(&recovered.items[1]).is_ok());
        std::fs::write(second_dir.join("beat.wav"), b"tampered beat snapshot").unwrap();
        assert!(
            validate_batch_snapshot(&recovered.items[1])
                .unwrap_err()
                .contains("Media snapshot bị thay đổi")
        );
        assert_eq!(
            BatchRecord::load(dir.path(), &record.id).unwrap().items[1].status,
            ItemStatus::Queued
        );
    }

    #[test]
    fn sync_suggestion_is_advisory_and_validates_anchors_without_mutating_project() {
        let project = Project::new("sync");
        let suggestion = serde_json::json!({
            "requires_confirmation": true,
            "anchors": [{"ref": 0.0, "beat": 2.0}, {"ref": 10.0, "beat": 12.0}],
        });
        let anchors = validate_sync_suggestion(&project, &suggestion).unwrap();
        assert_eq!(anchors.len(), 2);
        assert!(project.anchors.is_empty());

        let unconfirmed = serde_json::json!({
            "requires_confirmation": false,
            "anchors": [{"ref": 0.0, "beat": 0.0}],
        });
        assert!(validate_sync_suggestion(&project, &unconfirmed).is_err());

        let invalid = serde_json::json!({
            "requires_confirmation": true,
            "anchors": [{"ref": 1.0, "beat": 3.0}, {"ref": 0.0, "beat": 4.0}],
        });
        assert!(validate_sync_suggestion(&project, &invalid).is_err());
    }

    #[test]
    fn role_suggestions_are_two_lane_single_step_and_respect_locked_words() {
        let words = vec![
            serde_json::from_value::<Word>(serde_json::json!({
                "id":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","text":"Xin","line":7,
                "start":0.0,"end":0.4,"role":"all"
            }))
            .unwrap(),
            serde_json::from_value::<Word>(serde_json::json!({
                "id":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","text":"chào","line":7,
                "start":0.4,"end":0.8,"role":"all","locked":true
            }))
            .unwrap(),
            serde_json::from_value::<Word>(serde_json::json!({
                "id":"cccccccccccccccccccccccccccccccc","text":"bạn","line":8,
                "start":1.0,"end":1.4,"role":"female"
            }))
            .unwrap(),
            serde_json::from_value::<Word>(serde_json::json!({
                "id":"dddddddddddddddddddddddddddddddd","text":"đây","line":9,
                "start":2.0,"end":2.4,"role":"all"
            }))
            .unwrap(),
        ];
        let result = serde_json::json!({
            "two_registers":true,"roles":["male","female","female"],
        });
        let mut project = Project::new("roles");
        project.words = words.clone();
        let (command, changed) = role_application_command(&project, &[7, 8, 9], &result).unwrap();
        assert_eq!(changed, 2);
        let command_for_undo = command.clone().expect("role batch command");
        let Some(Command::Batch(commands)) = command else {
            panic!("role suggestions should be one batch command");
        };
        let Command::SetSettings(settings) = &commands[0] else {
            panic!("enabling duet and applying suggestions must share one undo step");
        };
        assert_eq!(
            settings.extra.get("singing_mode").and_then(Value::as_str),
            Some("duet")
        );
        let updated = commands
            .iter()
            .filter_map(|command| match command {
                Command::ReplaceWord { id, word } => Some((id.clone(), word.role.clone())),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            updated,
            [
                ("a".repeat(32), "male".to_owned()),
                ("d".repeat(32), "female".to_owned())
            ]
        );
        assert_eq!(words[0].role, "all");
        assert_eq!(words[1].role, "all");
        assert_eq!(words[2].role, "female");

        let mut editor = Editor::new(project).unwrap();
        editor.execute(command_for_undo).unwrap();
        assert_eq!(editor.project().words[0].role, "male");
        assert_eq!(
            editor
                .project()
                .settings
                .extra
                .get("singing_mode")
                .and_then(Value::as_str),
            Some("duet")
        );
        editor.undo().unwrap();
        assert_eq!(editor.project().words[0].role, "all");
        assert_ne!(
            editor
                .project()
                .settings
                .extra
                .get("singing_mode")
                .and_then(Value::as_str),
            Some("duet")
        );

        let unsupported =
            serde_json::json!({"two_registers":true,"roles":["voice_c","female","all"]});
        assert!(role_suggestion_command(&words, &[7, 8, 9], &unsupported).is_err());
    }

    #[test]
    fn role_vocal_cache_publish_never_overwrites_existing_files() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source.wav");
        let cache_directory = directory.path().join("cache");
        let destination = cache_directory.join("cache.wav");
        std::fs::write(&source, b"new vocal").unwrap();
        publish_cache_file(&source, &destination).unwrap();
        assert_eq!(std::fs::read(&destination).unwrap(), b"new vocal");
        let cached_entries = std::fs::read_dir(&cache_directory)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        assert_eq!(cached_entries, [std::ffi::OsString::from("cache.wav")]);
        std::fs::write(&destination, b"prior cache").unwrap();
        publish_cache_file(&source, &destination).unwrap();
        assert_eq!(std::fs::read(&destination).unwrap(), b"prior cache");
    }

    #[test]
    fn vocal_cache_reuses_only_the_reference_fingerprint_path() {
        let directory = tempfile::tempdir().unwrap();
        let reference = directory.path().join("reference.wav");
        let data_root = directory.path().join("data");
        std::fs::write(&reference, b"reference audio").unwrap();
        let fingerprint = vietkara_storage::sha256_file(&reference).unwrap();
        let cached = data_root
            .join("role-vocals")
            .join(format!("{fingerprint}.wav"));
        std::fs::create_dir_all(cached.parent().unwrap()).unwrap();
        std::fs::write(&cached, b"cached vocal").unwrap();
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let result = ensure_cached_vocal(
            VocalCacheRequest {
                config: &WorkerConfig::new("worker-that-must-not-run"),
                project_id: "project-id",
                reference_path: &reference,
                data_root: &data_root,
                device: "cpu",
            },
            &cancel,
            |_| panic!("cache hit must not start a worker"),
            |_, _| panic!("cache hit must not report worker progress"),
        )
        .unwrap();
        assert_eq!(result.0, cached);
        assert_eq!(result.1, fingerprint);
    }

    #[test]
    fn ai_lyrics_candidate_validates_words_and_preserves_line_roles() {
        let mut project = Project::new("AI lyrics");
        project.words = vec![
            serde_json::from_value(serde_json::json!({
                "id":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "line_uid":"11111111111111111111111111111111",
                "text":"Xin","start":0.0,"end":0.4,"line":7,"role":"male","row":0
            }))
            .unwrap(),
            serde_json::from_value(serde_json::json!({
                "id":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "line_uid":"22222222222222222222222222222222",
                "text":"chào","start":1.0,"end":1.4,"line":12,"role":"female","row":1
            }))
            .unwrap(),
        ];
        let result = serde_json::json!({
            "text":"Xin\nchào",
            "words":[
                {"text":"Xin","start":0.1,"end":0.5,"line":0,"role":"all","estimated":false},
                {"text":"chào","start":1.1,"end":1.5,"line":1,"role":"all","estimated":true}
            ]
        });
        let (words, text) = build_ai_lyrics_candidate(&result, &project, None).unwrap();
        assert_eq!(text, "Xin\nchào");
        assert_eq!(
            words.iter().map(|word| word.line).collect::<Vec<_>>(),
            [7, 12]
        );
        assert_eq!(
            words
                .iter()
                .map(|word| word.role.as_str())
                .collect::<Vec<_>>(),
            ["male", "female"]
        );
        assert_eq!(
            words.iter().map(|word| word.row).collect::<Vec<_>>(),
            [Some(0), Some(1)]
        );
        assert_ne!(words[0].id, project.words[0].id);
        assert!(words.iter().all(|word| vietkara_domain::is_hex32(&word.id)));

        project.words[0].locked = Some(true);
        let region_result = serde_json::json!({
            "text":"chào lại",
            "words":[
                {"text":"chào","start":1.1,"end":1.3,"line":0,"estimated":false},
                {"text":"lại","start":1.3,"end":1.5,"line":0,"estimated":true}
            ]
        });
        let (region_words, region_text) =
            build_ai_lyrics_candidate(&region_result, &project, Some(&[12])).unwrap();
        assert_eq!(region_words.len(), 3);
        assert_eq!(region_words[0].id, project.words[0].id);
        assert_eq!(region_words[0].locked, Some(true));
        assert_eq!(region_words[1].text, "chào");
        assert_eq!(region_words[2].text, "lại");
        assert_eq!(region_text, "Xin\nchào lại");

        let malformed = serde_json::json!({
            "words":[{"text":"Xin","start":0.5,"end":0.2,"line":0}]
        });
        assert!(build_ai_lyrics_candidate(&malformed, &Project::new("invalid"), None).is_err());
    }

    #[test]
    fn legacy_voice_labels_map_into_the_two_duet_roles() {
        assert_eq!(role_label("voice_c"), "Nam");
        assert_eq!(role_label("voice_e"), "Nam");
        assert_eq!(role_label("voice_d"), "Nữ");
        assert_eq!(role_label("voice_f"), "Nữ");
    }

    #[test]
    fn output_device_choices_exclude_default_only_when_it_is_known() {
        let devices = vec!["Speakers".into(), "Headphones".into()];
        assert_eq!(
            additional_output_devices(&devices, Some("Speakers")),
            ["Headphones"]
        );
        assert_eq!(
            additional_output_devices(&devices, None),
            ["Speakers", "Headphones"]
        );
    }

    #[test]
    fn worker_snapshot_does_not_clear_a_missing_audio_warning() {
        let missing = "Chưa có beat.wav; hãy nhập hoặc relink track đang nghe";
        assert_eq!(
            merge_playback_error(None, Some(missing)),
            Some(missing.into())
        );
        assert_eq!(
            merge_playback_error(Some("File audio hỏng".into()), Some(missing)),
            Some("File audio hỏng".into())
        );
        assert_eq!(merge_playback_error(None, Some("Seek thất bại")), None);
    }

    #[test]
    fn playback_track_resolves_to_managed_audio_file() {
        assert_eq!(audio_slot_filename(AudioSlot::Beat), "beat.wav");
        assert_eq!(audio_slot_filename(AudioSlot::Reference), "reference.wav");
    }

    #[test]
    fn missing_audio_is_visible_and_relink_resolves_project_local_asset() {
        let directory = tempfile::tempdir().unwrap();
        let project_path = directory.path().join("project.json");
        let mut project = Project::new("relink");
        project.beat = Some(serde_json::json!({
            "filename": "beat.wav",
            "sha256": "expected-hash",
        }));
        assert!(audio_slot_missing(
            &project,
            Some(&project_path),
            AudioSlot::Beat
        ));
        assert!(
            audio_slot_label(&project, Some(&project_path), AudioSlot::Beat).contains("thiếu file")
        );

        std::fs::write(directory.path().join("beat.wav"), b"audio fixture").unwrap();
        assert!(!audio_slot_missing(
            &project,
            Some(&project_path),
            AudioSlot::Beat
        ));
        assert_eq!(
            project_audio_file(&project, Some(&project_path), AudioSlot::Beat),
            Some(directory.path().join("beat.wav"))
        );

        project.beat = Some(serde_json::json!({ "filename": "../outside.wav" }));
        assert!(audio_slot_missing(
            &project,
            Some(&project_path),
            AudioSlot::Beat
        ));
        assert_eq!(
            project_audio_file(&project, Some(&project_path), AudioSlot::Beat),
            None
        );
    }

    #[test]
    fn preview_sweep_geometry_tracks_partial_fill_feather_and_glow() {
        assert_eq!(SWEEP_PREVIEW_GLOW_OFFSETS.len(), 9);
        assert_eq!(SWEEP_PREVIEW_GLOW_OFFSETS[0], (0.0, 0.0, 1.0));
        let geometry = sweep_preview_geometry(100.0, 0.5, 12.0, 90.0, 20.0, 2.0);
        assert!((geometry.solid_width - 44.0).abs() < 0.001);
        assert_eq!(geometry.feather_segments.len(), 4);
        assert!((geometry.feather_segments[0].0 - 44.0).abs() < 0.001);
        assert!((geometry.feather_segments[0].2 - 0.875).abs() < 0.001);
        let (glow_left, glow_width, glow_opacity) = geometry.glow_band.unwrap();
        assert!((glow_left - 36.6).abs() < 0.001);
        assert!((glow_width - 26.8).abs() < 0.001);
        assert!((glow_opacity - 0.6).abs() < 0.001);

        assert!(
            sweep_preview_geometry(100.0, 0.0, 12.0, 90.0, 20.0, 2.0)
                .glow_band
                .is_none()
        );
        for progress in [0.01, 0.05, 0.95, 0.99] {
            let edge_geometry = sweep_preview_geometry(100.0, progress, 12.0, 90.0, 20.0, 2.0);
            assert!(
                edge_geometry
                    .feather_segments
                    .iter()
                    .all(|(left, segment_width, _)| *left >= 0.0
                        && *segment_width > 0.0
                        && *left + *segment_width <= 100.0)
            );
            if let Some((left, band_width, _)) = edge_geometry.glow_band {
                assert!(left >= 0.0 && left + band_width <= 100.0);
            }
        }
        assert_eq!(
            sweep_preview_geometry(100.0, 0.5, 0.0, 0.0, 20.0, 2.0).solid_width,
            50.0
        );
    }

    #[test]
    fn workspace_columns_shrink_with_logical_viewport_width() {
        assert_eq!(
            workspace_size(800.),
            WorkspaceSize {
                left: 176.,
                preview: 320.,
                right: 260.,
            }
        );
        assert_eq!(
            workspace_size(1100.),
            WorkspaceSize {
                left: 190.,
                preview: 410.,
                right: 300.,
            }
        );
        assert_eq!(
            workspace_size(1440.),
            WorkspaceSize {
                left: 220.,
                preview: 520.,
                right: 320.,
            }
        );
    }

    #[test]
    fn workspace_splitters_resize_the_intended_column_and_respect_minimums() {
        let initial = workspace_size(1440.);
        let left_drag = resize_workspace(initial, WorkspaceEdge::Left, 80., 1440.);
        assert_eq!(left_drag.left, 300.);
        assert_eq!(left_drag.right, initial.right);
        assert_eq!(left_drag.preview, initial.preview);

        let right_drag = resize_workspace(initial, WorkspaceEdge::Right, -60., 1440.);
        assert_eq!(right_drag.right, 380.);
        assert_eq!(right_drag.left, initial.left);

        let narrow = resize_workspace(initial, WorkspaceEdge::Left, -500., 800.);
        assert_eq!(narrow.left, 170.);
        assert!(narrow.left + narrow.right + 392. <= 800.);

        let fitted = fit_workspace(
            WorkspaceSize {
                left: 420.,
                preview: 520.,
                right: 460.,
            },
            800.,
        );
        assert!(fitted.left + fitted.right + 392. <= 800.);
        assert!(fitted.preview <= 800. - fitted.left - fitted.right - 112.);
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
        // Integration coverage only; a checkout without the bundled
        // FFmpeg/FFprobe pair skips it instead of failing (same convention
        // as the storage/render integration tests).
        let Ok(ffmpeg) = vietkara_media::find_binary("ffmpeg") else {
            return;
        };
        let Ok(ffprobe) = vietkara_media::find_binary("ffprobe") else {
            return;
        };
        let audio = std::process::Command::new(&ffmpeg)
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
            || {},
        )
        .unwrap();
        assert!(target.is_file());
        assert!(progress > 0.0);
        let verified =
            vietkara_render::verify_output_with_ffprobe(&target, 1.0, 1920, 1080, &ffprobe)
                .expect("published MP4 must contain beat audio and a matching video stream");
        assert_eq!((verified.width, verified.height), (1920, 1080));
        assert_eq!(verified.audio_codec.as_deref(), Some("aac"));
        assert!((verified.duration - 1.0).abs() < 0.1);
        let samples = vietkara_render::sample_frames(
            &target,
            &output_directory.join("qa-frames"),
            1.0,
            &ffmpeg,
        )
        .expect("published MP4 should yield three reviewable frames");
        assert_eq!(samples.times, vec![0.25, 0.5]);
        assert!(samples.files.iter().all(|path| path.is_file()));
        let original_size = std::fs::metadata(&target).unwrap().len();
        let error = render_project(
            project,
            plan,
            &project_path,
            &target,
            &cancel,
            |_| {},
            || {},
        )
        .unwrap_err();
        assert!(error.contains("đã tồn tại"));
        assert_eq!(std::fs::metadata(&target).unwrap().len(), original_size);
    }
}
