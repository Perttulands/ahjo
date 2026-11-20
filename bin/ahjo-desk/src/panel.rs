//! The answer panel: the agent's options as cards, his answer under them
//! in a card of its own, Send at its foot where the eye ends.
//!
//! What the agent says is written and what he did is drawn. The agent's
//! recommendation is its reason, a sentence in its own voice; his choice
//! is the look of the card he chose. Nothing is labelled: no radio, no
//! tag, no kicker.
//!
//! Prototype: two parts are drawn more than one way, picked at start.
//! `AHJO_DESK_CHOSEN` = `lift` (the card takes the accent's edge and a
//! tint), `edge` (a bar of accent at its left and its name set heavier) or
//! `recede` (the others step back, as iA Writer's focus mode). The
//! recommendation (`AHJO_DESK_REC`) is `quote` (its reason in italics in
//! the card, signed by the agent) or `closing` (one sentence after the
//! cards, as a brief ends).

use std::sync::OnceLock;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chosen {
    Lift,
    Edge,
    Recede,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rec {
    Quote,
    Closing,
}

pub fn chosen_look() -> Chosen {
    static LOOK: OnceLock<Chosen> = OnceLock::new();
    *LOOK.get_or_init(|| match std::env::var("AHJO_DESK_CHOSEN").as_deref() {
        Ok("lift") => Chosen::Lift,
        Ok("recede") => Chosen::Recede,
        _ => Chosen::Edge,
    })
}

pub fn rec_look() -> Rec {
    static LOOK: OnceLock<Rec> = OnceLock::new();
    *LOOK.get_or_init(|| match std::env::var("AHJO_DESK_REC").as_deref() {
        Ok("closing") => Rec::Closing,
        _ => Rec::Quote,
    })
}

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

/// The recommendation as the brief's last sentence, after the cards.
pub fn closing(t: &Theme, options: &[Choice], rec: usize, reason: &str, by: &str) -> Div {
    let letter = model::letter(options, rec);
    let label = model::label(options, rec);
    let head = format!("{by} would take {letter}, {label}.");
    let reason = reason.trim();
    let text = if reason.is_empty() {
        head.clone()
    } else {
        format!("{head} {reason}")
    };
    reading(t)
        .text_color(t.text2)
        .child(StyledText::new(SharedString::from(text)).with_highlights([(
            0..head.len(),
            HighlightStyle {
                color: Some(t.text),
                ..Default::default()
            },
        )]))
}

/// How a card stands: chosen, another chosen, or none.
#[derive(Clone, Copy)]
pub struct Standing {
    pub on: bool,
    pub other_on: bool,
    pub live: bool,
}

/// An option as the agent made it: its letter hanging in the margin, its
/// name, why, and, on the one recommended, the agent's reason signed.
pub fn option_card(
    t: &Theme,
    options: &[Choice],
    i: usize,
    o: &Choice,
    reason: Option<(String, String)>,
    s: Standing,
) -> Div {
    let look = chosen_look();
    let letter = model::letter(options, i);
    let label = model::label(options, i);
    let edge = s.on && look == Chosen::Edge;
    let lift = s.on && look == Chosen::Lift;
    let receded = s.other_on && look == Chosen::Recede;
    div()
        .relative()
        .flex()
        .px(px(tokens::CARD_PAD_X))
        .py(px(tokens::CARD_PAD_Y))
        .rounded(px(tokens::CARD_RADIUS))
        .border_1()
        .border_color(if lift {
            t.accent
        } else if s.on && look == Chosen::Recede {
            alpha(t.text2, 0.7)
        } else {
            t.divider
        })
        .bg(if lift { t.accent_soft() } else { t.surface })
        .when(receded, |d| d.opacity(tokens::RECEDE))
        .when(s.live, |d| {
            d.hover(|d| d.border_color(alpha(t.accent, 0.6)).opacity(1.))
        })
        .when(edge, |d| {
            d.child(
                div()
                    .absolute()
                    .left(px(tokens::EDGE_INSET))
                    .top(px(tokens::CARD_PAD_Y))
                    .bottom(px(tokens::CARD_PAD_Y))
                    .w(px(tokens::EDGE_W))
                    .rounded_full()
                    .bg(t.accent),
            )
        })
        .child(
            reading(t)
                .flex_none()
                .w(px(tokens::HANG))
                .text_color(if lift || edge { t.accent } else { t.dim })
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
                        // In the edge's look the chosen name alone is set
                        // heavy, the others at the weight of the text.
                        .font_weight(match (look, s.on) {
                            (Chosen::Edge, true) => FontWeight::BOLD,
                            (Chosen::Edge, false) => FontWeight::NORMAL,
                            _ => FontWeight::SEMIBOLD,
                        })
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

/// The reason a recommendation gives for `name`, when the card carries it.
/// A recommendation that gives no reason of its own (or only repeats the
/// option's why) still says whose it is.
pub fn card_reason(recommend: &str, name: &str, why: &str, by: &str) -> Option<(String, String)> {
    if rec_look() != Rec::Quote {
        return None;
    }
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
        let any = ask.options.iter().enumerate().any(|(i, o)| on_at(i, o));
        let mut list = div().flex().flex_col().gap(px(tokens::CARD_GAP));
        for (i, o) in ask.options.iter().enumerate() {
            let on = on_at(i, o);
            let reason = (rec == Some(i))
                .then(|| card_reason(&ask.recommend, &o.name, &o.why, &by))
                .flatten();
            let card = option_card(
                t,
                &ask.options,
                i,
                o,
                reason,
                Standing {
                    on,
                    other_on: any && !on,
                    live: now,
                },
            );
            list = list.child(if now {
                card.id(("option", i))
                    .cursor_pointer()
                    .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| app.pick(i, cx)))
                    .into_any_element()
            } else {
                card.into_any_element()
            });
        }
        list.children(recommendation_after(t, &ask.options, &ask.recommend, &by))
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
            );
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
        let any = said.pick.is_some();
        let mut cards = div().flex().flex_col().gap(px(tokens::CARD_GAP));
        for (k, o) in q.options.iter().enumerate() {
            let on = said.pick == Some(k);
            let reason = (rec == Some(k))
                .then(|| card_reason(&q.recommend, &o.name, &o.why, &by))
                .flatten();
            let card = option_card(
                t,
                &q.options,
                k,
                o,
                reason,
                Standing {
                    on,
                    other_on: any && !on,
                    live: now,
                },
            );
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
        let cards = cards.children(recommendation_after(t, &q.options, &q.recommend, &by));
        // His words on the question: the field while they go to it, else
        // what he wrote, in his own column.
        let words: Option<AnyElement> = if now && active {
            let focused = self.words.read(cx).is_focused(window);
            let unclear = said.unclear;
            Some(
                div()
                    .flex()
                    .items_start()
                    .gap(px(GAP_SMALL))
                    .pl(px(tokens::CARD_PAD_X))
                    .pr(px(GAP_SMALL))
                    .py(px(tokens::FIELD_PAD_Y))
                    .rounded(px(tokens::CARD_RADIUS))
                    .border_1()
                    .border_color(if focused { t.accent } else { t.divider })
                    .child(div().flex_1().min_w_0().child(self.words.clone()))
                    .child(
                        quiet(
                            t,
                            "unclear",
                            if unclear {
                                "Unclear · undo"
                            } else {
                                "Unclear"
                            },
                        )
                        .when(unclear, |b| b.text_color(t.needs))
                        .tooltip(kit::tip(crate::views::answer_unclear_tip()))
                        .on_click(cx.listener(|app, _: &ClickEvent, _, cx| app.toggle_unclear(cx))),
                    )
                    .into_any_element(),
            )
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
    /// at its foot the way out at the left and Send at the right, last.
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
        } else {
            "Send".into()
        };
        let send = big(
            button(
                t,
                "send",
                send_word,
                if hands { Kind::Plain } else { Kind::Primary },
            ),
            t,
        )
        .when(self.sending, |d| d.opacity(0.5))
        .tooltip(kit::tip(format!("{key}{told}")))
        .on_click(
            cx.listener(|app, _: &ClickEvent, window, cx| app.send(Verdict::Send, window, cx)),
        );
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
            big(button(t, "good", "Looks good", Kind::Plain), t)
                .tooltip(kit::tip(
                    "Accept it as it is; your words, if any, go with it",
                ))
                .on_click(cx.listener(|app, _: &ClickEvent, window, cx| {
                    app.send(Verdict::LooksGood, window, cx)
                }))
        });
        let rest = grill.then(|| {
            big(button(t, "rest", "Rest as recommended", Kind::Plain), t)
                .tooltip(kit::tip(
                    "Every question you left takes the agent's recommendation",
                ))
                .on_click(cx.listener(|app, _: &ClickEvent, window, cx| {
                    app.send(Verdict::GrillRest, window, cx)
                }))
        });
        let elsewhere = quiet(t, "elsewhere", "Answered elsewhere")
            .tooltip(kit::tip(
                "You told the agent in its session or by voice: this closes the ask with your words as a note",
            ))
            .on_click(cx.listener(|app, _: &ClickEvent, window, cx| app.send(Verdict::Elsewhere, window, cx)));
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
            reading(t)
                .id("round-words")
                .px(px(tokens::CARD_PAD_X))
                .pt(px(tokens::FIELD_PAD_Y + 2.))
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
        let foot = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(GAP_SMALL))
            .px(px(tokens::FOOT_PAD))
            .pb(px(tokens::FOOT_PAD))
            .child(elsewhere)
            .child(div().flex_1())
            .children(rest)
            .children(good)
            .children(done)
            .child(send);
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

/// After the cards: the recommendation as the brief's closing sentence,
/// or, when it names no option, signed on its own.
fn recommendation_after(t: &Theme, options: &[Choice], recommend: &str, by: &str) -> Option<Div> {
    if recommend.trim().is_empty() {
        return None;
    }
    let pad = |d: Div| d.mt(px(tokens::CARD_GAP)).px(px(tokens::CARD_PAD_X + 1.));
    match model::recommended(options, recommend) {
        None => Some(pad(signed(t, recommend, by))),
        Some(i) if rec_look() == Rec::Closing => {
            let reason = crate::views::rest_of(recommend, &options[i].name);
            Some(pad(closing(t, options, i, &reason, by)))
        }
        Some(_) => None,
    }
}

/// A button at the answer's size, in the reading face.
fn big(b: Stateful<Div>, t: &Theme) -> Stateful<Div> {
    b.h(px(tokens::SEND_H))
        .px(px(tokens::CARD_PAD_X))
        .font_family(t.ty.reading)
        .text_size(px(t.ty.read))
}

/// A quiet way out, in the reading face.
fn quiet(t: &Theme, id: &'static str, words: &'static str) -> Stateful<Div> {
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
        let numbered = self.numbered();
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
