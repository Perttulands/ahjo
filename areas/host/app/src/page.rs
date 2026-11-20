//! The Host page: is this machine all right now, with its services as part
//! of the answer. The list has each node's machine and its services, what
//! failed first; the detail has the machine (the verdict, every figure on
//! one time axis, its disks, what is heavy) or one service (its address,
//! units, one action for its state and its log, newest first).
//!
//! Readings come over the host channel while the page shows. Services are
//! read as the page comes forward, when a node comes into reach, on
//! Refresh, and after an action until it settles; nothing reads on a timer.

use crate::host_store::Hosts;
use crate::services_store::{self, Change, Ended, Group, Service, Services, Step};
use ahjo_app_kit::{
    Ask, Host, Page, ask::Answers, host::Status, hub::now_ms, link::client::LinkState,
};
use ahjo_core::{Error, NodeId, TypedCommand};
use ahjo_host_contract::commands::{
    ServiceActed, ServiceList, ServiceLog, ServiceRestart, ServiceShow, ServiceStart, ServiceStop,
};
use ahjo_host_contract::{HOST, HOST_EVENT, HOST_STATE};
use ahjo_terminals_contract::SessionList;
use gpui::{App, Context, Entity, FocusHandle, KeyDownEvent, ScrollHandle, Window};
use std::collections::HashSet;
use std::time::Duration;

/// What the detail shows: a node's machine, or one of its services.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Chosen {
    Machine(String),
    Service(String, String),
}

impl Chosen {
    pub fn node(&self) -> &str {
        match self {
            Chosen::Machine(n) | Chosen::Service(n, _) => n,
        }
    }
}

pub struct HostPage {
    pub(crate) host: Host,
    pub(crate) hosts: Hosts,
    pub(crate) services: Services,
    chosen: Option<Chosen>,
    /// The moment under the mouse on the machine's figures.
    pub(crate) hover: Option<u64>,
    /// The machine's chart's width as last drawn, which says how many of
    /// its time labels fit.
    pub(crate) chart_w: Option<f32>,
    /// The width the figures and their chart share, as last drawn, which
    /// says whether the figures stand beside the chart or above it.
    pub(crate) figures_w: std::rc::Rc<std::cell::Cell<Option<f32>>>,
    pub(crate) shown: bool,
    pub(crate) sessions: Option<Entity<SessionList>>,
    pub(crate) focus: FocusHandle,
    /// The list's scroll, and whether the row the keys reached is to be
    /// brought into view at the next draw.
    pub(crate) list_scroll: ScrollHandle,
    pub(crate) reveal: bool,
    /// Services whose log shows its earlier runs too.
    pub(crate) earlier: HashSet<(String, String)>,
}

impl HostPage {
    pub fn new(host: Host, _: &mut Window, cx: &mut Context<Self>) -> Self {
        let sessions = host.model::<SessionList>(cx);
        if let Some(s) = &sessions {
            cx.observe(s, |page, _, cx| {
                if page.shown {
                    cx.notify()
                }
            })
            .detach();
        }
        Self {
            host,
            hosts: Hosts::default(),
            services: Services::default(),
            chosen: None,
            hover: None,
            chart_w: None,
            figures_w: Default::default(),
            shown: false,
            sessions,
            focus: cx.focus_handle(),
            list_scroll: ScrollHandle::new(),
            reveal: false,
            earlier: HashSet::new(),
        }
    }

    /// What the detail shows: the one chosen while its node is here, or
    /// else the first node's machine.
    pub(crate) fn chosen(&self, cx: &App) -> Option<Chosen> {
        let nodes = self.host.nodes(cx);
        self.chosen
            .clone()
            .filter(|c| nodes.iter().any(|n| n.id.as_str() == c.node()))
            .or_else(|| nodes.first().map(|n| Chosen::Machine(n.id.to_string())))
    }

    pub(crate) fn choose(&mut self, chosen: Chosen, cx: &mut Context<Self>) {
        if self.chosen.as_ref() != Some(&chosen) {
            self.hover = None;
            if let Chosen::Service(node, id) = &chosen {
                self.read_log(node, id, cx);
            }
            self.chosen = Some(chosen);
        }
        cx.notify();
    }

    fn ask<C: TypedCommand>(
        &mut self,
        node: NodeId,
        input: C,
        done: impl FnOnce(&mut Self, Result<C::Output, Error>, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let asked = self.host.call::<C>(node, input, cx);
        cx.spawn(async move |this, cx| {
            let answer = asked.await;
            let _ = this.update(cx, |s, cx| done(s, answer, cx));
        })
        .detach();
    }

    /// Reads every node's services, and the chosen service's log.
    pub(crate) fn read_all(&mut self, cx: &mut Context<Self>) {
        for node in self.host.nodes(cx) {
            self.read(node.id.as_str(), cx);
        }
        if let Some(Chosen::Service(node, id)) = self.chosen.clone() {
            self.read_log(&node, &id, cx);
        }
    }

    fn read(&mut self, node: &str, cx: &mut Context<Self>) {
        let state = self.services.nodes.entry(node.to_string()).or_default();
        if state.reading {
            state.reread = true;
            return;
        }
        state.reading = true;
        let at = node.to_owned();
        self.ask(
            NodeId(at.clone()),
            ServiceList {},
            move |s, answer, cx| {
                let state = s.services.nodes.entry(at.clone()).or_default();
                state.listed(answer, now_ms());
                if std::mem::take(&mut state.reread) {
                    s.read(&at, cx);
                }
                cx.notify();
            },
            cx,
        );
    }

    /// Reads a service's latest log lines.
    fn read_log(&mut self, node: &str, id: &str, cx: &mut Context<Self>) {
        let n = self.services.nodes.entry(node.to_owned()).or_default();
        let log = n.logs.entry(id.to_owned()).or_default();
        if log.reading {
            return;
        }
        log.reading = true;
        let (node, id) = (node.to_owned(), id.to_owned());
        self.ask(
            NodeId(node.clone()),
            ServiceLog {
                id: id.clone(),
                lines: Some(crate::tokens::LOG_LINES),
            },
            move |s, answer, cx| {
                s.services
                    .nodes
                    .entry(node)
                    .or_default()
                    .logs
                    .entry(id)
                    .or_default()
                    .read(answer);
                cx.notify();
            },
            cx,
        );
    }

    pub(crate) fn act(&mut self, node: &str, id: &str, change: Change, cx: &mut Context<Self>) {
        let n = self.services.nodes.entry(node.to_owned()).or_default();
        if n.changing.contains_key(id) {
            return;
        }
        n.changing.insert(id.to_owned(), change);
        n.failures.remove(id);
        n.unsettled.remove(id);
        let (at, row) = (node.to_owned(), id.to_owned());
        let node_id = NodeId(at.clone());
        let asked = match change {
            Change::Start => {
                self.host
                    .call::<ServiceStart>(node_id, ServiceStart { id: row.clone() }, cx)
            }
            Change::Stop => {
                self.host
                    .call::<ServiceStop>(node_id, ServiceStop { id: row.clone() }, cx)
            }
            Change::Restart => {
                self.host
                    .call::<ServiceRestart>(node_id, ServiceRestart { id: row.clone() }, cx)
            }
        };
        cx.spawn(async move |this, cx| {
            let answer: Result<ServiceActed, Error> = asked.await;
            let _ = this.update(cx, |s, cx| {
                let n = s.services.nodes.entry(at.clone()).or_default();
                match n.answered(&row, change, answer) {
                    Ended::Failed(message) => {
                        s.host.report(Status::failure(message), cx);
                        // The failed command carries its reason. Refresh the final state.
                        s.read(&at, cx);
                    }
                    // The node's event says it on the status line.
                    Ended::Done | Ended::Elsewhere => {}
                    Ended::Follow => s.follow(at.clone(), row.clone(), 0, cx),
                }
                s.read_log(&at, &row, cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Reads one service again after an action whose end is not known,
    /// further apart each time, until it settles.
    fn follow(&mut self, node: String, id: String, read: usize, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(services_store::FOLLOW[read]))
                .await;
            let _ = this.update(cx, |s, cx| {
                let at = node.clone();
                s.ask(
                    NodeId(node),
                    ServiceShow { id: id.clone() },
                    move |s, answer, cx| {
                        let last = read + 1 == services_store::FOLLOW.len();
                        let clock = ahjo_app_kit::clock::hhmm(now_ms());
                        if s.services
                            .nodes
                            .entry(at.clone())
                            .or_default()
                            .followed(&id, answer, last, &clock)
                        {
                            s.follow(at, id, read + 1, cx);
                        } else {
                            s.read_log(&at, &id, cx);
                        }
                        cx.notify();
                    },
                    cx,
                );
            });
        })
        .detach();
    }

    /// A node's services in the list's order: failed, running, stopped,
    /// each by name.
    pub(crate) fn sorted(&self, node: &str) -> Vec<&Service> {
        let mut list: Vec<&Service> = self
            .services
            .nodes
            .get(node)
            .and_then(|n| n.list.as_ref())
            .into_iter()
            .flatten()
            .collect();
        list.sort_by_key(|s| (Group::of(&s.state), s.name.to_lowercase()));
        list
    }

    /// Every row, in the list's order: each node's machine, then its
    /// services.
    fn order(&self, cx: &App) -> Vec<Chosen> {
        let mut out = vec![];
        for node in self.host.nodes(cx) {
            let id = node.id.to_string();
            out.push(Chosen::Machine(id.clone()));
            out.extend(
                self.sorted(&id)
                    .into_iter()
                    .map(|s| Chosen::Service(id.clone(), s.id.clone())),
            );
        }
        out
    }

    pub(crate) fn key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        if event.keystroke.modifiers.modified() {
            return;
        }
        let step = match event.keystroke.key.as_str() {
            "down" => Some(Step::Down),
            "up" => Some(Step::Up),
            "home" => Some(Step::Home),
            "end" => Some(Step::End),
            _ => None,
        };
        if let Some(step) = step {
            let order = self.order(cx);
            let at = self.chosen(cx);
            if let Some(to) = services_store::step(&order, at.as_ref(), step) {
                self.reveal = true;
                self.choose(to, cx);
            }
        } else if event.keystroke.key == "enter" {
            let Some(Chosen::Service(node, id)) = self.chosen(cx) else {
                return;
            };
            match self.services.service(&node, &id) {
                Some(s) if services_store::opens(s) => cx.open_url(&s.link),
                _ => return,
            }
        } else {
            return;
        }
        cx.stop_propagation();
        cx.notify();
    }
}

impl Page for HostPage {
    const ID: &'static str = "host";
    const TITLE: &'static str = "Host";
    const KEY: Option<char> = Some('H');
    const AREA: Option<&'static str> = Some("host");
    const KINDS: ahjo_core::frame::Kinds = &[HOST_STATE..=HOST_EVENT];
    const CHANNELS: &'static [u8] = &[HOST];
    const ANSWERS: &'static [Answers] = &[Answers::Reveal("service")];

    fn focus(&self, _: &App) -> Option<FocusHandle> {
        Some(self.focus.clone())
    }

    fn shown(&mut self, shown: bool, _: &mut Window, cx: &mut Context<Self>) {
        self.shown = shown;
        if shown {
            self.read_all(cx);
        }
    }

    fn answer(&mut self, ask: Ask, _: &'static str, _: &mut Window, cx: &mut Context<Self>) {
        let Ask::Reveal { id, node, .. } = ask else {
            return;
        };
        let node = node.map(|n| n.to_string()).or_else(|| {
            self.services
                .nodes
                .iter()
                .find(|(_, n)| n.row(&id).is_some())
                .map(|(n, _)| n.clone())
        });
        match node {
            Some(node) => {
                self.reveal = true;
                self.choose(Chosen::Service(node, id), cx);
            }
            None => self
                .host
                .report(Status::failure(format!("No service {id} is listed")), cx),
        }
    }

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

    fn frame(
        &mut self,
        node: &NodeId,
        kind: u8,
        body: &[u8],
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(event) = self.hosts.take(node.as_str(), kind, body) else {
            return;
        };
        if let Some(e) = event
            && self.shown
        {
            // A service someone acted on, here or from the command line.
            if e.kind == "service" {
                self.read(node.as_str(), cx);
            }
            let status = if e.failure {
                Status::failure(e.text)
            } else {
                Status::new(e.text)
            };
            self.host.report(status, cx);
        }
        if self.shown {
            cx.notify();
        }
    }
}
