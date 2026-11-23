//! The Desk on the kit's frame, against a desk of the test's own: Escape
//! closes one thing at a time, an error stays until he closes it, and an
//! edit to `desk.json` made outside the Desk applies at once.

mod support;

use ahjo_app_desk::desk::Config;
use ahjo_testkit::app::bounds;
use std::time::Duration;
use support::*;

/// Escape closes only what stands on top: the menu, then the theme picker
/// over Settings, then Settings; the ask in front is never touched.
#[gpui::test]
fn escape_closes_only_the_top_layer(cx: &mut gpui::TestAppContext) {
    let desk = FakeDesk::start(vec![
        doc_ask("AH1", "A document."),
        doc_ask("AH2", "Another."),
    ]);
    let dir = scratch("escape");
    let (app, cx) = open(&desk, &dir, cx);
    let current = |cx: &mut gpui::VisualTestContext| {
        app.read_with(cx, |a, _| a.current_ask().map(|a| a.id.clone()))
    };
    let first = current(cx);

    click(cx, "menu-open");
    until(cx, "the menu", |cx| {
        app.read_with(cx, |a, cx| a.menu_open(cx))
    });
    cx.simulate_keystrokes("escape");
    until(cx, "the menu to close", |cx| {
        app.read_with(cx, |a, cx| !a.menu_open(cx))
    });
    assert_eq!(current(cx), first, "the ask in front stays");
    assert!(bounds(cx, "list-left").is_some(), "the list stays");

    cx.simulate_keystrokes("ctrl-,");
    until(cx, "Settings", |cx| bounds(cx, "settings-layer").is_some());
    click(cx, "theme-open");
    until(cx, "the theme picker", |cx| {
        bounds(cx, "theme-picker").is_some()
    });
    cx.simulate_keystrokes("escape");
    until(cx, "the picker to close", |cx| {
        bounds(cx, "theme-picker").is_none()
    });
    assert!(
        app.read_with(cx, |a, cx| a.settings_open(cx)),
        "Settings stay under the picker"
    );
    cx.simulate_keystrokes("escape");
    until(cx, "Settings to close", |cx| {
        app.read_with(cx, |a, cx| !a.settings_open(cx))
    });
    assert_eq!(current(cx), first, "the ask in front stays");
    let _ = std::fs::remove_dir_all(dir);
}

/// An answer the desk refuses says so, and the words stay long after a
/// moment's word would have gone, until he closes them.
#[gpui::test]
fn an_error_stays_until_closed(cx: &mut gpui::TestAppContext) {
    let desk = FakeDesk::start(vec![
        doc_ask("AH1", "A document."),
        doc_ask("AH2", "Another."),
    ]);
    let dir = scratch("error-stays");
    let (app, cx) = open(&desk, &dir, cx);
    desk.state.lock().unwrap().refuse = true;
    click(cx, "elsewhere");
    let undo = Duration::from_millis(ahjo_app_desk::tokens::UNDO_MS);
    cx.executor().advance_clock(undo);
    let said = |cx: &mut gpui::VisualTestContext| {
        app.read_with(cx, |a, cx| {
            a.frame
                .read(cx)
                .status_shown()
                .filter(|s| s.failure)
                .map(|s| s.text.clone())
        })
    };
    until(cx, "the refusal", |cx| said(cx).is_some());
    let words = said(cx).unwrap();
    assert!(
        words.contains("Not sent") && words.contains("restarting"),
        "{words}"
    );
    cx.executor().advance_clock(Duration::from_secs(60));
    until(cx, "a frame", |_| true);
    assert!(
        bounds(cx, "status-text").is_some(),
        "the error is still on screen a minute later"
    );
    click(cx, "status-close");
    until(cx, "the error to close", |cx| {
        bounds(cx, "status-text").is_none()
    });
    let _ = std::fs::remove_dir_all(dir);
}

/// `desk.json` edited outside the Desk (by hand, or by an agent) applies
/// at once: here the list is put away.
#[gpui::test]
fn an_outside_edit_to_desk_json_is_picked_up(cx: &mut gpui::TestAppContext) {
    let desk = FakeDesk::start(vec![doc_ask("AH1", "A document.")]);
    let dir = scratch("outside-edit");
    let (app, cx) = open(&desk, &dir, cx);
    assert!(bounds(cx, "list-left").is_some(), "the list shows");
    let edited = Config {
        fold_list: true,
        tell_session: false,
        ..Default::default()
    };
    std::fs::write(
        dir.join("desk.json"),
        ahjo_app_kit::settings_file::text(&edited),
    )
    .expect("the edit");
    until(cx, "the edit to apply", |cx| {
        app.read_with(cx, |a, _| a.config.fold_list)
    });
    until(cx, "the list to slide away", |cx| {
        bounds(cx, "list-left").is_none()
    });
    let _ = std::fs::remove_dir_all(dir);
}
