//! The window's one panel: what a page opens beside the page shown
//! (a file, an issue, the brain, Send, a search) stands in it, one at a
//! time, at the window's edge. The shell owns it: it makes room for it, so
//! the page beside squeezes and nothing is covered; it draws its one
//! header (Back where it applies, the content's head, Close) and its edge,
//! dragged to size it and kept. Close, Escape with the keyboard in it,
//! Ctrl+B and the content's own key put it away; a click outside it takes
//! the keyboard away and leaves it open. What opened over another goes
//! back to it with Back, or when its page closes it.

use crate::chords::Run;
use crate::hub::SideOn;
use crate::theme::{Theme, motion};
use crate::widgets::kit::{self, GripEdge, Phase};
use crate::widgets::pane;
use gpui::{
    AnyElement, AnyView, App, Context, Entity, FocusHandle, KeyDownEvent, Render, Window, div,
    ease_out_quint, prelude::*, px,
};
use std::rc::Rc;
use std::time::{Duration, Instant};

type Head = Rc<dyn Fn(&mut Window, &mut App) -> AnyElement>;
type MayClose = Rc<dyn Fn(&mut Window, &mut App) -> bool>;
type Focus = Rc<dyn Fn(&App) -> Option<FocusHandle>>;

/// What a page shows in the panel.
pub trait PanelView: Render {
    /// What the header says between Back and Close: the content's title
    /// ([`crate::widgets::pane::title`]), and acts on what it shows.
    fn head(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement;

    /// Whether Back takes the content itself a step back now (from a file
    /// to the list it was opened from).
    fn can_go_back(&self, _: &App) -> bool {
        false
    }

    /// Back, when [`PanelView::can_go_back`].
    fn go_back(&mut self, _: &mut Window, _: &mut Context<Self>) {}

    /// The panel is to be put away (its Close, Escape, Ctrl+B): false
    /// keeps it, as unsaved edits do while the content asks about them.
    fn may_close(&mut self, _: &mut Window, _: &mut Context<Self>) -> bool {
        true
    }

    /// Where the keyboard goes when this content comes back into view
    /// with the keyboard in the panel (what opened over it closed).
    fn focus(&self, _: &App) -> Option<FocusHandle> {
        None
    }
}

/// A page's content for the panel, as the shell holds it.
#[derive(Clone)]
pub struct Panel {
    /// `<page>/<name>`: one of a page's contents (Issues has the list and
    /// an item's card).
    pub name: String,
    /// The page that opened it.
    pub page: &'static str,
    pub view: AnyView,
    /// Where it shows: on every page; on every page but its own, which
    /// shows the same (Files); or only on its own, being about what that
    /// page has open (a mission's card).
    pub on: SideOn,
    pub(crate) head: Head,
    pub(crate) can_go_back: Rc<dyn Fn(&App) -> bool>,
    pub(crate) go_back: Run,
    pub(crate) may_close: MayClose,
    pub(crate) focus: Focus,
}

impl Panel {
    /// `view` as content named `name` (unique within its page). Its page
    /// is set by [`crate::Host::show_panel`].
    pub fn new<T: PanelView>(name: &str, view: Entity<T>) -> Self {
        let (head, back, go, close, focus) = (
            view.downgrade(),
            view.downgrade(),
            view.downgrade(),
            view.downgrade(),
            view.downgrade(),
        );
        Self {
            name: name.to_string(),
            page: "",
            view: view.into(),
            on: SideOn::EveryPage,
            head: Rc::new(move |window, cx| {
                head.update(cx, |v, cx| v.head(window, cx))
                    .unwrap_or_else(|_| gpui::IntoElement::into_any_element(gpui::Empty))
            }),
            can_go_back: Rc::new(move |cx| {
                back.upgrade().is_some_and(|v| v.read(cx).can_go_back(cx))
            }),
            go_back: Rc::new(move |window, cx| {
                let _ = go.update(cx, |v, cx| v.go_back(window, cx));
            }),
            may_close: Rc::new(move |window, cx| {
                close
                    .update(cx, |v, cx| v.may_close(window, cx))
                    .unwrap_or(true)
            }),
            focus: Rc::new(move |cx| focus.upgrade().and_then(|v| v.read(cx).focus(cx))),
        }
    }

    /// Shown on every page but its own page.
    pub fn off_its_page(mut self) -> Self {
        self.on = SideOn::OtherPages;
        self
    }

    /// The header between Back and Close.
    pub fn head(&self, window: &mut Window, cx: &mut App) -> AnyElement {
        (self.head)(window, cx)
    }

    pub fn can_go_back(&self, cx: &App) -> bool {
        (self.can_go_back)(cx)
    }

    pub fn go_back(&self, window: &mut Window, cx: &mut App) {
        (self.go_back)(window, cx)
    }

    pub fn may_close(&self, window: &mut Window, cx: &mut App) -> bool {
        (self.may_close)(window, cx)
    }

    pub fn focus(&self, cx: &App) -> Option<FocusHandle> {
        (self.focus)(cx)
    }

    /// Shown only on its own page.
    pub fn on_its_page(mut self) -> Self {
        self.on = SideOn::OwnPage;
        self
    }

    /// Whether it stands while page `shown` shows.
    pub fn shows_on(&self, shown: &str) -> bool {
        self.on.stands(self.page, shown)
    }
}

/// How a column at the window's edge slides as it opens and goes: it
/// remembers whether it was drawn open and when that last changed.
#[derive(Default)]
pub struct Slide {
    drawn: Option<bool>,
    moved: Option<Instant>,
}

impl Slide {
    /// The share of its width the column shows this frame, while `open`:
    /// all of it once it has slid in, none once it has slid out (`None`:
    /// nothing to draw). A change of `open` since the last frame starts a
    /// slide, which follows the clock on the wall a frame at a time.
    pub fn share(&mut self, open: bool, window: &mut Window) -> Option<f32> {
        let slide = Duration::from_millis(motion::PANEL_SLIDE);
        if self.drawn != Some(open) {
            if self.drawn.is_some() {
                self.moved = Some(Instant::now());
            }
            self.drawn = Some(open);
        }
        let Some(at) = self.moved.filter(|at| at.elapsed() < slide) else {
            return open.then_some(1.);
        };
        // Drawn again each frame until the slide ends, so a column put
        // away is gone once it has slid out.
        window.request_animation_frame();
        let x = ease_out_quint()(at.elapsed().as_secs_f32() / slide.as_secs_f32());
        Some(if open { x } else { 1. - x })
    }
}

/// What a press on one of its controls does.
pub type Act = Rc<dyn Fn(&mut Window, &mut App)>;
type Sized = Rc<dyn Fn(f32, bool, &mut Window, &mut App)>;

/// A column at the window's edge, as the kit draws it everywhere: Swarm's
/// panel, and a standalone app's own column (Desk's list of asks). One
/// header (Back where it applies, the content's head, Close), the content
/// under it, sized by the grip on its inner edge ([`kit::grip`]), sliding
/// in and out ([`Slide`]). Escape with the keyboard in it closes it. It
/// takes the place it is put in a row; the page beside gives way.
pub struct EdgeColumn {
    /// Names its elements: `panel` is Swarm's.
    pub id: &'static str,
    /// At the window's left edge, its grip on its right.
    pub left: bool,
    pub width: f32,
    /// The least and most it is dragged to.
    pub range: (f32, f32),
    /// The width a double click on its grip puts back.
    pub first_width: f32,
    pub head: AnyElement,
    pub content: AnyElement,
    /// What has the keyboard while it is within.
    pub focus: FocusHandle,
    /// Back, when it applies.
    pub back: Option<Act>,
    pub close: Act,
    /// The keys that close it, in Close's tip.
    pub close_keys: &'static str,
    /// The width dragged, and whether to keep it (the grip let go).
    pub sized: Sized,
}

impl EdgeColumn {
    /// The column, `share` of it shown as it slides ([`Slide::share`]).
    pub fn render(self, t: &Theme, share: f32, window: &mut Window, cx: &mut App) -> AnyElement {
        let Self {
            id,
            left,
            width,
            range,
            first_width,
            head,
            content,
            focus,
            back,
            close,
            close_keys,
            sized,
        } = self;
        let on_escape = close.clone();
        let column = div()
            .id(format!("{id}-content"))
            .track_focus(&focus)
            .key_context("Panel")
            .size_full()
            .flex()
            .flex_col()
            .bg(t.surface)
            .when(left, |d| d.border_r_1())
            .when(!left, |d| d.border_l_1())
            .border_color(t.divider)
            .on_key_down(move |e: &KeyDownEvent, window, cx| {
                if e.keystroke.key == "escape" && !e.keystroke.modifiers.modified() {
                    cx.stop_propagation();
                    on_escape(window, cx);
                }
            })
            .child(
                pane::head(t)
                    .when_some(back, |d, back| {
                        let selector = format!("{id}-back");
                        d.child(
                            pane::act(t, format!("{id}-back"), "Back")
                                .debug_selector(move || selector)
                                .on_click(move |_, window, cx| back(window, cx)),
                        )
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .flex()
                            .items_center()
                            .gap(px(crate::theme::size::HEAD_GAP))
                            .child(head),
                    )
                    .child({
                        let selector = format!("{id}-close");
                        pane::act(t, format!("{id}-close"), "Close")
                            .debug_selector(move || selector)
                            .tooltip(kit::tip(close_keys))
                            .on_click(move |_, window, cx| close(window, cx))
                    }),
            )
            .child(div().flex_1().min_h_0().flex().flex_col().child(content));
        // The grip lies outside the frame, which clips its column as it
        // slides, and is drawn over the page beside.
        let grip = kit::grip(
            t,
            format!("{id}-grip"),
            if left {
                GripEdge::Right
            } else {
                GripEdge::Left
            },
            width,
            range,
            window,
            cx,
            move |width, phase, window, cx| {
                let (width, keep) = match phase {
                    Phase::Moving => (width, false),
                    Phase::Kept => (width, true),
                    Phase::Reset => (first_width, true),
                };
                sized(width, keep, window, cx);
            },
        );
        let frame = div()
            .id((gpui::SharedString::from(id), left as usize))
            .debug_selector(move || format!("{id}-{}", if left { "left" } else { "right" }))
            .flex_none()
            .h_full()
            .relative()
            .overflow_hidden()
            .w(px(width * share))
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .w(px(width))
                    .when(left, |d| d.right_0())
                    .when(!left, |d| d.left_0())
                    .child(column),
            );
        div()
            .flex_none()
            .h_full()
            .relative()
            .child(frame)
            .child(grip)
            .into_any_element()
    }
}

/// What the stack holds: content with a name and a page.
pub trait Named {
    fn name(&self) -> &str;
    fn page(&self) -> &str;
}

impl Named for Panel {
    fn name(&self) -> &str {
        &self.name
    }
    fn page(&self) -> &str {
        self.page
    }
}

/// The panel's contents, the newest on top, and whether it is open. Only
/// the top shows; Back or its page closing it brings the one under it.
#[derive(Clone)]
pub struct Stack<T = Panel> {
    contents: Vec<T>,
    pub open: bool,
}

impl<T> Default for Stack<T> {
    fn default() -> Self {
        Self {
            contents: Vec::new(),
            open: false,
        }
    }
}

impl<T: Named> Stack<T> {
    /// The content showing, open or put away.
    pub fn top(&self) -> Option<&T> {
        self.contents.last()
    }

    /// Content named as `panel` is goes on top, out of where it was, and
    /// the panel opens. Back leads only to what showed when it opened: put
    /// away, the panel starts over with it.
    pub fn show(&mut self, panel: T) {
        if !self.open {
            self.contents.clear();
        }
        self.contents.retain(|p| p.name() != panel.name());
        self.contents.push(panel);
        self.open = true;
    }

    /// Takes content `name` out; the panel closes when none is left.
    /// Whether it was the one showing.
    pub fn remove(&mut self, name: &str) -> bool {
        let was_top = self.top().is_some_and(|p| p.name() == name);
        self.contents.retain(|p| p.name() != name);
        if self.contents.is_empty() {
            self.open = false;
        }
        was_top
    }

    /// Content `name` shows now, the panel open.
    pub fn shows(&self, name: &str) -> bool {
        self.open && self.top().is_some_and(|p| p.name() == name)
    }

    /// Content is under the top, for Back to go back to.
    pub fn has_under(&self) -> bool {
        self.contents.len() > 1
    }

    /// Whether the panel is open with page `page`'s content showing.
    pub fn lends(&self, page: &str) -> bool {
        self.open && self.top().is_some_and(|p| p.page() == page)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Content(&'static str, &'static str);
    impl Named for Content {
        fn name(&self) -> &str {
            self.0
        }
        fn page(&self) -> &str {
            self.1
        }
    }

    fn names(stack: &Stack<Content>) -> Vec<&str> {
        stack.contents.iter().map(|c| c.0).collect()
    }

    #[test]
    fn one_content_shows_and_what_it_opened_over_comes_back() {
        let mut stack = Stack::default();
        assert!(!stack.open && stack.top().is_none());
        stack.show(Content("files/files", "files"));
        stack.show(Content("sessions/send", "sessions"));
        assert!(stack.shows("sessions/send"));
        assert!(!stack.shows("files/files"), "only the top shows");
        assert!(stack.lends("sessions") && !stack.lends("files"));
        assert!(stack.has_under());
        // Shown again, a content moves to the top rather than standing twice.
        stack.show(Content("files/files", "files"));
        assert_eq!(names(&stack), ["sessions/send", "files/files"]);
        // Its page closing it brings back what it opened over.
        assert!(stack.remove("files/files"));
        assert!(stack.shows("sessions/send"));
        // Put away, the top stays for Ctrl+B to bring back.
        stack.open = false;
        assert!(!stack.shows("sessions/send") && !stack.lends("sessions"));
        assert!(!stack.remove("files/files"), "not there, not the top");
        assert!(stack.remove("sessions/send"));
        assert!(!stack.open && stack.top().is_none());
    }

    #[test]
    fn content_opened_on_a_put_away_panel_starts_it_over() {
        let mut stack = Stack::default();
        stack.show(Content("issue/card", "issue"));
        stack.open = false;
        stack.show(Content("sessions/diagram", "sessions"));
        assert!(!stack.has_under(), "nothing showed when it opened");
        // Its toggle taking it out closes the panel, revealing nothing.
        assert!(stack.remove("sessions/diagram"));
        assert!(!stack.open && stack.top().is_none());
    }
}
