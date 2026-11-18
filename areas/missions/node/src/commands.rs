//! Commands use the same store and coordinator as the node's routes.
use crate::Missions;
use ahjo_core::Error;
use ahjo_mission_engine::{AdmissionFailure, Harnesses, validate};
use ahjo_missions_contract::{self as wire, LayoutPatch, NotePatch, Precondition, RunStart};
use ahjo_node_kit::{
    Command,
    args::{flag, maybe_text, object, text},
};
use serde_json::{Value, json};
use std::time::Duration;

pub const NAMES: &[&str] = &[
    "mission_list",
    "mission_archives",
    "mission_purge_archive",
    "mission_inspect",
    "mission_validate",
    "mission_new",
    "mission_examples",
    "mission_edit",
    "mission_notes",
    "mission_note",
    "mission_reply",
    "mission_resolve",
    "layout_show",
    "layout_edit",
    "mission_run",
    "formation_run",
    "run_list",
    "run_status",
    "run_wait",
    "run_evidence",
    "run_attempts",
    "run_gates",
    "run_seats",
    "run_logs",
    "run_abort",
    "run_resume",
    "run_land",
    "run_delete",
    "gate_request",
    "gate_approve",
    "gate_reject",
    "gate_pick",
    "mission_arrange",
    "mission_rename",
    "mission_prefix",
    "mission_move",
    "mission_create",
    "mission_update",
    "mission_delete",
    "mission_input",
    "mission_output",
    "mission_wire",
    "formation_list",
    "formation_inspect",
    "formation_create",
    "formation_rename",
    "formation_place",
    "formation_set_type",
    "formation_set_slots",
    "formation_assign",
    "formation_unassign",
    "formation_agent",
    "formation_fresh",
    "formation_set_brief",
    "formation_add_input",
    "formation_add_output",
    "formation_set_port",
    "formation_wire",
    "formation_unwire",
    "formation_delete",
    "gate_create",
    "gate_update",
    "gate_judge",
    "gate_delete",
    "end_create",
    "end_update",
    "end_delete",
    "limit_create",
    "limit_update",
    "limit_delete",
];

pub fn commands(missions: &Missions) -> Vec<Command> {
    NAMES
        .iter()
        .map(|&name| {
            let m = missions.clone();
            Command::new(
                name,
                description(name),
                move || schema(name),
                move |args| {
                    let m = m.clone();
                    async move { call(m, name, args).await }
                },
            )
        })
        .collect()
}
fn description(name: &str) -> &'static str {
    match name {
        "mission_list" => "List the missions on this node.",
        "mission_archives" => {
            "List archived mission definitions, layouts and notes, including partial archives."
        }
        "mission_purge_archive" => {
            "Permanently remove a listed mission archive's definition, layout and associated notes; unrelated notes and run history remain."
        }
        "mission_inspect" => {
            "Read a mission as an agent picks it up: the operator's notes beside the step, gate, limit or wire each is on, the steps in the order a run meets them with who does each and where they lead, and what still stops it running. --full gives the whole definition with every port."
        }
        "mission_validate" => {
            "Say whether a mission can run, with its errors and warnings and the next command."
        }
        "mission_new" => {
            "Create a mission: empty, or with --example a copy of a staffed, wired example to start from (mission examples lists them). Answers with what was made and its steps; mission inspect reads the whole graph."
        }
        "mission_examples" => "List the example missions a new mission can start from.",
        "mission_edit" => "Apply one mission authoring operation.",
        "mission_notes" => "Read every note thread as it is stored, by what it is on.",
        "mission_note" => {
            "Write a note on the mission, or with --target on a step, gate, limit, end, the Input card or a wire (id, title, or From->To). --edit <note-id> changes your own note and --delete <note-id> removes it. --author names you, e.g. --author claude-7."
        }
        "mission_reply" => "Answer a note or ask about it, in its thread.",
        "mission_resolve" => {
            "Close a note's thread with what you did about it, so the operator sees it taken up; a later note opens it again."
        }
        "layout_show" => "Read a mission's canvas layout.",
        "layout_edit" => "Change or arrange a mission's canvas layout.",
        "mission_run" => "Start a mission at its Input card.",
        "formation_run" => "Start a mission at one formation.",
        "run_list" => "List runs, optionally by mission.",
        "run_status" => "Read a run and its current events.",
        "run_wait" => "Wait for a run to need its driver, finish, or change.",
        "run_evidence" => "Read a run’s problems, briefs and files.",
        "run_attempts" => "Read a run’s step attempts, verdicts and picks.",
        "run_gates" => "List a run’s gates and their state.",
        "gate_request" => "Read a waiting gate’s request and routes.",
        "run_seats" => "Read the seats belonging to a run.",
        "run_logs" => "Read the public events of a run.",
        "run_abort" => "Abort a run.",
        "run_resume" => "Resume a blocked run.",
        "run_land" => "Land a finished repository run.",
        "run_delete" => {
            "Delete a finished run, its kept outputs, the branches a failed step's work was kept on and the folder Ahjo made for it; land a repository run first."
        }
        "gate_approve" => "Approve a pending human gate.",
        "gate_reject" => "Reject a pending human gate.",
        "gate_pick" => "Pick an attempt at a pending human gate.",
        _ if crate::authoring::NAMES.contains(&name) => crate::authoring::description(name),
        _ => unreachable!(),
    }
}
fn schema(name: &str) -> Value {
    let s = json!({"type":"string"});
    let i = json!({"type":"integer"});
    let b = json!({"type":"boolean"});
    let obj = json!({"type":"object"});
    match name {
        "mission_list" | "mission_archives" => object(json!({}), &[]),
        "mission_purge_archive" => {
            object(json!({"slug":s,"archive_id":s}), &["slug", "archive_id"])
        }
        "mission_new" => object(json!({"slug":s,"title":s,"example":s}), &["slug"]),
        "mission_examples" => object(json!({}), &[]),
        "mission_edit" => object(
            json!({"mission":s,"change":obj,"rev":i}),
            &["mission", "change"],
        ),
        "mission_note" => object(
            json!({"mission":s,
                "text":{"type":"string","description":"What the note says; several lines are fine."},
                "target":{"type":"string","description":"What the note is on: an id or title, or a wire as From->To. The mission itself when left out."},
                "edit":{"type":"string","description":"Change this note of yours to --text."},
                "delete":{"type":"string","description":"Remove this note of yours."},
                "author":{"type":"string","description":"Your name, e.g. claude-7 (agent:claude-7)."}}),
            &["mission"],
        ),
        "mission_reply" | "mission_resolve" => object(
            json!({"mission":s,"note":s,"text":s,
                "author":{"type":"string","description":"Your name, e.g. claude-7 (agent:claude-7)."}}),
            &["mission", "note", "text"],
        ),
        "mission_run" | "formation_run" => {
            let properties = json!({"mission":s,"formation":s,
                "input":{"type":"string","description":"Which Input card (id or title), when the mission has several; not an input value."},
                "inputs":{"type":"object","description":"The input values as a JSON object, e.g. '{\"topic\":\"fog\"}'."},
                "cwd":{"type":"string","description":"The folder the steps work in; a new one per run when left out."},
                "work":s,
                "repository":{"type":"string","description":"A repository named under [missions] repositories in the node's config; the run works on its own branch and worktree."},
                "base":{"type":"string","description":"The branch the run's branch starts from."},
                "context_paths":{"type":"array","items":s},"actor":s,"request_id":s});
            if name == "formation_run" {
                object(properties, &["mission", "formation"])
            } else {
                object(properties, &["mission"])
            }
        }
        "layout_edit" => object(
            json!({"mission":s,"change":obj,"etag":s}),
            &["mission", "change"],
        ),
        "mission_inspect" => object(
            json!({"mission":s,"full":{"type":"boolean","description":"The whole definition, with every port and slot id."}}),
            &["mission"],
        ),
        "mission_validate" | "mission_notes" | "layout_show" => {
            object(json!({"mission":s}), &["mission"])
        }
        "run_list" => object(json!({"mission":s}), &[]),
        "run_evidence" | "run_attempts" => object(json!({"run":s,"step":s}), &["run"]),
        "run_wait" => object(
            json!({"run":s,
                "until":{"type":"string","description":"needs-you (the default: a gate waits or the run ends), final, or any-change."},
                "since":{"type":"integer","description":"Count only what happened after this cursor, as an earlier answer gave it."},
                "timeout":{"type":"integer","description":"Seconds to wait at most (default 3600)."}}),
            &["run"],
        ),
        "run_abort" => object(json!({"run":s,"reason":s,"actor":s}), &["run"]),
        "run_resume" => object(
            json!({"run":s,"mode":s,"take_fail":b,"grant":b,"pick_best":b,"reason":s,"actor":s}),
            &["run"],
        ),
        "run_land" => object(json!({"run":s,"force":b}), &["run"]),
        "gate_request" => object(json!({"run":s,"gate":s}), &["run", "gate"]),
        "gate_approve" | "gate_reject" => object(
            json!({"run":s,"gate":s,"requested_seq":i,"response":s,"relayed_by":s,"actor":s}),
            &["run", "gate", "requested_seq"],
        ),
        "gate_pick" => object(
            json!({"run":s,"gate":s,"requested_seq":i,"attempt":i,"reason":s,"relayed_by":s,"actor":s}),
            &["run", "gate", "requested_seq", "attempt"],
        ),
        _ if crate::authoring::NAMES.contains(&name) => crate::authoring::schema(name),
        _ => object(json!({"run":s}), &["run"]),
    }
}
fn bad(e: impl std::fmt::Display) -> Error {
    Error::new("bad_request", e.to_string())
}
fn required<'a>(a: &'a Value, key: &str) -> Result<&'a str, Error> {
    text(a, key).map_err(bad).and_then(|v| {
        if v.is_empty() {
            Err(bad(format!("{key} is required")))
        } else {
            Ok(v)
        }
    })
}
fn optional<'a>(a: &'a Value, key: &str, default: &'a str) -> Result<&'a str, Error> {
    Ok(maybe_text(a, key).map_err(bad)?.unwrap_or(default))
}
/// `--author claude-7` is the agent claude-7; `human:` and `agent:` names
/// are kept as given.
fn note_author(a: &Value) -> Result<String, Error> {
    let author = optional(a, "author", "agent:ahjo")?.trim();
    Ok(match author {
        "" => "agent:ahjo".into(),
        _ if author.starts_with("agent:") || author.starts_with("human:") => author.into(),
        name => format!("agent:{name}"),
    })
}
/// What a note is on: the mission, a card by id or unique title, or a wire
/// by id or as `From->To`.
fn note_target(mission: &wire::Mission, target: &str) -> Result<String, Error> {
    let target = target.trim();
    if target.is_empty() || target == wire::MISSION_THREAD {
        return Ok(wire::MISSION_THREAD.into());
    }
    let cards = (mission.input_cards.iter().map(|n| (&n.id, &n.title)))
        .chain(mission.formations.iter().map(|n| (&n.id, &n.title)))
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
                format!(
                    "nothing called {name:?} in this mission; mission inspect names everything"
                ),
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
/// What a note command changed: the note written, and the thread it is in.
fn note_receipt(node: &str, edited: wire::NotesEdit, before: &wire::Notes) -> Value {
    let written = (edited.notes.entries.iter()).find(|e| {
        !before.entries.iter().any(|b| b.id == e.id)
            || before
                .entries
                .iter()
                .any(|b| b.id == e.id && b.text != e.text)
    });
    match written {
        Some(entry) => {
            let thread = (edited.notes.thread(&entry.target))
                .map(|e| {
                    let mut row = json!({"id":e.id,"by":e.author,"text":e.text});
                    if e.resolves {
                        row["resolves"] = json!(true);
                    }
                    row
                })
                .collect::<Vec<_>>();
            json!({"node":node,"note":entry.id,"on":entry.target,"resolved":edited.notes.resolved(&entry.target),"thread":thread})
        }
        None => json!({"node":node,"notes":"changed","rev":edited.notes.rev}),
    }
}
fn command_actor(a: &Value) -> Result<String, Error> {
    let actor = optional(a, "actor", "agent:ahjo")?.trim();
    Ok(if actor.is_empty() {
        "agent:ahjo"
    } else {
        actor
    }
    .into())
}
fn boolean(a: &Value, key: &str) -> Result<bool, Error> {
    flag(a, key, false).map_err(bad)
}
fn positive(a: &Value, key: &str) -> Result<u64, Error> {
    a.get(key)
        .and_then(Value::as_u64)
        .filter(|n| *n > 0)
        .ok_or_else(|| bad(format!("{key} must be positive")))
}
async fn blocking<T: Send + 'static>(
    operation: impl FnOnce() -> Result<T, Error> + Send + 'static,
) -> Result<T, Error> {
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|e| Error::new("internal", e.to_string()))?
}
fn value<T: serde::Serialize>(v: T) -> Result<Value, Error> {
    serde_json::to_value(v).map_err(|e| Error::new("internal", e.to_string()))
}
pub(super) fn document(mission: wire::Mission, etag: String) -> Result<Value, Error> {
    value(wire::document::MissionDocument {
        mission,
        etag: Some(etag),
    })
}
fn notes(edit: wire::NotesEdit) -> Result<Value, Error> {
    value(wire::document::NotesDocument::from_notes(
        edit.notes, edit.etag,
    ))
}
pub(super) fn layout(edit: wire::LayoutEdit) -> Result<Value, Error> {
    value(wire::document::LayoutDocument {
        layout: edit.layout,
        etag: edit.etag,
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
async fn call(m: Missions, name: &str, a: Value) -> Result<Value, Error> {
    if !a.is_object() {
        return Err(bad("arguments must be an object"));
    }
    let node = &m.node;
    match name {
        "mission_list" => {
            if a.as_object().is_some_and(|v| !v.is_empty()) {
                return Err(bad("no arguments expected"));
            }
            Ok(json!({"node":node,"missions":m.store.list_missions()?}))
        }
        "mission_archives" => {
            if a.as_object().is_some_and(|v| !v.is_empty()) {
                return Err(bad("no arguments expected"));
            }
            Ok(json!({"node":node,"archives":m.store.mission_archives()?}))
        }
        "mission_purge_archive" => {
            let removed = m
                .store
                .remove_mission_archive(required(&a, "slug")?, required(&a, "archive_id")?)?;
            Ok(json!({"node":node,"removed":removed}))
        }
        "mission_new" => {
            let slug = required(&a, "slug")?;
            let edit = match maybe_text(&a, "example").map_err(bad)? {
                Some(example) => m.store.create_example(
                    slug,
                    optional(&a, "title", "")?,
                    example,
                    "agent:ahjo",
                )?,
                None => m
                    .store
                    .create_mission(slug, optional(&a, "title", slug)?, "agent:ahjo")?,
            };
            m.changed();
            // A receipt, not the graph: mission inspect reads that.
            let mission = &edit.mission;
            let steps: Vec<_> = (mission.formations.iter())
                .map(|step| json!({"id": step.id, "title": step.title}))
                .collect();
            let slug = shell_word(&mission.slug);
            Ok(json!({
                "node": node,
                "created": mission.slug,
                "id": mission.id,
                "title": mission.title,
                "rev": mission.rev,
                "steps": steps,
                "next": [
                    format!("ahjo mission inspect {slug}"),
                    format!("ahjo mission validate {slug}"),
                ],
            }))
        }
        "mission_examples" => {
            if a.as_object().is_some_and(|v| !v.is_empty()) {
                return Err(bad("no arguments expected"));
            }
            let examples: Vec<_> = ahjo_missions_contract::EXAMPLES
                .iter()
                .map(|e| json!({"name":e.name,"title":e.title,"about":e.about}))
                .collect();
            Ok(json!({"node":node,"examples":examples}))
        }
        "mission_inspect" => {
            let selector = required(&a, "mission")?;
            let edit = m.store.mission_edit(selector)?;
            if boolean(&a, "full")? {
                return Ok(json!({"node":node,"mission":document(edit.mission,edit.etag)?}));
            }
            let notes = m.store.notes_edit(selector)?.notes;
            let report = validate(&edit.mission, &m.store.roles(), &Harnesses::from_host());
            let mut answer =
                crate::handover::handover(&edit.mission, &notes, &report.errors, &report.warnings);
            answer["node"] = json!(node);
            Ok(answer)
        }
        "mission_validate" => {
            let edit = m.store.mission_edit(required(&a, "mission")?)?;
            let report = validate(&edit.mission, &m.store.roles(), &Harnesses::from_host());
            // Valid means a run can start: an Input card with no wire out is
            // only a warning while drafting, but `mission run` refuses it.
            let unwired = report
                .warnings
                .iter()
                .find(|f| f.code == wire::Code::MissionNotRunnable);
            let valid = report.errors.is_empty() && unwired.is_none();
            let slug = shell_word(&edit.mission.slug);
            let (summary, next) = match unwired {
                Some(finding) if report.errors.is_empty() => {
                    let to = edit
                        .mission
                        .formations
                        .first()
                        .map_or_else(|| "<step-id>".to_owned(), |step| shell_word(&step.id));
                    (
                        format!("not runnable: {}", finding.message),
                        Some(format!("ahjo mission wire {slug} --to {to}")),
                    )
                }
                _ if valid => (
                    validation_summary(0, report.warnings.len()),
                    Some(run_command(&slug, &edit.mission)),
                ),
                _ => (
                    validation_summary(report.errors.len(), report.warnings.len()),
                    None,
                ),
            };
            let mut answer = json!({"node":node,"valid":valid,"summary":summary,"missionRev":edit.mission.rev,"missionEtag":edit.etag,"errors":report.errors,"warnings":report.warnings});
            if let Some(next) = next {
                answer["next"] = json!(next);
            }
            Ok(answer)
        }
        "mission_edit" => {
            let id = required(&a, "mission")?;
            let current = m.store.mission_edit(id)?;
            if let Some(rev) = a.get("rev").filter(|value| !value.is_null())
                && rev.as_i64() != Some(current.mission.rev)
            {
                return Err(Error::new("conflict", "mission changed"));
            }
            let change = a
                .get("change")
                .filter(|v| v.is_object())
                .ok_or_else(|| bad("change must be an object"))?;
            let patch = wire::patch::decode(change).map_err(bad)?;
            let before = document(current.mission.clone(), current.etag.clone())?;
            let edited = m.store.patch(
                id,
                Precondition {
                    rev: current.mission.rev,
                    etag: current.etag,
                },
                patch,
                "agent:ahjo",
            )?;
            m.changed();
            let after = document(edited.mission, edited.etag)?;
            let mut answer = json!({"node":node,"mission":after});
            for (operation, list, key) in [
                ("createInputCard", "inputCards", "inputCard"),
                ("createFormation", "formations", "formation"),
                ("createGate", "gates", "gate"),
                ("createEnd", "ends", "end"),
                ("createLimit", "limits", "limit"),
            ] {
                if change.get(operation).is_some() {
                    let prior = before[list].as_array();
                    if let Some(created) = answer["mission"][list]
                        .as_array()
                        .and_then(|items| {
                            items.iter().find(|item| {
                                !prior.is_some_and(|p| p.iter().any(|old| old["id"] == item["id"]))
                            })
                        })
                        .cloned()
                    {
                        answer[key] = created;
                    }
                }
            }
            Ok(answer)
        }
        "mission_notes" => {
            let edit = m.store.notes_edit(required(&a, "mission")?)?;
            Ok(json!({"node":node,"notes":notes(edit)?}))
        }
        "mission_note" => {
            let id = required(&a, "mission")?;
            let current = m.store.notes_edit(id)?;
            let author = note_author(&a)?;
            let text = optional(&a, "text", "")?.to_owned();
            let edit = maybe_text(&a, "edit").map_err(bad)?;
            let delete = maybe_text(&a, "delete").map_err(bad)?;
            let entry = |entry: &str| {
                (current.notes.entries.iter())
                    .find(|e| e.id == entry)
                    .map(|e| e.target.clone())
                    .ok_or_else(|| {
                        Error::new(
                            "not_found",
                            format!("no note {entry:?}; mission inspect lists them"),
                        )
                    })
            };
            let patch = match (edit, delete) {
                (Some(_), Some(_)) => return Err(bad("--edit or --delete, not both")),
                (Some(entry_id), None) => NotePatch::Edit {
                    target: entry(entry_id)?,
                    author,
                    id: entry_id.into(),
                    text,
                },
                (None, Some(entry_id)) => NotePatch::Delete {
                    target: entry(entry_id)?,
                    author,
                    id: entry_id.into(),
                },
                (None, None) => {
                    let mission = m.store.read_mission(id)?;
                    NotePatch::Append {
                        target: note_target(&mission, optional(&a, "target", "")?)?,
                        author,
                        text,
                    }
                }
            };
            let edited = m.store.patch_notes(id, &current.etag, patch)?;
            m.changed();
            Ok(note_receipt(node, edited, &current.notes))
        }
        "mission_reply" | "mission_resolve" => {
            let id = required(&a, "mission")?;
            let current = m.store.notes_edit(id)?;
            let patch = NotePatch::Reply {
                to: required(&a, "note")?.into(),
                author: note_author(&a)?,
                text: required(&a, "text")?.into(),
                resolves: name == "mission_resolve",
            };
            let edited = m.store.patch_notes(id, &current.etag, patch)?;
            m.changed();
            Ok(note_receipt(node, edited, &current.notes))
        }
        "layout_show" => {
            let edit = m.store.layout_edit(required(&a, "mission")?)?;
            Ok(json!({"node":node,"layout":layout(edit)?}))
        }
        "layout_edit" => {
            let id = required(&a, "mission")?;
            let change = a
                .get("change")
                .filter(|v| v.is_object())
                .ok_or_else(|| bad("change must be an object"))?;
            let current = m.store.layout_edit(id)?;
            let etag = optional(&a, "etag", &current.etag)?;
            let patch = if change["arrange"] == true {
                let mission = m.store.read_mission(id)?;
                LayoutPatch::Nodes(ahjo_mission_engine::arrange(&mission, &current.layout).nodes)
            } else if let Some(v) = change.get("nodes") {
                LayoutPatch::Nodes(serde_json::from_value(v.clone()).map_err(bad)?)
            } else if let Some(v) = change.get("edges") {
                LayoutPatch::Edges(serde_json::from_value(v.clone()).map_err(bad)?)
            } else {
                return Err(bad("layout needs nodes, edges or arrange"));
            };
            let edited = m.store.patch_layout(id, etag, patch)?;
            m.changed();
            Ok(json!({"node":node,"layout":layout(edited)?}))
        }
        "mission_run" | "formation_run" => start(m, name, &a).await,
        "run_list" => Ok(
            json!({"node":node,"runs":m.coordinator.list(maybe_text(&a,"mission").map_err(bad)?,false)?}),
        ),
        "run_status" => {
            let run = m.coordinator.run(required(&a, "run")?)?;
            let mut v = value(run)?;
            v["node"] = json!(node);
            Ok(v)
        }
        "run_wait" => wait(m, &a).await,
        "run_evidence" => Ok(
            json!({"evidence":m.coordinator.evidence(required(&a,"run")?, maybe_text(&a,"step").map_err(bad)?)?}),
        ),
        "run_attempts" => {
            let id = required(&a, "run")?;
            // Give both evidence commands the same unknown-run error.
            m.coordinator.run(id).map_err(|error| {
                if error.code == "not_found" || error.code == "invalid_input" {
                    Error::new("unknown_run", format!("run {id:?} not found"))
                } else {
                    error
                }
            })?;
            Ok(json!({"attempts":m.coordinator.attempts(id, maybe_text(&a,"step").map_err(bad)?)?}))
        }
        "run_gates" => Ok(json!({"gates":m.coordinator.gates(required(&a,"run")?)?})),
        "gate_request" => m
            .coordinator
            .gate_driver_request(required(&a, "run")?, required(&a, "gate")?),
        "run_seats" => {
            let id = required(&a, "run")?.to_owned();
            let coordinator = m.coordinator.clone();
            let seats = blocking(move || coordinator.seats(&id)).await?;
            Ok(json!({"node":node,"seats":seats}))
        }
        "run_logs" => {
            let id = required(&a, "run")?;
            let mission = m.coordinator.frozen_mission(id).ok();
            let events = m.coordinator.events(id)?;
            let events: Vec<Value> = events
                .into_iter()
                .map(|event| {
                    let step = [&event.node_id, &event.gate_id]
                        .into_iter()
                        .find(|id| !id.is_empty())
                        .and_then(|id| mission.as_ref().map(|m| m.node_name(id).to_owned()))
                        .unwrap_or_default();
                    let mut v = value(event)?;
                    if !step.is_empty() {
                        v["step"] = json!(step);
                    }
                    Ok(v)
                })
                .collect::<Result<_, Error>>()?;
            Ok(json!({"node":node,"events":events}))
        }
        "run_abort" => {
            let id = required(&a, "run")?.to_owned();
            let actor = optional(&a, "actor", "agent:ahjo")?.to_owned();
            let reason = optional(&a, "reason", "operator abort")?.to_owned();
            let coordinator = m.coordinator.clone();
            let run = blocking(move || coordinator.abort(&id, &actor, &reason)).await?;
            Ok(json!({"node":node,"run":run}))
        }
        "run_resume" => {
            let request = wire::ResumeRequest {
                mode: serde_json::from_value(json!(optional(&a, "mode", "reattach")?))
                    .map_err(bad)?,
                take_fail: boolean(&a, "take_fail")?,
                grant: boolean(&a, "grant")?,
                pick_best: boolean(&a, "pick_best")?,
                reason: optional(&a, "reason", "")?.into(),
                actor: optional(&a, "actor", "")?.into(),
            };
            let id = required(&a, "run")?.to_owned();
            let coordinator = m.coordinator.clone();
            let run = blocking(move || coordinator.resume(&id, request)).await?;
            Ok(json!({"node":node,"run":run}))
        }
        "run_delete" => {
            let id = required(&a, "run")?.to_owned();
            let coordinator = m.coordinator.clone();
            let gone = id.clone();
            blocking(move || coordinator.delete_run(&id)).await?;
            m.changed();
            Ok(json!({"node":node,"deleted":gone}))
        }
        "run_land" => {
            let id = required(&a, "run")?.to_owned();
            let force = boolean(&a, "force")?;
            let coordinator = m.coordinator.clone();
            let notes = blocking(move || coordinator.land(&id, force)).await?;
            Ok(json!({"node":node,"landed":true,"notes":notes}))
        }
        "gate_approve" | "gate_reject" => {
            let id = required(&a, "run")?;
            let gate = required(&a, "gate")?;
            m.coordinator.record_verdict(
                id,
                gate,
                wire::HumanVerdict {
                    requested_seq: positive(&a, "requested_seq")?,
                    verdict: if name == "gate_approve" {
                        "pass"
                    } else {
                        "fail"
                    }
                    .into(),
                    response: optional(&a, "response", "")?.into(),
                    relayed_by: optional(&a, "relayed_by", "")?.into(),
                    actor: command_actor(&a)?,
                },
            )?;
            Ok(json!({"node":node,"runId":id}))
        }
        "gate_pick" => {
            let id = required(&a, "run")?;
            let gate = required(&a, "gate")?;
            let attempt = i64::try_from(positive(&a, "attempt")?).map_err(bad)?;
            m.coordinator.record_pick(
                id,
                gate,
                wire::HumanPick {
                    requested_seq: positive(&a, "requested_seq")?,
                    attempt,
                    reason: optional(&a, "reason", "")?.into(),
                    relayed_by: optional(&a, "relayed_by", "")?.into(),
                    actor: command_actor(&a)?,
                },
            )?;
            Ok(json!({"node":node,"runId":id}))
        }
        _ if crate::authoring::NAMES.contains(&name) => crate::authoring::call(&m, name, &a),
        _ => unreachable!(),
    }
}
async fn start(m: Missions, name: &str, a: &Value) -> Result<Value, Error> {
    let selector = required(a, "mission")?;
    let edit = m.store.mission_edit(selector)?;
    let mission = edit.mission;
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
        expected_rev: mission.rev,
        expected_etag: edit.etag,
        actor: optional(a, "actor", "agent:ahjo")?.into(),
        cwd: optional(a, "cwd", "")?.into(),
        repository: optional(a, "repository", "")?.into(),
        base: optional(a, "base", "")?.into(),
        request_id: optional(a, "request_id", "")?.into(),
        known_devices: Some(vec![]),
        ..Default::default()
    };
    request.inputs =
        serde_json::from_value(a.get("inputs").cloned().unwrap_or(json!({}))).map_err(bad)?;
    request.context_paths =
        serde_json::from_value(a.get("context_paths").cloned().unwrap_or(json!([])))
            .map_err(bad)?;
    let work = optional(a, "work", "")?;
    if !work.is_empty() {
        request.work = m.resolver.resolve(work).await?.to_string();
    }
    if name == "mission_run" {
        request.input_card_id = pick(
            "Input card",
            mission
                .input_cards
                .iter()
                .map(|c| (c.id.as_str(), c.title.as_str()))
                .collect(),
            maybe_text(a, "input").map_err(bad)?,
        )?;
    } else {
        request.formation_id = pick(
            "step",
            mission
                .formations
                .iter()
                .map(|f| (f.id.as_str(), f.title.as_str()))
                .collect(),
            Some(required(a, "formation")?),
        )?;
    }
    let formation = name == "formation_run";
    let coordinator = m.coordinator.clone();
    let id = blocking(move || {
        if formation {
            coordinator.start_formation(request)
        } else {
            coordinator.start_mission(request)
        }
        .map_err(admission)
    })
    .await?;
    Ok(json!({"node":m.node,"runId":id}))
}
pub(crate) fn shell_word(value: &str) -> String {
    if value
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"_-./".contains(&b))
        && !value.is_empty()
    {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}
async fn wait(m: Missions, a: &Value) -> Result<Value, Error> {
    use ahjo_mission_engine::WaitUntil;
    let id = required(a, "run")?;
    let until = match optional(a, "until", "needs-you")? {
        "needs-you" => WaitUntil::NeedsYou,
        "final" => WaitUntil::Final,
        "any-change" => WaitUntil::AnyChange,
        _ => return Err(bad("until must be needs-you, final or any-change")),
    };
    let number = |key: &str| -> Result<Option<u64>, Error> {
        match a.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => v
                .as_u64()
                .map(Some)
                .ok_or_else(|| bad(format!("{key} must be a non-negative integer"))),
        }
    };
    let since = number("since")?;
    let timeout = number("timeout")?.unwrap_or(3600);
    let answer = m
        .coordinator
        .wait_run(id, until, since, Duration::from_secs(timeout))
        .await?;
    let run = &answer.run;
    let mut asks = Vec::new();
    let run_arg = shell_word(id);
    for gate in &run.waiting_gates {
        let gate_arg = shell_word(&gate.gate_id);
        let prefix = "ahjo gate";
        let commands: Vec<String> = if let Some(pick) = &gate.pick {
            pick["candidates"].as_array().into_iter().flatten().filter_map(|candidate| {
                candidate.as_i64().or_else(|| candidate["attempt"].as_i64()).map(|attempt| format!("{prefix} pick {run_arg} {gate_arg} --requested-seq {} --attempt {attempt}", gate.requested_seq))
            }).collect()
        } else {
            ["approve", "reject"]
                .into_iter()
                .map(|verb| {
                    format!(
                        "{prefix} {verb} {run_arg} {gate_arg} --requested-seq {}",
                        gate.requested_seq
                    )
                })
                .collect()
        };
        asks.push(json!({"gate":gate.gate_id,"what":if gate.pick.is_some() {"Pick an attempt"} else {"Approve or reject"},"commands":commands}));
    }
    if run.status == "blocked"
        && let Some(event) = run
            .events
            .iter()
            .rev()
            .find(|event| event.kind == "run_blocked")
    {
        let mut commands = Vec::new();
        if run.resume_allowed {
            let extra = if run.resume_policy == "grant" || run.resume_policy == "grant_or_pick" {
                " --grant"
            } else {
                ""
            };
            commands.push(format!("ahjo run resume {run_arg}{extra}"));
            if run.resume_policy == "grant_or_pick" {
                commands.push(format!("ahjo run resume {run_arg} --pick-best"));
            }
        }
        let why = match wire::plain_reason(&event.detail) {
            why if why.is_empty() => run.block_code.replace('_', " "),
            why => why,
        };
        asks.push(
            json!({"step":event.node_id,"what":run.block_code,"why":why,"commands":commands}),
        );
    }
    let next: Vec<_> = asks
        .iter()
        .flat_map(|ask| ask["commands"].as_array().into_iter().flatten().cloned())
        .collect();
    let outputs: Vec<Value> = run
        .outputs
        .iter()
        .map(|o| json!({"name":o.name,"kind":o.kind,"value":o.value}))
        .collect();
    Ok(
        json!({"outcome":answer.outcome,"status":run.status,"cursor":answer.cursor,"asks":asks,"next":next,"outputs":outputs}),
    )
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
