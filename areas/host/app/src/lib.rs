//! The Host page: is this machine all right now, with its services as part
//! of the answer.
mod chart;
mod host_store;
mod machine;
mod page;
mod service;
mod services_store;
mod tokens;
mod view;

use gpui::{App, Window};
use parvi_app_kit::{Host, Mounted, mount};

pub use page::HostPage;

/// Makes the page; the app lists this in its `PAGES`.
pub fn build(host: Host, window: &mut Window, cx: &mut App) -> Mounted {
    mount(host, window, cx, page::HostPage::new)
}
