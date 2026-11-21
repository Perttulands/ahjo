//! Revision-checked authoring of a complete mission document.
use crate::{Error, Store};
use ahjo_mission_model::{DocumentChanges, Layout, Mission};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};
use toml_edit::{DocumentMut, TableLike, value};

pub struct AppliedDocument {
    pub mission: Mission,
    pub assigned_ids: Vec<String>,
    pub changed: DocumentChanges,
    /// Whether a different document was committed.
    pub wrote: bool,
}

impl Store {
    /// Read the exact bytes of a stored definition.
    pub fn export_mission(&self, selector: &str) -> Result<String, Error> {
        let slug = self.resolve(selector)?;
        crate::links::read_definition(&mission_path(self, &slug))
    }

    /// Prepare both documents under the same lock and publish them together.
    /// Validation findings are reported by the caller, so drafts can be saved.
    pub fn apply_document(&self, source: &str, dry_run: bool) -> Result<AppliedDocument, Error> {
        let incoming = Mission::parse(source)?;
        let slug = &incoming.slug;
        crate::store::check_slug(slug)?;
        let scope = crate::write::locks(self.root(), "scope");
        // Creating a definition can introduce a caller-supplied mission id.
        // Fence that uniqueness check across slugs as well as fencing the rev.
        let _references = scope.references.write().unwrap_or_else(|e| e.into_inner());
        let locks = crate::write::locks(self.root(), &format!("mission:{slug}"));
        let _document = locks.document.lock().unwrap_or_else(|e| e.into_inner());
        let path = mission_path(self, slug);
        let old_source = read_optional(&path)?;
        let before = old_source.as_deref().map(Mission::parse).transpose()?;
        let mut doc = document(source)?;
        if let Some(before) = &before {
            if doc.get("rev").and_then(toml_edit::Item::as_integer) != Some(before.rev) {
                return Err(Error::new(
                    "conflict",
                    "mission revision changed; export it again",
                ));
            }
            if !incoming.id.is_empty() && incoming.id != before.id {
                return Err(Error::new(
                    "conflict",
                    "mission id changed; export it again",
                ));
            }
            if old_source.as_deref() == Some(source) {
                return Ok(AppliedDocument {
                    mission: incoming,
                    assigned_ids: vec![],
                    changed: DocumentChanges::default(),
                    wrote: false,
                });
            }
        }

        let listings = self.list_missions()?;
        let other_names: HashSet<_> = listings
            .iter()
            .filter(|m| m.slug != *slug)
            .flat_map(|m| [m.id.clone(), m.slug.clone()])
            .collect();
        if before.is_none() && other_names.contains(slug) {
            return Err(Error::new(
                "conflict",
                "mission slug is already used as another mission's id",
            ));
        }
        let mut assigned_ids = Vec::new();
        let mut used: HashSet<String> = all_ids(&incoming)
            .filter(|id| !id.is_empty())
            .map(str::to_owned)
            .collect();
        if incoming.id.is_empty() {
            let id = if let Some(before) = &before {
                before.id.clone()
            } else {
                let mut taken = other_names.clone();
                taken.extend(used.iter().cloned());
                let name = if incoming.title.is_empty() {
                    slug
                } else {
                    &incoming.title
                };
                new_id("msn", name, &mut taken)
            };
            crate::write::set(doc.as_table_mut(), "id", value(id.clone()));
            used.insert(id.clone());
            assigned_ids.push(id);
        } else if before.is_none() && other_names.contains(&incoming.id) {
            return Err(Error::new(
                "conflict",
                "mission id is already used by another mission",
            ));
        }
        fill_ids(doc.as_table_mut(), &mut used, &mut assigned_ids);
        let rev = before
            .as_ref()
            .map_or(Some(1), |old| old.rev.checked_add(1))
            .ok_or_else(|| Error::new("conflict", "mission revision has reached its limit"))?;
        crate::write::set(doc.as_table_mut(), "rev", value(rev));
        let raw = doc.to_string();
        let after = Mission::parse(&raw)?;

        let removed: HashSet<String> = before
            .iter()
            .flat_map(card_ids)
            .filter(|id| !card_ids(&after).any(|current| current == *id))
            .map(str::to_owned)
            .collect();
        if !removed.is_empty() {
            let notes = self.read_notes(slug)?;
            if let Some(note) = notes
                .entries
                .iter()
                .find(|note| removed.contains(&note.target) && !notes.resolved(&note.target))
            {
                return Err(Error::new(
                    "conflict",
                    format!(
                        "cannot remove card {:?} while note {:?} is open",
                        note.target, note.id
                    ),
                ));
            }
        }
        let empty = Mission::default();
        let prior = before.as_ref().unwrap_or(&empty);
        let changed = changes(prior, &after, before.is_some());
        let added: Vec<_> = card_ids(&after)
            .filter(|id| !card_ids(prior).any(|old| old == *id))
            .collect();
        let layout_path = self
            .root()
            .join("layout")
            .join(format!("{slug}.layout.toml"));
        let layout = stage_layout(&layout_path, &after, &added, &removed)?;
        if !dry_run {
            crate::mission_history::record(self.root(), slug, &raw)?;
            if let Some(layout) = layout {
                crate::write::pair((&path, raw.as_bytes()), (&layout_path, layout.as_bytes()))
                    .map_err(crate::error::io)?;
            } else {
                crate::write::atomic(&path, raw.as_bytes()).map_err(crate::error::io)?;
            }
        }
        Ok(AppliedDocument {
            mission: after,
            assigned_ids,
            changed,
            wrote: !dry_run,
        })
    }
}

fn mission_path(store: &Store, slug: &str) -> PathBuf {
    store
        .root()
        .join("missions")
        .join(format!("{slug}.mission.toml"))
}

fn read_optional(path: &Path) -> Result<Option<String>, Error> {
    match crate::links::read_definition(path) {
        Ok(raw) => Ok(Some(raw)),
        Err(error) if error.code == "not_found" => Ok(None),
        Err(error) => Err(error),
    }
}

fn document(source: &str) -> Result<DocumentMut, Error> {
    source.parse().map_err(|error: toml_edit::TomlError| {
        Error::new("invalid_definition_source", error.to_string())
    })
}

fn card_ids(m: &Mission) -> impl Iterator<Item = &str> {
    m.input_cards
        .iter()
        .map(|card| card.id.as_str())
        .chain(m.steps.iter().map(|card| card.id.as_str()))
        .chain(m.gates.iter().map(|card| card.id.as_str()))
        .chain(m.ends.iter().map(|card| card.id.as_str()))
        .chain(m.limits.iter().map(|card| card.id.as_str()))
}

fn all_ids(m: &Mission) -> impl Iterator<Item = &str> {
    card_ids(m)
        .chain(m.connections.iter().map(|wire| wire.id.as_str()))
        .chain(m.steps.iter().flat_map(|step| {
            step.inputs
                .iter()
                .chain(&step.outputs)
                .map(|port| port.id.as_str())
                .chain(step.seats.iter().map(|seat| seat.id.as_str()))
        }))
        .chain(std::iter::once(m.id.as_str()))
}

/// Fill an id even if the title has no letters usable in one.
fn new_id(kind: &str, title: &str, used: &mut HashSet<String>) -> String {
    let name = title
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("_")
        .to_ascii_lowercase();
    let stem = if name.is_empty() {
        kind.into()
    } else {
        format!("{kind}_{name}")
    };
    if used.insert(stem.clone()) {
        return stem;
    }
    for suffix in 2.. {
        let id = format!("{stem}_{suffix}");
        if used.insert(id.clone()) {
            return id;
        }
    }
    unreachable!("there is a free suffix")
}

fn fill_ids(doc: &mut dyn TableLike, used: &mut HashSet<String>, assigned: &mut Vec<String>) {
    fn each(doc: &mut dyn TableLike, key: &str, mut f: impl FnMut(&mut dyn TableLike)) {
        let Some(item) = doc.get_mut(key) else { return };
        if let Some(tables) = item.as_array_of_tables_mut() {
            for table in tables.iter_mut() {
                f(table);
            }
        } else if let Some(items) = item.as_array_mut() {
            for table in items
                .iter_mut()
                .filter_map(toml_edit::Value::as_inline_table_mut)
            {
                f(table);
            }
        }
    }
    let mut assign = |card: &mut dyn TableLike, kind, name| {
        if card
            .get("id")
            .and_then(toml_edit::Item::as_str)
            .is_none_or(str::is_empty)
        {
            let title = card
                .get(name)
                .and_then(toml_edit::Item::as_str)
                .unwrap_or_default();
            let id = new_id(kind, title, used);
            crate::write::set(card, "id", value(id.clone()));
            assigned.push(id);
        }
    };
    for (key, kind) in [
        ("input_card", "inp"),
        ("step", "fmn"),
        ("gate", "gate"),
        ("end", "end"),
        ("limit", "lim"),
    ] {
        each(doc, key, |card| {
            assign(card, kind, "title");
            if key == "step" {
                each(card, "input", |port| assign(port, "port", "label"));
                each(card, "output", |port| assign(port, "port", "label"));
                each(card, "seat", |seat| assign(seat, "seat", "label"));
            }
        });
    }
    each(doc, "connection", |wire| assign(wire, "wire", "from"));
}

fn changes(before: &Mission, after: &Mission, existed: bool) -> DocumentChanges {
    let mut diff = DocumentChanges::default();
    fn collect<T: PartialEq>(
        before: &[T],
        after: &[T],
        id: impl Fn(&T) -> &str,
        diff: &mut DocumentChanges,
    ) {
        for item in after {
            let name = id(item);
            match before.iter().find(|old| id(old) == name) {
                Some(old) if old != item => diff.changed.push(name.into()),
                None => diff.added.push(name.into()),
                _ => (),
            }
        }
        diff.removed.extend(
            before
                .iter()
                .filter(|old| !after.iter().any(|item| id(item) == id(old)))
                .map(|item| id(item).to_owned()),
        );
    }
    macro_rules! compare {
        ($key:ident) => {
            collect(&before.$key, &after.$key, |item| &item.id, &mut diff)
        };
    }
    compare!(input_cards);
    compare!(steps);
    compare!(gates);
    compare!(ends);
    compare!(limits);
    compare!(connections);
    if existed && (before.title != after.title || before.prefix != after.prefix) {
        diff.changed.push(after.id.clone());
    }
    diff
}

fn stage_layout(
    path: &Path,
    mission: &Mission,
    added: &[&str],
    removed: &HashSet<String>,
) -> Result<Option<String>, Error> {
    if added.is_empty() && removed.is_empty() {
        return Ok(None);
    }
    let (mut doc, mut layout) = if let Some(raw) = read_optional(path)? {
        let layout = Layout::parse(&raw)?;
        if layout.mission_id != mission.id {
            return Err(Error::new("conflict", "layout belongs to another mission"));
        }
        (document(&raw)?, layout)
    } else if added.is_empty() {
        return Ok(None);
    } else {
        let mut doc = DocumentMut::new();
        doc["schema"] = value(ahjo_mission_model::LAYOUT_SCHEMA);
        doc["mission_id"] = value(mission.id.clone());
        let layout = Layout {
            schema: ahjo_mission_model::LAYOUT_SCHEMA,
            mission_id: mission.id.clone(),
            ..Default::default()
        };
        (doc, layout)
    };
    layout.cards.retain(|card| !removed.contains(&card.id));
    crate::write::table_arrays(&mut doc, &["card"]);
    crate::authoring_delete::remove(&mut doc, "card", removed);
    let mut placed: HashSet<_> = layout.cards.iter().map(|card| card.id.clone()).collect();
    for &id in added {
        if placed.insert(id.to_owned()) {
            let mut card = crate::free_layout_position(mission, Some(&layout), 112, 112)?;
            card.id = id.to_owned();
            crate::authoring_create::append(&mut doc, "card", &card)?;
            layout.cards.push(card);
        }
    }
    crate::write::set(doc.as_table_mut(), "mission_rev", value(mission.rev));
    let raw = doc.to_string();
    Layout::parse(&raw)?;
    Ok(Some(raw))
}
