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
    let mounted = parvi_app_kit::mount(host.clone(), window, cx, FilesPage::new);
    // Its section of the side panel, on every page, asking as the page.
    let host = host.for_page(mounted.meta.id, mounted.meta.kinds);
    let side = gpui::AppContext::new(cx, |cx| side::FilesSide::new(host.clone(), cx));
    host.lend_side(side.into(), cx);
    mounted
}
