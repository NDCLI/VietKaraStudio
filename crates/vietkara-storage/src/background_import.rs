//! Atomic project-local image background import.
use crate::{create_version, save_project};
use std::{path::Path, process::Command};
use vietkara_domain::Project;

pub struct ImportedBackground {
    pub project: Project,
    pub version_id: String,
}

/// Import a project-local PNG logo, checkpoint the previous project, and
/// atomically publish the image and settings with rollback on save failure.
pub fn import_logo(
    project: &Project,
    main: &Path,
    source: &Path,
) -> Result<ImportedBackground, String> {
    if !source
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("png"))
    {
        return Err("Logo chỉ hỗ trợ file PNG".into());
    }
    if !source.is_file() {
        return Err("Không tìm thấy file logo".into());
    }
    project.validate().map_err(|error| error.to_string())?;
    let directory = main
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let destination = directory.join("logo.png");
    let temporary = directory.join(format!(".logo-{}.png", vietkara_domain::new_id()));
    let backup = directory.join(format!(".logo-backup-{}.png", vietkara_domain::new_id()));
    let result = (|| -> Result<ImportedBackground, String> {
        let ffmpeg = vietkara_media::find_binary("ffmpeg").map_err(|error| error.to_string())?;
        let output = Command::new(ffmpeg)
            .args(["-y", "-v", "error", "-i"])
            .arg(source)
            .args(["-frames:v", "1", "-update", "1", "-f", "image2"])
            .arg(&temporary)
            .output()
            .map_err(|error| format!("Không chạy được FFmpeg: {error}"))?;
        if !output.status.success()
            || !temporary.is_file()
            || std::fs::metadata(&temporary).map(|m| m.len()).unwrap_or(0) == 0
        {
            return Err(format!(
                "Không chuyển được logo PNG: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        let mut candidate = project.clone();
        candidate
            .settings
            .extra
            .insert("logo_enabled".into(), serde_json::Value::Bool(true));
        candidate.extra.insert(
            "logo_name".into(),
            serde_json::Value::String("logo.png".into()),
        );
        candidate.validate().map_err(|error| error.to_string())?;
        let version = create_version(directory, project, "Trước thay logo")
            .map_err(|error| error.to_string())?;
        let had_destination = destination.is_file();
        if had_destination {
            std::fs::rename(&destination, &backup).map_err(|error| error.to_string())?;
        }
        if let Err(error) = std::fs::rename(&temporary, &destination) {
            if had_destination {
                std::fs::rename(&backup, &destination)
                    .map_err(|restore| format!("{error}; rollback lỗi: {restore}"))?;
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
                    "{error}; rollback cleanup={cleanup_error:?}, restore={restore_error:?}; backup {}",
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

/// Import a project-local custom countdown PNG with versioning and rollback.
pub fn import_countdown_image(
    project: &Project,
    main: &Path,
    source: &Path,
) -> Result<ImportedBackground, String> {
    if !source
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("png"))
    {
        return Err("Ảnh countdown chỉ hỗ trợ PNG".into());
    }
    if !source.is_file() {
        return Err("Không tìm thấy ảnh countdown".into());
    }
    project.validate().map_err(|error| error.to_string())?;
    let directory = main
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let destination = directory.join("countdown.png");
    let temporary = directory.join(format!(".countdown-{}.png", vietkara_domain::new_id()));
    let backup = directory.join(format!(
        ".countdown-backup-{}.png",
        vietkara_domain::new_id()
    ));
    let result = (|| -> Result<ImportedBackground, String> {
        let ffmpeg = vietkara_media::find_binary("ffmpeg").map_err(|error| error.to_string())?;
        let output = Command::new(ffmpeg)
            .args(["-y", "-v", "error", "-i"])
            .arg(source)
            .args(["-frames:v", "1", "-update", "1", "-f", "image2"])
            .arg(&temporary)
            .output()
            .map_err(|error| format!("Không chạy được FFmpeg: {error}"))?;
        if !output.status.success()
            || !temporary.is_file()
            || std::fs::metadata(&temporary).map(|m| m.len()).unwrap_or(0) == 0
        {
            return Err(format!(
                "Không chuyển được ảnh countdown: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        let mut candidate = project.clone();
        candidate.settings.extra.insert(
            "countdown".into(),
            serde_json::Value::String("image".into()),
        );
        candidate.validate().map_err(|error| error.to_string())?;
        let version = create_version(directory, project, "Trước thay ảnh countdown")
            .map_err(|error| error.to_string())?;
        let had_destination = destination.is_file();
        if had_destination {
            std::fs::rename(&destination, &backup).map_err(|error| error.to_string())?;
        }
        if let Err(error) = std::fs::rename(&temporary, &destination) {
            if had_destination {
                std::fs::rename(&backup, &destination)
                    .map_err(|restore| format!("{error}; rollback lỗi: {restore}"))?;
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
                    "{error}; rollback cleanup={cleanup_error:?}, restore={restore_error:?}; backup {}",
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

fn supported_video(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "mp4" | "mov" | "mkv" | "webm" | "avi" | "m4v"
            )
        })
}

/// Extract one decoded video frame for the canvas without modifying project media.
pub fn extract_video_frame(video: &Path, seconds: f64, output: &Path) -> Result<(), String> {
    if !seconds.is_finite() || seconds < 0.0 {
        return Err("Vị trí frame không hợp lệ".into());
    }
    if !video.is_file() {
        return Err("Không tìm thấy video nền trong project".into());
    }
    let ffmpeg = vietkara_media::find_binary("ffmpeg").map_err(|error| error.to_string())?;
    let output_result = Command::new(ffmpeg)
        .args(["-y", "-v", "error", "-ss"])
        .arg(format!("{seconds:.3}"))
        .arg("-i")
        .arg(video)
        .args(["-frames:v", "1", "-q:v", "2", "-update", "1"])
        .arg(output)
        .output()
        .map_err(|error| format!("Không chạy được FFmpeg: {error}"))?;
    if !output_result.status.success() {
        return Err(format!(
            "Không trích xuất được frame video: {}",
            String::from_utf8_lossy(&output_result.stderr).trim()
        ));
    }
    if !output.is_file()
        || std::fs::metadata(output)
            .map(|meta| meta.len())
            .unwrap_or(0)
            == 0
    {
        return Err("FFmpeg không tạo được frame preview".into());
    }
    Ok(())
}

/// Import a project-local video source and a lightweight H.264 proxy. The
/// first proxy frame is also published as `background.jpg` for the GPUI canvas.
pub fn import_video_background(
    project: &Project,
    main: &Path,
    source: &Path,
) -> Result<ImportedBackground, String> {
    if !supported_video(source) {
        return Err("Nền video chỉ hỗ trợ MP4, MOV, MKV, WebM, AVI hoặc M4V".into());
    }
    if !source.is_file() {
        return Err("Không tìm thấy file video nền".into());
    }
    project.validate().map_err(|error| error.to_string())?;
    let directory = main
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let extension = source
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("mp4")
        .to_ascii_lowercase();
    let source_name = format!("background_source.{extension}");
    let destination_source = directory.join(&source_name);
    let destination_proxy = directory.join("background_preview.mp4");
    let destination_frame = directory.join("background.jpg");
    let id = vietkara_domain::new_id();
    let staged_source = directory.join(format!(".background-source-{id}.{extension}"));
    let staged_proxy = directory.join(format!(".background-proxy-{id}.mp4"));
    let staged_frame = directory.join(format!(".background-frame-{id}.jpg"));
    let backup = directory.join(format!(".background-backup-{id}"));
    let result = (|| -> Result<ImportedBackground, String> {
        std::fs::copy(source, &staged_source).map_err(|error| error.to_string())?;
        let ffmpeg = vietkara_media::find_binary("ffmpeg").map_err(|error| error.to_string())?;
        let info = vietkara_media::probe_with(
            source,
            &vietkara_media::find_binary("ffprobe").unwrap_or_else(|_| ffmpeg.clone()),
            &ffmpeg,
        )
        .map_err(|error| error.to_string())?;
        let output = Command::new(&ffmpeg)
            .args(["-y", "-v", "error", "-i"])
            .arg(&staged_source)
            .args([
                "-map",
                "0:v:0",
                "-an",
                "-vf",
                "scale='min(960,iw)':-2",
                "-r",
                "24",
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-crf",
                "27",
                "-pix_fmt",
                "yuv420p",
                "-movflags",
                "+faststart",
            ])
            .arg(&staged_proxy)
            .output()
            .map_err(|error| format!("Không chạy được FFmpeg: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "Không tạo được proxy video: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        let frame = Command::new(&ffmpeg)
            .args(["-y", "-v", "error", "-i"])
            .arg(&staged_proxy)
            .args(["-frames:v", "1", "-q:v", "2", "-update", "1"])
            .arg(&staged_frame)
            .output()
            .map_err(|error| format!("Không trích xuất được khung preview: {error}"))?;
        if !frame.status.success() {
            return Err(format!(
                "Không trích xuất được khung preview: {}",
                String::from_utf8_lossy(&frame.stderr).trim()
            ));
        }
        for path in [&staged_proxy, &staged_frame] {
            if !path.is_file() || std::fs::metadata(path).map(|m| m.len()).unwrap_or(0) == 0 {
                return Err("FFmpeg không tạo được media preview hợp lệ".into());
            }
        }
        let mut candidate = project.clone();
        candidate.settings.extra.insert(
            "background_type".into(),
            serde_json::Value::String("video".into()),
        );
        candidate
            .settings
            .extra
            .insert("use_background".into(), serde_json::Value::Bool(true));
        candidate.extra.insert("background_video".into(), serde_json::json!({"name": source.file_name().unwrap_or_default().to_string_lossy(), "source_file": source_name, "preview_file": "background_preview.mp4", "duration": info.duration, "fps": info.fps, "width": info.width, "height": info.height}));
        candidate.validate().map_err(|error| error.to_string())?;
        let version = create_version(directory, project, "Trước thay nền video")
            .map_err(|error| error.to_string())?;
        std::fs::create_dir(&backup).map_err(|error| error.to_string())?;
        let targets = [
            (&destination_source, &staged_source),
            (&destination_proxy, &staged_proxy),
            (&destination_frame, &staged_frame),
        ];
        let names = [&source_name, "background_preview.mp4", "background.jpg"];
        for (index, (target, _)) in targets.iter().enumerate() {
            if target.exists() {
                std::fs::rename(target, backup.join(format!("{index}")))
                    .map_err(|error| error.to_string())?;
            }
        }
        for (target, staged) in targets {
            if let Err(error) = std::fs::rename(staged, target) {
                for (index, name) in names.iter().enumerate() {
                    let published = directory.join(name);
                    if published.exists() {
                        let _ = std::fs::remove_file(published);
                    }
                    let saved = backup.join(format!("{index}"));
                    if saved.exists() {
                        let _ = std::fs::rename(saved, directory.join(name));
                    }
                }
                return Err(error.to_string());
            }
        }
        if let Err(error) = save_project(&candidate, main) {
            for (index, name) in names.iter().enumerate() {
                let published = directory.join(name);
                if published.exists() {
                    let _ = std::fs::remove_file(published);
                }
                let saved = backup.join(format!("{index}"));
                if saved.exists() {
                    let _ = std::fs::rename(saved, directory.join(name));
                }
            }
            return Err(error.to_string());
        }
        let _ = std::fs::remove_dir_all(backup);
        Ok(ImportedBackground {
            project: candidate,
            version_id: version.id,
        })
    })();
    for path in [&staged_source, &staged_proxy, &staged_frame] {
        let _ = std::fs::remove_file(path);
    }
    result
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
    fn accepts_supported_background_video_extensions_case_insensitively() {
        for extension in ["mp4", "mov", "mkv", "webm", "avi", "m4v", "MP4"] {
            assert!(supported_video(Path::new(&format!(
                "background.{extension}"
            ))));
        }
        for extension in ["mp3", "gif", "exe", ""] {
            assert!(!supported_video(Path::new(&format!(
                "background.{extension}"
            ))));
        }
    }

    #[test]
    fn imports_video_proxy_frame_and_project_metadata() {
        let Ok(ffmpeg) = vietkara_media::find_binary("ffmpeg") else {
            return;
        };
        let directory = tempdir().unwrap();
        let main = directory.path().join("project.json");
        let source = directory.path().join("clip.mp4");
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
                "color=c=blue:s=64x64:d=1",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(&source)
            .output()
            .unwrap();
        assert!(generated.status.success());
        let imported = import_video_background(&project, &main, &source).unwrap();
        let saved = Project::load(&main).unwrap();
        assert!(directory.path().join("background_source.mp4").is_file());
        assert!(directory.path().join("background_preview.mp4").is_file());
        assert!(directory.path().join("background.jpg").is_file());
        let scrub_frame = directory.path().join("scrub-frame.jpg");
        extract_video_frame(
            &directory.path().join("background_preview.mp4"),
            0.5,
            &scrub_frame,
        )
        .unwrap();
        assert!(scrub_frame.is_file());
        assert_eq!(saved.settings.extra["background_type"], "video");
        assert_eq!(
            saved.extra["background_video"]["preview_file"],
            "background_preview.mp4"
        );
        assert!(
            directory
                .path()
                .join("versions")
                .join(imported.version_id)
                .join("project.json")
                .is_file()
        );
    }

    #[test]
    fn imports_png_logo_with_version_snapshot_and_enables_render_setting() {
        let Ok(ffmpeg) = vietkara_media::find_binary("ffmpeg") else {
            return;
        };
        let directory = tempdir().unwrap();
        let main = directory.path().join("project.json");
        let source = directory.path().join("brand.png");
        let generated = Command::new(ffmpeg)
            .args([
                "-y",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=red@0.5:s=32x32",
                "-frames:v",
                "1",
                "-update",
                "1",
                "-f",
                "image2",
            ])
            .arg(&source)
            .output()
            .unwrap();
        assert!(
            generated.status.success(),
            "{}",
            String::from_utf8_lossy(&generated.stderr)
        );
        let project = Project::new("fixture");
        project.save(&main).unwrap();

        let imported = import_logo(&project, &main, &source).unwrap();
        let saved = Project::load(&main).unwrap();

        assert!(directory.path().join("logo.png").is_file());
        assert_eq!(saved.settings.extra["logo_enabled"], true);
        assert_eq!(saved.extra["logo_name"], "logo.png");
        assert!(
            directory
                .path()
                .join("versions")
                .join(imported.version_id)
                .join("project.json")
                .is_file()
        );
    }

    #[test]
    fn imports_custom_countdown_png_and_selects_image_mode() {
        let Ok(ffmpeg) = vietkara_media::find_binary("ffmpeg") else {
            return;
        };
        let directory = tempdir().unwrap();
        let main = directory.path().join("project.json");
        let source = directory.path().join("count-in.png");
        let generated = Command::new(ffmpeg)
            .args([
                "-y",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=yellow:s=24x24",
                "-frames:v",
                "1",
                "-update",
                "1",
                "-f",
                "image2",
            ])
            .arg(&source)
            .output()
            .unwrap();
        assert!(
            generated.status.success(),
            "{}",
            String::from_utf8_lossy(&generated.stderr)
        );
        let project = Project::new("fixture");
        project.save(&main).unwrap();

        let imported = import_countdown_image(&project, &main, &source).unwrap();
        let saved = Project::load(&main).unwrap();

        assert!(directory.path().join("countdown.png").is_file());
        assert_eq!(saved.settings.extra["countdown"], "image");
        assert!(
            directory
                .path()
                .join("versions")
                .join(imported.version_id)
                .join("project.json")
                .is_file()
        );
    }

    #[test]
    fn rejects_non_png_countdown_without_touching_project_or_assets() {
        let directory = tempdir().unwrap();
        let main = directory.path().join("project.json");
        let source = directory.path().join("countdown.gif");
        std::fs::write(&source, b"not png").unwrap();
        let error = match import_countdown_image(&Project::new("fixture"), &main, &source) {
            Ok(_) => panic!("non-PNG countdown image was accepted"),
            Err(error) => error,
        };
        assert!(error.contains("PNG"));
        assert!(!main.exists());
        assert!(!directory.path().join("countdown.png").exists());
    }

    #[test]
    fn rejects_non_png_logo_without_touching_project_or_assets() {
        let directory = tempdir().unwrap();
        let main = directory.path().join("project.json");
        let source = directory.path().join("brand.jpg");
        std::fs::write(&source, b"not png").unwrap();
        let error = match import_logo(&Project::new("fixture"), &main, &source) {
            Ok(_) => panic!("non-PNG logo was accepted"),
            Err(error) => error,
        };
        assert!(error.contains("PNG"));
        assert!(!main.exists());
        assert!(!directory.path().join("logo.png").exists());
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
