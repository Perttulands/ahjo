//! The Settings page: the shell's own settings (the node, the shortcuts),
//! then the settings each registered page declares ([`Page::SETTINGS`]),
//! one group each in the list, and the connected node's configuration.
//! Settings is not an area: what is listed comes from the pages, and a
//! change goes into the declaring page's section, which then hears
//! [`Page::settings_changed`]. The node's configuration (`/api/config`) is
//! read each time the page comes forward or the node's link opens while it
//! shows, and shown as named settings; it changes by editing its file.
//! Typing in the search shows every setting that matches, from every group.

use crate::theme_picker::{self, Key, Picker};
use ahjo_app_kit::ask::{Answers, Ask};
use ahjo_app_kit::host::{Request, Status};
use ahjo_app_kit::link::client::LinkState;
use ahjo_app_kit::page::Meta;
use ahjo_app_kit::settings::DEFAULT_NODE;
use ahjo_app_kit::theme::{Direction, Theme, size, theme};
use ahjo_app_kit::widgets::field::{FieldEvent, TextField};
use ahjo_app_kit::widgets::kit::{self, Kind};
use ahjo_app_kit::widgets::pane;
use ahjo_app_kit::{Control, Host, Hub, Page, Setting};
use ahjo_core::NodeId;
use gpui::{
    AnyElement, App, AppContext, Context, Div, Entity, FocusHandle, KeyDownEvent, SharedString,
    Subscription, Window, div, prelude::*, px,
};
use serde_json::Value;
use std::collections::HashMap;

/// The page's id, which no other page may take.
pub const ID: &str = "settings";

/// The shell's own settings, kept outside any page's section.
const APP: &[Setting] = &[
    Setting::choice("theme", "Theme", "night", Direction::CHOICES)
        .note("Odin's ravens three ways, or a colour theme from your editor."),
    Setting::text("node", "Node", DEFAULT_NODE)
        .note("The node this app talks to, as http://host:port. Enter connects."),
    Setting::toggle("keys", "Shortcuts", true)
        .note("Ahjo Swarm's shortcuts take their keys before the terminal does."),
    Setting::toggle("show_keys", "Show shortcut keys", false)
        .note("Each shortcut's keys show at the foot of the window for a moment."),
];

/// Whose a setting is: the shell's, or a page's.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Owner {
    App,
    Page(&'static str),
}

impl Owner {
    fn name(self) -> &'static str {
        match self {
            Owner::App => "app",
            Owner::Page(id) => id,
        }
    }
}

/// A group in the list: the shell's own, a page's, or the node's.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Group {
    App,
    Page(&'static str),
    Node,
}

pub struct SettingsPage {
    hub: Entity<Hub>,
    host: Host,
    shown: bool,
    chosen: Group,
    search: Entity<TextField>,
    /// The connected node's config as it last answered, or why it did not.
    node_config: Option<Result<Value, String>>,
    /// The pages that declare settings, in the app's order.
    pages: Vec<Meta>,
    /// A field for each text setting.
    fields: HashMap<(Owner, &'static str), Entity<TextField>>,
    _fields: Vec<Subscription>,
    /// The theme picker, while it is open, and what has the keyboard then.
    picker: Option<Picker>,
    picker_focus: FocusHandle,
}

impl Page for SettingsPage {
    const ID: &'static str = ID;
    const TITLE: &'static str = "Settings";
    const KEY: Option<char> = Some(',');
    const ANSWERS: &'static [Answers] = &[Answers::Reveal("setting")];

    /// The fields show what is kept now, and the node's config is read.
    fn shown(&mut self, shown: bool, _: &mut Window, cx: &mut Context<Self>) {
        self.shown = shown;
        if !shown {
            self.picker_back(cx);
        }
        if shown {
            self.refresh(cx);
            self.read_node_config(cx);
        }
    }

    fn link(
        &mut self,
        _: &NodeId,
        state: Option<LinkState>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.shown && state == Some(LinkState::Open) {
            self.read_node_config(cx);
        }
    }

    /// The settings were read again from the file.
    fn settings_changed(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.refresh(cx);
        cx.notify();
    }

    /// Shows a setting by its key: its group, and for the theme, the
    /// picker open.
    fn answer(&mut self, ask: Ask, _: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        let Ask::Reveal { id, .. } = ask else {
            return;
        };
        self.search.update(cx, |f, cx| f.set("", false, cx));
        if APP.iter().any(|s| s.key == id) {
            self.chosen = Group::App;
        } else if let Some(m) = self
            .pages
            .iter()
            .find(|m| m.settings.iter().any(|s| s.key == id))
        {
            self.chosen = Group::Page(m.id);
        }
        if id == "theme" {
            self.picker_open(window, cx);
        }
        cx.notify();
    }

    fn focus(&self, _: &App) -> Option<FocusHandle> {
        self.picker.as_ref().map(|_| self.picker_focus.clone())
    }
}

impl SettingsPage {
    pub fn new(
        hub: Entity<Hub>,
        host: Host,
        pages: Vec<Meta>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let pages: Vec<Meta> = pages
            .into_iter()
            .filter(|p| !p.settings.is_empty())
            .collect();
        let mut fields = HashMap::new();
        let mut subscriptions = vec![];
        let declared = APP.iter().map(|s| (Owner::App, s)).chain(
            pages
                .iter()
                .flat_map(|p| p.settings.iter().map(|s| (Owner::Page(p.id), s))),
        );
        for (owner, setting) in declared {
            let Control::Text { default } = setting.control else {
                continue;
            };
            let field = cx.new(|cx| TextField::new(cx, default));
            let key = setting.key;
            subscriptions.push(cx.subscribe_in(
                &field,
                window,
                move |page: &mut Self, field, ev: &FieldEvent, _, cx| {
                    if let FieldEvent::Submit = ev {
                        let text = field.read(cx).text.trim().to_string();
                        page.write(owner, key, Value::from(text), cx);
                    }
                },
            ));
            fields.insert((owner, key), field);
        }
        let search = cx.new(|cx| TextField::new(cx, "Search settings"));
        subscriptions.push(cx.subscribe(&search, |_, _, ev: &FieldEvent, cx| {
            if let FieldEvent::Changed = ev {
                cx.notify();
            }
        }));
        Self {
            hub,
            host,
            shown: false,
            chosen: Group::App,
            search,
            node_config: None,
            pages,
            fields,
            _fields: subscriptions,
            picker: None,
            picker_focus: cx.focus_handle(),
        }
    }

    fn picker_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = ahjo_app_kit::theme::theme(cx).direction;
        self.picker = Some(Picker::open(now));
        window.focus(&self.picker_focus, cx);
        cx.notify();
    }

    /// The whole app drawn in `d` while the picker tries it; nothing kept.
    fn picker_try(&mut self, d: Direction, cx: &mut Context<Self>) {
        if theme(cx).direction != d {
            cx.set_global(Theme::new(d));
            cx.refresh_windows();
        }
        cx.notify();
    }

    /// Keeps `d` and closes the picker.
    pub(crate) fn picker_keep(&mut self, d: Direction, cx: &mut Context<Self>) {
        self.picker = None;
        self.write(Owner::App, "theme", Value::from(d.key()), cx);
    }

    /// Closes the picker, back in the theme there was.
    pub(crate) fn picker_back(&mut self, cx: &mut Context<Self>) {
        if let Some(p) = self.picker.take() {
            self.picker_try(p.was, cx);
        }
    }

    pub(crate) fn picker_key(&mut self, e: &KeyDownEvent, cx: &mut Context<Self>) {
        let Some(picker) = self.picker.as_mut() else {
            return;
        };
        match picker.key(&e.keystroke.key) {
            Key::Try(d) => self.picker_try(d, cx),
            Key::Keep(d) => self.picker_keep(d, cx),
            Key::Back(_) => self.picker_back(cx),
            Key::Nothing => return,
        }
        cx.stop_propagation();
    }

    /// The theme's control: the theme drawn now, and the picker under it
    /// while it is open.
    fn theme_control(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let button = theme_picker::button(t, "setting-app-theme").on_click(cx.listener(
            |p, _, window, cx| {
                if p.picker.is_none() {
                    p.picker_open(window, cx);
                }
            },
        ));
        let list = self.picker.as_ref().map(|picker| {
            theme_picker::list(t, picker, &self.picker_focus, cx.entity().downgrade())
        });
        div()
            .flex()
            .flex_col()
            .child(button)
            // gpui places an anchored element absolutely, at its parent's
            // corner: this empty holder puts that corner under the button.
            .child(div().children(list))
            .into_any_element()
    }

    /// Asks the connected node for its config.
    fn read_node_config(&mut self, cx: &mut Context<Self>) {
        let asked = self
            .host
            .request(Request::get(NodeId::default(), "/api/config"), cx);
        cx.spawn(async move |this, cx| {
            let got = match asked.await {
                Ok(a) if a.ok() => serde_json::from_slice(&a.body).map_err(|e| e.to_string()),
                Ok(a) => Err(a.error_text()),
                Err(e) => Err(e.message),
            };
            let _ = this.update(cx, |p, cx| {
                p.node_config = Some(got);
                cx.notify();
            });
        })
        .detach();
    }

    /// The connected node's configuration, a part for each section it
    /// loaded, each key a named line; `matches` keeps the lines a search
    /// asks for. Then where it lives, and how it changes.
    fn node_parts(
        &self,
        t: &Theme,
        within: Option<&str>,
        matches: &dyn Fn(&str) -> bool,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let named = |part: &str| match within {
            Some(group) => format!("{group} · {part}"),
            None => part.to_string(),
        };
        let mut parts = vec![];
        let config = match &self.node_config {
            None => {
                return vec![
                    pane::empty(t, "Reading the node's configuration…", None).into_any_element(),
                ];
            }
            Some(Err(why)) => {
                return vec![
                    pane::empty(
                        t,
                        format!("The node's configuration did not read: {why}"),
                        None,
                    )
                    .into_any_element(),
                ];
            }
            Some(Ok(config)) => config,
        };
        for (name, keys) in config["sections"].as_object().into_iter().flatten() {
            let mut part = pane::section(t, named(&words(name, true)));
            let mut any = false;
            for (key, value) in keys.as_object().into_iter().flatten() {
                let label = config_label(name, key);
                if !matches(&label) && !matches(name) {
                    continue;
                }
                any = true;
                let items = plain(value);
                let lines = if items.len() >= COMPACT_FROM {
                    compact_list(t, name, key, &items)
                } else {
                    let mut lines = div().flex().flex_col();
                    for v in items {
                        // Each named for tests by where it is and what it says.
                        let selector = format!("node-config:{name}:{key}:{v}");
                        lines = lines.child(
                            div()
                                .debug_selector(move || selector)
                                .w_full()
                                .min_w_0()
                                .whitespace_normal()
                                .text_color(t.text)
                                .child(v),
                        );
                    }
                    lines
                };
                part = part.child(pane::line(t, label, lines).items_start());
            }
            if any {
                parts.push(part.into_any_element());
            }
        }
        let path = config["path"].as_str().unwrap_or_default().to_string();
        if matches("file") || matches(&path) {
            let selector = format!("node-config:path:{path}");
            let open = path.clone();
            parts.push(
                pane::section(t, named("File"))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(size::HEAD_GAP))
                            .child(
                                div()
                                    .debug_selector(move || selector)
                                    .flex_1()
                                    .min_w_0()
                                    .text_color(t.text)
                                    .child(path),
                            )
                            .child(
                                pane::act(t, "node-config-open", "Open")
                                    .tooltip(kit::tip("Edit it in Files"))
                                    .on_click(cx.listener(move |p, _, _, cx| {
                                        let ask = Ask::OpenPath {
                                            node: NodeId::default(),
                                            path: open.clone(),
                                            beside: false,
                                        };
                                        if let Err(e) = p.host.ask(ask, cx) {
                                            p.host.report(Status::failure(e.message), cx);
                                        }
                                    })),
                            ),
                    )
                    .child(kit::note(
                        t,
                        "A change applies when the node restarts; sessions stay.",
                        Some(t.dim),
                    ))
                    .into_any_element(),
            );
        }
        parts
    }

    /// Each field shows what is kept now.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        for (&(owner, key), field) in &self.fields {
            let kept = self.value(owner, key, cx);
            let text = kept.as_str().unwrap_or_default().to_string();
            field.update(cx, |f, cx| f.set(&text, false, cx));
        }
    }

    /// What is kept for `key` now, or its default.
    fn value(&self, owner: Owner, key: &str, cx: &App) -> Value {
        let hub = self.hub.read(cx);
        match owner {
            Owner::App => match key {
                "node" => Value::from(hub.settings.node.clone()),
                "show_keys" => Value::from(hub.settings.show_keys),
                "theme" => Value::from(hub.settings.theme.clone()),
                _ => Value::from(hub.settings.keys),
            },
            Owner::Page(id) => {
                let declared = self.declared(owner, key);
                declared.read(hub.settings.value(id, key))
            }
        }
    }

    fn declared(&self, owner: Owner, key: &str) -> &'static Setting {
        let list: &'static [Setting] = match owner {
            Owner::App => APP,
            Owner::Page(id) => {
                self.pages
                    .iter()
                    .find(|p| p.id == id)
                    .expect("a listed page")
                    .settings
            }
        };
        list.iter().find(|s| s.key == key).expect("a declared key")
    }

    /// Keeps a new value: the shell's own take effect here, a page's go
    /// into its section and the page hears it.
    fn write(&mut self, owner: Owner, key: &'static str, value: Value, cx: &mut Context<Self>) {
        self.hub.update(cx, |h, cx| match owner {
            Owner::App => {
                if key == "node" {
                    let node = value.as_str().unwrap_or_default();
                    h.settings.node = if node.is_empty() {
                        DEFAULT_NODE.into()
                    } else {
                        node.into()
                    };
                    h.connect(cx);
                } else if key == "theme" {
                    h.settings.theme = value.as_str().unwrap_or_default().into();
                    h.apply_theme(cx);
                } else if key == "show_keys" {
                    h.settings.show_keys = value.as_bool().unwrap_or(false);
                } else {
                    h.settings.keys = value.as_bool().unwrap_or(true);
                }
                h.save(cx);
                cx.notify();
            }
            Owner::Page(id) => h.set_value(id, key, value, cx),
        });
        if owner == Owner::App && key == "node" {
            let node = self.hub.read(cx).settings.node.clone();
            self.fields[&(owner, key)].update(cx, |f, cx| f.set(&node, false, cx));
        }
        cx.notify();
    }

    fn control(
        &self,
        owner: Owner,
        setting: &Setting,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = setting.key;
        if owner == Owner::App && key == "theme" {
            return self.theme_control(t, cx);
        }
        let id = format!("setting-{}-{key}", owner.name());
        let value = self.value(owner, key, cx);
        match setting.control {
            Control::Toggle { .. } => {
                let on = value.as_bool().unwrap_or(false);
                let selector = id.clone();
                kit::toggle(t, SharedString::from(id), on)
                    .debug_selector(move || selector)
                    .on_click(
                        cx.listener(move |p, _, _, cx| p.write(owner, key, Value::from(!on), cx)),
                    )
                    .into_any_element()
            }
            Control::Number { .. } => {
                let declared = *setting;
                let step = |steps: f64, text: &'static str, cx: &mut Context<Self>| {
                    let name = format!("{id}-{}", if steps < 0. { "down" } else { "up" });
                    let selector = name.clone();
                    let now = value.clone();
                    kit::button(t, SharedString::from(name), text, Kind::Plain)
                        .debug_selector(move || selector)
                        .on_click(cx.listener(move |p, _, _, cx| {
                            p.write(owner, key, declared.stepped(&now, steps), cx)
                        }))
                };
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(step(-1., "−", cx))
                    .child(
                        div()
                            .min_w(px(size::CONTROL * 2.))
                            .flex()
                            .justify_center()
                            .text_color(t.text)
                            .child(match value.as_f64() {
                                Some(n) if n.fract() == 0. => format!("{n:.0}"),
                                _ => value.to_string(),
                            }),
                    )
                    .child(step(1., "+", cx))
                    .into_any_element()
            }
            Control::Choice { options, .. } => {
                let now = value.as_str().unwrap_or_default().to_string();
                let mut row = div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(size::HEAD_GAP));
                for &(option, words) in options {
                    let name = format!("{id}-{option}");
                    let selector = name.clone();
                    let kind = if option == now {
                        Kind::Primary
                    } else {
                        Kind::Plain
                    };
                    row = row.child(
                        kit::button(t, SharedString::from(name), words, kind)
                            .debug_selector(move || selector)
                            .on_click(cx.listener(move |p, _, _, cx| {
                                p.write(owner, key, Value::from(option), cx)
                            })),
                    );
                }
                row.into_any_element()
            }
            Control::Text { .. } => {
                let field = self.fields[&(owner, key)].clone();
                let selector = format!("{id}:{}", field.read(cx).text);
                div()
                    .debug_selector(move || selector)
                    .w_full()
                    .min_w_0()
                    .child(field)
                    .into_any_element()
            }
        }
    }

    /// The lines of `settings` that `matches` keeps.
    fn lines(
        &self,
        owner: Owner,
        settings: &[Setting],
        matches: &dyn Fn(&str) -> bool,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut out = vec![];
        for setting in settings {
            if !matches(setting.label) && !matches(setting.note) {
                continue;
            }
            let value = div()
                .flex()
                .flex_col()
                .items_start()
                .gap(px(size::RADIUS))
                .child(self.control(owner, setting, t, cx))
                .when(!setting.note.is_empty(), |d| {
                    d.child(kit::note(t, setting.note, Some(t.dim)))
                });
            out.push(
                pane::line(t, setting.label, value)
                    .items_start()
                    .into_any_element(),
            );
        }
        out
    }

    /// The groups in the list's order, each with its name.
    fn groups(&self) -> Vec<(Group, String)> {
        let mut all = vec![(Group::App, "App".to_string())];
        all.extend(
            self.pages
                .iter()
                .map(|m| (Group::Page(m.id), m.title.to_string())),
        );
        all.push((Group::Node, "Node".to_string()));
        all
    }

    /// The parts of one group, or of every group a search matches.
    /// `within` when they stand under the group's own name, as a search
    /// shows them.
    fn parts(
        &self,
        group: Group,
        within: bool,
        matches: &dyn Fn(&str) -> bool,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        match group {
            Group::App => {
                let mut lines = self.lines(Owner::App, APP, matches, t, cx);
                let hub = self.hub.read(cx);
                if let Some(h) = &hub.fleet.here().hello
                    && matches("version")
                {
                    let words = format!("Ahjo {} on {} · {}", h.version, h.device, h.environment);
                    lines.push(pane::line(t, "Version", words).into_any_element());
                }
                lines
            }
            Group::Page(id) => {
                let settings = self
                    .pages
                    .iter()
                    .find(|m| m.id == id)
                    .map(|m| m.settings)
                    .unwrap_or_default();
                self.lines(Owner::Page(id), settings, matches, t, cx)
            }
            Group::Node => self.node_parts(t, within.then_some("Node"), matches, cx),
        }
    }
}

/// What a node's config key is called here: the few every node has by
/// their meaning, any other as its words.
fn config_label(section: &str, key: &str) -> String {
    match (section, key) {
        ("node", "listen") => "Address".into(),
        ("terminals", "socket") => "tmux socket".into(),
        ("files", "roots") => "Folders".into(),
        (_, "enabled") => "On".into(),
        _ => words(key, true),
    }
}

/// A config key or section as words: `copy_on_select` reads "Copy on
/// select"; `first` capitalises it.
fn words(key: &str, first: bool) -> String {
    let spaced = key.replace(['_', '-'], " ");
    let mut c = spaced.chars();
    match c.next() {
        Some(f) if first => f.to_uppercase().chain(c).collect(),
        _ => spaced,
    }
}

/// A config value as plain lines: a string as written, a list one item a
/// line, on and off for a switch.
/// A list this long reads as a compact one: its kinds said once, its
/// folders shown from where they share.
const COMPACT_FROM: usize = 5;

/// A long list's items in groups: the leading word they share (`beads`),
/// the folder they all sit in, if any, and each item as it reads there.
#[derive(Debug, PartialEq)]
struct Bunch {
    kind: String,
    folder: Option<String>,
    /// Each item whole, and as it reads under its kind and folder.
    items: Vec<(String, String)>,
}

fn bunches(items: &[String]) -> Vec<Bunch> {
    let mut out: Vec<(String, Vec<(String, String)>)> = vec![];
    for item in items {
        let (kind, rest) = item.split_once(' ').unwrap_or(("", item));
        let rest = (item.clone(), rest.trim().to_string());
        match out.iter_mut().find(|(k, _)| k == kind) {
            Some((_, list)) => list.push(rest),
            None => out.push((kind.to_string(), vec![rest])),
        }
    }
    out.into_iter()
        .map(|(kind, items)| {
            let folder = common_folder(items.iter().map(|(_, r)| r.as_str()));
            let items = match &folder {
                Some(f) => items
                    .into_iter()
                    .map(|(whole, rest)| {
                        let under = rest
                            .strip_prefix(f.as_str())
                            .and_then(|r| r.strip_prefix('/'))
                            .filter(|r| !r.is_empty())
                            .map_or(rest.clone(), str::to_string);
                        (whole, under)
                    })
                    .collect(),
                None => items,
            };
            Bunch {
                kind,
                folder,
                items,
            }
        })
        .collect()
}

/// The deepest folder every path sits in or is, when there are several
/// paths and it is not the root.
fn common_folder<'a>(paths: impl Iterator<Item = &'a str>) -> Option<String> {
    let paths: Vec<&str> = paths.collect();
    if paths.len() < 2 || !paths.iter().all(|p| p.starts_with('/')) {
        return None;
    }
    let parts = |p: &'a str| p.split('/').filter(|s| !s.is_empty()).collect::<Vec<_>>();
    let mut common = parts(paths[0]);
    for p in &paths[1..] {
        let other = parts(p);
        let same = common
            .iter()
            .zip(&other)
            .take_while(|(a, b)| a == b)
            .count();
        common.truncate(same);
    }
    // A path that is the folder itself stays whole; the folder is the one
    // the others sit under.
    if paths.iter().all(|p| parts(p).len() == common.len()) {
        common.pop();
    }
    (!common.is_empty()).then(|| format!("/{}", common.join("/")))
}

/// A long list as its kinds, each with its folder and then its items in a
/// wrapping flow; a folder that is itself listed is said in the head. Each
/// item is named for tests by its whole value.
fn compact_list(t: &Theme, name: &str, key: &str, items: &[String]) -> Div {
    let mut out = div().flex().flex_col().gap(px(size::LABEL_BOTTOM));
    for b in bunches(items) {
        let itself = b.folder.as_ref().and_then(|f| {
            b.items
                .iter()
                .find(|(_, shown)| shown == f)
                .map(|(whole, _)| whole.clone())
        });
        let others: Vec<_> = b
            .items
            .iter()
            .filter(|(whole, _)| Some(whole) != itself.as_ref())
            .collect();
        let count = others.len();
        let mut words = vec![];
        if !b.kind.is_empty() {
            words.push(b.kind.clone());
        }
        words.push(match (&b.folder, &itself) {
            (Some(f), Some(_)) => format!("{f} and {count} in it"),
            (Some(f), None) => format!("{count} in {f}"),
            (None, _) => format!("{count}"),
        });
        let mut head = div().text_color(t.dim).child(words.join(" · "));
        if let Some(whole) = itself {
            let selector = format!("node-config:{name}:{key}:{whole}");
            head = head.debug_selector(move || selector);
        }
        let mut flow = div()
            .flex()
            .flex_wrap()
            .gap_x(px(size::HEAD_GAP * 2.))
            .text_color(t.text);
        for (whole, shown) in others {
            let selector = format!("node-config:{name}:{key}:{whole}");
            flow = flow.child(div().debug_selector(move || selector).child(shown.clone()));
        }
        out = out.child(div().flex().flex_col().child(head).child(flow));
    }
    out
}

fn plain(value: &Value) -> Vec<String> {
    match value {
        Value::String(s) => vec![s.clone()],
        Value::Bool(true) => vec!["on".into()],
        Value::Bool(false) => vec!["off".into()],
        Value::Array(items) if items.is_empty() => vec!["none".into()],
        Value::Array(items) => items.iter().flat_map(plain).collect(),
        Value::Object(map) => map
            .iter()
            .map(|(k, v)| format!("{}: {}", words(k, false), plain(v).join(", ")))
            .collect(),
        other => vec![other.to_string()],
    }
}

impl Render for SettingsPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx).clone();
        let query = self.search.read(cx).text.trim().to_lowercase();
        let groups = self.groups();
        let mut rows = div().flex().flex_col();
        for (group, name) in &groups {
            let group = *group;
            let chosen = query.is_empty() && self.chosen == group;
            let id = match group {
                Group::App => "app".to_string(),
                Group::Page(p) => p.to_string(),
                Group::Node => "node".to_string(),
            };
            let selector = format!("settings-group-{id}");
            rows = rows.child(
                pane::row(&t, SharedString::from(selector.clone()), chosen)
                    .debug_selector(move || selector)
                    .child(pane::cell(name.clone()))
                    .on_click(cx.listener(move |p, _, window, cx| {
                        p.chosen = group;
                        p.search.update(cx, |f, cx| f.set("", false, cx));
                        let _ = window;
                        cx.notify();
                    })),
            );
        }
        let list = pane::list(
            &t,
            "settings-groups",
            pane::list_head(&t, Some(self.search.clone().into_any_element())),
            rows,
        );
        let matches = |words: &str| query.is_empty() || words.to_lowercase().contains(&query);
        let (title, body) = if query.is_empty() {
            let name = groups
                .iter()
                .find(|(g, _)| *g == self.chosen)
                .map(|(_, n)| n.clone())
                .unwrap_or_default();
            let title = match self.chosen {
                Group::Node => match &self.hub.read(cx).fleet.here().hello {
                    Some(h) => format!("Node on {} · {}", h.device, h.environment),
                    None => name,
                },
                _ => name,
            };
            (title, self.parts(self.chosen, false, &matches, &t, cx))
        } else {
            let mut all = vec![];
            for (group, name) in &groups {
                let parts = self.parts(*group, true, &matches, &t, cx);
                if parts.is_empty() {
                    continue;
                }
                // The node's parts carry their group's name themselves.
                if *group == Group::Node {
                    all.extend(parts);
                } else {
                    all.push(
                        pane::section(&t, name.clone())
                            .children(parts)
                            .into_any_element(),
                    );
                }
            }
            if all.is_empty() {
                all.push(pane::empty(&t, "No setting matches.", None).into_any_element());
            }
            (format!("Settings matching “{query}”"), all)
        };
        pane::page(
            &t,
            list,
            pane::detail(
                &t,
                "settings-page",
                pane::head(&t).child(pane::title(&t, title)),
                div()
                    .flex()
                    .flex_col()
                    .gap(px(size::HEAD_GAP))
                    .children(body),
            ),
        )
    }
}

#[cfg(test)]
mod compact_tests {
    use super::*;

    #[test]
    fn a_long_list_says_its_kind_once_and_its_folders_from_where_they_share() {
        let items: Vec<String> = [
            "beads /work",
            "beads /work/alpha",
            "beads /work/games/bravo",
            "github owner/repo",
            "beads /work/charlie",
        ]
        .map(String::from)
        .into();
        let b = bunches(&items);
        assert_eq!(
            (b[0].kind.as_str(), b[0].folder.as_deref()),
            ("beads", Some("/work"))
        );
        let shown: Vec<&str> = b[0].items.iter().map(|(_, s)| s.as_str()).collect();
        assert_eq!(shown, ["/work", "alpha", "games/bravo", "charlie"]);
        assert_eq!(b[0].items[1].0, "beads /work/alpha");
        assert_eq!(
            (b[1].kind.as_str(), b[1].folder.as_deref()),
            ("github", None)
        );
        assert_eq!(b[1].items[0].1, "owner/repo");
        // Siblings sit in their parent; nothing shared is no folder.
        assert_eq!(
            common_folder(["/a/b", "/a/c"].into_iter()).as_deref(),
            Some("/a")
        );
        assert_eq!(common_folder(["/a", "/b"].into_iter()), None);
    }
}
