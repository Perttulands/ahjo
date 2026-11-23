//! Harness: whether the harnesses can run with their skills current, and
//! what to do where they cannot, across nodes.
mod agents_page;
mod request;
pub mod shape_state;
mod shape_view;
mod skills_view;
pub mod state;
mod view;
pub use agents_page::build as build_agents;
mod tokens {
    /// The most an off line's fix takes; its words wrap within it.
    pub const FIX_W: f32 = 220.;
    /// Above and below an off line, which may wrap.
    pub const OFF_PAD: f32 = 6.;
    /// A sync on its way.
    pub const DIMMED: f32 = 0.5;
    pub const P4: f32 = 4.;
    /// The Agent view: the anatomy's share of the detail and its least,
    /// and the least the reader beside it keeps. A detail narrower than
    /// both shows one of them at a time.
    pub const ANATOMY: f32 = 0.55;
    pub const ANATOMY_MIN: f32 = 340.;
    pub const READER_MIN: f32 = 320.;
    /// An instruction's origin, its size bar and its size.
    pub const ORIGIN_W: f32 = 64.;
    pub const BAR_W: f32 = 72.;
    pub const BAR_H: f32 = 4.;
    pub const SIZE_W: f32 = 64.;
    /// A skill's name and a setting's key, at most.
    pub const SKILL_W: f32 = 180.;
    pub const KEY_W: f32 = 220.;
}
use ahjo_app_kit::frame::Status;
use ahjo_app_kit::link::client::LinkState;
use ahjo_app_kit::theme::Theme;
use ahjo_app_kit::widgets::kit::{self, Kind};
use ahjo_app_kit::{Host, Mounted, Page, mount};
use ahjo_core::NodeId;
use ahjo_terminals_contract::SessionList;
use gpui::{App, Context, Entity, IntoElement, Render, SharedString, Window, div, prelude::*};
use state::{HarnessState, Synced, WATCH};
use std::collections::BTreeMap;

/// The page's views: what an agent gets and the skills.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Sub {
    #[default]
    Agent,
    Skills,
}

pub struct HarnessPage {
    host: Host,
    sub: Sub,
    state: HarnessState,
    places: BTreeMap<String, String>,
    shape: shape_state::ShapeView,
    kept: shape_state::Kept,
    inputs: shape_view::Inputs,
    sessions: Option<Entity<SessionList>>,
    catalog: skills_view::Catalog,
    /// The recent folder is opened once a node answers.
    started: bool,
}
impl HarnessPage {
    fn new(host: Host, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let kept: shape_state::Kept = host.setting(cx).unwrap_or_default();
        let sessions = host.model::<SessionList>(cx);
        if let Some(sessions) = &sessions {
            cx.observe(sessions, |_, _, cx| cx.notify()).detach();
        }
        let shape = shape_state::ShapeView {
            harness: if kept.harness.is_empty() {
                "claude".into()
            } else {
                kept.harness.clone()
            },
            ..Default::default()
        };
        Self {
            inputs: shape_view::Inputs::new(window, cx),
            host,
            sub: Sub::Agent,
            state: HarnessState::default(),
            places: BTreeMap::new(),
            shape,
            kept,
            sessions,
            catalog: skills_view::Catalog::default(),
            started: false,
        }
    }
    /// The switch between the page's views, at the head of its list.
    fn sub_switch(&self, t: &Theme, cx: &mut Context<Self>) -> gpui::Div {
        let mut row = div().flex().items_center();
        for (sub, name, id) in [
            (Sub::Agent, "Agent", "harness-sub-agent"),
            (Sub::Skills, "Skills", "harness-sub-skills"),
        ] {
            let on = self.sub == sub;
            row = row.child(
                kit::button(t, SharedString::from(id), name, Kind::Quiet)
                    .debug_selector(move || id.into())
                    .when(on, |b| b.text_color(t.accent))
                    .on_click(cx.listener(move |s, _, _, cx| s.show_sub(sub, cx))),
            );
        }
        row
    }
    fn show_sub(&mut self, sub: Sub, cx: &mut Context<Self>) {
        if self.shape.editing && self.sub != sub {
            self.host.report(
                Status::failure("Save or discard the open file first (Ctrl+S, Escape)"),
                cx,
            );
            return;
        }
        self.sub = sub;
        match sub {
            Sub::Skills => {
                self.catalog_load(cx);
                if let Some(path) = self.catalog.chosen.clone() {
                    self.catalog_open(path, cx);
                }
            }
            Sub::Agent => {
                if let shape_state::Pick::File(path) = self.shape.pick.clone() {
                    self.file_read(path, cx);
                }
            }
        }
        cx.notify();
    }
    fn place_name(&self, node: &str) -> String {
        self.places
            .get(node)
            .cloned()
            .unwrap_or_else(|| node.to_owned())
    }
    fn harness_reports(&self) -> Vec<(&str, &state::Report)> {
        self.state
            .reports
            .iter()
            .map(|(n, r)| (n.as_str(), r))
            .collect()
    }
    fn harness_refresh(&mut self, cx: &mut Context<Self>) {
        for node in self.host.nodes(cx) {
            self.host.send(&node.id, vec![WATCH, 1], cx);
        }
    }
    fn harness_sync(&mut self, node: &str, checkout: &str, cx: &mut Context<Self>) {
        let frame = self.state.sync(node, checkout);
        if !self.host.send(&NodeId::from(node), frame, cx) {
            self.state.dropped(node);
            self.host.report(
                Status::failure(format!("Sync of {checkout} did not start: not connected")),
                cx,
            );
        }
        cx.notify();
    }
}
impl HarnessPage {
    fn harness_link(&mut self, node: &str, checkout: &str, cx: &mut Context<Self>) {
        let frame = self.state.link(node, checkout);
        if !self.host.send(&NodeId::from(node), frame, cx) {
            self.state.dropped(node);
            self.host.report(
                Status::failure(format!("{checkout} not linked: not connected")),
                cx,
            );
        }
        cx.notify();
    }
}
impl Page for HarnessPage {
    const ID: &'static str = "harness";
    const TITLE: &'static str = "Harness";
    const KEY: Option<char> = Some('A');
    const AREA: Option<&'static str> = Some("harness");
    const KINDS: ahjo_core::frame::Kinds = &[0x80..=0x8f];
    const CHANNELS: &'static [u8] = &[WATCH];
    /// The harness setup for a session: a folder (`id`) on a node, by a harness
    /// (`part`, "claude" or "codex", else the one shown).
    const ANSWERS: &'static [ahjo_app_kit::ask::Answers] =
        &[ahjo_app_kit::ask::Answers::Reveal("agent")];
    fn answer(
        &mut self,
        ask: ahjo_app_kit::Ask,
        _: &'static str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ahjo_app_kit::Ask::Reveal { id, node, part, .. } = ask else {
            return;
        };
        self.started = true;
        self.show_sub(Sub::Agent, cx);
        if self.sub == Sub::Agent {
            self.shape_open(node.map(|n| n.to_string()), id, part, cx);
        }
    }
    fn frame(
        &mut self,
        node: &NodeId,
        kind: u8,
        body: &[u8],
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.state.frame(node.as_str(), kind, body) {
            Some(Some(Synced::Started { checkout, session })) => self.host.toast(
                &format!("Syncing {checkout} in {session}; refresh when done"),
                cx,
            ),
            Some(Some(Synced::Linked { checkout, links })) => self.host.toast(
                &format!(
                    "Linked {links} skill{} from {checkout}",
                    if links == 1 { "" } else { "s" }
                ),
                cx,
            ),
            Some(Some(Synced::Refused { code, .. }))
                if code == ahjo_harness_contract::FETCH_FAILED => {}
            Some(Some(Synced::Refused { checkout, why, .. })) => self
                .host
                .report(Status::failure(format!("{checkout}: {why}")), cx),
            _ => {}
        }
        cx.notify();
    }
    fn link(
        &mut self,
        node: &NodeId,
        state: Option<LinkState>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if state != Some(LinkState::Open) {
            let pending = self.state.dropped(node.as_str());
            if !pending.is_empty() {
                self.host.report(
                    Status::failure(format!(
                        "Outcome unknown after disconnect: {}",
                        pending.join(", ")
                    )),
                    cx,
                );
            }
        }
    }
}
impl Render for HarnessPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.places = self
            .host
            .nodes(cx)
            .into_iter()
            .map(|n| {
                (
                    n.id.to_string(),
                    format!("{} · {}", n.device, n.environment),
                )
            })
            .collect();
        if !self.started && !self.places.is_empty() {
            self.started = true;
            // The folder looked at last; on first use a running session's,
            // else the home folder: what every agent here starts with.
            let dir = self
                .kept
                .recent
                .first()
                .cloned()
                .or_else(|| self.running(cx).into_iter().next().map(|(d, ..)| d))
                .unwrap_or_else(|| "~".into());
            self.shape_open(None, dir, None, cx);
        }
        match self.sub {
            Sub::Agent => self.render_shape(window, cx),
            Sub::Skills => self.render_agents(window, cx),
        }
    }
}
pub fn build(host: Host, window: &mut Window, cx: &mut App) -> Mounted {
    mount(host, window, cx, HarnessPage::new)
}
