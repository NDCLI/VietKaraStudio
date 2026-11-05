use crate::WorkspaceSize;
use std::path::PathBuf;

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
    read_workspace_layout(&path)
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

fn write_workspace_layout(path: &std::path::Path, layout: WorkspaceSize) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "Đường dẫn preferences không hợp lệ".to_owned())?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let contents = serde_json::to_vec(&serde_json::json!({
        "left": layout.left,
        "preview": layout.preview,
        "right": layout.right,
    }))
    .map_err(|error| error.to_string())?;
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
}
