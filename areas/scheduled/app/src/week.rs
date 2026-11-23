//! The week across the top of the chosen task: every task's runs over the
//! last day and the next seven, one lane each, now as a line; under it, in
//! words, where two tasks reach one session within a few minutes of each
//! other. Past runs are the node's (`recent_runs`); coming ones are read from
//! each task's schedule as the node reads it, from its `next_run`.
//!
//! The week is worked out when the tasks or the minute change, not on every
//! draw: an every-minute task alone is ten thousand runs.

use std::collections::BTreeMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use ahjo_app_kit::theme::{Theme, size};
use ahjo_app_kit::widgets::{kit, pane};
use ahjo_scheduled_contract::{SUCCESS, Task, schedule};
use gpui::{
    AnyElement, Context, Div, FontWeight, Hsla, SharedString, div, prelude::*, px, relative,
};
use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp};

use crate::form;
use crate::page::ScheduledPage;
use crate::tokens::*;

/// How close two prompts into one session are said.
pub const CLOSE: SignedDuration = SignedDuration::from_mins(15);
/// The days ahead the week shows, after the day behind.
const AHEAD: i64 = 7;
/// A task that runs more often than this many times in the week is drawn
/// as a steady band, not as marks.
const MOST_MARKS: usize = 200;
/// At most this many of a steady task's runs are read to find where it
/// comes close to another: a week of every minute.
const MOST_RUNS: usize = 7 * 24 * 60 + 60;

/// What a mark says happened or will happen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Ran,
    Failed,
    Coming,
    /// Would run, but the task is paused.
    Paused,
}

#[derive(Clone, Debug)]
pub struct Mark {
    pub at: Timestamp,
    pub kind: Kind,
    pub tip: String,
}

/// One task's lane.
#[derive(Clone, Debug)]
pub struct Lane {
    pub node: String,
    pub id: String,
    pub name: String,
    /// Why it will not run: paused, off or held.
    pub status: Option<&'static str>,
    pub marks: Vec<Mark>,
    /// Too many runs to draw one by one: from when the band starts, and
    /// what it says.
    pub steady: Option<(Timestamp, String)>,
}

/// Two tasks that reach one session close together, as often as they do
/// in the week.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Close {
    pub session: String,
    /// (node, id, name), the one that runs first.
    pub first: (String, String, String),
    pub second: (String, String, String),
    /// Minutes apart, where it is the same every time.
    pub apart: Option<i64>,
    /// Their times of day, where they are the same every time.
    pub times: Option<(String, String)>,
    pub count: usize,
    pub next: Timestamp,
}

pub struct Week {
    pub from: Timestamp,
    pub now: Timestamp,
    pub until: Timestamp,
    pub lanes: Vec<Lane>,
    pub close: Vec<Close>,
}

/// A run read for nearness: its task and sessions.
struct Coming<'a> {
    at: Timestamp,
    task: usize,
    sessions: &'a [String],
}

/// What the week depends on, so it is worked out again only when that
/// changes.
pub fn key(tasks: &[(&str, &Task)], now: Timestamp) -> u64 {
    let mut h = DefaultHasher::new();
    now.as_second().div_euclid(60).hash(&mut h);
    for (node, t) in tasks {
        (node, &t.id, &t.name, t.paused, t.enabled, &t.held).hash(&mut h);
        (t.next_run.map(|x| x.as_second()), t.recent_runs.len()).hash(&mut h);
        t.recent_runs
            .first()
            .map(|r| (r.started_at.as_second(), &r.status))
            .hash(&mut h);
        let s = &t.schedule;
        (
            &s.kind,
            &s.expression,
            &s.timezone,
            s.every_minutes,
            &s.duration,
        )
            .hash(&mut h);
        for x in &t.targets {
            x.label().hash(&mut h);
        }
    }
    h.finish()
}

/// The week for these tasks, in the list's order.
pub fn week(tasks: &[(&str, &Task)], now: Timestamp) -> Week {
    let from = now - SignedDuration::from_hours(24);
    let until = now + SignedDuration::from_hours(24 * AHEAD);
    let zone = TimeZone::system();
    let sessions: Vec<Vec<String>> = tasks
        .iter()
        .map(|(_, t)| t.targets.iter().map(|x| x.label()).collect())
        .collect();
    let mut lanes = vec![];
    let mut coming: Vec<Coming> = vec![];
    for (i, (node, task)) in tasks.iter().enumerate() {
        let to = sessions[i].join(", ");
        let mut marks: Vec<Mark> = task
            .recent_runs
            .iter()
            .filter(|r| r.started_at >= from && r.started_at <= now)
            .map(|r| {
                let ok = r.status == SUCCESS;
                Mark {
                    at: r.started_at,
                    kind: if ok { Kind::Ran } else { Kind::Failed },
                    tip: format!(
                        "{} · {} · {}",
                        day_time(r.started_at, &zone),
                        task.name,
                        form::result(&r.status, &r.message)
                    ),
                }
            })
            .collect();
        let status = if !task.held.is_empty() {
            Some("held")
        } else if !task.enabled {
            Some("off")
        } else if task.paused {
            Some("paused")
        } else {
            None
        };
        let mut steady = None;
        // A held or switched-off task will not run; a paused one shows
        // where it would.
        if task.enabled && task.held.is_empty() {
            let first = if task.paused { None } else { task.next_run };
            let (runs, short) =
                schedule::runs_between(&task.schedule, first, now, until, MOST_RUNS);
            if runs.len() > MOST_MARKS || short {
                let how = form::describe(&task.schedule);
                steady = runs.first().map(|at| {
                    let words = if task.paused {
                        format!("{how} · {} · paused", task.name)
                    } else {
                        format!("{how} · {} → {to}", task.name)
                    };
                    (*at, words)
                });
            } else {
                for at in &runs {
                    marks.push(Mark {
                        at: *at,
                        kind: if task.paused {
                            Kind::Paused
                        } else {
                            Kind::Coming
                        },
                        tip: if task.paused {
                            format!(
                                "{} · {} · paused, will not run",
                                day_time(*at, &zone),
                                task.name
                            )
                        } else {
                            format!("{} · {} → {to}", day_time(*at, &zone), task.name)
                        },
                    });
                }
            }
            if !task.paused {
                coming.extend(runs.into_iter().map(|at| Coming {
                    at,
                    task: i,
                    sessions: &sessions[i],
                }));
            }
        }
        lanes.push(Lane {
            node: node.to_string(),
            id: task.id.clone(),
            name: task.name.clone(),
            status,
            marks,
            steady,
        });
    }
    coming.sort_by_key(|c| c.at);
    let close = close_together(tasks, &coming, &zone);
    Week {
        from,
        now,
        until,
        lanes,
        close,
    }
}

/// Two different tasks reaching one session within [`CLOSE`], one line
/// for each pair of tasks and session, soonest first.
fn close_together(tasks: &[(&str, &Task)], coming: &[Coming], zone: &TimeZone) -> Vec<Close> {
    struct Seen {
        first: usize,
        second: usize,
        apart: Option<i64>,
        times: Option<(String, String)>,
        /// Each task's runs that came close, counted once each: the
        /// rarer of the two says how often the pair does.
        runs: [std::collections::BTreeSet<Timestamp>; 2],
        next: Timestamp,
    }
    let mut seen: BTreeMap<(String, usize, usize), Seen> = BTreeMap::new();
    for (i, x) in coming.iter().enumerate() {
        for y in &coming[i + 1..] {
            if y.at.duration_since(x.at) > CLOSE {
                break;
            }
            if x.task == y.task {
                continue;
            }
            for s in x.sessions.iter().filter(|s| y.sessions.contains(s)) {
                let pair = (x.task.min(y.task), x.task.max(y.task));
                let apart = y.at.duration_since(x.at).as_mins();
                let times = (hhmm(x.at, zone), hhmm(y.at, zone));
                let e = seen
                    .entry((s.clone(), pair.0, pair.1))
                    .or_insert_with(|| Seen {
                        first: x.task,
                        second: y.task,
                        apart: Some(apart),
                        times: Some(times.clone()),
                        runs: Default::default(),
                        next: x.at,
                    });
                if e.apart != Some(apart) {
                    e.apart = None;
                }
                if e.times.as_ref() != Some(&times) {
                    e.times = None;
                }
                for c in [x, y] {
                    e.runs[usize::from(c.task != pair.0)].insert(c.at);
                }
            }
        }
    }
    let who = |i: usize| {
        let (node, t) = tasks[i];
        (node.to_string(), t.id.clone(), t.name.clone())
    };
    let mut out: Vec<Close> = seen
        .into_iter()
        .map(|((session, _, _), s)| Close {
            session,
            first: who(s.first),
            second: who(s.second),
            apart: s.apart,
            times: s.times,
            count: s.runs[0].len().min(s.runs[1].len()),
            next: s.next,
        })
        .collect();
    out.sort_by_key(|c| c.next);
    out
}

fn hhmm(at: Timestamp, zone: &TimeZone) -> String {
    at.to_zoned(zone.clone()).strftime("%H:%M").to_string()
}

fn day_time(at: Timestamp, zone: &TimeZone) -> String {
    at.to_zoned(zone.clone())
        .strftime("%a %-d %b %H:%M")
        .to_string()
}

impl Close {
    /// The sentence: who gets what, how close, how often, when next.
    pub fn words(&self) -> (String, String) {
        let (a, b) = (&self.first.2, &self.second.2);
        let how = match (self.apart, &self.times) {
            (Some(0), Some((t, _))) => format!("at the same minute ({t})"),
            (Some(0), None) => "at the same minute".to_string(),
            (Some(m), Some((t, u))) => format!("{m} min apart ({t} and {u})"),
            (Some(m), None) => format!("{m} min apart"),
            (None, _) => format!("within {} min of each other", CLOSE.as_mins()),
        };
        let often = match self.count {
            1 => "once this week".to_string(),
            n => format!("{n} times this week"),
        };
        let next = self
            .next
            .to_zoned(TimeZone::system())
            .strftime("%a %H:%M")
            .to_string();
        (
            format!("gets {a} and {b} {how}"),
            format!("{often}, next {next}"),
        )
    }
}

impl Week {
    fn frac(&self, at: Timestamp) -> f32 {
        let whole = self.until.duration_since(self.from).as_secs_f64();
        (at.duration_since(self.from).as_secs_f64() / whole).clamp(0., 1.) as f32
    }

    /// The midnights inside the week.
    fn midnights(&self) -> Vec<jiff::Zoned> {
        let zone = TimeZone::system();
        let mut day = self.from.to_zoned(zone.clone()).date();
        let mut out = vec![];
        while let Ok(next) = day.tomorrow() {
            day = next;
            let Ok(z) = day.to_zoned(zone.clone()) else {
                break;
            };
            if z.timestamp() >= self.until {
                break;
            }
            out.push(z);
        }
        out
    }
}

impl ScheduledPage {
    /// The week for the tasks as listed now, worked out again only when
    /// they or the minute changed.
    fn the_week(&self) -> std::rc::Rc<Week> {
        let now = Timestamp::now();
        let tasks = self.ordered();
        let key = key(&tasks, now);
        let mut kept = self.week.borrow_mut();
        match &*kept {
            Some((k, w)) if *k == key => w.clone(),
            _ => {
                let w = std::rc::Rc::new(week(&tasks, now));
                *kept = Some((key, w.clone()));
                w
            }
        }
    }

    /// The week's lanes and, under them, what comes close together.
    pub(crate) fn week_view(
        &self,
        t: &Theme,
        chosen: (&str, &str),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let week = self.the_week();
        let mut lanes = div()
            .debug_selector(|| "scheduled-week".into())
            .flex()
            .flex_col()
            .child(lane(div(), self.axis(t, &week), false, t));
        let mut paused = false;
        for (i, l) in week.lanes.iter().enumerate() {
            let on = chosen == (l.node.as_str(), l.id.as_str());
            paused |= l.marks.iter().any(|m| m.kind == Kind::Paused);
            let mut track = self.ground(t, &week);
            if let Some((start, words)) = &l.steady {
                let colour = if on { t.accent } else { t.chart };
                let a = week.frac(*start);
                track = track.child(
                    div()
                        .id(SharedString::from(format!("week-steady-{i}")))
                        .absolute()
                        .left(relative(a))
                        .w(relative(1. - a))
                        .top(px((WEEK_LANE - WEEK_STEADY) / 2.))
                        .h(px(WEEK_STEADY))
                        .rounded(px(WEEK_STEADY / 2.))
                        .cursor_pointer()
                        .map(|d| {
                            if l.status.is_some() {
                                d.border_1().border_color(t.dim)
                            } else {
                                d.bg(colour)
                            }
                        })
                        .tooltip(kit::tip(words.clone()))
                        .on_click(self.chooser(&l.node, &l.id, cx)),
                );
            }
            for (k, m) in l.marks.iter().enumerate() {
                track = track.child(self.mark(t, &week, l, m, on, format!("week-{i}-{k}"), cx));
            }
            let selector = format!("week-lane-{}", l.name);
            let label = div()
                .id(SharedString::from(format!("week-lane-{i}")))
                .debug_selector(move || selector)
                .cursor_pointer()
                .flex()
                .items_center()
                .gap(px(size::HEAD_GAP))
                .overflow_hidden()
                .whitespace_nowrap()
                .text_size(px(size::SMALL))
                .text_color(if on { t.text } else { t.text2 })
                .when(on, |d| d.font_weight(FontWeight::SEMIBOLD))
                .hover(|d| d.text_color(t.text))
                .child(
                    div()
                        .min_w_0()
                        .overflow_hidden()
                        .text_ellipsis()
                        .child(l.name.clone()),
                )
                .children(l.status.map(|s| {
                    div()
                        .flex_none()
                        .font_weight(FontWeight::NORMAL)
                        .text_color(if s == "held" { t.error } else { t.dim })
                        .child(s)
                }))
                .on_click(self.chooser(&l.node, &l.id, cx));
            lanes = lanes.child(lane(label, track, on, t));
        }
        let mut out = pane::section(t, "Every task, the last day and the next seven")
            .child(lanes)
            .child(legend(t, paused));
        if !week.close.is_empty() {
            out = out.child(
                div()
                    .pt(px(size::SECTION_GAP))
                    .child(self.close_view(t, &week, chosen, cx)),
            );
        }
        out.into_any_element()
    }

    /// Where two tasks reach one session close together, in words; a line
    /// shows the other of the two.
    fn close_view(
        &self,
        t: &Theme,
        week: &Week,
        chosen: (&str, &str),
        cx: &mut Context<Self>,
    ) -> Div {
        let mut words = div().flex().flex_col();
        for (i, c) in week.close.iter().enumerate() {
            let (what, when) = c.words();
            // The line goes to the task not shown now.
            let to = if chosen == (c.first.0.as_str(), c.first.1.as_str()) {
                &c.second
            } else {
                &c.first
            };
            let selector = format!("scheduled-close-{}-{what}", c.session);
            words = words.child(
                div()
                    .id(SharedString::from(format!("week-close-{i}")))
                    .debug_selector(move || selector)
                    .flex()
                    .items_center()
                    .gap(px(size::HEAD_GAP))
                    .min_h(px(size::ROW))
                    .px(px(size::HEAD_GAP))
                    .mx(px(-size::HEAD_GAP))
                    .rounded(px(size::RADIUS))
                    .cursor_pointer()
                    .hover(|d| d.bg(t.raised))
                    .tooltip(kit::tip(format!("Show {}", to.2)))
                    .child(kit::state_dot(t.needs))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_wrap()
                            .items_baseline()
                            .gap_x(px(size::HEAD_GAP))
                            .child(div().text_color(t.text).child(c.session.clone()))
                            .child(div().text_color(t.text2).child(what))
                            .child(
                                div()
                                    .text_size(px(size::SMALL))
                                    .text_color(t.dim)
                                    .child(when),
                            ),
                    )
                    .on_click(self.chooser(&to.0, &to.1, cx)),
            );
        }
        pane::section(
            t,
            format!("Into one session within {} minutes", CLOSE.as_mins()),
        )
        .child(words)
    }

    #[allow(clippy::too_many_arguments)]
    fn mark(
        &self,
        t: &Theme,
        week: &Week,
        l: &Lane,
        m: &Mark,
        on: bool,
        id: String,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colour = match m.kind {
            Kind::Ran => t.done,
            Kind::Failed => t.error,
            Kind::Coming if on => t.accent,
            Kind::Coming | Kind::Paused => t.chart,
        };
        div()
            .id(SharedString::from(id))
            .absolute()
            .left(relative(week.frac(m.at)))
            .ml(px(-WEEK_MARK_W / 2.))
            .top(px((WEEK_LANE - WEEK_MARK_H) / 2.))
            .w(px(WEEK_MARK_W))
            .h(px(WEEK_MARK_H))
            .rounded(px(WEEK_MARK_RADIUS))
            .cursor_pointer()
            .map(|d| {
                if m.kind == Kind::Paused {
                    d.border_1().border_color(t.dim)
                } else {
                    d.bg(colour)
                }
            })
            .tooltip(kit::tip(m.tip.clone()))
            .on_click(self.chooser(&l.node, &l.id, cx))
            .into_any_element()
    }

    /// A lane's ground: the day behind shaded, the days ruled, now a line.
    fn ground(&self, t: &Theme, week: &Week) -> Div {
        let now = week.frac(week.now);
        let mut d = div().relative().flex_1().h(px(WEEK_LANE)).child(
            div()
                .absolute()
                .left_0()
                .top_0()
                .h_full()
                .w(relative(now))
                .bg(t.raised),
        );
        for m in week.midnights() {
            d = d.child(
                div()
                    .absolute()
                    .left(relative(week.frac(m.timestamp())))
                    .top_0()
                    .h_full()
                    .w(px(1.))
                    .bg(t.divider),
            );
        }
        d.child(
            div()
                .absolute()
                .left(relative(now))
                .top_0()
                .h_full()
                .w(px(WEEK_NOW_W))
                .bg(t.text2),
        )
    }

    /// The days' names over the lanes, and now.
    fn axis(&self, t: &Theme, week: &Week) -> Div {
        let zone = TimeZone::system();
        let now = week.now.to_zoned(zone.clone());
        let today = now.date();
        let at = week.frac(week.now);
        let midnights = week.midnights();
        // Now's name stands after its line, or before it where the next
        // day's name would be under it.
        let crowded = midnights.iter().any(|m| {
            let f = week.frac(m.timestamp());
            f > at && f - at < WEEK_NOW_ROOM
        });
        let now_name = div()
            .absolute()
            .top(px(WEEK_AXIS_TOP))
            .px(px(WEEK_AXIS_PAD))
            .text_color(t.text)
            .font_weight(FontWeight::SEMIBOLD)
            .child(format!("now {}", now.strftime("%H:%M")));
        let mut d = div()
            .relative()
            .flex_1()
            .h(px(WEEK_LANE))
            .text_size(px(size::SMALL))
            .text_color(t.text2)
            .child(if crowded {
                now_name.right(relative(1. - at))
            } else {
                now_name.left(relative(at))
            });
        for m in midnights {
            let f = week.frac(m.timestamp());
            // A day's name just before now's would run into it, whichever
            // side of its line now's name stands: it is left out.
            let covered = f <= at && at - f < WEEK_NOW_ROOM;
            if covered {
                continue;
            }
            let name = match (m.date() - today).get_days() {
                0 => "Today".to_string(),
                1 => "Tomorrow".to_string(),
                _ => m.strftime("%a %-d").to_string(),
            };
            d = d.child(
                div()
                    .absolute()
                    .left(relative(f))
                    .top(px(WEEK_AXIS_TOP))
                    .pl(px(WEEK_AXIS_PAD))
                    .child(name),
            );
        }
        d
    }
}

fn lane(label: impl IntoElement, track: Div, on: bool, t: &Theme) -> Div {
    div()
        .flex()
        .items_center()
        .h(px(WEEK_LANE))
        .rounded(px(size::RADIUS))
        .when(on, |d| d.bg(t.accent_soft()))
        .child(
            div()
                .flex_none()
                .w(px(WEEK_LABEL_W))
                .pl(px(WEEK_AXIS_PAD))
                .pr(px(size::HEAD_GAP))
                .child(label),
        )
        .child(track)
}

fn legend(t: &Theme, paused: bool) -> Div {
    let item = |c: Hsla, hollow: bool, w: &'static str| {
        div()
            .flex()
            .items_center()
            .gap(px(WEEK_AXIS_PAD))
            .child(
                div()
                    .w(px(WEEK_MARK_W))
                    .h(px(WEEK_MARK_H))
                    .rounded(px(WEEK_MARK_RADIUS))
                    .map(|d| {
                        if hollow {
                            d.border_1().border_color(c)
                        } else {
                            d.bg(c)
                        }
                    }),
            )
            .child(w)
    };
    div()
        .flex()
        .gap(px(size::PAD))
        .pt(px(size::HEAD_GAP))
        .pl(px(WEEK_LABEL_W))
        .text_size(px(size::SMALL))
        .text_color(t.dim)
        .child(item(t.done, false, "ran"))
        .child(item(t.error, false, "failed"))
        .child(item(t.chart, false, "will run"))
        .when(paused, |d| d.child(item(t.dim, true, "paused")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ahjo_scheduled_contract::{Schedule, Target};

    fn task(id: &str, cron: &str, sessions: &[&str]) -> Task {
        let mut t: Task = serde_json::from_value(serde_json::json!({
            "id": id,
            "name": id,
            "prompt": "p",
            "targets": [],
            "schedule": {"type": "cron", "expression": cron, "timezone": "UTC"},
            "enabled": true,
            "paused": false,
            "created_at": "2026-06-01T00:00:00Z",
            "updated_at": "2026-06-01T00:00:00Z",
        }))
        .unwrap();
        t.targets = sessions
            .iter()
            .map(|s| Target {
                session_name: s.to_string(),
                unix_user: String::new(),
            })
            .collect();
        t
    }

    #[test]
    fn two_tasks_into_one_session_close_together_are_one_line_with_their_count() {
        let now: Timestamp = "2026-06-29T00:00:00Z".parse().unwrap(); // a Monday
        let triage = task("triage", "45 8 * * 1-5", &["lead"]);
        let standup = task("standup", "0 9 * * 1-5", &["lead", "ops"]);
        let apart = task("apart", "0 12 * * *", &["lead"]);
        let elsewhere = task("elsewhere", "0 9 * * *", &["other"]);
        let tasks: Vec<(&str, &Task)> = [&triage, &standup, &apart, &elsewhere]
            .into_iter()
            .map(|t| ("n", t))
            .collect();
        let w = week(&tasks, now);
        assert_eq!(w.close.len(), 1, "{:?}", w.close);
        let c = &w.close[0];
        assert_eq!(c.session, "lead");
        assert_eq!(
            (c.first.2.as_str(), c.second.2.as_str()),
            ("triage", "standup")
        );
        assert_eq!(c.apart, Some(15));
        assert_eq!(c.count, 5);
        assert_eq!(c.next, "2026-06-29T08:45:00Z".parse::<Timestamp>().unwrap());
        // Each weekday's run is a mark; a paused task's would-be runs are
        // drawn apart and come close to nothing.
        assert_eq!(w.lanes[0].marks.len(), 5);
        let mut quiet = standup.clone();
        quiet.paused = true;
        let tasks: Vec<(&str, &Task)> = vec![("n", &triage), ("n", &quiet)];
        let w = week(&tasks, now);
        assert!(w.close.is_empty());
        assert!(w.lanes[1].marks.iter().all(|m| m.kind == Kind::Paused));
        assert_eq!(w.lanes[1].status, Some("paused"));
    }

    #[test]
    fn a_task_too_frequent_to_mark_is_a_steady_band_and_still_comes_close() {
        let now: Timestamp = "2026-06-29T00:00:00Z".parse().unwrap();
        let mut watch = task("watch", "", &["ops"]);
        watch.schedule = Schedule {
            kind: "interval".into(),
            every_minutes: 5,
            ..watch.schedule.clone()
        };
        let daily = task("daily", "2 10 * * *", &["ops"]);
        let tasks: Vec<(&str, &Task)> = vec![("n", &watch), ("n", &daily)];
        let w = week(&tasks, now);
        assert!(w.lanes[0].steady.is_some());
        assert!(w.lanes[0].marks.is_empty());
        let c = &w.close[0];
        assert_eq!(c.count, 7, "every run of the daily task");
        assert_eq!(c.apart, None, "how far apart differs");
    }
}
