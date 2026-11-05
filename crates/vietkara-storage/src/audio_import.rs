//! Checkpointed media replacement; JSON and WAV roll back together on save failure.
use crate::{create_version,save_project,sha256_file};
use std::path::Path;
use vietkara_domain::Project;
#[derive(Clone,Copy)]
pub enum AudioSlot {Beat,Reference}
pub struct ImportedAudio {pub project:Project,pub version_id:String,pub relinked:bool}
pub fn import_audio(project:&Project,main:&Path,source:&Path,slot:AudioSlot)->Result<ImportedAudio,String> {
    project.validate().map_err(|e|e.to_string())?;
    let directory=main.parent().filter(|p|!p.as_os_str().is_empty()).unwrap_or_else(||Path::new("."));
    let name=match slot {AudioSlot::Beat=>"beat",AudioSlot::Reference=>"reference"};
    let destination=directory.join(format!("{name}.wav"));
    let temporary=directory.join(format!(".import-{}.wav",vietkara_domain::new_id()));
    let backup=directory.join(format!(".backup-{}.wav",vietkara_domain::new_id()));
    let result=(||->Result<ImportedAudio,String> {
        let ffmpeg=vietkara_media::find_binary("ffmpeg").map_err(|e|e.to_string())?;
        vietkara_media::convert_to_reference_wav(source,&temporary,&ffmpeg).map_err(|e|e.to_string())?;
        let wave=vietkara_media::waveform(&temporary,2000).map_err(|e|e.to_string())?;
        let fingerprint=sha256_file(&temporary).map_err(|e|e.to_string())?;
        let old=match slot {AudioSlot::Beat=>&project.beat,AudioSlot::Reference=>&project.reference};
        let relinked=old.as_ref().and_then(|v|v.get("sha256")).and_then(|v|v.as_str()).is_some_and(|v|v==fingerprint)
            || (destination.is_file() && sha256_file(&destination).map_err(|e|e.to_string())?==fingerprint);
        let metadata=serde_json::json!({"name":source.file_name().unwrap_or_default().to_string_lossy(),"filename":format!("{name}.wav"),"duration":wave.duration,"sample_rate":wave.sample_rate,"peaks":wave.peaks,"sha256":fingerprint});
        let mut candidate=project.clone();
        if !relinked {
            if old.is_some() {candidate.anchors.clear();}
            candidate.alignment=None;
            if matches!(slot,AudioSlot::Reference) && old.is_some() {candidate.words.clear();}
        }
        match slot {AudioSlot::Beat=>candidate.beat=Some(metadata),AudioSlot::Reference=>candidate.reference=Some(metadata)}
        candidate.validate().map_err(|e|e.to_string())?;
        let version=create_version(directory,project,"Trước nhập/relink audio").map_err(|e|e.to_string())?;
        let had_destination=destination.exists();
        if had_destination {std::fs::rename(&destination,&backup).map_err(|e|e.to_string())?;}
        if let Err(error)=std::fs::rename(&temporary,&destination) {
            if had_destination {std::fs::rename(&backup,&destination).map_err(|restore|format!("{error}; rollback lỗi: {restore}; backup {}",backup.display()))?;}
            return Err(error.to_string());
        }
        if let Err(error)=save_project(&candidate,main) {
            std::fs::remove_file(&destination).map_err(|restore|format!("{error}; rollback lỗi: {restore}"))?;
            if had_destination {std::fs::rename(&backup,&destination).map_err(|restore|format!("{error}; rollback lỗi: {restore}; backup {}",backup.display()))?;}
            return Err(error.to_string());
        }
        if had_destination {let _=std::fs::remove_file(&backup);}
        Ok(ImportedAudio{project:candidate,version_id:version.id,relinked})
    })();
    let _=std::fs::remove_file(temporary);
    result
}
