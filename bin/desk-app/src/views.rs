//! How the window is laid out in each variant. Every view reads the same
//! state (`app`) and acts through the same calls; what differs is what the
//! window gives its room to, and what Enter means first.
//!
//! - **Focus**: the evidence takes the window, the ask's words and the
//!   answer sit in a band under it, the queue is a count and the next
//!   title. A grill goes one question at a time, then a summary.
//! - **Desk**: the queue on the left, the stage, the ask's words and the
//!   answer in a column on the right: for choosing what to answer.
//! - **Verdict**: the agent's recommendation leads. Its picture is on the
//!   stage, its words are the question ("Take Quiet?"), Enter agrees. A
//!   grill is the list of what the agent proposes, each changed in place.

use crate::answer::{Draft, Verdict};
use crate::app::{DeskApp, Reach, Variant};
use crate::model::{self, Ask, Choice, Form, MediaKind, Question};
use crate::tokens::{self, BODY, GAP, GAP_SMALL, PAD, SMALL, TEXT};
use gpui::{
    AnyElement, ClickEvent, Context, Div, FontWeight, IntoElement, ObjectFit, Render, SharedString,
    Stateful, StyledImage, Window, div, img, prelude::*, px, relative,
};
use parvi_app_kit::theme::{FAMILY, Theme, alpha, theme};
use parvi_app_kit::widgets::kit::{self, Kind};

impl Render for DeskApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx).clone();
        let body = if self.desks.is_empty() {
            no_desks(&t)
        } else if self.current.is_none() {
            self.all_clear(&t)
        } else {
            match self.variant {
                Variant::Focus => self.focus_view(&t, window, cx),
                Variant::Desk => self.desk_view(&t, window, cx),
                Variant::Verdict => self.verdict_view(&t, window, cx),
            }
        };
        div()
            .id("desk")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|app, e, window, cx| app.on_key(e, window, cx)))
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(t.bg)
            .text_color(t.text)
            .font_family(FAMILY)
            .text_size(px(TEXT))
            .child(self.header(&t, cx))
            .child(div().flex_1().min_h_0().child(body))
            .children(self.toast.as_ref().map(|(words, error)| {
                div()
                    .absolute()
                    .top(px(tokens::BAR + GAP_SMALL))
                    .left_0()
                    .right_0()
                    .flex()
                    .justify_center()
                    .child(
                        kit::panel(&t)
                            .px(px(GAP))
                            .py(px(GAP_SMALL))
                            .text_color(if *error { t.error } else { t.text })
                            .child(words.clone()),
                    )
            }))
    }
}

impl DeskApp {
    // ---- The header ----

    fn header(&mut self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let queue = self.queue();
        let waiting = queue.len();
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
        let mut switch = div()
            .flex()
            .items_center()
            .gap(px(2.))
            .p(px(2.))
            .rounded(px(tokens::RADIUS))
            .border_1()
            .border_color(t.divider);
        for v in Variant::ALL {
            let on = v == self.variant;
            switch = switch.child(
                kit::button(
                    t,
                    SharedString::from(v.key()),
                    v.name(),
                    if on { Kind::Primary } else { Kind::Quiet },
                )
                .tooltip(kit::tip(format!("{} · V switches", v.why())))
                .on_click(cx.listener(move |app, _: &ClickEvent, window, cx| {
                    app.set_variant(v, window, cx)
                })),
            );
        }
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
                            .font_weight(FontWeight::BOLD)
                            .text_size(px(BODY))
                            .child("Desk"),
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
                                    .text_color(if waiting > 0 {
                                        tokens::needs_you(t)
                                    } else {
                                        tokens::done(t)
                                    })
                                    .child(waiting.to_string()),
                            )
                            .child(div().text_color(t.text2).child("waiting")),
                    ),
            )
            .child(switch)
            .child(div().flex_1())
            .child(desks)
            .child(
                kit::button(t, "refresh", "Refresh", Kind::Quiet)
                    .tooltip(kit::tip("R"))
                    .on_click(cx.listener(|app, _: &ClickEvent, _, cx| app.refresh_all(cx))),
            )
    }

    fn all_clear(&self, t: &Theme) -> AnyElement {
        let reached = self.desks.iter().any(|d| matches!(d.reach, Reach::Ok));
        let failed: Vec<String> = self
            .desks
            .iter()
            .filter_map(|d| match &d.reach {
                Reach::Failed(e) => Some(format!("{}: {e}", d.name())),
                _ => None,
            })
            .collect();
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(GAP))
            .child(
                div()
                    .text_size(px(tokens::TITLE))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(if reached {
                        "All clear"
                    } else {
                        "Reaching the desks"
                    }),
            )
            .child(div().text_color(t.text2).child(if reached {
                "Nobody is waiting on you. New asks appear here as agents post them."
            } else {
                ""
            }))
            .children(
                failed
                    .into_iter()
                    .map(|f| div().text_color(t.error).child(f)),
            )
            .into_any_element()
    }

    // ---- Focus ----

    fn focus_view(&mut self, t: &Theme, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(ask) = self.current_ask().cloned() else {
            return div().into_any_element();
        };
        // Nothing to look at: the words take the window.
        let looking = !self.items().is_empty() && !self.review;
        let stage = looking.then(|| self.stage(window, cx));
        let band = match ask.form() {
            Form::Grill if self.review => self.grill_review(t, &ask, cx).into_any_element(),
            Form::Grill => self.grill_band(t, &ask, cx).into_any_element(),
            _ => self.ask_band(t, &ask, cx).into_any_element(),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(self.context_row(t, &ask))
            .children(stage.map(|s| div().flex_1().min_h_0().child(s)))
            .child(
                div()
                    .id("band")
                    .when(looking, |d| {
                        d.flex_none().max_h(px(tokens::STRIP_PANEL_MAX_H))
                    })
                    .when(!looking, |d| d.flex_1().min_h_0())
                    .overflow_y_scroll()
                    .border_t_1()
                    .border_color(t.divider)
                    .bg(t.surface)
                    .child(
                        div()
                            .w_full()
                            .when(!looking, |d| {
                                d.max_w(px(tokens::READING)).mx_auto().pt(px(PAD))
                            })
                            .child(band),
                    ),
            )
            .into_any_element()
    }

    /// Where this ask sits: its project and number, and what comes next.
    fn context_row(&self, t: &Theme, ask: &Ask) -> Div {
        let queue = self.queue();
        let at = self
            .current
            .as_ref()
            .and_then(|k| queue.iter().position(|q| q == k))
            .unwrap_or(0);
        let next = queue
            .get(at + 1)
            .and_then(|k| self.ask(k))
            .map(|a| a.title.clone());
        div()
            .h(px(tokens::BAR - 8.))
            .flex_none()
            .px(px(GAP))
            .flex()
            .items_center()
            .gap(px(GAP))
            .text_size(px(SMALL))
            .text_color(t.text2)
            .child(self.eyebrow(t, ask))
            .child(div().flex_1())
            .child(format!("{} of {}", at + 1, queue.len()))
            .children(next.map(|n| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(GAP_SMALL))
                    .max_w(px(tokens::COLUMN))
                    .child(div().text_color(t.dim).child("next"))
                    .child(div().truncate().child(n))
                    .child(cap(t, "J"))
            }))
    }

    /// Project, number, kind, round and age, in a line.
    fn eyebrow(&self, t: &Theme, ask: &Ask) -> Div {
        let desk = self
            .current
            .as_ref()
            .map(|k| self.desks[k.0].name())
            .unwrap_or_default();
        let mut bits = vec![ask.id.clone(), kind_word(ask).to_string()];
        if ask.round() > 1 {
            bits.push(format!("round {}", ask.round()));
        }
        if !ask.asked_by.is_empty() {
            bits.push(format!("from {}", ask.asked_by));
        }
        bits.push(age(&ask.created_at));
        div()
            .flex()
            .items_center()
            .gap(px(GAP_SMALL))
            .text_size(px(SMALL))
            .child(
                div()
                    .px(px(6.))
                    .rounded(px(3.))
                    .bg(t.identity_of(&desk))
                    .text_color(t.text)
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(desk),
            )
            .child(div().text_color(t.text2).child(bits.join(" · ")))
    }

    /// The ask's words and its answer, as a band under the stage.
    fn ask_band(&mut self, t: &Theme, ask: &Ask, cx: &mut Context<Self>) -> Div {
        let draft = self.draft();
        div()
            .p(px(PAD))
            .flex()
            .gap(px(PAD))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .max_w(px(tokens::COLUMN * 1.4))
                    .flex()
                    .flex_col()
                    .gap(px(GAP))
                    .child(title(t, &ask.title))
                    .when(!ask.question.is_empty() && ask.question != ask.title, |d| {
                        d.child(body(t, &ask.question))
                    })
                    .child(self.recommendation(t, ask))
                    .children(
                        ask.doc
                            .as_ref()
                            .filter(|d| !d.text.trim().is_empty())
                            .map(|d| doc_text(t, &d.name, &d.text)),
                    ),
            )
            .child(if ask.form() == Form::Web {
                div().flex_1().min_w_0().child(self.web_only(t, ask, cx))
            } else {
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(GAP))
                    .when(!ask.options.is_empty(), |d| {
                        d.child(self.options(
                            t,
                            &ask.options,
                            &ask.recommend,
                            draft.pick,
                            false,
                            cx,
                        ))
                    })
                    .child(self.note_box(t, cx))
                    .child(self.actions(t, ask, &draft, cx))
            })
    }

    fn recommendation(&self, t: &Theme, ask: &Ask) -> AnyElement {
        if ask.recommend.trim().is_empty() {
            return div().into_any_element();
        }
        recommend_box(t, "The agent recommends", &ask.recommend).into_any_element()
    }

    /// The options, lettered with the key that picks each. With pictures,
    /// each its first; without, one line each, as the film strip under the
    /// stage already shows them.
    fn options(
        &mut self,
        t: &Theme,
        options: &[Choice],
        recommend: &str,
        picked: Option<usize>,
        pictures: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let rec = model::recommended(options, recommend);
        let thumbs = pictures.then(|| self.option_pictures(options, cx));
        let mut list = div()
            .flex()
            .flex_col()
            .gap(px(if pictures { GAP_SMALL } else { 4. }));
        for (i, o) in options.iter().enumerate() {
            let on = picked == Some(i);
            let thumb = thumbs.as_ref().and_then(|v| v.get(i).cloned().flatten());
            let name = div()
                .flex()
                .items_center()
                .gap(px(GAP_SMALL))
                .child(
                    div()
                        .flex_none()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_size(px(BODY))
                        .child(model::label(options, i)),
                )
                .when(rec == Some(i), |d| {
                    d.child(
                        div()
                            .flex_none()
                            .text_size(px(SMALL))
                            .text_color(t.accent)
                            .child("Recommended"),
                    )
                });
            let words = if pictures {
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(name)
                    .when(!o.why.is_empty(), |d| {
                        d.child(div().text_color(t.text2).child(o.why.clone()))
                    })
            } else {
                name.flex_1().min_w_0().when(!o.why.is_empty(), |d| {
                    d.child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_color(t.text2)
                            .child(o.why.clone()),
                    )
                })
            };
            list = list.child(
                div()
                    .id(("option", i))
                    .flex()
                    .when(pictures, |d| d.items_start())
                    .when(!pictures, |d| d.items_center())
                    .gap(px(GAP))
                    .px(px(GAP_SMALL + 2.))
                    .py(px(if pictures { GAP_SMALL + 2. } else { 4. }))
                    .rounded(px(tokens::RADIUS))
                    .border_1()
                    .border_color(if on { t.accent } else { t.divider })
                    .when(on, |d| d.bg(t.accent_light()))
                    .cursor_pointer()
                    .hover(|d| d.bg(t.accent_soft()))
                    .child(key_letter(t, &model::letter(options, i), on))
                    .child(words)
                    .children(thumb.map(|image| {
                        img(image)
                            .flex_none()
                            .w(px(tokens::OPTION_THUMB_W))
                            .h(px(tokens::OPTION_THUMB_H))
                            .rounded(px(tokens::RADIUS))
                            .object_fit(ObjectFit::Cover)
                    }))
                    .child(cap(t, &(i + 1).to_string()))
                    .on_click(
                        cx.listener(move |app, _: &ClickEvent, window, cx| app.pick(i, window, cx)),
                    ),
            );
        }
        list
    }

    /// Each option's first picture, decoded, for lists of options.
    fn option_pictures(
        &mut self,
        options: &[Choice],
        cx: &mut Context<Self>,
    ) -> Vec<Option<std::sync::Arc<gpui::RenderImage>>> {
        let Some(key) = self.current.clone() else {
            return vec![];
        };
        let Some(ask) = self.ask(&key).cloned() else {
            return vec![];
        };
        options
            .iter()
            .map(|o| {
                let m = o.media.iter().find(|m| m.kind() == MediaKind::Image)?;
                match self.picture(key.0, &ask, m, cx)? {
                    crate::app::Pic::Ready { image, .. } => Some(image.clone()),
                    _ => None,
                }
            })
            .collect()
    }

    /// The words for the agent: the field while writing or once written,
    /// else a line saying the key that opens it.
    fn note_box(&mut self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let empty = self.note.read(cx).text.trim().is_empty();
        if !self.writing && empty {
            return div().child(
                div()
                    .id("note-closed")
                    .flex()
                    .items_center()
                    .gap(px(GAP_SMALL))
                    .px(px(GAP_SMALL))
                    .h(px(tokens::CAP + 8.))
                    .rounded(px(tokens::RADIUS))
                    .text_color(t.dim)
                    .cursor_text()
                    .hover(|d| d.bg(t.accent_soft()))
                    .child(cap(t, "N"))
                    .child("A word for the agent")
                    .on_click(cx.listener(|app, _: &ClickEvent, window, cx| {
                        let v = if app.current_ask().is_some_and(|a| a.form() == Form::Grill) {
                            Verdict::Grill
                        } else {
                            Verdict::Yes
                        };
                        app.write(v, window, cx)
                    })),
            );
        }
        div()
            .rounded(px(tokens::RADIUS))
            .border_1()
            .border_color(if self.writing { t.accent } else { t.divider })
            .bg(t.bg)
            .px(px(GAP_SMALL))
            .py(px(4.))
            .child(self.note.clone())
    }

    /// The answer's buttons, each with its key.
    fn actions(&mut self, t: &Theme, ask: &Ask, draft: &Draft, cx: &mut Context<Self>) -> Div {
        let (yes, no): (String, &str) = match ask.form() {
            Form::Pick => (
                match draft.pick.or(ask.recommended()) {
                    Some(i) => format!("Choose {}", model::letter(&ask.options, i)),
                    None => "Pick an option".into(),
                },
                "None of these",
            ),
            Form::Do => ("Done".into(), "Not yet"),
            _ => ("Looks good".into(), "Needs work"),
        };
        let sending = self.sending;
        div()
            .flex()
            .items_center()
            .gap(px(GAP))
            .child(
                action(t, "unclear", "Unclear", "U", Kind::Quiet)
                    .tooltip(kit::tip("Send it back: it is not complete in itself"))
                    .on_click(cx.listener(|app, _: &ClickEvent, window, cx| {
                        app.toggle_unclear(window, cx)
                    })),
            )
            .child(div().flex_1())
            .child(action(t, "no", no, "X", Kind::Plain).on_click(
                cx.listener(|app, _: &ClickEvent, window, cx| app.send(Verdict::No, window, cx)),
            ))
            .child(
                action(
                    t,
                    "yes",
                    &if sending { "Sending".to_string() } else { yes },
                    "Enter",
                    Kind::Primary,
                )
                .on_click(
                    cx.listener(|app, _: &ClickEvent, window, cx| {
                        app.send(Verdict::Yes, window, cx)
                    }),
                ),
            )
    }

    // ---- A grilling round, one question at a time ----

    fn grill_band(&mut self, t: &Theme, ask: &Ask, cx: &mut Context<Self>) -> Div {
        let draft = self.draft();
        let at = draft.at.min(ask.questions.len().saturating_sub(1));
        let Some(q) = ask.questions.get(at).cloned() else {
            return div();
        };
        let said = draft.said(&q);
        div()
            .p(px(PAD))
            .flex()
            .gap(px(PAD))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .max_w(px(tokens::COLUMN * 1.4))
                    .flex()
                    .flex_col()
                    .gap(px(GAP))
                    .child(self.question_chips(t, ask, &draft, cx))
                    .child(self.question_words(t, ask, &q)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(GAP))
                    .when(!q.options.is_empty(), |d| {
                        d.child(self.options(t, &q.options, &q.recommend, said.pick, false, cx))
                    })
                    .when(said.unclear, |d| {
                        d.child(
                            div()
                                .text_color(tokens::needs_you(t))
                                .child("Marked unclear: it goes back to be asked again"),
                        )
                    })
                    .child(self.note_box(t, cx))
                    .child(self.grill_actions(t, ask, &q, &draft, cx)),
            )
    }

    /// Q1 Q2 … with what is answered; a click goes there.
    fn question_chips(&self, t: &Theme, ask: &Ask, draft: &Draft, cx: &mut Context<Self>) -> Div {
        let mut row = div().flex().items_center().gap(px(4.));
        for (i, q) in ask.questions.iter().enumerate() {
            let said = draft.said(q);
            let here = i == draft.at && !self.review;
            let colour = if said.unclear {
                tokens::needs_you(t)
            } else if said.answered() {
                tokens::done(t)
            } else {
                t.dim
            };
            row = row.child(
                div()
                    .id(("q", i))
                    .px(px(6.))
                    .h(px(tokens::CAP))
                    .flex()
                    .items_center()
                    .rounded(px(3.))
                    .border_1()
                    .border_color(if here { t.accent } else { t.divider })
                    .text_size(px(SMALL))
                    .text_color(colour)
                    .font_weight(if here {
                        FontWeight::BOLD
                    } else {
                        FontWeight::NORMAL
                    })
                    .cursor_pointer()
                    .child(format!("Q{}", i + 1))
                    .tooltip(kit::tip(q.title.clone()))
                    .on_click(cx.listener(move |app, _: &ClickEvent, window, cx| {
                        app.goto_question(i, window, cx)
                    })),
            );
        }
        row.child(
            div()
                .pl(px(GAP_SMALL))
                .text_size(px(SMALL))
                .text_color(t.text2)
                .child(format!(
                    "{} of {} answered",
                    draft.answered(ask),
                    ask.questions.len()
                )),
        )
    }

    /// A question with what it needs to stand alone: the round it is in,
    /// its own words, the agent's recommendation, and what was answered to
    /// it last round.
    fn question_words(&self, t: &Theme, ask: &Ask, q: &Question) -> Div {
        let followed = ask.followed_up(q);
        let thin = q.question.trim().is_empty()
            && q.media.is_empty()
            && q.options
                .iter()
                .all(|o| o.why.is_empty() && o.media.is_empty());
        div()
            .flex()
            .flex_col()
            .gap(px(GAP))
            .child(div().text_size(px(SMALL)).text_color(t.dim).child(format!(
                "{}{}",
                ask.title,
                if ask.question.is_empty() || ask.question == ask.title {
                    String::new()
                } else {
                    format!(" — {}", ask.question)
                }
            )))
            .child(
                div()
                    .text_size(px(tokens::QUESTION_TITLE))
                    .font_weight(FontWeight::SEMIBOLD)
                    .line_height(relative(1.25))
                    .child(q.title.clone()),
            )
            .when(!q.question.is_empty(), |d| d.child(body(t, &q.question)))
            .when(!q.recommend.trim().is_empty(), |d| {
                d.child(recommend_box(t, "Recommended", &q.recommend))
            })
            .children(followed.map(|(round, old, said)| {
                let then = match said {
                    Some(s) => match &s.pick {
                        Some(p) => {
                            format!(
                                "you {} {p}",
                                if s.rec {
                                    "took the recommendation,"
                                } else {
                                    "picked"
                                }
                            ) + &if s.comment.is_empty() {
                                String::new()
                            } else {
                                format!(" — “{}”", s.comment)
                            }
                        }
                        None => format!("you wrote “{}”", s.comment),
                    },
                    None => "you skipped it".into(),
                };
                div()
                    .text_size(px(SMALL))
                    .text_color(t.text2)
                    .child(format!(
                        "Round {round}{}: {then}",
                        if old.title.trim() != q.title.trim() {
                            format!(" ({})", old.title)
                        } else {
                            String::new()
                        }
                    ))
            }))
            .when(thin, |d| {
                d.child(
                    div()
                        .text_size(px(SMALL))
                        .text_color(tokens::needs_you(t))
                        .child("This question says little on its own. U sends it back as unclear."),
                )
            })
    }

    fn grill_actions(
        &mut self,
        t: &Theme,
        ask: &Ask,
        q: &Question,
        draft: &Draft,
        cx: &mut Context<Self>,
    ) -> Div {
        let said = draft.said(q);
        let rec = model::recommended(&q.options, &q.recommend);
        let last = draft.at + 1 >= ask.questions.len();
        let enter = if said.answered() {
            if last {
                "Review".to_string()
            } else {
                "Next".to_string()
            }
        } else if let Some(r) = rec {
            format!("Take {}", model::letter(&q.options, r))
        } else if q.options.is_empty() {
            "Write".to_string()
        } else {
            "Skip".to_string()
        };
        // Two groups that wrap apart when the column is narrow: the quiet
        // ones stay left, the answer right.
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_end()
            .gap(px(GAP))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .gap(px(GAP))
                    .child(
                        action(t, "unclear", if said.unclear { "Clear" } else { "Unclear" }, "U", Kind::Quiet)
                            .tooltip(kit::tip("This question is not complete in itself: ask it again"))
                            .on_click(cx.listener(|app, _: &ClickEvent, window, cx| app.toggle_unclear(window, cx))),
                    )
                    .child(
                        action(t, "words", "Words", "W", Kind::Quiet)
                            .on_click(cx.listener(|app, _: &ClickEvent, window, cx| app.write(Verdict::Grill, window, cx))),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(GAP))
                    .child(
                        action(t, "rest", "Rest as recommended", "Shift Enter", Kind::Plain)
                            .tooltip(kit::tip("Every question left takes the agent's recommendation, and the round goes"))
                            .on_click(cx.listener(|app, _: &ClickEvent, window, cx| app.send(Verdict::GrillRest, window, cx))),
                    )
                    .child(
                        action(t, "enter", &enter, "Enter", Kind::Primary)
                            .on_click(cx.listener(|app, _: &ClickEvent, window, cx| app.accept_question(window, cx))),
                    ),
            )
    }

    /// The round as he answered it, before it goes.
    fn grill_review(&mut self, t: &Theme, ask: &Ask, cx: &mut Context<Self>) -> Div {
        let draft = self.draft();
        let mut rows = div().flex().flex_col().gap(px(GAP_SMALL));
        for (i, q) in ask.questions.iter().enumerate() {
            let said = draft.said(q);
            let (words, colour) = said_words(t, q, &said);
            rows = rows.child(
                div()
                    .id(("review", i))
                    .flex()
                    .gap(px(GAP))
                    .cursor_pointer()
                    .hover(|d| d.bg(t.accent_soft()))
                    .px(px(GAP_SMALL))
                    .py(px(2.))
                    .rounded(px(tokens::RADIUS))
                    .child(
                        div()
                            .w(px(32.))
                            .flex_none()
                            .text_color(t.dim)
                            .child(format!("Q{}", i + 1)),
                    )
                    .child(div().flex_1().min_w_0().truncate().child(q.title.clone()))
                    .child(
                        div()
                            .flex_none()
                            .max_w(px(tokens::COLUMN))
                            .truncate()
                            .text_color(colour)
                            .child(words),
                    )
                    .on_click(cx.listener(move |app, _: &ClickEvent, window, cx| {
                        app.goto_question(i, window, cx)
                    })),
            );
        }
        let n = draft.answered(ask);
        div()
            .p(px(PAD))
            .flex()
            .flex_col()
            .gap(px(GAP))
            .child(title(t, &format!("{} · your answers", ask.title)))
            .child(rows)
            .child(self.note_box(t, cx))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(GAP))
                    .child(div().text_color(t.text2).child("↑ goes back to a question"))
                    .child(div().flex_1())
                    .child(
                        action(t, "rest", "Rest as recommended", "Shift Enter", Kind::Plain)
                            .on_click(cx.listener(|app, _: &ClickEvent, window, cx| {
                                app.send(Verdict::GrillRest, window, cx)
                            })),
                    )
                    .child(
                        action(
                            t,
                            "send",
                            &format!("Send · {n} of {}", ask.questions.len()),
                            "Enter",
                            Kind::Primary,
                        )
                        .on_click(cx.listener(
                            |app, _: &ClickEvent, window, cx| app.send(Verdict::Grill, window, cx),
                        )),
                    ),
            )
    }

    // ---- Desk ----

    fn desk_view(&mut self, t: &Theme, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(ask) = self.current_ask().cloned() else {
            return div().into_any_element();
        };
        let rail = self.rail(t, cx);
        let stage = self.stage(window, cx);
        let column = match ask.form() {
            Form::Grill if self.review => self.grill_review(t, &ask, cx),
            Form::Grill => self.grill_column(t, &ask, cx),
            _ => self.ask_column(t, &ask, cx),
        };
        div()
            .size_full()
            .flex()
            .child(rail)
            .child(div().flex_1().min_w_0().h_full().child(stage))
            .child(
                div()
                    .id("column")
                    .w(px(tokens::COLUMN))
                    .flex_none()
                    .h_full()
                    .overflow_y_scroll()
                    .border_l_1()
                    .border_color(t.divider)
                    .bg(t.surface)
                    .child(column),
            )
            .into_any_element()
    }

    /// The queue, quickest first: each row its project, kind, title, age
    /// and first picture.
    fn rail(&mut self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
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
            match ask.form() {
                Form::Grill => meta.push(format!("{} questions", ask.questions.len())),
                Form::Pick => meta.push(format!("{} options", ask.options.len())),
                Form::Web => meta.push("on the web desk".into()),
                _ => {}
            }
            if ask.all_media().iter().any(|m| m.kind() == MediaKind::Video) {
                meta.push("clip".into());
            }
            meta.push(age(&ask.created_at));
            let key = k.clone();
            rows.push(
                div()
                    .id(("row", i))
                    .flex()
                    .gap(px(GAP))
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
                                    .text_size(px(SMALL))
                                    .child(
                                        div().size(px(8.)).rounded(px(2.)).bg(t.identity_of(&desk)),
                                    )
                                    .child(
                                        div()
                                            .text_color(t.text2)
                                            .child(format!("{desk} {}", ask.id)),
                                    ),
                            )
                            .child(
                                div()
                                    .line_clamp(2)
                                    .font_weight(if on {
                                        FontWeight::SEMIBOLD
                                    } else {
                                        FontWeight::NORMAL
                                    })
                                    .child(ask.title.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(SMALL))
                                    .text_color(t.dim)
                                    .truncate()
                                    .child(meta.join(" · ")),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .w(px(tokens::ROW_THUMB_W))
                            .h(px(tokens::ROW_THUMB_H))
                            .rounded(px(tokens::RADIUS))
                            .overflow_hidden()
                            .bg(t.raised)
                            .children(
                                thumb.map(|image| {
                                    img(image).size_full().object_fit(ObjectFit::Cover)
                                }),
                            ),
                    )
                    .on_click(cx.listener(move |app, _: &ClickEvent, window, cx| {
                        app.open(Some(key.clone()), window, cx)
                    })),
            );
        }
        div()
            .w(px(tokens::RAIL))
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
                    .child(cap(t, "J"))
                    .child(cap(t, "K")),
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

    fn ask_column(&mut self, t: &Theme, ask: &Ask, cx: &mut Context<Self>) -> Div {
        let draft = self.draft();
        div()
            .p(px(PAD))
            .flex()
            .flex_col()
            .gap(px(GAP))
            .child(self.eyebrow(t, ask))
            .child(title(t, &ask.title))
            .when(!ask.question.is_empty() && ask.question != ask.title, |d| {
                d.child(body(t, &ask.question))
            })
            .child(self.recommendation(t, ask))
            .children(
                ask.doc
                    .as_ref()
                    .filter(|d| !d.text.trim().is_empty())
                    .map(|d| doc_text(t, &d.name, &d.text)),
            )
            .when(ask.form() == Form::Web, |d| {
                d.child(self.web_only(t, ask, cx))
            })
            .when(!ask.options.is_empty() && ask.form() != Form::Web, |d| {
                d.child(self.options(t, &ask.options, &ask.recommend, draft.pick, true, cx))
            })
            .when(ask.form() != Form::Web, |d| {
                d.child(self.note_box(t, cx))
                    .child(self.actions(t, ask, &draft, cx))
            })
    }

    fn grill_column(&mut self, t: &Theme, ask: &Ask, cx: &mut Context<Self>) -> Div {
        let draft = self.draft();
        let at = draft.at.min(ask.questions.len().saturating_sub(1));
        let Some(q) = ask.questions.get(at).cloned() else {
            return div();
        };
        let said = draft.said(&q);
        div()
            .p(px(PAD))
            .flex()
            .flex_col()
            .gap(px(GAP))
            .child(self.eyebrow(t, ask))
            .child(self.question_chips(t, ask, &draft, cx))
            .child(self.question_words(t, ask, &q))
            .when(!q.options.is_empty(), |d| {
                d.child(self.options(t, &q.options, &q.recommend, said.pick, true, cx))
            })
            .when(said.unclear, |d| {
                d.child(
                    div()
                        .text_color(tokens::needs_you(t))
                        .child("Marked unclear: it goes back to be asked again"),
                )
            })
            .child(self.note_box(t, cx))
            .child(self.grill_actions(t, ask, &q, &draft, cx))
    }

    fn web_only(&self, t: &Theme, ask: &Ask, cx: &mut Context<Self>) -> Div {
        let what = if ask.doc.is_some() {
            "A page to read and mark: the web desk marks text."
        } else {
            "A job for your hands: the web desk runs its check."
        };
        div()
            .flex()
            .flex_col()
            .items_start()
            .gap(px(GAP))
            .child(div().text_color(t.text2).child(what))
            .child(
                action(t, "web", "Open on the web desk", "Enter", Kind::Primary)
                    .on_click(cx.listener(|app, _: &ClickEvent, _, cx| app.open_on_web(cx))),
            )
    }

    // ---- Verdict ----

    fn verdict_view(
        &mut self,
        t: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(ask) = self.current_ask().cloned() else {
            return div().into_any_element();
        };
        if ask.form() == Form::Grill {
            let stage = self.stage(window, cx);
            let list = self.proposals(t, &ask, cx);
            return div()
                .size_full()
                .flex()
                .flex_col()
                .child(self.context_row(t, &ask))
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .child(
                            div()
                                .id("proposals")
                                .w(relative(0.42))
                                .flex_none()
                                .h_full()
                                .overflow_y_scroll()
                                .border_r_1()
                                .border_color(t.divider)
                                .bg(t.surface)
                                .child(list),
                        )
                        .child(div().flex_1().min_w_0().h_full().child(stage)),
                )
                .into_any_element();
        }
        let stage = self.stage(window, cx);
        let band = self.verdict_band(t, &ask, cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(self.context_row(t, &ask))
            .child(div().flex_1().min_h_0().child(stage))
            .child(
                div()
                    .flex_none()
                    .border_t_1()
                    .border_color(t.divider)
                    .bg(t.surface)
                    .child(band),
            )
            .into_any_element()
    }

    /// "Take Quiet?": the recommendation as the question, and the answer
    /// one key away.
    fn verdict_band(&mut self, t: &Theme, ask: &Ask, cx: &mut Context<Self>) -> Div {
        let draft = self.draft();
        let rec = ask.recommended();
        let chosen = draft.pick.or(rec);
        let (headline, why): (String, String) = match (ask.form(), chosen) {
            (Form::Pick, Some(i)) => (
                format!(
                    "{} {}: {}?",
                    if draft.pick.is_some() && draft.pick != rec {
                        "Choose"
                    } else {
                        "Take"
                    },
                    model::letter(&ask.options, i),
                    model::label(&ask.options, i)
                ),
                if draft.pick.is_some() && draft.pick != rec {
                    ask.options[i].why.clone()
                } else {
                    rest_of(&ask.recommend, &ask.options[i].name)
                },
            ),
            (Form::Pick, None) => ("No recommendation: pick one".into(), String::new()),
            (Form::Web, _) => ("This one is answered on the web desk".into(), String::new()),
            _ if !ask.recommend.trim().is_empty() => {
                (format!("The agent says: {}", ask.recommend), String::new())
            }
            _ => (ask.title.clone(), String::new()),
        };
        let mut alternatives = div().flex().items_center().gap(px(GAP_SMALL));
        for (i, _) in ask.options.iter().enumerate() {
            let on = chosen == Some(i);
            alternatives = alternatives.child(
                div()
                    .id(("alt", i))
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .px(px(GAP_SMALL))
                    .h(px(tokens::CAP + 6.))
                    .rounded(px(tokens::RADIUS))
                    .border_1()
                    .border_color(if on { t.accent } else { t.divider })
                    .cursor_pointer()
                    .child(cap(t, &(i + 1).to_string()))
                    .child(
                        div()
                            .text_color(if on { t.text } else { t.text2 })
                            .child(format!(
                                "{} {}",
                                model::letter(&ask.options, i),
                                model::label(&ask.options, i)
                            )),
                    )
                    .on_click(
                        cx.listener(move |app, _: &ClickEvent, window, cx| app.pick(i, window, cx)),
                    ),
            );
        }
        let form = ask.form();
        let yes = match (self.sending, draft.pick) {
            (true, _) => "Sending".to_string(),
            (false, Some(i)) if Some(i) != rec => {
                format!("Choose {}", model::letter(&ask.options, i))
            }
            _ => "Agree".to_string(),
        };
        div()
            .p(px(PAD))
            .flex()
            .flex_col()
            .gap(px(GAP))
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(PAD))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(GAP_SMALL))
                            .child(
                                div()
                                    .text_size(px(SMALL))
                                    .text_color(t.dim)
                                    .child(ask.title.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(tokens::TITLE))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .line_height(relative(1.2))
                                    .child(headline),
                            )
                            .when(!why.is_empty(), |d| d.child(body(t, &why)))
                            .when(!ask.question.is_empty() && ask.question != ask.title, |d| {
                                d.child(div().text_color(t.text2).child(ask.question.clone()))
                            }),
                    )
                    .when(form == Form::Web, |d| {
                        d.child(
                            action(t, "web", "Open on the web desk", "Enter", Kind::Primary)
                                .on_click(
                                    cx.listener(|app, _: &ClickEvent, _, cx| app.open_on_web(cx)),
                                ),
                        )
                    })
                    .when(form != Form::Web, |d| {
                        d.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(GAP))
                                .child(
                                    action(
                                        t,
                                        "no",
                                        if form == Form::Pick {
                                            "None of these"
                                        } else {
                                            "Disagree"
                                        },
                                        "X",
                                        Kind::Plain,
                                    )
                                    .on_click(cx.listener(
                                        |app, _: &ClickEvent, window, cx| {
                                            app.send(Verdict::No, window, cx)
                                        },
                                    )),
                                )
                                .child(action(t, "yes", &yes, "Enter", Kind::Primary).on_click(
                                    cx.listener(|app, _: &ClickEvent, window, cx| {
                                        app.send(Verdict::Yes, window, cx)
                                    }),
                                )),
                        )
                    }),
            )
            .when(ask.options.len() > 1, |d| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(GAP))
                        .child(alternatives)
                        .child(div().flex_1())
                        .child(
                            div()
                                .text_size(px(SMALL))
                                .text_color(t.dim)
                                .child("C sets them side by side · F swaps"),
                        ),
                )
            })
            .when(form != Form::Web, |d| d.child(self.note_box(t, cx)))
    }

    /// What the agent proposes for each question, each changed in place:
    /// the selected one opens with its words and options.
    fn proposals(&mut self, t: &Theme, ask: &Ask, cx: &mut Context<Self>) -> Div {
        let draft = self.draft();
        let mut list = div().flex().flex_col();
        for (i, q) in ask.questions.iter().enumerate() {
            let said = draft.said(q);
            let rec = model::recommended(&q.options, &q.recommend);
            let here = i == draft.at;
            let (words, colour) = if said.answered() {
                said_words(t, q, &said)
            } else if let Some(r) = rec {
                (
                    format!(
                        "{} {}",
                        model::letter(&q.options, r),
                        model::label(&q.options, r)
                    ),
                    t.text2,
                )
            } else {
                ("needs your words".to_string(), tokens::needs_you(t))
            };
            let state = if said.unclear {
                ("?", tokens::needs_you(t))
            } else if said.answered() {
                ("✓", tokens::done(t))
            } else if rec.is_some() {
                ("·", t.dim)
            } else {
                ("!", tokens::needs_you(t))
            };
            let mut row = div()
                .id(("proposal", i))
                .flex()
                .flex_col()
                .gap(px(GAP_SMALL))
                .px(px(PAD))
                .py(px(GAP))
                .border_b_1()
                .border_color(t.rule())
                .when(here, |d| d.bg(t.accent_soft()))
                .cursor_pointer()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(GAP))
                        .child(
                            div()
                                .w(px(14.))
                                .flex_none()
                                .text_color(state.1)
                                .font_weight(FontWeight::BOLD)
                                .child(state.0),
                        )
                        .child(
                            div()
                                .w(px(28.))
                                .flex_none()
                                .text_color(t.dim)
                                .child(format!("Q{}", i + 1)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .when(here, |d| d.text_color(t.dim))
                                .child(if here { String::new() } else { q.title.clone() }),
                        )
                        .child(
                            div()
                                .flex_none()
                                .max_w(px(180.))
                                .truncate()
                                .text_color(colour)
                                .child(words),
                        ),
                )
                .on_click(cx.listener(move |app, _: &ClickEvent, window, cx| {
                    app.goto_question(i, window, cx)
                }));
            if here {
                row = row
                    .child(div().pl(px(54.)).child(self.question_words(t, ask, q)))
                    .when(!q.options.is_empty(), |d| d.child(div().pl(px(54.)).child(self.options(t, &q.options, &q.recommend, said.pick.or(if said.answered() { None } else { rec }), false, cx))))
                    .child(div().pl(px(54.)).flex().gap(px(GAP)).child(
                        action(t, "unclear", if said.unclear { "Clear" } else { "Unclear" }, "U", Kind::Quiet)
                            .on_click(cx.listener(|app, _: &ClickEvent, window, cx| app.toggle_unclear(window, cx))),
                    ).child(
                        action(t, "words", "Words", "W", Kind::Quiet)
                            .on_click(cx.listener(|app, _: &ClickEvent, window, cx| app.write(Verdict::Grill, window, cx))),
                    ))
                    .when(matches!(self.note_for, crate::app::NoteFor::Question(ref id) if *id == q.id), |d| d.child(div().pl(px(54.)).child(self.note_box(t, cx))));
            }
            list = list.child(row);
        }
        let open = ask
            .questions
            .iter()
            .filter(|q| {
                !draft.said(q).answered() && model::recommended(&q.options, &q.recommend).is_none()
            })
            .count();
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .px(px(PAD))
                    .pt(px(PAD))
                    .pb(px(GAP))
                    .flex()
                    .flex_col()
                    .gap(px(GAP_SMALL))
                    .child(title(t, &ask.title))
                    .child(div().text_color(t.text2).child(format!(
                        "The agent proposes an answer to {} of {}. ↑ ↓ to go through, a number to change one, Enter to send them all.",
                        ask.questions.iter().filter(|q| model::recommended(&q.options, &q.recommend).is_some()).count(),
                        ask.questions.len()
                    ))),
            )
            .child(list)
            .child(
                div()
                    .p(px(PAD))
                    .flex()
                    .items_center()
                    .gap(px(GAP))
                    .when(open > 0, |d| d.child(div().text_color(tokens::needs_you(t)).child(format!("{open} left without an answer"))))
                    .child(div().flex_1())
                    .child(
                        action(t, "send-all", "Send the round", "Enter", Kind::Primary)
                            .on_click(cx.listener(|app, _: &ClickEvent, window, cx| app.send(Verdict::GrillRest, window, cx))),
                    ),
            )
    }
}

// ---- Small pieces ----

fn no_desks(t: &Theme) -> AnyElement {
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(GAP))
        .child(div().text_size(px(tokens::TITLE)).font_weight(FontWeight::SEMIBOLD).child("No desks yet"))
        .child(div().text_color(t.text2).child("Name them with --desk Name=http://127.0.0.1:8774 (repeated), DESK_APP_DESKS, or \"desks\" in desk-app.json."))
        .into_any_element()
}

/// A page the ask is about, to read here; marking it is the web desk's.
fn doc_text(t: &Theme, name: &str, text: &str) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(GAP_SMALL))
        .p(px(GAP))
        .rounded(px(tokens::RADIUS))
        .border_1()
        .border_color(t.divider)
        .bg(t.bg)
        .child(
            div()
                .text_size(px(SMALL))
                .text_color(t.dim)
                .child(name.to_string()),
        )
        .child(
            div()
                .text_size(px(BODY))
                .line_height(relative(tokens::BODY_LINE))
                .child(text.to_string()),
        )
}

fn title(t: &Theme, words: &str) -> Div {
    div()
        .text_size(px(tokens::TITLE))
        .font_weight(FontWeight::SEMIBOLD)
        .line_height(relative(1.2))
        .text_color(t.text)
        .child(words.to_string())
}

fn body(t: &Theme, words: &str) -> Div {
    div()
        .text_size(px(BODY))
        .line_height(relative(tokens::BODY_LINE))
        .text_color(t.text)
        .child(words.to_string())
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
                        .text_size(px(SMALL))
                        .text_color(t.accent)
                        .child(label.to_string()),
                )
                .child(
                    div()
                        .text_size(px(BODY))
                        .line_height(relative(tokens::BODY_LINE))
                        .child(words.to_string()),
                ),
        )
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
        .size(px(26.))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .border_1()
        .border_color(if on { t.accent } else { t.text2 })
        .when(on, |d| d.bg(t.accent).text_color(t.on_accent()))
        .font_weight(FontWeight::BOLD)
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

fn said_words(t: &Theme, q: &Question, said: &crate::answer::Said) -> (String, gpui::Hsla) {
    if said.unclear {
        return ("unclear: asked again".into(), tokens::needs_you(t));
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
        (Some(p), w) => (format!("{p} — “{w}”"), t.text),
        (None, "") => ("skipped".into(), t.dim),
        (None, w) => (format!("“{w}”"), t.text),
    }
}

/// The recommendation's reason, after the option's name it starts with.
fn rest_of(recommend: &str, name: &str) -> String {
    recommend
        .trim()
        .strip_prefix(name)
        .map(|r| {
            r.trim_start_matches([':', '.', ',', '—', '-', ' '])
                .to_string()
        })
        .unwrap_or_else(|| recommend.to_string())
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

/// How long it has waited, in the largest unit that says it.
fn age(created: &str) -> String {
    let Ok(at) =
        time::OffsetDateTime::parse(created, &time::format_description::well_known::Rfc3339)
    else {
        return String::new();
    };
    let mins = (time::OffsetDateTime::now_utc() - at)
        .whole_minutes()
        .max(0);
    match mins {
        0 => "just now".into(),
        m if m < 60 => format!("{m} min"),
        m if m < 60 * 48 => format!("{} h", m / 60),
        m => format!("{} d", m / (60 * 24)),
    }
}
