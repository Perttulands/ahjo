//! A mission as an agent picks it up: the operator's notes beside what each
//! is on, the graph in the order a run meets it, and what still stops it
//! running, sized to read in one go. `mission inspect --full` is the whole
//! definition.
use ahjo_missions_contract::{
    Finding, MISSION_THREAD, Mission, NoteEntry, Notes, Slot,
    flow::{self, Decider, StepKind, Target},
};
use serde_json::{Map, Value, json};

use crate::commands::shell_word;

/// The hand-over for one mission.
pub fn handover(
    mission: &Mission,
    notes: &Notes,
    errors: &[Finding],
    warnings: &[Finding],
) -> Value {
    let flow = flow::build(mission);
    let mut out = Map::new();
    let slug = shell_word(&mission.slug);
    out.insert("mission".into(), json!(mission.slug));
    out.insert("title".into(), json!(mission.title));
    out.insert("rev".into(), json!(mission.rev));
    if !mission.updated_at.is_empty() {
        out.insert(
            "changed".into(),
            json!(format!(
                "{} by {}",
                minute(&mission.updated_at),
                mission.updated_by
            )),
        );
    }
    let open = (notes.entries.iter())
        .map(|entry| entry.target.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .filter(|target| !notes.resolved(target))
        .count();
    let threads = (notes.entries.iter())
        .map(|entry| entry.target.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    out.insert(
        "notes".into(),
        json!(format!("{threads} threads, {open} open")),
    );
    out.insert("about".into(), thread(notes, MISSION_THREAD));
    let mut placed: Vec<String> = vec![MISSION_THREAD.into()];

    let mut inputs = Vec::new();
    for card in &mission.input_cards {
        placed.push(card.id.clone());
        let asks = card
            .inputs
            .iter()
            .map(|value| {
                let mut words = format!("{} ({}", value.name, kind(&value.kind));
                words.push_str(if value.required { ")" } else { ", optional)" });
                if !value.description.is_empty() {
                    words.push_str(&format!(": {}", value.description));
                }
                words
            })
            .collect::<Vec<_>>();
        let gives = (card.outputs.iter())
            .map(|value| {
                let from = (value.from.split_once(':'))
                    .map(|(step, _)| format!(" from {}", mission.node_name(step)))
                    .unwrap_or_else(|| " (not wired)".into());
                format!("{}{from}", value.name)
            })
            .collect::<Vec<_>>();
        let starts = flow
            .sections
            .iter()
            .find(|section| section.input.as_deref() == Some(card.id.as_str()))
            .map(|section| targets(&section.start))
            .unwrap_or_default();
        let mut row = Map::new();
        row.insert("id".into(), json!(card.id));
        row.insert("title".into(), json!(card.title));
        row.insert("next".into(), json!(or_nothing(starts)));
        row.insert(
            "asks".into(),
            json!(if asks.is_empty() {
                "brief (text)".to_owned()
            } else {
                asks.join("; ")
            }),
        );
        if !gives.is_empty() {
            row.insert("gives".into(), json!(gives.join("; ")));
        }
        row.insert("goal".into(), json!(card.goal.trim()));
        row.insert("notes".into(), thread(notes, &card.id));
        inputs.push(Value::Object(row));
    }
    out.insert("input".into(), json!(inputs));

    let mut steps = Vec::new();
    for section in &flow.sections {
        for step in &section.steps {
            placed.push(step.id.clone());
            let mut row = Map::new();
            row.insert("n".into(), json!(step.number));
            row.insert("id".into(), json!(step.id));
            if section.input.is_none() {
                row.insert("unreached".into(), json!(true));
            }
            match &step.kind {
                StepKind::Formation { next } => {
                    let Some(formation) = mission.formation(&step.id) else {
                        continue;
                    };
                    row.insert("step".into(), json!(formation.title));
                    row.insert("who".into(), json!(who(&formation.slots)));
                    if formation.fresh {
                        row.insert("fresh".into(), json!(true));
                    }
                    row.insert("next".into(), json!(or_nothing(targets(next))));
                    if step.parallel.len() > 1 {
                        let others = (step.parallel.iter())
                            .filter(|other| **other != step.id)
                            .map(|other| mission.node_name(other))
                            .collect::<Vec<_>>();
                        row.insert("alongside".into(), json!(others.join(", ")));
                    }
                    let brief = formation.brief.as_ref().map(|b| b.goal.trim());
                    row.insert("brief".into(), json!(brief.unwrap_or_default()));
                }
                StepKind::Gate {
                    deciders,
                    judges,
                    pass,
                    fail,
                } => {
                    let Some(gate) = mission.gate(&step.id) else {
                        continue;
                    };
                    row.insert("gate".into(), json!(gate.title));
                    let by = (deciders.iter())
                        .map(|decider| match decider {
                            Decider::You => "the operator",
                            Decider::Judge => "a judge",
                            Decider::Code => "a code check",
                        })
                        .collect::<Vec<_>>();
                    row.insert(
                        "decided".into(),
                        json!(if by.is_empty() {
                            "by nobody yet".to_owned()
                        } else {
                            format!("by {}", by.join(" then "))
                        }),
                    );
                    row.insert("pass".into(), json!(or_nothing(targets(pass))));
                    row.insert("fail".into(), json!(or_nothing(targets(fail))));
                    if !gate.check.is_empty() {
                        row.insert(
                            "check".into(),
                            json!(format!("{} {}", gate.check, gate.check_value)),
                        );
                    }
                    row.insert("checks".into(), json!(gate.criterion.trim()));
                    let mut chain = Vec::new();
                    for judge in judges {
                        let Some(formation) = mission.formation(judge) else {
                            continue;
                        };
                        placed.push(judge.clone());
                        let mut j = Map::new();
                        j.insert("id".into(), json!(judge));
                        j.insert("step".into(), json!(formation.title));
                        j.insert("who".into(), json!(who(&formation.slots)));
                        let brief = formation.brief.as_ref().map(|b| b.goal.trim());
                        j.insert("brief".into(), json!(brief.unwrap_or_default()));
                        j.insert("notes".into(), thread(notes, judge));
                        chain.push(Value::Object(j));
                    }
                    row.insert("judges".into(), json!(chain));
                }
            }
            row.insert("notes".into(), thread(notes, &step.id));
            steps.push(Value::Object(row));
        }
    }
    // A step no wire reaches and no gate judges.
    for formation in &mission.formations {
        if !placed.contains(&formation.id) {
            placed.push(formation.id.clone());
            steps.push(json!({
                "id": formation.id,
                "step": formation.title,
                "unreached": true,
                "who": who(&formation.slots),
                "notes": thread(notes, &formation.id),
            }));
        }
    }
    out.insert("steps".into(), json!(steps));

    let limits = (mission.limits.iter())
        .map(|limit| {
            placed.push(limit.id.clone());
            let on = if mission.has_input_card(&limit.target) {
                "the whole mission".to_owned()
            } else {
                step_words(&flow, mission, &limit.target)
            };
            let mut caps = Vec::new();
            if let Some(n) = limit.rounds {
                caps.push(format!("{n} rounds"));
            }
            if let Some(n) = limit.seconds {
                caps.push(format!("{n}s"));
            }
            if let Some(n) = limit.tokens {
                caps.push(format!("{n} tokens"));
            }
            json!({
                "id": limit.id,
                "on": on,
                "caps": if caps.is_empty() { "nothing yet".to_owned() } else { caps.join(", ") },
                "then": match limit.when_spent.as_str() {
                    "best" => "the best attempt goes on",
                    "fail" => "the run fails",
                    _ => "the run waits for the operator",
                },
                "notes": thread(notes, &limit.id),
            })
        })
        .collect::<Vec<_>>();
    out.insert("limits".into(), json!(limits));
    let ends = (mission.ends.iter())
        .map(|end| {
            placed.push(end.id.clone());
            json!({"id": end.id, "end": end.title, "outcome": end.outcome, "notes": thread(notes, &end.id)})
        })
        .collect::<Vec<_>>();
    out.insert("ends".into(), json!(ends));
    // Wires are listed only where a note is on one.
    let wires = (mission.connections.iter())
        .filter(|wire| notes.thread(&wire.id).next().is_some())
        .map(|wire| {
            placed.push(wire.id.clone());
            json!({
                "id": wire.id,
                "wire": format!("{} -> {}", endpoint(mission, &wire.from), endpoint(mission, &wire.to)),
                "notes": thread(notes, &wire.id),
            })
        })
        .collect::<Vec<_>>();
    if !wires.is_empty() {
        out.insert("wires".into(), json!(wires));
    }
    // Notes left on something since removed.
    let mut orphans = Vec::new();
    for entry in &notes.entries {
        if !placed.contains(&entry.target) {
            placed.push(entry.target.clone());
            orphans.push(json!({"on": entry.target, "notes": thread(notes, &entry.target)}));
        }
    }
    if !orphans.is_empty() {
        out.insert("removed".into(), json!(orphans));
    }

    let problems = (errors.iter())
        .map(|f| format!("error: {}", f.message))
        .chain(warnings.iter().map(|f| format!("warning: {}", f.message)))
        .collect::<Vec<_>>();
    out.insert(
        "runs".into(),
        json!(if errors.is_empty() { "yes" } else { "not yet" }),
    );
    out.insert("problems".into(), json!(problems));
    out.insert(
        "next".into(),
        json!([
            format!("ahjo mission reply {slug} <note-id> --text '<answer or question>' --author <your-name>"),
            format!("ahjo mission resolve {slug} <note-id> --text '<what you did about it>' --author <your-name>"),
            format!("ahjo mission note {slug} --target <id-or-title> --text '<what you want the operator to see>' --author <your-name>"),
            format!("ahjo mission inspect {slug} --full --json"),
        ]),
    );
    Value::Object(out)
}

/// A thread as rows: id, author, when, whether it resolves, and its text.
fn thread(notes: &Notes, target: &str) -> Value {
    json!(
        notes
            .thread(target)
            .map(|entry: &NoteEntry| {
                let mut row = Map::new();
                row.insert("id".into(), json!(entry.id));
                row.insert("by".into(), json!(entry.author));
                row.insert("at".into(), json!(minute(&entry.created_at)));
                if entry.resolves {
                    row.insert("resolves".into(), json!(true));
                }
                row.insert("text".into(), json!(entry.text.trim()));
                Value::Object(row)
            })
            .collect::<Vec<_>>()
    )
}

/// `2026-11-18T14:15:22.51Z` as `2026-11-18 14:15`.
fn minute(at: &str) -> String {
    at.chars().take(16).collect::<String>().replace('T', " ")
}

fn kind(kind: &str) -> &str {
    if kind.is_empty() { "text" } else { kind }
}

/// Who works a step, slot by slot: `maker: claude-code haiku low as builder`.
fn who(slots: &[Slot]) -> String {
    if slots.is_empty() {
        return "nobody".into();
    }
    slots
        .iter()
        .map(|slot| {
            if !slot.staffed() {
                return format!("{}: not staffed", slot.name());
            }
            let mut words = vec![if slot.agent.is_empty() {
                slot.label.clone()
            } else {
                slot.agent.clone()
            }];
            words[0].push(':');
            for part in [&slot.harness, &slot.model, &slot.effort] {
                if !part.is_empty() {
                    words.push(part.clone());
                }
            }
            if !slot.agent_id.is_empty() {
                words.push(format!("as {}", slot.agent_id));
            }
            if slot.controller {
                words.push("(controller)".into());
            }
            words.join(" ")
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn targets(list: &[Target]) -> Vec<String> {
    list.iter()
        .map(|target| match target {
            Target::Step {
                number,
                title,
                back,
                ..
            } => {
                let at = number.map(|n| format!("{n} ")).unwrap_or_default();
                if *back {
                    format!("back to {at}{title}")
                } else {
                    format!("{at}{title}")
                }
            }
            Target::End { outcome, .. } => format!("end ({outcome})"),
            Target::Nowhere => "nothing yet".into(),
        })
        .collect()
}

fn or_nothing(words: Vec<String>) -> String {
    if words.is_empty() {
        "nothing yet".into()
    } else {
        words.join(", ")
    }
}

/// A step as the operator numbers it: `3 Draft`.
fn step_words(flow: &flow::Flow, mission: &Mission, id: &str) -> String {
    match flow.numbers.get(id) {
        Some(n) => format!("{n} {}", mission.node_name(id)),
        None => mission.node_name(id).to_owned(),
    }
}

/// `node:port` as a reader names it: the node, and the port where it
/// matters (a gate's verdict).
fn endpoint(mission: &Mission, endpoint: &str) -> String {
    let (node, port) = endpoint.split_once(':').unwrap_or((endpoint, ""));
    let name = mission.node_name(node);
    match port {
        "pass" | "fail" | "judge" => format!("{name}:{port}"),
        _ => name.to_owned(),
    }
}
