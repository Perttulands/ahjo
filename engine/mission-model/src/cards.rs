//! The cards of a mission graph and the wires between them.

use serde::{Deserialize, Serialize};

use crate::{
    Agent, Effort, EndOutcome, Formation, GateKind, Harness, HumanChannel, PortKind, WhenSpent,
};

/// The mission's entry card (`[[input_card]]`), with its `out` port.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields, rename_all = "snake_case")]
pub struct InputCard {
    pub id: String,
    pub title: String,
    pub goal: String,
    /// Reference files, as absolute paths.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    /// What a run's brief should hold, when the mission declares no inputs.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub input_hint: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<MissionInput>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<MissionOutput>,
    /// How to reach the operator. Blank uses notifications.
    #[serde(skip_serializing_if = "HumanChannel::is_unset")]
    pub human_channel: HumanChannel,
}

/// A named value each run supplies. Briefs reference it as `{name}`.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct MissionInput {
    pub name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// What this value carries. Blank uses text.
    pub kind: PortKind,
    pub required: bool,
}

/// A named result a successful run promises, bound to one step output port.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct MissionOutput {
    pub name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// What this value carries. Blank uses text.
    pub kind: PortKind,
    pub required: bool,
    /// The bound step output port, `<step-id>:<port-id>`; blank is a draft.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub from: String,
}

/// A step: the team of agents that does it.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Step {
    pub id: String,
    /// How the seats work together.
    pub formation: Formation,
    pub title: String,
    /// The environment the step runs on; blank runs it on the coordinator.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub environment: String,
    /// Each attempt starts its agents in sessions of their own, ended with
    /// the attempt, instead of using the agents' live sessions.
    #[serde(skip_serializing_if = "is_false")]
    pub fresh: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brief: Option<Brief>,
    #[serde(rename = "input")]
    pub inputs: Vec<Port>,
    #[serde(rename = "output")]
    pub outputs: Vec<Port>,
    #[serde(rename = "seat")]
    pub seats: Vec<Seat>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields, rename_all = "snake_case")]
pub struct Brief {
    pub goal: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub repository: String,
    /// The work item this step serves, when it differs from the run's: a
    /// reference in any written form (`docs/work.md`, References).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub work: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Port {
    pub id: String,
    pub label: String,
    /// What an output port carries: `text` (also absent), `file` or
    /// `folder`. Input ports carry no kind.
    #[serde(skip_serializing_if = "PortKind::is_unset")]
    pub kind: PortKind,
}

impl Port {
    /// The port's kind, `text` when unset.
    pub fn kind(&self) -> PortKind {
        self.kind.effective()
    }

    /// The port's label, or its id when it has none.
    pub fn name(&self) -> &str {
        match self.label.trim() {
            "" => &self.id,
            _ => &self.label,
        }
    }
}

/// A position in a step, owning what its seat runs.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields, rename_all = "snake_case")]
pub struct Seat {
    pub id: String,
    pub label: String,
    /// The reusable agent id. Blank means a plain agent.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub agent: String,
    #[serde(skip_serializing_if = "Harness::is_unset")]
    pub harness: Harness,
    /// Blank inherits the agent model. "default" uses the harness default.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub model: String,
    #[serde(skip_serializing_if = "Effort::is_unset")]
    pub effort: Effort,
    pub controller: bool,
}

impl Seat {
    /// Whether the seat names anything to run: a harness, model, effort or agent.
    pub fn staffed(&self) -> bool {
        !self.agent.is_empty()
            || !self.harness.is_unset()
            || !self.model.is_empty()
            || !self.effort.is_unset()
    }

    /// Resolve each optional override against the reusable agent.
    pub fn settings(&self, agent: Option<&Agent>) -> SeatSettings {
        let harness = if self.harness.is_unset() {
            agent.map_or(Harness::Unset, |agent| agent.harness)
        } else {
            self.harness
        };
        let effort = if self.effort.is_unset() {
            agent.map_or(Effort::Unset, |agent| agent.effort)
        } else {
            self.effort
        };
        let model = match self.model.as_str() {
            "" => agent.map_or_else(String::new, |agent| agent.model.clone()),
            HARNESS_DEFAULT_MODEL => String::new(),
            model => model.to_owned(),
        };
        SeatSettings {
            harness,
            model,
            effort,
        }
    }

    /// The seat as sentences name it: `"Worker" (w1)`, or `"w1"`.
    pub fn name(&self) -> String {
        if self.label.is_empty() {
            format!("{:?}", self.id)
        } else {
            format!("{:?} ({})", self.label, self.id)
        }
    }
}

/// Explicitly choose a harness's default model instead of inheriting one.
pub const HARNESS_DEFAULT_MODEL: &str = "default";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SeatSettings {
    pub harness: Harness,
    pub model: String,
    pub effort: Effort,
}

/// A criterion with one or more kinds. Its ports are `in`, `pass`, `fail`
/// and `judge`.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields, rename_all = "snake_case")]
pub struct Gate {
    pub id: String,
    pub title: String,
    /// Who or what decides.
    pub kinds: Vec<GateKind>,
    /// Free-text intent, never run as a step.
    pub criterion: String,
    /// A code check's profile, its exact version and its parameter.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub check: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub check_version: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub check_value: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
}

pub const GATE_IN: &str = "in";
pub const GATE_PASS: &str = "pass";
pub const GATE_FAIL: &str = "fail";
pub const GATE_JUDGE: &str = "judge";

/// Ends a path on purpose. Its only port is `in`.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct End {
    pub id: String,
    pub title: String,
    /// What this path ends with.
    pub outcome: EndOutcome,
}

pub const END_IN: &str = "in";

/// Caps the rounds, time and tokens of one step, or of the whole mission when
/// it covers the Input card. A knob is `None` when the card does not set it.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields, rename_all = "snake_case")]
pub struct Limit {
    pub id: String,
    pub title: String,
    pub scope: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rounds: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seconds: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warn_seconds: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens: Option<i64>,
    /// What to do at the cap. Blank stops the run.
    #[serde(skip_serializing_if = "WhenSpent::is_unset")]
    pub when_spent: WhenSpent,
}

/// A directed wire between `card-id:port-id` endpoints.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Connection {
    pub id: String,
    pub from: String,
    pub to: String,
}

fn is_false(value: &bool) -> bool {
    !value
}

/// An Input card's only port.
pub const INPUT_CARD_OUT: &str = "out";

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn seat_overrides_are_resolved_independently() {
        let agent = Agent {
            harness: Harness::ClaudeCode,
            model: "opus".into(),
            effort: Effort::High,
            ..Default::default()
        };
        let mut seat = Seat::default();
        assert_eq!(
            seat.settings(Some(&agent)),
            SeatSettings {
                harness: Harness::ClaudeCode,
                model: "opus".into(),
                effort: Effort::High,
            }
        );
        seat.harness = Harness::OpenAiCodex;
        seat.effort = Effort::Ultra;
        assert_eq!(
            seat.settings(Some(&agent)),
            SeatSettings {
                harness: Harness::OpenAiCodex,
                model: "opus".into(),
                effort: Effort::Ultra,
            }
        );
        seat.model = HARNESS_DEFAULT_MODEL.into();
        assert_eq!(seat.settings(Some(&agent)).model, "");
        seat.model = "model".into();
        assert_eq!(seat.settings(Some(&agent)).model, "model");
        assert_eq!(Seat::default().settings(None), SeatSettings::default());
    }
}
