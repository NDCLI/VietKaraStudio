//! Job lifecycle, persistence and batch-render queue state.
//!
//! Ports Python `engine.Job`, `app.py::persist_job` / `recover_jobs` and
//! `batch_render.py`'s record state machine — the parts that do not need
//! a GUI, an audio device or an AI model:
//!
//! * a job is `queued` → `running` → `done` / `error` / `cancelled`;
//!   `update()` checks cancellation first, exactly like Python
//!   `Job.update()` calling `check()`;
//! * progress is clamped to 0..=100, error messages are truncated to
//!   4,500 chars and device fallbacks to 1,000 chars, matching Python;
//! * jobs are persisted atomically as `jobs/<id>.json`; on recovery,
//!   jobs left `queued`/`running` become `interrupted` with the same
//!   Vietnamese message Python uses — the project itself is untouched;
//! * a batch record tracks `next_index`, `completed`/`failed` and
//!   per-item status; `first_pending()` is the first item not `done`,
//!   and retrying a failed item decrements `failed`, as in Python.
//!
//! Running the actual work (render, separation, recognition) is the
//! caller's job: it polls `check()` and calls `update()` /
//! `mark_done()` / `mark_error()`. A real Windows audio device, GPUI
//! progress UI and the AI worker itself live elsewhere (see
//! `vietkara-media`, `vietkara-ai-bridge` and the Phase 1 spike).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use thiserror::Error;
use vietkara_domain::{is_hex32, new_id};
use vietkara_storage::write_atomic;

pub const MAX_MESSAGE_CHARS: usize = 4_500;
pub const MAX_FALLBACK_REASON_CHARS: usize = 1_000;

#[derive(Debug, Error)]
pub enum JobError {
    #[error("Tác vụ đã bị hủy.")]
    Cancelled,
    #[error("Lỗi đọc/ghi file: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON không hợp lệ: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Lỗi storage: {0}")]
    Storage(#[from] vietkara_storage::StorageError),
    #[error("ID tác vụ không hợp lệ: {0}")]
    InvalidId(String),
    #[error("Trạng thái batch không hợp lệ: {0}")]
    InvalidBatch(String),
}

fn truncate_chars(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        value.to_owned()
    } else {
        value.chars().take(max).collect()
    }
}

fn unix_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobState {
    Queued,
    Running,
    Done,
    Error,
    Cancelled,
    Interrupted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fallback {
    pub stage: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JobData {
    id: String,
    project_id: String,
    kind: String,
    state: JobState,
    message: String,
    progress: f64,
    result: Option<serde_json::Value>,
    #[serde(default)]
    devices: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    fallbacks: Vec<Fallback>,
    #[serde(default)]
    updated_at: Option<f64>,
}

/// A cancellable, persistable job. Cheap to clone: clones share state.
#[derive(Debug, Clone)]
pub struct Job {
    inner: Arc<Mutex<JobData>>,
    cancel: Arc<AtomicBool>,
}

impl Job {
    pub fn new(
        id: impl Into<String>,
        project_id: impl Into<String>,
        kind: impl Into<String>,
    ) -> Self {
        Self {
            inner: Arc::new(Mutex::new(JobData {
                id: id.into(),
                project_id: project_id.into(),
                kind: kind.into(),
                state: JobState::Queued,
                message: "Đang đợi xử lý…".to_owned(),
                progress: 0.0,
                result: None,
                devices: BTreeMap::new(),
                fallbacks: Vec::new(),
                updated_at: None,
            })),
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn new_id() -> String {
        new_id()
    }

    pub fn id(&self) -> String {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .id
            .clone()
    }

    pub fn state(&self) -> JobState {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .state
    }

    pub fn progress(&self) -> f64 {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .progress
    }

    pub fn message(&self) -> String {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .message
            .clone()
    }

    pub fn is_cancel_requested(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    /// Like Python `Job.check()`: raises if cancellation was requested.
    pub fn check(&self) -> Result<(), JobError> {
        if self.is_cancel_requested() {
            Err(JobError::Cancelled)
        } else {
            Ok(())
        }
    }

    /// Like Python `Job.update()`: checks cancellation first, then
    /// records progress (clamped 0..=100) and message.
    pub fn update(&self, progress: f64, message: impl Into<String>) -> Result<(), JobError> {
        self.check()?;
        let mut data = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        data.progress = progress.clamp(0.0, 100.0);
        data.message = message.into();
        data.updated_at = Some(unix_now());
        Ok(())
    }

    pub fn set_state(&self, state: JobState) {
        let mut data = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        data.state = state;
        data.updated_at = Some(unix_now());
    }

    pub fn mark_done(&self, result: serde_json::Value) {
        let mut data = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        data.result = Some(result);
        data.state = JobState::Done;
        data.progress = 100.0;
        data.message = "Hoàn tất".to_owned();
        data.updated_at = Some(unix_now());
    }

    pub fn mark_error(&self, message: impl Into<String>) {
        let mut data = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        data.state = JobState::Error;
        data.message = truncate_chars(&message.into(), MAX_MESSAGE_CHARS);
        data.updated_at = Some(unix_now());
    }

    /// Like Python `Job.stop()`: request cancellation. Does not change
    /// state by itself; the running task observes it via `check()`.
    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub fn mark_cancelled(&self) {
        let mut data = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        data.state = JobState::Cancelled;
        data.message = "Đã hủy tác vụ.".to_owned();
        data.updated_at = Some(unix_now());
    }

    pub fn mark_interrupted(&self) {
        let mut data = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        data.state = JobState::Interrupted;
        data.message =
            "Ứng dụng đã đóng giữa tác vụ. Bước này cần chạy lại; project vẫn được giữ nguyên."
                .to_owned();
        data.updated_at = Some(unix_now());
    }

    pub fn record_device(
        &self,
        stage: impl Into<String>,
        device: serde_json::Value,
        reason: Option<String>,
    ) {
        let stage = stage.into();
        let mut data = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        data.devices.insert(stage.clone(), device);
        if let Some(reason) = reason
            && !reason.is_empty()
        {
            data.fallbacks.push(Fallback {
                stage,
                reason: truncate_chars(&reason, MAX_FALLBACK_REASON_CHARS),
            });
        }
        data.updated_at = Some(unix_now());
    }

    /// The `public()` shape from Python `engine.Job.public()`.
    pub fn public(&self) -> serde_json::Value {
        let data = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        serde_json::json!({
            "id": data.id,
            "project_id": data.project_id,
            "kind": data.kind,
            "state": data.state,
            "message": data.message,
            "progress": data.progress,
            "result": data.result,
            "devices": data.devices,
            "fallbacks": data.fallbacks,
            "cancel_requested": self.is_cancel_requested(),
            "updated_at": data.updated_at,
        })
    }

    pub fn job_file(store_dir: &Path, id: &str) -> PathBuf {
        store_dir.join(format!("{id}.json"))
    }

    /// Persist atomically to `store_dir/<id>.json`.
    pub fn persist(&self, store_dir: &Path) -> Result<(), JobError> {
        let mut data = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        data.updated_at = Some(unix_now());
        let payload = serde_json::to_vec_pretty(&*data)?;
        drop(data);
        let id = self.id();
        std::fs::create_dir_all(store_dir)?;
        write_atomic(&Self::job_file(store_dir, &id), &payload)?;
        Ok(())
    }

    pub fn load(store_dir: &Path, id: &str) -> Result<Self, JobError> {
        if !is_hex32(id) {
            return Err(JobError::InvalidId(id.to_owned()));
        }
        let text = std::fs::read_to_string(Self::job_file(store_dir, id))?;
        let data: JobData = serde_json::from_str(&text)?;
        Ok(Self {
            inner: Arc::new(Mutex::new(data)),
            cancel: Arc::new(AtomicBool::new(false)),
        })
    }
}

/// Recover all persisted jobs in `store_dir`: jobs left `queued` or
/// `running` become `interrupted` (and are re-persisted); finished jobs
/// are returned unchanged. Malformed files and non-hex IDs are skipped,
/// like Python `recover_jobs()`.
pub fn recover_jobs(store_dir: &Path) -> Vec<Job> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(store_dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if path.extension().and_then(|s| s.to_str()) != Some("json") || !is_hex32(stem) {
            continue;
        }
        let Ok(job) = Job::load(store_dir, stem) else {
            continue;
        };
        if matches!(job.state(), JobState::Queued | JobState::Running) {
            job.mark_interrupted();
            let _ = job.persist(store_dir);
        }
        out.push(job);
    }
    out.sort_by_key(|job| job.id());
    out
}

// ---------------------------------------------------------------------------
// Batch-render queue record (batch_render.py)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemStatus {
    Queued,
    Running,
    Done,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BatchStatus {
    Queued,
    Running,
    Done,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchItem {
    pub project_id: String,
    pub render_id: String,
    pub name: String,
    pub variant: serde_json::Value,
    pub directory: String,
    #[serde(default)]
    pub assets: Vec<String>,
    pub status: ItemStatus,
    #[serde(default)]
    pub result: Option<serde_json::Value>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchRecord {
    pub id: String,
    pub job_id: String,
    pub owner: String,
    pub mode: String,
    pub created_at: f64,
    pub updated_at: f64,
    pub status: BatchStatus,
    #[serde(default)]
    pub next_index: usize,
    #[serde(default)]
    pub items: Vec<BatchItem>,
    #[serde(default)]
    pub completed: usize,
    #[serde(default)]
    pub failed: usize,
}

impl BatchRecord {
    /// Validate the same choices Python `batch_render.create()` does.
    pub fn new(
        owner: impl Into<String>,
        mode: impl Into<String>,
        aspects: &[String],
        audio_outputs: &[String],
    ) -> Result<Self, JobError> {
        let mode = mode.into();
        if mode != "batch" && mode != "single-render" {
            return Err(JobError::InvalidBatch(format!("mode không hợp lệ: {mode}")));
        }
        if aspects.is_empty()
            || aspects
                .iter()
                .any(|a| a != "current" && a != "landscape" && a != "portrait")
        {
            return Err(JobError::InvalidBatch(
                "tỉ lệ đầu ra theo lô không hợp lệ.".into(),
            ));
        }
        if audio_outputs.is_empty() || audio_outputs.iter().any(|a| a != "beat" && a != "guide") {
            return Err(JobError::InvalidBatch(
                "loại âm thanh đầu ra không hợp lệ.".into(),
            ));
        }
        let now = unix_now();
        Ok(Self {
            id: new_id(),
            job_id: new_id(),
            owner: owner.into(),
            mode,
            created_at: now,
            updated_at: now,
            status: BatchStatus::Queued,
            next_index: 0,
            items: Vec::new(),
            completed: 0,
            failed: 0,
        })
    }

    /// First item whose status is not `done`, or `items.len()`.
    pub fn first_pending(&self) -> usize {
        self.items
            .iter()
            .position(|item| item.status != ItemStatus::Done)
            .unwrap_or(self.items.len())
    }

    pub fn mark_item_running(&mut self, index: usize) {
        if let Some(item) = self.items.get_mut(index) {
            // Retrying a failed item decrements `failed`, like Python.
            if item.status == ItemStatus::Error {
                self.failed = self.failed.saturating_sub(1);
            }
            item.status = ItemStatus::Running;
            item.error = None;
        }
        self.updated_at = unix_now();
    }

    pub fn mark_item_done(&mut self, index: usize, result: serde_json::Value) {
        if let Some(item) = self.items.get_mut(index) {
            item.status = ItemStatus::Done;
            item.result = Some(result);
            item.error = None;
            self.completed += 1;
        }
        self.next_index = index + 1;
        self.updated_at = unix_now();
    }

    pub fn mark_item_error(&mut self, index: usize, error: impl Into<String>) {
        if let Some(item) = self.items.get_mut(index) {
            item.status = ItemStatus::Error;
            item.error = Some(truncate_chars(&error.into(), 2_000));
            self.failed += 1;
        }
        self.next_index = index + 1;
        self.updated_at = unix_now();
    }

    pub fn record_path(data_root: &Path, batch_id: &str) -> PathBuf {
        data_root
            .join("batch-renders")
            .join(batch_id)
            .join("batch.json")
    }

    pub fn save(&self, data_root: &Path) -> Result<(), JobError> {
        let mut record = self.clone();
        record.updated_at = unix_now();
        let payload = serde_json::to_vec_pretty(&record)?;
        write_atomic(&Self::record_path(data_root, &record.id), &payload)?;
        Ok(())
    }

    pub fn load(data_root: &Path, batch_id: &str) -> Result<Self, JobError> {
        let text = std::fs::read_to_string(Self::record_path(data_root, batch_id))?;
        Ok(serde_json::from_str(&text)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_job() -> Job {
        Job::new(new_id(), new_id(), "render")
    }

    #[test]
    fn lifecycle_and_progress_clamping() {
        let job = sample_job();
        assert_eq!(job.state(), JobState::Queued);
        job.set_state(JobState::Running);
        job.update(150.0, "Đang dựng").unwrap();
        assert_eq!(job.progress(), 100.0);
        job.update(-5.0, "x").unwrap();
        assert_eq!(job.progress(), 0.0);
        job.mark_done(json!({"video": "karaoke.mp4"}));
        assert_eq!(job.state(), JobState::Done);
        assert_eq!(job.progress(), 100.0);
        assert_eq!(job.public()["result"]["video"], "karaoke.mp4");
    }

    #[test]
    fn update_after_cancel_fails_and_public_shape_matches_python() {
        let job = sample_job();
        job.request_cancel();
        assert!(matches!(job.update(10.0, "x"), Err(JobError::Cancelled)));
        assert!(matches!(job.check(), Err(JobError::Cancelled)));
        let public = job.public();
        assert_eq!(public["cancel_requested"], true);
        assert_eq!(public["state"], "queued");
        for key in [
            "id",
            "project_id",
            "kind",
            "state",
            "message",
            "progress",
            "result",
            "devices",
            "fallbacks",
            "cancel_requested",
            "updated_at",
        ] {
            assert!(public.get(key).is_some(), "missing {key}");
        }
    }

    #[test]
    fn record_device_and_fallback_truncation() {
        let job = sample_job();
        job.record_device("video", json!({"encoder": "libx264", "name": "CPU"}), None);
        job.record_device(
            "video",
            json!({"encoder": "h264_nvenc"}),
            Some("x".repeat(1500)),
        );
        let public = job.public();
        assert_eq!(public["devices"]["video"]["encoder"], "h264_nvenc");
        assert_eq!(public["fallbacks"].as_array().unwrap().len(), 1);
        assert_eq!(
            public["fallbacks"][0]["reason"]
                .as_str()
                .unwrap()
                .chars()
                .count(),
            MAX_FALLBACK_REASON_CHARS
        );
    }

    #[test]
    fn persist_and_recover_turns_unfinished_jobs_interrupted() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("jobs");
        let running = sample_job();
        running.set_state(JobState::Running);
        running.persist(&store).unwrap();
        let done = sample_job();
        done.mark_done(json!({}));
        done.persist(&store).unwrap();
        // A malformed file and a non-hex file must be skipped.
        std::fs::write(store.join("not-json.json"), b"{oops").unwrap();
        std::fs::write(store.join("abc.json"), b"{}").unwrap();

        let recovered = recover_jobs(&store);
        assert_eq!(recovered.len(), 2);
        let by_id: BTreeMap<String, JobState> =
            recovered.iter().map(|j| (j.id(), j.state())).collect();
        assert_eq!(by_id[&running.id()], JobState::Interrupted);
        assert_eq!(by_id[&done.id()], JobState::Done);
        // Re-persisted interrupted job loads back as interrupted.
        assert_eq!(
            Job::load(&store, &running.id()).unwrap().state(),
            JobState::Interrupted
        );
    }

    #[test]
    fn batch_first_pending_and_retry_counts() {
        let mut record = BatchRecord::new(
            "owner",
            "batch",
            &["current".to_string()],
            &["beat".to_string()],
        )
        .unwrap();
        assert!(matches!(
            BatchRecord::new("o", "nope", &["current".into()], &["beat".into()]),
            Err(JobError::InvalidBatch(_))
        ));
        for (i, status) in [
            (0, ItemStatus::Done),
            (1, ItemStatus::Error),
            (2, ItemStatus::Queued),
        ] {
            record.items.push(BatchItem {
                project_id: new_id(),
                render_id: new_id(),
                name: format!("item {i}"),
                variant: json!({"aspect": "landscape", "audio": "beat"}),
                directory: format!("/tmp/item{i}"),
                assets: vec![],
                status,
                result: None,
                error: None,
            });
        }
        record.failed = 1;
        assert_eq!(record.first_pending(), 1);
        record.mark_item_running(1); // retrying a failed item
        assert_eq!(record.failed, 0);
        record.mark_item_error(1, "boom");
        assert_eq!(record.failed, 1);
        record.mark_item_running(1);
        record.mark_item_done(1, json!({"ok": true}));
        assert_eq!(record.completed, 1);
        assert_eq!(record.next_index, 2);
        assert_eq!(record.first_pending(), 2);

        let dir = tempfile::tempdir().unwrap();
        record.save(dir.path()).unwrap();
        let loaded = BatchRecord::load(dir.path(), &record.id).unwrap();
        assert_eq!(loaded.items.len(), 3);
        assert_eq!(loaded.completed, 1);
    }
}
