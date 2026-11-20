//! Commands shared by the CLI, MCP and app for the reusable agent library.

use ahjo_core::TypedCommand;
use ahjo_mission_model::{
    Agent, AgentNote, AgentUsage, Effort, EffortPolicy, Harness, HarnessChoices, UnreadableAgent,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// List the agents and the harnesses and efforts this node offers.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListAgents {}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ListedAgents {
    pub node: String,
    pub agents: Vec<Agent>,
    pub unreadable: Vec<UnreadableAgent>,
    pub harnesses: Vec<HarnessChoices>,
    pub effort_policy: Vec<EffortPolicy>,
}

/// Show an agent and the missions where it is used.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ShowAgent {
    /// The agent's id.
    pub agent: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ShownAgent {
    pub node: String,
    pub agent: Agent,
    pub etag: String,
    pub usage: AgentUsage,
}

/// Create an agent with its instructions and harness settings.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateAgent {
    /// The agent's id.
    pub id: String,
    pub name: String,
    pub instructions: String,
    pub harness: Harness,
    /// An empty model uses the harness default.
    #[serde(default)]
    pub model: String,
    pub effort: Effort,
}

/// Change an agent's fields. Omitted fields keep their values.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditAgent {
    /// The agent's id.
    pub agent: String,
    /// A version from agent show. Omit to use the current version.
    pub etag: Option<String>,
    pub name: Option<String>,
    pub instructions: Option<String>,
    pub harness: Option<Harness>,
    pub model: Option<String>,
    pub effort: Option<Effort>,
    /// Replace the agent's notes.
    pub notes: Option<Vec<AgentNote>>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct EditedAgent {
    pub node: String,
    pub agent: Agent,
    pub etag: String,
}

/// Archive an agent. Its file can be moved back to restore it.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteAgent {
    /// The agent's id.
    pub agent: String,
    /// A version from agent show. Omit to use the current version.
    pub etag: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct DeletedAgent {
    pub node: String,
    pub agent: String,
}

impl TypedCommand for ListAgents {
    const NAME: &'static str = "agent_list";
    type Output = ListedAgents;
}
impl TypedCommand for ShowAgent {
    const NAME: &'static str = "agent_show";
    type Output = ShownAgent;
}
impl TypedCommand for CreateAgent {
    const NAME: &'static str = "agent_create";
    type Output = EditedAgent;
}
impl TypedCommand for EditAgent {
    const NAME: &'static str = "agent_edit";
    type Output = EditedAgent;
}
impl TypedCommand for DeleteAgent {
    const NAME: &'static str = "agent_delete";
    type Output = DeletedAgent;
}
