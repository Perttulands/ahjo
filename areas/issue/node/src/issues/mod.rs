//! Issues: the work seam. Each configured project names its tracker, a Beads
//! store read with `bd` or a GitHub repository read with `gh api`, and one
//! interface serves the Issues tab, Missions and the other modules
//! (ARCHITECTURE.md: Issues; `docs/work.md`). Ahjo files items through their configured trackers and never
//! handles a token.
//!
//! The public interface: [`Config`] says which projects; [`Issues`] keeps
//! their snapshot current while watched, resolves a written reference to a
//! [`IssueRef`] and reads one [`Item`] with its family; each browser
//! connection holds a [`Channel`].

mod beads;
mod beads_parse;
mod channel;
mod config;
mod github;
mod github_item;
pub use ahjo_issue_contract::item;
mod lookup;
pub use ahjo_issue_contract::model;
mod parse;
mod poller;
mod tree;

pub use beads::Bd;
pub use channel::Channel;
pub use config::{ADDING, Config};
pub use github::Gh;
pub use poller::Issues;

/// An item known to be absent, as opposed to a tracker that failed to answer.
#[derive(Debug)]
pub(crate) struct MissingItem(pub String);
impl std::fmt::Display for MissingItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for MissingItem {}
