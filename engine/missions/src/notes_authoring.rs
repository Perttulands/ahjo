//! Author-owned note threads, protected by a notes ETag.
use crate::{Error, Store};
use ahjo_mission_model::{NoteEntry, NotePatch, Notes, NotesEdit};
use toml_edit::{DocumentMut, Item, value};

impl Store {
    pub fn notes_edit(&self, selector: &str) -> Result<NotesEdit, Error> {
        let scope = crate::write::locks(self.root(), "scope");
        let _reference_guard = scope.references.read().unwrap_or_else(|e| e.into_inner());
        let slug = self.resolve(selector)?;
        let locks = crate::write::locks(self.root(), &format!("mission:{slug}"));
        let _document_guard = locks.document.lock().unwrap_or_else(|e| e.into_inner());
        if self.resolve(selector)? != slug {
            return Err(Error::new("conflict", "mission selector changed"));
        }
        self.notes_edit_locked(&slug)
    }
    fn notes_edit_locked(&self, slug: &str) -> Result<NotesEdit, Error> {
        let mission = self.read_mission(slug)?;
        let path = self.root().join("notes").join(format!("{slug}.notes.toml"));
        match crate::links::read_definition(&path) {
            Ok(raw) => {
                let notes = Notes::parse(&raw)?;
                if notes.mission_id == mission.id {
                    Ok(NotesEdit {
                        notes,
                        etag: crate::authoring::etag(&raw),
                    })
                } else {
                    Ok(NotesEdit {
                        notes: Notes::empty(&mission.id),
                        etag: "*".into(),
                    })
                }
            }
            Err(error) if error.code == "not_found" => Ok(NotesEdit {
                notes: Notes::empty(&mission.id),
                etag: "*".into(),
            }),
            Err(e) => Err(e),
        }
    }
    pub fn patch_notes(
        &self,
        selector: &str,
        expected_etag: &str,
        patch: NotePatch,
    ) -> Result<NotesEdit, Error> {
        let scope = crate::write::locks(self.root(), "scope");
        let _reference_guard = scope.references.read().unwrap_or_else(|e| e.into_inner());
        let slug = self.resolve(selector)?;
        let locks = crate::write::locks(self.root(), &format!("mission:{slug}"));
        let _document_guard = locks.document.lock().unwrap_or_else(|e| e.into_inner());
        if self.resolve(selector)? != slug {
            return Err(Error::new("conflict", "mission selector changed"));
        }
        let mission = self.read_mission(&slug)?;
        let current = self.notes_edit_locked(&slug)?;
        let (target, author, text) = match &patch {
            NotePatch::Append {
                target,
                author,
                text,
            }
            | NotePatch::Edit {
                target,
                author,
                text,
                ..
            } => (target.trim(), author.as_str(), Some(text.as_str())),
            NotePatch::Reply {
                to, author, text, ..
            } => {
                let entry = (current.notes.entries.iter())
                    .find(|entry| entry.id == *to)
                    .ok_or_else(|| Error::new("not_found", format!("note entry {to:?}")))?;
                (entry.target.as_str(), author.as_str(), Some(text.as_str()))
            }
            NotePatch::Delete { target, author, .. } => (target.trim(), author.as_str(), None),
        };
        let target = if target.is_empty() { "mission" } else { target };
        let author = author.trim();
        if !ahjo_mission_model::valid_author(author) || text.is_some_and(|s| s.trim().is_empty()) {
            return Err(Error::new(
                "invalid_note_patch",
                "invalid author or empty text",
            ));
        }
        if text.is_some_and(|s| s.len() > 64 * 1024) {
            return Err(Error::new("note_too_large", "entry exceeds 64 KiB"));
        }
        // A note goes on the mission, a card, or a wire.
        if target != "mission"
            && !mission.input_cards.iter().any(|n| n.id == target)
            && mission.formation(target).is_none()
            && mission.gate(target).is_none()
            && mission.end(target).is_none()
            && !mission.limits.iter().any(|n| n.id == target)
            && !mission.connections.iter().any(|n| n.id == target)
        {
            return Err(Error::new("not_found", "note target"));
        }
        if expected_etag.is_empty() {
            return Err(Error::new(
                "precondition_required",
                "notes ETag is required",
            ));
        }
        if expected_etag != current.etag {
            return Err(Error::new("conflict", "notes changed"));
        }
        let path = self.root().join("notes").join(format!("{slug}.notes.toml"));
        let mut doc = if current.etag == "*" {
            let mut doc = DocumentMut::new();
            doc["schema"] = value(ahjo_mission_model::NOTES_SCHEMA);
            crate::write::set(
                doc.as_table_mut(),
                "mission_id",
                toml_edit::value(mission.id.clone()),
            );
            doc
        } else {
            let raw = crate::links::read_definition(&path)?;
            raw.parse().map_err(|e: toml_edit::TomlError| {
                Error::new("invalid_definition", e.to_string())
            })?
        };
        crate::write::table_arrays(&mut doc, &["entry"]);
        let now = crate::authoring::now()?;
        match &patch {
            NotePatch::Append { text, .. } | NotePatch::Reply { text, .. } => {
                let entry = NoteEntry {
                    id: crate::authoring::id("nte")?,
                    target: target.into(),
                    author: author.into(),
                    created_at: now.clone(),
                    edited_at: None,
                    text: text.clone(),
                    resolves: matches!(patch, NotePatch::Reply { resolves: true, .. }),
                };
                crate::authoring_create::append(&mut doc, "entry", &entry)?;
            }
            NotePatch::Edit { id, text, .. } => {
                let entries = doc
                    .get_mut("entry")
                    .and_then(Item::as_array_of_tables_mut)
                    .ok_or_else(|| Error::new("not_found", "note entry"))?;
                let entry = entries
                    .iter_mut()
                    .find(|t| {
                        t.get("id").and_then(Item::as_str) == Some(id.as_str())
                            && t.get("target").and_then(Item::as_str) == Some(target)
                    })
                    .ok_or_else(|| Error::new("not_found", "note entry"))?;
                if entry.get("author").and_then(Item::as_str) != Some(author) {
                    return Err(Error::new(
                        "note_author_mismatch",
                        "entry belongs to another author",
                    ));
                }
                if entry.get("text").and_then(Item::as_str) == Some(text.as_str()) {
                    return Ok(current);
                }
                entry["text"] = value(text.as_str());
                entry["edited_at"] = value(now.clone());
            }
            NotePatch::Delete { id, .. } => {
                let entries = doc
                    .get_mut("entry")
                    .and_then(Item::as_array_of_tables_mut)
                    .ok_or_else(|| Error::new("not_found", "note entry"))?;
                let index = entries
                    .iter()
                    .position(|t| {
                        t.get("id").and_then(Item::as_str) == Some(id.as_str())
                            && t.get("target").and_then(Item::as_str) == Some(target)
                    })
                    .ok_or_else(|| Error::new("not_found", "note entry"))?;
                if entries
                    .get(index)
                    .and_then(|t| t.get("author"))
                    .and_then(Item::as_str)
                    != Some(author)
                {
                    return Err(Error::new(
                        "note_author_mismatch",
                        "entry belongs to another author",
                    ));
                }
                entries.remove(index);
            }
        }
        crate::write::set(
            doc.as_table_mut(),
            "rev",
            toml_edit::value(current.notes.rev + 1),
        );
        crate::write::set(doc.as_table_mut(), "updated_at", toml_edit::value(now));
        crate::write::set(doc.as_table_mut(), "updated_by", toml_edit::value(author));
        let raw = doc.to_string();
        let notes = Notes::parse(&raw)?;
        if current.etag == "*" && std::fs::symlink_metadata(&path).is_ok() {
            let archive = path.with_file_name(format!(
                "{slug}.notes.toml.deleted-{}",
                crate::authoring::id("archive")?
            ));
            std::fs::rename(&path, archive).map_err(crate::error::io)?;
        }
        crate::write::atomic(&path, raw.as_bytes()).map_err(crate::error::io)?;
        Ok(NotesEdit {
            notes,
            etag: crate::authoring::etag(&raw),
        })
    }
}
