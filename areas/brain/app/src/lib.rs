//! Brain page state and reader.
mod page;
mod prose;
pub mod state;
mod tokens;
mod view;

pub use page::BrainPage;

pub fn build(
    host: ahjo_app_kit::Host,
    window: &mut gpui::Window,
    cx: &mut gpui::App,
) -> ahjo_app_kit::Mounted {
    ahjo_app_kit::mount(host, window, cx, BrainPage::new)
}
