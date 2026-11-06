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
//!   backgrounds, scene layers, logo/countdown overlays and guide audio.
//! * `layout::display_plan` and `ass::build_lyrics_ass` — measured lyric row
//!   geometry and the native subtitle pass, consuming the same plan. Role
//!   icon artwork is quantized and embedded as portable ASS vector rectangles.
//! * `verify_output` — uses ffprobe JSON with an ffmpeg-summary fallback.
//!
//! # Remaining parity work
//!
//! * `TextMeasurer` is injected so preview/export can use the same actual font
//!   face. GPUI supplies the Windows preview measurer and passes its cached
//!   `DisplayPlan` to `prepare_lyrics_ass` in the export action.
//! * ASS covers planned row placement, subtitle backing, duet colors and the
//!   baseline's bundled role icon artwork, timed karaoke sweeps, gradients, title cards, scene-aware
//!   readability backing and vector countdown icons. VNI/TCVN3 text mapping is
//!   supported; bundled fonts and custom bitmap role artwork remain open.
//! * Scene image and decoration position/scale/alpha, logo effects and custom
//!   image countdown overlays are supported. GPUI export runs on a worker
//!   thread, reports FFmpeg progress, can be cancelled, probes NVENC and
//!   retries with libx264 when a hardware encoder failure is detected.
//! * `verify_output` prefers `ffprobe` JSON and falls back to parsing
//!   `ffmpeg -i` stderr when the probe binary is unavailable, matching
//!   the Python baseline.
//! * AI / playback / jobs — those live in `vietkara-ai-bridge`,
//!   `vietkara-media` and `vietkara-jobs`.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;

pub mod ass;
pub mod layout;
pub mod role_icons;

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
    #[error("Không tìm thấy logo trong project.")]
    MissingLogoImage,
    #[error("Không tìm thấy ảnh đếm ngược trong project.")]
    MissingCountdownImage,
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
    pub image: Option<PathBuf>,
    pub start: f64,
    pub end: f64,
    pub opacity: f64,
    pub backing_opacity: u8,
    pub fade_seconds: f64,
    pub keyframes: [SceneKeyframe; 2],
    pub decoration: Option<RenderSceneDecoration>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RenderSceneDecoration {
    pub image: PathBuf,
    pub image_width: u32,
    pub image_height: u32,
    pub size: f64,
    pub keyframes: [SceneKeyframe; 2],
}

/// Match Python `_bright_pixel_ratio`: grayscale 48×27 thumbnail and its 80th percentile.
pub fn readability_opacity(path: &Path) -> u8 {
    let Ok(image) = image::open(path) else {
        return 0;
    };
    let gray = image
        .thumbnail(96, 54)
        .resize_exact(48, 27, image::imageops::FilterType::Nearest)
        .to_luma8();
    let mut histogram = [0_u32; 256];
    for pixel in gray.pixels() {
        histogram[pixel.0[0] as usize] += 1;
    }
    let target = ((48 * 27) as f64 * 0.8).ceil() as u32;
    let mut count = 0;
    for (level, amount) in histogram.into_iter().enumerate() {
        count += amount;
        if count >= target {
            return if level as f64 / 255.0 >= 0.64 { 44 } else { 0 };
        }
    }
    0
}

/// Resolve a scene's contrast policy for preview and export.
pub fn scene_readability_opacity(scene: &serde_json::Value, directory: &Path) -> u8 {
    match scene
        .get("contrast_mode")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("auto")
    {
        "always" => 48,
        "off" => 0,
        _ => scene
            .get("id")
            .and_then(serde_json::Value::as_str)
            .filter(|id| vietkara_domain::is_hex32(id))
            .map(|id| readability_opacity(&directory.join(format!("scene_{id}.png"))))
            .unwrap_or(0),
    }
}

#[cfg(test)]
mod readability_tests {
    use super::*;

    #[test]
    fn bright_pixel_threshold_matches_scene_readability_policy() {
        let directory = tempfile::tempdir().unwrap();
        let dark = directory.path().join("dark.png");
        let bright = directory.path().join("bright.png");
        image::RgbImage::from_pixel(96, 54, image::Rgb([100, 100, 100]))
            .save(&dark)
            .unwrap();
        image::RgbImage::from_pixel(96, 54, image::Rgb([200, 200, 200]))
            .save(&bright)
            .unwrap();
        assert_eq!(readability_opacity(&dark), 0);
        assert_eq!(readability_opacity(&bright), 44);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RenderLogo {
    pub image: PathBuf,
    pub width: u32,
    pub height: u32,
    pub opacity: f64,
    pub x: f64,
    pub y: f64,
    pub effect: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RenderCountdown {
    pub image: PathBuf,
    pub start: f64,
    pub end: f64,
    pub x: f64,
    pub y: f64,
    pub size: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneKeyframe {
    pub x: f64,
    pub y: f64,
    pub scale: f64,
    pub opacity: f64,
}

impl Default for SceneKeyframe {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            scale: 100.0,
            opacity: 100.0,
        }
    }
}

fn png_dimensions(path: &Path) -> Option<(u32, u32)> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).ok()?;
    let mut bytes = [0u8; 24];
    file.read_exact(&mut bytes).ok()?;
    if &bytes[..8] != b"\x89PNG\r\n\x1a\n" || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    (width > 0 && height > 0).then_some((width, height))
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
    pub logo: Option<RenderLogo>,
    pub countdown: Vec<RenderCountdown>,
    /// Guide vocal file; required only when
    /// `profile.audio_output == AudioOutput::Guide`.
    pub vocals: Option<PathBuf>,
    /// Apply the Python renderer's `loudnorm=I=-16:TP=-1.5:LRA=11` filter
    /// to the selected soundtrack after any guide-vocal mix.
    pub audio_normalize: bool,
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
            logo: None,
            countdown: Vec::new(),
            vocals: None,
            audio_normalize: false,
            ass_file: PathBuf::from("lyrics.ass"),
            fonts_dir: PathBuf::from("fonts"),
            duration,
            preview: None,
            ffmpeg: PathBuf::from("ffmpeg"),
            encoder: VideoEncoder::Libx264,
        }
    }

    /// Serialize the shared measured plan to an export-local ASS file and
    /// point the FFmpeg command at it. The caller should place `path` inside
    /// its temporary render directory, never over the user's project files.
    pub fn prepare_lyrics_ass(
        &mut self,
        project: &vietkara_domain::Project,
        plan: &layout::DisplayPlan,
        path: impl AsRef<Path>,
    ) -> Result<(), RenderError> {
        let path = path.as_ref();
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        if let Err(error) = file
            .write_all(ass::build_lyrics_ass_with_scenes(project, plan, &self.scenes).as_bytes())
            .and_then(|()| file.sync_all())
        {
            drop(file);
            let _ = std::fs::remove_file(path);
            return Err(error.into());
        }
        self.ass_file = path.to_path_buf();
        Ok(())
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
                let image_path = directory.join(format!("scene_{id}.png"));
                let image = image_path.is_file().then_some(image_path);
                let contrast_mode = scene
                    .get("contrast_mode")
                    .and_then(serde_json::Value::as_str)
                    .filter(|mode| ["auto", "always", "off"].contains(mode))
                    .unwrap_or("auto")
                    .to_owned();
                let decoration_path = directory.join(format!("scene_decoration_{id}.png"));
                let decoration = png_dimensions(&decoration_path)
                    .map(|(width, height)| (decoration_path, width, height));
                if image.is_none() && decoration.is_none() {
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
                let parse_frame = |value: Option<&serde_json::Value>, fallback: SceneKeyframe| {
                    let value = value.filter(|value| value.is_object());
                    let frame_value = |key: &str, default: f64, min: f64, max: f64| {
                        value
                            .and_then(|frame| frame.get(key))
                            .and_then(serde_json::Value::as_f64)
                            .filter(|value| value.is_finite())
                            .unwrap_or(default)
                            .clamp(min, max)
                    };
                    SceneKeyframe {
                        x: frame_value("x", fallback.x, -100.0, 100.0),
                        y: frame_value("y", fallback.y, -100.0, 100.0),
                        scale: frame_value("scale", fallback.scale, 100.0, 200.0),
                        opacity: frame_value("opacity", fallback.opacity, 0.0, 100.0),
                    }
                };
                let frames = scene.get("keyframes").and_then(serde_json::Value::as_array);
                let first = parse_frame(
                    frames.and_then(|frames| frames.first()),
                    SceneKeyframe::default(),
                );
                let second = parse_frame(frames.and_then(|frames| frames.get(1)), first);
                let decoration_frames = scene
                    .get("decoration_keyframes")
                    .and_then(serde_json::Value::as_array);
                let center = SceneKeyframe {
                    x: 50.0,
                    y: 50.0,
                    ..SceneKeyframe::default()
                };
                let parse_decoration_frame =
                    |value: Option<&serde_json::Value>, fallback: SceneKeyframe| {
                        let value = value.filter(|value| value.is_object());
                        let frame_value = |key: &str, default: f64, min: f64, max: f64| {
                            value
                                .and_then(|frame| frame.get(key))
                                .and_then(serde_json::Value::as_f64)
                                .filter(|value| value.is_finite())
                                .unwrap_or(default)
                                .clamp(min, max)
                        };
                        SceneKeyframe {
                            x: frame_value("x", fallback.x, 0.0, 100.0),
                            y: frame_value("y", fallback.y, 0.0, 100.0),
                            scale: frame_value("scale", fallback.scale, 100.0, 200.0),
                            opacity: frame_value("opacity", fallback.opacity, 0.0, 100.0),
                        }
                    };
                let decoration_first = parse_decoration_frame(
                    decoration_frames.and_then(|frames| frames.first()),
                    center,
                );
                let decoration_second = parse_decoration_frame(
                    decoration_frames.and_then(|frames| frames.get(1)),
                    decoration_first,
                );
                let decoration =
                    decoration.map(|(image, image_width, image_height)| RenderSceneDecoration {
                        image,
                        image_width,
                        image_height,
                        size: number("decoration_size", 28.0).clamp(5.0, 80.0),
                        keyframes: [decoration_first, decoration_second],
                    });
                Some((
                    section.to_owned(),
                    image,
                    number("opacity", 100.0).clamp(0.0, 100.0) / 100.0,
                    if transition == "fade" {
                        number("transition_seconds", 0.6).clamp(0.0, 3.0)
                    } else {
                        0.0
                    },
                    [first, second],
                    decoration,
                    contrast_mode,
                ))
            })
            .map(
                |(section, image, opacity, transition, frames, decoration, contrast_mode)| {
                    (
                        section,
                        (
                            image,
                            opacity,
                            transition,
                            frames,
                            decoration,
                            contrast_mode,
                        ),
                    )
                },
            )
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
            let Some((image, opacity, base_transition, keyframes, decoration, contrast_mode)) =
                scenes.get(section)
            else {
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
                    backing_opacity: match contrast_mode.as_str() {
                        "always" => 48,
                        "auto" => image
                            .as_ref()
                            .map(|path| readability_opacity(path))
                            .unwrap_or(0),
                        _ => 0,
                    },
                    fade_seconds: transition.min((end - start) / 2.0).max(0.05),
                    keyframes: *keyframes,
                    decoration: decoration.clone(),
                });
            }
        }
        self.scenes = resolved;
    }

    /// Resolve project logo and custom image countdown overlays.
    pub fn configure_overlays_from_project(
        &mut self,
        project: &vietkara_domain::Project,
        directory: &Path,
        width: u32,
        height: u32,
    ) -> Result<(), RenderError> {
        use std::collections::BTreeMap;
        let settings = &project.settings.extra;
        let number = |key: &str, default: f64| {
            settings
                .get(key)
                .and_then(serde_json::Value::as_f64)
                .filter(|value| value.is_finite())
                .unwrap_or(default)
        };
        let enabled = settings
            .get("logo_enabled")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        self.logo = None;
        if enabled {
            if project
                .extra
                .get("logo_name")
                .and_then(serde_json::Value::as_str)
                .is_none()
            {
                return Err(RenderError::MissingLogoImage);
            }
            let image = directory.join("logo.png");
            let Some((image_width, image_height)) = png_dimensions(&image) else {
                return Err(RenderError::MissingLogoImage);
            };
            let ratio = image_width as f64 / image_height as f64;
            let size = number("logo_size", 12.0).clamp(3.0, 40.0) / 100.0;
            let logo_width = ((width as f64 * size).min(height as f64 * 0.4 * ratio))
                .round()
                .max(2.0) as u32;
            let logo_height = (logo_width as f64 / ratio).round().max(2.0) as u32;
            let effect = settings
                .get("logo_effect")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("none");
            self.logo = Some(RenderLogo {
                image,
                width: logo_width,
                height: logo_height,
                opacity: number("logo_opacity", 100.0).clamp(0.0, 100.0) / 100.0,
                x: number("logo_x", 96.0).clamp(0.0, 100.0) / 100.0,
                y: number("logo_y", 4.0).clamp(0.0, 100.0) / 100.0,
                effect: if ["none", "fade", "float", "pulse"].contains(&effect) {
                    effect.to_owned()
                } else {
                    "none".into()
                },
            });
        }

        self.countdown.clear();
        let countdown_image = settings
            .get("countdown")
            .and_then(serde_json::Value::as_str)
            == Some("image");
        let karaoke = settings
            .get("display_mode")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("karaoke")
            == "karaoke";
        if countdown_image && karaoke {
            let cd_path = directory.join("countdown.png");
            let duration = number("countdown_seconds", 4.0).clamp(1.0, 8.0);
            let scale = width as f64 / 1920.0;
            let font_size = project.settings.font_size * scale;
            let position = settings
                .get("position")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("bottom");
            let baseline = match position {
                "top" => font_size * 2.0,
                "center" => height as f64 / 2.0 + font_size * 0.5,
                _ => height as f64 - font_size * 1.25,
            };
            let y = (baseline - font_size * 2.05).max(0.0);
            let size = (font_size * 0.7).round().max(2.0) as u32;
            let mut lines: BTreeMap<u32, Vec<(f64, f64)>> = BTreeMap::new();
            for word in &project.words {
                let start = vietkara_domain::timing::map_time(word.start, &project.anchors, false);
                let end = vietkara_domain::timing::map_time(word.end, &project.anchors, false);
                lines.entry(word.line).or_default().push((start, end));
            }
            let mut ordered = Vec::new();
            for mut words in lines.into_values() {
                words.sort_by(|left, right| left.0.total_cmp(&right.0));
                let mut chunk: Option<(f64, f64)> = None;
                for (start, end) in words {
                    if let Some(current) = chunk.as_mut() {
                        if start - current.1 > 4.0 {
                            ordered.push(*current);
                            chunk = Some((start, end));
                        } else {
                            current.1 = current.1.max(end);
                        }
                    } else {
                        chunk = Some((start, end));
                    }
                }
                if let Some(chunk) = chunk {
                    ordered.push(chunk);
                }
            }
            ordered.sort_by(|left, right| left.0.total_cmp(&right.0));
            let mut previous_end: f64 = 0.0;
            for (index, (start, end)) in ordered.iter().enumerate() {
                let count_in = index == 0 || start - previous_end >= 8.0;
                if count_in {
                    let window_end = (start - 0.05).max(0.0);
                    let window_start = 0.0_f64.max(previous_end).max(window_end - duration);
                    if window_end - window_start >= 0.4 {
                        self.countdown.push(RenderCountdown {
                            image: cd_path.clone(),
                            start: window_start,
                            end: window_end,
                            x: width as f64 * 0.05,
                            y,
                            size,
                        });
                    }
                }
                previous_end = previous_end.max(*end);
            }
            if !self.countdown.is_empty() && png_dimensions(&cd_path).is_none() {
                return Err(RenderError::MissingCountdownImage);
            }
        }
        Ok(())
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
    let background_filter = format!(
        "setpts=PTS-STARTPTS,fps={fps},scale={width}:{height}:force_original_aspect_ratio=increase:flags=lanczos,crop={width}:{height},setsar=1"
    );
    let visual_filter = format!(
        "{background_filter},ass={}:fontsdir={}",
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
    let mut decoration_inputs: Vec<(usize, &RenderScene, &RenderSceneDecoration)> = Vec::new();
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
        if let Some(image) = &scene.image {
            args.extend([
                "-loop".into(),
                "1".into(),
                "-framerate".into(),
                fps.clone(),
                "-i".into(),
                image.display().to_string(),
            ]);
            scene_inputs.push((next_input, scene));
            next_input += 1;
        }
        if let Some(decoration) = &scene.decoration {
            args.extend([
                "-loop".into(),
                "1".into(),
                "-framerate".into(),
                fps.clone(),
                "-i".into(),
                decoration.image.display().to_string(),
            ]);
            decoration_inputs.push((next_input, scene, decoration));
            next_input += 1;
        }
    }
    let countdown_events = inputs
        .countdown
        .iter()
        .flat_map(|event| {
            (0..4).filter_map(move |slot| {
                (event.end - slot as f64 > event.start).then_some((event, slot))
            })
        })
        .collect::<Vec<_>>();
    let countdown_input = if let Some(event) = countdown_events.first().map(|(event, _)| event) {
        args.extend([
            "-loop".into(),
            "1".into(),
            "-framerate".into(),
            fps.clone(),
            "-i".into(),
            event.image.display().to_string(),
        ]);
        let index = next_input;
        next_input += 1;
        Some(index)
    } else {
        None
    };
    let logo_input = if let Some(logo) = &inputs.logo {
        args.extend([
            "-loop".into(),
            "1".into(),
            "-framerate".into(),
            fps.clone(),
            "-i".into(),
            logo.image.display().to_string(),
        ]);
        Some(next_input)
    } else {
        None
    };
    if guide
        || !scene_inputs.is_empty()
        || !decoration_inputs.is_empty()
        || countdown_input.is_some()
        || logo_input.is_some()
    {
        let mut graph = format!("[{background_input}:v:0]{background_filter}[scene_base]");
        let mut last_background = "scene_base".to_owned();
        for (index, (input_index, scene)) in scene_inputs.iter().enumerate() {
            let start = scene.start.max(0.0);
            let end = scene.end.min(inputs.duration);
            if end <= start {
                continue;
            }
            let fade = scene.fade_seconds.min((end - start) / 2.0).max(0.0);
            let opacity = scene.opacity.clamp(0.0, 1.0);
            let [first, last] = scene.keyframes;
            let local_progress = format!(
                "min(max(on/{:.6},0),1)",
                (end - start) * profile.fps_value()
            );
            let alpha_progress = format!("min(max(T/{:.6},0),1)", end - start);
            let lerp =
                |a: f64, b: f64, progress: &str| format!("({a:.4}+({:.4})*{progress})", b - a);
            let scale = format!(
                "max(1,{}/100)",
                lerp(first.scale, last.scale, &local_progress)
            )
            .replace(',', "\\,");
            let x = lerp(first.x, last.x, &local_progress);
            let y = lerp(first.y, last.y, &local_progress);
            let x = x.replace(',', "\\,");
            let y = y.replace(',', "\\,");
            let mut layer = format!(
                "[{input_index}:v:0]setpts=PTS-STARTPTS,fps={fps},scale={width}:{height}:force_original_aspect_ratio=increase:flags=lanczos,crop={width}:{height},setsar=1,zoompan=z='{scale}':x='iw/2-iw/zoom/2-({x})*(iw-iw/zoom)/100':y='ih/2-ih/zoom/2-({y})*(ih-ih/zoom)/100':d=1:s={width}x{height}:fps={fps},format=rgba"
            );
            if (first.opacity - last.opacity).abs() < 0.001 {
                layer.push_str(&format!(
                    ",colorchannelmixer=aa={:.4}",
                    opacity * first.opacity / 100.0
                ));
            } else {
                let opacity_curve =
                    lerp(first.opacity, last.opacity, &alpha_progress).replace(',', "\\,");
                layer.push_str(&format!(
                    ",geq=r='r(X\\,Y)':g='g(X\\,Y)':b='b(X\\,Y)':a='alpha(X\\,Y)*({opacity:.4}*{opacity_curve}/100)'"
                ));
            }
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
                ";[{last_background}][scene_layer{index}]overlay=x=0:y=0:eof_action=pass:shortest=0:enable='between(t\\,{start:.3}\\,{end:.3})'[scene_bg{index}]"
            ));
            last_background = format!("scene_bg{index}");
        }
        for (index, (input_index, scene, decoration)) in decoration_inputs.iter().enumerate() {
            let start = scene.start.max(0.0);
            let end = scene.end.min(inputs.duration);
            if end <= start {
                continue;
            }
            let duration = end - start;
            let fade = scene.fade_seconds.min(duration / 2.0).max(0.0);
            let [first, last] = decoration.keyframes;
            let alpha_progress = format!("min(max(T/{duration:.6},0),1)");
            let lerp =
                |a: f64, b: f64, progress: &str| format!("({a:.4}+({:.4})*{progress})", b - a);
            let max_width = (width as f64 * decoration.size * 2.0 / 100.0)
                .round()
                .max(1.0) as u32;
            let max_height = (max_width as f64 * decoration.image_height as f64
                / decoration.image_width.max(1) as f64)
                .round()
                .max(1.0) as u32;
            let initial_scale = first.scale.clamp(100.0, 200.0);
            let base_width = (max_width as f64 * initial_scale / 200.0).round().max(1.0) as u32;
            let base_height = (max_height as f64 * initial_scale / 200.0).round().max(1.0) as u32;
            let scale_progress = format!("min(max(on/{:.6},0),1)", duration * profile.fps_value());
            let zoom = format!(
                "{}/{}",
                lerp(first.scale, last.scale, &scale_progress),
                initial_scale
            )
            .replace(',', "\\,");
            let mut layer = format!(
                "[{input_index}:v:0]setpts=PTS-STARTPTS,format=rgba,scale={base_width}:{base_height}:force_original_aspect_ratio=decrease,pad={max_width}:{max_height}:(ow-iw)/2:(oh-ih)/2:color=black@0,zoompan=z='{zoom}':x='iw/2-iw/zoom/2':y='ih/2-ih/zoom/2':d=1:s={max_width}x{max_height}:fps={fps}"
            );
            let opacity_expr =
                lerp(first.opacity, last.opacity, &alpha_progress).replace(',', "\\,");
            layer.push_str(&format!(
                ",geq=r='r(X\\,Y)':g='g(X\\,Y)':b='b(X\\,Y)':a='alpha(X\\,Y)*({opacity_expr})/100'"
            ));
            if fade >= 0.05 {
                layer.push_str(&format!(",fade=t=in:st=0:d={fade:.3}:alpha=1"));
                layer.push_str(&format!(
                    ",fade=t=out:st={:.3}:d={fade:.3}:alpha=1",
                    (duration - fade).max(0.0)
                ));
            }
            layer.push_str(&format!(
                ",trim=duration={duration:.3},setpts=PTS-STARTPTS+{start:.3}/TB[scene_decoration{index}]"
            ));
            let position_progress = format!("min(max((t-{start:.6})/{duration:.6},0),1)");
            let x = lerp(first.x, last.x, &position_progress).replace(',', "\\,");
            let y = lerp(first.y, last.y, &position_progress).replace(',', "\\,");
            graph.push(';');
            graph.push_str(&layer);
            graph.push_str(&format!(
                ";[{last_background}][scene_decoration{index}]overlay=x='W*({x})/100-w/2':y='H*({y})/100-h/2':eof_action=pass:shortest=0:enable='between(t\\,{start:.3}\\,{end:.3})'[scene_decor{index}]"
            ));
            last_background = format!("scene_decor{index}");
        }
        graph.push_str(&format!(
            ";[{last_background}]ass={}:fontsdir={}[base]",
            inputs.ass_file.display(),
            inputs.fonts_dir.display()
        ));
        let mut last = "base".to_owned();
        if let Some(input_index) = countdown_input {
            let count = countdown_events.len();
            graph.push_str(&format!(";[{input_index}:v:0]format=rgba,split={count}"));
            for index in 0..count {
                graph.push_str(&format!("[countdown_source{index}]"));
            }
            for (index, (event, slot)) in countdown_events.iter().enumerate() {
                let size = event.size.max(2);
                graph.push_str(&format!(
                    ";[countdown_source{index}]scale={size}:{size}:force_original_aspect_ratio=decrease,pad={size}:{size}:(ow-iw)/2:(oh-ih)/2:color=black@0[countdown_icon{index}]"
                ));
                graph.push_str(&format!(
                    ";[{last}][countdown_icon{index}]overlay=x={:.2}:y={:.2}:enable='between(t\\,{:.4}\\,{:.4})'[countdown_overlay{index}]",
                    event.x + *slot as f64 * size as f64 * 1.28,
                    event.y,
                    event.start,
                    event.end - *slot as f64
                ));
                last = format!("countdown_overlay{index}");
            }
        }
        if let (Some(input_index), Some(logo)) = (logo_input, &inputs.logo) {
            let mut logo_filter = format!(
                "[{input_index}:v:0]setpts=PTS-STARTPTS,scale={}:{},format=rgba,colorchannelmixer=aa={:.4}",
                logo.width,
                logo.height,
                logo.opacity.clamp(0.0, 1.0)
            );
            match logo.effect.as_str() {
                "fade" => logo_filter.push_str(",fade=t=in:st=0:d=1:alpha=1"),
                "pulse" => logo_filter.push_str(",geq=r='r(X\\,Y)':g='g(X\\,Y)':b='b(X\\,Y)':a='alpha(X\\,Y)*(0.8+0.2*sin(PI*T/2))'"),
                _ => {}
            }
            graph.push(';');
            graph.push_str(&logo_filter);
            graph.push_str("[watermark]");
            let x = format!("(W-w)*{:.6}", logo.x.clamp(0.0, 1.0));
            let y = if logo.effect == "float" {
                format!(
                    "max(0\\,min(H-h\\,(H-h)*{:.6}+{:.4}*sin(PI*t/2)))",
                    logo.y.clamp(0.0, 1.0),
                    8.0 * height as f64 / 1080.0
                )
            } else {
                format!("(H-h)*{:.6}", logo.y.clamp(0.0, 1.0))
            };
            graph.push_str(&format!(
                ";[{last}][watermark]overlay=x='{x}':y='{y}':format=auto[withlogo]"
            ));
            last = "withlogo".into();
        }
        if guide {
            graph.push_str(&format!(
                ";[{beat_input}:a:0][{vocals_input}:a:0]amix=inputs=2:duration=first:dropout_transition=0:normalize=1[audio]"
            ));
        }
        args.push("-filter_complex".into());
        args.push(graph);
        args.push("-map".into());
        args.push(format!("[{last}]"));
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
    if inputs.audio_normalize {
        args.extend(["-af".into(), "loudnorm=I=-16:TP=-1.5:LRA=11".into()]);
    }
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
    let ffmpeg = std::env::var_os("OBS_FFMPEG")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("ffmpeg"));
    verify_output_with_tools(path, duration, width, height, Some(&ffprobe), &ffmpeg)
}

pub fn verify_output_with_ffprobe(
    path: &Path,
    duration: f64,
    width: u32,
    height: u32,
    ffprobe: &Path,
) -> Result<VerifiedOutput, RenderError> {
    let ffmpeg = ffprobe
        .parent()
        .map(|parent| {
            parent.join(if cfg!(windows) {
                "ffmpeg.exe"
            } else {
                "ffmpeg"
            })
        })
        .filter(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from("ffmpeg"));
    verify_output_with_tools(path, duration, width, height, Some(ffprobe), &ffmpeg)
}

/// Prefer ffprobe JSON, but fall back to FFmpeg's stream summary when ffprobe
/// is not installed, matching Python render_info.verify_output.
pub fn verify_output_with_tools(
    path: &Path,
    duration: f64,
    width: u32,
    height: u32,
    ffprobe: Option<&Path>,
    ffmpeg: &Path,
) -> Result<VerifiedOutput, RenderError> {
    let metadata = std::fs::metadata(path).map_err(|_| RenderError::MissingOutput)?;
    if !metadata.is_file() || metadata.len() < 1024 {
        return Err(RenderError::MissingOutput);
    }
    let output = if let Some(ffprobe) = ffprobe {
        match std::process::Command::new(ffprobe)
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
        {
            Ok(output) => output,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return verify_output_with_ffmpeg(path, duration, width, height, ffmpeg);
            }
            Err(error) => return Err(RenderError::FfprobeNotFound(error.to_string())),
        }
    } else {
        return verify_output_with_ffmpeg(path, duration, width, height, ffmpeg);
    };
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

fn verify_output_with_ffmpeg(
    path: &Path,
    duration: f64,
    width: u32,
    height: u32,
    ffmpeg: &Path,
) -> Result<VerifiedOutput, RenderError> {
    let output = std::process::Command::new(ffmpeg)
        .args(["-hide_banner", "-i"])
        .arg(path)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|error| {
            RenderError::FfprobeNotFound(format!(
                "Không chạy được ffprobe và fallback ffmpeg: {error}"
            ))
        })?;
    let summary = String::from_utf8_lossy(&output.stderr);
    let has_video = summary.contains("Video:");
    let has_audio = summary.contains("Audio:");
    if !has_video || !has_audio {
        return Err(RenderError::MissingStreams);
    }
    let output_duration = summary
        .split_once("Duration: ")
        .and_then(|(_, rest)| rest.split([',', '\n']).next())
        .and_then(parse_probe_duration)
        .unwrap_or(0.0);
    if output_duration != 0.0 && (output_duration - duration).abs() > f64::max(1.0, duration * 0.01)
    {
        return Err(RenderError::DurationMismatch(output_duration, duration));
    }
    let video_summary = summary
        .split_once("Video:")
        .and_then(|(_, rest)| rest.lines().next())
        .unwrap_or_default();
    let actual_size = video_summary
        .split(|character: char| !(character.is_ascii_digit() || character == 'x'))
        .find_map(|part| {
            let (width, height) = part.split_once('x')?;
            Some((width.parse::<u32>().ok()?, height.parse::<u32>().ok()?))
        });
    let (actual_width, actual_height) = actual_size.unwrap_or((0, 0));
    if actual_size.is_some() && (actual_width != width || actual_height != height) {
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
        video_codec: None,
        audio_codec: None,
    })
}

fn parse_probe_duration(value: &str) -> Option<f64> {
    let mut fields = value.trim().split(':');
    let hours = fields.next()?.parse::<f64>().ok()?;
    let minutes = fields.next()?.parse::<f64>().ok()?;
    let seconds = fields.next()?.parse::<f64>().ok()?;
    if fields.next().is_some()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn write_png_header(path: &Path, width: u32, height: u32) {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend(width.to_be_bytes());
        png.extend(height.to_be_bytes());
        std::fs::write(path, png).unwrap();
    }

    fn render_pixel(
        ffmpeg: &str,
        root: &Path,
        output: &Path,
        time: f64,
        frame_width: usize,
        x: usize,
        y: usize,
    ) -> [u8; 3] {
        let rendered_frame = std::process::Command::new(ffmpeg)
            .args(["-v", "error", "-ss"])
            .arg(format!("{time:.3}"))
            .args(["-i"])
            .arg(output.file_name().unwrap())
            .args([
                "-frames:v",
                "1",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgb24",
                "pipe:1",
            ])
            .current_dir(root)
            .output()
            .unwrap();
        assert!(
            rendered_frame.status.success(),
            "could not decode frame at {time}s: {}",
            String::from_utf8_lossy(&rendered_frame.stderr)
        );
        let offset = (y * frame_width + x) * 3;
        let pixel = &rendered_frame.stdout[offset..offset + 3];
        [pixel[0], pixel[1], pixel[2]]
    }

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
        let decoration_image = directory
            .path()
            .join(format!("scene_decoration_{scene_id}.png"));
        let mut png_header = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png_header.extend(64u32.to_be_bytes());
        png_header.extend(32u32.to_be_bytes());
        std::fs::write(&decoration_image, png_header).unwrap();
        let mut project = vietkara_domain::Project::new("scene");
        project.scenes.push(serde_json::json!({
            "id": scene_id,
            "section_name": "chorus",
            "opacity": 75,
            "transition": "fade",
            "transition_seconds": 0.8,
            "decoration_size": 30,
            "keyframes": [
                {"x":-50,"y":20,"scale":100,"opacity":100},
                {"x":50,"y":-20,"scale":150,"opacity":40}
            ],
            "decoration_keyframes": [
                {"x":30,"y":40,"scale":100,"opacity":100},
                {"x":70,"y":60,"scale":150,"opacity":40}
            ]
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
        assert_eq!(inputs.scenes[0].image, Some(image));
        let decoration = inputs.scenes[0].decoration.as_ref().unwrap();
        assert_eq!(decoration.image, decoration_image);
        assert_eq!((decoration.image_width, decoration.image_height), (64, 32));
        assert_eq!(decoration.size, 30.0);
        assert_eq!(decoration.keyframes[1].x, 70.0);
        assert_eq!(inputs.scenes[0].start, 2.0);
        assert_eq!(inputs.scenes[0].end, 10.0);
        assert_eq!(inputs.scenes[0].opacity, 0.75);
        assert_eq!(inputs.scenes[0].keyframes[0].x, -50.0);
        assert_eq!(inputs.scenes[0].keyframes[1].scale, 150.0);
        assert_eq!(inputs.scenes[0].keyframes[1].opacity, 40.0);

        let command = ffmpeg_command(&RenderProfile::default(), &inputs).unwrap();
        let graph_index = command
            .iter()
            .position(|arg| arg == "-filter_complex")
            .unwrap();
        let graph = &command[graph_index + 1];
        assert!(graph.contains("[2:v:0]"));
        assert!(graph.contains(
            "zoompan=z='max(1\\,(100.0000+(50.0000)*min(max(on/240.000000\\,0)\\,1))/100)'"
        ));
        assert!(graph.contains("geq=r='r(X\\,Y)'"));
        assert!(graph.contains("overlay=x=0:y=0"));
        assert!(graph.contains("ass=lyrics.ass:fontsdir=fonts[base]"));
        assert!(graph.contains("[3:v:0]setpts=PTS-STARTPTS,format=rgba,scale=576:288"));
        assert!(
            graph.contains("zoompan=z='(100.0000+(50.0000)*min(max(on/240.000000\\,0)\\,1))/100'"),
            "{graph}"
        );
        assert!(graph.contains("scene_decoration0"));
        assert_eq!(
            &command[graph_index + 2..graph_index + 6],
            ["-map", "[base]", "-map", "1:a:0"]
        );
    }

    #[test]
    fn project_overlay_resolves_logo_and_custom_countdown_events() {
        let directory = tempfile::tempdir().unwrap();
        write_png_header(&directory.path().join("logo.png"), 200, 100);
        write_png_header(&directory.path().join("countdown.png"), 64, 64);
        let mut project = vietkara_domain::Project::new("overlays");
        project
            .extra
            .insert("logo_name".into(), serde_json::json!("brand.png"));
        project.settings.extra.extend(
            serde_json::from_value::<serde_json::Map<String, serde_json::Value>>(
                serde_json::json!({
                    "logo_enabled":true,"logo_size":12,"logo_x":96,"logo_y":4,
                    "logo_opacity":80,"logo_effect":"pulse","countdown":"image",
                    "countdown_seconds":4,"display_mode":"karaoke","position":"bottom"
                }),
            )
            .unwrap(),
        );
        for (index, start) in [5.0, 15.0].into_iter().enumerate() {
            project.words.push(
                serde_json::from_value(serde_json::json!({
                    "id":format!("{:032x}", index + 1),
                    "line_uid":format!("{:032x}", index + 10),
                    "text":"Xin",
                    "start":start,
                    "end":start+0.5,
                    "line":index,
                    "role":"all"
                }))
                .unwrap(),
            );
        }
        let mut inputs = RenderInputs::new("beat.wav", 20.0);
        inputs
            .configure_overlays_from_project(&project, directory.path(), 1920, 1080)
            .unwrap();
        let logo = inputs.logo.as_ref().unwrap();
        assert_eq!((logo.width, logo.height), (230, 115));
        assert_eq!(logo.opacity, 0.8);
        assert_eq!(logo.effect, "pulse");
        assert_eq!(inputs.countdown.len(), 2);
        assert!((inputs.countdown[0].start - 0.95).abs() < 0.001);
        assert!((inputs.countdown[1].start - 10.95).abs() < 0.001);
        assert_eq!(
            inputs.countdown[0].size,
            (project.settings.font_size * 0.7).round() as u32
        );

        let command = ffmpeg_command(&RenderProfile::default(), &inputs).unwrap();
        let graph_index = command
            .iter()
            .position(|arg| arg == "-filter_complex")
            .unwrap();
        let graph = &command[graph_index + 1];
        assert!(graph.contains("split=8[countdown_source0]"));
        assert!(graph.contains("between(t\\,0.9500\\,4.9500)"));
        assert!(graph.contains("colorchannelmixer=aa=0.8000"));
        assert!(graph.contains("sin(PI*T/2)"));
        assert!(graph.contains("[withlogo]"));
    }

    #[test]
    fn image_countdown_follows_lyric_time_when_line_ids_are_out_of_order() {
        let directory = tempfile::tempdir().unwrap();
        write_png_header(&directory.path().join("countdown.png"), 64, 64);
        let mut project = vietkara_domain::Project::new("chronological countdown");
        project.settings.extra.extend(
            serde_json::from_value::<serde_json::Map<String, serde_json::Value>>(
                serde_json::json!({
                    "countdown":"image",
                    "countdown_seconds":4,
                    "display_mode":"karaoke"
                }),
            )
            .unwrap(),
        );
        for (index, (line, start)) in [(0, 15.0), (1, 5.0)].into_iter().enumerate() {
            project.words.push(
                serde_json::from_value(serde_json::json!({
                    "id":format!("{:032x}", index + 1),
                    "line_uid":format!("{:032x}", index + 10),
                    "text":"Xin",
                    "start":start,
                    "end":start+0.5,
                    "line":line,
                    "role":"all"
                }))
                .unwrap(),
            );
        }

        let mut inputs = RenderInputs::new("beat.wav", 20.0);
        inputs
            .configure_overlays_from_project(&project, directory.path(), 1920, 1080)
            .unwrap();
        assert_eq!(inputs.countdown.len(), 2);
        assert!((inputs.countdown[0].start - 0.95).abs() < 0.001);
        assert!((inputs.countdown[0].end - 4.95).abs() < 0.001);
        assert!((inputs.countdown[1].start - 10.95).abs() < 0.001);
        assert!((inputs.countdown[1].end - 14.95).abs() < 0.001);
    }

    #[test]
    fn image_countdown_splits_a_line_after_a_long_lyric_pause() {
        let directory = tempfile::tempdir().unwrap();
        write_png_header(&directory.path().join("countdown.png"), 64, 64);
        let mut project = vietkara_domain::Project::new("same-line pause");
        project.settings.extra.extend(
            serde_json::from_value::<serde_json::Map<String, serde_json::Value>>(
                serde_json::json!({
                    "countdown":"image",
                    "countdown_seconds":4,
                    "display_mode":"karaoke"
                }),
            )
            .unwrap(),
        );
        for (index, start) in [5.0, 15.0].into_iter().enumerate() {
            project.words.push(
                serde_json::from_value(serde_json::json!({
                    "id":format!("{:032x}", index + 1),
                    "line_uid":"00000000000000000000000000000011",
                    "text":"Xin",
                    "start":start,
                    "end":start+0.5,
                    "line":0,
                    "role":"all"
                }))
                .unwrap(),
            );
        }

        let mut inputs = RenderInputs::new("beat.wav", 20.0);
        inputs
            .configure_overlays_from_project(&project, directory.path(), 1920, 1080)
            .unwrap();
        assert_eq!(inputs.countdown.len(), 2);
        assert!((inputs.countdown[0].start - 0.95).abs() < 0.001);
        assert!((inputs.countdown[0].end - 4.95).abs() < 0.001);
        assert!((inputs.countdown[1].start - 10.95).abs() < 0.001);
        assert!((inputs.countdown[1].end - 14.95).abs() < 0.001);
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
    fn audio_normalization_matches_python_trailer_and_is_opt_in() {
        let profile = RenderProfile::default();
        let plain = ffmpeg_command(&profile, &RenderInputs::new("beat.wav", 8.0)).unwrap();
        assert!(!plain.iter().any(|argument| argument == "-af"));

        let mut normalized = RenderInputs::new("beat.wav", 8.0);
        normalized.audio_normalize = true;
        let argv = ffmpeg_command(&profile, &normalized).unwrap();
        let duration_index = argv.iter().position(|argument| argument == "-t").unwrap();
        assert_eq!(
            &argv[duration_index + 2..duration_index + 4],
            ["-af", "loudnorm=I=-16:TP=-1.5:LRA=11"]
        );
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
        let decoration_image = root.join("scene_decoration.png");
        let decoration = std::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=yellow@0.7:s=32x32:d=1,format=rgba",
                "-frames:v",
                "1",
            ])
            .arg(&decoration_image)
            .output()
            .unwrap();
        assert!(
            decoration.status.success(),
            "decoration image generation failed"
        );
        let mut scene_inputs = RenderInputs::new("beat.wav", 2.0);
        scene_inputs.scenes.push(RenderScene {
            image: Some(scene_image),
            start: 0.25,
            end: 1.75,
            opacity: 0.6,
            backing_opacity: 0,
            fade_seconds: 0.2,
            keyframes: [
                SceneKeyframe::default(),
                SceneKeyframe {
                    x: 50.0,
                    y: -25.0,
                    scale: 150.0,
                    opacity: 35.0,
                },
            ],
            decoration: Some(RenderSceneDecoration {
                image: decoration_image.clone(),
                image_width: 32,
                image_height: 32,
                size: 28.0,
                keyframes: [
                    SceneKeyframe {
                        x: 25.0,
                        y: 40.0,
                        opacity: 20.0,
                        ..SceneKeyframe::default()
                    },
                    SceneKeyframe {
                        x: 75.0,
                        y: 60.0,
                        scale: 160.0,
                        opacity: 80.0,
                    },
                ],
            }),
        });
        let argv = ffmpeg_command(&profile, &scene_inputs).unwrap();
        let render = std::process::Command::new(&argv[0])
            .args(&argv[1..])
            .current_dir(root)
            .output()
            .unwrap();
        assert!(
            render.status.success(),
            "scene render failed ({:?}): {}\n{}",
            render.status.code(),
            String::from_utf8_lossy(&render.stderr),
            argv.iter()
                .position(|arg| arg == "-filter_complex")
                .and_then(|index| argv.get(index + 1))
                .map(String::as_str)
                .unwrap_or("")
        );
        assert!((verify_output(&out, 2.0, 320, 240).unwrap().duration - 2.0).abs() < 0.1);
        let before_scene = render_pixel(&argv[0], root, &out, 0.1, 320, 10, 10);
        let during_scene = render_pixel(&argv[0], root, &out, 0.5, 320, 10, 10);
        let scene_graph = &argv[argv
            .iter()
            .position(|argument| argument == "-filter_complex")
            .unwrap()
            + 1];
        for (actual, expected) in before_scene.into_iter().zip([16, 29, 43]) {
            assert!(
                actual.abs_diff(expected) <= 8,
                "scene appeared before its start: pixel={before_scene:?}"
            );
        }
        // At 0.5 s the fade-in has completed and opacity has interpolated from
        // 100% to 35%; its effective alpha over the 0x101d2b base is ~53.5%.
        for (actual, expected) in during_scene.into_iter().zip([144, 14, 20]) {
            assert!(
                actual.abs_diff(expected) <= 14,
                "scene raster opacity/timing differs: pixel={during_scene:?}; graph={scene_graph}"
            );
        }
        let mut overlay_inputs = scene_inputs.clone();
        overlay_inputs.countdown.push(RenderCountdown {
            image: decoration_image.clone(),
            start: 0.1,
            end: 0.8,
            x: 10.0,
            y: 20.0,
            size: 24,
        });
        overlay_inputs.logo = Some(RenderLogo {
            image: decoration_image,
            width: 48,
            height: 48,
            opacity: 0.8,
            x: 0.92,
            y: 0.04,
            effect: "pulse".into(),
        });
        let argv = ffmpeg_command(&profile, &overlay_inputs).unwrap();
        let render = std::process::Command::new(&argv[0])
            .args(&argv[1..])
            .current_dir(root)
            .output()
            .unwrap();
        assert!(
            render.status.success(),
            "logo/countdown render failed ({:?}): {}",
            render.status.code(),
            String::from_utf8_lossy(&render.stderr)
        );
        assert!((verify_output(&out, 2.0, 320, 240).unwrap().duration - 2.0).abs() < 0.1);
        let countdown_on = render_pixel(&argv[0], root, &out, 0.5, 320, 15, 25);
        let countdown_off = render_pixel(&argv[0], root, &out, 0.9, 320, 15, 25);
        assert!(
            countdown_on[1] > 140 && countdown_on[2] < 45,
            "countdown image should be visible during its interval: {countdown_on:?}"
        );
        assert!(
            countdown_on[1].abs_diff(countdown_off[1]) > 90,
            "countdown should disappear after its interval: on={countdown_on:?}, off={countdown_off:?}"
        );
        let logo_pixel = render_pixel(&argv[0], root, &out, 0.5, 320, 255, 12);
        let outside_logo = render_pixel(&argv[0], root, &out, 0.5, 320, 230, 12);
        assert!(
            logo_pixel[1] > outside_logo[1].saturating_add(70),
            "pulsing logo should composite at its configured position: logo={logo_pixel:?}, outside={outside_logo:?}"
        );
        let decoration_early = render_pixel(&argv[0], root, &out, 0.5, 320, 107, 104);
        let decoration_late = render_pixel(&argv[0], root, &out, 0.9, 320, 107, 104);
        let outside_decoration = render_pixel(&argv[0], root, &out, 0.5, 320, 190, 104);
        assert!(
            decoration_early[1] > outside_decoration[1].saturating_add(30),
            "scene decoration should composite over the scene: decoration={decoration_early:?}, outside={outside_decoration:?}"
        );
        assert!(
            decoration_late[1] > decoration_early[1].saturating_add(12),
            "scene decoration opacity keyframes should interpolate: early={decoration_early:?}, late={decoration_late:?}"
        );
        overlay_inputs.logo.as_mut().unwrap().effect = "float".into();
        let argv = ffmpeg_command(&profile, &overlay_inputs).unwrap();
        let render = std::process::Command::new(&argv[0])
            .args(&argv[1..])
            .current_dir(root)
            .output()
            .unwrap();
        assert!(
            render.status.success(),
            "floating logo render failed ({:?}): {}",
            render.status.code(),
            String::from_utf8_lossy(&render.stderr)
        );
        assert!((verify_output(&out, 2.0, 320, 240).unwrap().duration - 2.0).abs() < 0.1);
    }

    #[test]
    fn video_background_loops_to_beat_duration_and_preserves_beat_audio() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        std::fs::create_dir(root.join("fonts")).unwrap();
        std::fs::write(
            root.join("lyrics.ass"),
            "[Script Info]\nScriptType: v4.00+\nPlayResX: 320\nPlayResY: 240\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Main,Arial,20,&H00FFFFFF,&H00FFFFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,1,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n",
        )
        .unwrap();

        let ffmpeg = "ffmpeg";
        let beat = root.join("beat.wav");
        let audio = std::process::Command::new(ffmpeg)
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
            .arg(&beat)
            .output()
            .unwrap();
        assert!(audio.status.success(), "beat generation failed");

        let background = root.join("short-background.mp4");
        let generated_background = std::process::Command::new(ffmpeg)
            .args([
                "-y",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=red:s=64x64:r=2:d=0.5",
                "-f",
                "lavfi",
                "-i",
                "color=c=blue:s=64x64:r=2:d=0.5",
                "-filter_complex",
                "[0:v][1:v]concat=n=2:v=1:a=0[v]",
                "-map",
                "[v]",
                "-an",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(&background)
            .output()
            .unwrap();
        assert!(
            generated_background.status.success(),
            "background generation failed: {}",
            String::from_utf8_lossy(&generated_background.stderr)
        );

        let profile = RenderProfile {
            fps: Fps::Fixed(10.0),
            frame_size_override: Some((320, 240)),
            ..RenderProfile::default()
        };
        let mut inputs = RenderInputs::new("beat.wav", 2.0);
        inputs.background = Some(RenderBackground::Video(background));
        inputs.beat = beat;
        let command = ffmpeg_command(&profile, &inputs).unwrap();
        let output_path = root.join("karaoke.mp4");
        let rendered = std::process::Command::new(&command[0])
            .args(&command[1..])
            .current_dir(root)
            .output()
            .unwrap();
        assert!(
            rendered.status.success(),
            "video background render failed: {}",
            String::from_utf8_lossy(&rendered.stderr)
        );

        let verified = verify_output(&output_path, 2.0, 320, 240).unwrap();
        assert!((verified.duration - 2.0).abs() < 0.1);
        assert_eq!(verified.width, 320);
        assert_eq!(verified.height, 240);
        assert_eq!(verified.audio_codec.as_deref(), Some("aac"));
        for (time, red) in [(0.25, true), (0.75, false), (1.25, true), (1.75, false)] {
            let pixel = render_pixel(&command[0], root, &output_path, time, 320, 10, 10);
            if red {
                assert!(
                    pixel[0] > 180 && pixel[2] < 70,
                    "expected red at {time}s: {pixel:?}"
                );
            } else {
                assert!(
                    pixel[2] > 180 && pixel[0] < 70,
                    "expected blue at {time}s: {pixel:?}"
                );
            }
        }
    }

    #[test]
    fn portrait_and_4k_profiles_render_expected_frame_geometry() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        std::fs::create_dir(root.join("fonts")).unwrap();
        std::fs::write(
            root.join("lyrics.ass"),
            "[Script Info]\nScriptType: v4.00+\nPlayResX: 1920\nPlayResY: 1080\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Main,Arial,20,&H00FFFFFF,&H00FFFFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,1,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n",
        )
        .unwrap();
        let audio = std::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=1",
                "-ar",
                "44100",
            ])
            .arg(root.join("beat.wav"))
            .output()
            .unwrap();
        assert!(audio.status.success(), "beat generation failed");

        for (aspect, resolution, expected_size) in [
            (Aspect::Portrait, Resolution::P1080, (1080, 1920)),
            (Aspect::Landscape, Resolution::K4, (3840, 2160)),
        ] {
            let profile = RenderProfile {
                aspect,
                resolution,
                fps: Fps::Fixed(1.0),
                quality: RenderQuality::Standard,
                ..RenderProfile::default()
            };
            let inputs = RenderInputs::new("beat.wav", 1.0);
            let command = ffmpeg_command(&profile, &inputs).unwrap();
            let rendered = std::process::Command::new(&command[0])
                .args(&command[1..])
                .current_dir(root)
                .output()
                .unwrap();
            assert!(
                rendered.status.success(),
                "profile render failed for {:?} {:?}: {}",
                aspect,
                resolution,
                String::from_utf8_lossy(&rendered.stderr)
            );
            let output = root.join("karaoke.mp4");
            let verified = verify_output(&output, 1.0, expected_size.0, expected_size.1).unwrap();
            assert_eq!((verified.width, verified.height), expected_size);
            let pixel = render_pixel(
                &command[0],
                root,
                &output,
                0.0,
                expected_size.0 as usize,
                expected_size.0 as usize / 2,
                expected_size.1 as usize / 2,
            );
            for (actual, expected) in pixel.into_iter().zip([16, 29, 43]) {
                assert!(
                    actual.abs_diff(expected) <= 10,
                    "profile background pixel differs for {:?} {:?}: {pixel:?}",
                    aspect,
                    resolution
                );
            }
        }
    }
}
