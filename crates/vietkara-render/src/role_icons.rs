use std::collections::BTreeMap;
use std::sync::OnceLock;

const ICONS: &[(&str, &[u8])] = &[
    (
        "man-new-blue.png",
        include_bytes!("../assets/role-icons/man-new-blue.png"),
    ),
    (
        "man-new-green.png",
        include_bytes!("../assets/role-icons/man-new-green.png"),
    ),
    (
        "lady-pink.png",
        include_bytes!("../assets/role-icons/lady-pink.png"),
    ),
    (
        "lady-red.png",
        include_bytes!("../assets/role-icons/lady-red.png"),
    ),
    (
        "lady-darkred.png",
        include_bytes!("../assets/role-icons/lady-darkred.png"),
    ),
    (
        "men(blue) & lady(pink).png",
        include_bytes!("../assets/role-icons/men(blue) & lady(pink).png"),
    ),
    (
        "men(blue) & lady(red).png",
        include_bytes!("../assets/role-icons/men(blue) & lady(red).png"),
    ),
    (
        "men(green) & lady(pink).png",
        include_bytes!("../assets/role-icons/men(green) & lady(pink).png"),
    ),
    (
        "men(green) & lady(red).png",
        include_bytes!("../assets/role-icons/men(green) & lady(red).png"),
    ),
];

#[derive(Debug, Clone, Copy)]
pub struct RoleIconRect {
    /// Coordinates are normalized to the image height.
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub color: [u8; 4],
}

#[derive(Debug)]
pub struct RoleIcon {
    pub width: f64,
    pub height: f64,
    pub rectangles: Vec<RoleIconRect>,
}

fn decode_icon(bytes: &[u8]) -> Option<RoleIcon> {
    let mut image = image::load_from_memory(bytes).ok()?.to_rgba8();
    let quantizer = color_quant::NeuQuant::new(1, 32, image.as_raw());
    for pixel in image.as_mut().chunks_exact_mut(4) {
        quantizer.map_pixel(pixel);
    }
    let width = image.width();
    let height = image.height();
    if width == 0 || height == 0 {
        return None;
    }
    let normalize = 1.0 / f64::from(height);
    let mut rectangles = Vec::new();
    for y in 0..height {
        let mut x = 0;
        while x < width {
            let color = image.get_pixel(x, y).0;
            let mut end = x + 1;
            while end < width && image.get_pixel(end, y).0 == color {
                end += 1;
            }
            if color[3] != 0 {
                rectangles.push(RoleIconRect {
                    x: f64::from(x) * normalize,
                    y: f64::from(y) * normalize,
                    width: f64::from(end - x) * normalize,
                    height: normalize,
                    color,
                });
            }
            x = end;
        }
    }
    Some(RoleIcon {
        width: f64::from(width) * normalize,
        height: 1.0,
        rectangles,
    })
}

pub fn role_icon(filename: &str) -> Option<&'static RoleIcon> {
    static CACHE: OnceLock<BTreeMap<&'static str, Option<RoleIcon>>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            ICONS
                .iter()
                .map(|(filename, bytes)| (*filename, decode_icon(bytes)))
                .collect()
        })
        .get(filename)
        .and_then(Option::as_ref)
}

pub fn role_icon_setting<'a>(
    role: &str,
    settings: &'a serde_json::Map<String, serde_json::Value>,
) -> &'a str {
    let (key, allowed, default): (&str, &[&str], &str) = match role {
        "male" | "voice_c" | "voice_e" => (
            "role_a_icon",
            &["man-new-blue.png", "man-new-green.png"],
            "man-new-blue.png",
        ),
        "female" | "voice_d" | "voice_f" => (
            "role_b_icon",
            &["lady-pink.png", "lady-red.png", "lady-darkred.png"],
            "lady-pink.png",
        ),
        _ => (
            "role_all_icon",
            &[
                "men(blue) & lady(pink).png",
                "men(blue) & lady(red).png",
                "men(green) & lady(pink).png",
                "men(green) & lady(red).png",
            ],
            "men(blue) & lady(pink).png",
        ),
    };
    settings
        .get(key)
        .and_then(serde_json::Value::as_str)
        .filter(|value| allowed.contains(value))
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_baseline_role_icon_decodes_to_transparent_rectangles() {
        for (filename, _) in ICONS {
            let icon = role_icon(filename).expect("bundled role icon should decode");
            assert_eq!(icon.height, 1.0);
            assert!(icon.width > 0.0);
            assert!(!icon.rectangles.is_empty());
            assert!(
                icon.rectangles
                    .iter()
                    .all(|rectangle| rectangle.color[3] > 0)
            );
        }
    }

    #[test]
    fn role_icon_choices_follow_the_two_supported_lanes() {
        let mut settings = serde_json::Map::new();
        settings.insert(
            "role_a_icon".into(),
            serde_json::Value::String("man-new-green.png".into()),
        );
        settings.insert(
            "role_b_icon".into(),
            serde_json::Value::String("lady-darkred.png".into()),
        );
        assert_eq!(role_icon_setting("male", &settings), "man-new-green.png");
        assert_eq!(role_icon_setting("voice_e", &settings), "man-new-green.png");
        assert_eq!(role_icon_setting("female", &settings), "lady-darkred.png");
        assert_eq!(role_icon_setting("voice_f", &settings), "lady-darkred.png");
        assert_eq!(
            role_icon_setting("all", &settings),
            "men(blue) & lady(pink).png"
        );
        settings.insert(
            "role_a_icon".into(),
            serde_json::Value::String("../../outside.png".into()),
        );
        assert_eq!(role_icon_setting("male", &settings), "man-new-blue.png");
    }
}
