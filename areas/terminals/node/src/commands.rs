//! The session commands, each an MCP tool and an `ahjo session <verb>`,
//! through the hub: list, read a screen or the whole history, search, send
//! text or keys, launch or copy, rename, kill, mark a report seen, and turn
//! tmux's mouse mode on or off.

use std::sync::Arc;
use std::time::UNIX_EPOCH;

use ahjo_core::{Error, NodeId};
use ahjo_node_kit::{Command, Effect};
use ahjo_terminals_contract::{COMMAND_FAILED, MAX_SEND, commands::*};
use anyhow::{Context, Result, bail};
use futures_util::{StreamExt, stream};
use schemars::JsonSchema;
use serde::{Serialize, de::DeserializeOwned};

use crate::launch::Launch;
use crate::launcher::Launcher;
use crate::{handoff, hub::Hub};
use ahjo_term::control::PaneId;
use ahjo_term::engine::SessionInfo;
use ahjo_term::search::{self, Target};

/// What the commands reach.
pub(crate) struct Ctx {
    pub node: NodeId,
    pub hub: Arc<Hub>,
    pub launcher: Arc<Launcher>,
    pub projects: Arc<crate::projects::Projects>,
    pub flags: Arc<crate::flags::Catalogue>,
}

fn command<Input, Output, Fut>(
    ctx: &Arc<Ctx>,
    name: &'static str,
    effect: Effect,
    call: fn(Arc<Ctx>, Input) -> Fut,
) -> Command
where
    Input: DeserializeOwned + JsonSchema + Send + 'static,
    Output: Serialize + JsonSchema,
    Fut: Future<Output = Result<Output>> + Send + 'static,
{
    let ctx = ctx.clone();
    Command::typed(name, effect, move |args| {
        let ctx = ctx.clone();
        async move {
            ctx.hub.ready().await.map_err(failed)?;
            call(ctx, args).await.map_err(failed)
        }
    })
}

fn failed(error: anyhow::Error) -> Error {
    Error::new(COMMAND_FAILED, format!("{error:#}"))
}

pub(crate) fn commands(ctx: &Arc<Ctx>) -> Vec<Command> {
    vec![
        session_list_launch_flags(ctx),
        session_list_folders(),
        project_list(ctx),
        session_list_windows(ctx),
        command(ctx, "session_list", Effect::Reads, list_sessions),
        command(ctx, "session_read", Effect::Reads, read),
        command(ctx, "session_send", Effect::Writes, send_text),
        command(ctx, "session_launch", Effect::Writes, launch_agent),
        command(ctx, "session_copy", Effect::Writes, copy),
        command(ctx, "session_search", Effect::Reads, search),
        command(ctx, "session_send_keys", Effect::Writes, keys),
        command(ctx, "session_edit", Effect::Sets, rename),
        command(ctx, "session_delete", Effect::Destroys, kill),
        command(ctx, "session_edit_mouse", Effect::Sets, mouse),
        command(ctx, "session_mark_seen", Effect::Sets, seen),
        command(ctx, "project_search", Effect::Reads, handoff::find_project),
        command(ctx, "handoff_launch", Effect::Writes, handoff::start),
        command(ctx, "handoff_show", Effect::Reads, handoff::status),
        command(ctx, "handoff_reply", Effect::Writes, handoff::reply),
    ]
}

fn session_list_launch_flags(ctx: &Arc<Ctx>) -> Command {
    let catalogue = ctx.flags.clone();
    Command::typed(
        "session_list_launch_flags",
        Effect::Reads,
        move |args: SessionListLaunchFlags| {
            let catalogue = catalogue.clone();
            async move {
                crate::flags::get_flags(&catalogue, &args.harness)
                    .await
                    .map(|flags| SessionLaunchFlagsListed { flags })
            }
        },
    )
}

fn session_list_folders() -> Command {
    Command::typed(
        "session_list_folders",
        Effect::Reads,
        |args: SessionListFolders| async move {
            let path = std::path::PathBuf::from(args.path);
            if !path.is_absolute() {
                return Err(Error::new(COMMAND_FAILED, "An absolute folder is required"));
            }
            let result = async {
                let mut entries = tokio::fs::read_dir(&path).await?;
                let mut names = Vec::new();
                while let Some(entry) = entries.next_entry().await? {
                    if tokio::fs::metadata(entry.path())
                        .await
                        .is_ok_and(|m| m.is_dir())
                    {
                        names.push(entry.file_name().to_string_lossy().into_owned());
                    }
                }
                names.sort();
                Ok::<_, std::io::Error>(names)
            }
            .await;
            result
                .map(|folders| SessionFoldersListed { folders })
                .map_err(|error| {
                    Error::new(
                        COMMAND_FAILED,
                        format!("Cannot list {}: {error}", path.display()),
                    )
                })
        },
    )
}

fn project_list(ctx: &Arc<Ctx>) -> Command {
    let projects = ctx.projects.clone();
    Command::typed("project_list", Effect::Reads, move |_: ListProjects| {
        let projects = projects.clone();
        async move {
            Ok(ProjectsListed {
                projects: (*projects.list().await).clone(),
            })
        }
    })
}

fn session_list_windows(ctx: &Arc<Ctx>) -> Command {
    let ctx = ctx.clone();
    Command::typed(
        "session_list_windows",
        Effect::Reads,
        move |args: SessionListWindows| {
            let ctx = ctx.clone();
            async move {
                let session = find(&ctx, &args.session)
                    .map_err(|error| Error::new(COMMAND_FAILED, format!("{error:#}")))?;
                ctx.hub
                    .windows(&session.id)
                    .await
                    .map(|windows| {
                        let windows = windows
                            .into_iter()
                            .map(|w| TerminalWindow {
                                index: w.index,
                                name: w.name,
                                pane: w.pane,
                                cwd: w.cwd,
                            })
                            .collect::<Vec<_>>();
                        SessionWindowsListed { windows }
                    })
                    .map_err(|error| Error::new(COMMAND_FAILED, format!("{error:#}")))
            }
        },
    )
}

async fn copy(ctx: Arc<Ctx>, args: SessionCopy) -> Result<SessionLaunched> {
    let source = find(&ctx, &args.session)?;
    let facts = source
        .facts
        .as_ref()
        .context("session facts are not yet available")?;
    if facts.path.trim().is_empty() {
        bail!("Working directory unavailable");
    }
    let launcher = ctx.launcher.get();
    let index = ahjo_terminals_contract::session_copy::harness_index(
        &facts.command,
        launcher.harnesses.iter().map(|h| h.command.as_str()),
    )
    .context("Running command is not a recognized harness")?;
    let harness = &launcher.harnesses[index];
    let sessions = ctx.hub.sessions();
    let taken: Vec<&str> = sessions
        .iter()
        .filter(|s| s.user == source.user)
        .map(|s| s.name.as_str())
        .collect();
    let name = ahjo_terminals_contract::session_copy::next_name(&source.name, &taken);
    let launch = Launch {
        harness: harness.id.clone(),
        folder: facts.path.clone(),
        name,
        flags: harness.flags.clone(),
        user: source.user.unwrap_or_default(),
        install_completion_hooks: args.install_completion_hooks.unwrap_or_default(),
        size: None,
    };
    let (session, warning) = ctx.hub.launch(&launcher, &launch).await?;
    Ok(SessionLaunched {
        node: ctx.node.clone(),
        session,
        warning,
    })
}

async fn list_sessions(ctx: Arc<Ctx>, _: SessionList) -> Result<SessionsListed> {
    let listed = ctx.hub.sessions();
    let mut reports = ctx.hub.events(&listed);
    let sessions = stream::iter(listed.into_iter().map(|info| {
        let report = reports.remove(&info.id);
        let ctx = &ctx;
        async move { session_summary(ctx, info, report).await }
    }))
    .buffered(8)
    .collect()
    .await;
    Ok(SessionsListed {
        node: ctx.node.clone(),
        sessions,
    })
}

async fn session_summary(
    ctx: &Ctx,
    info: SessionInfo,
    report: Option<crate::agent_events::Event>,
) -> SessionSummary {
    let (harness, state) = handoff::session_state(ctx, &info).await;
    let panes = info
        .panes
        .iter()
        .map(|&(id, cols, rows)| PaneSummary {
            id,
            cols,
            rows,
            waiting: info
                .waiting
                .iter()
                .find(|(pane, _)| *pane == id)
                .map(|(_, question)| question.clone()),
        })
        .collect();
    let last_output = info
        .activity
        .last_output
        .and_then(|at| at.duration_since(UNIX_EPOCH).ok())
        .map(|since| since.as_secs());
    SessionSummary {
        id: info.id,
        name: info.name,
        user: info.user,
        active: info.activity.active,
        last_output,
        harness,
        state,
        panes,
        report: report.map(|event| SessionReport {
            report: handoff::report_summary(&event),
            seen: event.seen,
        }),
    }
}

/// A pane's visible text, once the node has begun following it.
pub(crate) async fn visible(ctx: &Ctx, pane: PaneId) -> Result<String> {
    let mut changed = ctx.hub.watch();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Some(text) = ctx.hub.text(pane) {
                break Ok(text);
            }
            changed.changed().await.context("session stream closed")?;
        }
    })
    .await
    .context("the pane is still being read from tmux; try again")?
}

async fn read(ctx: Arc<Ctx>, args: SessionRead) -> Result<SessionReadout> {
    let all = args.all.unwrap_or_default();
    if all && args.lines.is_some() {
        bail!("give lines or all, not both");
    }
    let (session, pane) = target(&ctx, &args.session, args.pane).await?;
    let (cut, text) = if all || args.lines.is_some() {
        let (server, own) = ctx.hub.pane(pane).context("the pane went")?;
        let capture = search::capture(server.backend(), own).await?;
        let plain = strip_escapes(&capture);
        let lines: Vec<_> = plain.trim_end().lines().collect();
        let keep = args.lines.unwrap_or(lines.len());
        let left_out = lines.len().saturating_sub(keep);
        let cut = (left_out > 0).then(|| {
            format!(
                "the newest {keep} of {} lines; --lines <n> or --all for more",
                lines.len()
            )
        });
        (cut, lines[left_out..].join("\n"))
    } else {
        (None, visible(&ctx, pane).await?.trim_end().into())
    };
    Ok(SessionReadout {
        node: ctx.node.clone(),
        session: session.id,
        pane,
        cut,
        text,
    })
}

async fn send_text(ctx: Arc<Ctx>, args: SessionSend) -> Result<SessionSent> {
    let (session, pane) = target(&ctx, &args.session, args.pane).await?;
    if args.text.len() > MAX_SEND {
        bail!("text is longer than {MAX_SEND} bytes");
    }
    ctx.hub
        .deliver(pane, args.text, args.enter.unwrap_or(true))
        .await?
        .await?;
    Ok(SessionSent {
        node: ctx.node.clone(),
        session: session.id,
        pane,
        sent: true,
    })
}

async fn launch_agent(ctx: Arc<Ctx>, args: SessionLaunch) -> Result<SessionLaunched> {
    let launcher = ctx.launcher.get();
    let size = match (args.cols, args.rows) {
        (None, None) => None,
        (Some(cols), Some(rows)) => Some((cols.get(), rows.get())),
        _ => bail!("cols and rows must be supplied together"),
    };
    let flags = args.flags.unwrap_or_else(|| {
        launcher
            .harnesses
            .iter()
            .find(|h| h.id == args.harness)
            .map(|h| h.flags.clone())
            .unwrap_or_default()
    });
    let launch = Launch {
        harness: args.harness,
        // The launcher resolves ~ and checks the folder, as for the app.
        folder: args.folder,
        name: args.name,
        flags,
        user: args.user.unwrap_or_default(),
        install_completion_hooks: args.install_completion_hooks.unwrap_or_default(),
        size,
    };
    let (session, warning) = ctx.hub.launch(&launcher, &launch).await?;
    Ok(SessionLaunched {
        node: ctx.node.clone(),
        session,
        warning,
    })
}

async fn search(ctx: Arc<Ctx>, args: SessionSearch) -> Result<SessionsSearched> {
    let query = &args.query;
    if query.is_empty() {
        bail!("query is empty");
    }
    let sessions = ctx.hub.sessions();
    let panes = sessions
        .iter()
        .flat_map(|s| s.panes.iter().map(|&(pane, _, _)| pane))
        .filter_map(|id| {
            let (server, pane) = ctx.hub.pane(id)?;
            Some(Target {
                backend: server.engine.backend().clone(),
                pane,
                id,
            })
        })
        .collect();
    let (found, mut matches) = tokio::sync::mpsc::channel(64);
    let gather = async {
        let mut all = Vec::new();
        while let Some(found) = matches.recv().await {
            all.push(found);
        }
        all
    };
    let (summary, found) = tokio::join!(search::search(panes, query, found), gather);
    let matches = found
        .into_iter()
        .filter_map(|m| {
            let session = sessions
                .iter()
                .find(|s| s.panes.iter().any(|&(id, _, _)| id == m.pane))?;
            Some(SessionMatch {
                session: session.name.clone(),
                pane: m.pane,
                line: m.line,
                text: strip_escapes(&m.context[1]),
                before: strip_escapes(&m.context[0]),
                after: strip_escapes(&m.context[2]),
            })
        })
        .collect();
    Ok(SessionsSearched {
        node: ctx.node.clone(),
        matches,
        panes: summary.panes,
        truncated: summary.truncated,
    })
}

async fn keys(ctx: Arc<Ctx>, args: SessionSendKeys) -> Result<SessionSent> {
    let (session, pane) = target(&ctx, &args.session, args.pane).await?;
    let bytes: Vec<_> = args.keys.iter().flat_map(|key| key_bytes(key)).collect();
    if bytes.len() > MAX_SEND {
        bail!("keys come to more than {MAX_SEND} bytes");
    }
    ctx.hub.send_input(pane, bytes).await?;
    Ok(SessionSent {
        node: ctx.node.clone(),
        session: session.id,
        pane,
        sent: true,
    })
}

async fn kill(ctx: Arc<Ctx>, args: SessionDelete) -> Result<SessionKilled> {
    let session = find(&ctx, &args.session)?;
    let (server, id) = ctx.hub.session(&session.id)?;
    crate::launch::delete(server.backend(), id).await?;
    Ok(SessionKilled {
        node: ctx.node.clone(),
        session: session.id,
        killed: true,
    })
}

async fn rename(ctx: Arc<Ctx>, args: SessionEdit) -> Result<SessionRenamed> {
    let session = find(&ctx, &args.session)?;
    ctx.hub.rename(&session.id, &args.name).await?;
    Ok(SessionRenamed {
        node: ctx.node.clone(),
        session: session.id,
        name: args.name,
    })
}

async fn mouse(ctx: Arc<Ctx>, args: SessionEditMouse) -> Result<MouseSet> {
    let on = matches!(args.mouse, MouseMode::On);
    let mouse = ctx.hub.mouse(on).await;
    let applied = mouse.applied();
    if applied == 0 {
        bail!("{}", mouse.text(on));
    }
    Ok(MouseSet {
        node: ctx.node.clone(),
        mouse: args.mouse,
        applied,
        total: mouse.total,
        failures: mouse.failures,
    })
}

async fn seen(ctx: Arc<Ctx>, args: SessionMarkSeen) -> Result<SessionSeenResult> {
    let session = find(&ctx, &args.session)?;
    let reports = ctx.hub.events(&ctx.hub.sessions());
    if let Some(report) = reports.get(&session.id) {
        ctx.hub.event_seen(&session.id, report.at);
    }
    Ok(SessionSeenResult {
        node: ctx.node.clone(),
        report: reports.contains_key(&session.id),
        session: session.id,
    })
}

/// What pressing `key` sends: a named key's sequence, else the text itself.
fn key_bytes(key: &str) -> Vec<u8> {
    let named: &[u8] = match key {
        "Enter" => b"\r",
        "Tab" => b"\t",
        "BTab" => b"\x1b[Z",
        "Escape" => b"\x1b",
        "BSpace" => b"\x7f",
        "Space" => b" ",
        "Up" => b"\x1b[A",
        "Down" => b"\x1b[B",
        "Right" => b"\x1b[C",
        "Left" => b"\x1b[D",
        "Home" => b"\x1b[H",
        "End" => b"\x1b[F",
        "PageUp" => b"\x1b[5~",
        "PageDown" => b"\x1b[6~",
        "Delete" => b"\x1b[3~",
        _ => {
            return match key.strip_prefix("C-").map(str::as_bytes) {
                Some(&[letter]) if letter.is_ascii_alphabetic() => {
                    vec![letter.to_ascii_lowercase() - b'a' + 1]
                }
                _ => key.as_bytes().to_vec(),
            };
        }
    };
    named.to_vec()
}

/// `text` without its terminal escapes (colours and attributes).
fn strip_escapes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        if chars.next() == Some('[') {
            for c in chars.by_ref() {
                if ('@'..='~').contains(&c) {
                    break;
                }
            }
        }
    }
    out
}

/// The session `session` names, by id or else by name, and its pane
/// `pane`, or else its active one ([`Hub::active_pane`]).
pub(crate) async fn target(
    ctx: &Ctx,
    wanted: &str,
    pane: Option<PaneId>,
) -> Result<(SessionInfo, PaneId)> {
    let session = find(ctx, wanted)?;
    let pane = match pane {
        None => ctx.hub.active_pane(&session.id).await?,
        Some(pane) if session.panes.iter().any(|&(id, _, _)| id == pane) => pane,
        Some(_) => bail!("session {wanted} has no such pane"),
    };
    Ok((session, pane))
}

/// The session `wanted` names, by id or else by unique name.
pub(crate) fn find(ctx: &Ctx, wanted: &str) -> Result<SessionInfo> {
    let sessions = ctx.hub.sessions();
    if let Some(session) = sessions.iter().find(|s| s.id == wanted) {
        return Ok(session.clone());
    }
    let mut named = sessions.iter().filter(|s| s.name == wanted);
    match (named.next(), named.next()) {
        (Some(session), None) => Ok(session.clone()),
        (Some(_), Some(_)) => {
            bail!("several sessions are named {wanted}; give its id, as ahjo session list shows it")
        }
        (None, _) => bail!("no session named {wanted}; ahjo session list shows them"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_named_or_typed() {
        assert_eq!(key_bytes("Enter"), b"\r");
        assert_eq!(key_bytes("C-c"), [3]);
        assert_eq!(key_bytes("C-D"), [4]);
        assert_eq!(key_bytes("Up"), b"\x1b[A");
        assert_eq!(key_bytes("y"), b"y");
        assert_eq!(key_bytes("C-cat"), b"C-cat");
    }

    #[test]
    fn escapes_are_stripped() {
        assert_eq!(strip_escapes("\x1b[1;31mred\x1b[0m plain"), "red plain");
    }
}
