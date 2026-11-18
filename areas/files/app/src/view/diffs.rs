//! The open file's changes since the last commit: side by side when there
//! is room for both, else inline, each line with its numbers, the whole
//! file around the changes, stepped through with N.
use super::*;
use crate::open::Sides;
use crate::store::diff::Kind as RowKind;
use ahjo_app_kit::theme::{FAMILY, alpha};
use ahjo_app_kit::widgets::syntax;
use std::rc::Rc;

/// Under this width the two sides do not both fit: inline.
const SIDE_BY_SIDE: f32 = 960.;
/// A changed line's wash.
const WASH: f32 = 0.12;
/// The side a line is missing from: a faint fill where it would be.
const ABSENT: f32 = 0.03;
/// A line's height: the code's.
const LINE: f32 = size::ROW - size::HEAD_GAP;

impl FilesPage {
    pub(super) fn diff_view(
        &self,
        t: &Theme,
        sides: Rc<Sides>,
        truncated: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let room = self.body_room.get().unwrap_or(0.);
        let wide = room >= SIDE_BY_SIDE && !self.diff_inline;
        let n = if wide {
            sides.pairs.len()
        } else {
            sides.rows.len()
        };
        let digits = sides
            .rows
            .iter()
            .filter_map(|r| r.new.max(r.old))
            .max()
            .unwrap_or(1)
            .to_string()
            .len()
            .max(3);
        let list = uniform_list(
            "files-diff-rows",
            n,
            cx.processor(move |p, range: std::ops::Range<usize>, _, cx| {
                let t = theme(cx).clone();
                let Some(sides) = p.opened.as_ref().and_then(|o| o.sides.clone()) else {
                    return vec![];
                };
                range
                    .map(|i| {
                        if wide {
                            pair_row(&t, &sides, &sides.pairs[i], digits)
                        } else {
                            inline_row(&t, &sides, &sides.rows[i], digits)
                        }
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
            .text_size(px(size::TEXT))
            .child(measure(self.body_room.clone()))
            .child(div().flex_1().min_h_0().child(list))
            .when(truncated, |d| {
                d.child(pane::meta(t, "The rest is not shown", None))
            })
            .into_any_element()
    }

    /// N in the diff: to the next run of changes, or the one before.
    pub fn step_diff(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(sides) = self.opened.as_ref().and_then(|o| o.sides.clone()) else {
            return;
        };
        let wide = self.body_room.get().unwrap_or(0.) >= SIDE_BY_SIDE && !self.diff_inline;
        let starts = if wide {
            crate::store::diff::change_starts(sides.pairs.iter().map(|p| p.changed))
        } else {
            crate::store::diff::change_starts(
                sides
                    .rows
                    .iter()
                    .map(|r| matches!(r.kind, RowKind::Add | RowKind::Delete)),
            )
        };
        let offset = self.diff_scroll.0.borrow().base_handle.offset();
        let here = (f32::from(-offset.y) / LINE).round() as usize + GO_LINES;
        let next = if forward {
            starts.into_iter().find(|&s| s > here)
        } else {
            starts.into_iter().rev().find(|&s| s < here)
        };
        if let Some(s) = next {
            self.diff_scroll
                .scroll_to_item_strict(s.saturating_sub(GO_LINES), gpui::ScrollStrategy::Top);
            cx.notify();
        }
    }
}

fn number(t: &Theme, n: Option<usize>, digits: usize) -> Div {
    div()
        .flex_none()
        .w(px(size::SMALL * ADVANCE * (digits + 2) as f32))
        .pr(px(size::HEAD_GAP))
        .flex()
        .justify_end()
        .text_size(px(size::SMALL))
        .text_color(t.dim)
        .child(n.map(|n| n.to_string()).unwrap_or_default())
}

/// What colours a line as code: its language and what it starts inside.
type Paint = Option<(&'static syntax::Lang, syntax::Carry)>;

fn text_cell(t: &Theme, text: &str, paint: Paint) -> Div {
    let text = text.replace('\t', "    ");
    let cell = div()
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap();
    let Some((lang, carry)) = paint else {
        return cell.child(text);
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
    cell.child(gpui::StyledText::new(SharedString::from(text)).with_highlights(colours))
}

fn line_box() -> Div {
    div().h(px(LINE)).w_full().flex().items_center()
}

fn inline_row(t: &Theme, sides: &Sides, r: &crate::store::diff::Row, digits: usize) -> AnyElement {
    let (sign, wash, colour) = match r.kind {
        RowKind::Add => ("+", Some(t.done), t.text),
        RowKind::Delete => ("-", Some(t.error), t.text),
        RowKind::Context => (" ", None, t.text2),
        RowKind::Hunk => ("", None, t.dim),
    };
    if r.kind == RowKind::Hunk {
        return line_box()
            .bg(t.surface)
            .text_color(t.dim)
            .pl(px(size::HEAD_PAD))
            .child(text_cell(t, &r.text, None))
            .into_any_element();
    }
    line_box()
        .when_some(wash, |d, c| d.bg(alpha(c, WASH)))
        .text_color(colour)
        .child(number(t, r.old, digits))
        .child(number(t, r.new, digits))
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
        ))
        .into_any_element()
}

fn pair_row(t: &Theme, sides: &Sides, p: &crate::store::diff::Pair, digits: usize) -> AnyElement {
    if let Some(h) = &p.hunk {
        return line_box()
            .bg(t.surface)
            .text_color(t.dim)
            .pl(px(size::HEAD_PAD))
            .child(text_cell(t, h, None))
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
            .child(number(t, line.as_ref().map(|l| l.0), digits))
            .child(text_cell(
                t,
                line.as_ref().map_or("", |l| l.1.as_str()),
                line.as_ref().and_then(|l| sides.paint(l.0, new)),
            ))
    };
    line_box()
        .child(side(&p.old, t.error, false))
        .child(div().flex_none().w(px(1.)).h_full().bg(t.divider))
        .child(side(&p.new, t.done, true))
        .into_any_element()
}
