//! The open card's panel: what the page has open, lent to the window's
//! one edge panel, which squeezes the board beside it.
use super::MissionsPage;
use gpui::{Context, KeyDownEvent, Render, Subscription, WeakEntity, Window, div, prelude::*};

pub(crate) struct CardPanel {
    page: WeakEntity<MissionsPage>,
    _page_changed: Subscription,
}

impl CardPanel {
    pub(crate) fn new(page: &gpui::Entity<MissionsPage>, cx: &mut Context<Self>) -> Self {
        Self {
            page: page.downgrade(),
            _page_changed: cx.observe(page, |_, _, cx| cx.notify()),
        }
    }
}

impl Render for CardPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let column = self
            .page
            .update(cx, |page, cx| page.editor_column(cx))
            .ok()
            .flatten();
        div()
            .id("card-panel")
            .debug_selector(|| "card-panel".into())
            .size_full()
            .on_key_down(cx.listener(|panel, e: &KeyDownEvent, window, cx| {
                if e.keystroke.key == "escape" {
                    let _ = panel
                        .page
                        .update(cx, |page, cx| page.close_editor(window, cx));
                    cx.stop_propagation();
                }
            }))
            .children(column)
    }
}
