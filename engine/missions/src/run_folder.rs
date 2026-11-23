//! A stable folder per step and snapshots of its publishable attempts.
use crate::coordinator::Control;
use crate::run_worker::{append_live, event};
use crate::{Coordinator, RunEvent};
use ahjo_mission_model::{Mission, PublicOrigin};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::{fs, io};

const COPY: &str = "folder_attempt_started";
const PENDING: &str = "folder_verdict_pending";
pub(crate) const CONFLICT: &str = "folder_workspace_conflict";

fn run_dir(c: &Coordinator, id: &str) -> PathBuf {
    c.inner.root.join("folder-attempts").join(id)
}

fn attempt_dir(c: &Coordinator, id: &str, node: &str, attempt: i64) -> PathBuf {
    step_dir(c, id, node).join(attempt.to_string())
}

fn step_dir(c: &Coordinator, id: &str, node: &str) -> PathBuf {
    run_dir(c, id).join(hex::encode(Sha256::digest(node.as_bytes())))
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
    (mission, review): (&Mission, bool),
    workspace: &Path,
    control: &Control,
    node: &str,
    attempt: i64,
) -> io::Result<PathBuf> {
    let gates = if review {
        crate::run_workspace::review_gates(mission, node)
    } else {
        Vec::new()
    };
    let _workspace = c.inner.folder_workspace.lock().unwrap();
    let source = workspace.canonicalize()?;
    let excluded = excluded(c, &source)?;
    let step = step_dir(c, id, node);
    let work = step.join("work");
    let baseline = step.join("base");
    let dir = attempt_dir(c, id, node, attempt);
    if dir.exists() {
        let events = c.ledger(id)?.read().map_err(crate::error::to_io)?;
        if events
            .iter()
            .any(|e| e.kind == COPY && e.card_id == node && e.attempt == attempt)
        {
            return Ok(if work.exists() {
                work
            } else {
                dir.join("work")
            });
        }
        crate::kept_copy::remove_tree(&dir)?;
    }
    fs::create_dir_all(&step)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&step, fs::Permissions::from_mode(0o700))?;
    }
    if baseline.exists() && !work.exists() {
        crate::kept_copy::remove_tree(&baseline)?;
    }
    if !baseline.exists() {
        let staged_base = step.join(".initial-base");
        let staged_work = step.join(".initial-work");
        crate::kept_copy::remove_tree(&staged_base)?;
        crate::kept_copy::remove_tree(&staged_work)?;
        if work.exists() {
            // The previous publication left the live process in this directory.
            // Apply subsequent changes from other steps without attributing stale
            // files in the live workspace to this new round.
            let checkpoint = step.join("checkpoint");
            let incoming = step.join(".incoming");
            if !checkpoint.exists() {
                return Err(io::Error::other(
                    "the step's previous workspace checkpoint is missing",
                ));
            }
            if !incoming.exists() {
                crate::folder_snapshot::snapshot(&source, &incoming, &excluded)?;
            }
            crate::folder_snapshot::publish_changes(&work, &incoming, &checkpoint, &[]).map_err(
                |error| io::Error::other(format!("{CONFLICT}: updating {node} workspace: {error}")),
            )?;
            fs::rename(&incoming, &baseline)?;
            crate::kept_copy::remove_tree(&checkpoint)?;
            crate::kept_copy::remove_tree(&checkpoint.with_extension("publication"))?;
        } else {
            crate::folder_snapshot::prepare(&source, &staged_work, &staged_base, &excluded)?;
            fs::rename(staged_base, &baseline)?;
            fs::rename(staged_work, &work)?;
        }
    }
    fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
    }
    crate::folder_snapshot::snapshot(&baseline, &dir.join("base"), &[])?;
    if !gates.is_empty()
        && !append_live(
            c,
            id,
            control,
            event(COPY, node, attempt, json!({"source":source,"gates":gates})),
        )?
    {
        crate::kept_copy::remove_tree(&dir)?;
        return Err(io::Error::new(io::ErrorKind::Interrupted, "run canceled"));
    }
    Ok(work)
}

/// Seal the completed attempt before a gate can examine or select it.
pub(crate) fn snapshot(
    c: &Coordinator,
    id: &str,
    control: &Control,
    node: &str,
    attempt: i64,
) -> io::Result<()> {
    let _workspace = c.inner.folder_workspace.lock().unwrap();
    let events = c.ledger(id)?.read().map_err(crate::error::to_io)?;
    let recorded = events
        .iter()
        .any(|e| e.kind == COPY && e.card_id == node && e.attempt == attempt);
    let dir = attempt_dir(c, id, node, attempt);
    if !recorded && !dir.join("base").exists() {
        return Ok(());
    }
    let target = dir.join("work");
    if !target.exists() {
        let stage = dir.join(".snapshot");
        if stage.exists() {
            crate::kept_copy::remove_tree(&stage)?;
        }
        crate::folder_snapshot::snapshot(&step_dir(c, id, node).join("work"), &stage, &[])?;
        if control.canceled.load(Ordering::Acquire) || control.stopping.load(Ordering::Acquire) {
            crate::kept_copy::remove_tree(&stage)?;
            return Err(io::Error::new(io::ErrorKind::Interrupted, "run canceled"));
        }
        fs::rename(stage, &target)?;
    }
    if !recorded {
        if crate::folder_snapshot::same(&dir.join("base"), &target)? {
            checkpoint_work(c, id, node)?;
            crate::kept_copy::remove_tree(&step_dir(c, id, node).join("base"))?;
            crate::kept_copy::remove_tree(&dir)?;
            return Ok(());
        }
        let source = events
            .first()
            .and_then(|event| event.data.get("cwd"))
            .and_then(Value::as_str)
            .ok_or_else(|| io::Error::other("the run folder is missing"))?;
        if !append_live(
            c,
            id,
            control,
            event(COPY, node, attempt, json!({"source":source,"gates":[]})),
        )? {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "run canceled"));
        }
    }
    Ok(())
}

/// A judge's folder is its own: what it wrote there (a critic's notes and
/// frames) is never published, so it can neither reach the run folder nor
/// block publishing the attempt it reviewed. The run folder as the judge's
/// attempt found it becomes its checkpoint, so the judge's next visit brings
/// in what the run folder gained since and keeps the judge's own files.
pub(crate) fn keep_judge_files(
    c: &Coordinator,
    id: &str,
    node: &str,
    attempt: i64,
) -> io::Result<()> {
    let _workspace = c.inner.folder_workspace.lock().unwrap();
    let step = step_dir(c, id, node);
    let dir = attempt_dir(c, id, node, attempt);
    let found = dir.join("base");
    if !step.join("work").exists() || !found.exists() {
        // Already kept, or a run started before stable workspaces.
        return Ok(());
    }
    let checkpoint = step.join("checkpoint");
    crate::kept_copy::remove_tree(&checkpoint.with_extension("publication"))?;
    crate::kept_copy::remove_tree(&checkpoint)?;
    fs::rename(found, &checkpoint)?;
    crate::kept_copy::remove_tree(&step.join("base"))?;
    crate::kept_copy::remove_tree(&dir)
}

/// Whether this attempt has a durable unreviewed publication to finish.
pub(crate) fn unreviewed(c: &Coordinator, id: &str, node: &str, attempt: i64) -> io::Result<bool> {
    let events = c.ledger(id)?.read().map_err(crate::error::to_io)?;
    Ok(events.iter().any(|event| {
        event.kind == COPY
            && event.card_id == node
            && event.attempt == attempt
            && event
                .data
                .get("gates")
                .and_then(Value::as_array)
                .is_some_and(Vec::is_empty)
    }))
}

/// Publish a step that has no gate selecting one of its attempts.
pub(crate) fn publish_unreviewed(
    c: &Coordinator,
    id: &str,
    control: &Control,
    node: &str,
    attempt: i64,
) -> io::Result<()> {
    snapshot(c, id, control, node, attempt)?;
    let _workspace = c.inner.folder_workspace.lock().unwrap();
    let events = c.ledger(id)?.read().map_err(crate::error::to_io)?;
    let Some(start) = events
        .iter()
        .find(|e| e.kind == COPY && e.card_id == node && e.attempt == attempt)
    else {
        return Ok(());
    };
    if start
        .data
        .get("gates")
        .and_then(Value::as_array)
        .is_some_and(|gates| !gates.is_empty())
    {
        return Ok(());
    }
    let done = events
        .iter()
        .any(|e| e.kind == "folder_attempt_published" && e.card_id == node && e.attempt == attempt);
    if !done {
        let _done = control.done.lock().unwrap();
        if control.canceled.load(Ordering::Acquire) || control.stopping.load(Ordering::Acquire) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "run canceled"));
        }
        let source = start
            .data
            .get("source")
            .and_then(Value::as_str)
            .ok_or_else(|| io::Error::other("the attempt's run folder is missing"))?;
        let source = Path::new(source);
        let dir = attempt_dir(c, id, node, attempt);
        crate::folder_snapshot::publish_changes(
            source,
            &dir.join("work"),
            &dir.join("base"),
            &excluded(c, source)?,
        )
        .map_err(|error| {
            let code = if error.kind() == io::ErrorKind::AlreadyExists {
                CONFLICT
            } else {
                "folder_workspace_publish_failed"
            };
            io::Error::other(format!(
                "{code}: publishing {node} attempt {attempt}: {error}"
            ))
        })?;
        checkpoint_work(c, id, node)?;
        c.append(
            id,
            event("folder_attempt_published", node, attempt, json!({})),
        )?;
    }
    if !events
        .iter()
        .any(|e| e.kind == COPY && e.card_id == node && e.seq > start.seq)
    {
        crate::kept_copy::remove_tree(&step_dir(c, id, node).join("base"))?;
    }
    Ok(())
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
        .any(|e| e.kind == COPY && e.card_id == node && e.attempt == attempt)
        .then(|| attempt_dir(c, id, node, attempt).join("work"))
}

pub(crate) fn last_selection(events: &[RunEvent], node: &str) -> u64 {
    latest_selection(events, node).map_or(0, |e| e.seq)
}

fn latest_selection<'a>(events: &'a [RunEvent], node: &str) -> Option<&'a RunEvent> {
    events.iter().rev().find(|e| {
        e.kind == "gate_verdict"
            && e.data.get("folder_step_id").and_then(Value::as_str) == Some(node)
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
        let reference = verdict
            .data
            .get("pick")
            .map(|pick| (pick, "step_id"))
            .or_else(|| {
                verdict
                    .data
                    .get("input_ref")
                    .and_then(|input| input.get("origin"))
                    .map(|origin| (origin, "card_id"))
            });
        if let Some((reference, identity)) = reference
            && let Some(node) = reference.get(identity).and_then(Value::as_str)
            && let Some(attempt) = reference.get("attempt").and_then(Value::as_i64)
            && let Some(copy) = events.iter().rev().find(|e| {
                e.kind == COPY
                    && e.card_id == node
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
                .insert("folder_step_id".into(), copy.card_id.clone().into());
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
    let _workspace = c.inner.folder_workspace.lock().unwrap();
    let done = control.done.lock().unwrap();
    if control.canceled.load(Ordering::Acquire) || control.stopping.load(Ordering::Acquire) {
        return Ok(());
    }
    let (copy, source) = selection(events, pending)?;
    let prior = latest_selection(events, &copy.card_id).filter(|prior| prior.seq > copy.seq);
    if prior.is_some_and(|prior| {
        prior.data.get("folder_attempt_seq") != pending.data.get("folder_attempt_seq")
    }) {
        return Err(io::Error::other(format!(
            "{CONFLICT}: another gate already selected a different attempt of {}",
            copy.card_id
        )));
    }
    if prior.is_none() && workspace.canonicalize().ok().as_ref() != Some(&source) {
        return Err(io::Error::other(format!(
            "{CONFLICT}: run folder {} changed location before publishing {}",
            workspace.display(),
            copy.card_id,
        )));
    }
    if prior.is_none() {
        let dir = attempt_dir(c, id, &copy.card_id, copy.attempt);
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
                copy.card_id, copy.attempt
            ))
        })?;
    }
    if prior.is_none() {
        checkpoint_work(c, id, &copy.card_id)?;
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
        verdict.data.remove("folder_step_id");
    }
    c.append(id, verdict)?;
    drop(done);
    if prior.is_none() {
        discard_selected(c, id, &copy.card_id, events)?;
        crate::kept_copy::remove_tree(&step_dir(c, id, &copy.card_id).join("base"))?;
    }
    Ok(())
}

/// Remember the live cwd at the publication boundary for a later visit.
fn checkpoint_work(c: &Coordinator, id: &str, node: &str) -> io::Result<()> {
    let step = step_dir(c, id, node);
    let checkpoint = step.join("checkpoint");
    let staged = step.join(".checkpoint");
    if !step.join("work").exists() {
        // A run started before stable workspaces used an attempt directory.
        return Ok(());
    }
    crate::kept_copy::remove_tree(&checkpoint.with_extension("publication"))?;
    crate::kept_copy::remove_tree(&staged)?;
    crate::folder_snapshot::snapshot(&step.join("work"), &staged, &[])?;
    crate::kept_copy::remove_tree(&checkpoint)?;
    fs::rename(staged, checkpoint)
}

fn discard_selected(c: &Coordinator, id: &str, node: &str, events: &[RunEvent]) -> io::Result<()> {
    for copy in events
        .iter()
        .filter(|e| e.kind == COPY && e.card_id == node)
    {
        crate::kept_copy::remove_tree(&attempt_dir(c, id, node, copy.attempt))?;
    }
    Ok(())
}

/// Replay cleanup if the verdict was recorded just before a shutdown.
pub(crate) fn recover(c: &Coordinator, id: &str, events: &[RunEvent]) -> io::Result<()> {
    for verdict in events.iter().filter(|e| e.kind == "gate_verdict") {
        if let Some(node) = verdict.data.get("folder_step_id").and_then(Value::as_str) {
            let index = events.partition_point(|e| e.seq < verdict.seq);
            discard_selected(c, id, node, &events[..index])?;
            if !events
                .iter()
                .any(|e| e.kind == "card_started" && e.card_id == node && e.seq > verdict.seq)
            {
                crate::kept_copy::remove_tree(&step_dir(c, id, node).join("base"))?;
            }
        }
    }
    Ok(())
}

/// A finished or aborted run has no pending picks to retain.
pub(crate) fn discard_run(c: &Coordinator, id: &str, keep_sessions: bool) -> io::Result<()> {
    if run_dir(c, id).exists() {
        let events = c.ledger(id)?.read().map_err(crate::error::to_io)?;
        if let Some(verdict) = pending(&events) {
            let (copy, source) = selection(&events, verdict)?;
            let _workspace = c.inner.folder_workspace.lock().unwrap();
            let dir = attempt_dir(c, id, &copy.card_id, copy.attempt);
            crate::folder_snapshot::abandon(
                &source,
                &dir.join("base"),
                &dir.join("work"),
                &excluded(c, &source)?,
            )?;
        }
        // An unreviewed publication may have stopped after moving files.
        // Roll it back before dropping its journal, using its own operation plan.
        for copy in events.iter().filter(|event| {
            event.kind == COPY
                && event
                    .data
                    .get("gates")
                    .and_then(Value::as_array)
                    .is_some_and(Vec::is_empty)
        }) {
            let Some(source) = copy.data.get("source").and_then(Value::as_str) else {
                return Err(io::Error::other("the attempt's run folder is missing"));
            };
            let source = Path::new(source);
            let dir = attempt_dir(c, id, &copy.card_id, copy.attempt);
            crate::folder_snapshot::abandon_changes(
                source,
                &dir.join("base"),
                &dir.join("work"),
                &excluded(c, source)?,
            )?;
        }
        if keep_sessions {
            for step in fs::read_dir(run_dir(c, id))? {
                let step = step?.path();
                if !step.join("work").exists() {
                    // Older runs placed the live session inside an attempt.
                    continue;
                }
                let checkpoint = step.join("checkpoint");
                let incoming = step.join(".incoming");
                if checkpoint.exists() && incoming.exists() {
                    crate::folder_snapshot::abandon_changes(
                        &step.join("work"),
                        &checkpoint,
                        &incoming,
                        &[],
                    )?;
                }
                for entry in fs::read_dir(step)? {
                    let entry = entry?;
                    if entry.file_name() != "work" {
                        crate::kept_copy::remove_tree(&entry.path())?;
                    }
                }
            }
        } else {
            crate::kept_copy::remove_tree(&run_dir(c, id))?;
        }
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
    let last = last_selection(events, &origin.card_id);
    let boundary = events.partition_point(|e| e.seq <= last);
    attempt_path(c, id, &events[boundary..], &origin.card_id, origin.attempt)
}
