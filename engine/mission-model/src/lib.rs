//! A mission's data as it crosses the wire: the mission definition and its
//! nodes, its layout and notes, authoring requests and results, run
//! requests, a run's public projection, attempts and escalations, and
//! validation findings. It is plain data, so an area's contract may use it
//! (ARCHITECTURE.md, rule 3); the mission engine reads, writes, validates
//! and runs it.

mod agent;
mod authoring;
mod branches;
mod cards;
mod examples;
mod finding;
pub mod flow;
mod harness_choices;
mod layout;
mod mission;
mod notes;
mod run;
mod run_attempts;
mod run_evidence;
mod run_gates;
mod run_projection;
mod sessions;
mod vocabulary;

pub use agent::{AGENT_SCHEMA, Agent, AgentNote, AgentUsage, AgentUse, UnreadableAgent};
pub use authoring::{
    DocumentChanges, Edit, EndUpdate, GateUpdate, InputCardUpdate, LayoutEdit, LayoutPatch,
    LimitUpdate, MissionChange, MissionDeletion, NotePatch, NotesEdit, Precondition, RestoredCard,
};
pub use branches::{kept_branch, run_branch, run_tag};
pub use cards::{
    Brief, Connection, END_IN, End, GATE_FAIL, GATE_IN, GATE_JUDGE, GATE_PASS, Gate,
    HARNESS_DEFAULT_MODEL, INPUT_CARD_OUT, InputCard, Limit, MissionInput, MissionOutput, Port,
    Seat, SeatSettings, Step,
};
pub use examples::{EXAMPLES, Example, example};
pub use finding::{Code, Finding, Report};
pub use harness_choices::{EffortPolicy, HarnessChoices, HarnessModel};
pub use layout::{LAYOUT_SCHEMA, Layout, LayoutCard, LayoutEdge};
pub use mission::{MISSION_SCHEMA, Mission, MissionArchive, MissionListing};
pub use notes::{MISSION_THREAD, NOTES_SCHEMA, NoteEntry, Notes, valid_author};
pub use run::{
    HumanPick, HumanVerdict, ResumeMode, ResumeRequest, RunInput, RunOutput, RunSessionSummary,
    RunStart, RunSummary,
};
pub use run_attempts::{
    AttemptOutput, AttemptPick, AttemptVerdict, EvidenceItem, EvidenceText, RunAttempts,
    StepAttempt, StepAttempts, StepPort,
};
pub use run_evidence::{RunArtifact, RunBrief, RunEvidence, RunProblem, WrittenFile};
pub use run_gates::{
    DecisionInput, DecisionLimit, DecisionRoute, DecisionTarget, DecisionTargetKind, GateDecision,
    GateState, RunGate,
};
pub use run_projection::{
    Escalation, GateInput, GateRequest, PublicEvent, PublicInputRef, PublicOrigin, RunProjection,
    RunSendBack, WaitingGate, plain_reason,
};
pub use sessions::{fresh_session, seat_session, valid_prefix};
pub use vocabulary::{
    Effort, EndOutcome, Formation, GateKind, Harness, HumanChannel, PortKind, WhenSpent,
};
