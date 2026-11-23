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

use crate::answer::Gist;
use crate::app::{Aim, DeskApp, Grip, List, Reach, Reached};
use crate::icons::Icon;
use crate::marks;
use crate::model::{self, Ask, Form, MediaKind};
use crate::tokens::{self, CAP_TEXT, GAP, GAP_SMALL, META};
use ahjo_app_kit::frame::{self, Cover};
use ahjo_app_kit::panel::EdgeColumn;
use ahjo_app_kit::theme::{FAMILY, Theme, alpha, rgb, theme, ty};
use ahjo_app_kit::widgets::kit::{self, GripEdge, Kind, Phase};
use ahjo_app_kit::widgets::pane;
use gpui::{
    AnyElement, ClickEvent, Context, Div, FontWeight, Hsla, IntoElement, ObjectFit, Render,
    SharedString, Stateful, StyledImage, Window, deferred, div, img, prelude::*, px, relative,
};
use std::rc::Rc;

impl Render for DeskApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.take_aim(window, cx);
        self.keep_page();
        self.keep_drafts(cx);
        let t = theme(cx).clone();
        let wide = f32::from(window.viewport_size().width);
        let (_, talk_w) = self.columns(wide);
        let lines = tokens::words_lines(f32::from(window.viewport_size().height), t.ty.read);
        self.words.update(cx, |f, _| f.set_max_lines(lines));
        let shown = self.current_ask().cloned();
        let looked = shown.is_some() && !self.items().is_empty();
        let settings = self.settings_open(cx);
        let full = self.full && looked && !settings;
        let body = if settings {
            // Settings stand over the window: nothing of the surface is
            // drawn under them, as a page shown inside it would show
            // through.
            div().size_full().into_any_element()
        } else if self.desks.is_empty() {
            with_notices(self.notices(cx), no_desks(&t))
        } else if full {
            let stage = self.stage(window, cx);
            with_notices(self.notices(cx), stage)
        } else {
            let main = match shown {
                None => {
                    let clear = self.all_clear(&t, cx);
                    with_notices(self.notices(cx), clear)
                }
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
                    .child(self.talk_edge(&t, talk_w, wide, window, cx))
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
                .children(self.list_column(&t, wide, window, cx))
                .child(div().flex_1().min_w_0().h_full().child(main))
                .into_any_element()
        };
        let frame = self.frame.clone();
        div()
            .id("desk")
            .track_focus(&self.focus)
            // The frame's keys first: the open menu's, and Escape closing
            // what stands on top, wherever the keyboard is.
            .capture_key_down(cx.listener(|app, e, window, cx| {
                if frame::key(&app.frame, e, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .on_key_down(cx.listener(|app, e, window, cx| app.on_key(e, window, cx)))
            .size_full()
            .bg(t.bg)
            .text_color(t.text)
            .font_family(FAMILY)
            .text_size(px(META))
            .relative()
            .child(body)
            .children(frame::stack(&frame, Cover::Window, window, cx))
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
    // ---- The menu, the column edges ----

    /// The button that opens the settings menu where it is. A desk that
    /// is not running while another answers is a small dot on it, named
    /// in its tip and in the menu, not an error.
    fn menu_button(&self, t: &Theme, cx: &mut Context<Self>) -> Stateful<Div> {
        let away: Vec<String> = self
            .desks
            .iter()
            .filter(|d| matches!(d.reach, Reach::Away(_)))
            .map(|d| d.name())
            .collect();
        let tip = match away.as_slice() {
            [] => "Desks, order and settings".to_string(),
            [one] => format!("Desks, order and settings · {one} is not running"),
            [rest @ .., last] => format!(
                "Desks, order and settings · {} and {last} are not running",
                rest.join(", ")
            ),
        };
        let open = self.menu_open(cx);
        icon_button(t, "menu-open", Icon::Settings, open)
            .debug_selector(|| "menu-open".into())
            .relative()
            .child(ahjo_app_kit::pointer::watch())
            .when(!away.is_empty() && !open, |d| {
                d.child(
                    div()
                        .absolute()
                        .top(px(3.))
                        .right(px(3.))
                        .size(px(tokens::DOT))
                        .rounded_full()
                        .bg(t.dim),
                )
            })
            // Not over the menu it opened.
            .when(!open, |d| d.tooltip(kit::tip(tip)))
            .on_click(cx.listener(|app, e: &ClickEvent, _, cx| app.open_menu(e.position(), cx)))
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

    /// The edge between the answer and the surface: a line whose kit grip
    /// drags the answer wider or narrower; a double click puts it back.
    fn talk_edge(
        &self,
        t: &Theme,
        talk_w: f32,
        wide: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let me = cx.entity().downgrade();
        let grip = kit::grip(
            t,
            "grip-talk",
            GripEdge::Right,
            talk_w,
            self.column_range(Grip::Talk, wide),
            window,
            cx,
            move |width, phase, _, cx| {
                let _ = me.update(cx, |app, cx| match phase {
                    Phase::Moving => app.size_column(Grip::Talk, Some(width), false, cx),
                    Phase::Kept => app.size_column(Grip::Talk, Some(width), true, cx),
                    Phase::Reset => app.size_column(Grip::Talk, None, true, cx),
                });
            },
        )
        .debug_selector(|| "grip-talk".into());
        div()
            .w(px(1.))
            .h_full()
            .flex_none()
            .bg(t.divider)
            .relative()
            .child(deferred(grip))
    }

    /// The list of asks at the window's left, as the kit draws a column at
    /// the window's edge: its head (what waits, what he answered, the
    /// menu) and Close, which puts it away (Ctrl+B); dragged by its edge;
    /// sliding as it goes and comes back.
    fn list_column(
        &mut self,
        t: &Theme,
        wide: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let open = !self.config.fold_list;
        let share = self.list_slide.share(open, window)?;
        let width = tokens::columns(wide, true, self.config.list_w, self.config.talk_w).0;
        let range = self.column_range(Grip::List, wide);
        let first = tokens::columns(wide, true, None, self.config.talk_w).0;
        let (close, sized) = (cx.entity().downgrade(), cx.entity().downgrade());
        let column = EdgeColumn {
            id: "list",
            left: true,
            width,
            range,
            first_width: first,
            head: self.lists(t, cx).into_any_element(),
            content: self.side_panel(t, width, cx),
            focus: self.list_focus.clone(),
            back: None,
            close: Rc::new(move |_, cx| {
                let _ = close.update(cx, |app, cx| app.toggle_fold(cx));
            }),
            close_keys: "Esc, Ctrl+B",
            sized: Rc::new(move |width, keep, _, cx| {
                let _ = sized.update(cx, |app, cx| {
                    // A double click puts it back at its share of the
                    // window, which follows the window as it is sized.
                    let width = (!keep || (width - first).abs() > 0.5).then_some(width);
                    app.size_column(Grip::List, width, keep, cx)
                });
            }),
        };
        Some(column.render(t, share, window, cx))
    }

    /// Nothing to answer: every desk read and none waiting, the desks
    /// still being reached, or none of them answering. A desk away while
    /// another answers is left to the settings button.
    fn all_clear(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let none_answer = self.reached() == Reached::None;
        let failed: Vec<String> = self
            .desks
            .iter()
            .filter_map(|d| match &d.reach {
                Reach::Failed(e) => Some(format!("{}: {}", d.name(), e.message)),
                Reach::Away(e) if none_answer => Some(format!("{}: {}", d.name(), e.message)),
                _ => None,
            })
            .collect();
        let trouble = failed
            .into_iter()
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
        let many = self.many_desks();
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
            // The desk's name only tells one desk's asks from another's.
            let mut meta = if many {
                vec![format!("{} {}", self.desks[k.0].name(), ask.id)]
            } else {
                vec![ask.id.clone()]
            };
            if ask.round() > 1 {
                meta.push(format!("round {}", ask.round()));
            }
            // How many options or questions says what kind of ask it is.
            match ask.form() {
                Form::Grill => meta.push(format!("{} questions", ask.questions.len())),
                Form::Open if !ask.options.is_empty() => {
                    meta.push(format!("{} options", ask.options.len()))
                }
                Form::Hands => meta.push("by hand".into()),
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
            // What he has said on it, drawn as the panel draws it: the
            // options he chose lifted, his words in quotes. His marks are
            // counted in words; their numbered pins belong on the picture,
            // where they point at something (AH14: no little circles).
            let (gist, when) = match &ask.answer {
                Some(a) if answered_list => {
                    if a.verdict != "pick" && a.verdict != "marks" {
                        meta.push(verdict_words(a, &ask));
                    }
                    (Gist::of_answer(a, &ask), age(&a.at))
                }
                _ => {
                    let at = (self.desks[k.0].place.url.clone(), k.1.clone(), ask.round());
                    let gist = self
                        .drafts
                        .get(&at)
                        .map(|d| Gist::of_draft(d, &ask))
                        .unwrap_or_default();
                    (gist, age(&ask.created_at))
                }
            };
            match gist.marks {
                0 => {}
                1 => meta.push("1 mark".into()),
                n => meta.push(format!("{n} marks")),
            }
            meta.push(when);
            let key = k.clone();
            let thumb = thumb.filter(|_| width >= tokens::LIST_THUMBS);
            rows.push(
                crate::panel::lifted(t, on)
                    .id(("row", i))
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .px(px(GAP))
                    .py(px(GAP - 2.))
                    .when(!on, |d| {
                        d.border_color(gpui::transparent_black())
                            .bg(gpui::transparent_black())
                            .hover(|d| d.bg(t.accent_soft()))
                    })
                    .cursor_pointer()
                    // The picture stands beside the title; what he said and
                    // the quiet line under both take the row's whole width.
                    .child(
                        div()
                            .flex()
                            .gap(px(GAP))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .line_clamp(3)
                                    .text_ellipsis()
                                    .font_family(t.ty.reading)
                                    .text_size(px(t.ty.read))
                                    .line_height(relative(1.3))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(ask.title.clone()),
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
                                        // The whole picture at its own shape,
                                        // sized here: a portrait showed only
                                        // its top half.
                                        .child(
                                            img(image)
                                                .w(px(w))
                                                .h(px(h))
                                                .object_fit(ObjectFit::Fill),
                                        ),
                                )
                            }),
                    )
                    .children(
                        (!gist.picks.is_empty() || !gist.words.is_empty())
                            .then(|| said_line(t, &gist)),
                    )
                    .child(
                        div()
                            .text_size(px(CAP_TEXT))
                            .text_color(t.dim)
                            .truncate()
                            .child(meta.join(" · ")),
                    )
                    .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| {
                        app.open(Some(key.clone()), cx)
                    })),
            );
        }
        let failed: Vec<(String, String)> = self
            .desks
            .iter()
            .filter_map(|d| match &d.reach {
                Reach::Failed(e) => Some((d.name(), e.message.clone())),
                _ => None,
            })
            .collect();
        div()
            .id("list-asks")
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .flex()
                    .flex_col()
                    .child(
                        pane::scroll("rail")
                            .track_scroll(&self.rail)
                            .flex_1()
                            .p(px(tokens::LIST_PAD))
                            .gap(px(tokens::LIST_PAD / 2.))
                            .children(rows)
                            .when(queue.is_empty(), |d| {
                                d.child(div().p(px(GAP)).text_color(t.dim).child(
                                    if answered_list {
                                        "Nothing answered here yet."
                                    } else {
                                        "Nothing waiting."
                                    },
                                ))
                            }),
                    )
                    .children(fades(&self.rail, t.surface)),
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

    /// The list's head: what waits on him and what he answered, and the
    /// menu; the column's Close puts the list away.
    fn lists(&mut self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let waiting = self.queue().len();
        let known = self.reached() == Reached::Some;
        let tab = |id: &'static str, words: &'static str, n: String, on: bool, hot: bool| {
            div()
                .id(id)
                .flex_none()
                .h(px(tokens::CAP + 8.))
                .px(px(GAP_SMALL))
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
                .when(!n.is_empty(), |d| {
                    d.child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(if hot { t.needs } else { t.dim })
                            .child(n),
                    )
                })
        };
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .items_center()
            .gap(px(2.))
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
                        // What he answered is there to read again; how many
                        // is nothing he acts on, and the narrow list needs
                        // the room for the word.
                        tab(
                            "list-answered",
                            "Answered",
                            String::new(),
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
    }

    // ---- The conversation ----

    /// What the Desk has to say (an error until he closes it, a word that
    /// something happened), in the flow at the top of the answer: it
    /// pushes the answer down, never covering the list or the question.
    fn notices(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let notices = frame::notices_in(&self.frame, cx)?;
        Some(
            div()
                .flex_none()
                .px(px(tokens::COLUMN_PAD))
                .pt(px(GAP_SMALL))
                .child(notices)
                .into_any_element(),
        )
    }

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
        // Each round is one child of the scrolled column, which starts at
        // the round in front (`land`); every round keeps the same room above
        // it, so the one it starts at stands as a first round does.
        self.land(ask, cx);
        let round = || {
            div()
                .flex()
                .flex_col()
                .gap(px(tokens::BETWEEN_TURNS))
                .px(px(tokens::COLUMN_PAD))
                .pt(px(tokens::COLUMN_TOP))
        };
        let at = key.as_ref().map_or(0, |k| k.0);
        let mut rounds = vec![];
        for past in &ask.history {
            let pics = self.sent_pictures(t, at, past, cx);
            rounds.push(
                round()
                    .pb(px(tokens::BETWEEN_TURNS - tokens::COLUMN_TOP))
                    .child(self.agent_turn(t, past, false, window, cx))
                    .children(past.answer.as_ref().map(|_| his_turn(t, past, pics)))
                    .children(past.comments.iter().map(|c| comment(t, c))),
            );
        }
        let open = ask.is_open();
        let pics = self.sent_pictures(t, at, ask, cx);
        rounds.push(
            round()
                .child(self.agent_turn(t, ask, open, window, cx))
                .children(ask.comments.iter().map(|c| comment(t, c)))
                .children((!open).then(|| his_turn(t, ask, pics))),
        );
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
            // In a narrow window the words give way, never the button. The
            // desk's name only tells one desk's asks from another's.
            .child(
                div().flex_1().min_w_0().truncate().child(
                    [
                        self.many_desks().then(|| desk.clone()),
                        Some(ask.id.clone()),
                        (!ask.bead.trim().is_empty()).then(|| ask.bead.clone()),
                    ]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" · "),
                ),
            )
            // Up here, away from the answer's own buttons: it closes the
            // round on no words of the agent's, and waits out an undo.
            .when(open, |d| {
                d.child(
                    button(t, "elsewhere", "Answered elsewhere", Kind::Quiet)
                        .debug_selector(|| "elsewhere".into())
                        .tooltip(kit::tip(
                            "You told the agent in its session or by voice: this closes the ask with your words as a note, after a moment to undo",
                        ))
                        .on_click(cx.listener(|app, _: &ClickEvent, _, cx| {
                            app.send(crate::answer::Verdict::Elsewhere, cx)
                        })),
                )
            })
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
            .children(self.notices(cx))
            .child(
                // The answer follows the question rather than waiting at
                // the window's foot; a long conversation scrolls above it,
                // fading at an edge with more beyond it rather than cut.
                div()
                    .flex_initial()
                    .min_h_0()
                    .relative()
                    .flex()
                    .flex_col()
                    .child(
                        pane::scroll("turns")
                            .debug_selector(|| "turns".into())
                            .track_scroll(&self.turns)
                            .flex_initial()
                            .children(rounds),
                    )
                    .children(fades(&self.turns, t.bg)),
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

    /// Whether the asks listed come from more than one desk: only then
    /// does a desk's name tell anything.
    fn many_desks(&self) -> bool {
        let listed = self.listed();
        listed.iter().any(|k| k.0 != listed[0].0)
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
                pane::scroll("marks")
                    .max_h(px(tokens::MARKS_MAX_H))
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

/// A scrolled column's edges where it has more beyond them: its `ground`
/// over the last of what shows, so a card reads as going on rather than
/// cut through a line.
fn fades(scroll: &gpui::ScrollHandle, ground: Hsla) -> Vec<Div> {
    let at = -f32::from(scroll.offset().y);
    let most = f32::from(scroll.max_offset().y);
    let fade = |angle: f32| {
        div()
            .absolute()
            .left_0()
            .right_0()
            .h(px(tokens::FADE))
            .bg(gpui::linear_gradient(
                angle,
                gpui::linear_color_stop(alpha(ground, 0.), 0.),
                gpui::linear_color_stop(ground, 1.),
            ))
    };
    let mut fades = vec![];
    if at > 0.5 {
        fades.push(fade(0.).top_0());
    }
    if most - at > 0.5 {
        fades.push(fade(180.).bottom_0());
    }
    fades
}

/// What he said on an ask, in its row: the options he chose lifted as
/// their cards are, his words in quotes.
fn said_line(t: &Theme, g: &Gist) -> Div {
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(GAP_SMALL))
        .children(g.picks.iter().map(|name| {
            crate::panel::lifted(t, true)
                .rounded(px(tokens::RADIUS))
                .px(px(GAP_SMALL))
                .max_w_full()
                .font_family(t.ty.reading)
                .text_size(px(META))
                .line_height(relative(1.35))
                .text_color(t.text)
                .child(name.clone())
        }))
        .children((!g.words.is_empty()).then(|| {
            div()
                .min_w_0()
                .line_clamp(3)
                .text_ellipsis()
                .font_family(t.ty.reading)
                .text_size(px(META))
                .line_height(relative(1.35))
                .text_color(t.text2)
                .child(format!("“{}”", g.words))
        }))
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

/// `below` under the notices, which push it down.
fn with_notices(notices: Option<AnyElement>, below: AnyElement) -> AnyElement {
    div()
        .size_full()
        .flex()
        .flex_col()
        .children(notices)
        .child(div().flex_1().min_h_0().child(below))
        .into_any_element()
}
