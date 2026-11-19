//! The Scheduled page: what will run when, and how the last run went. The
//! tasks soonest first in the list; beside them the week across every task
//! (each one's runs over the last day and the next seven, and which two
//! reach one session close together) over the chosen task (Edit, Run now,
//! Pause or Resume, Duplicate, Delete asked twice), or the form (Prompt,
//! Send to, When, Name). Each node keeps its own tasks and sends
//! them into its own sessions, so a task's sessions are all on one node,
//! and the list names the device only when tasks stand on more than one.
//!
//! Read when shown, after a node's change event and once for a node that
//! comes into reach (no timer reads the store); countdowns tick every 30 s.
//! Nothing ticks while the page is not shown.
//!
//! The sessions to send to come from the Sessions page's published read model.

mod form;
mod page;
mod state;
mod tokens;
mod view;
mod week;

use ahjo_app_kit::{Mounted, mount};
use gpui::App;

pub use page::ScheduledPage;

/// The page's id in view control and settings, and the node area it needs.
pub const NAME: &str = "scheduled";

/// The actor the app writes as.
pub const ACTOR: &str = "user:ahjo-swarm";

/// Makes the page; the app lists this in its `PAGES`.
pub fn build(host: ahjo_app_kit::Host, window: &mut gpui::Window, cx: &mut App) -> Mounted {
    mount(host, window, cx, ScheduledPage::new)
}
