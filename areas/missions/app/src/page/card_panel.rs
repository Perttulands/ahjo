//! The open card in the window's panel, on the Missions page only: the
//! panel squeezes the board beside it, and its header carries the card's
//! head. Close and Escape put it away once its draft is saved.
use super::MissionsPage;
use gpui::{AnyElement, Context, Render, Subscription, WeakEntity, Window, div, prelude::*};

/// The open card's content in the window's panel.
pub(crate) const CARD: &str = "card";

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

impl ahjo_app_kit::panel::PanelView for CardPanel {
    fn head(&mut self, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        self.page
            .update(cx, |page, cx| page.editor_head(cx))
            .ok()
            .flatten()
            .unwrap_or_else(|| div().into_any_element())
    }
    fn may_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.page
            .update(cx, |page, cx| {
                page.close_editor(window, cx);
                !page.is_card_panel_open()
            })
            .unwrap_or(true)
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
            .children(column)
    }
}
