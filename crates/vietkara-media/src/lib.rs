//! Media probing, WAV reading, waveform / audio metrics and a
//! playback-clock abstraction for VietKara Studio.
//!
//! Ports the Python baseline contracts:
//!
//! * `render_info.fps_value` — fraction strings like `"30000/1001"`.
//! * `media_tools.probe_video` — ffprobe JSON first, `ffmpeg -i` stderr
//!   parsing as fallback (imageio-ffmpeg ships ffmpeg but no ffprobe).
//! * `engine.waveform` — peaks per `ceil(frames / bins)` block,
//!   normalised by the largest peak (scale floor `0.001`).
//! * `render_info._audio_metrics` — peak / RMS dBFS and clipped samples
//!   over **all** channel samples.
//! * `engine.convert` — ffmpeg args for a reference WAV
//!   (`-ar 44100 -ac 2 -c:a pcm_s16le`).
//!
//! Deliberate differences from Python:
//!
//! * `probe` never panics on malformed input: every parse failure is a
//!   `MediaError`.
//! * There is **no real audio device here**. `NullPlayer` advances only
//!   via the injected `Clock` (plan §3.5: the playback clock is the
//!   single source of time). A real Windows device player is Phase 1
//!   spike work, not this crate.

use serde::Deserialize;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MediaError {
    #[error("Lỗi đọc/ghi file: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON không hợp lệ: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Không đọc được thông tin media: {0}")]
    ProbeFailed(String),
    #[error("File không có hình ảnh video.")]
    NoVideoStream,
    #[error("File WAV không hợp lệ: {0}")]
    InvalidWav(String),
    #[error("Định dạng WAV chưa hỗ trợ: {0}")]
    UnsupportedWav(String),
    #[error("Tham số không hợp lệ: {0}")]
    InvalidArgument(String),
}

// ---------------------------------------------------------------------------
// fps_value — render_info.fps_value
// ---------------------------------------------------------------------------

/// Parse a frame rate like `"30000/1001"` or `"30"`.
///
/// Matches Python `float(Fraction(str(rate)))`, returning `30.0` when
/// parsing fails or the value is outside `1..=120`.
pub fn fps_value(rate: &str) -> f64 {
    let value = parse_rate(rate);
    match value {
        Some(v) if v.is_finite() && (1.0..=120.0).contains(&v) => v,
        _ => 30.0,
    }
}

fn parse_rate(rate: &str) -> Option<f64> {
    let text = rate.trim();
    if text.is_empty() {
        return None;
    }
    if let Some((num, den)) = text.split_once('/') {
        let numerator: f64 = num.trim().parse().ok()?;
        let denominator: f64 = den.trim().parse().ok()?;
        if denominator == 0.0 {
            return None;
        }
        Some(numerator / denominator)
    } else {
        text.parse::<f64>().ok()
    }
}

// ---------------------------------------------------------------------------
// Video probing — media_tools.probe_video
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct VideoInfo {
    pub duration: f64,
    pub fps: f64,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Deserialize)]
struct FfprobeOutput {
    #[serde(default)]
    streams: Vec<FfprobeStream>,
    #[serde(default)]
    format: Option<FfprobeFormat>,
}

#[derive(Debug, Deserialize)]
struct FfprobeStream {
    #[serde(default)]
    codec_type: Option<String>,
    #[serde(default)]
    width: Option<u32>,
    #[serde(default)]
    height: Option<u32>,
    #[serde(default)]
    avg_frame_rate: Option<String>,
    #[serde(default)]
    r_frame_rate: Option<String>,
    #[serde(default)]
    duration: Option<serde_json::Value>,
    #[serde(default)]
    disposition: Option<FfprobeDisposition>,
}

#[derive(Debug, Deserialize)]
struct FfprobeDisposition {
    #[serde(default)]
    attached_pic: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct FfprobeFormat {
    #[serde(default)]
    duration: Option<serde_json::Value>,
}

fn parse_f64_value(value: &serde_json::Value) -> Option<f64> {
    match value {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

fn video_info_from_ffprobe(data: &FfprobeOutput) -> Result<VideoInfo, MediaError> {
    let video = data
        .streams
        .iter()
        .find(|s| {
            s.codec_type.as_deref() == Some("video")
                && s.disposition
                    .as_ref()
                    .and_then(|d| d.attached_pic)
                    .unwrap_or(0)
                    == 0
        })
        .ok_or(MediaError::NoVideoStream)?;
    let width = video.width.ok_or(MediaError::NoVideoStream)?;
    let height = video.height.ok_or(MediaError::NoVideoStream)?;
    let duration = data
        .format
        .as_ref()
        .and_then(|f| f.duration.as_ref())
        .and_then(parse_f64_value)
        .or_else(|| video.duration.as_ref().and_then(parse_f64_value))
        .unwrap_or(0.0);
    // Python: `avg_frame_rate or r_frame_rate` — a present (even "0/0")
    // avg rate wins over r_frame_rate.
    let rate = video
        .avg_frame_rate
        .as_deref()
        .filter(|s| !s.is_empty())
        .or(video.r_frame_rate.as_deref())
        .unwrap_or("30");
    Ok(VideoInfo {
        duration,
        fps: fps_value(rate),
        width,
        height,
    })
}

/// Run ffprobe (`-show_streams -show_format -of json`) and pick the
/// first video stream that is not an attached picture.
pub fn probe_video(path: &Path, ffprobe_path: &Path) -> Result<VideoInfo, MediaError> {
    let output = Command::new(ffprobe_path)
        .args([
            "-v",
            "error",
            "-show_streams",
            "-show_format",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .map_err(|e| MediaError::ProbeFailed(format!("Không chạy được ffprobe: {e}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(MediaError::ProbeFailed(format!(
            "ffprobe báo lỗi: {}",
            stderr.chars().take(500).collect::<String>()
        )));
    }
    let data: FfprobeOutput = serde_json::from_slice(&output.stdout)
        .map_err(|e| MediaError::ProbeFailed(format!("Không đọc được kết quả ffprobe: {e}")))?;
    video_info_from_ffprobe(&data)
}

fn parse_hms(token: &str) -> Option<f64> {
    let mut parts = token.split(':');
    let hours: f64 = parts.next()?.trim().parse().ok()?;
    let minutes: f64 = parts.next()?.trim().parse().ok()?;
    let seconds: f64 = parts.next()?.trim().parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(hours * 3600.0 + minutes * 60.0 + seconds)
}

fn find_dimensions(line: &str) -> Option<(u32, u32, usize)> {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'x' && i > 0 && i + 1 < bytes.len() {
            let mut start = i;
            while start > 0 && bytes[start - 1].is_ascii_digit() {
                start -= 1;
            }
            let mut end = i + 1;
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            if start < i && end > i + 1 {
                // Like Python's `(\d+)x(\d+)(?:\s|\[)`: the height must
                // be followed by whitespace or '['. Without this,
                // "0x31637661" in a codec id parses as 0x31637661.
                let after = bytes.get(end).copied();
                if matches!(after, None | Some(b' ') | Some(b'\t') | Some(b'[')) {
                    let width: u32 = line[start..i].parse().ok()?;
                    let height: u32 = line[i + 1..end].parse().ok()?;
                    return Some((width, height, end));
                }
            }
        }
        i += 1;
    }
    None
}

/// Parse `ffmpeg -hide_banner -i <path>` stderr, as Python does when
/// ffprobe is unavailable. Duration is optional (0.0 when absent);
/// fps defaults to 30.0 when the stream line has no `… fps`.
pub fn parse_ffmpeg_stderr(text: &str) -> Result<VideoInfo, MediaError> {
    let mut duration = 0.0;
    for line in text.lines() {
        if let Some(rest) = line.trim_start().strip_prefix("Duration:") {
            let token = rest.trim().split([' ', ',']).next().unwrap_or("");
            if let Some(value) = parse_hms(token) {
                duration = value;
            }
            break;
        }
    }
    for line in text.lines() {
        if !line.contains("Video:") {
            continue;
        }
        let Some((width, height, dim_end)) = find_dimensions(line) else {
            continue;
        };
        let after = &line[dim_end..];
        let fps = after
            .find(" fps")
            .and_then(|idx| {
                let before = &after[..idx];
                let token: String = before
                    .trim_end()
                    .chars()
                    .rev()
                    .take_while(|c| c.is_ascii_digit() || *c == '.')
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                token.parse::<f64>().ok()
            })
            .unwrap_or(30.0);
        return Ok(VideoInfo {
            duration,
            fps,
            width,
            height,
        });
    }
    Err(MediaError::NoVideoStream)
}

/// Fallback probe using `ffmpeg -i`, reading the human-readable summary.
pub fn probe_with_ffmpeg(path: &Path, ffmpeg_path: &Path) -> Result<VideoInfo, MediaError> {
    let output = Command::new(ffmpeg_path)
        .args(["-hide_banner", "-i"])
        .arg(path)
        .output()
        .map_err(|e| MediaError::ProbeFailed(format!("Không chạy được ffmpeg: {e}")))?;
    // ffmpeg -i with no output file exits non-zero; the summary is on
    // stderr either way, so parse it regardless of the exit code.
    let stderr = String::from_utf8_lossy(&output.stderr);
    parse_ffmpeg_stderr(&stderr)
}

/// Probe a video file: ffprobe first, then the `ffmpeg -i` fallback.
pub fn probe(path: &Path) -> Result<VideoInfo, MediaError> {
    probe_with(path, Path::new("ffprobe"), Path::new("ffmpeg"))
}

pub fn probe_with(
    path: &Path,
    ffprobe_path: &Path,
    ffmpeg_path: &Path,
) -> Result<VideoInfo, MediaError> {
    match probe_video(path, ffprobe_path) {
        Ok(info) => Ok(info),
        Err(first) => probe_with_ffmpeg(path, ffmpeg_path).map_err(|_| first),
    }
}

/// Locate an ffmpeg/ffprobe binary: `OBS_FFMPEG` / `OBS_FFPROBE` first
/// (as in Python `engine.binary`), then the system PATH.
pub fn find_binary(name: &str) -> Result<PathBuf, MediaError> {
    let env_name = format!("OBS_{}", name.to_ascii_uppercase());
    if let Ok(explicit) = std::env::var(&env_name) {
        let candidate = PathBuf::from(&explicit);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Ok(candidate);
            }
            #[cfg(windows)]
            {
                let candidate = dir.join(format!("{name}.exe"));
                if candidate.is_file() {
                    return Ok(candidate);
                }
            }
        }
    }
    Err(MediaError::ProbeFailed(format!(
        "Chưa tìm thấy {name}. Đặt {name}.exe trong thư mục bin của app hoặc PATH."
    )))
}

// ---------------------------------------------------------------------------
// ffmpeg convert — engine.convert
// ---------------------------------------------------------------------------

/// ffmpeg arguments matching Python `engine.convert`: resample to
/// 44.1 kHz stereo PCM16 WAV, dropping video.
pub fn reference_wav_args(ffmpeg: &str, source: &Path, destination: &Path) -> Vec<String> {
    vec![
        ffmpeg.to_owned(),
        "-y".into(),
        "-v".into(),
        "error".into(),
        "-i".into(),
        source.to_string_lossy().into_owned(),
        "-vn".into(),
        "-ar".into(),
        "44100".into(),
        "-ac".into(),
        "2".into(),
        "-c:a".into(),
        "pcm_s16le".into(),
        destination.to_string_lossy().into_owned(),
    ]
}

/// Run the `engine.convert` ffmpeg command for a source file.
pub fn convert_to_reference_wav(
    source: &Path,
    destination: &Path,
    ffmpeg_path: &Path,
) -> Result<(), MediaError> {
    let args = reference_wav_args(&ffmpeg_path.to_string_lossy(), source, destination);
    let output = Command::new(&args[0])
        .args(&args[1..])
        .output()
        .map_err(|e| MediaError::ProbeFailed(format!("Không chạy được ffmpeg: {e}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(MediaError::ProbeFailed(format!(
            "ffmpeg convert báo lỗi: {}",
            stderr.chars().take(500).collect::<String>()
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// WAV reader
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct AudioInfo {
    pub sample_rate: u32,
    pub channels: u16,
    pub frames: u64,
    pub duration: f64,
}

#[derive(Debug, Clone)]
pub struct WavData {
    pub info: AudioInfo,
    /// Mono mix: mean of the channels per frame.
    pub samples: Vec<f32>,
    /// All channel samples, interleaved (used by `audio_metrics`).
    pub interleaved: Vec<f32>,
}

impl WavData {
    pub fn mono(&self) -> &[f32] {
        &self.samples
    }
}

fn read_u16_le(bytes: &[u8]) -> u16 {
    u16::from_le_bytes([bytes[0], bytes[1]])
}

fn read_u32_le(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn decode_frame(
    data: &[u8],
    format: u16,
    bits: u16,
    channels: usize,
) -> Result<Vec<f32>, MediaError> {
    let bytes_per_sample = match bits {
        16 | 24 | 32 => (bits / 8) as usize,
        _ => {
            return Err(MediaError::UnsupportedWav(format!(
                "độ sâu {bits} bit chưa hỗ trợ"
            )));
        }
    };
    let frame_bytes = bytes_per_sample * channels;
    if frame_bytes == 0 || !data.len().is_multiple_of(frame_bytes) {
        return Err(MediaError::InvalidWav(
            "dữ liệu WAV không khớp block align".into(),
        ));
    }
    let mut out = Vec::with_capacity(data.len() / bytes_per_sample);
    for frame in data.chunks_exact(frame_bytes) {
        for channel in 0..channels {
            let offset = channel * bytes_per_sample;
            let sample = &frame[offset..offset + bytes_per_sample];
            let value = match (format, bits) {
                (1, 16) => i16::from_le_bytes([sample[0], sample[1]]) as f32 / 32768.0,
                (1, 24) => {
                    let raw =
                        (sample[0] as i32) | ((sample[1] as i32) << 8) | ((sample[2] as i32) << 16);
                    let signed = if raw & 0x800000 != 0 {
                        raw - 0x1000000
                    } else {
                        raw
                    };
                    signed as f32 / 8_388_608.0
                }
                (1, 32) => {
                    i32::from_le_bytes([sample[0], sample[1], sample[2], sample[3]]) as f32
                        / 2_147_483_648.0
                }
                (3, 32) => f32::from_le_bytes([sample[0], sample[1], sample[2], sample[3]]),
                _ => {
                    return Err(MediaError::UnsupportedWav(format!(
                        "định dạng {format} / {bits} bit chưa hỗ trợ"
                    )));
                }
            };
            out.push(value);
        }
    }
    Ok(out)
}

/// Read a WAV file: PCM16, PCM24, PCM32-int and float32, mono/stereo
/// (any channel count), skipping unknown chunks (with odd-size padding).
pub fn read_wav(path: &Path) -> Result<WavData, MediaError> {
    let mut file = std::fs::File::open(path)?;
    // Bound chunk allocations by the real file size, so a corrupt
    // header claiming a multi-gigabyte chunk cannot OOM the reader.
    let file_len = file.metadata().map(|m| m.len()).unwrap_or(u64::MAX);
    let mut header = [0u8; 12];
    file.read_exact(&mut header)
        .map_err(|_| MediaError::InvalidWav("file quá ngắn, không phải WAV".into()))?;
    if &header[0..4] != b"RIFF" || &header[8..12] != b"WAVE" {
        return Err(MediaError::InvalidWav("không có header RIFF/WAVE".into()));
    }
    let mut format: Option<u16> = None;
    let mut bits: u16 = 0;
    let mut channels: u16 = 0;
    let mut sample_rate: u32 = 0;
    let mut block_align: u16 = 0;
    let mut data: Vec<u8> = Vec::new();
    loop {
        let mut chunk_header = [0u8; 8];
        match file.read_exact(&mut chunk_header) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.into()),
        }
        let chunk_id = &chunk_header[0..4];
        let chunk_size = read_u32_le(&chunk_header[4..8]) as u64;
        if chunk_id == b"fmt " {
            if !(16..=1_048_576).contains(&chunk_size) || chunk_size > file_len {
                return Err(MediaError::InvalidWav("chunk fmt không hợp lệ".into()));
            }
            let mut fmt = vec![0u8; chunk_size as usize];
            file.read_exact(&mut fmt)?;
            let raw_format = read_u16_le(&fmt[0..2]);
            channels = read_u16_le(&fmt[2..4]);
            sample_rate = read_u32_le(&fmt[4..8]);
            block_align = read_u16_le(&fmt[12..14]);
            bits = read_u16_le(&fmt[14..16]);
            format = Some(match raw_format {
                // WAVE_FORMAT_EXTENSIBLE: the real format is the first
                // two bytes of the SubFormat GUID at offset 24.
                0xFFFE if fmt.len() >= 26 => read_u16_le(&fmt[24..26]),
                other => other,
            });
            if chunk_size % 2 == 1 {
                file.seek(SeekFrom::Current(1))?;
            }
        } else if chunk_id == b"data" {
            if chunk_size > file_len {
                return Err(MediaError::InvalidWav("chunk data bị cắt ngắn".into()));
            }
            let mut buf = vec![0u8; chunk_size as usize];
            file.read_exact(&mut buf)
                .map_err(|_| MediaError::InvalidWav("chunk data bị cắt ngắn".into()))?;
            data = buf;
            if chunk_size % 2 == 1 {
                let _ = file.seek(SeekFrom::Current(1));
            }
            break;
        } else {
            // Unknown chunk: skip it (plus padding byte if odd-sized).
            let skip = chunk_size + chunk_size % 2;
            file.seek(SeekFrom::Current(skip as i64))?;
        }
    }
    let format = format.ok_or_else(|| MediaError::InvalidWav("thiếu chunk fmt".into()))?;
    if channels == 0 || sample_rate == 0 {
        return Err(MediaError::InvalidWav(
            "số kênh hoặc sample rate bằng 0".into(),
        ));
    }
    let expected_align = channels as usize * (bits as usize).div_ceil(8);
    if block_align as usize != expected_align && block_align != 0 {
        // Some writers report a padded block_align; trust the fmt fields
        // and decode by channel count instead.
    }
    if data.is_empty() {
        return Err(MediaError::InvalidWav("thiếu chunk data".into()));
    }
    let interleaved = decode_frame(&data, format, bits, channels as usize)?;
    let frames = (interleaved.len() / channels as usize) as u64;
    let mut samples = Vec::with_capacity(frames as usize);
    for frame in interleaved.chunks_exact(channels as usize) {
        let sum: f32 = frame.iter().sum();
        samples.push(sum / channels as f32);
    }
    Ok(WavData {
        info: AudioInfo {
            sample_rate,
            channels,
            frames,
            duration: frames as f64 / sample_rate as f64,
        },
        samples,
        interleaved,
    })
}

// ---------------------------------------------------------------------------
// Waveform — engine.waveform
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Waveform {
    pub duration: f64,
    pub sample_rate: u32,
    /// Peaks normalised so the largest is 1.0 (Python scale floor 0.001).
    pub peaks: Vec<f32>,
}

/// Peaks per `ceil(frames / bins)` block, rounded to 4 decimals like
/// Python, then divided by the largest peak. Like Python
/// (`np.max(np.abs(samples))` over the 2-D block), the peak is the max
/// absolute sample across **all channels** — a mono mix would cancel
/// out-of-phase stereo and under-report the peak.
pub fn waveform(path: &Path, bins: usize) -> Result<Waveform, MediaError> {
    let bins = bins.max(1);
    let wav = read_wav(path)?;
    let frames = wav.info.frames as usize;
    if frames == 0 {
        return Ok(Waveform {
            duration: 0.0,
            sample_rate: wav.info.sample_rate,
            peaks: Vec::new(),
        });
    }
    let channels = wav.info.channels.max(1) as usize;
    let step = frames.div_ceil(bins).max(1);
    let mut raw: Vec<f32> = Vec::new();
    for block_start in (0..frames).step_by(step) {
        let block_end = (block_start + step).min(frames);
        let peak = wav.interleaved[block_start * channels..block_end * channels]
            .iter()
            .fold(0.0f32, |acc, s| acc.max(s.abs()));
        raw.push((peak * 10_000.0).round() / 10_000.0);
    }
    let scale = raw.iter().fold(0.0f32, |acc, p| acc.max(*p)).max(0.001);
    let peaks: Vec<f32> = raw.iter().map(|p| p / scale).collect();
    Ok(Waveform {
        duration: (wav.info.duration * 10_000.0).round() / 10_000.0,
        sample_rate: wav.info.sample_rate,
        peaks,
    })
}

// ---------------------------------------------------------------------------
// Audio metrics — render_info._audio_metrics
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct AudioMetrics {
    pub peak_dbfs: f64,
    pub rms_dbfs: f64,
    pub clipped_samples: u64,
}

fn dbfs(value: f64) -> f64 {
    (20.0 * value.max(1e-12).log10() * 10.0).round() / 10.0
}

/// Peak / RMS dBFS and clipped-sample count over all channel samples.
pub fn audio_metrics(path: &Path) -> Result<AudioMetrics, MediaError> {
    let wav = read_wav(path)?;
    let mut peak: f64 = 0.0;
    let mut squares: f64 = 0.0;
    let mut clipped: u64 = 0;
    for sample in &wav.interleaved {
        let value = *sample as f64;
        peak = peak.max(value.abs());
        squares += value * value;
        if value.abs() >= 0.9999 {
            clipped += 1;
        }
    }
    let count = wav.interleaved.len().max(1) as f64;
    let rms = (squares / count).sqrt();
    Ok(AudioMetrics {
        peak_dbfs: dbfs(peak),
        rms_dbfs: dbfs(rms),
        clipped_samples: clipped,
    })
}

// ---------------------------------------------------------------------------
// Playback clock / player — plan §3.5
// ---------------------------------------------------------------------------

/// The single source of time. UI and timeline read snapshots from a
/// `Clock`; they never run their own clock from render frames.
pub trait Clock: Send + Sync {
    fn now(&self) -> f64;
}

/// A clock advanced manually by tests / the app — no audio device, no
/// wall-clock drift.
#[derive(Debug, Default)]
pub struct FakeClock {
    time: Mutex<f64>,
}

impl FakeClock {
    pub fn new() -> Self {
        Self::with_start(0.0)
    }

    pub fn with_start(start: f64) -> Self {
        Self {
            time: Mutex::new(start.max(0.0)),
        }
    }

    pub fn advance(&self, seconds: f64) {
        if let Ok(mut time) = self.time.lock() {
            *time = (*time + seconds).max(0.0);
        }
    }

    pub fn set(&self, seconds: f64) {
        if let Ok(mut time) = self.time.lock() {
            *time = seconds.max(0.0);
        }
    }
}

impl Clock for FakeClock {
    fn now(&self) -> f64 {
        self.time
            .lock()
            .map(|t| *t)
            .unwrap_or_else(|e| *e.into_inner())
    }
}

impl Clock for Arc<FakeClock> {
    fn now(&self) -> f64 {
        (**self).now()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerState {
    Stopped,
    Playing,
    Paused,
}

pub trait Player {
    fn play(&mut self);
    fn pause(&mut self);
    fn stop(&mut self);
    fn seek(&mut self, seconds: f64);
    fn position(&self) -> f64;
    fn duration(&self) -> f64;
    fn state(&self) -> PlayerState;
}

/// A player with no audio device: position advances only when the
/// injected clock advances. A real Windows device player (WASAPI /
/// DirectSound) is Phase 1 spike work and lives outside this crate.
pub struct NullPlayer<C: Clock> {
    clock: C,
    duration: f64,
    state: PlayerState,
    anchor_position: f64,
    anchor_time: f64,
}

impl<C: Clock> NullPlayer<C> {
    pub fn new(clock: C, duration: f64) -> Self {
        Self {
            clock,
            duration: duration.max(0.0),
            state: PlayerState::Stopped,
            anchor_position: 0.0,
            anchor_time: 0.0,
        }
    }

    fn elapsed_position(&self) -> f64 {
        let raw = if self.state == PlayerState::Playing {
            self.anchor_position + (self.clock.now() - self.anchor_time)
        } else {
            self.anchor_position
        };
        raw.clamp(0.0, self.duration)
    }
}

impl<C: Clock> Player for NullPlayer<C> {
    fn play(&mut self) {
        if self.duration <= 0.0 {
            return;
        }
        if self.elapsed_position() >= self.duration {
            self.anchor_position = self.duration;
            self.state = PlayerState::Stopped;
            return;
        }
        if self.state == PlayerState::Playing {
            return;
        }
        self.anchor_position = self.elapsed_position();
        self.anchor_time = self.clock.now();
        self.state = PlayerState::Playing;
    }

    fn pause(&mut self) {
        if self.state == PlayerState::Playing {
            self.anchor_position = self.elapsed_position();
            self.state = PlayerState::Paused;
        }
    }

    fn stop(&mut self) {
        self.anchor_position = 0.0;
        self.state = PlayerState::Stopped;
    }

    fn seek(&mut self, seconds: f64) {
        self.anchor_position = seconds.clamp(0.0, self.duration);
        if self.state == PlayerState::Playing {
            self.anchor_time = self.clock.now();
        }
    }

    fn position(&self) -> f64 {
        self.elapsed_position()
    }

    fn duration(&self) -> f64 {
        self.duration
    }

    fn state(&self) -> PlayerState {
        if self.state == PlayerState::Playing
            && self.duration > 0.0
            && self.elapsed_position() >= self.duration
        {
            PlayerState::Stopped
        } else {
            self.state
        }
    }
}

/// Convenience constructor for the common test/app shape.
pub fn null_player_with_clock(clock: Arc<FakeClock>, duration: f64) -> NullPlayer<Arc<FakeClock>> {
    NullPlayer::new(clock, duration)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn approx(value: f64, expected: f64, epsilon: f64) -> bool {
        (value - expected).abs() <= epsilon
    }

    // -- fps_value ---------------------------------------------------------

    #[test]
    fn fps_value_cases() {
        assert!(approx(fps_value("30000/1001"), 29.970_029_97, 1e-6));
        assert_eq!(fps_value("30"), 30.0);
        assert_eq!(fps_value("29.97"), 29.97);
        assert_eq!(fps_value("25/1"), 25.0);
        assert_eq!(fps_value("120"), 120.0);
        assert_eq!(fps_value("121"), 30.0);
        assert_eq!(fps_value("0"), 30.0);
        assert_eq!(fps_value("0/1"), 30.0);
        assert_eq!(fps_value("1/0"), 30.0);
        assert_eq!(fps_value("0/0"), 30.0);
        assert_eq!(fps_value("garbage"), 30.0);
        assert_eq!(fps_value(""), 30.0);
        assert_eq!(fps_value(" 30 "), 30.0);
        assert_eq!(fps_value("-5"), 30.0);
    }

    // -- ffmpeg -i stderr parser -------------------------------------------

    const FFMPEG_SAMPLE: &str = "ffmpeg version 8.1.2 Copyright (c) 2000-2026\nInput #0, mov,mp4,m4a,3gp,3g2,mj2, from 'clip.mp4':\n  Metadata:\n    major_brand     : isom\n  Duration: 00:00:01.04, start: 0.000000, bitrate: 123 kb/s\n  Stream #0:0(und): Video: h264 (High) (avc1 / 0x31637661), yuv420p, 320x240 [SAR 1:1 DAR 4:3], 30 fps, 30 tbr, 15360 tbn (default)\n  Stream #0:1(und): Audio: aac (LC) (mp4a / 0x6134706D), 44100 Hz, stereo, fltp, 128 kb/s (default)\n";

    #[test]
    fn fallback_parser_reads_duration_dimensions_and_fps() {
        let info = parse_ffmpeg_stderr(FFMPEG_SAMPLE).unwrap();
        assert_eq!(info.width, 320);
        assert_eq!(info.height, 240);
        assert_eq!(info.fps, 30.0);
        assert!(approx(info.duration, 1.04, 1e-9));
    }

    #[test]
    fn fallback_parser_defaults_fps_when_missing() {
        let text = "  Duration: 00:01:02.50, start: 0.000000, bitrate: 1 kb/s\n  Stream #0:0: Video: vp9, yuv420p, 640x360 [SAR 1:1 DAR 16:9], 25 tbr\n";
        let info = parse_ffmpeg_stderr(text).unwrap();
        assert_eq!(info.width, 640);
        assert_eq!(info.height, 360);
        assert_eq!(info.fps, 30.0);
        assert!(approx(info.duration, 62.5, 1e-9));
    }

    #[test]
    fn fallback_parser_rejects_audio_only() {
        let text = "  Duration: 00:00:01.00, start: 0.000000\n  Stream #0:0: Audio: aac, 44100 Hz, stereo\n";
        assert!(matches!(
            parse_ffmpeg_stderr(text),
            Err(MediaError::NoVideoStream)
        ));
    }

    // -- fixture generation --------------------------------------------------

    fn run_ffmpeg(args: &[&str]) {
        let status = Command::new("ffmpeg")
            .args(["-y"])
            .args(args)
            .output()
            .expect("ffmpeg must be installed for fixture generation");
        assert!(
            status.status.success(),
            "ffmpeg {:?} failed: {}",
            args,
            String::from_utf8_lossy(&status.stderr)
        );
    }

    fn make_fixture_video(dir: &Path) -> PathBuf {
        let path = dir.join("clip.mp4");
        run_ffmpeg(&[
            "-f",
            "lavfi",
            "-i",
            "testsrc=duration=1:size=320x240:rate=30",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=1",
            "-shortest",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            path.to_str().unwrap(),
        ]);
        path
    }

    #[test]
    fn probe_real_video_via_ffprobe_and_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let video = make_fixture_video(dir.path());
        let info = probe_video(&video, Path::new("ffprobe")).unwrap();
        assert_eq!(info.width, 320);
        assert_eq!(info.height, 240);
        assert!(approx(info.fps, 30.0, 0.01));
        assert!(approx(info.duration, 1.0, 0.15));
        // Same file through the ffmpeg -i fallback path.
        let fallback = probe_with_ffmpeg(&video, Path::new("ffmpeg")).unwrap();
        assert_eq!(fallback.width, 320);
        assert_eq!(fallback.height, 240);
        assert_eq!(fallback.fps, 30.0);
        assert!(approx(fallback.duration, 1.0, 0.15));
        // probe() picks ffprobe first.
        let probed = probe(&video).unwrap();
        assert_eq!(probed.width, 320);
    }

    #[test]
    fn probe_missing_file_errors_without_panic() {
        let missing = Path::new("/nonexistent/nope.mp4");
        assert!(probe(missing).is_err());
        assert!(probe_video(missing, Path::new("ffprobe")).is_err());
    }

    // -- WAV fixtures ----------------------------------------------------------

    fn write_wav_pcm16(path: &Path, samples: &[i16], sample_rate: u32, channels: u16) {
        let mut bytes = Vec::new();
        let data_len = (samples.len() * 2) as u32;
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVE");
        bytes.extend_from_slice(b"fmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&channels.to_le_bytes());
        bytes.extend_from_slice(&sample_rate.to_le_bytes());
        bytes.extend_from_slice(&(sample_rate * channels as u32 * 2).to_le_bytes());
        bytes.extend_from_slice(&(channels * 2).to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        std::fs::write(path, bytes).unwrap();
    }

    fn sine_pcm16(amplitude: f32, seconds: f32, sample_rate: u32) -> Vec<i16> {
        let count = (seconds * sample_rate as f32) as usize;
        (0..count)
            .map(|i| {
                let t = i as f32 / sample_rate as f32;
                ((2.0 * std::f32::consts::PI * 440.0 * t).sin() * amplitude * 32767.0) as i16
            })
            .collect()
    }

    #[test]
    fn wav_reader_waveform_and_metrics_on_sine() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sine.wav");
        write_wav_pcm16(&path, &sine_pcm16(0.5, 1.0, 44100), 44100, 1);
        let wav = read_wav(&path).unwrap();
        assert_eq!(wav.info.sample_rate, 44100);
        assert_eq!(wav.info.channels, 1);
        assert_eq!(wav.info.frames, 44100);
        assert!(approx(wav.info.duration, 1.0, 1e-9));
        assert_eq!(wav.samples.len(), 44100);

        let wave = waveform(&path, 1800).unwrap();
        assert_eq!(wave.sample_rate, 44100);
        assert!(approx(wave.duration, 1.0, 1e-3));
        assert!(!wave.peaks.is_empty());
        assert!(wave.peaks.len() <= 1800);
        // Normalised: the largest peak is 1.0.
        let max_peak = wave.peaks.iter().fold(0.0f32, |a, p| a.max(*p));
        assert!(approx(max_peak as f64, 1.0, 1e-3));

        let metrics = audio_metrics(&path).unwrap();
        // Sine amplitude 0.5: peak -6.0 dBFS, RMS 0.5/sqrt(2) ≈ -9.0 dBFS.
        assert!(approx(metrics.peak_dbfs, -6.0, 0.3));
        assert!(approx(metrics.rms_dbfs, -9.0, 0.3));
        assert_eq!(metrics.clipped_samples, 0);
    }

    #[test]
    fn waveform_uses_max_across_channels_not_mono_mix() {
        // Out-of-phase stereo: L = +0.5, R = -0.5 in every frame.
        // A mono mix cancels to 0, but Python's max-abs peak is 0.5.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out-of-phase.wav");
        let samples: Vec<i16> = (0..8000).flat_map(|_| [16384i16, -16384i16]).collect();
        write_wav_pcm16(&path, &samples, 8000, 2);
        let wave = waveform(&path, 10).unwrap();
        let max_peak = wave.peaks.iter().fold(0.0f32, |a, p| a.max(*p));
        assert!(approx(max_peak as f64, 1.0, 1e-3));
    }

    #[test]
    fn wav_reader_skips_unknown_chunks() {
        let dir = tempfile::tempdir().unwrap();
        let plain = dir.path().join("plain.wav");
        write_wav_pcm16(&plain, &[1000, -1000, 2000, -2000], 8000, 1);
        let mut bytes = std::fs::read(&plain).unwrap();
        let data_at = bytes
            .windows(4)
            .position(|w| w == b"data")
            .expect("data chunk");
        // Odd-sized JUNK chunk (3 payload bytes + 1 pad byte) before data.
        let mut junk: Vec<u8> = Vec::new();
        junk.extend_from_slice(b"JUNK");
        junk.extend_from_slice(&3u32.to_le_bytes());
        junk.extend_from_slice(b"xyz");
        junk.push(0);
        bytes.splice(data_at..data_at, junk);
        // Fix the RIFF size field (+12 bytes inserted).
        let riff_size = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) + 12;
        bytes[4..8].copy_from_slice(&riff_size.to_le_bytes());
        let path = dir.path().join("junk.wav");
        std::fs::write(&path, bytes).unwrap();
        let wav = read_wav(&path).unwrap();
        assert_eq!(wav.info.frames, 4);
        assert_eq!(wav.interleaved.len(), 4);
    }

    #[test]
    fn wav_reader_clipped_and_float_and_unknown_chunks() {
        let dir = tempfile::tempdir().unwrap();
        // Full-scale samples are clipped.
        let path = dir.path().join("clip.wav");
        write_wav_pcm16(&path, &[32767, -32768, 0, 32767], 8000, 1);
        let metrics = audio_metrics(&path).unwrap();
        assert_eq!(metrics.clipped_samples, 3);
        assert!(approx(metrics.peak_dbfs, 0.0, 0.1));

        // Float32 WAV via ffmpeg.
        let float_path = dir.path().join("float.wav");
        run_ffmpeg(&[
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=0.5",
            "-c:a",
            "pcm_f32le",
            "-ar",
            "8000",
            float_path.to_str().unwrap(),
        ]);
        let wav = read_wav(&float_path).unwrap();
        assert_eq!(wav.info.sample_rate, 8000);
        assert_eq!(wav.info.channels, 1);
        assert!(approx(wav.info.duration, 0.5, 0.05));
        assert!(waveform(&float_path, 10).unwrap().peaks.len() <= 10);
    }

    #[test]
    fn wav_reader_rejects_malformed_input() {
        let dir = tempfile::tempdir().unwrap();
        let empty = dir.path().join("empty.wav");
        std::fs::write(&empty, b"").unwrap();
        assert!(read_wav(&empty).is_err());
        let text = dir.path().join("text.wav");
        std::fs::write(&text, b"hello, not a wav file at all").unwrap();
        assert!(read_wav(&text).is_err());
        let truncated = dir.path().join("trunc.wav");
        std::fs::write(&truncated, b"RIFF\x24\x00\x00\x00WAVEfmt ").unwrap();
        assert!(read_wav(&truncated).is_err());
    }

    #[test]
    fn convert_args_match_python_engine_convert() {
        let args = reference_wav_args("ffmpeg", Path::new("in.mp4"), Path::new("out.wav"));
        assert_eq!(
            args,
            vec![
                "ffmpeg",
                "-y",
                "-v",
                "error",
                "-i",
                "in.mp4",
                "-vn",
                "-ar",
                "44100",
                "-ac",
                "2",
                "-c:a",
                "pcm_s16le",
                "out.wav"
            ]
        );
        let dir = tempfile::tempdir().unwrap();
        let video = make_fixture_video(dir.path());
        let out = dir.path().join("reference.wav");
        convert_to_reference_wav(&video, &out, Path::new("ffmpeg")).unwrap();
        let wav = read_wav(&out).unwrap();
        assert_eq!(wav.info.sample_rate, 44100);
        assert_eq!(wav.info.channels, 2);
    }

    // -- Playback clock / NullPlayer -------------------------------------------

    #[test]
    fn fake_clock_and_null_player() {
        let clock = Arc::new(FakeClock::new());
        let mut player = null_player_with_clock(Arc::clone(&clock), 10.0);
        assert_eq!(player.state(), PlayerState::Stopped);
        assert_eq!(player.position(), 0.0);
        assert_eq!(player.duration(), 10.0);

        player.play();
        assert_eq!(player.state(), PlayerState::Playing);
        clock.advance(2.5);
        assert!(approx(player.position(), 2.5, 1e-9));

        player.pause();
        assert_eq!(player.state(), PlayerState::Paused);
        clock.advance(5.0);
        assert!(approx(player.position(), 2.5, 1e-9));

        player.play();
        clock.advance(1.0);
        assert!(approx(player.position(), 3.5, 1e-9));

        player.seek(9.0);
        assert!(approx(player.position(), 9.0, 1e-9));
        player.seek(99.0);
        assert!(approx(player.position(), 10.0, 1e-9));
        player.seek(-5.0);
        assert!(approx(player.position(), 0.0, 1e-9));

        // Playing past the end stops at the end and stays there.
        player.seek(9.5);
        player.play();
        clock.advance(2.0);
        assert!(approx(player.position(), 10.0, 1e-9));
        assert_eq!(player.state(), PlayerState::Stopped);
        player.play();
        assert_eq!(player.state(), PlayerState::Stopped);
        assert!(approx(player.position(), 10.0, 1e-9));

        player.stop();
        assert_eq!(player.state(), PlayerState::Stopped);
        assert_eq!(player.position(), 0.0);
    }

    #[test]
    fn null_player_zero_duration_never_plays() {
        let clock = Arc::new(FakeClock::new());
        let mut player = null_player_with_clock(clock, 0.0);
        player.play();
        assert_eq!(player.state(), PlayerState::Stopped);
        assert_eq!(player.position(), 0.0);
    }
}
