//! The page: what it reads from each node, what the form changes, and the
//! requests that save, act and delete. Drawing is in [`crate::view`].

use std::time::Duration;

use ahjo_app_kit::host::Request;
use ahjo_app_kit::hub::now_ms;
use ahjo_app_kit::link::client::LinkState;
use ahjo_app_kit::widgets::field::{self, FieldEvent, TextField};
use ahjo_app_kit::{Host, Page};
use ahjo_core::NodeId;
use ahjo_scheduled_contract::{CHANGED, INTENT, INTENT_VALUE, PATH, Target, Task, WATCH};
use ahjo_terminals_contract::SessionList;
use gpui::{
    App, AppContext, Context, Entity, FocusHandle, Focusable, KeyDownEvent, ScrollHandle, Window,
};
use serde_json::{Value, json};

use crate::ACTOR;
use crate::form::{Mode, TaskForm};
use crate::state::{Saved, Scheduled, data};

/// How often the countdowns move: CHROTE's.
const TICK: Duration = Duration::from_secs(30);

/// The page's text fields.
pub(crate) struct Fields {
    pub prompt: Entity<TextField>,
    pub name: Entity<TextField>,
    pub every: Entity<TextField>,
    pub time: Entity<TextField>,
    pub cron: Entity<TextField>,
}

#[derive(Clone, Copy)]
enum Field {
    Prompt,
    Name,
    Every,
    Time,
    Cron,
}

/// A session a task may send to, as a node's terminals list it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Session {
    /// The server's id for it (tmux's `$3`): what a task's target was
    /// chosen as.
    pub id: String,
    pub name: String,
    pub user: Option<String>,
}

impl Session {
    pub fn target(&self) -> Target {
        Target {
            session_name: self.name.clone(),
            unix_user: self.user.clone().unwrap_or_default(),
        }
    }
}

pub struct ScheduledPage {
    pub(crate) host: Host,
    pub(crate) state: Scheduled,
    pub(crate) fields: Fields,
    /// The sessions the Sessions page hears, across nodes.
    sessions: Option<Entity<SessionList>>,
    /// The page is on show.
    shown: bool,
    /// The countdown clock runs (only while the page is shown).
    ticking: bool,
    /// The keyboard's place while no form is open: the list's keys.
    pub(crate) focus: FocusHandle,
    /// The list's scroll, so the keys keep the chosen task in view.
    pub(crate) scroll: ScrollHandle,
    /// The chosen task moved by a key: the next draw scrolls to it.
    pub(crate) reveal: bool,
    /// The week as last worked out, and what it was worked out from.
    pub(crate) week: std::cell::RefCell<Option<(u64, std::rc::Rc<crate::week::Week>)>>,
    /// Itself, for its keys as the keys panel runs them.
    me: gpui::WeakEntity<Self>,
}

impl Page for ScheduledPage {
    const ID: &'static str = crate::NAME;
    const TITLE: &'static str = "Scheduled";
    const KEY: Option<char> = Some('S');
    const AREA: Option<&'static str> = Some(crate::NAME);
    const KINDS: ahjo_core::frame::Kinds = &[CHANGED..=CHANGED];
    const CHANNELS: &'static [u8] = &[WATCH];

    /// The prompt while a task is written, else the list.
    fn focus(&self, cx: &App) -> Option<FocusHandle> {
        Some(match self.state.form {
            Some(_) => self.fields.prompt.focus_handle(cx),
            None => self.focus.clone(),
        })
    }

    fn frame(&mut self, node: &NodeId, _: u8, _: &[u8], _: &mut Window, cx: &mut Context<Self>) {
        if self.shown {
            self.read(node.as_str(), cx);
        }
    }

    /// A node that came into reach while the page shows is read at once;
    /// one that came while it was behind is read when it comes forward.
    fn link(
        &mut self,
        node: &NodeId,
        state: Option<LinkState>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.shown && state == Some(LinkState::Open) {
            self.read(node.as_str(), cx);
        }
    }

    fn shown(&mut self, shown: bool, _: &mut Window, cx: &mut Context<Self>) {
        self.shown = shown;
        if shown {
            self.read_all(cx);
        }
    }

    /// The keys the list takes, so the keys panel lists them.
    fn chords(&self, _: &App) -> Vec<ahjo_app_kit::chords::Chord> {
        let (new, edit) = (self.me.clone(), self.me.clone());
        vec![
            ahjo_app_kit::chords::Chord::on_page("N", "New scheduled task", move |window, cx| {
                let _ = new.update(cx, |page, cx| page.new_task(window, cx));
            }),
            ahjo_app_kit::chords::Chord::on_page("Enter", "Edit the task", move |window, cx| {
                let _ = edit.update(cx, |page, cx| page.edit_shown(window, cx));
            }),
        ]
    }
}

impl ScheduledPage {
    pub fn new(host: Host, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut make = |which: Field, placeholder: &str, prose: bool| {
            let f = cx.new(|cx| {
                let f = TextField::new(cx, placeholder);
                if prose { f.prose() } else { f }
            });
            cx.subscribe_in(&f, window, move |page, f, ev, window, cx| match ev {
                FieldEvent::Changed => {
                    let text = f.read(cx).text.clone();
                    page.state.change(|form| match which {
                        Field::Prompt => form.prompt = text,
                        Field::Name => form.name = text,
                        Field::Every => form.schedule.every_value = text,
                        Field::Time => form.schedule.time = text,
                        Field::Cron => form.schedule.cron = text,
                    });
                    cx.notify();
                }
                // Ctrl+Enter saves from any field; Enter does from the
                // one-line ones.
                FieldEvent::SubmitAlt | FieldEvent::Submit => {
                    if !page.state.busy {
                        page.save(cx)
                    }
                }
                FieldEvent::Tab => page.next_field(which, window, cx),
                _ => {}
            })
            .detach();
            f
        };
        let fields = Fields {
            prompt: make(Field::Prompt, "Continue if work is clear…", true),
            name: make(
                Field::Name,
                "Defaults to the first line of the prompt",
                false,
            ),
            every: make(Field::Every, "30", false),
            time: make(Field::Time, "HH:MM", false),
            cron: make(Field::Cron, "minute hour day month weekday", false),
        };
        let sessions = host.model::<SessionList>(cx);
        if let Some(sessions) = &sessions {
            cx.observe(sessions, |_, _, cx| cx.notify()).detach();
        }
        ScheduledPage {
            host,
            state: Scheduled::default(),
            fields,
            sessions,
            shown: false,
            ticking: false,
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            reveal: false,
            week: Default::default(),
            me: cx.weak_entity(),
        }
    }

    /// A node's device and environment, for words.
    pub(crate) fn place_name(&self, node: &str, cx: &gpui::App) -> String {
        self.host
            .nodes(cx)
            .into_iter()
            .find(|n| n.id.as_str() == node)
            .map(|n| format!("{} ({})", n.device, n.environment))
            .unwrap_or_else(|| node.to_string())
    }

    /// Sends `request` to `node` and hands the answer's status and body to
    /// `done`; a request that went nowhere reads as status 0 and its reason.
    fn ask(
        &mut self,
        request: Request,
        done: impl FnOnce(&mut Self, u16, Vec<u8>, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let asked = self.host.request(request, cx);
        cx.spawn(async move |this, cx| {
            let (status, body) = match asked.await {
                Ok(a) => (a.status, a.body),
                Err(e) => (0, e.message.into_bytes()),
            };
            let _ = this.update(cx, |page, cx| done(page, status, body, cx));
        })
        .detach();
    }

    /// A change to the node's tasks: it carries the intent header.
    fn write(node: &str, method: &'static str, path: &str, body: Option<Value>) -> Request {
        let request =
            Request::new(method, NodeId(node.to_string()), path).header(INTENT, INTENT_VALUE);
        match body {
            Some(body) => request.json(&body),
            None => request,
        }
    }

    /// Reads every live node's tasks (the page opened).
    pub(crate) fn read_all(&mut self, cx: &mut Context<Self>) {
        for node in self.host.nodes(cx) {
            self.read(node.id.as_str(), cx);
        }
        self.tick(cx);
    }

    /// The sessions on `node` a task may send to, by name.
    pub(crate) fn sessions_on(&self, node: &str, cx: &App) -> Vec<Session> {
        let Some(list) = &self.sessions else {
            return vec![];
        };
        let mut sessions: Vec<Session> = list
            .read(cx)
            .sessions
            .iter()
            .filter(|s| s.node == node)
            .map(|s| Session {
                id: s.id.clone(),
                name: s.name.clone(),
                user: s.user.clone(),
            })
            .collect();
        sessions.sort_by(|a, b| a.name.cmp(&b.name));
        sessions
    }

    /// Reads one node's tasks, once.
    pub(crate) fn read(&mut self, node: &str, cx: &mut Context<Self>) {
        let n = self.state.nodes.entry(node.to_string()).or_default();
        if n.reading {
            n.reread = true;
            return;
        }
        n.reading = true;
        let at = node.to_string();
        self.ask(
            Request::get(NodeId(node.to_string()), PATH),
            move |page, status, body, cx| {
                let n = page.state.nodes.entry(at.clone()).or_default();
                if std::mem::take(&mut n.reread) {
                    n.reading = false;
                    page.read(&at, cx);
                    return;
                }
                let before = page
                    .state
                    .selected_task()
                    .map(|(n, t)| (n.to_string(), t.clone()));
                page.state
                    .nodes
                    .entry(at)
                    .or_default()
                    .listed(status, &body);
                let after = page
                    .state
                    .selected_task()
                    .map(|(n, t)| (n.to_string(), t.clone()));
                if before != after && status == 200 {
                    page.state.notice = None;
                }
                cx.notify();
            },
            cx,
        );
    }

    /// The countdowns move every 30 s while the page is shown.
    fn tick(&mut self, cx: &mut Context<Self>) {
        if self.ticking {
            return;
        }
        self.ticking = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(TICK).await;
                let go_on = this
                    .update(cx, |page, cx| {
                        if !page.shown {
                            page.ticking = false;
                        }
                        cx.notify();
                        page.shown
                    })
                    .unwrap_or(false);
                if !go_on {
                    break;
                }
            }
        })
        .detach();
    }

    /// Fills the fields from the form just opened.
    fn fill(&mut self, cx: &mut Context<Self>) {
        let Some(f) = &self.state.form else {
            return;
        };
        let fields = &self.fields;
        let pairs = [
            (fields.prompt.clone(), f.prompt.clone()),
            (fields.name.clone(), f.name.clone()),
            (fields.every.clone(), f.schedule.every_value.clone()),
            (fields.time.clone(), f.schedule.time.clone()),
            (fields.cron.clone(), f.schedule.cron.clone()),
        ];
        for (field, text) in pairs {
            field.update(cx, |f, cx| f.set(&text, false, cx));
        }
    }

    /// Opens the form with the keyboard in its prompt.
    pub(crate) fn open(&mut self, form: TaskForm, window: &mut Window, cx: &mut Context<Self>) {
        if self.state.open(form, now_ms()) {
            self.fill(cx);
            field::focus(&self.fields.prompt, window, cx);
        }
        cx.notify();
    }

    /// Tab goes on to the form's next field: the prompt, the schedule's
    /// one field, the name, and round again.
    fn next_field(&mut self, from: Field, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = &self.state.form else {
            return;
        };
        let fields = &self.fields;
        let measure = match form.schedule.mode {
            Mode::Every => &fields.every,
            Mode::Daily | Mode::Weekly => &fields.time,
            Mode::Cron => &fields.cron,
        };
        let to = match from {
            Field::Prompt => measure,
            Field::Every | Field::Time | Field::Cron => &fields.name,
            Field::Name => &fields.prompt,
        };
        field::focus(to, window, cx);
        // What is there is selected, so typing replaces it.
        to.update(cx, |f, cx| {
            let text = f.text.clone();
            f.set(&text, true, cx);
        });
    }

    /// The form's fields are gone from the page but one still holds the
    /// keyboard (the form was saved or left): the list takes it back.
    pub(crate) fn keys_to_list(&self, window: &mut Window, cx: &mut Context<Self>) {
        let f = &self.fields;
        let held = [&f.prompt, &f.name, &f.every, &f.time, &f.cron]
            .iter()
            .any(|field| field.read(cx).is_focused(window));
        if self.state.form.is_none() && held {
            window.focus(&self.focus, cx);
        }
    }

    /// The list's keys: the arrows, Home and End choose a task, Enter
    /// edits it, N starts a new one; Esc leaves the form.
    pub(crate) fn key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.keystroke.modifiers.modified() {
            return;
        }
        let key = event.keystroke.key.as_str();
        if self.state.form.is_some() {
            if key != "escape" {
                return;
            }
            self.state.close(now_ms());
        } else {
            let order = self.order(cx);
            let at = self
                .shown_task()
                .and_then(|(n, t)| order.iter().position(|(on, id)| on == n && *id == t.id));
            let to = match key {
                "down" => at.map_or(0, |i| i + 1),
                "up" => at.map_or(0, |i| i.saturating_sub(1)),
                "home" => 0,
                "end" => order.len().saturating_sub(1),
                "enter" => {
                    if self.edit_shown(window, cx) {
                        cx.stop_propagation();
                    }
                    return;
                }
                "n" => {
                    self.new_task(window, cx);
                    cx.stop_propagation();
                    return;
                }
                _ => return,
            };
            let Some(to) = order.get(to.min(order.len().saturating_sub(1))) else {
                return;
            };
            if self.state.selected.as_ref() != Some(to) {
                self.state.notice = None;
                self.state.error = None;
            }
            self.state.selected = Some(to.clone());
            self.reveal = true;
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// Edit opens on the task on show; whether there was one.
    pub(crate) fn edit_shown(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some((node, task)) = self.shown_task() else {
            return false;
        };
        let form = TaskForm::edit(node, task);
        self.open(form, window, cx);
        true
    }

    /// New task: on the node of the selected task, or the first live node
    /// (the serving node's).
    pub(crate) fn new_task(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let node = self
            .state
            .selected
            .as_ref()
            .map(|(n, _)| n.clone())
            .or_else(|| {
                self.host
                    .nodes(cx)
                    .first()
                    .map(|n| n.id.as_str().to_string())
            })
            .unwrap_or_default();
        self.open(TaskForm::new(&node, vec![]), window, cx);
    }

    /// Adds or removes a session as a target. A session of another node
    /// moves a new task there, with its sessions; an edited task stays on
    /// its node.
    pub(crate) fn pick(&mut self, node: &str, target: &Target, cx: &mut Context<Self>) {
        let target = target.clone();
        let Some(form) = &self.state.form else {
            return;
        };
        if form.node != node {
            if form.id.is_some() {
                let place = self.place_name(&form.node.clone(), cx);
                self.state.notice = Some(format!(
                    "This task runs on {place}; its sessions are there."
                ));
                cx.notify();
                return;
            }
            let moved = !form.targets.is_empty();
            self.state.change(|f| {
                f.node = node.to_string();
                f.targets = vec![target];
            });
            if moved {
                let place = self.place_name(node, cx);
                self.state.notice = Some(format!(
                    "A task sends into one place's sessions: now {place}."
                ));
            }
            cx.notify();
            return;
        }
        self.state.change(|f| f.toggle(target));
        cx.notify();
    }

    pub(crate) fn save(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.state.form.clone() else {
            return;
        };
        if let Some(why) = form.error() {
            self.state.error = Some(why.into());
            cx.notify();
            return;
        }
        self.state.busy = true;
        self.state.error = None;
        let (method, path) = match &form.id {
            Some(id) => ("PATCH", format!("{PATH}/{id}")),
            None => ("POST", PATH.to_string()),
        };
        let node = form.node.clone();
        let request = Self::write(&node, method, &path, Some(form.body()));
        self.ask(
            request,
            move |page, status, body, cx| {
                let sc = &mut page.state;
                sc.busy = false;
                match data::<Saved>(status, &body) {
                    Ok(saved) => {
                        sc.notice = Some(
                            if form.id.is_some() {
                                "Task updated."
                            } else {
                                "Task created."
                            }
                            .into(),
                        );
                        sc.selected = Some((node.clone(), saved.task.id.clone()));
                        sc.nodes.entry(node.clone()).or_default().put(saved.task);
                        sc.form = None;
                        sc.dirty = false;
                        page.read(&node, cx);
                    }
                    Err(e) => sc.error = Some(e),
                }
                cx.notify();
            },
            cx,
        );
        cx.notify();
    }

    /// Pause, resume or run now.
    pub(crate) fn act(
        &mut self,
        node: &str,
        task: &Task,
        action: &'static str,
        cx: &mut Context<Self>,
    ) {
        self.state.busy = true;
        self.state.error = None;
        let path = format!("{PATH}/{}/{action}", task.id);
        let (at, targets) = (node.to_string(), task.targets.len());
        let request = Self::write(node, "POST", &path, Some(json!({ "updatedBy": ACTOR })));
        self.ask(
            request,
            move |page, status, body, cx| {
                let sc = &mut page.state;
                sc.busy = false;
                match data::<Saved>(status, &body) {
                    Ok(saved) => {
                        sc.notice = Some(match (action, &saved.run) {
                            ("run-now", Some(run)) => Scheduled::sent_words(run, targets),
                            ("pause", _) => "Task paused.".into(),
                            _ => "Task resumed.".into(),
                        });
                        sc.nodes.entry(at.clone()).or_default().put(saved.task);
                        page.read(&at, cx);
                    }
                    // Why it is held is said beside the task (the hold is
                    // announced, so it reads again): not twice.
                    Err(e) if e.starts_with(ahjo_scheduled_contract::HELD) => {
                        sc.error = Some("Nothing was sent: the task is held.".into());
                    }
                    Err(e) => sc.error = Some(e),
                }
                cx.notify();
            },
            cx,
        );
        cx.notify();
    }

    /// Deletes the task; the menu that offers it has asked twice.
    pub(crate) fn delete(&mut self, node: &str, id: &str, cx: &mut Context<Self>) {
        if self.state.busy {
            return;
        }
        self.state.busy = true;
        let (at, gone) = (node.to_string(), id.to_string());
        let request = Self::write(node, "DELETE", &format!("{PATH}/{id}"), None);
        self.ask(
            request,
            move |page, status, body, cx| {
                let sc = &mut page.state;
                sc.busy = false;
                match data::<Value>(status, &body) {
                    Ok(_) => {
                        sc.notice = Some("Task deleted.".into());
                        if sc.form.as_ref().and_then(|f| f.id.as_deref()) == Some(gone.as_str()) {
                            sc.form = None;
                            sc.dirty = false;
                        }
                        if let Some(list) = sc.nodes.get_mut(&at).and_then(|n| n.list.as_mut()) {
                            list.retain(|t| t.id != gone);
                        }
                        page.read(&at, cx);
                    }
                    Err(e) => sc.error = Some(e),
                }
                cx.notify();
            },
            cx,
        );
        cx.notify();
    }
}
