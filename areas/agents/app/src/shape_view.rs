//! The Agent view: what an agent started in a folder gets. The list picks
//! the folder (a typed path, the running sessions' folders, the recent
//! ones); the detail draws the agent's anatomy on the left (what it reads
//! at the start and later, the skills it sees, its hooks, settings and
//! servers) and on the right the one part opened, read or edited in place.

use crate::shape_state::{self as sh, Load, Pick, Shape, Text};
use crate::{AgentsPage, tokens};
use ahjo_app_kit::host::Status;
use ahjo_app_kit::theme::{Theme, size, theme};
use ahjo_app_kit::widgets::code::{Code, CodeEvent};
use ahjo_app_kit::widgets::field::{FieldEvent, TextField};
use ahjo_app_kit::widgets::kit::{self, Kind};
use ahjo_app_kit::widgets::{markdown, pane, syntax};
use gpui::{
    AnyElement, AppContext, Context, Div, Entity, Hsla, SharedString, Stateful, Window, div,
    prelude::*, px, relative,
};
use serde_json::{Value, json};

/// The entities the Agent and Skills views type into, made once.
pub(crate) struct Inputs {
    pub folder: Entity<TextField>,
    pub reader: Entity<Code>,
    pub event: Entity<TextField>,
    pub matcher: Entity<TextField>,
    pub command: Entity<TextField>,
    pub timeout: Entity<TextField>,
    pub key: Entity<TextField>,
    pub value: Entity<TextField>,
    pub find: Entity<TextField>,
}

impl Inputs {
    pub fn new(window: &mut Window, cx: &mut Context<AgentsPage>) -> Self {
        let folder = cx.new(|cx| TextField::new(cx, "Folder, such as ~/project"));
        cx.subscribe_in(&folder, window, |page, field, event, _, cx| {
            if let FieldEvent::Submit = event {
                let dir = field.read(cx).text.trim().to_owned();
                if !dir.is_empty() {
                    page.shape_open(None, dir, None, cx);
                }
            }
        })
        .detach();
        // Instructions are prose with long lines: the editor wraps them, as
        // the Files editor does by default.
        let reader = cx.new(|cx| {
            let mut c = Code::new(cx);
            c.set_wrap(true, cx);
            c
        });
        cx.subscribe_in(&reader, window, |page, _, event, window, cx| match event {
            CodeEvent::Save => page.file_save(cx),
            CodeEvent::Escape => page.file_discard(window, cx),
            CodeEvent::Changed | CodeEvent::Moved => {}
        })
        .detach();
        let field = |cx: &mut Context<AgentsPage>, hint: &str| {
            let hint = hint.to_owned();
            cx.new(move |cx| TextField::new(cx, &hint))
        };
        let find = field(cx, "Find a skill");
        cx.subscribe_in(&find, window, |_, _, event, _, cx| {
            if let FieldEvent::Changed = event {
                cx.notify();
            }
        })
        .detach();
        Self {
            folder,
            reader,
            event: field(cx, "PreToolUse"),
            matcher: field(cx, "Every one, or a tool such as Bash"),
            command: field(cx, "The command it runs"),
            timeout: field(cx, "Seconds, or the harness's own"),
            key: field(cx, "permissions.defaultMode"),
            value: field(cx, "A value, as words or JSON"),
            find,
        }
    }
}

/// A value as typed: JSON when it reads as JSON, otherwise the words.
fn typed(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_owned()))
}

/// The files a new hook can go in: settings files for Claude Code,
/// `hooks.json` files for Codex; the operator's own and the project's,
/// never the managed ones.
pub(crate) fn hook_files(shape: &Shape) -> Vec<String> {
    shape
        .files
        .iter()
        .filter(|f| f.hooks && f.origin != sh::Origin::Managed)
        .map(|f| f.path.clone())
        .collect()
}

/// The files a setting can go in.
pub(crate) fn setting_files(shape: &Shape) -> Vec<String> {
    shape
        .files
        .iter()
        .filter(|f| f.origin != sh::Origin::Managed && !f.path.ends_with("hooks.json"))
        .filter(|f| f.path.ends_with(".json") || f.path.ends_with(".toml"))
        .map(|f| f.path.clone())
        .collect()
}

impl AgentsPage {
    /// Shows what an agent started in `dir` gets, by `harness` (the one
    /// shown, unless named), asking `node` (the first, unless named).
    pub(crate) fn shape_open(
        &mut self,
        node: Option<String>,
        dir: String,
        harness: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let v = &mut self.shape;
        let node = node
            .or_else(|| (!v.node.is_empty()).then(|| v.node.clone()))
            .or_else(|| self.host.nodes(cx).first().map(|n| n.id.to_string()));
        let Some(node) = node else {
            v.error = Some("No node is connected.".into());
            cx.notify();
            return;
        };
        let other_harness = harness.as_ref().is_some_and(|h| *h != v.harness);
        if (v.dir != dir || v.node != node || other_harness) && v.editing {
            self.host.report(
                Status::failure("Save or discard the open file first (Ctrl+S, Escape)"),
                cx,
            );
            return;
        }
        // Another folder or harness is not shown as the one before it
        // while it is read.
        if v.dir != dir || v.node != node || other_harness {
            v.shape = None;
        }
        if v.dir != dir || v.node != node {
            v.pick = Pick::None;
            v.file = None;
            v.editing = false;
            v.reading = false;
        }
        v.node = node;
        v.dir = dir.clone();
        if let Some(h) = harness {
            v.harness = h;
        }
        if v.harness.is_empty() {
            v.harness = "claude".into();
        }
        self.kept.harness = self.shape.harness.clone();
        let _ = self.host.set_setting(&self.kept, cx);
        self.shape_load(cx);
    }

    pub(crate) fn shape_load(&mut self, cx: &mut Context<Self>) {
        let v = &mut self.shape;
        if v.dir.is_empty() {
            return;
        }
        let asked = (v.dir.clone(), v.harness.clone());
        v.asking = Some(asked.clone());
        let path = format!(
            "{}?dir={}&harness={}",
            sh::PATH,
            sh::encode(&asked.0),
            sh::encode(&asked.1)
        );
        let node = v.node.clone();
        self.mission_call(&node, "GET", &path, None, cx, move |page, answer, cx| {
            let v = &mut page.shape;
            if v.asking.as_ref() != Some(&asked) {
                return;
            }
            v.asking = None;
            match crate::library::response::<Shape>(answer) {
                Ok(shape) => {
                    v.dir = shape.dir.clone();
                    // Only a folder that could be read is offered again.
                    sh::remember(&mut page.kept.recent, &shape.dir);
                    let _ = page.host.set_setting(&page.kept, cx);
                    let v = &mut page.shape;
                    // What the other harness, or the folder before a
                    // change on disk, had open is not this one's: the
                    // reader follows what is shown.
                    if !v.editing && !sh::holds(&shape, &v.pick) {
                        v.pick = Pick::None;
                        v.file = None;
                    }
                    // The reader opens on what the folder itself tells its
                    // agent, rather than on an empty half.
                    let first = (v.pick == Pick::None && !v.editing)
                        .then(|| sh::opening(&shape))
                        .flatten();
                    v.shape = Some(shape);
                    v.error = None;
                    if let Some(path) = first {
                        page.shape.pick = Pick::File(path.clone());
                        page.file_read(path, cx);
                    }
                }
                Err(e) => {
                    v.shape = None;
                    v.error = Some(e);
                }
            }
            cx.notify();
        });
        cx.notify();
    }

    pub(crate) fn shape_pick(&mut self, pick: Pick, window: &mut Window, cx: &mut Context<Self>) {
        if self.shape.editing && self.shape.pick != pick {
            self.host.report(
                Status::failure("Save or discard the open file first (Ctrl+S, Escape)"),
                cx,
            );
            return;
        }
        let set = |field: &Entity<TextField>, text: &str, cx: &mut Context<Self>| {
            field.update(cx, |f, cx| f.set(text, false, cx));
        };
        let i = &self.inputs;
        match &pick {
            Pick::File(path) => self.file_read(path.clone(), cx),
            Pick::Hook(..) => {
                self.shape.pick = pick.clone();
                if let Some(h) = self.shape.hook().cloned() {
                    let i = &self.inputs;
                    set(&i.event, &h.event, cx);
                    set(&i.matcher, h.matcher.as_deref().unwrap_or(""), cx);
                    set(&i.command, &h.run, cx);
                    set(
                        &i.timeout,
                        &h.timeout.map(|t| t.to_string()).unwrap_or_default(),
                        cx,
                    );
                }
            }
            Pick::NewHook(_) => {
                for f in [&i.matcher, &i.command, &i.timeout] {
                    set(f, "", cx);
                }
                let first = sh::events(&self.shape.harness)[0];
                set(&self.inputs.event, first, cx);
                let command = self.inputs.command.clone();
                window.defer(cx, move |window, cx| {
                    ahjo_app_kit::widgets::field::focus(&command, window, cx)
                });
            }
            Pick::Setting(..) => {
                self.shape.pick = pick.clone();
                if let Some(s) = self.shape.setting().cloned() {
                    set(&self.inputs.key, &s.key, cx);
                    set(&self.inputs.value, &sh::value_words(&s.value), cx);
                }
            }
            Pick::NewSetting(_) => {
                set(&i.key, "", cx);
                set(&i.value, "", cx);
                let key = self.inputs.key.clone();
                window.defer(cx, move |window, cx| {
                    ahjo_app_kit::widgets::field::focus(&key, window, cx)
                });
            }
            Pick::None => {}
        }
        self.shape.reading = pick != Pick::None;
        self.shape.pick = pick;
        cx.notify();
    }

    pub(crate) fn file_read(&mut self, path: String, cx: &mut Context<Self>) {
        self.shape.file = None;
        self.shape.editing = false;
        let node = self.shape.node.clone();
        let url = format!("{}?path={}", sh::FILE, sh::encode(&path));
        self.mission_call(&node, "GET", &url, None, cx, move |page, answer, cx| {
            match crate::library::response::<Text>(answer) {
                Ok(text) => {
                    let lang = syntax::lang(&text.path);
                    page.inputs.reader.update(cx, |c, cx| {
                        c.set_lang(lang, cx);
                        c.set(&text.text, cx);
                    });
                    page.shape.file = Some(text);
                }
                Err(e) => page
                    .host
                    .report(Status::failure(format!("Not read: {e}")), cx),
            }
            cx.notify();
        });
    }

    pub(crate) fn file_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.shape.file.is_none() {
            return;
        }
        self.shape.editing = true;
        let reader = self.inputs.reader.clone();
        reader.update(cx, |c, cx| c.edit(true, window, cx));
        Code::focus(&reader, window, cx);
        cx.notify();
    }

    pub(crate) fn file_discard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(file) = &self.shape.file {
            let text = file.text.clone();
            self.inputs.reader.update(cx, |c, cx| {
                c.set(&text, cx);
                c.edit(false, window, cx);
            });
        }
        self.shape.editing = false;
        cx.notify();
    }

    pub(crate) fn file_save(&mut self, cx: &mut Context<Self>) {
        let Some(file) = &self.shape.file else {
            return;
        };
        if self.shape.saving {
            return;
        }
        let text = self.inputs.reader.read(cx).text().to_owned();
        let body = json!(sh::Write {
            path: file.path.clone(),
            text,
            etag: file.etag.clone(),
        });
        self.shape.saving = true;
        let node = self.shape.node.clone();
        self.mission_call(
            &node,
            "PUT",
            sh::FILE,
            Some(body),
            cx,
            |page, answer, cx| {
                page.shape.saving = false;
                match crate::library::response::<Text>(answer) {
                    Ok(text) => {
                        let shown = match &page.shape.shape {
                            Some(sp) => sh::short(&text.path, sp.root.as_deref(), &sp.home),
                            None => text.path.clone(),
                        };
                        page.host.toast(&format!("Saved {shown}"), cx);
                        page.shape.file = Some(text);
                        page.shape.editing = false;
                        page.shape.close_editor = true;
                        page.shape_load(cx);
                        page.catalog_load(cx);
                    }
                    Err(e) => page
                        .host
                        .report(Status::failure(format!("Not saved: {e}")), cx),
                }
                cx.notify();
            },
        );
        cx.notify();
    }

    fn hook_send(&mut self, change: sh::HookChange, file: String, cx: &mut Context<Self>) {
        let edit = sh::HookEdit {
            harness: self.shape.harness.clone(),
            file,
            change,
        };
        let node = self.shape.node.clone();
        self.mission_call(
            &node,
            "POST",
            sh::HOOK,
            Some(json!(edit)),
            cx,
            |page, answer, cx| {
                match crate::library::response::<Value>(answer) {
                    Ok(_) => {
                        page.shape.pick = Pick::None;
                        page.shape_load(cx);
                    }
                    Err(e) => page
                        .host
                        .report(Status::failure(format!("Hook not changed: {e}")), cx),
                }
                cx.notify();
            },
        );
    }

    fn hook_save(&mut self, cx: &mut Context<Self>) {
        let read = |f: &Entity<TextField>, cx: &Context<Self>| f.read(cx).text.trim().to_owned();
        let i = &self.inputs;
        let (event, matcher, command, timeout) = (
            read(&i.event, cx),
            read(&i.matcher, cx),
            read(&i.command, cx),
            read(&i.timeout, cx),
        );
        if command.is_empty() {
            self.host
                .report(Status::failure("A hook needs a command to run"), cx);
            return;
        }
        let timeout = match timeout.parse::<u64>() {
            Ok(n) => Some(n),
            Err(_) if timeout.is_empty() => None,
            Err(_) => {
                self.host
                    .report(Status::failure("The timeout is a number of seconds"), cx);
                return;
            }
        };
        let hook = sh::HookSpec {
            event,
            matcher: (!matcher.is_empty()).then_some(matcher),
            command,
            timeout,
        };
        match self.shape.pick.clone() {
            Pick::NewHook(file) => self.hook_send(sh::HookChange::Add { hook }, file, cx),
            Pick::Hook(file, at) => {
                let Some(was) = self.shape.hook().map(|h| h.run.clone()) else {
                    return;
                };
                self.hook_send(sh::HookChange::Set { at, was, hook }, file, cx)
            }
            _ => {}
        }
    }

    fn setting_send(&mut self, file: String, key: String, value: Value, cx: &mut Context<Self>) {
        if key.is_empty() {
            self.host
                .report(Status::failure("A setting needs its key"), cx);
            return;
        }
        let edit = sh::SettingEdit {
            harness: self.shape.harness.clone(),
            file,
            key,
            value,
        };
        let node = self.shape.node.clone();
        self.mission_call(
            &node,
            "POST",
            sh::SETTING,
            Some(json!(edit)),
            cx,
            |page, answer, cx| {
                match crate::library::response::<Value>(answer) {
                    Ok(_) => {
                        page.shape.pick = Pick::None;
                        page.shape_load(cx);
                    }
                    Err(e) => page
                        .host
                        .report(Status::failure(format!("Setting not changed: {e}")), cx),
                }
                cx.notify();
            },
        );
    }

    /// The folders the list offers: running sessions' first, then recent.
    pub(crate) fn running(&self, cx: &Context<Self>) -> Vec<(String, String, String)> {
        let Some(list) = &self.sessions else {
            return vec![];
        };
        let mut out: Vec<(String, String, String)> = vec![];
        for s in &list.read(cx).sessions {
            let Some(cwd) = &s.cwd else { continue };
            if out.iter().any(|(d, ..)| d == cwd) {
                continue;
            }
            let harness = s
                .command
                .as_deref()
                .and_then(sh::harness_of)
                .unwrap_or_default();
            out.push((cwd.clone(), s.name.clone(), harness.to_owned()));
        }
        out
    }

    pub(crate) fn render_shape(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if std::mem::take(&mut self.shape.close_editor) {
            self.inputs
                .reader
                .update(cx, |c, cx| c.edit(false, window, cx));
        }
        let t = theme(cx).clone();
        let list_head = pane::list_head(&t, Some(self.sub_switch(&t, cx).into_any_element()));
        let mut rows = div().flex().flex_col().child(
            div()
                .px(px(size::ROW_PAD))
                .pb(px(size::HEAD_GAP))
                .debug_selector(|| "shape-folder".into())
                .child(self.inputs.folder.clone()),
        );
        let running = self.running(cx);
        let home = self
            .shape
            .shape
            .as_ref()
            .map(|s| s.home.clone())
            .unwrap_or_default();
        let folder_row = |page: &Self,
                          d: &str,
                          meta: String,
                          colour: Option<Hsla>,
                          cx: &mut Context<Self>,
                          harness: Option<String>| {
            let name = std::path::Path::new(d)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| d.to_owned());
            let chosen = page.shape.dir == d;
            let dir = d.to_owned();
            let selector = format!("shape-dir-{d}");
            pane::row(&t, SharedString::from(selector.clone()), chosen)
                .debug_selector(move || selector)
                .tooltip(kit::tip(sh::short(d, None, &home)))
                .child(pane::cell(name))
                .child(pane::meta(&t, meta, colour))
                .on_click(cx.listener(move |s, _, _, cx| {
                    s.shape_open(None, dir.clone(), harness.clone(), cx)
                }))
        };
        if !running.is_empty() {
            rows = rows.child(pane::label(&t, "Running"));
            for (d, session, harness) in &running {
                let h = (!harness.is_empty()).then(|| harness.clone());
                let colour = h.as_ref().map(|_| t.running);
                rows = rows.child(folder_row(self, d, session.clone(), colour, cx, h));
            }
        }
        let recent: Vec<String> = self
            .kept
            .recent
            .iter()
            .filter(|d| !running.iter().any(|(r, ..)| r == *d))
            .cloned()
            .collect();
        if !recent.is_empty() {
            rows = rows.child(pane::label(&t, "Recent"));
            for d in &recent {
                let parent = std::path::Path::new(d)
                    .parent()
                    .map(|p| sh::short(&p.to_string_lossy(), None, &home))
                    .unwrap_or_default();
                rows = rows.child(folder_row(self, d, parent, None, cx, None));
            }
        }
        let list = pane::list(&t, "shape-list", list_head, rows);
        let detail = self.shape_detail(&t, window, cx);
        pane::page(&t, list, detail)
            .text_color(t.text)
            .into_any_element()
    }

    fn shape_detail(&mut self, t: &Theme, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let v = &self.shape;
        let mut head = pane::head(t).px(px(size::PAD));
        let title = match &v.shape {
            Some(s) => sh::short(&s.dir, None, &s.home),
            None if !v.dir.is_empty() => v.dir.clone(),
            None => "What an agent gets".into(),
        };
        head = head.child(pane::title(t, title));
        for (id, name) in sh::HARNESSES {
            let on = v.harness == *id || v.harness.is_empty() && *id == "claude";
            let harness = (*id).to_owned();
            let selector = format!("shape-harness-{id}");
            head = head.child(
                kit::button(t, SharedString::from(selector.clone()), *name, Kind::Quiet)
                    .debug_selector(move || selector)
                    .when(on, |b| b.text_color(t.accent).border_color(t.divider))
                    .on_click(cx.listener(move |s, _, _, cx| {
                        let dir = s.shape.dir.clone();
                        if dir.is_empty() {
                            s.shape.harness = harness.clone();
                            cx.notify();
                        } else {
                            s.shape_open(None, dir, Some(harness.clone()), cx);
                        }
                    })),
            );
        }
        if !v.dir.is_empty() {
            head = head.child(
                pane::act(t, "shape-refresh", "Refresh")
                    .tooltip(kit::tip("Read the folder again"))
                    .on_click(cx.listener(|s, _, _, cx| s.shape_load(cx))),
            );
        }
        let Some(shape) = v.shape.clone() else {
            let words = if let Some(e) = &v.error {
                e.clone()
            } else if v.asking.is_some() {
                "Reading what an agent there gets…".into()
            } else {
                "Pick a folder: what an agent started there reads, sees and runs.".into()
            };
            let colour = v.error.as_ref().map(|_| t.error);
            return pane::detail_frame(
                head,
                pane::vacant(t, words, None).when_some(colour, |d, c| d.text_color(c)),
            );
        };
        // Too narrow for both columns (a small window, the app zoomed in):
        // one at a time, the part picked read in place of the anatomy.
        let side = self.host.side_room(cx);
        let room = f32::from(window.viewport_size().width) - side - size::LIST;
        if room < tokens::ANATOMY_MIN + tokens::READER_MIN {
            let body = if self.shape.reading {
                div()
                    .size_full()
                    .bg(t.surface)
                    .flex()
                    .flex_col()
                    .child(self.reader_pane(t, &shape, true, window, cx))
            } else {
                div().size_full().child(
                    div()
                        .id("shape-anatomy")
                        .size_full()
                        .overflow_y_scroll()
                        .p(px(size::PAD))
                        .flex()
                        .flex_col()
                        .gap(px(size::SECTION_GAP))
                        .child(self.anatomy(t, &shape, cx)),
                )
            };
            return pane::detail_frame(head, body);
        }
        let anatomy = self.anatomy(t, &shape, cx);
        let reader = self.reader_pane(t, &shape, false, window, cx);
        let body = div()
            .size_full()
            .flex()
            .child(
                div()
                    .id("shape-anatomy")
                    .w(relative(tokens::ANATOMY))
                    .min_w(px(tokens::ANATOMY_MIN))
                    .h_full()
                    .overflow_y_scroll()
                    .p(px(size::PAD))
                    .flex()
                    .flex_col()
                    .gap(px(size::SECTION_GAP))
                    .child(anatomy),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(tokens::READER_MIN))
                    .h_full()
                    .border_l_1()
                    .border_color(t.divider)
                    .bg(t.surface)
                    .flex()
                    .flex_col()
                    .child(reader),
            );
        pane::detail_frame(head, body)
    }

    /// The agent's anatomy: what it costs at the start, then each part.
    fn anatomy(&self, t: &Theme, shape: &Shape, cx: &mut Context<Self>) -> Div {
        let at_start = sh::start_tokens(shape);
        let seen = shape.skills.iter().filter(|s| s.load != Load::Off).count();
        let hooks_on = shape.hooks.iter().filter(|h| h.on).count();
        let untrusted = shape
            .hooks
            .iter()
            .filter(|h| h.on && h.note.is_some())
            .count();
        let servers_on = shape.servers.iter().filter(|s| s.on).count();
        let plural = |n: usize, one: &'static str| -> &'static str {
            match (n, one) {
                (1, _) => one,
                (_, "skill") => "skills",
                (_, "hook") => "hooks",
                _ => "servers",
            }
        };
        let glance = |value: String, unit: &str, colour: Option<Hsla>, selector: &str| {
            let selector = selector.to_owned();
            div().debug_selector(move || selector).child(kit::glance(
                t,
                value,
                unit.to_owned(),
                colour,
            ))
        };
        let glances = div()
            .flex()
            .flex_wrap()
            .gap_x(px(size::SECTION_GAP))
            .gap_y(px(size::HEAD_GAP))
            .child(glance(
                sh::tokens_words(at_start),
                "tokens at start",
                None,
                "shape-tokens",
            ))
            .child(glance(
                seen.to_string(),
                plural(seen, "skill"),
                None,
                "shape-skills",
            ))
            .child(glance(
                hooks_on.to_string(),
                plural(hooks_on, "hook"),
                (untrusted > 0).then_some(t.needs),
                "shape-hooks",
            ))
            .child(glance(
                servers_on.to_string(),
                plural(servers_on, "server"),
                None,
                "shape-servers",
            ));
        let mut out = div()
            .flex()
            .flex_col()
            .gap(px(size::SECTION_GAP))
            .child(glances);
        if !shape.notes.is_empty() {
            let mut notes = div().flex().flex_col().gap(px(tokens::P4));
            for n in &shape.notes {
                notes = notes.child(kit::note(t, n.clone(), Some(t.needs)));
            }
            out = out.child(notes);
        }
        out = out.child(self.instruction_stack(t, shape, cx));
        out = out.child(self.hook_section(t, shape, cx));
        out = out.child(self.skill_section(t, shape, cx));
        out = out.child(self.setting_section(t, shape, cx));
        if !shape.servers.is_empty() {
            let mut servers = pane::section(t, "Servers");
            for s in &shape.servers {
                let why = s.why.clone().unwrap_or_else(|| s.origin.word().to_owned());
                servers = servers.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(size::HEAD_GAP))
                        .min_h(px(size::ROW))
                        .when(!s.on, |d| d.opacity(tokens::DIMMED))
                        .child(kit::state_dot(if s.on { t.running } else { t.dim }))
                        .child(div().flex_none().child(s.name.clone()))
                        .child(pane::cell(s.run.clone()).text_color(t.text2))
                        .child(pane::meta(t, why, None)),
                );
            }
            out = out.child(servers);
        }
        out
    }

    /// One row of the anatomy: picked on a click, quiet fill when picked.
    fn part(
        &self,
        t: &Theme,
        id: SharedString,
        pick: Pick,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let chosen = self.shape.pick == pick;
        let selector = id.to_string();
        div()
            .id(id)
            .debug_selector(move || selector)
            .flex()
            .items_center()
            .gap(px(size::HEAD_GAP))
            .min_h(px(size::ROW))
            .px(px(tokens::P4))
            .rounded(px(size::RADIUS))
            .cursor_pointer()
            .when(chosen, |d| d.bg(t.raised))
            .hover(|d| d.bg(t.accent_soft()))
            .on_click(cx.listener(move |s, _, window, cx| s.shape_pick(pick.clone(), window, cx)))
    }

    fn instruction_stack(&self, t: &Theme, shape: &Shape, cx: &mut Context<Self>) -> Div {
        let most = shape
            .instructions
            .iter()
            .map(|i| i.bytes)
            .max()
            .unwrap_or(1)
            .max(1);
        let root = shape.root.as_deref();
        let mut stack = pane::section(t, "Instructions, in the order it reads them");
        let mut shown_later = false;
        // What it reads at the start, in order; then the rest, in order.
        let mut order: Vec<(usize, &sh::Instruction)> =
            shape.instructions.iter().enumerate().collect();
        order.sort_by_key(|(_, i)| i.load != Load::Start);
        for (n, i) in order {
            if i.load != Load::Start && !shown_later {
                shown_later = true;
                stack = stack.child(
                    div()
                        .pt(px(size::HEAD_GAP))
                        .child(kit::label(t, "Later, or not at all")),
                );
            }
            let share = i.bytes as f32 / most as f32;
            let row = self
                .part(
                    t,
                    SharedString::from(format!("shape-instruction-{n}")),
                    Pick::File(i.path.clone()),
                    cx,
                )
                .when(i.load != Load::Start, |d| d.opacity(tokens::DIMMED))
                .child(
                    div()
                        .w(px(tokens::ORIGIN_W))
                        .flex_none()
                        .text_size(px(size::SMALL))
                        .text_color(t.text2)
                        .child(i.origin.word()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(pane::cell(sh::short(&i.path, root, &shape.home)))
                        .children(i.why.clone().map(|w| {
                            div()
                                .text_size(px(size::SMALL))
                                .text_color(if i.load == Load::Off { t.needs } else { t.dim })
                                .child(w)
                        })),
                )
                .child(
                    div()
                        .w(px(tokens::BAR_W))
                        .h(px(tokens::BAR_H))
                        .flex_none()
                        .rounded(px(tokens::BAR_H))
                        .bg(t.raised)
                        .child(
                            div()
                                .h_full()
                                .rounded(px(tokens::BAR_H))
                                .w(relative(share.max(0.02)))
                                .bg(t.chart),
                        ),
                )
                .child(
                    div()
                        .w(px(tokens::SIZE_W))
                        .flex_none()
                        .flex()
                        .justify_end()
                        .child(pane::meta(
                            t,
                            format!("{} tok", sh::tokens_words(sh::tokens(i.bytes))),
                            None,
                        )),
                );
            stack = stack.child(row);
        }
        if shape.instructions.is_empty() {
            stack = stack.child(kit::note(
                t,
                "No instruction file reaches this folder.",
                None,
            ));
        }
        stack
    }

    fn hook_section(&self, t: &Theme, shape: &Shape, cx: &mut Context<Self>) -> Div {
        let files = hook_files(shape);
        let mut head = div()
            .flex()
            .items_center()
            .justify_between()
            .child(kit::group_label(t, "Hooks: when it happens, what runs"));
        if let Some(file) = files.first().cloned() {
            head = head.child(
                pane::act(t, "shape-hook-add", "Add")
                    .debug_selector(|| "shape-hook-add".into())
                    .on_click(cx.listener(move |s, _, window, cx| {
                        s.shape_pick(Pick::NewHook(file.clone()), window, cx)
                    })),
            );
        }
        let mut section = div()
            .flex()
            .flex_col()
            .child(head.pb(px(size::LABEL_BOTTOM)));
        if shape.hooks.is_empty() {
            section = section.child(kit::note(t, "No hooks: nothing runs on its events.", None));
        }
        for (event, hooks) in sh::by_event(&shape.harness, &shape.hooks) {
            let words = sh::event_words(&event);
            section = section.child(
                div()
                    .pt(px(size::HEAD_GAP))
                    .flex()
                    .gap(px(size::HEAD_GAP))
                    .items_baseline()
                    .child(div().text_color(t.text).child(if words.is_empty() {
                        event.clone()
                    } else {
                        words.to_owned()
                    }))
                    .child(pane::meta(t, event.clone(), None)),
            );
            for h in hooks {
                let at = h.at.clone();
                let id = format!(
                    "shape-hook-{}-{}-{}{}",
                    at.event,
                    at.group,
                    at.index,
                    if at.parked { "-off" } else { "" }
                );
                let switch = {
                    let (file, at, was, on) = (h.file.clone(), at.clone(), h.run.clone(), h.on);
                    kit::toggle(t, SharedString::from(format!("{id}-switch")), on)
                        .debug_selector({
                            let s = format!("{id}-switch");
                            move || s
                        })
                        .tooltip(kit::tip(if on {
                            "On: switch it off, kept to switch back on"
                        } else {
                            "Off: switch it back on"
                        }))
                        .on_click(cx.listener(move |s, _, _, cx| {
                            cx.stop_propagation();
                            s.hook_send(
                                sh::HookChange::Switch {
                                    at: at.clone(),
                                    was: was.clone(),
                                    on: !on,
                                },
                                file.clone(),
                                cx,
                            );
                        }))
                };
                let row = self
                    .part(
                        t,
                        SharedString::from(id),
                        Pick::Hook(h.file.clone(), at),
                        cx,
                    )
                    .items_start()
                    .py(px(tokens::P4))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .when(!h.on, |d| d.opacity(tokens::DIMMED))
                            .child(
                                div()
                                    .flex()
                                    .gap(px(size::HEAD_GAP))
                                    .items_center()
                                    .children(h.matcher.clone().filter(|m| !m.is_empty()).map(
                                        |m| {
                                            kit::badge(
                                                t,
                                                SharedString::from(format!("m-{}", h.run)),
                                                m,
                                                "Its matcher".into(),
                                            )
                                        },
                                    ))
                                    .child(pane::cell(h.run.clone())),
                            )
                            .child(
                                div()
                                    .text_size(px(size::SMALL))
                                    .text_color(t.dim)
                                    .child(sh::short(&h.file, shape.root.as_deref(), &shape.home)),
                            )
                            .children(h.note.clone().map(|n| {
                                div()
                                    .text_size(px(size::SMALL))
                                    .text_color(t.needs)
                                    .child(n)
                            })),
                    )
                    .child(switch);
                section = section.child(row);
            }
        }
        section
    }

    fn skill_section(&self, t: &Theme, shape: &Shape, cx: &mut Context<Self>) -> Div {
        let off = shape.skills.iter().filter(|s| s.load == Load::Off).count();
        let mut head = div()
            .flex()
            .items_center()
            .justify_between()
            .pb(px(size::LABEL_BOTTOM))
            .child(kit::group_label(t, "Skills it sees"));
        if off > 0 {
            let showing = self.shape.show_off_skills;
            head = head.child(
                pane::act(
                    t,
                    "shape-skills-off",
                    if showing {
                        "Hide those it does not see".to_string()
                    } else {
                        format!("{off} it does not see")
                    },
                )
                .on_click(cx.listener(|s, _, _, cx| {
                    s.shape.show_off_skills = !s.shape.show_off_skills;
                    cx.notify();
                })),
            );
        }
        let mut section = div().flex().flex_col().child(head);
        for (n, s) in shape.skills.iter().enumerate() {
            if s.load == Load::Off && !self.shape.show_off_skills {
                continue;
            }
            let row = self
                .part(
                    t,
                    SharedString::from(format!("shape-skill-{n}")),
                    Pick::File(s.path.clone()),
                    cx,
                )
                .when(s.load == Load::Off, |d| d.opacity(tokens::DIMMED))
                // A description that wraps keeps the name on its first line.
                .items_start()
                .child(
                    div()
                        .flex_none()
                        .w(px(tokens::SKILL_W))
                        .py(px(size::LABEL_BOTTOM))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(s.name.clone()),
                )
                // The whole description, wrapped: it is how the agent
                // decides to use the skill.
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .py(px(size::LABEL_BOTTOM))
                        .whitespace_normal()
                        .text_color(if s.load == Load::Off {
                            t.needs
                        } else {
                            t.text2
                        })
                        .child(s.why.clone().unwrap_or_else(|| s.description.clone())),
                )
                .child(div().py(px(size::LABEL_BOTTOM)).child(pane::meta(
                    t,
                    s.origin.word(),
                    None,
                )));
            section = section.child(row);
        }
        if shape.skills.is_empty() {
            section = section.child(kit::note(t, "No skills reach this folder.", None));
        }
        section
    }

    fn setting_section(&self, t: &Theme, shape: &Shape, cx: &mut Context<Self>) -> Div {
        let mut head = div()
            .flex()
            .items_center()
            .justify_between()
            .pb(px(size::LABEL_BOTTOM))
            .child(kit::group_label(t, "Settings"));
        if let Some(file) = setting_files(shape).first().cloned() {
            head = head.child(
                pane::act(t, "shape-setting-add", "Add")
                    .debug_selector(|| "shape-setting-add".into())
                    .on_click(cx.listener(move |s, _, window, cx| {
                        s.shape_pick(Pick::NewSetting(file.clone()), window, cx)
                    })),
            );
        }
        let mut section = div().flex().flex_col().child(head);
        for (n, s) in shape.settings.iter().enumerate() {
            let row = self
                .part(
                    t,
                    SharedString::from(format!("shape-setting-{n}")),
                    Pick::Setting(s.key.clone(), s.file.clone()),
                    cx,
                )
                .when(s.overridden, |d| d.opacity(tokens::DIMMED))
                .child(
                    div()
                        .flex_none()
                        .max_w(px(tokens::KEY_W))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_color(t.text2)
                        .child(s.key.clone()),
                )
                .child(pane::cell(sh::value_words(&s.value)))
                .child(pane::meta(
                    t,
                    if s.overridden {
                        format!("{}, overridden", s.origin.word())
                    } else {
                        s.origin.word().to_owned()
                    },
                    None,
                ));
            section = section.child(row);
        }
        if shape.settings.is_empty() {
            section = section.child(kit::note(t, "Nothing set: the harness's defaults.", None));
        }
        section
    }

    /// The reader beside the anatomy: the part picked, read or edited.
    /// `alone`: it stands in place of the anatomy, and offers the way back.
    fn reader_pane(
        &mut self,
        t: &Theme,
        shape: &Shape,
        alone: bool,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let v = &self.shape;
        let bar = |cx: &mut Context<Self>| {
            div()
                .flex_none()
                .h(px(size::TAB_BAR))
                .px(px(size::PAD))
                .flex()
                .items_center()
                .gap(px(size::HEAD_GAP))
                .border_b_1()
                .border_color(t.divider)
                .when(alone, |d| {
                    d.child(
                        pane::act(t, "shape-back", "‹ Back")
                            .debug_selector(|| "shape-back".into())
                            .tooltip(kit::tip("Back to everything the agent gets"))
                            .on_click(cx.listener(|s, _, _, cx| {
                                s.shape.reading = false;
                                cx.notify();
                            })),
                    )
                })
        };
        let short = |p: &str| sh::short(p, shape.root.as_deref(), &shape.home);
        match v.pick.clone() {
            Pick::None => div().size_full().child(pane::vacant(
                t,
                "Open an instruction, skill, hook or setting to read or change it here.",
                None,
            )),
            Pick::File(path) => {
                let opened = format!("shape-open-{path}");
                let mut head = bar(cx)
                    .debug_selector(move || opened)
                    .child(pane::title(t, short(&path)));
                let Some(file) = &v.file else {
                    return div()
                        .flex()
                        .flex_col()
                        .size_full()
                        .child(head)
                        .child(pane::empty(t, "Reading…", None));
                };
                let target = file.target.clone().filter(|x| x != &file.path);
                if v.editing {
                    head = head
                        .child(
                            kit::button(
                                t,
                                "shape-save",
                                if v.saving { "Saving…" } else { "Save" },
                                Kind::Primary,
                            )
                            .debug_selector(|| "shape-save".into())
                            .on_click(cx.listener(|s, _, _, cx| s.file_save(cx))),
                        )
                        .child(
                            pane::act(t, "shape-discard", "Discard").on_click(
                                cx.listener(|s, _, window, cx| s.file_discard(window, cx)),
                            ),
                        );
                } else {
                    head = head.child(
                        pane::act(t, "shape-edit", "Edit")
                            .debug_selector(|| "shape-edit".into())
                            .tooltip(kit::tip("Change it here; Ctrl+S saves"))
                            .on_click(cx.listener(|s, _, window, cx| s.file_edit(window, cx))),
                    );
                }
                let mut out = div().flex().flex_col().size_full().child(head);
                if let Some(target) = target {
                    out = out.child(div().px(px(size::PAD)).pt(px(size::HEAD_GAP)).child(
                        kit::note(
                            t,
                            format!("A link to {}; saving changes that file.", short(&target)),
                            None,
                        ),
                    ));
                }
                if v.editing || !path.ends_with(".md") {
                    return out.child(div().flex_1().min_h_0().child(self.inputs.reader.clone()));
                }
                let look = markdown::Look {
                    t,
                    refs: false,
                    on_link: None,
                    select: None,
                };
                let blocks = markdown::parse(sh::without_front_matter(&file.text));
                out.child(
                    div()
                        .id("shape-read")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .p(px(size::PAD))
                        .font_family(t.ty.reading)
                        .text_size(px(t.ty.read))
                        .child(
                            markdown::render(&look, "shape-prose", &blocks)
                                .max_w(px(size::READING)),
                        ),
                )
            }
            Pick::Hook(file, _) | Pick::NewHook(file) => {
                let new = matches!(v.pick, Pick::NewHook(_));
                let hook = v.hook().cloned();
                if !new && hook.is_none() {
                    return div().size_full().child(pane::vacant(
                        t,
                        "That hook is gone: it changed on disk.",
                        None,
                    ));
                }
                let mut head = bar(cx).child(pane::title(t, if new { "New hook" } else { "Hook" }));
                if let (Some(h), false) = (&hook, new) {
                    let (file, at, was) = (h.file.clone(), h.at.clone(), h.run.clone());
                    head = head.child(
                        pane::act(t, "shape-hook-remove", "Remove")
                            .debug_selector(|| "shape-hook-remove".into())
                            .on_click(cx.listener(move |s, _, _, cx| {
                                s.hook_send(
                                    sh::HookChange::Remove {
                                        at: at.clone(),
                                        was: was.clone(),
                                    },
                                    file.clone(),
                                    cx,
                                )
                            })),
                    );
                }
                head = head.child(
                    kit::button(t, "shape-hook-save", "Save", Kind::Primary)
                        .debug_selector(|| "shape-hook-save".into())
                        .on_click(cx.listener(|s, _, _, cx| s.hook_save(cx))),
                );
                let i = &self.inputs;
                let mut body = div()
                    .p(px(size::PAD))
                    .flex()
                    .flex_col()
                    .gap(px(size::HEAD_GAP));
                if new {
                    body =
                        body.child(self.file_choice(t, &hook_files(shape), &file, shape, true, cx));
                } else {
                    body = body.child(pane::line(t, "In", div().child(short(&file))));
                }
                let events = sh::events(&shape.harness).join(", ");
                for (label, field, hint) in [
                    ("When", &i.event, events),
                    (
                        "Matching",
                        &i.matcher,
                        "Empty matches every one".to_string(),
                    ),
                    ("Run", &i.command, String::new()),
                    ("Timeout", &i.timeout, String::new()),
                ] {
                    let selector = format!("shape-hook-field-{label}");
                    body = body.child(pane::line(
                        t,
                        label,
                        div()
                            .flex()
                            .flex_col()
                            .child(div().debug_selector(move || selector).child(field.clone()))
                            .when(!hint.is_empty(), |d| {
                                d.child(kit::note(t, hint, Some(t.dim)))
                            }),
                    ));
                }
                if let Some(n) = hook.as_ref().and_then(|h| h.note.clone()) {
                    body = body.child(kit::note(t, n, Some(t.needs)));
                }
                if shape.harness == "claude" {
                    body = body.child(kit::note(
                        t,
                        "Claude Code has no switch of its own: a hook switched off is kept by Ahjo and put back when switched on.",
                        Some(t.dim),
                    ));
                } else {
                    body = body.child(kit::note(
                        t,
                        "Codex runs a new or changed hook only once you trust it in its /hooks.",
                        Some(t.dim),
                    ));
                }
                div().flex().flex_col().size_full().child(head).child(body)
            }
            Pick::Setting(_, file) | Pick::NewSetting(file) => {
                let new = matches!(v.pick, Pick::NewSetting(_));
                let current = v.setting().cloned();
                let mut head =
                    bar(cx).child(pane::title(t, if new { "New setting" } else { "Setting" }));
                if let (Some(s), false) = (&current, new) {
                    let (file, key) = (s.file.clone(), s.key.clone());
                    head = head.child(
                        pane::act(t, "shape-setting-unset", "Take out")
                            .debug_selector(|| "shape-setting-unset".into())
                            .on_click(cx.listener(move |s, _, _, cx| {
                                s.setting_send(file.clone(), key.clone(), Value::Null, cx)
                            })),
                    );
                }
                let target = file.clone();
                head = head.child(
                    kit::button(t, "shape-setting-save", "Save", Kind::Primary)
                        .debug_selector(|| "shape-setting-save".into())
                        .on_click(cx.listener(move |s, _, _, cx| {
                            let file = match &s.shape.pick {
                                Pick::NewSetting(f) => f.clone(),
                                _ => target.clone(),
                            };
                            let key = s.inputs.key.read(cx).text.trim().to_owned();
                            let value = typed(s.inputs.value.read(cx).text.trim());
                            s.setting_send(file, key, value, cx)
                        })),
                );
                let i = &self.inputs;
                let mut body = div()
                    .p(px(size::PAD))
                    .flex()
                    .flex_col()
                    .gap(px(size::HEAD_GAP));
                if new {
                    body = body.child(self.file_choice(
                        t,
                        &setting_files(shape),
                        &file,
                        shape,
                        false,
                        cx,
                    ));
                } else {
                    body = body.child(pane::line(t, "In", div().child(short(&file))));
                }
                for (label, field) in [("Key", &i.key), ("Value", &i.value)] {
                    let selector = format!("shape-setting-field-{label}");
                    body = body.child(pane::line(
                        t,
                        label,
                        div().debug_selector(move || selector).child(field.clone()),
                    ));
                }
                body = body.child(kit::note(
                    t,
                    "A dotted key reaches inside: env.NAME. A value reads as JSON when it can (true, 3, [\"a\"]), otherwise as words.",
                    Some(t.dim),
                ));
                div().flex().flex_col().size_full().child(head).child(body)
            }
        }
    }

    /// Which file a new hook or setting goes in.
    fn file_choice(
        &self,
        t: &Theme,
        files: &[String],
        chosen: &str,
        shape: &Shape,
        hook: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut choices = div().flex().flex_wrap().gap(px(tokens::P4));
        for (n, f) in files.iter().enumerate() {
            let on = f == chosen;
            let file = f.clone();
            choices = choices.child(
                kit::button(
                    t,
                    SharedString::from(format!("shape-into-{n}")),
                    sh::short(f, shape.root.as_deref(), &shape.home),
                    Kind::Quiet,
                )
                .when(on, |b| b.text_color(t.accent).border_color(t.divider))
                .on_click(cx.listener(move |s, _, _, cx| {
                    s.shape.pick = if hook {
                        Pick::NewHook(file.clone())
                    } else {
                        Pick::NewSetting(file.clone())
                    };
                    cx.notify();
                })),
            );
        }
        pane::line(t, "Into", choices)
    }
}
