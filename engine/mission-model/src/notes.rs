//! Notes: operator intent on a mission, as threads of entries. Notes are
//! read and translated into briefs, staffing and wires; they never run.
//!
//! `<slug>.notes.toml` is schema 2: `mission_id`, `rev`, and one `[[entry]]`
//! per entry with its `target`: `mission` for the mission's own thread, a
//! node id, or a connection id for a note on a wire. An entry that
//! `resolves` its thread says what was done about it; the thread is
//! resolved while that entry is its latest, and a later entry opens it
//! again.

use serde::{Deserialize, Deserializer, Serialize};

use ahjo_core::Error;

pub const NOTES_SCHEMA: i64 = 3;
/// The target of the mission's own thread.
pub const MISSION_THREAD: &str = "mission";
const MAX_ENTRY_BYTES: usize = 64 * 1024;
const MAX_NOTES_BYTES: usize = 512 * 1024;

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields, rename_all = "snake_case")]
pub struct Notes {
    pub schema: i64,
    pub mission_id: String,
    pub rev: i64,
    #[serde(deserialize_with = "timestamp")]
    pub updated_at: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub updated_by: String,
    /// Every entry, oldest first within each thread.
    #[serde(rename = "entry")]
    pub entries: Vec<NoteEntry>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields, rename_all = "snake_case")]
pub struct NoteEntry {
    pub id: String,
    pub target: String,
    /// `human:<name>` or `agent:<name>`.
    pub author: String,
    #[serde(deserialize_with = "timestamp")]
    pub created_at: String,
    #[serde(
        deserialize_with = "optional_timestamp",
        skip_serializing_if = "Option::is_none"
    )]
    pub edited_at: Option<String>,
    pub text: String,
    /// This entry answers its thread for good: what was done about it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub resolves: bool,
}

impl Notes {
    /// No notes yet for a mission.
    pub fn empty(mission_id: &str) -> Notes {
        Notes {
            schema: NOTES_SCHEMA,
            mission_id: mission_id.to_owned(),
            ..Notes::default()
        }
    }

    pub fn parse(text: &str) -> Result<Notes, Error> {
        let invalid = |message| Error::new("invalid_definition_source", message);
        let notes: Notes = toml::from_str(text).map_err(|err| invalid(err.to_string()))?;
        if notes.schema != NOTES_SCHEMA {
            return Err(Error::new(
                "unsupported_schema",
                format!("notes schema {}", notes.schema),
            ));
        }
        if notes.mission_id.trim().is_empty() || notes.rev < 1 || notes.updated_at.is_empty() {
            return Err(invalid("invalid mission notes metadata".to_owned()));
        }
        let mut seen = std::collections::HashSet::new();
        let mut size = 0;
        for entry in &notes.entries {
            if entry.id.trim().is_empty() || !seen.insert(&entry.id) {
                return Err(invalid(format!(
                    "note entry id {:?} is missing or repeated",
                    entry.id
                )));
            }
            if entry.target.trim().is_empty() {
                return Err(invalid(format!("note entry {:?} has no target", entry.id)));
            }
            if !valid_author(&entry.author) {
                return Err(invalid(format!(
                    "note entry {:?} has invalid author {:?}",
                    entry.id, entry.author
                )));
            }
            if entry.created_at.is_empty() {
                return Err(invalid(format!(
                    "note entry {:?} has no created_at",
                    entry.id
                )));
            }
            if entry.text.len() > MAX_ENTRY_BYTES {
                return Err(invalid(format!(
                    "note entry {:?} is larger than 64 KiB",
                    entry.id
                )));
            }
            size += entry.author.len() + entry.text.len();
        }
        if size > MAX_NOTES_BYTES {
            return Err(invalid("the notes are larger than 512 KiB".to_owned()));
        }
        Ok(notes)
    }

    /// Whether a thread has been answered for good: its latest entry
    /// resolves it.
    pub fn resolved(&self, target: &str) -> bool {
        self.thread(target)
            .last()
            .is_some_and(|entry| entry.resolves)
    }

    /// One thread, oldest entry first.
    pub fn thread<'a>(&'a self, target: &'a str) -> impl Iterator<Item = &'a NoteEntry> {
        self.entries
            .iter()
            .filter(move |entry| entry.target == target)
    }
}

/// `human:` or `agent:`, then a letter or digit and letters, digits or `._@-`.
pub fn valid_author(author: &str) -> bool {
    let Some(name) = author
        .strip_prefix("human:")
        .or_else(|| author.strip_prefix("agent:"))
    else {
        return false;
    };
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || "._@-".contains(c))
}

/// An RFC 3339 time with an offset, written as a TOML datetime or a string.
fn timestamp<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Written {
        Datetime(toml::value::Datetime),
        Text(String),
    }
    let written = match Written::deserialize(deserializer)? {
        Written::Datetime(datetime) => datetime.to_string(),
        Written::Text(text) => text,
    };
    let datetime: toml::value::Datetime = (written.parse())
        .map_err(|_| serde::de::Error::custom(format!("{written:?} is not an RFC 3339 time")))?;
    if datetime.date.is_none() || datetime.time.is_none() || datetime.offset.is_none() {
        return Err(serde::de::Error::custom(format!(
            "{written:?} needs a date, a time and an offset"
        )));
    }
    Ok(written)
}

fn optional_timestamp<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    timestamp(deserializer).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTES: &str = r#"schema = 3
mission_id = "msn_a"
rev = 1
updated_at = "2026-09-09T00:00:00Z"

[[entry]]
id = "nte_1"
target = "mission"
author = "agent:archon"
created_at = "2026-09-09T00:00:00Z"
text = "The whole mission"

[[entry]]
id = "nte_2"
target = "fmn_plan"
author = "human:ui"
created_at = 2026-09-10T08:00:00+03:00
edited_at = 2026-09-10T09:00:00Z
text = "This step"
"#;

    #[test]
    fn notes_are_threads_by_target() {
        let notes = Notes::parse(NOTES).unwrap();
        assert_eq!(notes.thread(MISSION_THREAD).count(), 1);
        let step: Vec<_> = notes.thread("fmn_plan").collect();
        assert_eq!(step[0].edited_at.as_deref(), Some("2026-09-10T09:00:00Z"));
    }

    #[test]
    fn notes_refuse_bad_entries() {
        for (from, to) in [
            ("author = \"human:ui\"", "author = \"operator\""),
            ("id = \"nte_2\"", "id = \"nte_1\""),
            (
                "created_at = 2026-09-10T08:00:00+03:00",
                "created_at = \"yesterday\"",
            ),
            ("schema = 3", "schema = 1"),
            (
                "text = \"This step\"",
                "text = \"This step\"\ncolour = \"red\"",
            ),
        ] {
            assert!(Notes::parse(&NOTES.replace(from, to)).is_err(), "{to}");
        }
    }
}
