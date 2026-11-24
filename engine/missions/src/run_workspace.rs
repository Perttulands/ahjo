//! Isolated turn worktrees and their durable publication to the run branch.
use crate::coordinator::Control;
use crate::run_worker::{append_live, event};
use crate::{Coordinator, RunEvent, Workspaces, Worktree};
use ahjo_mission_model::{Mission, RunStart};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    ffi::OsStr,
    io,
    path::{Path, PathBuf},
    process::Command,
};

pub(crate) fn turn_path(c: &Coordinator, id: &str, node: &str, attempt: i64) -> PathBuf {
    c.inner.root.join("worktrees").join(format!(
        ".turn-{id}-{}-{attempt}",
        hex::encode(Sha256::digest(node.as_bytes()))
    ))
}

/// One checkout belongs to the step for the life of its run. Each attempt
/// records its own commit while the harness continues in this directory.
pub(crate) fn stable_path(c: &Coordinator, id: &str, node: &str) -> PathBuf {
    c.inner.root.join("worktrees").join(format!(
        ".turn-{id}-{}-1",
        hex::encode(Sha256::digest(node.as_bytes()))
    ))
}

pub(crate) fn snapshot_path(c: &Coordinator, id: &str, node: &str, attempt: i64) -> PathBuf {
    c.inner.root.join("worktrees").join(format!(
        ".snapshot-{id}-{}-{attempt}",
        hex::encode(Sha256::digest(node.as_bytes()))
    ))
}

pub(crate) fn recorded_path(
    c: &Coordinator,
    id: &str,
    node: &str,
    attempt: i64,
    events: &[RunEvent],
) -> PathBuf {
    if events.iter().any(|event| {
        event.kind == "workspace_started"
            && event.card_id == node
            && event.attempt == attempt
            && event.data.get("stable").and_then(|v| v.as_bool()) == Some(true)
    }) {
        stable_path(c, id, node)
    } else {
        turn_path(c, id, node, attempt)
    }
}

/// A pending review starts each retry from the same recorded base. Once
/// the step publishes, a later visit starts from the current run branch.
fn retry_base(c: &Coordinator, id: &str, node: &str) -> io::Result<Option<String>> {
    let events = c.ledger(id)?.read().map_err(crate::error::to_io)?;
    Ok(events
        .iter()
        .rev()
        .find(|event| {
            event.card_id == node
                && (event.kind == "workspace_settled"
                    || (event.kind == "workspace_started"
                        && event.data.get("stable").and_then(|v| v.as_bool()) == Some(true)))
        })
        .filter(|event| event.kind == "workspace_started")
        .and_then(|event| event.data.get("base"))
        .and_then(|base| base.as_str())
        .map(str::to_owned))
}

/// The gates that may choose another output over this step's attempt: one
/// that can send the step back for another attempt, or one that picks among
/// several steps feeding it. The attempt's work waits for their choice before
/// it reaches the run branch.
pub(crate) fn review_gates(mission: &Mission, node: &str) -> Vec<String> {
    use crate::graph::{endpoint_card, endpoint_port, gathers, outgoing, outgoing_from_port};
    let only_one = mission.limits.iter().any(|card| {
        (card.scope == node || mission.has_input_card(&card.scope))
            && card.rounds == Some(1)
            && card.when_spent == ahjo_mission_model::WhenSpent::Fail
    });
    outgoing(mission, node)
        .filter_map(|wire| {
            let gate = endpoint_card(&wire.to);
            let sends_back = !only_one
                && outgoing_from_port(mission, gate, ahjo_mission_model::GATE_FAIL)
                    .any(|wire| endpoint_card(&wire.to) == node);
            (endpoint_port(&wire.to) == ahjo_mission_model::GATE_IN
                && mission.gate(gate).is_some()
                && (sends_back || gathers(mission, gate)))
            .then(|| gate.to_owned())
        })
        .collect()
}

/// Save a pass or pick until the selected attempt has reached the run branch.
pub(crate) fn record_verdict(
    c: &Coordinator,
    id: &str,
    control: &Control,
    events: &[RunEvent],
    mut verdict: RunEvent,
) -> io::Result<()> {
    if verdict.kind == "gate_verdict"
        && verdict.data.get("route_port").and_then(|v| v.as_str()) == Some("pass")
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
            && let Some(node) = reference.get(identity).and_then(|v| v.as_str())
            && let Some(attempt) = reference.get("attempt").and_then(|v| v.as_i64())
            && let Some(start) = events.iter().find(|e| {
                e.kind == "workspace_started"
                    && e.card_id == node
                    && e.attempt == attempt
                    && e.data
                        .get("review_gates")
                        .and_then(|v| v.as_array())
                        .is_some_and(|gates| {
                            gates
                                .iter()
                                .any(|gate| gate.as_str() == Some(&verdict.gate_id))
                        })
            })
        {
            verdict.kind = "workspace_verdict_pending".into();
            verdict.data.insert("workspace_step_id".into(), node.into());
            verdict
                .data
                .insert("workspace_attempt".into(), attempt.into());
            verdict
                .data
                .insert("workspace_start_seq".into(), start.seq.into());
            return append_workspace(c, id, control, verdict);
        }
    }
    crate::run_folder::record_verdict(c, id, control, events, verdict)
}

pub(crate) fn pending(events: &[RunEvent]) -> Option<&RunEvent> {
    events.iter().find(|pending| {
        pending.kind == "workspace_verdict_pending"
            && !events.iter().any(|event| {
                event.kind == "gate_verdict"
                    && event
                        .data
                        .get("workspace_verdict_seq")
                        .and_then(|v| v.as_u64())
                        == Some(pending.seq)
            })
    })
}

pub(crate) fn settle(
    c: &Coordinator,
    id: &str,
    req: &RunStart,
    workspace: &Path,
    control: &Control,
    events: &[RunEvent],
    pending: &RunEvent,
) -> io::Result<()> {
    let node = pending
        .data
        .get("workspace_step_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| io::Error::other("selected step missing"))?;
    let attempt = pending
        .data
        .get("workspace_attempt")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| io::Error::other("selected attempt missing"))?;
    let start_seq = pending
        .data
        .get("workspace_start_seq")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    if events.iter().any(|event| {
        event.kind == "workspace_selection"
            && event.card_id == node
            && event.attempt != attempt
            && event.seq > start_seq
    }) {
        return Err(io::Error::other(format!(
            "workspace_conflict: another gate selected a different attempt of {node}"
        )));
    }
    if !events.iter().any(|event| {
        event.kind == "workspace_selection" && event.card_id == node && event.attempt == attempt
    }) {
        append_workspace(
            c,
            id,
            control,
            event(
                "workspace_selection",
                node,
                attempt,
                json!({"verdict_seq":pending.seq}),
            ),
        )?;
    }
    commit_turn(c, id, req, workspace, control, node, attempt)?;
    let mut verdict = pending.clone();
    verdict.seq = 0;
    verdict.kind = "gate_verdict".into();
    verdict
        .data
        .insert("workspace_verdict_seq".into(), pending.seq.into());
    append_workspace(c, id, control, verdict)
}

pub(crate) fn attempt_path(
    c: &Coordinator,
    id: &str,
    events: &[RunEvent],
    node: &str,
    attempt: i64,
) -> Option<PathBuf> {
    let path = snapshot_path(c, id, node, attempt);
    (path.exists()
        && events
            .iter()
            .any(|e| e.kind == "workspace_snapshot" && e.card_id == node && e.attempt == attempt))
    .then_some(path)
}

pub(crate) fn reviewed_path(
    c: &Coordinator,
    id: &str,
    events: &[RunEvent],
    origin: Option<&ahjo_mission_model::PublicOrigin>,
) -> Option<PathBuf> {
    let origin = origin?;
    attempt_path(c, id, events, &origin.card_id, origin.attempt)
        .or_else(|| crate::run_folder::reviewed_path(c, id, events, Some(origin)))
}

pub(crate) fn prepare_turn(
    c: &Coordinator,
    id: &str,
    (mission, req, review): (&Mission, &RunStart, bool),
    workspace: &Path,
    control: &Control,
    node: &str,
    attempt: i64,
) -> io::Result<PathBuf> {
    if req.repository.is_empty() {
        return Ok(workspace.to_path_buf());
    }
    let _guard = control.workspace.lock().unwrap();
    if control.canceled.load(std::sync::atomic::Ordering::Acquire) {
        return Err(io::Error::new(io::ErrorKind::Interrupted, "run canceled"));
    }
    let path = stable_path(c, id, node);
    let retry = retry_base(c, id, node)?;
    let base = match retry.as_ref() {
        Some(base) => base.clone(),
        None => git(workspace, &["rev-parse".as_ref(), "HEAD".as_ref()])?,
    };
    if retry.is_none() && path.exists() {
        git(&path, &["reset".as_ref(), "--keep".as_ref(), base.as_ref()])?;
    }
    let gates = if review {
        review_gates(mission, node)
    } else {
        Vec::new()
    };
    append_workspace(
        c,
        id,
        control,
        event(
            "workspace_started",
            node,
            attempt,
            json!({"base":base,"stable":true,"review_gates":gates}),
        ),
    )?;
    if !path.exists() {
        git(
            workspace,
            &[
                "worktree".as_ref(),
                "add".as_ref(),
                "--detach".as_ref(),
                path.as_os_str(),
                base.as_ref(),
            ],
        )?;
    }
    Ok(path)
}

/// A turn on another environment starts from the run branch's head, pushed
/// to the repository's shared remote for that environment to fetch.
pub(crate) fn prepare_placed_turn(
    c: &Coordinator,
    id: &str,
    (mission, req, review): (&Mission, &RunStart, bool),
    workspace: &Path,
    control: &Control,
    (node, environment): (&str, &str),
    attempt: i64,
) -> io::Result<crate::TurnWorkspace> {
    if req.repository.is_empty() {
        return Err(io::Error::other(format!(
            "step {node} runs on {environment}, which needs a repository run to share its workspace"
        )));
    }
    let _guard = control.workspace.lock().unwrap();
    if control.canceled.load(std::sync::atomic::Ordering::Acquire) {
        return Err(io::Error::new(io::ErrorKind::Interrupted, "run canceled"));
    }
    let retry = retry_base(c, id, node)?;
    let base = match retry.as_ref() {
        Some(base) => base.clone(),
        None => git(workspace, &["rev-parse".as_ref(), "HEAD".as_ref()])?,
    };
    let gates = if review {
        review_gates(mission, node)
    } else {
        Vec::new()
    };
    append_workspace(
        c,
        id,
        control,
        event(
            "workspace_started",
            node,
            attempt,
            json!({"base":base,"environment":environment,"stable":true,"placed":true,"review_gates":gates}),
        ),
    )?;
    let branch = ahjo_mission_model::run_branch(id);
    let target = format!("HEAD:refs/heads/{branch}");
    git(
        workspace,
        &[
            "push".as_ref(),
            "--quiet".as_ref(),
            "origin".as_ref(),
            target.as_ref(),
        ],
    )?;
    append_workspace(
        c,
        id,
        control,
        event(
            "workspace_push",
            node,
            attempt,
            json!({"commit":base,"branch":branch}),
        ),
    )?;
    Ok(crate::TurnWorkspace {
        run_id: id.into(),
        step_id: node.into(),
        attempt,
        repository: req.repository.clone(),
        commit: base,
    })
}

/// The commits a placed turn published: fetched from the shared remote,
/// from the turn's base to its published commit.
fn placed_commits(
    workspace: &Path,
    node: &str,
    attempt: i64,
    base: &str,
    events: &[RunEvent],
) -> io::Result<Vec<String>> {
    let published = events
        .iter()
        .find(|e| e.kind == "workspace_turn" && e.card_id == node && e.attempt == attempt)
        .ok_or_else(|| {
            io::Error::other(format!(
                "step {node} attempt {attempt} ran on another environment and did not publish its work"
            ))
        })?;
    let field = |name: &str| {
        published
            .data
            .get(name)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned()
    };
    let (commit, reference) = (field("commit"), field("reference"));
    git(
        workspace,
        &[
            "fetch".as_ref(),
            "--quiet".as_ref(),
            "origin".as_ref(),
            reference.as_ref(),
        ],
    )?;
    let range = format!("{base}..{commit}");
    Ok(git(
        workspace,
        &[
            "rev-list".as_ref(),
            "--reverse".as_ref(),
            "--topo-order".as_ref(),
            range.as_ref(),
        ],
    )?
    .lines()
    .map(str::to_owned)
    .collect())
}

/// Keep changed outputs before publishing them so a restart can finish the
/// same attempt without asking the seat to do the work again. A judge's
/// files in a run folder are its own and are not published.
pub(crate) fn publish_output(
    c: &Coordinator,
    id: &str,
    req: &RunStart,
    workspace: &Path,
    control: &Control,
    mut output: RunEvent,
) -> io::Result<()> {
    let is_judge = output.data.get("reason").and_then(|v| v.as_str()) == Some("judge");
    let needs_publication = if req.repository.is_empty() && is_judge {
        crate::run_folder::keep_judge_files(c, id, &output.card_id, output.attempt)?;
        false
    } else if req.repository.is_empty() {
        crate::run_folder::snapshot(c, id, control, &output.card_id, output.attempt)?;
        crate::run_folder::unreviewed(c, id, &output.card_id, output.attempt)?
    } else {
        true
    };
    if needs_publication {
        output.kind = "card_output_pending".into();
        append_workspace(c, id, control, output.clone())?;
        settle_output(c, id, req, workspace, control, &output)?;
        output.kind = "card_output".into();
    }
    append_workspace(c, id, control, output)
}

pub(crate) fn settle_output(
    c: &Coordinator,
    id: &str,
    req: &RunStart,
    workspace: &Path,
    control: &Control,
    output: &RunEvent,
) -> io::Result<()> {
    if req.repository.is_empty() {
        crate::run_folder::publish_unreviewed(c, id, control, &output.card_id, output.attempt)
    } else {
        commit_turn(
            c,
            id,
            req,
            workspace,
            control,
            &output.card_id,
            output.attempt,
        )
    }
}

pub(crate) fn commit_turn(
    c: &Coordinator,
    id: &str,
    req: &RunStart,
    workspace: &Path,
    control: &Control,
    node: &str,
    attempt: i64,
) -> io::Result<()> {
    if req.repository.is_empty() {
        return Ok(());
    }
    let _guard = control.workspace.lock().unwrap();
    if control.canceled.load(std::sync::atomic::Ordering::Acquire) {
        return Err(io::Error::new(io::ErrorKind::Interrupted, "run canceled"));
    }
    let events = c.ledger(id)?.read().map_err(crate::error::to_io)?;
    let turn = |kind: &str| {
        events
            .iter()
            .find(|e| e.kind == kind && e.card_id == node && e.attempt == attempt)
    };
    let path = recorded_path(c, id, node, attempt, &events);
    let stable = turn("workspace_started")
        .and_then(|e| e.data.get("stable"))
        .and_then(|v| v.as_bool())
        == Some(true);
    if turn("workspace_settled").is_some() {
        return if stable {
            Ok(())
        } else {
            remove(workspace, &path)
        };
    }
    let first = &events[0];
    let field = |name: &str| {
        first
            .data
            .get(name)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned()
    };
    let tree = Worktree {
        path: path.clone(),
        checkout: PathBuf::from(field("checkout")),
        branch: ahjo_mission_model::run_branch(id),
        base: field("base"),
    };
    let workspaces = Workspaces::new(
        c.inner.root.join("worktrees"),
        c.inner.settings.repositories.clone(),
    );
    let mut published = turn("workspace_published")
        .and_then(|e| e.data.get("commit"))
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    if published.is_none() {
        let commits: Vec<String> = if let Some(snapshot) = turn("workspace_snapshot") {
            serde_json::from_value(snapshot.data.get("commits").cloned().unwrap_or_default())
                .map_err(io::Error::other)?
        } else if turn("workspace_push").is_some() {
            let base = turn("workspace_started")
                .and_then(|e| e.data.get("base"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| io::Error::other("turn base is missing"))?;
            let commits = placed_commits(workspace, node, attempt, base, &events)?;
            append_workspace(
                c,
                id,
                control,
                event(
                    "workspace_snapshot",
                    node,
                    attempt,
                    json!({"base":base,"commits":commits}),
                ),
            )?;
            commits
        } else {
            let base = turn("workspace_started")
                .and_then(|e| e.data.get("base"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| io::Error::other("turn base is missing"))?;
            if !path.exists() {
                return Err(io::Error::other(format!(
                    "workspace for {node} attempt {attempt} is missing"
                )));
            }
            workspaces
                .commit_turn(&tree, id, node, attempt)
                .map_err(crate::error::to_io)?;
            let range = format!("{base}..HEAD");
            let commits = git(
                &path,
                &[
                    "rev-list".as_ref(),
                    "--reverse".as_ref(),
                    "--topo-order".as_ref(),
                    range.as_ref(),
                ],
            )?
            .lines()
            .map(str::to_owned)
            .collect::<Vec<_>>();
            append_workspace(
                c,
                id,
                control,
                event(
                    "workspace_snapshot",
                    node,
                    attempt,
                    json!({"base":base,"commits":commits}),
                ),
            )?;
            commits
        };
        let reviewed = turn("workspace_started")
            .and_then(|e| e.data.get("review_gates"))
            .and_then(|v| v.as_array())
            .is_some_and(|gates| !gates.is_empty());
        if reviewed {
            let snapshot = snapshot_path(c, id, node, attempt);
            if !snapshot.exists() {
                let base = turn("workspace_started")
                    .and_then(|e| e.data.get("base"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| io::Error::other("turn base is missing"))?;
                let head = commits.last().map(String::as_str).unwrap_or(base);
                git(
                    workspace,
                    &[
                        "worktree".as_ref(),
                        "add".as_ref(),
                        "--detach".as_ref(),
                        snapshot.as_os_str(),
                        head.as_ref(),
                    ],
                )?;
            }
            if turn("workspace_selection").is_none() {
                return Ok(());
            }
        }
        for source in commits {
            let prior = events.iter().any(|e| {
                e.kind == "workspace_commit"
                    && e.card_id == node
                    && e.attempt == attempt
                    && e.data.get("source").and_then(|v| v.as_str()) == Some(&source)
            });
            if prior {
                continue;
            }
            let history = git(workspace, &["log".as_ref(), "--format=%B".as_ref()])?;
            let marker = format!("(cherry picked from commit {source})");
            if !history.lines().any(|line| line == marker)
                && let Err(error) = git(
                    workspace,
                    &[
                        "cherry-pick".as_ref(),
                        "-x".as_ref(),
                        "--allow-empty".as_ref(),
                        "--keep-redundant-commits".as_ref(),
                        source.as_ref(),
                    ],
                )
            {
                let _ = git(workspace, &["cherry-pick".as_ref(), "--abort".as_ref()]);
                return Err(error);
            }
            let head = git(workspace, &["rev-parse".as_ref(), "HEAD".as_ref()])?;
            append_workspace(
                c,
                id,
                control,
                event(
                    "workspace_commit",
                    node,
                    attempt,
                    json!({"commit":head,"source":source}),
                ),
            )?;
        }
        let head = git(workspace, &["rev-parse".as_ref(), "HEAD".as_ref()])?;
        append_workspace(
            c,
            id,
            control,
            event("workspace_published", node, attempt, json!({"commit":head})),
        )?;
        published = Some(head);
    }
    if !stable {
        remove(workspace, &path)?;
    }
    append_workspace(
        c,
        id,
        control,
        event(
            "workspace_settled",
            node,
            attempt,
            json!({"commit":published}),
        ),
    )?;
    Ok(())
}

fn append_workspace(
    c: &Coordinator,
    id: &str,
    control: &Control,
    event: RunEvent,
) -> io::Result<()> {
    if append_live(c, id, control, event)? {
        Ok(())
    } else {
        Err(io::Error::new(io::ErrorKind::Interrupted, "run canceled"))
    }
}

fn git(cwd: &Path, args: &[&OsStr]) -> io::Result<String> {
    let output = Command::new("git").current_dir(cwd).args(args).output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "git: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}
pub(crate) fn remove(workspace: &Path, path: &Path) -> io::Result<()> {
    let registered = if path.exists() {
        true
    } else {
        let entries = git(
            workspace,
            &[
                "worktree".as_ref(),
                "list".as_ref(),
                "--porcelain".as_ref(),
                "-z".as_ref(),
            ],
        )?;
        entries
            .split('\0')
            .filter_map(|entry| entry.strip_prefix("worktree "))
            .any(|entry| Path::new(entry) == path)
    };
    if registered {
        git(
            workspace,
            &[
                "worktree".as_ref(),
                "remove".as_ref(),
                "--force".as_ref(),
                path.as_os_str(),
            ],
        )?;
    }
    Ok(())
}
