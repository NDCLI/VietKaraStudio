//! SRT import follows static/app.js: beat-clock cues are mapped to reference time.
use crate::{MAX_WORDS, Project, ProjectError, Word, edit::Command, new_id};
use regex::Regex;
use thiserror::Error;
#[derive(Debug, Error)]
pub enum ImportError {
    #[error("Mở khóa các câu trước khi thay toàn bộ lời")]
    Locked,
    #[error("Không đọc được câu nào từ SRT")]
    Empty,
    #[error(transparent)]
    Invalid(#[from] ProjectError),
}

/// Rebuild `project.text` from timed words using the Python studio's line
/// ordering and blank-line marker behavior.
pub fn text_from_words(words: &[Word]) -> String {
    let mut indexes = std::collections::HashMap::new();
    let mut lines: Vec<(u32, Vec<&Word>)> = Vec::new();
    for word in words {
        let index = if let Some(index) = indexes.get(&word.line) {
            *index
        } else {
            let index = lines.len();
            lines.push((word.line, vec![word]));
            indexes.insert(word.line, index);
            continue;
        };
        lines[index].1.push(word);
    }
    let mut output = String::new();
    for (index, (_, line)) in lines.iter().enumerate() {
        if index > 0 {
            output.push('\n');
            if line.first().is_some_and(|word| word.section == Some(true)) {
                output.push('\n');
            }
        }
        output.push_str(
            &line
                .iter()
                .map(|word| word.text.as_str())
                .collect::<Vec<_>>()
                .join(" "),
        );
    }
    output
}
pub fn import_srt(project: &Project, source: &str) -> Result<Command, ImportError> {
    project.validate()?;
    if project.words.iter().any(|word| word.locked == Some(true)) {
        return Err(ImportError::Locked);
    }
    let cues = Regex::new(r"(\d+):(\d+):(\d+)[,.](\d{3})\s*-->\s*(\d+):(\d+):(\d+)[,.](\d{3})")
        .expect("constant cue regex");
    let tags = Regex::new(r"<[^>]+>").expect("constant tag regex");
    let blocks = Regex::new(r"\n\s*\n").expect("constant block regex");
    let source = source.trim_start_matches('\u{feff}').replace('\r', "");
    let mut words = Vec::new();
    let mut line = 0u32;
    let mut texts = Vec::new();
    for block in blocks.split(source.trim()) {
        let lines: Vec<_> = block.lines().collect();
        let Some(index) = lines.iter().position(|line| line.contains("-->")) else {
            continue;
        };
        let Some(capture) = cues.captures(lines[index]) else {
            continue;
        };
        let seconds = |i: usize| -> Option<f64> {
            Some(
                capture[i].parse::<f64>().ok()? * 3600.
                    + capture[i + 1].parse::<f64>().ok()? * 60.
                    + capture[i + 2].parse::<f64>().ok()?
                    + capture[i + 3].parse::<f64>().ok()? / 1000.,
            )
        };
        let (Some(start), Some(end)) = (seconds(1), seconds(5)) else {
            continue;
        };
        let joined = lines[index + 1..].join(" ");
        let stripped = tags.replace_all(&joined, "");
        let tokens: Vec<_> = stripped.split_whitespace().take(MAX_WORDS + 1).collect();
        let uid = new_id();
        let mut text = Vec::new();
        for (i, token) in tokens.iter().enumerate() {
            let a = crate::timing::map_time(
                start + (end - start) * i as f64 / tokens.len() as f64,
                &project.anchors,
                true,
            );
            let b = crate::timing::map_time(
                start + (end - start) * (i + 1) as f64 / tokens.len() as f64,
                &project.anchors,
                true,
            );
            if !a.is_finite() || !b.is_finite() || a < 0. || b <= a {
                continue;
            }
            if words.len() >= MAX_WORDS {
                return Err(ProjectError::TooManyWords.into());
            }
            let token: String = token.chars().take(200).collect();
            let word:Word=serde_json::from_value(serde_json::json!({"id":new_id(),"line_uid":uid,"text":token,"start":a,"end":b,"line":line,"role":"all","estimated":true})).map_err(ProjectError::from)?;
            text.push(word.text.clone());
            words.push(word);
        }
        if !text.is_empty() {
            texts.push(text.join(" "));
        }
        line += 1;
    }
    if words.is_empty() {
        return Err(ImportError::Empty);
    }
    let mut candidate = project.clone();
    candidate.words = words;
    candidate.text = texts.join("\n");
    candidate.migrate()?;
    candidate.validate()?;
    Ok(Command::SetLyrics {
        words: candidate.words,
        text: candidate.text,
    })
}

/// Estimate word timings for plain, one-line-per-lyric text, matching
/// `timing.estimate_words` in the Python editor. Times are in beat seconds.
pub fn estimate_lyrics(
    project: &Project,
    source: &str,
    duration: f64,
) -> Result<Command, ImportError> {
    project.validate()?;
    if project.words.iter().any(|word| word.locked == Some(true)) {
        return Err(ImportError::Locked);
    }
    if !duration.is_finite() || duration <= 0. {
        return Err(ImportError::Empty);
    }
    let lines: Vec<Vec<&str>> = source
        .lines()
        .map(str::split_whitespace)
        .map(|line| line.collect::<Vec<_>>())
        .filter(|line: &Vec<&str>| !line.is_empty())
        .collect();
    let count: usize = lines.iter().map(Vec::len).sum();
    if count == 0 {
        return Err(ImportError::Empty);
    }
    if count > MAX_WORDS {
        return Err(ProjectError::TooManyWords.into());
    }
    let mut cursor = 5_f64.min(duration * 0.05);
    let step = ((duration - cursor - 1.) / count as f64).max(0.1);
    let mut words = Vec::with_capacity(count);
    for (line_index, tokens) in lines.iter().enumerate() {
        let line_uid = new_id();
        for token in tokens {
            let text: String = token.chars().take(200).collect();
            words.push(Word {
                id: new_id(),
                text,
                start: cursor,
                end: cursor + step * 0.9,
                line_uid: line_uid.clone(),
                role: "all".into(),
                line: line_index as u32,
                estimated: true,
                locked: None,
                section: None,
                section_name: None,
                manual: None,
                label_color: None,
                row: None,
                extra: Default::default(),
            });
            cursor += step;
        }
    }
    let text = lines
        .iter()
        .map(|line| line.join(" "))
        .collect::<Vec<_>>()
        .join("\n");
    let mut candidate = project.clone();
    candidate.words = words;
    candidate.text = text.clone();
    candidate.migrate()?;
    candidate.validate()?;
    Ok(Command::SetLyrics {
        words: candidate.words,
        text,
    })
}

#[cfg(test)]
mod estimate_tests {
    use super::*;

    #[test]
    fn estimates_one_word_per_token_and_preserves_line_boundaries() {
        let project = Project::new("Bài hát");
        let Command::SetLyrics { words, text } =
            estimate_lyrics(&project, "Xin chào\nTạm biệt", 60.).unwrap()
        else {
            panic!("expected lyrics command");
        };
        assert_eq!(words.len(), 4);
        assert_eq!(text, "Xin chào\nTạm biệt");
        assert_eq!(words[0].line, words[1].line);
        assert_ne!(words[1].line_uid, words[2].line_uid);
        assert!(words.iter().all(|word| word.estimated));
        assert!((words[0].start - 3.).abs() < 1e-9);
        assert_eq!(text_from_words(&words), text);
        let mut section_words = words;
        section_words[2].section = Some(true);
        assert_eq!(text_from_words(&section_words), "Xin chào\n\nTạm biệt");
    }

    #[test]
    fn rejects_empty_lyrics_and_locked_words() {
        let project = Project::new("Bài hát");
        assert!(matches!(
            estimate_lyrics(&project, "\n  ", 30.),
            Err(ImportError::Empty)
        ));
        assert!(matches!(
            estimate_lyrics(&project, "Xin chào", f64::NAN),
            Err(ImportError::Empty)
        ));
        let mut locked = project;
        locked.words.push(Word {
            id: new_id(),
            text: "Xin".into(),
            start: 0.,
            end: 1.,
            line_uid: new_id(),
            role: "all".into(),
            line: 0,
            estimated: false,
            locked: Some(true),
            section: None,
            section_name: None,
            manual: None,
            label_color: None,
            row: None,
            extra: Default::default(),
        });
        assert!(matches!(
            estimate_lyrics(&locked, "Xin chào", 30.),
            Err(ImportError::Locked)
        ));
    }
}
