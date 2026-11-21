//! Recover pending turn publications and clean abandoned worktrees.
use crate::coordinator::Control;
use crate::run_worker::{append_live, event};
use crate::run_workspace::{recorded_path, remove, settle_output, snapshot_path, stable_path};
use crate::{Coordinator, RunEvent, Workspaces, Worktree};
use ahjo_mission_model::{Mission, RunStart, kept_branch};
use serde_json::json;
use std::{
    io,
    path::{Path, PathBuf},
};

/// Retire unaccepted attempts after executors have stopped, preserving pending
/// outputs for publication on resume. Also retries cleanup after a crash.
/// What an attempt that failed or was stopped left in its checkout is kept
/// on a branch of its own first; what is said of each kept now is returned.
pub(crate) fn cleanup(
    c: &Coordinator,
    id: &str,
    mission: &Mission,
    req: &RunStart,
    workspace: &Path,
    control: &Control,
) -> io::Result<Vec<String>> {
    let mut kept = Vec::new();
    if req.repository.is_empty() {
        return Ok(kept);
    }
    let _guard = control.workspace.lock().unwrap();
    let events = c.ledger(id)?.read().map_err(crate::error::to_io)?;
    for start in events
        .iter()
        .filter(|e| e.kind == "workspace_started" || e.kind == "workspace_settled")
    {
        let stable = start.data.get("stable").and_then(|v| v.as_bool()) == Some(true)
            || events.iter().any(|event| {
                event.kind == "workspace_started"
                    && event.card_id == start.card_id
                    && event.attempt == start.attempt
                    && event.data.get("stable").and_then(|v| v.as_bool()) == Some(true)
            });
        let same = |e: &&RunEvent| e.card_id == start.card_id && e.attempt == start.attempt;
        let pending = events.iter().filter(same).any(|e| {
            e.kind == "card_output_pending"
                || (e.kind == "output_capture_pending"
                    && !crate::run_capture::discarded(&events, e))
        });
        let settled = events
            .iter()
            .filter(same)
            .any(|e| e.kind == "workspace_settled");
        if !pending || settled || control.canceled.load(std::sync::atomic::Ordering::Acquire) {
            let path = recorded_path(c, id, &start.card_id, start.attempt, &events);
            if !settled {
                kept.extend(keep(c, id, mission, &events, start, &path)?);
            }
            if !stable {
                remove(workspace, &path)?;
            }
        }
    }
    Ok(kept)
}

/// Once the run's seats have stopped, release its stable local worktrees.
/// A kept session needs its working directory to remain accessible.
pub(crate) fn release(
    c: &Coordinator,
    id: &str,
    workspace: &Path,
    keep_sessions: bool,
) -> io::Result<()> {
    let events = c.ledger(id)?.read().map_err(crate::error::to_io)?;
    let repository = events
        .first()
        .and_then(|e| e.data.get("repository"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if repository.is_empty() {
        return Ok(());
    }
    let checkout = events
        .first()
        .and_then(|e| e.data.get("checkout"))
        .and_then(|v| v.as_str())
        .map(PathBuf::from);
    let git_root = if workspace.exists() {
        Some(workspace)
    } else {
        checkout.as_deref().filter(|p| p.exists())
    };
    let mut released = std::collections::HashSet::new();
    for start in events.iter().filter(|event| {
        event.kind == "workspace_started"
            && event.data.get("stable").and_then(|v| v.as_bool()) == Some(true)
    }) {
        if let Some(git_root) = git_root {
            remove(
                git_root,
                &snapshot_path(c, id, &start.card_id, start.attempt),
            )?;
        }
        if keep_sessions || !released.insert(start.card_id.clone()) {
            continue;
        }
        if start.data.get("placed").and_then(|v| v.as_bool()) == Some(true) {
            let field = |name| start.data.get(name).and_then(|v| v.as_str()).unwrap_or("");
            let turn = crate::TurnWorkspace {
                run_id: id.into(),
                step_id: start.card_id.clone(),
                attempt: start.attempt,
                repository: repository.into(),
                commit: field("base").into(),
            };
            c.inner
                .executor
                .release_turn(field("environment"), &turn)
                .map_err(crate::error::to_io)?;
        } else if let Some(git_root) = git_root {
            remove(git_root, &stable_path(c, id, &start.card_id))?;
        }
    }
    Ok(())
}

/// Commit what the attempt left in its checkout, files it never committed
/// included, and keep it on a [`kept_branch`], so removing the checkout loses
/// nothing an agent wrote. The branch goes when the run is deleted.
fn keep(
    c: &Coordinator,
    id: &str,
    mission: &Mission,
    events: &[RunEvent],
    start: &RunEvent,
    path: &Path,
) -> io::Result<Option<String>> {
    let (node, attempt) = (&start.card_id, start.attempt);
    let ours = |e: &&RunEvent| e.card_id == *node && e.attempt == attempt;
    if !path.exists()
        || events
            .iter()
            .filter(ours)
            .any(|e| e.kind == "workspace_kept")
    {
        return Ok(None);
    }
    let Some(base) = (events.iter().filter(ours))
        .find(|e| e.kind == "workspace_started")
        .and_then(|e| e.data.get("base"))
        .and_then(|v| v.as_str())
    else {
        return Ok(None);
    };
    let tree = Worktree {
        path: path.to_path_buf(),
        checkout: PathBuf::new(),
        branch: String::new(),
        base: base.to_owned(),
    };
    Workspaces::new(c.inner.root.join("worktrees"), Default::default())
        .commit_turn(&tree, id, node, attempt)
        .map_err(crate::error::to_io)?;
    let head = git(path, &["rev-parse", "HEAD"])?;
    if head == base {
        return Ok(None);
    }
    let step = (mission.step(node).map(|f| f.title.as_str()))
        .or_else(|| mission.gate(node).map(|g| g.title.as_str()))
        .filter(|title| !title.is_empty())
        .unwrap_or(node);
    let branch = kept_branch(id, step, attempt);
    git(path, &["branch", "--force", &branch, &head])?;
    let repository = events[0]
        .data
        .get("repository")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let detail = format!(
        "What {step} wrote in attempt {attempt} is kept on branch {branch}{}.",
        if repository.is_empty() {
            String::new()
        } else {
            format!(" of {repository}")
        }
    );
    // Written directly: a stopped run still records where its work went
    // before its end, which the worker writes after this.
    c.append(
        id,
        event(
            "workspace_kept",
            node,
            attempt,
            json!({"commit":head,"branch":branch,"detail":detail}),
        ),
    )?;
    Ok(Some(detail))
}

fn git(cwd: &Path, args: &[&str]) -> io::Result<String> {
    let output = std::process::Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "git: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

pub(crate) fn recover_pending(
    c: &Coordinator,
    id: &str,
    mission: &Mission,
    req: &RunStart,
    workspace: &Path,
    control: &Control,
) -> io::Result<()> {
    cleanup(c, id, mission, req, workspace, control)?;
    let events = c.ledger(id)?.read().map_err(crate::error::to_io)?;
    for pending in events.iter().filter(|e| e.kind == "card_output_pending") {
        if events.iter().any(|e| {
            e.kind == "card_output" && e.card_id == pending.card_id && e.attempt == pending.attempt
        }) {
            continue;
        }
        settle_output(c, id, req, workspace, control, pending)?;
        let mut output = pending.clone();
        output.kind = "card_output".into();
        output.seq = 0;
        if !append_live(c, id, control, output)? {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "run canceled"));
        }
    }
    Ok(())
}
