//! A run's attempts (Archon's run attempts evidence): for each step, every
//! attempt with what it output, the verdicts that judged it and the pick
//! that took it.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// One served text's cap (Archon's `EvidenceTextMaxBytes`).
const TEXT_MAX: usize = 64 << 10;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct EvidenceText {
    pub text: String,
    pub bytes: usize,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub truncated: bool,
}

impl EvidenceText {
    /// The text as served: capped at a run's served text size.
    pub fn of(text: &str) -> Self {
        let mut t = Self {
            text: text.to_owned(),
            bytes: text.len(),
            truncated: false,
        };
        t.cap(TEXT_MAX);
        t
    }
    /// Cuts at a character boundary so the text stays valid UTF-8.
    pub fn cap(&mut self, max: usize) {
        if self.text.len() > max {
            let end = self.text.floor_char_boundary(max);
            self.text.truncate(end);
            self.truncated = true;
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct RunAttempts {
    pub run_id: String,
    pub steps: Vec<StepAttempts>,
}

/// One step and its attempts, oldest first.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct StepAttempts {
    pub step_id: String,
    pub title: String,
    /// The gates this step judges for, when it is a judge.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub judge_of: Vec<String>,
    pub outputs: Vec<StepPort>,
    pub attempts: Vec<StepAttempt>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct StepPort {
    pub id: String,
    pub label: String,
    pub kind: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct StepAttempt {
    pub attempt: i64,
    /// `running`, `done` or `stopped`.
    pub status: String,
    pub started_seq: u64,
    pub started_at: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ended_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<EvidenceText>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub output_seq: u64,
    pub outputs: Vec<AttemptOutput>,
    pub verdicts: Vec<AttemptVerdict>,
    /// The pick that took this attempt as its step's best.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub picked: Option<AttemptPick>,
    /// Who sat in each of the step's seats for this attempt and what it
    /// spent, in the order the seats were first sent work.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub seats: Vec<AttemptSeat>,
    /// From start to end, once the attempt ended.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub wall_ms: u64,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

/// One seat's part in an attempt: the agent, harness, model, effort and
/// session it ran on, the tokens its finished turns spent and how long it
/// worked. Every turn the seat was sent in the attempt counts.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct AttemptSeat {
    pub seat_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub agent: String,
    pub harness: String,
    /// The model exactly as the harness's transcript names it, which a seat
    /// set to the harness default learns only here; empty until a turn
    /// finished.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub model: String,
    /// The effort the transcript names, or the one the seat was started
    /// with where the harness does not record it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub effort: String,
    /// The tmux session the seat's agent worked in.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub session: String,
    /// The harness's own id for the conversation, which resumes it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub harness_session: String,
    /// Summed over the seat's finished turns; absent until one finished.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens: Option<AttemptTokens>,
    /// From the first turn sent to the last one finished.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub wall_ms: u64,
}

/// Tokens as the harness counted them, raw: fresh input, input written to
/// the prompt cache, input read from it, and output.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct AttemptTokens {
    pub input: i64,
    pub cache_write: i64,
    pub cache_read: i64,
    pub output: i64,
}

/// One output port's accepted payload; `path` is a file or folder output's
/// absolute path.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct AttemptOutput {
    pub port_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
    pub kind: String,
    pub text: EvidenceText,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub path: String,
}

/// One gate evaluation of the attempt's output and the verdict it reached,
/// or `pending` while it waits. `superseded` marks a human request a later
/// evaluation replaced before anyone answered it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct AttemptVerdict {
    pub gate_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub gate_title: String,
    pub evaluation_seq: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub seq: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub verdict: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pending: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub superseded: bool,
    pub reason: EvidenceText,
    pub evidence: Vec<EvidenceItem>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub decided_by: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct EvidenceItem {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub kind: String,
    pub text: EvidenceText,
}

/// The pick that took an attempt: its gate, the verdict's seq, why and who,
/// the attempts it chose from and what started it (`when_spent` or `resume`).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct AttemptPick {
    pub gate_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub gate_title: String,
    pub seq: u64,
    pub reason: EvidenceText,
    pub decided_by: String,
    pub candidates: Vec<i64>,
    pub trigger: String,
}
