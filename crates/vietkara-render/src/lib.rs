//! Pure render helpers for VietKara Studio: ASS text helpers, render
//! profiles, FFmpeg command construction and output verification.
//!
//! Ported from the Python baseline (`subtitles.py`, `render_info.py`,
//! `engine.render()` — the simple colour-background path — and the
//! render keys in `styles.DEFAULT_SETTINGS`). See
//! `docs/compatibility-matrix.md` for the compatibility contract.
//!
//! # Scope
//!
//! This crate is deliberately pure and Linux-testable:
//!
//! * ASS helpers (`stamp`, `ass_color`, `literal`, `blend`,
//!   `animation`, `rounded_box`, `ICONS`) — exact string ports.
//! * `RenderProfile` / `frame_size` / `fps_value` / `encoding_options`
//!   — the rules from `render_info.describe` / `encoding_options`.
//! * `ffmpeg_command` — a pure argv builder for color/image/video
//!   backgrounds, section-image scene overlays and guide audio, so tests
//!   can assert the exact command.
//! * `verify_output` — runs `ffprobe` on a rendered file.
//!
//! # Explicitly NOT in scope (Phase 4, after the layout contract
//! stabilises)
//!
//! * `build_ass()` / `display_plan()` layout: that needs font
//!   measurement (PIL / fontTools, Windows fonts) to compute row
//!   positions, word widths and line wrapping. Porting it now, on
//!   Linux with different fonts, would produce different layout and
//!   different golden ASS output. Do not try to port `display_plan`
//!   here.
//! * Scene keyframe motion/decorations, logo and countdown overlays, and
//!   the NVENC fallback in `engine.render()` remain outside this slice.
//! * Audio normalisation (`loudnorm`) — the simple builder omits it;
//!   the Python trailer only adds `-af loudnorm=...` when the
//!   `audio_normalize` setting is on.
//! * The `ffprobe`-missing fallback in `verify_output` (parsing
//!   `ffmpeg -i` stderr). `ffprobe` is the primary path in Python
//!   too, and it is installed wherever this crate is tested.
//! * AI / playback / jobs — those live in `vietkara-ai-bridge`,
//!   `vietkara-media` and `vietkara-jobs`.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;

// ---------------------------------------------------------------------------
// Python-compatible numeric formatting
// ---------------------------------------------------------------------------

/// Python `round(x)`: round-half-to-even on the binary float value.
/// Rust's `round()` is half-away-from-zero, which differs exactly on
/// `.5` cases — e.g. Python `round(0.5) == 0`, `round(127.5) == 128`.
/// `stamp(0.005)` is `0:00:00.00` in Python and would be `0:00:00.01`
/// with half-away-from-zero rounding.
fn py_round(value: f64) -> f64 {
    value.round_ties_even()
}

/// Python `str(float)`: `30.0` prints as `"30.0"`, `29.97` as
/// `"29.97"`. Rust's `{}` for an `f64` prints `30` for `30.0`, which
/// would change the FFmpeg argv (`r=30` vs the Python `r=30.0`).
fn py_float(value: f64) -> String {
    if value.is_finite() && value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{value:.1}")
    } else {
        format!("{value}")
    }
}

// ---------------------------------------------------------------------------
// ASS helpers — subtitles.py
// ---------------------------------------------------------------------------

/// `subtitles.stamp`: centiseconds, `H:MM:SS.cc`, clamped at 0.
pub fn stamp(seconds: f64) -> String {
    let n = py_round(seconds * 100.0) as i64;
    let n = n.max(0);
    format!(
        "{}:{:02}:{:02}.{:02}",
        n / 360_000,
        (n / 6_000) % 60,
        (n / 100) % 60,
        n % 100
    )
}

fn hex_channel(hex: &str, offset: usize) -> u32 {
    hex.get(offset..offset + 2)
        .and_then(|s| u32::from_str_radix(s, 16).ok())
        .unwrap_or(0)
}

/// `subtitles.color`: `&H{alpha:02X}{BB}{GG}{RR}`, where
/// `alpha = round(255 * (1 - opacity/100))` and the BB/GG/RR slices
/// are taken verbatim from the input hex (case preserved, as in
/// Python — e.g. `#aabbcc` at 33% gives `&HABccbbaa`).
pub fn ass_color(hex: &str, opacity: u8) -> String {
    let opacity = opacity.min(100);
    let alpha = py_round(255.0 * (1.0 - f64::from(opacity) / 100.0)) as u32;
    let (bb, gg, rr) = (
        hex.get(5..7).unwrap_or("00"),
        hex.get(3..5).unwrap_or("00"),
        hex.get(1..3).unwrap_or("00"),
    );
    format!("&H{alpha:02X}{bb}{gg}{rr}")
}

/// `subtitles.literal`: escape backslash / braces for ASS, newline
/// becomes a space. Backslash becomes the fullwidth `＼`, braces
/// become `｛` / `｝`.
pub fn literal(text: &str) -> String {
    text.replace('\\', "＼")
        .replace('{', "｛")
        .replace('}', "｝")
        .replace('\n', " ")
}

/// `subtitles.blend`: per-channel linear blend, `p` clamped to
/// `0..=1`, each channel rounded (half-to-even, as in Python).
pub fn blend(a: &str, b: &str, p: f64) -> String {
    let p = p.clamp(0.0, 1.0);
    let mut out = String::from("#");
    for offset in [1, 3, 5] {
        let av = hex_channel(a, offset) as f64;
        let bv = hex_channel(b, offset) as f64;
        let value = py_round(av * (1.0 - p) + bv * p) as u32;
        out.push_str(&format!("{value:02x}"));
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TextAlign {
    Left,
    #[default]
    Center,
    Right,
    Justify,
}

/// One lyric row, as `subtitles.animation` reads it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RowAnim {
    pub show_start: f64,
    pub show_end: f64,
    pub x: f64,
    pub y: f64,
    pub font_size: f64,
    pub align: TextAlign,
    pub section_start: bool,
    pub section_end: bool,
}

impl Default for RowAnim {
    fn default() -> Self {
        Self {
            show_start: 0.0,
            show_end: 0.0,
            x: 0.0,
            y: 0.0,
            font_size: 72.0,
            align: TextAlign::Center,
            section_start: false,
            section_end: false,
        }
    }
}

/// Options `subtitles.animation` reads: `line_animation` is the
/// validated settings string (`styles.ANIMATIONS`), and
/// `horizontal_scale` is the validated float (so `100.0`, not `100`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnimOpts {
    pub line_animation: String,
    pub horizontal_scale: f64,
}

impl Default for AnimOpts {
    fn default() -> Self {
        Self {
            line_animation: "none".to_owned(),
            horizontal_scale: 100.0,
        }
    }
}

/// `subtitles.animation`: position / fade / scale / move tags for a
/// row. `duration` is in ms (`max(1, round(...))`), `ms_anim` is
/// `min(600, duration / 3)`. Unknown modes fall through to the
/// continuous-move branch, exactly as in Python.
pub fn animation(row: &RowAnim, opts: &AnimOpts) -> String {
    let duration = py_round((row.show_end - row.show_start) * 1000.0) as i64;
    let duration = duration.max(1);
    let ms = (duration / 3).min(600);
    let mode = opts.line_animation.as_str();
    let pos = format!("\\an7\\pos({:.2},{:.2})", row.x, row.y);
    let fade_in = if mode != "fadesec" || row.section_start {
        ms
    } else {
        0
    };
    let fade_out = if mode != "fadesec" || row.section_end {
        ms
    } else {
        0
    };
    if mode == "none" {
        return pos;
    }
    if mode == "slowfade" || mode == "fadesec" {
        return format!("{pos}\\fad({fade_in},{fade_out})");
    }
    let sx = opts.horizontal_scale;
    if matches!(mode, "zoom" | "zoomout" | "popbounce" | "punchzoom") {
        let initial: i64 = match mode {
            "zoom" => 65,
            "zoomout" => 135,
            "popbounce" => 70,
            _ => 135, // punchzoom
        };
        let mid: i64 = if mode == "popbounce" { 112 } else { 100 };
        let half = ms / 2;
        let tags = format!(
            "\\fscx{:.2}\\fscy{initial}\\t(0,{half},\\fscx{:.2}\\fscy{mid})\\t({half},{ms},\\fscx{}\\fscy100)",
            sx * initial as f64 / 100.0,
            sx * mid as f64 / 100.0,
            py_float(sx),
        );
        let tags = format!(
            "{tags}\\t({},{duration},\\fscx{:.2}\\fscy{initial})",
            duration - ms,
            sx * initial as f64 / 100.0,
        );
        return format!("{pos}{tags}\\fad({half},{half})");
    }
    if mode == "flipup" {
        return format!(
            "{pos}\\frx70\\t(0,{ms},\\frx0)\\t({},{duration},\\frx70)\\fad({ms},{ms})",
            duration - ms,
        );
    }
    // Continuous movement with a short enter and reverse exit.
    let dx = if mode == "split" {
        row.font_size
            * if row.align == TextAlign::Right {
                -1.0
            } else {
                1.0
            }
    } else {
        0.0
    };
    let dy = if mode == "slidedown" {
        -row.font_size
    } else if mode == "slideup" {
        row.font_size
    } else {
        0.0
    };
    format!(
        "\\an7\\move({:.2},{:.2},{:.2},{:.2},0,{ms})\\fad({ms},{ms})",
        row.x + dx,
        row.y + dy,
        row.x,
        row.y,
    )
}

/// `subtitles.rounded_box`: a rounded-rectangle drawing path,
/// `k = r * 0.552285`, segments in the same order as Python.
pub fn rounded_box(x: f64, y: f64, w: f64, h: f64, radius: f64) -> String {
    let r = radius.min(w / 2.0).min(h / 2.0);
    let k = r * 0.552_285;
    let mut out = String::new();
    out.push_str(&format!(
        "m {:.2} {:.2} l {:.2} {:.2} ",
        x + r,
        y,
        x + w - r,
        y
    ));
    out.push_str(&format!(
        "b {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} ",
        x + w - r + k,
        y,
        x + w,
        y + r - k,
        x + w,
        y + r,
    ));
    out.push_str(&format!(
        "l {:.2} {:.2} b {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} ",
        x + w,
        y + h - r,
        x + w,
        y + h - r + k,
        x + w - r + k,
        y + h,
        x + w - r,
        y + h,
    ));
    out.push_str(&format!(
        "l {:.2} {:.2} b {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} ",
        x + r,
        y + h,
        x + r - k,
        y + h,
        x,
        y + h - r + k,
        x,
        y + h - r,
    ));
    out.push_str(&format!(
        "l {:.2} {:.2} b {:.2} {:.2} {:.2} {:.2} {:.2} {:.2}",
        x,
        y + r,
        x,
        y + r - k,
        x + r - k,
        y,
        x + r,
        y,
    ));
    out
}

/// `subtitles.ICONS`: countdown vector shapes (ASS drawing commands).
pub const ICONS: &[(&str, &str)] = &[
    (
        "mic",
        "m 8 3 b 8 0 16 0 16 3 l 16 11 b 16 16 8 16 8 11 l 8 3 m 5 10 l 6 10 b 6 20 18 20 18 10 l 19 10 b 19 17 15 20 13 20 l 13 23 18 23 18 25 6 25 6 23 11 23 11 20 b 7 20 5 16 5 10",
    ),
    (
        "star",
        "m 12 0 l 15 8 24 9 17 15 19 24 12 19 5 24 7 15 0 9 9 8 12 0",
    ),
    ("heart", "m 12 23 b -7 11 0 -5 12 5 b 24 -5 31 11 12 23"),
    (
        "note",
        "m 9 3 l 22 0 22 18 b 20 24 12 22 15 17 b 16 15 19 15 20 15 l 20 6 11 9 11 21 b 8 27 0 24 3 20 b 4 18 7 18 9 18 l 9 3",
    ),
    (
        "moon",
        "m 20 1 b 1 -4 -7 22 13 25 b 20 26 25 22 26 18 b 8 23 4 5 20 1",
    ),
];

pub fn icon_shape(name: &str) -> Option<&'static str> {
    ICONS.iter().find(|(n, _)| *n == name).map(|(_, s)| *s)
}

// ---------------------------------------------------------------------------
// Render profile — render_info.describe / styles.DEFAULT_SETTINGS
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Aspect {
    Landscape,
    Portrait,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Resolution {
    #[serde(rename = "1080p")]
    P1080,
    #[serde(rename = "4k")]
    K4,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Fps {
    /// `render_fps == "source"`: no video source in the simple
    /// colour-background path, so this resolves to 30 fps, as
    /// `describe` does when there is no source.
    Source,
    Fixed(f64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RenderQuality {
    Standard,
    High,
    Maximum,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RenderFormat {
    Mp4,
    Mov,
    Mkv,
}

impl RenderFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Mp4 => "mp4",
            Self::Mov => "mov",
            Self::Mkv => "mkv",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AudioOutput {
    Beat,
    Guide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VideoEncoder {
    Libx264,
    H264Nvenc,
}

impl VideoEncoder {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Libx264 => "libx264",
            Self::H264Nvenc => "h264_nvenc",
        }
    }
}

/// Render settings, mirroring the `render_*` / `aspect` /
/// `resolution` / `audio_output` / `background` keys of
/// `styles.DEFAULT_SETTINGS` (`render_quality: "high"`,
/// `render_fps: "source"`, `render_format: "mp4"`,
/// `audio_output: "beat"`, `resolution: "1080p"`,
/// `background: "#101d2b"`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RenderProfile {
    pub aspect: Aspect,
    pub resolution: Resolution,
    pub fps: Fps,
    pub quality: RenderQuality,
    pub format: RenderFormat,
    pub audio_output: AudioOutput,
    pub background_color: String,
    /// Test-only override for `frame_size()`. Production profiles
    /// leave this `None` and get the real 1920×1080 / 3840×2160
    /// sizes. The integration test sets `(320, 240)` so a real
    /// FFmpeg render stays fast.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_size_override: Option<(u32, u32)>,
}

impl Default for RenderProfile {
    fn default() -> Self {
        Self {
            aspect: Aspect::Landscape,
            resolution: Resolution::P1080,
            fps: Fps::Source,
            quality: RenderQuality::High,
            format: RenderFormat::Mp4,
            audio_output: AudioOutput::Beat,
            background_color: "#101d2b".to_owned(),
            frame_size_override: None,
        }
    }
}

impl RenderProfile {
    /// `render_info.describe`: `(1920, 1080)` landscape,
    /// `(1080, 1920)` portrait, doubled for 4k.
    pub fn frame_size(&self) -> (u32, u32) {
        if let Some(size) = self.frame_size_override {
            return size;
        }
        let (width, height) = match self.aspect {
            Aspect::Landscape => (1920, 1080),
            Aspect::Portrait => (1080, 1920),
        };
        if self.resolution == Resolution::K4 {
            (width * 2, height * 2)
        } else {
            (width, height)
        }
    }

    /// Effective fps: `Source` is 30 (no video source in the simple
    /// path); `Fixed` is clamped to `1..=120` like `fps_value`,
    /// falling back to 30 outside that range.
    pub fn fps_value(&self) -> f64 {
        match self.fps {
            Fps::Source => 30.0,
            Fps::Fixed(value) => fps_value_f64(value),
        }
    }

    pub fn output_name(&self) -> String {
        format!("karaoke.{}", self.format.extension())
    }

    /// Build a profile from a project's `settings` object (the
    /// `vietkara-domain` `Project.settings` value), falling back to
    /// `DEFAULT_SETTINGS` for missing / invalid keys.
    pub fn from_project(project: &vietkara_domain::Project) -> Self {
        use vietkara_domain::settings as domain;
        let settings = &project.settings;
        let mut profile = Self {
            aspect: match settings.aspect {
                domain::Aspect::Landscape => Aspect::Landscape,
                domain::Aspect::Portrait => Aspect::Portrait,
            },
            resolution: match settings.resolution {
                domain::Resolution::Hd => Resolution::P1080,
                domain::Resolution::UltraHd => Resolution::K4,
            },
            quality: match settings.render_quality {
                domain::RenderQuality::Standard => RenderQuality::Standard,
                domain::RenderQuality::High => RenderQuality::High,
                domain::RenderQuality::Maximum => RenderQuality::Maximum,
            },
            format: match settings.render_format {
                domain::RenderFormat::Mp4 => RenderFormat::Mp4,
                domain::RenderFormat::Mov => RenderFormat::Mov,
                domain::RenderFormat::Mkv => RenderFormat::Mkv,
            },
            audio_output: match settings.audio_output {
                domain::AudioOutput::Beat => AudioOutput::Beat,
                domain::AudioOutput::Guide => AudioOutput::Guide,
            },
            fps: match settings.render_fps {
                domain::RenderFps::Source => Fps::Source,
                domain::RenderFps::Thirty => Fps::Fixed(30.),
                domain::RenderFps::Sixty => Fps::Fixed(60.),
            },
            ..Default::default()
        };
        if let Some(serde_json::Value::String(v)) = settings.extra.get("background")
            && is_hex_color(v)
        {
            profile.background_color = v.to_ascii_lowercase();
        }
        profile
    }
}

fn is_hex_color(value: &str) -> bool {
    value.len() == 7 && value.starts_with('#') && value[1..].bytes().all(|b| b.is_ascii_hexdigit())
}

fn fps_value_f64(value: f64) -> f64 {
    if value.is_finite() && (1.0..=120.0).contains(&value) {
        value
    } else {
        30.0
    }
}

/// `render_info.fps_value`: parse a rate given as a decimal
/// (`"29.97"`) or a fraction (`"30000/1001"`, as `Fraction` does in
/// Python); anything unparsable or outside `1..=120` is 30.
pub fn fps_value(rate: &str) -> f64 {
    let rate = rate.trim();
    let parsed = if let Some((num, den)) = rate.split_once('/') {
        let num: Option<f64> = num.trim().parse().ok();
        let den: Option<f64> = den.trim().parse().ok();
        match (num, den) {
            (Some(n), Some(d)) if d != 0.0 => Some(n / d),
            _ => None,
        }
    } else {
        rate.parse().ok()
    };
    parsed.map_or(30.0, fps_value_f64)
}

/// `render_info.encoding_options`: libx264 uses
/// standard `(crf 20, veryfast)`, high `(16, medium)`,
/// maximum `(12, slow)`; NVENC uses preset `p4` for standard and
/// `p7` otherwise, with `-rc vbr -cq <cq> -b:v 0`.
pub fn encoding_options(quality: RenderQuality, encoder: VideoEncoder) -> Vec<String> {
    let (crf, cq, preset) = match quality {
        RenderQuality::Standard => (20, 20, "veryfast"),
        RenderQuality::High => (16, 16, "medium"),
        RenderQuality::Maximum => (12, 12, "slow"),
    };
    match encoder {
        VideoEncoder::H264Nvenc => vec![
            "-c:v".into(),
            "h264_nvenc".into(),
            "-preset".into(),
            if quality == RenderQuality::Standard {
                "p4".into()
            } else {
                "p7".into()
            },
            "-rc".into(),
            "vbr".into(),
            "-cq".into(),
            cq.to_string(),
            "-b:v".into(),
            "0".into(),
        ],
        VideoEncoder::Libx264 => vec![
            "-c:v".into(),
            "libx264".into(),
            "-preset".into(),
            preset.into(),
            "-crf".into(),
            crf.to_string(),
        ],
    }
}

// ---------------------------------------------------------------------------
// FFmpeg command — engine.render(), simple colour-background path
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum RenderError {
    #[error("Video sau render bị thiếu hoặc quá nhỏ để phát.")]
    MissingOutput,
    #[error("Không tìm thấy ffprobe: {0}")]
    FfprobeNotFound(String),
    #[error("Không đọc được thông tin video sau render: {0}")]
    ProbeFailed(String),
    #[error("Video sau render phải có cả luồng hình và âm thanh.")]
    MissingStreams,
    #[error("Thời lượng sau render lệch nguồn: {0:.2}s / {1:.2}s.")]
    DurationMismatch(f64, f64),
    #[error("Kích thước video sau render là {0}×{1}, cần {2}×{3}.")]
    SizeMismatch(u32, u32, u32, u32),
    #[error("Cần bản hát tham chiếu hoặc vocal đã tách để tạo bản hướng dẫn.")]
    MissingGuideAudio,
    #[error("Màu nền không hợp lệ: {0}")]
    InvalidBackgroundColor(String),
    #[error("Đường dẫn nền không hợp lệ trong project: {0}")]
    InvalidBackgroundPath(String),
    #[error("Chưa có video nền hợp lệ trong project.")]
    MissingBackgroundVideo,
    #[error("Chưa có ảnh nền hợp lệ trong project.")]
    MissingBackgroundImage,
    #[error("Thời lượng render không hợp lệ: {0}")]
    InvalidDuration(f64),
    #[error("Lỗi đọc/ghi file: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON không hợp lệ: {0}")]
    Json(#[from] serde_json::Error),
}

/// A preview render range, as `engine.render()` reads
/// `project["render_preview"]`: `start` is clamped to
/// `0..=duration - 0.5` and `duration` to `0.5..=12` seconds.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PreviewRange {
    pub start: f64,
    pub duration: f64,
}

/// Optional visual background input. Audio is always sourced from the beat
/// (and vocals for guide output), matching `engine.render()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderBackground {
    Image(PathBuf),
    Video(PathBuf),
}

#[derive(Debug, Clone, PartialEq)]
pub struct RenderScene {
    pub image: PathBuf,
    pub start: f64,
    pub end: f64,
    pub opacity: f64,
    pub fade_seconds: f64,
}

/// Inputs for `ffmpeg_command`. Paths are used verbatim in the argv,
/// as in Python (which passes `str(directory / "beat.wav")` and the
/// relative `lyrics.ass` / `fonts` names, with the render run from
/// the export directory as its working directory).
#[derive(Debug, Clone, PartialEq)]
pub struct RenderInputs {
    pub beat: PathBuf,
    pub background: Option<RenderBackground>,
    pub scenes: Vec<RenderScene>,
    /// Guide vocal file; required only when
    /// `profile.audio_output == AudioOutput::Guide`.
    pub vocals: Option<PathBuf>,
    pub ass_file: PathBuf,
    pub fonts_dir: PathBuf,
    /// Beat duration in seconds; also the full-render `-t` value.
    pub duration: f64,
    pub preview: Option<PreviewRange>,
    pub ffmpeg: PathBuf,
    pub encoder: VideoEncoder,
}

impl RenderInputs {
    pub fn new(beat: impl Into<PathBuf>, duration: f64) -> Self {
        Self {
            beat: beat.into(),
            background: None,
            scenes: Vec::new(),
            vocals: None,
            ass_file: PathBuf::from("lyrics.ass"),
            fonts_dir: PathBuf::from("fonts"),
            duration,
            preview: None,
            ffmpeg: PathBuf::from("ffmpeg"),
            encoder: VideoEncoder::Libx264,
        }
    }

    /// Resolve the legacy Python background settings and project media names
    /// into project-local render inputs. All filenames must be basenames.
    pub fn configure_background_from_project(
        &mut self,
        project: &vietkara_domain::Project,
        directory: &Path,
    ) -> Result<(), RenderError> {
        let kind = project
            .settings
            .extra
            .get("background_type")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("auto");
        let local_file = |name: &str| -> Result<Option<PathBuf>, RenderError> {
            let path = PathBuf::from(name);
            if path.file_name().and_then(|value| value.to_str()) != Some(name) {
                return Err(RenderError::InvalidBackgroundPath(name.to_owned()));
            }
            let path = directory.join(path);
            Ok(path.is_file().then_some(path))
        };
        self.background = match kind {
            "video" => {
                let name = project
                    .extra
                    .get("background_video")
                    .and_then(|video| video.get("source_file"))
                    .and_then(serde_json::Value::as_str)
                    .ok_or(RenderError::MissingBackgroundVideo)?;
                Some(RenderBackground::Video(
                    local_file(name)?.ok_or(RenderError::MissingBackgroundVideo)?,
                ))
            }
            "original" => {
                let mut found = None;
                for media in [&project.reference, &project.beat].into_iter().flatten() {
                    if let Some(name) = media
                        .get("video")
                        .and_then(|video| video.get("source_file"))
                        .and_then(serde_json::Value::as_str)
                        && let Some(path) = local_file(name)?
                    {
                        found = Some(RenderBackground::Video(path));
                        break;
                    }
                }
                Some(found.ok_or(RenderError::MissingBackgroundVideo)?)
            }
            "image" | "auto" => {
                let enabled = kind == "image"
                    || project
                        .settings
                        .extra
                        .get("use_background")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(true);
                if enabled {
                    let path = directory.join("background.jpg");
                    if path.is_file() {
                        Some(RenderBackground::Image(path))
                    } else if kind == "image" {
                        return Err(RenderError::MissingBackgroundImage);
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            _ => None,
        };
        Ok(())
    }

    /// Resolve section-tagged scene stills onto the beat timeline, matching
    /// `scene_tools.timeline` for the image layer used by the renderer.
    pub fn configure_scenes_from_project(
        &mut self,
        project: &vietkara_domain::Project,
        directory: &Path,
    ) {
        use std::collections::{BTreeMap, HashMap};
        let valid_sections = [
            "intro",
            "verse",
            "pre-chorus",
            "chorus",
            "bridge",
            "outro",
            "instrumental",
        ];
        let scenes = project
            .scenes
            .iter()
            .filter_map(|scene| {
                let section = scene.get("section_name")?.as_str()?.trim();
                if !valid_sections.contains(&section) {
                    return None;
                }
                let id = scene.get("id")?.as_str()?;
                if !vietkara_domain::is_hex32(id) {
                    return None;
                }
                let image = directory.join(format!("scene_{id}.png"));
                if !image.is_file() {
                    return None;
                }
                let number = |key: &str, default: f64| {
                    scene
                        .get(key)
                        .and_then(serde_json::Value::as_f64)
                        .filter(|value| value.is_finite())
                        .unwrap_or(default)
                };
                let transition = scene
                    .get("transition")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("fade");
                Some((
                    section.to_owned(),
                    image,
                    number("opacity", 100.0).clamp(0.0, 100.0) / 100.0,
                    if transition == "fade" {
                        number("transition_seconds", 0.6).clamp(0.0, 3.0)
                    } else {
                        0.0
                    },
                ))
            })
            .map(|(section, image, opacity, transition)| (section, (image, opacity, transition)))
            .collect::<HashMap<_, _>>();
        if scenes.is_empty() || project.words.is_empty() {
            self.scenes.clear();
            return;
        }
        let mut lines: BTreeMap<u32, (usize, f64, f64, String)> = BTreeMap::new();
        for (index, word) in project.words.iter().enumerate() {
            let start = vietkara_domain::timing::map_time(word.start, &project.anchors, false);
            let end = vietkara_domain::timing::map_time(word.end, &project.anchors, false);
            let entry = lines
                .entry(word.line)
                .or_insert((index, start, end, String::new()));
            entry.0 = entry.0.min(index);
            entry.1 = entry.1.min(start);
            entry.2 = entry.2.max(end);
            if entry.3.is_empty() {
                entry.3 = word.section_name.as_deref().unwrap_or("").trim().to_owned();
            }
        }
        let mut ordered = lines.into_values().collect::<Vec<_>>();
        ordered.sort_by(|left, right| left.1.total_cmp(&right.1).then(left.0.cmp(&right.0)));
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
        let duration = self.duration.max(0.0);
        let mut resolved = Vec::new();
        for (index, (section, run_start, _)) in runs.iter().enumerate() {
            let Some((image, opacity, base_transition)) = scenes.get(section) else {
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
            let start = if previous_has_scene {
                run_start - transition / 2.0
            } else {
                *run_start
            };
            let end = if following_has_scene {
                runs[next_index].1 + transition / 2.0
            } else if next_index < runs.len() {
                runs[next_index].1
            } else {
                duration
            };
            let start = start.max(0.0);
            let end = end.min(duration);
            if end - start >= 0.05 {
                resolved.push(RenderScene {
                    image: image.clone(),
                    start,
                    end,
                    opacity: *opacity,
                    fade_seconds: transition.min((end - start) / 2.0).max(0.05),
                });
            }
        }
        self.scenes = resolved;
    }
}

/// Build the render command from
/// `engine.render()`.
///
/// Layout of the argv (beat-only):
/// `ffmpeg -y -v error -progress pipe:1 -nostats -f lavfi -i
/// color=c=0x<bg>:s=WxH:r=<fps> -i <beat.wav> -vf <visual filter>
/// -map 0:v:0 -map 1:a:0 <encoding options> -pix_fmt yuv420p
/// -c:a aac -b:a 320k [-ss <start>] -t <duration>
/// [-movflags +faststart] karaoke.<ext>`
///
/// With `audio_output == Guide` the visual filter goes into
/// `-filter_complex` with an `amix` of the beat and the vocal
/// (`[1:a:0][2:a:0]amix=inputs=2:duration=first:
/// dropout_transition=0:normalize=1[audio]`) and the maps become
/// `[base]` / `[audio]`. This is a pure builder — it never spawns
/// a process.
pub fn ffmpeg_command(
    profile: &RenderProfile,
    inputs: &RenderInputs,
) -> Result<Vec<String>, RenderError> {
    if !is_hex_color(&profile.background_color) {
        return Err(RenderError::InvalidBackgroundColor(
            profile.background_color.clone(),
        ));
    }
    if !inputs.duration.is_finite() || inputs.duration <= 0.0 {
        return Err(RenderError::InvalidDuration(inputs.duration));
    }
    let (render_start, render_duration) = match inputs.preview {
        Some(preview) => {
            let start = preview.start.max(0.0).min((inputs.duration - 0.5).max(0.0));
            let duration = preview
                .duration
                .min(12.0)
                .min(inputs.duration - start)
                .max(0.5);
            (start, duration)
        }
        None => (0.0, inputs.duration),
    };
    let fps = py_float(profile.fps_value());
    let (width, height) = profile.frame_size();
    let bg = &profile.background_color[1..];
    let visual_filter = format!(
        "setpts=PTS-STARTPTS,fps={fps},scale={width}:{height}:force_original_aspect_ratio=increase:flags=lanczos,crop={width}:{height},setsar=1,ass={}:fontsdir={}",
        inputs.ass_file.display(),
        inputs.fonts_dir.display(),
    );
    let mut args: Vec<String> = vec![
        inputs.ffmpeg.display().to_string(),
        "-y".into(),
        "-v".into(),
        "error".into(),
        "-progress".into(),
        "pipe:1".into(),
        "-nostats".into(),
    ];
    match &inputs.background {
        Some(RenderBackground::Image(path)) => args.extend([
            "-loop".into(),
            "1".into(),
            "-framerate".into(),
            fps.clone(),
            "-i".into(),
            path.display().to_string(),
        ]),
        Some(RenderBackground::Video(path)) => args.extend([
            "-stream_loop".into(),
            "-1".into(),
            "-i".into(),
            path.display().to_string(),
        ]),
        None => args.extend([
            "-f".into(),
            "lavfi".into(),
            "-i".into(),
            format!("color=c=0x{bg}:s={width}x{height}:r={fps}"),
        ]),
    }
    let background_input = 0usize;
    let beat_input = 1usize;
    args.extend(["-i".into(), inputs.beat.display().to_string()]);
    let guide = profile.audio_output == AudioOutput::Guide;
    let vocals_input = beat_input + 1;
    if guide {
        let vocals = inputs
            .vocals
            .as_ref()
            .ok_or(RenderError::MissingGuideAudio)?;
        args.push("-i".into());
        args.push(vocals.display().to_string());
    }
    let mut scene_inputs: Vec<(usize, &RenderScene)> = Vec::new();
    let mut next_input = beat_input + 1 + usize::from(guide);
    for scene in &inputs.scenes {
        if !scene.start.is_finite()
            || !scene.end.is_finite()
            || scene.end <= scene.start
            || !scene.opacity.is_finite()
            || !scene.fade_seconds.is_finite()
        {
            continue;
        }
        args.extend([
            "-loop".into(),
            "1".into(),
            "-framerate".into(),
            fps.clone(),
            "-i".into(),
            scene.image.display().to_string(),
        ]);
        scene_inputs.push((next_input, scene));
        next_input += 1;
    }
    if guide || !scene_inputs.is_empty() {
        let mut graph = format!("[{background_input}:v:0]{visual_filter}[scene_base]");
        let mut last_background = "scene_base".to_owned();
        for (index, (input_index, scene)) in scene_inputs.iter().enumerate() {
            let start = scene.start.max(0.0);
            let end = scene.end.min(inputs.duration);
            if end <= start {
                continue;
            }
            let fade = scene.fade_seconds.min((end - start) / 2.0).max(0.0);
            let opacity = scene.opacity.clamp(0.0, 1.0);
            let mut layer = format!(
                "[{input_index}:v:0]setpts=PTS-STARTPTS,fps={fps},scale={width}:{height}:force_original_aspect_ratio=increase:flags=lanczos,crop={width}:{height},setsar=1,format=rgba,colorchannelmixer=aa={opacity:.4}"
            );
            if fade >= 0.05 {
                layer.push_str(&format!(",fade=t=in:st=0:d={fade:.3}:alpha=1"));
                layer.push_str(&format!(
                    ",fade=t=out:st={:.3}:d={fade:.3}:alpha=1",
                    (end - start - fade).max(0.0)
                ));
            }
            layer.push_str(&format!(
                ",trim=duration={:.3},setpts=PTS-STARTPTS+{start:.3}/TB[scene_layer{index}]",
                end - start
            ));
            graph.push(';');
            graph.push_str(&layer);
            graph.push_str(&format!(
                ";[{last_background}][scene_layer{index}]overlay=x=0:y=0:eof_action=pass:shortest=0:enable='between(t,{start:.3},{end:.3})'[scene_bg{index}]"
            ));
            last_background = format!("scene_bg{index}");
        }
        graph.push_str(&format!(
            ";[{last_background}]ass={}:fontsdir={}[base]",
            inputs.ass_file.display(),
            inputs.fonts_dir.display()
        ));
        if guide {
            graph.push_str(&format!(
                ";[{beat_input}:a:0][{vocals_input}:a:0]amix=inputs=2:duration=first:dropout_transition=0:normalize=1[audio]"
            ));
        }
        args.push("-filter_complex".into());
        args.push(graph);
        args.push("-map".into());
        args.push("[base]".into());
        args.push("-map".into());
        args.push(if guide {
            "[audio]".into()
        } else {
            format!("{beat_input}:a:0")
        });
    } else {
        args.push("-vf".into());
        args.push(visual_filter);
        args.push("-map".into());
        args.push(format!("{background_input}:v:0"));
        args.push("-map".into());
        args.push(format!("{beat_input}:a:0"));
    }
    args.extend(encoding_options(profile.quality, inputs.encoder));
    args.extend([
        "-pix_fmt".into(),
        "yuv420p".into(),
        "-c:a".into(),
        "aac".into(),
        "-b:a".into(),
        "320k".into(),
    ]);
    if render_start > 0.0 {
        args.push("-ss".into());
        args.push(py_float(render_start));
    }
    args.push("-t".into());
    args.push(py_float(render_duration));
    if matches!(profile.format, RenderFormat::Mp4 | RenderFormat::Mov) {
        args.push("-movflags".into());
        args.push("+faststart".into());
    }
    args.push(profile.output_name());
    Ok(args)
}

// ---------------------------------------------------------------------------
// Output verification — render_info.verify_output
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerifiedOutput {
    pub duration: f64,
    pub width: u32,
    pub height: u32,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
}

fn json_f64(value: Option<&serde_json::Value>) -> f64 {
    match value {
        Some(serde_json::Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        Some(serde_json::Value::String(s)) => s.parse().unwrap_or(0.0),
        _ => 0.0,
    }
}

/// `render_info.verify_output` with the `ffprobe` binary taken from
/// `PATH` (or `OBS_FFPROBE`, as `engine.binary` does for packaged
/// binaries). The file must exist and be at least 1024 bytes; the
/// probe must report a video and an audio stream; the duration must
/// be within `max(1.0, duration * 0.01)` of the expected value; and
/// the frame size must match exactly.
pub fn verify_output(
    path: &Path,
    duration: f64,
    width: u32,
    height: u32,
) -> Result<VerifiedOutput, RenderError> {
    let ffprobe = std::env::var_os("OBS_FFPROBE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("ffprobe"));
    verify_output_with_ffprobe(path, duration, width, height, &ffprobe)
}

pub fn verify_output_with_ffprobe(
    path: &Path,
    duration: f64,
    width: u32,
    height: u32,
    ffprobe: &Path,
) -> Result<VerifiedOutput, RenderError> {
    let metadata = std::fs::metadata(path).map_err(|_| RenderError::MissingOutput)?;
    if !metadata.is_file() || metadata.len() < 1024 {
        return Err(RenderError::MissingOutput);
    }
    let output = std::process::Command::new(ffprobe)
        .args([
            "-v",
            "error",
            "-show_streams",
            "-show_format",
            "-of",
            "json",
        ])
        .arg(path)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| RenderError::FfprobeNotFound(e.to_string()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let tail: String = stderr
            .chars()
            .rev()
            .take(700)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        return Err(RenderError::ProbeFailed(tail));
    }
    let data: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let streams = data
        .get("streams")
        .and_then(|s| s.as_array())
        .cloned()
        .unwrap_or_default();
    let video = streams
        .iter()
        .find(|s| s.get("codec_type").and_then(|c| c.as_str()) == Some("video"));
    let audio = streams
        .iter()
        .find(|s| s.get("codec_type").and_then(|c| c.as_str()) == Some("audio"));
    let (Some(video), Some(audio)) = (video, audio) else {
        return Err(RenderError::MissingStreams);
    };
    let output_duration = json_f64(data.get("format").and_then(|f| f.get("duration")));
    if output_duration != 0.0 && (output_duration - duration).abs() > f64::max(1.0, duration * 0.01)
    {
        return Err(RenderError::DurationMismatch(output_duration, duration));
    }
    let actual_width = video.get("width").and_then(|w| w.as_u64()).unwrap_or(0) as u32;
    let actual_height = video.get("height").and_then(|h| h.as_u64()).unwrap_or(0) as u32;
    if (actual_width, actual_height) != (width, height) {
        return Err(RenderError::SizeMismatch(
            actual_width,
            actual_height,
            width,
            height,
        ));
    }
    Ok(VerifiedOutput {
        duration: py_round(output_duration * 1000.0) / 1000.0,
        width: actual_width,
        height: actual_height,
        video_codec: video
            .get("codec_name")
            .and_then(|c| c.as_str())
            .map(str::to_owned),
        audio_codec: audio
            .get("codec_name")
            .and_then(|c| c.as_str())
            .map(str::to_owned),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row() -> RowAnim {
        RowAnim {
            show_start: 1.0,
            show_end: 3.0,
            x: 100.0,
            y: 200.0,
            font_size: 72.0,
            align: TextAlign::Center,
            section_start: false,
            section_end: false,
        }
    }

    fn opts(mode: &str) -> AnimOpts {
        AnimOpts {
            line_animation: mode.to_owned(),
            horizontal_scale: 100.0,
        }
    }

    // Golden values computed by running the Python functions in the
    // repo (`python3 -c "from subtitles import ..."`) — see the
    // crate docs. The `horizontal_scale` golden values use 100.0,
    // the float `validate_settings` produces (Python `str(100.0)`
    // is `"100.0"`, not `"100"`).

    #[test]
    fn stamp_matches_python() {
        assert_eq!(stamp(0.0), "0:00:00.00");
        assert_eq!(stamp(61.234), "0:01:01.23");
        assert_eq!(stamp(3661.999), "1:01:02.00");
        assert_eq!(stamp(-5.0), "0:00:00.00");
        assert_eq!(stamp(359999.99), "99:59:59.99");
        // Python round() is half-to-even: round(0.5) == 0.
        assert_eq!(stamp(0.005), "0:00:00.00");
        assert_eq!(stamp(59.995), "0:01:00.00");
    }

    #[test]
    fn ass_color_matches_python() {
        assert_eq!(ass_color("#52efb9", 100), "&H00b9ef52");
        assert_eq!(ass_color("#52efb9", 0), "&HFFb9ef52");
        assert_eq!(ass_color("#ff0000", 50), "&H800000ff");
        assert_eq!(ass_color("#101d2b", 100), "&H002b1d10");
        assert_eq!(ass_color("#aabbcc", 33), "&HABccbbaa");
        // Opacity is clamped to 0..=100, as in Python.
        assert_eq!(ass_color("#aabbcc", 255), "&H00ccbbaa");
    }

    #[test]
    fn literal_matches_python() {
        assert_eq!(literal("a\\b{c}d\ne"), "a＼b｛c｝d e");
        assert_eq!(literal("Xin chào"), "Xin chào");
    }

    #[test]
    fn blend_matches_python() {
        assert_eq!(blend("#000000", "#ffffff", 0.5), "#808080");
        assert_eq!(blend("#ff0000", "#0000ff", 0.25), "#bf0040");
        assert_eq!(blend("#123456", "#abcdef", 2.0), "#abcdef");
        assert_eq!(blend("#123456", "#abcdef", -1.0), "#123456");
        assert_eq!(blend("#000000", "#ffffff", 0.333), "#555555");
        assert_eq!(blend("#ffffff", "#000000", 0.333), "#aaaaaa");
    }

    #[test]
    fn animation_matches_python() {
        assert_eq!(
            animation(&row(), &opts("none")),
            "\\an7\\pos(100.00,200.00)"
        );
        assert_eq!(
            animation(&row(), &opts("slowfade")),
            "\\an7\\pos(100.00,200.00)\\fad(600,600)"
        );
        assert_eq!(
            animation(&row(), &opts("fadesec")),
            "\\an7\\pos(100.00,200.00)\\fad(0,0)"
        );
        let section = RowAnim {
            section_start: true,
            section_end: true,
            ..row()
        };
        assert_eq!(
            animation(&section, &opts("fadesec")),
            "\\an7\\pos(100.00,200.00)\\fad(600,600)"
        );
        assert_eq!(
            animation(&row(), &opts("zoom")),
            "\\an7\\pos(100.00,200.00)\\fscx65.00\\fscy65\\t(0,300,\\fscx100.00\\fscy100)\\t(300,600,\\fscx100.0\\fscy100)\\t(1400,2000,\\fscx65.00\\fscy65)\\fad(300,300)"
        );
        assert_eq!(
            animation(&row(), &opts("zoomout")),
            "\\an7\\pos(100.00,200.00)\\fscx135.00\\fscy135\\t(0,300,\\fscx100.00\\fscy100)\\t(300,600,\\fscx100.0\\fscy100)\\t(1400,2000,\\fscx135.00\\fscy135)\\fad(300,300)"
        );
        assert_eq!(
            animation(&row(), &opts("popbounce")),
            "\\an7\\pos(100.00,200.00)\\fscx70.00\\fscy70\\t(0,300,\\fscx112.00\\fscy112)\\t(300,600,\\fscx100.0\\fscy100)\\t(1400,2000,\\fscx70.00\\fscy70)\\fad(300,300)"
        );
        assert_eq!(
            animation(&row(), &opts("punchzoom")),
            "\\an7\\pos(100.00,200.00)\\fscx135.00\\fscy135\\t(0,300,\\fscx100.00\\fscy100)\\t(300,600,\\fscx100.0\\fscy100)\\t(1400,2000,\\fscx135.00\\fscy135)\\fad(300,300)"
        );
        assert_eq!(
            animation(&row(), &opts("flipup")),
            "\\an7\\pos(100.00,200.00)\\frx70\\t(0,600,\\frx0)\\t(1400,2000,\\frx70)\\fad(600,600)"
        );
        assert_eq!(
            animation(&row(), &opts("split")),
            "\\an7\\move(172.00,200.00,100.00,200.00,0,600)\\fad(600,600)"
        );
        let right = RowAnim {
            align: TextAlign::Right,
            ..row()
        };
        assert_eq!(
            animation(&right, &opts("split")),
            "\\an7\\move(28.00,200.00,100.00,200.00,0,600)\\fad(600,600)"
        );
        assert_eq!(
            animation(&row(), &opts("slidedown")),
            "\\an7\\move(100.00,128.00,100.00,200.00,0,600)\\fad(600,600)"
        );
        assert_eq!(
            animation(&row(), &opts("slideup")),
            "\\an7\\move(100.00,272.00,100.00,200.00,0,600)\\fad(600,600)"
        );
        // Unknown modes fall through to the move branch, as in Python.
        assert_eq!(
            animation(&row(), &opts("unknown")),
            "\\an7\\move(100.00,200.00,100.00,200.00,0,600)\\fad(600,600)"
        );
        // Horizontal scale is a float: 80.0 prints as "80.0".
        assert_eq!(
            animation(
                &row(),
                &AnimOpts {
                    line_animation: "zoom".into(),
                    horizontal_scale: 80.0
                }
            ),
            "\\an7\\pos(100.00,200.00)\\fscx52.00\\fscy65\\t(0,300,\\fscx80.00\\fscy100)\\t(300,600,\\fscx80.0\\fscy100)\\t(1400,2000,\\fscx52.00\\fscy65)\\fad(300,300)"
        );
    }

    #[test]
    fn animation_short_duration() {
        let short = RowAnim {
            show_start: 0.0,
            show_end: 0.3,
            x: 0.0,
            y: 0.0,
            font_size: 50.0,
            ..RowAnim::default()
        };
        assert_eq!(
            animation(&short, &opts("zoom")),
            "\\an7\\pos(0.00,0.00)\\fscx65.00\\fscy65\\t(0,50,\\fscx100.00\\fscy100)\\t(50,100,\\fscx100.0\\fscy100)\\t(200,300,\\fscx65.00\\fscy65)\\fad(50,50)"
        );
        assert_eq!(
            animation(&short, &opts("flipup")),
            "\\an7\\pos(0.00,0.00)\\frx70\\t(0,100,\\frx0)\\t(200,300,\\frx70)\\fad(100,100)"
        );
    }

    #[test]
    fn rounded_box_matches_python() {
        assert_eq!(
            rounded_box(10.0, 20.0, 100.0, 50.0, 12.0),
            "m 22.00 20.00 l 98.00 20.00 b 104.63 20.00 110.00 25.37 110.00 32.00 l 110.00 58.00 b 110.00 64.63 104.63 70.00 98.00 70.00 l 22.00 70.00 b 15.37 70.00 10.00 64.63 10.00 58.00 l 10.00 32.00 b 10.00 25.37 15.37 20.00 22.00 20.00"
        );
        assert_eq!(
            rounded_box(0.0, 0.0, 10.0, 10.0, 99.0),
            "m 5.00 0.00 l 5.00 0.00 b 7.76 0.00 10.00 2.24 10.00 5.00 l 10.00 5.00 b 10.00 7.76 7.76 10.00 5.00 10.00 l 5.00 10.00 b 2.24 10.00 0.00 7.76 0.00 5.00 l 0.00 5.00 b 0.00 2.24 2.24 0.00 5.00 0.00"
        );
    }

    #[test]
    fn icons_are_present() {
        assert!(icon_shape("mic").is_some());
        assert!(icon_shape("moon").is_some());
        assert!(icon_shape("none").is_none());
    }

    #[test]
    fn fps_value_matches_python() {
        assert_eq!(fps_value("30"), 30.0);
        assert_eq!(fps_value("60"), 60.0);
        assert_eq!(fps_value("29.97"), 29.97);
        assert!((fps_value("30000/1001") - 29.970_029_970_029_97).abs() < 1e-9);
        assert_eq!(fps_value("0"), 30.0);
        assert_eq!(fps_value("abc"), 30.0);
        assert_eq!(fps_value("121"), 30.0);
        assert_eq!(fps_value("120"), 120.0);
    }

    #[test]
    fn encoding_options_match_python() {
        assert_eq!(
            encoding_options(RenderQuality::Standard, VideoEncoder::Libx264),
            vec!["-c:v", "libx264", "-preset", "veryfast", "-crf", "20"]
        );
        assert_eq!(
            encoding_options(RenderQuality::High, VideoEncoder::Libx264),
            vec!["-c:v", "libx264", "-preset", "medium", "-crf", "16"]
        );
        assert_eq!(
            encoding_options(RenderQuality::Maximum, VideoEncoder::Libx264),
            vec!["-c:v", "libx264", "-preset", "slow", "-crf", "12"]
        );
        assert_eq!(
            encoding_options(RenderQuality::Standard, VideoEncoder::H264Nvenc),
            vec![
                "-c:v",
                "h264_nvenc",
                "-preset",
                "p4",
                "-rc",
                "vbr",
                "-cq",
                "20",
                "-b:v",
                "0"
            ]
        );
        assert_eq!(
            encoding_options(RenderQuality::High, VideoEncoder::H264Nvenc),
            vec![
                "-c:v",
                "h264_nvenc",
                "-preset",
                "p7",
                "-rc",
                "vbr",
                "-cq",
                "16",
                "-b:v",
                "0"
            ]
        );
        assert_eq!(
            encoding_options(RenderQuality::Maximum, VideoEncoder::H264Nvenc),
            vec![
                "-c:v",
                "h264_nvenc",
                "-preset",
                "p7",
                "-rc",
                "vbr",
                "-cq",
                "12",
                "-b:v",
                "0"
            ]
        );
    }

    #[test]
    fn frame_size_matches_describe() {
        let mut profile = RenderProfile::default();
        assert_eq!(profile.frame_size(), (1920, 1080));
        profile.aspect = Aspect::Portrait;
        assert_eq!(profile.frame_size(), (1080, 1920));
        profile.aspect = Aspect::Landscape;
        profile.resolution = Resolution::K4;
        assert_eq!(profile.frame_size(), (3840, 2160));
        profile.aspect = Aspect::Portrait;
        assert_eq!(profile.frame_size(), (2160, 3840));
        profile.frame_size_override = Some((320, 240));
        assert_eq!(profile.frame_size(), (320, 240));
        assert_eq!(RenderProfile::default().fps_value(), 30.0);
        assert_eq!(
            RenderProfile {
                fps: Fps::Fixed(60.0),
                ..RenderProfile::default()
            }
            .fps_value(),
            60.0
        );
        assert_eq!(
            RenderProfile {
                fps: Fps::Fixed(999.0),
                ..RenderProfile::default()
            }
            .fps_value(),
            30.0
        );
    }

    #[test]
    fn profile_from_project_settings() {
        let mut project = vietkara_domain::Project::new("Bài test");
        project.settings = serde_json::json!({
            "aspect": "portrait",
            "resolution": "4k",
            "render_fps": "60",
            "render_quality": "maximum",
            "render_format": "mkv",
            "audio_output": "guide",
            "background": "#AABBCC",
        })
        .try_into()
        .unwrap();
        let profile = RenderProfile::from_project(&project);
        assert_eq!(profile.aspect, Aspect::Portrait);
        assert_eq!(profile.resolution, Resolution::K4);
        assert_eq!(profile.frame_size(), (2160, 3840));
        assert_eq!(profile.fps, Fps::Fixed(60.0));
        assert_eq!(profile.quality, RenderQuality::Maximum);
        assert_eq!(profile.format, RenderFormat::Mkv);
        assert_eq!(profile.audio_output, AudioOutput::Guide);
        assert_eq!(profile.background_color, "#aabbcc");
        assert_eq!(profile.output_name(), "karaoke.mkv");
    }

    #[test]
    fn project_background_resolves_image_video_and_original_sources() {
        let directory = tempfile::tempdir().unwrap();
        let image = directory.path().join("background.jpg");
        let video = directory.path().join("background_source.mp4");
        let original = directory.path().join("reference_source.mov");
        std::fs::write(&image, b"jpg").unwrap();
        std::fs::write(&video, b"mp4").unwrap();
        std::fs::write(&original, b"mov").unwrap();

        let mut project = vietkara_domain::Project::new("image");
        project
            .settings
            .extra
            .insert("background_type".into(), serde_json::json!("image"));
        let mut inputs = RenderInputs::new("beat.wav", 8.0);
        inputs
            .configure_background_from_project(&project, directory.path())
            .unwrap();
        assert_eq!(inputs.background, Some(RenderBackground::Image(image)));

        project
            .settings
            .extra
            .insert("background_type".into(), serde_json::json!("video"));
        project.extra.insert(
            "background_video".into(),
            serde_json::json!({"source_file":"background_source.mp4"}),
        );
        inputs
            .configure_background_from_project(&project, directory.path())
            .unwrap();
        assert_eq!(inputs.background, Some(RenderBackground::Video(video)));

        project
            .settings
            .extra
            .insert("background_type".into(), serde_json::json!("original"));
        project.reference =
            Some(serde_json::json!({"video":{"source_file":"reference_source.mov"}}));
        inputs
            .configure_background_from_project(&project, directory.path())
            .unwrap();
        assert_eq!(inputs.background, Some(RenderBackground::Video(original)));
    }

    #[test]
    fn project_background_rejects_traversal_and_missing_required_assets() {
        let directory = tempfile::tempdir().unwrap();
        let mut project = vietkara_domain::Project::new("unsafe");
        project
            .settings
            .extra
            .insert("background_type".into(), serde_json::json!("video"));
        project.extra.insert(
            "background_video".into(),
            serde_json::json!({"source_file":"..\\outside.mp4"}),
        );
        let mut inputs = RenderInputs::new("beat.wav", 8.0);
        assert!(matches!(
            inputs.configure_background_from_project(&project, directory.path()),
            Err(RenderError::InvalidBackgroundPath(_))
        ));
        project.extra.insert(
            "background_video".into(),
            serde_json::json!({"source_file":"missing.mp4"}),
        );
        assert!(matches!(
            inputs.configure_background_from_project(&project, directory.path()),
            Err(RenderError::MissingBackgroundVideo)
        ));
        project
            .settings
            .extra
            .insert("background_type".into(), serde_json::json!("image"));
        assert!(matches!(
            inputs.configure_background_from_project(&project, directory.path()),
            Err(RenderError::MissingBackgroundImage)
        ));
    }

    #[test]
    fn project_scene_resolves_section_to_beat_interval_and_filter_graph() {
        let directory = tempfile::tempdir().unwrap();
        let scene_id = "0123456789abcdef0123456789abcdef";
        let image = directory.path().join(format!("scene_{scene_id}.png"));
        std::fs::write(&image, b"png").unwrap();
        let mut project = vietkara_domain::Project::new("scene");
        project.scenes.push(serde_json::json!({
            "id": scene_id,
            "section_name": "chorus",
            "opacity": 75,
            "transition": "fade",
            "transition_seconds": 0.8
        }));
        project.words.push(
            serde_json::from_value(serde_json::json!({
                "id": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "line_uid": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "text": "Chorus",
                "start": 2.0,
                "end": 3.0,
                "line": 0,
                "role": "all",
                "section_name": "chorus"
            }))
            .unwrap(),
        );
        let mut inputs = RenderInputs::new("beat.wav", 10.0);
        inputs.configure_scenes_from_project(&project, directory.path());
        assert_eq!(inputs.scenes.len(), 1);
        assert_eq!(inputs.scenes[0].image, image);
        assert_eq!(inputs.scenes[0].start, 2.0);
        assert_eq!(inputs.scenes[0].end, 10.0);
        assert_eq!(inputs.scenes[0].opacity, 0.75);

        let command = ffmpeg_command(&RenderProfile::default(), &inputs).unwrap();
        let graph_index = command
            .iter()
            .position(|arg| arg == "-filter_complex")
            .unwrap();
        let graph = &command[graph_index + 1];
        assert!(graph.contains("[2:v:0]"));
        assert!(graph.contains("colorchannelmixer=aa=0.7500"));
        assert!(graph.contains("overlay=x=0:y=0"));
        assert!(graph.contains("ass=lyrics.ass:fontsdir=fonts[base]"));
        assert_eq!(
            &command[graph_index + 2..graph_index + 6],
            ["-map", "[base]", "-map", "1:a:0"]
        );
    }

    #[test]
    fn ffmpeg_command_beat_only_matches_engine() {
        let profile = RenderProfile {
            fps: Fps::Fixed(30.0),
            ..RenderProfile::default()
        };
        let inputs = RenderInputs::new("beat.wav", 8.0);
        let argv = ffmpeg_command(&profile, &inputs).unwrap();
        let expected: Vec<String> = vec![
            "ffmpeg", "-y", "-v", "error", "-progress", "pipe:1", "-nostats",
            "-f", "lavfi", "-i",
            "color=c=0x101d2b:s=1920x1080:r=30.0",
            "-i", "beat.wav",
            "-vf",
            "setpts=PTS-STARTPTS,fps=30.0,scale=1920:1080:force_original_aspect_ratio=increase:flags=lanczos,crop=1920:1080,setsar=1,ass=lyrics.ass:fontsdir=fonts",
            "-map", "0:v:0", "-map", "1:a:0",
            "-c:v", "libx264", "-preset", "medium", "-crf", "16",
            "-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "320k",
            "-t", "8.0", "-movflags", "+faststart", "karaoke.mp4",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        assert_eq!(argv, expected);
    }

    #[test]
    fn ffmpeg_command_guide_uses_amix() {
        let profile = RenderProfile {
            audio_output: AudioOutput::Guide,
            fps: Fps::Fixed(30.0),
            ..RenderProfile::default()
        };
        let mut inputs = RenderInputs::new("beat.wav", 8.0);
        inputs.vocals = Some(PathBuf::from("vocals.wav"));
        let argv = ffmpeg_command(&profile, &inputs).unwrap();
        // Input order: colour, beat, vocals.
        assert_eq!(&argv[11..15], ["-i", "beat.wav", "-i", "vocals.wav"]);
        let complex = argv
            .iter()
            .position(|a| a == "-filter_complex")
            .expect("filter_complex");
        assert!(argv[complex + 1].contains(
            "[1:a:0][2:a:0]amix=inputs=2:duration=first:dropout_transition=0:normalize=1[audio]"
        ));
        assert!(!argv.iter().any(|a| a == "-vf"));
        assert_eq!(
            &argv[complex + 2..complex + 6],
            ["-map", "[base]", "-map", "[audio]"]
        );
        assert_eq!(argv.last().unwrap(), "karaoke.mp4");
    }

    #[test]
    fn ffmpeg_command_video_background_loops_video_and_keeps_beat_audio() {
        let profile = RenderProfile {
            fps: Fps::Fixed(30.0),
            ..RenderProfile::default()
        };
        let mut inputs = RenderInputs::new("beat.wav", 8.0);
        inputs.background = Some(RenderBackground::Video(PathBuf::from("background.mp4")));
        let argv = ffmpeg_command(&profile, &inputs).unwrap();
        assert!(
            argv.windows(4)
                .any(|args| args == ["-stream_loop", "-1", "-i", "background.mp4"])
        );
        assert!(argv.windows(4).any(|args| args == ["-i", "beat.wav", "-vf", "setpts=PTS-STARTPTS,fps=30.0,scale=1920:1080:force_original_aspect_ratio=increase:flags=lanczos,crop=1920:1080,setsar=1,ass=lyrics.ass:fontsdir=fonts"]));
        assert!(argv.windows(2).any(|args| args == ["-map", "0:v:0"]));
        assert!(argv.windows(2).any(|args| args == ["-map", "1:a:0"]));
        assert!(!argv.iter().any(|arg| arg.starts_with("color=c=")));
    }

    #[test]
    fn ffmpeg_command_image_background_loops_at_output_rate() {
        let mut inputs = RenderInputs::new("beat.wav", 8.0);
        inputs.background = Some(RenderBackground::Image(PathBuf::from("background.jpg")));
        let argv = ffmpeg_command(&RenderProfile::default(), &inputs).unwrap();
        assert!(
            argv.windows(6)
                .any(|args| args == ["-loop", "1", "-framerate", "30.0", "-i", "background.jpg"])
        );
        assert!(argv.windows(2).any(|args| args == ["-map", "1:a:0"]));
    }

    #[test]
    fn ffmpeg_command_guide_with_video_background_maps_guide_audio_inputs() {
        let profile = RenderProfile {
            audio_output: AudioOutput::Guide,
            ..RenderProfile::default()
        };
        let mut inputs = RenderInputs::new("beat.wav", 8.0);
        inputs.background = Some(RenderBackground::Video(PathBuf::from("background.mp4")));
        inputs.vocals = Some(PathBuf::from("vocals.wav"));
        let argv = ffmpeg_command(&profile, &inputs).unwrap();
        let complex = argv
            .iter()
            .position(|arg| arg == "-filter_complex")
            .unwrap();
        assert!(argv[complex + 1].contains("[0:v:0]"));
        assert!(argv[complex + 1].contains("[1:a:0][2:a:0]amix="));
        assert_eq!(
            &argv[complex + 2..complex + 6],
            ["-map", "[base]", "-map", "[audio]"]
        );
    }

    #[test]
    fn ffmpeg_command_guide_without_vocals_errors() {
        let profile = RenderProfile {
            audio_output: AudioOutput::Guide,
            ..RenderProfile::default()
        };
        let inputs = RenderInputs::new("beat.wav", 8.0);
        assert!(matches!(
            ffmpeg_command(&profile, &inputs),
            Err(RenderError::MissingGuideAudio)
        ));
    }

    #[test]
    fn ffmpeg_command_preview_and_mkv() {
        let profile = RenderProfile {
            format: RenderFormat::Mkv,
            fps: Fps::Fixed(30.0),
            ..RenderProfile::default()
        };
        let mut inputs = RenderInputs::new("beat.wav", 20.0);
        inputs.preview = Some(PreviewRange {
            start: 2.0,
            duration: 5.0,
        });
        let argv = ffmpeg_command(&profile, &inputs).unwrap();
        assert!(argv.windows(2).any(|w| w == ["-ss", "2.0"]));
        assert!(argv.windows(2).any(|w| w == ["-t", "5.0"]));
        // MKV gets no -movflags.
        assert!(!argv.iter().any(|a| a == "-movflags"));
        assert_eq!(argv.last().unwrap(), "karaoke.mkv");
    }

    #[test]
    fn verify_output_rejects_missing_and_small_files() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope.mp4");
        assert!(matches!(
            verify_output(&missing, 2.0, 320, 240),
            Err(RenderError::MissingOutput)
        ));
        let small = dir.path().join("small.mp4");
        std::fs::write(&small, b"tiny").unwrap();
        assert!(matches!(
            verify_output(&small, 2.0, 320, 240),
            Err(RenderError::MissingOutput)
        ));
    }

    /// Integration test: render a real 2 s colour video with the exact
    /// argv `ffmpeg_command` produces, then verify it.
    #[test]
    fn verify_output_accepts_real_render_and_rejects_wrong_expectations() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir(root.join("fonts")).unwrap();
        // Minimal valid ASS file, as the `ass` filter requires one.
        std::fs::write(
            root.join("lyrics.ass"),
            "[Script Info]\nScriptType: v4.00+\nPlayResX: 320\nPlayResY: 240\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Main,Arial,20,&H00FFFFFF,&H00FFFFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,1,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n",
        )
        .unwrap();
        // 2 s sine beat.
        let sine = std::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=2",
                "-ar",
                "44100",
            ])
            .arg(root.join("beat.wav"))
            .output()
            .unwrap();
        assert!(sine.status.success(), "sine generation failed");

        let profile = RenderProfile {
            fps: Fps::Fixed(30.0),
            frame_size_override: Some((320, 240)),
            ..RenderProfile::default()
        };
        let inputs = RenderInputs::new("beat.wav", 2.0);
        let argv = ffmpeg_command(&profile, &inputs).unwrap();
        let render = std::process::Command::new(&argv[0])
            .args(&argv[1..])
            .current_dir(root)
            .output()
            .unwrap();
        assert!(
            render.status.success(),
            "ffmpeg render failed: {}",
            String::from_utf8_lossy(&render.stderr)
        );
        let out = root.join("karaoke.mp4");
        assert!(out.is_file());

        let verified = verify_output(&out, 2.0, 320, 240).unwrap();
        assert_eq!(verified.width, 320);
        assert_eq!(verified.height, 240);
        assert_eq!(verified.video_codec.as_deref(), Some("h264"));
        assert_eq!(verified.audio_codec.as_deref(), Some("aac"));
        assert!((verified.duration - 2.0).abs() < 0.1);

        assert!(matches!(
            verify_output(&out, 2.0, 1920, 1080),
            Err(RenderError::SizeMismatch(320, 240, 1920, 1080))
        ));
        assert!(matches!(
            verify_output(&out, 10.0, 320, 240),
            Err(RenderError::DurationMismatch(_, _))
        ));

        let scene_image = root.join("scene.png");
        let image = std::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=red:s=64x64:d=1",
                "-frames:v",
                "1",
            ])
            .arg(&scene_image)
            .output()
            .unwrap();
        assert!(image.status.success(), "scene image generation failed");
        let mut scene_inputs = RenderInputs::new("beat.wav", 2.0);
        scene_inputs.scenes.push(RenderScene {
            image: scene_image,
            start: 0.25,
            end: 1.75,
            opacity: 0.6,
            fade_seconds: 0.2,
        });
        let argv = ffmpeg_command(&profile, &scene_inputs).unwrap();
        let render = std::process::Command::new(&argv[0])
            .args(&argv[1..])
            .current_dir(root)
            .output()
            .unwrap();
        assert!(
            render.status.success(),
            "scene render failed: {}",
            String::from_utf8_lossy(&render.stderr)
        );
        assert!((verify_output(&out, 2.0, 320, 240).unwrap().duration - 2.0).abs() < 0.1);
    }
}
