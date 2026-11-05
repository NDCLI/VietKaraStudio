//! Save As preserves Python's project-local media layout.
use crate::{StorageError, managed_media, save_project, sha256_file};
use std::path::Path;
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
                Err(error) => Err(error.into()),
            }
        })();
        let _ = std::fs::remove_file(&temporary);
        result?;
    }
    // The JSON is published last. On failure, existing JSON/media remain intact;
    // newly added media may remain and can be reused by a retry.
    save_project(project, target)
}
