//! Missions navigator and canvas.
mod page;
pub mod store;
use ahjo_app_kit::{Mounted, mount};
use gpui::App;
pub fn build(host: ahjo_app_kit::Host, window: &mut gpui::Window, cx: &mut App) -> Mounted {
    mount(host, window, cx, page::MissionsPage::new)
}
