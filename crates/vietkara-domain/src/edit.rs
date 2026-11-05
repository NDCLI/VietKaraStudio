//! Transactional editor commands. Failed edits never publish a partial project.
use crate::{Anchor, Project, ProjectError, Word};
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Clone)]
pub enum Command {
    SetLyrics { words: Vec<Word>, text: String },
    InsertWord {
        index: usize,
        word: Box<Word>,
    },
    RemoveWord {
        id: String,
    },
    /// One undo step. Each child must leave a valid intermediate project.
    Batch(Vec<Command>),
    ReplaceWord {
        id: String,
        word: Box<Word>,
    },
    SetAnchors(Vec<Anchor>),
    SetSettings(crate::settings::Settings),
    SetScenes(Vec<Value>),
}

#[derive(Debug, Error)]
pub enum EditError {
    #[error("Vị trí chèn từ vượt quá số từ hiện tại: {0}")]
    InvalidInsertIndex(usize),
    #[error("Không tìm thấy từ: {0}")]
    MissingWord(String),
    #[error("Lệnh sửa từ không được đổi ID")]
    ChangedWordId,
    #[error(transparent)]
    InvalidProject(#[from] ProjectError),
}

impl Command {
    fn apply(&self, project: &mut Project) -> Result<Self, EditError> {
        Ok(match self {
            Self::SetLyrics {words,text} => Self::SetLyrics {
                words: std::mem::replace(&mut project.words,words.clone()),
                text: std::mem::replace(&mut project.text,text.clone()),
            },
            Self::InsertWord { index, word } => {
                if *index > project.words.len() {
                    return Err(EditError::InvalidInsertIndex(*index));
                }
                project.words.insert(*index, *word.clone());
                Self::RemoveWord {
                    id: word.id.clone(),
                }
            }
            Self::RemoveWord { id } => {
                let index = project
                    .words
                    .iter()
                    .position(|word| word.id == *id)
                    .ok_or_else(|| EditError::MissingWord(id.clone()))?;
                Self::InsertWord {
                    index,
                    word: Box::new(project.words.remove(index)),
                }
            }
            Self::Batch(commands) => {
                let mut inverses = Vec::with_capacity(commands.len());
                for command in commands {
                    inverses.push(command.apply(project)?);
                    project.validate()?;
                }
                inverses.reverse();
                Self::Batch(inverses)
            }
            Self::ReplaceWord { id, word } => {
                if word.id != *id {
                    return Err(EditError::ChangedWordId);
                }
                let target = project
                    .words
                    .iter_mut()
                    .find(|w| w.id == *id)
                    .ok_or_else(|| EditError::MissingWord(id.clone()))?;
                Self::ReplaceWord {
                    id: id.clone(),
                    word: Box::new(std::mem::replace(target, *word.clone())),
                }
            }
            Self::SetAnchors(value) => {
                Self::SetAnchors(std::mem::replace(&mut project.anchors, value.clone()))
            }
            Self::SetSettings(value) => {
                Self::SetSettings(std::mem::replace(&mut project.settings, value.clone()))
            }
            Self::SetScenes(value) => {
                Self::SetScenes(std::mem::replace(&mut project.scenes, value.clone()))
            }
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditEvent {
    Applied,
    Undone,
    Redone,
}

/// Owns the only mutable project. Consumers receive events only after validation.
/// Commands hold inverse values for affected fields, rather than whole projects.
pub struct Editor {
    project: Project,
    undo: Vec<Command>,
    redo: Vec<Command>,
}

impl Editor {
    pub fn new(project: Project) -> Result<Self, EditError> {
        project.validate()?;
        Ok(Self {
            project,
            undo: Vec::new(),
            redo: Vec::new(),
        })
    }
    pub fn project(&self) -> &Project {
        &self.project
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    fn commit(&mut self, command: &Command) -> Result<Command, EditError> {
        let mut candidate = self.project.clone();
        let inverse = command.apply(&mut candidate)?;
        candidate.validate()?;
        self.project = candidate;
        Ok(inverse)
    }
    pub fn execute(&mut self, command: Command) -> Result<EditEvent, EditError> {
        let inverse = self.commit(&command)?;
        self.undo.push(inverse);
        self.redo.clear();
        Ok(EditEvent::Applied)
    }
    pub fn undo(&mut self) -> Result<Option<EditEvent>, EditError> {
        let Some(command) = self.undo.last().cloned() else {
            return Ok(None);
        };
        let inverse = self.commit(&command)?;
        self.undo.pop();
        self.redo.push(inverse);
        Ok(Some(EditEvent::Undone))
    }
    pub fn redo(&mut self) -> Result<Option<EditEvent>, EditError> {
        let Some(command) = self.redo.last().cloned() else {
            return Ok(None);
        };
        let inverse = self.commit(&command)?;
        self.redo.pop();
        self.undo.push(inverse);
        Ok(Some(EditEvent::Redone))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Project {
        Project::load(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/project-v2.json"
        ))
        .unwrap()
    }
    #[test]
    fn edit_undo_redo_save_reopen_preserves_unknown_fields() {
        let original = fixture();
        let mut editor = Editor::new(original.clone()).unwrap();
        let mut word = original.words[0].clone();
        word.text = "Đã sửa".into();
        editor
            .execute(Command::ReplaceWord {
                id: word.id.clone(),
                word: Box::new(word),
            })
            .unwrap();
        let edited = serde_json::to_value(editor.project()).unwrap();
        editor.undo().unwrap();
        assert_eq!(
            serde_json::to_value(editor.project()).unwrap(),
            serde_json::to_value(&original).unwrap()
        );
        editor.redo().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("Bài hát.json");
        editor.project().save(&path).unwrap();
        assert_eq!(
            serde_json::to_value(Project::load(path).unwrap()).unwrap(),
            edited
        );
    }
    #[test]
    fn failed_command_preserves_project_and_redo() {
        let mut editor = Editor::new(fixture()).unwrap();
        editor
            .execute(Command::SetSettings(
                serde_json::json!({"unknown":true}).try_into().unwrap(),
            ))
            .unwrap();
        editor.undo().unwrap();
        let before = serde_json::to_value(editor.project()).unwrap();
        let mut word = editor.project().words[0].clone();
        word.end = -1.;
        assert!(
            editor
                .execute(Command::ReplaceWord {
                    id: word.id.clone(),
                    word: Box::new(word)
                })
                .is_err()
        );
        assert_eq!(serde_json::to_value(editor.project()).unwrap(), before);
        assert!(editor.can_redo());
        assert!(!editor.can_undo());
        editor.redo().unwrap();
        editor
            .execute(Command::SetScenes(vec![serde_json::json!({"future":42})]))
            .unwrap();
        assert!(!editor.can_redo());
    }
    #[test]
    fn batch_restores_order_anchors_settings_scenes_and_unknown_fields() {
        let original = fixture();
        let mut editor = Editor::new(original.clone()).unwrap();
        let mut word = original.words[0].clone();
        word.id = "123456789abcdef0123456789abcdef0".into();
        editor
            .execute(Command::Batch(vec![
                Command::RemoveWord {
                    id: original.words[1].id.clone(),
                },
                Command::InsertWord {
                    index: 0,
                    word: Box::new(word),
                },
                Command::SetAnchors(vec![]),
                Command::SetSettings(
                    serde_json::json!({"unknown_setting":42})
                        .try_into()
                        .unwrap(),
                ),
                Command::SetScenes(vec![serde_json::json!({"unknown_scene":true})]),
            ]))
            .unwrap();
        let edited = serde_json::to_value(editor.project()).unwrap();
        assert_eq!(editor.undo().unwrap(), Some(EditEvent::Undone));
        assert!(!editor.can_undo());
        assert_eq!(
            serde_json::to_value(editor.project()).unwrap(),
            serde_json::to_value(original).unwrap()
        );
        editor.redo().unwrap();
        assert_eq!(serde_json::to_value(editor.project()).unwrap(), edited);
    }

    #[test]
    fn late_batch_failure_is_atomic_and_insert_checks_bounds_and_duplicates() {
        let original = fixture();
        let expected = serde_json::to_value(&original).unwrap();
        let mut editor = Editor::new(original.clone()).unwrap();
        assert!(
            editor
                .execute(Command::Batch(vec![
                    Command::SetSettings(serde_json::json!({"changed":true}).try_into().unwrap()),
                    Command::RemoveWord {
                        id: "missing".into()
                    },
                ]))
                .is_err()
        );
        assert!(!editor.can_undo());
        assert!(!editor.can_redo());
        for index in [0, usize::MAX] {
            assert!(
                editor
                    .execute(Command::InsertWord {
                        index,
                        word: Box::new(original.words[0].clone())
                    })
                    .is_err()
            );
            assert_eq!(serde_json::to_value(editor.project()).unwrap(), expected);
        }
    }
}
