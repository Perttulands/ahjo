//! A reusable agent and its mission usage.
use crate::{Effort, Harness};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const AGENT_SCHEMA: i64 = 1;

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Agent {
    pub schema: i64,
    pub id: String,
    pub name: String,
    pub instructions: String,
    pub harness: Harness,
    pub model: String,
    pub effort: Effort,
    pub notes: Vec<AgentNote>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct AgentNote {
    pub at: String,
    pub by: String,
    pub text: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct AgentUse {
    pub mission_id: String,
    pub mission_slug: String,
    pub mission_title: String,
    pub step_id: String,
    pub step_title: String,
    pub seat_id: String,
    pub seat_label: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct UnreadableAgent {
    pub name: String,
    pub reason: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct AgentUsage {
    pub usage: Vec<AgentUse>,
    pub complete: bool,
    pub unreadable: Vec<UnreadableAgent>,
}
