//! Separate recovery files. Inspection and acceptance never overwrite the main file.
use crate::{StorageError, load_project, save_project};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    time::SystemTime,
};
use vietkara_domain::Project;

pub fn autosave_path(main: &Path) -> PathBuf {
    let mut name = OsString::from(".");
    name.push(main.file_name().unwrap_or_default());
    name.push(".autosave.json");
    main.with_file_name(name)
}

pub fn save_autosave(project: &Project, main: &Path) -> Result<(), StorageError> {
    project.validate()?;
    if main.exists() && load_project(main)?.id != project.id {
        return Err(StorageError::RecoveryIdentityMismatch);
    }
    save_project(project, &autosave_path(main))
}

pub struct RecoveryCandidate {
    pub path: PathBuf,
    pub modified: SystemTime,
    project: Project,
}
impl RecoveryCandidate {
    pub fn project(&self) -> &Project {
        &self.project
    }
    /// Call only after the user chooses recovery. Returns the inspected snapshot
    /// in memory; an explicit normal save is still required to update the main file.
    pub fn accept(self) -> Project {
        self.project
    }
}

pub fn recovery_candidate(main: &Path) -> Result<Option<RecoveryCandidate>, StorageError> {
    let path = autosave_path(main);
    let metadata = match std::fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let modified = metadata.modified()?;
    match std::fs::metadata(main) {
        Ok(main_metadata) if modified <= main_metadata.modified()? => return Ok(None),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    if metadata.len() > crate::MAX_PROJECT_JSON_SIZE {
        return Err(StorageError::ArchiveTooLarge);
    }
    let project = load_project(&path)?;
    if main.exists() && load_project(main)?.id != project.id {
        return Err(StorageError::RecoveryIdentityMismatch);
    }
    Ok(Some(RecoveryCandidate {
        path,
        modified,
        project,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{File, FileTimes};
    use std::time::Duration;
    fn timestamp(path: &Path, seconds: u64) {
        File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_times(
                FileTimes::new()
                    .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)),
            )
            .unwrap();
    }
    #[test]
    fn recovers_newer_snapshot_only_by_explicit_acceptance() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("Bài hát.json");
        let mut project = Project::new("original");
        save_project(&project, &main).unwrap();
        let original = std::fs::read(&main).unwrap();
        project.name = "edited".into();
        save_autosave(&project, &main).unwrap();
        timestamp(&main, 10);
        timestamp(&autosave_path(&main), 20);
        let candidate = recovery_candidate(&main).unwrap().unwrap();
        assert_eq!(candidate.project().name, "edited");
        let recovered = candidate.accept();
        assert_eq!(recovered.id, project.id);
        assert_eq!(std::fs::read(&main).unwrap(), original);
        timestamp(&main, 30);
        assert!(recovery_candidate(&main).unwrap().is_none());
    }
    #[test]
    fn bad_autosave_does_not_touch_main_or_previous_recovery() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("project.json");
        let mut project = Project::new("original");
        save_project(&project, &main).unwrap();
        save_autosave(&project, &main).unwrap();
        let bytes = std::fs::read(autosave_path(&main)).unwrap();
        project.settings.font_size = f64::NAN;
        assert!(save_autosave(&project, &main).is_err());
        assert_eq!(std::fs::read(autosave_path(&main)).unwrap(), bytes);
        assert!(save_autosave(&Project::new("other"), &main).is_err());
        std::fs::write(autosave_path(&main), b"broken").unwrap();
        timestamp(&main, 10);
        timestamp(&autosave_path(&main), 20);
        assert!(recovery_candidate(&main).is_err());
        assert_eq!(load_project(&main).unwrap().name, "original");
    }
}
