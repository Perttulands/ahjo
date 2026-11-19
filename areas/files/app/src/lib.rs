//! The Files page's state.
pub mod model;
pub mod open;
pub mod store;

mod page;
mod picture;
mod side;
mod view;
pub use page::FilesPage;

pub fn build(
    host: ahjo_app_kit::Host,
    window: &mut gpui::Window,
    cx: &mut gpui::App,
) -> ahjo_app_kit::Mounted {
    ahjo_app_kit::mount(host, window, cx, |host, window, cx| {
        let mut page = FilesPage::new(host, window, cx);
        page.lends_side = true;
        // Its tree, as the window's Files panel on the other pages.
        let me = cx.entity();
        let side = gpui::AppContext::new(cx, |cx| side::FilesSide::new(&me, cx));
        page.panel_view = Some(side.into());
        page.lend_panel(cx);
        page
    })
}
