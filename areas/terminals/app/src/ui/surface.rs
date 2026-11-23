//! What opens over the page, each a layer on the app's one stack: the app
//! draws it, closes it on Escape or a click outside, and gives it the
//! keyboard (Send, which lies beside the windows, only until a terminal is
//! clicked); the page keeps its state and draws its content.

use super::page::Terminals;
use crate::store::sessions::Key;
use ahjo_app_kit::host::Cover;
use gpui::{
    AnyElement, App, AppContext, Context, Entity, IntoElement, Pixels, Point, Render, Subscription,
    WeakEntity, Window, div,
};

/// Where the drawers lie: Send, the output search and a view beside the
/// windows. The one rule: at the right edge of the terminals, full height,
/// because a terminal's lines and its prompt start at its left. Send is
/// the exception that proves it: one writes to sessions with it while
/// reading them, so the windows make room for it instead
/// (`Terminals::send_room`), and it lies right of them. The search and a
/// view beside lie over the windows, so no session changes size for a
/// glance; the search lies `inset` in from the right, at the left edge of
/// a view beside or of Send, so they stay in sight
/// ([`Terminals::beside_inset`]).
///
/// What lies over the windows takes every pointer event inside it: a click,
/// a wheel or a hover there never reaches a window under it, so a click in
/// a panel can neither type into nor focus a session the operator cannot see.
pub fn at_edge<E: gpui::Styled + gpui::InteractiveElement>(e: E, inset: f32) -> E {
    e.absolute().top_0().right(gpui::px(inset)).occlude()
}

pub const SEND: &str = "send";
pub const OVERVIEW: &str = "overview";
pub const FIND: &str = "find";
pub const PEEK: &str = "peek";
pub const RENAME_TAB: &str = "rename-tab";
pub const RENAME_SESSION: &str = "rename-session";
pub const VIEW_NAME: &str = "view-name";

/// A surface over the page, and what it is about.
#[derive(Clone, Debug)]
pub enum Surface {
    Send,
    Overview,
    Find,
    Peek,
    /// Its field is drawn in the tab itself.
    RenameTab(usize),
    /// Its field is drawn where the session's name was right-clicked.
    RenameSession(Key, NameAt),
    /// Naming the view kept on a number key, where the menu was: keeping
    /// the arrangement on screen there, or only renaming it.
    ViewName(Point<Pixels>, u8, bool),
}

impl Surface {
    pub fn name(&self) -> &'static str {
        match self {
            Surface::Send => SEND,
            Surface::Overview => OVERVIEW,
            Surface::Find => FIND,
            Surface::Peek => PEEK,
            Surface::RenameTab(_) => RENAME_TAB,
            Surface::RenameSession(..) => RENAME_SESSION,
            Surface::ViewName(..) => VIEW_NAME,
        }
    }

    /// The drawers and Peek cover the page; naming a view the window.
    pub fn cover(&self) -> Cover {
        match self {
            Surface::ViewName(..) => Cover::Window,
            _ => Cover::Page,
        }
    }
}

/// Where a session's name is renamed: the place its menu was opened from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameAt {
    /// Its row in the rail.
    Rail,
    /// Its tag in window `n` of the shown tab.
    Window(usize),
}

/// The layer's view: it draws the page's surface `name`, again whenever the
/// page changes.
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
    /// The content of surface `name`, while it is open.
    fn render_surface(
        &mut self,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let s = self.surfaces.iter().find(|s| s.name() == name)?.clone();
        match s {
            Surface::Send => Some(self.render_send(window, cx)),
            Surface::Overview => Some(self.render_overview(window, cx)),
            Surface::Find => Some(self.render_find(window, cx)),
            Surface::Peek => self.render_peek(window, cx),
            Surface::RenameTab(_) | Surface::RenameSession(..) => None,
            Surface::ViewName(at, key, rename) => {
                Some(self.render_view_name(at, key, rename, window, cx))
            }
        }
    }
}
