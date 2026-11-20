//! The grouped notes carried by the notes endpoints.
use crate::NoteEntry;
use serde::{Deserialize, Serialize};

/// Notes are grouped by target on the wire, rather than the flat file form.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct NotesDocument {
    pub schema: i64,
    pub mission_id: String,
    pub rev: i64,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub updated_by: String,
    pub mission: Vec<Note>,
    pub elements: Vec<NoteThread>,
    pub etag: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct Note {
    pub id: String,
    pub author: String,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edited_at: Option<String>,
    pub text: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub resolves: bool,
}
impl From<NoteEntry> for Note {
    fn from(n: NoteEntry) -> Self {
        Self {
            id: n.id,
            author: n.author,
            created_at: n.created_at,
            edited_at: n.edited_at,
            text: n.text,
            resolves: n.resolves,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct NoteThread {
    pub node_id: String,
    pub entries: Vec<Note>,
}

impl NotesDocument {
    /// Group the file's entries exactly as the HTTP notes response does.
    pub fn from_notes(notes: crate::Notes, etag: String) -> Self {
        let mut mission = Vec::new();
        let mut elements: Vec<NoteThread> = Vec::new();
        for entry in notes.entries {
            let target = entry.target.clone();
            let note = Note::from(entry);
            if target == crate::MISSION_THREAD {
                mission.push(note);
            } else if let Some(thread) = elements.iter_mut().find(|t| t.node_id == target) {
                thread.entries.push(note);
            } else {
                elements.push(NoteThread {
                    node_id: target,
                    entries: vec![note],
                });
            }
        }
        Self {
            schema: notes.schema,
            mission_id: notes.mission_id,
            rev: notes.rev,
            updated_at: if notes.updated_at.is_empty() {
                "0001-01-01T00:00:00Z".into()
            } else {
                notes.updated_at
            },
            updated_by: notes.updated_by,
            mission,
            elements,
            etag,
        }
    }
}
