//! SRT import follows static/app.js: beat-clock cues are mapped to reference time.
use crate::{Project, ProjectError, Word, MAX_WORDS, new_id, edit::Command};
use regex::Regex;
use thiserror::Error;
#[derive(Debug,Error)]
pub enum ImportError {
    #[error("Mở khóa các câu trước khi thay toàn bộ lời")]
    Locked,
    #[error("Không đọc được câu nào từ SRT")]
    Empty,
    #[error(transparent)]
    Invalid(#[from] ProjectError),
}
pub fn import_srt(project:&Project, source:&str)->Result<Command,ImportError> {
    project.validate()?;
    if project.words.iter().any(|word|word.locked==Some(true)) {return Err(ImportError::Locked);}
    let cues=Regex::new(r"(\d+):(\d+):(\d+)[,.](\d{3})\s*-->\s*(\d+):(\d+):(\d+)[,.](\d{3})").expect("constant cue regex");
    let tags=Regex::new(r"<[^>]+>").expect("constant tag regex");
    let blocks=Regex::new(r"\n\s*\n").expect("constant block regex");
    let source=source.trim_start_matches('\u{feff}').replace('\r',"");
    let mut words=Vec::new();
    let mut line=0u32;
    let mut texts=Vec::new();
    for block in blocks.split(source.trim()) {
        let lines:Vec<_>=block.lines().collect();
        let Some(index)=lines.iter().position(|line|line.contains("-->")) else {continue;};
        let Some(capture)=cues.captures(lines[index]) else {continue;};
        let seconds=|i:usize|->Option<f64> {Some(capture[i].parse::<f64>().ok()?*3600.+capture[i+1].parse::<f64>().ok()?*60.+capture[i+2].parse::<f64>().ok()?+capture[i+3].parse::<f64>().ok()?/1000.)};
        let (Some(start),Some(end))=(seconds(1),seconds(5)) else {continue;};
        let joined=lines[index+1..].join(" ");
        let stripped=tags.replace_all(&joined,"");
        let tokens:Vec<_>=stripped.split_whitespace().take(MAX_WORDS+1).collect();
        let uid=new_id();
        let mut text=Vec::new();
        for (i,token) in tokens.iter().enumerate() {
            let a=crate::timing::map_time(start+(end-start)*i as f64/tokens.len() as f64,&project.anchors,true);
            let b=crate::timing::map_time(start+(end-start)*(i+1) as f64/tokens.len() as f64,&project.anchors,true);
            if !a.is_finite() || !b.is_finite() || a<0. || b<=a {continue;}
            if words.len()>=MAX_WORDS {return Err(ProjectError::TooManyWords.into());}
            let token:String=token.chars().take(200).collect();
            let word:Word=serde_json::from_value(serde_json::json!({"id":new_id(),"line_uid":uid,"text":token,"start":a,"end":b,"line":line,"role":"all","estimated":true})).map_err(ProjectError::from)?;
            text.push(word.text.clone()); words.push(word);
        }
        if !text.is_empty() {texts.push(text.join(" "));}
        line+=1;
    }
    if words.is_empty() {return Err(ImportError::Empty);}
    let mut candidate=project.clone();
    candidate.words=words; candidate.text=texts.join("\n");
    candidate.migrate()?; candidate.validate()?;
    Ok(Command::SetLyrics {words:candidate.words,text:candidate.text})
}
