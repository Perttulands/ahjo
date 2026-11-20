//! The same decision prompts guide a driver in run show and run wait.

use std::time::Duration;

use ahjo_core::Error;
use ahjo_mission_engine::{WaitUntil, gate_commands, resume_commands};
use ahjo_missions_contract::{
    HumanPick, HumanVerdict, ResumeRequest, RunProjection, RunSendBack, plain_reason,
    run_commands::*,
};
use ahjo_node_kit::{Command, Effect};

use crate::{
    Missions,
    commands::{bad, command, required},
    run_routes::seat_wire,
};

pub use ahjo_missions_contract::run_commands::NAMES;

pub fn commands(m: &Missions) -> Vec<Command> {
    use Effect::*;
    let wait_on = m.clone();
    vec![
        command(m, Reads, |m, args: RunList| {
            Ok(RunsListed {
                node: m.node.clone(),
                runs: m.coordinator.list(args.mission.as_deref(), false)?,
            })
        }),
        command(m, Reads, show),
        Command::for_request(Reads, move |args: RunWait| wait(wait_on.clone(), args)),
        command(m, Writes, |m, args: RunApprove| {
            verdict(
                m,
                &args.run,
                &args.gate,
                HumanVerdict {
                    requested_seq: args.seq,
                    verdict: "pass".into(),
                    reason: args.reason.unwrap_or_default(),
                    relayed_by: args.relayed_by.unwrap_or_default(),
                    actor: decision_actor(args.actor),
                },
            )
        }),
        command(m, Writes, |m, args: RunReject| {
            verdict(
                m,
                &args.run,
                &args.gate,
                HumanVerdict {
                    requested_seq: args.seq,
                    verdict: "fail".into(),
                    reason: args.reason.unwrap_or_default(),
                    relayed_by: args.relayed_by.unwrap_or_default(),
                    actor: decision_actor(args.actor),
                },
            )
        }),
        command(m, Writes, pick),
        command(m, Writes, resume),
        command(m, Destroys, abort),
        command(m, Writes, |m, args: RunLand| {
            Ok(RunLanded {
                node: m.node.clone(),
                landed: true,
                notes: m
                    .coordinator
                    .land(required(&args.run, "run")?, args.force)?,
            })
        }),
        command(m, Destroys, |m, args: RunDelete| {
            m.coordinator.delete_run(required(&args.run, "run")?)?;
            m.changed();
            Ok(RunDeleted {
                node: m.node.clone(),
                deleted: args.run,
            })
        }),
    ]
}

fn show(m: &Missions, args: RunShow) -> Result<RunShown, Error> {
    let id = required(&args.run, "run")?;
    let run = m.coordinator.run(id).map_err(|error| {
        if error.code == "not_found" || error.code == "invalid_input" {
            Error::new("unknown_run", format!("run {id:?} not found"))
        } else {
            error
        }
    })?;
    let mut asks = asks(&run);
    for ask in &mut asks {
        if let Some(gate) = &ask.gate {
            match m.coordinator.gate_driver_request(id, gate) {
                Ok(request) if Some(request.requested_seq) == ask.seq => {
                    ask.request = Some(request);
                }
                // A newer request or its decision can arrive after the snapshot.
                Ok(_) => (),
                Err(error) if error.code == "gate_not_waiting" => (),
                Err(error) => return Err(error),
            }
        }
    }
    let sent_back = m.coordinator.send_back(&run)?;
    let why = why(&run, &asks, sent_back.as_ref())
        .unwrap_or_else(|| format!("Run is {}.", run.status.replace('_', " ")));
    let next = next(&asks);
    let gates = m.coordinator.gates(id)?;
    let state = state(&run);
    let events = args.events.then(|| {
        let mission = m.coordinator.frozen_mission(id).ok();
        run.events
            .into_iter()
            .map(|event| {
                let step = [&event.card_id, &event.gate_id]
                    .into_iter()
                    .find(|id| !id.is_empty())
                    .and_then(|id| {
                        mission
                            .as_ref()
                            .map(|mission| mission.card_name(id).to_owned())
                    });
                RunLogEvent { event, step }
            })
            .collect()
    });
    let seats = args
        .seats
        .then(|| {
            m.coordinator
                .seats(id)
                .map(|seats| seats.into_iter().map(seat_wire).collect())
        })
        .transpose()?;
    let attempts = args
        .attempts
        .then(|| m.coordinator.attempts(id, args.step.as_deref()))
        .transpose()?;
    let evidence = args
        .evidence
        .then(|| m.coordinator.evidence(id, args.step.as_deref()))
        .transpose()?;
    Ok(RunShown {
        why,
        sent_back,
        next,
        state,
        outputs: run.outputs,
        asks,
        gates,
        events,
        seats,
        attempts,
        evidence,
        node: m.node.clone(),
    })
}

fn why(run: &RunProjection, asks: &[RunAsk], sent_back: Option<&RunSendBack>) -> Option<String> {
    if let Some(send_back) = sent_back {
        let label = if send_back.gate_id.is_empty() {
            "Sent back".to_owned()
        } else if send_back.gate_title == send_back.gate_id {
            format!("Sent back by {}", send_back.gate_id)
        } else {
            format!(
                "Sent back by {} ({})",
                send_back.gate_title, send_back.gate_id,
            )
        };
        let reason = plain_reason(&send_back.reason);
        return Some(if reason.is_empty() {
            format!("{label}.")
        } else {
            format!("{label}. {reason}")
        });
    }
    asks.first()
        .map(|ask| ask.why.as_deref().unwrap_or(&ask.what).to_owned())
        .or_else(|| ended_reason(run))
}

fn ended_reason(run: &RunProjection) -> Option<String> {
    if !run.r#final {
        return None;
    }
    run.events
        .iter()
        .rev()
        .find(|event| {
            matches!(
                event.kind.as_str(),
                "run_succeeded" | "run_failed" | "run_picked" | "run_canceled"
            )
        })
        .map(|event| plain_reason(&event.detail))
        .filter(|why| !why.is_empty())
}

fn state(run: &RunProjection) -> RunState {
    RunState {
        run_id: run.run_id.clone(),
        status: run.status.clone(),
        r#final: run.r#final,
        resume_allowed: run.resume_allowed,
        epoch: run.epoch,
        resume_policy: run.resume_policy.clone(),
        block_code: run.block_code.clone(),
        mission_slug: run.mission_slug.clone(),
        mission_id: run.mission_id.clone(),
        mission_rev: run.mission_rev,
        input_card_id: run.input_card_id.clone(),
        issue: run.issue.clone(),
        cwd: run.cwd.clone(),
        repository: run.repository.clone(),
        base: run.base.clone(),
        request_id: run.request_id.clone(),
        context_paths: run.context_paths.clone(),
        inputs: run.inputs.clone(),
        event_count: run.event_count,
        started_by: run.started_by.clone(),
        started_at: run.started_at.clone(),
        updated_at: run.updated_at.clone(),
        ended_by: run.ended_by.clone(),
    }
}

async fn wait(m: Missions, args: RunWait) -> Result<RunWaited, Error> {
    let id = required(&args.run, "run")?;
    let until = match args.until {
        WaitFor::NeedsYou => WaitUntil::NeedsYou,
        WaitFor::Final => WaitUntil::Final,
        WaitFor::AnyChange => WaitUntil::AnyChange,
    };
    let answer = m
        .coordinator
        .wait_run(
            id,
            until,
            args.since,
            Duration::from_secs(args.timeout.unwrap_or(3600)),
        )
        .await?;
    let asks = asks(&answer.run);
    let sent_back = m.coordinator.send_back(&answer.run)?;
    Ok(RunWaited {
        outcome: answer.outcome.into(),
        why: why(&answer.run, &asks, sent_back.as_ref()),
        sent_back,
        status: answer.run.status,
        cursor: answer.cursor,
        next: next(&asks),
        asks,
        outputs: answer
            .run
            .outputs
            .into_iter()
            .map(|output| RunResult {
                name: output.name,
                kind: output.kind,
                value: output.value,
            })
            .collect(),
    })
}

fn asks(run: &RunProjection) -> Vec<RunAsk> {
    let mut asks: Vec<_> = run
        .waiting_gates
        .iter()
        .map(|gate| {
            let (what, why) = if gate.pick.is_some() {
                (
                    "Pick an attempt",
                    format!("Gate {} waits for an attempt to be picked.", gate.gate_id),
                )
            } else {
                (
                    "Approve or reject",
                    format!("Gate {} waits for approval or rejection.", gate.gate_id),
                )
            };
            RunAsk {
                gate: Some(gate.gate_id.clone()),
                seq: Some(gate.requested_seq),
                step: None,
                what: what.into(),
                why: Some(why),
                commands: gate_commands(&run.run_id, gate),
                request: None,
            }
        })
        .collect();
    if run.status == "blocked"
        && let Some(event) = run
            .events
            .iter()
            .rev()
            .find(|event| event.kind == "run_blocked")
    {
        let why = match plain_reason(&event.detail) {
            why if why.is_empty() => run.block_code.replace('_', " "),
            why => why,
        };
        asks.insert(
            0,
            RunAsk {
                gate: None,
                seq: None,
                step: Some(event.card_id.clone()),
                what: run.block_code.clone(),
                why: Some(why),
                commands: resume_commands(run),
                request: None,
            },
        );
    }
    asks
}

fn next(asks: &[RunAsk]) -> Vec<String> {
    asks.iter()
        .flat_map(|ask| ask.commands.iter().cloned())
        .collect()
}

fn verdict(
    m: &Missions,
    id: &str,
    gate: &str,
    verdict: HumanVerdict,
) -> Result<DecisionRecorded, Error> {
    if verdict.requested_seq == 0 {
        return Err(bad("seq must be positive"));
    }
    m.coordinator
        .record_verdict(required(id, "run")?, required(gate, "gate")?, verdict)?;
    Ok(DecisionRecorded {
        node: m.node.clone(),
        run_id: id.into(),
    })
}

fn pick(m: &Missions, args: RunPick) -> Result<DecisionRecorded, Error> {
    if args.seq == 0 {
        return Err(bad("seq must be positive"));
    }
    if args.attempt <= 0 {
        return Err(bad("attempt must be positive"));
    }
    let id = required(&args.run, "run")?;
    let gate = required(&args.gate, "gate")?;
    m.coordinator.record_pick(
        id,
        gate,
        HumanPick {
            requested_seq: args.seq,
            attempt: args.attempt,
            reason: args.reason.unwrap_or_default(),
            relayed_by: args.relayed_by.unwrap_or_default(),
            actor: decision_actor(args.actor),
        },
    )?;
    Ok(DecisionRecorded {
        node: m.node.clone(),
        run_id: args.run,
    })
}

fn decision_actor(actor: Option<String>) -> String {
    match actor.as_deref().unwrap_or_default().trim() {
        "" => "agent:ahjo".into(),
        actor => actor.into(),
    }
}

fn resume(m: &Missions, args: RunResume) -> Result<RunChanged, Error> {
    let id = required(&args.run, "run")?;
    let run = m.coordinator.resume(
        id,
        ResumeRequest {
            mode: args.mode,
            take_fail: args.take_fail,
            grant: args.grant,
            pick_best: args.pick_best,
            reason: args.reason.unwrap_or_default(),
            actor: args.actor.unwrap_or_default(),
        },
    )?;
    Ok(RunChanged {
        node: m.node.clone(),
        run: state(&run),
    })
}

fn abort(m: &Missions, args: RunAbort) -> Result<RunChanged, Error> {
    let id = required(&args.run, "run")?;
    let actor = args.actor.as_deref().unwrap_or("agent:ahjo");
    let reason = args.reason.as_deref().unwrap_or("operator abort");
    let run = m.coordinator.abort(id, actor, reason)?;
    Ok(RunChanged {
        node: m.node.clone(),
        run: state(&run),
    })
}
