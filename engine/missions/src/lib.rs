//! Ahjo's mission engine. This crate holds the state directory that stores
//! missions, notes, layouts, agents, the validation that
//! names what stops a run, and the runs. The data it reads, writes and
//! answers with is `ahjo-mission-model`'s.
//!
//! Mission definitions and run state use Ahjo's schema and validation codes.

mod agents;
mod agents_migration;
mod arrange;
mod attempt_limits;
mod authoring;
mod authoring_controls;
mod authoring_create;
mod authoring_delete;
mod authoring_input;
mod authoring_judge;
mod authoring_port;
mod authoring_restore;
mod authoring_staff;
mod authoring_type;
mod authoring_wire;
mod code_binding;
mod code_check;
mod contract_names;
mod contract_outputs;
mod contract_verdict;
mod coordinator;
mod document_migrate;
mod error;
mod examples;
mod executor;
mod folder_snapshot;
mod gate_replay;
mod gate_request;
mod gate_views;
mod graph;
mod harness;
mod human_replay;
mod kept_copy;
mod layout_authoring;
mod layout_placement;
mod ledger;
mod ledger_migrate;
mod limits;
mod links;
mod mission_archives;
mod mission_documents;
mod notes_authoring;
mod notifications;
mod output_refs;
pub mod outputs;
mod port_delivery;
mod run_admission;
mod run_attempts;
mod run_bindings;
mod run_capture;
mod run_delete;
mod run_dispatch;
mod run_escalation;
mod run_evidence;
mod run_folder;
mod run_gate;
mod run_inputs;
mod run_job;
mod run_judges;
mod run_left_seats;
mod run_limit;
mod run_open_dispatch;
mod run_pick;
mod run_projection;
mod run_reattach;
mod run_recovery;
mod run_replay;
mod run_resume;
mod run_scheduler;
mod run_shutdown;
mod run_storage;
mod run_wait;
mod run_worker;
mod run_workspace;
mod run_workspace_recovery;
mod seats;
mod store;
mod turn_worktrees;
mod validate;

mod verdict;
mod workspaces;
mod write;

pub use agents::{AgentEdit, AgentStore, check_agent_id};
pub use ahjo_core::Error;
pub use arrange::arrange;
pub use attempt_limits::{AttemptLimits, TokenMeter};
pub use code_binding::CodeGateBinding;
pub use code_check::{CodeGateEvaluator, GateEvaluator, evaluate_code_check};
pub use contract_outputs::{OutputPayload, parse_outputs};
pub use contract_verdict::{JudgeVerdict, parse_verdict};
pub use coordinator::{AdmissionFailure, Coordinator, OnBlock, RunSettings};
pub use executor::{
    ExecutionEvent, ExecutionSink, Executor, JudgeCorrection, LabExecutor, LeftSeat, OpenDispatch,
    StepExecution, StepResult, UnavailableExecutor,
};
pub use harness::{
    Claude, Codex, HarnessAdapter, Harnesses, Keys, Launch, Pi, Pointer, SeatBinding, Shell,
    TokenUsage, TranscriptRoots, Turn, TurnState, codex_threads, harness_id, harness_word,
};
pub use kept_copy::{Budget, KeptCopy};
pub use layout_placement::free_layout_position;
pub use ledger::{Ledger, RunEvent, validate as validate_ledger};
pub use limits::{LimitUse, LimitWarning};
pub use mission_documents::AppliedDocument;
pub use notifications::{gate_commands, resume_commands, shell_word};
pub use port_delivery::{GateEvidence, GateFeedback, GateResponse, RoutedInput, merge_port_inputs};
pub use run_admission::run_admission;
pub use run_bindings::{BindingsSnapshot, RunBinding};
pub use run_dispatch::{CompletionSentinel, parse_completion_sentinel};
pub use run_escalation::{EscalationSentinel, parse_escalation, project_escalations};
pub use run_inputs::{
    ISSUE_REFERENCE, input_findings, issue_placeholder, mission_run_inputs, render_run_inputs,
    resolve_run_inputs, substitute_run_inputs,
};
pub use run_projection::project_run;
pub use run_wait::{RunWait, WaitUntil};
pub use seats::{
    AgentSeat, Dispatch, DispatchId, DispatchResult, LocalSeats, SeatEvent, SeatExecutor,
    SeatFuture, SeatId, SeatRecords, SeatSide, SeatWait, Submission, SubmissionAction, TurnWatch,
    stage_pointer,
};
pub use store::Store;
pub use turn_worktrees::{PublishedTurn, TurnWorkspace, TurnWorktrees};
pub use validate::validate;
pub use workspaces::{Workspaces, Worktree};

/// Who acted when a request names no one: Ahjo itself, as Archon's
/// `agent:archon` named Archon.
pub const DEFAULT_ACTOR: &str = "agent:ahjo";
