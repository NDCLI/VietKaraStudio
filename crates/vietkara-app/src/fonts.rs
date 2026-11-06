use rust_embed::RustEmbed;
use serde_json::Value;
use std::borrow::Cow;
use std::collections::HashSet;
use std::path::Path;
use std::sync::OnceLock;

#[derive(RustEmbed)]
#[folder = "../../resources/fonts/"]
struct FontAssets;

#[derive(Debug, Clone)]
pub(super) struct FontFamily {
    pub(super) family: String,
    pub(super) encoding: String,
    pub(super) vietnamese: bool,
    files: Vec<String>,
}

fn catalog() -> &'static [FontFamily] {
    static CATALOG: OnceLock<Vec<FontFamily>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let Some(file) = FontAssets::get("fonts.json") else {
            return Vec::new();
        };
        let Ok(value) = serde_json::from_slice::<Value>(&file.data) else {
            return Vec::new();
        };
        let Some(entries) = value.as_array() else {
            return Vec::new();
        };
        let mut families: Vec<FontFamily> = Vec::new();
        for entry in entries {
            let Some(family) = entry.get("family").and_then(Value::as_str) else {
                continue;
            };
            let Some(path) = entry.get("file").and_then(Value::as_str) else {
                continue;
            };
            if !path.starts_with("FONT/") || path.contains("..") {
                continue;
            }
            let encoding = entry
                .get("encoding")
                .and_then(Value::as_str)
                .unwrap_or("unicode");
            let vietnamese = entry
                .get("vietnamese")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let Some(existing) = families
                .iter_mut()
                .find(|item| item.family.eq_ignore_ascii_case(family))
            else {
                families.push(FontFamily {
                    family: family.to_owned(),
                    encoding: encoding.to_owned(),
                    vietnamese,
                    files: vec![path.to_owned()],
                });
                continue;
            };
            if !existing.files.iter().any(|file| file == path) {
                existing.files.push(path.to_owned());
            }
            if encoding == "unicode" {
                existing.encoding = "unicode".into();
                existing.vietnamese |= vietnamese;
            }
        }
        families.sort_by_key(|family| family.family.to_lowercase());
        families
    })
}

pub(super) fn families() -> &'static [FontFamily] {
    catalog()
}

pub(super) fn font_bytes(path: &str) -> Option<Cow<'static, [u8]>> {
    FontAssets::get(path).map(|font| font.data)
}

pub(super) fn first_font_path(family: &str) -> Option<&'static str> {
    catalog()
        .iter()
        .find(|entry| entry.family.eq_ignore_ascii_case(family))?
        .files
        .first()
        .map(String::as_str)
}

pub(super) fn inventory_counts() -> (usize, usize) {
    (
        catalog().len(),
        catalog().iter().map(|family| family.files.len()).sum(),
    )
}

pub(super) fn add_embedded_fonts(app: &mut gpui::App) -> Result<usize, String> {
    let mut seen = HashSet::new();
    let mut fonts = Vec::new();
    for family in catalog() {
        for path in &family.files {
            if !seen.insert(path) {
                continue;
            }
            if let Some(font) = FontAssets::get(path) {
                fonts.push(Cow::Owned(font.data.into_owned()));
            }
        }
    }
    let count = fonts.len();
    if count == 0 {
        return Err("Không tìm thấy font nhúng trong ứng dụng".into());
    }
    app.text_system()
        .add_fonts(fonts)
        .map_err(|error| error.to_string())?;
    Ok(count)
}

pub(super) fn write_family(family: &str, destination: &Path) -> Result<usize, String> {
    let Some(entry) = catalog()
        .iter()
        .find(|entry| entry.family.eq_ignore_ascii_case(family))
    else {
        return Ok(0);
    };
    std::fs::create_dir_all(destination).map_err(|error| error.to_string())?;
    let mut count = 0;
    for relative in &entry.files {
        let Some(asset) = FontAssets::get(relative) else {
            continue;
        };
        let Some(file_name) = Path::new(relative).file_name() else {
            continue;
        };
        std::fs::write(destination.join(file_name), asset.data.as_ref())
            .map_err(|error| error.to_string())?;
        count += 1;
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_font_catalog_includes_unicode_and_legacy_vietnamese_families() {
        assert!(families().len() >= 180);
        let files = families()
            .iter()
            .flat_map(|family| family.files.iter())
            .collect::<HashSet<_>>();
        assert_eq!(files.len(), 315);
        assert!(files.iter().all(|path| FontAssets::get(path).is_some()));
        let vni = families()
            .iter()
            .find(|family| family.family.eq_ignore_ascii_case("VNI-Times"))
            .unwrap();
        assert_eq!(vni.encoding, "vni");
        assert!(vni.files.len() >= 3);
        assert!(families().iter().any(|family| family.vietnamese));
    }

    #[test]
    fn selected_family_exports_all_style_files() {
        let dir = tempfile::tempdir().unwrap();
        let count = write_family("VNI-Times", dir.path()).unwrap();
        assert!(count >= 3);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), count);
    }
}
