//! Handing work to a coding agent in a session of its own, from a
//! conversation elsewhere (ChatGPT through the node's MCP, or another
//! agent through `ahjo`): find the project, start the agent with a brief,
//! follow it, answer it. Built from what the session commands use: the
//! launcher's project walk and harnesses, a launch with notices on, the
//! send that waits for a fresh pane to be ready, the screen's waiting
//! question and the agents' reports.
//!
//! Nothing runs between calls. `handoff launch` and `handoff reply` wait a
//! bounded time to see the agent take what they sent, and `handoff show`
//! waits only when asked, at most [`MAX_WAIT`]: each well under the 120 s a
//! ChatGPT tunnel request may take.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use ahjo_term::control::PaneId;
use ahjo_term::engine::{NotReady, SessionInfo};
use ahjo_term::waiting::harness::{self, Harness};
use ahjo_terminals_contract::session_copy::next_name;
use ahjo_terminals_contract::{
    MAX_SEND,
    commands::{HandoffState as State, *},
};
use anyhow::{Context, Result, bail};
use tokio::time::{Instant, sleep, sleep_until, timeout};

use crate::agent_events::{Event, Kind};
use crate::commands::{Ctx, find, target, visible};
use crate::launch::{Launch, SHELL, check_name};
use crate::projects;

/// How long `handoff launch` waits in all for the agent to take its brief.
const START_WAIT: Duration = Duration::from_secs(60);

/// The longest `handoff show` waits while the agent works.
const MAX_WAIT: u64 = 90;

/// How long `handoff reply` waits to see the agent take the reply.
const REPLY_WAIT: Duration = Duration::from_secs(20);

/// How long a question must stay on a starting agent's screen before
/// `handoff launch` stops waiting to send the brief and says so.
const ASKS_FOR: Duration = Duration::from_millis(1500);

/// How many lines of the screen a status shows, from the bottom.
const TAIL: usize = 40;

/// The most projects `project search` lists.
const FOUND: usize = 12;

/// How often a wait looks again, besides when the sessions change.
const LOOK_EVERY: Duration = Duration::from_millis(500);

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
    let (state, question, screen) = observe(
        ctx,
        info,
        Some(pane),
        kind.or_else(|| session_harness(info)),
        ctx.hub.text(pane),
    );
    let state = match state {
        SessionState::Working => State::Working,
        SessionState::NeedsInput => State::NeedsInput,
        SessionState::Idle if report.as_ref().is_some_and(|r| r.kind == Kind::Finished) => {
            State::Finished
        }
        SessionState::Idle => State::Idle,
    };
    Look {
        state,
        question,
        report,
        screen,
        last_output: last_output(info),
    }
}

fn session_harness(info: &SessionInfo) -> Option<Harness> {
    info.facts
        .as_ref()
        .and_then(|facts| kind_of(&facts.command))
}

/// The active pane's harness and state, using the same screen recognition
/// as a handoff. Wait for the screen of a recognized harness on a cold node.
pub(crate) async fn session_state(ctx: &Ctx, info: &SessionInfo) -> (SessionHarness, SessionState) {
    let kind = session_harness(info);
    let pane = if kind.is_some() || !info.waiting.is_empty() {
        ctx.hub.active_pane(&info.id).await.ok()
    } else {
        None
    };
    let text = match (kind, pane) {
        (Some(_), Some(pane)) => visible(ctx, pane).await.ok(),
        _ => None,
    };
    let (state, _, _) = observe(ctx, info, pane, kind, text);
    let harness = match kind {
        Some(Harness::Claude) => SessionHarness::ClaudeCode,
        Some(Harness::Codex) => SessionHarness::OpenaiCodex,
        Some(Harness::Pi) => SessionHarness::Pi,
        None => SessionHarness::None,
    };
    (harness, state)
}

/// A question on the active pane takes precedence over a running turn.
/// Without a harness screen, use the engine's activity observation.
fn observe(
    ctx: &Ctx,
    info: &SessionInfo,
    pane: Option<PaneId>,
    kind: Option<Harness>,
    text: Option<String>,
) -> (SessionState, Option<String>, Option<String>) {
    let question = pane
        .and_then(|pane| {
            info.waiting
                .iter()
                .find(|(id, _)| *id == pane)
                .map(|(_, question)| question.clone())
        })
        .or_else(|| ahjo_term::waiting::waiting(text.as_deref().unwrap_or_default().lines()));
    let working = kind
        .zip(pane)
        .and_then(|(kind, pane)| {
            ctx.hub
                .read_screen(pane)
                .map(|screen| harness::working(kind, &screen))
        })
        .unwrap_or(info.activity.active);
    let state = if question.is_some() {
        SessionState::NeedsInput
    } else if working {
        SessionState::Working
    } else {
        SessionState::Idle
    };
    (state, question, text.map(|text| text.trim_end().to_owned()))
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

pub(crate) fn report_summary(event: &Event) -> HandoffReport {
    HandoffReport {
        kind: match event.kind {
            Kind::Finished => ReportKind::Finished,
            Kind::NeedsInput => ReportKind::NeedsInput,
        },
        summary: event.summary.clone(),
        at: event.at,
    }
}

/// The details shared by each handoff answer about a session it observed.
fn detail(info: &SessionInfo, look: &Look, lines: usize) -> HandoffDetail {
    HandoffDetail {
        name: info.name.clone(),
        question: look.question.clone(),
        report: look.report.as_ref().map(report_summary),
        last_output_seconds_ago: look.last_output,
        screen: look.screen.as_deref().map(|screen| tail(screen, lines)),
    }
}

/// `project search`: the project folders a fragment names.
pub async fn find_project(ctx: Arc<Ctx>, args: ProjectSearch) -> Result<ProjectsFound> {
    let query = &args.query;
    let projects = ctx.projects.list().await;
    let found = projects::matching(&projects, query);
    let listed = found
        .iter()
        .take(FOUND)
        .map(|&(_, path)| {
            let (git, beads) = projects::kinds(Path::new(path));
            ProjectSummary {
                path: path.into(),
                name: path.rsplit(['/', '\\']).next().unwrap_or(path).into(),
                git,
                beads,
            }
        })
        .collect();
    let none = found.is_empty().then(|| {
        let roots: Vec<_> = ctx
            .projects
            .roots()
            .iter()
            .map(|r| r.display().to_string())
            .collect();
        format!(
            "no project under {} matches {query:?}; give the folder's absolute path",
            roots.join(", ")
        )
    });
    Ok(ProjectsFound {
        node: ctx.node.clone(),
        projects: listed,
        more: found.len().saturating_sub(FOUND),
        none,
    })
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
        [] => bail!("no project matches {project:?}; project search lists them, or give its path"),
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

/// `handoff launch`: a coding agent in a session of its own, in the
/// project's folder, given the brief as its first message.
pub async fn start(ctx: Arc<Ctx>, args: HandoffLaunch) -> Result<HandoffStarted> {
    let begun = Instant::now();
    let deadline = begun + START_WAIT;
    let brief = args.brief.trim();
    if brief.is_empty() {
        bail!("brief is empty: say what the agent is to do");
    }
    let brief = match args.issue.as_deref() {
        Some(issue) => format!("{brief}\n\nYour issue: {}", issue.trim()),
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
    let harness = match args.harness.as_deref() {
        Some(id) => *agents
            .iter()
            .find(|h| h.id == id)
            .with_context(|| harness_choice(id, &agents))?,
        None => *agents
            .first()
            .context("the launcher has no agent harness; add one to launch.conf")?,
    };
    let folder = folder(&ctx, &args.project).await?;
    let name = args.name.unwrap_or_else(|| session_name(&ctx, &folder));
    check_name(&name)?;
    let launch = Launch {
        harness: harness.id.clone(),
        folder: folder.clone(),
        name,
        flags: harness.flags.clone(),
        user: String::new(),
        install_completion_hooks: true,
        size: None,
    };
    let (session, warning) = ctx.hub.launch(&launcher, &launch).await?;
    let info = find(&ctx, &session)?;
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
                let now = look(&ctx, &session, pane, kind);
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
    let mut now = look(&ctx, &session, pane, kind);
    if sent.is_ok() {
        while now.state == State::Idle && Instant::now() < deadline {
            next_look(&mut changed).await;
            now = look(&ctx, &session, pane, kind);
        }
    }
    let follow = format!(
        "Follow it with handoff_show (session {}, wait up to {MAX_WAIT} seconds while it works) \
         and answer it with handoff_reply; from a shell, ahjo handoff show {} --wait 60.",
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
    Ok(HandoffStarted {
        node: ctx.node.clone(),
        detail: detail(&info, &now, TAIL),
        session: info.id,
        state: now.state,
        folder,
        harness: harness.id.clone(),
        warning,
        brief_sent: sent.is_ok(),
        say: said,
        follow,
    })
}

fn harness_choice(id: &str, agents: &[&crate::launch::Harness]) -> String {
    let ids: Vec<&str> = agents.iter().map(|h| h.id.as_str()).collect();
    format!(
        "no agent harness {id:?}; the launcher has {}",
        ids.join(", ")
    )
}

/// `handoff show`: where the agent is; with `wait`, once it stops
/// working or the wait ends.
pub async fn status(ctx: Arc<Ctx>, args: HandoffShow) -> Result<HandoffStatusResult> {
    let wait = args.wait.unwrap_or_default().min(MAX_WAIT);
    let lines = args.lines.map_or(TAIL, |lines| lines.get());
    let wanted = args.session;
    let Ok(info) = find(&ctx, &wanted) else {
        return Ok(HandoffStatusResult {
            node: ctx.node.clone(),
            say: Some(format!(
                "no session {wanted} on this node: it has ended, or never was; session_list shows those there are"
            )),
            session: wanted,
            state: State::Gone,
            detail: None,
            waited_seconds: None,
        });
    };
    let pane = ctx.hub.active_pane(&info.id).await?;
    let _follow = ctx.hub.subscribe(pane);
    let mut changed = ctx.hub.watch();
    let _ = visible(&ctx, pane).await;
    let begun = Instant::now();
    let deadline = begun + Duration::from_secs(wait);
    let mut now = look(&ctx, &info.id, pane, None);
    while now.state == State::Working && Instant::now() < deadline {
        tokio::select! {
            () = next_look(&mut changed) => {}
            () = sleep_until(deadline) => {}
        }
        now = look(&ctx, &info.id, pane, None);
    }
    Ok(HandoffStatusResult {
        node: ctx.node.clone(),
        detail: Some(detail(&info, &now, lines)),
        session: info.id,
        state: now.state,
        say: None,
        waited_seconds: (wait > 0).then(|| begun.elapsed().as_secs()),
    })
}

/// `handoff reply`: the text typed into the agent and submitted, or, while
/// its screen asks with a menu, the option's key pressed.
pub async fn reply(ctx: Arc<Ctx>, args: HandoffReply) -> Result<HandoffReplied> {
    let text = args.text;
    if text.trim().is_empty() {
        bail!("text is empty");
    }
    if text.len() > MAX_SEND {
        bail!("text is longer than {MAX_SEND} bytes");
    }
    let (info, pane) = target(&ctx, &args.session, None).await?;
    let _follow = ctx.hub.subscribe(pane);
    let before = look(&ctx, &info.id, pane, None);
    let pressed = match (&before.state, &before.question) {
        (State::Gone, _) => bail!("{} has ended", info.name),
        (State::NeedsInput, Some(question)) => {
            let key = text.trim();
            if !(key.len() == 1 && key.bytes().all(|b| b.is_ascii_alphanumeric())) {
                bail!(
                    "{} asks {question:?} and waits on its menu: reply with the number of an \
                     option, as handoff_show shows the screen",
                    info.name
                );
            }
            ctx.hub.send_input(pane, key.as_bytes().to_vec()).await?;
            true
        }
        _ => {
            ctx.hub.deliver(pane, text, true).await?.await?;
            false
        }
    };
    // Until the agent has taken it: its screen changed from what it was.
    let mut changed = ctx.hub.watch();
    let deadline = Instant::now() + REPLY_WAIT;
    let mut now = look(&ctx, &info.id, pane, None);
    while (now.state == before.state && now.question == before.question)
        && now.state != State::Working
        && Instant::now() < deadline
    {
        next_look(&mut changed).await;
        now = look(&ctx, &info.id, pane, None);
    }
    // A moment more for a menu's answer to be drawn over.
    if pressed && now.state == State::NeedsInput {
        sleep(LOOK_EVERY).await;
        now = look(&ctx, &info.id, pane, None);
    }
    Ok(HandoffReplied {
        node: ctx.node.clone(),
        detail: detail(&info, &now, TAIL),
        session: info.id,
        state: now.state,
        sent: if pressed {
            ReplyKind::Key
        } else {
            ReplyKind::Text
        },
    })
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
