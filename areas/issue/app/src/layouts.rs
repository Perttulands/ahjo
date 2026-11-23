//! The strip and the time page (DESIGN.md, Issue): the two takes that put
//! time on the page beside the tree. Both draw the same chart
//! ([`crate::chart`]) and answer a range at once; they differ in what
//! leads. The strip keeps the tree first and counts the projects and
//! states beside it; the time page leads with the chart and says what a
//! range opened and finished, and what sat idle.

use crate::chart::{self, Picture, Shape};
use crate::store::issue::Status;
use crate::store::issue::timeline::{day_label, range_label};
use crate::tokens::{self, *};
use crate::view::on_days;
use crate::words;
use crate::{Issue, Moved};
use ahjo_app_kit::theme::{Theme, size};
use ahjo_app_kit::widgets::{kit, pane};
use gpui::{AnyElement, Context, Div, FontWeight, Hsla, SharedString, div, prelude::*, px};

impl Issue {
    /// The strip over the facets, the list and the item.
    pub(crate) fn strip_page(
        &mut self,
        t: &Theme,
        list: Div,
        detail: Div,
        cx: &mut Context<Self>,
    ) -> Div {
        let folded = self.chart_folded;
        let fold = pane::act(
            t,
            "issue-chart-fold",
            if folded { "Show chart" } else { "Hide chart" },
        )
        .debug_selector(|| "issue-chart-fold".into())
        .tooltip(kit::tip(if folded {
            "Show what opened and finished each day  C"
        } else {
            "Fold the chart away to this line  C"
        }))
        .on_click(cx.listener(move |w, _, _, cx| w.fold_chart(!folded, cx)));
        let head = if folded {
            self.folded_head(t, cx)
        } else {
            self.band_head(t, None, cx)
        };
        let band = div()
            .flex_none()
            .flex()
            .flex_col()
            .bg(t.surface)
            .border_b_1()
            .border_color(t.divider)
            .child(head.child(fold))
            .when(!folded, |d| {
                d.child(
                    div()
                        .debug_selector(|| "issue-strip".into())
                        .cursor(gpui::CursorStyle::Crosshair)
                        .h(px(STRIP_H))
                        .w_full()
                        .child(self.picture(t, Shape::Strip, cx)),
                )
            });
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(t.bg)
            .child(band)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(self.facets(t, cx))
                    .child(list)
                    .child(detail),
            )
    }

    /// The chart over the range's totals, then the list and the item.
    pub(crate) fn time_page(
        &mut self,
        t: &Theme,
        list: Div,
        detail: Div,
        cx: &mut Context<Self>,
    ) -> Div {
        let scope = self.scope(t, cx);
        let band = div()
            .flex_none()
            .flex()
            .flex_col()
            .bg(t.surface)
            .border_b_1()
            .border_color(t.divider)
            .child(self.band_head(t, Some(scope), cx))
            .child(
                div()
                    .debug_selector(|| "issue-chart".into())
                    .cursor(gpui::CursorStyle::Crosshair)
                    .h(px(CHART_H))
                    .w_full()
                    .child(self.picture(t, Shape::Chart, cx)),
            )
            .child(self.totals(t, cx));
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(t.bg)
            .child(band)
            .child(div().flex_1().min_h_0().flex().child(list).child(detail))
    }

    fn picture(&self, t: &Theme, shape: Shape, cx: &mut Context<Self>) -> impl IntoElement {
        chart::chart(
            t,
            Picture {
                timeline: self.timeline.clone(),
                shape,
                range: self.range,
                hover: self.hover,
            },
            cx.weak_entity(),
        )
    }

    /// The line over the chart: what it is scoped to (the time page's),
    /// the range chosen, and the key. The day under the mouse says its
    /// numbers on the chart.
    fn band_head(&self, t: &Theme, scope: Option<Div>, cx: &mut Context<Self>) -> Div {
        let mut head = div()
            .h(px(size::TAB_BAR))
            .px(px(size::HEAD_PAD))
            .flex()
            .items_center()
            .gap(px(size::HEAD_GAP))
            .text_size(px(size::SMALL))
            .children(scope);
        head = match self.range {
            Some(r) => {
                let (opened, closed) = self.timeline.totals(r);
                head.child(
                    pane::act(t, "issue-range", format!("{}  ×", range_label(r)))
                        .debug_selector(|| "issue-range".into())
                        .text_color(t.text)
                        .tooltip(kit::tip("Let the days go (Esc)"))
                        .on_click(cx.listener(|w, _, _, cx| w.clear_range(cx))),
                )
                .child(
                    div()
                        .text_color(t.text2)
                        .child(format!("{opened} opened · {closed} finished")),
                )
            }
            None => head.child(
                div()
                    .text_color(t.dim)
                    .child("Drag across the days to see what moved"),
            ),
        };
        head.child(div().flex_1())
            .child(swatch(t, tokens::opened(t), "Opened", false))
            .child(swatch(t, t.done, "Finished", false))
            .child(swatch(t, t.chart, "Open", true))
    }

    /// The strip's line when its chart is folded away: the days still
    /// chosen, else what is open and how the last week went.
    fn folded_head(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let head = div()
            .h(px(size::TAB_BAR))
            .px(px(size::HEAD_PAD))
            .flex()
            .items_center()
            .gap(px(size::HEAD_GAP))
            .text_size(px(size::SMALL));
        let head = match self.range {
            Some(r) => head.child(
                pane::act(t, "issue-range", format!("{}  ×", range_label(r)))
                    .debug_selector(|| "issue-range".into())
                    .text_color(t.text)
                    .tooltip(kit::tip("Let the days go (Esc)"))
                    .on_click(cx.listener(|w, _, _, cx| w.clear_range(cx))),
            ),
            None => {
                let tl = &self.timeline;
                let days = tokens::OPENING_RANGE;
                let week =
                    crate::store::issue::timeline::Range::between(tl.last() + 1 - days, tl.last());
                let (opened, closed) = tl.totals(week);
                head.child(
                    div()
                        .debug_selector(|| "issue-chart-folded".into())
                        .text_color(t.text2)
                        .child(format!(
                            "The last {days} days: {opened} opened, {closed} finished"
                        )),
                )
            }
        };
        head.child(div().flex_1())
    }

    /// The time page's scope: every project, or the one chosen.
    fn scope(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let label = self
            .project(cx)
            .map(|p| self.board.projects[p].label.clone())
            .unwrap_or_else(|| "Project".into());
        let every = self.every;
        div()
            .flex()
            .items_center()
            .gap(px(SPACE_XS))
            .text_size(px(size::TEXT))
            .child(
                choice(t, "issue-scope-every", "Every project", every)
                    .tooltip(kit::tip("All issues on every node"))
                    .on_click(cx.listener(|w, _, _, cx| w.show_every(true, cx))),
            )
            .child(
                choice(t, "issue-scope-project", label, !every)
                    .max_w(px(PROJECT_ACT_W))
                    .tooltip(kit::tip("One project's issues (P chooses another)"))
                    .on_click(cx.listener(move |w, _, window, cx| {
                        if w.every {
                            w.show_every(false, cx);
                        } else {
                            w.start_picking(window, cx);
                        }
                    })),
            )
    }

    /// What the range did, in numbers worth reading at a glance; each
    /// number lists its work below.
    fn totals(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let row = div()
            .flex_none()
            .px(px(size::HEAD_PAD))
            .pb(px(size::HEAD_PAD))
            .flex()
            .items_end()
            .gap(px(size::HEAD_GAP));
        let Some(r) = self.range else {
            return row.child(
                div()
                    .h(px(NUMBER * 2.))
                    .flex()
                    .items_center()
                    .px(px(TOTAL_PAD))
                    .text_color(t.dim)
                    .child("Choose days on the chart: what they opened and finished, and what sat idle, lists here."),
            );
        };
        let m = &self.movement;
        let tabs = [
            (
                Moved::Opened,
                m.opened.len(),
                "opened",
                t.text,
                "Opened in these days",
            ),
            (
                Moved::Closed,
                m.closed.len(),
                "finished",
                t.done,
                "Finished in these days",
            ),
            (
                Moved::Idle,
                m.idle.len(),
                "idle",
                t.text2,
                "Open at the end of these days and untouched for the days chosen beside: under way first, then the longest idle",
            ),
        ];
        let mut row = row;
        for (moved, n, word, ink, tip) in tabs {
            let chosen = self.moved == moved;
            row = row.child(
                div()
                    .id(SharedString::from(format!("issue-moved-{word}")))
                    .debug_selector(move || format!("issue-moved-{word}"))
                    .min_w(px(TOTAL_W))
                    .px(px(TOTAL_PAD))
                    .py(px(SPACE_XS))
                    .rounded(px(size::RADIUS))
                    .cursor_pointer()
                    .flex()
                    .flex_col()
                    .when(chosen, |d| d.bg(t.raised))
                    .hover(|d| d.bg(t.accent_soft()))
                    .tooltip(kit::tip(tip))
                    .child(
                        div()
                            .text_size(px(NUMBER))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(if n == 0 { t.dim } else { ink })
                            .child(n.to_string()),
                    )
                    .child(
                        div()
                            .text_size(px(size::SMALL))
                            .text_color(if chosen { t.text } else { t.dim })
                            .child(word),
                    )
                    .on_click(cx.listener(move |w, _, _, cx| w.show_moved(moved, cx))),
            );
        }
        row = row.child(self.idle_days(t, cx));
        // The open work at the range's end, and how it changed across it.
        let tl = &self.timeline;
        if let Some(end) = tl.index(r.to.min(tl.last())) {
            let start = tl.index(r.from).map_or(0, |d| d.saturating_sub(1));
            let change = tl.open[end] as i64 - tl.open[start] as i64;
            row = row.child(div().flex_1()).child(
                div()
                    .px(px(TOTAL_PAD))
                    .py(px(SPACE_XS))
                    .flex()
                    .flex_col()
                    .items_end()
                    .child(
                        div()
                            .flex()
                            .items_end()
                            .gap(px(size::HEAD_GAP))
                            .child(div().text_size(px(size::SMALL)).text_color(t.dim).child(
                                match change {
                                    0 => "no change".to_string(),
                                    c => format!("{c:+} {}", on_days(r)),
                                },
                            ))
                            .child(
                                div()
                                    .text_size(px(NUMBER))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(t.text2)
                                    .child(tl.open[end].to_string()),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(size::SMALL))
                            .text_color(t.dim)
                            .child(format!("open at the end of {}", day_label(r.to))),
                    ),
            );
        }
        row
    }

    /// How long untouched makes open work idle: the days, each a choice.
    fn idle_days(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let mut days = div()
            .flex()
            .items_center()
            .gap(px(SPACE_XS))
            .pb(px(SPACE_XS))
            .text_size(px(size::SMALL))
            .text_color(t.dim)
            .child("untouched");
        for d in IDLE_DAYS {
            let id: &'static str = match d {
                7 => "issue-idle-7",
                14 => "issue-idle-14",
                30 => "issue-idle-30",
                _ => "issue-idle-60",
            };
            days = days.child(
                choice(t, id, d.to_string(), self.idle_days == d)
                    .tooltip(kit::tip(format!("Idle: untouched for {d} days or more")))
                    .on_click(cx.listener(move |w, _, _, cx| w.set_idle_days(d, cx))),
            );
        }
        days.child("days or more")
    }

    /// The strip's facets: the projects, then the states, each counted;
    /// open work, or with a range the work that moved in it.
    fn facets(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let project = self.project(cx);
        let total: usize = self.project_counts.iter().sum();
        let mut order: Vec<usize> = (0..self.board.projects.len()).collect();
        let count = |p: usize| self.project_counts.get(p).copied().unwrap_or(0);
        order.sort_by(|&a, &b| {
            count(b).cmp(&count(a)).then_with(|| {
                self.board.projects[a]
                    .label
                    .cmp(&self.board.projects[b].label)
            })
        });
        // The states first: five rows that a long project list would
        // otherwise push out of sight.
        let mut rows: Vec<AnyElement> = vec![pane::label(t, "State").into_any_element()];
        // Each with its mark, so the facets are the list's key.
        for state in [
            Status::Open,
            Status::InProgress,
            Status::Blocked,
            Status::Deferred,
            Status::Closed,
        ] {
            let chosen = self.state == Some(state);
            let n = self.state_counts[state as usize];
            let selector = format!("issue-state-{}", state.label());
            let s = selector.clone();
            rows.push(
                pane::row(t, SharedString::from(selector), chosen)
                    .debug_selector(move || s)
                    .when(n == 0 && !chosen, |d| d.text_color(t.dim))
                    .child(crate::view::mark(t, state))
                    .child(pane::cell(words::state(state)))
                    .child(pane::meta(t, n.to_string(), None))
                    .tooltip(kit::tip(words::state_means(state)))
                    .on_click(
                        cx.listener(move |w, _, _, cx| {
                            w.show_state((!chosen).then_some(state), cx)
                        }),
                    )
                    .into_any_element(),
            );
        }
        rows.extend([
            pane::label(
                t,
                if self.range.is_some() {
                    "Moved in these days"
                } else {
                    "Open issues"
                },
            )
            .into_any_element(),
            facet(
                t,
                "issue-facet-every".into(),
                "Every project".into(),
                total,
                self.every,
            )
            .on_click(cx.listener(|w, _, _, cx| w.show_every(true, cx)))
            .into_any_element(),
        ]);
        for p in order {
            let info = &self.board.projects[p];
            let n = count(p);
            let chosen = !self.every && Some(p) == project;
            let why = info.error.clone();
            let selector = format!("issue-facet-{}", info.label);
            // A project that could not be read says so where its count
            // would be: none open and not read are not the same thing.
            let row = if why.is_some() {
                let s = selector.clone();
                pane::row(t, SharedString::from(selector), chosen)
                    .debug_selector(move || s)
                    .child(pane::cell(info.label.clone()))
                    .child(pane::meta(t, "not read", Some(t.error)))
            } else {
                facet(t, selector, info.label.clone(), n, chosen)
            };
            rows.push(
                row.when_some(why, |d, why| d.tooltip(kit::tip(why)))
                    .on_click(cx.listener(move |w, _, window, cx| {
                        w.every = false;
                        w.choose_project(p, window, cx);
                    }))
                    .into_any_element(),
            );
        }
        div()
            .w(px(FACETS_W))
            .flex_none()
            .h_full()
            .bg(t.surface)
            .border_r_1()
            .border_color(t.divider)
            .child(
                pane::scroll("issue-facets")
                    .size_full()
                    .py(px(size::ROW_INSET))
                    .children(rows),
            )
    }
}

/// One facet: its name and its count; none counted reads quiet.
fn facet(t: &Theme, id: String, name: String, n: usize, chosen: bool) -> gpui::Stateful<Div> {
    let selector = id.clone();
    pane::row(t, SharedString::from(id), chosen)
        .debug_selector(move || selector)
        .when(n == 0 && !chosen, |d| d.text_color(t.dim))
        .child(pane::cell(name))
        .child(pane::meta(t, n.to_string(), None))
}

/// One of two or three choices side by side: the chosen one raised.
fn choice(
    t: &Theme,
    id: &'static str,
    text: impl Into<SharedString>,
    chosen: bool,
) -> gpui::Stateful<Div> {
    pane::act(t, id, text)
        .debug_selector(move || id.into())
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .when(chosen, |d| d.bg(t.raised).text_color(t.text))
}

/// A colour in the chart's key, a bar's or a line's, and what it stands
/// for.
fn swatch(t: &Theme, ink: Hsla, word: &'static str, line: bool) -> Div {
    let mark = if line {
        div().w(px(SWATCH)).h(px(STROKE))
    } else {
        div().size(px(SWATCH)).rounded(px(SWATCH_RADIUS))
    };
    div()
        .flex()
        .items_center()
        .gap(px(SPACE_XS))
        .text_color(t.dim)
        .child(mark.bg(ink))
        .child(word)
}
