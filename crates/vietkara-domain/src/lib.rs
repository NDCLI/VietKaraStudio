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
    // Preserve the previous prototype field if it ever appears in an early build.
    #[serde(default)]
    pub lyrics: Vec<LyricLine>,
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
            lyrics: Vec::new(),
            extra: Default::default(),
        }
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self, ProjectError> {
        let mut project: Self = serde_json::from_slice(&std::fs::read(path)?)?;
        if project.schema > CURRENT_SCHEMA_VERSION {
            return Err(ProjectError::FutureSchema(project.schema));
        }
        // Schema-less projects from the Python app are imported as version 1.
        project.schema = CURRENT_SCHEMA_VERSION;
        Ok(project)
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), ProjectError> {
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
}
