use serde_json::json;
use vietkara_domain::{Project, Word};
use vietkara_render::{
    ass::build_lyrics_ass,
    layout::{DisplayPlan, DisplayRow, PlannedWord},
};

fn word(id: &str, text: &str, start: f64, end: f64) -> Word {
    Word {
        id: id.to_owned(),
        text: text.to_owned(),
        start,
        end,
        line_uid: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
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
    }
}

#[test]
fn simple_karaoke_ass_matches_python_baseline_c8dae55() {
    let mut project = Project::new("ASS golden");
    project.settings.font = "Arial".into();
    project.settings.font_size = 72.0;
    for (key, value) in [
        ("display_mode", json!("karaoke")),
        ("singing_mode", json!("solo")),
        ("role_icons", json!(false)),
        ("sung_gradient", json!(false)),
        ("line_animation", json!("none")),
        ("horizontal_scale", json!(100)),
        ("outline_width", json!(2)),
        ("shadow", json!(0)),
        ("subtitle_box", json!(false)),
        ("sweep_feather", json!(0)),
        ("sweep_glow", json!(0)),
    ] {
        project.settings.extra.insert(key.into(), value);
    }
    let first = word("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "Xin", 1.0, 1.5);
    let second = word("cccccccccccccccccccccccccccccccc", "chào", 1.5, 2.0);
    project.words = vec![first.clone(), second.clone()];

    let plan = DisplayPlan {
        width: 1920,
        height: 1080,
        font: "Arial".into(),
        rows: vec![DisplayRow {
            index: 0,
            line: 0,
            slot: 0,
            start: 1.0,
            end: 2.0,
            show_start: 0.0,
            show_end: 3.0,
            x: 100.0,
            y: 800.0,
            width: 300.0,
            height: 86.4,
            font_size: 72.0,
            align: "center".into(),
            role: "all".into(),
            role_marker: None,
            text: "Xin chào".into(),
            section_start: true,
            section_end: true,
            words: vec![
                PlannedWord {
                    source_index: 0,
                    word: first,
                    display_text: "Xin".into(),
                    start: 1.0,
                    end: 1.5,
                    x: 100.0,
                    width: 140.0,
                },
                PlannedWord {
                    source_index: 1,
                    word: second,
                    display_text: "chào".into(),
                    start: 1.5,
                    end: 2.0,
                    x: 260.0,
                    width: 140.0,
                },
            ],
        }],
        title: None,
    };

    let actual = build_lyrics_ass(&project, &plan);
    if let Ok(path) = std::env::var("VIETKARA_ASS_GOLDEN_DUMP") {
        std::fs::write(path, &actual).expect("write ASS golden comparison output");
    }
    assert!(actual.contains("PlayResX: 1920\nPlayResY: 1080"));
    let dialogue = actual
        .lines()
        .filter(|line| line.starts_with("Dialogue:"))
        .collect::<Vec<_>>();
    assert_eq!(dialogue.len(), 4);
    assert!(dialogue.iter().any(|line| line.ends_with("}Xin")));
    assert!(dialogue.iter().any(|line| line.ends_with("}chào")));
    assert!(
        dialogue
            .iter()
            .all(|line| line.contains("0:00:00.00,0:00:03.00"))
    );
}
