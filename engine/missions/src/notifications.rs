//! Needs-you messages projected from the durable run ledger.
use crate::{Coordinator, Error, RunEvent};
use ahjo_mission_model::{RunProjection, WaitingGate};
use serde_json::{Value, json};

fn field<'a>(event: &'a RunEvent, name: &str) -> &'a str {
    event.data.get(name).and_then(Value::as_str).unwrap_or("")
}

impl Coordinator {
    /// Current asks, keyed by their ledger sequence. Blocks are announced only
    /// after the worker has settled, so a concurrent cancellation can win.
    pub fn notifications(&self, id: &str) -> Result<Vec<Value>, Error> {
        let events = self.ledger(id).map_err(crate::error::io)?.read()?;
        let run = crate::project_run(&events, id)?;
        let mission = self.frozen_mission(id)?;
        let settled = !self.inner.controls.lock().unwrap().contains_key(id);
        let mut asks = Vec::new();
        let last_resume = events
            .iter()
            .rev()
            .find(|e| e.kind == "run_resumed")
            .map_or(0, |e| e.seq);
        if !run.r#final {
            for gate in &run.waiting_gates {
                if let Some(event) = events.iter().find(|e| e.seq == gate.requested_seq) {
                    let definition = mission.gate(&gate.gate_id);
                    let title = definition
                        .map(|g| g.title.as_str())
                        .filter(|s| !s.is_empty())
                        .unwrap_or(&gate.gate_id);
                    let criterion = definition.map(|g| g.criterion.as_str()).unwrap_or("");
                    let input = event
                        .data
                        .get("input_ref")
                        .and_then(|v| v.get("text"))
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let input = &input[..input.floor_char_boundary(input.len().min(64 * 1024))];
                    let severity = if gate.pick.is_some() {
                        "pick"
                    } else {
                        "verdict"
                    };
                    let commands = gate_commands(id, gate).join("\n");
                    let subject = format!("Ahjo needs your answer: {title} ({})", mission.title);
                    let body = format!(
                        "{} is waiting at {title}.\n\nCriterion: {criterion}\n\nInput:\n{input}\n\n{commands}\n\nRun: {id}\nMission: {}\n",
                        mission.title, run.mission_slug
                    );
                    asks.push(message(
                        &run,
                        event,
                        &mission.title,
                        "human_gate",
                        criterion,
                        severity,
                        true,
                        title,
                        subject,
                        body,
                    ));
                }
            }
            let escalations: Vec<_> = events
                .iter()
                .filter(|e| {
                    e.kind == "escalation_raised"
                        && e.seq > last_resume
                        && e.data.get("blocks").and_then(Value::as_bool) == Some(true)
                })
                .collect();
            for event in &escalations {
                let reason = field(event, "reason");
                let subject = format!(
                    "Ahjo needs you: {} escalated ({})",
                    event.card_id, mission.title
                );
                let body = format!(
                    "Run {id} needs you: {reason}\n\nahjo run resume {id} --reason 'how it was resolved'\n"
                );
                asks.push(message(
                    &run,
                    event,
                    &mission.title,
                    "escalation",
                    reason,
                    field(event, "severity"),
                    true,
                    "",
                    subject,
                    body,
                ));
            }
            if settled
                && run.status == "blocked"
                && escalations.is_empty()
                && let Some(event) = events.iter().rev().find(|e| e.kind == "run_blocked")
                && (field(event, "code") != "coordinator_shutdown" || run.waiting_gates.is_empty())
            {
                let reason = field(event, "reason");
                let subject = format!("Ahjo run blocked: {}", mission.title);
                let commands = if run.resume_allowed {
                    resume_commands(&run).join("\n")
                } else {
                    "Inspect the run and start a new one.".to_owned()
                };
                let body = format!("Run {id} is blocked: {reason}\n\n{commands}\n");
                asks.push(message(
                    &run,
                    event,
                    &mission.title,
                    "blocked",
                    reason,
                    "",
                    true,
                    "",
                    subject,
                    body,
                ));
            }
        } else if settled && let Some(event) = events.last() {
            let subject = format!("Ahjo run {}: {}", run.status, mission.title);
            let picked = if run.status == "picked" {
                "Work continued through a pick of failed attempts.\n"
            } else {
                ""
            };
            let body = format!(
                "Run {id} on {} finished: {}.\n{picked}{}",
                mission.title,
                run.status,
                field(event, "reason")
            );
            asks.push(message(
                &run,
                event,
                &mission.title,
                "final",
                field(event, "reason"),
                "",
                false,
                "",
                subject,
                body,
            ));
        }
        asks.sort_by_key(|a| a["seq"].as_u64().unwrap_or(0));
        Ok(asks)
    }
}

#[allow(clippy::too_many_arguments)]
fn message(
    run: &ahjo_mission_model::RunProjection,
    event: &RunEvent,
    mission: &str,
    kind: &str,
    ask: &str,
    severity: &str,
    blocks: bool,
    gate_title: &str,
    subject: String,
    body: String,
) -> Value {
    json!({"run_id":run.run_id,"mission_slug":run.mission_slug,"mission_title":mission,"seq":event.seq,"kind":kind,"run_status":run.status,"card_id":event.card_id,"gate_id":event.gate_id,"gate_title":gate_title,"ask":ask,"severity":severity,"blocks":blocks,"text":subject,"subject":subject,"body":body})
}

/// The commands that answer a pending gate at its current sequence.
pub fn gate_commands(run: &str, gate: &WaitingGate) -> Vec<String> {
    let run = shell_word(run);
    let gate_arg = shell_word(&gate.gate_id);
    let seq = gate.requested_seq;
    if let Some(pick) = &gate.pick {
        pick["candidates"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|candidate| {
                candidate
                    .as_i64()
                    .or_else(|| candidate["attempt"].as_i64())
                    .map(|attempt| {
                        format!("ahjo run pick {run} {gate_arg} --seq {seq} --attempt {attempt}")
                    })
            })
            .collect()
    } else {
        ["approve", "reject"]
            .map(|verb| format!("ahjo run {verb} {run} {gate_arg} --seq {seq}"))
            .to_vec()
    }
}

/// The supported ways to resume a blocked run.
pub fn resume_commands(run: &RunProjection) -> Vec<String> {
    if !run.resume_allowed {
        return Vec::new();
    }
    let id = shell_word(&run.run_id);
    match (run.resume_policy.as_str(), run.block_code.as_str()) {
        ("grant", _) => vec![format!("ahjo run resume {id} --grant --reason '<reason>'")],
        ("grant_or_pick", _) => vec![
            format!("ahjo run resume {id} --grant --reason '<reason>'"),
            format!("ahjo run resume {id} --pick-best --reason '<reason>'"),
        ],
        (_, "seat_gone") => vec![format!(
            "ahjo run resume {id} --mode redispatch --reason '<reason>'"
        )],
        _ => vec![format!("ahjo run resume {id}")],
    }
}

pub fn shell_word(value: &str) -> String {
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
