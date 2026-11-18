//! How the window is laid out: the queue on the left, the surface (what
//! the agent published, where he marks) in the middle, and the
//! conversation on the right: each round as a turn of the agent's, his
//! answer to it as his turn, their comments between, and at its foot his
//! words, always there, with his marks above them. Options and the
//! agent's recommendation stand in the agent's turn as an aid: he can lean
//! to one, but his words are the answer.

use crate::answer::{Said, Verdict};
use crate::app::{Aim, DeskApp, Reach, Reached};
use crate::marks;
use crate::model::{self, Ask, Choice, Form, MediaKind, Question};
use crate::tokens::{self, BODY, GAP, GAP_SMALL, PAD, SMALL, TEXT};
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
        let body = if self.desks.is_empty() {
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
                Some(ask) => div()
                    .size_full()
                    .flex()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .child(self.stage(window, cx)),
                    )
                    .child(
                        div()
                            .w(px(column_w))
                            .flex_none()
                            .h_full()
                            .border_l_1()
                            .border_color(t.divider)
                            .child(self.conversation(&t, &ask, window, cx)),
                    )
                    .into_any_element(),
            };
            div()
                .size_full()
                .flex()
                .child(self.rail(&t, rail_w, cx))
                .child(div().flex_1().min_w_0().h_full().child(main))
                .into_any_element()
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
            .text_size(px(TEXT))
            .child(self.header(&t, cx))
            .child(div().flex_1().min_h_0().child(body))
    }
}

impl DeskApp {
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
                kit::button(t, "refresh", "Refresh", Kind::Quiet)
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
                            kit::button(t, "try-again", "Try again", Kind::Primary).on_click(
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
        let queue = self.queue();
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
            let thumb = first.and_then(|m| match self.picture(k.0, &ask, &m, cx) {
                Some(crate::app::Pic::Ready { image, .. }) => Some(image.clone()),
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
            meta.push(age(&ask.created_at));
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
                                            .text_size(px(TEXT))
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
                            .when_some(thumb, |d, image| {
                                d.child(
                                    div()
                                        .flex_none()
                                        .w(px(tokens::ROW_THUMB_W))
                                        .h(px(tokens::ROW_THUMB_H))
                                        .rounded(px(tokens::RADIUS))
                                        .overflow_hidden()
                                        .bg(t.raised)
                                        .child(img(image).size_full().object_fit(ObjectFit::Cover)),
                                )
                            }),
                    )
                    // Under the title and its picture, so the picture never
                    // cuts what the ask is and how long it has waited.
                    .child(
                        div()
                            .text_size(px(TEXT))
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
            .child(
                div()
                    .px(px(GAP))
                    .py(px(GAP_SMALL))
                    .flex()
                    .items_center()
                    .gap(px(GAP_SMALL))
                    .text_size(px(SMALL))
                    .text_color(t.dim)
                    .child("Quickest first")
                    .child(div().flex_1())
                    .child("Ctrl")
                    .child(cap(t, "PgDn")),
            )
            .child(
                div()
                    .id("rail")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(rows),
            )
            .into_any_element()
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
        for past in &ask.history {
            turns = turns
                .child(self.agent_turn(t, past, false, cx))
                .children(past.answer.as_ref().map(|_| his_turn(t, past)))
                .children(past.comments.iter().map(|c| comment(t, c)));
        }
        turns = turns
            .child(self.agent_turn(t, ask, true, cx))
            .children(ask.comments.iter().map(|c| comment(t, c)));
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
            .text_size(px(TEXT))
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
                kit::button(t, "web", "Open on web", Kind::Quiet)
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
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(turns),
            )
            .child(self.marks_list(t, ask, window, cx))
            .child(self.composer(t, ask, window, cx))
            .into_any_element()
    }

    /// What the agent put to him in one round.
    fn agent_turn(&mut self, t: &Theme, round: &Ask, now: bool, cx: &mut Context<Self>) -> Div {
        let by = if round.asked_by.is_empty() {
            "The agent"
        } else {
            round.asked_by.as_str()
        };
        let mut turn = div()
            .flex()
            .flex_col()
            .gap(px(GAP_SMALL + 2.))
            .child(div().text_size(px(TEXT)).text_color(t.dim).child(format!(
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
                    .when(!now, |d| d.text_color(t.text2))
                    .child(round.title.clone()),
            );
        if !round.question.trim().is_empty() && round.question.trim() != round.title.trim() {
            turn = turn.child(prose(t, &round.question, !now));
        }
        if let Some(doc) = &round.doc {
            turn = turn.child(
                div()
                    .text_size(px(TEXT))
                    .text_color(t.text2)
                    .child(format!("Document: {}", doc.name)),
            );
        }
        if !now {
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
                        .text_size(px(TEXT))
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
                row.id(("option", i))
                    .cursor_pointer()
                    .hover(|d| d.bg(t.accent_soft()))
                    .tooltip(kit::tip(if picked == Some(i) {
                        "You lean to this one: click again to take it back"
                    } else {
                        "Lean to this one: it goes with your words"
                    }))
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
            card = card.child(div().text_size(px(TEXT)).text_color(t.dim).child(format!(
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
        card = card.child(div().text_size(px(TEXT)).text_color(colour).child(if now {
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
                        .child(div().text_size(px(TEXT)).text_color(t.text2).child(what))
                        .child(div().flex_1())
                        .child(
                            kit::button(
                                t,
                                SharedString::from(format!("drop-{}", m.id)),
                                "Remove",
                                Kind::Quiet,
                            )
                            .text_size(px(SMALL))
                            .on_click(cx.listener(
                                move |app, _: &ClickEvent, _, cx| {
                                    cx.stop_propagation();
                                    app.delete_mark(&gone, cx)
                                },
                            )),
                        ),
                )
                .child(if on {
                    div().child(self.mark_note.clone()).into_any_element()
                } else if m.text.trim().is_empty() {
                    div()
                        .text_color(t.dim)
                        .text_size(px(TEXT))
                        .child("No words on it yet")
                        .into_any_element()
                } else {
                    prose(t, &m.text, false).line_clamp(2).into_any_element()
                })
                .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| {
                    app.select_mark(Some(id.clone()), true, cx);
                    app.aim(Aim::MarkNote, cx);
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
                    .text_size(px(TEXT))
                    .text_color(t.dim)
                    .child("Your marks · they go with your words"),
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
        } else if form == Form::Open && !ask.options.is_empty() {
            has_top = true;
            top = top.child(div().text_size(px(TEXT)).text_color(t.dim).child("Lean to"));
            for (i, _) in ask.options.iter().enumerate() {
                let letter = model::letter(&ask.options, i);
                let label = model::label(&ask.options, i);
                top = top.child(
                    chip(
                        t,
                        SharedString::from(format!("lean-{i}")),
                        &format!("{letter} {}", clip_words(&label, 18)),
                        draft.pick == Some(i),
                        None,
                    )
                    .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| app.pick(i, cx))),
                );
            }
            top = top.child(
                div()
                    .text_size(px(TEXT))
                    .text_color(t.dim)
                    .child("· optional"),
            );
        }
        let marks = draft.marks(ask).len();
        let send_word = match (form, draft.pick.and_then(|i| ask.options.get(i).map(|_| i))) {
            (Form::Hands, _) => "Send back".to_string(),
            (Form::Grill, _) => {
                format!("Send · {} of {}", draft.answered(ask), ask.questions.len())
            }
            (_, Some(i)) => format!("Send · {}", model::letter(&ask.options, i)),
            _ => "Send".to_string(),
        };
        let mut actions = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(GAP_SMALL))
            .child(
                action(t, "send", &send_word, "Ctrl Enter", Kind::Primary)
                    .when(self.sending, |d| d.opacity(0.5))
                    .on_click(cx.listener(|app, _: &ClickEvent, window, cx| {
                        app.send(Verdict::Send, window, cx)
                    })),
            );
        if form == Form::Open {
            actions = actions.child(
                kit::button(t, "good", "Looks good", Kind::Plain)
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
                kit::button(t, "rest", "Rest as recommended", Kind::Plain)
                    .tooltip(kit::tip(
                        "Every question you left takes the agent's recommendation",
                    ))
                    .on_click(cx.listener(|app, _: &ClickEvent, window, cx| {
                        app.send(Verdict::GrillRest, window, cx)
                    })),
            );
        }
        actions = actions.child(div().flex_1()).child(
            kit::button(t, "elsewhere", "Answered elsewhere", Kind::Quiet)
                .tooltip(kit::tip(
                    "You told the agent in its session or by voice: this closes the ask with your words as a note",
                ))
                .on_click(cx.listener(|app, _: &ClickEvent, window, cx| app.send(Verdict::Elsewhere, window, cx))),
        );
        let status = match marks {
            0 => String::new(),
            1 => "1 mark goes with it".into(),
            n => format!("{n} marks go with it"),
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
            .child(
                div()
                    .id("words")
                    .max_h(px(tokens::WORDS_MAX_H))
                    .overflow_y_scroll()
                    .child(self.words.clone()),
            )
            .child(actions)
            .when(!status.is_empty(), |d| {
                d.child(div().text_size(px(TEXT)).text_color(t.text2).child(status))
            })
            .into_any_element()
    }
}

fn answer_unclear_tip() -> String {
    "The question does not say enough to answer: it goes back to be asked again".into()
}

/// His answer to a round, as the desk keeps it: his turn in the dialogue.
fn his_turn(t: &Theme, round: &Ask) -> AnyElement {
    let Some(a) = &round.answer else {
        return div().into_any_element();
    };
    let what = match a.verdict.as_str() {
        "pick" => format!("Chose {}", a.options.join(", ")),
        "accept" => "Looks good".into(),
        "send back" => "Sent back".into(),
        "acknowledge" => "Done".into(),
        "reject" => "Rejected".into(),
        "marks" => "His words and marks".into(),
        "elsewhere" => "Answered elsewhere".into(),
        "grill" => format!("Answered {} of {}", a.answers.len(), round.questions.len()),
        v => v.to_string(),
    };
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
                .text_size(px(TEXT))
                .text_color(t.text2)
                .child(format!("You · {what} · {}", age(&a.at))),
        )
        .when(!a.comment.trim().is_empty(), |d| {
            d.child(prose(t, &a.comment, false))
        })
        .children(
            marks
                .into_iter()
                .map(|m| div().text_size(px(TEXT)).text_color(t.text2).child(m)),
        )
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
        .child(div().text_size(px(TEXT)).text_color(t.dim).child(format!(
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
                                    .text_size(px(TEXT))
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
                        .text_size(px(TEXT))
                        .text_color(t.accent)
                        .child(label.to_string()),
                )
                .child(prose(t, words, false)),
        )
}

/// A small toggle: on, filled; a dot in `state`'s colour when given.
fn chip(
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
        .text_size(px(TEXT))
        .children(state.map(|c| div().size(px(6.)).rounded_full().bg(c)))
        .child(words.to_string())
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

/// A key cap: the key that does it.
fn cap(t: &Theme, key: &str) -> Div {
    div()
        .flex_none()
        .min_w(px(tokens::CAP))
        .h(px(tokens::CAP))
        .px(px(5.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(3.))
        .border_1()
        .border_color(t.divider)
        .text_size(px(SMALL))
        .text_color(t.text2)
        .child(key.to_string())
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
        .text_size(px(TEXT))
        .child(letter.to_string())
}

/// A button with its key beside its words.
fn action(t: &Theme, id: &'static str, words: &str, key: &str, kind: Kind) -> Stateful<Div> {
    let on_fill = kind == Kind::Primary;
    kit::button(t, id, words.to_string(), kind)
        .h(px(tokens::CAP + 12.))
        .px(px(GAP))
        .gap(px(GAP_SMALL))
        .child(
            div()
                .text_size(px(SMALL))
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
