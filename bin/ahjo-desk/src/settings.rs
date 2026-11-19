//! Settings: how the Desk looks and behaves, set where he answers and kept
//! in `desk.json`. A theme is a card drawn in its own colours; a click
//! draws the whole Desk in it at once and keeps it.

use crate::app::{DeskApp, Reach};
use crate::desk::{self, Order};
use crate::tokens::{self, GAP, GAP_SMALL, META, PAD};
use crate::views::{button, chip};
use ahjo_app_kit::theme::{Direction, Theme, alpha};
use ahjo_app_kit::widgets::kit::{self, Kind};
use ahjo_app_kit::zoom::{Zoom, app as zoom};

use gpui::{
    AnyElement, ClickEvent, Context, Div, FontWeight, SharedString, Window, div, prelude::*, px,
};

impl DeskApp {
    pub fn settings_view(
        &mut self,
        t: &Theme,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let page = div()
            .w_full()
            .max_w(px(tokens::SETTINGS_W))
            .flex()
            .flex_col()
            .gap(px(tokens::SECTION_GAP))
            .py(px(tokens::SECTION_GAP))
            .px(px(PAD))
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(kit::page_title(t, "Settings"))
                    .child(div().flex_1())
                    .child(
                        button(t, "settings-done", "Done", Kind::Primary)
                            .tooltip(kit::tip("Esc"))
                            .on_click(cx.listener(|app, _: &ClickEvent, window, cx| {
                                app.toggle_settings(window, cx)
                            })),
                    ),
            )
            .child(self.themes(t, cx))
            .child(self.reading(t, cx))
            .child(self.answering(t, cx))
            .child(self.telling(t, cx))
            .child(self.desks_kept(t));
        div()
            .id("settings")
            .size_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .items_center()
            .child(page.flex_none())
            .into_any_element()
    }

    fn themes(&mut self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let chosen = Direction::from_key(&self.config.theme);
        let swarm = ahjo_app_kit::theme::chosen().0;
        let mut grid = div().flex().flex_wrap().gap(px(GAP));
        grid = grid.child(
            theme_card(t, &swarm, "As in Swarm", chosen.is_none(), "swarm")
                .on_click(cx.listener(|app, _: &ClickEvent, _, cx| app.set_theme(None, cx))),
        );
        for d in Direction::ALL {
            let th = Theme::new(d);
            grid = grid.child(
                theme_card(t, &th, d.label(), chosen == Some(d), d.key()).on_click(
                    cx.listener(move |app, _: &ClickEvent, _, cx| app.set_theme(Some(d), cx)),
                ),
            );
        }
        section(
            t,
            "Theme",
            Some("A click draws the Desk in it and keeps it."),
        )
        .child(grid)
    }

    fn reading(&mut self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let size = self.app_size;
        let step = |id: &'static str, words: &'static str, z: Zoom| {
            button(t, id, words, Kind::Plain).on_click(
                cx.listener(move |app, _: &ClickEvent, window, cx| app.zoom_app(z, 1, window, cx)),
            )
        };
        let mut order = div().flex().flex_wrap().gap(px(GAP_SMALL));
        for o in Order::ALL {
            order = order.child(
                chip(
                    t,
                    SharedString::from(format!("order-{o:?}")),
                    o.label(),
                    self.config.order == o,
                    None,
                )
                .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| app.set_order(o, cx))),
            );
        }
        section(t, "Reading", None)
            .child(row(
                t,
                "Size",
                "The whole Desk, text and pictures alike. Ctrl+= and Ctrl+- anywhere.",
                div()
                    .flex()
                    .items_center()
                    .gap(px(GAP_SMALL))
                    .child(step("size-down", "−", Zoom::Out))
                    .child(
                        div()
                            .min_w(px(48.))
                            .text_center()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(format!("{size}%")),
                    )
                    .child(step("size-up", "+", Zoom::In))
                    .when(size != zoom::DEFAULT, |d| d.child(step("size-reset", "Reset", Zoom::Reset))),
            ))
            .child(row(
                t,
                "Waiting list",
                match self.config.order {
                    Order::Quickest => "Quick asks first: each answer frees an agent, and quick ones free the most soonest.",
                    Order::Oldest => "The agent that has waited longest first.",
                    Order::Newest => "What just came in first.",
                },
                order,
            ))
    }

    fn answering(&mut self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let enter = self.config.enter_sends;
        section(t, "Answering", None).child(row(
            t,
            "Enter sends",
            if enter {
                "Enter sends your answer; Shift+Enter starts a new line."
            } else {
                "Enter starts a new line; Ctrl+Enter sends."
            },
            kit::toggle(t, "enter-sends", enter).on_click(
                cx.listener(|app, _: &ClickEvent, window, cx| app.toggle_enter_sends(window, cx)),
            ),
        ))
    }

    fn telling(&mut self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let on = self.config.tell_session;
        let check = match &self.node_check {
            None => None,
            Some(Ok(w)) => Some((w.clone(), t.done)),
            Some(Err(e)) => Some((e.clone(), t.error)),
        };
        section(t, "Telling the agent", None)
            .child(row(
                t,
                "Tell its session when I send",
                "The agent that asked hears at once, in its session, that you answered and how to read it: it need not keep looking. Asks posted before this was there do not say their session.",
                kit::toggle(t, "tell-session", on).on_click(
                    cx.listener(|app, _: &ClickEvent, _, cx| app.toggle_tell(cx)),
                ),
            ))
            .when(on, |d| {
                d.child(row(
                    t,
                    "Through the node",
                    "The Ahjo Swarm node that reaches the sessions. Left empty, the one Ahjo Swarm uses.",
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(GAP_SMALL))
                        .w(px(tokens::SETTINGS_FIELD))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(GAP_SMALL))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .child(self.node_field.clone()),
                                )
                                .child(button(t, "node-check", "Check", Kind::Plain).on_click(
                                    cx.listener(|app, _: &ClickEvent, _, cx| app.check_node(cx)),
                                )),
                        )
                        .children(check.map(|(w, c)| {
                            div().text_size(px(META)).text_color(c).child(w)
                        })),
                ))
            })
    }

    fn desks_kept(&self, t: &Theme) -> Div {
        let mut list = div().flex().flex_col().gap(px(4.));
        for d in &self.desks {
            let (dot, state) = match &d.reach {
                Reach::Ok => (t.done, "answering".to_string()),
                Reach::Loading => (t.dim, "reaching…".to_string()),
                Reach::Failed(e) => (t.error, e.clone()),
            };
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(GAP_SMALL))
                    .child(div().size(px(tokens::DOT)).rounded_full().bg(dot))
                    .child(div().w(px(120.)).flex_none().truncate().child(d.name()))
                    .child(div().text_color(t.text2).child(d.place.url.clone()))
                    .child(div().min_w_0().truncate().text_color(t.dim).child(state)),
            );
        }
        let kept = desk::config_path()
            .map(|p| format!("Settings are kept in {}.", p.display()))
            .unwrap_or_default();
        section(
            t,
            "Desks",
            Some("Every desk the first one knows of is read; start from another with --desk."),
        )
        .child(list)
        .child(div().text_size(px(META)).text_color(t.dim).child(kept))
    }
}

/// A section: its name, a line on it, and what it holds.
fn section(t: &Theme, name: &str, about: Option<&str>) -> Div {
    div().flex().flex_col().gap(px(GAP)).child(
        div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(kit::group_label(t, name))
            .children(about.map(|a| {
                div()
                    .text_size(px(META))
                    .text_color(t.dim)
                    .child(a.to_string())
            })),
    )
}

/// A setting: its name and what it does on the left, its control on the
/// right.
fn row(t: &Theme, name: &str, about: &str, control: impl IntoElement) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(PAD))
        .py(px(GAP_SMALL))
        .border_t_1()
        .border_color(t.divider)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(div().text_size(px(tokens::BODY)).child(name.to_string()))
                .child(
                    div()
                        .text_size(px(META))
                        .text_color(t.text2)
                        .child(about.to_string()),
                ),
        )
        .child(div().flex_none().child(control))
}

/// A theme as a card: a small desk drawn in its colours (a waiting row,
/// a line of words, its accent's Send), its name under it.
fn theme_card(t: &Theme, th: &Theme, name: &str, on: bool, id: &str) -> gpui::Stateful<Div> {
    let line = |w: f32, c| div().w(px(w)).h(px(5.)).rounded_full().bg(c);
    let accent = t.accent;
    let selector = format!("theme-{id}");
    div()
        .id(SharedString::from(selector.clone()))
        .debug_selector(move || selector)
        .w(px(tokens::THEME_CARD))
        .flex()
        .flex_col()
        .gap(px(6.))
        .p(px(6.))
        .rounded(px(tokens::RADIUS + 2.))
        .border_2()
        .border_color(if on { t.accent } else { alpha(t.divider, 0.0) })
        .cursor_pointer()
        .hover(move |d| d.border_color(alpha(accent, 0.5)))
        .child(
            div()
                .h(px(72.))
                .rounded(px(tokens::RADIUS))
                .border_1()
                .border_color(th.divider)
                .bg(th.bg)
                .overflow_hidden()
                .flex()
                .child(
                    // The waiting list, one row in front.
                    div()
                        .w(px(46.))
                        .h_full()
                        .bg(th.surface)
                        .border_r_1()
                        .border_color(th.divider)
                        .p(px(5.))
                        .flex()
                        .flex_col()
                        .gap(px(5.))
                        .child(div().h(px(10.)).rounded(px(2.)).bg(alpha(th.accent, 0.25)))
                        .child(line(28., th.dim))
                        .child(line(32., th.dim)),
                )
                .child(
                    div()
                        .flex_1()
                        .p(px(7.))
                        .flex()
                        .flex_col()
                        .gap(px(5.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .child(div().size(px(6.)).rounded_full().bg(th.needs))
                                .child(line(44., th.text)),
                        )
                        .child(line(60., th.text2))
                        .child(line(38., th.text2))
                        .child(div().flex_1())
                        .child(
                            div()
                                .flex()
                                .justify_end()
                                .child(div().w(px(26.)).h(px(10.)).rounded(px(3.)).bg(th.accent)),
                        ),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .text_size(px(META))
                .text_color(if on { t.text } else { t.text2 })
                .when(on, |d| d.font_weight(FontWeight::SEMIBOLD))
                .child(div().min_w_0().truncate().child(name.to_string()))
                .when(on, |d| d.child(div().text_color(t.accent).child("✓"))),
        )
}
