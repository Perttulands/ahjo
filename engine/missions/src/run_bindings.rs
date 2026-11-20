//! Frozen settings and agent definitions for a run.
use crate::AgentStore;

pub(crate) const BINDINGS_SCHEMA: u8 = 5;
use ahjo_mission_model::{Agent, Mission, Seat};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct BindingsSnapshot {
    pub schema: u8,
    pub run_id: String,
    pub mission_id: String,
    pub mission_slug: String,
    pub mission_rev: i64,
    pub input_card_id: String,
    #[serde(default, rename = "binding")]
    pub bindings: Vec<RunBinding>,
    #[serde(
        default,
        rename = "gate_binding",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub gate_bindings: Vec<crate::CodeGateBinding>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RunBinding {
    pub node_id: String,
    pub seat_id: String,
    pub agent: String,
    pub harness: String,
    pub model: String,
    pub effort: String,
    #[serde(default)]
    pub agent_path: String,
    #[serde(default)]
    pub agent_toml: String,
    #[serde(default)]
    pub agent_sha256: String,
}
impl RunBinding {
    pub(crate) fn agent_definition(&self) -> Result<Option<Agent>, crate::Error> {
        if self.agent.is_empty() {
            return Ok(None);
        }
        if self.agent_toml.is_empty() || hash(&self.agent_toml) != self.agent_sha256 {
            return Err(crate::Error::new(
                "persona_snapshot_invalid",
                "frozen agent definition failed its integrity check",
            ));
        }
        crate::agents::parse_agent(&self.agent, &self.agent_toml).map(Some)
    }

    pub(crate) fn instructions(&self) -> Option<String> {
        self.agent_definition()
            .ok()
            .flatten()
            .map(|agent| agent.instructions)
    }
}

impl BindingsSnapshot {
    pub fn capture(
        run_id: &str,
        mission: &Mission,
        input_card_id: &str,
        agents: &AgentStore,
    ) -> Result<Self, crate::Error> {
        (|| -> Result<Self, String> {
            let mut bindings = Vec::new();
            for step in &mission.steps {
                for seat in &step.seats {
                    if !seat.staffed() {
                        continue;
                    }
                    let agent_toml = if seat.agent.is_empty() {
                        String::new()
                    } else {
                        agents.source(&seat.agent).map_err(|e| e.to_string())?
                    };
                    let agent_sha256 = if agent_toml.is_empty() {
                        String::new()
                    } else {
                        hash(&agent_toml)
                    };
                    let agent = if seat.agent.is_empty() {
                        None
                    } else {
                        Some(
                            crate::agents::parse_agent(&seat.agent, &agent_toml)
                                .map_err(|e| e.to_string())?,
                        )
                    };
                    let settings = seat.settings(agent.as_ref());
                    bindings.push(RunBinding {
                        node_id: step.id.clone(),
                        seat_id: seat.id.clone(),
                        agent: seat.agent.clone(),
                        harness: settings.harness.as_str().to_owned(),
                        model: settings.model,
                        effort: settings.effort.as_str().to_owned(),
                        agent_path: if seat.agent.is_empty() {
                            String::new()
                        } else {
                            agents
                                .dir()
                                .join(format!("{}.toml", seat.agent))
                                .to_string_lossy()
                                .into_owned()
                        },
                        agent_toml,
                        agent_sha256,
                    });
                }
            }
            let gate_bindings = mission
                .gates
                .iter()
                .filter(|gate| gate.kinds.contains(&ahjo_mission_model::GateKind::Code))
                .filter_map(|gate| crate::CodeGateBinding::new(run_id, gate).ok())
                .collect();
            Ok(Self {
                schema: BINDINGS_SCHEMA,
                run_id: run_id.into(),
                mission_id: mission.id.clone(),
                mission_slug: mission.slug.clone(),
                mission_rev: mission.rev,
                input_card_id: input_card_id.into(),
                bindings,
                gate_bindings,
            })
        })()
        .map_err(|message| crate::Error::new("invalid_contract", message))
    }
    pub fn gate_binding(
        &self,
        run_id: &str,
        mission: &Mission,
        gate: &ahjo_mission_model::Gate,
    ) -> Result<&crate::CodeGateBinding, crate::Error> {
        (|| -> Result<&crate::CodeGateBinding, String> {
            if self.schema != BINDINGS_SCHEMA
                || self.run_id != run_id
                || self.mission_id != mission.id
                || self.mission_slug != mission.slug
                || self.mission_rev != mission.rev
            {
                return Err("code gate snapshot identity mismatch".into());
            }
            let mut matches = self
                .gate_bindings
                .iter()
                .filter(|binding| binding.gate_id == gate.id);
            let binding = matches
                .next()
                .ok_or_else(|| "code gate binding missing".to_string())?;
            if matches.next().is_some() {
                return Err("duplicate code gate binding".into());
            }
            binding
                .validate(run_id, gate)
                .map_err(|error| error.message)?;
            Ok(binding)
        })()
        .map_err(|message| crate::Error::new("invalid_contract", message))
    }
    /// A mismatched or altered binding must never be used to launch a seat.
    pub fn binding(
        &self,
        mission: &Mission,
        run_id: &str,
        input_card_id: &str,
        node_id: &str,
        seat: &Seat,
    ) -> Result<&RunBinding, crate::Error> {
        let invalid = || {
            crate::Error::new(
                "persona_snapshot_invalid",
                "persona snapshot does not match the requested seat",
            )
        };
        if self.schema != BINDINGS_SCHEMA
            || self.run_id != run_id
            || self.mission_id != mission.id
            || self.mission_slug != mission.slug
            || self.mission_rev != mission.rev
            || self.input_card_id != input_card_id
        {
            return Err(invalid());
        }
        let mut matches = self
            .bindings
            .iter()
            .filter(|binding| binding.node_id == node_id && binding.seat_id == seat.id);
        let Some(binding) = matches.next() else {
            return Err(invalid());
        };
        let agent = binding.agent_definition().map_err(|_| invalid())?;
        let settings = seat.settings(agent.as_ref());
        if matches.next().is_some()
            || binding.agent != seat.agent
            || binding.harness != settings.harness.as_str()
            || binding.model != settings.model
            || binding.effort != settings.effort.as_str()
            || settings.harness.is_unset()
            || settings.effort.is_unset()
        {
            return Err(invalid());
        }
        Ok(binding)
    }
}
pub(crate) fn hash(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}
