//! ASS lyric serialization from the shared measured display plan.
//!
//! This native pass covers line timing/animation, karaoke word sweeps and
//! vector count-in icons. Gradients, title cards and role artwork remain open.

use serde_json::Value;
use vietkara_domain::Project;

use crate::layout::{DisplayPlan, DisplayRow, PlannedWord};
use crate::{AnimOpts, RowAnim, TextAlign, animation, ass_color, literal, stamp};

fn string<'a>(
    settings: &'a serde_json::Map<String, Value>,
    key: &str,
    default: &'a str,
) -> &'a str {
    settings.get(key).and_then(Value::as_str).unwrap_or(default)
}

fn color<'a>(settings: &'a serde_json::Map<String, Value>, key: &str, default: &'a str) -> &'a str {
    settings
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| crate::is_hex_color(value))
        .unwrap_or(default)
}

fn number(settings: &serde_json::Map<String, Value>, key: &str, default: f64) -> f64 {
    settings
        .get(key)
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .unwrap_or(default)
}

fn boolean(settings: &serde_json::Map<String, Value>, key: &str, default: bool) -> bool {
    settings
        .get(key)
        .and_then(Value::as_bool)
        .unwrap_or(default)
}

fn opacity(settings: &serde_json::Map<String, Value>, key: &str, default: u8) -> u8 {
    number(settings, key, f64::from(default))
        .round_ties_even()
        .clamp(0.0, 100.0) as u8
}

fn role_color(
    row: &DisplayRow,
    settings: &serde_json::Map<String, Value>,
    fallback: &str,
) -> String {
    let key = match row.role.as_str() {
        "male" => "role_a_color",
        "female" => "role_b_color",
        "all" => "role_all_color",
        "voice_c" => "role_c_color",
        "voice_d" => "role_d_color",
        "voice_e" => "role_e_color",
        "voice_f" => "role_f_color",
        "unknown" => "role_unknown_color",
        _ => return fallback.to_owned(),
    };
    settings
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| crate::is_hex_color(value))
        .unwrap_or(fallback)
        .to_owned()
}

fn row_anim(row: &DisplayRow) -> RowAnim {
    RowAnim {
        show_start: row.show_start,
        show_end: row.show_end,
        x: row.x,
        y: row.y,
        font_size: row.font_size,
        align: match row.align.as_str() {
            "left" => TextAlign::Left,
            "right" => TextAlign::Right,
            "justify" => TextAlign::Justify,
            _ => TextAlign::Center,
        },
        section_start: row.section_start,
        section_end: row.section_end,
    }
}

#[derive(Clone, Copy)]
struct AssStyle<'a> {
    sung: &'a str,
    unsung: &'a str,
    outline_unsung: &'a str,
    outline_sung: &'a str,
    shadow_color: &'a str,
    italic: bool,
    outline_width: f64,
    outline_unsung_opacity: u8,
    outline_sung_opacity: u8,
    shadow: f64,
    horizontal_scale: f64,
}

fn paint(hex: &str, outline: &str, outline_opacity: u8, size: f64, style: &AssStyle<'_>) -> String {
    format!(
        "\\fs{size:.3}\\fscx{:.2}\\i{}\\1c{}\\3c{}\\4c{}\\bord{:.2}\\shad{:.2}",
        style.horizontal_scale,
        if style.italic { -1 } else { 0 },
        ass_color(hex, 100),
        ass_color(outline, outline_opacity),
        ass_color(style.shadow_color, 100),
        style.outline_width,
        style.shadow,
    )
}

fn dialogue(layer: u8, start: f64, end: f64, tags: &str, text: &str) -> Option<String> {
    (end > start).then(|| {
        format!(
            "Dialogue: {layer},{},{},Main,,0,0,0,,{{{tags}}}{text}",
            stamp(start),
            stamp(end)
        )
    })
}

fn word_event(
    row: &DisplayRow,
    word: &PlannedWord,
    opts: &AnimOpts,
    style: &AssStyle<'_>,
) -> Vec<String> {
    let mut output = Vec::with_capacity(2);
    let mut anim = row_anim(row);
    anim.x = word.x;
    let motion = animation(&anim, opts);
    let unsung = paint(
        style.unsung,
        style.outline_unsung,
        style.outline_unsung_opacity,
        row.font_size,
        style,
    );
    if let Some(event) = dialogue(
        1,
        row.show_start,
        row.show_end,
        &format!("{motion}{unsung}"),
        &literal(&word.word.text),
    ) {
        output.push(event);
    }

    let start = word.start.max(row.show_start);
    let end = word.end.min(row.show_end);
    if end > start {
        let duration = ((end - start) * 1000.0).round_ties_even().max(1.0);
        let top = row.y - row.font_size * 0.12;
        let bottom = row.y + row.height;
        let left = word.x;
        let right = word.x + word.width;
        let clip = format!(
            "\\clip({left:.2},{top:.2},{left:.2},{bottom:.2})\\t(0,{duration:.0},\\clip({left:.2},{top:.2},{right:.2},{bottom:.2}))"
        );
        let sung = paint(
            style.sung,
            style.outline_sung,
            style.outline_sung_opacity,
            row.font_size,
            style,
        );
        if let Some(event) = dialogue(
            2,
            start,
            end,
            &format!("{motion}{sung}{clip}"),
            &literal(&word.word.text),
        ) {
            output.push(event);
        }
    }
    output
}

/// Build a UTF-8 ASS subtitle file from a project and its measured layout.
/// The same `DisplayPlan` can be used by GPUI preview and passed here for
/// export, preserving row breaks, positions, font sizes and word advances.
pub fn build_lyrics_ass(project: &Project, plan: &DisplayPlan) -> String {
    let settings = &project.settings.extra;
    let font = project.settings.font.replace(',', "");
    let font_size = project.settings.font_size;
    let sung = color(settings, "sung", "#52efb9");
    let unsung = color(settings, "unsung", "#ffffff");
    let outline_unsung = color(settings, "outline_unsung", "#15202a");
    let outline_sung = color(settings, "outline_sung", "#15202a");
    let shadow_color = color(settings, "shadow_color", "#000000");
    let outline_width = number(settings, "outline_width", 3.0).clamp(0.0, 12.0);
    let outline_sung_opacity = opacity(settings, "outline_sung_opacity", 100);
    let outline_unsung_opacity = opacity(settings, "outline_unsung_opacity", 100);
    let shadow = number(settings, "shadow", 1.0).clamp(0.0, 15.0);
    let hscale = number(settings, "horizontal_scale", 100.0).clamp(50.0, 150.0);
    let italic = boolean(settings, "italic", false);
    let mode = string(settings, "display_mode", "karaoke");
    let duet = string(settings, "singing_mode", "solo") == "duet";
    let options = AnimOpts {
        line_animation: string(settings, "line_animation", "none").to_owned(),
        horizontal_scale: hscale,
    };
    let base_style = AssStyle {
        sung,
        unsung,
        outline_unsung,
        outline_sung,
        shadow_color,
        italic,
        outline_width,
        outline_unsung_opacity,
        outline_sung_opacity,
        shadow,
        horizontal_scale: hscale,
    };
    let style = format!(
        "Style: Main,{font},{font_size},{},{},{},{},-1,{},0,0,{hscale:.2},100,0,0,1,{outline_width:.2},{shadow:.2},7,0,0,0,1",
        ass_color(sung, 100),
        ass_color(unsung, 100),
        ass_color(outline_unsung, outline_unsung_opacity),
        ass_color(shadow_color, 100),
        if italic { -1 } else { 0 },
    );
    let mut lines = vec![
        "[Script Info]".into(), "ScriptType: v4.00+".into(),
        format!("PlayResX: {}", plan.width), format!("PlayResY: {}", plan.height),
        "WrapStyle: 2".into(), "ScaledBorderAndShadow: yes".into(), String::new(),
        "[V4+ Styles]".into(),
        "Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding".into(),
        style, String::new(), "[Events]".into(),
        "Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text".into(),
    ];
    for row in &plan.rows {
        let anim = animation(&row_anim(row), &options);
        let sung_color = if duet && mode == "karaoke" {
            role_color(row, settings, sung)
        } else {
            sung.to_owned()
        };
        if mode != "karaoke" {
            let tags = format!(
                "{anim}\\fs{:.3}\\fscx{hscale:.2}\\1c{}\\3c{}\\bord{outline_width:.2}\\shad{shadow:.2}\\i{}",
                row.font_size,
                ass_color(unsung, 100),
                ass_color(outline_unsung, outline_unsung_opacity),
                if italic { -1 } else { 0 }
            );
            if let Some(event) =
                dialogue(2, row.show_start, row.show_end, &tags, &literal(&row.text))
            {
                lines.push(event);
            }
            continue;
        }
        let row_style = AssStyle {
            sung: &sung_color,
            ..base_style
        };
        for word in &row.words {
            lines.extend(word_event(row, word, &options, &row_style));
        }
    }
    lines.extend(countdown_events(
        project,
        plan,
        settings,
        mode,
        sung,
        outline_unsung,
    ));
    lines.join("\n") + "\n"
}

fn countdown_events(
    project: &Project,
    plan: &DisplayPlan,
    settings: &serde_json::Map<String, Value>,
    mode: &str,
    sung: &str,
    outline: &str,
) -> Vec<String> {
    let icon_name = string(settings, "countdown", "none");
    let Some((_, drawing)) = crate::ICONS.iter().find(|(name, _)| *name == icon_name) else {
        return Vec::new();
    };
    if mode != "karaoke" {
        return Vec::new();
    }

    let duration = number(settings, "countdown_seconds", 4.0).clamp(1.0, 8.0);
    let font_size = project.settings.font_size * plan.width as f64 / 1920.0;
    let baseline = match string(settings, "position", "bottom") {
        "top" => font_size * 2.0,
        "center" => plan.height as f64 / 2.0 + font_size * 0.5,
        _ => plan.height as f64 - font_size * 1.25,
    };
    let y = (baseline - font_size * 2.05).max(0.0);
    let size = (font_size * 0.7).round().max(2.0);
    let mut line_times: Vec<(u32, f64, f64)> = Vec::new();
    for word in &project.words {
        let start = vietkara_domain::timing::map_time(word.start, &project.anchors, false);
        let end = vietkara_domain::timing::map_time(word.end, &project.anchors, false);
        if let Some((_, line_start, line_end)) = line_times
            .iter_mut()
            .find(|(line, _, _)| *line == word.line)
        {
            *line_start = line_start.min(start);
            *line_end = line_end.max(end);
        } else {
            line_times.push((word.line, start, end));
        }
    }

    let mut output = Vec::new();
    let mut previous_end: f64 = 0.0;
    for (index, (_, start, end)) in line_times.into_iter().enumerate() {
        let count_in = index == 0 || start - previous_end >= 8.0;
        if count_in {
            let end = (start - 0.05).max(0.0);
            let begin = 0.0_f64.max(previous_end).max(end - duration);
            if end - begin >= 0.4 {
                for slot in 0..4 {
                    let slot_end = end - slot as f64;
                    let x = plan.width as f64 * 0.05 + slot as f64 * size * 1.28;
                    let scale = size / 25.0 * 100.0;
                    let tags = format!(
                        "\\an7\\pos({x:.2},{y:.2})\\p1\\fscx{scale:.2}\\fscy{scale:.2}\\bord.5\\shad0\\1c{}\\3c{}\\fad(120,100)",
                        crate::ass_color(sung, 100),
                        crate::ass_color(outline, 100),
                    );
                    if let Some(event) = dialogue(6, begin, slot_end, &tags, drawing) {
                        output.push(event);
                    }
                }
            }
        }
        previous_end = end;
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use vietkara_domain::settings::Settings;

    struct FixedWidth;
    impl crate::layout::TextMeasurer for FixedWidth {
        fn width(&self, text: &str, _family: &str, size: f64, _italic: bool) -> f64 {
            text.chars().count() as f64 * size * 0.5
        }
    }

    fn project() -> Project {
        serde_json::from_value(serde_json::json!({
            "schema":2, "id":"p", "name":"Test", "text":"Xin chào",
            "settings":{"font":"Arial","font_size":72,"sung":"#52efb9",
                         "line_animation":"slowfade"},
            "words":[
                {"id":"00000000000000000000000000000001","line_uid":"00000000000000000000000000000010","text":"Xin","start":1.0,"end":1.5,"line":0},
                {"id":"00000000000000000000000000000002","line_uid":"00000000000000000000000000000010","text":"chào","start":1.5,"end":2.0,"line":0}
            ]
        })).unwrap()
    }

    #[test]
    fn ass_uses_shared_plan_coordinates_and_emits_word_timed_sweeps() {
        let project = project();
        let plan = crate::layout::display_plan(&project, &FixedWidth);
        let ass = build_lyrics_ass(&project, &plan);
        assert!(ass.contains("PlayResX: 1920\nPlayResY: 1080"));
        assert!(ass.contains(&format!(
            "\\pos({:.2},{:.2})",
            plan.rows[0].words[0].x, plan.rows[0].y
        )));
        assert!(ass.contains("0:00:01.00,0:00:01.50"));
        assert!(ass.contains("\\clip("));
        assert!(ass.contains("\\t(0,500,\\clip("));
        assert!(ass.contains("\\fad("));
        assert!(ass.ends_with("\n"));
    }

    #[test]
    fn non_karaoke_ass_emits_one_subtitle_event_per_planned_row() {
        let mut project = project();
        project
            .settings
            .extra
            .insert("display_mode".into(), Value::String("subtitle".into()));
        let plan = crate::layout::display_plan(&project, &FixedWidth);
        let ass = build_lyrics_ass(&project, &plan);
        let dialogues = ass
            .lines()
            .filter(|line| line.starts_with("Dialogue:"))
            .collect::<Vec<_>>();
        assert_eq!(dialogues.len(), 1);
        assert!(dialogues[0].ends_with("Xin chào"));
        assert!(!ass.contains("\\clip("));
    }

    #[test]
    fn role_color_uses_the_two_lane_project_role() {
        let mut project = project();
        project.settings = Settings::from_value(serde_json::json!({
            "singing_mode":"duet", "role_b_color":"#ff87b7"
        }))
        .unwrap();
        project.words[0].role = "female".into();
        let plan = crate::layout::display_plan(&project, &FixedWidth);
        let ass = build_lyrics_ass(&project, &plan);
        assert!(ass.contains("\\1c&H00b787ff"));
    }

    #[test]
    fn vector_countdown_emits_four_timed_icons_before_long_gaps() {
        let mut project = project();
        project.settings.extra.extend(
            serde_json::from_value::<serde_json::Map<String, Value>>(serde_json::json!({
                "countdown":"mic", "countdown_seconds":4, "position":"bottom"
            }))
            .unwrap(),
        );
        project.words[0].start = 5.0;
        project.words[0].end = 5.5;
        project.words[1].start = 5.5;
        project.words[1].end = 6.0;
        let mut next_line = project.words[0].clone();
        next_line.id = "00000000000000000000000000000003".into();
        next_line.line_uid = "00000000000000000000000000000011".into();
        next_line.text = "lại".into();
        next_line.start = 15.0;
        next_line.end = 15.5;
        next_line.line = 1;
        project.words.push(next_line);

        let plan = crate::layout::display_plan(&project, &FixedWidth);
        let ass = build_lyrics_ass(&project, &plan);
        let countdown = ass
            .lines()
            .filter(|line| line.starts_with("Dialogue: 6,"))
            .collect::<Vec<_>>();
        assert_eq!(countdown.len(), 8);
        assert!(countdown[0].contains("0:00:00.95,0:00:04.95"));
        assert!(countdown[0].contains("m 8 3"));
        assert!(countdown[1].contains("0:00:00.95,0:00:03.95"));
    }

    #[test]
    fn prepare_ass_points_ffmpeg_at_export_local_file() {
        let project = project();
        let plan = crate::layout::display_plan(&project, &FixedWidth);
        let directory = tempfile::tempdir().unwrap();
        let ass_path = directory.path().join("lyrics.ass");
        let mut inputs = crate::RenderInputs::new("beat.wav", 4.0);
        inputs
            .prepare_lyrics_ass(&project, &plan, &ass_path)
            .unwrap();
        assert_eq!(inputs.ass_file, ass_path);
        let content = std::fs::read_to_string(&ass_path).unwrap();
        assert!(content.contains("Xin"));
        assert!(content.contains("chào"));
        let argv = crate::ffmpeg_command(&crate::RenderProfile::default(), &inputs).unwrap();
        assert!(
            argv.iter()
                .any(|arg| arg.contains("ass=") && arg.contains("lyrics.ass"))
        );
    }

    #[test]
    fn prepare_ass_does_not_overwrite_an_existing_file() {
        let project = project();
        let plan = crate::layout::display_plan(&project, &FixedWidth);
        let directory = tempfile::tempdir().unwrap();
        let ass_path = directory.path().join("lyrics.ass");
        std::fs::write(&ass_path, "keep this file").unwrap();
        let mut inputs = crate::RenderInputs::new("beat.wav", 4.0);
        assert!(
            inputs
                .prepare_lyrics_ass(&project, &plan, &ass_path)
                .is_err()
        );
        assert_eq!(std::fs::read_to_string(ass_path).unwrap(), "keep this file");
    }

    #[test]
    fn generated_karaoke_ass_renders_through_ffmpeg() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        std::fs::create_dir(root.join("fonts")).unwrap();
        let mut project = project();
        project
            .settings
            .extra
            .insert("countdown".into(), Value::String("mic".into()));
        let plan = crate::layout::display_plan(&project, &FixedWidth);
        let mut inputs = crate::RenderInputs::new(root.join("beat.wav"), 2.0);
        inputs
            .prepare_lyrics_ass(&project, &plan, root.join("lyrics.ass"))
            .unwrap();
        // Relative filter paths avoid drive-colon escaping on Windows.
        inputs.ass_file = "lyrics.ass".into();
        assert!(
            std::fs::read_to_string(root.join("lyrics.ass"))
                .unwrap()
                .contains("Dialogue: 6,")
        );
        let sine = std::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=2",
                "-ar",
                "44100",
            ])
            .arg(root.join("beat.wav"))
            .output()
            .unwrap();
        assert!(sine.status.success(), "sine generation failed");
        let profile = crate::RenderProfile {
            fps: crate::Fps::Fixed(30.0),
            frame_size_override: Some((320, 240)),
            ..crate::RenderProfile::default()
        };
        let argv = crate::ffmpeg_command(&profile, &inputs).unwrap();
        let render = std::process::Command::new(&argv[0])
            .args(&argv[1..])
            .current_dir(root)
            .output()
            .unwrap();
        assert!(
            render.status.success(),
            "FFmpeg rejected generated ASS: {}",
            String::from_utf8_lossy(&render.stderr)
        );
        assert!(crate::verify_output(&root.join("karaoke.mp4"), 2.0, 320, 240).is_ok());
    }
}
