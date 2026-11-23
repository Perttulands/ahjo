//! A step's panel on a run's board: who sits in each of its seats and what
//! its session shows now, a way to open the session or write to it, then
//! what each attempt came to, newest first.
use super::*;
use crate::store::run_state::{self, CardState};
use crate::store::seats::{self, Seat, State};
use crate::store::{attempts, words};
use ahjo_app_kit::host::Status;
use ahjo_app_kit::{
    Ask,
    theme::{FAMILY, Theme},
};
use ahjo_missions_contract::{Mission, StepAttempt};
use ahjo_terminals_contract::{
    SessionList,
    commands::{SessionRead, SessionSend},
};
use gpui::{ScrollHandle, SharedString};
use std::collections::{BTreeMap, HashSet};
use std::time::Duration;

/// How high a seat's screen stands in the panel; its newest lines show.
const SCREEN_H: f32 = 260.;
/// How soon a seat's screen is read again while its session is writing.
const READ_AGAIN: Duration = Duration::from_secs(1);

/// A live seat's session, as `(node, session id)`.
pub(super) type SeatKey = (String, String);

/// What a live seat's session showed when last read.
#[derive(Default)]
pub(super) struct SeatScreen {
    text: Option<Result<String, String>>,
    asking: bool,
    /// Something changed while a read was under way: read once more.
    again: bool,
    /// The session's activity as the Sessions page last heard it.
    marker: Option<(Option<bool>, Option<u64>)>,
    scroll: ScrollHandle,
}

/// The step's live seats: what the panel follows.
#[derive(Default)]
pub(super) struct SeatFollow {
    pub(super) screens: BTreeMap<SeatKey, SeatScreen>,
    pub(super) messages: BTreeMap<SeatKey, Entity<TextField>>,
}

impl MissionsPage {
    /// Hears the Sessions page's list, so an open seat's screen is read
    /// again when its session starts or stops writing.
    pub(super) fn watch_sessions(&mut self, cx: &mut Context<Self>) {
        let Some(sessions) = self.host.model::<SessionList>(cx) else {
            return;
        };
        cx.observe(&sessions, |page, _, cx| page.follow_seats(false, cx))
            .detach();
        self.sessions = Some(sessions);
    }

    /// The mission the run shown carries out.
    fn run_mission(&self) -> Option<&Mission> {
        self.run.frozen.as_ref().or(self.state.mission.as_ref())
    }

    /// The step whose panel is open on the run shown.
    fn inspected_step(&self) -> Option<&str> {
        let id = self.run.inspect.as_deref()?;
        self.selected_run()?;
        self.run_mission()?
            .steps
            .iter()
            .any(|step| step.id == id)
            .then_some(id)
    }

    /// Follows the live seats of the open step: each screen is read now
    /// (`read`), or when its session's activity changed, and while it
    /// writes. Seats no longer live, or another card open, are let go.
    pub(super) fn follow_seats(&mut self, read: bool, cx: &mut Context<Self>) {
        let live: Vec<Seat> = match (self.inspected_step(), self.selected_run()) {
            (Some(step), Some(run)) => seats::seats(run)
                .into_iter()
                .filter(|seat| seat.step == step && seat.state == State::Live)
                .collect(),
            _ => vec![],
        };
        let keys: Vec<SeatKey> = live
            .iter()
            .map(|seat| (seat.node.clone(), seat.session.clone()))
            .collect();
        self.run.seats.screens.retain(|key, _| keys.contains(key));
        self.run.seats.messages.retain(|key, _| keys.contains(key));
        for (seat, key) in live.iter().zip(keys) {
            if !self.run.seats.messages.contains_key(&key) {
                let field = cx.new(|cx| TextField::new(cx, "Write to its session"));
                let to = key.clone();
                cx.subscribe(&field, move |page, _, event: &FieldEvent, cx| {
                    if matches!(event, FieldEvent::Submit) {
                        page.send_to_seat(&to, cx);
                    }
                })
                .detach();
                self.run.seats.messages.insert(key.clone(), field);
            }
            let marker = self.session_marker(&key, &seat.name, cx);
            let screen = self.run.seats.screens.entry(key.clone()).or_default();
            let changed = screen.marker != marker;
            screen.marker = marker;
            if read || changed || screen.text.is_none() {
                self.read_screen(key, cx);
            }
        }
    }

    /// A session's activity as the Sessions page heard it.
    fn session_marker(
        &self,
        (node, id): &SeatKey,
        name: &str,
        cx: &gpui::App,
    ) -> Option<(Option<bool>, Option<u64>)> {
        let list = self.sessions.as_ref()?.read(cx);
        list.sessions
            .iter()
            .find(|s| &s.node == node && (&s.id == id || s.name == name))
            .map(|s| (s.active, s.last))
    }

    /// Reads what a seat's session shows now, from the node it runs on;
    /// again a moment later while the session is writing and its panel
    /// stays open.
    fn read_screen(&mut self, key: SeatKey, cx: &mut Context<Self>) {
        // Nothing is read for a panel no longer open.
        if self.inspected_step().is_none() {
            self.run.seats.screens.clear();
            return;
        }
        let Some(screen) = self.run.seats.screens.get_mut(&key) else {
            return;
        };
        if screen.asking {
            screen.again = true;
            return;
        }
        screen.asking = true;
        let asked = self.host.call(
            NodeId(key.0.clone()),
            SessionRead {
                session: key.1.clone(),
                pane: None,
                lines: None,
                all: None,
            },
            cx,
        );
        cx.spawn(async move |this, cx| {
            let result = asked.await;
            let writing = this
                .update(cx, |page, cx| {
                    let Some(screen) = page.run.seats.screens.get_mut(&key) else {
                        return false;
                    };
                    screen.asking = false;
                    screen.text = Some(result.map(|read| read.text).map_err(|error| error.message));
                    screen.scroll.scroll_to_bottom();
                    let again = std::mem::take(&mut screen.again);
                    let writing = matches!(screen.marker, Some((Some(true), _)));
                    cx.notify();
                    if again {
                        page.read_screen(key.clone(), cx);
                        return false;
                    }
                    writing
                })
                .unwrap_or(false);
            if writing {
                cx.background_executor().timer(READ_AGAIN).await;
                let _ = this.update(cx, |page, cx| page.read_screen(key, cx));
            }
        })
        .detach();
    }

    /// Writes the seat's message into its session, as Send does.
    fn send_to_seat(&mut self, key: &SeatKey, cx: &mut Context<Self>) {
        let Some(field) = self.run.seats.messages.get(key).cloned() else {
            return;
        };
        let text = field.read(cx).text.trim().to_owned();
        if text.is_empty() {
            return;
        }
        let asked = self.host.call(
            NodeId(key.0.clone()),
            SessionSend {
                session: key.1.clone(),
                text,
                pane: None,
                enter: None,
            },
            cx,
        );
        let key = key.clone();
        cx.spawn(async move |this, cx| {
            let result = asked.await;
            let _ = this.update(cx, |page, cx| match result {
                Ok(sent) if sent.sent => {
                    field.update(cx, |field, cx| field.set("", false, cx));
                    page.read_screen(key, cx);
                }
                Ok(_) => page.host.report(
                    Status::failure("Not sent: the session did not take it."),
                    cx,
                ),
                Err(error) => page
                    .host
                    .report(Status::failure(format!("Not sent: {}", error.message)), cx),
            });
        })
        .detach();
    }

    /// The step's part of its panel in the run shown.
    pub(super) fn step_run_part(&self, t: &Theme, id: &str, cx: &mut Context<Self>) -> Option<Div> {
        let mission = self.run_mission()?;
        let step = mission.steps.iter().find(|step| step.id == id)?;
        let run = self.selected_run()?;
        let state = run_state::project(&run.events).states.get(id).copied();
        let in_run = seats::seats(run);
        let mut part = div().flex().flex_col().gap(px(PAD));
        for seat in &step.seats {
            let sat = in_run
                .iter()
                .rev()
                .find(|s| s.step == id && s.seat == seat.id);
            part = part.child(self.seat_card(t, mission, seat, sat, state, cx));
        }
        Some(part.child(self.attempts_log(t, id, cx)))
    }

    /// One seat: its agent with the harness's mark, what it runs on, and
    /// while its session lives, what it shows, Open session and a line to
    /// write to it.
    fn seat_card(
        &self,
        t: &Theme,
        mission: &Mission,
        seat: &ahjo_missions_contract::Seat,
        sat: Option<&Seat>,
        step: Option<CardState>,
        cx: &mut Context<Self>,
    ) -> Div {
        let agent = self
            .agents
            .as_ref()
            .and_then(|catalog| catalog.agents.iter().find(|a| a.id == seat.agent));
        let settings = seat.settings(agent);
        let name = if seat.agent.trim().is_empty() {
            words::seat_label(seat).to_owned()
        } else {
            words::agent_name(&seat.agent)
        };
        let (word, colour) = match (sat.map(|s| s.state), step) {
            (None, _) => ("not started", t.dim),
            (Some(State::Live), Some(CardState::Running)) => ("working", t.running),
            (Some(State::Live), _) => ("session open", t.text2),
            (Some(State::Ended), _) => ("session closed", t.dim),
            (Some(State::Unavailable), _) => ("session gone", t.dim),
        };
        let id = seat.id.clone();
        let mut card = div()
            .debug_selector({
                let id = id.clone();
                move || format!("run-seat-{id}")
            })
            .flex()
            .flex_col()
            .gap(px(TAB_GAP))
            .p(px(PAD))
            .rounded(px(size::RADIUS))
            .border_1()
            .border_color(t.divider)
            .bg(t.raised)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(TAB_GAP))
                    .children(canvas::harness_mark(
                        settings.harness.as_str(),
                        t.text,
                        t.text2,
                        1.,
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(name),
                    )
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(px(TAB_GAP))
                            .text_size(px(size::SMALL))
                            .text_color(colour)
                            .when(colour == t.running, |d| {
                                d.child(div().size(px(TAB_GAP)).rounded_full().bg(colour))
                            })
                            .child(word),
                    ),
            )
            .child(
                div()
                    .text_size(px(size::SMALL))
                    .text_color(t.text2)
                    .child(words::seat_line(seat, agent)),
            );
        if mission.steps.iter().any(|s| s.seats.len() > 1) && !seat.label.trim().is_empty() {
            card = card.child(
                div()
                    .text_size(px(size::SMALL))
                    .text_color(t.dim)
                    .child(seat.label.trim().to_owned()),
            );
        }
        let Some(sat) = sat.filter(|s| s.state == State::Live) else {
            return card;
        };
        let key = (sat.node.clone(), sat.session.clone());
        if let Some(screen) = self.run.seats.screens.get(&key) {
            card = card.child(screen_view(t, &id, screen));
        }
        let host = self.host.clone();
        let session = sat.clone();
        card = card.child(
            div().flex().child(
                kit::button(
                    t,
                    SharedString::from(format!("run-seat-open-{id}")),
                    "Open session",
                    Kind::Primary,
                )
                .debug_selector({
                    let id = id.clone();
                    move || format!("run-seat-open-{id}")
                })
                .tooltip(kit::tip(format!("Show {} in Sessions", sat.name)))
                .on_click(move |_, _, cx| {
                    if let Err(error) = host.ask(
                        Ask::Reveal {
                            what: "session".into(),
                            id: session.session.clone(),
                            node: Some(NodeId(session.node.clone())),
                            part: None,
                            beside: false,
                        },
                        cx,
                    ) {
                        host.report(Status::failure(format!("Not shown: {}", error.message)), cx);
                    }
                }),
            ),
        );
        let Some(field) = self.run.seats.messages.get(&key) else {
            return card;
        };
        let to = key.clone();
        card.child(
            div()
                .flex()
                .items_center()
                .gap(px(TAB_GAP))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .debug_selector({
                            let id = id.clone();
                            move || format!("run-seat-message-{id}")
                        })
                        .child(field.clone()),
                )
                .child(
                    kit::button(
                        t,
                        SharedString::from(format!("run-seat-send-{id}")),
                        "Send",
                        Kind::Plain,
                    )
                    .debug_selector(move || format!("run-seat-send-{id}"))
                    .on_click(cx.listener(move |page, _, _, cx| page.send_to_seat(&to, cx))),
                ),
        )
    }

    /// What each attempt came to, newest first, and Compare when there
    /// are two to set side by side.
    fn attempts_log(&self, t: &Theme, id: &str, cx: &mut Context<Self>) -> Div {
        let mut log = div().flex().flex_col().gap(px(TAB_GAP));
        let Some(step) = self
            .run
            .attempts
            .as_ref()
            .and_then(|all| all.steps.iter().find(|step| step.step_id == id))
        else {
            return log;
        };
        let mut head = div()
            .flex()
            .items_center()
            .justify_between()
            .child(kit::label(t, "Attempts"));
        if step.attempts.len() > 1 {
            let step_id = id.to_owned();
            head = head.child(
                pane::act(t, "run-step-compare", "Compare")
                    .debug_selector(|| "run-step-compare".into())
                    .tooltip(kit::tip("Two attempts side by side"))
                    .on_click(cx.listener(move |page, _, window, cx| {
                        page.run.pending_attempt = Some((step_id.clone(), true));
                        page.run.attempts_from = Some(Spot::here(window, cx));
                        page.attempts(cx);
                    })),
            );
        }
        log = log.child(head);
        let waiting: HashSet<String> = self
            .selected_run()
            .map(|run| {
                run.waiting_gates
                    .iter()
                    .map(|gate| gate.gate_id.clone())
                    .collect()
            })
            .unwrap_or_default();
        let now = time::OffsetDateTime::now_utc();
        for attempt in step.attempts.iter().rev() {
            log = log.child(self.attempt_entry(t, id, attempt, &waiting, now, cx));
        }
        log
    }

    /// One attempt: how it ended and when, each gate's verdict and why,
    /// and what it made.
    fn attempt_entry(
        &self,
        t: &Theme,
        step: &str,
        attempt: &StepAttempt,
        waiting: &HashSet<String>,
        now: time::OffsetDateTime,
        cx: &mut Context<Self>,
    ) -> Div {
        let outcome = attempts::outcome(attempt, waiting);
        let colour = match outcome {
            attempts::Outcome::Running | attempts::Outcome::Judging => t.running,
            attempts::Outcome::Waiting => t.needs,
            attempts::Outcome::Fail => t.fail,
            attempts::Outcome::Pass | attempts::Outcome::Picked => t.pass,
            attempts::Outcome::Done => t.done,
            attempts::Outcome::Stopped => t.text2,
        };
        let number = attempt.attempt;
        let small = |words: String, colour| {
            div()
                .text_size(px(size::SMALL))
                .text_color(colour)
                .child(words)
        };
        let mut entry = div()
            .debug_selector(move || format!("run-attempt-{number}"))
            .flex()
            .flex_col()
            .gap(px(EDGE))
            .py(px(TAB_GAP))
            .border_t_1()
            .border_color(t.divider)
            .child(
                // The timing moves under the outcome when the panel is
                // too narrow for both.
                div()
                    .flex()
                    .flex_wrap()
                    .items_baseline()
                    .justify_between()
                    .gap_x(px(TAB_GAP))
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap(px(TAB_GAP))
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(format!("Attempt {number}")),
                            )
                            .child(
                                div()
                                    .text_color(colour)
                                    .child(attempts::outcome_word(outcome)),
                            ),
                    )
                    .child(small(attempts::timing(attempt, now), t.dim)),
            );
        if let Some(reason) = &attempt.stop_reason {
            entry = entry.child(small(reason.text.clone(), t.text2));
        }
        for verdict in &attempt.verdicts {
            entry = entry.child(small(
                attempts::verdict_headline(verdict, waiting),
                if verdict.verdict == "fail" {
                    t.fail
                } else {
                    t.text2
                },
            ));
            if !verdict.reason.text.trim().is_empty() {
                entry = entry.child(small(verdict.reason.text.clone(), t.text2).line_clamp(3));
            }
        }
        if let Some(pick) = &attempt.picked {
            let by = attempts::picker_words(&pick.decided_by, |id| {
                self.run_mission()
                    .map(|m| m.card_name(id).to_owned())
                    .unwrap_or_else(|| id.to_owned())
            });
            entry = entry.child(small(format!("Picked as the best by {by}"), t.pass));
        }
        if attempt.outputs.is_empty() && attempt.stop_reason.is_none() {
            entry = entry.child(small(
                match attempt.status.as_str() {
                    "running" => "No output yet.",
                    "stopped" => "Stopped before its output.",
                    _ => "No output recorded.",
                }
                .into(),
                t.dim,
            ));
        }
        let node = self.state.selected.as_ref().map(|(node, _)| node.clone());
        for (index, output) in attempt.outputs.iter().enumerate() {
            let label = if output.label.is_empty() {
                &output.port_id
            } else {
                &output.label
            };
            let mut line = div().flex().items_start().gap(px(TAB_GAP)).child(
                small(format!("{label}: {}", output.text.text), t.text)
                    .flex_1()
                    .min_w_0()
                    .line_clamp(4),
            );
            if let (false, Some(node)) = (output.path.is_empty(), node.clone()) {
                let path = output.path.clone();
                line = line.child(
                    pane::act(
                        t,
                        SharedString::from(format!("run-attempt-file-{step}-{number}-{index}")),
                        "Open",
                    )
                    .tooltip(kit::tip(path.clone()))
                    .on_click(cx.listener(move |page, _, _, cx| {
                        page.open_beside(&node, &path, cx);
                    })),
                );
            }
            entry = entry.child(line);
        }
        entry
    }
}

/// What a live seat's session shows, in the terminal's own colours, its
/// newest lines in view.
fn screen_view(t: &Theme, seat: &str, screen: &SeatScreen) -> pane::Scroll {
    let selector = format!("run-seat-screen-{seat}");
    let body = match &screen.text {
        None => div().text_color(t.dim).child("Reading its screen…"),
        Some(Err(error)) => div()
            .text_color(t.error)
            .child(format!("Its screen did not read: {error}")),
        Some(Ok(text)) => {
            let mut lines = div().flex().flex_col();
            for line in text.trim_end().lines() {
                lines = lines.child(if line.trim().is_empty() {
                    div().h(px(size::SMALL)).into_any_element()
                } else {
                    // A terminal line keeps its shape: what is wider than
                    // the panel is cut, not wrapped.
                    div()
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .child(line.trim_end().to_owned())
                        .into_any_element()
                });
            }
            lines
        }
    };
    pane::scroll(SharedString::from(selector.clone()))
        .debug_selector(move || selector.clone())
        .max_h(px(SCREEN_H))
        .track_scroll(&screen.scroll)
        .p(px(TAB_GAP))
        .rounded(px(size::RADIUS))
        .bg(t.term_bg)
        .text_color(t.term_fg)
        .font_family(FAMILY)
        .text_size(px(size::SMALL))
        .child(body)
}
