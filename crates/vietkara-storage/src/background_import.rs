//! Atomic project-local image background import.
use crate::{create_version, save_project};
use std::{path::Path, process::Command};
use vietkara_domain::Project;

pub struct ImportedBackground {
    pub project: Project,
    pub version_id: String,
}

fn supported_image(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "webp"
            )
        })
}

/// Convert the selected image to the Python-compatible `background.jpg`,
/// checkpoint the previous project, then publish media and JSON with rollback.
pub fn import_image_background(
    project: &Project,
    main: &Path,
    source: &Path,
) -> Result<ImportedBackground, String> {
    if !supported_image(source) {
        return Err("Nền ảnh chỉ hỗ trợ PNG, JPG, JPEG hoặc WebP".into());
    }
    if !source.is_file() {
        return Err("Không tìm thấy file ảnh nền".into());
    }
    project.validate().map_err(|error| error.to_string())?;
    let directory = main
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let destination = directory.join("background.jpg");
    let temporary = directory.join(format!(".background-{}.jpg", vietkara_domain::new_id()));
    let backup = directory.join(format!(
        ".background-backup-{}.jpg",
        vietkara_domain::new_id()
    ));

    let result = (|| -> Result<ImportedBackground, String> {
        let ffmpeg = vietkara_media::find_binary("ffmpeg").map_err(|error| error.to_string())?;
        let output = Command::new(ffmpeg)
            .args(["-y", "-v", "error", "-i"])
            .arg(source)
            .args(["-frames:v", "1", "-q:v", "2", "-update", "1"])
            .arg(&temporary)
            .output()
            .map_err(|error| format!("Không chạy được FFmpeg: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "Không chuyển được ảnh nền: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        if !temporary.is_file() || std::fs::metadata(&temporary).map(|m| m.len()).unwrap_or(0) == 0
        {
            return Err("FFmpeg không tạo được ảnh nền hợp lệ".into());
        }

        let mut candidate = project.clone();
        candidate.settings.extra.insert(
            "background_type".into(),
            serde_json::Value::String("image".into()),
        );
        candidate
            .settings
            .extra
            .insert("use_background".into(), serde_json::Value::Bool(true));
        candidate.extra.insert(
            "background_name".into(),
            serde_json::Value::String(
                source
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
            ),
        );
        candidate.validate().map_err(|error| error.to_string())?;
        let version = create_version(directory, project, "Trước thay ảnh nền")
            .map_err(|error| error.to_string())?;

        let had_destination = destination.is_file();
        if had_destination {
            std::fs::rename(&destination, &backup).map_err(|error| error.to_string())?;
        }
        if let Err(error) = std::fs::rename(&temporary, &destination) {
            if had_destination {
                std::fs::rename(&backup, &destination).map_err(|restore| {
                    format!(
                        "{error}; rollback lỗi: {restore}; backup {}",
                        backup.display()
                    )
                })?;
            }
            return Err(error.to_string());
        }
        if let Err(error) = save_project(&candidate, main) {
            let cleanup_error = std::fs::remove_file(&destination).err();
            let restore_error = if had_destination {
                std::fs::rename(&backup, &destination).err()
            } else {
                None
            };
            if cleanup_error.is_some() || restore_error.is_some() {
                return Err(format!(
                    "{error}; rollback cleanup={:?}, restore={:?}; backup {}",
                    cleanup_error,
                    restore_error,
                    backup.display()
                ));
            }
            return Err(error.to_string());
        }
        if had_destination {
            let _ = std::fs::remove_file(&backup);
        }
        Ok(ImportedBackground {
            project: candidate,
            version_id: version.id,
        })
    })();
    let _ = std::fs::remove_file(temporary);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn accepts_only_the_same_image_formats_as_the_python_background_uploader() {
        for extension in ["png", "jpg", "jpeg", "webp", "PNG", "JpG"] {
            assert!(supported_image(Path::new(&format!(
                "background.{extension}"
            ))));
        }
        for extension in ["mp4", "gif", "svg", "exe", ""] {
            assert!(!supported_image(Path::new(&format!(
                "background.{extension}"
            ))));
        }
    }

    #[test]
    fn rejects_video_before_touching_project_or_media() {
        let directory = tempdir().unwrap();
        let main = directory.path().join("project.json");
        let source = directory.path().join("background.mp4");
        std::fs::write(&source, b"video placeholder").unwrap();
        let project = Project::new("fixture");

        let error = match import_image_background(&project, &main, &source) {
            Ok(_) => panic!("video input was accepted as an image"),
            Err(error) => error,
        };

        assert!(error.contains("PNG, JPG"));
        assert!(!main.exists());
        assert!(!directory.path().join("background.jpg").exists());
    }

    #[test]
    fn imports_and_saves_a_real_image_background_with_a_version_snapshot() {
        let Ok(ffmpeg) = vietkara_media::find_binary("ffmpeg") else {
            return;
        };
        let directory = tempdir().unwrap();
        let main = directory.path().join("project.json");
        let source = directory.path().join("chosen.png");
        let project = Project::new("fixture");
        project.save(&main).unwrap();
        let generated = Command::new(ffmpeg)
            .args([
                "-y",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=red:s=32x32",
                "-frames:v",
                "1",
            ])
            .arg(&source)
            .output()
            .unwrap();
        assert!(generated.status.success());

        let imported = import_image_background(&project, &main, &source).unwrap();
        let saved = Project::load(&main).unwrap();

        assert!(directory.path().join("background.jpg").is_file());
        assert_eq!(imported.project.settings.extra["background_type"], "image");
        assert_eq!(saved.extra["background_name"], "chosen.png");
        assert!(
            directory
                .path()
                .join("versions")
                .join(imported.version_id)
                .join("project.json")
                .is_file()
        );
    }
}
