//! What opens over the page, each a layer or a popup on the app's one
//! stack: the app draws it, closes it on Escape or a click outside, and
//! gives it the keyboard; the stack says what is open, the page keeps what
//! it is about and draws its content. Send and the output search are
//! content of the window's panel instead, beside the page, drawn by the
//! same view.

use super::page::Terminals;
use crate::store::sessions::Key;
use ahjo_app_kit::popup::PopupId;
use gpui::{
    AnyElement, App, AppContext, Context, Entity, Focusable, IntoElement, Render, Subscription,
    WeakEntity, Window, div,
};

pub const SEND: &str = "send";
pub const OVERVIEW: &str = "overview";
pub const FIND: &str = "find";
pub const PEEK: &str = "peek";
pub const RENAME_TAB: &str = "rename-tab";
pub const RENAME_SESSION: &str = "rename-session";

/// A layer to open over the page, and what it is about.
#[derive(Clone, Debug)]
pub enum Surface {
    Overview,
    Peek,
    /// Its field is drawn in the tab itself.
    RenameTab(usize),
    /// Its field is drawn where the session's name was right-clicked.
    RenameSession(Key, NameAt),
}

impl Surface {
    pub fn name(&self) -> &'static str {
        match self {
            Surface::Overview => OVERVIEW,
            Surface::Peek => PEEK,
            Surface::RenameTab(_) => RENAME_TAB,
            Surface::RenameSession(..) => RENAME_SESSION,
        }
    }
}

/// What the page's one name field names while it is open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Naming {
    /// A tab, in the tab itself.
    Tab(usize),
    /// A session, where its name was right-clicked.
    Session(Key, NameAt),
    /// The view kept on a number key, in a popup beside its menu: keeping
    /// the arrangement on screen there, or only renaming it.
    View {
        popup: PopupId,
        key: u8,
        rename: bool,
    },
}

/// Where a session's name is renamed: the place its menu was opened from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameAt {
    /// Its row in the rail.
    Rail,
    /// Its tag in window `n` of the shown tab.
    Window(usize),
}

/// The layer's view, or the panel's content: it draws the page's surface
/// `name`, again whenever the page changes.
pub struct View {
    page: WeakEntity<Terminals>,
    name: &'static str,
    _page: Option<Subscription>,
}

pub fn view(page: WeakEntity<Terminals>, name: &'static str, cx: &mut App) -> Entity<View> {
    cx.new(|cx| {
        let watch = page
            .upgrade()
            .map(|p| cx.observe(&p, |_, _, cx| cx.notify()));
        View {
            page,
            name,
            _page: watch,
        }
    })
}

impl ahjo_app_kit::panel::PanelView for View {
    fn head(&mut self, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let name = self.name;
        self.page
            .update(cx, |p, cx| p.render_panel_head(name, cx))
            .unwrap_or_else(|_| div().into_any_element())
    }
    fn focus(&self, cx: &App) -> Option<gpui::FocusHandle> {
        let page = self.page.upgrade()?.read(cx);
        Some(match self.name {
            FIND => page.find_query.focus_handle(cx),
            _ if page.send_targets.is_empty() && !page.send.new_agent => {
                page.send_finder.read(cx).query.focus_handle(cx)
            }
            _ => page.send_note.focus_handle(cx),
        })
    }
}

impl Render for View {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let name = self.name;
        self.page
            .update(cx, |p, cx| p.render_surface(name, window, cx))
            .ok()
            .flatten()
            .unwrap_or_else(|| div().into_any_element())
    }
}

impl Terminals {
    /// The content of surface `name`, while it is open; Send's or the
    /// search's in the panel.
    fn render_surface(
        &mut self,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        match name {
            SEND => return Some(self.render_send(cx)),
            FIND => return Some(self.render_find(cx)),
            _ => {}
        }
        if !self.host.is_open(name, cx) {
            return None;
        }
        match name {
            OVERVIEW => Some(self.render_overview(window, cx)),
            PEEK => self.render_peek(window, cx),
            _ => None,
        }
    }

    /// What Send or the search says in the panel's header.
    fn render_panel_head(&mut self, name: &str, cx: &mut Context<Self>) -> AnyElement {
        let t = crate::theme::theme(cx).clone();
        let title = match name {
            FIND => "Search output",
            _ => "Send to sessions",
        };
        ahjo_app_kit::widgets::pane::title(&t, title).into_any_element()
    }
}
