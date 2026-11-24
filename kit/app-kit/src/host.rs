//! What a page reaches the rest of the app through: its [`Host`], a cheap
//! handle on the [`Hub`]. A page never holds the shell; what it asks of
//! the shell arrives there as a [`HubEvent`](crate::hub::HubEvent), after
//! the page's own update is over.

use crate::ask::Ask;
use crate::frame::{AppFrame, Layer, Status, Surface};
use crate::hub::{Hub, HubEvent};
use crate::link::client::LinkState;
use crate::link::http;
use crate::link::socket::Tap;
use crate::menu::{Menu, MenuRow};
use crate::popup::{Popup, PopupId, Spot};
use crate::settings::Place;
use ahjo_core::frame::{Kinds, owns};
use ahjo_core::{Error, NodeId, TypedCommand};
use gpui::{AnyView, App, Context, Entity, Pixels, Point, SystemNotification, Task, WeakEntity};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

/// A page's handle on the hub. Cloning it is cheap.
#[derive(Clone)]
pub struct Host {
    page: &'static str,
    /// The page's frame kinds: all its taps see.
    kinds: Kinds,
    hub: WeakEntity<Hub>,
}

/// A node that answers now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub id: NodeId,
    pub device: String,
    pub environment: String,
}

/// What the window shows and runs, as a report names it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WindowState {
    /// The app's build; empty when its binary named none.
    pub app_build: &'static str,
    /// The node the app is served by: its id, address and build, once it
    /// has said hello.
    pub node_id: String,
    pub node_url: String,
    pub node_build: String,
    /// The page shown.
    pub page: Option<&'static str>,
    /// The panel's content on top (`<page>/<name>`), and whether the panel
    /// is open with it or put away.
    pub panel: Option<String>,
    pub panel_is_open: bool,
    /// Whether the side panel at the left is open or put away to its edge.
    pub side_is_open: bool,
    /// The popups and layers open over the page, oldest first.
    pub surfaces: Vec<String>,
    /// The window's size in its own pixels, and the display's scale.
    pub width: f32,
    pub height: f32,
    pub scale: f32,
}

/// An HTTP request to a node, through the serving node.
#[derive(Clone, Debug)]
pub struct Request {
    /// The node it is for; empty for the serving node itself.
    pub node: NodeId,
    pub method: &'static str,
    pub path: String,
    pub headers: Vec<(String, String)>,
    /// Its content type and bytes.
    pub body: Option<(String, Vec<u8>)>,
    /// How long the node may take to answer.
    pub within: Duration,
}

impl Request {
    pub fn new(method: &'static str, node: NodeId, path: impl Into<String>) -> Self {
        Self {
            node,
            method,
            path: path.into(),
            headers: vec![],
            body: None,
            within: http::ANSWER,
        }
    }

    pub fn get(node: NodeId, path: impl Into<String>) -> Self {
        Self::new("GET", node, path)
    }

    pub fn json(mut self, body: &serde_json::Value) -> Self {
        self.body = Some(("application/json".into(), body.to_string().into_bytes()));
        self
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// For an action the node may take longer to answer.
    pub fn within(mut self, within: Duration) -> Self {
        self.within = within;
        self
    }
}

/// A node's answer: its status and body, whatever the status.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answer {
    pub status: u16,
    pub body: Vec<u8>,
}

impl Answer {
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// The node's `{error}` words for a failed answer, or its status.
    pub fn error_text(&self) -> String {
        http::error_text(self.status, &self.body)
    }
}

/// The nodes as the app reached them when it was taken
/// ([`Host::reach`]): a read sent through it goes to the serving node,
/// which passes on one for another node. It works from any thread.
#[derive(Clone)]
pub struct Reach {
    base: String,
    here: Option<String>,
    rt: tokio::runtime::Handle,
}

impl Reach {
    /// The request's path, naming its node when that is not the serving one.
    fn path(&self, r: &Request) -> String {
        if r.node.as_str().is_empty() || self.here.as_deref() == Some(r.node.as_str()) {
            r.path.clone()
        } else {
            let sep = if r.path.contains('?') { '&' } else { '?' };
            format!("{}{sep}node={}", r.path, r.node)
        }
    }

    /// Reads what the GET `r` asks for, its body to come as it is sent
    /// ([`http::Streamed`]): `unreachable` when no answer began.
    pub fn open(
        &self,
        r: Request,
    ) -> impl std::future::Future<Output = Result<http::Streamed, Error>> + Send + 'static {
        let reach = self.clone();
        let opened = self.rt.spawn(async move {
            let path = reach.path(&r);
            let headers: Vec<(&str, &str)> = r
                .headers
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();
            http::open(&reach.base, &path, &headers, r.within).await
        });
        async move {
            match opened.await {
                Ok(Ok(streamed)) => Ok(streamed),
                Ok(Err(e)) => Err(Error::new("unreachable", e.message)),
                Err(e) => Err(Error::new("unreachable", e.to_string())),
            }
        }
    }

    /// Whether `node`'s files are this computer's own: it is the serving
    /// node, reached on this computer's own address.
    pub fn on_this_computer(&self, node: &str) -> bool {
        let serving = node.is_empty() || self.here.as_deref() == Some(node);
        let host = self
            .base
            .split_once("://")
            .map_or(self.base.as_str(), |(_, rest)| rest)
            .split('/')
            .next()
            .unwrap_or_default();
        let host = match host.rsplit_once(':') {
            Some((h, port)) if port.chars().all(|c| c.is_ascii_digit()) => h,
            _ => host,
        };
        let host = host.trim_start_matches('[').trim_end_matches(']');
        let local = host == "localhost"
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback());
        serving && local
    }

    /// Runs `work` on the link's own threads, where reads are sent.
    pub fn spawn(&self, work: impl std::future::Future<Output = ()> + Send + 'static) {
        self.rt.spawn(work);
    }
}

impl Host {
    /// The shell's own handle on `hub`; pages get theirs from [`crate::mount`].
    pub fn new(hub: &Entity<Hub>) -> Self {
        Self {
            page: "",
            kinds: &[],
            hub: hub.downgrade(),
        }
    }

    pub fn page(&self) -> &'static str {
        self.page
    }

    /// The same hub, for page `page`, whose frame kinds are `kinds`.
    pub fn for_page(&self, page: &'static str, kinds: Kinds) -> Host {
        Host {
            page,
            kinds,
            hub: self.hub.clone(),
        }
    }

    fn read<R>(&self, cx: &App, f: impl FnOnce(&Hub) -> R) -> Option<R> {
        self.hub.upgrade().map(|h| f(h.read(cx)))
    }

    fn update<R>(
        &self,
        cx: &mut App,
        f: impl FnOnce(&mut Hub, &mut Context<Hub>) -> R,
    ) -> Option<R> {
        self.hub.update(cx, f).ok()
    }

    /// The window's frame: its stack, status line and toast.
    pub fn frame(&self, cx: &App) -> Option<Entity<AppFrame>> {
        self.read(cx, |h| h.frame.clone())
    }

    fn read_frame<R>(&self, cx: &App, f: impl FnOnce(&AppFrame) -> R) -> Option<R> {
        self.frame(cx).map(|frame| f(frame.read(cx)))
    }

    fn update_frame<R>(
        &self,
        cx: &mut App,
        f: impl FnOnce(&mut AppFrame, &mut Context<AppFrame>) -> R,
    ) -> Option<R> {
        self.frame(cx).map(|frame| frame.update(cx, f))
    }

    /// The nodes that answer now, serving node first.
    pub fn nodes(&self, cx: &App) -> Vec<Node> {
        self.read(cx, Hub::live_nodes).unwrap_or_default()
    }

    /// The serving node's address, where HTTP requests go.
    pub fn base(&self, cx: &App) -> String {
        self.read(cx, |h| h.fleet.base().to_string())
            .unwrap_or_default()
    }

    /// Sends `r` and hands back the node's answer, or why there was none:
    /// `unreachable` when it went nowhere, `unanswered` when it may have
    /// reached the node (a write's outcome is unknown, not failed).
    pub fn request(&self, r: Request, cx: &mut App) -> Task<Result<Answer, Error>> {
        let Some(reach) = self.reach(cx) else {
            return Task::ready(Err(Error::new("closed", "The app is closing.")));
        };
        cx.background_executor().spawn(async move {
            let sent = reach.rt.clone().spawn(async move {
                let path = reach.path(&r);
                let headers: Vec<(&str, &str)> = r
                    .headers
                    .iter()
                    .map(|(k, v)| (k.as_str(), v.as_str()))
                    .collect();
                let body = r.body.as_ref().map(|(t, b)| (t.as_str(), b.as_slice()));
                http::send(&reach.base, r.method, &path, &headers, body, r.within).await
            });
            match sent.await {
                Ok(Ok((status, body))) => Ok(Answer { status, body }),
                Ok(Err(e)) if e.maybe_done => Err(Error::new("unanswered", e.message)),
                Ok(Err(e)) => Err(Error::new("unreachable", e.message)),
                Err(e) => Err(Error::new("unreachable", e.to_string())),
            }
        })
    }

    /// The nodes as they are reached now, to read from off the app's
    /// thread (a web view's requests): `None` once the app is closing.
    pub fn reach(&self, cx: &App) -> Option<Reach> {
        self.read(cx, |h| Reach {
            base: h.fleet.base().to_string(),
            here: h.fleet.here_id().map(str::to_string),
            rt: h.fleet.here().runtime().clone(),
        })
    }

    /// Runs the node's typed command and reads its typed result.
    pub fn call<C: TypedCommand>(
        &self,
        node: NodeId,
        input: C,
        cx: &mut App,
    ) -> Task<Result<C::Output, Error>> {
        let body = match serde_json::to_vec(&input) {
            Ok(body) => body,
            Err(error) => {
                return Task::ready(Err(Error::new("invalid_argument", error.to_string())));
            }
        };
        let mut request = Request::new("POST", node, format!("/api/call/{}", C::NAME));
        request.body = Some(("application/json".into(), body));
        if let Some(within) = C::TIMEOUT {
            request = request.within(within);
        }
        let answer = self.request(request, cx);
        cx.background_executor().spawn(async move {
            let answer = answer.await?;
            if answer.ok() {
                return serde_json::from_slice(&answer.body).map_err(|error| {
                    Error::new(
                        "invalid_response",
                        format!("The node's {} answer did not read: {error}", C::NAME),
                    )
                });
            }
            #[derive(serde::Deserialize)]
            struct Failure {
                error: Error,
            }
            Err(serde_json::from_slice::<Failure>(&answer.body)
                .map(|failure| failure.error)
                .unwrap_or_else(|_| Error::new("invalid_response", answer.error_text())))
        })
    }

    /// Sends a frame to one node. False when it is not reached now.
    pub fn send(&self, node: &NodeId, frame: Vec<u8>, cx: &App) -> bool {
        self.read(cx, |h| h.fleet.send(node.as_str(), frame))
            .unwrap_or(false)
    }

    /// The state of the link to `node`; `None` when the app follows no
    /// such node (it went from the fleet, or was never in it).
    pub fn link(&self, node: &NodeId, cx: &App) -> Option<LinkState> {
        self.read(cx, |h| h.link(node.as_str())).flatten()
    }

    /// No node answers: the serving node's link went or was refused, as
    /// the top bar's offline words say. A page with nothing read yet says
    /// so rather than that it is reading.
    pub fn offline(&self, cx: &App) -> bool {
        self.read(cx, |h| {
            matches!(h.fleet.here().state, LinkState::Lost | LinkState::Refused)
        })
        .unwrap_or(false)
    }

    /// The fleet as last seen, by node: each node's device and environment,
    /// also while it is away.
    pub fn places(&self, cx: &App) -> BTreeMap<String, Place> {
        self.read(cx, |h| h.settings.places.clone())
            .unwrap_or_default()
    }

    /// Hands every frame of this page's kinds, from every node, to `tap`
    /// on the network thread first, from now on (see [`Tap`]). Frames of
    /// other kinds never reach it.
    pub fn tap(&self, tap: Tap, cx: &App) {
        let kinds = self.kinds;
        let mine: Tap =
            Arc::new(move |node, kind, body| owns(kinds, kind) && tap(node, kind, body));
        self.read(cx, |h| h.fleet.tap(mine));
    }

    /// Turns a channel on or off on every node, and on each node that comes
    /// into reach while it is on.
    pub fn channel(&self, kind: u8, on: bool, cx: &mut App) {
        self.update(cx, |h, _| h.fleet.channel(kind, on));
    }

    /// Says something on the status line.
    pub fn report(&self, status: Status, cx: &mut App) {
        self.update_frame(cx, |f, cx| f.report(status, cx));
    }

    /// Takes back what was said on the status line `at` (its `Status::at`),
    /// once it is no longer true; anything said since stays.
    pub fn withdraw(&self, at: SystemTime, cx: &mut App) {
        self.update_frame(cx, |f, cx| f.withdraw(at, cx));
    }

    /// A moment's word in the status line.
    pub fn toast(&self, text: &str, cx: &mut App) {
        self.update_frame(cx, |f, cx| f.toast(text, cx));
    }

    /// Opens `popup` over the others; the shell places it, closes it and
    /// gives the keyboard back ([`crate::popup`]).
    pub fn popup(&self, popup: Popup, cx: &mut App) -> Option<PopupId> {
        let page = self.page;
        self.update_frame(cx, |f, cx| f.open_popup(page, popup, cx))
    }

    /// Closes popup `id`, running its dismiss, and whatever opened above
    /// it; nothing when it is already closed.
    pub fn close_popup(&self, id: PopupId, cx: &mut App) {
        self.update_frame(cx, |f, cx| f.close_popup(id, cx));
    }

    /// Opens a menu beside `at`: the button pressed there, else the point.
    pub fn menu(&self, rows: Vec<MenuRow>, at: Point<Pixels>, cx: &mut App) {
        let page = self.page;
        self.update_frame(cx, |f, cx| f.menu(page, rows, at, false, cx));
    }

    /// Opens a menu beside `at` with its danger row `armed` already: a key
    /// that asked for the danger opens it one confirmation away.
    pub fn menu_armed(&self, rows: Vec<MenuRow>, at: Point<Pixels>, armed: usize, cx: &mut App) {
        let menu = Menu::new(rows).armed(armed);
        self.popup(Popup::menu(Spot::at(at, cx), menu), cx);
    }

    /// Opens a menu beside `at` for the keyboard: its first row it can pick
    /// is chosen, so Enter runs it and the arrows move on.
    pub fn menu_keyed(&self, rows: Vec<MenuRow>, at: Point<Pixels>, cx: &mut App) {
        let page = self.page;
        self.update_frame(cx, |f, cx| f.menu(page, rows, at, true, cx));
    }

    /// Asks before `row` runs, beside `spot`: its one row, a danger armed
    /// already, runs on a click or Enter; Escape or a press outside lets
    /// it go. Its hint says what is lost.
    pub fn confirm(&self, spot: Spot, row: MenuRow, cx: &mut App) -> Option<PopupId> {
        let page = self.page;
        self.update_frame(cx, |f, cx| f.confirm(page, spot, row, cx))
    }

    /// Opens this page's layer `layer.name` over the others; one of that
    /// name already open moves to the top.
    pub fn layer(&self, mut layer: Layer, cx: &mut App) {
        layer.name = self.layer_name(&layer.name);
        self.update_frame(cx, |f, cx| f.open(Surface::Layer(layer), cx));
    }

    /// Closes this page's layer `name` and whatever opened above it.
    pub fn close_layer(&self, name: &str, cx: &mut App) {
        let kind = self.layer_name(name);
        self.update_frame(cx, |f, cx| f.close(&kind, cx));
    }

    pub fn is_open(&self, name: &str, cx: &App) -> bool {
        let kind = self.layer_name(name);
        self.read_frame(cx, |f| f.stack.is_open(&kind))
            .unwrap_or(false)
    }

    /// Something this page opened, other than a menu, is on the stack.
    pub fn has_open(&self, cx: &App) -> bool {
        let prefix = format!("{}/", self.page);
        self.read_frame(cx, |f| f.has_open(&prefix))
            .unwrap_or(false)
    }

    /// Popup `id` is open and nothing, not even a menu, opened over it.
    pub fn is_popup_top(&self, id: PopupId, cx: &App) -> bool {
        self.read_frame(
            cx,
            |f| matches!(f.stack.top(), Some(Surface::Popup(p)) if p.id() == id),
        )
        .unwrap_or(false)
    }

    /// This page's layer `name` is on top of the stack: nothing, not even
    /// a menu, opened over it.
    pub fn is_top(&self, name: &str, cx: &App) -> bool {
        let kind = self.layer_name(name);
        self.read_frame(cx, |f| {
            f.stack
                .top()
                .is_some_and(|s| crate::layers::Layer::kind(s) == kind)
        })
        .unwrap_or(false)
    }

    fn layer_name(&self, name: &str) -> String {
        format!("{}/{name}", self.page)
    }

    /// This page's settings: their defaults while it has none.
    pub fn setting<T: DeserializeOwned + Default>(&self, cx: &App) -> Result<T, Error> {
        self.read(cx, |h| h.settings.section(self.page))
            .unwrap_or_else(|| Ok(T::default()))
    }

    /// Keeps this page's settings.
    pub fn set_setting<T: Serialize>(&self, value: &T, cx: &mut App) -> Result<(), Error> {
        let page = self.page;
        self.update(cx, |h, cx| {
            h.settings.set_section(page, value)?;
            h.save(cx);
            Ok(())
        })
        .unwrap_or(Ok(()))
    }

    /// Hands `ask` to the page that answers it. When none does, the status
    /// line says so and so does the error.
    pub fn ask(&self, ask: Ask, cx: &mut App) -> Result<(), Error> {
        let from = self.page;
        self.update(cx, |h, cx| h.ask(from, ask, cx))
            .unwrap_or_else(|| Err(Error::new("closed", "The app is closing.")))
    }

    /// Guards `view` (this page, or its content in the panel)
    /// against the window closing over its unsaved work: while
    /// `view` lives, closing the window asks `unsaved`, which names what it
    /// holds unsaved (a file's name) or is `None` when all is saved. While
    /// any guarded view names something, the window stays open until the
    /// operator chooses to leave.
    pub fn guard_close<T: 'static>(
        &self,
        view: &Entity<T>,
        unsaved: impl Fn(&T, &App) -> Option<String> + 'static,
        cx: &mut App,
    ) {
        let page = self.page;
        let weak = view.downgrade();
        let any = weak.clone().into();
        let unsaved = Rc::new(move |cx: &App| weak.upgrade().and_then(|v| unsaved(v.read(cx), cx)));
        self.update(cx, |h, _| h.guard_close(page, any, unsaved));
    }

    /// The read model of type `T` pages share: one page publishes it, any
    /// page reads and observes it, built in any order. `None` only while
    /// the app closes.
    pub fn model<T: Default + 'static>(&self, cx: &mut App) -> Option<Entity<T>> {
        self.update(cx, |h, cx| h.model::<T>(cx))
    }

    /// Publishes `value` as the read model `T`, for every page that reads
    /// it; refused when another page publishes `T`.
    /// Readers hear only a value that differs from the last.
    pub fn publish<T: Default + PartialEq + 'static>(
        &self,
        value: T,
        cx: &mut App,
    ) -> Result<(), Error> {
        let page = self.page;
        let model = self
            .update(cx, |h, cx| h.publisher::<T>(page, cx))
            .unwrap_or_else(|| Err(Error::new("closed", "The app is closing.")))?;
        model.update(cx, |m, cx| {
            if *m != value {
                *m = value;
                cx.notify();
            }
        });
        Ok(())
    }

    /// Posts a desktop notification; a click on it runs `click` in the
    /// window it came from. A later one with the same tag replaces it.
    pub fn notify(
        &self,
        notification: SystemNotification,
        click: impl Fn(&mut gpui::Window, &mut App) + 'static,
        cx: &mut App,
    ) {
        self.update(cx, |h, cx| h.notify(notification, Rc::new(click), cx));
    }

    /// Puts `view` in the side panel as this page's section: the column at
    /// the window's left, drawn in the pages' order on the pages `on` says.
    /// While the side panel is open this page's channels stay on, as while
    /// its content shows in the panel.
    pub fn lend_side(&self, view: AnyView, on: crate::hub::SideOn, cx: &mut App) {
        let page = self.page;
        assert!(
            !page.is_empty(),
            "a side section is lent by a page's own host"
        );
        self.update(cx, |h, cx| h.set_side(page, Some((view, on)), cx));
    }

    /// Whether the side panel is open.
    pub fn side_open(&self, cx: &App) -> bool {
        self.read(cx, |h| h.settings.side_open).unwrap_or(true)
    }

    /// Opens or puts away the side panel, kept for the next run.
    pub fn set_side_open(&self, open: bool, cx: &mut App) {
        self.update(cx, |h, cx| h.set_side_open(open, cx));
    }

    /// Opens the panel at the window's edge with `panel` showing, as this
    /// page's content of that name; what showed there before waits under
    /// it, for Back. The page shown makes room for it. While it shows,
    /// this page's channels stay on.
    pub fn show_panel(&self, mut panel: crate::panel::Panel, cx: &mut App) {
        let page = self.page;
        assert!(!page.is_empty(), "panel content is a page's own");
        panel.page = page;
        panel.name = self.layer_name(&panel.name);
        self.update(cx, |h, cx| h.show_panel(panel, cx));
    }

    /// Takes this page's content `name` out of the panel: what it opened
    /// over shows again, or the panel closes.
    pub fn close_panel(&self, name: &str, cx: &mut App) {
        let name = self.layer_name(name);
        self.update(cx, |h, cx| h.remove_panel(&name, cx));
    }

    /// Puts the panel away, keeping what it shows for Ctrl+B to bring back.
    pub fn put_panel_away(&self, cx: &mut App) {
        self.update(cx, |h, cx| h.set_panel_open(false, cx));
    }

    /// This page's content `name` shows in the open panel.
    pub fn panel_shows(&self, name: &str, cx: &App) -> bool {
        let name = self.layer_name(name);
        self.read(cx, |h| h.panel.shows(&name)).unwrap_or(false)
    }

    /// The panel's width.
    pub fn panel_width(&self, cx: &App) -> f32 {
        self.read(cx, |h| h.settings.panel_width)
            .unwrap_or(crate::theme::size::PANEL)
    }

    /// What the window shows now: its page, panel and open surfaces,
    /// the app's and node's builds, and its size.
    pub fn window_state(&self, window: &gpui::Window, cx: &App) -> WindowState {
        let size = window.viewport_size();
        let surfaces = self
            .read_frame(cx, |f| {
                f.stack
                    .iter()
                    .map(|s| crate::layers::Layer::kind(s).to_string())
                    .collect()
            })
            .unwrap_or_default();
        let mut state = self
            .read(cx, |h| {
                let here = h.fleet.here();
                let hello = here.hello.as_ref();
                WindowState {
                    app_build: h.build,
                    node_id: hello.map(|h| h.node.clone()).unwrap_or_default(),
                    node_url: here.base.clone(),
                    node_build: hello.map(|h| h.version.clone()).unwrap_or_default(),
                    page: h.shown(),
                    panel: h.panel.top().map(|p| p.name.clone()),
                    panel_is_open: h.panel.open,
                    side_is_open: h.settings.side_open,
                    ..WindowState::default()
                }
            })
            .unwrap_or_default();
        state.surfaces = surfaces;
        state.width = f32::from(size.width);
        state.height = f32::from(size.height);
        state.scale = window.scale_factor();
        state
    }

    /// The window's pixels as it shows now ([`crate::capture`]).
    pub fn capture(
        &self,
        window: &gpui::Window,
        cx: &App,
    ) -> Result<crate::capture::Picture, Error> {
        let capture = self.read(cx, |h| h.capture).unwrap_or(crate::capture::none);
        capture(window)
    }

    /// The page the window shows.
    pub fn shown_page(&self, cx: &App) -> Option<&'static str> {
        self.read(cx, |h| h.shown()).flatten()
    }

    /// The width the side panel and the open panel take from the window
    /// while this page shows: what is left is the page's.
    pub fn side_room(&self, cx: &App) -> f32 {
        let page = self.page;
        self.read(cx, |h| {
            let side = if h.settings.side_open {
                crate::theme::size::SIDE
            } else {
                crate::theme::size::SIDE_SHUT
            };
            let panel = if h.panel.open && h.panel_on(page).is_some() {
                h.settings.panel_width
            } else {
                0.
            };
            side + panel
        })
        .unwrap_or(crate::theme::size::SIDE)
    }

    /// Shows this page.
    pub fn show(&self, cx: &mut App) {
        let page = self.page;
        self.update(cx, |_, cx| cx.emit(HubEvent::Show(page)));
    }
}
