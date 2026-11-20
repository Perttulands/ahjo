//! Harness and effort choices a node offers to authors.

use crate::{Effort, Harness};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HarnessChoices {
    pub id: Harness,
    pub efforts: Vec<Effort>,
    pub default_effort: Effort,
    pub models: Vec<HarnessModel>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HarnessModel {
    pub id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub efforts: Vec<Effort>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EffortPolicy {
    pub effort: Effort,
    pub usage: String,
}
