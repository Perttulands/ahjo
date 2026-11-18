//! The machine: is it all right now. The verdict and what is wrong; every
//! figure as a band on one time axis, each with its number now, its whole,
//! its peak and its average, and the moment under the mouse read in all of
//! them at once; the disks as bars; what is heavy while something is; and
//! what happened today.

use crate::chart::{self, Band, Stack};
use crate::host_store::{
    self as store, DAY, Figure, HOUR, State, Trouble, average, bands, peak, percent,
};
use crate::page::{Chosen, HostPage};
use crate::services_store::{self as services, Service};
use crate::tokens;
use crate::view::title;
use ahjo_app_kit::{
    clock::hhmm,
    host::Node,
    hub::now_ms,
    theme::{Theme, size},
    widgets::{
        kit,
        pane::{self, Col},
    },
};
use ahjo_host_contract::MINUTE;
use gpui::{Context, Div, Hsla, SharedString, div, prelude::*, px, relative};

/// What is heavy: the program, where it runs, its CPU and memory.
const HEAVY: [Col; 4] = [
    Col::fill("Program"),
    Col::fill("Where"),
    Col::fixed("CPU", tokens::FIGURE_COL),
    Col::fixed("Memory", tokens::FIGURE_COL),
];
const EVENTS: [Col; 2] = [Col::fixed("At", tokens::TIME_COL), Col::fill("What")];
/// Events the machine shows: today's, newest first.
const EVENTS_SHOWN: usize = 5;

fn local_offset() -> i64 {
    time::UtcOffset::current_local_offset()
        .unwrap_or(time::UtcOffset::UTC)
        .whole_seconds()
        .into()
}

impl HostPage {
    /// A node's services that need the operator: those the list shows
    /// under Failed.
    pub(crate) fn failed(&self, node: &str) -> Vec<&Service> {
        self.sorted(node)
            .into_iter()
            .filter(|s| services::needs_you(s))
            .collect()
    }

    /// How many things on a node need looking at.
    pub(crate) fn trouble_count(&self, node: &str, s: &State) -> usize {
        store::troubles(s, now_ms()).len()
            + self.failed(node).len()
            + usize::from(self.unread(node).is_some())
    }

    /// Why a node's services could not be read, when that is a failure.
    fn unread(&self, node: &str) -> Option<&str> {
        self.services.nodes.get(node)?.unread()
    }

    pub(crate) fn machine(&self, t: &Theme, node: &Node, cx: &mut Context<Self>) -> Div {
        let id = node.id.as_str();
        let state = self.hosts.nodes.get(id).map(|n| &n.state);
        let head = pane::head(t).px(px(size::PAD));
        let Some(s) = state.filter(|s| s.latest.is_some()) else {
            return frame(
                head.child(pane::title(t, title(node))),
                pane::empty(t, "Waiting for the first reading…", None).p_0(),
            );
        };
        let now = now_ms();
        let reference = store::reference(&node.device, &node.environment, s, now);
        let head = head.child(pane::title(t, title(node))).child(pane::send(
            t,
            "host-send",
            &self.host,
            reference,
        ));
        let troubles = store::troubles(s, now);
        let mut body = div()
            .flex()
            .flex_col()
            .gap(px(size::SECTION_GAP))
            .child(self.verdict(t, id, s, &troubles, cx))
            .child(self.figures(t, s, &troubles, cx));
        if let Some(disks) = disks(t, s, &troubles) {
            body = body.child(disks);
        }
        // Named only while something weighs on the machine.
        let weighing = store::weighing(s.latest.as_ref().expect("a reading"));
        if !weighing.is_empty() {
            body = body.child(self.heavy(t, id, s, &weighing, cx));
        }
        if let Some(today) = today(t, s, now) {
            body = body.child(today);
        }
        frame(head, body)
    }

    /// All right, or what is wrong: each trouble in the failed colour, a
    /// failed service a click from its log.
    fn verdict(
        &self,
        t: &Theme,
        id: &str,
        s: &State,
        troubles: &[Trouble],
        cx: &mut Context<Self>,
    ) -> Div {
        let r = s.latest.as_ref().expect("a reading");
        let failed = self.failed(id);
        let unread = self
            .unread(id)
            .map(|e| format!("Services could not be read: {e}"));
        let n = troubles.len() + failed.len() + usize::from(unread.is_some());
        let mut facts = vec![];
        if let Some(up) = r.uptime {
            facts.push(format!("Up {}", store::uptime(up)));
        }
        let listed = self.sorted(id);
        if !listed.is_empty() {
            let running = listed.iter().filter(|s| s.state == "running").count();
            facts.push(format!("{running} of {} services running", listed.len()));
        }
        // The headline names what to look at; the lines under it say why.
        let mut names: Vec<String> = troubles
            .iter()
            .map(|x| match x.figure {
                Some(f) => f.name(Some(r)),
                None => x.text.clone(),
            })
            .collect();
        if unread.is_some() {
            names.push("the services".into());
        }
        names.extend(failed.iter().map(|s| s.name.clone()));
        names.dedup();
        let (words, colour) = match n {
            0 => ("All right".to_string(), t.text),
            _ => (format!("Look at {}", store::listed(&names, 3)), t.error),
        };
        let mut out = div().flex().flex_col().gap(px(size::LABEL_BOTTOM)).child(
            div()
                .debug_selector(move || format!("host-verdict-{n}"))
                .flex()
                .flex_wrap()
                .items_baseline()
                .gap_x(px(size::HEAD_GAP * 2.))
                .child(kit::page_title(t, words).text_color(colour))
                .child(div().text_color(t.text2).child(facts.join(" · "))),
        );
        for trouble in troubles {
            let selector = format!("host-trouble-{}", trouble.text);
            out = out.child(
                div()
                    .debug_selector(move || selector)
                    .min_h(px(size::ROW))
                    .flex()
                    .items_center()
                    .text_color(t.error)
                    .child(trouble.text.clone()),
            );
        }
        if let Some(text) = unread {
            let selector = format!("host-trouble-{text}");
            out = out.child(
                // The reason names a path and a parse error: it wraps.
                div()
                    .debug_selector(move || selector)
                    .min_h(px(size::ROW))
                    .min_w_0()
                    .whitespace_normal()
                    .text_color(t.error)
                    .child(text),
            );
        }
        for service in failed {
            let text = match services::unknown_why(service) {
                Some(why) => format!("{}: {}", service.name, lower_first(&why)),
                None => format!("Service {} {}", service.name, services::state_word(service)),
            };
            let selector = format!("host-trouble-{text}");
            let chosen = Chosen::Service(id.to_owned(), service.id.clone());
            out = out.child(
                pane::row(t, SharedString::from(selector.clone()), false)
                    .mx_0()
                    .px_0()
                    .debug_selector(move || selector)
                    .gap(px(size::HEAD_GAP))
                    .text_color(t.error)
                    .child(div().flex_none().child(text))
                    .child(pane::meta(t, "Show it", Some(t.text2)))
                    .on_click(cx.listener(move |page, _, _, cx| {
                        page.reveal = true;
                        page.choose(chosen.clone(), cx)
                    })),
            );
        }
        out
    }

    /// Every figure over the same span, one band each under one axis.
    fn figures(&self, t: &Theme, s: &State, troubles: &[Trouble], cx: &mut Context<Self>) -> Div {
        let r = s.latest.as_ref().expect("a reading");
        let end = r.at;
        let oldest = s.minutes.first().map_or(end, |p| p.at);
        let start = oldest
            .max(end.saturating_sub(DAY))
            .min(end.saturating_sub(HOUR));
        let figures = bands(Some(r));
        let ticks = store::ticks(start, end, local_offset());
        // Where the room is too small for the legends and a chart worth
        // reading side by side, the legends stand above the chart, side by
        // side, and the chart takes the whole width, each band named.
        let above = self
            .figures_w
            .get()
            .is_some_and(|w| w < tokens::LEGEND_W + size::SECTION_GAP + tokens::CHART_MIN);
        let mut legends = if above {
            div()
                .w_full()
                .flex()
                .flex_wrap()
                .gap_x(px(size::SECTION_GAP))
                .gap_y(px(tokens::BAND_GAP))
        } else {
            // As wide as its longest line (a card's whole name), so
            // nothing is cut beside a chart that has the room.
            div()
                .min_w(px(tokens::LEGEND_W))
                .flex_none()
                .flex()
                .flex_col()
                .gap(px(tokens::BAND_GAP))
        };
        let mut scales = div().absolute().inset_0();
        let mut drawn = vec![];
        for (i, figure) in figures.iter().copied().enumerate() {
            let points = store::line(&s.minutes, figure, start);
            let top = store::ceiling(figure, &points);
            let warn = store::warns(troubles, figure);
            let ink = if warn { t.error } else { t.chart };
            let legend = self.legend(t, s, figure, &points, warn);
            legends = legends.child(if above {
                legend.h_auto().flex_1().min_w(px(tokens::LEGEND_MIN))
            } else {
                legend
            });
            scales = scales.child(
                div()
                    .absolute()
                    .top(px(
                        i as f32 * (tokens::BAND_H + tokens::BAND_GAP) + size::LABEL_BOTTOM
                    ))
                    .left(px(size::LABEL_BOTTOM))
                    .text_size(px(size::SMALL))
                    .text_color(t.dim)
                    .child(if above {
                        format!("{} · {top:.0}%", figure.name(Some(r)))
                    } else {
                        format!("{top:.0}%")
                    }),
            );
            drawn.push(Band {
                runs: store::runs(&points),
                top,
                ink,
            });
        }
        let stack = Stack {
            bands: drawn,
            start,
            end,
            ticks: ticks.clone(),
            hover: self.hover,
            known_w: self.chart_w,
            grid: t.divider,
            hair: ahjo_app_kit::theme::alpha(t.text2, tokens::LINE_ALPHA),
        };
        let width = end.saturating_sub(start).max(1) as f32;
        let place = |at: u64| relative(at.saturating_sub(start) as f32 / width);
        let mut axis = div().relative().w_full().h(px(tokens::AXIS_H));
        // The moment under the mouse takes the place of the ticks near it.
        let room = (end - start) / (ticks.len() as u64 + 1) / 2;
        let hovered = self.hover.filter(|at| *at >= start);
        // Only as many labels as stand apart at the chart's width.
        let labelled = match self.chart_w {
            Some(w) => store::labelled(&ticks, start, end, w, tokens::TICK_W * 1.25),
            None => ticks,
        };
        for at in labelled {
            if hovered.is_some_and(|h| h.abs_diff(at) < room) {
                continue;
            }
            axis = axis.child(tick(t, place(at), hhmm(at), t.dim));
        }
        if let Some(at) = hovered {
            axis = axis.child(
                tick(t, place(at), hhmm(at), t.text)
                    .bg(t.bg)
                    .debug_selector(|| "host-hover".into()),
            );
        }
        let hours = (end - start + HOUR / 2) / HOUR;
        let chart = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .debug_selector(|| "host-chart".into())
                    .relative()
                    .w_full()
                    .h(px(chart::height(figures.len())))
                    .child(chart::stack(stack, cx.entity().downgrade()))
                    .child(scales),
            )
            .child(axis);
        // Its width as drawn decides the next frame's arrangement; a new
        // one draws again at once.
        let known = self.figures_w.clone();
        let page = cx.entity().downgrade();
        let measure = gpui::canvas(
            move |b, _, cx| {
                let w = Some(f32::from(b.size.width));
                if known.replace(w) != w {
                    cx.defer(move |cx| {
                        let _ = page.update(cx, |_, cx| cx.notify());
                    });
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();
        pane::section(t, format!("Last {hours} h")).child(
            div()
                .relative()
                .flex()
                .when(above, |d| d.flex_col())
                .gap(px(size::SECTION_GAP))
                .child(measure)
                .child(legends)
                .child(chart),
        )
    }

    /// A band's legend: the figure's name, its number now (or under the
    /// mouse), what it is of, and its peak and average over the span.
    fn legend(
        &self,
        t: &Theme,
        s: &State,
        figure: Figure,
        points: &[(u64, f32)],
        warn: bool,
    ) -> Div {
        let r = s.latest.as_ref().expect("a reading");
        let key = key(figure);
        let hovered = self.hover.map(|at| {
            points
                .iter()
                .min_by_key(|(p, _)| p.abs_diff(at))
                .filter(|(p, _)| p.abs_diff(at) <= 2 * MINUTE)
                .map(|(p, v)| (*p, *v))
        });
        let (value, whole) = match hovered {
            // The mouse is over the axis: the figure then.
            Some(Some((at, v))) => {
                let of = figure
                    .whole(r)
                    .map(|w| format!("{} at {}", store::bytes(store::share_of(v, w)), hhmm(at)))
                    .unwrap_or_else(|| format!("at {}", hhmm(at)));
                (Some(v), of)
            }
            Some(None) => (None, "nothing read then".to_string()),
            None => (figure.now(r), whole_now(figure, s)),
        };
        let now = percent(value);
        let (number, unit) = match now.strip_suffix('%') {
            Some(v) => (v.to_string(), "%"),
            None => (now.clone(), ""),
        };
        let colour = (warn && hovered.is_none()).then_some(t.error);
        let mut spread = vec![];
        if let Some((at, v)) = peak(points) {
            spread.push(format!("peak {} at {}", percent(Some(v)), hhmm(at)));
        }
        if let Some(v) = average(points) {
            spread.push(format!("avg {}", percent(Some(v))));
        }
        div()
            .debug_selector(move || format!("host-legend-{key}-{now}"))
            .h(px(tokens::BAND_H))
            .flex()
            .flex_col()
            .justify_center()
            .gap(px(tokens::LEGEND_GAP))
            .child(kit::label(t, figure.name(Some(r))))
            .child(kit::glance(t, number, unit, colour))
            .child(
                div()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_color(t.text2)
                    .child(whole),
            )
            .child(
                div()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_size(px(size::SMALL))
                    .text_color(t.dim)
                    .child(spread.join(" · ")),
            )
    }

    /// The programs that weigh on the machine, heaviest first.
    fn heavy(
        &self,
        t: &Theme,
        node: &str,
        s: &State,
        heavy: &[store::Heavy],
        cx: &mut Context<Self>,
    ) -> Div {
        let home = s.facts.user.as_deref().map(|u| match u {
            "root" => "/root".to_string(),
            u => format!("/home/{u}"),
        });
        let sessions: Vec<(String, String)> = self
            .sessions
            .as_ref()
            .map(|m| {
                m.read(cx)
                    .sessions
                    .iter()
                    .filter(|x| x.node == node)
                    .filter_map(|x| Some((x.name.clone(), x.cwd.clone()?)))
                    .collect()
            })
            .unwrap_or_default();
        let sessions: Vec<(&str, &str)> = sessions
            .iter()
            .map(|(n, c)| (n.as_str(), c.as_str()))
            .collect();
        let rows = store::sorted(heavy, Figure::Memory)
            .into_iter()
            .map(|h| {
                let selector = format!("host-heavy-{}", store::program(h));
                let place = store::place(h.cwd.as_deref(), &sessions, home.as_deref());
                pane::table_row(
                    t,
                    &HEAVY,
                    vec![
                        pane::cell(store::program(h))
                            .debug_selector(move || selector)
                            .into_any_element(),
                        pane::cell(place).text_color(t.text2).into_any_element(),
                        div()
                            .child(h.cpu.map_or_else(String::new, |c| percent(Some(c))))
                            .into_any_element(),
                        div().child(store::bytes(h.memory)).into_any_element(),
                    ],
                )
                .into_any_element()
            })
            .collect();
        pane::section(t, "Heavy now").child(pane::table(t, &HEAVY, rows))
    }
}

/// A sentence as a clause: "No unit" reads "no unit" after a colon.
fn lower_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// The machine's detail: its head, and its body scrolling under it, as
/// wide as the page (the figures want the room a reading column would cut).
fn frame(head: Div, body: impl IntoElement) -> Div {
    pane::detail_frame(
        head,
        div()
            .id("host-detail")
            .absolute()
            .inset_0()
            .overflow_y_scroll()
            .child(div().w_full().p(px(size::PAD)).child(body)),
    )
}

/// A figure's key in selectors: `cpu`, `memory`.
fn key(figure: Figure) -> String {
    match figure {
        Figure::Cpu => "cpu".into(),
        Figure::Memory => "memory".into(),
        Figure::Swap => "swap".into(),
        Figure::Gpu => "gpu".into(),
        Figure::Vram => "vram".into(),
        Figure::Disk(i) => format!("disk-{i}"),
    }
}

/// What a figure is of now: its bytes of the whole, the CPU's threads, the
/// card and how hot it runs.
fn whole_now(figure: Figure, s: &State) -> String {
    let r = s.latest.as_ref().expect("a reading");
    match figure {
        Figure::Cpu if s.facts.threads > 0 => format!("{} threads", s.facts.threads),
        Figure::Gpu => match (r.gpus.first(), &r.gpu_reason) {
            (Some(g), _) => match g.temp {
                Some(c) => format!("{c:.0} °C · {}", g.name),
                None => g.name.clone(),
            },
            (None, Some(why)) => format!("Not read: {why}"),
            (None, None) => String::new(),
        },
        f => match (f.whole(r), f) {
            (Some(w), Figure::Swap) => match r.swap_activity_bps.filter(|b| *b >= 1.) {
                Some(bps) => format!("{}, moving {}/s", store::of(w), store::bytes(bps as u64)),
                None => store::of(w),
            },
            (Some(w), _) => store::of(w),
            (None, _) => String::new(),
        },
    }
}

/// A label on the time axis, centred on its moment.
fn tick(t: &Theme, at: gpui::DefiniteLength, words: String, colour: Hsla) -> Div {
    div()
        .absolute()
        .top_0()
        .left(at)
        .ml(px(-tokens::TICK_W / 2.))
        .w(px(tokens::TICK_W))
        .flex()
        .justify_center()
        .text_size(px(size::SMALL))
        .text_color(colour)
        .rounded(px(size::RADIUS))
        .child(words)
        .when(colour == t.text, |d| d.font_weight(gpui::FontWeight::BOLD))
}

/// The disks: how full each is, what is free and how it moved over the
/// span; one nearly full in the failed colour.
fn disks(t: &Theme, s: &State, troubles: &[Trouble]) -> Option<Div> {
    let r = s.latest.as_ref()?;
    if r.disks.is_empty() {
        return None;
    }
    let end = r.at;
    let start = s
        .minutes
        .first()
        .map_or(end, |p| p.at)
        .max(end.saturating_sub(DAY));
    let mut row = div().flex().flex_wrap().gap(px(size::SECTION_GAP));
    for (i, d) in r.disks.iter().enumerate() {
        let warn = store::warns(troubles, Figure::Disk(i));
        let share = d.usage().percent().unwrap_or(0.);
        let free = d.total.saturating_sub(d.used);
        let mut moved = format!("{} free", store::bytes(free));
        if let Some((grew, at)) = store::grew(s, i, start) {
            moved.push_str(&format!(
                " · {} since {}",
                store::change(grew, d.total),
                hhmm(at)
            ));
        }
        let ink = if warn { t.error } else { t.chart };
        let selector = format!("host-disk-{}-{}", d.name, percent(Some(share)));
        row = row.child(
            div()
                .debug_selector(move || selector)
                .flex_1()
                .min_w(px(tokens::DISK_W))
                .max_w(px(tokens::DISK_W * 2.))
                .flex()
                .flex_col()
                .gap(px(size::LABEL_BOTTOM))
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .gap(px(size::HEAD_GAP))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .child(d.name.clone()),
                        )
                        .child(
                            div()
                                .text_color(if warn { t.error } else { t.text })
                                .child(percent(Some(share))),
                        ),
                )
                .child(
                    div()
                        .h(px(tokens::DISK_BAR_H))
                        .w_full()
                        .rounded(px(size::RADIUS))
                        .bg(t.raised)
                        .child(
                            div()
                                .h_full()
                                .w(relative(share / 100.))
                                .rounded(px(size::RADIUS))
                                .bg(ink),
                        ),
                )
                .child(
                    div()
                        .text_size(px(size::SMALL))
                        .text_color(t.text2)
                        .child(store::of(d.usage())),
                )
                .child(
                    div()
                        .text_size(px(size::SMALL))
                        .text_color(if warn { t.error } else { t.dim })
                        .child(moved),
                ),
        );
    }
    Some(pane::section(t, "Disks").child(row))
}

/// Today's events, newest first.
fn today(t: &Theme, s: &State, now: u64) -> Option<Div> {
    let today: Vec<_> = s
        .events
        .iter()
        .rev()
        .filter(|e| e.at + DAY > now)
        .take(EVENTS_SHOWN)
        .collect();
    if today.is_empty() {
        return None;
    }
    let rows = today
        .into_iter()
        .map(|e| {
            let selector = format!("host-event-{}", e.text);
            pane::table_row(
                t,
                &EVENTS,
                vec![
                    div().text_color(t.dim).child(hhmm(e.at)).into_any_element(),
                    div()
                        .debug_selector(move || selector)
                        .when(e.failure, |d| d.text_color(t.error))
                        .child(e.text.clone())
                        .into_any_element(),
                ],
            )
            .into_any_element()
        })
        .collect();
    Some(pane::section(t, "Today").child(pane::table(t, &EVENTS, rows)))
}
