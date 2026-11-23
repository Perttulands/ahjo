//! The answer panel: the agent's options as cards, his answer under them
//! in a card of its own, Send at its foot where the eye ends.
//!
//! What the agent says is written and what he did is drawn (AH14). The
//! agent's recommendation is its reason, in italics on the card it argues
//! for, signed with its name; his choice lifts its card: the accent's
//! border and a faint tint. Nothing is labelled: no radio, no tag, no
//! kicker, and no thin bar at an edge.

use crate::answer::{Said, Verdict};
use crate::app::{Aim, DeskApp};
use crate::marks;
use crate::model::{self, Ask, Choice, Form, Question};
use crate::tokens::{self, GAP_SMALL, META};
use crate::views::{button, check_block, chip, number};
use ahjo_app_kit::theme::{Theme, alpha, rgb, ty};
use ahjo_app_kit::widgets::kit::{self, Kind};
use gpui::{
    AnyElement, ClickEvent, Context, Div, FontStyle, FontWeight, HighlightStyle, SharedString,
    Stateful, StyledText, Window, div, prelude::*, px, relative,
};

/// Who asked, as the recommendation is signed.
pub fn asker(ask: &Ask) -> String {
    if ask.asked_by.trim().is_empty() {
        "the agent".into()
    } else {
        ask.asked_by.trim().to_string()
    }
}

/// Words to read, in the theme's reading face at its size.
fn reading(t: &Theme) -> Div {
    div()
        .font_family(t.ty.reading)
        .text_size(px(t.ty.read))
        .line_height(relative(ty::READ_LEADING))
}

/// The agent's reason for the option it recommends: italics in its voice,
/// signed (unless `by` is empty, when the sentence names the agent itself).
/// A recommendation that names no option stands alone the same way.
pub fn signed(t: &Theme, reason: &str, by: &str) -> Div {
    let reason = reason.trim();
    let sign = if by.is_empty() {
        String::new()
    } else {
        format!(" — {by}")
    };
    let text = format!("{reason}{sign}");
    let cut = reason.len();
    reading(t).text_color(t.text).child(
        StyledText::new(SharedString::from(text.clone())).with_highlights([
            (
                0..cut,
                HighlightStyle {
                    font_style: Some(FontStyle::Italic),
                    ..Default::default()
                },
            ),
            (
                cut..text.len(),
                HighlightStyle {
                    color: Some(t.dim),
                    ..Default::default()
                },
            ),
        ]),
    )
}

/// An option as the agent made it: its letter hanging in the margin, its
/// name, why, and, on the one recommended, the agent's reason signed. The
/// one he chose (`on`) is lifted; one he can still choose (`live`) answers
/// the pointer.
pub fn option_card(
    t: &Theme,
    options: &[Choice],
    i: usize,
    o: &Choice,
    reason: Option<(String, String)>,
    on: bool,
    live: bool,
) -> Div {
    let letter = model::letter(options, i);
    let label = model::label(options, i);
    lifted(t, on)
        .flex()
        .px(px(tokens::CARD_PAD_X))
        .py(px(tokens::CARD_PAD_Y))
        .when(live && !on, |d| {
            d.hover(|d| d.border_color(alpha(t.accent, 0.6)))
        })
        .child(
            reading(t)
                .flex_none()
                .w(px(tokens::HANG))
                .text_color(if on { t.accent } else { t.dim })
                .child(letter),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(tokens::CARD_LINES))
                .child(
                    reading(t)
                        .text_color(t.text)
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(label),
                )
                .when(!o.why.trim().is_empty(), |d| {
                    d.child(
                        reading(t)
                            .text_color(t.text2)
                            .child(o.why.trim().to_string()),
                    )
                })
                .children(reason.map(|(r, by)| signed(t, &r, &by).mt(px(tokens::BEFORE_REASON)))),
        )
}

/// A card, lifted when `on`: the accent's border and a faint tint. The
/// Desk marks what is chosen or in front this one way (AH14: never a thin
/// bar at an edge).
pub fn lifted(t: &Theme, on: bool) -> Div {
    div()
        .rounded(px(tokens::CARD_RADIUS))
        .border_1()
        .border_color(if on { t.accent } else { t.divider })
        .bg(if on { t.accent_soft() } else { t.surface })
}

/// The reason a recommendation gives for `name`, when the card carries it.
/// A recommendation that gives no reason of its own (or only repeats the
/// option's why) still says whose it is.
pub fn card_reason(recommend: &str, name: &str, why: &str, by: &str) -> Option<(String, String)> {
    let r = crate::views::rest_of(recommend, name);
    if r.trim().is_empty() || r.trim() == why.trim() {
        return Some((format!("{by} would take this one."), String::new()));
    }
    Some((r, by.to_string()))
}

impl DeskApp {
    /// The options as cards, the recommended one with its reason; in the
    /// open round a click chooses one (again: no longer).
    pub fn option_cards(&mut self, t: &Theme, ask: &Ask, now: bool, cx: &mut Context<Self>) -> Div {
        let rec = ask.recommended();
        let by = asker(ask);
        let picked = if now { self.draft().pick } else { None };
        let answered: Vec<String> = ask
            .answer
            .as_ref()
            .map(|a| a.options.clone())
            .unwrap_or_default();
        let on_at =
            |i: usize, o: &Choice| picked == Some(i) || (!now && answered.contains(&o.name));
        let mut list = div().flex().flex_col().gap(px(tokens::CARD_GAP));
        for (i, o) in ask.options.iter().enumerate() {
            let on = on_at(i, o);
            let reason = (rec == Some(i))
                .then(|| card_reason(&ask.recommend, &o.name, &o.why, &by))
                .flatten();
            let card = option_card(t, &ask.options, i, o, reason, on, now);
            list = list.child(if now {
                card.id(("option", i))
                    .cursor_pointer()
                    .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| app.pick(i, cx)))
                    .into_any_element()
            } else {
                card.into_any_element()
            });
        }
        list.children(unattached(t, &ask.options, &ask.recommend, &by))
    }

    /// A grill's question: its number hanging where the letters hang, its
    /// options as cards, and, while his words are on it, the field.
    #[allow(clippy::too_many_arguments)]
    pub fn question_section(
        &mut self,
        t: &Theme,
        ask: &Ask,
        i: usize,
        q: &Question,
        active: bool,
        now: bool,
        window: &mut Window,
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
        let by = asker(ask);
        let rec = model::recommended(&q.options, &q.recommend);
        let head = div()
            .flex()
            .px(px(tokens::CARD_PAD_X + 1.))
            .child(
                reading(t)
                    .flex_none()
                    .w(px(tokens::HANG))
                    .text_color(if active { t.accent } else { t.dim })
                    .child((i + 1).to_string()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(tokens::CARD_LINES))
                    .child(
                        reading(t)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(t.text)
                            .child(q.title.trim().to_string()),
                    )
                    .when(
                        !q.question.trim().is_empty() && q.question.trim() != q.title.trim(),
                        |d| {
                            d.child(
                                reading(t)
                                    .text_color(t.text2)
                                    .child(q.question.trim().to_string()),
                            )
                        },
                    )
                    .children(ask.followed_up(q).map(|(round, old, answer)| {
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
                        div().text_size(px(META)).text_color(t.dim).child(format!(
                            "Round {round} asked “{}”; you said {was}",
                            old.title.trim()
                        ))
                    })),
            )
            // Unclear is said of the question: at its head, on the one his
            // words are on or one he called unclear. Elsewhere it keeps its
            // room unseen, so moving between questions moves nothing.
            .when(now, |d| {
                let shown = active || said.unclear;
                d.child(
                    quiet(t, SharedString::from(format!("unclear-{i}")), "Unclear")
                        .ml(px(GAP_SMALL))
                        .h(px(line(t)))
                        .when(said.unclear, |b| b.text_color(t.needs))
                        .when(!shown, |b| b.opacity(0.))
                        .when(shown, |b| {
                            b.tooltip(kit::tip(crate::views::answer_unclear_tip()))
                                .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| {
                                    cx.stop_propagation();
                                    app.goto_question(Some(i), cx);
                                    app.toggle_unclear(cx);
                                }))
                        }),
                )
            });
        let head = if now {
            head.id(("question", i))
                .cursor_pointer()
                .on_click(
                    cx.listener(move |app, _: &ClickEvent, _, cx| app.goto_question(Some(i), cx)),
                )
                .into_any_element()
        } else {
            head.into_any_element()
        };
        let mut cards = div().flex().flex_col().gap(px(tokens::CARD_GAP));
        for (k, o) in q.options.iter().enumerate() {
            let on = said.pick == Some(k);
            let reason = (rec == Some(k))
                .then(|| card_reason(&q.recommend, &o.name, &o.why, &by))
                .flatten();
            let card = option_card(t, &q.options, k, o, reason, on, now);
            cards = cards.child(if now {
                card.id(SharedString::from(format!("q{i}-o{k}")))
                    .cursor_pointer()
                    .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| {
                        app.goto_question(Some(i), cx);
                        app.pick(k, cx);
                    }))
                    .into_any_element()
            } else {
                card.into_any_element()
            });
        }
        let cards = cards.children(unattached(t, &q.options, &q.recommend, &by));
        // His words on the question, in a field of its own on each one
        // (DESIGN: one free text field per question). The one he is on
        // takes the keyboard; the others hold what he wrote at the same
        // size, so the words moving from one to the next moves nothing.
        let words: Option<AnyElement> =
            if now {
                let focused = active && self.words.read(cx).is_focused(window);
                let field = words_box(t, focused)
                    .id(("words", i))
                    .debug_selector(move || format!("words-{i}"));
                Some(if active {
                    field.child(self.words.clone()).into_any_element()
                } else {
                    field
                        .cursor_text()
                        .hover(|d| d.border_color(alpha(t.accent, 0.6)))
                        .child(
                            div()
                                .font_family(t.ty.reading)
                                .text_size(px(t.ty.read))
                                .line_height(px(line(t)))
                                .min_h(px(line(t)))
                                .text_color(t.text)
                                .child(said.words.trim().to_string()),
                        )
                        .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| {
                            app.goto_question(Some(i), cx)
                        }))
                        .into_any_element()
                })
            } else if said.unclear {
                Some(
                    div()
                        .px(px(tokens::CARD_PAD_X + 1.))
                        .text_size(px(META))
                        .text_color(t.needs)
                        .child("Unclear: it goes back to be asked again")
                        .into_any_element(),
                )
            } else if !said.words.trim().is_empty() {
                Some(
                    div()
                        .flex()
                        .px(px(tokens::CARD_PAD_X + 1.))
                        .child(div().flex_none().w(px(tokens::HANG)))
                        .child(
                            reading(t)
                                .flex_1()
                                .min_w_0()
                                .text_color(t.text)
                                .child(format!("“{}”", said.words.trim())),
                        )
                        .into_any_element(),
                )
            } else {
                None
            };
        div()
            .flex()
            .flex_col()
            .gap(px(tokens::CARD_GAP + 4.))
            .child(head)
            .when(!q.options.is_empty(), |d| d.child(cards))
            .children(words)
            .into_any_element()
    }

    /// His answer: a card holding his words, the pictures he pasted, and
    /// at its foot the ways to answer, Send last at the right. (Answered
    /// elsewhere stands at the top, away from them.)
    pub fn answer_card(
        &mut self,
        t: &Theme,
        ask: &Ask,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let draft = self.draft();
        let form = ask.form();
        let grill = form == Form::Grill;
        let hands = form == Form::Hands;
        let paper = ask.is_paper();
        let elsewhere_q = grill && draft.at.is_some();
        let focused = self.words.read(cx).is_focused(window);
        let key = if self.config.enter_sends {
            "Enter"
        } else {
            "Ctrl+Enter"
        };
        let told = match (self.config.tell_session, ask.session.trim()) {
            (true, s) if !s.is_empty() => format!(" · tells {s}"),
            _ => String::new(),
        };
        let send_word = if grill {
            format!("Send {} of {}", draft.answered(ask), ask.questions.len())
        } else if hands {
            "Send back".into()
        } else if paper {
            "Send my marks only".into()
        } else {
            "Send".into()
        };
        let send = big(
            button(
                t,
                "send",
                send_word,
                if paper {
                    Kind::Quiet
                } else if hands {
                    Kind::Plain
                } else {
                    Kind::Primary
                },
            ),
            t,
        )
        .debug_selector(|| "send".into())
        .when(self.sending, |d| d.opacity(0.5))
        .tooltip(kit::tip(format!("{key}{told}")))
        .on_click(cx.listener(|app, _: &ClickEvent, _, cx| app.send(Verdict::Send, cx)));
        let running = ask.check.as_ref().is_some_and(|c| c.running);
        let done = hands.then(|| {
            big(
                button(
                    t,
                    "done",
                    if running {
                        "Checking out/…"
                    } else {
                        "Done · check out/"
                    },
                    Kind::Primary,
                ),
                t,
            )
            .when(running, |b| b.opacity(0.6))
            .tooltip(kit::tip(
                "The desk checks what you put in out/; a pass answers the job",
            ))
            .on_click(cx.listener(|app, _: &ClickEvent, _, cx| app.hands_done(cx)))
        });
        let good = (form == Form::Open).then(|| {
            big(
                button(
                    t,
                    "good",
                    if paper { "Move on" } else { "Looks good" },
                    if paper { Kind::Primary } else { Kind::Plain },
                ),
                t,
            )
            .debug_selector(|| "good".into())
            .tooltip(kit::tip(
                "Accept it as it is; your words and marks, if any, go with it",
            ))
            .on_click(cx.listener(|app, _: &ClickEvent, _, cx| app.send(Verdict::LooksGood, cx)))
        });
        let again = paper.then(|| {
            big(button(t, "again", "Another round", Kind::Plain), t)
                .debug_selector(|| "again".into())
                .tooltip(kit::tip(
                    "Send it back to be worked on, as your marks and words say",
                ))
                .on_click(cx.listener(|app, _: &ClickEvent, _, cx| app.send(Verdict::SendBack, cx)))
        });
        let rest = grill.then(|| {
            big(button(t, "rest", "Rest as recommended", Kind::Plain), t)
                .tooltip(kit::tip(
                    "Every question you left takes the agent's recommendation",
                ))
                .on_click(
                    cx.listener(|app, _: &ClickEvent, _, cx| app.send(Verdict::GrillRest, cx)),
                )
        });
        // Hands: the job's folders, above the card.
        let links: Vec<AnyElement> = if hands {
            [("in", "Open in/"), ("out", "Open out/"), ("guide", "Guide")]
                .into_iter()
                .filter_map(|(side, words)| {
                    ask.links.get(side).cloned().map(|url| {
                        chip(
                            t,
                            SharedString::from(format!("link-{side}")),
                            words,
                            false,
                            None,
                        )
                        .tooltip(kit::tip(url.clone()))
                        .on_click(move |_, _, cx| cx.open_url(&url))
                        .into_any_element()
                    })
                })
                .collect()
        } else {
            vec![]
        };
        let pasted = self.pasted_strip(t, ask, &draft.pictures, cx);
        // On a grill whose words are on a question, the round's own words
        // wait here, a click away.
        let words: AnyElement = if elsewhere_q {
            div()
                .id("round-words")
                .px(px(tokens::CARD_PAD_X))
                .pt(px(tokens::FIELD_PAD_Y + 2.))
                .font_family(t.ty.reading)
                .text_size(px(t.ty.read))
                .line_height(px(line(t)))
                .text_color(t.dim)
                .cursor_pointer()
                .child(if draft.words.trim().is_empty() {
                    "On the whole round".to_string()
                } else {
                    draft.words.trim().to_string()
                })
                .on_click(cx.listener(|app, _: &ClickEvent, _, cx| app.goto_question(None, cx)))
                .into_any_element()
        } else {
            div()
                .px(px(tokens::CARD_PAD_X))
                .pt(px(tokens::FIELD_PAD_Y + 2.))
                .child(self.words.clone())
                .into_any_element()
        };
        // Read from the right, the way he answers: the main way first at
        // the right, the others beside it, and in a narrow column what
        // does not fit wraps to the line under it, at the right too. The
        // main way never moves.
        let foot = div()
            .flex()
            .flex_row_reverse()
            .flex_wrap()
            .items_center()
            .gap(px(GAP_SMALL))
            .px(px(tokens::FOOT_PAD))
            .pb(px(tokens::FOOT_PAD))
            .map(|d| {
                if paper {
                    d.children(good).children(again).child(send)
                } else {
                    d.child(send).children(done).children(good)
                }
            })
            .children(rest);
        div()
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(GAP_SMALL))
            .px(px(tokens::COLUMN_PAD))
            .pt(px(tokens::BEFORE_ANSWER))
            .pb(px(tokens::COLUMN_PAD))
            .when(!links.is_empty(), |d| {
                d.child(div().flex().flex_wrap().gap(px(GAP_SMALL)).children(links))
            })
            .children(ask.check.as_ref().map(|c| check_block(t, c)))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(GAP_SMALL))
                    .rounded(px(tokens::CARD_RADIUS))
                    .border_1()
                    .border_color(if focused && !elsewhere_q {
                        t.accent
                    } else {
                        t.divider
                    })
                    .bg(t.surface)
                    .children(self.mark_rows(t, ask, cx))
                    .child(words)
                    .children(pasted.map(|p| p.px(px(tokens::CARD_PAD_X))))
                    .child(foot),
            )
            .into_any_element()
    }
}

/// A recommendation that names none of the options stands after them,
/// signed; one that names an option sits on its card instead.
fn unattached(t: &Theme, options: &[Choice], recommend: &str, by: &str) -> Option<Div> {
    (!recommend.trim().is_empty() && model::recommended(options, recommend).is_none()).then(|| {
        signed(t, recommend, by)
            .mt(px(tokens::CARD_GAP))
            .px(px(tokens::CARD_PAD_X + 1.))
    })
}

/// One line of his words as the field sets them, so words written and
/// words being written stand the same height.
fn line(t: &Theme) -> f32 {
    f32::from(ahjo_app_kit::widgets::field::line_height(t.ty.read))
}

/// The edge around his words on a grill's question: the accent while
/// they are being written.
fn words_box(t: &Theme, focused: bool) -> Div {
    div()
        .px(px(tokens::CARD_PAD_X))
        .py(px(tokens::FIELD_PAD_Y))
        .rounded(px(tokens::CARD_RADIUS))
        .border_1()
        .border_color(if focused { t.accent } else { t.divider })
}

/// A button at the answer's size, in the reading face.
fn big(b: Stateful<Div>, t: &Theme) -> Stateful<Div> {
    b.h(px(tokens::SEND_H))
        .px(px(tokens::CARD_PAD_X))
        .font_family(t.ty.reading)
        .text_size(px(t.ty.read))
}

/// A quiet way out, in the reading face.
fn quiet(t: &Theme, id: impl Into<gpui::ElementId>, words: &'static str) -> Stateful<Div> {
    button(t, id, words, Kind::Quiet)
        .h(px(tokens::SEND_H))
        .font_family(t.ty.reading)
        .text_size(px(t.ty.read))
        .text_color(t.dim)
}

impl DeskApp {
    /// His marks, at the head of his answer: each its number as the
    /// picture draws it and his words on it; the one chosen open to write.
    fn mark_rows(&mut self, t: &Theme, ask: &Ask, cx: &mut Context<Self>) -> Option<Div> {
        // Marks on the document's words are on the paper, not listed.
        let mut numbered = self.numbered();
        numbered.retain(|(_, m)| !m.on_text());
        if numbered.is_empty() {
            return None;
        }
        let doc = ask.doc.as_ref().map(|d| d.text.as_str());
        let items = self.items();
        let mut rows = div()
            .flex()
            .flex_col()
            .pt(px(tokens::FOOT_PAD))
            .pb(px(tokens::FOOT_PAD))
            .border_b_1()
            .border_color(t.rule());
        for (n, m) in numbered {
            let on = self.selected.as_deref() == Some(&m.id);
            let place = m
                .media
                .as_ref()
                .and_then(|id| items.iter().find(|i| &i.media.id == id))
                .filter(|i| !i.tag.is_empty())
                .map(|i| format!("{} · ", i.tag))
                .unwrap_or_default();
            let what = format!("{place}{}", marks::what(&m, doc));
            let id = m.id.clone();
            let gone = m.id.clone();
            let words: AnyElement = if on {
                div().child(self.mark_note.clone()).into_any_element()
            } else if m.text.trim().is_empty() {
                reading(t).text_color(t.dim).child(what).into_any_element()
            } else {
                reading(t)
                    .text_color(t.text)
                    .line_clamp(2)
                    .child(m.text.trim().to_string())
                    .into_any_element()
            };
            rows = rows.child(
                div()
                    .id(SharedString::from(format!("mark-{}", m.id)))
                    .flex()
                    .items_start()
                    .gap(px(tokens::MARK_GAP))
                    .px(px(tokens::CARD_PAD_X))
                    .py(px(tokens::MARK_PAD_Y))
                    .cursor_pointer()
                    .when(on, |d| d.bg(t.accent_soft()))
                    .hover(|d| d.bg(t.accent_soft()))
                    .child(
                        div()
                            .flex_none()
                            .h(px(t.ty.read * ty::READ_LEADING))
                            .flex()
                            .items_center()
                            .child(number(t, n, rgb(tokens::ink(m.color.as_deref())))),
                    )
                    .child(div().flex_1().min_w_0().child(words))
                    .when(on, |d| {
                        d.child(quiet(t, "drop-mark", "Remove").on_click(cx.listener(
                            move |app, _: &ClickEvent, _, cx| {
                                cx.stop_propagation();
                                app.delete_mark(&gone, cx)
                            },
                        )))
                    })
                    .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| {
                        app.select_mark(Some(id.clone()), true, cx);
                        app.aim(Aim::MarkNote, cx);
                    })),
            );
        }
        Some(rows)
    }
}
