//! Read-only records of the problems and files a run left.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::EvidenceText;

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
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
#[serde(rename_all = "snake_case")]
pub struct RunProblem {
    pub seq: u64,
    #[serde(rename = "type")]
    pub kind: String,
    pub card_id: String,
    pub gate_id: String,
    pub code: Option<String>,
    pub reason: EvidenceText,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct RunBrief {
    pub step_id: String,
    pub seat_id: String,
    pub attempt: i64,
    pub dispatch_seq: u64,
    pub path: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct RunArtifact {
    pub step_id: String,
    pub path: String,
    pub size: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct WrittenFile {
    pub path: String,
    pub size: u64,
}
