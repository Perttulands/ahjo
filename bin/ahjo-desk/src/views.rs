//! How the window is laid out: the list of asks at the left (put away
//! with Ctrl+B), the conversation beside it, and the surface (what the
//! agent published, where he marks) taking the rest; each edge between
//! them dragged where he wants it, and kept. No bar runs over the three:
//! every row of the window is for the ask or his answer, and what is done
//! to the whole Desk sits behind one menu. The conversation is each round
//! as a turn of the agent's, his answer to it as his turn, their comments
//! between, and right under it his words, with his marks above them.
//! Options and the agent's recommendation stand in the agent's turn as an
//! aid: he can lean to one, but his words are the answer.

use crate::app::{Aim, DeskApp, Grip, List, MenuAct, Reach, Reached};
use crate::icons::Icon;
use crate::marks;
use crate::model::{self, Ask, Form, MediaKind};
use crate::tokens::{self, CAP_TEXT, GAP, GAP_SMALL, META};
use ahjo_app_kit::theme::{FAMILY, Theme, alpha, rgb, theme, ty};
use ahjo_app_kit::widgets::kit::{self, Kind};
use ahjo_app_kit::widgets::pane;
use gpui::{
    AnyElement, ClickEvent, Context, Div, FontWeight, Hsla, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, ObjectFit, Render, SharedString, Stateful, StyledImage, Window,
    anchored, deferred, div, img, point, prelude::*, px, relative,
};

impl Render for DeskApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.take_aim(window, cx);
        self.keep_page();
        self.keep_drafts(cx);
        let t = theme(cx).clone();
        let wide = f32::from(window.viewport_size().width);
        let (list_w, talk_w) = self.columns(wide);
        let lines = tokens::words_lines(f32::from(window.viewport_size().height), t.ty.read);
        self.words.update(cx, |f, _| f.set_max_lines(lines));
        let shown = self.current_ask().cloned();
        let looked = shown.is_some() && !self.items().is_empty();
        let full = self.full && looked && !self.settings_open;
        // Where a notice stands: over the answer, which nothing covers (a
        // page shown inside the window lies over the surface).
        let notice_over = if full || self.settings_open {
            (0., wide)
        } else if looked {
            (list_w, talk_w)
        } else {
            (list_w, wide - list_w)
        };
        let body = if self.settings_open {
            self.settings_view(&t, window, cx)
        } else if self.desks.is_empty() {
            no_desks(&t)
        } else if full {
            self.stage(window, cx)
        } else {
            let main = match shown {
                None => self.all_clear(&t, cx),
                Some(ask) if !looked => div()
                    .size_full()
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .h_full()
                            .w_full()
                            .max_w(px(tokens::READING))
                            .border_x_1()
                            .border_color(t.divider)
                            .child(self.conversation(&t, &ask, window, cx)),
                    )
                    .into_any_element(),
                Some(ask) => div()
                    .size_full()
                    .flex()
                    .child(
                        div()
                            .w(px(talk_w))
                            .flex_none()
                            .h_full()
                            .child(self.conversation(&t, &ask, window, cx)),
                    )
                    .child(self.grip(&t, Grip::Talk, wide, cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .child(self.stage(window, cx)),
                    )
                    .into_any_element(),
            };
            div()
                .size_full()
                .flex()
                .when(list_w > 0., |d| {
                    d.child(self.side_panel(&t, list_w, cx)).child(self.grip(
                        &t,
                        Grip::List,
                        wide,
                        cx,
                    ))
                })
                .child(div().flex_1().min_w_0().h_full().child(main))
                .into_any_element()
        };
        div()
            .id("desk")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|app, e, window, cx| app.on_key(e, window, cx)))
            .size_full()
            .bg(t.bg)
            .text_color(t.text)
            .font_family(FAMILY)
            .text_size(px(META))
            .relative()
            .child(body)
            .children(self.toast_view(&t, notice_over))
            .children(self.menu_view(&t, cx))
            .children(self.drag_cover(cx))
            // Ctrl+wheel a picture did not take sizes the whole app.
            .child(ahjo_app_kit::zoom::app_catch(self.app_wheel_handle(), {
                let me = cx.entity().downgrade();
                move |n, window, cx| {
                    let _ = me.update(cx, |app, cx| {
                        use ahjo_app_kit::zoom::Zoom;
                        let z = if n > 0 { Zoom::In } else { Zoom::Out };
                        app.zoom_app(z, n.abs(), window, cx)
                    });
                }
            }))
    }
}

impl DeskApp {
    // ---- The frame: notices, the menu, the column edges ----

    /// A notice, a moment over the column given (its left and width).
    fn toast_view(&self, t: &Theme, (left, width): (f32, f32)) -> Option<AnyElement> {
        let (words, error) = self.toast.clone()?;
        Some(
            div()
                .absolute()
                .top(px(tokens::TOOLS + GAP_SMALL))
                .left(px(left))
                .w(px(width))
                .px(px(GAP))
                .flex()
                .justify_center()
                .child(
                    div()
                        .id("toast")
                        .max_w_full()
                        .truncate()
                        .px(px(GAP))
                        .py(px(GAP_SMALL))
                        .rounded(px(tokens::RADIUS + 2.))
                        .border_1()
                        .border_color(if error {
                            alpha(t.error, 0.5)
                        } else {
                            t.divider
                        })
                        .bg(if error {
                            t.surface.blend(alpha(t.error, 0.18))
                        } else {
                            t.raised
                        })
                        .shadow_md()
                        .text_color(if error { t.error } else { t.text })
                        .child(words),
                )
                .into_any_element(),
        )
    }

    /// The settings menu: the desks and how they stand, the list's order,
    /// and the few things done to the whole Desk. Up, Down and Enter work
    /// it; Escape or a click beside it shuts it.
    fn menu_view(&mut self, t: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (at, sel) = self.menu?;
        let acts = self.menu_acts();
        let label = |words: &str| {
            div()
                .px(px(GAP))
                .pt(px(GAP_SMALL + 2.))
                .pb(px(2.))
                .text_size(px(CAP_TEXT))
                .text_color(t.dim)
                .child(t.label_case(words))
        };
        let rule = || div().my(px(4.)).h(px(1.)).bg(t.divider);
        let mut card = div()
            .id("menu")
            .occlude()
            .w(px(tokens::MENU_W))
            .py(px(4.))
            .flex()
            .flex_col()
            .rounded(px(tokens::RADIUS + 2.))
            .border_1()
            .border_color(t.divider)
            .bg(t.raised)
            .shadow_lg()
            .text_size(px(META))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(label("Desks"));
        for d in &self.desks {
            let n = d.asks.iter().filter(|a| a.is_open()).count();
            let (dot, state, ink) = match &d.reach {
                Reach::Ok if n == 0 => (t.done, "nothing waiting".to_string(), t.dim),
                Reach::Ok => (t.needs, format!("{n} waiting"), t.text2),
                Reach::Loading => (t.dim, "reaching…".to_string(), t.dim),
                Reach::Failed(e) => (t.error, e.clone(), t.error),
            };
            card = card.child(
                div()
                    .id(SharedString::from(format!("menu-desk-{}", d.place.url)))
                    .h(px(tokens::CAP + 8.))
                    .px(px(GAP))
                    .flex()
                    .items_center()
                    .gap(px(GAP_SMALL))
                    .tooltip(kit::tip(d.place.url.clone()))
                    .child(div().size(px(tokens::DOT)).rounded_full().bg(dot))
                    .child(div().flex_none().child(d.name()))
                    .child(div().flex_1())
                    .child(div().min_w_0().truncate().text_color(ink).child(state)),
            );
        }
        card = card.child(rule()).child(label("Waiting list"));
        for (i, act) in acts.iter().copied().enumerate() {
            let (words, key, checked) = match act {
                MenuAct::Order(o) => (o.label().to_string(), None, self.config.order == o),
                MenuAct::Fold if self.config.fold_list => {
                    ("Show the list".to_string(), Some("Ctrl B"), false)
                }
                MenuAct::Fold => ("Put the list away".to_string(), Some("Ctrl B"), false),
                MenuAct::Refresh => ("Read the desks again".to_string(), Some("R"), false),
                MenuAct::Settings => ("Settings".to_string(), Some("Ctrl ,"), false),
            };
            if act == MenuAct::Fold {
                card = card.child(rule());
            }
            let row = div()
                .id(("menu-row", i))
                .mx(px(4.))
                .h(px(tokens::CAP + 10.))
                .px(px(GAP - 4.))
                .flex()
                .items_center()
                .gap(px(GAP_SMALL))
                .rounded(px(tokens::RADIUS))
                .cursor_pointer()
                .when(i == sel, |d| d.bg(t.accent_light()))
                .hover(|d| d.bg(t.accent_soft()))
                .child(
                    div()
                        .w(px(14.))
                        .flex_none()
                        .text_color(t.accent)
                        .child(if checked { "✓" } else { "" }),
                )
                .child(div().flex_1().min_w_0().truncate().child(words))
                .children(key.map(|k| {
                    div()
                        .flex_none()
                        .text_size(px(CAP_TEXT))
                        .text_color(t.dim)
                        .child(k)
                }))
                .on_click(cx.listener(move |app, _: &ClickEvent, window, cx| {
                    app.run_menu(act, window, cx)
                }));
            card = card.child(row);
        }
        Some(
            deferred(
                div()
                    .id("menu-shade")
                    .absolute()
                    .inset_0()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|app, _, _, cx| app.open_menu(None, cx)),
                    )
                    .child(
                        anchored()
                            .position(point(at.x, at.y + px(GAP_SMALL)))
                            .snap_to_window_with_margin(px(GAP))
                            .child(card),
                    ),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }

    /// The button that opens the settings menu where it is.
    fn menu_button(&self, t: &Theme, cx: &mut Context<Self>) -> Stateful<Div> {
        icon_button(t, "menu-open", Icon::Settings, self.menu.is_some())
            // Not over the menu it opened.
            .when(self.menu.is_none(), |d| {
                d.tooltip(kit::tip("Desks, order and settings"))
            })
            .on_click(cx.listener(|app, e: &ClickEvent, _, cx| {
                let at = if app.menu.is_some() {
                    None
                } else {
                    Some(e.position())
                };
                app.open_menu(at, cx)
            }))
    }

    /// Update, when the node it was installed from hands out another
    /// build: quiet words in the action colour, at the list's foot, or
    /// beside the menu with the list away.
    fn update_button(&self, t: &Theme, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        use ahjo_app_kit::update::Tone;
        let badge = self.updates.read(cx).badge()?;
        let updates = self.updates.clone();
        Some(
            button(t, "update", badge.words, Kind::Quiet)
                .flex_none()
                .text_color(match badge.tone {
                    Tone::Action => t.accent,
                    Tone::Busy => t.dim,
                    Tone::Failed => t.error,
                })
                .tooltip(kit::tip(badge.tip))
                .when(badge.acts, |b| {
                    b.on_click(move |_, window, cx| updates.update(cx, |u, cx| u.act(window, cx)))
                }),
        )
    }

    /// The button that puts the list away or brings it back.
    fn list_button(&self, t: &Theme, cx: &mut Context<Self>) -> Stateful<Div> {
        let open = !self.config.fold_list;
        icon_button(t, "list-toggle", Icon::Sidebar, false)
            .tooltip(kit::tip(if open {
                "Put the list away · Ctrl+B"
            } else {
                "Show the list · Ctrl+B"
            }))
            .on_click(cx.listener(|app, _: &ClickEvent, _, cx| app.toggle_fold(cx)))
    }

    /// A column's edge: a line, caught a little either side of it, that
    /// drags the column wider or narrower; a double click puts it back.
    fn grip(&self, t: &Theme, grip: Grip, wide: f32, cx: &mut Context<Self>) -> Div {
        let name = match grip {
            Grip::List => "grip-list",
            Grip::Talk => "grip-talk",
        };
        let on = self.drag.is_some_and(|d| d.grip == grip);
        let lit = t.accent_soft();
        let hit = div()
            .id(name)
            .debug_selector(move || name.into())
            .group(name)
            .absolute()
            .top_0()
            .bottom_0()
            .left(px(-tokens::GRIP_HIT / 2.))
            .w(px(tokens::GRIP_HIT))
            .flex()
            .justify_center()
            .cursor_col_resize()
            .child(
                div()
                    .w(px(3.))
                    .h_full()
                    .when(on, |d| d.bg(lit))
                    .group_hover(name, |d| d.bg(lit)),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |app, e: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    if e.click_count == 2 {
                        app.reset_width(grip, cx);
                    } else {
                        app.grab(grip, f32::from(e.position.x), wide, cx);
                    }
                }),
            );
        div()
            .w(px(1.))
            .h_full()
            .flex_none()
            .bg(t.divider)
            .relative()
            .child(deferred(hit))
    }

    /// While an edge is dragged, the whole window follows the pointer, so
    /// it never slips off the thin edge.
    fn drag_cover(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.drag?;
        Some(
            deferred(
                div()
                    .id("drag-cover")
                    .absolute()
                    .inset_0()
                    .cursor_col_resize()
                    .on_mouse_move(cx.listener(|app, e: &MouseMoveEvent, window, cx| {
                        if e.pressed_button != Some(MouseButton::Left) {
                            return app.let_go(cx);
                        }
                        let wide = f32::from(window.viewport_size().width);
                        app.drag_to(f32::from(e.position.x), wide, cx)
                    }))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|app, _, _, cx| app.let_go(cx)),
                    ),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    /// Nothing to answer: every desk read and none waiting, the desks
    /// still being reached, or none of them answering.
    fn all_clear(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let failed: Vec<String> = self
            .desks
            .iter()
            .filter_map(|d| match &d.reach {
                Reach::Failed(e) => Some(format!("{}: {e}", d.name())),
                _ => None,
            })
            .collect();
        let trouble = failed
            .into_iter()
            .chain(self.notice.clone())
            .map(|f| div().text_color(t.error).child(f).into_any_element());
        let lines = |d: Div| {
            d.flex()
                .flex_col()
                .items_center()
                .gap(px(GAP_SMALL))
                .text_center()
        };
        let body = match self.reached() {
            Reached::Some => pane::vacant(
                t,
                "All clear. Nobody is waiting on you; a new ask shows here as an agent posts it.",
                Some(lines(div()).children(trouble).into_any_element()),
            )
            .into_any_element(),
            Reached::Waiting => pane::vacant(t, "Reaching the desks…", None).into_any_element(),
            Reached::None => pane::vacant(
                t,
                "No desk is answering. The Desk reads each one again when it comes back.",
                Some(
                    lines(div())
                        .children(trouble)
                        .child(div().pt(px(GAP_SMALL)).child(
                            button(t, "try-again", "Try again", Kind::Primary).on_click(
                                cx.listener(|app, _: &ClickEvent, _, cx| app.refresh_all(cx)),
                            ),
                        ))
                        .into_any_element(),
                ),
            )
            .into_any_element(),
        };
        // With the list away, its way back and the menu stand at the top.
        div()
            .size_full()
            .relative()
            .child(body)
            .when(self.config.fold_list, |d| {
                d.child(
                    div()
                        .absolute()
                        .top(px(GAP_SMALL))
                        .left(px(GAP_SMALL))
                        .flex()
                        .gap(px(2.))
                        .child(self.list_button(t, cx))
                        .children(self.update_button(t, cx))
                        .child(self.menu_button(t, cx)),
                )
            })
            .into_any_element()
    }

    // ---- The list of asks ----

    /// The list at the window's left, as the web desk has it: each ask by
    /// its kind, its title in words to read, where it comes from and how
    /// long it has waited, its picture beside. Put away with Ctrl+B.
    fn side_panel(&mut self, t: &Theme, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let queue = self.listed();
        let answered_list = self.list == List::Answered;
        let mut rows = Vec::new();
        for (i, k) in queue.iter().enumerate() {
            let Some(ask) = self.ask(k).cloned() else {
                continue;
            };
            let on = self.current.as_ref() == Some(k);
            let first = ask
                .all_media()
                .into_iter()
                .find(|m| m.kind() == MediaKind::Image)
                .cloned();
            // Answered asks are many and looked at seldom: their pictures
            // are fetched when one is opened, not for the list.
            let first = first.filter(|_| !answered_list);
            let thumb = first.and_then(|m| match self.picture(k.0, &ask, &m, cx) {
                Some(crate::app::Pic::Ready { image, w, h, .. }) => {
                    Some((image.clone(), tokens::thumb_fit(*w, *h)))
                }
                _ => None,
            });
            let desk = self.desks[k.0].name();
            let mut meta = vec![format!("{desk} {}", ask.id)];
            if ask.round() > 1 {
                meta.push(format!("round {}", ask.round()));
            }
            match ask.form() {
                Form::Grill => meta.push(format!("{} questions", ask.questions.len())),
                Form::Open if !ask.options.is_empty() => {
                    meta.push(format!("{} options", ask.options.len()))
                }
                _ => {}
            }
            let all = ask.all_media();
            if ask.doc.is_some() {
                meta.push("document".into());
            }
            if all.iter().any(|m| m.kind() == MediaKind::Page) {
                meta.push("page".into());
            }
            if all
                .iter()
                .any(|m| matches!(m.kind(), MediaKind::Video | MediaKind::OtherVideo))
            {
                meta.push("clip".into());
            }
            let (kind, when) = match &ask.answer {
                Some(a) if answered_list => (verdict_words(a, &ask), age(&a.at)),
                _ => (kind_word(&ask).to_string(), age(&ask.created_at)),
            };
            let key = k.clone();
            let url = &self.desks[k.0].place.url;
            let started = self
                .drafts
                .iter()
                .any(|((d, id, _), draft)| d == url && *id == k.1 && draft.touched());
            let thumb = thumb.filter(|_| width >= tokens::LIST_THUMBS);
            rows.push(
                div()
                    .id(("row", i))
                    .flex()
                    .gap(px(GAP))
                    .pl(px(GAP + 2.))
                    .pr(px(GAP))
                    .py(px(GAP - 2.))
                    .relative()
                    .border_b_1()
                    .border_color(t.divider)
                    .when(on, |d| {
                        d.bg(t.accent_light()).child(
                            div()
                                .absolute()
                                .left_0()
                                .top_0()
                                .bottom_0()
                                .w(px(2.))
                                .bg(t.accent),
                        )
                    })
                    .hover(|d| d.bg(t.accent_soft()))
                    .cursor_pointer()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(3.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(GAP_SMALL))
                                    .text_size(px(CAP_TEXT))
                                    .child(
                                        div()
                                            .flex_none()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(if on { t.accent } else { t.text2 })
                                            .child(t.label_case(&kind)),
                                    )
                                    .when(started, |d| {
                                        d.child(
                                            div()
                                                .min_w_0()
                                                .truncate()
                                                .text_color(t.accent)
                                                .child("· your draft"),
                                        )
                                    })
                                    .child(div().flex_1())
                                    .child(div().flex_none().text_color(t.dim).child(when)),
                            )
                            .child(
                                div()
                                    .line_clamp(3)
                                    .text_ellipsis()
                                    .font_family(t.ty.reading)
                                    .text_size(px(t.ty.read))
                                    .line_height(relative(1.3))
                                    .font_weight(if on {
                                        FontWeight::SEMIBOLD
                                    } else {
                                        FontWeight::MEDIUM
                                    })
                                    .child(ask.title.clone()),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(GAP_SMALL))
                                    .text_size(px(CAP_TEXT))
                                    .text_color(t.dim)
                                    .child(
                                        div()
                                            .flex_none()
                                            .size(px(7.))
                                            .rounded(px(2.))
                                            .bg(t.identity_of(&desk)),
                                    )
                                    .child(div().min_w_0().truncate().child(meta.join(" · "))),
                            ),
                    )
                    .when_some(thumb, |d, (image, (w, h))| {
                        d.child(
                            div()
                                .flex_none()
                                .mt(px(2.))
                                .w(px(tokens::ROW_THUMB_W))
                                .h(px(tokens::ROW_THUMB_H))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(tokens::RADIUS))
                                .overflow_hidden()
                                .bg(t.raised)
                                // The whole picture at its own shape, sized
                                // here: a portrait showed only its top half.
                                .child(img(image).w(px(w)).h(px(h)).object_fit(ObjectFit::Fill)),
                        )
                    })
                    .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| {
                        app.open(Some(key.clone()), cx)
                    })),
            );
        }
        let failed: Vec<(String, String)> = self
            .desks
            .iter()
            .filter_map(|d| match &d.reach {
                Reach::Failed(e) => Some((d.name(), e.clone())),
                _ => None,
            })
            .collect();
        div()
            .id("list")
            .w(px(width))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .bg(t.surface)
            .child(self.lists(t, cx))
            .child(
                div()
                    .id("rail")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(rows)
                    .when(queue.is_empty(), |d| {
                        d.child(div().p(px(GAP)).text_color(t.dim).child(if answered_list {
                            "Nothing answered here yet."
                        } else {
                            "Nothing waiting."
                        }))
                    }),
            )
            // A desk that does not answer says so where its asks would be.
            .children((!failed.is_empty()).then(|| {
                div()
                    .flex_none()
                    .p(px(GAP))
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .border_t_1()
                    .border_color(t.divider)
                    .text_size(px(CAP_TEXT))
                    .children(failed.into_iter().map(|(name, e)| {
                        div()
                            .flex()
                            .gap(px(GAP_SMALL))
                            .child(div().flex_none().text_color(t.error).child(name))
                            .child(div().min_w_0().truncate().text_color(t.text2).child(e))
                    }))
            }))
            // The list's head has no room to spare at its narrowest.
            .children(self.update_button(t, cx).map(|b| {
                div()
                    .flex_none()
                    .px(px(GAP_SMALL))
                    .py(px(4.))
                    .flex()
                    .border_t_1()
                    .border_color(t.divider)
                    .child(b)
            }))
            .into_any_element()
    }

    /// The list's head: what waits on him and what he answered, the
    /// menu, and the way to put the list away.
    fn lists(&mut self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let waiting = self.queue().len();
        let answered = self.answered().len();
        let known = self.reached() == Reached::Some;
        let tab = |id: &'static str, words: &'static str, n: String, on: bool, hot: bool| {
            div()
                .id(id)
                .flex_none()
                .h(px(tokens::CAP + 8.))
                .px(px(GAP_SMALL + 2.))
                .flex()
                .items_center()
                .gap(px(GAP_SMALL))
                .rounded(px(tokens::RADIUS))
                .cursor_pointer()
                .when(on, |d| d.bg(t.accent_light()).text_color(t.text))
                .when(!on, |d| {
                    d.text_color(t.text2).hover(|d| d.text_color(t.text))
                })
                .child(words)
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(if hot { t.needs } else { t.dim })
                        .child(n),
                )
        };
        div()
            .flex_none()
            .h(px(tokens::TOOLS))
            .px(px(GAP_SMALL))
            .flex()
            .items_center()
            .gap(px(2.))
            .border_b_1()
            .border_color(t.divider)
            .text_size(px(META))
            // In a narrow list the tabs give way; the buttons stay.
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .overflow_hidden()
                    .child(
                        // A count nobody read is not a zero.
                        tab(
                            "list-waiting",
                            "Waiting",
                            if known {
                                waiting.to_string()
                            } else {
                                "–".into()
                            },
                            self.list == List::Waiting,
                            known && waiting > 0,
                        )
                        .tooltip(kit::tip(format!(
                            "{} · J and K step, Ctrl+PgDn from anywhere",
                            self.config.order.label()
                        )))
                        .on_click(
                            cx.listener(|app, _: &ClickEvent, _, cx| {
                                app.set_list(List::Waiting, cx)
                            }),
                        ),
                    )
                    .child(
                        tab(
                            "list-answered",
                            "Answered",
                            answered.to_string(),
                            self.list == List::Answered,
                            false,
                        )
                        .tooltip(kit::tip("What you sent, the last first, to read again"))
                        .on_click(cx.listener(
                            |app, _: &ClickEvent, _, cx| app.set_list(List::Answered, cx),
                        )),
                    ),
            )
            .child(self.menu_button(t, cx))
            .child(self.list_button(t, cx))
    }

    // ---- The conversation ----

    fn conversation(
        &mut self,
        t: &Theme,
        ask: &Ask,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = self.current.clone();
        let desk = key
            .as_ref()
            .map(|k| self.desks[k.0].name())
            .unwrap_or_default();
        let mut turns = div()
            .flex()
            .flex_col()
            .gap(px(tokens::BETWEEN_TURNS))
            .px(px(tokens::COLUMN_PAD))
            .pt(px(tokens::COLUMN_TOP));
        let at = key.as_ref().map_or(0, |k| k.0);
        for past in &ask.history {
            let pics = self.sent_pictures(t, at, past, cx);
            turns = turns
                .child(self.agent_turn(t, past, false, window, cx))
                .children(past.answer.as_ref().map(|_| his_turn(t, past, pics)))
                .children(past.comments.iter().map(|c| comment(t, c)));
        }
        let open = ask.is_open();
        let pics = self.sent_pictures(t, at, ask, cx);
        turns = turns
            .child(self.agent_turn(t, ask, open, window, cx))
            .children(ask.comments.iter().map(|c| comment(t, c)))
            .children((!open).then(|| his_turn(t, ask, pics)));
        let folded = self.config.fold_list;
        let head = div()
            .flex_none()
            .h(px(tokens::TOOLS))
            .pl(px(if folded {
                GAP_SMALL
            } else {
                tokens::COLUMN_PAD
            }))
            .pr(px(GAP_SMALL))
            .flex()
            .items_center()
            .gap(px(GAP_SMALL))
            .bg(t.surface)
            .border_b_1()
            .border_color(t.divider)
            .text_size(px(META))
            .text_color(t.text2)
            // With the list away, its way back and the menu stand here.
            .when(folded, |d| {
                d.child(self.list_button(t, cx))
                    .children(self.update_button(t, cx))
                    .child(self.menu_button(t, cx))
                    .child(div().w(px(GAP_SMALL)))
            })
            .child(
                div()
                    .flex_none()
                    .size(px(8.))
                    .rounded(px(2.))
                    .bg(t.identity_of(&desk)),
            )
            // In a narrow window the words give way, never the button.
            .child(div().flex_1().min_w_0().truncate().child(format!(
                "{desk} {} · {} · {}",
                ask.id,
                kind_word(ask),
                ask.bead
            )))
            .child(
                icon_button(t, "web", Icon::External, false)
                    .tooltip(kit::tip("Open on the web desk · O"))
                    .on_click(cx.listener(|app, _: &ClickEvent, _, cx| app.open_on_web(cx))),
            );
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(t.bg)
            .child(head)
            .child(
                // The answer follows the question rather than waiting at
                // the window's foot; a long conversation scrolls above it.
                div()
                    .id("turns")
                    .flex_initial()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(turns),
            )
            .when(!open, |d| d.child(self.marks_list(t, ask, window, cx)))
            .child(if open {
                self.composer(t, ask, window, cx)
            } else {
                sent_note(t, ask)
            })
            .child(div().flex_1())
            .into_any_element()
    }

    /// What the agent put to him in one round: who and when, the title,
    /// the question, then its options or questions after a breath.
    fn agent_turn(
        &mut self,
        t: &Theme,
        round: &Ask,
        now: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let by = if round.asked_by.is_empty() {
            "The agent"
        } else {
            round.asked_by.as_str()
        };
        // An answered ask's own round reads as the ask, not as history.
        let past = !now
            && self
                .current_ask()
                .is_none_or(|a| a.round() != round.round());
        // The first round is not worth a word.
        let meta = if round.round() > 1 {
            format!(
                "{by} · round {} · {}",
                round.round(),
                age(&round.created_at)
            )
        } else {
            format!("{by} · {}", age(&round.created_at))
        };
        let mut turn = div()
            .flex()
            .flex_col()
            .child(
                div()
                    .text_size(px(META))
                    .text_color(t.dim)
                    .truncate()
                    .child(meta),
            )
            .child(
                div()
                    .mt(px(tokens::META_TO_TITLE))
                    .font_family(t.ty.reading)
                    .text_size(px(tokens::TITLE))
                    .font_weight(FontWeight::SEMIBOLD)
                    .line_height(relative(tokens::TITLE_LEADING))
                    .when(past, |d| d.text_color(t.text2))
                    .child(round.title.clone()),
            );
        if !round.question.trim().is_empty() && round.question.trim() != round.title.trim() {
            turn = turn.child(
                prose(t, &round.question, past)
                    .mt(px(tokens::TITLE_TO_QUESTION))
                    .text_color(t.text2),
            );
        }
        if let Some(doc) = &round.doc {
            turn = turn.child(
                div()
                    .mt(px(tokens::TITLE_TO_QUESTION))
                    .text_size(px(META))
                    .text_color(t.text2)
                    .child(format!("Document: {}", doc.name)),
            );
        }
        if past {
            let shown: Vec<String> = round
                .all_media()
                .iter()
                .map(|m| {
                    if m.caption.is_empty() {
                        m.file_name().to_string()
                    } else {
                        m.caption.clone()
                    }
                })
                .collect();
            if !shown.is_empty() {
                turn = turn.child(
                    div()
                        .mt(px(tokens::TITLE_TO_QUESTION))
                        .text_size(px(META))
                        .text_color(t.dim)
                        .child(format!("Showed: {}", shown.join(" · "))),
                );
            }
        }
        match round.form() {
            Form::Grill => {
                let at = self.draft().at;
                for (i, q) in round.questions.iter().enumerate() {
                    let section = self.question_section(
                        t,
                        round,
                        i,
                        q,
                        now && at == Some(i),
                        now,
                        window,
                        cx,
                    );
                    turn = turn.child(
                        div()
                            .mt(px(if i == 0 {
                                tokens::BEFORE_OPTIONS
                            } else {
                                tokens::BETWEEN_QUESTIONS
                            }))
                            .child(section),
                    );
                }
            }
            _ => {
                if !round.options.is_empty() {
                    turn = turn.child(
                        self.option_cards(t, round, now, cx)
                            .mt(px(tokens::BEFORE_OPTIONS)),
                    );
                } else if !round.recommend.trim().is_empty() {
                    turn = turn.child(
                        crate::panel::signed(t, &round.recommend, &crate::panel::asker(round))
                            .mt(px(tokens::BEFORE_OPTIONS)),
                    );
                }
            }
        }
        turn
    }

    // ---- His marks ----

    fn marks_list(
        &mut self,
        t: &Theme,
        ask: &Ask,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let numbered = self.numbered();
        if numbered.is_empty() {
            return div().into_any_element();
        }
        let doc = ask.doc.as_ref().map(|d| d.text.as_str());
        let items = self.items();
        let fixed = self.read_only();
        let mut rows = div().flex().flex_col();
        for (n, m) in numbered {
            let on = self.selected.as_deref() == Some(&m.id);
            let ink = rgb(tokens::ink(m.color.as_deref()));
            let place = m
                .media
                .as_ref()
                .and_then(|id| items.iter().find(|i| &i.media.id == id))
                .map(|i| {
                    if i.tag.is_empty() {
                        String::new()
                    } else {
                        format!("{} · ", i.tag)
                    }
                })
                .unwrap_or_default();
            let what = format!("{place}{}", marks::what(&m, doc));
            let id = m.id.clone();
            let gone = m.id.clone();
            let row = div()
                .id(SharedString::from(format!("mark-{}", m.id)))
                .flex()
                .flex_col()
                .gap(px(4.))
                .px(px(GAP))
                .py(px(GAP_SMALL))
                .when(on, |d| d.bg(t.accent_light()))
                .hover(|d| d.bg(t.accent_soft()))
                .cursor_pointer()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(GAP_SMALL))
                        .child(number(t, n, ink))
                        .child(div().text_size(px(META)).text_color(t.text2).child(what))
                        .child(div().flex_1())
                        .when(!fixed, |d| {
                            d.child(
                                button(
                                    t,
                                    SharedString::from(format!("drop-{}", m.id)),
                                    "Remove",
                                    Kind::Quiet,
                                )
                                .on_click(cx.listener(
                                    move |app, _: &ClickEvent, _, cx| {
                                        cx.stop_propagation();
                                        app.delete_mark(&gone, cx)
                                    },
                                )),
                            )
                        }),
                )
                .child(if on && !fixed {
                    div().child(self.mark_note.clone()).into_any_element()
                } else if m.text.trim().is_empty() {
                    div()
                        .text_color(t.dim)
                        .text_size(px(META))
                        .child("No words on it")
                        .into_any_element()
                } else {
                    prose(t, &m.text, false).line_clamp(2).into_any_element()
                })
                .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| {
                    app.select_mark(Some(id.clone()), true, cx);
                    if !app.read_only() {
                        app.aim(Aim::MarkNote, cx);
                    }
                }));
            rows = rows.child(row);
        }
        div()
            .flex_none()
            .border_t_1()
            .border_color(t.divider)
            .bg(t.surface)
            .child(
                div()
                    .px(px(GAP))
                    .pt(px(GAP_SMALL))
                    .text_size(px(META))
                    .text_color(t.dim)
                    .child(if fixed {
                        "Your marks, as sent"
                    } else {
                        "Your marks · they go with your words"
                    }),
            )
            .child(
                div()
                    .id("marks")
                    .max_h(px(tokens::MARKS_MAX_H))
                    .overflow_y_scroll()
                    .child(rows),
            )
            .into_any_element()
    }

    // ---- His words ----

    fn composer(
        &mut self,
        t: &Theme,
        ask: &Ask,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.answer_card(t, ask, window, cx)
    }
}

impl DeskApp {
    /// The pictures he pasted into the answer he is writing: each small,
    /// with a way to take it out, and those still on their way.
    pub(crate) fn pasted_strip(
        &mut self,
        t: &Theme,
        ask: &Ask,
        ids: &[String],
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        if ids.is_empty() && self.pasting == 0 {
            return None;
        }
        let at = self.current.as_ref().map_or(0, |k| k.0);
        let mut strip = div().flex().flex_wrap().items_center().gap(px(GAP_SMALL));
        for id in ids {
            let pic = self.pasted_thumb(t, at, &ask.id, id, cx);
            let gone = id.clone();
            strip = strip.child(
                div()
                    .id(SharedString::from(format!("pasted-{id}")))
                    .relative()
                    .child(pic)
                    .child(
                        div()
                            .id(SharedString::from(format!("unpaste-{id}")))
                            .absolute()
                            .top(px(-6.))
                            .right(px(-6.))
                            .size(px(18.))
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(t.raised)
                            .border_1()
                            .border_color(t.divider)
                            .text_size(px(tokens::CAP_TEXT))
                            .text_color(t.text2)
                            .cursor_pointer()
                            .hover(|d| d.text_color(t.error))
                            .tooltip(kit::tip("Take it out of the answer"))
                            .child("×")
                            .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| {
                                app.unpaste(&gone, cx)
                            })),
                    ),
            );
        }
        if self.pasting > 0 {
            strip = strip.child(
                div()
                    .h(px(tokens::PASTED_H))
                    .px(px(GAP))
                    .flex()
                    .items_center()
                    .rounded(px(tokens::RADIUS))
                    .border_1()
                    .border_color(t.divider)
                    .text_color(t.dim)
                    .child("Adding the picture…"),
            );
        }
        Some(strip)
    }

    /// The pictures that went with a sent answer, at a size to recognise.
    fn sent_pictures(
        &mut self,
        t: &Theme,
        desk: usize,
        round: &Ask,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let ids: Vec<String> = round
            .answer
            .as_ref()?
            .attachments
            .iter()
            .map(|a| a.id.clone())
            .collect();
        if ids.is_empty() {
            return None;
        }
        let mut strip = div().flex().flex_wrap().gap(px(GAP_SMALL));
        for id in &ids {
            let url = format!(
                "{}{}",
                self.desks[desk].place.url,
                crate::desk::attachment_path(&round.id, id)
            );
            strip = strip.child(
                div()
                    .id(SharedString::from(format!("sent-{}-{id}", round.id)))
                    .cursor_pointer()
                    .tooltip(kit::tip("Open it whole in the browser"))
                    .on_click(move |_, _, cx| cx.open_url(&url))
                    .child(self.pasted_thumb(t, desk, &round.id, id, cx)),
            );
        }
        Some(strip)
    }

    fn pasted_thumb(
        &mut self,
        t: &Theme,
        desk: usize,
        ask: &str,
        id: &str,
        cx: &mut Context<Self>,
    ) -> Div {
        let h = tokens::PASTED_H;
        let frame = div()
            .h(px(h))
            .rounded(px(tokens::RADIUS))
            .overflow_hidden()
            .border_1()
            .border_color(t.divider)
            .bg(t.raised);
        match self.pasted_picture(desk, ask, id, cx) {
            Some(crate::app::Pic::Ready {
                image, w, h: ih, ..
            }) => {
                let w = (w / ih.max(1.) * h).clamp(h * 0.5, h * 3.);
                frame
                    .w(px(w))
                    .child(img(image.clone()).size_full().object_fit(ObjectFit::Cover))
            }
            Some(crate::app::Pic::Failed(e)) => frame
                .w(px(h * 1.5))
                .p(px(4.))
                .text_size(px(tokens::CAP_TEXT))
                .text_color(t.error)
                .child(e.clone()),
            _ => frame.w(px(h * 1.5)),
        }
    }
}

/// How a hands job's check came out, or that it runs.
pub(crate) fn check_block(t: &Theme, c: &model::Check) -> Div {
    let (head, ink) = match (c.running, c.ok) {
        (true, _) => ("Checking out/… up to a minute".to_string(), t.text2),
        (_, true) => (format!("Check passed · {}", c.summary), t.done),
        _ => (format!("Check failed · {}", c.summary), t.error),
    };
    div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .p(px(GAP_SMALL))
        .rounded(px(tokens::RADIUS))
        .border_1()
        .border_color(alpha(ink, 0.5))
        .child(div().text_size(px(META)).text_color(ink).child(head))
        .when(!c.running, |d| {
            d.children(c.lines.iter().map(|l| {
                div()
                    .text_size(px(META))
                    .text_color(if l.ok { t.text2 } else { t.error })
                    .child(format!("{} {}", if l.ok { "✓" } else { "✗" }, l.text))
            }))
        })
}

pub(crate) fn answer_unclear_tip() -> String {
    "The question does not say enough to answer: it goes back to be asked again".into()
}

/// His answer to a round, as the desk keeps it: his turn in the dialogue.
fn his_turn(t: &Theme, round: &Ask, pictures: Option<Div>) -> AnyElement {
    let Some(a) = &round.answer else {
        return div().into_any_element();
    };
    let what = verdict_words(a, round);
    let doc = round.doc.as_ref().map(|d| d.text.as_str());
    let marks: Vec<String> = round
        .marks
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let words = m.text.trim();
            format!(
                "#{} {}{}",
                i + 1,
                marks::what(m, doc),
                if words.is_empty() {
                    String::new()
                } else {
                    format!(": {words}")
                }
            )
        })
        .collect();
    div()
        .ml(px(tokens::REPLY_INDENT))
        .flex()
        .flex_col()
        .gap(px(4.))
        .p(px(GAP))
        .rounded(px(tokens::RADIUS))
        .bg(t.accent_light())
        .border_1()
        .border_color(alpha(t.accent, 0.35))
        .child(
            div()
                .text_size(px(META))
                .text_color(t.text2)
                .child(format!("You · {what} · {}", age(&a.at))),
        )
        .when(!a.comment.trim().is_empty(), |d| {
            d.child(prose(t, &a.comment, false))
        })
        .children(pictures)
        .children(
            round
                .check
                .as_ref()
                .filter(|c| !c.running)
                .map(|c| check_block(t, c)),
        )
        .children(
            marks
                .into_iter()
                .map(|m| div().text_size(px(META)).text_color(t.text2).child(m)),
        )
        .into_any_element()
}

/// What his answer came to, in a few words.
fn verdict_words(a: &model::Answer, round: &Ask) -> String {
    match a.verdict.as_str() {
        "pick" => format!("Chose {}", a.options.join(", ")),
        "accept" => "Looks good".into(),
        "send back" => "Sent back".into(),
        "acknowledge" => "Done".into(),
        "reject" => "Rejected".into(),
        "marks" => match (round.marks.len(), a.attachments.len()) {
            (0, 0) => "Your words".into(),
            (0, 1) => "Your words and a picture".into(),
            (0, n) => format!("Your words and {n} pictures"),
            (1, _) => "Your words and a mark".into(),
            (n, _) => format!("Your words and {n} marks"),
        },
        "elsewhere" => "Answered elsewhere".into(),
        "grill" => format!("Answered {} of {}", a.answers.len(), round.questions.len()),
        v => v.to_string(),
    }
}

/// Where the words go on an ask already answered: that it was sent, and
/// when.
fn sent_note(t: &Theme, ask: &Ask) -> AnyElement {
    let when = ask.answer.as_ref().map(|a| age(&a.at)).unwrap_or_default();
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap(px(GAP_SMALL))
        .p(px(GAP))
        .border_t_1()
        .border_color(t.divider)
        .bg(t.surface)
        .text_size(px(META))
        .text_color(t.text2)
        .child(div().size(px(tokens::DOT)).rounded_full().bg(t.done))
        .child(format!("Sent {when} · read only"))
        .into_any_element()
}

/// A word in the thread between rounds: his to the right, the agent's
/// to the left.
fn comment(t: &Theme, c: &model::Comment) -> AnyElement {
    let his = c.author == "WisQi";
    div()
        .when(his, |d| d.ml(px(tokens::REPLY_INDENT)))
        .when(!his, |d| d.mr(px(tokens::REPLY_INDENT)))
        .flex()
        .flex_col()
        .gap(px(2.))
        .p(px(GAP_SMALL + 2.))
        .rounded(px(tokens::RADIUS))
        .border_1()
        .border_color(t.divider)
        .child(div().text_size(px(META)).text_color(t.dim).child(format!(
            "{} · {}",
            if his { "You" } else { c.author.as_str() },
            age(&c.at)
        )))
        .child(prose(t, &c.text, false))
        .into_any_element()
}

fn no_desks(t: &Theme) -> AnyElement {
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(GAP))
        .child(div().text_size(px(tokens::TITLE)).font_weight(FontWeight::SEMIBOLD).child("No desks yet"))
        .child(div().text_color(t.text2).child("Name one with --desk Name=http://127.0.0.1:8774, AHJO_DESK_DESKS, or \"desks\" in desk.json: the others on its host are found from it."))
        .into_any_element()
}

/// Words to read: the theme's reading face.
fn prose(t: &Theme, words: &str, faint: bool) -> Div {
    div()
        .font_family(t.ty.reading)
        .text_size(px(t.ty.read))
        .line_height(relative(ty::READ_LEADING))
        .text_color(if faint { t.text2 } else { t.text })
        .child(words.trim().to_string())
}

/// A small toggle: on, filled; a dot in `state`'s colour when given.
pub(crate) fn chip(
    t: &Theme,
    id: impl Into<gpui::ElementId>,
    words: &str,
    on: bool,
    state: Option<Hsla>,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(4.))
        .h(px(tokens::CAP + 4.))
        .px(px(GAP_SMALL + 2.))
        .rounded(px(tokens::RADIUS))
        .border_1()
        .border_color(if on { t.accent } else { t.divider })
        .when(on, |d| d.bg(t.accent).text_color(t.on_accent()))
        .when(!on, |d| d.hover(|d| d.bg(t.accent_soft())))
        .cursor_pointer()
        .text_size(px(META))
        .max_w_full()
        .children(state.map(|c| div().size(px(6.)).rounded_full().bg(c)))
        // A name longer than the column ends in an ellipsis, not past it.
        .child(div().min_w_0().truncate().child(words.to_string()))
}

/// A number on a mark's ink, as the surface draws it.
pub(crate) fn number(t: &Theme, n: usize, ink: Hsla) -> Div {
    div()
        .flex_none()
        .size(px(tokens::BADGE))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(ink)
        .text_color(t.term_bg)
        .text_size(px(tokens::BADGE_TEXT))
        .font_weight(FontWeight::BOLD)
        .child(n.to_string())
}

/// A square button holding an icon: lit when `on`; its name and key go
/// in its tooltip.
pub fn icon_button(
    t: &Theme,
    id: impl Into<gpui::ElementId>,
    icon: Icon,
    on: bool,
) -> Stateful<Div> {
    let (ink, hover) = (t.text2, t.text);
    div()
        .id(id)
        .flex_none()
        .size(px(tokens::ICON_BUTTON))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(tokens::RADIUS + 1.))
        .cursor_pointer()
        .when(on, |d| {
            d.bg(t.accent).child(icon.draw(tokens::ICON, t.on_accent()))
        })
        .when(!on, |d| {
            d.hover(|d| d.bg(t.accent_soft())).group("icon").child(
                icon.draw(tokens::ICON, ink)
                    .group_hover("icon", |d| d.text_color(hover)),
            )
        })
}

/// The kit's button, its words at the Desk's size.
pub fn button(
    t: &Theme,
    id: impl Into<gpui::ElementId>,
    text: impl Into<SharedString>,
    kind: Kind,
) -> Stateful<Div> {
    kit::button(t, id, text, kind).text_size(px(META))
}

/// The recommendation's reason, after the option's name it starts with,
/// as a sentence of its own.
pub(crate) fn rest_of(recommend: &str, name: &str) -> String {
    let rest = recommend
        .trim()
        .strip_prefix(name)
        .map(|r| r.trim_start_matches([':', '.', ',', '—', '-', ' ']))
        .unwrap_or(recommend);
    let mut letters = rest.chars();
    match letters.next() {
        Some(first) => first.to_uppercase().chain(letters).collect(),
        None => String::new(),
    }
}

fn kind_word(ask: &Ask) -> &'static str {
    match ask.kind.as_str() {
        "decide" => "Decide",
        "look" => "Look",
        "do" => "Do",
        "grill" => "Grill",
        "hands" => "Hands",
        _ => "Ask",
    }
}

/// How long ago, in the largest unit that says it.
fn age(at: &str) -> String {
    let Ok(at) = time::OffsetDateTime::parse(at, &time::format_description::well_known::Rfc3339)
    else {
        return String::new();
    };
    let mins = (time::OffsetDateTime::now_utc() - at)
        .whole_minutes()
        .max(0);
    match mins {
        0 => "just now".into(),
        m if m < 60 => format!("{m} min ago"),
        m if m < 60 * 48 => format!("{} h ago", m / 60),
        m => format!("{} d ago", m / (60 * 24)),
    }
}
