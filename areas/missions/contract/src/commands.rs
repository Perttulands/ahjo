//! The mission authoring, layout and start commands shared by the clients.

use std::collections::BTreeMap;

use ahjo_core::TypedCommand;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::document_commands::{AppliedMission, ApplyMission, ExportMission, ExportedMission};
use crate::{
    EditLayout, End, Finding, Gate, InputCard, Layout, Limit, Mission, MissionArchive,
    MissionDeletion, MissionListing, Step, document::NotesDocument, patch::EditRequest,
};

/// List the missions on this node.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MissionList {}

/// List configured repositories for mission runs.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MissionRepositories {}

/// List mission archives, including partially removed archives.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MissionArchives {}

/// Permanently remove one mission archive. Run history remains.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MissionPurge {
    pub slug: String,
    pub archive_id: String,
}

/// Create an empty mission, a rough sketch, or a copy of an example.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MissionCreate {
    pub slug: String,
    pub title: Option<String>,
    /// Copy a staffed, wired example from mission examples.
    pub example: Option<String>,
    /// Rough steps, one per line or separated by semicolons, chained from Input to Done.
    pub steps: Option<String>,
    /// What the mission is for, kept as a note.
    pub note: Option<String>,
    pub updated_by: Option<String>,
}

/// List the examples a new mission can start from.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MissionExamples {}

/// Read a mission's intent, steps in run order, staffing, and what stops a run.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MissionShow {
    pub mission: String,
    /// Read the whole definition with every port and seat id.
    #[serde(default)]
    pub full: bool,
}

/// Say whether a mission can run, with its errors, warnings and next command.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MissionValidate {
    pub mission: String,
}

/// Apply one mission authoring operation or a batch in one revision.
#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MissionEdit {
    pub mission: String,
    pub change: EditRequest,
    /// The mission revision as last read. A mission changed since is refused.
    pub rev: Option<i64>,
    pub etag: Option<String>,
}

/// Read every note thread, grouped by what it is on.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MissionNotes {
    pub mission: String,
}

/// Write a note on a mission, card or wire, or change a note of your own.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MissionNote {
    pub mission: String,
    /// What the note says. It may span several lines.
    pub text: Option<String>,
    /// A card id or title, a wire as From->To, or the mission when omitted.
    pub on: Option<String>,
    /// Change the note with this id to --text.
    pub edit: Option<String>,
    /// Remove the note with this id.
    pub delete: Option<String>,
    /// Your name, e.g. claude-7 (agent:claude-7).
    pub author: Option<String>,
    pub etag: Option<String>,
}

/// Answer a note in its thread.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MissionReply {
    pub mission: String,
    pub note: String,
    pub text: String,
    pub author: Option<String>,
    pub etag: Option<String>,
}

/// Close a note's thread with what you did about it. A later note reopens it.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MissionResolve {
    pub mission: String,
    pub note: String,
    pub text: String,
    pub author: Option<String>,
    pub etag: Option<String>,
}

/// Read a mission's canvas layout.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LayoutShow {
    pub mission: String,
}

/// Change or arrange a mission's canvas layout.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LayoutEdit {
    pub mission: String,
    pub change: EditLayout,
    pub etag: Option<String>,
}

/// Arrange the mission board from its flow.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MissionArrange {
    pub mission: String,
}

/// Archive the definition, layout and notes. Run history remains.
/// Use mission archives to see them and mission purge to remove one.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MissionDelete {
    pub mission: String,
    pub rev: Option<i64>,
    pub etag: Option<String>,
}

/// Start a mission at an Input card or at a particular step.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MissionRun {
    pub mission: String,
    /// Start at this step (id or unique title).
    pub from: Option<String>,
    /// Select an Input card by id or title. Use --inputs for values.
    pub input: Option<String>,
    /// The input values as JSON, e.g. '{"topic":"fog"}'.
    #[serde(default)]
    pub inputs: BTreeMap<String, String>,
    /// Run folder. Omit it for a fresh folder.
    pub cwd: Option<String>,
    pub issue: Option<String>,
    /// A configured repository. The run works in its own branch and worktree.
    pub repository: Option<String>,
    /// The branch the run starts from.
    pub base: Option<String>,
    #[serde(default)]
    pub context_paths: Vec<String>,
    pub actor: Option<String>,
    /// Return the existing run when retrying this start with the same key.
    pub request_id: Option<String>,
    pub expected_rev: Option<i64>,
    pub expected_etag: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct MissionsListed {
    pub node: String,
    pub missions: Vec<MissionListing>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct MissionRepositoriesListed {
    pub node: String,
    pub repositories: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct MissionArchivesListed {
    pub node: String,
    pub archives: Vec<MissionArchive>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct MissionPurged {
    pub node: String,
    pub removed: MissionArchive,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct MissionCreated {
    pub node: String,
    pub created: String,
    pub id: String,
    pub title: String,
    pub rev: i64,
    pub steps: Vec<SketchStep>,
    pub next: [String; 2],
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SketchStep {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct MissionExamplesListed {
    pub node: String,
    pub examples: Vec<MissionExample>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct MissionExample {
    pub name: String,
    pub title: String,
    pub about: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum MissionShown {
    Full(Box<MissionDefinition>),
    Summary(Box<MissionHandover>),
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct MissionDefinition {
    pub node: String,
    pub mission: Mission,
    pub etag: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct MissionHandover {
    pub mission: String,
    pub title: String,
    pub rev: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed: Option<String>,
    pub notes: String,
    pub about: Vec<HandoverNote>,
    pub input: Vec<HandoverInput>,
    pub steps: Vec<HandoverStep>,
    pub limits: Vec<HandoverLimit>,
    pub ends: Vec<HandoverEnd>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub wires: Vec<HandoverWire>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed: Vec<RemovedThread>,
    pub runs: String,
    pub problems: Vec<String>,
    pub next: [String; 4],
    pub node: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct HandoverNote {
    pub id: String,
    pub by: String,
    pub at: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub resolves: bool,
    pub text: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct HandoverInput {
    pub id: String,
    pub title: String,
    pub next: String,
    pub asks: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gives: Option<String>,
    pub goal: String,
    pub notes: Vec<HandoverNote>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct HandoverStep {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n: Option<usize>,
    pub id: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unreached: bool,
    #[serde(flatten)]
    pub detail: HandoverStepDetail,
    pub notes: Vec<HandoverNote>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum HandoverStepDetail {
    Step {
        #[serde(flatten)]
        worker: HandoverWorker,
        #[serde(skip_serializing_if = "Option::is_none")]
        next: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        alongside: Option<String>,
    },
    Gate {
        gate: String,
        decided: String,
        pass: String,
        fail: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        check: Option<String>,
        checks: String,
        judges: Vec<HandoverJudge>,
    },
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct HandoverWorker {
    pub step: String,
    pub who: String,
    pub seats: Vec<HandoverSeat>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fresh: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brief: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct HandoverSeat {
    pub id: String,
    pub agent: String,
    pub harness: crate::Harness,
    pub model: String,
    pub effort: crate::Effort,
    /// The base session name. Fresh attempts receive their own suffix.
    pub session: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct HandoverJudge {
    pub id: String,
    #[serde(flatten)]
    pub worker: HandoverWorker,
    pub notes: Vec<HandoverNote>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct HandoverLimit {
    pub id: String,
    pub on: String,
    pub caps: String,
    pub then: String,
    pub notes: Vec<HandoverNote>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct HandoverEnd {
    pub id: String,
    pub end: String,
    pub outcome: String,
    pub notes: Vec<HandoverNote>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct HandoverWire {
    pub id: String,
    pub wire: String,
    pub notes: Vec<HandoverNote>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RemovedThread {
    pub on: String,
    pub notes: Vec<HandoverNote>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct MissionValidated {
    pub node: String,
    pub valid: bool,
    pub summary: String,
    pub mission_rev: i64,
    pub mission_etag: String,
    pub errors: Vec<Finding>,
    pub warnings: Vec<Finding>,
    pub next: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct MissionEdited {
    pub layout: Layout,
    pub layout_etag: String,
    #[serde(flatten)]
    pub definition: MissionDefinition,
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub added: Option<AddedCard>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AddedCard {
    InputCard(InputCard),
    Step(Step),
    Gate(Gate),
    End(End),
    Limit(Limit),
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct MissionNotesListed {
    pub node: String,
    pub notes: NotesDocument,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct MissionNoteReceipt {
    pub node: String,
    pub document: NotesDocument,
    #[serde(flatten)]
    pub change: NoteChange,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum NoteChange {
    Written {
        note: String,
        on: String,
        resolved: bool,
        thread: Vec<NoteRow>,
    },
    Changed {
        notes: String,
        rev: i64,
    },
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct NoteRow {
    pub id: String,
    pub by: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub resolves: bool,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct LayoutShown {
    pub node: String,
    pub layout: Layout,
    pub etag: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct MissionDeleted {
    pub node: String,
    pub deletion: MissionDeletion,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct MissionStarted {
    pub node: String,
    pub run_id: String,
}

// Match the command names used by the node, CLI and MCP.
macro_rules! commands {
    ($($request:ident => $name:literal, $output:ty;)*) => {
        $(impl TypedCommand for $request {
            const NAME: &'static str = $name;
            type Output = $output;
        })*

        pub const NAMES: &[&str] = &[$($request::NAME,)* MissionRun::NAME];
    };
}

commands! {
    MissionList => "mission_list", MissionsListed;
    MissionRepositories => "mission_repositories", MissionRepositoriesListed;
    MissionArchives => "mission_archives", MissionArchivesListed;
    MissionPurge => "mission_purge", MissionPurged;
    MissionShow => "mission_show", MissionShown;
    MissionValidate => "mission_validate", MissionValidated;
    MissionCreate => "mission_create", MissionCreated;
    MissionExamples => "mission_examples", MissionExamplesListed;
    ExportMission => "mission_export", ExportedMission;
    ApplyMission => "mission_apply", AppliedMission;
    MissionEdit => "mission_edit", MissionEdited;
    MissionNotes => "mission_notes", MissionNotesListed;
    MissionNote => "mission_note", MissionNoteReceipt;
    MissionReply => "mission_reply", MissionNoteReceipt;
    MissionResolve => "mission_resolve", MissionNoteReceipt;
    LayoutShow => "layout_show", LayoutShown;
    LayoutEdit => "layout_edit", LayoutShown;
    MissionArrange => "mission_arrange", LayoutShown;
    MissionDelete => "mission_delete", MissionDeleted;
}

impl TypedCommand for MissionRun {
    const NAME: &'static str = "mission_run";
    const TIMEOUT: Option<std::time::Duration> = Some(std::time::Duration::from_secs(60));
    type Output = MissionStarted;
}
