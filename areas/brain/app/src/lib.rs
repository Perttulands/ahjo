//! The brain area's part in Swarm: the Brain panel over the terminals
//! (Alt+L), to search the brain, read a page, follow its links and send it
//! to a session. Reading at length and editing live in Ahjo Brain.
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
