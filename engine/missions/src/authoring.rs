//! Revision-checked edits of mission definitions.
use crate::{Error, Store};
use ahjo_mission_model::{
    Edit, Layout, LayoutCard, Mission, MissionChange, MissionDeletion, Precondition,
};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use toml_edit::{DocumentMut, value};

pub(crate) fn etag(raw: &str) -> String {
    Sha256::digest(raw.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
pub(crate) fn now() -> Result<String, Error> {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|e| Error::new("io", e.to_string()))
}
pub(crate) fn id(prefix: &str) -> Result<String, Error> {
    let mut bytes = [0; 8];
    getrandom::fill(&mut bytes).map_err(|e| Error::new("io", e.to_string()))?;
    Ok(format!(
        "{prefix}_{}",
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
    ))
}
fn path(store: &Store, slug: &str) -> PathBuf {
    store
        .root()
        .join("missions")
        .join(format!("{slug}.mission.toml"))
}
fn parse_document(raw: &str) -> Result<DocumentMut, Error> {
    raw.parse()
        .map_err(|e: toml_edit::TomlError| Error::new("invalid_definition", e.to_string()))
}

fn editable_arrays(patch: &MissionChange) -> &'static [&'static str] {
    match patch {
        MissionChange::Title(_) | MissionChange::Prefix(_) => &[],
        MissionChange::UpdateInputCard(_) | MissionChange::CreateInputCard { .. } => {
            &["input_card"]
        }
        MissionChange::CreateStep { .. }
        | MissionChange::SetBrief { .. }
        | MissionChange::ClearBrief { .. }
        | MissionChange::UpdateStep { .. }
        | MissionChange::EditStep { .. }
        | MissionChange::StaffSeat { .. }
        | MissionChange::SwapSeats { .. }
        | MissionChange::MakeController { .. }
        | MissionChange::SetStepFormation { .. } => &["step"],
        MissionChange::CreateGate { .. } => &["gate"],
        MissionChange::CreateEnd { .. } | MissionChange::UpdateEnd(_) => &["end"],
        MissionChange::CreateLimit { .. } | MissionChange::UpdateLimit(_) => &["limit"],
        MissionChange::SetGateJudge { .. }
        | MissionChange::DetachGateJudge { .. }
        | MissionChange::UpdateGate(_) => &["gate", "connection"],
        MissionChange::AddPort { .. }
        | MissionChange::UpdatePort { .. }
        | MissionChange::RemovePort { .. }
        | MissionChange::RestorePort { .. }
        | MissionChange::WireConnection { .. }
        | MissionChange::RewireConnection { .. }
        | MissionChange::UnwireConnection { .. } => &["step", "connection"],
        MissionChange::DeleteInputCard { .. } => &["input_card", "connection"],
        MissionChange::DeleteStep { .. } => &["step", "limit", "connection"],
        MissionChange::DeleteGate { .. } => &["gate", "connection"],
        MissionChange::DeleteEnd { .. } => &["end", "connection"],
        MissionChange::DeleteLimit { .. } => &["limit", "connection"],
        MissionChange::RestoreCard { .. } => {
            &["input_card", "step", "gate", "end", "limit", "connection"]
        }
    }
}
pub(crate) fn edit(raw: &str) -> Result<Edit, Error> {
    Ok(Edit {
        mission: Mission::parse(raw)?,
        etag: etag(raw),
    })
}
fn check(precondition: &Precondition, current: &Edit) -> Result<(), Error> {
    if precondition.rev == 0 || precondition.etag.is_empty() {
        return Err(Error::new(
            "precondition_required",
            "revision and ETag are required",
        ));
    }
    if precondition.rev != current.mission.rev || precondition.etag != current.etag {
        return Err(Error::new("conflict", "mission changed"));
    }
    Ok(())
}

impl Store {
    pub fn mission_edit(&self, selector: &str) -> Result<Edit, Error> {
        let slug = self.resolve(selector)?;
        let raw = crate::links::read_definition(&path(self, &slug))?;
        edit(&raw)
    }

    pub fn create_mission(&self, slug: &str, title: &str, actor: &str) -> Result<Edit, Error> {
        self.create_from(DocumentMut::new(), slug, title, actor)
    }

    /// A new mission that starts as a copy of one of the
    /// [`crate::EXAMPLES`], under its own id, slug and title.
    pub fn create_example(
        &self,
        slug: &str,
        title: &str,
        example: &str,
        actor: &str,
    ) -> Result<Edit, Error> {
        let example = crate::examples::find(example)?;
        let doc = parse_document(example.definition)?;
        let title = if title.trim().is_empty() {
            example.title
        } else {
            title
        };
        self.create_from(doc, slug, title, actor)
    }

    fn create_from(
        &self,
        mut doc: DocumentMut,
        slug: &str,
        title: &str,
        actor: &str,
    ) -> Result<Edit, Error> {
        crate::store::check_slug(slug)?;
        let locks = crate::write::locks(self.root(), &format!("mission:{slug}"));
        let _reference_guard = locks.references.read().unwrap_or_else(|e| e.into_inner());
        let _document_guard = locks.document.lock().unwrap_or_else(|e| e.into_inner());
        let target = path(self, slug);
        match std::fs::symlink_metadata(&target) {
            Ok(_) => {
                return Err(Error::new("already_exists", "mission already exists"));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(crate::error::io(error)),
        }
        doc["schema"] = value(ahjo_mission_model::MISSION_SCHEMA);
        doc["id"] = value(id("msn")?);
        doc["slug"] = value(slug);
        doc["title"] = value(if title.trim().is_empty() {
            slug
        } else {
            title.trim()
        });
        crate::write::set(doc.as_table_mut(), "rev", toml_edit::value(1));
        if !actor.trim().is_empty() {
            crate::write::set(
                doc.as_table_mut(),
                "updated_by",
                toml_edit::value(actor.trim()),
            );
        }
        crate::write::set(doc.as_table_mut(), "updated_at", toml_edit::value(now()?));
        let raw = doc.to_string();
        let result = edit(&raw)?;
        crate::mission_history::record(self.root(), slug, &raw)?;
        crate::write::atomic(&target, raw.as_bytes()).map_err(crate::error::io)?;
        Ok(result)
    }

    pub fn patch(
        &self,
        selector: &str,
        precondition: Precondition,
        patch: MissionChange,
        actor: &str,
    ) -> Result<Edit, Error> {
        self.patch_changes(selector, precondition, [patch], false, actor)
    }

    /// Apply ordered changes under one precondition and publish one revision.
    pub fn patch_batch(
        &self,
        selector: &str,
        precondition: Precondition,
        changes: Vec<MissionChange>,
        actor: &str,
    ) -> Result<Edit, Error> {
        if changes.is_empty() {
            return Err(Error::new(
                "bad_request",
                "changes must contain at least one operation",
            ));
        }
        self.patch_changes(selector, precondition, changes, true, actor)
    }

    fn patch_changes(
        &self,
        selector: &str,
        precondition: Precondition,
        changes: impl IntoIterator<Item = MissionChange>,
        batch: bool,
        actor: &str,
    ) -> Result<Edit, Error> {
        let scope = crate::write::locks(self.root(), "scope");
        let _reference_guard = scope.references.read().unwrap_or_else(|e| e.into_inner());
        let slug = self.resolve(selector)?;
        let locks = crate::write::locks(self.root(), &format!("mission:{slug}"));
        let _document_guard = locks.document.lock().unwrap_or_else(|e| e.into_inner());
        if self.resolve(selector)? != slug {
            return Err(Error::new("conflict", "mission selector changed"));
        }
        let target = path(self, &slug);
        let raw = crate::links::read_definition(&target)?;
        let current = edit(&raw)?;
        check(&precondition, &current)?;
        let mut doc = parse_document(&raw)?;
        let mut mission = current.mission.clone();
        let layout_path = self
            .root()
            .join("layout")
            .join(format!("{slug}.layout.toml"));
        let mut layout = None;
        let mut changed = false;
        for (index, patch) in changes.into_iter().enumerate() {
            let step = || {
                let mut next = doc.clone();
                let effect = apply_change(self, &mut next, &mission, patch)?;
                if matches!(effect, ChangeEffect::Unchanged) {
                    return Ok::<_, Error>(());
                }
                let updated = Mission::parse(&next.to_string())?;
                if !matches!(effect, ChangeEffect::Remove(_)) {
                    check_supported(&mission, &updated)?;
                }
                stage_layout(&mut layout, &layout_path, &updated, effect)?;
                changed = true;
                doc = next;
                mission = updated;
                Ok(())
            };
            step().map_err(|error| {
                if batch {
                    Error::new(
                        error.code.as_str(),
                        format!("change at index {index}: {}", error.message),
                    )
                } else {
                    error
                }
            })?;
        }
        if !changed && !batch {
            return Ok(current);
        }
        crate::write::set(
            doc.as_table_mut(),
            "rev",
            toml_edit::value(current.mission.rev + 1),
        );
        // A change by someone unnamed is not credited to whoever changed it last.
        if actor.is_empty() {
            doc.as_table_mut().remove("updated_by");
        } else {
            crate::write::set(doc.as_table_mut(), "updated_by", toml_edit::value(actor));
        }
        crate::write::set(doc.as_table_mut(), "updated_at", toml_edit::value(now()?));
        let raw = doc.to_string();
        let updated = edit(&raw)?;
        crate::mission_history::record(self.root(), &updated.mission.slug, &raw)?;
        if let Some(mut layout) = layout {
            crate::write::set(
                layout.as_table_mut(),
                "mission_rev",
                value(updated.mission.rev),
            );
            crate::write::set(
                layout.as_table_mut(),
                "updated_at",
                value(updated.mission.updated_at.clone()),
            );
            let layout_raw = layout.to_string();
            Layout::parse(&layout_raw)?;
            crate::write::pair(
                (&target, raw.as_bytes()),
                (&layout_path, layout_raw.as_bytes()),
            )
            .map_err(crate::error::io)?;
        } else {
            crate::write::atomic(&target, raw.as_bytes()).map_err(crate::error::io)?;
        }
        Ok(updated)
    }
}

enum ChangeEffect {
    Unchanged,
    Changed,
    Place(LayoutCard),
    Remove(HashSet<String>),
}

fn apply_change(
    store: &Store,
    doc: &mut DocumentMut,
    mission: &Mission,
    patch: MissionChange,
) -> Result<ChangeEffect, Error> {
    crate::write::table_arrays(doc, editable_arrays(&patch));
    let effect = match patch {
        MissionChange::Title(title) => {
            if mission.title == title {
                return Ok(ChangeEffect::Unchanged);
            }
            let decor = doc["title"].as_value().map(|v| v.decor().clone());
            let mut updated = value(title);
            if let (Some(decor), Some(v)) = (decor, updated.as_value_mut()) {
                *v.decor_mut() = decor;
            }
            doc["title"] = updated;
            ChangeEffect::Changed
        }
        MissionChange::Prefix(prefix) => {
            let prefix = prefix.trim().to_ascii_lowercase();
            if !prefix.is_empty() && !ahjo_mission_model::valid_prefix(&prefix) {
                return Err(Error::new(
                    "invalid_prefix",
                    "a prefix is letters and digits only, at most 24 of them",
                ));
            }
            if mission.prefix == prefix {
                return Ok(ChangeEffect::Unchanged);
            }
            crate::authoring_staff::set(doc.as_table_mut(), "prefix", &prefix);
            ChangeEffect::Changed
        }
        patch @ (MissionChange::SetGateJudge { .. } | MissionChange::DetachGateJudge { .. }) => {
            if !crate::authoring_judge::apply(doc, mission, patch)? {
                return Ok(ChangeEffect::Unchanged);
            }
            ChangeEffect::Changed
        }
        patch @ MissionChange::CreateLimit { .. } => ChangeEffect::Place(
            crate::authoring_controls::create_limit(doc, mission, patch)?,
        ),
        patch @ (MissionChange::UpdateLimit(_)
        | MissionChange::UpdateEnd(_)
        | MissionChange::UpdateGate(_)) => {
            if !crate::authoring_controls::update(doc, mission, patch)? {
                return Ok(ChangeEffect::Unchanged);
            }
            ChangeEffect::Changed
        }
        MissionChange::RestoreCard {
            card,
            connections,
            index,
            x,
            y,
        } => ChangeEffect::Place(crate::authoring_restore::apply(
            doc,
            mission,
            card,
            connections,
            index,
            x,
            y,
        )?),
        patch @ (MissionChange::AddPort { .. }
        | MissionChange::UpdatePort { .. }
        | MissionChange::RemovePort { .. }
        | MissionChange::RestorePort { .. }) => {
            if !crate::authoring_port::apply(doc, mission, patch)? {
                return Ok(ChangeEffect::Unchanged);
            }
            ChangeEffect::Changed
        }
        MissionChange::UpdateInputCard(update) => {
            if !crate::authoring_input::apply(doc, mission, update)? {
                return Ok(ChangeEffect::Unchanged);
            }
            ChangeEffect::Changed
        }
        patch @ (MissionChange::UpdateStep { .. }
        | MissionChange::EditStep { .. }
        | MissionChange::StaffSeat { .. }
        | MissionChange::SwapSeats { .. }
        | MissionChange::MakeController { .. }
        | MissionChange::SetBrief { .. }
        | MissionChange::ClearBrief { .. }) => {
            if !crate::authoring_staff::apply(doc, mission, &store.agents(), patch)? {
                return Ok(ChangeEffect::Unchanged);
            }
            ChangeEffect::Changed
        }
        patch @ MissionChange::SetStepFormation { .. } => {
            if !crate::authoring_type::apply(doc, mission, patch)? {
                return Ok(ChangeEffect::Unchanged);
            }
            ChangeEffect::Changed
        }
        patch @ (MissionChange::WireConnection { .. }
        | MissionChange::UnwireConnection { .. }
        | MissionChange::RewireConnection { .. }) => {
            crate::authoring_wire::apply(doc, mission, patch)?;
            ChangeEffect::Changed
        }
        patch @ (MissionChange::DeleteInputCard { .. }
        | MissionChange::DeleteStep { .. }
        | MissionChange::DeleteGate { .. }
        | MissionChange::DeleteEnd { .. }
        | MissionChange::DeleteLimit { .. }) => {
            ChangeEffect::Remove(crate::authoring_delete::apply(doc, mission, patch)?)
        }
        other => ChangeEffect::Place(crate::authoring_create::apply(doc, mission, other)?),
    };
    Ok(effect)
}

fn check_supported(before: &Mission, after: &Mission) -> Result<(), Error> {
    // A legacy definition may be repaired one unsupported card at a time.
    // Other edits cannot save a mission the runtime cannot execute.
    let old = crate::validate::unsupported(before);
    let unsupported = crate::validate::unsupported(after);
    let repairing = unsupported.len() < old.len()
        && unsupported.iter().all(|finding| {
            old.iter().any(|existing| {
                existing.code == finding.code && existing.card_id == finding.card_id
            })
        });
    if !repairing && let Some(finding) = unsupported.first() {
        return Err(Error::new(finding.code.as_str(), &finding.message));
    }
    Ok(())
}

fn stage_layout(
    layout: &mut Option<DocumentMut>,
    path: &Path,
    mission: &Mission,
    effect: ChangeEffect,
) -> Result<(), Error> {
    if matches!(effect, ChangeEffect::Changed | ChangeEffect::Unchanged) {
        return Ok(());
    }
    if layout.is_none() {
        *layout = match crate::links::read_definition(path) {
            Ok(raw) => {
                if Layout::parse(&raw)?.mission_id != mission.id {
                    return Err(Error::new("conflict", "layout belongs to another mission"));
                }
                Some(parse_document(&raw)?)
            }
            Err(error) if error.code == "not_found" => None,
            Err(error) => return Err(error),
        };
    }
    match effect {
        ChangeEffect::Place(card) => {
            let layout = layout.get_or_insert_with(|| {
                let mut doc = DocumentMut::new();
                doc["schema"] = value(ahjo_mission_model::LAYOUT_SCHEMA);
                doc["mission_id"] = value(mission.id.clone());
                doc
            });
            crate::authoring_create::append(layout, "card", &card)?;
        }
        ChangeEffect::Remove(ids) => {
            if let Some(layout) = layout {
                crate::write::table_arrays(layout, &["card"]);
                crate::authoring_delete::remove(layout, "card", &ids);
            }
        }
        _ => unreachable!(),
    }
    Ok(())
}

impl Store {
    pub fn delete_mission(
        &self,
        selector: &str,
        precondition: Precondition,
    ) -> Result<MissionDeletion, Error> {
        let scope = crate::write::locks(self.root(), "scope");
        let _reference_guard = scope.references.read().unwrap_or_else(|e| e.into_inner());
        let slug = self.resolve(selector)?;
        let locks = crate::write::locks(self.root(), &format!("mission:{slug}"));
        let _document_guard = locks.document.lock().unwrap_or_else(|e| e.into_inner());
        if self.resolve(selector)? != slug {
            return Err(Error::new("conflict", "mission selector changed"));
        }
        let target = path(self, &slug);
        let raw = crate::links::read_definition(&target)?;
        let current = edit(&raw)?;
        check(&precondition, &current)?;
        let archive_id = id("archive")?;
        let notes = self.root().join("notes").join(format!("{slug}.notes.toml"));
        let archived_notes =
            notes.with_file_name(format!("{slug}.notes.toml.deleted-{archive_id}"));
        // Only archive notes that identify this mission. Refuse an unreadable
        // notes file: its ownership cannot safely be inferred from the slug.
        let archive_notes = match crate::links::read_definition(&notes) {
            Ok(raw) => ahjo_mission_model::Notes::parse(&raw)?.mission_id == current.mission.id,
            Err(error) if error.code == "not_found" => false,
            Err(error) => return Err(error),
        };
        let layout = self
            .root()
            .join("layout")
            .join(format!("{slug}.layout.toml"));
        let archived_layout =
            layout.with_file_name(format!("{slug}.layout.toml.deleted-{archive_id}"));
        let archived_mission =
            target.with_file_name(format!("{slug}.mission.toml.deleted-{archive_id}"));
        let mut moves = Vec::new();
        if archive_notes {
            moves.push((notes, archived_notes));
        }
        match std::fs::symlink_metadata(&layout) {
            Ok(_) => moves.push((layout, archived_layout)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(crate::error::io(e)),
        }
        // Its history goes with it, so a new mission of the same name starts
        // its own.
        for kept in crate::mission_history::files(self.root(), &slug)? {
            let name = kept.file_name().unwrap_or_default().to_string_lossy();
            let archived = kept.with_file_name(format!("{name}.deleted-{archive_id}"));
            moves.push((kept, archived));
        }
        moves.push((target, archived_mission));
        crate::mission_archives::move_archives(moves)?;
        Ok(MissionDeletion {
            id: current.mission.id,
            slug,
            title: current.mission.title,
            archive_id,
        })
    }
}
