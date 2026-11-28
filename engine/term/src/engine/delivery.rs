//! Send: text pasted into a pane as one paste, then Enter if asked.
//!
//! Agent TUIs can swallow a submit key that arrives in the same burst as a
//! large bracketed paste, so Enter waits until the paste has settled, as
//! CHROTE's does (`tmux_send.go`, "harden prompt submit"). Before pressing
//! it, the pane must still run the process the text went to: a respawned pane
//! keeps its id but not its pid. The server and the pane id cannot change
//! under us, because the control client that sends is attached to that server
//! and tmux never reuses a pane id within one server.
//!
//! A session delivers one text at a time, in the order asked, so a second
//! paste never lands between the first and its Enter. Typing keeps its place
//! in that order: keys typed while a delivery is in flight or waiting go to
//! the pane after it, so they never become part of the submitted text.
//! A delivery whose waiter has gone before its turn is dropped unsent, so
//! one given up on (a deadline passed) never pastes late; one that has
//! started finishes, and its waiter can tell which it was ([`Delivering`]).
//!
//! A text that went in without its Enter is neither delivered nor refused:
//! the operator must look before sending it again, so it is
//! [`NotSubmitted`], which a waiter can tell from a refusal.
//!
//! A pane that was just launched may not be ready for a paste (#242): a
//! paste before its program has asked for bracketed paste arrives as typed
//! lines, so each line break submits part of the text. A delivery asked to
//! wait first waits, up to its limit, until the pane's program has turned
//! bracketed paste on, its screen has been quiet for [`READY_QUIET`], and
//! nothing on it asks the operator a question (a paste would answer it). A
//! pane that never gets there gets nothing, and its waiter hears
//! [`NotReady`]. Bracketed paste is the signal because it is what makes a
//! multi-line paste one paste: shells (bash, zsh, fish) and agent TUIs
//! (Claude Code, Codex) turn it on while they read a prompt, and a shell
//! turns it off while the command it started runs.

use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use anyhow::{Result, bail};

use tokio::sync::oneshot;
use tokio::time::Instant;

use super::{Pending, send};
use crate::control::{self, PaneId};

/// How long a paste settles before Enter: CHROTE's proven delay.
pub(super) const SUBMIT_SETTLE: Duration = Duration::from_millis(1200);

/// How long a pane waited on must have written nothing before it counts as
/// ready: a program that has just turned bracketed paste on is still
/// drawing its first screen.
pub(super) const READY_QUIET: Duration = Duration::from_millis(800);

/// How a delivery paces itself: an agent's TUI needs its screen quiet
/// before a paste and a settled paste before Enter; a shell that was just
/// started for a launch needs neither, since its line editor reads the
/// moment it turns bracketed paste on and takes a paste and Enter in one
/// burst.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Pace {
    Agent,
    FreshShell,
}

impl Pace {
    fn quiet(self) -> Duration {
        match self {
            Pace::Agent => READY_QUIET,
            Pace::FreshShell => Duration::ZERO,
        }
    }

    fn settle(self) -> Duration {
        match self {
            Pace::Agent => SUBMIT_SETTLE,
            Pace::FreshShell => Duration::ZERO,
        }
    }
}

/// What became of a delivery.
#[derive(Debug)]
pub(super) enum Heard {
    /// tmux took the text, and the Enter if asked.
    Delivered,
    /// Nothing went in, or only the Enter of an empty text failed.
    Refused,
    /// The text was pasted and its Enter was not pressed.
    Pasted(Option<PaneInputBlocked>),
    /// A tmux mode would intercept input before any text went in.
    Blocked(PaneInputBlocked),
    /// The pane never got ready for the paste: nothing went in.
    NotReady,
}

/// tmux names its active pane mode, or reports that input is disabled.
#[derive(Debug)]
pub struct PaneInputBlocked(String);

impl std::fmt::Display for PaneInputBlocked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0.as_str() {
            "input-disabled" => f.write_str("tmux pane input is disabled"),
            mode => write!(f, "tmux pane is in {mode}"),
        }
    }
}

impl std::error::Error for PaneInputBlocked {}

/// A paste may be in the pane and its submission has not been confirmed.
#[derive(Debug)]
pub struct NotSubmitted(Option<PaneInputBlocked>);

impl std::fmt::Display for NotSubmitted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(blocker) = &self.0 {
            write!(
                f,
                "tmux accepted the paste but Ahjo withheld Enter because {blocker}"
            )?;
        } else {
            f.write_str("the text may be in the pane and submission could not be confirmed")?;
        }
        f.write_str(". Inspect the pane before sending again")
    }
}

impl std::error::Error for NotSubmitted {}

// Keep the guard and the diagnostic read on the same tmux format.
pub(super) const INPUT_BLOCKER: &str =
    "#{?pane_input_off,input-disabled,#{?pane_in_mode,#{?pane_mode,#{pane_mode},pane-mode},}}";

fn guarded_input(pane: PaneId, pid: Option<&str>, command: &str) -> String {
    let available = format!("#{{==:{INPUT_BLOCKER},}}");
    let condition = match pid {
        Some(pid) => format!("#{{&&:{available},#{{==:#{{pane_pid}},{pid}}}}}"),
        None => available,
    };
    format!(
        "if-shell -F -t %{pane} '{condition}' '{command}' 'display-message -p -t %{pane} \"#{{?{INPUT_BLOCKER},blocked:{INPUT_BLOCKER},target-changed}}\"'"
    )
}

/// The error of a delivery that waited for its pane to get ready for a
/// paste, which did not in time or asks the operator something. Nothing
/// was pasted, but the pane is not as the sender expected: look at it
/// before sending again.
#[derive(Debug)]
pub struct NotReady;

impl std::fmt::Display for NotReady {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the pane did not get ready for a paste; nothing was pasted")
    }
}

impl std::error::Error for NotReady {}

/// What a delivery waiting on its pane saw when it looked.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Readiness {
    /// Bracketed paste is on, and the screen asks nothing.
    Ready,
    /// The screen asks the operator something; a paste would answer it.
    Asking,
    /// Not seeded yet, or bracketed paste is off.
    NotYet,
    /// The pane is gone.
    Gone,
}

/// A text to deliver. `done` hears whether tmux took the text, and pressed
/// Enter if asked, against the pane it was meant for.
pub(super) struct Delivery {
    pub pane: PaneId,
    pub text: String,
    pub enter: bool,
    /// Wait up to this long for the pane to get ready for a paste.
    pub ready: Option<Duration>,
    pub pace: Pace,
    pub done: oneshot::Sender<Heard>,
    /// [`WAITING`], then [`STARTED`] when its turn comes or [`WITHDRAWN`]
    /// if its waiter gives up first: whichever happens first holds.
    pub turn: Arc<AtomicU8>,
}

const WAITING: u8 = 0;
const STARTED: u8 = 1;
const WITHDRAWN: u8 = 2;

impl Delivery {
    /// A delivery and what its waiter holds.
    pub fn new(
        pane: PaneId,
        text: String,
        enter: bool,
        ready: Option<Duration>,
    ) -> (Delivery, Delivering) {
        Delivery::paced(pane, text, enter, ready, Pace::Agent)
    }

    /// A delivery into a shell just started for a launch: it waits up to
    /// `wait` for the shell's line editor, then pastes and submits at once.
    pub fn into_fresh_shell(pane: PaneId, text: String, wait: Duration) -> (Delivery, Delivering) {
        Delivery::paced(pane, text, true, Some(wait), Pace::FreshShell)
    }

    fn paced(
        pane: PaneId,
        text: String,
        enter: bool,
        ready: Option<Duration>,
        pace: Pace,
    ) -> (Delivery, Delivering) {
        let (done, heard) = oneshot::channel();
        let turn = Arc::new(AtomicU8::new(WAITING));
        let delivering = Delivering {
            heard,
            turn: turn.clone(),
        };
        (
            Delivery {
                pane,
                text,
                enter,
                ready,
                pace,
                done,
                turn,
            },
            delivering,
        )
    }
}

/// A queued delivery's outcome: whether tmux took the text, and pressed
/// Enter if asked, against the pane it was meant for. Dropping it before
/// the delivery's turn withdraws the delivery.
pub struct Delivering {
    heard: oneshot::Receiver<Heard>,
    turn: Arc<AtomicU8>,
}

impl Delivering {
    /// A delivery already over, taken or refused: what a stand-in for the
    /// engine answers, so its callers can be tested without a pane.
    pub fn settled(delivered: bool) -> Delivering {
        let (done, heard) = oneshot::channel();
        let _ = done.send(if delivered {
            Heard::Delivered
        } else {
            Heard::Refused
        });
        Delivering {
            heard,
            turn: Arc::new(AtomicU8::new(STARTED)),
        }
    }

    /// Whether tmux has been asked yet.
    pub fn started(&self) -> bool {
        self.turn.load(Ordering::Acquire) == STARTED
    }

    /// Withdraws the delivery if its turn has not come: true if it will
    /// never be sent, false if it has started and runs to its end.
    pub fn withdraw(&self) -> bool {
        match self
            .turn
            .compare_exchange(WAITING, WITHDRAWN, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => true,
            Err(now) => now == WITHDRAWN,
        }
    }
}

impl Drop for Delivering {
    fn drop(&mut self) {
        self.withdraw();
    }
}

impl Future for Delivering {
    type Output = Result<()>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<()>> {
        Pin::new(&mut self.heard).poll(cx).map(|heard| match heard {
            Ok(Heard::Delivered) => Ok(()),
            Ok(Heard::Refused) => bail!("tmux refused the text"),
            Ok(Heard::Pasted(blocker)) => Err(NotSubmitted(blocker).into()),
            Ok(Heard::Blocked(blocker)) => {
                Err(anyhow::Error::new(blocker).context(
                    "no input was sent. Inspect the pane and let the operator leave the mode or restore input",
                ))
            }
            Ok(Heard::NotReady) => Err(NotReady.into()),
            Err(_) => bail!("session is gone"),
        })
    }
}

/// What a command of a delivery was, so its reply can be handled.
pub(super) enum Step {
    /// The pane's pid before the paste.
    Pid,
    /// Loading and pasting the buffer.
    Paste,
    /// The tmux guard and its selected branch each reply in control mode.
    Guard,
    GuardResult,
    Submit,
}

enum Phase {
    /// Waiting for the pane to get ready for the paste, until `deadline`.
    /// The pane last wrote at `quiet_since`; the next look is at `check`.
    Readying {
        deadline: Instant,
        quiet_since: Instant,
        check: Instant,
    },
    Pasting,
    /// Waiting until then to press Enter.
    Settling(Instant),
    Submitting,
}

struct Current {
    pane: PaneId,
    /// The text, held here while the pane gets ready.
    text: String,
    enter: bool,
    pace: Pace,
    done: oneshot::Sender<Heard>,
    phase: Phase,
    /// Replies still to come in this phase.
    outstanding: usize,
    pid: Option<String>,
    failed: bool,
    /// Text was asked to be pasted and tmux took it.
    pasting: bool,
    paste_failed: bool,
    blocked: Option<PaneInputBlocked>,
}

impl Current {
    fn heard(&mut self) -> Heard {
        if !self.failed {
            Heard::Delivered
        } else if self.pasting && !self.paste_failed {
            Heard::Pasted(self.blocked.take())
        } else if let Some(blocker) = self.blocked.take() {
            Heard::Blocked(blocker)
        } else {
            Heard::Refused
        }
    }

    /// Asks tmux for the pane's pid and to paste the text, from the
    /// session's own buffer, so sessions delivering at once never paste
    /// each other's text.
    fn paste(
        &mut self,
        session: &str,
        has_pane_modes: bool,
        out: &mut String,
        pending: &mut VecDeque<Pending>,
    ) {
        let text = std::mem::take(&mut self.text);
        let pane = self.pane;
        let buffer = buffer_name(session);
        let paste = format!("paste-buffer -p -d -b {buffer} -t %{pane}");
        let paste = if has_pane_modes {
            (guarded_input(pane, None, &paste), Step::Guard)
        } else {
            (paste, Step::Paste)
        };
        let steps = [
            (pid_command(pane), Step::Pid),
            (
                format!("set-buffer -b {buffer} -- {}", control::quote(&text)),
                Step::Paste,
            ),
            paste,
        ];
        self.phase = Phase::Pasting;
        self.pasting = true;
        self.outstanding = steps.len();
        for (command, step) in steps {
            send(out, pending, command, Pending::Delivery(step));
        }
    }
}

/// What waits its turn behind the delivery in flight.
enum Queued {
    Delivery(Delivery),
    /// Keys typed into a pane, held so they keep their place.
    Input(PaneId, Vec<u8>),
}

/// A session's deliveries: the one in flight and what waits its turn.
#[derive(Default)]
pub(super) struct Deliveries {
    has_pane_modes: bool,
    queue: VecDeque<Queued>,
    current: Option<Current>,
}

impl Deliveries {
    pub fn new(has_pane_modes: bool) -> Self {
        Self {
            has_pane_modes,
            current: None,
            queue: VecDeque::new(),
        }
    }

    /// Queues a delivery, and starts it if none is in flight.
    pub fn push(
        &mut self,
        session: &str,
        delivery: Delivery,
        out: &mut String,
        pending: &mut VecDeque<Pending>,
    ) {
        self.queue.push_back(Queued::Delivery(delivery));
        if self.current.is_none() {
            self.start_next(session, out, pending);
        }
    }

    /// Keys typed into `pane`: sent now when no delivery is in flight or
    /// waiting, else after them, in the order they came.
    pub fn input(
        &mut self,
        pane: PaneId,
        data: Vec<u8>,
        out: &mut String,
        pending: &mut VecDeque<Pending>,
    ) {
        if self.current.is_none() && self.queue.is_empty() {
            send(
                out,
                pending,
                control::send_keys_hex(pane, &data),
                Pending::Input,
            );
        } else {
            self.queue.push_back(Queued::Input(pane, data));
        }
    }

    /// When Enter is due, while a paste settles.
    pub fn due(&self) -> Option<Instant> {
        match self.current.as_ref()?.phase {
            Phase::Settling(at) => Some(at),
            _ => None,
        }
    }

    /// The pane a delivery waits on to get ready, and when to look at it.
    pub fn readying(&self) -> Option<(PaneId, Instant)> {
        let current = self.current.as_ref()?;
        match current.phase {
            Phase::Readying { check, .. } => Some((current.pane, check)),
            _ => None,
        }
    }

    /// `pane` wrote at `now`: if a delivery waits on it, it is not quiet.
    pub fn heard_output(&mut self, pane: PaneId, now: Instant) {
        let Some(current) = self.current.as_mut().filter(|c| c.pane == pane) else {
            return;
        };
        if let Phase::Readying {
            deadline,
            quiet_since,
            check,
        } = &mut current.phase
        {
            *quiet_since = now;
            *check = (now + current.pace.quiet()).min(*deadline);
        }
    }

    /// The pane waited on looked like `readiness` at `now`: paste once it is
    /// ready and has been quiet, give up when it asks a question or the
    /// time is up, or look again later.
    pub fn looked(
        &mut self,
        session: &str,
        readiness: Readiness,
        now: Instant,
        out: &mut String,
        pending: &mut VecDeque<Pending>,
    ) {
        let Some(current) = self.current.as_mut() else {
            return;
        };
        let Phase::Readying {
            deadline,
            quiet_since,
            ..
        } = current.phase
        else {
            return;
        };
        let quiet_for = current.pace.quiet();
        let quiet = now >= quiet_since + quiet_for;
        let heard = match readiness {
            Readiness::Gone => Heard::Refused,
            Readiness::Ready if quiet => {
                current.paste(session, self.has_pane_modes, out, pending);
                return;
            }
            Readiness::Asking if quiet => Heard::NotReady,
            _ if now >= deadline => Heard::NotReady,
            _ => {
                // Only output changes what the pane shows, and output looks
                // again sooner.
                let next = if quiet {
                    deadline
                } else {
                    (quiet_since + quiet_for).min(deadline)
                };
                current.phase = Phase::Readying {
                    deadline,
                    quiet_since,
                    check: next,
                };
                return;
            }
        };
        let current = self.current.take().expect("a delivery in flight");
        let _ = current.done.send(heard);
        self.start_next(session, out, pending);
    }

    /// The paste has settled: checks the pane, then presses Enter.
    pub fn settled(&mut self, out: &mut String, pending: &mut VecDeque<Pending>) {
        let Some(current) = self.current.as_mut() else {
            return;
        };
        if !matches!(current.phase, Phase::Settling(_)) {
            return;
        }
        current.phase = Phase::Submitting;
        current.outstanding = 1;
        let Some(pid) = current
            .pid
            .as_deref()
            .filter(|pid| pid.bytes().all(|b| b.is_ascii_digit()) && !pid.is_empty())
        else {
            current.failed = true;
            current.outstanding = 0;
            return;
        };
        let pane = current.pane;
        let enter = format!("send-keys -t %{pane} Enter");
        let command = if self.has_pane_modes {
            guarded_input(pane, Some(pid), &enter)
        } else {
            format!(
                "if-shell -F -t %{pane} '#{{==:#{{pane_pid}},{pid}}}' '{enter}' 'display-message -p target-changed'"
            )
        };
        send(out, pending, command, Pending::Delivery(Step::Guard));
    }

    /// Handles the reply to a delivery's command.
    pub fn replied(
        &mut self,
        session: &str,
        step: Step,
        ok: bool,
        lines: &[String],
        out: &mut String,
        pending: &mut VecDeque<Pending>,
    ) {
        let Some(current) = self.current.as_mut() else {
            return;
        };
        current.failed |= !ok;
        current.outstanding -= 1;
        match step {
            Step::Pid => current.pid = lines.first().cloned().filter(|_| ok),
            Step::Guard if ok => {
                current.outstanding += 1;
                pending.push_front(Pending::Delivery(Step::GuardResult));
            }
            Step::Guard | Step::GuardResult => {
                if let Some(reason) = lines.first().and_then(|line| line.strip_prefix("blocked:")) {
                    current.blocked = Some(PaneInputBlocked(reason.to_owned()));
                }
                let branch_failed = !ok || !lines.is_empty();
                current.failed |= branch_failed;
                if branch_failed && matches!(current.phase, Phase::Pasting) {
                    current.paste_failed = true;
                    // A refusal leaves our named buffer behind. Remove only that buffer.
                    let buffer = buffer_name(session);
                    send(
                        out,
                        pending,
                        format!("delete-buffer -b {buffer}"),
                        Pending::Input,
                    );
                }
            }
            Step::Paste => current.paste_failed |= !ok,
            Step::Submit => {}
        }
        if current.outstanding > 0 {
            return;
        }
        if matches!(current.phase, Phase::Pasting) && current.enter && !current.failed {
            if !current
                .pid
                .as_ref()
                .is_some_and(|pid| !pid.is_empty() && pid.bytes().all(|b| b.is_ascii_digit()))
            {
                current.failed = true;
            } else {
                current.phase = Phase::Settling(Instant::now() + current.pace.settle());
                return;
            }
        }
        let mut current = self.current.take().expect("a delivery in flight");
        let heard = current.heard();
        let _ = current.done.send(heard);
        self.start_next(session, out, pending);
    }

    fn start_next(&mut self, session: &str, out: &mut String, pending: &mut VecDeque<Pending>) {
        while let Some(next) = self.queue.pop_front() {
            let Delivery {
                pane,
                text,
                enter,
                ready,
                pace,
                done,
                turn,
            } = match next {
                Queued::Delivery(d) => d,
                Queued::Input(pane, data) => {
                    send(
                        out,
                        pending,
                        control::send_keys_hex(pane, &data),
                        Pending::Input,
                    );
                    continue;
                }
            };
            if turn
                .compare_exchange(WAITING, STARTED, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
            {
                // Its waiter gave up before its turn: never sent.
                continue;
            }
            let mut current = Current {
                pane,
                text,
                enter,
                pace,
                done,
                phase: Phase::Pasting,
                outstanding: 0,
                pid: None,
                failed: false,
                pasting: false,
                paste_failed: false,
                blocked: None,
            };
            match ready {
                _ if current.text.is_empty() => {}
                Some(wait) => {
                    let now = Instant::now();
                    current.phase = Phase::Readying {
                        deadline: now + wait,
                        quiet_since: now,
                        check: now + pace.quiet().min(wait),
                    };
                }
                None => current.paste(session, self.has_pane_modes, out, pending),
            }
            if current.text.is_empty() && !current.pasting && enter {
                // Nothing pasted, so nothing to settle.
                current.phase = Phase::Submitting;
                current.outstanding = 1;
                let pane = current.pane;
                let enter = format!("send-keys -t %{pane} Enter");
                let (command, step) = if self.has_pane_modes {
                    (guarded_input(pane, None, &enter), Step::Guard)
                } else {
                    (enter, Step::Submit)
                };
                send(out, pending, command, Pending::Delivery(step));
            } else if current.text.is_empty() && !current.pasting {
                let _ = current.done.send(Heard::Delivered);
                continue;
            }
            self.current = Some(current);
            return;
        }
    }
}

/// The session's engine went (the node stopping, its server gone) with a
/// delivery in flight: once its paste was asked for, the text may be in
/// the pane, so its waiter hears it pasted unsubmitted, not refused. Those
/// still queued, or waiting for their pane to get ready, were never sent.
impl Drop for Deliveries {
    fn drop(&mut self) {
        if let Some(current) = self.current.take() {
            let heard = if current.pasting && !current.paste_failed {
                Heard::Pasted(None)
            } else {
                Heard::Refused
            };
            let _ = current.done.send(heard);
        }
    }
}

fn buffer_name(session: &str) -> String {
    format!("ahjo-send-{}", session.trim_start_matches('$'))
}

fn pid_command(pane: PaneId) -> String {
    format!("display-message -p -t %{pane} '#{{pane_pid}}'")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guarded_submit_queues_branch_reply_before_later_commands() {
        let (delivery, result) = Delivery::new(4, "hello".into(), true, None);
        let mut deliveries = Deliveries::default();
        let mut out = String::new();
        let mut pending = VecDeque::new();
        deliveries.push("$1", delivery, &mut out, &mut pending);
        pending.clear();
        deliveries.replied(
            "$1",
            Step::Pid,
            true,
            &["123".into()],
            &mut out,
            &mut pending,
        );
        deliveries.replied("$1", Step::Paste, true, &[], &mut out, &mut pending);
        deliveries.replied("$1", Step::Paste, true, &[], &mut out, &mut pending);
        out.clear();
        deliveries.settled(&mut out, &mut pending);
        assert!(out.contains("if-shell -F -t %4 '#{==:#{pane_pid},123}' 'send-keys -t %4 Enter'"));
        pending.pop_front();
        pending.push_back(Pending::Activity);
        deliveries.replied("$1", Step::Guard, true, &[], &mut out, &mut pending);
        assert!(matches!(
            pending.pop_front(),
            Some(Pending::Delivery(Step::GuardResult))
        ));
        deliveries.replied(
            "$1",
            Step::GuardResult,
            true,
            &["target-changed".into()],
            &mut out,
            &mut pending,
        );
        let error = futures_block(result).unwrap_err();
        assert!(error.is::<NotSubmitted>(), "pasted, not submitted: {error}");
        assert!(matches!(pending.pop_front(), Some(Pending::Activity)));
    }

    /// A paste tmux refused put nothing in the pane: refused, not unknown.
    #[test]
    fn a_refused_paste_is_a_refusal_not_an_unknown() {
        let (delivery, result) = Delivery::new(4, "hello".into(), true, None);
        let mut deliveries = Deliveries::default();
        let mut out = String::new();
        let mut pending = VecDeque::new();
        deliveries.push("$1", delivery, &mut out, &mut pending);
        let lines = ["123".to_string()];
        deliveries.replied("$1", Step::Pid, true, &lines, &mut out, &mut pending);
        deliveries.replied("$1", Step::Paste, true, &[], &mut out, &mut pending);
        deliveries.replied("$1", Step::Paste, false, &[], &mut out, &mut pending);
        let error = futures_block(result).unwrap_err();
        assert!(!error.is::<NotSubmitted>(), "nothing went in: {error}");
    }

    /// Lane 183, seen on screen: a node stopped while a send settled had
    /// pasted the text, yet said the session was gone and the pane
    /// unchanged. In flight is unknown; only the queued were never sent.
    #[test]
    fn a_send_in_flight_when_the_engine_goes_is_unknown() {
        let (first, in_flight) = Delivery::new(4, "hello".into(), true, None);
        let (second, queued) = Delivery::new(4, "later".into(), true, None);
        let mut deliveries = Deliveries::default();
        let mut out = String::new();
        let mut pending = VecDeque::new();
        deliveries.push("$1", first, &mut out, &mut pending);
        deliveries.push("$1", second, &mut out, &mut pending);
        drop(deliveries);
        let error = futures_block(in_flight).unwrap_err();
        assert!(error.is::<NotSubmitted>(), "may be in the pane: {error}");
        let error = futures_block(queued).unwrap_err();
        assert!(!error.is::<NotSubmitted>(), "never sent: {error}");
    }

    /// Review 13: a scheduled send given up at its deadline while queued
    /// behind another must never paste afterwards.
    #[test]
    fn a_delivery_given_up_before_its_turn_is_never_sent() {
        let mut deliveries = Deliveries::default();
        let mut out = String::new();
        let mut pending = VecDeque::new();
        let (first, first_heard) = Delivery::new(4, "first".into(), false, None);
        let (second, second_heard) = Delivery::new(4, "second".into(), true, None);
        let (third, third_heard) = Delivery::new(4, "third".into(), false, None);
        deliveries.push("$1", first, &mut out, &mut pending);
        deliveries.push("$1", second, &mut out, &mut pending);
        deliveries.push("$1", third, &mut out, &mut pending);
        assert!(first_heard.started());
        assert!(!second_heard.started());
        drop(second_heard);
        out.clear();
        for step in [Step::Pid, Step::Paste, Step::Paste] {
            let lines = ["7".to_string()];
            deliveries.replied("$1", step, true, &lines, &mut out, &mut pending);
        }
        assert!(futures_block(first_heard).is_ok());
        assert!(!out.contains("second"), "withdrawn, never pasted: {out}");
        assert!(out.contains("third"), "the next one in line goes: {out}");
        assert!(third_heard.started());
    }

    /// Review: keys typed while a paste settled reached the pane before its
    /// Enter and were submitted with the text. They wait their turn now,
    /// and keys typed before the next delivery go before it.
    #[test]
    fn typing_during_a_delivery_goes_after_its_enter() {
        let mut deliveries = Deliveries::default();
        let mut out = String::new();
        let mut pending = VecDeque::new();
        deliveries.input(4, b"a".to_vec(), &mut out, &mut pending);
        assert!(out.contains("send-keys -t %4 -H 61"), "at once: {out}");
        let (first, first_heard) = Delivery::new(4, "first".into(), true, None);
        let (second, _second_heard) = Delivery::new(4, "second".into(), false, None);
        deliveries.push("$1", first, &mut out, &mut pending);
        let lines = ["7".to_string()];
        for step in [Step::Pid, Step::Paste, Step::Paste] {
            deliveries.replied("$1", step, true, &lines, &mut out, &mut pending);
        }
        assert!(deliveries.due().is_some(), "settling");
        out.clear();
        deliveries.input(4, b"x".to_vec(), &mut out, &mut pending);
        deliveries.push("$1", second, &mut out, &mut pending);
        deliveries.input(4, b"y".to_vec(), &mut out, &mut pending);
        assert!(out.is_empty(), "held while the paste settles: {out}");
        deliveries.settled(&mut out, &mut pending);
        deliveries.replied("$1", Step::Guard, true, &[], &mut out, &mut pending);
        deliveries.replied("$1", Step::GuardResult, true, &[], &mut out, &mut pending);
        assert!(futures_block(first_heard).is_ok());
        let enter = out.find("send-keys -t %4 Enter").expect("Enter");
        let x = out.find("-H 78").expect("x sent");
        let second = out.find("second").expect("second pasted");
        assert!(enter < x && x < second, "in order: {out}");
        assert!(!out.contains("-H 79"), "y waits for the second: {out}");
        for step in [Step::Pid, Step::Paste, Step::Paste] {
            deliveries.replied("$1", step, true, &lines, &mut out, &mut pending);
        }
        assert!(out.contains("-H 79"), "then y: {out}");
    }

    /// #242: a delivery that waits pastes nothing until its pane is ready
    /// and has been quiet, and output pushes the paste back.
    #[test]
    fn a_waiting_delivery_pastes_once_its_pane_is_ready_and_quiet() {
        let mut deliveries = Deliveries::default();
        let mut out = String::new();
        let mut pending = VecDeque::new();
        let wait = Duration::from_secs(30);
        let (delivery, _heard) = Delivery::new(4, "a\nb".into(), true, Some(wait));
        let start = Instant::now();
        deliveries.push("$1", delivery, &mut out, &mut pending);
        assert!(out.is_empty(), "nothing asked of tmux yet: {out}");
        let (pane, check) = deliveries.readying().expect("waits");
        assert_eq!(pane, 4);
        assert!(check >= start + READY_QUIET);

        // Ready already, but not yet quiet for long enough.
        let early = start + READY_QUIET / 2;
        deliveries.looked("$1", Readiness::Ready, early, &mut out, &mut pending);
        assert!(out.is_empty());
        // Output from another pane changes nothing; from this one, it does.
        deliveries.heard_output(5, early);
        assert_eq!(deliveries.readying().unwrap().1, check);
        deliveries.heard_output(4, early);
        let later = deliveries.readying().unwrap().1;
        assert_eq!(later, early + READY_QUIET);
        // Quiet now, but the program has not asked for bracketed paste.
        deliveries.looked("$1", Readiness::NotYet, later, &mut out, &mut pending);
        assert!(out.is_empty());
        assert!(deliveries.readying().unwrap().1 >= start + wait);
        deliveries.looked("$1", Readiness::Ready, later, &mut out, &mut pending);
        assert!(
            out.contains("paste-buffer -p -d -b ahjo-send-1 -t %4"),
            "{out}"
        );
        assert!(deliveries.readying().is_none());
    }

    /// ahjo-cdi: a launch's command line goes into its fresh shell the
    /// moment the line editor reads, and Enter follows at once.
    #[test]
    fn a_fresh_shell_takes_its_command_line_without_a_fixed_wait() {
        let mut deliveries = Deliveries::default();
        let mut out = String::new();
        let mut pending = VecDeque::new();
        let (delivery, heard) =
            Delivery::into_fresh_shell(4, "claude --x".into(), Duration::from_secs(30));
        let start = Instant::now();
        deliveries.push("$1", delivery, &mut out, &mut pending);
        assert!(
            deliveries.readying().unwrap().1 <= Instant::now(),
            "looked at once"
        );
        // The shell is still reading its rc files.
        deliveries.looked("$1", Readiness::NotYet, start, &mut out, &mut pending);
        assert!(out.is_empty());
        // Its prompt turns bracketed paste on: looked at with no quiet wait.
        deliveries.heard_output(4, start);
        assert_eq!(deliveries.readying().unwrap().1, start);
        deliveries.looked("$1", Readiness::Ready, start, &mut out, &mut pending);
        assert!(out.contains("claude --x"), "{out}");
        let lines = ["7".to_string()];
        for step in [Step::Pid, Step::Paste, Step::Paste] {
            deliveries.replied("$1", step, true, &lines, &mut out, &mut pending);
        }
        assert!(
            deliveries.due().is_some_and(|at| at <= Instant::now()),
            "Enter is due at once"
        );
        deliveries.settled(&mut out, &mut pending);
        assert!(out.contains("send-keys -t %4 Enter"), "{out}");
        deliveries.replied("$1", Step::Guard, true, &[], &mut out, &mut pending);
        deliveries.replied("$1", Step::GuardResult, true, &[], &mut out, &mut pending);
        assert!(futures_block(heard).is_ok());
    }

    #[test]
    fn an_unready_pane_never_receives_a_prompt_refresh() {
        let mut deliveries = Deliveries::default();
        let mut out = String::new();
        let mut pending = VecDeque::new();
        let (delivery, _heard) =
            Delivery::new(4, "a\nb".into(), false, Some(Duration::from_secs(10)));
        deliveries.push("$1", delivery, &mut out, &mut pending);
        let check = deliveries.readying().unwrap().1;
        deliveries.looked("$1", Readiness::NotYet, check, &mut out, &mut pending);
        assert!(out.is_empty());
    }

    /// #242: a pane that never gets ready, or asks a question, gets nothing,
    /// and the waiter hears that; the next delivery in line goes.
    #[test]
    fn a_pane_that_never_gets_ready_gets_nothing() {
        for (readiness, at) in [
            (Readiness::NotYet, Duration::from_secs(4)),
            (Readiness::Asking, READY_QUIET + Duration::from_millis(100)),
        ] {
            let mut deliveries = Deliveries::default();
            let mut out = String::new();
            let mut pending = VecDeque::new();
            let wait = Some(Duration::from_secs(3));
            let (fresh, fresh_heard) = Delivery::new(4, "prompt".into(), true, wait);
            let (next, next_heard) = Delivery::new(4, "next".into(), false, None);
            let start = Instant::now();
            deliveries.push("$1", fresh, &mut out, &mut pending);
            deliveries.push("$1", next, &mut out, &mut pending);
            deliveries.looked("$1", readiness, start + at, &mut out, &mut pending);
            let error = futures_block(fresh_heard).unwrap_err();
            assert!(error.is::<NotReady>(), "{readiness:?}: {error}");
            assert!(
                !out.contains("prompt"),
                "{readiness:?}: nothing pasted: {out}"
            );
            assert!(out.contains("next"), "{readiness:?}: the next goes: {out}");
            assert!(next_heard.started());
        }
    }

    /// A pane that went away while a delivery waited on it got nothing:
    /// refused, as is one still waiting when the engine goes.
    #[test]
    fn a_waiting_delivery_whose_pane_or_engine_goes_is_refused() {
        let wait = Some(Duration::from_secs(3));
        let mut deliveries = Deliveries::default();
        let mut out = String::new();
        let mut pending = VecDeque::new();
        let (gone, gone_heard) = Delivery::new(4, "x".into(), false, wait);
        deliveries.push("$1", gone, &mut out, &mut pending);
        let now = Instant::now();
        deliveries.looked("$1", Readiness::Gone, now, &mut out, &mut pending);
        let error = futures_block(gone_heard).unwrap_err();
        assert!(!error.is::<NotSubmitted>() && !error.is::<NotReady>());

        let (stopped, stopped_heard) = Delivery::new(4, "x".into(), false, wait);
        deliveries.push("$1", stopped, &mut out, &mut pending);
        drop(deliveries);
        let error = futures_block(stopped_heard).unwrap_err();
        assert!(!error.is::<NotSubmitted>(), "never pasted: {error}");
    }

    fn futures_block(d: Delivering) -> Result<()> {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(d)
    }
}
