//! Request and response types for the scheduled commands.

use ahjo_core::NodeId;
use jiff::Timestamp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{RunEntry, Schedule, Task};

/// List the scheduled prompts and when they run.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScheduledList {}

/// Show a scheduled prompt in full, with its recent runs and audit.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScheduledShow {
    /// The scheduled prompt’s id, or its name if unique.
    pub id: String,
}

/// Schedule a prompt into sessions, every N minutes or on a cron.
///
/// Example: ahjo scheduled create nudge 'Check the build' ops build --every 60
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScheduledCreate {
    /// The name of the scheduled prompt.
    pub name: String,
    /// Pasted into each session, then Enter.
    pub prompt: String,
    /// Session names, user/name for another Unix user's. On the command line, give each after the prompt.
    pub sessions: Vec<String>,
    /// Fire every this many minutes.
    #[schemars(range(min = 1))]
    pub every: Option<i64>,
    /// Five cron fields, instead of every.
    pub cron: Option<String>,
    /// IANA zone for cron. Defaults to the node's zone.
    pub timezone: Option<String>,
    /// Start paused. Defaults to false.
    pub paused: Option<bool>,
}

/// Change the given fields of a scheduled prompt.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScheduledEdit {
    /// The scheduled prompt’s id, or its name if unique.
    pub id: String,
    /// The new name of the scheduled prompt.
    pub name: Option<String>,
    /// Pasted into each session, then Enter.
    pub prompt: Option<String>,
    /// Replacement session names, user/name for another Unix user's. Repeat --sessions for each.
    pub sessions: Option<Vec<String>>,
    /// Fire every this many minutes.
    #[schemars(range(min = 1))]
    pub every: Option<i64>,
    /// Five cron fields, instead of every.
    pub cron: Option<String>,
    /// IANA zone for cron. Defaults to the node's zone.
    pub timezone: Option<String>,
}

/// Duplicate a scheduled prompt and start the copy paused.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScheduledCopy {
    /// The scheduled prompt’s id, or its name if unique.
    pub id: String,
    /// The copy's name. Defaults to the original name with (copy) appended.
    pub name: Option<String>,
}

/// Pause a scheduled prompt.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScheduledPause {
    /// The scheduled prompt’s id, or its name if unique.
    pub id: String,
}

/// Resume a paused or held scheduled prompt.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScheduledResume {
    /// The scheduled prompt’s id, or its name if unique.
    pub id: String,
}

/// Send a scheduled prompt now, whatever its schedule.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScheduledRun {
    /// The scheduled prompt’s id, or its name if unique.
    pub id: String,
}

/// Delete a scheduled prompt.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScheduledDelete {
    /// The scheduled prompt’s id, or its name if unique.
    pub id: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ScheduledListed {
    #[schemars(with = "String")]
    pub node: NodeId,
    pub scheduled_prompts: Vec<ScheduledSummary>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ScheduledSummary {
    pub id: String,
    pub name: String,
    pub sessions: Vec<String>,
    pub schedule: Schedule,
    pub paused: bool,
    /// Why the node is holding the prompt, or an empty string.
    pub held: String,
    pub next_run: Option<Timestamp>,
    pub last_status: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ScheduledPromptResult {
    #[schemars(with = "String")]
    pub node: NodeId,
    pub scheduled_prompt: Task,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ScheduledRan {
    pub scheduled_prompt: Task,
    pub run: RunEntry,
    #[schemars(with = "String")]
    pub node: NodeId,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ScheduledDeleted {
    pub deleted: String,
    #[schemars(with = "String")]
    pub node: NodeId,
}
