//! A scroll area scrolls to its end wherever it stands: here in a flex
//! row, holding one tall page, the shape in which Desk's document once
//! measured nothing past the window and the wheel did nothing.

use ahjo_app_kit::theme::Theme;
use ahjo_app_kit::widgets::pane;
use gpui::{
    Bounds, Modifiers, Pixels, ScrollDelta, ScrollEvent, TestAppContext, VisualTestContext, Window,
    div, point, prelude::*, px, size,
};

const LINES: usize = 60;

/// A side column beside a scroll area holding one page of many lines.
struct Room;

impl Render for Room {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .child(div().w(px(120.)).h_full())
            .child(
                pane::scroll("doc")
                    .debug_selector(|| "doc".into())
                    .flex_1()
                    .h_full()
                    .child(div().w_full().children((0..LINES).map(|i| {
                        div()
                            .debug_selector(move || format!("line-{i}"))
                            .h(px(40.))
                            .child(format!("Line {i}"))
                    }))),
            )
    }
}

fn shown(cx: &mut VisualTestContext, selector: &'static str) -> Bounds<Pixels> {
    cx.run_until_parked();
    cx.debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is not drawn"))
}

#[gpui::test]
fn a_scroll_area_in_a_row_scrolls_to_its_end(cx: &mut TestAppContext) {
    cx.update(|cx| cx.set_global(Theme::default()));
    let (_, cx) = cx.add_window_view(|_, _| Room);
    cx.simulate_resize(size(px(800.), px(600.)));
    let room = shown(cx, "doc");
    assert!(room.size.height <= px(600.), "{room:?} keeps to the window");
    let last = "line-59";
    assert!(
        shown(cx, last).top() > room.bottom(),
        "the end starts below"
    );

    for _ in 0..10 {
        cx.simulate_event(ScrollEvent {
            position: room.center(),
            delta: ScrollDelta::Pixels(point(px(0.), px(-400.))),
            modifiers: Modifiers::default(),
            ..Default::default()
        });
    }
    let end = shown(cx, last);
    assert!(
        end.bottom() <= room.bottom() + px(0.5) && end.top() >= room.top(),
        "the last line {end:?} is in view in {room:?}"
    );
}

/// A padded scroll area holding less than its height.
struct Short {
    scroll: gpui::ScrollHandle,
}

impl Render for Short {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().flex().flex_col().child(
            pane::scroll("short")
                .flex_1()
                .p(px(24.))
                .track_scroll(&self.scroll)
                .child(div().h(px(40.)).child("One line")),
        )
    }
}

#[gpui::test]
fn a_padded_area_that_fits_does_not_scroll(cx: &mut TestAppContext) {
    cx.update(|cx| cx.set_global(Theme::default()));
    let (short, cx) = cx.add_window_view(|_, _| Short {
        scroll: gpui::ScrollHandle::new(),
    });
    cx.simulate_resize(size(px(800.), px(600.)));
    cx.run_until_parked();
    let most = short.read_with(cx, |s, _| s.scroll.max_offset());
    assert_eq!(most, point(px(0.), px(0.)), "nothing to scroll, no thumb");
}
