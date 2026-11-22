//! Typed issue commands shared by the node and clients.
use crate::{Item, Line, Listed};
use ahjo_core::TypedCommand;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// List tracker items in configured projects.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListIssues {
    /// Include closed items.
    #[serde(default)]
    pub all: bool,
    /// Only this project: its name as the list shows it, or the last part of
    /// that name (ahjo for /srv/ahjo).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct IssuesListed {
    pub node: String,
    pub projects: Vec<Listed>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub none: Option<String>,
}
/// Search tracker items by words in their text and metadata.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchIssues {
    pub query: String,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct IssueMatch {
    pub project: String,
    #[serde(flatten)]
    pub item: Line,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct IssuesFound {
    pub node: String,
    pub items: Vec<IssueMatch>,
}
/// Show a tracker item and its family.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ShowIssue {
    #[serde(rename = "ref")]
    pub reference: String,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct IssueShown {
    pub node: String,
    pub item: Item,
    pub line: String,
    pub family: Vec<Item>,
}
/// File a tracker item in a configured project.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateIssue {
    /// The project: its name as the list shows it, or the last part of that
    /// name (ahjo for /srv/ahjo).
    pub project: String,
    pub title: String,
    pub description: String,
    #[serde(rename = "type", default)]
    pub kind: IssueKind,
}
#[derive(Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum IssueKind {
    #[default]
    Bug,
    Task,
    Feature,
}
impl IssueKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Bug => "bug",
            Self::Task => "task",
            Self::Feature => "feature",
        }
    }
}
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct IssueCreated {
    pub reference: String,
}

/// Read one item's text from the last snapshot, falling back to its tracker.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SnapshotText {
    pub reference: String,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct SnapshotTextRead {
    pub text: String,
}
/// Search the last snapshot without refreshing it, including its private text.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SnapshotSearch {
    pub query: String,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct SnapshotMatches {
    pub references: Vec<String>,
}
macro_rules! typed {
    ($input:ty, $output:ty, $name:literal, $seconds:expr) => {
        impl TypedCommand for $input {
            const NAME: &'static str = $name;
            const TIMEOUT: Option<Duration> = Some(Duration::from_secs($seconds));
            type Output = $output;
        }
    };
}
typed!(ListIssues, IssuesListed, "issue_list", 35);
typed!(SearchIssues, IssuesFound, "issue_search", 35);
typed!(ShowIssue, IssueShown, "issue_show", 95);
typed!(CreateIssue, IssueCreated, "issue_create", 95);
typed!(SnapshotText, SnapshotTextRead, "issue_snapshot_text", 30);
typed!(SnapshotSearch, SnapshotMatches, "issue_snapshot_search", 30);
