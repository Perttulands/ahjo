//! The commands for driving a run and their shared answers.

use ahjo_core::TypedCommand;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    AgentSeat, GateDecision, PublicEvent, ResumeMode, RunAttempts, RunEvidence, RunGate, RunInput,
    RunOutput, RunSendBack, RunSummary,
};

/// List runs, optionally for one mission.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunList {
    pub mission: Option<String>,
}

/// Read why a run is waiting or stopped, the next command, its state and outputs.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunShow {
    pub run: String,
    #[serde(default)]
    pub events: bool,
    #[serde(default)]
    pub seats: bool,
    #[serde(default)]
    pub attempts: bool,
    #[serde(default)]
    pub evidence: bool,
    /// Limit attempts and evidence to a step id.
    pub step: Option<String>,
}

/// Wait for a run to need its driver, end, or change.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunWait {
    pub run: String,
    #[serde(default)]
    pub until: WaitFor,
    /// Count events after this cursor from an earlier answer.
    pub since: Option<u64>,
    /// Seconds to wait at most. Default 3600.
    pub timeout: Option<u64>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum WaitFor {
    #[default]
    NeedsYou,
    Final,
    AnyChange,
}

macro_rules! decision {
    ($(#[$description:meta])* $name:ident $($field:ident: $type:ty),*) => {
        $(#[$description])*
        #[derive(Debug, Deserialize, Serialize, JsonSchema)]
        #[serde(deny_unknown_fields)]
        pub struct $name {
            pub run: String,
            pub gate: String,
            /// The sequence from run show or run wait.
            pub seq: u64,
            $(pub $field: $type,)*
            pub reason: Option<String>,
            pub relayed_by: Option<String>,
            pub actor: Option<String>,
        }
    };
}

decision!(/// Approve a pending human gate.
    RunApprove);
decision!(/// Reject a pending human gate.
    RunReject);
decision!(/// Pick an attempt at a pending human gate.
    RunPick attempt: i64);

/// Abort a run.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunAbort {
    pub run: String,
    pub reason: Option<String>,
    pub actor: Option<String>,
}

/// Resume a blocked run.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunResume {
    pub run: String,
    #[serde(default)]
    pub mode: ResumeMode,
    #[serde(default)]
    pub take_fail: bool,
    #[serde(default)]
    pub grant: bool,
    #[serde(default)]
    pub pick_best: bool,
    pub reason: Option<String>,
    pub actor: Option<String>,
}

/// Land a finished repository run.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunLand {
    pub run: String,
    #[serde(default)]
    pub force: bool,
}

/// Delete a finished run, its outputs and work it kept. Land a repository run first.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunDelete {
    pub run: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RunsListed {
    pub node: String,
    pub runs: Vec<RunSummary>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RunShown {
    pub why: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sent_back: Option<RunSendBack>,
    pub next: Vec<String>,
    pub state: RunState,
    pub outputs: Vec<RunOutput>,
    pub asks: Vec<RunAsk>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gates: Vec<RunGate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub events: Option<Vec<RunLogEvent>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seats: Option<Vec<AgentSeat>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempts: Option<RunAttempts>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence: Option<RunEvidence>,
    pub node: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RunState {
    pub run_id: String,
    pub status: String,
    pub r#final: bool,
    pub resume_allowed: bool,
    pub epoch: i64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub resume_policy: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub block_code: String,
    pub mission_slug: String,
    pub mission_id: String,
    pub mission_rev: i64,
    pub input_card_id: String,
    pub issue: String,
    pub cwd: String,
    pub repository: String,
    pub base: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub request_id: String,
    pub context_paths: Vec<String>,
    pub inputs: Vec<RunInput>,
    pub event_count: usize,
    pub started_by: String,
    pub started_at: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ended_by: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RunLogEvent {
    #[serde(flatten)]
    pub event: PublicEvent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RunWaited {
    pub outcome: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sent_back: Option<RunSendBack>,
    pub cursor: u64,
    pub asks: Vec<RunAsk>,
    pub next: Vec<String>,
    pub outputs: Vec<RunResult>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RunAsk {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gate: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<String>,
    pub what: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    pub commands: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request: Option<GateDecision>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RunResult {
    pub name: String,
    pub kind: String,
    pub value: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct DecisionRecorded {
    pub node: String,
    pub run_id: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RunChanged {
    pub node: String,
    pub run: RunState,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RunLanded {
    pub node: String,
    pub landed: bool,
    pub notes: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RunDeleted {
    pub node: String,
    pub deleted: String,
}

macro_rules! commands {
    ($($request:ident => $name:literal, $output:ty;)*) => {
        $(impl TypedCommand for $request {
            const NAME: &'static str = $name;
            type Output = $output;
        })*
        pub const NAMES: &[&str] = &[$($request::NAME),*];
    }
}

commands! {
    RunList => "run_list", RunsListed;
    RunShow => "run_show", RunShown;
    RunWait => "run_wait", RunWaited;
    RunApprove => "run_approve", DecisionRecorded;
    RunReject => "run_reject", DecisionRecorded;
    RunPick => "run_pick", DecisionRecorded;
    RunResume => "run_resume", RunChanged;
    RunAbort => "run_abort", RunChanged;
    RunLand => "run_land", RunLanded;
    RunDelete => "run_delete", RunDeleted;
}
