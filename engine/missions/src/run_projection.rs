//! Public, sanitized views of a validated run ledger.
use crate::{Coordinator, RunEvent, run_replay::Replay, validate_ledger};
use ahjo_mission_model::{PublicEvent, PublicInputRef, RunProjection, RunSendBack, WaitingGate};
use std::io;

fn string(event: &RunEvent, name: &str) -> String {
    event
        .data
        .get(name)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned()
}
fn strings(event: &RunEvent, name: &str) -> Vec<String> {
    event
        .data
        .get(name)
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect()
}

impl Coordinator {
    /// The End's original delivery, reconstructed against the frozen graph
    /// so the findings come from the path that actually ended the run.
    pub fn send_back(&self, run: &RunProjection) -> Result<Option<RunSendBack>, crate::Error> {
        let Some(end) = run
            .events
            .iter()
            .rfind(|event| event.kind == "run_failed" && event.code == "path_rejected")
            .filter(|_| run.r#final && run.status == "sent_back")
        else {
            return Ok(None);
        };
        let mission = self.frozen_mission(&run.run_id)?;
        let events = self.ledger(&run.run_id).map_err(crate::error::io)?.read()?;
        let findings = Replay::read(&mission, &events)
            .ended
            .into_iter()
            .find(|delivery| {
                delivery.outcome == "rejected"
                    && delivery.end_id == end.node_id
                    && delivery.gate_id == end.gate_id
            })
            .map_or_else(String::new, |delivery| delivery.findings);
        Ok(Some(RunSendBack {
            end_id: end.node_id.clone(),
            gate_id: end.gate_id.clone(),
            gate_title: mission.node_name(&end.gate_id).to_owned(),
            reason: end.detail.clone(),
            findings,
        }))
    }
}

/// The run's public projection from its validated ledger.
pub fn project_run(events: &[RunEvent], run_id: &str) -> Result<RunProjection, crate::Error> {
    (|| -> io::Result<RunProjection> {
        validate_ledger(events, run_id).map_err(crate::error::to_io)?;
        let first = &events[0];
        let mut result = RunProjection {
            run_id: run_id.to_owned(),
            status: "running".into(),
            r#final: false,
            resume_allowed: false,
            epoch: 0,
            resume_policy: String::new(),
            block_code: String::new(),
            mission_slug: string(first, "mission_slug"),
            mission_id: first.mission_id.clone(),
            mission_rev: first.mission_rev,
            input_card_id: first.input_card_id.clone(),
            work: first.work.clone(),
            cwd: string(first, "cwd"),
            repository: string(first, "repository"),
            base: string(first, "base"),
            request_id: string(first, "request_id"),
            context_paths: strings(first, "context_paths"),
            inputs: first
                .data
                .get("inputs")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default(),
            outputs: Vec::new(),
            event_count: events.len(),
            started_by: first.actor.clone(),
            started_at: first.timestamp.clone(),
            updated_at: first.timestamp.clone(),
            ended_by: String::new(),
            waiting_gates: Vec::new(),
            events: Vec::new(),
        };
        for event in events {
            result.updated_at.clone_from(&event.timestamp);
            result.epoch = event.epoch;
            let code = string(event, "code");
            let sent_back = event.kind == "run_failed" && code == "path_rejected";
            match event.kind.as_str() {
                "run_started" | "run_resumed" => {
                    result.status = "running".into();
                    result.r#final = false;
                    result.resume_allowed = false;
                    result.block_code.clear();
                    result.resume_policy.clear();
                }
                "run_blocked" => {
                    result.status = "blocked".into();
                    result.block_code.clone_from(&code);
                    result.resume_policy = string(event, "resume_policy");
                    result.resume_allowed = event
                        .data
                        .get("resume_allowed")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                }
                "run_succeeded" | "run_failed" | "run_picked" | "run_canceled" => {
                    result.outputs = event
                        .data
                        .get("outputs")
                        .and_then(|v| serde_json::from_value(v.clone()).ok())
                        .unwrap_or_default();
                    result.status = if sent_back {
                        "sent_back".into()
                    } else {
                        event.kind.trim_start_matches("run_").into()
                    };
                    result.r#final = true;
                    result.resume_allowed = false;
                    result.block_code.clear();
                    result.resume_policy.clear();
                    if matches!(result.status.as_str(), "failed" | "canceled" | "sent_back") {
                        result.ended_by.clone_from(&event.actor);
                    }
                }
                "gate_evaluating" => {
                    result
                        .waiting_gates
                        .retain(|gate| gate.gate_id != event.gate_id);
                }
                "human_input_requested" => {
                    result
                        .waiting_gates
                        .retain(|gate| gate.gate_id != event.gate_id);
                    result.waiting_gates.push(WaitingGate {
                        gate_id: event.gate_id.clone(),
                        requested_seq: event.seq,
                        requested_at: event.timestamp.clone(),
                        pick: event.data.get("pick").cloned(),
                    });
                }
                "human_verdict_recorded" => {
                    let requested = event
                        .data
                        .get("requested_seq")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    result.waiting_gates.retain(|gate| {
                        gate.gate_id != event.gate_id
                            || (requested != 0 && gate.requested_seq != requested)
                    });
                }
                _ => {}
            }
            result.events.push(PublicEvent {
                seq: event.seq,
                kind: event.kind.clone(),
                node_id: event.node_id.clone(),
                slot_id: event.slot_id.clone(),
                gate_id: event.gate_id.clone(),
                attempt: event.attempt,
                requested_seq: event
                    .data
                    .get("requested_seq")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                status: string(event, "status"),
                commit: string(event, "commit"),
                verdict: string(event, "verdict"),
                outcome: string(event, "outcome"),
                code,
                session_name: string(event, "session_name"),
                seat_node: if event.kind == "seat_created" {
                    string(event, "node")
                } else {
                    String::new()
                },
                session_id: if event.kind == "seat_created" {
                    string(event, "session_id")
                } else {
                    String::new()
                },
                state: string(event, "state"),
                detail: if sent_back
                    || matches!(
                        event.kind.as_str(),
                        "human_verdict_recorded" | "run_canceled"
                    ) {
                    string(event, "reason")
                } else if event.kind == "run_blocked" && string(event, "detail").is_empty() {
                    // A block says why in its reason when it has no detail.
                    string(event, "reason")
                } else {
                    string(event, "detail")
                },
                since: string(event, "since"),
                end_ids: strings(event, "end_ids"),
                input_refs: event
                    .data
                    .get("input_refs")
                    .and_then(|v| v.as_array())
                    .into_iter()
                    .flatten()
                    .map(|v| PublicInputRef {
                        edge_id: v["edge_id"].as_str().unwrap_or("").into(),
                        from_node_id: v["from_node_id"].as_str().unwrap_or("").into(),
                        from_port_id: v["from_port_id"].as_str().unwrap_or("").into(),
                        to_port_id: v["to_port_id"].as_str().unwrap_or("").into(),
                        origin: serde_json::from_value(v["origin"].clone()).ok(),
                    })
                    .collect(),
            });
        }
        if result.r#final {
            result.waiting_gates.clear();
        } else if result.status != "blocked" && !result.waiting_gates.is_empty() {
            result.status = "waiting_human".into();
        }
        Ok(result)
    })()
    .map_err(crate::error::io)
}
