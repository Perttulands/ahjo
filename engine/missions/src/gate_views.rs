//! Driver-facing gate state and consequences on the frozen graph.

use ahjo_mission_model::{
    DecisionInput, DecisionLimit, DecisionRoute, DecisionTarget, DecisionTargetKind, GateDecision,
    GateState, RunGate,
};
use serde_json::Value;
use time::OffsetDateTime;

use crate::{Coordinator, Error};

impl Coordinator {
    /// Every gate on the frozen mission, including gates not reached yet.
    pub fn gates(&self, id: &str) -> Result<Vec<RunGate>, Error> {
        let events = self.ledger(id).map_err(crate::error::io)?.read()?;
        let run = crate::project_run(&events, id)?;
        let mission = self.frozen_mission(id)?;
        Ok(mission
            .gates
            .iter()
            .map(|gate| {
                let waiting = run.waiting_gates.iter().find(|g| g.gate_id == gate.id);
                let answer = events.iter().rev().find(|e| {
                    e.gate_id == gate.id
                        && (e.kind == "human_verdict_recorded" || e.kind == "gate_verdict")
                });
                let state = if let Some(waiting) = waiting {
                    GateState::Waiting {
                        requested_seq: waiting.requested_seq,
                    }
                } else if let Some(answer) = answer {
                    let answer = answer
                        .data
                        .get("requested_seq")
                        .and_then(Value::as_u64)
                        .and_then(|seq| {
                            events.iter().rev().find(|e| {
                                e.kind == "human_verdict_recorded"
                                    && e.gate_id == gate.id
                                    && e.data.get("requested_seq").and_then(Value::as_u64)
                                        == Some(seq)
                            })
                        })
                        .unwrap_or(answer);
                    GateState::Answered {
                        verdict: answer
                            .data
                            .get("verdict")
                            .and_then(Value::as_str)
                            .unwrap_or(if answer.data.contains_key("pick") {
                                "pass"
                            } else {
                                ""
                            })
                            .into(),
                        by: answer
                            .data
                            .get("decided_by")
                            .and_then(Value::as_str)
                            .filter(|s| !s.is_empty())
                            .unwrap_or(&answer.actor)
                            .into(),
                        relayed_by: answer
                            .data
                            .get("relayed_by")
                            .and_then(Value::as_str)
                            .filter(|s| !s.is_empty())
                            .map(str::to_owned),
                    }
                } else {
                    GateState::NotReached
                };
                RunGate {
                    gate: gate.id.clone(),
                    title: gate.title.clone(),
                    state,
                }
            })
            .collect())
    }

    /// A waiting gate's request and each possible route, from the frozen run.
    pub fn gate_driver_request(&self, id: &str, gate: &str) -> Result<GateDecision, Error> {
        let events = self.ledger(id).map_err(crate::error::io)?.read()?;
        let mission = self.frozen_mission(id)?;
        let definition = mission
            .gate(gate)
            .ok_or_else(|| Error::new("not_found", "gate not found"))?;
        let request =
            crate::gate_request::pending_request(id, gate, &events, &definition.criterion)
                .map_err(|e| {
                    if e.code == "not_found" || e.code == "not_pending" {
                        Error::new("gate_not_waiting", "gate is not waiting")
                    } else {
                        e
                    }
                })?;
        let raw = events
            .iter()
            .find(|e| e.seq == request.requested_seq)
            .expect("pending request exists");
        let input = raw.data.get("input_ref").cloned().unwrap_or_default();
        let replay = crate::run_replay::Replay::read(&mission, &events);
        let projection = crate::project_run(&events, id)?;
        let now = OffsetDateTime::now_utc();
        let routes: Vec<_> = ["pass", "fail"]
            .into_iter()
            .filter(|verdict| request.pick.is_none() || *verdict == "pass")
            .map(|verdict| {
                let mut targets = Vec::new();
                for edge in crate::graph::outgoing_from_port(&mission, gate, verdict) {
                    let node = crate::graph::endpoint_node(&edge.to);
                    let detail = if let Some(step) = mission.step(node) {
                        let limits = crate::limits::uses_for_step(&mission, &events, node, now)
                            .into_iter().map(DecisionLimit::from).collect();
                        let waits_for_inputs = step.inputs.iter().any(|port| {
                            !replay.ports.get(node).is_some_and(|ports| ports.contains_key(&port.id))
                                && !crate::graph::outgoing_from_port(&mission, gate, verdict).any(|edge| crate::graph::endpoint_node(&edge.to) == node && crate::graph::endpoint_port(&edge.to) == port.id)
                        });
                        let spent = if waits_for_inputs { None } else {
                            crate::limits::spent_before(&mission, &events, node, now)
                        };
                        let picks_best = spent.as_ref().map(|spent| {
                            crate::limits::card(&mission, &spent.limit_id)
                                .is_some_and(|card| card.when_spent == "best")
                                && crate::run_pick::pick_for(&mission, &events, node, spent)
                                    .is_some_and(|pick| crate::run_pick::judged_since_last_pick(&events, node, &pick.candidates))
                        });
                        Some(DecisionTargetKind::Step {
                            title: step.title.clone(), limits, waits_for_inputs,
                            picks_best, spent_limit: spent.map(DecisionLimit::from),
                        })
                    } else if let Some(end) = mission.end(node) {
                        Some(DecisionTargetKind::End { title: end.title.clone(), outcome: end.outcome.clone() })
                    } else {
                        mission.gate(node).map(|next| DecisionTargetKind::Gate { title: next.title.clone() })
                    };
                    targets.push(DecisionTarget {
                        node_id: node.into(), port_id: crate::graph::endpoint_port(&edge.to).into(), detail,
                    });
                }
                // Only claim a final run when all destinations are ends and no
                // other active work or pending gate remains.
                let rejected = targets.iter().any(|target| {
                    matches!(&target.detail, Some(DecisionTargetKind::End {outcome, ..}) if outcome == "rejected")
                }) || replay.ended.iter().any(|delivery| delivery.outcome == "rejected");
                let starved = mission.steps.iter().any(|step| replay.ports.get(&step.id).is_some_and(|ports| ports.values().any(|(_, unread)| *unread) && step.inputs.iter().any(|port| !ports.contains_key(&port.id))));
                let ready = mission.steps.iter().any(|step| replay.ports.get(&step.id).is_some_and(|ports| ports.values().any(|(_, unread)| *unread) && step.inputs.iter().all(|port| ports.contains_key(&port.id))));
                let ends_run = !ready && (rejected || !starved) && !targets.is_empty()
                    && targets.iter().all(|target| matches!(target.detail, Some(DecisionTargetKind::End {..})))
                    && projection.waiting_gates.len() == 1
                    && replay.open.is_empty()
                    && replay.evaluating.is_empty()
                    && replay.picking.iter().all(|pending| pending == gate)
                    && replay.queue.values().all(|q| q.is_empty())
                    && replay.unfinished.is_empty();
                let picks_best = targets.iter().any(|target| {
                    matches!(target.detail, Some(DecisionTargetKind::Step { picks_best: Some(true), .. }))
                });
                DecisionRoute { verdict: verdict.into(), targets, ends_run, picks_best }
            })
            .collect();
        let inputs = vec![DecisionInput {
            from_node_id: request.input.from_node_id,
            from_port_id: request.input.from_port_id,
            text: request.input.text,
            truncated: request.input.truncated,
            reference: input["ref"].clone(),
            report_ref: input["report_ref"].clone(),
            artifact_ref: input["artifact_ref"].clone(),
        }];
        Ok(GateDecision {
            gate: gate.into(),
            requested_seq: request.requested_seq,
            criterion: request.criterion,
            inputs,
            ends_run: routes.iter().any(|route| route.ends_run),
            picks_best: request.pick.is_some() || routes.iter().any(|route| route.picks_best),
            routes,
            pick: request.pick,
        })
    }
}

impl From<crate::limits::LimitUse> for DecisionLimit {
    fn from(limit: crate::limits::LimitUse) -> Self {
        Self {
            kind: limit.kind,
            limit_id: limit.limit_id,
            node_id: limit.node_id,
            used: limit.used,
            max: limit.max,
            left: (limit.max - limit.used).max(0),
            granted: limit.granted,
        }
    }
}
