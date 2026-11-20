//! Validation findings: stable codes located on the mission.

use serde::{Deserialize, Serialize};

/// Stable codes for validation findings. Most follow Archon; unsupported
/// capabilities use Ahjo-specific codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Code {
    DanglingConnection,
    DuplicateInputProducer,
    DuplicateCardId,
    DuplicateSeatId,
    EmptyLimit,
    StepWithoutSeats,
    GateNotRoutable,
    InvalidEnd,
    InvalidStepShape,
    InvalidStepFormation,
    InvalidHumanChannel,
    InvalidLimit,
    InvalidMissionInput,
    InvalidMissionOutput,
    InvalidPortKind,
    InvalidSeatId,
    InvalidSeatSettings,
    MissingFile,
    NoCheckout,
    PlacementWithoutRepository,
    MissingInput,
    MissionCount,
    MissionNotRunnable,
    MissionOutputKindMismatch,
    OrchestratedController,
    RelativeFile,
    RetiredPersona,
    RouteLeadsNowhere,
    SeveralInputCards,
    UnavailablePersona,
    UnboundMissionOutput,
    UnknownEnvironment,
    UnknownInputReference,
    UnknownInput,
    UnsupportedStep,
    UnsupportedHumanChannel,
    InvalidInput,
    InputWithoutProducer,
    UnreachableCard,
    UnstaffedSeat,
}

impl Code {
    pub fn as_str(self) -> &'static str {
        match self {
            Code::DanglingConnection => "dangling_connection",
            Code::DuplicateInputProducer => "duplicate_input_producer",
            Code::DuplicateCardId => "duplicate_card_id",
            Code::DuplicateSeatId => "duplicate_seat_id",
            Code::EmptyLimit => "empty_limit",
            Code::StepWithoutSeats => "step_without_seats",
            Code::GateNotRoutable => "gate_not_routable",
            Code::InvalidEnd => "invalid_end",
            Code::InvalidStepShape => "invalid_step_shape",
            Code::InvalidStepFormation => "invalid_step_formation",
            Code::InvalidHumanChannel => "invalid_human_channel",
            Code::InvalidLimit => "invalid_limit",
            Code::InvalidMissionInput => "invalid_mission_input",
            Code::InvalidMissionOutput => "invalid_mission_output",
            Code::InvalidPortKind => "invalid_port_kind",
            Code::InvalidSeatId => "invalid_seat_id",
            Code::InvalidSeatSettings => "invalid_seat_settings",
            Code::MissingFile => "missing_file",
            Code::NoCheckout => "no_checkout",
            Code::PlacementWithoutRepository => "placement_without_repository",
            Code::MissingInput => "missing_input",
            Code::MissionCount => "mission_count",
            Code::MissionNotRunnable => "mission_not_runnable",
            Code::MissionOutputKindMismatch => "mission_output_kind_mismatch",
            Code::OrchestratedController => "orchestrated_controller",
            Code::RelativeFile => "relative_file",
            Code::RetiredPersona => "retired_persona",
            Code::RouteLeadsNowhere => "route_leads_nowhere",
            Code::SeveralInputCards => "several_input_cards",
            Code::UnavailablePersona => "unavailable_persona",
            Code::UnboundMissionOutput => "unbound_mission_output",
            Code::UnknownEnvironment => "unknown_environment",
            Code::UnknownInputReference => "unknown_input_reference",
            Code::UnknownInput => "unknown_input",
            Code::UnsupportedStep => "unsupported_step",
            Code::UnsupportedHumanChannel => "unsupported_human_channel",
            Code::InvalidInput => "invalid_input",
            Code::InputWithoutProducer => "input_without_producer",
            Code::UnreachableCard => "unreachable_card",
            Code::UnstaffedSeat => "unstaffed_seat",
        }
    }
}

impl Serialize for Code {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// One problem, located on the card it is about: a card id, a connection id,
/// or "" for the mission itself.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct Finding {
    pub code: Code,
    pub card_id: String,
    pub message: String,
    /// The reference file a `missing_file` or `relative_file` finding names.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

impl Finding {
    pub fn new(code: Code, card_id: &str, message: String) -> Finding {
        Finding {
            code,
            card_id: card_id.to_owned(),
            message,
            path: None,
        }
    }
}

/// Every problem that would stop a run, and advice that would not.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Report {
    pub errors: Vec<Finding>,
    pub warnings: Vec<Finding>,
}

impl Report {
    /// Orders findings by code, then card, then message, so reports compare.
    pub fn sort(&mut self) {
        for findings in [&mut self.errors, &mut self.warnings] {
            findings.sort_by(|a, b| {
                (a.code.as_str(), &a.card_id, &a.message).cmp(&(
                    b.code.as_str(),
                    &b.card_id,
                    &b.message,
                ))
            });
        }
    }
}
