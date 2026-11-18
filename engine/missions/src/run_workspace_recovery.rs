//! Recover pending turn publications and clean abandoned worktrees.
use crate::coordinator::Control;
use crate::run_worker::{append_live, event};
use crate::run_workspace::{commit_turn, remove, turn_path};
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
        let same = |e: &&RunEvent| e.node_id == start.node_id && e.attempt == start.attempt;
        let pending = events.iter().filter(same).any(|e| {
            e.kind == "node_output_pending"
                || (e.kind == "output_capture_pending"
                    && !crate::run_capture::discarded(&events, e))
        });
        let settled = events
            .iter()
            .filter(same)
            .any(|e| e.kind == "workspace_settled");
        if !pending || settled || control.canceled.load(std::sync::atomic::Ordering::Acquire) {
            let path = turn_path(c, id, &start.node_id, start.attempt);
            if !settled {
                kept.extend(keep(c, id, mission, &events, start, &path)?);
            }
            remove(workspace, &path)?;
        }
    }
    Ok(kept)
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
    let (node, attempt) = (&start.node_id, start.attempt);
    let ours = |e: &&RunEvent| e.node_id == *node && e.attempt == attempt;
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
    let step = (mission.formation(node).map(|f| f.title.as_str()))
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
    if req.repository.is_empty() {
        return Ok(());
    }
    cleanup(c, id, mission, req, workspace, control)?;
    let events = c.ledger(id)?.read().map_err(crate::error::to_io)?;
    for pending in events.iter().filter(|e| e.kind == "node_output_pending") {
        if events.iter().any(|e| {
            e.kind == "node_output" && e.node_id == pending.node_id && e.attempt == pending.attempt
        }) {
            continue;
        }
        commit_turn(
            c,
            id,
            req,
            workspace,
            control,
            &pending.node_id,
            pending.attempt,
        )?;
        let mut output = pending.clone();
        output.kind = "node_output".into();
        output.seq = 0;
        if !append_live(c, id, control, output)? {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "run canceled"));
        }
    }
    Ok(())
}
