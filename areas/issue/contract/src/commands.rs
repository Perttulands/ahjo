//! Typed issue commands shared by the node and clients.
use crate::{Item, Line, Listed};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// List tracker items in configured projects.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListIssues {
    /// Include closed items.
    #[serde(default)]
    pub all: bool,
}
#[derive(Debug, Serialize, JsonSchema)]
pub struct IssuesListed {
    pub node: String,
    pub projects: Vec<Listed>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub none: Option<String>,
}
/// Search tracker items by words in their text and metadata.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchIssues {
    pub query: String,
}
#[derive(Debug, Serialize, JsonSchema)]
pub struct IssueMatch {
    pub project: String,
    #[serde(flatten)]
    pub item: Line,
}
#[derive(Debug, Serialize, JsonSchema)]
pub struct IssuesFound {
    pub node: String,
    pub items: Vec<IssueMatch>,
}
/// Show a tracker item and its family.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ShowIssue {
    #[serde(rename = "ref")]
    pub reference: String,
}
#[derive(Debug, Serialize, JsonSchema)]
pub struct IssueShown {
    pub node: String,
    pub item: Item,
    pub line: String,
}
/// File a tracker item in a configured project.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateIssue {
    pub project: String,
    pub title: String,
    pub description: String,
    #[serde(rename = "type", default)]
    pub kind: IssueKind,
}
#[derive(Debug, Default, Deserialize, JsonSchema)]
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
#[derive(Debug, Serialize, JsonSchema)]
pub struct IssueCreated {
    pub reference: String,
}
