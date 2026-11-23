//! A small form opened from a thing stands beside that thing: under the
//! button it came from, aligned to its nearer edge, above it when there is
//! no room below, and kept inside the window. The place says what it acts
//! on, and it covers the thing's neighbours, not the work. Only a decision
//! about the whole window sits centred; a rename edits the name in place.

use crate::pointer;
use crate::theme::size;
use gpui::{
    Anchor, AnyElement, App, Bounds, MouseButton, Pixels, Point, Window, anchored, deferred, div,
    point, prelude::*, px,
};

/// What a popover stands beside: the button pressed, or the point clicked
/// (a right click, a menu's row).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spot(pub Bounds<Pixels>);

impl Spot {
    /// The button pressed at `at`, else the point itself.
    pub fn at(at: Point<Pixels>, cx: &App) -> Self {
        match pointer::pressed_at(at, cx).and_then(|p| p.bounds) {
            Some(bounds) => Spot(bounds),
            None => Spot(Bounds::new(at, gpui::size(px(0.), px(0.)))),
        }
    }

    /// Where the pointer is now: the button under it, else the point.
    pub fn here(window: &Window, cx: &App) -> Self {
        Self::at(window.mouse_position(), cx)
    }
}

/// `content` beside `spot`, over everything on the page. A press outside
/// it runs `dismiss` and reaches nothing under it.
pub fn popover(
    spot: Spot,
    window: &Window,
    dismiss: impl Fn(&mut Window, &mut App) + 'static,
    content: impl IntoElement,
) -> AnyElement {
    let target = spot.0;
    // In the window's right half it hangs from the thing's right edge, so
    // it grows into the window, not out of it.
    let (corner, anchor) = if target.center().x > window.viewport_size().width / 2. {
        (Anchor::BottomRight, Anchor::TopRight)
    } else {
        (Anchor::BottomLeft, Anchor::TopLeft)
    };
    deferred(
        div()
            .absolute()
            .inset_0()
            .occlude()
            .on_mouse_down(MouseButton::Left, move |_, window, cx| dismiss(window, cx))
            .child(
                anchored()
                    .attach(target, corner)
                    .anchor(anchor)
                    .offset(point(px(0.), px(size::POPOVER_GAP)))
                    .snap_to_window_with_margin(px(size::POPOVER_EDGE))
                    .child(
                        div()
                            .occlude()
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .child(content),
                    ),
            ),
    )
    .with_priority(10)
    .into_any_element()
}
