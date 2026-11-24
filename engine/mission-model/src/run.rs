//! Run requests and summaries: starting, resuming and answering a run, and
//! the values it is given and gives back.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "snake_case")]
pub struct RunStart {
    pub mission: String,
    pub input_card_id: String,
    pub step_id: String,
    pub expected_rev: i64,
    pub expected_etag: String,
    pub cwd: String,
    pub repository: String,
    pub base: String,
    pub context_paths: Vec<String>,
    /// Leave this run's seat sessions and their working directories available.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub keep_sessions: bool,
    /// The run's issue as a canonical reference (`docs/issues.md`), or
    /// empty. The node resolves what was written before admission.
    pub issue: String,
    pub actor: String,
    pub inputs: BTreeMap<String, String>,
    /// The client's key for this start. A start repeated with a key that
    /// already made a run answers with that run instead of making another,
    /// so a client that lost the answer can safely send it again.
    pub request_id: String,
    /// An earlier run whose `step_id` this run starts again as it started
    /// there: the input that step got, from the commit it started at, with
    /// that run's inputs, issue, repository, base and context paths. The
    /// mission is as it is now, so what was changed since is what differs.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub rerun_of: String,
    /// Known machine names, including machines that are offline. A step on an
    /// unknown machine is refused. An offline environment can still be selected.
    #[serde(skip)]
    pub known_machines: Option<Vec<String>>,
}

/// A resume. A field the node does not know is refused, not dropped, so a
/// client never believes it asked for something the node ignored.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "snake_case")]
pub struct ResumeRequest {
    pub mode: ResumeMode,
    pub take_fail: bool,
    /// Give the Limit card a spent limit stopped one more allowance: a
    /// round, or the card's time or tokens again.
    pub grant: bool,
    /// Pick the best attempt the spent step's gate judged, and go on.
    pub pick_best: bool,
    pub reason: String,
    pub actor: String,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ResumeMode {
    #[default]
    Reattach,
    Redispatch,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct RunSummary {
    pub run_id: String,
    pub number: usize,
    pub status: String,
    pub r#final: bool,
    pub mission_slug: String,
    pub mission_id: String,
    pub mission_rev: i64,
    pub input_card_id: String,
    pub issue: String,
    pub event_count: usize,
    pub resume_allowed: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub resume_policy: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub block_code: String,
    pub started_by: String,
    pub started_at: String,
    pub updated_at: String,
    pub input_excerpt: String,
    pub outputs: Vec<crate::RunOutput>,
    pub sessions: Vec<RunSessionSummary>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct RunSessionSummary {
    pub session_name: String,
    pub step_id: String,
    pub seat_id: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "snake_case")]
pub struct HumanVerdict {
    pub requested_seq: u64,
    pub verdict: String,
    pub reason: String,
    pub relayed_by: String,
    pub actor: String,
}

/// The operator's pick on a gate's pending pick.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "snake_case")]
pub struct HumanPick {
    /// The exact pending request picked on.
    pub requested_seq: u64,
    /// The picked attempt's step, needed where several steps feed the gate.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub step: String,
    pub attempt: i64,
    pub reason: String,
    /// The seat ID of the seat that typed a pick the actor made.
    pub relayed_by: String,
    pub actor: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RunInput {
    pub name: String,
    pub kind: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct RunOutput {
    pub name: String,
    pub kind: String,
    pub value: String,
    pub card_id: String,
    pub port_id: String,
    pub attempt: i64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub picks: Vec<serde_json::Value>,
}
