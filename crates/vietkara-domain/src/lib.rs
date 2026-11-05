use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;
use uuid::Uuid;

pub const CURRENT_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    #[serde(default = "default_schema_version", rename = "schema")]
    pub schema: u32,
    #[serde(default = "new_id")]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub reference: Option<serde_json::Value>,
    #[serde(default)]
    pub beat: Option<serde_json::Value>,
    #[serde(default)]
    pub anchors: Vec<Anchor>,
    #[serde(default)]
    pub settings: serde_json::Value,
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
    #[serde(default = "new_id")]
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
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Anchor {
    #[serde(default)]
    pub r#ref: f64,
    #[serde(default)]
    pub beat: f64,
}

#[derive(Debug, Error)]
pub enum ProjectError {
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
    #[error("Project vượt quá 30.000 từ")]
    TooManyWords,
    #[error("Project vượt quá 1.000 mốc đồng bộ")]
    TooManyAnchors,
    #[error("Mốc đồng bộ số {0} không hợp lệ")]
    InvalidAnchor(usize),
    #[error("Mốc tham chiếu và beat phải tăng cùng nhau, cách nhau ít nhất 0,02 giây")]
    UnorderedAnchors,
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
            settings: serde_json::Value::Object(Default::default()),
            text: String::new(),
            alignment: None,
            scenes: Vec::new(),
            words: Vec::new(),
            extra: Default::default(),
        }
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self, ProjectError> {
        let mut project: Self = serde_json::from_slice(&std::fs::read(path)?)?;
        if project.schema > CURRENT_SCHEMA_VERSION {
            return Err(ProjectError::FutureSchema(project.schema));
        }
        project.validate()?;
        // Schema-less projects from the Python app are imported as version 1.
        project.schema = CURRENT_SCHEMA_VERSION;
        Ok(project)
    }

    pub fn validate(&self) -> Result<(), ProjectError> {
        if self.words.len() > 30_000 {
            return Err(ProjectError::TooManyWords);
        }
        let mut ids = std::collections::HashSet::new();
        for (index, word) in self.words.iter().enumerate() {
            if !word.start.is_finite()
                || !word.end.is_finite()
                || word.start < 0.0
                || word.end <= word.start
            {
                return Err(ProjectError::InvalidWordTime(index + 1));
            }
            if word.text.len() > 200 {
                return Err(ProjectError::WordTextTooLong(index + 1));
            }
            if !word.id.is_empty() && !ids.insert(word.id.as_str()) {
                return Err(ProjectError::DuplicateWordId(word.id.clone()));
            }
            if !matches!(
                word.role.as_str(),
                "all"
                    | "male"
                    | "female"
                    | "voice_c"
                    | "voice_d"
                    | "voice_e"
                    | "voice_f"
                    | "unknown"
            ) {
                return Err(ProjectError::UnknownRole(word.role.clone()));
            }
        }
        if self.anchors.len() > 1_000 {
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
            if let Some(previous) = previous {
                if anchor.r#ref - previous.r#ref < 0.02 || anchor.beat - previous.beat < 0.02 {
                    return Err(ProjectError::UnorderedAnchors);
                }
            }
            previous = Some(anchor);
        }
        Ok(())
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), ProjectError> {
        self.validate()?;
        let json = serde_json::to_vec_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }
}

fn default_schema_version() -> u32 {
    CURRENT_SCHEMA_VERSION
}
fn new_id() -> String {
    Uuid::new_v4().to_string()
}
fn default_role() -> String {
    "all".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

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
            extra: Default::default(),
        };
        project.words.push(word.clone());
        assert!(matches!(
            project.validate(),
            Err(ProjectError::InvalidWordTime(1))
        ));

        project.words[0].end = 1.2;
        project.words.push(word);
        assert!(matches!(
            project.validate(),
            Err(ProjectError::DuplicateWordId(_))
        ));
    }
}
