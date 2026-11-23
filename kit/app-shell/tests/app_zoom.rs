//! The app's zoom: Ctrl+=, Ctrl+- and Ctrl+0, and Ctrl+wheel over anything
//! without a zoom of its own, size the whole window and keep the size; a
//! surface that claims its Ctrl+wheel keeps it; the Settings page steps and
//! resets it; a size kept in app.json is drawn from the start.

use ahjo_app_kit::widgets::pane;
use ahjo_app_kit::{Host, Mounted, Page, mount, zoom};
use ahjo_app_shell::Shell;
use ahjo_testkit::app::until;
use gpui::{
    Entity, Modifiers, ScrollDelta, ScrollEvent, TestAppContext, VisualTestContext, Window, div,
    point, prelude::*, px,
};
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

thread_local! {
    /// The Ctrl+wheels the surface with its own zoom heard.
    static OWN: Rc<Cell<usize>> = Rc::new(Cell::new(0));
}

/// A page of a plain list and a surface with its own zoom.
struct Form;

impl Page for Form {
    const ID: &'static str = "form";
    const TITLE: &'static str = "Form";
}

impl Render for Form {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let own = OWN.with(Rc::clone);
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                pane::scroll("plain")
                    .debug_selector(|| "plain".into())
                    .h(px(120.))
                    .child(div().h(px(600.))),
            )
            .child(
                div()
                    .id("own")
                    .debug_selector(|| "own".into())
                    .relative()
                    .h(px(120.))
                    .on_scroll(move |e, _, _| {
                        if e.modifiers.control {
                            own.set(own.get() + 1);
                        }
                    })
                    .child(zoom::claim(|_, _| true)),
            )
    }
}

fn form(host: Host, window: &mut Window, cx: &mut gpui::App) -> Mounted {
    mount(host, window, cx, |_, _, _| Form)
}

/// No node answers there: the zoom needs none.
const NODE: &str = "http://127.0.0.1:9";

fn settings_path(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("ahjo-app-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch folder");
    dir.join("app.json")
}

fn start(
    path: std::path::PathBuf,
    cx: &mut TestAppContext,
) -> (Entity<Shell>, &mut VisualTestContext) {
    cx.executor().allow_parking();
    cx.update(ahjo_app_shell::prepare);
    cx.add_window_view(move |window, cx| {
        ahjo_app_shell::start(&[form], Some(NODE.into()), Some(path), window, cx)
    })
}

fn size(shell: &Entity<Shell>, cx: &mut VisualTestContext) -> u16 {
    shell.read_with(cx, |s, cx| s.hub().read(cx).settings.size)
}

fn ui(cx: &mut VisualTestContext) -> f32 {
    cx.update(|window, _| window.ui_scale())
}

fn toast(shell: &Entity<Shell>, cx: &mut VisualTestContext) -> String {
    shell.read_with(cx, |s, cx| {
        let hub = s.hub().read(cx);
        hub.toast
            .as_ref()
            .map(|t| t.text.clone())
            .unwrap_or_default()
    })
}

/// A Ctrl+wheel notch up over what `selector` draws. The platform reports
/// where the pointer is in its own pixels, the window's times its zoom.
fn ctrl_wheel(cx: &mut VisualTestContext, selector: &'static str) {
    cx.run_until_parked();
    let ui = ui(cx);
    let at = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is not drawn"))
        .center()
        .map(|v| v * ui);
    cx.simulate_event(ScrollEvent {
        position: at,
        delta: ScrollDelta::Lines(point(0., 3.)),
        modifiers: Modifiers {
            control: true,
            ..Default::default()
        },
        ..Default::default()
    });
    cx.run_until_parked();
}

fn click(cx: &mut VisualTestContext, selector: &'static str) {
    cx.run_until_parked();
    let ui = ui(cx);
    let at = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is not drawn"))
        .center()
        .map(|v| v * ui);
    cx.simulate_click(at, Modifiers::none());
    cx.run_until_parked();
}

fn on_disk(path: &std::path::Path) -> Option<u16> {
    let text = std::fs::read_to_string(path).ok()?;
    Some(ahjo_app_kit::settings::Settings::parse(&text)?.size)
}

#[gpui::test]
fn the_app_zooms_wherever_no_surface_zooms_itself(cx: &mut TestAppContext) {
    let settings = settings_path("app-zoom");
    let (shell, cx) = start(settings.clone(), cx);
    cx.run_until_parked();
    assert_eq!((size(&shell, cx), ui(cx)), (100, 1.));
    let whole = cx.update(|window, _| window.viewport_size());

    cx.simulate_keystrokes("ctrl-=");
    assert_eq!((size(&shell, cx), ui(cx)), (105, 1.05));
    assert_eq!(toast(&shell, cx), "App zoom 105% · Ctrl+0 resets");
    // Everything grows: the window holds fewer of the app's pixels.
    let now = cx.update(|window, _| window.viewport_size());
    assert!((f32::from(now.width) * 1.05 - f32::from(whole.width)).abs() < 0.01);

    // Over a list, Ctrl+wheel is the app's zoom, not the list's scroll.
    ctrl_wheel(cx, "plain");
    assert_eq!(size(&shell, cx), 110);
    // Over a surface with a zoom of its own, it is the surface's.
    ctrl_wheel(cx, "own");
    assert_eq!(size(&shell, cx), 110, "the surface keeps its wheel");
    assert_eq!(OWN.with(|o| o.get()), 1);

    cx.simulate_keystrokes("ctrl--");
    assert_eq!(size(&shell, cx), 105);
    cx.simulate_keystrokes("ctrl-0");
    assert_eq!((size(&shell, cx), ui(cx)), (100, 1.));
    assert_eq!(toast(&shell, cx), "App zoom 100%");

    // The Settings page steps it and resets it, Reset shown while needed.
    click(cx, "tab-settings");
    assert!(cx.debug_bounds("setting-app-size-reset").is_none());
    click(cx, "setting-app-size-up");
    click(cx, "setting-app-size-up");
    assert_eq!(size(&shell, cx), 110);
    assert!(cx.debug_bounds("setting-app-size:110").is_some());
    cx.executor().advance_clock(Duration::from_millis(300));
    until(cx, "the size kept", |_| on_disk(&settings) == Some(110));
    click(cx, "setting-app-size-reset");
    assert_eq!((size(&shell, cx), ui(cx)), (100, 1.));
    let _ = std::fs::remove_dir_all(settings.parent().expect("its folder"));
}

#[gpui::test]
fn a_size_kept_is_drawn_from_the_start(cx: &mut TestAppContext) {
    let settings = settings_path("app-zoom-kept");
    let kept = ahjo_app_kit::settings::Settings {
        size: 130,
        ..Default::default()
    };
    std::fs::write(&settings, kept.text()).expect("the kept settings");
    let (shell, cx) = start(settings.clone(), cx);
    cx.run_until_parked();
    assert_eq!((size(&shell, cx), ui(cx)), (130, 1.3));
    let _ = std::fs::remove_dir_all(settings.parent().expect("its folder"));
}
