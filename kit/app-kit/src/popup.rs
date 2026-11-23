//! A popup: a menu, a confirm or a small form opened from a thing, on the
//! one dismissal stack ([`crate::layers`]). The shell owns all it does
//! besides its content: it stands beside what was clicked ([`beside`]),
//! kept on screen; Escape closes the top one; a press outside closes it or
//! not, as it declares ([`Outside`]); and when it closes with the keyboard
//! in it, the keyboard goes back to where it was when it opened. A page
//! opens one with [`crate::Host::popup`] and is told it closed through
//! [`Popup::on_dismiss`].

use crate::chords::Run;
use crate::menu::Menu;
use crate::pointer;
use crate::theme::{Theme, size};
use crate::widgets::{kit, pane};
use gpui::{
    Anchor, Anchored, AnyElement, AnyView, App, AppContext, Bounds, Context, Div, ElementId,
    Entity, FocusHandle, IntoElement, Pixels, Point, Render, Size, Subscription, WeakEntity,
    WeakFocusHandle, Window, anchored, div, point, prelude::*, px,
};
use std::rc::Rc;

/// What a popup stands beside: the button pressed, or the point clicked
/// (a right click, a key's place).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spot(pub Bounds<Pixels>);

impl Spot {
    /// The button pressed at `at`, else the point itself.
    pub fn at(at: Point<Pixels>, cx: &App) -> Self {
        match pointer::pressed_at(at, cx) {
            Some(bounds) => Spot(bounds),
            None => Self::point(at),
        }
    }

    /// Where the pointer is now: the button under it, else the point.
    pub fn here(window: &Window, cx: &App) -> Self {
        Self::at(window.mouse_position(), cx)
    }

    /// The point itself, whatever was pressed there.
    pub fn point(at: Point<Pixels>) -> Self {
        Spot(Bounds::new(at, gpui::size(px(0.), px(0.))))
    }
}

/// What a press outside an open popup does. Either way it reaches nothing
/// under the popup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outside {
    /// Closes it, as Escape does: a menu, a confirm, a form with nothing
    /// typed that matters.
    Close,
    /// Leaves it open, for a form holding what was typed; Escape or its
    /// own Cancel closes it.
    Keep,
}

/// An open popup's handle, for its page to close it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PopupId(pub(crate) u64);

impl PopupId {
    /// The same popup's id for one of its elements, as `name`.
    pub fn element(self, name: &'static str) -> gpui::ElementId {
        gpui::ElementId::NamedInteger(name.into(), self.0)
    }
}

/// What a popup shows.
#[derive(Clone)]
pub enum Content {
    /// A page's view.
    View(AnyView),
    /// Rows the shell draws and steps through with the keys.
    Menu(Menu),
}

/// A popup on the stack: where it stands, what it shows, what has the
/// keyboard while it is open and what closing it does.
#[derive(Clone)]
pub struct Popup {
    pub spot: Spot,
    pub content: Content,
    pub focus: Option<FocusHandle>,
    pub outside: Outside,
    pub dismiss: Option<Run>,
    may_close: Option<MayClose>,
    /// Its name on the stack, `<page>/popup/<n>`; set as it opens.
    pub(crate) name: String,
    pub(crate) id: PopupId,
    /// What had the keyboard as it opened; set by the shell.
    pub back: Option<WeakFocusHandle>,
}

impl Popup {
    /// `view` beside `spot`, closed by a press outside it.
    pub fn beside(spot: Spot, view: impl Into<AnyView>) -> Self {
        Self::new(spot, Content::View(view.into()))
    }

    /// `menu`'s rows beside `spot`. A menu passes: anything else opening
    /// closes it.
    pub fn menu(spot: Spot, menu: Menu) -> Self {
        Self::new(spot, Content::Menu(menu))
    }

    fn new(spot: Spot, content: Content) -> Self {
        Self {
            spot,
            content,
            focus: None,
            outside: Outside::Close,
            dismiss: None,
            may_close: None,
            name: String::new(),
            id: PopupId(0),
            back: None,
        }
    }

    /// The keyboard is in `focus` while the popup is open.
    pub fn focus(mut self, focus: FocusHandle) -> Self {
        self.focus = Some(focus);
        self
    }

    pub fn outside(mut self, outside: Outside) -> Self {
        self.outside = outside;
        self
    }

    /// What closing it does, by Escape, a press outside or
    /// [`crate::Host::close_popup`].
    pub fn on_dismiss(mut self, run: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.dismiss = Some(Rc::new(run));
        self
    }

    /// Asked before Escape or a press outside closes it: false keeps it
    /// open, as a form does while what it sent has not answered (the
    /// form says why). Its page's own [`crate::Host::close_popup`] is not
    /// asked.
    pub fn may_close(mut self, ask: impl Fn(&mut Window, &mut App) -> bool + 'static) -> Self {
        self.may_close = Some(Rc::new(ask));
        self
    }

    /// Whether Escape or a press outside may close it now.
    pub fn lets_close(&self, window: &mut Window, cx: &mut App) -> bool {
        self.may_close.as_ref().is_none_or(|ask| ask(window, cx))
    }

    pub fn id(&self) -> PopupId {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn is_menu(&self) -> bool {
        matches!(self.content, Content::Menu(_))
    }

    /// The keyboard is in it.
    pub fn has_keyboard(&self, window: &Window, cx: &App) -> bool {
        self.focus
            .as_ref()
            .is_some_and(|f| f.contains_focused(window, cx))
    }
}

/// The one placement of every popup and menu: under `spot`, hung from its
/// left edge in the window's left half and from its right edge in the
/// right half, so it grows into the window; above it when there is no room
/// below; and kept inside the window by [`size::POPUP_EDGE`].
pub fn beside(spot: Spot, viewport: Size<Pixels>) -> Anchored {
    let target = spot.0;
    let (corner, anchor) = if target.center().x > viewport.width / 2. {
        (Anchor::BottomRight, Anchor::TopRight)
    } else {
        (Anchor::BottomLeft, Anchor::TopLeft)
    };
    anchored()
        .attach(target, corner)
        .anchor(anchor)
        .offset(point(px(0.), px(size::POPUP_GAP)))
        .snap_to_window_with_margin(px(size::POPUP_EDGE))
}

/// A small form in a popup: its `fields` scroll when the window is too
/// short for them, and its `actions` (Cancel and the form's verb) stand
/// under them, always in view. Both come styled by the page (padding,
/// gaps); `id` names the fields' scroll.
pub fn form(
    t: &Theme,
    id: impl Into<ElementId>,
    fields: impl IntoElement,
    actions: impl IntoElement,
) -> Div {
    kit::panel(t)
        .min_h_0()
        .flex()
        .flex_col()
        .child(pane::scroll(id).child(fields))
        .child(div().flex_none().child(actions))
}

type MayClose = Rc<dyn Fn(&mut Window, &mut App) -> bool>;

type Draw<V> = Rc<dyn Fn(&mut V, &mut Window, &mut Context<V>) -> Option<AnyElement>>;

/// A popup's view drawn by its page: `draw` on the page, again whenever
/// the page changes, so a page keeps a form's state and draws it as it
/// drew it in place.
pub struct Drawn<V: 'static> {
    page: WeakEntity<V>,
    draw: Draw<V>,
    _page: Subscription,
}

/// A view drawing `draw` on `page`, for [`Popup::beside`].
pub fn drawn<V: 'static>(
    page: &Entity<V>,
    draw: impl Fn(&mut V, &mut Window, &mut Context<V>) -> Option<AnyElement> + 'static,
    cx: &mut App,
) -> Entity<Drawn<V>> {
    cx.new(|cx| Drawn {
        page: page.downgrade(),
        draw: Rc::new(draw),
        _page: cx.observe(page, |_, _, cx| cx.notify()),
    })
}

impl<V: 'static> Render for Drawn<V> {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let draw = self.draw.clone();
        // It gives way to the window the popup is kept inside: a
        // [`form`] keeps its actions in view, and anything else that is
        // taller scrolls.
        pane::scroll("popup-drawn").children(
            self.page
                .update(cx, |page, cx| draw(page, window, cx))
                .ok()
                .flatten(),
        )
    }
}
