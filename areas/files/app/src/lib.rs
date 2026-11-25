//! The Files page's state.
pub mod live;
pub mod model;
pub mod open;
pub mod outside;
pub mod store;

mod page;
mod picture;
mod view;
pub use page::FilesPage;

pub fn build(
    host: ahjo_app_kit::Host,
    window: &mut gpui::Window,
    cx: &mut gpui::App,
) -> ahjo_app_kit::Mounted {
    ahjo_app_kit::mount(host, window, cx, |host, window, cx| {
        let mut page = FilesPage::new(host.clone(), window, cx);
        // Files in the window's panel, on the other pages: a Files of its
        // own in one column, the list or the file opened from it.
        let panel = gpui::AppContext::new(cx, |cx| FilesPage::panel(host, window, cx));
        page.panel = Some(panel);
        page
    })
}
