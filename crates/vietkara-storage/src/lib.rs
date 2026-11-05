//! Storage for VietKara Studio projects: atomic save, version snapshots
//! and portable bundles.
//!
//! Ports the rules from Python `project_archive.py`, with the safety
//! fixes the plan (`RUST_REWRITE_PLAN.md` §2.3) requires:
//!
//! * `restore_version` and bundle import **verify everything first**
//!   (size + SHA-256) in a staging directory and only then swap media
//!   in. Python's `restore_version` deletes the current media *before*
//!   copying the snapshot, and `read_bundle` extracts media into the
//!   destination before the manifest/fingerprint check finishes — both
//!   can leave a project with missing or half-written media on error.
//!   Rust must not copy that order of operations.
//! * Limits: ZIP 3 GiB / 2,000 entries, project JSON 20 MiB, media
//!   allowlist (`FIXED_MEDIA` + the `MEDIA_PATTERN` from Python).
//!
//! `Project::save` in `vietkara-domain` is already atomic; this crate
//! adds the archive/version/bundle layer on top of it.

pub mod audio_import;
pub mod autosave;
pub mod background_import;
pub mod copy;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;
use vietkara_domain::{Project, ProjectError, is_hex32, new_id};

pub const MAX_ARCHIVE_SIZE: u64 = 3 * 1024 * 1024 * 1024;
pub const MAX_ARCHIVE_ENTRIES: usize = 2_000;
pub const MAX_PROJECT_JSON_SIZE: u64 = 20 * 1024 * 1024;

pub const FIXED_MEDIA: &[&str] = &[
    "reference.wav",
    "beat.wav",
    "vocals.wav",
    "reference_instrumental.wav",
    "background.jpg",
    "logo.png",
    "countdown.png",
];

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("Media đích đã tồn tại với nội dung khác: {0}")]
    MediaCopyConflict(PathBuf),
    #[error("Bản phục hồi thuộc project khác.")]
    RecoveryIdentityMismatch,
    #[error("Lỗi đọc/ghi file: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON không hợp lệ: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Lỗi project: {0}")]
    Project(#[from] ProjectError),
    #[error("Lỗi ZIP: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("Mã phiên bản không hợp lệ.")]
    InvalidId,
    #[error("Không tìm thấy phiên bản project.")]
    VersionNotFound,
    #[error("Phiên bản thiếu file media: {0}")]
    MissingMedia(String),
    #[error("File media không khớp dấu vân tay: {0}")]
    FingerprintMismatch(String),
    #[error("Tên file media không hợp lệ: {0}")]
    InvalidMediaName(String),
    #[error("Project ZIP vượt quá giới hạn dung lượng.")]
    ArchiveTooLarge,
    #[error("Project ZIP thiếu project.json hợp lệ.")]
    MissingProjectJson,
    #[error("Project ZIP có manifest không hợp lệ.")]
    InvalidManifest,
    #[error("File project ZIP không đọc được.")]
    BadArchive,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MediaEntry {
    pub name: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionRecord {
    pub id: String,
    pub created_at: f64,
    pub name: String,
    pub note: String,
    pub media: Vec<MediaEntry>,
}

/// Python `MEDIA_PATTERN` + `FIXED_MEDIA`, without a regex dependency:
/// `(reference|beat|background)_source.<ext 1-8>`,
/// `(reference|beat|background)_preview.mp4`,
/// `scene_(decoration_)?[0-9a-f]{32}.png`.
pub fn is_managed_media_name(name: &str) -> bool {
    if FIXED_MEDIA.contains(&name) {
        return true;
    }
    for slot in ["reference", "beat", "background"] {
        if let Some(rest) = name.strip_prefix(&format!("{slot}_source."))
            && !rest.is_empty()
            && rest.len() <= 8
            && rest
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        {
            return true;
        }
        if name == format!("{slot}_preview.mp4") {
            return true;
        }
    }
    if let Some(rest) = name.strip_prefix("scene_") {
        let rest = rest.strip_prefix("decoration_").unwrap_or(rest);
        if let Some(hex) = rest.strip_suffix(".png") {
            return is_hex32(hex);
        }
    }
    false
}

pub fn sha256_file(path: &Path) -> Result<String, StorageError> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 1024 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub fn sha256_bytes(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

pub fn managed_media(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str())
            && is_managed_media_name(name)
        {
            out.push(path);
        }
    }
    out.sort();
    out
}

pub fn media_manifest(dir: &Path) -> Result<Vec<MediaEntry>, StorageError> {
    let mut out = Vec::new();
    for item in managed_media(dir) {
        let name = item.file_name().unwrap().to_string_lossy().into_owned();
        out.push(MediaEntry {
            size: item.metadata()?.len(),
            sha256: sha256_file(&item)?,
            name,
        });
    }
    Ok(out)
}

fn unix_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Write bytes atomically: temp file in the same directory + rename.
pub fn write_atomic(path: &Path, data: &[u8]) -> Result<(), StorageError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_owned());
    let temp = parent.join(format!(
        ".{file_name}.tmp-{}-{}",
        std::process::id(),
        new_id()
    ));
    let result = (|| -> Result<(), StorageError> {
        std::fs::write(&temp, data)?;
        std::fs::rename(&temp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

pub fn load_project(path: &Path) -> Result<Project, StorageError> {
    Ok(Project::load(path)?)
}

pub fn save_project(project: &Project, path: &Path) -> Result<(), StorageError> {
    Ok(project.save(path)?)
}

// ---------------------------------------------------------------------------
// Version snapshots
// ---------------------------------------------------------------------------

pub fn create_version(
    project_dir: &Path,
    project: &Project,
    note: &str,
) -> Result<VersionRecord, StorageError> {
    let versions = project_dir.join("versions");
    std::fs::create_dir_all(&versions)?;
    let id = new_id();
    let target = versions.join(&id);
    let assets = target.join("media");
    std::fs::create_dir_all(&assets)?;
    let mut normalised = project.clone();
    normalised.migrate()?;
    let project_json = serde_json::to_vec_pretty(&normalised)?;
    write_atomic(&target.join("project.json"), &project_json)?;
    let mut entries = Vec::new();
    for item in managed_media(project_dir) {
        let name = item.file_name().unwrap().to_string_lossy().into_owned();
        let target_file = assets.join(&name);
        if std::fs::hard_link(&item, &target_file).is_err() {
            std::fs::copy(&item, &target_file)?;
        }
        entries.push(MediaEntry {
            size: target_file.metadata()?.len(),
            sha256: sha256_file(&target_file)?,
            name,
        });
    }
    let record = VersionRecord {
        id,
        created_at: unix_now(),
        name: project.name.clone(),
        note: note.trim().chars().take(200).collect(),
        media: entries,
    };
    write_atomic(
        &target.join("version.json"),
        &serde_json::to_vec_pretty(&record)?,
    )?;
    Ok(record)
}

pub fn list_versions(project_dir: &Path) -> Vec<VersionRecord> {
    let mut out = Vec::new();
    let root = project_dir.join("versions");
    let Ok(entries) = std::fs::read_dir(&root) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !is_hex32(name) {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(path.join("version.json"))
            && let Ok(record) = serde_json::from_str::<VersionRecord>(&text)
        {
            out.push(record);
        }
    }
    out.sort_by(|a, b| {
        b.created_at
            .partial_cmp(&a.created_at)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

fn read_version(
    project_dir: &Path,
    identifier: &str,
) -> Result<(Project, VersionRecord), StorageError> {
    if !is_hex32(identifier) {
        return Err(StorageError::InvalidId);
    }
    let source = project_dir.join("versions").join(identifier);
    let project = Project::load(source.join("project.json")).map_err(|e| match e {
        ProjectError::Io(_) => StorageError::VersionNotFound,
        other => StorageError::Project(other),
    })?;
    let record_text = std::fs::read_to_string(source.join("version.json"))
        .map_err(|_| StorageError::VersionNotFound)?;
    let record: VersionRecord =
        serde_json::from_str(&record_text).map_err(|_| StorageError::VersionNotFound)?;
    Ok((project, record))
}

/// Restore a version safely: verify every snapshot file (size + SHA-256)
/// first, stage the verified media, then swap. On any error the current
/// media is left untouched, or rolled back from the backup directory.
pub fn restore_version(project_dir: &Path, identifier: &str) -> Result<Project, StorageError> {
    let (project, record) = read_version(project_dir, identifier)?;
    let source_dir = project_dir.join("versions").join(identifier).join("media");

    // 1. Verify everything before touching the live project.
    for entry in &record.media {
        if !is_managed_media_name(&entry.name) {
            continue;
        }
        let source_file = source_dir.join(&entry.name);
        if !source_file.is_file() {
            return Err(StorageError::MissingMedia(entry.name.clone()));
        }
        if source_file.metadata()?.len() != entry.size || sha256_file(&source_file)? != entry.sha256
        {
            return Err(StorageError::FingerprintMismatch(entry.name.clone()));
        }
    }

    // 2. Stage verified media in a temp directory inside the project.
    let staging = project_dir.join(format!(".restore-{}", new_id()));
    std::fs::create_dir_all(&staging)?;
    let mut staged: Vec<(PathBuf, PathBuf)> = Vec::new();
    let stage_result = (|| -> Result<(), StorageError> {
        for entry in &record.media {
            if !is_managed_media_name(&entry.name) {
                continue;
            }
            let from = source_dir.join(&entry.name);
            let to = staging.join(&entry.name);
            std::fs::copy(&from, &to)?;
            staged.push((to, project_dir.join(&entry.name)));
        }
        Ok(())
    })();
    if let Err(e) = stage_result {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e);
    }

    // 3. Swap: move current media to backup, move staged media in,
    //    then write project.json atomically. Roll back on failure.
    let backup = project_dir.join(format!(".restore-backup-{}", new_id()));
    std::fs::create_dir_all(&backup)?;
    let mut moved_in: Vec<PathBuf> = Vec::new();
    let mut moved_out: Vec<(PathBuf, PathBuf)> = Vec::new();
    let swap = (|| -> Result<(), StorageError> {
        for item in managed_media(project_dir) {
            let name = item.file_name().unwrap().to_string_lossy().into_owned();
            let dest = backup.join(&name);
            std::fs::rename(&item, &dest)?;
            moved_out.push((dest, item));
        }
        for (from, to) in &staged {
            std::fs::rename(from, to)?;
            moved_in.push(to.clone());
        }
        project.save(project_dir.join("project.json"))?;
        Ok(())
    })();
    if let Err(e) = swap {
        for path in &moved_in {
            let _ = std::fs::remove_file(path);
        }
        for (from, to) in &moved_out {
            let _ = std::fs::rename(from, to);
        }
        let _ = std::fs::remove_dir_all(&staging);
        let _ = std::fs::remove_dir_all(&backup);
        return Err(e);
    }
    let _ = std::fs::remove_dir_all(&staging);
    let _ = std::fs::remove_dir_all(&backup);
    Ok(project)
}

// ---------------------------------------------------------------------------
// Portable bundles (ZIP)
// ---------------------------------------------------------------------------

pub fn write_bundle(
    project: &Project,
    project_dir: &Path,
    include_media: bool,
    output: &Path,
) -> Result<(), StorageError> {
    let mut normalised = project.clone();
    normalised.migrate()?;
    let project_json = serde_json::to_vec_pretty(&normalised)?;
    if project_json.len() as u64 > MAX_PROJECT_JSON_SIZE {
        return Err(StorageError::ArchiveTooLarge);
    }
    let entries = media_manifest(project_dir)?;
    if entries.len() > MAX_ARCHIVE_ENTRIES {
        return Err(StorageError::ArchiveTooLarge);
    }
    let manifest = serde_json::json!({
        "format": "vietkara-project",
        "version": 1,
        "includes_media": include_media,
        "media": entries,
    });
    let file = File::create(output)?;
    let mut archive = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    archive.start_file("project.json", options)?;
    archive.write_all(&project_json)?;
    archive.start_file("manifest.json", options)?;
    archive.write_all(serde_json::to_vec_pretty(&manifest)?.as_slice())?;
    if include_media {
        for entry in &entries {
            archive.start_file(format!("media/{}", entry.name), options)?;
            let mut media = File::open(project_dir.join(&entry.name))?;
            std::io::copy(&mut media, &mut archive)?;
        }
    }
    archive.finish()?;
    Ok(())
}

/// Result of `read_bundle`: the migrated project, the copied media
/// names, and the manifest fingerprints.
pub type BundleImport = (Project, Vec<String>, HashMap<String, MediaEntry>);

/// Read a bundle into `destination`, staging first and only moving media
/// in after the manifest/fingerprint checks pass. Returns the migrated
/// project, the copied media names, and the manifest fingerprints.
pub fn read_bundle<R: Read + Seek>(
    source: R,
    destination: &Path,
) -> Result<BundleImport, StorageError> {
    let mut archive = zip::ZipArchive::new(source).map_err(|_| StorageError::BadArchive)?;
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err(StorageError::ArchiveTooLarge);
    }
    let total: u64 = (0..archive.len())
        .filter_map(|i| archive.by_index(i).ok().map(|f| f.size()))
        .sum();
    if total > MAX_ARCHIVE_SIZE {
        return Err(StorageError::ArchiveTooLarge);
    }

    let project_index = (0..archive.len()).find(|&i| {
        archive
            .by_index(i)
            .map(|f| f.name() == "project.json")
            .unwrap_or(false)
    });
    let Some(project_index) = project_index else {
        return Err(StorageError::MissingProjectJson);
    };
    let mut project_file = archive
        .by_index(project_index)
        .map_err(|_| StorageError::BadArchive)?;
    if project_file.size() > MAX_PROJECT_JSON_SIZE {
        return Err(StorageError::MissingProjectJson);
    }
    let mut project_bytes = Vec::new();
    project_file.read_to_end(&mut project_bytes)?;
    drop(project_file);
    let mut project: Project =
        serde_json::from_slice(&project_bytes).map_err(|_| StorageError::MissingProjectJson)?;
    if project.schema > 2 {
        return Err(StorageError::Project(ProjectError::FutureSchema(
            project.schema,
        )));
    }
    project.migrate()?;

    let mut expected: HashMap<String, MediaEntry> = HashMap::new();
    if let Ok(mut manifest_file) = archive.by_name("manifest.json") {
        let mut text = String::new();
        manifest_file
            .read_to_string(&mut text)
            .map_err(|_| StorageError::InvalidManifest)?;
        let manifest: serde_json::Value =
            serde_json::from_str(&text).map_err(|_| StorageError::InvalidManifest)?;
        let media = manifest
            .get("media")
            .and_then(|m| m.as_array())
            .ok_or(StorageError::InvalidManifest)?;
        for item in media {
            let entry: MediaEntry =
                serde_json::from_value(item.clone()).map_err(|_| StorageError::InvalidManifest)?;
            expected.insert(entry.name.clone(), entry);
        }
    }

    // Stage media in a sibling temp directory; nothing is written to
    // `destination` until every file has been verified.
    let staging = destination.with_file_name(format!(
        ".{}-staging-{}",
        destination
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "project".to_owned()),
        new_id()
    ));
    std::fs::create_dir_all(&staging)?;
    let mut copied: Vec<String> = Vec::new();
    let mut seen: HashMap<String, ()> = HashMap::new();
    let extract = (|| -> Result<Vec<String>, StorageError> {
        let mut names: Vec<String> = Vec::new();
        for i in 0..archive.len() {
            let mut file = archive.by_index(i).map_err(|_| StorageError::BadArchive)?;
            let name = file.name().to_owned();
            let Some(media_name) = name.strip_prefix("media/") else {
                continue;
            };
            if media_name.is_empty()
                || media_name.contains('/')
                || media_name.contains('\\')
                || !is_managed_media_name(media_name)
            {
                return Err(StorageError::InvalidMediaName(media_name.to_owned()));
            }
            if seen.contains_key(media_name) {
                return Err(StorageError::InvalidMediaName(media_name.to_owned()));
            }
            seen.insert(media_name.to_owned(), ());
            let target = staging.join(media_name);
            let mut out = File::create(&target)?;
            std::io::copy(&mut file, &mut out)?;
            drop(out);
            if let Some(record) = expected.get(media_name) {
                let size = target.metadata()?.len();
                if size != record.size || sha256_file(&target)? != record.sha256 {
                    let _ = std::fs::remove_file(&target);
                    return Err(StorageError::FingerprintMismatch(media_name.to_owned()));
                }
            }
            names.push(media_name.to_owned());
        }
        Ok(names)
    })();
    let copied_names = match extract {
        Ok(names) => names,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e);
        }
    };
    copied.extend(copied_names);

    std::fs::create_dir_all(destination)?;
    for name in &copied {
        std::fs::rename(staging.join(name), destination.join(name))?;
    }
    project.save(destination.join("project.json"))?;
    let _ = std::fs::remove_dir_all(&staging);
    Ok((project, copied, expected))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn sample_project() -> Project {
        let mut project = Project::new("Bài test");
        project.words = vec![vietkara_domain::Word {
            id: new_id(),
            text: "Xin".into(),
            start: 0.2,
            end: 0.6,
            line_uid: new_id(),
            role: "all".into(),
            line: 0,
            estimated: false,
            locked: None,
            section: None,
            section_name: None,
            manual: None,
            label_color: None,
            row: None,
            extra: Default::default(),
        }];
        project
    }

    fn project_dir_with_media(dir: &Path) -> Project {
        std::fs::write(dir.join("beat.wav"), b"fake-beat-bytes").unwrap();
        std::fs::write(dir.join("reference.wav"), b"fake-reference").unwrap();
        std::fs::write(dir.join("notes.txt"), b"not managed").unwrap();
        sample_project()
    }

    #[test]
    fn managed_media_allowlist_matches_python() {
        assert!(is_managed_media_name("beat.wav"));
        assert!(is_managed_media_name("reference_source.mp3"));
        assert!(is_managed_media_name("background_preview.mp4"));
        assert!(is_managed_media_name(&format!(
            "scene_{}.png",
            "a".repeat(32)
        )));
        assert!(is_managed_media_name(&format!(
            "scene_decoration_{}.png",
            "a".repeat(32)
        )));
        assert!(!is_managed_media_name("scene_abc.png"));
        assert!(!is_managed_media_name("evil.wav"));
        assert!(!is_managed_media_name("../beat.wav"));
        // Python's pattern allows any 1-8 char lowercase ext, so .exe
        // is (surprisingly) managed; uppercase and >8 chars are not.
        assert!(is_managed_media_name("beat_source.exe"));
        assert!(!is_managed_media_name("beat_source.MP3"));
        assert!(!is_managed_media_name("beat_source.toolongext"));
    }

    #[test]
    fn sha256_matches_python_hashlib() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.bin");
        std::fs::write(&path, b"hello\n").unwrap();
        // echo hello | sha256sum
        assert_eq!(
            sha256_file(&path).unwrap(),
            "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03"
        );
    }

    #[test]
    fn version_snapshot_and_safe_restore() {
        let dir = tempfile::tempdir().unwrap();
        let project_dir = dir.path().join("proj");
        std::fs::create_dir_all(&project_dir).unwrap();
        let project = project_dir_with_media(&project_dir);
        project.save(project_dir.join("project.json")).unwrap();
        let record = create_version(&project_dir, &project, "Trước khi sửa").unwrap();
        assert_eq!(list_versions(&project_dir).len(), 1);
        assert_eq!(list_versions(&project_dir)[0].id, record.id);

        // Change live media, then restore: snapshot media comes back.
        // Replace (rename) the live file, do not truncate it in place:
        // the snapshot is a hard link to the same inode, like Python's
        // `os.link` in `create_version`, so truncating would corrupt
        // the snapshot too. Callers must replace media atomically.
        std::fs::write(project_dir.join("beat.wav.next"), b"changed").unwrap();
        std::fs::rename(
            project_dir.join("beat.wav.next"),
            project_dir.join("beat.wav"),
        )
        .unwrap();
        let restored = restore_version(&project_dir, &record.id).unwrap();
        assert_eq!(restored.name, "Bài test");
        assert_eq!(
            std::fs::read(project_dir.join("beat.wav")).unwrap(),
            b"fake-beat-bytes"
        );
    }

    #[test]
    fn restore_with_corrupt_snapshot_leaves_live_media_alone() {
        let dir = tempfile::tempdir().unwrap();
        let project_dir = dir.path().join("proj");
        std::fs::create_dir_all(&project_dir).unwrap();
        let project = project_dir_with_media(&project_dir);
        project.save(project_dir.join("project.json")).unwrap();
        let record = create_version(&project_dir, &project, "v1").unwrap();
        // Corrupt the snapshot copy (not the live file). Replace it
        // via rename, not truncate-in-place, so the hard-linked live
        // file is not corrupted along with it.
        let snap_media = project_dir
            .join("versions")
            .join(&record.id)
            .join("media")
            .join("beat.wav");
        std::fs::write(project_dir.join("tampered.next"), b"tampered").unwrap();
        std::fs::rename(project_dir.join("tampered.next"), &snap_media).unwrap();
        std::fs::write(project_dir.join("beat.wav.next"), b"live-version").unwrap();
        std::fs::rename(
            project_dir.join("beat.wav.next"),
            project_dir.join("beat.wav"),
        )
        .unwrap();
        let err = restore_version(&project_dir, &record.id).unwrap_err();
        assert!(matches!(err, StorageError::FingerprintMismatch(_)));
        assert_eq!(
            std::fs::read(project_dir.join("beat.wav")).unwrap(),
            b"live-version"
        );
    }

    #[test]
    fn bundle_write_read_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let project_dir = dir.path().join("proj");
        std::fs::create_dir_all(&project_dir).unwrap();
        let project = project_dir_with_media(&project_dir);
        let bundle = dir.path().join("bundle.zip");
        write_bundle(&project, &project_dir, true, &bundle).unwrap();

        let dest = dir.path().join("imported");
        let bytes = std::fs::read(&bundle).unwrap();
        let (imported, copied, fingerprints) = read_bundle(Cursor::new(bytes), &dest).unwrap();
        assert_eq!(imported.name, "Bài test");
        assert!(copied.contains(&"beat.wav".to_string()));
        assert!(fingerprints.contains_key("beat.wav"));
        assert_eq!(
            std::fs::read(dest.join("beat.wav")).unwrap(),
            b"fake-beat-bytes"
        );
        assert_eq!(
            std::fs::read(dest.join("reference.wav")).unwrap(),
            b"fake-reference"
        );
        // Unmanaged files are not bundled.
        assert!(!dest.join("notes.txt").exists());
    }

    #[test]
    fn bundle_rejects_path_traversal_media() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("evil.zip");
        {
            let file = File::create(&bundle).unwrap();
            let mut archive = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            archive.start_file("project.json", options).unwrap();
            archive
                .write_all(br#"{"id":"0123456789abcdef0123456789abcdef","schema":2,"name":"x","words":[]}"#)
                .unwrap();
            archive.start_file("manifest.json", options).unwrap();
            archive
                .write_all(br#"{"format":"vietkara-project","version":1,"media":[]}"#)
                .unwrap();
            archive.start_file("media/../evil.wav", options).unwrap();
            archive.write_all(b"evil").unwrap();
            archive.finish().unwrap();
        }
        let bytes = std::fs::read(&bundle).unwrap();
        let dest = dir.path().join("imported");
        let err = read_bundle(Cursor::new(bytes), &dest).unwrap_err();
        assert!(matches!(err, StorageError::InvalidMediaName(_)));
        assert!(!dest.join("evil.wav").exists());
    }
}
