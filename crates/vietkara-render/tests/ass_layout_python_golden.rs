use serde_json::Value;
use vietkara_domain::Project;
use vietkara_render::{ass::build_lyrics_ass, layout::TextMeasurer, layout::display_plan};

struct FixedWidth;

impl TextMeasurer for FixedWidth {
    fn width(&self, text: &str, _family: &str, size: f64, _italic: bool) -> f64 {
        text.chars().count() as f64 * size * 0.5
    }
}

#[test]
fn complete_ass_events_match_python_layout_baseline_with_fixed_width_metrics() {
    let fixture: Value = serde_json::from_str(include_str!(
        "fixtures/python-c8dae55-layout-fixed-width.json"
    ))
    .unwrap();
    let project: Project = serde_json::from_value(fixture["project"].clone()).unwrap();
    let plan = display_plan(&project, &FixedWidth);
    let actual = build_lyrics_ass(&project, &plan);
    if let Ok(path) = std::env::var("VIETKARA_ASS_LAYOUT_DUMP") {
        std::fs::write(path, &actual).expect("write generated ASS for comparison");
    }
    let expected = include_str!("fixtures/python-c8dae55-layout-fixed-width.ass");
    let expected = expected.replace("\r\n", "\n");
    let icon_signature = |text: &str| {
        let mut icons = text
            .lines()
            .filter(|line| line.starts_with("Dialogue: 2,"))
            .map(|line| {
                let (prefix, tagged) = line.split_once('{').expect("ASS event has override tags");
                let (tags, drawing) = tagged.split_once('}').expect("ASS event closes tags");
                let color = tags
                    .split("\\1c")
                    .nth(1)
                    .and_then(|value| value.split(['\\', '}']).next())
                    .expect("role artwork event has a color");
                (prefix.to_owned(), color.to_owned(), drawing.to_owned())
            })
            .collect::<Vec<_>>();
        icons.sort();
        icons
    };
    assert_eq!(icon_signature(&actual), icon_signature(&expected));

    let comparable = |text: &str| {
        let mut lines = text
            .lines()
            .filter(|line| !line.starts_with("Style: Main,"))
            .filter(|line| {
                !(line.starts_with("Dialogue: ")
                    && line
                        .split_once(',')
                        .is_some_and(|(layer, _)| layer == "Dialogue: 2"))
            })
            .map(|line| {
                if let Some((prefix, tagged)) = line.split_once('{') {
                    let (_, drawing) = tagged.split_once('}').expect("ASS event closes tags");
                    format!("{prefix}{drawing}")
                } else {
                    line.to_owned()
                }
            })
            .collect::<Vec<_>>();
        lines.sort();
        lines
    };
    let actual_lines = comparable(&actual);
    let expected_lines = comparable(&expected);

    if actual_lines != expected_lines {
        let first_difference = actual_lines
            .iter()
            .zip(&expected_lines)
            .position(|(actual, expected)| actual != expected)
            .unwrap_or(actual_lines.len().min(expected_lines.len()));
        panic!(
            "ASS differs from Python at line {}\nRust: {:?}\nPython: {:?}\nline counts Rust={} Python={}",
            first_difference + 1,
            actual_lines.get(first_difference),
            expected_lines.get(first_difference),
            actual_lines.len(),
            expected_lines.len(),
        );
    }
}
