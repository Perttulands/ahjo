//! How the window is laid out: the queue on the left, the surface (what
//! the agent published, where he marks) in the middle, and the
//! conversation on the right: each round as a turn of the agent's, his
//! answer to it as his turn, their comments between, and at its foot his
//! words, always there, with his marks above them. Options and the
//! agent's recommendation stand in the agent's turn as an aid: he can lean
//! to one, but his words are the answer.

use crate::answer::{Said, Verdict};
use crate::app::{Aim, DeskApp, List, Reach, Reached};
use crate::marks;
use crate::model::{self, Ask, Choice, Form, MediaKind, Question};
use crate::tokens::{self, BODY, CAP_TEXT, GAP, GAP_SMALL, META, PAD};
use ahjo_app_kit::theme::{FAMILY, Theme, alpha, rgb, theme, ty};
use ahjo_app_kit::widgets::kit::{self, Kind};
use ahjo_app_kit::widgets::pane;
use gpui::{
    AnyElement, ClickEvent, Context, Div, FontWeight, Hsla, IntoElement, ObjectFit, Render,
    SharedString, Stateful, StyledImage, Window, div, img, prelude::*, px, relative,
};

impl Render for DeskApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.take_aim(window, cx);
        self.keep_page();
        self.keep_drafts(cx);
        let t = theme(cx).clone();
        let wide = f32::from(window.viewport_size().width);
        let (rail_w, column_w) = tokens::columns(wide);
        let body = if self.settings_open {
            self.settings_view(&t, window, cx)
        } else if self.desks.is_empty() {
            no_desks(&t)
        } else {
            let main = match self.current_ask().cloned() {
                None => self.all_clear(&t, cx),
                Some(ask) if self.items().is_empty() => div()
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
                Some(ask) => self.proto_layout(&t, &ask, column_w, window, cx),
            };
            // PROTOTYPE (proto/desk-layout): AHJO_DESK_LAYOUT picks where
            // the queue stands.
            match layout().as_str() {
                "b" => div()
                    .size_full()
                    .flex()
                    .child(self.strip(&t, cx))
                    .child(div().flex_1().min_w_0().h_full().child(main))
                    .into_any_element(),
                "c" => div()
                    .size_full()
                    .flex()
                    .child(div().flex_1().min_w_0().h_full().child(main))
                    .child(
                        div()
                            .flex_none()
                            .border_l_1()
                            .border_color(t.divider)
                            .child(self.rail(&t, 300., cx)),
                    )
                    .into_any_element(),
                _ => div()
                    .size_full()
                    .flex()
                    .child(self.rail(&t, rail_w, cx))
                    .child(div().flex_1().min_w_0().h_full().child(main))
                    .into_any_element(),
            }
        };
        div()
            .id("desk")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|app, e, window, cx| app.on_key(e, window, cx)))
            .size_full()
            .flex()
            .flex_col()
            .bg(t.bg)
            .text_color(t.text)
            .font_family(FAMILY)
            .text_size(px(META))
            .relative()
            .child(self.header(&t, cx))
            .child(div().flex_1().min_h_0().child(body))
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

/// PROTOTYPE: which layout to draw (`AHJO_DESK_LAYOUT`): empty, today's.
fn layout() -> String {
    std::env::var("AHJO_DESK_LAYOUT").unwrap_or_default()
}

impl DeskApp {
    /// PROTOTYPE: the ask in front, with his answer central and the thing
    /// looked at beside it.
    fn proto_layout(
        &mut self,
        t: &Theme,
        ask: &Ask,
        column_w: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let stage = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(self.stage(window, cx));
        let talk = |w: f32| {
            div()
                .w(px(w))
                .flex_none()
                .h_full()
                .border_x_1()
                .border_color(t.divider)
        };
        match layout().as_str() {
            // A: the answer in the middle, what it is about to its right.
            "a" => div()
                .size_full()
                .flex()
                .child(talk(tokens::PROTO_TALK).child(self.conversation(t, ask, window, cx)))
                .child(stage)
                .into_any_element(),
            // B: the queue folded to a strip; the answer wide in the middle,
            // the picture to its right.
            "b" => div()
                .size_full()
                .flex()
                .child(talk(tokens::PROTO_TALK_WIDE).child(self.conversation(t, ask, window, cx)))
                .child(stage)
                .into_any_element(),
            // C: the picture to the left, the answer next to it in the
            // middle, the queue at the far right.
            "c" => div()
                .size_full()
                .flex()
                .child(stage)
                .child(talk(tokens::PROTO_TALK).child(self.conversation(t, ask, window, cx)))
                .into_any_element(),
            _ => div()
                .size_full()
                .flex()
                .child(stage)
                .child(
                    div()
                        .w(px(column_w))
                        .flex_none()
                        .h_full()
                        .border_l_1()
                        .border_color(t.divider)
                        .child(self.conversation(t, ask, window, cx)),
                )
                .into_any_element(),
        }
    }

    /// PROTOTYPE: the queue folded to a strip of numbers, the one in front
    /// lit; a click opens one.
    fn strip(&mut self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let listed = self.listed();
        let mut col = div()
            .w(px(tokens::PROTO_STRIP))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(GAP_SMALL))
            .py(px(GAP))
            .border_r_1()
            .border_color(t.divider)
            .bg(t.surface);
        for key in listed {
            let Some(a) = self.ask(&key) else { continue };
            let on = self.current.as_ref() == Some(&key);
            let name = self.desks[key.0].name();
            let id = a.id.clone();
            let open = key.clone();
            col = col.child(
                div()
                    .id(SharedString::from(format!("strip-{}-{id}", key.0)))
                    .w(px(tokens::PROTO_STRIP - 12.))
                    .py(px(6.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .rounded(px(tokens::RADIUS))
                    .cursor_pointer()
                    .when(on, |d| d.bg(t.accent_soft()).text_color(t.text))
                    .when(!on, |d| d.text_color(t.text2))
                    .tooltip(kit::tip(a.title.clone()))
                    .child(div().size(px(8.)).rounded(px(2.)).bg(t.identity_of(&name)))
                    .child(div().text_size(px(CAP_TEXT)).child(id))
                    .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| {
                        app.open(Some(open.clone()), cx)
                    })),
            );
        }
        col.into_any_element()
    }

    // ---- The header ----

    fn header(&mut self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let waiting = self.queue().len();
        let known = self.reached() == Reached::Some;
        let mut desks = div().flex().items_center().gap(px(GAP));
        for d in &self.desks {
            let n = d.asks.iter().filter(|a| a.is_open()).count();
            let (dot, why) = match &d.reach {
                Reach::Ok => (t.text2, format!("{} · {}", d.name(), d.place.url)),
                Reach::Loading => (t.dim, format!("{} · reaching {}", d.name(), d.place.url)),
                Reach::Failed(e) => (t.error, format!("{} · {e}", d.name())),
            };
            desks = desks.child(
                div()
                    .id(SharedString::from(format!("desk-{}", d.place.url)))
                    .flex()
                    .items_center()
                    .gap(px(GAP_SMALL))
                    .tooltip(kit::tip(why))
                    .child(div().size(px(7.)).rounded_full().bg(dot))
                    .child(div().text_color(t.text2).child(d.name()))
                    .when(n > 0, |d| {
                        d.child(div().text_color(t.dim).child(n.to_string()))
                    }),
            );
        }
        // The notice lives in the bar: a page view lies over the surface
        // and would hide anything drawn there.
        let toast = self.toast.as_ref().map(|(words, error)| {
            div()
                .min_w_0()
                .truncate()
                .px(px(GAP))
                .py(px(2.))
                .rounded(px(tokens::RADIUS))
                .bg(if *error {
                    alpha(t.error, 0.15)
                } else {
                    t.raised
                })
                .text_color(if *error { t.error } else { t.text })
                .child(words.clone())
        });
        div()
            .h(px(tokens::BAR))
            .flex_none()
            .px(px(GAP))
            .flex()
            .items_center()
            .gap(px(PAD))
            .border_b_1()
            .border_color(t.divider)
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap(px(GAP))
                    .child(
                        div()
                            .font_family(t.ty.display)
                            .font_weight(t.ty.display_weight)
                            .text_size(px(BODY))
                            .child(t.label_case("Desk")),
                    )
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap(px(GAP_SMALL))
                            .child(
                                div()
                                    .text_size(px(tokens::COUNT))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(match (waiting, known) {
                                        (_, false) => t.dim,
                                        (0, true) => t.done,
                                        _ => t.needs,
                                    })
                                    // A count nobody read is not a zero.
                                    .child(if known {
                                        waiting.to_string()
                                    } else {
                                        "–".to_string()
                                    }),
                            )
                            .child(div().text_color(t.text2).child("waiting")),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .justify_center()
                    .children(toast),
            )
            .child(desks)
            .child(
                button(t, "settings", "Settings", Kind::Quiet)
                    .when(self.settings_open, |b| b.text_color(t.accent))
                    .tooltip(kit::tip("Ctrl ,"))
                    .on_click(cx.listener(|app, _: &ClickEvent, window, cx| {
                        app.toggle_settings(window, cx)
                    })),
            )
            .child(
                button(t, "refresh", "Refresh", Kind::Quiet)
                    .tooltip(kit::tip("R"))
                    .on_click(cx.listener(|app, _: &ClickEvent, _, cx| app.refresh_all(cx))),
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
        match self.reached() {
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
        }
    }

    // ---- The queue ----

    fn rail(&mut self, t: &Theme, width: f32, cx: &mut Context<Self>) -> AnyElement {
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
            let mut meta = vec![kind_word(&ask).to_string()];
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
            match &ask.answer {
                Some(a) if answered_list => {
                    meta = vec![verdict_words(a, &ask), age(&a.at)];
                }
                _ => meta.push(age(&ask.created_at)),
            }
            let key = k.clone();
            let url = &self.desks[k.0].place.url;
            let started = self
                .drafts
                .iter()
                .any(|((d, id, _), draft)| d == url && *id == k.1 && draft.touched());
            let thumb = thumb.filter(|_| width >= tokens::RAIL_THUMBS);
            rows.push(
                div()
                    .id(("row", i))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .px(px(GAP))
                    .py(px(GAP_SMALL + 2.))
                    .border_l_2()
                    .border_color(if on {
                        t.accent
                    } else {
                        gpui::transparent_black()
                    })
                    .when(on, |d| d.bg(t.accent_light()))
                    .hover(|d| d.bg(t.accent_soft()))
                    .cursor_pointer()
                    .child(
                        div()
                            .flex()
                            .gap(px(GAP))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(GAP_SMALL))
                                            .text_size(px(META))
                                            .child(
                                                div()
                                                    .size(px(8.))
                                                    .rounded(px(2.))
                                                    .bg(t.identity_of(&desk)),
                                            )
                                            .child(
                                                div()
                                                    .text_color(t.text2)
                                                    .child(format!("{desk} {}", ask.id)),
                                            )
                                            .when(started, |d| {
                                                d.child(
                                                    div()
                                                        .text_color(t.accent)
                                                        .child("· your draft"),
                                                )
                                            }),
                                    )
                                    .child(
                                        div()
                                            .line_clamp(2)
                                            .text_ellipsis()
                                            .font_family(t.ty.reading)
                                            .text_size(px(t.ty.read))
                                            .line_height(relative(1.3))
                                            .font_weight(if on {
                                                FontWeight::SEMIBOLD
                                            } else {
                                                FontWeight::NORMAL
                                            })
                                            .child(ask.title.clone()),
                                    ),
                            )
                            .when_some(thumb, |d, (image, (w, h))| {
                                d.child(
                                    div()
                                        .flex_none()
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
                                        .child(
                                            img(image)
                                                .w(px(w))
                                                .h(px(h))
                                                .object_fit(ObjectFit::Fill),
                                        ),
                                )
                            }),
                    )
                    // Under the title and its picture, so the picture never
                    // cuts what the ask is and how long it has waited.
                    .child(
                        div()
                            .text_size(px(META))
                            .text_color(t.dim)
                            .truncate()
                            .child(meta.join(" · ")),
                    )
                    .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| {
                        app.open(Some(key.clone()), cx)
                    })),
            );
        }
        div()
            .w(px(width))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(t.divider)
            .bg(t.surface)
            .child(self.lists(t, cx))
            .child(
                div()
                    .id("rail")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(rows)
                    .when(answered_list && queue.is_empty(), |d| {
                        d.child(
                            div()
                                .p(px(GAP))
                                .text_color(t.dim)
                                .child("Nothing answered here yet."),
                        )
                    }),
            )
            .into_any_element()
    }

    /// The two lists: what waits on him, quickest first, and what he
    /// answered, the last first.
    fn lists(&mut self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let waiting = self.queue().len();
        let answered = self.answered().len();
        let tab = |id: &'static str, words: String, on: bool| {
            div()
                .id(id)
                .h(px(tokens::CAP + 4.))
                .px(px(GAP_SMALL + 2.))
                .flex()
                .items_center()
                .rounded(px(tokens::RADIUS))
                .cursor_pointer()
                .when(on, |d| d.bg(t.accent_light()).text_color(t.text))
                .when(!on, |d| d.text_color(t.dim).hover(|d| d.text_color(t.text)))
                .child(words)
        };
        div()
            .px(px(GAP_SMALL))
            .py(px(GAP_SMALL))
            .flex()
            .items_center()
            .gap(px(GAP_SMALL))
            .border_b_1()
            .border_color(t.divider)
            .text_size(px(META))
            .child(
                tab(
                    "list-waiting",
                    format!("Waiting {waiting}"),
                    self.list == List::Waiting,
                )
                .tooltip(kit::tip(
                    "Quickest first · J and K step, Ctrl+PgDn from anywhere",
                ))
                .on_click(
                    cx.listener(|app, _: &ClickEvent, _, cx| app.set_list(List::Waiting, cx)),
                ),
            )
            .child(
                tab(
                    "list-answered",
                    format!("Answered {answered}"),
                    self.list == List::Answered,
                )
                .tooltip(kit::tip("What you sent, the last first, to read again"))
                .on_click(
                    cx.listener(|app, _: &ClickEvent, _, cx| app.set_list(List::Answered, cx)),
                ),
            )
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
        let mut turns = div().flex().flex_col().gap(px(GAP + 4.)).p(px(GAP));
        let at = key.as_ref().map_or(0, |k| k.0);
        for past in &ask.history {
            let pics = self.sent_pictures(t, at, past, cx);
            turns = turns
                .child(self.agent_turn(t, past, false, cx))
                .children(past.answer.as_ref().map(|_| his_turn(t, past, pics)))
                .children(past.comments.iter().map(|c| comment(t, c)));
        }
        let open = ask.is_open();
        let pics = self.sent_pictures(t, at, ask, cx);
        turns = turns
            .child(self.agent_turn(t, ask, open, cx))
            .children(ask.comments.iter().map(|c| comment(t, c)))
            .children((!open).then(|| his_turn(t, ask, pics)));
        let head = div()
            .flex_none()
            .h(px(tokens::TOOLS))
            .px(px(GAP))
            .flex()
            .items_center()
            .gap(px(GAP_SMALL))
            .bg(t.surface)
            .border_b_1()
            .border_color(t.divider)
            .text_size(px(META))
            .text_color(t.text2)
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
                button(t, "web", "Open on web", Kind::Quiet)
                    .flex_none()
                    .tooltip(kit::tip("O · the web desk, on this ask"))
                    .on_click(cx.listener(|app, _: &ClickEvent, _, cx| app.open_on_web(cx))),
            );
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(t.bg)
            .child(head)
            .child(
                div()
                    .id("turns")
                    // PROTOTYPE B: his words follow the question, not
                    // the window's foot.
                    .when(layout() != "b", |d| d.flex_1())
                    .when(layout() == "b", |d| d.flex_shrink(1.))
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(turns),
            )
            .child(self.marks_list(t, ask, window, cx))
            .child(if open {
                self.composer(t, ask, window, cx)
            } else {
                sent_note(t, ask)
            })
            .into_any_element()
    }

    /// What the agent put to him in one round.
    fn agent_turn(&mut self, t: &Theme, round: &Ask, now: bool, cx: &mut Context<Self>) -> Div {
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
        let mut turn = div()
            .flex()
            .flex_col()
            .gap(px(GAP_SMALL + 2.))
            .child(div().text_size(px(META)).text_color(t.dim).child(format!(
                "{by} · round {} · {}",
                round.round(),
                age(&round.created_at)
            )))
            .child(
                div()
                    .font_family(t.ty.reading)
                    .text_size(px(tokens::TITLE))
                    .font_weight(FontWeight::SEMIBOLD)
                    .line_height(relative(1.25))
                    .when(past, |d| d.text_color(t.text2))
                    .child(round.title.clone()),
            );
        if !round.question.trim().is_empty() && round.question.trim() != round.title.trim() {
            turn = turn.child(prose(t, &round.question, past));
        }
        if let Some(doc) = &round.doc {
            turn = turn.child(
                div()
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
                    turn = turn.child(self.question_card(
                        t,
                        round,
                        i,
                        q,
                        now && at == Some(i),
                        now,
                        cx,
                    ));
                }
            }
            _ => {
                if !round.options.is_empty() {
                    turn = turn.child(self.options(t, round, now, cx));
                }
                if !round.recommend.trim().is_empty() && round.recommended().is_none() {
                    turn = turn.child(recommend_box(t, "The agent recommends", &round.recommend));
                }
            }
        }
        turn
    }

    /// The options as the agent made them, its recommendation marked; in
    /// the open round a click leans to one (and shows it).
    fn options(&mut self, t: &Theme, ask: &Ask, now: bool, cx: &mut Context<Self>) -> Div {
        let rec = ask.recommended();
        let picked = if now { self.draft().pick } else { None };
        let answered: Vec<String> = ask
            .answer
            .as_ref()
            .map(|a| a.options.clone())
            .unwrap_or_default();
        let mut list = div().flex().flex_col().gap(px(GAP_SMALL));
        for (i, o) in ask.options.iter().enumerate() {
            let on = picked == Some(i) || (!now && answered.contains(&o.name));
            let row = option_row(
                t,
                &ask.options,
                i,
                o,
                rec == Some(i),
                on,
                rest_of(&ask.recommend, &o.name),
            );
            list = list.child(if now {
                // No tooltip: it stood over the option just clicked, and
                // the fill and the Send button already say what a click did.
                row.id(("option", i))
                    .cursor_pointer()
                    .hover(|d| d.bg(t.accent_soft()))
                    .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| app.pick(i, cx)))
                    .into_any_element()
            } else {
                row.into_any_element()
            });
        }
        list
    }

    /// A grill's question: what it asks, its options, what he has said on
    /// it; in the open round a click puts his words on it.
    #[allow(clippy::too_many_arguments)]
    fn question_card(
        &mut self,
        t: &Theme,
        ask: &Ask,
        i: usize,
        q: &Question,
        active: bool,
        now: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let said = if now {
            self.said(q)
        } else {
            ask.answer
                .as_ref()
                .and_then(|a| a.answers.get(&q.id))
                .map(|a| Said {
                    pick: a
                        .pick
                        .as_ref()
                        .and_then(|p| q.options.iter().position(|o| &o.name == p)),
                    words: a.comment.clone(),
                    rec: a.rec,
                    unclear: false,
                })
                .unwrap_or_default()
        };
        let rec = model::recommended(&q.options, &q.recommend);
        let mut card = div()
            .flex()
            .flex_col()
            .gap(px(GAP_SMALL))
            .p(px(GAP))
            .rounded(px(tokens::RADIUS))
            .border_1()
            .border_color(if active { t.accent } else { t.divider })
            .when(active, |d| d.bg(t.accent_light()))
            .child(
                div()
                    .flex()
                    .gap(px(GAP_SMALL))
                    .child(div().text_color(t.dim).child(format!("{}.", i + 1)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .font_family(t.ty.reading)
                            .text_size(px(t.ty.read))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(q.title.clone()),
                    ),
            );
        if !q.question.trim().is_empty() && q.question.trim() != q.title.trim() {
            card = card.child(prose(t, &q.question, false));
        }
        if let Some((round, old, answer)) = ask.followed_up(q) {
            let was = answer
                .map(|a| {
                    let pick = a.pick.clone().unwrap_or_default();
                    match (pick.is_empty(), a.comment.trim().is_empty()) {
                        (false, true) => pick,
                        (true, false) => format!("“{}”", a.comment.trim()),
                        (false, false) => format!("{pick} — “{}”", a.comment.trim()),
                        _ => "nothing".into(),
                    }
                })
                .unwrap_or_else(|| "nothing".into());
            card = card.child(div().text_size(px(META)).text_color(t.dim).child(format!(
                "Round {round} asked “{}”; you said {was}",
                old.title.trim()
            )));
        }
        for (k, o) in q.options.iter().enumerate() {
            let on = said.pick == Some(k);
            let row = option_row(
                t,
                &q.options,
                k,
                o,
                rec == Some(k),
                on,
                rest_of(&q.recommend, &o.name),
            );
            card = card.child(if now {
                row.id(SharedString::from(format!("q{i}-o{k}")))
                    .cursor_pointer()
                    .hover(|d| d.bg(t.accent_soft()))
                    .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| {
                        app.goto_question(Some(i), cx);
                        app.pick(k, cx);
                    }))
                    .into_any_element()
            } else {
                row.into_any_element()
            });
        }
        if !q.recommend.trim().is_empty() && rec.is_none() {
            card = card.child(recommend_box(t, "The agent recommends", &q.recommend));
        }
        let (words, colour) = said_words(t, q, &said);
        card = card.child(div().text_size(px(META)).text_color(colour).child(if now {
            format!("You: {words}")
        } else {
            format!("You said: {words}")
        }));
        if now {
            card.id(("question", i))
                .cursor_pointer()
                .on_click(
                    cx.listener(move |app, _: &ClickEvent, _, cx| app.goto_question(Some(i), cx)),
                )
                .into_any_element()
        } else {
            card.into_any_element()
        }
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
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let draft = self.draft();
        let form = ask.form();
        let mut top = div().flex().flex_wrap().items_center().gap(px(GAP_SMALL));
        let mut has_top = false;
        if form == Form::Grill {
            has_top = true;
            top = top.child(
                chip(t, "q-round", "Whole round", draft.at.is_none(), None).on_click(
                    cx.listener(|app, _: &ClickEvent, _, cx| app.goto_question(None, cx)),
                ),
            );
            for (i, q) in ask.questions.iter().enumerate() {
                let answered = draft.said(q).answered();
                top =
                    top.child(
                        chip(
                            t,
                            SharedString::from(format!("q-{i}")),
                            &format!("{}", i + 1),
                            draft.at == Some(i),
                            answered.then_some(t.done),
                        )
                        .tooltip(kit::tip(q.title.clone()))
                        .on_click(cx.listener(
                            move |app, _: &ClickEvent, _, cx| app.goto_question(Some(i), cx),
                        )),
                    );
            }
            if let Some(q) = draft.at.and_then(|i| ask.questions.get(i)) {
                let unclear = draft.said(q).unclear;
                top = top.child(div().w(px(GAP_SMALL))).child(
                    chip(t, "unclear", "Unclear", unclear, None)
                        .tooltip(kit::tip(answer_unclear_tip()))
                        .on_click(cx.listener(|app, _: &ClickEvent, _, cx| app.toggle_unclear(cx))),
                );
            }
        } else if form == Form::Hands {
            for (side, words) in [("in", "Open in/"), ("out", "Open out/"), ("guide", "Guide")] {
                if let Some(url) = ask.links.get(side).cloned() {
                    has_top = true;
                    top = top.child(
                        chip(
                            t,
                            SharedString::from(format!("link-{side}")),
                            words,
                            false,
                            None,
                        )
                        .tooltip(kit::tip(url.clone()))
                        .on_click(move |_, _, cx| cx.open_url(&url)),
                    );
                }
            }
        } else if form == Form::Open && !ask.options.is_empty() {
            has_top = true;
            top = top.child(div().text_size(px(META)).text_color(t.dim).child("Lean to"));
            for (i, _) in ask.options.iter().enumerate() {
                let letter = model::letter(&ask.options, i);
                let label = model::label(&ask.options, i);
                top = top.child(
                    chip(
                        t,
                        SharedString::from(format!("lean-{i}")),
                        &format!("{letter} {}", clip_words(&label, tokens::LEAN_WORDS)),
                        draft.pick == Some(i),
                        None,
                    )
                    .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| app.pick(i, cx))),
                );
            }
            top = top.child(
                div()
                    .text_size(px(META))
                    .text_color(t.dim)
                    .child("· optional"),
            );
        }
        let marks = draft.marks(ask).len();
        let pasted = self.pasted_strip(t, ask, &draft.pictures, cx);
        let send_word = match (form, draft.pick.and_then(|i| ask.options.get(i).map(|_| i))) {
            (Form::Hands, _) => "Send back".to_string(),
            (Form::Grill, _) => {
                format!("Send · {} of {}", draft.answered(ask), ask.questions.len())
            }
            (_, Some(i)) => format!("Send · {}", model::letter(&ask.options, i)),
            _ => "Send".to_string(),
        };
        let hands = form == Form::Hands;
        let running = ask.check.as_ref().is_some_and(|c| c.running);
        let mut actions = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(GAP_SMALL))
            // A hands job is done by doing it: Done checks out/, and his
            // words go with it.
            .when(hands, |d| {
                d.child(
                    button(
                        t,
                        "done",
                        if running {
                            "Checking out/…"
                        } else {
                            "Done · check out/"
                        },
                        Kind::Primary,
                    )
                    .when(running, |b| b.opacity(0.6))
                    .tooltip(kit::tip(
                        "The desk checks what you put in out/; a pass answers the job",
                    ))
                    .on_click(cx.listener(|app, _: &ClickEvent, _, cx| app.hands_done(cx))),
                )
            })
            .child(
                action(
                    t,
                    "send",
                    &send_word,
                    if self.config.enter_sends {
                        "Enter"
                    } else {
                        "Ctrl Enter"
                    },
                    if hands { Kind::Plain } else { Kind::Primary },
                )
                .when(self.sending, |d| d.opacity(0.5))
                .on_click(cx.listener(|app, _: &ClickEvent, window, cx| {
                    app.send(Verdict::Send, window, cx)
                })),
            );
        if form == Form::Open {
            actions = actions.child(
                button(t, "good", "Looks good", Kind::Plain)
                    .tooltip(kit::tip(
                        "Accept it as it is; your words, if any, go with it",
                    ))
                    .on_click(cx.listener(|app, _: &ClickEvent, window, cx| {
                        app.send(Verdict::LooksGood, window, cx)
                    })),
            );
        }
        if form == Form::Grill {
            actions = actions.child(
                button(t, "rest", "Rest as recommended", Kind::Plain)
                    .tooltip(kit::tip(
                        "Every question you left takes the agent's recommendation",
                    ))
                    .on_click(cx.listener(|app, _: &ClickEvent, window, cx| {
                        app.send(Verdict::GrillRest, window, cx)
                    })),
            );
        }
        // The way out stands on the line under the actions, quiet, so the
        // actions keep one line in the column.
        let elsewhere = button(t, "elsewhere", "Answered elsewhere", Kind::Quiet)
            .flex_none()
            .tooltip(kit::tip(
                "You told the agent in its session or by voice: this closes the ask with your words as a note",
            ))
            .on_click(cx.listener(|app, _: &ClickEvent, window, cx| {
                app.send(Verdict::Elsewhere, window, cx)
            }));
        let goes: Vec<String> = [
            (marks, "mark", "marks"),
            (draft.pictures.len(), "picture", "pictures"),
        ]
        .into_iter()
        .filter(|(n, ..)| *n > 0)
        .map(|(n, one, many)| format!("{n} {}", if n == 1 { one } else { many }))
        .collect();
        let status = match goes.len() {
            0 => String::new(),
            _ if marks + draft.pictures.len() == 1 => format!("{} goes with it", goes[0]),
            _ => format!("{} go with it", goes.join(" and ")),
        };
        let status = match (self.config.tell_session, ask.session.trim()) {
            (true, s) if !s.is_empty() => {
                let told = format!("Send tells {s}");
                if status.is_empty() {
                    told
                } else {
                    format!("{status} · {told}")
                }
            }
            _ => status,
        };
        div()
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(GAP_SMALL))
            .p(px(GAP))
            .border_t_1()
            .border_color(t.divider)
            .bg(t.surface)
            .when(has_top, |d| d.child(top))
            .children(ask.check.as_ref().map(|c| check_block(t, c)))
            .child(self.words.clone())
            .children(pasted)
            .child(actions)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(GAP_SMALL))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(META))
                            .text_color(t.text2)
                            .child(status),
                    )
                    .child(elsewhere),
            )
            .into_any_element()
    }
}

impl DeskApp {
    /// The pictures he pasted into the answer he is writing: each small,
    /// with a way to take it out, and those still on their way.
    fn pasted_strip(
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
fn check_block(t: &Theme, c: &model::Check) -> Div {
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

fn answer_unclear_tip() -> String {
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

/// An option as the agent made it: its letter, name and why, the one it
/// recommends marked, the one he leans to (or chose) filled.
#[allow(clippy::too_many_arguments)]
fn option_row(
    t: &Theme,
    options: &[Choice],
    i: usize,
    o: &Choice,
    rec: bool,
    on: bool,
    why_rec: String,
) -> Div {
    let letter = model::letter(options, i);
    let label = model::label(options, i);
    div()
        .flex()
        .gap(px(GAP_SMALL + 2.))
        .p(px(GAP_SMALL))
        .rounded(px(tokens::RADIUS))
        .child(key_letter(t, &letter, on))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(GAP_SMALL))
                        .items_baseline()
                        .child(
                            div()
                                .font_family(t.ty.reading)
                                .text_size(px(t.ty.read))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(label),
                        )
                        .when(rec, |d| {
                            d.child(
                                div()
                                    .text_size(px(META))
                                    .text_color(t.accent)
                                    .child("Recommended"),
                            )
                        }),
                )
                .when(!o.why.trim().is_empty(), |d| {
                    d.child(prose(t, &o.why, true))
                })
                .when(
                    rec && !why_rec.trim().is_empty() && why_rec.trim() != o.why.trim(),
                    |d| d.child(prose(t, &why_rec, true)),
                ),
        )
}

fn recommend_box(t: &Theme, label: &str, words: &str) -> Div {
    div()
        .flex()
        .gap(px(GAP))
        .child(
            div()
                .w(px(tokens::RULE_W))
                .flex_none()
                .rounded_full()
                .bg(t.accent),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .text_size(px(META))
                        .text_color(t.accent)
                        .child(label.to_string()),
                )
                .child(prose(t, words, false)),
        )
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
fn number(t: &Theme, n: usize, ink: Hsla) -> Div {
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

/// An option's letter in its round mark, filled when picked.
fn key_letter(t: &Theme, letter: &str, on: bool) -> Div {
    div()
        .flex_none()
        .size(px(24.))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .border_1()
        .border_color(if on { t.accent } else { t.text2 })
        .when(on, |d| d.bg(t.accent).text_color(t.on_accent()))
        .font_weight(FontWeight::BOLD)
        .text_size(px(META))
        .child(letter.to_string())
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

/// A button with its key beside its words.
fn action(t: &Theme, id: &'static str, words: &str, key: &str, kind: Kind) -> Stateful<Div> {
    let on_fill = kind == Kind::Primary;
    button(t, id, words.to_string(), kind)
        .h(px(tokens::CAP + 12.))
        .px(px(GAP))
        .gap(px(GAP_SMALL))
        .child(
            div()
                .text_size(px(CAP_TEXT))
                .text_color(if on_fill {
                    alpha(t.on_accent(), 0.7)
                } else {
                    t.dim
                })
                .child(key.to_string()),
        )
}

fn said_words(t: &Theme, q: &Question, said: &Said) -> (String, Hsla) {
    if said.unclear {
        return ("unclear: asked again".into(), t.needs);
    }
    let pick = said.pick.and_then(|i| {
        q.options.get(i).map(|_| {
            format!(
                "{} {}",
                model::letter(&q.options, i),
                model::label(&q.options, i)
            )
        })
    });
    match (pick, said.words.trim()) {
        (Some(p), "") => (p, t.text),
        (Some(p), w) => (format!("{p} — “{}”", clip_words(w, 80)), t.text),
        (None, "") => ("nothing yet".into(), t.dim),
        (None, w) => (format!("“{}”", clip_words(w, 80)), t.text),
    }
}

fn clip_words(s: &str, n: usize) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= n {
        s
    } else {
        format!("{}…", s.chars().take(n).collect::<String>().trim_end())
    }
}

/// The recommendation's reason, after the option's name it starts with,
/// as a sentence of its own.
fn rest_of(recommend: &str, name: &str) -> String {
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
