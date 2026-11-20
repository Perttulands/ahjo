//! Authoring requests and results: one edit of a mission, its layout or its
//! notes, the precondition it is checked against, and what it answers.

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

use crate::{
    End, Formation, Gate, InputCard, Layout, LayoutEdge, LayoutNode, Limit, Mission, MissionInput,
    MissionOutput, Notes,
};

#[derive(Clone, Debug, Default)]
pub struct Precondition {
    pub rev: i64,
    pub etag: String,
}

#[derive(Clone, Debug)]
pub struct Edit {
    pub mission: Mission,
    pub etag: String,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize, JsonSchema)]
pub struct DocumentChanges {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub changed: Vec<String>,
}

/// A single board mutation, shared by the engine and the missions contract.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(
    rename_all = "snake_case",
    rename_all_fields = "snake_case",
    deny_unknown_fields
)]
pub enum MissionChange {
    /// Set the title of the mission.
    Title(String),
    /// Set the mission’s session prefix. Blank uses the mission’s slug.
    Prefix(String),
    /// Add an Input card.
    CreateInputCard {
        /// The card’s title.
        #[serde(default, deserialize_with = "default_on_null")]
        title: String,
        /// What the work should accomplish.
        #[serde(default, deserialize_with = "default_on_null")]
        goal: String,
        /// Reference files as absolute paths.
        #[serde(default, deserialize_with = "default_on_null")]
        files: Vec<String>,
        /// How to reach the operator, `notify` (default) or `session`.
        #[serde(default, deserialize_with = "default_on_null")]
        human_channel: String,
        /// Horizontal board position.
        #[serde(default, deserialize_with = "default_on_null")]
        x: i64,
        /// Vertical board position.
        #[serde(default, deserialize_with = "default_on_null")]
        y: i64,
    },
    /// Change the details or declared inputs and outputs of an Input card.
    UpdateInputCard(InputCardUpdate),
    /// Remove an Input card and its wires.
    DeleteInputCard {
        /// The id of the card or wire.
        #[serde(default, deserialize_with = "default_on_null")]
        id: String,
    },
    /// Add a step with seats for its formation type.
    CreateFormation {
        /// `solo`, `peer` or `orchestrated`.
        #[serde(rename = "type")]
        #[serde(default, deserialize_with = "default_on_null")]
        kind: String,
        /// The card’s title.
        #[serde(default, deserialize_with = "default_on_null")]
        title: String,
        /// Horizontal board position.
        #[serde(default, deserialize_with = "default_on_null")]
        x: i64,
        /// Vertical board position.
        #[serde(default, deserialize_with = "default_on_null")]
        y: i64,
    },
    /// Change a step’s title, environment or session reuse.
    UpdateFormation {
        /// The id of the card or wire.
        #[serde(default, deserialize_with = "default_on_null")]
        id: String,
        /// The card’s title.
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        /// Where the step runs. Blank uses the coordinator.
        #[serde(skip_serializing_if = "Option::is_none")]
        device: Option<String>,
        /// Whether each attempt starts fresh sessions.
        #[serde(skip_serializing_if = "Option::is_none")]
        fresh: Option<bool>,
    },
    /// Replace a step’s title and brief.
    EditFormation {
        /// The id of the card or wire.
        #[serde(default, deserialize_with = "default_on_null")]
        id: String,
        /// The card’s title.
        #[serde(default, deserialize_with = "default_on_null")]
        title: String,
        /// Goal, repository, work reference, files and links. Null clears the brief.
        #[serde(skip_serializing_if = "Option::is_none")]
        brief: Option<crate::Brief>,
    },
    /// Change how the seats in a step work together.
    SetFormationType {
        /// The id of the card or wire.
        #[serde(default, deserialize_with = "default_on_null")]
        id: String,
        /// `solo`, `peer` or `orchestrated`.
        #[serde(rename = "type")]
        #[serde(default, deserialize_with = "default_on_null")]
        kind: String,
        /// The seat to keep when changing the formation type.
        #[serde(default, deserialize_with = "default_on_null")]
        keep_slot_id: String,
        /// Replacement seats for the changed formation type.
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        slots: Option<Vec<crate::Slot>>,
    },
    /// Remove a step, its wires and its Limit cards.
    DeleteFormation {
        /// The id of the card or wire.
        #[serde(default, deserialize_with = "default_on_null")]
        id: String,
    },
    /// Set a seat’s role and harness settings.
    AssignSlot {
        /// The step’s id.
        #[serde(default, deserialize_with = "default_on_null")]
        formation_id: String,
        /// The seat’s id.
        #[serde(default, deserialize_with = "default_on_null")]
        slot_id: String,
        /// The seat’s role id.
        #[serde(default, deserialize_with = "default_on_null")]
        agent_id: String,
        /// The harness the seat runs.
        #[serde(default, deserialize_with = "default_on_null")]
        harness: String,
        /// The model the seat uses. Blank uses its harness default.
        #[serde(default, deserialize_with = "default_on_null")]
        model: String,
        /// The seat’s reasoning effort.
        #[serde(default, deserialize_with = "default_on_null")]
        effort: String,
    },
    /// Name the agent in a seat and, by extension, its session.
    NameAgent {
        /// The step’s id.
        #[serde(default, deserialize_with = "default_on_null")]
        formation_id: String,
        /// The seat’s id.
        #[serde(default, deserialize_with = "default_on_null")]
        slot_id: String,
        /// The agent’s name. Blank names its session after the step.
        #[serde(default, deserialize_with = "default_on_null")]
        agent: String,
    },
    /// Swap staffing between two seats.
    SwapSlots {
        /// The source step’s id.
        #[serde(default, deserialize_with = "default_on_null")]
        from_formation: String,
        /// The source seat’s id.
        #[serde(default, deserialize_with = "default_on_null")]
        from_slot: String,
        /// The destination step’s id.
        #[serde(default, deserialize_with = "default_on_null")]
        to_formation: String,
        /// The destination seat’s id.
        #[serde(default, deserialize_with = "default_on_null")]
        to_slot: String,
    },
    /// Make a seat the controller of its step.
    MakeController {
        /// The step’s id.
        #[serde(default, deserialize_with = "default_on_null")]
        formation_id: String,
        /// The seat’s id.
        #[serde(default, deserialize_with = "default_on_null")]
        slot_id: String,
    },
    /// Replace a step’s brief.
    SetBrief {
        /// The step’s id.
        #[serde(default, deserialize_with = "default_on_null")]
        formation_id: String,
        /// What the work should accomplish.
        #[serde(default, deserialize_with = "default_on_null")]
        goal: String,
        /// The repository this step works in.
        #[serde(default, deserialize_with = "default_on_null")]
        repository: String,
        /// The work reference this step serves.
        #[serde(default, deserialize_with = "default_on_null")]
        work: String,
        /// Reference files as absolute paths.
        #[serde(default, deserialize_with = "default_on_null")]
        files: Vec<String>,
        /// Reference links for the step.
        #[serde(default, deserialize_with = "default_on_null")]
        links: Vec<String>,
    },
    /// Remove a step’s brief.
    ClearBrief {
        /// The step’s id.
        #[serde(default, deserialize_with = "default_on_null")]
        formation_id: String,
    },
    /// Add an input or output port to a step.
    AddPort {
        /// The step’s id.
        #[serde(default, deserialize_with = "default_on_null")]
        formation_id: String,
        /// Which side of the step, `input` or `output`.
        #[serde(default, deserialize_with = "default_on_null")]
        direction: String,
        /// The port’s label.
        #[serde(default, deserialize_with = "default_on_null")]
        label: String,
        /// What an output carries, `text`, `file` or `folder`.
        #[serde(default, deserialize_with = "default_on_null")]
        kind: String,
    },
    /// Change a port’s label or kind.
    UpdatePort {
        /// The step’s id.
        #[serde(default, deserialize_with = "default_on_null")]
        formation_id: String,
        /// The port’s id.
        #[serde(default, deserialize_with = "default_on_null")]
        port_id: String,
        /// The port’s label.
        #[serde(skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        /// What an output carries, `text`, `file` or `folder`.
        #[serde(skip_serializing_if = "Option::is_none")]
        kind: Option<String>,
    },
    /// Remove a port and its wires.
    RemovePort {
        /// The step’s id.
        #[serde(default, deserialize_with = "default_on_null")]
        formation_id: String,
        /// The port’s id.
        #[serde(default, deserialize_with = "default_on_null")]
        port_id: String,
    },
    /// Restore a removed port and its wires.
    RestorePort {
        /// The step’s id.
        #[serde(default, deserialize_with = "default_on_null")]
        formation_id: String,
        /// Which side of the step, `input` or `output`.
        #[serde(default, deserialize_with = "default_on_null")]
        direction: String,
        /// The port to restore.
        port: crate::Port,
        /// Where to reinsert the removed item. Omit to append.
        #[serde(
            default,
            deserialize_with = "restore_index",
            skip_serializing_if = "Option::is_none"
        )]
        index: Option<usize>,
        /// Wires to restore with the item.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        connections: Vec<crate::Connection>,
    },
    /// Connect two ports.
    WireConnection {
        /// The source endpoint as `<node-id>:<port-id>`.
        #[serde(default, deserialize_with = "default_on_null")]
        from: String,
        /// The destination endpoint as `<node-id>:<port-id>`.
        #[serde(default, deserialize_with = "default_on_null")]
        to: String,
        /// The id of the card or wire.
        #[serde(default, deserialize_with = "default_on_null")]
        id: String,
        /// Whether to join a wire to an occupied input.
        #[serde(default, deserialize_with = "default_on_null")]
        join_if_occupied: bool,
    },
    /// Disconnect two ports.
    UnwireConnection {
        /// The source endpoint as `<node-id>:<port-id>`.
        #[serde(default, deserialize_with = "default_on_null")]
        from: String,
        /// The destination endpoint as `<node-id>:<port-id>`.
        #[serde(default, deserialize_with = "default_on_null")]
        to: String,
    },
    /// Move an existing wire to another input.
    RewireConnection {
        /// The source endpoint as `<node-id>:<port-id>`.
        #[serde(default, deserialize_with = "default_on_null")]
        from: String,
        /// The former destination endpoint as `<node-id>:<port-id>`.
        #[serde(default, deserialize_with = "default_on_null")]
        previous_to: String,
        /// The destination endpoint as `<node-id>:<port-id>`.
        #[serde(default, deserialize_with = "default_on_null")]
        to: String,
        /// Whether to join a wire to an occupied input.
        #[serde(default, deserialize_with = "default_on_null")]
        join_if_occupied: bool,
        /// Whether to remove the old input port.
        #[serde(default, deserialize_with = "default_on_null")]
        remove_previous_input: bool,
    },
    /// Add a gate to the board.
    CreateGate {
        /// The card’s title.
        #[serde(default, deserialize_with = "default_on_null")]
        title: String,
        /// `code`, `formation` or `human`. Defaults to `human` on creation.
        #[serde(default = "human_gate", deserialize_with = "gate_kinds")]
        kinds: Vec<String>,
        /// What the gate checks.
        #[serde(default, deserialize_with = "default_on_null")]
        criterion: String,
        /// The profile for a code check.
        #[serde(default, deserialize_with = "default_on_null")]
        check: String,
        /// The exact version of the code check.
        #[serde(default, deserialize_with = "default_on_null")]
        check_version: String,
        /// The code check’s parameter.
        #[serde(default, deserialize_with = "default_on_null")]
        check_value: String,
        /// Reference files as absolute paths.
        #[serde(default, deserialize_with = "default_on_null")]
        files: Vec<String>,
        /// Horizontal board position.
        #[serde(default, deserialize_with = "default_on_null")]
        x: i64,
        /// Vertical board position.
        #[serde(default, deserialize_with = "default_on_null")]
        y: i64,
    },
    /// Change a gate’s kinds, criterion or code check.
    UpdateGate(GateUpdate),
    /// Set the chain of steps that judges a gate.
    SetGateJudge {
        /// The gate’s id.
        #[serde(default, deserialize_with = "default_on_null")]
        gate_id: String,
        /// The ids of the judging steps, in order.
        #[serde(default, deserialize_with = "default_on_null")]
        chain: Vec<String>,
    },
    /// Detach a gate’s judge.
    DetachGateJudge {
        /// The gate’s id.
        #[serde(default, deserialize_with = "default_on_null")]
        gate_id: String,
    },
    /// Remove a gate and its wires.
    DeleteGate {
        /// The id of the card or wire.
        #[serde(default, deserialize_with = "default_on_null")]
        id: String,
    },
    /// Add an end card.
    CreateEnd {
        /// The card’s title.
        #[serde(default, deserialize_with = "default_on_null")]
        title: String,
        /// `done` or `rejected`.
        #[serde(default, deserialize_with = "default_on_null")]
        outcome: String,
        /// Horizontal board position.
        #[serde(default, deserialize_with = "default_on_null")]
        x: i64,
        /// Vertical board position.
        #[serde(default, deserialize_with = "default_on_null")]
        y: i64,
    },
    /// Change an end card’s title or outcome.
    UpdateEnd(EndUpdate),
    /// Remove an end card and its wires.
    DeleteEnd {
        /// The id of the card or wire.
        #[serde(default, deserialize_with = "default_on_null")]
        id: String,
    },
    /// Cap a step or mission.
    CreateLimit {
        /// The card’s title.
        #[serde(default, deserialize_with = "default_on_null")]
        title: String,
        /// The id of the step or Input card the limit covers.
        #[serde(default, deserialize_with = "default_on_null")]
        target: String,
        /// The round cap.
        #[serde(skip_serializing_if = "Option::is_none")]
        rounds: Option<i64>,
        /// The time cap in seconds.
        #[serde(skip_serializing_if = "Option::is_none")]
        seconds: Option<i64>,
        /// When to warn before time runs out, in seconds.
        #[serde(skip_serializing_if = "Option::is_none")]
        warn_seconds: Option<i64>,
        /// The token cap.
        #[serde(skip_serializing_if = "Option::is_none")]
        tokens: Option<i64>,
        /// What to do at the cap, `stop`, `best` or `fail`.
        #[serde(default, deserialize_with = "default_on_null")]
        when_spent: String,
        /// Horizontal board position.
        #[serde(default, deserialize_with = "default_on_null")]
        x: i64,
        /// Vertical board position.
        #[serde(default, deserialize_with = "default_on_null")]
        y: i64,
    },
    /// Change the target or caps of a Limit card.
    UpdateLimit(LimitUpdate),
    /// Remove a Limit card.
    DeleteLimit {
        /// The id of the card or wire.
        #[serde(default, deserialize_with = "default_on_null")]
        id: String,
    },
    /// Restore exactly one removed card and its wires.
    RestoreNode {
        /// The card to restore.
        #[serde(flatten)]
        node: RestoredNode,
        /// Wires to restore with the item.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        connections: Vec<crate::Connection>,
        /// Where to reinsert the removed item. Omit to append.
        #[serde(
            default,
            deserialize_with = "restore_index",
            skip_serializing_if = "Option::is_none"
        )]
        index: Option<usize>,
        /// Horizontal board position.
        #[serde(default, deserialize_with = "default_on_null")]
        x: i64,
        /// Vertical board position.
        #[serde(default, deserialize_with = "default_on_null")]
        y: i64,
    },
}

fn default_on_null<'de, D: Deserializer<'de>, T: Deserialize<'de> + Default>(
    deserializer: D,
) -> Result<T, D::Error> {
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

fn some<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(deserializer).map(Some)
}

fn human_gate() -> Vec<String> {
    vec!["human".into()]
}

fn gate_kinds<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<String>, D::Error> {
    let kinds: Vec<String> = default_on_null(deserializer)?;
    Ok(if kinds.is_empty() {
        human_gate()
    } else {
        kinds
    })
}

fn restore_index<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<usize>, D::Error> {
    Ok(Option::<i64>::deserialize(deserializer)?.and_then(|index| index.try_into().ok()))
}

/// An archived mission and the id shared by its archived layout.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct MissionDeletion {
    pub id: String,
    pub slug: String,
    pub title: String,
    pub archive_id: String,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(default, rename_all = "snake_case", deny_unknown_fields)]
pub struct EndUpdate {
    /// The card’s id.
    #[serde(deserialize_with = "default_on_null")]
    pub id: String,
    /// The card’s title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// `done` or `rejected`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(default, rename_all = "snake_case", deny_unknown_fields)]
pub struct LimitUpdate {
    /// The card’s id.
    #[serde(deserialize_with = "default_on_null")]
    pub id: String,
    /// The card’s title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The id of the step or Input card the limit covers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// The round cap.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rounds: Option<i64>,
    /// The time cap in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seconds: Option<i64>,
    /// When to warn before time runs out, in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warn_seconds: Option<i64>,
    /// The token cap.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens: Option<i64>,
    /// What to do at the cap, `stop`, `best` or `fail`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub when_spent: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(default, rename_all = "snake_case", deny_unknown_fields)]
pub struct GateUpdate {
    /// The card’s id.
    #[serde(deserialize_with = "default_on_null")]
    pub id: String,
    /// The card’s title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// `code`, `formation` or `human`. Defaults to `human` on creation.
    #[serde(deserialize_with = "some", skip_serializing_if = "Option::is_none")]
    pub kinds: Option<Vec<String>>,
    /// What the gate checks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub criterion: Option<String>,
    /// The profile for a code check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub check: Option<String>,
    /// The exact version of the code check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub check_version: Option<String>,
    /// The code check’s parameter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub check_value: Option<String>,
    /// Reference files as absolute paths.
    #[serde(deserialize_with = "some", skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<String>>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(default, rename_all = "snake_case", deny_unknown_fields)]
pub struct InputCardUpdate {
    /// The card’s id.
    #[serde(deserialize_with = "default_on_null")]
    pub id: String,
    /// The card’s title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// What the work should accomplish.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub goal: Option<String>,
    /// Reference files as absolute paths.
    #[serde(deserialize_with = "some", skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<String>>,
    /// What to include in the run brief.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_hint: Option<String>,
    /// How to reach the operator, `notify` (default) or `session`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub human_channel: Option<String>,
    /// Inputs declared by the card.
    #[serde(deserialize_with = "some", skip_serializing_if = "Option::is_none")]
    pub inputs: Option<Vec<MissionInput>>,
    /// Outputs declared by the card.
    #[serde(deserialize_with = "some", skip_serializing_if = "Option::is_none")]
    pub outputs: Option<Vec<MissionOutput>>,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case", try_from = "RestoredCards")]
pub enum RestoredNode {
    InputCard(InputCard),
    Formation(Formation),
    Gate(Gate),
    End(End),
    Limit(Limit),
}

// The legacy restore operation allows null placeholders for the other card kinds.
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
struct RestoredCards {
    input_card: Option<InputCard>,
    formation: Option<Formation>,
    gate: Option<Gate>,
    end: Option<End>,
    limit: Option<Limit>,
}

impl TryFrom<RestoredCards> for RestoredNode {
    type Error = &'static str;

    fn try_from(cards: RestoredCards) -> Result<Self, Self::Error> {
        let mut nodes = [
            cards.input_card.map(Self::InputCard),
            cards.formation.map(Self::Formation),
            cards.gate.map(Self::Gate),
            cards.end.map(Self::End),
            cards.limit.map(Self::Limit),
        ]
        .into_iter()
        .flatten();
        match (nodes.next(), nodes.next()) {
            (Some(node), None) => Ok(node),
            _ => Err("restore_node requires exactly one node"),
        }
    }
}

#[derive(Clone, Debug)]
pub enum LayoutPatch {
    Nodes(Vec<LayoutNode>),
    Edges(Vec<LayoutEdge>),
}

#[derive(Clone, Debug)]
pub struct LayoutEdit {
    pub layout: Layout,
    pub etag: String,
}

#[derive(Clone, Debug)]
pub enum NotePatch {
    Append {
        target: String,
        author: String,
        text: String,
    },
    /// Add to the thread the entry `to` is in; `resolves` says this answers
    /// it for good.
    Reply {
        to: String,
        author: String,
        text: String,
        resolves: bool,
    },
    Edit {
        target: String,
        author: String,
        id: String,
        text: String,
    },
    Delete {
        target: String,
        author: String,
        id: String,
    },
}

#[derive(Clone, Debug)]
pub struct NotesEdit {
    pub notes: Notes,
    pub etag: String,
}
