//! The page's list: each node's machine, with its verdict and its figures
//! now in a line, then its services in groups, what failed first. A
//! service's row says what it is and where it answers, and offers the one
//! action its state wants.

use crate::host_store::{self as store, Figure, percent};
use crate::page::{Chosen, HostPage};
use crate::services_store::{self as services, Group, NodeServices, Service};
use crate::tokens;
use ahjo_app_kit::{
    host::Node,
    hub::now_ms,
    theme::{self, Theme, size},
    widgets::{kit, pane},
};
use gpui::{AnyElement, Context, Div, KeyDownEvent, SharedString, Window, div, prelude::*, px};

/// A running service started less than this long ago says so in the list.
const LATELY: u64 = 3_600_000;

impl Render for HostPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme::theme(cx).clone();
        let nodes = self.host.nodes(cx);
        let chosen = self.chosen(cx);
        let several = nodes.len() > 1;
        let (mut rows, mut chosen_at) = (Vec::new(), None);
        for node in &nodes {
            self.node_rows(
                &t,
                node,
                several,
                chosen.as_ref(),
                &mut rows,
                &mut chosen_at,
                cx,
            );
        }
        // The row the keys reached stays in view.
        if std::mem::take(&mut self.reveal)
            && let Some(at) = chosen_at
        {
            self.list_scroll.scroll_to_item(at);
        }
        let rows = div()
            .id("host-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.list_scroll)
            .flex()
            .flex_col()
            .children(rows);
        let head = pane::list_head(&t, None).child(
            pane::act(&t, "services-refresh", "Refresh")
                .debug_selector(|| "services-refresh".into())
                .tooltip(kit::tip("Read the services again"))
                .on_click(cx.listener(|s, _, _, cx| s.read_all(cx))),
        );
        let list = pane::list_wide_frame(&t, head, rows);
        let node = chosen
            .as_ref()
            .and_then(|c| nodes.iter().find(|n| n.id.as_str() == c.node()));
        let detail = match (node, &chosen) {
            (Some(node), Some(Chosen::Machine(_))) => self.machine(&t, node, cx),
            (Some(node), Some(Chosen::Service(_, id))) => {
                match self.services.service(node.id.as_str(), id).cloned() {
                    Some(s) => self.service(&t, node.id.as_str(), &s, cx),
                    None => self.machine(&t, node, cx),
                }
            }
            // No node answers, in the words the other pages use.
            _ => pane::detail(
                &t,
                "host-detail",
                pane::head(&t),
                pane::vacant(&t, "Not connected to the node.", None),
            ),
        };
        div()
            .id("host")
            .size_full()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|s, e: &KeyDownEvent, _, cx| s.key(e, cx)))
            .child(pane::page(&t, list, detail).text_color(t.text))
    }
}

impl HostPage {
    /// One node's rows join `out`, each a child of the list so the keys
    /// can scroll to it; `chosen_at` is where the chosen row stands.
    #[allow(clippy::too_many_arguments)]
    fn node_rows(
        &self,
        t: &Theme,
        node: &Node,
        several: bool,
        chosen: Option<&Chosen>,
        out: &mut Vec<AnyElement>,
        chosen_at: &mut Option<usize>,
        cx: &mut Context<Self>,
    ) {
        let id = node.id.as_str();
        if several {
            out.push(
                div()
                    .flex_none()
                    .px(px(size::ROW_PAD))
                    .pt(px(size::LABEL_TOP))
                    .pb(px(size::LABEL_BOTTOM))
                    .child(kit::device_label(t, &node.device, &node.environment))
                    .into_any_element(),
            );
        }
        let machine = Chosen::Machine(id.to_owned());
        if chosen == Some(&machine) {
            *chosen_at = Some(out.len());
        }
        out.push(self.machine_row(t, node, chosen == Some(&machine), cx));
        let n = self.services.nodes.get(id);
        if let Some(how) = n.and_then(|n| n.no_catalog()) {
            let words = format!("No services are listed on this node. To show them, {how}.");
            out.push(
                div()
                    .debug_selector(|| "host-no-catalog".into())
                    .flex_none()
                    .px(px(size::ROW_PAD))
                    .py(px(size::LABEL_BOTTOM))
                    .text_size(px(size::SMALL))
                    .text_color(t.dim)
                    .whitespace_normal()
                    .child(words)
                    .into_any_element(),
            );
        }
        if let Some(e) = n.and_then(|n| n.unread()) {
            out.push(
                div()
                    .flex_none()
                    .px(px(size::ROW_PAD))
                    .py(px(size::LABEL_BOTTOM))
                    .child(pane::meta(
                        t,
                        format!("Services could not be read: {e}"),
                        Some(t.error),
                    ))
                    .into_any_element(),
            );
        }
        let Some(n) = n else { return };
        let now = now_ms();
        let sorted = self.sorted(id);
        let mut group = None;
        for service in &sorted {
            let g = Group::of(&service.state);
            if group != Some(g) {
                group = Some(g);
                let count = sorted.iter().filter(|s| Group::of(&s.state) == g).count();
                out.push(
                    pane::label(t, g.label())
                        .items_center()
                        .gap(px(size::HEAD_GAP))
                        .child(kit::count(t, count))
                        .into_any_element(),
                );
            }
            let this = Chosen::Service(id.to_owned(), service.id.clone());
            let on = chosen == Some(&this);
            if on {
                *chosen_at = Some(out.len());
            }
            out.push(self.service_row(t, id, service, n, on, now, cx));
        }
        if n.list.as_ref().is_some_and(Vec::is_empty) {
            out.push(
                div()
                    .flex_none()
                    .px(px(size::ROW_PAD))
                    .child(pane::meta(t, "The catalog lists no services.", None))
                    .into_any_element(),
            );
        }
    }

    /// The machine's row: its name and verdict, and every figure now.
    fn machine_row(&self, t: &Theme, node: &Node, on: bool, cx: &mut Context<Self>) -> AnyElement {
        let id = node.id.as_str();
        let state = self.hosts.nodes.get(id).map(|n| &n.state);
        let (verdict, colour) = match state {
            Some(s) if s.latest.is_some() => match self.trouble_count(id, s) {
                0 => ("all right".to_string(), None),
                n => (format!("{n} to look at"), Some(t.error)),
            },
            _ => ("waiting".to_string(), None),
        };
        let figures = state
            .and_then(|s| s.latest.as_ref())
            .map(|r| {
                store::bands(Some(r))
                    .into_iter()
                    .filter(|f| *f != Figure::Swap)
                    .map(|f| format!("{} {}", short(f), percent(f.now(r))))
                    .collect::<Vec<_>>()
                    .join("  ")
            })
            .unwrap_or_default();
        let selector = format!("host-row-{id}-machine");
        let verdict_selector = format!("host-verdict-{id}-{verdict}");
        let at = id.to_owned();
        pane::row(t, SharedString::from(selector.clone()), on)
            .debug_selector(move || selector)
            .h(px(tokens::SERVICE_ROW))
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
                            .child(pane::cell(title(node)))
                            .child(
                                pane::meta(t, verdict, colour)
                                    .debug_selector(move || verdict_selector),
                            ),
                    )
                    .child(second_line(t).text_color(t.dim).child(figures)),
            )
            .on_click(cx.listener(move |page, _, window, cx| {
                window.focus(&page.focus, cx);
                page.choose(Chosen::Machine(at.clone()), cx);
            }))
            .into_any_element()
    }

    /// A service's row: its mark and name, its state where it needs saying,
    /// what it is and where it answers, and the action its state wants
    /// (on show for one that failed or is chosen, else under the mouse).
    #[allow(clippy::too_many_arguments)]
    fn service_row(
        &self,
        t: &Theme,
        node: &str,
        s: &Service,
        n: &NodeServices,
        on: bool,
        now: u64,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let g = Group::of(&s.state);
        let changing = n.changing.get(&s.id).map(|c| c.word());
        let (meta, colour) = match (changing, g) {
            (Some(w), _) => (w.to_string(), None),
            (None, Group::Failed) => (services::state_word(s).to_string(), Some(t.error)),
            (None, Group::Running) => match s.since() {
                _ if s.state != "running" => (services::word(&s.state).to_string(), None),
                Some(since) if now.saturating_sub(since) < LATELY => {
                    (format!("up {}", services::ago(since, now)), None)
                }
                _ => (String::new(), None),
            },
            (None, Group::Stopped) => (String::new(), None),
        };
        let shown = n.shown(s).to_string();
        let state_selector = format!("svc-state-{node}-{}-{shown}", s.id);
        let selector = format!("svc-row-{node}-{}", s.id);
        let group = SharedString::from(format!("svc-group-{node}-{}", s.id));
        let mut words = second_line(t).gap(px(size::HEAD_GAP));
        words = words.child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .text_ellipsis()
                .text_color(t.dim)
                .child(s.description.clone()),
        );
        if !s.link.is_empty() {
            let link = s.link.clone();
            let opens = services::opens(s);
            let address = services::address(&s.link, tokens::ADDRESS_CHARS);
            let address_selector = format!("svc-row-address-{node}-{}", s.id);
            words = words.child(
                div()
                    .id(SharedString::from(address_selector.clone()))
                    .debug_selector(move || address_selector)
                    .flex_none()
                    .tooltip(kit::tip(if opens {
                        format!("Open {link}")
                    } else {
                        format!("{link}: nothing answers while it is {}", s.state)
                    }))
                    .child(address)
                    .map(|d| match opens {
                        true => d
                            .text_color(t.accent)
                            .cursor_pointer()
                            .hover(|d| d.underline())
                            .on_click(move |_, _, cx| {
                                cx.stop_propagation();
                                cx.open_url(&link)
                            }),
                        false => d.text_color(t.dim),
                    }),
            );
        }
        let (at, sid) = (node.to_owned(), s.id.clone());
        let mut row = pane::row(t, SharedString::from(selector.clone()), on)
            .debug_selector(move || selector)
            .group(group.clone())
            .h(px(tokens::SERVICE_ROW))
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
                            .child(glyph(t, &s.state))
                            .child(pane::cell(s.name.clone()))
                            .child(
                                div()
                                    .debug_selector(move || state_selector)
                                    .child(pane::meta(t, meta, colour)),
                            ),
                    )
                    .child(words.pl(px(tokens::SECOND_LINE_INDENT))),
            );
        if s.controllable
            && changing.is_none()
            && let Some(&change) = services::actions(&s.state, services::looping(s)).first()
        {
            let action = format!("svc-row-{}-{node}-{}", change.verb(), s.id);
            let (act_node, act_id) = (node.to_owned(), s.id.clone());
            let always = on || g == Group::Failed;
            row = row.child(
                pane::act(t, SharedString::from(action.clone()), change.label())
                    .debug_selector(move || action)
                    .when(!always, |b| {
                        b.opacity(0.).group_hover(group, |s| s.opacity(1.))
                    })
                    .on_click(cx.listener(move |page, _, _, cx| {
                        cx.stop_propagation();
                        page.act(&act_node, &act_id, change, cx)
                    })),
            );
        }
        row.on_click(cx.listener(move |page, _, window, cx| {
            window.focus(&page.focus, cx);
            page.choose(Chosen::Service(at.clone(), sid.clone()), cx);
        }))
        .into_any_element()
    }
}

/// A two-line row's second line: small, one line.
fn second_line(_: &Theme) -> Div {
    div()
        .flex()
        .items_center()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_size(px(size::SMALL))
}

/// A figure's name where a line has little room.
fn short(f: Figure) -> &'static str {
    match f {
        Figure::Cpu => "CPU",
        Figure::Memory => "mem",
        Figure::Swap => "swap",
        Figure::Gpu => "GPU",
        Figure::Vram => "VRAM",
        Figure::Disk(_) => "disk",
    }
}

pub fn title(node: &Node) -> String {
    if node.environment.is_empty() {
        node.device.clone()
    } else {
        format!("{} · {}", node.device, node.environment)
    }
}

/// A service's state mark: running filled, starting half there, stopped
/// an outline, failed filled in the failed colour.
pub fn glyph(t: &Theme, state: &str) -> Div {
    let dot = div()
        .size(px(tokens::GLYPH))
        .flex_none()
        .rounded_full()
        .overflow_hidden();
    match state {
        "running" => dot.bg(t.running),
        "starting" => dot.bg(theme::alpha(t.running, tokens::FADED)),
        "partial" => dot
            .border_1()
            .border_color(t.error)
            .child(div().w(px(tokens::GLYPH / 2.)).h_full().bg(t.error)),
        "failed" => dot.bg(t.error),
        "unknown" => dot.border_1().border_dashed().border_color(t.error),
        "stopping" => dot.border_1().border_color(t.running),
        _ => dot.border_1().border_color(t.dim),
    }
}
