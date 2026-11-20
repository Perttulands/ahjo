//! Ahjo Swarm's native app: the shell (`ahjo-app-shell`) in a window, holding
//! the pages listed here. Each area's page is listed here, and nowhere else.

mod app_log;

use ahjo_app_kit::Build;
use ahjo_app_shell::start;
use gpui::{App, AppContext, Bounds, WindowBounds, WindowOptions, point, px, size};
use std::borrow::Cow;
use std::path::Path;

/// The first page is the one the app opens on: the terminals, where the
/// operator spends the day.
pub const PAGES: &[Build] = &[
    ahjo_terminals_app::build,
    ahjo_files_app::build,
    ahjo_harness_app::build,
    ahjo_issue_app::build,
    ahjo_brain_app::build,
    ahjo_missions_app::build,
    ahjo_scheduled_app::build,
    ahjo_host_app::build,
];

fn fonts() -> Vec<Cow<'static, [u8]>> {
    ahjo_app_kit::theme::FONT_FILES
        .iter()
        .map(|f| Cow::Borrowed(*f))
        .collect()
}

/// `AHJO_SWARM_BOUNDS=x,y,w,h` places the window exactly (tests on an
/// invisible display); otherwise it opens centred.
fn bounds(cx: &App) -> Bounds<gpui::Pixels> {
    let given: Option<Vec<f32>> = std::env::var("AHJO_SWARM_BOUNDS")
        .ok()
        .map(|v| v.split(',').filter_map(|n| n.trim().parse().ok()).collect());
    match given.as_deref() {
        Some([x, y, w, h]) => Bounds::new(point(px(*x), px(*y)), size(px(*w), px(*h))),
        _ => Bounds::centered(None, size(px(1440.), px(900.)), cx),
    }
}

/// Retain saved choices when a page gets a new id.
fn migrate_page_preferences(path: &Path) -> std::io::Result<()> {
    use ahjo_app_kit::settings::Settings;
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if let Some(mut settings) = Settings::parse(&text) {
        let mut changed = false;
        for (old, new) in [("agents", "harness"), ("work", "issue")] {
            if !settings.sections.contains_key(new)
                && let Some(previous) = settings.sections.remove(old)
            {
                settings.sections.insert(new.into(), previous);
                changed = true;
            }
        }
        if changed {
            Settings::write(path, &settings.text())?;
        }
    }
    Ok(())
}

/// The app: `ahjo-swarm [--node http://host:port]`.
pub fn run() {
    let node = {
        let args: Vec<String> = std::env::args().collect();
        args.iter()
            .position(|a| a == "--node")
            .and_then(|i| args.get(i + 1).cloned())
    };
    app_log::install();
    let settings = ahjo_app_kit::settings::path();
    if let Some(path) = settings.as_deref()
        && let Err(error) = migrate_page_preferences(path)
    {
        log::warn!(
            "could not migrate Swarm page preferences at {}: {error}",
            path.display()
        );
    }
    gpui_platform::application().run(move |cx: &mut App| {
        if let Err(e) = cx.text_system().add_fonts(fonts()) {
            panic!("the bundled fonts did not load: {e}");
        }
        ahjo_app_shell::prepare(cx);
        // Desktop notifications need the app named before they are posted.
        cx.set_app_identity("io.ahjo.swarm", "Ahjo Swarm");
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds(cx))),
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some("Ahjo Swarm".into()),
                    ..Default::default()
                }),
                app_id: Some("ahjo-swarm".into()),
                ..Default::default()
            },
            |window, cx| {
                cx.new(|cx| {
                    let mut shell = start(PAGES, node, settings, window, cx);
                    shell.offer_updates(env!("AHJO_VERSION"), window, cx);
                    shell
                })
            },
        );
        if let Err(e) = opened {
            panic!("the window did not open: {e}");
        }
        cx.on_window_closed(|cx, _| cx.quit()).detach();
    });
}
