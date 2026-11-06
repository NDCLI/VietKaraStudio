//! AI bridge: orchestration for VietKara Studio's AI worker processes.
//!
//! This crate does **not** contain any AI model. It only speaks the
//! worker protocol from `RUST_REWRITE_PLAN.md` §3.4 / Phase 5 to a
//! separate worker process:
//!
//! * the worker runs as a separate process, started with `Worker::spawn`;
//! * the request is sent as **one JSON line on stdin**, then stdin is
//!   closed; the worker answers with **JSON Lines on stdout**;
//! * **stdout is only for protocol**, **stderr is only for logs** —
//!   stderr is collected separately and only surfaces (truncated, like
//!   Python's 4500-char job error tail in `engine.command`) when the
//!   worker fails, times out, or exits without a result;
//! * every message carries `protocol_version` (currently 1) and the
//!   `request_id` of the request it answers.
//!
//! Wire format — request (one line):
//!
//! ```json
//! {"protocol_version":1,"request_id":"<32 hex>","kind":"align",
//!  "project_id":"<32 hex>","input":{"vocals":".../vocals.wav"},
//!  "options":{"model":"small","language":"vi"}}
//! ```
//!
//! Wire format — responses (one JSON object per line):
//!
//! ```json
//! {"protocol_version":1,"request_id":"<32 hex>","type":"progress","progress":50.0,"message":"...”}
//! {"protocol_version":1,"request_id":"<32 hex>","type":"result","result":{"words":[...]}}
//! {"protocol_version":1,"request_id":"<32 hex>","type":"error","error":"..."}
//! ```
//!
//! `input` carries **paths and metadata only, never audio bytes** —
//! media is passed by path (as in Python `worker.run`, which reads
//! `request["vocals"]` from disk), so large audio never crosses the
//! protocol. `options` carries model/language/region settings.
//!
//! The Python entry point `ai_worker.py` adapts this protocol to the
//! existing Demucs, Whisper, and role-analysis functions. The legacy
//! desktop entry (`worker.py`, driven via `desktop_entry.py
//! --vcl-task speech`) keeps its request-file and `OBS_PROGRESS` contract
//! for the current application. Fake-worker tests exercise bridge
//! lifecycle behavior; a Rust integration test also launches the Python
//! adapter without requiring an AI model.

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use thiserror::Error;
use vietkara_domain::new_id;

/// Current worker protocol version. Bump only with a matching worker.
pub const PROTOCOL_VERSION: u32 = 1;

/// Maximum stderr characters kept in error reports, matching the
/// 4500-char truncation Python applies to job errors (`engine.command`).
pub const MAX_STDERR_TAIL_CHARS: usize = 4500;

/// How long `Worker::run` waits for a child to exit on its own after it
/// has sent a terminal (result/error) message, before killing it.
const TERMINAL_EXIT_GRACE: Duration = Duration::from_secs(2);

/// How often the progress/timeout loop wakes up to re-check cancellation.
const POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Error)]
pub enum AiError {
    #[error("Không đọc/ghi được dữ liệu worker: {0}")]
    Io(#[from] std::io::Error),
    #[error("Dòng JSON từ worker không hợp lệ: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Worker dùng protocol_version không hỗ trợ (nhận {found:?}, cần {expected})")]
    WrongProtocolVersion { expected: u32, found: Option<u32> },
    #[error("Worker trả request_id không khớp (nhận {found}, cần {expected})")]
    RequestIdMismatch { expected: String, found: String },
    #[error("Worker trả loại message không biết: {0}")]
    UnknownMessageType(String),
    #[error("Tiến độ không hợp lệ: {0} (phải từ 0 đến 100)")]
    InvalidProgress(f32),
    #[error("Không tìm thấy worker: {0}")]
    WorkerNotFound(String),
    #[error("Worker quá thời gian cho phép")]
    Timeout,
    #[error("Tác vụ AI đã bị hủy")]
    Cancelled,
    #[error("Worker dừng mà không trả kết quả (mã {code:?}): {stderr_tail}")]
    WorkerExited {
        code: Option<i32>,
        stderr_tail: String,
    },
    #[error("Worker báo lỗi: {0}")]
    WorkerFailed(String),
}

/// Kind of AI job. Serialised as the snake_case strings the worker
/// protocol uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiKind {
    Separate,
    Recognize,
    Align,
    AlignRegion,
    RoleSuggest,
}

/// One AI request, sent as a single JSON line on the worker's stdin.
///
/// `input` holds paths + metadata only — never audio bytes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiRequest {
    pub protocol_version: u32,
    pub request_id: String,
    pub kind: AiKind,
    pub project_id: String,
    pub input: serde_json::Value,
    pub options: serde_json::Value,
}

impl AiRequest {
    pub fn new(
        kind: AiKind,
        project_id: impl Into<String>,
        input: serde_json::Value,
        options: serde_json::Value,
    ) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            request_id: new_id(),
            kind,
            project_id: project_id.into(),
            input,
            options,
        }
    }

    /// Serialise to one JSON line (newline-terminated, no trailing
    /// spaces), ready to write to the worker's stdin.
    pub fn to_line(&self) -> Result<String, AiError> {
        let mut line = serde_json::to_string(self)?;
        line.push('\n');
        Ok(line)
    }
}

/// One message from the worker, parsed from a single JSON Lines line.
#[derive(Debug, Clone, PartialEq)]
pub enum AiMessage {
    Progress {
        request_id: String,
        progress: f32,
        message: String,
    },
    Result {
        request_id: String,
        result: serde_json::Value,
    },
    Error {
        request_id: String,
        error: String,
    },
}

/// Parse one JSON Lines response line.
///
/// Returns `Ok(None)` for blank lines (they are skipped, not errors).
/// Rejects malformed JSON, a wrong `protocol_version`, a `request_id`
/// that does not match `expected_request_id`, an unknown `type`, and a
/// `progress` outside 0..=100 — each with its own `AiError` variant.
pub fn parse_message(line: &str, expected_request_id: &str) -> Result<Option<AiMessage>, AiError> {
    if line.trim().is_empty() {
        return Ok(None);
    }
    let value: serde_json::Value = serde_json::from_str(line)?;
    let found_version = value
        .get("protocol_version")
        .and_then(serde_json::Value::as_u64)
        .map(|v| v as u32);
    match found_version {
        Some(v) if v == PROTOCOL_VERSION => {}
        other => {
            return Err(AiError::WrongProtocolVersion {
                expected: PROTOCOL_VERSION,
                found: other,
            });
        }
    }
    let request_id = value
        .get("request_id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    if request_id != expected_request_id {
        return Err(AiError::RequestIdMismatch {
            expected: expected_request_id.to_owned(),
            found: request_id.to_owned(),
        });
    }
    let message_type = value
        .get("type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    match message_type {
        "progress" => {
            let progress = value
                .get("progress")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(f64::NAN) as f32;
            if !progress.is_finite() || !(0.0..=100.0).contains(&progress) {
                return Err(AiError::InvalidProgress(progress));
            }
            let message = value
                .get("message")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .to_owned();
            Ok(Some(AiMessage::Progress {
                request_id: request_id.to_owned(),
                progress,
                message,
            }))
        }
        "result" => Ok(Some(AiMessage::Result {
            request_id: request_id.to_owned(),
            result: value
                .get("result")
                .cloned()
                .unwrap_or(serde_json::Value::Null),
        })),
        "error" => Ok(Some(AiMessage::Error {
            request_id: request_id.to_owned(),
            error: value
                .get("error")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .to_owned(),
        })),
        other => Err(AiError::UnknownMessageType(other.to_owned())),
    }
}

/// Configuration for one worker process.
#[derive(Debug, Clone)]
pub struct WorkerConfig {
    /// Program to run: a path, or a name resolved via `discover_worker`.
    pub command: String,
    pub args: Vec<String>,
    /// Maximum time `Worker::run` waits for a result.
    pub timeout: Duration,
    /// Extra environment variables for the child.
    pub env: Vec<(String, String)>,
}

impl WorkerConfig {
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            args: Vec::new(),
            // AI jobs (separation / recognition) can take a long time;
            // callers that need a tighter bound should set `timeout`.
            timeout: Duration::from_secs(30 * 60),
            env: Vec::new(),
        }
    }

    pub fn args(mut self, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.args = args.into_iter().map(Into::into).collect();
        self
    }

    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }
}

/// A spawned AI worker process.
///
/// `spawn` writes the request line to the child's stdin and closes it;
/// `run` then reads stdout lines until a result or error message,
/// calling `on_progress` for progress messages. Stderr is drained on a
/// background thread so a chatty worker can never block on a full pipe,
/// and is only surfaced (truncated) in error reports.
pub struct Worker {
    config: WorkerConfig,
    request: AiRequest,
    cancel: Arc<AtomicBool>,
    child: Option<Child>,
    lines: mpsc::Receiver<Result<String, std::io::Error>>,
    stderr: Arc<Mutex<String>>,
}

fn tail_chars(text: &str, max_chars: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_chars {
        text.to_owned()
    } else {
        chars[chars.len() - max_chars..].iter().collect()
    }
}

fn is_executable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Look a worker program up in `PATH`, like Python's `shutil.which`.
/// A value containing a path separator is checked as a path instead.
/// Returns `AiError::WorkerNotFound` naming the program when it is not
/// found (or, for a path, is not an executable file).
pub fn discover_worker(command_name: &str) -> Result<PathBuf, AiError> {
    if command_name.is_empty() {
        return Err(AiError::WorkerNotFound(command_name.to_owned()));
    }
    let path = Path::new(command_name);
    if command_name.contains('/') || command_name.contains('\\') {
        if is_executable(path) {
            return Ok(path.to_path_buf());
        }
        return Err(AiError::WorkerNotFound(command_name.to_owned()));
    }
    if let Ok(paths) = std::env::var("PATH") {
        for dir in std::env::split_paths(&paths) {
            let candidate = dir.join(command_name);
            if is_executable(&candidate) {
                return Ok(candidate);
            }
            #[cfg(windows)]
            {
                let with_exe = candidate.with_extension("exe");
                if is_executable(&with_exe) {
                    return Ok(with_exe);
                }
            }
        }
    }
    Err(AiError::WorkerNotFound(command_name.to_owned()))
}

impl Worker {
    /// Spawn the worker and send it the request (one JSON line on stdin,
    /// then stdin is closed). The child is not waited on here; call
    /// [`Worker::run`] to collect its answer.
    pub fn spawn(config: WorkerConfig, request: AiRequest) -> Result<Self, AiError> {
        let program = if config.command.contains('/') || config.command.contains('\\') {
            PathBuf::from(&config.command)
        } else {
            discover_worker(&config.command)?
        };
        let mut child = Command::new(program)
            .args(&config.args)
            .envs(config.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let request_line = request.to_line()?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(request_line.as_bytes())?;
            stdin.flush()?;
        }
        let stdout = child.stdout.take().expect("stdout was piped");
        let stderr = child.stderr.take().expect("stderr was piped");

        let stderr_shared = Arc::new(Mutex::new(String::new()));
        let stderr_writer = Arc::clone(&stderr_shared);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stderr);
            let mut buf = String::new();
            // Read to EOF; the child closing stderr ends this thread.
            let _ = reader.read_to_string(&mut buf);
            if let Ok(mut guard) = stderr_writer.lock() {
                *guard = buf;
            }
        });

        let (sender, lines) = mpsc::channel::<Result<String, std::io::Error>>();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => break, // EOF
                    Ok(_) => {
                        if sender.send(Ok(line)).is_err() {
                            break;
                        }
                    }
                    Err(err) => {
                        let _ = sender.send(Err(err));
                        break;
                    }
                }
            }
        });

        Ok(Self {
            config,
            request,
            cancel: Arc::new(AtomicBool::new(false)),
            child: Some(child),
            lines,
            stderr: stderr_shared,
        })
    }

    /// Shared cancellation flag: set it from another thread to make a
    /// running `run()` kill the worker and return `AiError::Cancelled`.
    /// `cancel()` is the same flag, set in place.
    pub fn cancel_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancel)
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub fn request_id(&self) -> &str {
        &self.request.request_id
    }

    fn stderr_tail(&self) -> String {
        let guard = self.stderr.lock().map(|g| g.clone()).unwrap_or_default();
        tail_chars(&guard, MAX_STDERR_TAIL_CHARS)
    }

    fn kill_and_wait(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// After a terminal message, give the child a short grace period to
    /// exit on its own, then kill it so `run` never hangs on a worker
    /// that answered but did not exit.
    fn finish_after_terminal(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let deadline = Instant::now() + TERMINAL_EXIT_GRACE;
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => {
                        self.child.take();
                        return;
                    }
                    Ok(None) => {
                        if Instant::now() >= deadline {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    Err(_) => break,
                }
            }
        }
        self.kill_and_wait();
    }

    fn exited_error(&mut self) -> AiError {
        let code = match self.child.as_mut() {
            Some(child) => match child.wait() {
                Ok(status) => {
                    self.child.take();
                    status.code()
                }
                Err(_) => {
                    self.child.take();
                    None
                }
            },
            None => None,
        };
        AiError::WorkerExited {
            code,
            stderr_tail: self.stderr_tail(),
        }
    }

    /// Blocking: read stdout lines until a result or error message.
    /// `on_progress` is called for each progress message. Enforces the
    /// configured timeout and the shared cancel flag; on either, the
    /// child is killed and the matching error returned. On success the
    /// worker's `result` value is returned.
    pub fn run(
        &mut self,
        mut on_progress: impl FnMut(f32, &str),
    ) -> Result<serde_json::Value, AiError> {
        let deadline = Instant::now() + self.config.timeout;
        loop {
            if self.cancel.load(Ordering::SeqCst) {
                self.kill_and_wait();
                return Err(AiError::Cancelled);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                self.kill_and_wait();
                return Err(AiError::Timeout);
            }
            match self.lines.recv_timeout(remaining.min(POLL_INTERVAL)) {
                Ok(Ok(line)) => match parse_message(&line, &self.request.request_id)? {
                    None => continue,
                    Some(AiMessage::Progress {
                        progress, message, ..
                    }) => on_progress(progress, &message),
                    Some(AiMessage::Result { result, .. }) => {
                        self.finish_after_terminal();
                        return Ok(result);
                    }
                    Some(AiMessage::Error { error, .. }) => {
                        self.finish_after_terminal();
                        return Err(AiError::WorkerFailed(error));
                    }
                },
                Ok(Err(_)) => return Err(self.exited_error()),
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => return Err(self.exited_error()),
            }
        }
    }
}

/// Convenience: spawn a worker and run it to completion with a fresh
/// (never externally cancelled) flag. Prefer `Worker::spawn` + `run`
/// when cancellation from another thread is needed.
pub fn run_request(
    config: WorkerConfig,
    request: AiRequest,
    on_progress: impl FnMut(f32, &str),
) -> Result<serde_json::Value, AiError> {
    let mut worker = Worker::spawn(config, request)?;
    worker.run(on_progress)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering as AtomicOrdering;

    fn python_command() -> &'static str {
        if cfg!(windows) { "python" } else { "python3" }
    }

    fn test_request(kind: AiKind) -> AiRequest {
        AiRequest::new(
            kind,
            "0123456789abcdef0123456789abcdef",
            serde_json::json!({"vocals": "/tmp/vocals.wav", "model": "small"}),
            serde_json::json!({"language": "vi"}),
        )
    }

    #[test]
    fn request_serialises_as_one_json_line() {
        let request = test_request(AiKind::Align);
        assert_eq!(request.protocol_version, PROTOCOL_VERSION);
        assert_eq!(request.protocol_version, 1);
        assert!(vietkara_domain::is_hex32(&request.request_id));
        let line = request.to_line().unwrap();
        assert!(line.ends_with('\n'));
        assert_eq!(line.trim_end_matches('\n').lines().count(), 1);
        let parsed: AiRequest = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(parsed, request);
        let value: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(value["kind"], "align");
        assert_eq!(value["input"]["vocals"], "/tmp/vocals.wav");
    }

    #[test]
    fn kind_strings_are_snake_case() {
        let cases = [
            (AiKind::Separate, "separate"),
            (AiKind::Recognize, "recognize"),
            (AiKind::Align, "align"),
            (AiKind::AlignRegion, "align_region"),
            (AiKind::RoleSuggest, "role_suggest"),
        ];
        for (kind, expected) in cases {
            let request = test_request(kind);
            let value: serde_json::Value =
                serde_json::from_str(request.to_line().unwrap().trim()).unwrap();
            assert_eq!(value["kind"], expected, "{kind:?}");
        }
    }

    fn message_line(request_id: &str, body: serde_json::Value) -> String {
        let mut map = serde_json::Map::new();
        map.insert("protocol_version".to_owned(), serde_json::json!(1));
        map.insert(
            "request_id".to_owned(),
            serde_json::Value::String(request_id.to_owned()),
        );
        if let serde_json::Value::Object(extra) = body {
            map.extend(extra);
        }
        serde_json::to_string(&serde_json::Value::Object(map)).unwrap()
    }

    #[test]
    fn parses_progress_result_and_error_messages() {
        let id = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let progress = parse_message(
            &message_line(
                id,
                serde_json::json!({"type": "progress", "progress": 50, "message": "Đang căn lời"}),
            ),
            id,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            progress,
            AiMessage::Progress {
                request_id: id.to_owned(),
                progress: 50.0,
                message: "Đang căn lời".to_owned(),
            }
        );
        let result = parse_message(
            &message_line(
                id,
                serde_json::json!({"type": "result", "result": {"words": []}}),
            ),
            id,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            result,
            AiMessage::Result {
                request_id: id.to_owned(),
                result: serde_json::json!({"words": []}),
            }
        );
        let error = parse_message(
            &message_line(
                id,
                serde_json::json!({"type": "error", "error": "model missing"}),
            ),
            id,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            error,
            AiMessage::Error {
                request_id: id.to_owned(),
                error: "model missing".to_owned(),
            }
        );
    }

    #[test]
    fn blank_lines_are_skipped() {
        assert_eq!(parse_message("", "x").unwrap(), None);
        assert_eq!(parse_message("   \n", "x").unwrap(), None);
    }

    #[test]
    fn rejects_wrong_version_mismatched_id_bad_json_and_unknown_type() {
        let id = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let wrong_version = serde_json::json!({
            "protocol_version": 99, "request_id": id,
            "type": "progress", "progress": 1, "message": "x",
        });
        assert!(matches!(
            parse_message(&wrong_version.to_string(), id),
            Err(AiError::WrongProtocolVersion {
                expected: 1,
                found: Some(99)
            })
        ));
        let missing_version = serde_json::json!({
            "request_id": id, "type": "progress", "progress": 1, "message": "x",
        });
        assert!(matches!(
            parse_message(&missing_version.to_string(), id),
            Err(AiError::WrongProtocolVersion {
                expected: 1,
                found: None
            })
        ));
        let wrong_id = message_line(
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            serde_json::json!({"type": "progress", "progress": 1, "message": "x"}),
        );
        assert!(matches!(
            parse_message(&wrong_id, id),
            Err(AiError::RequestIdMismatch { .. })
        ));
        assert!(matches!(
            parse_message("{not json", id),
            Err(AiError::Json(_))
        ));
        let unknown = message_line(id, serde_json::json!({"type": "teleport", "progress": 1}));
        assert!(matches!(
            parse_message(&unknown, id),
            Err(AiError::UnknownMessageType(t)) if t == "teleport"
        ));
        let bad_progress = message_line(
            id,
            serde_json::json!({"type": "progress", "progress": 150, "message": "x"}),
        );
        assert!(matches!(
            parse_message(&bad_progress, id),
            Err(AiError::InvalidProgress(_))
        ));
    }

    fn write_fake_worker(script: &str) -> tempfile::NamedTempFile {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(script.as_bytes()).unwrap();
        file.flush().unwrap();
        file
    }

    const FAKE_WORKER: &str = r#"
import json, sys
line = sys.stdin.readline()
req = json.loads(line)
def emit(obj):
    print(json.dumps(obj), flush=True)
emit({"protocol_version": 1, "request_id": req["request_id"], "type": "progress", "progress": 25, "message": "Đang tách..."})
emit({"protocol_version": 1, "request_id": req["request_id"], "type": "progress", "progress": 80, "message": "Gần xong"})
emit({"protocol_version": 1, "request_id": req["request_id"], "type": "result", "result": {"words": [{"text": "Xin", "start": 0.2, "end": 0.6}]}})
"#;

    #[test]
    fn run_returns_result_and_reports_progress() {
        let script = write_fake_worker(FAKE_WORKER);
        let config =
            WorkerConfig::new(python_command()).arg(script.path().to_string_lossy().into_owned());
        let request = test_request(AiKind::Separate);
        let mut worker = Worker::spawn(config, request).unwrap();
        let mut seen: Vec<(f32, String)> = Vec::new();
        let result = worker
            .run(|progress, message| seen.push((progress, message.to_owned())))
            .unwrap();
        assert_eq!(
            seen,
            vec![
                (25.0, "Đang tách...".to_owned()),
                (80.0, "Gần xong".to_owned()),
            ]
        );
        assert_eq!(
            result,
            serde_json::json!({"words": [{"text": "Xin", "start": 0.2, "end": 0.6}]})
        );
    }

    #[test]
    fn run_request_uses_python_jsonl_entrypoint() {
        let entry = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ai_worker.py");
        assert!(
            entry.is_file(),
            "missing Python JSONL worker: {}",
            entry.display()
        );
        let config = WorkerConfig::new(python_command()).arg(entry.to_string_lossy().into_owned());
        let request = AiRequest::new(
            AiKind::Recognize,
            "project-1",
            serde_json::json!({}),
            serde_json::json!({}),
        );
        let error = run_request(config, request, |_, _| {}).unwrap_err();
        assert!(
            matches!(error, AiError::WorkerFailed(ref message) if message.contains("vocal")),
            "unexpected bridge error: {error:?}"
        );
    }

    #[test]
    fn run_reports_worker_error_message() {
        let script = write_fake_worker(
            r#"
import json, sys
req = json.loads(sys.stdin.readline())
print(json.dumps({"protocol_version": 1, "request_id": req["request_id"], "type": "error", "error": "model missing"}), flush=True)
"#,
        );
        let config =
            WorkerConfig::new(python_command()).arg(script.path().to_string_lossy().into_owned());
        let mut worker = Worker::spawn(config, test_request(AiKind::Recognize)).unwrap();
        let err = worker.run(|_, _| {}).unwrap_err();
        assert!(matches!(err, AiError::WorkerFailed(m) if m == "model missing"));
    }

    #[test]
    fn crashing_worker_returns_exited_with_stderr() {
        let script = write_fake_worker(
            "import sys\nprint('boom: model file not found', file=sys.stderr, flush=True)\nsys.exit(3)\n",
        );
        let config =
            WorkerConfig::new(python_command()).arg(script.path().to_string_lossy().into_owned());
        let mut worker = Worker::spawn(config, test_request(AiKind::Align)).unwrap();
        let err = worker.run(|_, _| {}).unwrap_err();
        match err {
            AiError::WorkerExited { code, stderr_tail } => {
                assert_eq!(code, Some(3));
                assert!(
                    stderr_tail.contains("boom: model file not found"),
                    "stderr_tail was {stderr_tail:?}"
                );
            }
            other => panic!("expected WorkerExited, got {other:?}"),
        }
    }

    #[test]
    fn slow_worker_is_cancelled_via_flag() {
        let script = write_fake_worker(
            "import json, sys, time\nreq = json.loads(sys.stdin.readline())\ntime.sleep(30)\nprint(json.dumps({\"protocol_version\": 1, \"request_id\": req[\"request_id\"], \"type\": \"result\", \"result\": {}}), flush=True)\n",
        );
        let config = WorkerConfig::new(python_command())
            .arg(script.path().to_string_lossy().into_owned())
            .timeout(Duration::from_secs(60));
        let mut worker = Worker::spawn(config, test_request(AiKind::Align)).unwrap();
        let flag = worker.cancel_flag();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            flag.store(true, AtomicOrdering::SeqCst);
        });
        let err = worker.run(|_, _| {}).unwrap_err();
        assert!(matches!(err, AiError::Cancelled));
    }

    #[test]
    fn slow_worker_times_out() {
        let script = write_fake_worker(
            "import json, sys, time\nreq = json.loads(sys.stdin.readline())\ntime.sleep(30)\nprint(json.dumps({\"protocol_version\": 1, \"request_id\": req[\"request_id\"], \"type\": \"result\", \"result\": {}}), flush=True)\n",
        );
        let config = WorkerConfig::new(python_command())
            .arg(script.path().to_string_lossy().into_owned())
            .timeout(Duration::from_millis(300));
        let mut worker = Worker::spawn(config, test_request(AiKind::Align)).unwrap();
        let err = worker.run(|_, _| {}).unwrap_err();
        assert!(matches!(err, AiError::Timeout));
    }

    #[test]
    fn discover_worker_finds_python_and_rejects_missing_programs() {
        assert!(discover_worker(python_command()).is_ok());
        let err = discover_worker("vietkara-no-such-worker-xyz").unwrap_err();
        assert!(
            matches!(err, AiError::WorkerNotFound(name) if name == "vietkara-no-such-worker-xyz")
        );
    }
}
