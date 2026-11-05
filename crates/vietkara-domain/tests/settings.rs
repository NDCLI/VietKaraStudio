use serde_json::json;
use vietkara_domain::{
    Project,
    edit::{Command, Editor},
    settings::Settings,
};

#[test]
fn project_load_normalizes_core_and_round_trips_future_settings() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Bài hát.json");
    let source = json!({"schema":2,"settings":{"font_size":"999", "font":"{Arial},", "future":{"nested":[1,true]}}});
    let bytes = serde_json::to_vec(&source).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    let project = Project::load(&path).unwrap();
    assert_eq!(project.settings.font_size, 240.);
    assert_eq!(project.settings.font, "Arial");
    assert_eq!(
        project.settings.extra["future"],
        source["settings"]["future"]
    );
    assert_eq!(
        std::fs::read(&path).unwrap(),
        bytes,
        "load must not rewrite source"
    );
    project.save(&path).unwrap();
    let reopened = Project::load(path).unwrap();
    assert_eq!(reopened.settings, project.settings);
}

#[test]
fn invalid_settings_load_and_save_leave_original_file_untouched() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("project.json");
    for settings in [
        json!(null),
        json!({"render_format":"exe"}),
        json!({"audio_normalize":"true"}),
        json!({"font_size":"Infinity"}),
    ] {
        let bytes = serde_json::to_vec(&json!({"schema":2,"settings":settings})).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        assert!(Project::load(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
    let mut project = Project::new("safe");
    project.save(&path).unwrap();
    let original = std::fs::read(&path).unwrap();
    project.settings.font_size = f64::NAN;
    assert!(project.save(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn typed_settings_commands_undo_redo_and_reject_invalid_candidate() {
    let mut editor = Editor::new(Project::new("test")).unwrap();
    let original = editor.project().settings.clone();
    let edited = Settings::from_value(json!({"aspect":"portrait","future":42})).unwrap();
    editor
        .execute(Command::SetSettings(edited.clone()))
        .unwrap();
    editor.undo().unwrap();
    let mut invalid = edited.clone();
    invalid.font_size = -1.;
    assert!(editor.execute(Command::SetSettings(invalid)).is_err());
    assert_eq!(editor.project().settings, original);
    assert!(editor.can_redo());
    editor.redo().unwrap();
    assert_eq!(editor.project().settings, edited);
}
