//! The open file's changes since the last commit: side by side when there
//! is room for both, else inline, each line with its numbers, the whole
//! changes with a few lines around them and the rest folded, stepped
//! through with N and Shift+N.
use super::*;
use crate::open::Sides;
use crate::store::diff::Kind as RowKind;
use crate::store::diff::{Shown, change_starts, fold};
use ahjo_app_kit::theme::{FAMILY, alpha};
use ahjo_app_kit::widgets::syntax;
use std::rc::Rc;

/// Under this width the two sides do not both fit: inline.
const SIDE_BY_SIDE: f32 = 960.;
/// A changed line's wash.
const WASH: f32 = 0.12;
/// The side a line is missing from: a faint fill where it would be.
const ABSENT: f32 = 0.03;
/// A line's height at text `text` pixels: the code's.
fn line(text: f32) -> f32 {
    (text * 1.55).round()
}

/// What the changes view shows: its rows, folds included, and which of
/// the diff's lines (rows inline, pairs side by side) are changed.
pub struct Laid {
    pub wide: bool,
    pub shown: Vec<Shown>,
    pub changed: Vec<bool>,
}

impl Laid {
    /// Where each change starts among the rows shown.
    pub fn starts(&self) -> Vec<usize> {
        change_starts(self.shown.iter().map(|s| s.changed(&self.changed)))
    }

    /// The row the view starts at to show a change starting at row `s`:
    /// the lines before it, and the fold above those when there is one.
    pub fn top_for(&self, s: usize) -> usize {
        let top = s.saturating_sub(GO_LINES);
        match top.checked_sub(1).and_then(|t| self.shown.get(t)) {
            Some(Shown::Fold { .. }) => top - 1,
            _ => top,
        }
    }
}

impl FilesPage {
    /// The open file's changes as the view lays them out now.
    pub fn diff_laid(&self) -> Option<Laid> {
        let o = self.opened.as_ref()?;
        let sides = o.sides.as_ref()?;
        let wide = self.body_room.get().unwrap_or(0.) >= SIDE_BY_SIDE && !self.diff_inline;
        // A whole file is one hunk, whose head says nothing; between
        // several, each head says where the next starts.
        let (changed, hunk): (Vec<bool>, Vec<bool>) = if wide {
            sides
                .pairs
                .iter()
                .map(|p| (p.changed, p.hunk.is_some()))
                .unzip()
        } else {
            sides
                .rows
                .iter()
                .map(|r| {
                    (
                        matches!(r.kind, RowKind::Add | RowKind::Delete),
                        r.kind == RowKind::Hunk,
                    )
                })
                .unzip()
        };
        let one_hunk = hunk.iter().filter(|h| **h).count() == 1;
        let hidden = if one_hunk { hunk } else { vec![] };
        Some(Laid {
            wide,
            shown: fold(&changed, &hidden, &o.diff_folds),
            changed,
        })
    }

    pub(super) fn diff_view(
        &self,
        t: &Theme,
        sides: Rc<Sides>,
        truncated: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(laid) = self.diff_laid() else {
            return div().into_any_element();
        };
        let room = self.body_room.get().unwrap_or(0.);
        let text = self.code_text(cx);
        let wide = laid.wide;
        let starts = laid.starts();
        let at = self.opened.as_ref().map_or(0, |o| o.diff_at);
        let digits = sides
            .rows
            .iter()
            .filter_map(|r| r.new.max(r.old))
            .max()
            .unwrap_or(1)
            .to_string()
            .len()
            .max(3);
        // Long lines run on to the right, scrolled to sideways (Shift and
        // the wheel) as the code view does; the numbers stay.
        let longest = sides
            .rows
            .iter()
            .filter(|r| r.kind != RowKind::Hunk)
            .map(|r| r.text.chars().count() + 3 * r.text.matches('\t').count())
            .max()
            .unwrap_or(0) as f32;
        let numbers = number_width(digits, text);
        let room_for_text = if wide {
            room / 2. - numbers
        } else {
            room - 2. * numbers - size::HEAD_PAD
        };
        let most_left = (longest * ADVANCE * text + size::PAD - room_for_text).max(0.);
        let left = self.diff_left.get().min(most_left);
        self.diff_left.set(left);
        let shown = Rc::new(laid.shown);
        let list = uniform_list(
            "files-diff-rows",
            shown.len(),
            cx.processor(move |p, range: std::ops::Range<usize>, _, cx| {
                let t = theme(cx).clone();
                let Some(sides) = p.opened.as_ref().and_then(|o| o.sides.clone()) else {
                    return vec![];
                };
                range
                    .filter_map(|i| shown.get(i).copied())
                    .map(|s| match s {
                        Shown::Fold { from, to } => fold_row(&t, from, to, text, cx),
                        Shown::Line(i) if wide => sides.pairs.get(i).map_or_else(
                            || div().into_any_element(),
                            |pair| pair_row(&t, &sides, pair, digits, text, left),
                        ),
                        Shown::Line(i) => sides.rows.get(i).map_or_else(
                            || div().into_any_element(),
                            |row| inline_row(&t, &sides, row, digits, text, left),
                        ),
                    })
                    .collect()
            }),
        )
        .track_scroll(&self.diff_scroll)
        .size_full();
        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .debug_selector(move || {
                if wide {
                    "files-diff-sides".into()
                } else {
                    "files-diff-inline".into()
                }
            })
            .font_family(FAMILY)
            .text_size(px(text))
            .child(measure(self.body_room.clone()))
            .child(self.sideways(text, most_left, cx))
            .child(self.diff_steps(t, at, starts.len(), cx))
            .child(div().flex_1().min_h_0().child(list))
            .when(truncated, |d| {
                d.child(pane::meta(t, "The rest is not shown", None))
            })
            .into_any_element()
    }

    /// Shift and the wheel (or a sideways wheel) over the changes scroll
    /// their lines sideways, before the list takes the wheel as up and
    /// down.
    fn sideways(&self, text: f32, most: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let me = cx.entity().downgrade();
        gpui::canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                let me = me.clone();
                window.on_mouse_event(move |e: &gpui::ScrollEvent, phase, _, cx| {
                    if phase != gpui::DispatchPhase::Capture || !bounds.contains(&e.position) {
                        return;
                    }
                    let d = e.delta.pixel_delta(px(line(text)));
                    let dx = if e.modifiers.shift && d.x == px(0.) {
                        d.y
                    } else {
                        d.x
                    };
                    if dx == px(0.) {
                        return;
                    }
                    cx.stop_propagation();
                    let _ = me.update(cx, |p, cx| {
                        let now = p.diff_left.get();
                        p.diff_left.set((now - f32::from(dx)).clamp(0., most));
                        cx.notify();
                    });
                });
            },
        )
        .absolute()
        .inset_0()
    }

    /// Above the changes: which change is in view of how many, and the
    /// way to the one before and the next, as find has.
    fn diff_steps(&self, t: &Theme, at: usize, of: usize, cx: &mut Context<Self>) -> Div {
        let step = |id: &'static str, label: &'static str, tip: &'static str, forward: bool| {
            pane::act(t, id, label)
                .debug_selector(move || id.into())
                .tooltip(kit::tip(tip))
                .on_click(cx.listener(move |p, _, _, cx| p.step_diff(forward, cx)))
        };
        let count = if of == 0 {
            "No changes".to_string()
        } else {
            format!("{} of {of}", (at + 1).min(of))
        };
        div()
            .flex_none()
            .flex()
            .items_center()
            .justify_end()
            .gap(px(size::HEAD_GAP / 2.))
            .px(px(size::HEAD_PAD))
            .border_b_1()
            .border_color(t.divider)
            .child(pane::meta(t, count, None).debug_selector(|| "files-diff-count".into()))
            .when(of > 0, |d| {
                d.child(step(
                    "files-diff-back",
                    "\u{2191}",
                    "Previous change (Shift+N)",
                    false,
                ))
                .child(step(
                    "files-diff-next",
                    "\u{2193}",
                    "Next change (N)",
                    true,
                ))
            })
    }

    /// N in the diff: to the next run of changes, or the one before.
    pub fn step_diff(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(laid) = self.diff_laid() else {
            return;
        };
        let starts = laid.starts();
        let offset = self.diff_scroll.0.borrow().base_handle.offset();
        let top = (f32::from(-offset.y) / line(self.code_text(cx))).round() as usize;
        // Where each change is shown from, against where the view is.
        let tops: Vec<usize> = starts.iter().map(|&s| laid.top_for(s)).collect();
        let at = self.opened.as_ref().map_or(0, |o| o.diff_at);
        let at_end = self.diff_scroll.is_scrolled_to_end() == Some(true);
        let next = if forward {
            // At the end the view cannot stand on the last few changes:
            // they come one at a time all the same.
            tops.iter()
                .position(|&t| t > top)
                .map(|k| if at_end { k.max(at + 1) } else { k })
                .or((at_end && at + 1 < tops.len()).then_some(at + 1))
                .filter(|&k| k < tops.len())
        } else if at_end && at > 0 && tops.get(at).is_some_and(|&t| t > top) {
            Some(at - 1)
        } else {
            tops.iter().rposition(|&t| t < top)
        };
        if let Some(k) = next {
            self.go_diff(k, cx);
        }
    }

    /// Shows change `k` with the lines before it, and the fold above
    /// those when there is one, so the view says what it left out.
    fn go_diff(&mut self, k: usize, cx: &mut Context<Self>) {
        let Some(laid) = self.diff_laid() else {
            return;
        };
        let Some(&s) = laid.starts().get(k) else {
            self.diff_scroll
                .scroll_to_item_strict(0, gpui::ScrollStrategy::Top);
            return;
        };
        let top = laid.top_for(s);
        if let Some(o) = self.opened.as_mut() {
            o.diff_at = k;
        }
        self.diff_scroll
            .scroll_to_item_strict(top, gpui::ScrollStrategy::Top);
        cx.notify();
    }

    /// The changes view opened: on its first change.
    pub fn diff_opened(&mut self, cx: &mut Context<Self>) {
        self.diff_left.set(0.);
        self.go_diff(0, cx);
    }

    /// Shows the unchanged lines a fold hid.
    fn open_fold(&mut self, from: usize, cx: &mut Context<Self>) {
        if let Some(o) = self.opened.as_mut() {
            o.diff_folds.insert(from);
        }
        cx.notify();
    }
}

/// Folded unchanged lines: how many, and a click shows them.
fn fold_row(
    t: &Theme,
    from: usize,
    to: usize,
    text: f32,
    cx: &mut Context<FilesPage>,
) -> AnyElement {
    let n = to - from;
    line_box(text)
        .id(("files-diff-fold", from))
        .debug_selector(|| "files-diff-fold".into())
        .bg(t.surface)
        .text_color(t.dim)
        .pl(px(size::HEAD_PAD))
        .cursor_pointer()
        .hover(|s| s.text_color(t.text))
        .tooltip(kit::tip("Show these lines"))
        .child(format!(
            "\u{22ef} {n} unchanged {}",
            if n == 1 { "line" } else { "lines" }
        ))
        .on_click(cx.listener(move |p, _, _, cx| p.open_fold(from, cx)))
        .into_any_element()
}

fn number_width(digits: usize, text: f32) -> f32 {
    size::SMALL * text / size::TEXT * ADVANCE * (digits + 2) as f32
}

fn number(t: &Theme, n: Option<usize>, digits: usize, text: f32) -> Div {
    let small = size::SMALL * text / size::TEXT;
    div()
        .flex_none()
        .w(px(number_width(digits, text)))
        .pr(px(size::HEAD_GAP))
        .flex()
        .justify_end()
        .text_size(px(small))
        .text_color(t.dim)
        .child(n.map(|n| n.to_string()).unwrap_or_default())
}

/// What colours a line as code: its language and what it starts inside.
type Paint = Option<(&'static syntax::Lang, syntax::Carry)>;

/// A line's text, `left` pixels of it scrolled out of view to the left.
fn text_cell(t: &Theme, text: &str, paint: Paint, left: f32) -> Div {
    let text = text.replace('\t', "    ");
    let cell = div()
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap();
    let inner = div().ml(px(-left));
    let Some((lang, carry)) = paint else {
        return cell.child(inner.child(text));
    };
    let colours: Vec<_> = syntax::spans(lang, &text, carry)
        .0
        .into_iter()
        .map(|(range, token)| {
            let style = gpui::HighlightStyle {
                color: Some(syntax::colour(t, token)),
                ..Default::default()
            };
            (range, style)
        })
        .collect();
    cell.child(
        inner.child(gpui::StyledText::new(SharedString::from(text)).with_highlights(colours)),
    )
}

fn line_box(text: f32) -> Div {
    div().h(px(line(text))).w_full().flex().items_center()
}

fn inline_row(
    t: &Theme,
    sides: &Sides,
    r: &crate::store::diff::Row,
    digits: usize,
    text: f32,
    left: f32,
) -> AnyElement {
    let (sign, wash, colour) = match r.kind {
        RowKind::Add => ("+", Some(t.done), t.text),
        RowKind::Delete => ("-", Some(t.error), t.text),
        RowKind::Context => (" ", None, t.text2),
        RowKind::Hunk => ("", None, t.dim),
    };
    if r.kind == RowKind::Hunk {
        return line_box(text)
            .bg(t.surface)
            .text_color(t.dim)
            .pl(px(size::HEAD_PAD))
            .child(text_cell(t, &r.text, None, 0.))
            .into_any_element();
    }
    line_box(text)
        .when_some(wash, |d, c| d.bg(alpha(c, WASH)))
        .text_color(colour)
        .child(number(t, r.old, digits, text))
        .child(number(t, r.new, digits, text))
        .child(
            div()
                .flex_none()
                .w(px(size::HEAD_PAD))
                .text_color(wash.unwrap_or(t.dim))
                .child(sign),
        )
        .child(text_cell(
            t,
            &r.text,
            match (r.new, r.old) {
                (Some(n), _) => sides.paint(n, true),
                (None, Some(n)) => sides.paint(n, false),
                (None, None) => None,
            },
            left,
        ))
        .into_any_element()
}

fn pair_row(
    t: &Theme,
    sides: &Sides,
    p: &crate::store::diff::Pair,
    digits: usize,
    text: f32,
    left: f32,
) -> AnyElement {
    if let Some(h) = &p.hunk {
        return line_box(text)
            .bg(t.surface)
            .text_color(t.dim)
            .pl(px(size::HEAD_PAD))
            .child(text_cell(t, h, None, 0.))
            .into_any_element();
    }
    let side = |line: &Option<(usize, String)>, colour: gpui::Hsla, new: bool| {
        let present = line.is_some();
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .items_center()
            .when(p.changed && present, |d| d.bg(alpha(colour, WASH)))
            // A side with no line here: the other side's lines came in or
            // went out.
            .when(!present, |d| d.bg(alpha(t.text, ABSENT)))
            .text_color(if p.changed { t.text } else { t.text2 })
            .child(number(t, line.as_ref().map(|l| l.0), digits, text))
            .child(text_cell(
                t,
                line.as_ref().map_or("", |l| l.1.as_str()),
                line.as_ref().and_then(|l| sides.paint(l.0, new)),
                left,
            ))
    };
    line_box(text)
        .child(side(&p.old, t.error, false))
        .child(div().flex_none().w(px(1.)).h_full().bg(t.divider))
        .child(side(&p.new, t.done, true))
        .into_any_element()
}
