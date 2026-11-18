//! The one page skeleton every page is drawn from (DESIGN.md, One app): a
//! list on the left, the thing on the right, one header line across both,
//! and the rows, tables, section labels, empty states and the Send button
//! that go in them. A page composes these; it does not draw its own frame,
//! card or table.
//!
//! ```text
//! ┌ list ───────────────────┬ detail ─────────────────────────────┐
//! │ head: filter   ≤3 acts  │ head: the thing's name   Send  acts │
//! ├─────────────────────────┼─────────────────────────────────────┤
//! │ label                   │ section label                       │
//! │ row                     │ table / lines                       │
//! └─────────────────────────┴─────────────────────────────────────┘
//! ```
//!
//! The list's head and the detail's head stand on one line, so the page has
//! one bar of chrome. A page without a list draws the detail alone, head
//! and all; a page without anything chosen fills the detail with [`empty`].

use crate::ask::Ask;
use crate::host::{Host, Status};
use crate::theme::{Theme, art, size, ty};
use gpui::{
    AnyElement, Bounds, Div, ElementId, FontWeight, Hsla, Pixels, ScrollHandle, SharedString,
    Stateful, canvas, div, fill, img, point, prelude::*, px, relative,
};

/// Where a list scrolled by `handle` is and that it scrolls: a thin thumb
/// at its right edge, drawn over it, sized to the share of the list shown.
/// Nothing shows while the list fits. Put it after the list in a parent
/// laid out `relative()`; it reads the handle as it paints, so it follows
/// a scroll without the view drawing again.
pub fn scrollbar(t: &Theme, handle: &ScrollHandle) -> impl IntoElement {
    let handle = handle.clone();
    let colour = crate::theme::alpha(t.dim, 0.55);
    canvas(
        |_, _, _| (),
        move |_, _, window, _| {
            if let Some(thumb) = thumb(handle.bounds(), handle.max_offset().y, handle.offset().y) {
                window.paint_quad(fill(thumb, colour).corner_radii(px(SCROLLBAR_W / 2.)));
            }
        },
    )
    .absolute()
    .inset_0()
}

const SCROLLBAR_W: f32 = 4.;
const SCROLLBAR_LEAST: f32 = 24.;

/// The thumb of a list shown in `view` that scrolls `most` further, now
/// scrolled by `offset` (negative going down).
fn thumb(view: Bounds<Pixels>, most: Pixels, offset: Pixels) -> Option<Bounds<Pixels>> {
    if most <= px(0.5) || view.size.height <= px(0.) {
        return None;
    }
    let h = view.size.height;
    let len = (h * (h / (h + most))).max(px(SCROLLBAR_LEAST)).min(h);
    let at = (-offset).clamp(px(0.), most) / most;
    let top = view.top() + (h - len) * at;
    let left = view.right() - px(SCROLLBAR_W + 2.);
    Some(Bounds::new(
        point(left, top),
        gpui::size(px(SCROLLBAR_W), len),
    ))
}

/// The page: the list beside the detail, filling the page's area.
pub fn page(t: &Theme, list: impl IntoElement, detail: impl IntoElement) -> Div {
    div().size_full().flex().bg(t.bg).child(list).child(detail)
}

/// The list column: [`size::LIST`] wide, its head on top, its rows under
/// it scrolling. `id` names its scroll.
pub fn list(t: &Theme, id: impl Into<ElementId>, head: Div, rows: impl IntoElement) -> Div {
    list_frame(t, head, scrolling(id, rows))
}

/// The list column around a body that scrolls itself, such as a uniform
/// list of many rows.
pub fn list_frame(t: &Theme, head: Div, body: impl IntoElement) -> Div {
    frame(t, size::LIST, head, body)
}

/// A list whose rows are sentences rather than names (work items, search
/// results), around a body that scrolls itself: [`size::LIST_WIDE`] wide
/// where the page has the room. In a narrow page the detail wins: the list
/// takes a third of it at most, and never less than [`size::LIST`].
pub fn list_wide_frame(t: &Theme, head: Div, body: impl IntoElement) -> Div {
    frame(t, size::LIST_WIDE, head, body)
        .max_w(relative(1. / 3.))
        .min_w(px(size::LIST))
}

fn scrolling(id: impl Into<ElementId>, rows: impl IntoElement) -> Stateful<Div> {
    div()
        .id(id)
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .child(rows)
}

fn frame(t: &Theme, width: f32, head: Div, body: impl IntoElement) -> Div {
    div()
        .w(px(width))
        .flex_none()
        .h_full()
        .flex()
        .flex_col()
        .bg(t.surface)
        .border_r_1()
        .border_color(t.divider)
        .child(head)
        .child(
            div()
                .flex_1()
                .min_h_0()
                .py(px(size::ROW_INSET))
                .flex()
                .flex_col()
                .child(body),
        )
}

/// The detail column: its head on top, its body under it scrolling. The
/// body is [`size::READING`] at most, starting at the detail's left; the
/// head spans the detail, so a detail's actions stand at its right edge on
/// every page, however wide the body. `id` names its scroll.
pub fn detail(t: &Theme, id: impl Into<ElementId>, head: Div, body: impl IntoElement) -> Div {
    reading(t, head, div().id(id), body)
}

/// [`detail`] whose body uses the detail's whole width: for what is
/// read as lines that are long by nature (paths, addresses), not prose.
pub fn detail_wide(t: &Theme, id: impl Into<ElementId>, head: Div, body: impl IntoElement) -> Div {
    framed(t, head, div().id(id), body, None)
}

/// [`detail`] whose scroll the page holds, to go back to the top or to a
/// heading.
pub fn detail_scrolled(
    t: &Theme,
    id: impl Into<ElementId>,
    scroll: &ScrollHandle,
    head: Div,
    body: impl IntoElement,
) -> Div {
    reading(t, head, div().id(id).track_scroll(scroll), body)
}

/// [`detail`] with a second column: `body` is what the thing says, kept to
/// a reading measure ([`size::MEASURE`]); `aside` is what it stands among
/// (its relations, its runs, its history). Where the detail has room for
/// both, the aside stands beside the body; where it has not, above or
/// below it as `narrow` says. Wrapping does it, so the layout needs no
/// measure of its own and holds at every width.
pub fn detail_beside(
    t: &Theme,
    id: impl Into<ElementId>,
    head: Div,
    body: impl IntoElement,
    aside: Option<AnyElement>,
    narrow: Narrow,
) -> Div {
    let main = div()
        .flex_grow(1.)
        .flex_shrink(1.)
        .flex_basis(px(size::MEASURE_MIN))
        .min_w_0()
        .max_w(px(size::MEASURE))
        .flex()
        .flex_col()
        .gap(px(size::SECTION_GAP))
        .child(body);
    let pair = div().w_full().flex();
    let pair = match narrow {
        // Reversed, the aside's line stands above the body's when the two
        // do not fit side by side.
        Narrow::Above => pair.flex_wrap_reverse(),
        Narrow::Below => pair.flex_wrap(),
    }
    .gap(px(size::ASIDE_GAP))
    .child(main)
    .children(aside.map(|aside| {
        div()
            .debug_selector(|| "pane-aside".into())
            .flex_grow(1.)
            .flex_shrink(1.)
            .flex_basis(px(size::ASIDE))
            .min_w_0()
            .max_w(px(size::ASIDE_MAX))
            .flex()
            .flex_col()
            .gap(px(size::SECTION_GAP))
            .child(aside)
    }));
    framed(
        t,
        head,
        div().id(id),
        pair,
        Some(size::MEASURE + size::ASIDE_GAP + size::ASIDE_MAX),
    )
}

/// Where a [`detail_beside`]'s aside goes when the detail is too narrow
/// for it to stand beside the body: above, when it says what to read the
/// body by (what an item is part of and waits on); below, when it is
/// history.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Narrow {
    Above,
    Below,
}

fn reading(t: &Theme, head: Div, scroller: Stateful<Div>, body: impl IntoElement) -> Div {
    framed(t, head, scroller, body, Some(size::READING))
}

/// The detail's head across the detail, over its scrolling body, `widest`
/// at most and starting at the detail's left.
fn framed(
    t: &Theme,
    head: Div,
    scroller: Stateful<Div>,
    body: impl IntoElement,
    widest: Option<f32>,
) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .h_full()
        .flex()
        .flex_col()
        .child(
            div()
                .flex_none()
                .w_full()
                .h(px(size::TAB_BAR))
                .border_b_1()
                .border_color(t.divider)
                .child(head.border_0().h_full().w_full().px(px(size::PAD))),
        )
        .child(
            scroller.flex_1().min_h_0().overflow_y_scroll().child(
                div()
                    .w_full()
                    .when_some(widest, |d, w| d.max_w(px(w)))
                    .p(px(size::PAD))
                    .flex()
                    .flex_col()
                    .gap(px(size::SECTION_GAP))
                    .child(body),
            ),
        )
}

/// The detail column around a body that fills it and scrolls itself, such
/// as code or a picture: no reading width, no padding.
pub fn detail_frame(head: Div, body: impl IntoElement) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .h_full()
        .flex()
        .flex_col()
        .child(head)
        .child(div().flex_1().min_h_0().relative().child(body))
}

/// The header line: as high as the tab bar, a rule under it. A list's
/// head comes from [`list_head`]; a detail's holds the thing's [`title`],
/// then [`send`] and its actions.
pub fn head(t: &Theme) -> Div {
    div()
        .flex_none()
        .h(px(size::TAB_BAR))
        .px(px(size::HEAD_PAD))
        .flex()
        .items_center()
        .gap(px(size::HEAD_GAP))
        .border_b_1()
        .border_color(t.divider)
}

/// A list's head, the same on every page: the list's filter on the left,
/// filling, when the list has one, and nothing there when it has not; then
/// the list's actions (at most three [`act`]s) on the right. The tab names
/// the page, so the list's head does not name it again.
pub fn list_head(t: &Theme, filter: Option<AnyElement>) -> Div {
    let left = div().flex_1().min_w_0();
    head(t).child(match filter {
        Some(f) => left.child(f),
        None => left,
    })
}

/// A thing's name in a head: weighted, shortened at its end when it does
/// not fit, the actions after it pushed to the right.
pub fn title(t: &Theme, text: impl Into<SharedString>) -> Div {
    let text: SharedString = text.into();
    let selector = format!("pane-title-{text}");
    div()
        .debug_selector(move || selector)
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .text_size(px(ty::TITLE))
        .font_weight(FontWeight::BOLD)
        .text_color(t.text)
        .child(text)
}

/// An action in a head or on a row: words, quiet until hovered. The one
/// primary action of a page uses `kit::button` with `Kind::Primary`.
pub fn act(t: &Theme, id: impl Into<ElementId>, text: impl Into<SharedString>) -> Stateful<Div> {
    crate::widgets::kit::button(t, id, text, crate::widgets::kit::Kind::Quiet)
}

/// Send: hands `reference` to a session (the focused one, as Send to
/// Session aims) through [`Ask::Send`]. The same words in the same place
/// on every page: last before a head's own actions, or at a row's end.
pub fn send(t: &Theme, id: impl Into<ElementId>, host: &Host, reference: String) -> Stateful<Div> {
    let host = host.clone();
    act(t, id, "Send")
        .tooltip(crate::widgets::kit::tip("Send it to a session"))
        .on_click(move |_, _, cx| {
            let ask = Ask::Send {
                reference: reference.clone(),
                target: None,
            };
            if let Err(e) = host.ask(ask, cx) {
                host.report(Status::failure(format!("Not sent: {}", e.message)), cx);
            }
        })
}

/// A group's label in a list, or a section's over part of a detail: the
/// theme's group label ([`crate::widgets::kit::group_label`]).
pub fn label(t: &Theme, text: impl Into<SharedString>) -> Div {
    // Standing on its foot, the label keeps its own height where a list
    // gives its row a fixed one.
    div()
        .flex_none()
        .flex()
        .items_end()
        .px(px(size::ROW_PAD))
        .pt(px(size::LABEL_TOP))
        .pb(px(size::LABEL_BOTTOM))
        .child(crate::widgets::kit::group_label(t, text))
}

/// A section of a detail: its [`label`] over its parts.
pub fn section(t: &Theme, title: impl Into<SharedString>) -> Div {
    div()
        .w_full()
        .flex()
        .flex_col()
        .child(label(t, title).px_0().pt_0())
}

/// One row of a list: [`size::ROW`] high, inset from the list's sides. The
/// chosen one on a quiet fill with its name in weight; never a strip or a
/// bar. Give it a [`cell`] for the name and [`meta`] for what follows.
pub fn row(t: &Theme, id: impl Into<ElementId>, chosen: bool) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .min_h(px(size::ROW))
        .mx(px(size::ROW_INSET))
        .px(px(size::ROW_PAD - size::ROW_INSET))
        .flex()
        .items_center()
        .gap(px(size::HEAD_GAP))
        .rounded(px(size::RADIUS))
        .cursor_pointer()
        .text_color(if chosen { t.text } else { t.text2 })
        .when(chosen, |d| d.bg(t.raised).font_weight(FontWeight::SEMIBOLD))
        .hover(|d| d.bg(t.accent_soft()))
}

/// A row's or a table cell's words: one line, shortened at the end, taking
/// the room left.
pub fn cell(text: impl Into<SharedString>) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .child(text.into())
}

/// What follows a row's name: small, in `colour` (dim unless it is a state
/// that needs the operator), never shrinking.
pub fn meta(t: &Theme, text: impl Into<SharedString>, colour: Option<Hsla>) -> Div {
    div()
        .flex_none()
        .text_size(px(size::SMALL))
        .text_color(colour.unwrap_or(t.dim))
        .child(text.into())
}

/// A table column: its label and its width, fixed in pixels or sharing
/// what is left with the other filling columns.
#[derive(Clone, Copy)]
pub struct Col {
    pub label: &'static str,
    /// Pixels, or `None` to fill.
    pub width: Option<f32>,
}

impl Col {
    pub const fn fixed(label: &'static str, px: f32) -> Self {
        Self {
            label,
            width: Some(px),
        }
    }
    pub const fn fill(label: &'static str) -> Self {
        Self { label, width: None }
    }
}

fn sized(d: Div, w: Option<f32>) -> Div {
    match w {
        Some(w) => d.flex_none().w(px(w)),
        None => d.flex_1().min_w_0(),
    }
}

/// A table: the column labels on a line of their own, then `rows`, each
/// from [`table_row`]. No frame around it and no inset of its own, so its
/// first column lines up under its section's label; rules between rows.
pub fn table(t: &Theme, cols: &[Col], rows: Vec<AnyElement>) -> Div {
    let mut head = div()
        .flex()
        .items_center()
        .gap(px(size::HEAD_GAP))
        .h(px(size::ROW))
        .border_b_1()
        .border_color(t.divider)
        .text_size(px(size::SMALL))
        .text_color(t.dim);
    for c in cols {
        head =
            head.child(sized(div().overflow_hidden().whitespace_nowrap(), c.width).child(c.label));
    }
    div().w_full().flex().flex_col().child(head).children(rows)
}

/// A table's row: one cell per column, in the table's widths.
pub fn table_row(t: &Theme, cols: &[Col], cells: Vec<AnyElement>) -> Div {
    let mut r = div()
        .flex()
        .items_center()
        .gap(px(size::HEAD_GAP))
        .min_h(px(size::ROW))
        .border_b_1()
        .border_color(t.rule());
    for (c, cell) in cols.iter().zip(cells) {
        r = r.child(sized(div().overflow_hidden(), c.width).child(cell));
    }
    r
}

/// A named line in a detail: the name in its column, the value after it.
/// A value that wraps keeps its name on its first line; the name column
/// gives way to the value in a narrow detail.
pub fn line(t: &Theme, name: impl Into<SharedString>, value: impl IntoElement) -> Div {
    div()
        .flex()
        .items_start()
        .gap(px(size::HEAD_GAP))
        .child(
            div()
                .w(relative(NAME_SHARE))
                .max_w(px(size::NAME_COL))
                .flex_none()
                .min_h(px(size::ROW))
                .flex()
                .items_center()
                .text_color(t.text2)
                // A name longer than its column wraps instead of running
                // under the value (a narrow or zoomed window).
                .child(div().w_full().child(name.into())),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .min_h(px(size::ROW))
                .flex()
                .items_center()
                .child(div().w_full().child(value)),
        )
}

/// The most of a narrow line its name takes.
const NAME_SHARE: f32 = 0.3;

/// An empty list or detail, or one still loading: one short line in the
/// reading face, and the action that fills it if there is one.
pub fn empty(t: &Theme, words: impl Into<SharedString>, action: Option<AnyElement>) -> Div {
    let words: SharedString = words.into();
    let selector = format!("empty-{words}");
    div()
        .debug_selector(move || selector)
        .w_full()
        .p(px(size::PAD))
        .flex()
        .flex_col()
        .items_start()
        .gap(px(size::HEAD_GAP))
        .child(
            div()
                .font_family(t.ty.reading)
                .text_size(px(t.ty.read))
                .text_color(t.text2)
                .child(words),
        )
        .children(action)
}

/// A whole detail with nothing in it (nothing chosen, nothing yet): the
/// theme's raven over its words, in the middle of the room, and the action
/// that fills it. In a room too small for the picture, the words alone.
pub fn vacant(t: &Theme, words: impl Into<SharedString>, action: Option<AnyElement>) -> Div {
    let words: SharedString = words.into();
    let selector = format!("empty-{words}");
    div()
        .debug_selector(move || selector)
        .size_full()
        .min_h(px(art::EMPTY_ROOM))
        .p(px(size::PAD))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(size::SECTION_GAP))
        .child(
            img(t.art.empty.clone())
                .flex_none()
                .size(px(art::EMPTY))
                .opacity(t.art.empty_show),
        )
        .child(
            div()
                .max_w(px(size::NOTICE))
                .text_center()
                .font_family(t.ty.reading)
                .text_size(px(ty::EMPTY))
                .text_color(t.text2)
                .child(words),
        )
        .children(action)
}

/// A name shortened in its middle to `chars`, so both its start and its
/// end (a path's file) still read: `ahjo-own…/src/main.rs`.
pub fn middle(text: &str, chars: usize) -> String {
    let n = text.chars().count();
    if n <= chars || chars < 3 {
        return text.to_string();
    }
    let keep = chars - 1;
    let head = keep / 2;
    let tail = keep - head;
    let start: String = text.chars().take(head).collect();
    let end: String = text.chars().skip(n - tail).collect();
    format!("{start}…{end}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_thumb_shows_the_share_shown_and_where() {
        let view = Bounds::new(point(px(0.), px(100.)), gpui::size(px(200.), px(400.)));
        assert_eq!(
            thumb(view, px(0.), px(0.)),
            None,
            "a list that fits has none"
        );
        let top = thumb(view, px(400.), px(0.)).unwrap();
        assert_eq!((top.top(), top.size.height), (px(100.), px(200.)));
        let end = thumb(view, px(400.), px(-400.)).unwrap();
        assert_eq!(end.bottom(), px(500.));
        let long = thumb(view, px(40_000.), px(-20_000.)).unwrap();
        assert_eq!(
            long.size.height,
            px(SCROLLBAR_LEAST),
            "a long list's thumb stays findable"
        );
    }

    use super::middle;

    #[test]
    fn a_long_name_keeps_its_start_and_end() {
        assert_eq!(middle("short", 10), "short");
        assert_eq!(middle("abcdefghij", 7), "abc…hij");
        assert_eq!(
            middle("/home/me/project/src/main.rs", 15).chars().count(),
            15
        );
        assert!(middle("/home/me/project/src/main.rs", 15).ends_with("main.rs"));
    }
}
