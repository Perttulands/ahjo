//! The state the shell and every page share: the link to the nodes, the
//! settings, the status line and toast, and the one stack of layers. Pages
//! reach it through their [`Host`](crate::Host); the shell holds it and
//! hears its [`HubEvent`]s. The hub never calls a page, so a page may use
//! its Host from inside its own update.

use crate::ask::{Answers, Ask};
use crate::chords::Run;
use crate::clock::hhmm;
use crate::host::{Layer, Node, Status};
use crate::layers::{self, Stack};
use crate::link::client::{LinkState, Notice};
use crate::link::devices;
use crate::link::fleet::Fleet;
use crate::link::socket::{Event, Net};
use crate::popup::{Popup, PopupId};
use crate::settings::Settings;
use crate::theme::{Direction, Theme, motion};
use ahjo_core::{Error, NodeId};
use gpui::{
    AnyEntity, AnyView, AnyWeakEntity, App, AppContext, Context, Entity, EventEmitter,
    SystemNotification, Task,
};
use std::any::{TypeId, type_name};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime};

pub struct Toast {
    pub text: String,
    pub at: Instant,
}

/// What is on the dismissal stack: a popup (a menu among them), or a
/// layer over the page or the window.
#[derive(Clone)]
pub enum Surface {
    Popup(Popup),
    Layer(Layer),
}

impl Surface {
    /// Escape closes it when it is on top; a layer that keeps Escape for
    /// its content does not close on it.
    pub fn escape_closes(&self) -> bool {
        match self {
            Surface::Popup(_) => true,
            Surface::Layer(l) => !l.keeps_escape,
        }
    }

    /// The keyboard is in it.
    pub fn has_keyboard(&self, window: &gpui::Window, cx: &App) -> bool {
        match self {
            Surface::Popup(p) => p.has_keyboard(window, cx),
            Surface::Layer(l) => l.has_keyboard(window),
        }
    }

    /// What has the keyboard while it is on top.
    pub fn focus(&self) -> Option<&gpui::FocusHandle> {
        match self {
            Surface::Popup(p) => p.focus.as_ref(),
            Surface::Layer(l) => l.focus.as_ref(),
        }
    }

    pub fn dismiss(&self) -> Option<&Run> {
        match self {
            Surface::Popup(p) => p.dismiss.as_ref(),
            Surface::Layer(l) => l.dismiss.as_ref(),
        }
    }
}

impl layers::Layer for Surface {
    fn kind(&self) -> &str {
        match self {
            Surface::Popup(p) => &p.name,
            Surface::Layer(l) => &l.name,
        }
    }
    fn passing(&self) -> bool {
        matches!(self, Surface::Popup(p) if p.is_menu())
    }
}

/// The pages a page's section of the side panel shows on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SideOn {
    /// Every page, the lender's own too: it draws the section nowhere else.
    EveryPage,
    /// Every other page: on its own the lender shows the same thing itself,
    /// so the panel would only show it twice.
    OtherPages,
    /// Only the lender's own page: what it shows is about that page, such
    /// as the card a board has open.
    OwnPage,
}

impl SideOn {
    /// Whether what page `lender` lent stands while page `shown` shows.
    pub fn stands(self, lender: &str, shown: &str) -> bool {
        match self {
            SideOn::EveryPage => true,
            SideOn::OtherPages => lender != shown,
            SideOn::OwnPage => lender == shown,
        }
    }
}

/// Which edge of the window the panel stands at.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Edge {
    Left,
    #[default]
    Right,
}

impl Edge {
    /// The edge a setting names: `left`, else the right.
    pub fn named(name: &str) -> Edge {
        if name == "left" {
            Edge::Left
        } else {
            Edge::Right
        }
    }
}

/// What the shell hears from the hub, after the update that caused it.
pub enum HubEvent {
    /// A frame for the page whose kinds hold it.
    Frame {
        node: NodeId,
        kind: u8,
        body: Vec<u8>,
    },
    /// Page `to` answers this, which page `from` asked (`""`: the shell).
    Ask {
        to: &'static str,
        from: &'static str,
        ask: Ask,
    },
    /// Show this page.
    Show(&'static str),
    /// The stack changed: the keyboard goes to the top surface that takes
    /// it, or back to where it was, and what closed runs its dismiss.
    Layers { closed: Vec<Surface> },
    /// A setting page `page` declares was changed for it.
    SettingChanged(&'static str),
    /// `app.json` was edited outside the app and read again: these pages'
    /// sections changed.
    SettingsReloaded(Vec<&'static str>),
    /// What page `page` lent (its section of the side panel, or its
    /// content in the panel) opened, closed or gave way.
    Lent(&'static str),
    /// The link to `node` changed state; `None` when the app no longer
    /// follows that node. Every page hears it.
    Link {
        node: NodeId,
        state: Option<LinkState>,
    },
}

pub struct Hub {
    pub fleet: Fleet,
    pub settings: Settings,
    settings_path: Option<PathBuf>,
    save_task: Option<Task<()>>,
    /// The file's text as the app last wrote or read it: an event that
    /// finds it so is the app's own write.
    known: Option<String>,
    /// The app is writing the file: what is there until it is done is not
    /// an edit.
    writing: bool,
    /// Watches the settings' folder while the hub lives.
    watcher: Option<notify::RecommendedWatcher>,
    watch_task: Option<Task<()>>,
    pub status: Option<Status>,
    pub toast: Option<Toast>,
    toast_task: Option<Task<()>>,
    pub stack: Stack<Surface>,
    /// The number the next popup opened is known by.
    next_popup: u64,
    /// The asks each page answers, by page id.
    answers: Vec<(&'static str, &'static [Answers])>,
    /// Nodes whose link is open, as last tracked.
    heard: HashSet<String>,
    /// Devices said to be away.
    away: HashSet<String>,
    /// What a click on each desktop notification posted does, by its tag.
    clicks: HashMap<String, Run>,
    /// The read models pages share, by type.
    models: HashMap<TypeId, Model>,
    /// The sections of the side panel, by the page that lent each,
    /// with the pages each shows on.
    side: HashMap<&'static str, (AnyView, SideOn)>,
    /// The panel's contents at the window's edge: the one showing on top.
    pub panel: crate::panel::Stack,
    /// The page the window shows, as the shell last drew it.
    shown: Option<&'static str>,
    /// The views that may hold unsaved work, each with the page it is from.
    guards: Vec<Guard>,
}

/// What a guarded view holds unsaved now: `None` when all is saved or the
/// view is gone.
pub type Unsaved = Rc<dyn Fn(&App) -> Option<String>>;

/// A view that may hold unsaved work: while it lives, closing the window
/// asks it first.
struct Guard {
    page: &'static str,
    view: AnyWeakEntity,
    unsaved: Unsaved,
}

/// A read model: its entity, and the page that publishes it once one has.
struct Model {
    entity: AnyEntity,
    publisher: Option<&'static str>,
}

impl EventEmitter<HubEvent> for Hub {}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl Hub {
    /// The hub with the settings at `settings_path` (none: nothing is
    /// kept), not yet dialling. Link events arrive on `events`.
    pub fn new(
        net: Net,
        events: async_channel::Sender<Event>,
        settings_path: Option<PathBuf>,
    ) -> Self {
        let (settings, notice) = match &settings_path {
            Some(p) => Settings::load(p),
            None => (
                Settings::default(),
                Some("No config folder; settings will not be kept.".into()),
            ),
        };
        Self {
            fleet: Fleet::new(net, events),
            settings,
            settings_path,
            save_task: None,
            known: None,
            writing: false,
            watcher: None,
            watch_task: None,
            status: notice.map(Status::failure),
            toast: None,
            toast_task: None,
            stack: Stack::default(),
            next_popup: 1,
            answers: vec![],
            heard: HashSet::new(),
            away: HashSet::new(),
            clicks: HashMap::new(),
            models: HashMap::new(),
            side: HashMap::new(),
            panel: Default::default(),
            shown: None,
            guards: vec![],
        }
    }

    /// Draws the app in the theme the settings name, when it is not the
    /// one drawn: every window draws again. A name no direction has is
    /// reported, and the theme stays.
    pub fn apply_theme(&mut self, cx: &mut Context<Self>) {
        let Some(direction) = Direction::from_key(&self.settings.theme) else {
            let text = format!(
                "No theme is called {:?}; the theme stays as it was",
                self.settings.theme
            );
            return self.report(Status::failure(text), cx);
        };
        if cx.try_global::<Theme>().map(|t| t.direction) != Some(direction) {
            cx.set_global(Theme::new(direction));
            cx.refresh_windows();
        }
    }

    /// Sizes the whole app by a zoom gesture made over no surface with a
    /// zoom of its own, keeps it and says the size it is at now. The shell
    /// draws its window at the size kept.
    pub fn zoom_app(&mut self, z: crate::zoom::Zoom, n: i32, cx: &mut Context<Self>) {
        let Some(to) = crate::zoom::app::zoomed(self.settings.size, z, n) else {
            return;
        };
        self.settings.size = to;
        self.save(cx);
        self.toast(&crate::zoom::app::words(to), cx);
        cx.notify();
    }

    /// What each page answers, as the shell mounted them.
    pub fn set_answers(&mut self, answers: Vec<(&'static str, &'static [Answers])>) {
        self.answers = answers;
    }

    /// Dials the configured node.
    pub fn connect(&mut self, cx: &mut Context<Self>) {
        let node = self.settings.node.clone();
        self.fleet.connect(&node);
        cx.notify();
    }

    /// Dials every link again now.
    pub fn redial(&mut self, cx: &mut Context<Self>) {
        self.fleet.redial();
        cx.notify();
    }

    /// Posts a desktop notification, keeping what a click on it does.
    pub fn notify(&mut self, n: SystemNotification, click: Run, cx: &mut Context<Self>) {
        self.clicks.insert(n.tag.to_string(), click);
        cx.show_system_notification(n);
    }

    /// What a click on the notification tagged `tag` does, once.
    pub fn clicked(&mut self, tag: &str) -> Option<Run> {
        self.clicks.remove(tag)
    }

    /// A status action that dials again.
    fn reconnect(cx: &mut Context<Self>) -> (String, Run) {
        let hub = cx.entity().downgrade();
        let run: Run = Rc::new(move |_, cx| {
            let _ = hub.update(cx, |h, cx| h.redial(cx));
        });
        ("Reconnect".into(), run)
    }

    /// A link event: the fleet takes it; frames go to the pages, and what
    /// the link did is told in the status line.
    pub fn handle(&mut self, ev: Event, cx: &mut Context<Self>) {
        let was = self.fleet.here().state;
        let links = self.links();
        for (node, n) in self.fleet.handle(ev, &self.settings.places) {
            self.notice(node, n, cx);
        }
        self.links_changed(links, cx);
        self.remember_fleet(cx);
        self.track(cx);
        let here = self.fleet.here();
        if was != here.state && here.state == LinkState::Refused {
            let why = here.reason.clone().unwrap_or_default();
            let (label, run) = Self::reconnect(cx);
            self.status = Some(Status {
                action: Some((label, run)),
                ..Status::failure(why)
            });
        }
        cx.notify();
    }

    fn notice(&mut self, node: String, n: Notice, cx: &mut Context<Self>) {
        match n {
            Notice::Frame { kind, body } => cx.emit(HubEvent::Frame {
                node: NodeId(node),
                kind,
                body,
            }),
            Notice::Nodes(_) | Notice::Serving(None) => {}
            Notice::Serving(Some(through)) => {
                let name = self
                    .settings
                    .places
                    .get(&through)
                    .map(|p| format!("{} ({})", p.device, p.environment))
                    .unwrap_or(through);
                let text = format!("{} is away; trying {name}", self.home_name());
                self.status = Some(Status::failure(text));
            }
            Notice::Opened if Some(node.as_str()) != self.fleet.here_id() => {}
            Notice::Opened => {
                if let Some(h) = self.fleet.here().hello.clone() {
                    let to = format!("{} ({}), Ahjo {}", h.device, h.environment, h.version);
                    let text = match self.fleet.away() {
                        Some(_) => {
                            format!("Connected through {to} while {} is away", self.home_name())
                        }
                        // Connected as asked: nothing to say, unless it
                        // follows a failure the operator saw.
                        None if self.status.as_ref().is_some_and(|s| s.failure) => {
                            format!("Connected to {to}")
                        }
                        None => return,
                    };
                    self.status = Some(Status::new(text));
                }
            }
        }
    }

    /// Each followed node's link state, by node.
    fn links(&self) -> BTreeMap<String, LinkState> {
        self.fleet
            .clients()
            .iter()
            .map(|c| (c.node(), c.state))
            .filter(|(n, _)| !n.is_empty())
            .collect()
    }

    /// Tells the pages of each link that changed since `before`.
    fn links_changed(&mut self, before: BTreeMap<String, LinkState>, cx: &mut Context<Self>) {
        let now = self.links();
        for (node, state) in &now {
            if before.get(node) != Some(state) {
                cx.emit(HubEvent::Link {
                    node: NodeId(node.clone()),
                    state: Some(*state),
                });
            }
        }
        for node in before.keys().filter(|n| !now.contains_key(*n)) {
            cx.emit(HubEvent::Link {
                node: NodeId(node.clone()),
                state: None,
            });
        }
    }

    /// The state of the link to `node`; `None` when the app follows no such node.
    pub fn link(&self, node: &str) -> Option<LinkState> {
        self.fleet.get(node).map(|c| c.state)
    }

    /// The configured node by its device, or by its address before it has
    /// said hello.
    fn home_name(&self) -> String {
        self.fleet
            .home_device()
            .map(str::to_string)
            .unwrap_or_else(|| self.settings.node.clone())
    }

    /// Keeps `places` the last known fleet: each node's device, environment
    /// and address. Nodes the configured node no longer lists go.
    fn remember_fleet(&mut self, cx: &mut Context<Self>) {
        let places = &mut self.settings.places;
        let before = places.clone();
        for c in self.fleet.clients() {
            let node = c.node();
            if node.is_empty() {
                continue;
            }
            let p = places.entry(node).or_default();
            if let Some(h) = &c.hello {
                p.device = h.device.clone();
                p.environment = h.environment.clone();
            }
        }
        for peer in &self.fleet.peers {
            let p = places.entry(peer.node.clone()).or_default();
            p.device = peer.device.clone();
            p.environment = peer.environment.clone();
            if !peer.address.is_empty() {
                p.address = peer.address.clone();
            }
        }
        // A node serving while the configured one is away may list a cold
        // fleet (it has not probed since nobody asked), the configured node
        // missing: forget nothing until the configured node serves again.
        if self.fleet.listed
            && self.fleet.away().is_none()
            && let Some(here) = self.fleet.here_id()
        {
            let peers = &self.fleet.peers;
            places.retain(|n, _| n == here || peers.iter().any(|p| p.node == *n));
        }
        if *places != before {
            self.save(cx);
        }
    }

    /// Stamps a node that stopped answering with when it was last heard,
    /// so an away device says since when, also after a restart; says in
    /// the status line when a device goes and comes back.
    fn track(&mut self, cx: &mut Context<Self>) {
        let before = self.heard.clone();
        let now = now_ms();
        let mut stamped = false;
        // A link still dialling for the first time (a new serving node's
        // peers) has not lost its node: only one that went says it left.
        let links: Vec<(String, LinkState)> = self
            .fleet
            .clients()
            .iter()
            .map(|c| (c.node(), c.state))
            .filter(|(n, s)| !n.is_empty() && *s != LinkState::Connecting)
            .collect();
        for (node, state) in links {
            if state == LinkState::Open {
                self.heard.insert(node);
            } else if self.heard.remove(&node)
                && let Some(p) = self.settings.places.get_mut(&node)
            {
                p.seen = Some(now);
                stamped = true;
            }
        }
        if stamped {
            self.save(cx);
        }
        let (gone, came) = devices::changed(&self.settings.places, &before, &self.heard);
        let serving = self.fleet.here().hello.as_ref().map(|h| h.device.clone());
        for device in gone {
            let line = devices::away_line(&device, Some(&hhmm(now)));
            let mut status = Status::new(line);
            if serving.as_deref() == Some(device.as_str()) {
                status.action = Some(Self::reconnect(cx));
            }
            self.status = Some(status);
            self.away.insert(device);
        }
        for device in came {
            if self.away.remove(&device) && serving.as_deref() != Some(device.as_str()) {
                self.status = Some(Status::new(format!("{device} is back")));
            }
        }
    }

    pub fn report(&mut self, status: Status, cx: &mut Context<Self>) {
        self.status = Some(status);
        cx.notify();
    }

    /// Takes back the status said `at`, if nothing has replaced it.
    pub fn withdraw(&mut self, at: SystemTime, cx: &mut Context<Self>) {
        if self.status.as_ref().is_some_and(|s| s.at == at) {
            self.status = None;
            cx.notify();
        }
    }

    pub fn toast(&mut self, text: &str, cx: &mut Context<Self>) {
        let at = Instant::now();
        self.toast = Some(Toast {
            text: text.to_string(),
            at,
        });
        self.toast_task = Some(cx.spawn(async move |this, cx| {
            // Two wakes: one to start fading, one to go.
            cx.background_executor()
                .timer(Duration::from_millis(
                    motion::TOAST_IN + motion::TOAST_SHOWN,
                ))
                .await;
            let _ = this.update(cx, |_, cx| cx.notify());
            cx.background_executor()
                .timer(Duration::from_millis(motion::TOAST_OUT))
                .await;
            let _ = this.update(cx, |h, cx| {
                if h.toast.as_ref().is_some_and(|t| t.at == at) {
                    h.toast = None;
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// Writes the settings soon: a burst of changes is one write.
    pub fn save(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.settings_path.clone() else {
            return;
        };
        self.save_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(250))
                .await;
            let Ok(text) = this.update(cx, |h, _| {
                let text = h.settings.text();
                h.known = Some(text.clone());
                h.writing = true;
                text
            }) else {
                return;
            };
            let saved = cx
                .background_executor()
                .spawn(async move { Settings::write(&path, &text) })
                .await;
            let _ = this.update(cx, |h, _| h.writing = false);
            if let Err(e) = saved {
                let _ = this.update(cx, |h, cx| {
                    h.report(
                        Status::failure(format!("Settings could not be saved: {e}")),
                        cx,
                    );
                });
            }
        }));
    }

    /// Reads the settings again whenever their file changes on disk, as an
    /// agent edits it. Only the file's own events wake it.
    pub fn watch(&mut self, cx: &mut Context<Self>) {
        use notify::Watcher;
        let Some(path) = self.settings_path.clone() else {
            return;
        };
        let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
            return;
        };
        let name = name.to_os_string();
        // One pending wake is enough: the reload reads the file as it is.
        let (tx, rx) = async_channel::bounded::<()>(1);
        let watched = std::fs::create_dir_all(dir)
            .map_err(notify::Error::io)
            .and_then(|_| {
                let mut w =
                    notify::recommended_watcher(move |ev: notify::Result<notify::Event>| {
                        // Reading the file is an event too: only a change wakes it.
                        let changed = |ev: &notify::Event| {
                            !ev.kind.is_access()
                                && ev.paths.iter().any(|p| p.file_name() == Some(&name))
                        };
                        if ev.is_ok_and(|ev| changed(&ev)) {
                            let _ = tx.try_send(());
                        }
                    })?;
                w.watch(dir, notify::RecursiveMode::NonRecursive)?;
                Ok(w)
            });
        match watched {
            Ok(w) => self.watcher = Some(w),
            Err(e) => {
                let text = format!(
                    "{} is not watched ({e}); edits to it apply at the next start",
                    path.display()
                );
                self.report(Status::failure(text), cx);
                return;
            }
        }
        self.watch_task = Some(cx.spawn(async move |this, cx| {
            while rx.recv().await.is_ok() {
                if this.update(cx, |h, cx| h.reload(cx)).is_err() {
                    break;
                }
            }
        }));
    }

    /// Reads the settings file again. An edit made outside the app replaces
    /// the settings: the node is dialled if it changed and every page whose
    /// section changed hears it. A file that does not read is reported and
    /// the settings stay as they were.
    fn reload(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.settings_path.clone().filter(|_| !self.writing) else {
            return;
        };
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            // Gone, or between a write and its rename: nothing to read.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(e) => {
                let text = format!("{} could not be read: {e}", path.display());
                return self.report(Status::failure(text), cx);
            }
        };
        if self.known.as_deref() == Some(text.as_str()) {
            return;
        }
        let new: Settings = match serde_json::from_str(&text) {
            Ok(new) => new,
            Err(e) => {
                let text = format!(
                    "{} does not read ({e}); the settings stay as they were",
                    path.display()
                );
                return self.report(Status::failure(text), cx);
            }
        };
        self.known = Some(text);
        if new == self.settings {
            return;
        }
        let old = std::mem::replace(&mut self.settings, new);
        let changed: Vec<&'static str> = self
            .answers
            .iter()
            .map(|(id, _)| *id)
            .filter(|id| old.sections.get(*id) != self.settings.sections.get(*id))
            .collect();
        if old.node != self.settings.node {
            self.connect(cx);
        }
        if old.theme != self.settings.theme {
            self.apply_theme(cx);
        }
        cx.emit(HubEvent::SettingsReloaded(changed));
        self.report(
            Status::new(format!("Settings read again from {}", path.display())),
            cx,
        );
    }

    /// Opens a surface over the others.
    pub fn open(&mut self, s: Surface, cx: &mut Context<Self>) {
        let closed: Vec<Surface> = self
            .stack
            .iter()
            .filter(|x| layers::Layer::passing(*x))
            .cloned()
            .collect();
        self.stack.open(s);
        cx.emit(HubEvent::Layers { closed });
        cx.notify();
    }

    /// Closes the surface of this kind and whatever opened above it.
    pub fn close(&mut self, kind: &str, cx: &mut Context<Self>) {
        let closed = self.stack.close(kind);
        if !closed.is_empty() {
            cx.emit(HubEvent::Layers { closed });
            cx.notify();
        }
    }

    /// Opens `popup` over the others, named `<page>/popup/<n>`.
    pub fn open_popup(&mut self, page: &str, mut popup: Popup, cx: &mut Context<Self>) -> PopupId {
        let id = PopupId(self.next_popup);
        self.next_popup += 1;
        popup.id = id;
        popup.name = format!("{page}/popup/{}", id.0);
        self.open(Surface::Popup(popup), cx);
        id
    }

    /// Closes popup `id` and whatever opened above it, if it is open.
    pub fn close_popup(&mut self, id: PopupId, cx: &mut Context<Self>) {
        if let Some(name) = self.popup(id).map(|p| p.name.clone()) {
            self.close(&name, cx);
        }
    }

    /// Popup `id`, while it is open.
    pub fn popup(&self, id: PopupId) -> Option<&Popup> {
        self.stack.iter().find_map(|s| match s {
            Surface::Popup(p) if p.id == id => Some(p),
            _ => None,
        })
    }

    /// Closes the open menu, if one is.
    pub fn close_menu(&mut self, cx: &mut Context<Self>) {
        let menu = self.stack.iter().find_map(|s| match s {
            Surface::Popup(p) if p.is_menu() => Some(p.name.clone()),
            _ => None,
        });
        if let Some(name) = menu {
            self.close(&name, cx);
        }
    }

    /// Escape: the top surface closes. False when nothing was open.
    pub fn dismiss(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(top) = self.stack.dismiss() else {
            return false;
        };
        cx.emit(HubEvent::Layers { closed: vec![top] });
        cx.notify();
        true
    }

    /// The read model `T`, made empty by whichever page reaches it first.
    pub fn model<T: Default + 'static>(&mut self, cx: &mut Context<Self>) -> Entity<T> {
        self.models
            .entry(TypeId::of::<T>())
            .or_insert_with(|| Model {
                entity: cx.new(|_| T::default()).into_any(),
                publisher: None,
            })
            .entity
            .clone()
            .downcast::<T>()
            .expect("a model is kept by its own type")
    }

    /// The read model `T` for page `page` to publish; refused when another
    /// page publishes it.
    pub fn publisher<T: Default + 'static>(
        &mut self,
        page: &'static str,
        cx: &mut Context<Self>,
    ) -> Result<Entity<T>, Error> {
        let entity = self.model::<T>(cx);
        let model = self
            .models
            .get_mut(&TypeId::of::<T>())
            .expect("made just now");
        match model.publisher {
            Some(other) if other != page => Err(Error::new(
                "published_twice",
                format!("{other} and {page} both publish {}", type_name::<T>()),
            )),
            _ => {
                model.publisher = Some(page);
                Ok(entity)
            }
        }
    }

    /// Keeps `value` for the setting `key` page `page` declares, and tells
    /// the page.
    pub fn set_value(
        &mut self,
        page: &'static str,
        key: &str,
        value: serde_json::Value,
        cx: &mut Context<Self>,
    ) {
        self.settings.set_value(page, key, value);
        self.save(cx);
        cx.emit(HubEvent::SettingChanged(page));
        cx.notify();
    }

    /// Whether a view page `page` lent shows on other pages: its
    /// section of the open side panel, or its content in the open panel.
    pub fn lends(&self, page: &str) -> bool {
        (self.settings.side_open && self.side.contains_key(page)) || self.panel.lends(page)
    }

    /// The panel's top content while it stands beside page `shown`: open,
    /// or put away and still sliding (`open` says which).
    pub fn panel_on(&self, shown: &str) -> Option<&crate::panel::Panel> {
        self.panel.top().filter(|p| p.shows_on(shown))
    }

    /// Opens the panel with `panel` on top. The page whose content showed
    /// before hears it gave way, and this page that its content shows.
    pub fn show_panel(&mut self, panel: crate::panel::Panel, cx: &mut Context<Self>) {
        let before = self.panel_page();
        let page = panel.page;
        self.panel.show(panel);
        self.lent_panel(before, Some(page), cx);
    }

    /// Takes content `name` out of the panel: what it opened over shows
    /// again, or the panel closes when none is left.
    pub fn remove_panel(&mut self, name: &str, cx: &mut Context<Self>) {
        let before = self.panel_page();
        self.panel.remove(name);
        let after = self.panel_page();
        self.lent_panel(before, after, cx);
    }

    /// Opens or puts away the panel, its content kept for Ctrl+B to bring
    /// back.
    pub fn set_panel_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.panel.open == open || (open && self.panel.top().is_none()) {
            return;
        }
        let before = self.panel_page();
        self.panel.open = open;
        let after = self.panel_page();
        self.lent_panel(before, after, cx);
    }

    /// The page whose content shows in the open panel.
    fn panel_page(&self) -> Option<&'static str> {
        self.panel.top().filter(|_| self.panel.open).map(|p| p.page)
    }

    fn lent_panel(
        &mut self,
        before: Option<&'static str>,
        after: Option<&'static str>,
        cx: &mut Context<Self>,
    ) {
        for page in before.into_iter().chain(after) {
            cx.emit(HubEvent::Lent(page));
        }
        // Even when the same page shows: the shell looks where the
        // keyboard was.
        if before.is_none() && after.is_none() {
            cx.emit(HubEvent::Lent(""));
        }
        cx.notify();
    }

    /// The panel's width as dragged; `keep` writes it for the next run.
    pub fn set_panel_width(&mut self, width: f32, keep: bool, cx: &mut Context<Self>) {
        self.settings.panel_width = crate::settings::panel_width(width);
        if keep {
            self.save(cx);
        }
        cx.notify();
    }

    /// The page the window shows.
    pub fn shown(&self) -> Option<&'static str> {
        self.shown
    }

    /// The shell shows page `page`.
    pub fn set_shown(&mut self, page: Option<&'static str>) {
        self.shown = page;
    }

    /// Page `page`'s section of the side panel, and the pages it shows on.
    pub fn side(&self, page: &str) -> Option<(AnyView, SideOn)> {
        self.side.get(page).cloned()
    }

    /// Puts `view` in the side panel as page `page`'s section, shown on
    /// the pages `on` says, or takes its section out.
    pub fn set_side(
        &mut self,
        page: &'static str,
        view: Option<(AnyView, SideOn)>,
        cx: &mut Context<Self>,
    ) {
        match view {
            Some(view) => self.side.insert(page, view),
            None => self.side.remove(page),
        };
        cx.emit(HubEvent::Lent(page));
        cx.notify();
    }

    /// Opens or puts away the side panel, kept for the next run.
    pub fn set_side_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.settings.side_open == open {
            return;
        }
        self.settings.side_open = open;
        self.save(cx);
        let lenders: Vec<&'static str> = self.side.keys().copied().collect();
        for page in lenders {
            cx.emit(HubEvent::Lent(page));
        }
        cx.notify();
    }

    /// Asks `unsaved` about `view`, of page `page`, whenever the window
    /// would close, until `view` is gone.
    pub fn guard_close(&mut self, page: &'static str, view: AnyWeakEntity, unsaved: Unsaved) {
        self.guards.retain(|g| g.view.is_upgradable());
        self.guards.push(Guard {
            page,
            view,
            unsaved,
        });
    }

    /// What every live view holds unsaved now, by the page it is from, in
    /// the order they were guarded.
    pub fn unsaved(&self, cx: &App) -> Vec<(&'static str, String)> {
        self.guards
            .iter()
            .filter_map(|g| (g.unsaved)(cx).map(|label| (g.page, label)))
            .collect()
    }

    /// Hands `ask`, from page `from`, to the page that answers it. When
    /// none does, the status line says so and so does the error.
    pub fn ask(
        &mut self,
        from: &'static str,
        ask: Ask,
        cx: &mut Context<Self>,
    ) -> Result<(), Error> {
        let to = self
            .answers
            .iter()
            .find(|(_, a)| a.iter().any(|a| ask.answered_by(a)))
            .map(|(id, _)| *id);
        match to {
            Some(to) => {
                cx.emit(HubEvent::Ask { to, from, ask });
                Ok(())
            }
            None => {
                let e = Error::new(
                    "unanswered",
                    format!("Nothing here can {}.", ask.describe()),
                );
                self.report(Status::failure(e.message.clone()), cx);
                Err(e)
            }
        }
    }

    /// The nodes that answer now, serving node first.
    pub fn live_nodes(&self) -> Vec<Node> {
        self.fleet
            .clients()
            .iter()
            .filter(|c| c.state == LinkState::Open)
            .filter_map(|c| {
                let h = c.hello.as_ref()?;
                Some(Node {
                    id: NodeId(c.node()),
                    device: h.device.clone(),
                    environment: h.environment.clone(),
                })
            })
            .collect()
    }

    /// The areas every node reached has turned off; none before any hello.
    pub fn areas_off(&self) -> Vec<String> {
        let hellos: Vec<_> = self
            .fleet
            .clients()
            .iter()
            .filter_map(|c| c.hello.as_ref())
            .collect();
        let Some(first) = hellos.first() else {
            return vec![];
        };
        first
            .off
            .iter()
            .filter(|a| hellos.iter().all(|h| h.off.contains(a)))
            .cloned()
            .collect()
    }
}
