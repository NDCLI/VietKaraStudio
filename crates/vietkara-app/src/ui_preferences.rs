use crate::WorkspaceSize;
use serde_json::Value;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AiPreferences {
    pub(crate) device: String,
    pub(crate) model: String,
}

impl Default for AiPreferences {
    fn default() -> Self {
        Self {
            device: "auto".into(),
            model: "medium".into(),
        }
    }
}

fn preferences_path() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .map(|home| home.join(".config"))
            })
    }?;
    Some(base.join("VietKaraStudio").join("ui-preferences.json"))
}

pub(crate) fn load_workspace_layout() -> Option<WorkspaceSize> {
    let path = preferences_path()?;
    read_workspace_layout(&path).map(migrate_workspace_layout)
}

fn migrate_workspace_layout(mut layout: WorkspaceSize) -> WorkspaceSize {
    // Older GPUI defaults used a 204–270 px inspector. Move those layouts to
    // the width needed by the current two-tab controls while preserving wider
    // layouts that the user has resized deliberately.
    if layout.right < 280.0 {
        layout.right = 300.0;
    }
    layout
}

fn read_workspace_layout(path: &std::path::Path) -> Option<WorkspaceSize> {
    let bytes = std::fs::read(path).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    workspace_layout_from_preferences(&value)
}

fn workspace_layout_from_preferences(value: &serde_json::Value) -> Option<WorkspaceSize> {
    let layout = WorkspaceSize {
        left: value.get("left")?.as_f64()? as f32,
        preview: value.get("preview")?.as_f64()? as f32,
        right: value.get("right")?.as_f64()? as f32,
    };
    (layout.left.is_finite()
        && layout.preview.is_finite()
        && layout.right.is_finite()
        && (170.0..=600.0).contains(&layout.left)
        && (240.0..=1000.0).contains(&layout.preview)
        && (204.0..=600.0).contains(&layout.right))
    .then_some(layout)
}

pub(crate) fn save_workspace_layout(layout: WorkspaceSize) -> Result<(), String> {
    let path = preferences_path().ok_or_else(|| "Không tìm thấy thư mục preferences".to_owned())?;
    write_workspace_layout(&path, layout)
}

pub(crate) fn load_ai_preferences() -> AiPreferences {
    let Some(path) = preferences_path() else {
        return AiPreferences::default();
    };
    let Ok(bytes) = std::fs::read(path) else {
        return AiPreferences::default();
    };
    let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
        return AiPreferences::default();
    };
    ai_preferences_from_value(&value)
}

fn ai_preferences_from_value(value: &Value) -> AiPreferences {
    let mut preferences = AiPreferences::default();
    if let Some(device) = value.get("ai_device").and_then(Value::as_str)
        && ["auto", "cpu"].contains(&device)
    {
        preferences.device = device.to_owned();
    }
    if let Some(model) = value.get("ai_model").and_then(Value::as_str)
        && ["small", "medium", "large-v3-turbo"].contains(&model)
    {
        preferences.model = model.to_owned();
    }
    preferences
}

pub(crate) fn save_ai_preferences(preferences: &AiPreferences) -> Result<(), String> {
    if !["auto", "cpu"].contains(&preferences.device.as_str())
        || !["small", "medium", "large-v3-turbo"].contains(&preferences.model.as_str())
    {
        return Err("Cấu hình AI không hợp lệ".into());
    }
    let path = preferences_path().ok_or_else(|| "Không tìm thấy thư mục preferences".to_owned())?;
    let parent = path
        .parent()
        .ok_or_else(|| "Đường dẫn preferences không hợp lệ".to_owned())?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let mut value = std::fs::read(&path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    value["ai_device"] = Value::String(preferences.device.clone());
    value["ai_model"] = Value::String(preferences.model.clone());
    let bytes = serde_json::to_vec_pretty(&value).map_err(|error| error.to_string())?;
    std::fs::write(&path, bytes).map_err(|error| error.to_string())
}

fn write_workspace_layout(path: &std::path::Path, layout: WorkspaceSize) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "Đường dẫn preferences không hợp lệ".to_owned())?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let mut value = std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    value["left"] = serde_json::json!(layout.left);
    value["preview"] = serde_json::json!(layout.preview);
    value["right"] = serde_json::json!(layout.right);
    let contents = serde_json::to_vec(&value).map_err(|error| error.to_string())?;
    std::fs::write(path, contents).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_preferences_reject_non_finite_and_out_of_range_values() {
        let invalid = serde_json::json!({ "left": 10000, "preview": 500, "right": 240 });
        assert_eq!(workspace_layout_from_preferences(&invalid), None);
        let non_finite = serde_json::json!({ "left": 120, "preview": 1e100, "right": 240 });
        assert_eq!(workspace_layout_from_preferences(&non_finite), None);
    }

    #[test]
    fn workspace_preference_payload_round_trips_all_columns() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ui-preferences.json");
        let expected = WorkspaceSize {
            left: 310.0,
            preview: 680.0,
            right: 355.0,
        };
        write_workspace_layout(&path, expected).unwrap();
        assert_eq!(read_workspace_layout(&path), Some(expected));
    }

    #[test]
    fn older_narrow_inspector_preferences_expand_on_load() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ui-preferences.json");
        std::fs::write(&path, r#"{"left":210,"preview":410,"right":236}"#).unwrap();

        let loaded = migrate_workspace_layout(read_workspace_layout(&path).unwrap());

        assert_eq!(loaded.left, 210.0);
        assert_eq!(loaded.preview, 410.0);
        assert_eq!(loaded.right, 300.0);
    }

    #[test]
    fn saving_workspace_layout_preserves_ai_preferences() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ui-preferences.json");
        std::fs::write(
            &path,
            br#"{"ai_device":"cpu","ai_model":"small","left":300,"preview":600,"right":300}"#,
        )
        .unwrap();
        write_workspace_layout(
            &path,
            WorkspaceSize {
                left: 320.0,
                preview: 650.0,
                right: 340.0,
            },
        )
        .unwrap();
        let value: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(value["ai_device"], "cpu");
        assert_eq!(value["ai_model"], "small");
    }

    #[test]
    fn ai_preferences_validate_known_options_and_keep_defaults_for_bad_values() {
        let preferences = ai_preferences_from_value(&serde_json::json!({
            "ai_device": "cuda-invented",
            "ai_model": "unknown",
        }));
        assert_eq!(preferences, AiPreferences::default());
        assert_eq!(
            ai_preferences_from_value(&serde_json::json!({
                "ai_device": "cpu",
                "ai_model": "small",
            })),
            AiPreferences {
                device: "cpu".into(),
                model: "small".into(),
            }
        );
    }
}
