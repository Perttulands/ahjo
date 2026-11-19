//! The session commands, each an MCP tool and an `ahjo session <verb>`,
//! through the hub: list, read a screen or the whole history, search, send
//! text or keys, launch or copy, rename, kill, mark a report seen, and turn
//! tmux's mouse mode on or off.

use std::sync::Arc;
use std::time::UNIX_EPOCH;

use ahjo_core::{Error, NodeId};
use ahjo_node_kit::args::{flag, maybe_text, object, text};
use ahjo_node_kit::{Command, Effect};
use ahjo_terminals_contract::{COMMAND_FAILED, MAX_SEND};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use crate::hub::Hub;
use crate::launch::Launch;
use crate::launcher::Launcher;
use ahjo_term::control::PaneId;
use ahjo_term::engine::SessionInfo;
use ahjo_term::search::{self, Target};

/// What the commands reach.
pub(crate) struct Ctx {
    pub node: NodeId,
    pub hub: Arc<Hub>,
    pub launcher: Arc<Launcher>,
    pub projects: Arc<crate::projects::Projects>,
}

/// One command: its schema from `ctx` as the tools are listed, and its
/// call with `ctx` once the sessions are listed, whose failure is a
/// [`COMMAND_FAILED`] error.
macro_rules! command {
    ($ctx:expr, $name:expr, $effect:expr, $description:expr, |$s:ident| $schema:expr, $call:path) => {{
        let (schema_ctx, call_ctx) = ($ctx.clone(), $ctx.clone());
        Command::new(
            $name,
            $effect,
            $description,
            move || {
                let $s: &Ctx = &schema_ctx;
                $schema
            },
            move |args: Value| {
                let ctx = call_ctx.clone();
                async move {
                    ctx.hub.ready().await.map_err(failed)?;
                    $call(&ctx, &args).await.map_err(failed)
                }
            },
        )
    }};
}

fn failed(error: anyhow::Error) -> Error {
    Error::new(COMMAND_FAILED, format!("{error:#}"))
}

/// Every session command.
pub(crate) fn commands(ctx: &Arc<Ctx>) -> Vec<Command> {
    vec![
        command!(
            ctx,
            "session_list",
            Effect::Reads,
            "List the sessions on this node: id, name, panes, whether \
        they are active, when they last wrote, and the question a pane waits on.",
            |_ctx| object(json!({}), &[]),
            list_sessions
        ),
        command!(
            ctx,
            "session_read",
            Effect::Reads,
            "Read the visible screen of a session's pane as plain text.",
            |_ctx| object(target_properties(json!({})), &["session"]),
            read_screen
        ),
        command!(
            ctx,
            "session_send",
            Effect::Writes,
            "Type text into a session's pane as one paste, then press Enter \
        unless enter is false. Answers once tmux has taken it.",
            |_ctx| {
                object(
                    target_properties(json!({
                        "text": { "type": "string" },
                        "enter": { "type": "boolean", "default": true },
                    })),
                    &["session", "text"],
                )
            },
            send_text
        ),
        command!(
            ctx,
            "session_launch",
            Effect::Writes,
            "Start a new session running a harness in a folder. Answers with \
        the new session's id once it is listed.",
            |ctx| {
                let launcher = ctx.launcher.get();
                let harnesses: Vec<&str> =
                    launcher.harnesses.iter().map(|h| h.id.as_str()).collect();
                let mut properties = json!({
                    "harness": { "type": "string", "enum": harnesses },
                    "folder": {
                        "type": "string",
                        "description": "Absolute or under ~.",
                        "examples": launcher.folders,
                    },
                    "name": {
                        "type": "string",
                        "description": "Letters, digits, _ and -, at most 50.",
                    },
                    "flags": {
                        "type": "string",
                        "description": "The whole flag line; the harness's defaults if absent.",
                    },
                    "cols": { "type": "integer", "minimum": 1, "maximum": 65535, "description": "Initial terminal columns; supply with rows." },
                    "rows": { "type": "integer", "minimum": 1, "maximum": 65535, "description": "Initial terminal rows; supply with cols." },
                    "notify": {
                        "type": "boolean",
                        "default": false,
                        "description": "Report when the agent finishes or waits on input.",
                    },
                });
                // Only a node that follows other users' servers has a choice
                // to offer: an empty enum would take no value at all.
                let users = ctx.hub.users();
                if !users.is_empty() {
                    properties["user"] = json!({
                        "type": "string",
                        "enum": users,
                        "description": "Whose tmux server runs it; the node's own if absent.",
                    });
                }
                object(properties, &["harness", "folder", "name"])
            },
            launch_agent
        ),
        command!(
            ctx,
            "session_copy",
            Effect::Writes,
            "Copy a session's running harness in its folder on the same user, under the next free family name.",
            |_ctx| {
                object(
                    json!({"session": {"type": "string", "description": "A session id or name."}, "notify": {"type": "boolean", "default": false, "description": "Install notification hooks; the app preference is unavailable to the node."}}),
                    &["session"],
                )
            },
            copy
        ),
        command!(
            ctx,
            "session_history",
            Effect::Reads,
            "Read a pane's scrollback and screen as plain text, oldest line \
        first: the newest 200 lines, or the newest last lines, or with all every \
        line. A cut answer says how many lines it left out.",
            |_ctx| {
                object(
                    target_properties(json!({
                        "last": { "type": "integer", "description": "Keep only this many of the newest lines (200 if not said)." },
                        "all": { "type": "boolean", "description": "Every line, however many." },
                    })),
                    &["session"],
                )
            },
            history
        ),
        command!(
            ctx,
            "session_search",
            Effect::Reads,
            "Search every pane's scrollback for text. Case matters only if \
        the query has a capital letter. Answers with each matching line and the \
        lines either side, at most 500.",
            |_ctx| object(json!({ "query": { "type": "string" } }), &["query"]),
            search
        ),
        command!(
            ctx,
            "session_keys",
            Effect::Writes,
            "Press keys in a session's pane, in order. Each key is a name \
        (Enter, Tab, Escape, BSpace, Space, Up, Down, Left, Right, Home, End, \
        PageUp, PageDown, Delete, C-c and other C-<letter>) or else text typed as it is.",
            |_ctx| {
                object(
                    target_properties(json!({
                        "keys": { "type": "array", "items": { "type": "string" } },
                    })),
                    &["session", "keys"],
                )
            },
            keys
        ),
        command!(
            ctx,
            "session_rename",
            Effect::Sets,
            "Rename a session. Refused if another session on its tmux \
        server has the name. Answers once the session list shows the new name.",
            |_ctx| {
                object(
                    json!({
                        "session": { "type": "string", "description": "A session id or name." },
                        "name": {
                            "type": "string",
                            "description": "The new name: letters, digits, _ and -, at most 50.",
                        },
                    }),
                    &["session", "name"],
                )
            },
            rename
        ),
        command!(
            ctx,
            "session_kill",
            Effect::Destroys,
            "End a session and everything running in it.",
            |_ctx| {
                object(
                    json!({ "session": { "type": "string", "description": "A session id or name." } }),
                    &["session"],
                )
            },
            kill
        ),
        command!(
            ctx,
            "session_mouse",
            Effect::Sets,
            "Turn tmux's mouse mode on or off on every tmux server this \
        node follows. On, the wheel scrolls a session's history and a drag \
        selects inside tmux; off, the wheel and clicks go to the program.",
            |_ctx| {
                object(
                    json!({ "mouse": { "type": "string", "enum": ["on", "off"] } }),
                    &["mouse"],
                )
            },
            mouse
        ),
        command!(
            ctx,
            "session_seen",
            Effect::Sets,
            "Mark a session's last report (finished, needs-input) as seen, \
        so it stops showing as news.",
            |_ctx| {
                object(
                    json!({ "session": { "type": "string", "description": "A session id or name." } }),
                    &["session"],
                )
            },
            seen
        ),
        command!(
            ctx,
            "project_find",
            Effect::Reads,
            "Find project folders on this machine by a fragment of their name or path \
        (\"kor\" finds /srv/games/korpi), best match first, each with whether it has Git and \
        Beads. Use this when the user names a project and you need its folder.",
            |_ctx| {
                object(
                    json!({ "query": {
                        "type": "string",
                        "description": "A fragment of the project's name, or words in its path.",
                    } }),
                    &["query"],
                )
            },
            crate::handoff::find_project
        ),
        command!(
            ctx,
            "handoff_start",
            Effect::Writes,
            "Hand work to a coding agent on Swarm. Use this when the user wants an agent \
        (Codex, Claude Code) to take over work from this conversation. It opens a session in \
        the project's folder, gives the agent the brief as its first message, waits up to a \
        minute for it to start, and answers the session to follow and a line to tell the user. \
        Write the brief to stand on its own: the goal, what was decided, constraints, what \
        counts as done, and links (brain [[slugs]], a Bead id).",
            |ctx| {
                let launcher = ctx.launcher.get();
                let agents: Vec<&str> = launcher
                    .harnesses
                    .iter()
                    .map(|h| h.id.as_str())
                    .filter(|id| *id != crate::launch::SHELL)
                    .collect();
                let mut harness = json!({ "type": "string" });
                if let Some(first) = agents.first() {
                    harness["enum"] = json!(agents);
                    harness["description"] = json!(format!("The agent; {first} if not said."));
                }
                object(
                    json!({
                        "project": {
                            "type": "string",
                            "description": "The project's folder, or a fragment of its name as project_find takes it.",
                        },
                        "brief": {
                            "type": "string",
                            "description": "The agent's first message, standing on its own.",
                        },
                        "harness": harness,
                        "name": {
                            "type": "string",
                            "description": "The session's name: letters, digits, _ and -; the folder's name if not said.",
                        },
                        "work": {
                            "type": "string",
                            "description": "A work item (Bead id) the agent is to own, named at the end of the brief.",
                        },
                    }),
                    &["project", "brief"],
                )
            },
            crate::handoff::start
        ),
        command!(
            ctx,
            "handoff_status",
            Effect::Reads,
            "Where an agent's session is: working, needs-input (with the question its screen \
        asks), finished (with its last report), idle (at its prompt with no finished turn \
        reported) or gone, and the bottom of its screen. Use this to follow a handoff or any \
        session. With wait, it answers as soon as the agent stops working, waiting at most \
        that many seconds.",
            |_ctx| {
                object(
                    json!({
                        "session": { "type": "string", "description": "A session id or name." },
                        "wait": {
                            "type": "integer",
                            "minimum": 0,
                            "maximum": crate::handoff::MAX_WAIT,
                            "description": "Seconds to wait while the agent works; 0 if not said.",
                        },
                        "lines": {
                            "type": "integer",
                            "minimum": 1,
                            "description": "How many of the screen's bottom lines (40 if not said).",
                        },
                    }),
                    &["session"],
                )
            },
            crate::handoff::status
        ),
        command!(
            ctx,
            "handoff_reply",
            Effect::Writes,
            "Answer an agent or send it a follow-up: the text is typed into its session and \
        submitted. While its screen asks with a menu (handoff_status says needs-input and shows \
        the options), the text is the option's number, pressed as a key. Answers where the \
        agent is just after.",
            |_ctx| {
                object(
                    json!({
                        "session": { "type": "string", "description": "A session id or name." },
                        "text": { "type": "string" },
                    }),
                    &["session", "text"],
                )
            },
            crate::handoff::reply
        ),
    ]
}

async fn copy(ctx: &Ctx, args: &Value) -> Result<Value> {
    let source = find(ctx, text(args, "session")?)?;
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
        notify: flag(args, "notify", false)?,
        size: None,
    };
    let (session, warning) = ctx.hub.launch(&launcher, &launch).await?;
    Ok(json!({"node": ctx.node, "session": session, "warning": warning}))
}

/// `session` and an optional `pane`, besides `more`.
fn target_properties(mut more: Value) -> Value {
    more["session"] = json!({ "type": "string", "description": "A session id or name." });
    more["pane"] = json!({
        "type": "integer",
        "description": "One of the session's panes; its first if absent.",
    });
    more
}

async fn list_sessions(ctx: &Ctx, _args: &Value) -> Result<Value> {
    let listed = ctx.hub.sessions();
    let reports = ctx.hub.events(&listed);
    let sessions: Vec<Value> = listed
        .iter()
        .map(|s| {
            let mut value = session(s);
            value["report"] = reports.get(&s.id).map_or(Value::Null, |event| {
                json!({
                    "kind": event.kind.word(),
                    "at": event.at,
                    "summary": event.summary,
                    "seen": event.seen,
                })
            });
            value
        })
        .collect();
    Ok(json!({ "node": ctx.node, "sessions": sessions }))
}

fn session(session: &SessionInfo) -> Value {
    let waiting = |pane: PaneId| {
        session
            .waiting
            .iter()
            .find(|(id, _)| *id == pane)
            .map(|(_, question)| question.as_str())
    };
    let panes: Vec<Value> = session
        .panes
        .iter()
        .map(|&(id, cols, rows)| {
            json!({ "id": id, "cols": cols, "rows": rows, "waiting": waiting(id) })
        })
        .collect();
    let last_output = session
        .activity
        .last_output
        .and_then(|at| at.duration_since(UNIX_EPOCH).ok())
        .map(|since| since.as_secs());
    json!({
        "id": session.id,
        "name": session.name,
        "user": session.user,
        "active": session.activity.active,
        "last_output": last_output,
        "panes": panes,
    })
}

async fn read_screen(ctx: &Ctx, args: &Value) -> Result<Value> {
    let (session, pane) = target(ctx, args).await?;
    let mut changed = ctx.hub.watch();
    let text = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Some(text) = ctx.hub.text(pane) {
                break Ok::<_, anyhow::Error>(text);
            }
            changed.changed().await.context("session stream closed")?;
        }
    })
    .await
    .context("the pane is still being read from tmux; try again")??;
    Ok(json!({
        "node": ctx.node,
        "session": session.id,
        "pane": pane,
        "text": text.trim_end(),
    }))
}

async fn send_text(ctx: &Ctx, args: &Value) -> Result<Value> {
    let (session, pane) = target(ctx, args).await?;
    let text = text(args, "text")?;
    if text.len() > MAX_SEND {
        bail!("text is longer than {MAX_SEND} bytes");
    }
    let enter = flag(args, "enter", true)?;
    ctx.hub.deliver(pane, text.to_owned(), enter).await?.await?;
    Ok(json!({ "node": ctx.node, "session": session.id, "pane": pane, "sent": true }))
}

async fn launch_agent(ctx: &Ctx, args: &Value) -> Result<Value> {
    let harness = text(args, "harness")?;
    let folder = text(args, "folder")?;
    let launcher = ctx.launcher.get();
    let dimension = |key: &str| -> Result<Option<u16>> {
        match args.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(value) => {
                let n = value
                    .as_u64()
                    .and_then(|n| u16::try_from(n).ok())
                    .filter(|n| *n > 0)
                    .with_context(|| format!("{key} must be between 1 and 65535"))?;
                Ok(Some(n))
            }
        }
    };
    let size = match (dimension("cols")?, dimension("rows")?) {
        (None, None) => None,
        (Some(cols), Some(rows)) => Some((cols, rows)),
        _ => bail!("cols and rows must be supplied together"),
    };
    let launch = Launch {
        harness: harness.to_owned(),
        // The launcher resolves ~ and checks the folder, as for the app.
        folder: folder.to_owned(),
        name: text(args, "name")?.to_owned(),
        flags: match maybe_text(args, "flags")? {
            Some(flags) => flags.to_owned(),
            None => launcher
                .harnesses
                .iter()
                .find(|h| h.id == harness)
                .map(|h| h.flags.clone())
                .unwrap_or_default(),
        },
        user: maybe_text(args, "user")?.unwrap_or_default().to_owned(),
        notify: flag(args, "notify", false)?,
        size,
    };
    let (session, warning) = ctx.hub.launch(&launcher, &launch).await?;
    Ok(json!({ "node": ctx.node, "session": session, "warning": warning }))
}

/// How many of a pane's newest lines `session history` answers with when
/// not told: a screenful or several, never a whole scrollback unasked.
const HISTORY_LINES: usize = 200;

async fn history(ctx: &Ctx, args: &Value) -> Result<Value> {
    let (session, pane) = target(ctx, args).await?;
    let (server, own) = ctx.hub.pane(pane).context("the pane went")?;
    let capture = search::capture(server.backend(), own).await?;
    let plain = strip_escapes(&capture);
    let lines: Vec<&str> = plain.trim_end().lines().collect();
    let keep = match (args.get("last"), flag(args, "all", false)?) {
        (None | Some(Value::Null), true) => lines.len(),
        (None | Some(Value::Null), false) => HISTORY_LINES,
        (Some(_), true) => bail!("give last or all, not both"),
        (Some(last), false) => last.as_u64().context("last is a count of lines")? as usize,
    };
    let left_out = lines.len().saturating_sub(keep);
    let text = lines[left_out..].join("\n");
    // Said before the text, so a reader cut short by it still sees it.
    let cut = (left_out > 0).then(|| {
        format!(
            "the newest {keep} of {} lines; --last <n> or --all for more",
            lines.len()
        )
    });
    Ok(json!({ "node": ctx.node, "session": session.id, "pane": pane, "cut": cut, "text": text }))
}

async fn search(ctx: &Ctx, args: &Value) -> Result<Value> {
    let query = text(args, "query")?;
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
    let session_of = |pane: PaneId| {
        sessions
            .iter()
            .find(|s| s.panes.iter().any(|&(id, _, _)| id == pane))
            .map(|s| s.id.as_str())
    };
    let matches: Vec<Value> = found
        .iter()
        .map(|m| {
            json!({
                "session": session_of(m.pane),
                "pane": m.pane,
                "line": m.line,
                "text": strip_escapes(&m.context[1]),
                "before": strip_escapes(&m.context[0]),
                "after": strip_escapes(&m.context[2]),
            })
        })
        .collect();
    Ok(json!({
        "node": ctx.node,
        "matches": matches,
        "panes": summary.panes,
        "truncated": summary.truncated,
    }))
}

async fn keys(ctx: &Ctx, args: &Value) -> Result<Value> {
    let (session, pane) = target(ctx, args).await?;
    let keys = args
        .get("keys")
        .and_then(Value::as_array)
        .context("keys is required, as a list of keys")?;
    let mut bytes = Vec::new();
    for key in keys {
        bytes.extend(key_bytes(key.as_str().context("each key is a string")?));
    }
    if bytes.len() > MAX_SEND {
        bail!("keys come to more than {MAX_SEND} bytes");
    }
    ctx.hub.send_input(pane, bytes).await?;
    Ok(json!({ "node": ctx.node, "session": session.id, "pane": pane, "sent": true }))
}

async fn kill(ctx: &Ctx, args: &Value) -> Result<Value> {
    let session = find(ctx, text(args, "session")?)?;
    let (server, id) = ctx.hub.session(&session.id)?;
    crate::launch::delete(server.backend(), id).await?;
    Ok(json!({ "node": ctx.node, "session": session.id, "killed": true }))
}

async fn rename(ctx: &Ctx, args: &Value) -> Result<Value> {
    let session = find(ctx, text(args, "session")?)?;
    let name = text(args, "name")?;
    ctx.hub.rename(&session.id, name).await?;
    Ok(json!({ "node": ctx.node, "session": session.id, "name": name }))
}

async fn mouse(ctx: &Ctx, args: &Value) -> Result<Value> {
    let on = match text(args, "mouse")? {
        "on" => true,
        "off" => false,
        other => bail!("mouse is on or off, not {other:?}"),
    };
    let mouse = ctx.hub.mouse(on).await;
    if mouse.applied() == 0 {
        bail!("{}", mouse.text(on));
    }
    Ok(json!({
        "node": ctx.node,
        "mouse": if on { "on" } else { "off" },
        "applied": mouse.applied(),
        "total": mouse.total,
        "failures": mouse.failures,
    }))
}

async fn seen(ctx: &Ctx, args: &Value) -> Result<Value> {
    let session = find(ctx, text(args, "session")?)?;
    let reports = ctx.hub.events(std::slice::from_ref(&session));
    let Some(report) = reports.get(&session.id) else {
        return Ok(json!({ "node": ctx.node, "session": session.id, "report": false }));
    };
    ctx.hub.event_seen(&session.id, report.at);
    Ok(json!({ "node": ctx.node, "session": session.id, "report": true }))
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
async fn target(ctx: &Ctx, args: &Value) -> Result<(SessionInfo, PaneId)> {
    let wanted = text(args, "session")?;
    let session = find(ctx, wanted)?;
    let session = &session;
    let pane = match args.get("pane") {
        None | Some(Value::Null) => Some(ctx.hub.active_pane(&session.id).await?),
        Some(pane) => {
            let pane = pane
                .as_u64()
                .and_then(|id| PaneId::try_from(id).ok())
                .context("pane is a pane id")?;
            session
                .panes
                .iter()
                .any(|&(id, _, _)| id == pane)
                .then_some(pane)
        }
    }
    .with_context(|| format!("session {wanted} has no such pane"))?;
    Ok((session.clone(), pane))
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
