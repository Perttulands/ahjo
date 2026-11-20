//! The operator's notes beside the intent, graph and next commands.

use std::collections::BTreeSet;

use ahjo_missions_contract::{
    Finding, MISSION_THREAD, Mission, Notes, Seat, Step,
    commands::{
        HandoverEnd, HandoverInput, HandoverJudge, HandoverLimit, HandoverNote, HandoverSeat,
        HandoverStep, HandoverStepDetail, HandoverWire, HandoverWorker, MissionHandover,
        RemovedThread,
    },
    flow::{self, Decider, FlowStepKind, Target},
    seat_session,
};

use crate::commands::shell_word;

pub fn handover(
    node: String,
    mission: &Mission,
    agents: &ahjo_mission_engine::AgentStore,
    notes: &Notes,
    errors: &[Finding],
    warnings: &[Finding],
) -> MissionHandover {
    let flow = flow::build(mission);
    let prefix = mission.session_prefix();
    let slug = shell_word(&mission.slug);
    let changed = (!mission.updated_at.is_empty())
        .then(|| format!("{} by {}", minute(&mission.updated_at), mission.updated_by));
    let threads: BTreeSet<_> = notes
        .entries
        .iter()
        .map(|entry| entry.target.as_str())
        .collect();
    let open = threads
        .iter()
        .filter(|target| !notes.resolved(target))
        .count();
    let thread_count = threads.len();
    let mut placed = BTreeSet::from([MISSION_THREAD]);

    let input = mission
        .input_cards
        .iter()
        .map(|card| {
            placed.insert(card.id.as_str());
            let asks: Vec<_> = card
                .inputs
                .iter()
                .map(|value| {
                    let mut words = format!("{} ({}", value.name, value.kind.effective());
                    words.push_str(if value.required { ")" } else { ", optional)" });
                    if !value.description.is_empty() {
                        words.push_str(&format!(": {}", value.description));
                    }
                    words
                })
                .collect();
            let gives: Vec<_> = card
                .outputs
                .iter()
                .map(|value| {
                    let from = value
                        .from
                        .split_once(':')
                        .map(|(step, _)| format!(" from {}", mission.node_name(step)))
                        .unwrap_or_else(|| " (not wired)".into());
                    format!("{}{from}", value.name)
                })
                .collect();
            let starts = flow
                .sections
                .iter()
                .find(|section| section.input.as_deref() == Some(card.id.as_str()))
                .map(|section| targets(&section.start))
                .unwrap_or_default();
            HandoverInput {
                id: card.id.clone(),
                title: card.title.clone(),
                next: or_nothing(starts),
                asks: if asks.is_empty() {
                    "brief (text)".into()
                } else {
                    asks.join("; ")
                },
                gives: (!gives.is_empty()).then(|| gives.join("; ")),
                goal: card.goal.trim().into(),
                notes: thread(notes, &card.id),
            }
        })
        .collect();

    let mut steps = Vec::new();
    for section in &flow.sections {
        for step in &section.steps {
            placed.insert(step.id.as_str());
            let detail = match &step.kind {
                FlowStepKind::Step { next } => {
                    let Some(work_step) = mission.step(&step.id) else {
                        continue;
                    };
                    let alongside = (step.parallel.len() > 1).then(|| {
                        step.parallel
                            .iter()
                            .filter(|other| **other != step.id)
                            .map(|other| mission.node_name(other))
                            .collect::<Vec<_>>()
                            .join(", ")
                    });
                    HandoverStepDetail::Step {
                        worker: worker(&prefix, work_step, agents, true),
                        next: Some(or_nothing(targets(next))),
                        alongside,
                    }
                }
                FlowStepKind::Gate {
                    deciders,
                    judges,
                    pass,
                    fail,
                } => {
                    let Some(gate) = mission.gate(&step.id) else {
                        continue;
                    };
                    let by: Vec<_> = deciders
                        .iter()
                        .map(|decider| match decider {
                            Decider::You => "the operator",
                            Decider::Judge => "a judge",
                            Decider::Code => "a code check",
                        })
                        .collect();
                    let judges = judges
                        .iter()
                        .filter_map(|judge| {
                            let step = mission.step(judge)?;
                            placed.insert(judge.as_str());
                            Some(HandoverJudge {
                                id: judge.clone(),
                                worker: worker(&prefix, step, agents, true),
                                notes: thread(notes, judge),
                            })
                        })
                        .collect();
                    HandoverStepDetail::Gate {
                        gate: gate.title.clone(),
                        decided: if by.is_empty() {
                            "by nobody yet".into()
                        } else {
                            format!("by {}", by.join(" then "))
                        },
                        pass: or_nothing(targets(pass)),
                        fail: or_nothing(targets(fail)),
                        check: (!gate.check.is_empty())
                            .then(|| format!("{} {}", gate.check, gate.check_value)),
                        checks: gate.criterion.trim().into(),
                        judges,
                    }
                }
            };
            steps.push(HandoverStep {
                n: Some(step.number),
                id: step.id.clone(),
                unreached: section.input.is_none(),
                detail,
                notes: thread(notes, &step.id),
            });
        }
    }
    // A step no wire reaches and no gate judges.
    for step in &mission.steps {
        if placed.insert(step.id.as_str()) {
            steps.push(HandoverStep {
                n: None,
                id: step.id.clone(),
                unreached: true,
                detail: HandoverStepDetail::Step {
                    worker: worker(&prefix, step, agents, false),
                    next: None,
                    alongside: None,
                },
                notes: thread(notes, &step.id),
            });
        }
    }

    let limits = mission
        .limits
        .iter()
        .map(|limit| {
            placed.insert(limit.id.as_str());
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
            HandoverLimit {
                id: limit.id.clone(),
                on: if mission.has_input_card(&limit.target) {
                    "the whole mission".into()
                } else {
                    step_words(&flow, mission, &limit.target)
                },
                caps: if caps.is_empty() {
                    "nothing yet".into()
                } else {
                    caps.join(", ")
                },
                then: match limit.when_spent.as_str() {
                    "best" => "the best attempt goes on",
                    "fail" => "the run fails",
                    _ => "the run waits for the operator",
                }
                .into(),
                notes: thread(notes, &limit.id),
            }
        })
        .collect();
    let ends = mission
        .ends
        .iter()
        .map(|end| {
            placed.insert(end.id.as_str());
            HandoverEnd {
                id: end.id.clone(),
                end: end.title.clone(),
                outcome: end.outcome.as_str().to_owned(),
                notes: thread(notes, &end.id),
            }
        })
        .collect();
    let wires = mission
        .connections
        .iter()
        .filter(|wire| notes.thread(&wire.id).next().is_some())
        .map(|wire| {
            placed.insert(wire.id.as_str());
            HandoverWire {
                id: wire.id.clone(),
                wire: format!(
                    "{} -> {}",
                    endpoint(mission, &wire.from),
                    endpoint(mission, &wire.to)
                ),
                notes: thread(notes, &wire.id),
            }
        })
        .collect();
    // Notes left on something since removed, in the order they were written.
    let removed = notes
        .entries
        .iter()
        .filter(|entry| placed.insert(entry.target.as_str()))
        .map(|entry| RemovedThread {
            on: entry.target.clone(),
            notes: thread(notes, &entry.target),
        })
        .collect();

    MissionHandover {
        mission: mission.slug.clone(),
        title: mission.title.clone(),
        rev: mission.rev,
        changed,
        notes: format!("{thread_count} threads, {open} open"),
        about: thread(notes, MISSION_THREAD),
        input,
        steps,
        limits,
        ends,
        wires,
        removed,
        runs: if errors.is_empty() { "yes" } else { "not yet" }.into(),
        problems: errors
            .iter()
            .map(|f| format!("error: {}", f.message))
            .chain(warnings.iter().map(|f| format!("warning: {}", f.message)))
            .collect(),
        next: [
            format!(
                "ahjo mission reply {slug} <note-id> --text '<answer or question>' --author <your-name>"
            ),
            format!(
                "ahjo mission resolve {slug} <note-id> --text '<what you did about it>' --author <your-name>"
            ),
            format!(
                "ahjo mission note {slug} --target <id-or-title> --text '<what you want the operator to see>' --author <your-name>"
            ),
            format!("ahjo mission show {slug} --full --json"),
        ],
        node,
    }
}

fn thread(notes: &Notes, target: &str) -> Vec<HandoverNote> {
    notes
        .thread(target)
        .map(|entry| HandoverNote {
            id: entry.id.clone(),
            by: entry.author.clone(),
            at: minute(&entry.created_at),
            resolves: entry.resolves,
            text: entry.text.trim().into(),
        })
        .collect()
}

/// `2026-11-18T14:15:22.51Z` as `2026-11-18 14:15`.
fn minute(at: &str) -> String {
    at.chars().take(16).collect::<String>().replace('T', " ")
}

/// The named session each seat asks for. A fresh step gets a run- and
/// attempt-specific session when it executes.
fn worker(
    prefix: &str,
    step: &Step,
    agents: &ahjo_mission_engine::AgentStore,
    include_brief: bool,
) -> HandoverWorker {
    HandoverWorker {
        step: step.title.clone(),
        who: who(&step.seats, agents),
        seats: step
            .seats
            .iter()
            .map(|seat| {
                let agent = agents.read(&seat.agent).ok();
                let settings = seat.settings(agent.as_ref());
                HandoverSeat {
                    id: seat.id.clone(),
                    agent: seat.agent.clone(),
                    harness: settings.harness,
                    model: settings.model,
                    effort: settings.effort,
                    session: seat_session(prefix, &step.session_part(seat)),
                }
            })
            .collect(),
        fresh: step.fresh,
        brief: include_brief.then(|| {
            step.brief
                .as_ref()
                .map_or("", |brief| brief.goal.trim())
                .into()
        }),
    }
}

/// Who works a step, seat by seat: `maker: claude-code haiku low as builder`.
fn who(seats: &[Seat], agents: &ahjo_mission_engine::AgentStore) -> String {
    if seats.is_empty() {
        return "nobody".into();
    }
    seats
        .iter()
        .map(|seat| {
            if !seat.staffed() {
                return format!("{}: not staffed", seat.name());
            }
            let agent = agents.read(&seat.agent).ok();
            let settings = seat.settings(agent.as_ref());
            let mut words = vec![format!("{}:", seat.name())];
            if !seat.agent.is_empty() {
                words.push(
                    agent
                        .as_ref()
                        .map_or(seat.agent.clone(), |agent| agent.name.clone()),
                );
            }
            for part in [
                settings.harness.as_str(),
                settings.model.as_str(),
                settings.effort.as_str(),
            ] {
                if !part.is_empty() {
                    words.push(part.to_owned());
                }
            }
            if seat.controller {
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
