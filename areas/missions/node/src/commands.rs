//! Typed mission authoring commands.

use ahjo_core::{Error, TypedCommand};
pub(crate) use ahjo_mission_engine::shell_word;
use ahjo_mission_engine::{AdmissionFailure, Harnesses, validate};
use ahjo_missions_contract::{
    self as wire, LayoutPatch, NotePatch, Precondition, RunStart,
    commands::*,
    document::NotesDocument,
    document_commands::{AppliedMission, ApplyMission, ExportMission, ExportedMission},
    patch,
};
use ahjo_node_kit::{Command, Effect};
use schemars::JsonSchema;
use serde_json::{Value, json};

use crate::{Missions, run_commands, sketch};

pub const DOCUMENT_FILE_ARGUMENTS: &[(&str, &str)] = &[(ApplyMission::NAME, "document")];

pub const NAMES: &[&str] = &{
    let mut names = [""; wire::commands::NAMES.len() + run_commands::NAMES.len()];
    let mut i = 0;
    while i < wire::commands::NAMES.len() {
        names[i] = wire::commands::NAMES[i];
        i += 1;
    }
    let mut j = 0;
    while j < run_commands::NAMES.len() {
        names[i + j] = run_commands::NAMES[j];
        j += 1;
    }
    names
};

pub(super) fn command<C>(
    m: &Missions,
    effect: Effect,
    handler: fn(&Missions, C) -> Result<C::Output, Error>,
) -> Command
where
    C: TypedCommand + JsonSchema + Send,
    C::Output: JsonSchema,
{
    let m = m.clone();
    Command::for_request(effect, move |input| {
        let m = m.clone();
        async move { blocking(move || handler(&m, input)).await }
    })
}

pub fn commands(m: &Missions) -> Vec<Command> {
    use Effect::*;
    let start = m.clone();
    let mut commands = vec![
        command(m, Reads, |m, _: MissionList| {
            Ok(MissionsListed {
                node: m.node.clone(),
                missions: m.store.list_missions()?,
            })
        }),
        command(m, Reads, |m, _: MissionRepositories| {
            Ok(MissionRepositoriesListed {
                node: m.node.clone(),
                repositories: m.coordinator.repositories(),
            })
        }),
        command(m, Reads, |m, _: MissionArchives| {
            Ok(MissionArchivesListed {
                node: m.node.clone(),
                archives: m.store.mission_archives()?,
            })
        }),
        command(m, Destroys, |m, args: MissionPurge| {
            Ok(MissionPurged {
                node: m.node.clone(),
                removed: m.store.remove_mission_archive(
                    required(&args.slug, "slug")?,
                    required(&args.archive_id, "archive_id")?,
                )?,
            })
        }),
        command(m, Reads, show),
        command(m, Reads, check_mission),
        command(m, Writes, create),
        command(m, Reads, |m, _: MissionExamples| {
            Ok(MissionExamplesListed {
                node: m.node.clone(),
                examples: wire::EXAMPLES
                    .iter()
                    .map(|example| MissionExample {
                        name: example.name.into(),
                        title: example.title.into(),
                        about: example.about.into(),
                    })
                    .collect(),
            })
        }),
        command(m, Reads, export_document),
        command(m, Destroys, apply_document),
        command(m, Destroys, edit),
        command(m, Reads, |m, args: MissionNotes| {
            let current = m.store.notes_edit(required(&args.mission, "mission")?)?;
            Ok(MissionNotesListed {
                node: m.node.clone(),
                notes: NotesDocument::from_notes(current.notes, current.etag),
            })
        }),
        command(m, Reads, |m, args: MissionHistory| {
            Ok(MissionHistoryListed {
                node: m.node.clone(),
                revisions: m
                    .store
                    .mission_history(required(&args.mission, "mission")?)?,
            })
        }),
        command(m, Destroys, note),
        command(m, Writes, |m, args: MissionReply| {
            reply(
                m,
                args.mission,
                args.note,
                args.text,
                args.author,
                args.etag,
                false,
            )
        }),
        command(m, Sets, |m, args: MissionResolve| {
            reply(
                m,
                args.mission,
                args.note,
                args.text,
                args.author,
                args.etag,
                true,
            )
        }),
        command(m, Reads, |m, args: LayoutShow| {
            let layout = m.store.layout_edit(required(&args.mission, "mission")?)?;
            Ok(LayoutShown {
                node: m.node.clone(),
                layout: layout.layout,
                etag: layout.etag,
            })
        }),
        command(m, Sets, edit_layout),
        command(m, Sets, |m, args: MissionArrange| {
            arrange(m, required(&args.mission, "mission")?, None)
        }),
        command(m, Destroys, delete),
        Command::for_request(Writes, move |args: MissionRun| {
            start_run(start.clone(), args)
        }),
    ];
    commands.extend(run_commands::commands(m));
    commands
}

fn create(m: &Missions, args: MissionCreate) -> Result<MissionCreated, Error> {
    let given_slug = args.slug.trim();
    let requested_title = args.title.as_deref().unwrap_or_default().trim();
    let unnamed = given_slug.is_empty() && requested_title.is_empty();
    let title = if unnamed {
        "Untitled mission"
    } else if requested_title.is_empty() && args.example.is_none() {
        given_slug
    } else {
        requested_title
    };
    let slug = if given_slug.is_empty() {
        slug_from_title(title)
    } else {
        given_slug.to_owned()
    };
    if slug.is_empty() {
        return Err(bad("Mission name must contain a letter or number"));
    }
    let steps = sketch::steps(args.steps.as_deref().unwrap_or_default());
    if !steps.is_empty() && args.example.is_some() {
        return Err(bad("--steps sketches a new mission; leave out --example"));
    }
    let actor = args
        .updated_by
        .as_deref()
        .filter(|a| !a.is_empty())
        .unwrap_or("agent:ahjo");
    let mut created = None;
    for suffix in 1..=100 {
        let name = if suffix == 1 {
            slug.clone()
        } else {
            format!("{slug}-{suffix}")
        };
        let display = if suffix == 1 {
            title.to_owned()
        } else {
            format!("{title} {suffix}")
        };
        let result = match &args.example {
            Some(example) => m.store.create_example(&name, &display, example, actor),
            None => m.store.create_mission(&name, &display, actor),
        };
        match result {
            Ok(edit) => {
                created = Some(edit);
                break;
            }
            Err(e) if e.code == "already_exists" && unnamed && suffix < 100 => continue,
            Err(e) => return Err(e),
        }
    }
    let created = created.ok_or_else(|| bad("no untitled mission name available"))?;
    sketch::fill(
        m,
        &created.mission.id,
        &created.mission.slug,
        &steps,
        args.note.as_deref().unwrap_or_default(),
        actor,
    )?;
    let saved = m.store.mission_edit(&created.mission.slug)?;
    let mission = saved.mission;
    m.changed();
    let slug = shell_word(&mission.slug);
    Ok(MissionCreated {
        node: m.node.clone(),
        created: mission.slug,
        id: mission.id,
        title: mission.title,
        rev: mission.rev,
        steps: mission
            .steps
            .into_iter()
            .map(|step| SketchStep {
                id: step.id,
                title: step.title,
            })
            .collect(),
        next: [
            format!("ahjo mission show {slug}"),
            format!("ahjo mission validate {slug}"),
        ],
    })
}

fn slug_from_title(title: &str) -> String {
    let mut slug = String::new();
    let mut separator = false;
    for c in title.trim().to_lowercase().chars() {
        if c.is_alphanumeric() {
            slug.push(c);
            separator = false;
        } else if !slug.is_empty() && !separator {
            slug.push('-');
            separator = true;
        }
        if slug.len() >= 64 {
            break;
        }
    }
    slug.trim_matches('-').to_owned()
}

fn show(m: &Missions, args: MissionShow) -> Result<MissionShown, Error> {
    let id = required(&args.mission, "mission")?;
    let current = m.store.mission_edit(id)?;
    if args.full {
        return Ok(MissionShown::Full(Box::new(MissionDefinition {
            node: m.node.clone(),
            mission: current.mission,
            etag: current.etag,
        })));
    }
    let notes = m.store.notes_edit(id)?.notes;
    let report = validate(&current.mission, &m.store.agents(), &Harnesses::from_host());
    Ok(MissionShown::Summary(Box::new(crate::handover::handover(
        m.node.clone(),
        &current.mission,
        &m.store.agents(),
        &notes,
        &report.errors,
        &report.warnings,
    ))))
}

fn check_mission(m: &Missions, args: MissionValidate) -> Result<MissionValidated, Error> {
    let edit = m.store.mission_edit(required(&args.mission, "mission")?)?;
    let report = validate(&edit.mission, &m.store.agents(), &Harnesses::from_host());
    // An unwired Input is allowed in a draft, but makes admission fail.
    let unwired = report
        .warnings
        .iter()
        .find(|f| f.code == wire::Code::MissionNotRunnable);
    let slug = shell_word(&edit.mission.slug);
    let valid = report.errors.is_empty() && unwired.is_none();
    let next = if valid {
        run_command(&slug, &edit.mission)
    } else {
        format!("ahjo mission export {slug}")
    };
    let summary = match unwired {
        Some(finding) if report.errors.is_empty() => format!("not runnable: {}", finding.message),
        _ => validation_summary(report.errors.len(), report.warnings.len()),
    };
    Ok(MissionValidated {
        node: m.node.clone(),
        valid,
        summary,
        mission_rev: edit.mission.rev,
        mission_etag: edit.etag,
        errors: report.errors,
        warnings: report.warnings,
        next,
    })
}

fn edit(m: &Missions, args: MissionEdit) -> Result<MissionEdited, Error> {
    let id = required(&args.mission, "mission")?;
    let current = m.store.mission_edit(id)?;
    let before = current.mission;
    let before_etag = current.etag.clone();
    let rev_mismatch = |rev: Option<i64>| rev.is_some_and(|rev| rev != before.rev);
    if rev_mismatch(args.rev) || rev_mismatch(args.change.rev) {
        return Err(Error::new("conflict", "mission changed"));
    }
    let precondition = Precondition {
        rev: before.rev,
        etag: args.etag.unwrap_or(current.etag),
    };
    let actor = if args.change.updated_by.is_empty() {
        "agent:ahjo"
    } else {
        &args.change.updated_by
    };
    let (edited, added) = match args.change.changes {
        patch::Changes::Single(change) => {
            let created = crate::created_card::CreatedCard::from_change(&change);
            let edited = m.store.patch(id, precondition, *change, actor)?;
            let added = created.and_then(|kind| kind.added(&before, &edited.mission));
            (edited, added)
        }
        patch::Changes::Batch(changes) => {
            (m.store.patch_batch(id, precondition, changes, actor)?, None)
        }
    };
    if edited.etag != before_etag {
        m.changed();
    }
    let layout = m.store.layout_edit(id)?;
    Ok(MissionEdited {
        layout: layout.layout,
        layout_etag: layout.etag,
        definition: MissionDefinition {
            node: m.node.clone(),
            mission: edited.mission,
            etag: edited.etag,
        },
        added,
    })
}

fn note(m: &Missions, args: MissionNote) -> Result<MissionNoteReceipt, Error> {
    let id = required(&args.mission, "mission")?;
    let current = m.store.notes_edit(id)?;
    let author = note_author(args.author.as_deref());
    let text = args.text.unwrap_or_default();
    let entry = |entry: &str| {
        current
            .notes
            .entries
            .iter()
            .find(|e| e.id == entry)
            .map(|e| e.target.clone())
            .ok_or_else(|| {
                Error::new(
                    "not_found",
                    format!("no note {entry:?}; mission show lists them"),
                )
            })
    };
    let patch = match (args.edit, args.delete) {
        (Some(_), Some(_)) => return Err(bad("--edit or --delete, not both")),
        (Some(entry_id), None) => NotePatch::Edit {
            target: entry(&entry_id)?,
            author,
            id: entry_id,
            text,
        },
        (None, Some(entry_id)) => NotePatch::Delete {
            target: entry(&entry_id)?,
            author,
            id: entry_id,
        },
        (None, None) => {
            let mission = m.store.read_mission(id)?;
            NotePatch::Append {
                target: note_target(&mission, args.on.as_deref().unwrap_or_default())?,
                author,
                text,
            }
        }
    };
    let edited = m
        .store
        .patch_notes(id, args.etag.as_deref().unwrap_or(&current.etag), patch)?;
    m.changed();
    Ok(note_receipt(&m.node, edited, &current.notes))
}

fn reply(
    m: &Missions,
    mission: String,
    note: String,
    text: String,
    author: Option<String>,
    etag: Option<String>,
    resolves: bool,
) -> Result<MissionNoteReceipt, Error> {
    let id = required(&mission, "mission")?;
    let current = m.store.notes_edit(id)?;
    required(&note, "note")?;
    required(&text, "text")?;
    let patch = NotePatch::Reply {
        to: note,
        author: note_author(author.as_deref()),
        text,
        resolves,
    };
    let edited = m
        .store
        .patch_notes(id, etag.as_deref().unwrap_or(&current.etag), patch)?;
    m.changed();
    Ok(note_receipt(&m.node, edited, &current.notes))
}

fn note_author(author: Option<&str>) -> String {
    match author.unwrap_or_default().trim() {
        "" => "agent:ahjo".into(),
        name if name.starts_with("agent:") || name.starts_with("human:") => name.into(),
        name => format!("agent:{name}"),
    }
}

fn note_receipt(node: &str, edited: wire::NotesEdit, before: &wire::Notes) -> MissionNoteReceipt {
    let written = edited.notes.entries.iter().find(|entry| {
        !before
            .entries
            .iter()
            .any(|old| old.id == entry.id && old.text == entry.text)
    });
    let change = match written {
        Some(entry) => NoteChange::Written {
            note: entry.id.clone(),
            on: entry.target.clone(),
            resolved: edited.notes.resolved(&entry.target),
            thread: edited
                .notes
                .thread(&entry.target)
                .map(|note| NoteRow {
                    id: note.id.clone(),
                    by: note.author.clone(),
                    text: note.text.clone(),
                    resolves: note.resolves,
                })
                .collect(),
        },
        None => NoteChange::Changed {
            notes: "changed".into(),
            rev: edited.notes.rev,
        },
    };
    MissionNoteReceipt {
        node: node.into(),
        document: NotesDocument::from_notes(edited.notes, edited.etag),
        change,
    }
}

fn edit_layout(m: &Missions, args: LayoutEdit) -> Result<LayoutShown, Error> {
    let id = required(&args.mission, "mission")?;
    if args.change.arrange == Some(true) {
        return arrange(m, id, args.etag.as_deref());
    }
    let patch = match (args.change.cards, args.change.edges) {
        (Some(nodes), _) => LayoutPatch::Cards(nodes),
        (None, Some(edges)) => LayoutPatch::Edges(edges),
        (None, None) => return Err(bad("layout needs cards, edges or arrange")),
    };
    let current = m.store.layout_edit(id)?;
    patch_layout(m, id, args.etag.as_deref().unwrap_or(&current.etag), patch)
}

fn arrange(m: &Missions, id: &str, etag: Option<&str>) -> Result<LayoutShown, Error> {
    let current = m.store.layout_edit(id)?;
    let mission = m.store.read_mission(id)?;
    let nodes = ahjo_mission_engine::arrange(&mission, &current.layout).cards;
    patch_layout(
        m,
        id,
        etag.unwrap_or(&current.etag),
        LayoutPatch::Cards(nodes),
    )
}

fn patch_layout(
    m: &Missions,
    id: &str,
    etag: &str,
    patch: LayoutPatch,
) -> Result<LayoutShown, Error> {
    let edited = m.store.patch_layout(id, etag, patch)?;
    m.changed();
    Ok(LayoutShown {
        node: m.node.clone(),
        layout: edited.layout,
        etag: edited.etag,
    })
}

fn delete(m: &Missions, args: MissionDelete) -> Result<MissionDeleted, Error> {
    let id = required(&args.mission, "mission")?;
    let current = m.store.mission_edit(id)?;
    let deletion = m.store.delete_mission(
        id,
        Precondition {
            rev: args.rev.unwrap_or(current.mission.rev),
            etag: args.etag.unwrap_or(current.etag),
        },
    )?;
    m.changed();
    Ok(MissionDeleted {
        node: m.node.clone(),
        deletion,
    })
}

pub(crate) fn bad(error: impl std::fmt::Display) -> Error {
    Error::new("bad_request", error.to_string())
}

pub(super) fn required<'a>(value: &'a str, field: &str) -> Result<&'a str, Error> {
    if value.is_empty() {
        Err(bad(format!("{field} is required")))
    } else {
        Ok(value)
    }
}

pub(crate) async fn blocking<T: Send + 'static>(
    operation: impl FnOnce() -> Result<T, Error> + Send + 'static,
) -> Result<T, Error> {
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|e| Error::new("internal", e.to_string()))?
}

/// What a note is on: the mission, a card by id or unique title, or a wire
/// by id or as `From->To`.
fn note_target(mission: &wire::Mission, target: &str) -> Result<String, Error> {
    let target = target.trim();
    if target.is_empty() || target == wire::MISSION_THREAD {
        return Ok(wire::MISSION_THREAD.into());
    }
    let cards = (mission.input_cards.iter().map(|n| (&n.id, &n.title)))
        .chain(mission.steps.iter().map(|n| (&n.id, &n.title)))
        .chain(mission.gates.iter().map(|n| (&n.id, &n.title)))
        .chain(mission.ends.iter().map(|n| (&n.id, &n.title)))
        .chain(mission.limits.iter().map(|n| (&n.id, &n.title)))
        .collect::<Vec<_>>();
    let card = |name: &str| -> Result<String, Error> {
        let name = name.trim();
        if let Some((id, _)) = cards.iter().find(|(id, _)| id.as_str() == name) {
            return Ok((*id).clone());
        }
        match cards
            .iter()
            .filter(|(_, title)| title.as_str() == name)
            .collect::<Vec<_>>()
            .as_slice()
        {
            [(id, _)] => Ok((*id).clone()),
            [] => Err(Error::new(
                "not_found",
                format!("nothing called {name:?} in this mission; mission show names everything"),
            )),
            _ => Err(bad(format!("{name:?} names several; use the id"))),
        }
    };
    if mission.connections.iter().any(|c| c.id == target) {
        return Ok(target.into());
    }
    if let Some((from, to)) = target.split_once("->") {
        let (from, from_port) = from.trim().split_once(':').unwrap_or((from, ""));
        let (from, to) = (card(from)?, card(to)?);
        let wires = (mission.connections.iter())
            .filter(|c| {
                let (a, port) = c.from.split_once(':').unwrap_or((&c.from, ""));
                a == from
                    && c.to.split(':').next() == Some(to.as_str())
                    && (from_port.is_empty() || port == from_port.trim())
            })
            .collect::<Vec<_>>();
        return match wires.as_slice() {
            [one] => Ok(one.id.clone()),
            [] => Err(Error::new("not_found", format!("no wire {target}"))),
            _ => Err(bad(format!(
                "several wires {target}; name the gate's verdict, as Gate:pass->Step"
            ))),
        };
    }
    card(target)
}
fn export_document(m: &Missions, input: ExportMission) -> Result<ExportedMission, Error> {
    let document = match (input.mission, input.example) {
        (Some(mission), None) => m.store.export_mission(&mission)?,
        (None, Some(name)) => wire::example(&name)
            .ok_or_else(|| Error::new("not_found", format!("no mission example {name:?}")))?
            .definition
            .into(),
        _ => return Err(bad("give one mission or --example")),
    };
    Ok(ExportedMission { document })
}

fn apply_document(m: &Missions, input: ApplyMission) -> Result<AppliedMission, Error> {
    let applied = m.store.apply_document(&input.document, input.dry_run)?;
    if applied.wrote {
        m.changed();
    }
    let validation = validate(&applied.mission, &m.store.agents(), &Harnesses::from_host());
    let slug = shell_word(&applied.mission.slug);
    let next = if input.dry_run {
        "ahjo mission apply -".to_owned()
    } else if !validation.errors.is_empty()
        || validation
            .warnings
            .iter()
            .any(|finding| finding.code == wire::Code::MissionNotRunnable)
    {
        format!("ahjo mission export {slug}")
    } else {
        run_command(&slug, &applied.mission)
    };
    Ok(AppliedMission {
        mission: applied.mission.slug,
        rev: applied.mission.rev,
        assigned_ids: applied.assigned_ids,
        changed: applied.changed,
        validation,
        next,
    })
}

fn admission(e: AdmissionFailure) -> Error {
    match e {
        AdmissionFailure::NotFound(s) => Error::new("not_found", s),
        AdmissionFailure::Conflict => Error::new("conflict", "mission changed"),
        AdmissionFailure::Invalid(s) => bad(s),
        AdmissionFailure::Io(e) => Error::new("io", e.to_string()),
        AdmissionFailure::Findings(findings) => Error::new(
            "run_admission_failed",
            format!(
                "run admission failed: {}",
                serde_json::to_string(&findings).unwrap_or_default()
            ),
        ),
    }
}
/// The command that starts a run of a valid mission, with each input it
/// requires to be filled in.
fn run_command(slug: &str, mission: &wire::Mission) -> String {
    let card = mission.input_cards.first();
    let required: Vec<String> = match card.map(|c| &c.inputs) {
        Some(inputs) if !inputs.is_empty() => inputs
            .iter()
            .filter(|input| input.required)
            .map(|input| input.name.clone())
            .collect(),
        _ => vec!["brief".into()],
    };
    if required.is_empty() {
        return format!("ahjo mission run {slug}");
    }
    let values: serde_json::Map<String, Value> = required
        .into_iter()
        .map(|name| (name, json!("...")))
        .collect();
    format!(
        "ahjo mission run {slug} --inputs '{}'",
        Value::Object(values)
    )
}

/// One line a person reads first: whether the mission can run as it is.
fn validation_summary(errors: usize, warnings: usize) -> String {
    let plural = |n: usize, word: &str| format!("{n} {word}{}", if n == 1 { "" } else { "s" });
    match (errors, warnings) {
        (0, 0) => "valid".into(),
        (0, w) => format!("valid, with {}", plural(w, "warning")),
        (e, 0) => format!("not valid: {}", plural(e, "error")),
        (e, w) => format!(
            "not valid: {} and {}",
            plural(e, "error"),
            plural(w, "warning")
        ),
    }
}

async fn start_run(m: Missions, args: MissionRun) -> Result<MissionStarted, Error> {
    let selector = required(&args.mission, "mission")?;
    if args.from.is_some() && args.input.is_some() {
        return Err(bad("use either --from or --input"));
    }
    if args.rerun_of.is_some() && args.from.is_none() {
        return Err(bad("--rerun-of starts one step again; name it with --from"));
    }
    let edit = m.store.mission_edit(selector)?;
    let mission = edit.mission;
    let stale = args.expected_rev.is_some_and(|rev| rev != mission.rev)
        || args
            .expected_etag
            .as_ref()
            .is_some_and(|etag| !etag.is_empty() && etag != &edit.etag);
    if stale && args.request_id.as_deref().is_none_or(str::is_empty) {
        return Err(Error::new("conflict", "mission changed"));
    }
    // A wrong name is answered with the names there are; --input is the
    // easiest to mistake for an input value.
    let inputs: Vec<String> = mission
        .input_cards
        .iter()
        .flat_map(|c| c.inputs.iter().map(|i| i.name.clone()))
        .collect();
    let pick = |what: &str,
                items: Vec<(&str, &str)>,
                wanted: Option<&str>|
     -> Result<String, Error> {
        let titles: Vec<&str> = items.iter().map(|(_, title)| *title).collect();
        let found: Vec<_> = items
            .iter()
            .filter(|(id, title)| wanted.is_none_or(|w| w == *id || w == *title))
            .collect();
        match found.as_slice() {
            [(id, _)] => Ok((*id).into()),
            [] => {
                let mut message = format!(
                    "no {what} named {:?}; this mission has: {}",
                    wanted.unwrap_or(""),
                    titles.join(", ")
                );
                if what == "Input card" {
                    let names = if inputs.is_empty() {
                        "brief".to_owned()
                    } else {
                        inputs.join(", ")
                    };
                    message.push_str(&format!(
                        ". --input picks an Input card; give input values with --inputs '{{\"name\":\"value\"}}' (this mission takes: {names})"
                    ));
                }
                Err(Error::new("not_found", message))
            }
            _ => Err(bad(format!("{what} is ambiguous; name its id"))),
        }
    };

    let mut request = RunStart {
        mission: selector.into(),
        expected_rev: args.expected_rev.unwrap_or(mission.rev),
        expected_etag: args.expected_etag.unwrap_or(edit.etag),
        actor: args.actor.unwrap_or_else(|| "agent:ahjo".into()),
        cwd: args.cwd.unwrap_or_default(),
        repository: args.repository.unwrap_or_default(),
        base: args.base.unwrap_or_default(),
        keep_sessions: args.keep_sessions,
        request_id: args.request_id.unwrap_or_default(),
        rerun_of: args.rerun_of.unwrap_or_default(),
        known_machines: Some(vec![]),
        inputs: args.inputs,
        context_paths: args.context_paths,
        ..Default::default()
    };
    if let Some(issue) = args.issue.filter(|issue| !issue.is_empty()) {
        request.issue = m.resolver.resolve(&issue).await?.to_string();
    }
    let from_step = args.from.is_some();
    if from_step {
        request.step_id = if stale && !request.request_id.is_empty() {
            args.from.clone().unwrap_or_default()
        } else {
            pick(
                "step",
                mission
                    .steps
                    .iter()
                    .map(|f| (f.id.as_str(), f.title.as_str()))
                    .collect(),
                args.from.as_deref(),
            )?
        };
    } else {
        request.input_card_id = if stale && !request.request_id.is_empty() && args.input.is_some() {
            args.input.clone().unwrap_or_default()
        } else {
            pick(
                "Input card",
                mission
                    .input_cards
                    .iter()
                    .map(|c| (c.id.as_str(), c.title.as_str()))
                    .collect(),
                args.input.as_deref(),
            )?
        };
    }
    let coordinator = m.coordinator.clone();
    let run_id = blocking(move || {
        if from_step {
            coordinator.start_step(request)
        } else {
            coordinator.start_mission(request)
        }
        .map_err(admission)
    })
    .await?;
    Ok(MissionStarted {
        node: m.node,
        run_id,
    })
}
