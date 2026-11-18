//! The Desk: the shared surface where an agent publishes something (a
//! picture, a clip, a page, a document, a question) and the operator
//! answers it freely, in his words and by marking what he sees, in a window
//! of its own over each decision desk's HTTP API. `model` is the desk's
//! asks as its server sends them, `desk` the server's calls, `answer` what
//! an answer says, `marks` his marks, `page` a published page inside the
//! window, `video` and `player` the clips, `app` the window's state, and
//! `stage` and `views` how it is drawn.

pub mod answer;
pub mod app;
pub mod desk;
pub mod marks;
pub mod model;
pub mod page;
pub mod player;
pub mod stage;
pub mod tokens;
pub mod video;
pub mod views;

use gpui::{App, AppContext, Bounds, WindowBounds, WindowOptions, point, px, size};

use ahjo_app_kit::widgets::field;
use std::borrow::Cow;

/// `AHJO_DESK_BOUNDS=x,y,w,h` places the window exactly (runs on an
/// invisible display); otherwise it opens centred.
fn bounds(cx: &App) -> Bounds<gpui::Pixels> {
    let given: Option<Vec<f32>> = std::env::var("AHJO_DESK_BOUNDS")
        .ok()
        .map(|v| v.split(',').filter_map(|n| n.trim().parse().ok()).collect());
    match given.as_deref() {
        Some([x, y, w, h]) => Bounds::new(point(px(*x), px(*y)), size(px(*w), px(*h))),
        _ => Bounds::centered(None, size(px(1440.), px(900.)), cx),
    }
}

/// The app: `ahjo-desk [--desk [Name=]http://127.0.0.1:8774]...`.
pub fn run() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (config, notice) = desk::load(&args);
    gpui_platform::application().run(move |cx: &mut App| {
        let fonts = ahjo_app_kit::theme::FONT_FILES
            .iter()
            .map(|f| Cow::Borrowed(*f))
            .collect();
        if let Err(e) = cx.text_system().add_fonts(fonts) {
            panic!("the bundled fonts did not load: {e}");
        }
        let (theme, unknown) = ahjo_app_kit::theme::chosen();
        let notice = notice.or(unknown);
        cx.set_global(theme);
        field::bind_keys(cx);
        cx.set_app_identity("io.ahjo.desk", "Ahjo Desk");
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds(cx))),
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some("Ahjo Desk".into()),
                    ..Default::default()
                }),
                app_id: Some("ahjo-desk".into()),
                window_min_size: Some(size(px(tokens::WINDOW_MIN.0), px(tokens::WINDOW_MIN.1))),
                ..Default::default()
            },
            |window, cx| {
                // Its own size once zoomed here, else the one chosen in
                // Ahjo Swarm.
                let own = desk::size_path().map(|p| desk::read_size(&p));
                let scale = match &own {
                    Some(Ok(Some(size))) => ahjo_app_kit::zoom::app::scale(*size),
                    _ => ahjo_app_kit::theme::chosen_scale(),
                };
                window.set_ui_scale(scale, cx);
                let notice = match own {
                    Some(Err(e)) => Some(notice.map_or(e.clone(), |n| format!("{n} · {e}"))),
                    _ => notice,
                };
                cx.new(|cx| app::DeskApp::new(config.desks, notice, window, cx))
            },
        );
        if let Err(e) = opened {
            panic!("the window did not open: {e}");
        }
        cx.on_window_closed(|cx, _| cx.quit()).detach();
    });
}
