//! Save As preserves Python's project-local media layout.
use crate::{StorageError, managed_media, save_project, sha256_file};
use std::{io, path::Path};
use vietkara_domain::Project;

pub fn save_project_copy(
    project: &Project,
    source: Option<&Path>,
    target: &Path,
) -> Result<(), StorageError> {
    project.validate()?;
    let Some(source) = source else {
        return save_project(project, target);
    };
    let source_dir = source.parent().unwrap_or_else(|| Path::new("."));
    let target_dir = target
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(target_dir)?;
    if std::fs::canonicalize(source_dir)? == std::fs::canonicalize(target_dir)? {
        return save_project(project, target);
    }
    let media = managed_media(source_dir);
    // Preflight all collisions before adding any files. Never replace target media.
    for source_file in &media {
        let destination = target_dir.join(source_file.file_name().unwrap());
        if destination.exists() && sha256_file(source_file)? != sha256_file(&destination)? {
            return Err(StorageError::MediaCopyConflict(destination));
        }
    }
    for source_file in &media {
        let destination = target_dir.join(source_file.file_name().unwrap());
        if destination.exists() {
            continue;
        }
        let temporary = target_dir.join(format!(".media-copy-{}", vietkara_domain::new_id()));
        let result = (|| -> Result<(), StorageError> {
            std::fs::copy(source_file, &temporary)?;
            // Hard-link publish is atomic and fails if a destination appeared meanwhile.
            match std::fs::hard_link(&temporary, &destination) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    if sha256_file(&temporary)? == sha256_file(&destination)? {
                        Ok(())
                    } else {
                        Err(StorageError::MediaCopyConflict(destination.clone()))
                    }
                }
                // Fall back on filesystems that do not implement hard links.
                Err(_) => copy_publish_noclobber(&temporary, &destination),
            }
        })();
        let _ = std::fs::remove_file(&temporary);
        result?;
    }
    // The JSON is published last. On failure, existing JSON/media remain intact;
    // newly added media may remain and can be reused by a retry.
    save_project(project, target)
}

fn copy_publish_noclobber(source: &Path, destination: &Path) -> Result<(), StorageError> {
    let parent = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut staged = tempfile::NamedTempFile::new_in(parent)?;
    io::copy(&mut std::fs::File::open(source)?, staged.as_file_mut())?;
    staged.as_file().sync_all()?;
    match staged.persist_noclobber(destination) {
        Ok(_) => Ok(()),
        Err(error) if error.error.kind() == io::ErrorKind::AlreadyExists => {
            if sha256_file(source)? == sha256_file(destination)? {
                Ok(())
            } else {
                Err(StorageError::MediaCopyConflict(destination.to_owned()))
            }
        }
        Err(error) => Err(error.error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn save_as_copies_managed_media_and_keeps_unmanaged_files_out() {
        let root = tempfile::tempdir().unwrap();
        let source_dir = root.path().join("source");
        let target_dir = root.path().join("target");
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::create_dir_all(&target_dir).unwrap();
        let source_project = source_dir.join("project.json");
        let target_project = target_dir.join("project.json");
        let audio = b"fixture audio";
        std::fs::write(source_dir.join("beat.wav"), audio).unwrap();
        std::fs::write(source_dir.join("notes.txt"), b"not project media").unwrap();

        let mut project = Project::new("copy");
        project.beat = Some(json!({"filename": "beat.wav", "sha256": "fixture"}));
        save_project_copy(&project, Some(&source_project), &target_project).unwrap();

        assert_eq!(std::fs::read(target_dir.join("beat.wav")).unwrap(), audio);
        assert!(!target_dir.join("notes.txt").exists());
        assert_eq!(Project::load(&target_project).unwrap().name, "copy");
        assert_eq!(std::fs::read(source_dir.join("beat.wav")).unwrap(), audio);
    }

    #[test]
    fn save_as_accepts_identical_existing_media_on_retry() {
        let root = tempfile::tempdir().unwrap();
        let source_dir = root.path().join("source");
        let target_dir = root.path().join("target");
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::create_dir_all(&target_dir).unwrap();
        let source_project = source_dir.join("project.json");
        let target_project = target_dir.join("project.json");
        std::fs::write(source_dir.join("beat.wav"), b"same bytes").unwrap();
        std::fs::write(target_dir.join("beat.wav"), b"same bytes").unwrap();
        let project = Project::new("retry");

        save_project_copy(&project, Some(&source_project), &target_project).unwrap();

        assert!(target_project.is_file());
        assert_eq!(
            std::fs::read(target_dir.join("beat.wav")).unwrap(),
            b"same bytes"
        );
    }

    #[test]
    fn media_conflict_leaves_existing_target_and_project_untouched() {
        let root = tempfile::tempdir().unwrap();
        let source_dir = root.path().join("source");
        let target_dir = root.path().join("target");
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::create_dir_all(&target_dir).unwrap();
        let source_project = source_dir.join("project.json");
        let target_project = target_dir.join("project.json");
        std::fs::write(source_dir.join("beat.wav"), b"source audio").unwrap();
        std::fs::write(target_dir.join("beat.wav"), b"target audio").unwrap();
        std::fs::write(&target_project, b"previous project bytes").unwrap();
        let before = std::fs::read(&target_project).unwrap();

        let result = save_project_copy(
            &Project::new("conflict"),
            Some(&source_project),
            &target_project,
        );

        assert!(matches!(result, Err(StorageError::MediaCopyConflict(_))));
        assert_eq!(std::fs::read(&target_project).unwrap(), before);
        assert_eq!(
            std::fs::read(target_dir.join("beat.wav")).unwrap(),
            b"target audio"
        );
    }

    #[test]
    fn no_clobber_copy_publish_supports_filesystems_without_hard_links() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("staged");
        let destination = root.path().join("published");
        std::fs::write(&source, b"complete media bytes").unwrap();

        copy_publish_noclobber(&source, &destination).unwrap();
        assert_eq!(
            std::fs::read(&destination).unwrap(),
            b"complete media bytes"
        );

        std::fs::write(&source, b"different bytes").unwrap();
        assert!(matches!(
            copy_publish_noclobber(&source, &destination),
            Err(StorageError::MediaCopyConflict(_))
        ));
        assert_eq!(
            std::fs::read(&destination).unwrap(),
            b"complete media bytes"
        );
    }
}
