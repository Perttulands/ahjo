//! Run commands and gate decisions.

use std::time::Duration;

use ahjo_core::Error;
use ahjo_missions_contract as wire;
use ahjo_node_kit::{
    Command, Effect,
    args::{flag, maybe_text, object, text},
};
use serde_json::{Value, json};

use crate::{
    Missions,
    commands::{bad, blocking, shell_word},
};

// The run and gate verbs keep their existing surface until the run commands
// move under one noun.
const DEFINITIONS: [(&str, Effect, &str); 16] = [
    (
        "run_list",
        Effect::Reads,
        "List runs, optionally by mission.",
    ),
    (
        "run_status",
        Effect::Reads,
        "Read a run and its current events.",
    ),
    (
        "run_wait",
        Effect::Reads,
        "Wait for a run to need its driver, finish, or change.",
    ),
    (
        "run_evidence",
        Effect::Reads,
        "Read a run’s problems, briefs and files.",
    ),
    (
        "run_attempts",
        Effect::Reads,
        "Read a run’s step attempts, verdicts and picks.",
    ),
    (
        "run_gates",
        Effect::Reads,
        "List a run’s gates and their state.",
    ),
    (
        "run_seats",
        Effect::Reads,
        "Read the seats belonging to a run.",
    ),
    (
        "run_logs",
        Effect::Reads,
        "Read the public events of a run.",
    ),
    ("run_abort", Effect::Destroys, "Abort a run."),
    ("run_resume", Effect::Writes, "Resume a blocked run."),
    (
        "run_land",
        Effect::Writes,
        "Land a finished repository run.",
    ),
    (
        "run_delete",
        Effect::Destroys,
        "Delete a finished run, its kept outputs, the branches a failed step's work was kept on and the folder Ahjo made for it; land a repository run first.",
    ),
    (
        "gate_request",
        Effect::Reads,
        "Read a waiting gate’s request and routes.",
    ),
    (
        "gate_approve",
        Effect::Writes,
        "Approve a pending human gate.",
    ),
    (
        "gate_reject",
        Effect::Writes,
        "Reject a pending human gate.",
    ),
    (
        "gate_pick",
        Effect::Writes,
        "Pick an attempt at a pending human gate.",
    ),
];

pub const NAMES: [&str; DEFINITIONS.len()] = {
    let mut names = [""; DEFINITIONS.len()];
    let mut i = 0;
    while i < names.len() {
        names[i] = DEFINITIONS[i].0;
        i += 1;
    }
    names
};

pub fn commands(missions: &Missions) -> impl Iterator<Item = Command> + '_ {
    DEFINITIONS.into_iter().map(|(name, effect, description)| {
        let missions = missions.clone();
        Command::new(
            name,
            effect,
            description,
            move || schema(name),
            move |args| {
                let missions = missions.clone();
                async move { call(missions, name, args).await }
            },
        )
    })
}

fn schema(name: &str) -> Value {
    let s = json!({"type":"string"});
    let i = json!({"type":"integer"});
    let b = json!({"type":"boolean"});
    match name {
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
            json!({"run":s,"gate":s,"requested_seq":i,"reason":s,"relayed_by":s,"actor":s}),
            &["run", "gate", "requested_seq"],
        ),
        "gate_pick" => object(
            json!({"run":s,"gate":s,"requested_seq":i,"attempt":i,"reason":s,"relayed_by":s,"actor":s}),
            &["run", "gate", "requested_seq", "attempt"],
        ),
        _ => object(json!({"run":s}), &["run"]),
    }
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
fn value<T: serde::Serialize>(v: T) -> Result<Value, Error> {
    serde_json::to_value(v).map_err(|e| Error::new("internal", e.to_string()))
}

async fn call(m: Missions, name: &str, a: Value) -> Result<Value, Error> {
    let node = &m.node;
    match name {
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
                    reason: optional(&a, "reason", "")?.into(),
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
        _ => Err(bad(format!("unknown run command {name}"))),
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
