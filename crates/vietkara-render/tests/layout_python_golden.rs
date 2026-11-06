use serde_json::Value;
use vietkara_domain::Project;
use vietkara_render::layout::{DisplayPlan, TextMeasurer, display_plan};

struct FixedWidth;

impl TextMeasurer for FixedWidth {
    fn width(&self, text: &str, _family: &str, size: f64, _italic: bool) -> f64 {
        text.chars().count() as f64 * size * 0.5
    }
}

fn close(actual: f64, expected: f64, tolerance: f64, label: &str) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{label}: Rust={actual}, Python={expected}, tolerance={tolerance}"
    );
}

fn projected(plan: &DisplayPlan) -> Value {
    serde_json::json!({
        "width": plan.width,
        "height": plan.height,
        "rows": plan.rows.iter().map(|row| serde_json::json!({
            "index": row.index,
            "line": row.line,
            "slot": row.slot,
            "start": row.start,
            "end": row.end,
            "show_start": row.show_start,
            "show_end": row.show_end,
            "x": row.x,
            "y": row.y,
            "width": row.width,
            "height": row.height,
            "font_size": row.font_size,
            "align": row.align,
            "role": row.role,
            "role_icon": row.role_marker.as_ref().map(|marker| serde_json::json!({
                "x": marker.x,
                "y": marker.y,
                "size": marker.size,
                "width": marker.width,
                "role": marker.role,
                "color": marker.color,
            })),
            "text": row.text,
            "section_start": row.section_start,
            "section_end": row.section_end,
            "words": row.words.iter().map(|word| serde_json::json!({
                "text": word.word.text,
                "display_text": word.display_text,
                "start": word.start,
                "end": word.end,
                "x": word.x,
                "width": word.width,
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

fn projected_title(plan: &DisplayPlan) -> Value {
    let Some(title) = plan.title.as_ref() else {
        return Value::Null;
    };
    serde_json::json!({
        "start": title.start,
        "end": title.end,
        "effect_seconds": title.effect_seconds,
        "enter": title.enter,
        "exit": title.exit,
        "color": title.color,
        "outline": title.outline,
        "outline_width": title.outline_width,
        "shadow": title.shadow,
        "shadow_color": title.shadow_color,
        "weight": title.weight,
        "italic": title.italic,
        "font": title.font,
        "rows": title.rows.iter().map(|row| serde_json::json!({
            "text": row.text,
            "display_text": row.display_text,
            "x": row.x,
            "y": row.y,
            "width": row.width,
            "size": row.size,
        })).collect::<Vec<_>>(),
        "template": title.template.as_ref().map(|template| serde_json::json!({
            "x": template.x,
            "y": template.y,
            "width": template.width,
            "height": template.height,
            "colors": template.template.colors,
            "opacity": template.template.opacity,
            "border": template.template.border,
            "border_width": template.template.border_width,
            "radius": template.template.radius,
        })),
    })
}

fn projected_countdowns(plan: &DisplayPlan) -> Value {
    serde_json::json!(
        plan.countdowns
            .iter()
            .map(|item| serde_json::json!({
                "start": item.start,
                "end": item.end,
                "x": item.x,
                "y": item.y,
                "size": item.size,
            }))
            .collect::<Vec<_>>()
    )
}

#[test]
fn layout_geometry_matches_python_baseline_c8dae55_with_fixed_width_metrics() {
    let fixture: Value = serde_json::from_str(include_str!(
        "fixtures/python-c8dae55-layout-fixed-width.json"
    ))
    .unwrap();
    let project: Project = serde_json::from_value(fixture["project"].clone()).unwrap();
    let expected = &fixture["expected"];
    let plan = display_plan(&project, &FixedWidth);
    let actual = projected(&plan);
    let actual_title = projected_title(&plan);
    let expected_title = &expected["title"];
    let actual_countdowns = projected_countdowns(&plan);
    let expected_countdowns = expected["countdowns"].as_array().unwrap();

    assert_eq!(actual["width"], expected["width"]);
    assert_eq!(actual["height"], expected["height"]);
    for key in ["start", "end", "effect_seconds"] {
        close(
            actual_title[key].as_f64().unwrap(),
            expected_title[key].as_f64().unwrap(),
            1e-7,
            &format!("title {key}"),
        );
    }
    for key in [
        "enter",
        "exit",
        "color",
        "outline",
        "shadow_color",
        "weight",
        "italic",
        "font",
    ] {
        assert_eq!(actual_title[key], expected_title[key], "title {key}");
    }
    let actual_countdowns = actual_countdowns.as_array().unwrap();
    assert_eq!(actual_countdowns.len(), expected_countdowns.len());
    for (index, (actual, expected)) in actual_countdowns
        .iter()
        .zip(expected_countdowns)
        .enumerate()
    {
        for key in ["start", "end"] {
            close(
                actual[key].as_f64().unwrap(),
                expected[key].as_f64().unwrap(),
                1e-7,
                &format!("countdown {index} {key}"),
            );
        }
        for key in ["x", "y", "size"] {
            close(
                actual[key].as_f64().unwrap(),
                expected[key].as_f64().unwrap(),
                0.002,
                &format!("countdown {index} {key}"),
            );
        }
    }
    let actual_title_rows = actual_title["rows"].as_array().unwrap();
    let expected_title_rows = expected_title["rows"].as_array().unwrap();
    assert_eq!(actual_title_rows.len(), expected_title_rows.len());
    for (row_index, (actual, expected)) in actual_title_rows
        .iter()
        .zip(expected_title_rows)
        .enumerate()
    {
        for key in ["text", "display_text"] {
            assert_eq!(actual[key], expected[key], "title row {row_index} {key}");
        }
        for key in ["x", "y", "width", "size"] {
            close(
                actual[key].as_f64().unwrap(),
                expected[key].as_f64().unwrap(),
                0.002,
                &format!("title row {row_index} {key}"),
            );
        }
    }
    let actual_box = &actual_title["template"];
    let expected_box = &expected_title["template"];
    for key in ["colors", "border"] {
        assert_eq!(actual_box[key], expected_box[key], "title template {key}");
    }
    for key in [
        "x",
        "y",
        "width",
        "height",
        "opacity",
        "border_width",
        "radius",
    ] {
        close(
            actual_box[key].as_f64().unwrap(),
            expected_box[key].as_f64().unwrap(),
            0.002,
            &format!("title template {key}"),
        );
    }
    let actual_rows = actual["rows"].as_array().unwrap();
    let expected_rows = expected["rows"].as_array().unwrap();
    assert_eq!(actual_rows.len(), expected_rows.len());

    for (row_index, (actual, expected)) in actual_rows.iter().zip(expected_rows).enumerate() {
        for key in [
            "index",
            "line",
            "slot",
            "align",
            "role",
            "text",
            "section_start",
            "section_end",
        ] {
            assert_eq!(actual[key], expected[key], "row {row_index}, field {key}");
        }
        assert_eq!(
            actual["role_icon"].is_null(),
            expected["role_icon"].is_null(),
            "row {row_index} role icon presence"
        );
        if !actual["role_icon"].is_null() {
            for key in ["role", "color"] {
                assert_eq!(
                    actual["role_icon"][key], expected["role_icon"][key],
                    "row {row_index} role icon {key}"
                );
            }
            for key in ["x", "y", "size", "width"] {
                close(
                    actual["role_icon"][key].as_f64().unwrap(),
                    expected["role_icon"][key].as_f64().unwrap(),
                    0.002,
                    &format!("row {row_index} role icon {key}"),
                );
            }
        }
        for key in ["start", "end", "show_start", "show_end"] {
            close(
                actual[key].as_f64().unwrap(),
                expected[key].as_f64().unwrap(),
                1e-7,
                &format!("row {row_index}, {key}"),
            );
        }
        for key in ["x", "y", "width", "height", "font_size"] {
            close(
                actual[key].as_f64().unwrap(),
                expected[key].as_f64().unwrap(),
                0.002,
                &format!("row {row_index}, {key}"),
            );
        }
        let actual_words = actual["words"].as_array().unwrap();
        let expected_words = expected["words"].as_array().unwrap();
        assert_eq!(
            actual_words.len(),
            expected_words.len(),
            "row {row_index} word count"
        );
        for (word_index, (actual, expected)) in actual_words.iter().zip(expected_words).enumerate()
        {
            for key in ["text", "display_text"] {
                assert_eq!(
                    actual[key], expected[key],
                    "row {row_index}, word {word_index}, {key}"
                );
            }
            for key in ["start", "end"] {
                close(
                    actual[key].as_f64().unwrap(),
                    expected[key].as_f64().unwrap(),
                    1e-7,
                    &format!("row {row_index}, word {word_index}, {key}"),
                );
            }
            for key in ["x", "width"] {
                close(
                    actual[key].as_f64().unwrap(),
                    expected[key].as_f64().unwrap(),
                    0.002,
                    &format!("row {row_index}, word {word_index}, {key}"),
                );
            }
        }
    }
}
