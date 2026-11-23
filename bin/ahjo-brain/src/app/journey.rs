//! Journeys through the window against a brain of the test's own: what
//! opens over the sky closes one at a time, and nothing at the top covers
//! the search, however narrow the window.

use super::*;
use ahjo_testkit::app::{bounds, click, until};
use gpui::{TestAppContext, VisualTestContext, size};
use std::path::Path;

/// A brain of a few pages in two subjects, linked, in a folder of the
/// test's own; its settings and cache beside it.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ahjo-brain-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let pages = [
        ("sea", ["Tides", "Kelp", "Knots"]),
        ("wood", ["Birch", "Resin", "Axes"]),
    ];
    for (subject, titles) in pages {
        let folder = dir.join("brain/knowledge").join(subject);
        std::fs::create_dir_all(&folder).unwrap();
        for title in titles {
            let text = format!(
                "---\nsummary: What we know of {title}\n---\n# {title}\n\nSee [[tides]], [[birch]] and [[kelp]].\n"
            );
            std::fs::write(folder.join(format!("{}.md", title.to_lowercase())), text).unwrap();
        }
    }
    dir
}

/// Brain in a test window `w` wide, on the brain in `dir`, reading `open`
/// once it has come.
fn open<'a>(
    dir: &Path,
    w: f32,
    read: Option<&str>,
    cx: &'a mut TestAppContext,
) -> (Entity<BrainApp>, &'a mut VisualTestContext) {
    cx.executor().allow_parking();
    cx.update(|cx| {
        cx.set_global(Theme::default());
        field::bind_keys(cx);
    });
    let start = Start {
        place: Where {
            brain: Some(dir.join("brain")),
            // Nothing answers there: the search matches titles here.
            citadel: "http://127.0.0.1:1".into(),
            cache: Some(dir.join("cache")),
            from: None,
        },
        from: None,
        settings: SettingsFile::load(Some(dir.join("settings.json"))),
        query: None,
        open: read.map(str::to_string),
        bench: None,
    };
    let (app, cx) = cx.add_window_view(move |window, cx| BrainApp::new(start, window, cx));
    cx.simulate_resize(size(px(w), px(768.)));
    until(cx, "the sky", |cx| {
        app.read_with(cx, |a, _| {
            a.data.is_some() && (read.is_none() || a.reading.is_some())
        })
    });
    (app, cx)
}

fn shown(cx: &mut VisualTestContext, selector: &str) -> gpui::Bounds<Pixels> {
    let mut at = None;
    until(cx, &format!("{selector} on screen"), |cx| {
        at = bounds(cx, selector);
        at.is_some()
    });
    at.unwrap()
}

fn gone(cx: &mut VisualTestContext, selector: &str) {
    until(cx, &format!("{selector} gone"), |cx| {
        bounds(cx, selector).is_none()
    });
}

/// Escape closes what opened last, and only that: the theme picker over
/// the page read, then the menu of the folded actions, then the page.
#[gpui::test]
fn escape_closes_only_the_top(cx: &mut TestAppContext) {
    let dir = scratch("escape");
    let (app, cx) = open(&dir, 1024., Some("knowledge/sea/tides.md"), cx);
    let reading = |cx: &mut VisualTestContext| app.read_with(cx, |a, _| a.reading.is_some());

    cx.simulate_keystrokes("ctrl-t");
    shown(cx, "theme-picker");
    cx.simulate_keystrokes("escape");
    gone(cx, "theme-picker");
    assert!(reading(cx), "the page stays while the picker closes");

    // Narrow beside the page, the actions fold into one chip and its menu.
    click(cx, "more");
    shown(cx, "menu-row-0");
    cx.simulate_keystrokes("escape");
    gone(cx, "menu-row-0");
    assert!(reading(cx), "the page stays while the menu closes");

    cx.simulate_keystrokes("escape");
    until(cx, "the page to close", |cx| !reading(cx));
    let _ = std::fs::remove_dir_all(dir);
}

/// However wide the window, with a page read or not, the actions at the
/// top and what they open stand clear of the search.
#[gpui::test]
fn nothing_covers_the_search(cx: &mut TestAppContext) {
    let dir = scratch("search");
    let (app, cx) = open(&dir, 1920., None, cx);
    let open_page = |cx: &mut VisualTestContext, read: bool| {
        app.update(cx, |a, cx| match read {
            true => a.open(0, cx),
            false => a.close(cx),
        });
    };
    for read in [false, true] {
        open_page(cx, read);
        for w in (800..=1920).step_by(40) {
            cx.simulate_resize(size(px(w as f32), px(768.)));
            cx.run_until_parked();
            let search = shown(cx, "search");
            let actions = shown(cx, "actions");
            assert!(
                !search.intersects(&actions),
                "at {w} px, reading {read}: the actions {actions:?} cover the search {search:?}"
            );
        }
    }
    // The narrowest beside a page: what the folded chip opens stands
    // clear of the search too.
    cx.simulate_resize(size(px(1024.), px(768.)));
    click(cx, "more");
    let menu = shown(cx, "popup");
    let search = shown(cx, "search");
    assert!(
        !search.intersects(&menu),
        "the menu {menu:?} covers the search {search:?}"
    );
    let _ = std::fs::remove_dir_all(dir);
}
