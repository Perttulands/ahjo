//! Drawing the page on the kit's skeleton: the tasks in the list, soonest
//! first, and beside them the chosen task (when it runs, where it sends,
//! how its last run went) or the form.

use ahjo_app_kit::Ask;
use ahjo_app_kit::hub::now_ms;
use ahjo_app_kit::menu::MenuRow;
use ahjo_app_kit::theme::{Theme, size, theme};
use ahjo_app_kit::widgets::kit::{self, Kind};
use ahjo_app_kit::widgets::pane::{self, Col};
use ahjo_core::NodeId;
use ahjo_scheduled_contract::{SUCCESS, Target, Task, tied_session};
use gpui::{
    AnyElement, App, ClickEvent, Context, Div, FontWeight, KeyDownEvent, SharedString, Window, div,
    prelude::*, px,
};
use jiff::Timestamp;

use crate::form::{self, MODES, Mode, ScheduleForm, TaskForm, WEEKDAYS};
use crate::page::ScheduledPage;
use crate::tokens::*;

/// Recent runs: when, what started it, how it went.
const RUNS: [Col; 3] = [
    Col::fixed("When", RUN_WHEN_W),
    Col::fixed("By", RUN_BY_W),
    Col::fill("Result"),
];

impl Render for ScheduledPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx).clone();
        self.keys_to_list(window, cx);
        let shown = self
            .shown_task()
            .map(|(n, task)| (n.to_string(), task.clone()));
        // With no task, the empty detail's New task is the one way in.
        let list_head = pane::list_head(&t, None).children(shown.is_some().then(|| {
            pane::act(&t, "scheduled-new", "New")
                .debug_selector(|| "scheduled-new".into())
                .tooltip(kit::tip("New task (N)"))
                .on_click(cx.listener(|page, _, window, cx| page.new_task(window, cx)))
        }));
        let chosen = shown
            .as_ref()
            .map(|(n, task)| (n.as_str(), task.id.as_str()));
        // Each row says when, how often and how the last run went: the
        // wide list, so the names have room.
        let (rows, chosen_at) = self.list(&t, chosen, cx);
        if std::mem::take(&mut self.reveal)
            && let Some(at) = chosen_at
        {
            self.scroll.scroll_to_item(at);
        }
        let rows = rows
            .id("scheduled-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.scroll);
        let list = pane::list_wide_frame(&t, list_head, rows);
        let detail = if self.state.form.is_some() {
            self.form(&t, cx)
        } else if let Some((node, task)) = &shown {
            self.detail(&t, node, task, cx)
        } else {
            // Nothing can be scheduled where no node answers: no form
            // that could not be saved is offered.
            let apart = self.host.nodes(cx).is_empty();
            let words = if self.reading() {
                "Reading scheduled tasks…"
            } else if apart {
                "Not connected to the node."
            } else {
                "Nothing scheduled. A task sends a prompt to your sessions on a timetable."
            };
            let action = (!self.reading() && !apart).then(|| {
                kit::button(&t, "scheduled-empty-new", "New task", Kind::Primary)
                    .on_click(cx.listener(|page, _, window, cx| page.new_task(window, cx)))
                    .into_any_element()
            });
            pane::detail(
                &t,
                "scheduled-detail",
                pane::head(&t),
                div().children(self.notices(&t)).child(if self.reading() {
                    pane::empty(&t, words, action).p_0()
                } else {
                    pane::vacant(&t, words, action)
                }),
            )
        };
        // With nothing scheduled anywhere, an empty list beside the
        // invitation says nothing: the invitation takes the page.
        let nothing = shown.is_none()
            && self.state.form.is_none()
            && self.ordered().is_empty()
            && self.state.nodes.values().all(|n| n.error.is_none());
        let page = if nothing {
            div().size_full().flex().bg(t.bg).child(detail)
        } else {
            pane::page(&t, list, detail)
        };
        div()
            .id("scheduled")
            .size_full()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|page, e: &KeyDownEvent, window, cx| page.key(e, window, cx)))
            .child(page.text_color(t.text))
    }
}

impl ScheduledPage {
    /// Some node's first read is still out and nothing is listed yet.
    fn reading(&self) -> bool {
        self.state
            .nodes
            .values()
            .any(|n| n.reading && n.list.is_none())
    }

    /// Every node's tasks in the list's order: the live ones soonest
    /// first, then the paused and the disabled by name.
    fn ordered(&self) -> Vec<(&str, &Task)> {
        let mut all: Vec<(&str, &Task)> = self
            .state
            .nodes
            .iter()
            .flat_map(|(n, s)| s.list.iter().flatten().map(move |task| (n.as_str(), task)))
            .collect();
        all.sort_by(|(_, a), (_, b)| {
            (!a.live(), a.live().then_some(a.next_run).flatten(), &a.name).cmp(&(
                !b.live(),
                b.live().then_some(b.next_run).flatten(),
                &b.name,
            ))
        });
        all
    }

    /// The task the detail shows: the one chosen, or else the first.
    pub(crate) fn shown_task(&self) -> Option<(&str, &Task)> {
        self.state
            .selected_task()
            .or_else(|| self.ordered().into_iter().next())
    }

    /// The tasks as the list shows them, top to bottom: grouped by device
    /// where more than one has tasks (and then whether they are), else in
    /// the one order.
    fn listed(&self, cx: &App) -> (Vec<(&str, &Task)>, bool) {
        let ordered = self.ordered();
        let several = ordered
            .iter()
            .any(|(n, _)| Some(*n) != ordered.first().map(|f| f.0));
        if !several {
            return (ordered, false);
        }
        let mut out = vec![];
        for node in self.host.nodes(cx) {
            out.extend(ordered.iter().filter(|(n, _)| *n == node.id.as_str()));
        }
        (out, true)
    }

    /// The listed tasks by (node, id), for the keys.
    pub(crate) fn order(&self, cx: &App) -> Vec<(String, String)> {
        let (rows, _) = self.listed(cx);
        rows.into_iter()
            .map(|(n, t)| (n.to_string(), t.id.clone()))
            .collect()
    }

    /// The list, each row its child so the keys can scroll to one, and
    /// where the chosen row stands among them.
    fn list(
        &self,
        t: &Theme,
        chosen: Option<(&str, &str)>,
        cx: &mut Context<Self>,
    ) -> (Div, Option<usize>) {
        let now = Timestamp::now();
        let nodes = self.host.nodes(cx);
        let mut out = div().flex().flex_col();
        let (mut count, mut chosen_at) = (0, None);
        for node in &nodes {
            let id = node.id.as_str();
            let Some(state) = self.state.nodes.get(id) else {
                continue;
            };
            if let Some(e) = &state.error {
                out = out.child(
                    div()
                        .px(px(size::ROW_PAD))
                        .py(px(size::LABEL_BOTTOM))
                        .flex()
                        .flex_col()
                        .child(kit::device_label(t, &node.device, &node.environment))
                        .child(pane::meta(t, format!("Not read: {e}"), Some(t.error))),
                );
                count += 1;
            }
        }
        // A device's label only where there are tasks on more than one.
        let (rows, several) = self.listed(cx);
        let mut under: Option<&str> = None;
        for (n, task) in rows {
            if several && under != Some(n) {
                under = Some(n);
                if let Some(node) = nodes.iter().find(|x| x.id.as_str() == n) {
                    out = out.child(
                        div()
                            .px(px(size::ROW_PAD))
                            .pt(px(size::LABEL_TOP))
                            .pb(px(size::LABEL_BOTTOM))
                            .child(kit::device_label(t, &node.device, &node.environment)),
                    );
                    count += 1;
                }
            }
            if chosen == Some((n, task.id.as_str())) {
                chosen_at = Some(count);
            }
            out = out.child(self.row(t, n, task, chosen, now, cx));
            count += 1;
        }
        (out, chosen_at)
    }

    fn row(
        &self,
        t: &Theme,
        node: &str,
        task: &Task,
        chosen: Option<(&str, &str)>,
        now: Timestamp,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // The row of the task being edited stays marked; a new task has none.
        let on = match &self.state.form {
            None => chosen == Some((node, task.id.as_str())),
            Some(f) => f.node == node && f.id.as_deref() == Some(task.id.as_str()),
        };
        // When it runs next, or why it will not.
        let (next, next_colour) = if !task.held.is_empty() {
            ("held".to_string(), Some(t.error))
        } else if task.paused {
            ("paused".to_string(), None)
        } else if !task.enabled {
            ("off".to_string(), None)
        } else {
            (form::countdown(task.next_run, now), None)
        };
        // How the last run went: its mark, and since when.
        let failed = !task.last_status.is_empty() && task.last_status != SUCCESS;
        let (mark, last, last_colour) = match task.last_run {
            None => (None, "never run".to_string(), t.dim),
            Some(at) if failed => (
                Some(t.error),
                format!("failed {}", form::ago(at, now)),
                t.error,
            ),
            Some(at) => (Some(t.done), format!("ran {}", form::ago(at, now)), t.dim),
        };
        let mark = match mark {
            Some(c) => kit::state_dot(c),
            None => kit::state_dot(t.dim)
                .bg(gpui::transparent_black())
                .border_1()
                .border_color(t.dim),
        };
        let (pick_node, pick_id) = (node.to_string(), task.id.clone());
        let selector = format!("task-{node}-{}", task.id);
        let name = task.name.clone();
        let result_selector = format!("task-last-{node}-{}-{last}", task.id);
        let chosen_selector = format!("scheduled-chosen-{}", task.id);
        pane::row(t, SharedString::from(selector.clone()), on)
            .debug_selector(move || selector)
            .h(px(TASK_ROW))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(size::HEAD_GAP))
                            .when(on, |d| d.debug_selector(move || chosen_selector))
                            .child(mark)
                            .child(
                                pane::cell(task.name.clone())
                                    .debug_selector(move || format!("scheduled-task-name-{name}")),
                            )
                            .child(pane::meta(t, next, next_colour)),
                    )
                    .child(
                        div()
                            .pl(px(SECOND_LINE_INDENT))
                            .flex()
                            .gap(px(size::HEAD_GAP))
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_size(px(size::SMALL))
                            .font_weight(FontWeight::NORMAL)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .text_color(t.dim)
                                    .child(form::describe(&task.schedule)),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .debug_selector(move || result_selector)
                                    .text_color(last_colour)
                                    .child(last),
                            ),
                    ),
            )
            .on_click(cx.listener(move |page, e: &ClickEvent, window, cx| {
                let key = (pick_node.clone(), pick_id.clone());
                if page.state.form.is_some() {
                    if !page.state.may_leave(now_ms()) {
                        cx.notify();
                        return;
                    }
                    page.state.form = None;
                    page.state.dirty = false;
                }
                if page.state.selected.as_ref() != Some(&key) {
                    page.state.notice = None;
                    page.state.error = None;
                }
                page.state.selected = Some(key);
                if e.click_count() >= 2
                    && let Some(task) = page.state.task(&pick_node, &pick_id).cloned()
                {
                    page.open(TaskForm::edit(&pick_node, &task), window, cx);
                } else {
                    window.focus(&page.focus, cx);
                }
                cx.notify();
            }))
            .into_any_element()
    }

    /// What the last action came to, or why it failed: one line on top of
    /// the detail until something else is done.
    fn notices(&self, t: &Theme) -> Vec<Div> {
        let line = |text: &String, colour| {
            let selector = format!("scheduled-banner-{text}");
            div()
                .debug_selector(move || selector)
                .text_color(colour)
                .child(text.clone())
        };
        let mut out = vec![];
        if let Some(e) = &self.state.error {
            out.push(line(e, t.error));
        }
        if let Some(n) = &self.state.notice {
            out.push(line(n, t.text2));
        }
        out
    }

    fn detail(&self, t: &Theme, node: &str, task: &Task, cx: &mut Context<Self>) -> Div {
        let busy = self.state.busy;
        let at = |n: &str| n.to_string();
        let (edit_node, edit) = (at(node), task.clone());
        let (run_node, run) = (at(node), task.clone());
        let (pause_node, pause) = (at(node), task.clone());
        let paused = task.paused;
        let (more_node, more) = (at(node), task.clone());
        let head = pane::head(t)
            .child(pane::title(t, task.name.clone()))
            .child(
                pane::act(t, "task-edit", "Edit")
                    .debug_selector(|| "task-edit".into())
                    .tooltip(kit::tip("Edit (Enter)"))
                    .on_click(cx.listener(move |page, _, window, cx| {
                        page.open(TaskForm::edit(&edit_node, &edit), window, cx)
                    })),
            )
            .child(
                pane::act(t, "task-run", "Run now")
                    .debug_selector(|| "task-run".into())
                    .tooltip(kit::tip("Send the prompt now"))
                    .when(busy, |b| b.opacity(BUSY))
                    .on_click(cx.listener(move |page, _, _, cx| {
                        if !page.state.busy {
                            page.act(&run_node, &run, "run-now", cx)
                        }
                    })),
            )
            .child(
                pane::act(t, "task-pause", if paused { "Resume" } else { "Pause" })
                    .debug_selector(move || format!("task-pause-{paused}"))
                    .when(busy, |b| b.opacity(BUSY))
                    .on_click(cx.listener(move |page, _, _, cx| {
                        if !page.state.busy {
                            let verb = if pause.paused { "resume" } else { "pause" };
                            page.act(&pause_node, &pause, verb, cx)
                        }
                    })),
            )
            .child(
                pane::act(t, "task-more", "…")
                    .debug_selector(|| "task-more".into())
                    .on_click(cx.listener(move |page, e: &ClickEvent, _, cx| {
                        let rows = page.task_menu(&more_node, &more, cx);
                        page.host.menu(rows, e.position(), cx);
                    })),
            );

        let now = Timestamp::now();
        let mut when = form::describe(&task.schedule);
        let zone = &task.schedule.timezone;
        // "Local" is the node's own zone, as CHROTE writes it.
        if !zone.is_empty() && zone != "Local" && *zone != form::local_zone() {
            when = format!("{when} · {zone}");
        }
        let when = if task.paused {
            format!("{when} · paused")
        } else if !task.enabled {
            format!("{when} · off")
        } else if task.next_run.is_some() {
            format!(
                "{when} · next {} ({})",
                form::when(task.next_run),
                form::countdown(task.next_run, now)
            )
        } else {
            when
        };

        let failed = !task.last_status.is_empty() && task.last_status != SUCCESS;
        let last = match task.last_run {
            None => div().text_color(t.dim).child("Never run"),
            Some(_) => {
                let why = task
                    .recent_runs
                    .first()
                    .filter(|r| r.status != SUCCESS && !r.message.is_empty())
                    .map(|r| r.message.as_str())
                    .unwrap_or_default();
                div().when(failed, |d| d.text_color(t.error)).child(format!(
                    "{} · {}",
                    form::when(task.last_run),
                    form::result(&task.last_status, why)
                ))
            }
        };

        let mut body = div()
            .flex()
            .flex_col()
            .gap(px(size::SECTION_GAP))
            .children(self.notices(t));
        let mut lines = div().flex().flex_col();
        if !task.held.is_empty() {
            lines = lines.child(pane::line(
                t,
                "Held back",
                div().text_color(t.error).child(task.held.clone()),
            ));
        }
        lines = lines
            .child(pane::line(t, "When", div().child(when)))
            .child(pane::line(t, "Sends to", self.sends_to(t, node, task, cx)))
            .child(pane::line(t, "Last run", last));
        body = body.child(lines).child(
            pane::section(t, "Prompt").child(
                div()
                    .whitespace_normal()
                    .text_color(t.text)
                    .child(task.prompt.clone()),
            ),
        );
        // The runs stand beside the task where the window has the room.
        let runs = (!task.recent_runs.is_empty()).then(|| {
            let rows = task
                .recent_runs
                .iter()
                .map(|r| {
                    let ok = r.status == SUCCESS;
                    let mut result = form::result(&r.status, &r.message);
                    // Which sessions it missed, where some got it: where
                    // none did, the reason has said so.
                    let missed: Vec<&str> = r
                        .targets
                        .iter()
                        .filter(|x| x.status != SUCCESS)
                        .map(|x| x.session_name.as_str())
                        .collect();
                    if !missed.is_empty() && missed.len() < r.targets.len() {
                        result = format!("{result} (missed {})", missed.join(", "));
                    }
                    let by = if r.trigger == "manual" {
                        "Run now"
                    } else {
                        "schedule"
                    };
                    let selector =
                        format!("task-run-{}-{}", r.status, form::when(Some(r.started_at)));
                    pane::table_row(
                        t,
                        &RUNS,
                        vec![
                            div()
                                .debug_selector(move || selector)
                                .flex()
                                .items_center()
                                .gap(px(size::HEAD_GAP))
                                .child(kit::state_dot(if ok { t.done } else { t.error }))
                                .child(pane::cell(form::when(Some(r.started_at))))
                                .into_any_element(),
                            pane::cell(by).text_color(t.text2).into_any_element(),
                            pane::cell(result)
                                .text_color(if ok { t.text2 } else { t.error })
                                .into_any_element(),
                        ],
                    )
                    .into_any_element()
                })
                .collect();
            pane::section(t, "Runs, newest first")
                .child(pane::table(t, &RUNS, rows))
                .into_any_element()
        });
        pane::detail_beside(t, "scheduled-detail", head, body, runs, pane::Narrow::Below)
    }

    /// What the head's "…" offers: the actions a task needs now and then.
    fn task_menu(&self, node: &str, task: &Task, cx: &mut Context<Self>) -> Vec<MenuRow> {
        let page = cx.entity().downgrade();
        let (dup_node, dup) = (node.to_string(), task.clone());
        let duplicate = MenuRow::new("Duplicate", {
            let page = page.clone();
            move |window, cx| {
                let _ = page.update(cx, |p, cx| {
                    p.open(TaskForm::duplicate(&dup_node, &dup), window, cx)
                });
            }
        });
        let (del_node, del) = (node.to_string(), task.id.clone());
        let delete = MenuRow::new("Delete", move |_, cx| {
            let _ = page.update(cx, |p, cx| p.delete(&del_node, &del, cx));
        })
        .danger();
        vec![duplicate, delete]
    }

    /// The task's sessions, each a link that shows it in Terminal; one
    /// not running, or running as another session than the task was made
    /// for, is said so.
    fn sends_to(&self, t: &Theme, node: &str, task: &Task, cx: &App) -> Div {
        let mut out = div().flex().flex_wrap().gap(px(size::HEAD_GAP));
        // A node that lists no session at all may only not have been heard
        // yet: then nothing is said of any.
        let running = self.sessions_on(node, cx);
        for target in &task.targets {
            let now = running.iter().find(|s| s.target() == *target);
            let trouble = match (now, task.session_ids.get(&target.label())) {
                _ if running.is_empty() => None,
                (None, _) => Some(("not running", "No session of this name is running")),
                (Some(s), Some(tie)) if !s.id.is_empty() && s.id != tied_session(tie) => Some((
                    "another session",
                    "The session this task was made for was closed or renamed, and another \
                     has its name now. Edit and save the task to send to it.",
                )),
                _ => None,
            };
            let ask = Ask::Reveal {
                what: "session".into(),
                id: target.session_name.clone(),
                node: Some(NodeId(node.to_string())),
                part: None,
                beside: false,
            };
            let host = self.host.clone();
            out = out.child(
                div()
                    .id(SharedString::from(format!(
                        "sends-to-{}",
                        target_key(target)
                    )))
                    .cursor_pointer()
                    .text_color(t.text)
                    .hover(|d| d.underline())
                    .child(match trouble {
                        Some((said, _)) => format!("{} ({said})", target_name(target)),
                        None => target_name(target),
                    })
                    .map(|d| match trouble {
                        Some((_, tip)) => d.text_color(t.error).tooltip(kit::tip(tip)),
                        None => d.tooltip(kit::tip("Show it in Terminal")),
                    })
                    .on_click(move |_, _, cx| {
                        let _ = host.ask(ask.clone(), cx);
                    }),
            );
        }
        out
    }

    fn form(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let form = self.state.form.as_ref().expect("the form is open");
        let busy = self.state.busy;
        let fields = &self.fields;
        let head = pane::head(t)
            .child(pane::title(
                t,
                match form
                    .id
                    .as_deref()
                    .and_then(|id| self.state.task(&form.node, id))
                {
                    Some(task) => format!("Edit {}", task.name),
                    None if form.id.is_some() => "Edit task".to_string(),
                    None => "New task".to_string(),
                },
            ))
            .child(
                pane::act(t, "form-cancel", "Cancel")
                    .tooltip(kit::tip("Esc"))
                    .on_click(cx.listener(|page, _, _, cx| {
                        page.state.close(now_ms());
                        cx.notify();
                    })),
            )
            .child(
                kit::button(
                    t,
                    "form-save",
                    if form.id.is_some() { "Save" } else { "Create" },
                    Kind::Primary,
                )
                .debug_selector(|| "form-save".into())
                .tooltip(kit::tip("Ctrl+Enter"))
                .when(busy, |b| b.opacity(BUSY))
                .on_click(cx.listener(|page, _, _, cx| {
                    if !page.state.busy {
                        page.save(cx)
                    }
                })),
            );
        let body = div()
            .flex()
            .flex_col()
            .gap(px(size::SECTION_GAP))
            .children(self.notices(t))
            .child(
                pane::section(t, "Prompt").child(
                    div()
                        .debug_selector(|| "scheduled-prompt".into())
                        .child(fields.prompt.clone()),
                ),
            )
            .child(pane::section(t, "Send to").child(self.targets(t, form, cx)))
            .child(pane::section(t, "When").child(self.when(t, &form.schedule, cx)))
            .child(pane::section(t, "Name").child(fields.name.clone()));
        pane::detail(t, "scheduled-form", head, body)
    }

    /// The sessions a task may send to, each pressed or not; a task's
    /// sessions are all on one node, so an edited task offers its own.
    fn targets(&self, t: &Theme, form: &TaskForm, cx: &mut Context<Self>) -> Div {
        let nodes: Vec<_> = self
            .host
            .nodes(cx)
            .into_iter()
            .filter(|n| form.id.is_none() || n.id.as_str() == form.node)
            .map(|n| {
                let sessions = self.sessions_on(n.id.as_str(), cx);
                (n, sessions)
            })
            .filter(|(n, s)| !s.is_empty() || n.id.as_str() == form.node)
            .collect();
        let several = nodes.len() > 1;
        let mut out = div().flex().flex_col().gap(px(size::HEAD_GAP));
        for (node, sessions) in nodes {
            let id = node.id.as_str();
            let mut row = div().flex().flex_wrap().gap(px(TOGGLE_GAP));
            let mut offered: Vec<Target> = sessions.iter().map(|s| s.target()).collect();
            // A chosen session that is not running now stays on show.
            if form.node == id {
                for chosen in &form.targets {
                    if !offered.contains(chosen) {
                        offered.push(chosen.clone());
                    }
                }
            }
            if offered.is_empty() {
                row = row.child(div().text_color(t.dim).child("No session is running."));
            }
            for target in offered {
                let running = sessions.iter().any(|s| s.target() == target);
                let on = form.node == id && form.targets.contains(&target);
                let selector = format!("pick-{id}-{}", target_key(&target));
                let chosen = format!("chosen-{}", target_key(&target));
                let (at, session) = (id.to_string(), target.clone());
                row = row.child(
                    toggle(t, selector.clone(), "", on)
                        .debug_selector(move || selector)
                        .when(!running, |b| b.tooltip(kit::tip("Not running now")))
                        .child(
                            div()
                                .when(on, |d| d.debug_selector(move || chosen))
                                .when(!running, |d| d.text_color(t.dim))
                                .child(target_name(&target)),
                        )
                        .on_click(cx.listener(move |page, _, _, cx| page.pick(&at, &session, cx))),
                );
            }
            out = out.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(TOGGLE_GAP))
                    .when(several, |d| {
                        d.child(kit::device_label(t, &node.device, &node.environment))
                    })
                    .child(row),
            );
        }
        out
    }

    fn when(&self, t: &Theme, s: &ScheduleForm, cx: &mut Context<Self>) -> Div {
        let fields = &self.fields;
        let mut modes = div().flex().flex_wrap().gap(px(TOGGLE_GAP));
        for (mode, label) in MODES {
            let selector = format!("mode-{label}");
            modes = modes.child(
                toggle(t, selector.clone(), label, s.mode == mode)
                    .debug_selector(move || selector)
                    .on_click(cx.listener(move |page, _, _, cx| {
                        page.state.change(|f| f.schedule.mode = mode);
                        cx.notify();
                    })),
            );
        }
        let mut out = div().flex().flex_col().gap(px(size::HEAD_GAP)).child(modes);
        let measure = match s.mode {
            Mode::Every => div()
                .flex()
                .items_center()
                .gap(px(TOGGLE_GAP))
                .child("Every")
                .child(div().w(px(SHORT_FIELD)).child(fields.every.clone()))
                .child(unit(t, "minutes", false, s.every_hours, cx))
                .child(unit(t, "hours", true, s.every_hours, cx)),
            Mode::Daily | Mode::Weekly => div()
                .flex()
                .items_center()
                .gap(px(TOGGLE_GAP))
                .child("At")
                .child(div().w(px(SHORT_FIELD)).child(fields.time.clone())),
            Mode::Cron => div().child(
                div()
                    .debug_selector(|| "scheduled-cron".into())
                    .w(px(CRON_FIELD))
                    .child(fields.cron.clone()),
            ),
        };
        out = out.child(measure);
        if s.mode == Mode::Weekly {
            let mut days = div().flex().flex_wrap().gap(px(TOGGLE_GAP));
            for (value, short, _) in WEEKDAYS {
                let on = s.weekdays.contains(&value);
                days = days.child(
                    toggle(t, format!("day-{short}"), short, on)
                        .min_w(px(DAY_W))
                        .on_click(cx.listener(move |page, _, _, cx| {
                            page.state.change(|f| {
                                let w = &mut f.schedule.weekdays;
                                match w.iter().position(|d| *d == value) {
                                    Some(i) => {
                                        w.remove(i);
                                    }
                                    None => w.push(value),
                                }
                            });
                            cx.notify();
                        })),
                );
            }
            out = out.child(days);
        }
        // "Every" counts minutes, which no zone changes.
        if s.mode == Mode::Every {
            return match s.error() {
                Some(why) => out.child(div().text_color(t.error).child(why)),
                None => out.child(div().text_color(t.dim).child(form::describe(&s.schedule()))),
            };
        }
        // The zone is said only to be changed: this computer's, the task's,
        // or UTC.
        let local = form::local_zone();
        let mut zones: Vec<String> = vec![local.clone()];
        for z in [s.timezone.clone(), "UTC".into()] {
            if !z.is_empty() && !zones.contains(&z) {
                zones.push(z);
            }
        }
        let mut zone_row = div().flex().flex_wrap().gap(px(TOGGLE_GAP));
        for z in zones {
            let on = s.timezone == z;
            zone_row = zone_row.child(toggle(t, format!("zone-{z}"), &z, on).on_click(
                cx.listener(move |page, _, _, cx| {
                    page.state.change(|f| f.schedule.timezone = z.clone());
                    cx.notify();
                }),
            ));
        }
        out = out.child(zone_row);
        match s.error() {
            Some(why) => out.child(div().text_color(t.error).child(why)),
            None => out.child(div().text_color(t.dim).child(form::describe(&s.schedule()))),
        }
    }
}

fn target_key(t: &Target) -> String {
    format!("{}:{}", t.unix_user, t.session_name)
}

/// A session's name, with its user's when it is not the node's own.
fn target_name(t: &Target) -> String {
    if t.unix_user.is_empty() {
        t.session_name.clone()
    } else {
        format!("{}:{}", t.unix_user, t.session_name)
    }
}

/// A choice pressed or not: a mode, a session, a weekday, a zone, a unit.
/// Pressed reads on a quiet fill in weight, as a chosen row does.
fn toggle(t: &Theme, id: String, label: &str, on: bool) -> gpui::Stateful<Div> {
    kit::button(t, SharedString::from(id), label.to_string(), Kind::Plain)
        .when(on, |b| {
            b.bg(t.raised)
                .border_color(t.text2)
                .text_color(t.text)
                .font_weight(FontWeight::SEMIBOLD)
        })
        .when(!on, |b| b.text_color(t.text2))
}

fn unit(
    t: &Theme,
    label: &'static str,
    hours: bool,
    now_hours: bool,
    cx: &mut Context<ScheduledPage>,
) -> gpui::Stateful<Div> {
    toggle(t, format!("unit-{label}"), label, hours == now_hours).on_click(cx.listener(
        move |page, _, _, cx| {
            page.state.change(|f| f.schedule.every_hours = hours);
            cx.notify();
        },
    ))
}
