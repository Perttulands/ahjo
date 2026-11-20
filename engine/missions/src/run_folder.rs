//! Folder copies for steps that a review can send back. The verdict stays
//! pending until its chosen folder has been published.
use crate::coordinator::Control;
use crate::graph::{endpoint_node, endpoint_port, outgoing, outgoing_from_port};
use crate::run_worker::{append_live, event};
use crate::{Coordinator, RunEvent};
use ahjo_mission_model::{GATE_FAIL, GATE_IN, Mission, PublicOrigin};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::{fs, io};

const COPY: &str = "folder_attempt_started";
const PENDING: &str = "folder_verdict_pending";
pub(crate) const CONFLICT: &str = "folder_workspace_conflict";

fn review_gates<'a>(mission: &'a Mission, node: &'a str) -> impl Iterator<Item = &'a str> {
    outgoing(mission, node).filter_map(move |wire| {
        let gate = endpoint_node(&wire.to);
        (endpoint_port(&wire.to) == GATE_IN
            && mission.gate(gate).is_some()
            && outgoing_from_port(mission, gate, GATE_FAIL)
                .any(|wire| endpoint_node(&wire.to) == node))
        .then_some(gate)
    })
}

fn run_dir(c: &Coordinator, id: &str) -> PathBuf {
    c.inner.root.join("folder-attempts").join(id)
}

fn attempt_dir(c: &Coordinator, id: &str, node: &str, attempt: i64) -> PathBuf {
    run_dir(c, id)
        .join(hex::encode(Sha256::digest(node.as_bytes())))
        .join(attempt.to_string())
}

fn excluded(c: &Coordinator, workspace: &Path) -> io::Result<Vec<PathBuf>> {
    let state = c.inner.root.canonicalize()?;
    if state == workspace {
        return Err(io::Error::other(
            "a retrying step cannot use the coordinator's state directory as its run folder",
        ));
    }
    Ok(state
        .starts_with(workspace)
        .then_some(state)
        .into_iter()
        .collect())
}

pub(crate) fn prepare(
    c: &Coordinator,
    id: &str,
    mission: &Mission,
    workspace: &Path,
    control: &Control,
    node: &str,
    attempt: i64,
) -> io::Result<PathBuf> {
    let gates: Vec<_> = review_gates(mission, node).collect();
    let only_one = mission.limits.iter().any(|card| {
        (card.target == node || mission.has_input_card(&card.target))
            && card.rounds == Some(1)
            && card.when_spent == crate::limits::WHEN_SPENT_FAIL
    });
    if gates.is_empty() || only_one {
        return Ok(workspace.to_path_buf());
    }
    let _workspace = c.inner.folder_workspace.lock().unwrap();
    let source = workspace.canonicalize()?;
    let excluded = excluded(c, &source)?;
    let dir = attempt_dir(c, id, node, attempt);
    let copy = dir.join("work");
    if dir.exists() {
        let events = c.ledger(id)?.read().map_err(crate::error::to_io)?;
        if let Some(existing) = attempt_path(c, id, &events, node, attempt) {
            return Ok(existing);
        }
        crate::kept_copy::remove_tree(&dir)?;
    }
    fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
    }
    crate::folder_snapshot::prepare(&source, &copy, &dir.join("base"), &excluded)?;
    if !append_live(
        c,
        id,
        control,
        event(COPY, node, attempt, json!({"source":source,"gates":gates})),
    )? {
        crate::kept_copy::remove_tree(&dir)?;
        return Err(io::Error::new(io::ErrorKind::Interrupted, "run canceled"));
    }
    Ok(copy)
}

pub(crate) fn attempt_path(
    c: &Coordinator,
    id: &str,
    events: &[RunEvent],
    node: &str,
    attempt: i64,
) -> Option<PathBuf> {
    events
        .iter()
        .any(|e| e.kind == COPY && e.node_id == node && e.attempt == attempt)
        .then(|| attempt_dir(c, id, node, attempt).join("work"))
}

pub(crate) fn last_selection(events: &[RunEvent], node: &str) -> u64 {
    latest_selection(events, node).map_or(0, |e| e.seq)
}

fn latest_selection<'a>(events: &'a [RunEvent], node: &str) -> Option<&'a RunEvent> {
    events.iter().rev().find(|e| {
        e.kind == "gate_verdict" && e.data.get("folder_node").and_then(Value::as_str) == Some(node)
    })
}

/// A pass at a gate that could send this attempt back selects it. A best
/// pick names its selection explicitly, regardless of the verdict it had.
pub(crate) fn record_verdict(
    c: &Coordinator,
    id: &str,
    control: &Control,
    events: &[RunEvent],
    mut verdict: RunEvent,
) -> io::Result<()> {
    if verdict.kind == "gate_verdict"
        && verdict.data.get("route_port").and_then(Value::as_str) == Some("pass")
    {
        let reference = verdict.data.get("pick").or_else(|| {
            verdict
                .data
                .get("input_ref")
                .and_then(|input| input.get("origin"))
        });
        if let Some(reference) = reference
            && let Some(node) = reference.get("node_id").and_then(Value::as_str)
            && let Some(attempt) = reference.get("attempt").and_then(Value::as_i64)
            && let Some(copy) = events.iter().rev().find(|e| {
                e.kind == COPY
                    && e.node_id == node
                    && e.attempt == attempt
                    && e.data
                        .get("gates")
                        .and_then(Value::as_array)
                        .is_some_and(|gates| {
                            gates
                                .iter()
                                .any(|gate| gate.as_str() == Some(&verdict.gate_id))
                        })
            })
            && copy.seq > last_selection(events, node)
        {
            verdict.kind = PENDING.into();
            verdict
                .data
                .insert("folder_attempt_seq".into(), copy.seq.into());
            verdict
                .data
                .insert("folder_node".into(), copy.node_id.clone().into());
        }
    }
    append_live(c, id, control, verdict).map(|_| ())
}

pub(crate) fn pending(events: &[RunEvent]) -> Option<&RunEvent> {
    events.iter().find(|e| {
        e.kind == PENDING
            && !events.iter().any(|f| {
                f.kind == "gate_verdict"
                    && f.data.get("folder_verdict_seq").and_then(Value::as_u64) == Some(e.seq)
            })
    })
}

fn selection<'a>(
    events: &'a [RunEvent],
    verdict: &RunEvent,
) -> io::Result<(&'a RunEvent, PathBuf)> {
    let copy = events
        .iter()
        .find(|e| {
            e.kind == COPY
                && Some(e.seq)
                    == verdict
                        .data
                        .get("folder_attempt_seq")
                        .and_then(Value::as_u64)
        })
        .ok_or_else(|| io::Error::other("the selected attempt's folder record is missing"))?;
    let source = copy
        .data
        .get("source")
        .and_then(Value::as_str)
        .ok_or_else(|| io::Error::other("the selected attempt's run folder is missing"))?;
    Ok((copy, PathBuf::from(source)))
}

/// Publish at the scheduler boundary after all active steps have settled.
/// The pending verdict survives a refusal or a stopped coordinator.
pub(crate) fn settle(
    c: &Coordinator,
    id: &str,
    workspace: &Path,
    control: &Control,
    events: &[RunEvent],
    pending: &RunEvent,
) -> io::Result<()> {
    if let Some(node) = pending.data.get("folder_node").and_then(Value::as_str) {
        crate::run_left_seats::end(c, id, Some(node), "folder_selected")?;
    }
    let _workspace = c.inner.folder_workspace.lock().unwrap();
    let done = control.done.lock().unwrap();
    if control.canceled.load(Ordering::Acquire) || control.stopping.load(Ordering::Acquire) {
        return Ok(());
    }
    let (copy, source) = selection(events, pending)?;
    let prior = latest_selection(events, &copy.node_id).filter(|prior| prior.seq > copy.seq);
    if prior.is_some_and(|prior| {
        prior.data.get("folder_attempt_seq") != pending.data.get("folder_attempt_seq")
    }) {
        return Err(io::Error::other(format!(
            "{CONFLICT}: another gate already selected a different attempt of {}",
            copy.node_id
        )));
    }
    if prior.is_none() && workspace.canonicalize().ok().as_ref() != Some(&source) {
        return Err(io::Error::other(format!(
            "{CONFLICT}: run folder {} changed location before publishing {}",
            workspace.display(),
            copy.node_id,
        )));
    }
    if prior.is_none() {
        let dir = attempt_dir(c, id, &copy.node_id, copy.attempt);
        crate::folder_snapshot::publish(
            &source,
            &dir.join("work"),
            &dir.join("base"),
            &excluded(c, &source)?,
        )
        .map_err(|error| {
            let code = if error.kind() == io::ErrorKind::AlreadyExists {
                CONFLICT
            } else {
                "folder_workspace_publish_failed"
            };
            io::Error::other(format!(
                "{code}: publishing {} attempt {}: {error}",
                copy.node_id, copy.attempt
            ))
        })?;
    }
    let mut verdict = pending.clone();
    verdict.seq = 0;
    verdict.kind = "gate_verdict".into();
    verdict
        .data
        .insert("folder_verdict_seq".into(), pending.seq.into());
    if prior.is_some() {
        // A second gate can pass the copy that already landed. Only the
        // first accepting verdict advances the step's snapshot boundary.
        verdict.data.remove("folder_node");
    }
    c.append(id, verdict)?;
    drop(done);
    if prior.is_none() {
        discard_selected(c, id, &copy.node_id, events)?;
    }
    Ok(())
}

fn discard_selected(c: &Coordinator, id: &str, node: &str, events: &[RunEvent]) -> io::Result<()> {
    for copy in events
        .iter()
        .filter(|e| e.kind == COPY && e.node_id == node)
    {
        crate::kept_copy::remove_tree(&attempt_dir(c, id, node, copy.attempt))?;
    }
    Ok(())
}

/// Replay cleanup if the verdict was recorded just before a shutdown.
pub(crate) fn recover(c: &Coordinator, id: &str, events: &[RunEvent]) -> io::Result<()> {
    for verdict in events.iter().filter(|e| e.kind == "gate_verdict") {
        if let Some(node) = verdict.data.get("folder_node").and_then(Value::as_str) {
            let index = events.partition_point(|e| e.seq < verdict.seq);
            discard_selected(c, id, node, &events[..index])?;
        }
    }
    Ok(())
}

/// A finished or aborted run has no pending picks to retain.
pub(crate) fn discard_run(c: &Coordinator, id: &str) -> io::Result<()> {
    if run_dir(c, id).exists() {
        let events = c.ledger(id)?.read().map_err(crate::error::to_io)?;
        if let Some(verdict) = pending(&events) {
            let (copy, source) = selection(&events, verdict)?;
            let _workspace = c.inner.folder_workspace.lock().unwrap();
            let dir = attempt_dir(c, id, &copy.node_id, copy.attempt);
            crate::folder_snapshot::abandon(
                &source,
                &dir.join("base"),
                &dir.join("work"),
                &excluded(c, &source)?,
            )?;
        }
        crate::kept_copy::remove_tree(&run_dir(c, id))?;
    }
    Ok(())
}

pub(crate) fn reviewed_path(
    c: &Coordinator,
    id: &str,
    events: &[RunEvent],
    origin: Option<&PublicOrigin>,
) -> Option<PathBuf> {
    let origin = origin?;
    let last = last_selection(events, &origin.node_id);
    let boundary = events.partition_point(|e| e.seq <= last);
    attempt_path(c, id, &events[boundary..], &origin.node_id, origin.attempt)
}
