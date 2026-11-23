//! A popup behaves the same way on every page, because the shell does it
//! all but its content: it opens beside what was clicked and stays on the
//! screen, Escape closes only the top one, a press outside closes it or
//! not as it declares and never reaches the page under it, and when it
//! closes the keyboard goes back where it was: to a terminal, which then
//! gets its keys again. A toy page, made here, with a stand-in terminal.

use ahjo_app_kit::popup::{Outside, Popup, PopupId, Spot};
use ahjo_app_kit::{Build, Host, Mounted, Page, mount};
use ahjo_app_shell::Shell;
use gpui::{
    App, Bounds, Entity, FocusHandle, KeyDownEvent, Modifiers, MouseButton, Pixels, TestAppContext,
    VisualTestContext, Window, div, point, prelude::*, px, size,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// A small form in a popup: its own field, counting the keys it gets.
struct Form {
    name: &'static str,
    focus: FocusHandle,
    keys: Rc<Cell<usize>>,
}

impl Render for Form {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let name = self.name;
        let keys = self.keys.clone();
        div()
            .debug_selector(move || name.into())
            .track_focus(&self.focus)
            .on_key_down(move |_: &KeyDownEvent, _, _| keys.set(keys.get() + 1))
            .w(px(320.))
            .h(px(200.))
            .child(
                div()
                    .id("inner-open")
                    .debug_selector(|| "inner-open".into())
                    .size(px(40.))
                    .on_click(cx.listener(|_, _, window, cx| {
                        PAD.with(|p| p.borrow().clone())
                            .expect("Pad")
                            .update(cx, |p, cx| p.open("inner", Outside::Close, window, cx));
                    })),
            )
    }
}

/// The page: a terminal stand-in filling it, and buttons in two corners
/// that open a popup.
struct Pad {
    host: Host,
    term: FocusHandle,
    term_keys: usize,
    presses: usize,
    forms: Vec<(PopupId, Entity<Form>)>,
    /// What a popup asks before Escape or a press outside closes it.
    may_close: Rc<Cell<bool>>,
}

thread_local! {
    static PAD: RefCell<Option<Entity<Pad>>> = const { RefCell::new(None) };
}

impl Page for Pad {
    const ID: &'static str = "pad";
    const TITLE: &'static str = "Pad";
    fn focus(&self, _: &App) -> Option<FocusHandle> {
        Some(self.term.clone())
    }
}

impl Pad {
    fn open(
        &mut self,
        name: &'static str,
        outside: Outside,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let form = cx.new(|cx| Form {
            name,
            focus: cx.focus_handle(),
            keys: Rc::new(Cell::new(0)),
        });
        let asked = self.may_close.clone();
        let popup = Popup::beside(Spot::here(window, cx), form.clone())
            .focus(form.read(cx).focus.clone())
            .outside(outside)
            .may_close(move |_, _| asked.get());
        if let Some(id) = self.host.popup(popup, cx) {
            self.forms.push((id, form));
        }
    }

    /// The keys the newest popup called `name` got.
    fn keys_of(&self, name: &str, cx: &App) -> usize {
        self.forms
            .iter()
            .rev()
            .find(|(_, f)| f.read(cx).name == name)
            .map(|(_, f)| f.read(cx).keys.get())
            .unwrap_or(0)
    }
}

fn corner_button(id: &'static str, outside: Outside) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .debug_selector(move || id.into())
        .absolute()
        .w(px(80.))
        .h(px(28.))
        // Reports its press, as every kit button does.
        .child(ahjo_app_kit::pointer::watch())
        .on_click(move |_, window, cx| {
            PAD.with(|p| p.borrow().clone())
                .expect("Pad")
                .update(cx, |p, cx| p.open("form", outside, window, cx));
        })
}

impl Render for Pad {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .debug_selector(|| "term".into())
            .size_full()
            .relative()
            .track_focus(&self.term)
            .on_key_down(cx.listener(|p, _: &KeyDownEvent, _, _| p.term_keys += 1))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|p, _, window, cx| {
                    p.presses += 1;
                    window.focus(&p.term, cx);
                }),
            )
            .child(
                corner_button("open-near", Outside::Close)
                    .top(px(10.))
                    .left(px(10.)),
            )
            .child(
                corner_button("open-corner", Outside::Close)
                    .bottom(px(2.))
                    .right(px(2.)),
            )
            .child(
                corner_button("open-kept", Outside::Keep)
                    .top(px(10.))
                    .left(px(200.)),
            )
    }
}

fn pad(host: Host, window: &mut Window, cx: &mut App) -> Mounted {
    mount(host, window, cx, |host, _, cx| {
        PAD.with(|p| *p.borrow_mut() = Some(cx.entity()));
        Pad {
            host,
            term: cx.focus_handle(),
            term_keys: 0,
            presses: 0,
            forms: vec![],
            may_close: Rc::new(Cell::new(true)),
        }
    })
}

const PAGES: &[Build] = &[pad];

fn press(cx: &mut VisualTestContext, at: gpui::Point<Pixels>) {
    cx.simulate_click(at, Modifiers::none());
    cx.run_until_parked();
}

fn click(cx: &mut VisualTestContext, selector: &'static str) {
    cx.run_until_parked();
    let at = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is not drawn"))
        .center();
    press(cx, at);
}

fn drawn(cx: &mut VisualTestContext, selector: &'static str) -> Option<Bounds<Pixels>> {
    cx.run_until_parked();
    cx.debug_bounds(selector)
}

fn open_popups(shell: &Entity<Shell>, cx: &mut VisualTestContext) -> usize {
    shell.read_with(cx, |s, cx| s.hub().read(cx).stack.iter().count())
}

fn start(cx: &mut TestAppContext) -> (Entity<Shell>, &mut VisualTestContext, Entity<Pad>) {
    let node = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").expect("a port");
        format!("http://{}", l.local_addr().expect("its address"))
    };
    cx.executor().allow_parking();
    cx.update(ahjo_app_shell::prepare);
    let (shell, cx) = cx.add_window_view(move |window, cx| {
        ahjo_app_shell::start(PAGES, Some(node), None, window, cx)
    });
    cx.simulate_resize(size(px(1366.), px(768.)));
    cx.run_until_parked();
    let pad = PAD.with(|p| p.borrow().clone()).expect("Pad was made");
    let term = pad.read_with(cx, |p, _| p.term.clone());
    cx.update(|window, cx| window.focus(&term, cx));
    cx.run_until_parked();
    (shell, cx, pad)
}

#[gpui::test]
fn a_popup_stands_beside_its_button_and_on_screen_at_1366(cx: &mut TestAppContext) {
    let (_, cx, _) = start(cx);
    let window = Bounds::new(point(px(0.), px(0.)), size(px(1366.), px(768.)));

    // Near the top left: under the button, at its left edge.
    let button = drawn(cx, "open-near").unwrap();
    click(cx, "open-near");
    let popup = drawn(cx, "popup").expect("open beside its button");
    assert!(
        popup.top() >= button.bottom() && popup.top() - button.bottom() < px(12.),
        "{popup:?} under {button:?}"
    );
    assert!(
        (popup.left() - button.left()).abs() < px(1.),
        "{popup:?} at {button:?}'s left"
    );
    cx.simulate_keystrokes("escape");
    assert!(drawn(cx, "popup").is_none());

    // In the bottom right corner: it turns to stay whole on the screen,
    // still touching its button.
    let button = drawn(cx, "open-corner").unwrap();
    click(cx, "open-corner");
    let popup = drawn(cx, "popup").expect("open beside the corner button");
    assert!(
        window.contains(&popup.origin)
            && popup.right() <= window.right()
            && popup.bottom() <= window.bottom(),
        "{popup:?} inside {window:?}"
    );
    assert!(
        popup.bottom() <= button.top() && button.top() - popup.bottom() < px(12.),
        "{popup:?} over {button:?}"
    );
    assert!(
        (popup.right() - button.right()).abs() < px(12.),
        "{popup:?} at {button:?}'s right"
    );
}

#[gpui::test]
fn escape_closes_the_top_popup_only_and_the_keys_go_back(cx: &mut TestAppContext) {
    let (shell, cx, pad) = start(cx);
    let term = pad.read_with(cx, |p, _| p.term.clone());

    click(cx, "open-near");
    assert!(drawn(cx, "form").is_some());
    cx.simulate_keystrokes("a");
    assert_eq!(
        pad.read_with(cx, |p, cx| p.keys_of("form", cx)),
        1,
        "the form has the keys"
    );
    assert_eq!(
        pad.read_with(cx, |p, _| p.term_keys),
        0,
        "the terminal does not"
    );

    // A second popup from inside the first.
    click(cx, "inner-open");
    assert!(drawn(cx, "inner").is_some());
    assert_eq!(open_popups(&shell, cx), 2);
    cx.simulate_keystrokes("escape");
    assert!(drawn(cx, "inner").is_none(), "the top one closed");
    assert!(drawn(cx, "form").is_some(), "the one under it stays");
    cx.simulate_keystrokes("b");
    assert_eq!(
        pad.read_with(cx, |p, cx| p.keys_of("form", cx)),
        2,
        "the keys went back to the form"
    );

    cx.simulate_keystrokes("escape");
    assert!(drawn(cx, "form").is_none());
    assert_eq!(open_popups(&shell, cx), 0);
    assert!(
        cx.update(|window, _| term.is_focused(window)),
        "the keys went back to the terminal"
    );
    // Escape with nothing open is the terminal's again, and so is typing.
    let before = pad.read_with(cx, |p, _| p.term_keys);
    cx.simulate_keystrokes("c escape");
    assert_eq!(pad.read_with(cx, |p, _| p.term_keys), before + 2);
    assert_eq!(
        pad.read_with(cx, |p, cx| p.keys_of("form", cx)),
        2,
        "a closed popup takes no keys"
    );
}

#[gpui::test]
fn a_press_outside_closes_or_keeps_as_declared_and_reaches_nothing(cx: &mut TestAppContext) {
    let (_, cx, pad) = start(cx);
    let term = pad.read_with(cx, |p, _| p.term.clone());
    let presses = |cx: &mut VisualTestContext| pad.read_with(cx, |p, _| p.presses);
    let outside = point(px(900.), px(500.));

    // Close: the press closes it, and the page under it never hears it.
    click(cx, "open-near");
    let before = presses(cx);
    press(cx, outside);
    assert!(drawn(cx, "form").is_none(), "a press outside closed it");
    assert_eq!(presses(cx), before, "the press reached nothing under it");
    assert!(cx.update(|window, _| term.is_focused(window)));

    // Keep: the press is swallowed and it stays, with the keys.
    click(cx, "open-kept");
    let before = presses(cx);
    press(cx, outside);
    assert!(drawn(cx, "form").is_some(), "a kept popup stays");
    assert_eq!(presses(cx), before);
    cx.simulate_keystrokes("x");
    assert_eq!(pad.read_with(cx, |p, cx| p.keys_of("form", cx)), 1);
    assert_eq!(pad.read_with(cx, |p, _| p.term_keys), 0);

    // A popup that may not close now stays through Escape too.
    pad.read_with(cx, |p, _| p.may_close.set(false));
    cx.simulate_keystrokes("escape");
    assert!(drawn(cx, "form").is_some(), "it said not now");
    pad.read_with(cx, |p, _| p.may_close.set(true));
    cx.simulate_keystrokes("escape");
    assert!(drawn(cx, "form").is_none());
    assert!(cx.update(|window, _| term.is_focused(window)));

    // Its page closing it is not asked.
    click(cx, "open-near");
    pad.read_with(cx, |p, _| p.may_close.set(false));
    let id = pad.read_with(cx, |p, _| p.forms.last().unwrap().0);
    cx.update(|_, cx| pad.read(cx).host.clone().close_popup(id, cx));
    assert!(drawn(cx, "form").is_none());
    assert!(cx.update(|window, _| term.is_focused(window)));
}

#[gpui::test]
fn the_theme_picker_is_a_popup_beside_its_control(cx: &mut TestAppContext) {
    let (shell, cx, _) = start(cx);
    let theme =
        |cx: &mut VisualTestContext| cx.update(|_, cx| ahjo_app_kit::theme::theme(cx).direction);
    click(cx, "tab-settings");
    assert_eq!(shell.read_with(cx, |s, _| s.shown_page()), Some("settings"));
    let before = theme(cx);
    let control = drawn(cx, "setting-app-theme").expect("the theme control");
    click(cx, "setting-app-theme");
    assert!(drawn(cx, "theme-picker").is_some(), "the picker opened");
    let picker = drawn(cx, "popup").expect("in a popup");
    assert!(
        picker.top() >= control.bottom() && picker.top() - control.bottom() < px(12.),
        "{picker:?} under {control:?}"
    );
    // Its press is the button's inside its border.
    assert!(
        (picker.left() - control.left()).abs() < px(2.),
        "{picker:?} at {control:?}'s left"
    );
    // The keys try a theme; Escape takes it back and closes the picker.
    cx.simulate_keystrokes("down");
    assert_ne!(theme(cx), before, "down tries the next theme");
    cx.simulate_keystrokes("escape");
    assert!(drawn(cx, "theme-picker").is_none());
    assert_eq!(theme(cx), before, "Escape keeps the theme it was");
    // Enter keeps the one tried.
    click(cx, "setting-app-theme");
    cx.simulate_keystrokes("down");
    let tried = theme(cx);
    cx.simulate_keystrokes("enter");
    assert!(drawn(cx, "theme-picker").is_none());
    assert_eq!(theme(cx), tried, "Enter kept it");
}
