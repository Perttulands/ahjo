//! Settings: how the Desk looks and behaves, set where he answers and kept
//! in `desk.json`, a layer over the window that Escape closes. The theme
//! is chosen with the kit's picker, as in Ahjo Swarm.

use crate::app::{DeskApp, Reach};
use crate::desk::{self, Order};
use crate::tokens::{self, GAP, GAP_SMALL, META, PAD};
use crate::views::{button, chip};
use ahjo_app_kit::theme::{Direction, Theme, theme};
use ahjo_app_kit::widgets::kit::{self, Kind};
use ahjo_app_kit::widgets::{pane, theme_picker};
use ahjo_app_kit::zoom::{Zoom, app as zoom};

use gpui::{
    AnyElement, ClickEvent, Context, Div, Entity, FocusHandle, FontWeight, IntoElement, Render,
    SharedString, Window, div, prelude::*, px,
};

/// Settings as a layer on the Desk's frame: drawn from the app's state,
/// over the whole window, with the keyboard in it.
pub struct SettingsLayer {
    app: Entity<DeskApp>,
    pub focus: FocusHandle,
}

impl SettingsLayer {
    pub fn new(app: Entity<DeskApp>, cx: &mut Context<Self>) -> Self {
        cx.observe(&app, |_, _, cx| cx.notify()).detach();
        Self {
            app,
            focus: cx.focus_handle(),
        }
    }
}

impl Render for SettingsLayer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx).clone();
        let page = self
            .app
            .update(cx, |app, cx| app.settings_view(&t, window, cx));
        div()
            .id("settings-layer")
            .debug_selector(|| "settings-layer".into())
            .track_focus(&self.focus)
            .size_full()
            .bg(t.bg)
            .text_color(t.text)
            .child(page)
    }
}

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
            // The shell draws the close control at the window's top right.
            .child(kit::page_title(t, "Settings"))
            .child(self.reading(t, cx))
            .child(self.answering(t, cx))
            .child(self.telling(t, cx))
            .child(self.desks_kept(t));
        pane::scroll("settings")
            .size_full()
            .items_center()
            .child(page.flex_none())
            .into_any_element()
    }

    /// The theme's row: the kit's picker, which can follow Ahjo Swarm.
    fn theme_row(&mut self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let follows = Direction::from_key(&self.config.theme).is_none();
        row(
            t,
            "Theme",
            if follows {
                "As in Ahjo Swarm: the Desk follows the theme chosen there."
            } else {
                "The Desk's own. Up and Down try one, Enter keeps it."
            },
            theme_picker::button(t, "theme-open").on_click(
                cx.listener(|app, _: &ClickEvent, window, cx| app.pick_theme(window, cx)),
            ),
        )
    }

    fn reading(&mut self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let theme = self.theme_row(t, cx);
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
            .child(theme)
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
            .child(row(
                t,
                "Put the list away",
                "The answer and the picture take its room. Ctrl+B anywhere.",
                kit::toggle(t, "fold-list-setting", self.config.fold_list)
                    .on_click(cx.listener(|app, _: &ClickEvent, _, cx| app.toggle_fold(cx))),
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
            Some(Err(e)) => Some((e.message.clone(), t.error)),
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
                Reach::Away(_) => (t.dim, "not running".to_string()),
                Reach::Failed(e) => (t.error, e.message.clone()),
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
