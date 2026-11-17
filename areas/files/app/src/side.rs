//! The Files section of the side panel, on the other pages: the page's own
//! tree, drawn by the page, so its folders open, its change marks and its
//! pick are the same in both places. It starts at the folder of the session
//! in the focused terminal window while that session is on the tree's
//! device, else at the roots. A folder opens in place; a file opens on the
//! Files page. On the Files page the list holds the tree and this gives way.

use crate::FilesPage;
use gpui::{Context, Entity, Subscription, WeakEntity, Window, div, prelude::*};

pub struct FilesSide {
    page: WeakEntity<FilesPage>,
    _page_changed: Subscription,
}

impl FilesSide {
    pub fn new(page: &Entity<FilesPage>, cx: &mut Context<Self>) -> Self {
        Self {
            page: page.downgrade(),
            _page_changed: cx.observe(page, |_, _, cx| cx.notify()),
        }
    }
}

impl Render for FilesSide {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.page
            .update(cx, |p, cx| p.render_side(cx))
            .unwrap_or_else(|_| div().into_any_element())
    }
}
