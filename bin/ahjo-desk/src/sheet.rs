//! The paper a document is graded on, as the web desk lays it out: a light
//! sheet with the document set on it, his marks drawn over its words in a
//! grader's hand (a tick, a cut, a rewrite written in after the words it
//! replaces, a note looped and written in the margin), the pens under the
//! passage he chose, and what changed since the round he marked. A tap
//! takes a sentence (narrowed to a phrase or widened to the paragraph), a
//! drag takes exactly what it covers, and T, X, R and N mark it.

use crate::app::{Aim, DeskApp};
use crate::marks;
use crate::model::{Ask, Mark};
use crate::paper::{self, Extra, Followed, Kind as Block, Level, Piece, Rect, Shown, Style};
use crate::tokens::{self, paper as P};
use ahjo_app_kit::theme::{FAMILY, Theme, alpha, rgb, rgba};
use gpui::{
    AnyElement, ClickEvent, Context, Div, FontStyle, FontWeight, HighlightStyle, Hsla, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PathBuilder, Pixels, SharedString,
    StrikethroughStyle, StyledText, TextLayout, UnderlineStyle, Window, canvas, div, point,
    prelude::*, px,
};
use std::ops::Range;
use std::rc::Rc;

/// The sheet's colours.
#[derive(Clone, Copy)]
struct Inks {
    paper: Hsla,
    ink: Hsla,
    soft: Hsla,
    rule: Hsla,
    grade: Hsla,
    wash: Hsla,
    old: Hsla,
    new: Hsla,
    gone: Hsla,
}

fn inks() -> Inks {
    Inks {
        paper: rgb(P::PAPER),
        ink: rgb(P::INK),
        soft: rgb(P::SOFT),
        rule: rgb(P::RULE),
        grade: rgb(P::GRADE),
        wash: rgba(P::GRADE, P::GRADE_WASH),
        old: rgba(P::GRADE, P::GRADE_OLD),
        new: rgba(P::NEW, P::NEW_WASH),
        gone: rgb(P::GONE),
    }
}

/// A mark as one piece of text draws it.
#[derive(Clone)]
struct Stroked {
    seed: String,
    kind: String,
    range: Range<usize>,
    /// Its tick stands here: the piece shows its last character.
    color: Hsla,
    width: f32,
}

/// What the page washes behind words: the passage in hand, and new words.
#[derive(Clone, Default)]
struct Washes {
    chosen: Option<Range<usize>>,
    new: Vec<Range<usize>>,
}

/// The verdict as the paper's grade says it, as the web desk says it.
pub fn grade_word(verdict: &str, options: &[String]) -> String {
    match verdict {
        "accept" => "Move on".into(),
        "send back" => "Another round".into(),
        "marks" => "Marks only".into(),
        "elsewhere" => "Answered elsewhere".into(),
        "pick" => options.join(", "),
        v => v.to_string(),
    }
}

impl DeskApp {
    // ---- His hand on the paper ----

    /// A press on the document's character `at`.
    pub fn press_text(&mut self, at: usize, cx: &mut Context<Self>) {
        if self.read_only() {
            return;
        }
        self.grading.writing = None;
        self.grading.press = Some((at, at));
        cx.notify();
    }

    pub fn drag_text(&mut self, at: usize, cx: &mut Context<Self>) {
        if let Some((from, now)) = self.grading.press
            && now != at
        {
            self.grading.press = Some((from, at));
            cx.notify();
        }
    }

    /// The press let go: a tap takes his mark there, or the passage of the
    /// level he reads at; a drag takes what it covered.
    pub fn release_text(&mut self, cx: &mut Context<Self>) {
        let Some((from, to)) = self.grading.press.take() else {
            return;
        };
        let Some(paper) = self.paper() else {
            return;
        };
        let level = self.grading.level;
        self.grading.close();
        self.grading.level = level;
        if from == to {
            if let Some(m) = paper::mark_at(&self.marks(), from) {
                self.grading.chosen = paper::range(&m);
                self.grading.mark = Some(m.id);
            } else if let Some(piece) = piece_of(&paper.plain, from) {
                self.grading.chosen = paper::passage_at(piece, from, level);
                self.grading.tapped = Some(from);
            }
        } else {
            let r = paper::trimmed(&paper.text, from.min(to)..from.max(to) + 1);
            if !r.is_empty() {
                self.grading.mark = self
                    .marks()
                    .into_iter()
                    .find(|m| paper::is_text(m) && paper::range(m) == Some(r.clone()))
                    .map(|m| m.id);
                self.grading.chosen = Some(r);
            }
        }
        // The pens' keys go to the paper, not into his words.
        if self.grading.chosen.is_some() {
            self.aim(Aim::Surface, cx);
        }
        cx.notify();
    }

    /// Reads the tapped passage at `level` instead.
    pub fn set_level(&mut self, level: Level, cx: &mut Context<Self>) {
        self.grading.level = level;
        if let (Some(at), Some(paper)) = (self.grading.tapped, self.paper())
            && let Some(piece) = piece_of(&paper.plain, at)
        {
            self.grading.chosen = paper::passage_at(piece, at, level);
            self.grading.mark = None;
        }
        cx.notify();
    }

    /// Marks the chosen passage with a pen: a tick or a cut at once; a
    /// rewrite or a note once he has written it.
    pub fn use_pen(&mut self, kind: &'static str, cx: &mut Context<Self>) {
        let Some(r) = self.grading.chosen.clone() else {
            return;
        };
        if self.read_only() {
            return;
        }
        if kind == "tick" || kind == "strike" {
            self.place_mark(kind, r, String::new(), cx);
            return;
        }
        let there = self
            .marks()
            .into_iter()
            .find(|m| m.kind == kind && paper::range(m) == Some(r.clone()));
        let text = match (&there, kind) {
            (Some(m), _) => m.text.clone(),
            // A rewrite starts from the words it replaces.
            (None, "rewrite") => self
                .paper()
                .map(|p| paper::quote(&p.text, &r))
                .unwrap_or_default(),
            _ => String::new(),
        };
        self.grading.writing = Some(kind);
        self.pen.update(cx, |f, cx| {
            f.set_placeholder(if kind == "note" {
                "Your note"
            } else {
                "The words instead"
            });
            f.set(&text, true, cx);
        });
        self.aim(Aim::Pen, cx);
    }

    /// What the pen wrote goes on the passage: a rewrite left as it was,
    /// or emptied, is a cut; an empty note takes the note away.
    pub fn put_pen_down(&mut self, cx: &mut Context<Self>) {
        let (Some(kind), Some(r)) = (self.grading.writing, self.grading.chosen.clone()) else {
            return;
        };
        let text = self.pen.read(cx).text.trim().to_string();
        let same = self
            .paper()
            .is_some_and(|p| paper::quote(&p.text, &r) == text);
        match kind {
            "rewrite" if text.is_empty() || same => self.place_mark("strike", r, String::new(), cx),
            "note" if text.is_empty() => {
                self.edit_marks(|m| {
                    m.retain(|m| !(m.kind == "note" && paper::range(m) == Some(r.clone())))
                });
                self.close_paper(cx);
            }
            _ => self.place_mark(kind, r, text, cx),
        }
        self.aim(Aim::Surface, cx);
    }

    fn place_mark(&mut self, kind: &str, r: Range<usize>, text: String, cx: &mut Context<Self>) {
        let quote = self.paper().map(|p| paper::quote(&p.text, &r));
        let mark = Mark {
            id: marks::next_id(&self.marks()),
            kind: kind.into(),
            start: Some(r.start),
            end: Some(r.end),
            quote: quote.filter(|q| !q.is_empty()),
            text,
            ..Default::default()
        };
        self.edit_marks(|m| paper::place(m, mark));
        self.close_paper(cx);
    }

    /// Takes away the mark he chose.
    pub fn remove_chosen(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.grading.mark.clone() {
            self.delete_mark(&id, cx);
        }
        self.close_paper(cx);
    }

    pub fn close_paper(&mut self, cx: &mut Context<Self>) {
        self.grading.close();
        cx.notify();
    }

    /// A key while a passage is chosen; whether it was the paper's.
    pub fn paper_key(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        if self.read_only() {
            return false;
        }
        match key {
            "t" => self.use_pen("tick", cx),
            "x" => self.use_pen("strike", cx),
            "r" => self.use_pen("rewrite", cx),
            "n" => self.use_pen("note", cx),
            "delete" | "backspace" if self.grading.mark.is_some() => self.remove_chosen(cx),
            "escape" => self.close_paper(cx),
            _ => return false,
        }
        true
    }

    // ---- The sheet ----

    /// The document as a paper, set in the scrolling room.
    pub fn paper_sheet(&mut self, t: &Theme, cx: &mut Context<Self>) -> Option<Div> {
        let paper = self.paper()?;
        let ask = self.current_ask()?.clone();
        let k = inks();
        let marks = paper::text_marks(&self.marks());
        let changes = paper.before.is_some() && !self.grading.hide_changes;
        let (shown, gone_blocks) = paper.shown(&marks, changes);
        let washes = Washes {
            chosen: self.grading.pressed().or(self.grading.chosen.clone()),
            new: if changes { paper.new_since() } else { vec![] },
        };
        // The last round's marks, where their words are now.
        let mut olds: Vec<(Mark, Vec<Range<usize>>, Followed)> = vec![];
        if changes && let Some(b) = &paper.before {
            for m in &b.marks {
                let Some(r) = paper::range(m) else { continue };
                let (kept, _) = paper::map_old(&b.segs, &r);
                olds.push((m.clone(), kept, paper::followed(m, &b.segs, &paper.text)));
            }
        }
        let mut strokes: Vec<Stroked> = marks
            .iter()
            .filter_map(|m| {
                Some(Stroked {
                    seed: m.id.clone(),
                    kind: m.kind.clone(),
                    range: paper::range(m)?,
                    color: k.grade,
                    width: if m.kind == "tick" { P::TICK } else { P::STROKE },
                })
            })
            .collect();
        for (m, kept, _) in &olds {
            for (i, r) in kept.iter().enumerate() {
                strokes.push(Stroked {
                    seed: format!("{}-old-{i}", m.id),
                    kind: m.kind.clone(),
                    range: r.clone(),
                    color: k.old,
                    width: P::STROKE_OLD,
                });
            }
        }
        let strokes = Rc::new(strokes);
        // How wide the sheet was last drawn: room for a margin or not.
        let width = self.paper_width.clone();
        let wide = width.get() >= P::WIDE;
        let mut body = div().flex().flex_col();
        for (i, s) in shown.iter().enumerate() {
            for (_, words) in gone_blocks.iter().filter(|(at, _)| *at == i) {
                body = body.child(gone_block(&k, words));
            }
            let round = paper.before.as_ref().map(|b| b.round);
            // On a narrow sheet a table's notes go under their cells, not
            // under the whole table.
            let in_cells = !wide && matches!(s.block.kind, Block::Table(_));
            let cell_notes: Vec<Vec<AnyElement>> = if in_cells {
                s.pieces
                    .iter()
                    .map(|p| match p.span() {
                        Some(r) => self.margin_notes(t, &k, r, &marks, &olds, round, cx),
                        None => vec![],
                    })
                    .collect()
            } else {
                vec![]
            };
            let notes = if in_cells {
                vec![]
            } else {
                let r = s.block.start..s.block.end + 1;
                self.margin_notes(t, &k, r, &marks, &olds, round, cx)
            };
            // A wide sheet keeps its notes in the margin; a narrow one
            // writes them under the block, as the web desk does.
            body = body.child(
                div()
                    .flex()
                    .when(!wide, |d| d.flex_col())
                    .gap(px(if wide { P::MARGIN_GAP } else { 4. }))
                    .mb(px(P::BLOCK_GAP))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(self.block(t, &k, s, &strokes, &washes, cell_notes, cx)),
                    )
                    .child(
                        div()
                            .flex_none()
                            .when(wide, |d| d.w(px(P::MARGIN)))
                            .when(!wide, |d| d.pl(px(P::TEXT)))
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .children(notes),
                    ),
            );
        }
        for (_, words) in gone_blocks.iter().filter(|(at, _)| *at >= shown.len()) {
            body = body.child(gone_block(&k, words));
        }
        let this = cx.entity().downgrade();
        let release = move |_: &MouseUpEvent, _: &mut Window, cx: &mut gpui::App| {
            let _ = this.update(cx, |app, cx| app.release_text(cx));
        };
        let sheet =
            div()
                .id("paper")
                .relative()
                .debug_selector(|| "paper".into())
                .w_full()
                .max_w(px(P::WIDTH))
                .flex()
                .flex_col()
                .px(px(P::PAD_X))
                .py(px(P::PAD_Y))
                .bg(k.paper)
                .border_1()
                .border_color(rgb(P::EDGE))
                .rounded(px(2.))
                .shadow_lg()
                .text_color(k.ink)
                .font_family(t.ty.reading)
                .text_size(px(P::TEXT))
                .line_height(px(P::TEXT * P::LEADING))
                .cursor_text()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|app, _: &MouseDownEvent, _, cx| {
                        if app.grading.writing.is_none() && app.grading.chosen.is_some() {
                            app.close_paper(cx);
                        }
                    }),
                )
                .on_mouse_up(MouseButton::Left, release.clone())
                .on_mouse_up_out(MouseButton::Left, release)
                .child(head(t, &k, &ask))
                .children(paper.before.as_ref().map(|b| {
                    self.rubric(t, &k, b.round, b.marks.len(), b.acted_on(&paper.text), cx)
                }))
                .child(body)
                .children(grade(t, &k, &ask))
                .child(
                    canvas(
                        move |b, window, _| {
                            let w = f32::from(b.size.width);
                            if (width.get() >= P::WIDE) != (w >= P::WIDE) {
                                window.refresh();
                            }
                            width.set(w);
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .inset_0(),
                );
        Some(
            div()
                .w_full()
                .flex()
                .flex_col()
                .items_center()
                .pb(px(tokens::PAD))
                .child(sheet),
        )
    }

    /// One block of the document, set as the web desk sets it.
    #[allow(clippy::too_many_arguments)]
    fn block(
        &self,
        t: &Theme,
        k: &Inks,
        s: &Shown,
        strokes: &Rc<Vec<Stroked>>,
        washes: &Washes,
        cell_notes: Vec<Vec<AnyElement>>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut cell_notes = cell_notes.into_iter();
        let text = |p: &Piece, cx: &mut Context<Self>| piece(p, k, strokes, washes, cx);
        match &s.block.kind {
            Block::Heading(level) => {
                let size = match level {
                    1 => P::H1,
                    2 => P::H2,
                    3 => P::H3,
                    _ => P::TEXT,
                };
                div()
                    .mt(px(P::BLOCK_GAP))
                    .font_family(t.ty.display)
                    .font_weight(t.ty.display_weight)
                    .text_size(px(size))
                    .line_height(px(size * 1.25))
                    .children(s.pieces.first().map(|p| text(p, cx)))
                    .into_any_element()
            }
            Block::Item { number, depth } => div()
                .flex()
                .gap(px(10.))
                .pl(px(*depth as f32 * P::TEXT * 1.4))
                .child(
                    div()
                        .flex_none()
                        .min_w(px(P::TEXT * 0.8))
                        .flex()
                        .justify_end()
                        .text_color(k.soft)
                        .child(match number {
                            Some(n) => div().child(n.clone()),
                            None => div()
                                .mt(px(P::TEXT * P::LEADING / 2. - 3.))
                                .size(px(6.))
                                .rounded(px(1.))
                                .bg(k.ink),
                        }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .children(s.pieces.first().map(|p| text(p, cx))),
                )
                .into_any_element(),
            Block::Quote => div()
                .pl(px(P::TEXT * 1.2))
                .italic()
                .text_color(k.soft)
                .children(s.pieces.first().map(|p| text(p, cx)))
                .into_any_element(),
            Block::Code => div()
                .px(px(12.))
                .py(px(10.))
                .rounded(px(2.))
                .bg(alpha(k.ink, 0.05))
                .font_family(FAMILY)
                .text_size(px(P::TEXT * 0.8))
                .line_height(px(P::TEXT * 0.8 * 1.5))
                .children(s.pieces.first().map(|p| text(p, cx)))
                .into_any_element(),
            Block::Rule => div()
                .my(px(P::BLOCK_GAP))
                .h(px(1.5))
                .bg(k.rule)
                .into_any_element(),
            Block::Table(rows) => {
                let mut cells = s.pieces.iter();
                let mut table = div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .text_size(px(P::TEXT * 0.9))
                    .border_1()
                    .border_color(k.rule);
                for (r, n) in rows.iter().enumerate() {
                    let mut row = div()
                        .flex()
                        .when(r > 0, |d| d.border_t_1().border_color(k.rule));
                    for c in 0..*n {
                        let cell = cells.next();
                        row = row.child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .px(px(8.))
                                .py(px(4.))
                                .when(c > 0, |d| d.border_l_1().border_color(k.rule))
                                .when(r == 0, |d| d.font_weight(FontWeight::BOLD))
                                .children(cell.map(|p| text(p, cx)))
                                .children(cell_notes.next().unwrap_or_default()),
                        );
                    }
                    table = table.child(row);
                }
                table.into_any_element()
            }
            Block::Para => div()
                .children(s.pieces.first().map(|p| text(p, cx)))
                .into_any_element(),
        }
    }

    /// The notes on passages ending in `span`: his, then the last
    /// round's, fainter.
    #[allow(clippy::too_many_arguments)]
    fn margin_notes(
        &self,
        t: &Theme,
        k: &Inks,
        span: Range<usize>,
        marks: &[Mark],
        olds: &[(Mark, Vec<Range<usize>>, Followed)],
        round: Option<u32>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let here = |end: usize| span.contains(&end.saturating_sub(1));
        let mut out = vec![];
        for m in marks {
            if m.kind != "note"
                || m.text.trim().is_empty()
                || !paper::range(m).is_some_and(|r| here(r.end))
            {
                continue;
            }
            let id = m.id.clone();
            let on = self.grading.mark.as_deref() == Some(&m.id);
            out.push(
                div()
                    .id(SharedString::from(format!("note-{}", m.id)))
                    .font_family(P::HAND)
                    .text_size(px(P::HAND_TEXT))
                    .line_height(px(P::HAND_TEXT * 1.1))
                    .text_color(k.grade)
                    .cursor_pointer()
                    .when(on, |d| d.bg(k.wash))
                    .rounded(px(2.))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| {
                        if let Some(m) = app.marks().into_iter().find(|m| m.id == id) {
                            app.grading.close();
                            app.grading.chosen = paper::range(&m);
                            app.grading.mark = Some(m.id);
                            cx.notify();
                        }
                    }))
                    .child(m.text.trim().to_string())
                    .into_any_element(),
            );
        }
        for (m, kept, followed) in olds {
            let Some(last) = kept.last() else { continue };
            if m.text.trim().is_empty() || !here(last.end) {
                continue;
            }
            out.push(
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .font_family(t.ty.reading)
                            .text_size(px(10.))
                            .line_height(px(14.))
                            .text_color(k.soft)
                            .child(format!(
                                "ROUND {} {} · {}",
                                round.unwrap_or(1),
                                paper::pen_name(&m.kind).to_uppercase(),
                                followed.words().to_uppercase()
                            )),
                    )
                    .child(
                        div()
                            .font_family(P::HAND)
                            .text_size(px(P::HAND_TEXT * 0.9))
                            .line_height(px(P::HAND_TEXT))
                            .text_color(k.old)
                            .child(m.text.trim().to_string()),
                    )
                    .into_any_element(),
            );
        }
        out
    }

    /// What the agent did with his last round's marks, and the way to
    /// show or hide what changed.
    fn rubric(
        &self,
        t: &Theme,
        k: &Inks,
        round: u32,
        marked: usize,
        acted: usize,
        cx: &mut Context<Self>,
    ) -> Div {
        let shown = !self.grading.hide_changes;
        div()
            .flex()
            .items_center()
            .gap(px(10.))
            .mb(px(P::BLOCK_GAP + 4.))
            .text_size(px(P::TEXT * 0.82))
            .text_color(k.soft)
            .child(div().flex_1().child(format!(
                "Round {round}: {acted} of your {marked} marked {} changed",
                if marked == 1 { "passage" } else { "passages" }
            )))
            .child(
                pen_button(t, k, "changes", "Changes", None, shown)
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(|app, _: &ClickEvent, _, cx| {
                        app.grading.hide_changes = !app.grading.hide_changes;
                        cx.notify();
                    })),
            )
    }

    /// The pens for the passage he chose, or the pen he writes with: at
    /// the foot of the room, so they are in reach wherever the passage is
    /// (a long table's cell, a passage scrolled half away).
    pub fn paper_tray(&self, t: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let g = &self.grading;
        if self.read_only() || g.press.is_some() {
            return None;
        }
        let r = g.chosen.clone()?;
        let k = inks();
        let words = self
            .paper
            .as_ref()
            .map(|(_, p)| paper::quote(&p.text, &r))
            .unwrap_or_default();
        let clipped: String = if words.chars().count() > 60 {
            format!("{}…", words.chars().take(60).collect::<String>().trim_end())
        } else {
            words
        };
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .w_full()
                .max_w(px(P::WIDTH))
                .px(px(14.))
                .py(px(10.))
                .rounded(px(3.))
                .border_1()
                .border_color(rgb(P::EDGE))
                .bg(k.paper)
                .shadow_lg()
                .child(
                    div()
                        .font_family(t.ty.reading)
                        .text_size(px(P::TEXT * 0.75))
                        .line_height(px(P::TEXT))
                        .text_color(k.soft)
                        .truncate()
                        .child(format!("\u{201c}{clipped}\u{201d}")),
                )
                .child(self.tray(t, &k, cx))
                .into_any_element(),
        )
    }

    fn tray(&self, t: &Theme, k: &Inks, cx: &mut Context<Self>) -> gpui::Stateful<Div> {
        let g = &self.grading;
        let on = g
            .mark
            .as_ref()
            .and_then(|id| self.marks().into_iter().find(|m| &m.id == id))
            .map(|m| m.kind);
        let tray = div()
            .id("tray")
            .debug_selector(|| "tray".into())
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(6.))
            .font_family(t.ty.reading)
            .text_size(px(P::TEXT * 0.8))
            .line_height(px(P::TEXT))
            .cursor_default()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation());
        if let Some(kind) = g.writing {
            return tray
                .child(div().text_color(k.soft).child(if kind == "note" {
                    "Note"
                } else {
                    "Rewrite as"
                }))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(200.))
                        .border_b_1()
                        .border_color(k.grade)
                        .child(self.pen.clone()),
                )
                .child(
                    pen_button(t, k, "pen-done", "Done", Some("Enter"), true)
                        .on_click(cx.listener(|app, _: &ClickEvent, _, cx| app.put_pen_down(cx))),
                );
        }
        let mut tray = tray;
        for kind in paper::TEXT_MARKS {
            let words = if kind == "tick" {
                "\u{2713} Right"
            } else {
                paper::pen_name(kind)
            };
            tray = tray.child(
                pen_button(
                    t,
                    k,
                    SharedString::from(format!("pen-{kind}")),
                    words,
                    Some(paper::pen_key(kind)),
                    on.as_deref() == Some(kind),
                )
                .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| app.use_pen(kind, cx))),
            );
        }
        if g.tapped.is_some() {
            tray = tray.child(div().w(px(10.)));
            for level in Level::ALL {
                tray = tray.child(
                    pen_button(
                        t,
                        k,
                        SharedString::from(format!("level-{}", level.name())),
                        level.name(),
                        None,
                        g.level == level,
                    )
                    .on_click(
                        cx.listener(move |app, _: &ClickEvent, _, cx| app.set_level(level, cx)),
                    ),
                );
            }
        }
        tray.child(div().flex_1())
            .when(g.mark.is_some(), |d| {
                d.child(
                    pen_button(t, k, "pen-remove", "Remove", Some("Del"), false)
                        .on_click(cx.listener(|app, _: &ClickEvent, _, cx| app.remove_chosen(cx))),
                )
            })
            .child(
                pen_button(t, k, "pen-close", "\u{2715}", Some("Esc"), false)
                    .on_click(cx.listener(|app, _: &ClickEvent, _, cx| app.close_paper(cx))),
            )
    }
}

/// A pen in the tray: its name and key; in the grader's red when it is
/// the one on the passage.
fn pen_button(
    _t: &Theme,
    k: &Inks,
    id: impl Into<gpui::ElementId>,
    words: &'static str,
    key: Option<&'static str>,
    on: bool,
) -> gpui::Stateful<Div> {
    let ink = if on { k.grade } else { k.ink };
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(5.))
        .px(px(9.))
        .py(px(3.))
        .rounded(px(3.))
        .border_1()
        .border_color(if on { k.grade } else { k.rule })
        .bg(if on { k.wash } else { k.paper })
        .text_color(ink)
        .cursor_pointer()
        .hover(|d| d.border_color(k.grade))
        .child(words)
        .children(key.map(|key| {
            div()
                .text_size(px(P::TEXT * 0.62))
                .text_color(k.soft)
                .child(key)
        }))
}

/// The sheet's head: the document's name, its round, a rule and the
/// grader's diamond.
fn head(t: &Theme, k: &Inks, ask: &Ask) -> Div {
    let name = ask.doc.as_ref().map(|d| d.name.clone()).unwrap_or_default();
    let (ink, grade) = (k.ink, k.grade);
    div()
        .relative()
        .flex()
        .justify_between()
        .gap(px(12.))
        .pb(px(10.))
        .mb(px(20.))
        .font_family(t.ty.display)
        .text_size(px(12.))
        .line_height(px(16.))
        .text_color(k.soft)
        .child(div().min_w_0().truncate().child(name.to_uppercase()))
        .child(div().flex_none().child(format!("ROUND {}", ask.round())))
        .child(
            canvas(
                |_, _, _| {},
                move |b, _, window, _| {
                    let y = b.bottom() - px(0.75);
                    let mut line = PathBuilder::stroke(px(1.5));
                    line.move_to(point(b.left(), y));
                    line.line_to(point(b.right(), y));
                    if let Ok(p) = line.build() {
                        window.paint_path(p, ink);
                    }
                    let c = point(b.center().x, y);
                    let r = px(5.5);
                    for (fill, w) in [(grade, None), (ink, Some(1.5))] {
                        let mut d = match w {
                            Some(w) => PathBuilder::stroke(px(w)),
                            None => PathBuilder::fill(),
                        };
                        d.move_to(point(c.x, c.y - r));
                        d.line_to(point(c.x + r, c.y));
                        d.line_to(point(c.x, c.y + r));
                        d.line_to(point(c.x - r, c.y));
                        d.close();
                        if let Ok(p) = d.build() {
                            window.paint_path(p, fill);
                        }
                    }
                },
            )
            .absolute()
            .inset_0(),
        )
}

/// Lines taken out since the round he marked, struck through in grey.
fn gone_block(k: &Inks, words: &str) -> Div {
    div()
        .mb(px(P::BLOCK_GAP))
        .pl(px(12.))
        .text_size(px(P::TEXT * 0.92))
        .text_color(k.gone)
        .child(StyledText::new(words.to_string()).with_highlights([(
            0..words.len(),
            HighlightStyle {
                strikethrough: Some(StrikethroughStyle {
                    thickness: px(1.),
                    color: Some(k.gone),
                }),
                ..Default::default()
            },
        )]))
}

/// The grade at the foot of an answered paper: his verdict circled in the
/// grader's hand, and his words under it.
fn grade(_t: &Theme, k: &Inks, ask: &Ask) -> Option<Div> {
    let answer = ask.answer.as_ref().filter(|_| !ask.is_open())?;
    let word = grade_word(&answer.verdict, &answer.options);
    let grade = k.grade;
    let seed = format!("{}-{}", ask.id, ask.round());
    Some(
        div()
            .mt(px(P::BLOCK_GAP * 2.))
            .pt(px(10.))
            .border_t_1()
            .border_color(k.rule)
            .flex()
            .flex_col()
            .items_end()
            .gap(px(10.))
            .font_family(P::HAND)
            .text_color(k.grade)
            .child(
                div()
                    .relative()
                    .px(px(18.))
                    .py(px(4.))
                    .text_size(px(P::STAMP))
                    .line_height(px(P::STAMP * 1.1))
                    .child(word)
                    .child(
                        canvas(
                            |_, _, _| {},
                            move |b, _, window, _| {
                                let r = Rect {
                                    x: f32::from(b.origin.x) + 6.,
                                    y: f32::from(b.origin.y) + 2.,
                                    w: f32::from(b.size.width) - 12.,
                                    h: f32::from(b.size.height) - 4.,
                                };
                                let mut hand = paper::Hand::new(&seed);
                                paint_stroke(&paper::hand_loop(r, &mut hand), 2.5, grade, window);
                            },
                        )
                        .absolute()
                        .inset_0(),
                    ),
            )
            .when(!answer.comment.trim().is_empty(), |d| {
                d.child(
                    div()
                        .max_w(px(P::WIDTH * 0.6))
                        .text_size(px(P::HAND_TEXT * 1.1))
                        .line_height(px(P::HAND_TEXT * 1.2))
                        .child(answer.comment.trim().to_string()),
                )
            }),
    )
}

/// The piece of the plain page that shows the document's character `at`.
fn piece_of(shown: &[Shown], at: usize) -> Option<&Piece> {
    shown
        .iter()
        .flat_map(|s| s.pieces.iter())
        .find(|p| p.src.contains(&Some(at)))
}

/// One text of the page: its words styled, washed and inked, taking his
/// press, drag and release.
fn piece(
    p: &Piece,
    k: &Inks,
    strokes: &Rc<Vec<Stroked>>,
    washes: &Washes,
    cx: &mut Context<DeskApp>,
) -> AnyElement {
    if p.text.is_empty() {
        return div().into_any_element();
    }
    let mut spans: Vec<(Range<usize>, HighlightStyle)> = vec![];
    let mut faces: Vec<Range<usize>> = vec![];
    let mut monos: Vec<Range<usize>> = vec![];
    for (r, style) in &p.styles {
        let h = match style {
            Style::Bold => HighlightStyle {
                font_weight: Some(FontWeight::BOLD),
                ..Default::default()
            },
            Style::Em => HighlightStyle {
                font_style: Some(FontStyle::Italic),
                ..Default::default()
            },
            Style::Code => {
                monos.push(r.clone());
                HighlightStyle {
                    background_color: Some(alpha(k.ink, 0.06)),
                    ..Default::default()
                }
            }
            Style::Struck => HighlightStyle {
                strikethrough: Some(StrikethroughStyle {
                    thickness: px(1.),
                    color: Some(k.ink),
                }),
                ..Default::default()
            },
            Style::Link => HighlightStyle {
                underline: Some(UnderlineStyle {
                    thickness: px(1.),
                    color: Some(k.rule),
                    wavy: false,
                }),
                ..Default::default()
            },
        };
        spans.push((r.clone(), h));
    }
    for (r, extra) in &p.extras {
        match extra {
            Extra::Rewrite => {
                faces.push(r.clone());
                spans.push((
                    r.clone(),
                    HighlightStyle {
                        color: Some(k.grade),
                        ..Default::default()
                    },
                ));
            }
            Extra::Gone => spans.push((
                r.clone(),
                HighlightStyle {
                    color: Some(k.gone),
                    strikethrough: Some(StrikethroughStyle {
                        thickness: px(1.),
                        color: Some(k.gone),
                    }),
                    ..Default::default()
                },
            )),
        }
    }
    for r in &washes.new {
        if let Some(b) = p.bytes_of(r) {
            spans.push((
                b,
                HighlightStyle {
                    background_color: Some(k.new),
                    ..Default::default()
                },
            ));
        }
    }
    if let Some(b) = washes.chosen.as_ref().and_then(|r| p.bytes_of(r)) {
        spans.push((
            b,
            HighlightStyle {
                background_color: Some(k.wash),
                ..Default::default()
            },
        ));
    }
    let families: Vec<(Range<usize>, SharedString)> = merge_faces(&faces, &monos);
    // Every face's stretch is a highlight of its own, so its runs fall
    // wholly inside it.
    for (r, _) in &families {
        spans.push((r.clone(), HighlightStyle::default()));
    }
    let text = StyledText::new(p.text.clone())
        .with_highlights(flatten(&p.text, spans))
        .with_font_family_overrides(families);
    let layout = text.layout().clone();
    let piece = Rc::new(p.clone());
    let mine: Vec<Stroked> = strokes
        .iter()
        .filter(|s| p.bytes_of(&s.range).is_some())
        .cloned()
        .collect();
    let (down, moved) = (
        (layout.clone(), piece.clone()),
        (layout.clone(), piece.clone()),
    );
    let ink = (layout, piece);
    div()
        .relative()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |app, e: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                if let Some(at) = source_under(&down.0, &down.1, e.position) {
                    app.press_text(at, cx);
                }
            }),
        )
        .on_mouse_move(cx.listener(move |app, e: &MouseMoveEvent, _, cx| {
            if e.pressed_button == Some(MouseButton::Left)
                && app.grading.press.is_some()
                && let Some(at) = source_under(&moved.0, &moved.1, e.position)
            {
                app.drag_text(at, cx);
            }
        }))
        .child(text)
        .child(
            canvas(
                |_, _, _| {},
                move |_, _, window, _| {
                    for s in &mine {
                        paint_mark(&ink.0, &ink.1, s, window);
                    }
                },
            )
            .absolute()
            .inset_0(),
        )
        .into_any_element()
}

/// The document's character under a point of a piece's text.
fn source_under(layout: &TextLayout, p: &Piece, at: gpui::Point<Pixels>) -> Option<usize> {
    let i = layout.index_for_position(at).unwrap_or_else(|i| i);
    let mut i = i.min(p.text.len().saturating_sub(1));
    while !p.text.is_char_boundary(i) {
        i -= 1;
    }
    p.source_at(i)
}

/// The hand font over rewrites and the mono face over code, in order.
fn merge_faces(hand: &[Range<usize>], mono: &[Range<usize>]) -> Vec<(Range<usize>, SharedString)> {
    let mut v: Vec<(Range<usize>, SharedString)> = hand
        .iter()
        .map(|r| (r.clone(), SharedString::from(P::HAND)))
        .chain(mono.iter().map(|r| (r.clone(), SharedString::from(FAMILY))))
        .collect();
    v.sort_by_key(|(r, _)| r.start);
    let mut out: Vec<(Range<usize>, SharedString)> = vec![];
    for (r, f) in v {
        match out.last() {
            Some((last, _)) if r.start < last.end => {}
            _ => out.push((r, f)),
        }
    }
    out
}

/// Overlapping highlights as the sorted, separate stretches the text
/// takes, each with every highlight over it combined.
fn flatten(
    text: &str,
    spans: Vec<(Range<usize>, HighlightStyle)>,
) -> Vec<(Range<usize>, HighlightStyle)> {
    let mut cuts: Vec<usize> = spans.iter().flat_map(|(r, _)| [r.start, r.end]).collect();
    cuts.retain(|c| *c <= text.len() && text.is_char_boundary(*c));
    cuts.sort_unstable();
    cuts.dedup();
    let mut out = vec![];
    for w in cuts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let mut style: Option<HighlightStyle> = None;
        for (r, h) in &spans {
            if r.start <= a && b <= r.end {
                style = Some(match style {
                    Some(s) => s.highlight(*h),
                    None => *h,
                });
            }
        }
        if let Some(s) = style {
            out.push((a..b, s));
        }
    }
    out
}

/// The boxes of the lines a stretch of a piece's text sits on.
fn line_boxes(layout: &TextLayout, p: &Piece, bytes: Range<usize>) -> Vec<Rect> {
    let h = f32::from(layout.line_height());
    let mut boxes = vec![];
    for (i, c) in p.text[bytes.clone()].char_indices() {
        let i = bytes.start + i;
        if c.is_whitespace() {
            continue;
        }
        let Some(at) = layout.position_for_index(i) else {
            continue;
        };
        let right = layout
            .position_for_index(i + c.len_utf8())
            .filter(|n| n.y == at.y && n.x > at.x)
            .map_or(f32::from(at.x) + h * 0.4, |n| f32::from(n.x));
        boxes.push(Rect {
            x: f32::from(at.x),
            y: f32::from(at.y),
            w: right - f32::from(at.x),
            h,
        });
    }
    paper::join_lines(boxes)
}

/// A mark's strokes over a piece's words; a tick only where its passage
/// ends.
fn paint_mark(layout: &TextLayout, p: &Piece, s: &Stroked, window: &mut Window) {
    let Some(bytes) = p.bytes_of(&s.range) else {
        return;
    };
    // A rewrite strikes only the words it replaces, not itself.
    let lines = line_boxes(layout, p, bytes);
    if lines.is_empty() {
        return;
    }
    let ends_here = p.src.contains(&Some(s.range.end.saturating_sub(1)));
    if s.kind == "tick" && !ends_here {
        return;
    }
    // The text's line box is taller than its letters: strike and loop the
    // letters.
    let lines: Vec<Rect> = lines
        .into_iter()
        .map(|r| Rect {
            y: r.y + r.h * 0.18,
            h: r.h * 0.64,
            ..r
        })
        .collect();
    for stroke in paper::ink(&s.kind, &s.seed, &lines) {
        paint_stroke(&stroke, s.width, s.color, window);
    }
}

fn paint_stroke(stroke: &paper::Stroke, width: f32, color: Hsla, window: &mut Window) {
    let ((x, y), curves) = stroke;
    if curves.is_empty() {
        return;
    }
    let p = |(x, y): (f32, f32)| point(px(x), px(y));
    let mut path = PathBuilder::stroke(px(width));
    path.move_to(p((*x, *y)));
    for [a, b, to] in curves {
        path.cubic_bezier_to(p(*to), p(*a), p(*b));
    }
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_highlights_become_separate_stretches() {
        let bold = HighlightStyle {
            font_weight: Some(FontWeight::BOLD),
            ..Default::default()
        };
        let wash = HighlightStyle {
            background_color: Some(gpui::red()),
            ..Default::default()
        };
        let out = flatten("abcdefgh", vec![(0..4, bold), (2..6, wash)]);
        let ranges: Vec<Range<usize>> = out.iter().map(|(r, _)| r.clone()).collect();
        assert_eq!(ranges, [0..2, 2..4, 4..6]);
        assert_eq!(out[1].1.font_weight, Some(FontWeight::BOLD));
        assert!(out[1].1.background_color.is_some());
    }
}
