//! Mission wire types shared by the node and its clients. This crate does no I/O.

use serde::{Deserialize, Serialize};

pub mod patch;
pub use ahjo_mission_model::*;

pub const WATCH: u8 = 0xa8;
pub const WATCH_RUNS: u8 = 0xa9;
pub const SNAPSHOT: u8 = 0xa0;
pub const RUNS_SNAPSHOT: u8 = 0xa1;
pub const KINDS: ahjo_core::frame::Kinds = &[0xa0..=0xa1, 0xa8..=0xa9];

pub mod error {
    pub const BAD_REQUEST: &str = "bad_request";
    pub const NOT_FOUND: &str = "not_found";
    pub const CONFLICT: &str = "conflict";
    pub const RUN_ADMISSION_FAILED: &str = "run_admission_failed";
    pub const RUN_ERROR: &str = "run_error";
    pub const RUN_FINAL: &str = "run_final";
    pub const NOT_PENDING: &str = "not_pending";
    pub const PRECONDITION_REQUIRED: &str = "precondition_required";
    pub const MISSION_EXISTS: &str = "mission_exists";
    pub const ALREADY_EXISTS: &str = "already_exists";
    pub const DUPLICATE_CONNECTION: &str = "duplicate_connection";
    pub const INPUT_OCCUPIED: &str = "input_occupied";
    pub const INVALID_ISSUE_REF: &str = "invalid_issue_ref";
    pub const INVALID_CODE_GATE_PROFILE: &str = "invalid_code_gate_profile";
    pub const INVALID_CONTROLLER_ROLE: &str = "invalid_controller_role";
    pub const INVALID_DEFINITION: &str = "invalid_definition";
    pub const INVALID_GATE_KIND: &str = "invalid_gate_kind";
    pub const INVALID_LIMIT: &str = "invalid_limit";
    pub const INVALID_MISSION_INPUT: &str = "invalid_mission_input";
    pub const INVALID_MISSION_OUTPUT: &str = "invalid_mission_output";
    pub const INVALID_CARD_RESTORE: &str = "invalid_card_restore";
    pub const INVALID_NOTE_PATCH: &str = "invalid_note_patch";
    pub const INVALID_PORT: &str = "invalid_port";
    pub const INVALID_PORT_DIRECTION: &str = "invalid_port_direction";
    pub const INVALID_SEAT_SETTINGS: &str = "invalid_seat_settings";
    pub const INVALID_TYPE_CHANGE: &str = "invalid_type_change";
    pub const IO: &str = "io";
    pub const NOTE_AUTHOR_MISMATCH: &str = "note_author_mismatch";
    pub const NOTE_TOO_LARGE: &str = "note_too_large";
    pub const RELATIVE_FILE_REFERENCE: &str = "relative_file_reference";
    pub const SELF_WIRE: &str = "self_wire";
    pub const SEAT_CHOICE_REQUIRED: &str = "seat_choice_required";
    pub const UNSUPPORTED_STEP_FORMATION: &str = "unsupported_step_formation";
    pub const UNSUPPORTED_SCHEMA: &str = "unsupported_schema";
    pub const INVALID_DEFINITION_SOURCE: &str = "invalid_definition_source";
}

/// Recover the structured findings carried in a rejected run's core error.
pub fn admission_findings(error: &ahjo_core::Error) -> Option<Vec<Finding>> {
    if error.code != error::RUN_ADMISSION_FAILED {
        return None;
    }
    let json = error.message.strip_prefix("run admission failed: ")?;
    serde_json::from_str(json).ok()
}

/// A watch frame carries exactly one byte; zero turns the subscription off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Watch(pub bool);
impl Watch {
    pub fn encode(self) -> [u8; 1] {
        [u8::from(self.0)]
    }
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        match bytes {
            [value] => Some(Self(*value != 0)),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MissionSnapshot {
    pub missions: Vec<MissionListing>,
}
/// Both the initial snapshot and incremental updates use this payload.
pub type RunsSnapshot = Vec<RunProjection>;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct EditLayout {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arrange: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cards: Option<Vec<LayoutCard>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edges: Option<Vec<LayoutEdge>>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct AgentSeat {
    pub dispatch_id: String,
    pub seat_id: String,
    pub node: String,
    /// The session's name.
    pub session: String,
    /// The backend session id used to open it.
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pane: Option<u32>,
    pub outcome: String,
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub waiting: Option<SeatWait>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SeatWait {
    pub state: String,
    pub detail: String,
    pub since: String,
    pub seq: u64,
}

pub mod commands;
pub mod document;
pub mod document_commands;
pub mod run_commands;
