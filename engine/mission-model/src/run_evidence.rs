//! Read-only records of the problems and files a run left.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::EvidenceText;

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RunEvidence {
    pub run_id: String,
    pub problems: Vec<RunProblem>,
    pub briefs: Vec<RunBrief>,
    pub files: Vec<RunArtifact>,
    #[serde(default)]
    pub written: Vec<WrittenFile>,
    #[serde(default)]
    pub truncated: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RunProblem {
    pub seq: u64,
    #[serde(rename = "type")]
    pub kind: String,
    pub node_id: String,
    pub gate_id: String,
    pub code: Option<String>,
    pub reason: EvidenceText,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RunBrief {
    pub node_id: String,
    pub slot_id: String,
    pub attempt: i64,
    pub dispatch_seq: u64,
    pub path: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RunArtifact {
    pub node_id: String,
    pub path: String,
    pub size: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct WrittenFile {
    pub path: String,
    pub size: u64,
}
