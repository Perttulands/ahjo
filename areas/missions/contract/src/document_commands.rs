//! Inputs and outputs of whole-mission authoring commands.
use crate::{DocumentChanges, Report};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Read a mission or example as its stored TOML document.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportMission {
    /// The mission's slug or id.
    #[schemars(extend("x-ahjo-cli-positional" = true))]
    pub mission: Option<String>,
    /// The name of an example from mission examples.
    pub example: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ExportedMission {
    /// The exact TOML text, including its revision.
    #[schemars(extend("x-ahjo-cli-toml" = true))]
    pub document: String,
}

/// Create or replace a mission from its TOML document, or preview the change.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ApplyMission {
    /// TOML text over MCP. On the command line, a local file or - for stdin.
    #[schemars(extend("x-ahjo-cli-file" = true))]
    pub document: String,
    /// Compute the change and validation while leaving files as they are.
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct AppliedMission {
    /// The slug named in the document.
    pub mission: String,
    pub rev: i64,
    pub assigned_ids: Vec<String>,
    pub changed: DocumentChanges,
    pub validation: Report,
    pub next: String,
}
