//! ASS lyric serialization from the shared measured display plan.
//!
//! This native pass covers line timing/animation, karaoke word sweeps,
//! gradients, subtitle backing, title cards and vector count-in icons. Role
//! artwork remains open.

use serde_json::Value;
use vietkara_domain::Project;

use crate::layout::{DisplayPlan, DisplayRow, PlannedWord, TitleLayout};
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

fn role_marker_path(role: &str, size: f64) -> String {
    let scale = size / 32.0;
    let path = std::cell::RefCell::new(String::new());
    let circle = |cx: f64, cy: f64, radius: f64| {
        let k = radius * 0.55228475;
        let (cx, cy, r, k) = (cx * scale, cy * scale, radius * scale, k * scale);
        path.borrow_mut().push_str(&format!("m {:.3} {:.3} b {:.3} {:.3} {:.3} {:.3} {:.3} {:.3} b {:.3} {:.3} {:.3} {:.3} {:.3} {:.3} b {:.3} {:.3} {:.3} {:.3} {:.3} {:.3} b {:.3} {:.3} {:.3} {:.3} {:.3} {:.3} ",cx+r,cy,cx+r,cy+k,cx+k,cy+r,cx,cy+r,cx-k,cy+r,cx-r,cy+k,cx-r,cy,cx-r,cy-k,cx-k,cy-r,cx,cy-r,cx+k,cy-r,cx+r,cy-k,cx+r,cy));
    };
    let poly = |points: &[(f64, f64)]| {
        for (index, (x, y)) in points.iter().enumerate() {
            path.borrow_mut().push_str(&format!(
                "{} {:.3} {:.3} ",
                if index == 0 { "m" } else { "l" },
                x * scale,
                y * scale
            ));
        }
        path.borrow_mut().push_str("l ");
        let (x, y) = points[0];
        path.borrow_mut()
            .push_str(&format!("{:.3} {:.3} ", x * scale, y * scale));
    };
    match role {
        "male" => {
            circle(16., 5., 3.5);
            poly(&[
                (12., 10.),
                (20., 10.),
                (23., 12.),
                (26., 20.),
                (22., 22.),
                (19., 16.),
                (19., 23.),
                (13., 23.),
                (13., 16.),
                (10., 22.),
                (6., 20.),
                (9., 12.),
            ]);
            poly(&[
                (13., 22.),
                (19., 22.),
                (20., 31.),
                (16.7, 31.),
                (16., 25.),
                (15.3, 31.),
                (12., 31.),
            ]);
        }
        "female" => {
            circle(16., 5., 3.5);
            poly(&[
                (13., 10.),
                (19., 10.),
                (23., 12.),
                (26., 20.),
                (22., 22.),
                (19., 16.),
                (13., 16.),
                (10., 22.),
                (6., 20.),
                (9., 12.),
            ]);
            poly(&[(13., 16.), (19., 16.), (24., 28.), (8., 28.)]);
            poly(&[(11., 28.), (15., 28.), (15., 32.), (11., 32.)]);
            poly(&[(17., 28.), (21., 28.), (21., 32.), (17., 32.)]);
        }
        _ => {
            circle(10., 7., 2.6);
            circle(22., 7., 2.6);
            poly(&[
                (7., 11.),
                (13., 11.),
                (15., 14.),
                (15., 21.),
                (5., 21.),
                (5., 14.),
            ]);
            poly(&[
                (19., 11.),
                (25., 11.),
                (27., 14.),
                (27., 21.),
                (17., 21.),
                (17., 14.),
            ]);
            poly(&[
                (7., 20.),
                (13., 20.),
                (14., 31.),
                (11., 31.),
                (10., 24.),
                (9., 31.),
                (6., 31.),
            ]);
            poly(&[
                (19., 20.),
                (25., 20.),
                (26., 31.),
                (23., 31.),
                (22., 24.),
                (21., 31.),
                (18., 31.),
            ]);
        }
    }
    path.into_inner()
}

fn readability_events(
    plan: &DisplayPlan,
    settings: &serde_json::Map<String, Value>,
    scenes: &[crate::RenderScene],
) -> Vec<String> {
    let subtitle_box = string(settings, "display_mode", "karaoke") == "subtitle"
        && boolean(settings, "subtitle_box", true);
    if subtitle_box {
        return Vec::new();
    }
    let mut output = Vec::new();
    for row in &plan.rows {
        let opacity = scenes
            .iter()
            .filter(|scene| scene.start <= row.start && row.start <= scene.end)
            .map(|scene| scene.backing_opacity)
            .max()
            .unwrap_or(0);
        if opacity == 0 {
            continue;
        }
        let pad = row.font_size * 0.22;
        let shape = crate::rounded_box(
            0.0,
            0.0,
            row.width + 2.0 * pad,
            row.height + 2.0 * pad,
            12.0,
        );
        let mut anim = row_anim(row);
        anim.x -= pad;
        anim.y -= pad;
        let options = AnimOpts {
            line_animation: "none".into(),
            horizontal_scale: 100.0,
        };
        let alpha = ((100 - opacity as u32) * 255 / 100) as u8;
        let tags = format!(
            "\\fscx100\\fscy100{}\\p1\\bord0\\shad0\\1c{}\\1a&H{alpha:02X}&",
            animation(&anim, &options),
            ass_color("#000000", 100)
        );
        if let Some(event) = dialogue(0, row.show_start, row.show_end, &tags, &shape) {
            output.push(event);
        }
    }
    output
}

fn title_state(mode: &str) -> (f64, f64, u8, u8) {
    match mode {
        "slide_up" => (0.0, 1.0, 100, 0),
        "slide_down" => (0.0, -1.0, 100, 0),
        "slide_left" => (1.0, 0.0, 100, 0),
        "slide_right" => (-1.0, 0.0, 100, 0),
        "zoom" => (0.0, 0.0, 65, 0),
        "flip" => (0.0, 0.0, 100, 75),
        _ => (0.0, 0.0, 100, 0),
    }
}

fn title_enter_tags(layout: &TitleLayout, row: &crate::layout::TitleRow) -> String {
    let ms = (layout.effect_seconds * 1000.0).round_ties_even().max(1.0);
    let (dx, dy, scale, rx) = title_state(&layout.enter);
    let offset = row.size * 0.65;
    let x = row.x;
    let y = row.y;
    let place = if dx != 0.0 || dy != 0.0 {
        format!(
            "\\move({:.3},{:.3},{x:.3},{y:.3},0,{ms:.0})",
            x + dx * offset,
            y + dy * offset
        )
    } else {
        format!("\\pos({x:.3},{y:.3})")
    };
    format!("\\an7{place}\\fscx{scale}\\fscy{scale}\\frx{rx}\\t(0,{ms:.0},\\fscx100\\fscy100\\frx0)\\fad({:.0},0)", if layout.enter == "none" {0.0} else {ms}).replace(&format!("\\t(0,{ms:.0},\\fscx100\\fscy100\\frx0)"), &if layout.enter == "none" {String::new()} else {format!("\\t(0,{ms:.0},\\fscx100\\fscy100\\frx0)")}).replace(&format!("\\fscx{scale}\\fscy{scale}\\frx{rx}"), &if layout.enter == "none" {"\\fscx100\\fscy100\\frx0".to_owned()} else {format!("\\fscx{scale}\\fscy{scale}\\frx{rx}")})
}

fn title_exit_tags(layout: &TitleLayout, row: &crate::layout::TitleRow) -> String {
    let ms = (layout.effect_seconds * 1000.0).round_ties_even().max(1.0);
    let (dx, dy, scale, rx) = title_state(&layout.exit);
    let offset = row.size * 0.65;
    let x = row.x;
    let y = row.y;
    let place = if dx != 0.0 || dy != 0.0 {
        format!(
            "\\move({x:.3},{y:.3},{:.3},{:.3},0,{ms:.0})",
            x - dx * offset,
            y - dy * offset
        )
    } else {
        format!("\\pos({x:.3},{y:.3})")
    };
    format!(
        "\\an7{place}\\fscx100\\fscy100\\frx0\\t(0,{ms:.0},\\fscx{scale}\\fscy{scale}\\frx{rx})\\fad(0,{:.0})",
        if layout.exit == "none" { 0.0 } else { ms }
    )
}

fn title_events(layout: &TitleLayout) -> Vec<String> {
    let mut output = Vec::new();
    let split = layout.end - layout.effect_seconds;
    let fade_in = if layout.enter == "none" {
        0.0
    } else {
        (layout.effect_seconds * 1000.0).round_ties_even()
    };
    let fade_out = if layout.exit == "none" {
        0.0
    } else {
        (layout.effect_seconds * 1000.0).round_ties_even()
    };
    if let Some(box_layout) = &layout.template {
        let template = &box_layout.template;
        let shape = crate::rounded_box(
            0.0,
            0.0,
            box_layout.width,
            box_layout.height,
            template.radius,
        );
        let opacity = ((100.0 - template.opacity) / 100.0 * 255.0).round_ties_even() as u8;
        for index in 0..24 {
            let amount = index as f64 / 24.0 * (template.colors.len() - 1) as f64;
            let stop = (amount.floor() as usize).min(template.colors.len() - 2);
            let fill = crate::blend(
                &template.colors[stop],
                &template.colors[stop + 1],
                amount - stop as f64,
            );
            let x0 = box_layout.width * index as f64 / 24.0;
            let x1 = box_layout.width * (index + 1) as f64 / 24.0 + 0.25;
            let band = format!(
                "m {x0:.2} 0 l {x1:.2} 0 {x1:.2} {:.2} {x0:.2} {:.2}",
                box_layout.height, box_layout.height
            );
            let tags = format!(
                "\\an7\\pos({:.2},{:.2})\\p1\\bord0\\shad0\\1c{}\\1a&H{opacity:02X}&\\fad({fade_in:.0},{fade_out:.0})",
                box_layout.x,
                box_layout.y,
                crate::ass_color(&fill, 100)
            );
            if let Some(event) = dialogue(8, layout.start, layout.end, &tags, &band) {
                output.push(event);
            }
        }
        if template.border_width > 0.0 {
            let tags = format!(
                "\\an7\\pos({:.2},{:.2})\\p1\\bord{:.2}\\shad0\\1a&HFF&\\3c{}\\3a&H00&\\fad({fade_in:.0},{fade_out:.0})",
                box_layout.x,
                box_layout.y,
                template.border_width,
                crate::ass_color(&template.border, 100)
            );
            if let Some(event) = dialogue(9, layout.start, layout.end, &tags, &shape) {
                output.push(event);
            }
        }
    }
    for row in &layout.rows {
        let motion = title_enter_tags(layout, row);
        let bold = layout.weight >= 600;
        let tags = format!(
            "{motion}\\fn{}\\fs{:.3}\\b{}\\i{}\\1c{}\\3c{}\\4c{}\\bord{:.2}\\shad{:.2}",
            layout.font.replace(',', ""),
            row.size,
            if bold { 1 } else { 0 },
            if layout.italic { -1 } else { 0 },
            crate::ass_color(&layout.color, 100),
            crate::ass_color(&layout.outline, 100),
            crate::ass_color(&layout.shadow_color, 100),
            layout.outline_width,
            layout.shadow
        );
        if let Some(event) = dialogue(10, layout.start, split, &tags, &literal(&row.display_text)) {
            output.push(event);
        }
        let tags = format!(
            "{}\\fn{}\\fs{:.3}\\b{}\\i{}\\1c{}\\3c{}\\4c{}\\bord{:.2}\\shad{:.2}",
            title_exit_tags(layout, row),
            layout.font.replace(',', ""),
            row.size,
            if bold { 1 } else { 0 },
            if layout.italic { -1 } else { 0 },
            crate::ass_color(&layout.color, 100),
            crate::ass_color(&layout.outline, 100),
            crate::ass_color(&layout.shadow_color, 100),
            layout.outline_width,
            layout.shadow
        );
        if let Some(event) = dialogue(10, split, layout.end, &tags, &literal(&row.display_text)) {
            output.push(event);
        }
    }
    output
}

fn gradient_bands(
    row: &DisplayRow,
    settings: &serde_json::Map<String, Value>,
    fallback: &str,
    height: u32,
) -> Vec<(f64, f64, String)> {
    if !boolean(settings, "sung_gradient", false) || row.role != "all" {
        return vec![(
            row.y - row.font_size * 0.12,
            row.y + row.height,
            fallback.to_owned(),
        )];
    }
    let top_color = color(settings, "gradient_top", "#ffffff");
    let bottom_color = color(settings, "gradient_bottom", "#1a3ad6");
    let start = number(settings, "gradient_start", 0.0).clamp(0.0, 99.0);
    let end = number(settings, "gradient_end", 100.0).clamp(start + 1.0, 100.0);
    let top = row.y - row.font_size * 0.12;
    let bottom = row.y + row.height;
    let mut bands = (0..10)
        .map(|index| {
            let fraction = (index as f64 + 0.5) / 10.0;
            let amount = (100.0 - fraction * 100.0 - start) / (end - start).max(1.0);
            let y1 = (top + (bottom - top) * index as f64 / 10.0).max(0.0);
            let y2 =
                (top + (bottom - top) * (index + 1) as f64 / 10.0 + 0.5).min(f64::from(height));
            (y1, y2, crate::blend(bottom_color, top_color, amount))
        })
        .collect::<Vec<_>>();
    if let Some(first) = bands.first_mut() {
        first.0 = 0.0;
    }
    if let Some(last) = bands.last_mut() {
        last.1 = f64::from(height);
    }
    bands
}

fn word_event(
    row: &DisplayRow,
    word: &PlannedWord,
    opts: &AnimOpts,
    style: &AssStyle<'_>,
    sung_bands: &[(f64, f64, String)],
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
        &literal(&word.display_text),
    ) {
        output.push(event);
    }

    let start = word.start.max(row.show_start);
    let end = word.end.min(row.show_end);
    if end > start {
        let duration = ((end - start) * 1000.0).round_ties_even().max(1.0);
        let left = word.x;
        let right = word.x + word.width;
        for (top, bottom, hex_color) in sung_bands {
            let clip = format!(
                "\\clip({left:.2},{top:.2},{left:.2},{bottom:.2})\\t(0,{duration:.0},\\clip({left:.2},{top:.2},{right:.2},{bottom:.2}))"
            );
            let sung = paint(
                hex_color,
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
                &literal(&word.display_text),
            ) {
                output.push(event);
            }
        }
    }
    output
}

/// Build a UTF-8 ASS subtitle file from a project and its measured layout.
/// The same `DisplayPlan` can be used by GPUI preview and passed here for
/// export, preserving row breaks, positions, font sizes and word advances.
pub fn build_lyrics_ass(project: &Project, plan: &DisplayPlan) -> String {
    build_lyrics_ass_with_scenes(project, plan, &[])
}

pub fn build_lyrics_ass_with_scenes(
    project: &Project,
    plan: &DisplayPlan,
    scenes: &[crate::RenderScene],
) -> String {
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
    lines.extend(readability_events(plan, settings, scenes));
    for row in &plan.rows {
        if let Some(marker) = &row.role_marker {
            let tags = format!(
                "\\an7\\pos({:.2},{:.2})\\p1\\bord0\\shad0\\1c{}",
                marker.x,
                marker.y,
                ass_color(&marker.color, 100)
            );
            if let Some(event) = dialogue(
                4,
                row.show_start,
                row.show_end,
                &tags,
                &role_marker_path(&marker.role, marker.size),
            ) {
                lines.push(event);
            }
        }
        let anim = animation(&row_anim(row), &options);
        let sung_color = if duet && mode == "karaoke" {
            role_color(row, settings, sung)
        } else {
            sung.to_owned()
        };
        if mode != "karaoke" {
            if mode == "subtitle" && boolean(settings, "subtitle_box", true) {
                let pad = row.font_size * 0.18;
                let shape = crate::rounded_box(
                    0.0,
                    0.0,
                    row.width + 2.0 * pad,
                    row.height + 2.0 * pad,
                    number(settings, "subtitle_box_radius", 8.0).clamp(0.0, 40.0),
                );
                let mut box_row = row_anim(row);
                box_row.x -= pad;
                box_row.y -= pad;
                let box_color = color(settings, "subtitle_box_color", "#000000");
                let box_opacity = opacity(settings, "subtitle_box_opacity", 50);
                let tags = format!(
                    "\\fscx100\\fscy100{}\\p1\\bord0\\shad0\\1c{}\\1a&H{:02X}&",
                    animation(&box_row, &options),
                    ass_color(box_color, 100),
                    255 - (255.0 * f64::from(box_opacity) / 100.0).round_ties_even() as u8,
                );
                if let Some(event) = dialogue(0, row.show_start, row.show_end, &tags, &shape) {
                    lines.push(event);
                }
            }
            for (top, bottom, hex_color) in gradient_bands(row, settings, unsung, plan.height) {
                let clip = if boolean(settings, "sung_gradient", false) && row.role == "all" {
                    format!("\\clip(0,{top:.2},{},{bottom:.2})", plan.width)
                } else {
                    String::new()
                };
                let row_tags = format!(
                    "{anim}\\fs{:.3}\\fscx{hscale:.2}\\1c{}\\3c{}\\bord{outline_width:.2}\\shad{shadow:.2}\\i{}",
                    row.font_size,
                    ass_color(&hex_color, 100),
                    ass_color(outline_unsung, outline_unsung_opacity),
                    if italic { -1 } else { 0 }
                );
                if let Some(event) = dialogue(
                    2,
                    row.show_start,
                    row.show_end,
                    &format!("{row_tags}{clip}"),
                    &literal(&crate::layout::font_text(&row.text, &plan.font)),
                ) {
                    lines.push(event);
                }
            }
            continue;
        }
        let row_style = base_style;
        let bands = gradient_bands(row, settings, &sung_color, plan.height);
        for word in &row.words {
            lines.extend(word_event(row, word, &options, &row_style, &bands));
        }
    }
    if let Some(title) = &plan.title {
        lines.extend(title_events(title));
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
        assert_eq!(dialogues.len(), 2);
        assert!(dialogues[0].starts_with("Dialogue: 0,"));
        assert!(dialogues[0].contains("\\p1\\bord0\\shad0"));
        assert!(dialogues[1].starts_with("Dialogue: 2,"));
        assert!(dialogues[1].ends_with("Xin chào"));
        assert!(!ass.contains("\\clip("));
    }

    #[test]
    fn title_card_uses_measured_layout_and_template_events() {
        let mut project = project();
        project.settings.extra.extend([
            ("title_enabled".into(), Value::Bool(true)),
            (
                "title_text".into(),
                Value::String("Xin chào Việt Nam".into()),
            ),
            ("title_credit".into(), Value::String("NDCLI".into())),
            ("title_template".into(), Value::String("midnight".into())),
            ("title_enter".into(), Value::String("slide_up".into())),
            ("title_exit".into(), Value::String("fade".into())),
        ]);
        let plan = crate::layout::display_plan(&project, &FixedWidth);
        let title = plan.title.as_ref().unwrap();
        assert_eq!(title.rows.len(), 2);
        assert_eq!(title.rows[0].text, "Xin chào Việt Nam");
        assert_eq!(title.rows[1].text, "NDCLI");
        assert!(title.template.is_some());

        let ass = build_lyrics_ass(&project, &plan);
        assert!(ass.contains("Xin chào Việt Nam"));
        assert!(ass.contains("NDCLI"));
        assert!(ass.contains("\\move("));
        assert!(ass.contains("\\fad(0,"));
        assert!(ass.contains("\\clip("));
        assert!(ass.contains("Dialogue: 8,0:00:00.00,0:00:08.00"));
    }

    #[test]
    fn scene_readability_backing_respects_intervals_and_subtitle_box() {
        let project = project();
        let plan = crate::layout::display_plan(&project, &FixedWidth);
        let mut scene = crate::RenderScene {
            image: None,
            start: 0.8,
            end: 1.8,
            opacity: 1.0,
            backing_opacity: 44,
            fade_seconds: 0.0,
            keyframes: [crate::SceneKeyframe::default(); 2],
            decoration: None,
        };
        let ass = build_lyrics_ass_with_scenes(&project, &plan, &[scene.clone()]);
        assert_eq!(
            ass.lines()
                .filter(|line| line.starts_with("Dialogue: 0,"))
                .count(),
            1
        );
        assert!(
            ass.lines()
                .filter(|line| line.starts_with("Dialogue: 0,"))
                .all(|line| line.contains("0:00:00.00,0:00:02.60"))
        );
        scene.backing_opacity = 48;
        let mut subtitle = project;
        subtitle
            .settings
            .extra
            .insert("display_mode".into(), Value::String("subtitle".into()));
        subtitle
            .settings
            .extra
            .insert("subtitle_box".into(), Value::Bool(true));
        let subtitle_plan = crate::layout::display_plan(&subtitle, &FixedWidth);
        let ass = build_lyrics_ass_with_scenes(&subtitle, &subtitle_plan, &[scene]);
        assert_eq!(
            ass.lines()
                .filter(|line| line.starts_with("Dialogue: 0,"))
                .count(),
            1
        );
    }

    #[test]
    fn sung_gradient_emits_ten_clipped_color_bands_per_karaoke_word() {
        let mut project = project();
        project.settings.extra.extend(
            serde_json::from_value::<serde_json::Map<String, Value>>(serde_json::json!({
                "sung_gradient":true,
                "gradient_top":"#ff0000",
                "gradient_bottom":"#0000ff",
                "gradient_start":0,
                "gradient_end":100
            }))
            .unwrap(),
        );
        let plan = crate::layout::display_plan(&project, &FixedWidth);
        let ass = build_lyrics_ass(&project, &plan);
        let sung = ass
            .lines()
            .filter(|line| line.starts_with("Dialogue: 2,"))
            .collect::<Vec<_>>();
        assert_eq!(sung.len(), 20);
        assert!(sung.iter().all(|line| line.contains("\\clip(")));
        assert!(ass.contains("\\1c&H000d00f2"));
        assert!(ass.contains("\\1c&H00f2000d"));
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
        assert_eq!(plan.rows[0].role_marker.as_ref().unwrap().role, "female");
        assert!(
            ass.lines()
                .any(|line| line.starts_with("Dialogue: 4,") && line.contains("\\p1"))
        );
    }

    #[test]
    fn ass_encodes_vietnamese_for_legacy_font_families() {
        let mut project = project();
        project.settings.font = "VNI-Times".into();
        project.words[0].text = "Xin chào".into();
        let plan = crate::layout::display_plan(&project, &FixedWidth);
        let ass = build_lyrics_ass(&project, &plan);
        assert!(ass.contains("Xin chaøo"));

        project.settings.font = ".VnArialH".into();
        let plan = crate::layout::display_plan(&project, &FixedWidth);
        let ass = build_lyrics_ass(&project, &plan);
        assert!(ass.contains("Xin chµo"));
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
        project
            .settings
            .extra
            .insert("sung_gradient".into(), Value::Bool(true));
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
