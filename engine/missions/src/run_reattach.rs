//! Resolve dispatches named by a block before scheduling new work.
use crate::coordinator::Control;
use crate::{Coordinator, OpenDispatch, RunEvent};
use ahjo_mission_model::{Mission, ResumeMode, RunStart};
use serde_json::json;
use std::{io, path::Path};

fn request(
    c: &Coordinator,
    id: &str,
    mission: &Mission,
    dispatch: &RunEvent,
    control: Option<&Control>,
) -> Result<OpenDispatch, String> {
    let step = mission
        .step(&dispatch.card_id)
        .cloned()
        .ok_or_else(|| "step missing from snapshot".to_owned())?;
    Ok(OpenDispatch {
        run_id: id.into(),
        dispatch_id: dispatch
            .data
            .get("dispatch_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .into(),
        step_id: dispatch.card_id.clone(),
        seat_id: dispatch.seat_id.clone(),
        attempt: dispatch.attempt,
        step,
        dispatch_data: dispatch.data.clone(),
        artifact_dir: crate::run_capture::attempt_dir(c, id, &dispatch.card_id, dispatch.attempt)
            .map_err(|e| e.to_string())?,
        canceled: control.map(|control| control.canceled.clone()),
        stopping: control.map(|control| control.stopping.clone()),
        final_judge: mission.gates.iter().any(|gate| {
            crate::graph::judge_chain(mission, &gate.id)
                .last()
                .is_some_and(|judge| judge.id == dispatch.card_id)
        }),
        limits: Default::default(),
    })
}

/// Stage completed evidence from a blocked run without changing its ledger.
pub(crate) fn stage_completed(c: &Coordinator, id: &str, mission: &Mission, dispatch: &RunEvent) {
    if let Ok(request) = request(c, id, mission, dispatch, None)
        && let Some(result) = c.inner.executor.recover_completed(request.clone())
        && valid(&request.step, &result)
    {
        c.inner
            .recovered
            .lock()
            .unwrap()
            .insert((id.into(), request.dispatch_id), result);
    }
}

pub(crate) fn recover_completed(
    c: &Coordinator,
    id: &str,
    mission: &Mission,
    req: &RunStart,
    workspace: &Path,
    control: &Control,
    dispatch: &RunEvent,
) -> io::Result<bool> {
    let Ok(request) = request(c, id, mission, dispatch, Some(control)) else {
        return Ok(false);
    };
    let Some(result) = c.inner.executor.recover_completed(request.clone()) else {
        return Ok(false);
    };
    if !valid(&request.step, &result) {
        return Ok(false);
    }
    keep(c, id, mission, req, workspace, control, dispatch, result)?;
    Ok(true)
}

fn valid(step: &ahjo_mission_model::Step, result: &crate::StepResult) -> bool {
    result.status == "done"
        && step
            .outputs
            .iter()
            .all(|p| result.outputs.contains_key(&p.id))
        && result
            .outputs
            .keys()
            .all(|id| step.outputs.iter().any(|p| &p.id == id))
}

#[allow(clippy::too_many_arguments)]
fn keep(
    c: &Coordinator,
    id: &str,
    mission: &Mission,
    req: &RunStart,
    workspace: &Path,
    control: &Control,
    dispatch: &RunEvent,
    result: crate::StepResult,
) -> io::Result<()> {
    let dispatch_id = dispatch
        .data
        .get("dispatch_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let request = request(c, id, mission, dispatch, Some(control)).map_err(io::Error::other)?;
    let prior = c.ledger(id)?.read().map_err(crate::error::to_io)?;
    for replay in c
        .inner
        .executor
        .completed_events(&request)
        .map_err(io::Error::other)?
    {
        let already_recorded = prior.iter().any(|event| {
            event.card_id == dispatch.card_id
                && event.attempt == dispatch.attempt
                && event.kind == replay.kind
                && event.data.get("dispatch_id") == replay.data.get("dispatch_id")
                && (replay.kind == "seat_dispatch"
                    || replay.kind == "workspace_turn"
                    || event.data.get("seat_event_seq").is_some()
                        && event.data.get("seat_event_seq") == replay.data.get("seat_event_seq"))
        });
        if already_recorded {
            continue;
        }
        let mut record = crate::run_worker::event(
            &replay.kind,
            &dispatch.card_id,
            dispatch.attempt,
            serde_json::Value::Object(replay.data),
        );
        record.seat_id = replay.seat_id;
        c.append(id, record)?;
    }
    let mut seat = crate::run_worker::event(
        "seat_result",
        &dispatch.card_id,
        dispatch.attempt,
        json!({"dispatch_id":dispatch_id,"status":"ok","sentinel":{"run_id":id,"status":"ok"}}),
    );
    seat.seat_id = dispatch.seat_id.clone();
    if !c
        .ledger(id)?
        .read()
        .map_err(crate::error::to_io)?
        .iter()
        .any(|e| {
            e.kind == "seat_result"
                && e.data.get("dispatch_id").and_then(|v| v.as_str()) == Some(dispatch_id)
        })
    {
        c.append(id, seat)?;
    }
    let events = c.ledger(id)?.read().map_err(crate::error::to_io)?;
    let reason = events
        .iter()
        .rev()
        .find(|e| {
            e.kind == "card_started"
                && e.card_id == dispatch.card_id
                && e.attempt == dispatch.attempt
        })
        .and_then(|e| e.data.get("reason"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let mut output = crate::run_worker::event(
        "card_output",
        &dispatch.card_id,
        dispatch.attempt,
        json!({"status":result.status,"text":result.text,"outputs":result.outputs,"report_ref":format!("lab://{id}/{}/report.md",dispatch.card_id)}),
    );
    if reason == "judge" {
        output.data.insert("reason".into(), "judge".into());
    }
    if reason == "judge" {
        crate::run_workspace::publish_output(c, id, req, workspace, control, output)
    } else {
        crate::run_capture::publish(c, id, mission, req, workspace, control, output)
    }
}

/// Record what a reattached dispatch's seats spent, as a live one records
/// it when it ends, so later attempts and the mission's card count it.
fn record_tokens(
    c: &Coordinator,
    id: &str,
    control: &Control,
    dispatch: &RunEvent,
    limits: &crate::AttemptLimits,
) -> io::Result<()> {
    for (dispatch_id, tokens) in limits.counted_tokens() {
        let mut usage = crate::run_worker::event(
            "token_usage",
            &dispatch.card_id,
            dispatch.attempt,
            json!({"dispatch_id":dispatch_id,"tokens":tokens}),
        );
        usage.seat_id.clone_from(&dispatch.seat_id);
        crate::run_worker::append_live(c, id, control, usage)?;
    }
    Ok(())
}

/// A limit ran out while a reattached dispatch worked: its attempt is cut
/// off as a live one is, and the run picks or keeps the stop for its worker.
fn cut_off(
    c: &Coordinator,
    id: &str,
    mission: &Mission,
    control: &Control,
    dispatch: &RunEvent,
    limits: &crate::AttemptLimits,
) -> io::Result<()> {
    let error = crate::run_job::cut_off(
        c,
        id,
        mission,
        control,
        &dispatch.card_id,
        dispatch.attempt,
        limits,
    )?;
    let Some(reached) = crate::run_limit::of(&error) else {
        return Err(error);
    };
    if let Some(stop) = crate::run_limit::pick_or_stop(c, id, control, mission, reached)? {
        let mut held = control.limit.lock().unwrap();
        if held.as_ref().is_none_or(|old| stop.fail && !old.fail) {
            *held = Some(stop);
        }
    }
    Ok(())
}

pub(crate) struct ResumeDispatches<'a> {
    pub blocked: &'a RunEvent,
    pub mode: ResumeMode,
}

pub(crate) fn resolve(
    c: &Coordinator,
    id: &str,
    mission: &Mission,
    req: &RunStart,
    workspace: &Path,
    control: &Control,
    resume: ResumeDispatches<'_>,
) -> io::Result<bool> {
    let ResumeDispatches { blocked, mode } = resume;
    let events = c.ledger(id)?.read().map_err(crate::error::to_io)?;
    let named: Vec<_> = blocked
        .data
        .get("open_dispatches")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|v| v.get("dispatch_id").and_then(|v| v.as_str()))
        .collect();
    let mut failed = Vec::new();
    let mut reasons = Vec::new();
    for dispatch in crate::run_open_dispatch::unresolved(&events) {
        if control.stopping.load(std::sync::atomic::Ordering::Acquire)
            || control.canceled.load(std::sync::atomic::Ordering::Acquire)
        {
            return Ok(true);
        }
        // Keeping a corrected answer also resolves its correction dispatch.
        if !crate::run_open_dispatch::unresolved(
            &c.ledger(id)?.read().map_err(crate::error::to_io)?,
        )
        .iter()
        .any(|open| open.seq == dispatch.seq)
        {
            continue;
        }
        let dispatch_id = dispatch
            .data
            .get("dispatch_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if mode == ResumeMode::Redispatch || !named.contains(&dispatch_id) {
            c.inner
                .recovered
                .lock()
                .unwrap()
                .remove(&(id.to_owned(), dispatch_id.to_owned()));
            let mut abandoned = crate::run_worker::event(
                "seat_result",
                &dispatch.card_id,
                dispatch.attempt,
                json!({"dispatch_id":dispatch_id,"seat_id":dispatch.seat_id,"status":"abandoned"}),
            );
            abandoned.seat_id = dispatch.seat_id.clone();
            c.append(id, abandoned)?;
            continue;
        }
        // A dispatch still working is held to its step's Limit cards, which
        // count what it worked before the restart.
        let mut limits = None;
        let result = request(c, id, mission, &dispatch, Some(control)).and_then(|mut request| {
            let staged = c
                .inner
                .recovered
                .lock()
                .unwrap()
                .remove(&(id.to_owned(), request.dispatch_id.clone()));
            let result = match staged
                .or_else(|| c.inner.executor.recover_completed(request.clone()))
            {
                Some(result) => result,
                None => {
                    let (held, _timer) =
                        crate::run_job::attempt_limits(c, id, mission, control, &dispatch.card_id)
                            .map_err(|e| e.to_string())?;
                    request.limits = held.clone();
                    limits = Some(held);
                    c.inner
                        .executor
                        .reattach(request.clone())
                        .map_err(|error| error.message)?
                }
            };
            if !valid(&request.step, &result) {
                return Err("reattached step returned invalid outputs".into());
            }
            Ok(result)
        });
        if control.stopping.load(std::sync::atomic::Ordering::Acquire)
            || control.canceled.load(std::sync::atomic::Ordering::Acquire)
        {
            return Ok(true);
        }
        if let Some(limits) = &limits {
            record_tokens(c, id, control, &dispatch, limits)?;
            if limits.is_spent() {
                cut_off(c, id, mission, control, &dispatch, limits)?;
                continue;
            }
        }
        match result {
            Ok(result) => {
                keep(c, id, mission, req, workspace, control, &dispatch, result)?;
            }
            Err(reason) => {
                let mut error = crate::run_worker::event(
                    "error",
                    &dispatch.card_id,
                    dispatch.attempt,
                    json!({"code":"dispatch_reattach_failed","reason":reason,"dispatch_id":dispatch_id,"recoverable":true}),
                );
                error.seat_id = dispatch.seat_id.clone();
                c.append(id, error)?;
                reasons.push(format!("{}: {reason}", dispatch.card_id));
                let mut record = dispatch.data.clone();
                record.insert("step_id".into(), dispatch.card_id.clone().into());
                failed.push(record);
            }
        }
    }
    if !failed.is_empty() {
        let (code, reason) = block_reason(&reasons);
        c.append(id, RunEvent { kind:"run_blocked".into(), timestamp:crate::run_storage::now(), data:json!({"code":code,"reason":reason,"resume_allowed":true,"resume_policy":"explicit","open_dispatches":failed}).as_object().unwrap().clone(), card_id: if failed.len() == 1 { failed[0]["step_id"].as_str().unwrap_or("").into() } else { String::new() }, ..Default::default() })?;
        return Ok(false);
    }
    Ok(true)
}

/// Why a reattach blocks the run. A seat that stopped short again, or whose
/// terminal or session is gone, keeps its own code and words, so the way on
/// offered fits it (an ended session can only run the step again); anything
/// else is a reattach failure.
fn block_reason(reasons: &[String]) -> (&'static str, String) {
    match reasons {
        [one]
            if matches!(
                crate::run_worker::error_code(one),
                "seat_idle_without_sentinel" | "seat_stuck"
            ) || one.contains("seat_gone")
                || one.contains("session ended") =>
        {
            let detail = one.split_once(": ").map_or(one.as_str(), |(_, rest)| rest);
            let code = if detail.contains("seat_gone") || detail.contains("session ended") {
                "seat_gone"
            } else {
                crate::run_worker::error_code(detail)
            };
            (code, detail.to_owned())
        }
        // Each reason without the step id in front; the block names the
        // step when there is one.
        _ => (
            "dispatch_reattach_failed",
            format!(
                "could not go on after resuming: {}",
                reasons
                    .iter()
                    .map(|one| one.split_once(": ").map_or(one.as_str(), |(_, rest)| rest))
                    .collect::<Vec<_>>()
                    .join("; ")
            ),
        ),
    }
}

#[cfg(test)]
mod block_tests {
    #[test]
    fn an_ended_session_blocks_as_gone_in_its_own_words() {
        let (code, reason) =
            super::block_reason(&["fmn_work: the agent's session ended before it finished".into()]);
        assert_eq!(code, "seat_gone");
        assert_eq!(reason, "the agent's session ended before it finished");
        let (code, _) = super::block_reason(&["fmn_work: something else".into()]);
        assert_eq!(code, "dispatch_reattach_failed");
    }
}
