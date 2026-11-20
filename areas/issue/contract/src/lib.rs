//! Shared Work wire types and channel constants.

pub mod commands;
pub mod item;
pub mod list;
pub mod model;
mod resolver;
pub mod text;
/// The syntax of a work item's reference, for pages that recognise one in text.
pub use ahjo_issue_ref as issue_ref;
pub use ahjo_issue_ref::IssueRef;
pub use item::{Family, Item};
pub use list::{Line, List, Listed};
pub use model::*;
pub use resolver::{Resolver, Resolving};
pub const WATCH: u8 = 0x98;
pub const SNAPSHOT: u8 = 0x90;
pub const KINDS: ahjo_core::frame::Kinds = &[WATCH..=WATCH];
pub const BAD_REQUEST: &str = "bad_request";
pub const TRACKER_ERROR: &str = "tracker_error";

/// A request to file an item in a configured project.
#[derive(Debug, serde::Deserialize)]
pub struct Create {
    pub project: String,
    pub title: String,
    pub description: String,
    #[serde(rename = "type", default = "default_kind")]
    pub kind: String,
}

fn default_kind() -> String {
    "bug".into()
}
