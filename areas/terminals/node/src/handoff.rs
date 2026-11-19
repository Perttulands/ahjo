//! Handing work to a coding agent in a session of its own, from a
//! conversation elsewhere (ChatGPT through the node's MCP, or another
//! agent through `ahjo`): find the project, start the agent with a brief,
//! follow it, answer it. Built from what the session commands use: the
//! launcher's project walk and harnesses, a launch with notices on, the
//! send that waits for a fresh pane to be ready, the screen's waiting
//! question and the agents' reports.
//!
//! Nothing runs between calls. `handoff start` and `handoff reply` wait a
//! bounded time to see the agent take what they sent, and `handoff status`
//! waits only when asked, at most [`MAX_WAIT`]: each well under the 120 s a
//! ChatGPT tunnel request may take.

use std::path::Path;
use std::time::Duration;

use ahjo_term::control::PaneId;
use ahjo_term::engine::{NotReady, SessionInfo};
use ahjo_term::waiting::harness::{self, Harness};
use ahjo_terminals_contract::MAX_SEND;
use ahjo_terminals_contract::session_copy::next_name;
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use tokio::time::{Instant, sleep, sleep_until, timeout};

use crate::agent_events::{Event, Kind};
use crate::commands::{Ctx, find};
use crate::launch::{Launch, SHELL, check_name};
use crate::projects;
use ahjo_node_kit::args::{maybe_text, text};

/// How long `handoff start` waits in all for the agent to take its brief.
const START_WAIT: Duration = Duration::from_secs(60);

/// The longest `handoff status` waits while the agent works.
pub const MAX_WAIT: u64 = 90;

/// How long `handoff reply` waits to see the agent take the reply.
const REPLY_WAIT: Duration = Duration::from_secs(20);

/// How long a question must stay on a starting agent's screen before
/// `handoff start` stops waiting to send the brief and says so.
const ASKS_FOR: Duration = Duration::from_millis(1500);

/// How many lines of the screen a status shows, from the bottom.
const TAIL: usize = 40;

/// The most projects `project find` lists.
const FOUND: usize = 12;

/// How often a wait looks again, besides when the sessions change.
const LOOK_EVERY: Duration = Duration::from_millis(500);

/// Where an agent is, as a handoff reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
enum State {
    /// Running a turn.
    Working,
    /// Its screen asks something: a menu, a permission.
    NeedsInput,
    /// At its prompt, having reported a finished turn.
    Finished,
    /// At its prompt with no finished turn reported: not yet started, or
    /// a harness that does not report.
    Idle,
    /// The session has ended.
    Gone,
}

impl State {
    fn word(&self) -> &'static str {
        match self {
            State::Working => "working",
            State::NeedsInput => "needs-input",
            State::Finished => "finished",
            State::Idle => "idle",
            State::Gone => "gone",
        }
    }
}

/// One look at a session.
struct Look {
    state: State,
    question: Option<String>,
    report: Option<Event>,
    /// The screen's text, trailing blank lines dropped.
    screen: Option<String>,
    last_output: Option<u64>,
}

/// The harness whose screens Ahjo can read, by the program's name.
fn kind_of(command: &str) -> Option<Harness> {
    let program = command
        .trim()
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let program = program.strip_suffix(".exe").unwrap_or(&program);
    match program {
        p if p.contains("claude") => Some(Harness::Claude),
        p if p.contains("codex") => Some(Harness::Codex),
        "pi" => Some(Harness::Pi),
        _ => None,
    }
}

/// Where the agent in `session`'s `pane` is now. `kind` is its harness when
/// known, else read from what the pane runs; a harness Ahjo cannot read
/// counts as working while it writes.
fn look(ctx: &Ctx, session: &str, pane: PaneId, kind: Option<Harness>) -> Look {
    let sessions = ctx.hub.sessions();
    let Some(info) = sessions.iter().find(|s| s.id == session) else {
        return Look {
            state: State::Gone,
            question: None,
            report: None,
            screen: None,
            last_output: None,
        };
    };
    let report = ctx.hub.events(&sessions).remove(session);
    let screen = ctx.hub.read_screen(pane);
    let text = ctx.hub.text(pane);
    let question = info
        .waiting
        .iter()
        .find(|(id, _)| *id == pane)
        .map(|(_, question)| question.clone())
        .or_else(|| ahjo_term::waiting::waiting(text.as_deref().unwrap_or_default().lines()));
    let kind = kind.or_else(|| info.facts.as_ref().and_then(|f| kind_of(&f.command)));
    let working = match (kind, &screen) {
        (Some(kind), Some(screen)) => harness::working(kind, screen),
        _ => info.activity.active,
    };
    let state = if question.is_some() {
        State::NeedsInput
    } else if working {
        State::Working
    } else if report.as_ref().is_some_and(|r| r.kind == Kind::Finished) {
        State::Finished
    } else {
        State::Idle
    };
    Look {
        state,
        question,
        report,
        screen: text.map(|t| t.trim_end().to_owned()),
        last_output: last_output(info),
    }
}

/// Seconds since the session last wrote, if known: none while it writes,
/// as the engine marks only when a burst of output starts and stops.
fn last_output(info: &SessionInfo) -> Option<u64> {
    if info.activity.active {
        return Some(0);
    }
    let at = info.activity.last_output?;
    Some(at.elapsed().map_or(0, |d| d.as_secs()))
}

/// Waits for the next change to the sessions or the next look, whichever
/// is first.
async fn next_look(changed: &mut tokio::sync::watch::Receiver<u64>) {
    let _ = timeout(LOOK_EVERY, changed.changed()).await;
}

/// The newest `lines` lines of a screen.
fn tail(screen: &str, lines: usize) -> String {
    let all: Vec<&str> = screen.lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

/// A look as the commands answer it.
fn answer(ctx: &Ctx, info: &SessionInfo, look: &Look, lines: usize) -> Value {
    json!({
        "node": ctx.node,
        "session": info.id,
        "name": info.name,
        "state": look.state.word(),
        "question": look.question,
        "report": look.report.as_ref().map(|r| json!({
            "kind": r.kind.word(),
            "summary": r.summary,
            "at": r.at,
        })),
        "last_output_seconds_ago": look.last_output,
        "screen": look.screen.as_deref().map(|s| tail(s, lines)),
    })
}

/// `project find`: the project folders a fragment names.
pub async fn find_project(ctx: &Ctx, args: &Value) -> Result<Value> {
    let query = text(args, "query")?;
    let projects = ctx.projects.list().await;
    let found = projects::matching(&projects, query);
    let listed: Vec<Value> = found
        .iter()
        .take(FOUND)
        .map(|&(_, path)| {
            let (git, beads) = projects::kinds(Path::new(path));
            json!({
                "path": path,
                "name": path.rsplit(['/', '\\']).next().unwrap_or(path),
                "git": git,
                "beads": beads,
            })
        })
        .collect();
    let mut answer = json!({
        "node": ctx.node,
        "projects": listed,
        "more": found.len().saturating_sub(FOUND),
    });
    if found.is_empty() {
        let roots: Vec<String> = ctx
            .projects
            .roots()
            .iter()
            .map(|r| r.display().to_string())
            .collect();
        answer["none"] = json!(format!(
            "no project under {} matches {query:?}; give the folder's absolute path",
            roots.join(", ")
        ));
    }
    Ok(answer)
}

/// The folder `project` names: a path as it is, else the one project the
/// walk ranks best for it.
async fn folder(ctx: &Ctx, project: &str) -> Result<String> {
    let project = project.trim();
    if project.starts_with('~') || Path::new(project).is_absolute() {
        return Ok(project.to_owned());
    }
    let projects = ctx.projects.list().await;
    let found = projects::matching(&projects, project);
    match found.as_slice() {
        [] => bail!("no project matches {project:?}; project find lists them, or give its path"),
        [(best, path), rest @ ..] if rest.first().is_none_or(|(next, _)| next > best) => {
            Ok((*path).to_owned())
        }
        tied => {
            let best = tied[0].0;
            let paths: Vec<&str> = tied
                .iter()
                .take_while(|(rank, _)| *rank == best)
                .map(|(_, path)| *path)
                .collect();
            bail!("{project:?} could be {}; give the path", paths.join(" or "))
        }
    }
}

/// A session name from a folder's: its letters, digits, `_` and `-`, free
/// on the node's own server, numbered if taken.
fn session_name(ctx: &Ctx, folder: &str) -> String {
    let base: String = folder
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .take(40)
        .collect();
    let base = if base.trim_matches('-').is_empty() {
        "agent".to_owned()
    } else {
        base
    };
    let sessions = ctx.hub.sessions();
    let taken: Vec<&str> = sessions
        .iter()
        .filter(|s| s.user.is_none())
        .map(|s| s.name.as_str())
        .collect();
    if taken.contains(&base.as_str()) {
        next_name(&base, &taken)
    } else {
        base
    }
}

/// `handoff start`: a coding agent in a session of its own, in the
/// project's folder, given the brief as its first message.
pub async fn start(ctx: &Ctx, args: &Value) -> Result<Value> {
    let begun = Instant::now();
    let deadline = begun + START_WAIT;
    let brief = text(args, "brief")?.trim();
    if brief.is_empty() {
        bail!("brief is empty: say what the agent is to do");
    }
    let brief = match maybe_text(args, "work")? {
        Some(work) => format!("{brief}\n\nYour work item: {}", work.trim()),
        None => brief.to_owned(),
    };
    if brief.len() > MAX_SEND {
        bail!("the brief is longer than {MAX_SEND} bytes");
    }
    let launcher = ctx.launcher.get();
    let agents: Vec<&crate::launch::Harness> = launcher
        .harnesses
        .iter()
        .filter(|h| h.id != SHELL)
        .collect();
    let harness = match maybe_text(args, "harness")? {
        Some(id) => *agents
            .iter()
            .find(|h| h.id == id)
            .with_context(|| harness_choice(id, &agents))?,
        None => *agents
            .first()
            .context("the launcher has no agent harness; add one to launch.conf")?,
    };
    let folder = folder(ctx, text(args, "project")?).await?;
    let name = match maybe_text(args, "name")? {
        Some(name) => name.to_owned(),
        None => session_name(ctx, &folder),
    };
    check_name(&name)?;
    let launch = Launch {
        harness: harness.id.clone(),
        folder: folder.clone(),
        name,
        flags: harness.flags.clone(),
        user: String::new(),
        notify: true,
        size: None,
    };
    let (session, warning) = ctx.hub.launch(&launcher, &launch).await?;
    let info = find(ctx, &session)?;
    let pane = ctx.hub.active_pane(&session).await?;
    let kind = kind_of(&harness.command);

    // The brief goes in once the agent is ready for a paste. A question on
    // its screen meanwhile (trusting the folder, a sign-in) is the
    // operator's to answer, so the brief waits for that, and is withdrawn
    // unsent if it stays.
    let mut changed = ctx.hub.watch();
    let delivering = ctx.hub.deliver_when_ready(pane, brief, true).await?;
    tokio::pin!(delivering);
    let mut asked_since: Option<Instant> = None;
    let sent: Result<(), String> = loop {
        tokio::select! {
            delivered = &mut delivering => {
                break delivered.map_err(|error| {
                    if error.is::<NotReady>() {
                        "the agent was not ready for its brief in time".to_owned()
                    } else {
                        format!("{error:#}")
                    }
                });
            }
            () = next_look(&mut changed) => {
                let now = look(ctx, &session, pane, kind);
                match (now.state, asked_since) {
                    (State::Gone, _) => break Err("the session ended before it took the brief".into()),
                    (State::NeedsInput, Some(since)) if since.elapsed() >= ASKS_FOR => {
                        break Err("the agent asks something before it can take the brief".into());
                    }
                    (State::NeedsInput, None) => asked_since = Some(Instant::now()),
                    (State::NeedsInput, Some(_)) => {}
                    _ => asked_since = None,
                }
            }
            () = sleep_until(deadline) => {
                break Err("the agent was not ready for its brief in time".into());
            }
        }
    };

    // Then until the agent has taken it: working, or already done.
    let mut now = look(ctx, &session, pane, kind);
    if sent.is_ok() {
        while now.state == State::Idle && Instant::now() < deadline {
            next_look(&mut changed).await;
            now = look(ctx, &session, pane, kind);
        }
    }
    let mut answered = answer(ctx, &info, &now, TAIL);
    answered["folder"] = json!(folder);
    answered["harness"] = json!(harness.id);
    answered["warning"] = json!(warning);
    answered["brief_sent"] = json!(sent.is_ok());
    let follow = format!(
        "Follow it with handoff_status (session {}, wait up to {MAX_WAIT} seconds while it works) \
         and answer it with handoff_reply; from a shell, ahjo handoff status {} --wait 60.",
        info.name, info.name
    );
    let said = match (&sent, &now.state) {
        (Ok(()), State::Working | State::Finished) => format!(
            "Opened {} in Swarm: {} in {folder} took the brief and is {}.",
            info.name,
            harness.id,
            now.state.word()
        ),
        (Ok(()), _) => format!(
            "Opened {} in Swarm and sent {} the brief; it is {} after {} s.",
            info.name,
            harness.id,
            now.state.word(),
            begun.elapsed().as_secs()
        ),
        (Err(why), _) => format!(
            "Opened {} in Swarm, but the brief was not sent: {why}.{} Send the brief with \
             handoff_reply once it is answered.",
            info.name,
            match &now.question {
                Some(question) => format!(
                    " It asks: {question:?}; answer with handoff_reply and the option's number."
                ),
                None => String::new(),
            }
        ),
    };
    answered["say"] = json!(said);
    answered["follow"] = json!(follow);
    Ok(answered)
}

fn harness_choice(id: &str, agents: &[&crate::launch::Harness]) -> String {
    let ids: Vec<&str> = agents.iter().map(|h| h.id.as_str()).collect();
    format!(
        "no agent harness {id:?}; the launcher has {}",
        ids.join(", ")
    )
}

/// The session `args` names and its pane: the one a send goes to.
async fn target(ctx: &Ctx, args: &Value) -> Result<(SessionInfo, PaneId)> {
    let info = find(ctx, text(args, "session")?)?;
    let pane = ctx.hub.active_pane(&info.id).await?;
    Ok((info, pane))
}

/// `handoff status`: where the agent is; with `wait`, once it stops
/// working or the wait ends.
pub async fn status(ctx: &Ctx, args: &Value) -> Result<Value> {
    let wait = match args.get("wait") {
        None | Some(Value::Null) => 0,
        Some(wait) => wait
            .as_u64()
            .context("wait is a whole number of seconds")?
            .min(MAX_WAIT),
    };
    let lines = match args.get("lines") {
        None | Some(Value::Null) => TAIL,
        Some(lines) => lines.as_u64().context("lines is a count")? as usize,
    };
    let wanted = text(args, "session")?;
    if find(ctx, wanted).is_err() {
        return Ok(json!({
            "node": ctx.node,
            "session": wanted,
            "state": State::Gone.word(),
            "say": format!("no session {wanted} on this node: it has ended, or never was; session_list shows those there are"),
        }));
    }
    let (info, pane) = target(ctx, args).await?;
    // The screen is read once the engine follows the pane.
    let _follow = ctx.hub.subscribe(pane);
    let mut changed = ctx.hub.watch();
    let seeded = async {
        while ctx.hub.text(pane).is_none() {
            if changed.changed().await.is_err() {
                break;
            }
        }
    };
    let _ = timeout(Duration::from_secs(5), seeded).await;
    let begun = Instant::now();
    let deadline = begun + Duration::from_secs(wait);
    let mut now = look(ctx, &info.id, pane, None);
    while now.state == State::Working && Instant::now() < deadline {
        tokio::select! {
            () = next_look(&mut changed) => {}
            () = sleep_until(deadline) => {}
        }
        now = look(ctx, &info.id, pane, None);
    }
    let mut answered = answer(ctx, &info, &now, lines);
    if wait > 0 {
        answered["waited_seconds"] = json!(begun.elapsed().as_secs());
    }
    Ok(answered)
}

/// `handoff reply`: the text typed into the agent and submitted, or, while
/// its screen asks with a menu, the option's key pressed.
pub async fn reply(ctx: &Ctx, args: &Value) -> Result<Value> {
    let text = text(args, "text")?;
    if text.trim().is_empty() {
        bail!("text is empty");
    }
    if text.len() > MAX_SEND {
        bail!("text is longer than {MAX_SEND} bytes");
    }
    let (info, pane) = target(ctx, args).await?;
    let _follow = ctx.hub.subscribe(pane);
    let before = look(ctx, &info.id, pane, None);
    let pressed = match (&before.state, &before.question) {
        (State::Gone, _) => bail!("{} has ended", info.name),
        (State::NeedsInput, Some(question)) => {
            let key = text.trim();
            if !(key.len() == 1 && key.bytes().all(|b| b.is_ascii_alphanumeric())) {
                bail!(
                    "{} asks {question:?} and waits on its menu: reply with the number of an \
                     option, as handoff_status shows the screen",
                    info.name
                );
            }
            ctx.hub.send_input(pane, key.as_bytes().to_vec()).await?;
            true
        }
        _ => {
            ctx.hub.deliver(pane, text.to_owned(), true).await?.await?;
            false
        }
    };
    // Until the agent has taken it: its screen changed from what it was.
    let mut changed = ctx.hub.watch();
    let deadline = Instant::now() + REPLY_WAIT;
    let mut now = look(ctx, &info.id, pane, None);
    while (now.state == before.state && now.question == before.question)
        && now.state != State::Working
        && Instant::now() < deadline
    {
        next_look(&mut changed).await;
        now = look(ctx, &info.id, pane, None);
    }
    // A moment more for a menu's answer to be drawn over.
    if pressed && now.state == State::NeedsInput {
        sleep(LOOK_EVERY).await;
        now = look(ctx, &info.id, pane, None);
    }
    let mut answered = answer(ctx, &info, &now, TAIL);
    answered["sent"] = json!(if pressed { "key" } else { "text" });
    Ok(answered)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_harness_is_known_by_its_program() {
        assert_eq!(kind_of("claude"), Some(Harness::Claude));
        assert_eq!(kind_of("/usr/local/bin/codex"), Some(Harness::Codex));
        assert_eq!(kind_of("C:\\tools\\codex.exe"), Some(Harness::Codex));
        assert_eq!(kind_of("bash"), None);
        assert_eq!(kind_of("pi"), Some(Harness::Pi));
    }

    #[test]
    fn a_tail_is_the_newest_lines() {
        assert_eq!(tail("a\nb\nc", 2), "b\nc");
        assert_eq!(tail("a", 5), "a");
    }
}
