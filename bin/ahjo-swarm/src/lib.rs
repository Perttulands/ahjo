//! Ahjo Swarm's native app: the shell (`ahjo-app-shell`) in a window, holding
//! the pages listed here. Each area's page is listed here, and nowhere else.

mod app_log;

use ahjo_app_kit::Build;
use ahjo_app_shell::start;
use gpui::{App, AppContext, Bounds, WindowBounds, WindowOptions, point, px, size};
use std::borrow::Cow;

/// The first page is the one the app opens on: the terminals, where the
/// operator spends the day.
pub const PAGES: &[Build] = &[
    ahjo_terminals_app::build,
    ahjo_files_app::build,
    ahjo_agents_app::build,
    ahjo_work_app::build,
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

/// The app: `ahjo-swarm [--node http://host:port]`.
pub fn run() {
    let node = {
        let args: Vec<String> = std::env::args().collect();
        args.iter()
            .position(|a| a == "--node")
            .and_then(|i| args.get(i + 1).cloned())
    };
    app_log::install();
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
                cx.new(|cx| start(PAGES, node, ahjo_app_kit::settings::path(), window, cx))
            },
        );
        if let Err(e) = opened {
            panic!("the window did not open: {e}");
        }
        cx.on_window_closed(|cx, _| cx.quit()).detach();
    });
}
