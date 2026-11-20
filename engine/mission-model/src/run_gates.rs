//! A run's gates and the routes a pending operator decision could take.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct RunGate {
    pub gate: String,
    pub title: String,
    #[serde(flatten)]
    pub state: GateState,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(
    tag = "state",
    rename_all = "snake_case",
    rename_all_fields = "snake_case"
)]
pub enum GateState {
    Waiting {
        requested_seq: u64,
    },
    Answered {
        verdict: String,
        by: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        relayed_by: Option<String>,
    },
    NotReached,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct GateDecision {
    pub gate: String,
    pub requested_seq: u64,
    pub criterion: String,
    pub inputs: Vec<DecisionInput>,
    pub routes: Vec<DecisionRoute>,
    pub ends_run: bool,
    pub picks_best: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pick: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct DecisionInput {
    pub from_node_id: String,
    pub from_port_id: String,
    pub text: String,
    pub truncated: bool,
    #[serde(rename = "ref")]
    pub reference: Value,
    pub report_ref: Value,
    pub artifact_ref: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct DecisionRoute {
    pub verdict: String,
    pub targets: Vec<DecisionTarget>,
    pub ends_run: bool,
    pub picks_best: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct DecisionTarget {
    pub node_id: String,
    pub port_id: String,
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub detail: Option<DecisionTargetKind>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "snake_case"
)]
pub enum DecisionTargetKind {
    Step {
        title: String,
        limits: Vec<DecisionLimit>,
        waits_for_inputs: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        picks_best: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        spent_limit: Option<DecisionLimit>,
    },
    End {
        title: String,
        outcome: String,
    },
    Gate {
        title: String,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct DecisionLimit {
    pub kind: String,
    pub limit_id: String,
    pub node_id: String,
    pub used: i64,
    pub max: i64,
    pub left: i64,
    pub granted: i64,
}
