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
    host: parvi_app_kit::Host,
    window: &mut gpui::Window,
    cx: &mut gpui::App,
) -> parvi_app_kit::Mounted {
    parvi_app_kit::mount(host, window, cx, |host, window, cx| {
        let mut page = FilesPage::new(host, window, cx);
        page.lends_side = true;
        // Its tree, as its section of the side panel on the other pages.
        let me = cx.entity();
        let side = gpui::AppContext::new(cx, |cx| side::FilesSide::new(&me, cx));
        page.host
            .lend_side(side.into(), parvi_app_kit::hub::SideOn::OtherPages, cx);
        page
    })
}
