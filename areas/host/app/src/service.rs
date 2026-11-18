//! One service: what state it is in and since when, where it answers, what
//! it is, its units, the one action its state wants (the rest under More)
//! and its log, newest first. A running service's lines from its earlier
//! runs wait folded under this run's.

use crate::page::HostPage;
use crate::services_store::{self as services, Change, Group, Log, Service};
use crate::tokens;
use ahjo_app_kit::{
    hub::now_ms,
    menu::MenuRow,
    theme::{Theme, size},
    widgets::{kit, pane},
};
use gpui::{ClipboardItem, Context, Div, SharedString, div, prelude::*, px};

impl HostPage {
    pub(crate) fn service(
        &self,
        t: &Theme,
        node: &str,
        s: &Service,
        cx: &mut Context<Self>,
    ) -> Div {
        let n = self.services.nodes.get(node).expect("listed");
        let now = now_ms();
        let mut head = pane::head(t)
            .child(pane::title(t, s.name.clone()))
            .child(pane::send(
                t,
                "svc-send",
                &self.host,
                services::reference(s, now),
            ));
        if services::opens(s) {
            let link = s.link.clone();
            head = head.child(
                pane::act(t, "svc-open", "Open")
                    .debug_selector(|| "svc-open".into())
                    .tooltip(kit::tip(s.link.clone()))
                    .on_click(move |_, _, cx| cx.open_url(&link)),
            );
        }
        // One action per state in the head; what else the state allows
        // waits under More.
        if s.controllable && !n.changing.contains_key(&s.id) {
            let mut offered = services::actions(&s.state, services::looping(s))
                .iter()
                .copied();
            if let Some(change) = offered.next() {
                let selector = format!("svc-{}-{node}-{}", change.verb(), s.id);
                let (at, id) = (node.to_owned(), s.id.clone());
                head = head.child(
                    pane::act(t, SharedString::from(selector.clone()), change.label())
                        .debug_selector(move || selector)
                        .on_click(cx.listener(move |p, _, _, cx| p.act(&at, &id, change, cx))),
                );
            }
            let more: Vec<Change> = offered.collect();
            if !more.is_empty() {
                let (at, id) = (node.to_owned(), s.id.clone());
                head = head.child(
                    pane::act(t, "svc-more", "…")
                        .debug_selector(|| "svc-more".into())
                        .on_click(cx.listener(move |p, e: &gpui::ClickEvent, _, cx| {
                            let me = cx.weak_entity();
                            let rows = more
                                .iter()
                                .map(|&change| {
                                    let (me, at, id) = (me.clone(), at.clone(), id.clone());
                                    MenuRow::new(change.label(), move |_, cx| {
                                        let _ = me.update(cx, |p, cx| p.act(&at, &id, change, cx));
                                    })
                                })
                                .collect();
                            p.host.menu(rows, e.position(), cx);
                        })),
                );
            }
        }

        let group = Group::of(&s.state);
        let mut state = n.shown(s).to_string();
        if let Some(since) = s.since() {
            state.push_str(&format!(" for {}", services::ago(since, now)));
        }
        if s.state == "failed" && services::looping(s) {
            state.push_str(": systemd keeps restarting it");
            if s.restarts() > 0 {
                state.push_str(&format!(", {} so far", times(s.restarts())));
            }
        } else if s.restarts() > 0 {
            state.push_str(&format!(", restarted {} by systemd", times(s.restarts())));
        }
        let state_selector = format!("svc-detail-state-{state}");
        let mut lines = div().flex().flex_col().child(pane::line(
            t,
            "State",
            div()
                .debug_selector(move || state_selector)
                .flex()
                .items_center()
                .gap(px(size::HEAD_GAP))
                .child(crate::view::glyph(t, &s.state))
                .child(
                    div()
                        .when(group == Group::Failed, |d| d.text_color(t.error))
                        .child(state),
                ),
        ));
        if let Some(failure) = n.failures.get(&s.id) {
            let selector = format!("svc-failure-{node}-{}", s.id);
            lines = lines.child(pane::line(
                t,
                "Last action",
                div()
                    .debug_selector(move || selector)
                    .text_color(t.error)
                    .child(failure.clone()),
            ));
        } else if let Some(note) = n.unsettled.get(&s.id) {
            lines = lines.child(pane::line(
                t,
                "Last action",
                div().text_color(t.text2).child(note.clone()),
            ));
        }
        if !s.link.is_empty() {
            let (open, copy) = (s.link.clone(), s.link.clone());
            lines = lines.child(pane::line(
                t,
                "Address",
                div()
                    .flex()
                    .items_center()
                    .gap(px(size::HEAD_GAP))
                    .child(
                        div()
                            .id("svc-address")
                            .debug_selector(|| "svc-address".into())
                            .min_w_0()
                            .child(s.link.clone())
                            // A link while something may answer there.
                            .map(|d| match services::opens(s) {
                                true => d
                                    .text_color(t.accent)
                                    .cursor_pointer()
                                    .hover(|d| d.underline())
                                    .on_click(move |_, _, cx| cx.open_url(&open)),
                                false => d.text_color(t.text2),
                            }),
                    )
                    .child(
                        pane::act(t, "svc-copy", "Copy")
                            .debug_selector(|| "svc-copy".into())
                            .on_click(cx.listener(move |p, _, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()));
                                p.host.toast("Address copied", cx);
                            })),
                    ),
            ));
        }
        if !s.description.is_empty() {
            lines = lines.child(pane::line(
                t,
                "What",
                div().text_color(t.text2).child(s.description.clone()),
            ));
        }
        if !s.components.is_empty() {
            let mut units = div().flex().flex_col();
            for c in &s.components {
                let state = if c.sub.is_empty() || c.sub == c.state {
                    c.state.clone()
                } else {
                    format!("{} ({})", c.state, c.sub)
                };
                units = units.child(
                    div()
                        .min_h(px(size::ROW))
                        .flex()
                        .items_center()
                        .gap(px(size::HEAD_GAP))
                        .child(
                            div()
                                .min_w_0()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .text_color(t.text2)
                                .child(c.unit.clone()),
                        )
                        .child(pane::meta(
                            t,
                            state,
                            (c.state == "failed").then_some(t.error),
                        )),
                );
            }
            let name = if s.components.len() == 1 {
                "Unit"
            } else {
                "Units"
            };
            lines = lines.child(pane::line(t, name, units));
        }
        if !s.controllable {
            lines = lines.child(pane::line(
                t,
                "Control",
                div()
                    .text_color(t.dim)
                    .child("Read-only: the catalog does not let Ahjo start or stop it"),
            ));
        }
        let key = (node.to_owned(), s.id.clone());
        let unfolded = self.earlier.contains(&key);
        let body = div()
            .flex()
            .flex_col()
            .gap(px(size::SECTION_GAP))
            .child(lines)
            .child(self.log(
                t,
                n.logs.get(&s.id),
                s.since().filter(|_| s.state == "running"),
                unfolded,
                key,
                cx,
            ));
        pane::detail(t, "services-detail", head, body)
    }

    /// The latest log lines, newest first, errors in the failed colour. A
    /// running service's lines from before this run (it began at
    /// `running_since`) wait folded under the run's own; a line from
    /// another day says its day.
    fn log(
        &self,
        t: &Theme,
        log: Option<&Log>,
        running_since: Option<u64>,
        unfolded: bool,
        key: (String, String),
        cx: &mut Context<Self>,
    ) -> Div {
        let mut section = pane::section(t, "Log, newest first");
        let Some(log) = log else {
            return section.child(div().text_color(t.dim).child("Reading the log…"));
        };
        for e in &log.errors {
            section = section.child(div().text_color(t.dim).child(e.clone()));
        }
        if log.lines.is_empty() {
            let words = if log.reading {
                "Reading the log…"
            } else {
                "Nothing logged."
            };
            if log.errors.is_empty() || log.reading {
                section = section.child(div().text_color(t.dim).child(words));
            }
            return section;
        }
        let now = now_ms();
        let earlier = services::earlier(&log.lines, running_since);
        let (before, run) = log.lines.split_at(earlier);
        let stamps = |lines: &[ahjo_host_contract::LogLine]| -> Vec<String> {
            lines
                .iter()
                .map(|l| ahjo_app_kit::clock::stamp(l.at, now))
                .collect()
        };
        // As wide as the stamps on show need: folded lines of another day
        // do not push this run's lines aside.
        let shown = if unfolded { &log.lines[..] } else { run };
        let time_col = match stamps(shown).iter().any(|s| s.len() > 5) {
            true => tokens::DAY_TIME_COL,
            false => tokens::TIME_COL,
        };
        let line = |l: &ahjo_host_contract::LogLine, stamp: String| {
            let selector = format!("svc-log-{}", l.text);
            div()
                .debug_selector(move || selector)
                .flex()
                .items_start()
                .gap(px(size::HEAD_GAP))
                .text_size(px(size::SMALL))
                .child(
                    div()
                        .flex_none()
                        .w(px(time_col))
                        .text_color(t.dim)
                        .child(stamp),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_color(if l.error { t.error } else { t.text2 })
                        .child(l.text.clone()),
                )
        };
        if run.is_empty() && earlier > 0 {
            section = section.child(
                pane::meta(t, "Nothing logged in this run yet.", None)
                    .debug_selector(|| "svc-log-this-run".into()),
            );
        }
        for (l, stamp) in run.iter().zip(stamps(run)).rev() {
            section = section.child(line(l, stamp));
        }
        if earlier == 0 {
            return section;
        }
        let words = match (unfolded, earlier) {
            (true, _) => "Earlier runs".to_string(),
            (false, 1) => "1 line from earlier runs".to_string(),
            (false, n) => format!("{n} lines from earlier runs"),
        };
        section = section.child(
            div().pt(px(size::LABEL_TOP)).flex().child(
                pane::act(t, "svc-log-earlier", words)
                    .debug_selector(|| "svc-log-earlier".into())
                    .px_0()
                    .tooltip(kit::tip(if unfolded { "Fold them" } else { "Show them" }))
                    .on_click(cx.listener(move |p, _, _, cx| {
                        if !p.earlier.remove(&key) {
                            p.earlier.insert(key.clone());
                        }
                        cx.notify();
                    })),
            ),
        );
        if unfolded {
            for (l, stamp) in before.iter().zip(stamps(before)).rev() {
                section = section.child(line(l, stamp));
            }
        }
        section
    }
}

/// A count of times in words: "once", "3 times".
fn times(n: u32) -> String {
    if n == 1 {
        "once".into()
    } else {
        format!("{n} times")
    }
}
