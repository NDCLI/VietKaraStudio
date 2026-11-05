//! Domain model for VietKara Studio projects.
//!
//! This crate mirrors the Python baseline contract (`timing.py`,
//! `app.py::load/save`) — see `docs/compatibility-matrix.md`:
//!
//! * project / word / line IDs are 32 lowercase hex chars
//!   (`uuid.uuid4().hex` in Python, `Uuid::new_v4().simple()` here);
//! * a missing `schema` means schema 1, and `load()` migrates to schema 2
//!   by normalising words/anchors exactly like `timing.validate_words` /
//!   `validate_anchors` (new hex IDs for missing/invalid/duplicate IDs,
//!   one `line_uid` per `line`, invalid role coerced to `"all"`, empty
//!   words dropped, times rounded to 4 decimals, anchors sorted by `ref`);
//! * `save()` is atomic (temp file in the same directory + rename);
//! * unknown JSON fields are kept via `serde(flatten)` at project and
//!   word level, so a round-trip through Rust does not lose fields the
//!   typed model does not understand yet.
//!
//! Differences from Python, deliberately: `load()` does **not** write the
//! migrated project back to disk (Python's `load()` saves on migration).
//! Callers that want that behaviour must call `save()` explicitly —
//! `vietkara-storage` does this. `save()` also does **not** set
//! `updated_at`; the caller/storage layer owns timestamps.

pub mod edit;
pub mod lyrics;
pub mod timing;
pub mod timeline;
pub mod tapping;
pub mod settings;

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use thiserror::Error;
use uuid::Uuid;

pub const CURRENT_SCHEMA_VERSION: u32 = 2;
pub const MAX_WORDS: usize = 30_000;
pub const MAX_ANCHORS: usize = 1_000;
pub const MAX_WORD_TEXT_CHARS: usize = 200;
pub const MAX_SECTION_NAME_CHARS: usize = 40;

pub const ROLES: &[&str] = &[
    "all", "male", "female", "voice_c", "voice_d", "voice_e", "voice_f", "unknown",
];

/// True iff `value` is exactly 32 lowercase hex chars, the ID format
/// Python uses everywhere (`uuid.uuid4().hex`, validated with
/// `re.fullmatch(r"[0-9a-f]{32}", ...)`).
pub fn is_hex32(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    #[serde(default = "default_schema_version", rename = "schema")]
    pub schema: u32,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub reference: Option<serde_json::Value>,
    #[serde(default)]
    pub beat: Option<serde_json::Value>,
    #[serde(default)]
    pub anchors: Vec<Anchor>,
    #[serde(default, deserialize_with = "settings::deserialize_settings")]
    pub settings: settings::Settings,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub alignment: Option<serde_json::Value>,
    #[serde(default)]
    pub scenes: Vec<serde_json::Value>,
    #[serde(default)]
    pub words: Vec<Word>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Word {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub start: f64,
    #[serde(default)]
    pub end: f64,
    #[serde(default)]
    pub line_uid: String,
    #[serde(default = "default_role")]
    pub role: String,
    #[serde(default)]
    pub line: u32,
    /// Python always writes `estimated` (bool) in `validate_words`.
    #[serde(default)]
    pub estimated: bool,
    /// Only written when true in Python (`if word.get("locked")`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locked: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manual: Option<bool>,
    /// `#RRGGBB`, stored lowercase. Validated, not silently coerced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_color: Option<String>,
    /// Lyric track 0..=5 in Python (`range(6)`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row: Option<u8>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Anchor {
    #[serde(default)]
    pub r#ref: f64,
    #[serde(default)]
    pub beat: f64,
}

#[derive(Debug, Error)]
pub enum ProjectError {
    #[error("Thiết lập không hợp lệ: {0}")]
    InvalidSettings(String),
    #[error("Không đọc được project: {0}")]
    Io(#[from] std::io::Error),
    #[error("Project JSON không hợp lệ: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Project dùng schema mới hơn app hỗ trợ ({0})")]
    FutureSchema(u32),
    #[error("Mốc của từ {0} không hợp lệ")]
    InvalidWordTime(usize),
    #[error("Từ {0} vượt quá giới hạn 200 ký tự")]
    WordTextTooLong(usize),
    #[error("ID từ bị trùng: {0}")]
    DuplicateWordId(String),
    #[error("Vai hát không được hỗ trợ: {0}")]
    UnknownRole(String),
    #[error("Màu nhãn của từ {0} phải có dạng #RRGGBB")]
    InvalidLabelColor(usize),
    #[error("Track lời của từ {0} phải nằm trong khoảng 0 đến 5")]
    InvalidRow(usize),
    #[error("Tên section của từ {0} vượt quá giới hạn 40 ký tự")]
    SectionNameTooLong(usize),
    #[error("Project vượt quá 30.000 từ")]
    TooManyWords,
    #[error("Project vượt quá 1.000 mốc đồng bộ")]
    TooManyAnchors,
    #[error("Mốc đồng bộ số {0} không hợp lệ")]
    InvalidAnchor(usize),
    #[error("Mốc tham chiếu và beat phải tăng cùng nhau, cách nhau ít nhất 0,02 giây")]
    UnorderedAnchors,
}

fn round4(value: f64) -> f64 {
    (value * 10_000.0).round() / 10_000.0
}

fn is_valid_label_color(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 7 && bytes[0] == b'#' && bytes[1..].iter().all(|b| b.is_ascii_hexdigit())
}

impl Project {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            schema: CURRENT_SCHEMA_VERSION,
            id: new_id(),
            name: name.into(),
            reference: None,
            beat: None,
            anchors: Vec::new(),
            settings: settings::Settings::default(),
            text: String::new(),
            alignment: None,
            scenes: Vec::new(),
            words: Vec::new(),
            extra: Default::default(),
        }
    }

    /// Load and migrate a project file. Does not write anything back:
    /// unlike Python `app.load()`, migration is in-memory only.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ProjectError> {
        let mut project: Self = serde_json::from_slice(&std::fs::read(path)?)?;
        if project.schema > CURRENT_SCHEMA_VERSION {
            return Err(ProjectError::FutureSchema(project.schema));
        }
        project.migrate()?;
        project.validate()?;
        Ok(project)
    }

    /// Normalise this project to schema 2, following Python
    /// `timing.validate_words` / `validate_anchors` and `app.load()`.
    /// Idempotent: running it on an already-normalised project is a no-op
    /// (except for re-sorting anchors and re-rounding times).
    pub fn migrate(&mut self) -> Result<(), ProjectError> {
        if self.schema == 0 {
            self.schema = 1;
        }
        if self.id.is_empty() {
            self.id = new_id();
        }
        // Keep the original words if migration fails, so a failed
        // migrate() does not silently drop the project content.
        self.words = migrate_words(self.words.clone())?;
        // Python sorts anchors by `ref` before validating the 0.02s gaps.
        self.anchors.sort_by(|a, b| {
            a.r#ref
                .partial_cmp(&b.r#ref)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| {
                    a.beat
                        .partial_cmp(&b.beat)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
        });
        self.schema = CURRENT_SCHEMA_VERSION;
        Ok(())
    }

    pub fn validate(&self) -> Result<(), ProjectError> {
        if !self.settings.font_size.is_finite()
            || !(20.0..=240.0).contains(&self.settings.font_size)
        {
            return Err(ProjectError::InvalidSettings(
                "font_size phải hữu hạn trong 20..240".into(),
            ));
        }
        if self.words.len() > MAX_WORDS {
            return Err(ProjectError::TooManyWords);
        }
        let mut ids = HashSet::new();
        for (index, word) in self.words.iter().enumerate() {
            let n = index + 1;
            if !word.start.is_finite()
                || !word.end.is_finite()
                || word.start < 0.0
                || word.end <= word.start
            {
                return Err(ProjectError::InvalidWordTime(n));
            }
            if word.text.chars().count() > MAX_WORD_TEXT_CHARS {
                return Err(ProjectError::WordTextTooLong(n));
            }
            if !word.id.is_empty() && !ids.insert(word.id.as_str()) {
                return Err(ProjectError::DuplicateWordId(word.id.clone()));
            }
            if !ROLES.contains(&word.role.as_str()) {
                return Err(ProjectError::UnknownRole(word.role.clone()));
            }
            if let Some(color) = &word.label_color
                && !is_valid_label_color(color)
            {
                return Err(ProjectError::InvalidLabelColor(n));
            }
            if let Some(row) = word.row
                && row > 5
            {
                return Err(ProjectError::InvalidRow(n));
            }
            if let Some(name) = &word.section_name
                && name.chars().count() > MAX_SECTION_NAME_CHARS
            {
                return Err(ProjectError::SectionNameTooLong(n));
            }
        }
        if self.anchors.len() > MAX_ANCHORS {
            return Err(ProjectError::TooManyAnchors);
        }
        let mut previous: Option<&Anchor> = None;
        for (index, anchor) in self.anchors.iter().enumerate() {
            if !anchor.r#ref.is_finite()
                || !anchor.beat.is_finite()
                || anchor.r#ref < 0.0
                || anchor.beat < 0.0
            {
                return Err(ProjectError::InvalidAnchor(index + 1));
            }
            if let Some(previous) = previous
                && (anchor.r#ref - previous.r#ref < 0.02 || anchor.beat - previous.beat < 0.02)
            {
                return Err(ProjectError::UnorderedAnchors);
            }
            previous = Some(anchor);
        }
        Ok(())
    }

    /// Save atomically: normalise (as Python `save()` does via
    /// `validate_words`), validate, write a uniquely-named temp file in
    /// the destination directory, then rename over the destination.
    /// A failure leaves the previous file untouched and removes the temp.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), ProjectError> {
        let mut normalised = self.clone();
        normalised.migrate()?;
        normalised.validate()?;
        let json = serde_json::to_vec_pretty(&normalised)?;
        let path = path.as_ref();
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "project.json".to_owned());
        let temp_path = parent.join(format!(
            ".{file_name}.tmp-{}-{}",
            std::process::id(),
            new_id()
        ));
        let result = (|| -> Result<(), ProjectError> {
            std::fs::write(&temp_path, json)?;
            std::fs::rename(&temp_path, path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temp_path);
        }
        result
    }
}

/// Mirror of Python `timing.validate_words`, including its order of
/// operations: timing check → strip/truncate text (drop empty words) →
/// role coercion → ID assignment → one `line_uid` per `line`.
fn migrate_words(words: Vec<Word>) -> Result<Vec<Word>, ProjectError> {
    if words.len() > MAX_WORDS {
        return Err(ProjectError::TooManyWords);
    }
    let mut out: Vec<Word> = Vec::with_capacity(words.len());
    let mut used_ids: HashSet<String> = HashSet::new();
    let mut used_line_ids: HashSet<String> = HashSet::new();
    let mut line_ids: HashMap<u32, String> = HashMap::new();

    for (index, mut word) in words.into_iter().enumerate() {
        let n = index + 1;
        if !word.start.is_finite()
            || !word.end.is_finite()
            || word.start < 0.0
            || word.end <= word.start
        {
            return Err(ProjectError::InvalidWordTime(n));
        }
        let text: String = word.text.trim().chars().take(MAX_WORD_TEXT_CHARS).collect();
        if text.is_empty() {
            continue;
        }
        word.text = text;
        word.start = round4(word.start);
        word.end = round4(word.end);
        if !ROLES.contains(&word.role.as_str()) {
            word.role = "all".to_owned();
        }
        if !is_hex32(&word.id) || used_ids.contains(&word.id) {
            word.id = new_id();
        }
        used_ids.insert(word.id.clone());
        let line_uid = match line_ids.get(&word.line) {
            Some(existing) => existing.clone(),
            None => {
                let candidate = word.line_uid.clone();
                let chosen = if is_hex32(&candidate) && !used_line_ids.contains(&candidate) {
                    candidate
                } else {
                    new_id()
                };
                used_line_ids.insert(chosen.clone());
                line_ids.insert(word.line, chosen.clone());
                chosen
            }
        };
        word.line_uid = line_uid;
        // Python only keeps these flags when truthy.
        if word.locked == Some(false) {
            word.locked = None;
        }
        if word.section == Some(false) {
            word.section = None;
        }
        if word.manual == Some(false) {
            word.manual = None;
        }
        if let Some(name) = word.section_name.take() {
            let cleaned: String = name.trim().chars().take(MAX_SECTION_NAME_CHARS).collect();
            if !cleaned.is_empty() {
                word.section_name = Some(cleaned);
            }
        }
        if let Some(color) = word.label_color.take() {
            if !is_valid_label_color(&color) {
                return Err(ProjectError::InvalidLabelColor(n));
            }
            word.label_color = Some(color.to_ascii_lowercase());
        }
        if let Some(row) = word.row
            && row > 5
        {
            return Err(ProjectError::InvalidRow(n));
        }
        out.push(word);
    }
    Ok(out)
}

fn default_schema_version() -> u32 {
    // A project without a `schema` field is schema 1 in Python
    // (`project.get("schema", 1)`). The previous default of 2 skipped
    // migration for exactly those projects.
    1
}

/// 32 lowercase hex chars, no hyphens — `uuid.uuid4().hex` in Python.
/// The previous `Uuid::to_string()` produced hyphenated UUIDs, which
/// Python rejects as project/job/media IDs (`[0-9a-f]{32}`).
pub fn new_id() -> String {
    Uuid::new_v4().simple().to_string()
}

fn default_role() -> String {
    "all".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_id_is_32_lowercase_hex() {
        for _ in 0..20 {
            let id = new_id();
            assert!(is_hex32(&id), "{id}");
            assert_eq!(id.len(), 32);
        }
    }

    #[test]
    fn imports_python_schema_two_and_preserves_unknown_fields() {
        let source = r##"{
            "id":"0123456789abcdef0123456789abcdef",
            "schema":2,
            "name":"Bài hát tiếng Việt",
            "reference":{"filename":"source.wav","duration":12.5},
            "beat":{"filename":"beat.wav"},
            "anchors":[{"ref":1.0,"beat":1.2}],
            "settings":{"font_size":110},
            "text":"Xin chào",
            "alignment":null,
            "scenes":[],
            "words":[{"id":"abc","line_uid":"line-a","text":"Xin","start":0.2,"end":0.6,"line":0,"role":"all","locked":true}],
            "future_python_field":{"keep":true}
        }"##;

        let project: Project = serde_json::from_str(source).unwrap();
        assert_eq!(project.schema, CURRENT_SCHEMA_VERSION);
        assert_eq!(project.words[0].text, "Xin");
        assert!(project.extra.contains_key("future_python_field"));
        let round_trip = serde_json::to_value(project).unwrap();
        assert_eq!(round_trip["words"][0]["locked"], true);
        assert_eq!(round_trip["future_python_field"]["keep"], true);
    }

    #[test]
    fn schema_less_project_defaults_to_one() {
        let project: Project =
            serde_json::from_str(r#"{"id":"0123456789abcdef0123456789abcdef","name":"x"}"#)
                .unwrap();
        assert_eq!(project.schema, 1);
    }

    #[test]
    fn migrates_v1_fixture_like_python() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/project-v1.json"
        );
        let project = Project::load(path).unwrap();
        assert_eq!(project.schema, 2);
        // 7 input words, 1 empty-text word dropped.
        assert_eq!(project.words.len(), 5);
        // Anchors were unsorted in the fixture; Python sorts by `ref`.
        assert_eq!(
            project.anchors,
            vec![
                Anchor {
                    r#ref: 1.0,
                    beat: 1.2
                },
                Anchor {
                    r#ref: 2.0,
                    beat: 2.2
                },
            ]
        );
        for word in &project.words {
            assert!(is_hex32(&word.id), "{}", word.id);
            assert!(is_hex32(&word.line_uid), "{}", word.line_uid);
        }
        let ids: HashSet<_> = project.words.iter().map(|w| &w.id).collect();
        assert_eq!(ids.len(), 5);
        // Same `line` shares one `line_uid`, different lines do not.
        assert_eq!(project.words[0].line_uid, project.words[1].line_uid);
        assert_eq!(project.words[2].line_uid, project.words[3].line_uid);
        assert_ne!(project.words[0].line_uid, project.words[2].line_uid);
        assert_ne!(project.words[2].line_uid, project.words[4].line_uid);
        // Role coercion, time rounding, typed optional fields.
        assert_eq!(project.words[1].role, "all");
        assert_eq!(project.words[0].start, 0.2);
        assert_eq!(project.words[0].end, 0.6);
        assert_eq!(project.words[3].label_color.as_deref(), Some("#ff0000"));
        assert_eq!(project.words[3].row, Some(2));
        assert_eq!(project.words[3].section_name.as_deref(), Some("Điệp khúc"));
        assert_eq!(project.words[2].locked, Some(true));
        assert_eq!(project.words[4].manual, Some(true));
        assert_eq!(project.words[4].text, "Bỏ");
        assert!(project.extra.contains_key("future_python_field"));
    }

    #[test]
    fn v2_fixture_round_trips() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/project-v2.json"
        );
        let project = Project::load(path).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("project.json");
        project.save(&out).unwrap();
        // No temp files left behind by the atomic save.
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        let reloaded = Project::load(&out).unwrap();
        assert_eq!(
            serde_json::to_value(&project).unwrap(),
            serde_json::to_value(&reloaded).unwrap()
        );
    }

    #[test]
    fn rejects_a_project_from_a_newer_schema() {
        let source = r#"{"schema":999,"id":"p","name":"future"}"#;
        let path = std::env::temp_dir().join(format!("vietkara-{}.json", new_id()));
        std::fs::write(&path, source).unwrap();
        let error = Project::load(&path).unwrap_err();
        std::fs::remove_file(path).unwrap();
        assert!(matches!(error, ProjectError::FutureSchema(999)));
    }

    #[test]
    fn rejects_invalid_timing_and_duplicate_ids() {
        let mut project = Project::new("Fixture");
        let word = Word {
            id: "same".into(),
            text: "Xin".into(),
            start: 1.0,
            end: 0.9,
            line_uid: "line-a".into(),
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
        };
        project.words.push(word.clone());
        assert!(matches!(
            project.validate(),
            Err(ProjectError::InvalidWordTime(1))
        ));

        project.words[0].end = 1.2;
        // The duplicate must itself have valid timing, otherwise
        // validation (correctly) reports InvalidWordTime(2) first.
        let mut duplicate = word;
        duplicate.end = 1.2;
        project.words.push(duplicate);
        assert!(matches!(
            project.validate(),
            Err(ProjectError::DuplicateWordId(_))
        ));
    }

    #[test]
    fn rejects_invalid_label_color_and_row_on_migrate() {
        let mut project = Project::new("Fixture");
        project.words.push(Word {
            id: new_id(),
            text: "Xin".into(),
            start: 0.0,
            end: 1.0,
            line_uid: new_id(),
            role: "all".into(),
            line: 0,
            estimated: false,
            locked: None,
            section: None,
            section_name: None,
            manual: None,
            label_color: Some("red".into()),
            row: Some(9),
            extra: Default::default(),
        });
        assert!(matches!(
            project.migrate(),
            Err(ProjectError::InvalidLabelColor(1))
        ));
        project.words[0].label_color = None;
        assert!(matches!(
            project.migrate(),
            Err(ProjectError::InvalidRow(1))
        ));
    }
}
