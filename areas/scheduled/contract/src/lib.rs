//! The scheduled area's commands, events and HTTP types. The node and the
//! app share the same task document and request types. HTTP carries
//! [`Envelope`], with errors in the one shape `ahjo_core::Error` defines.

use std::collections::BTreeMap;

pub mod commands;
pub mod schedule;

use jiff::Timestamp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Watch task changes while the Scheduled page is shown.
pub const WATCH: u8 = 0xc8;
/// The node's tasks changed; the page should read them again.
pub const CHANGED: u8 = 0xc0;
pub const KINDS: ahjo_core::frame::Kinds = &[WATCH..=WATCH];

/// The tasks' HTTP root: `GET` lists, `POST` creates; `/{id}` reads,
/// patches and deletes; `POST /{id}/pause`, `/resume` and `/run-now` act.
pub const PATH: &str = "/api/scheduled-tasks";

/// Every mutation carries this header with [`INTENT_VALUE`], CHROTE's guard
/// against a page that posts by accident; a create or patch is also JSON.
pub const INTENT: &str = "x-chrote-intent";
pub const INTENT_VALUE: &str = "scheduled-task";

/// The task does not exist.
pub const NOT_FOUND: &str = "not_found";
/// A request that cannot be kept: a bad task, schedule or JSON.
pub const BAD_REQUEST: &str = "bad_request";
/// A target names a session this node does not have.
pub const TARGET_NOT_FOUND: &str = "target_not_found";
/// The task is being modified or run by another call.
pub const CONFLICT: &str = "conflict";
/// A mutation without [`INTENT`].
pub const FORBIDDEN: &str = "forbidden";
/// A create or patch that is not JSON.
pub const UNSUPPORTED_MEDIA_TYPE: &str = "unsupported_media_type";
/// The node's own failure: a file that cannot be written.
pub const INTERNAL: &str = "internal";

/// What an HTTP route answers: `{success, data, timestamp}`, or
/// `{success: false, error, timestamp}` with the error in the one shape.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Envelope {
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ahjo_core::Error>,
    pub timestamp: String,
}

pub const SUCCESS: &str = "success";
/// How a run refused because its task is held begins; why follows the
/// colon, as the task's `held` says it.
pub const HELD: &str = "nothing was sent and the task is held";
pub const ERROR: &str = "error";
/// Some targets took the prompt and others did not.
pub const PARTIAL: &str = "partial";
/// The receipt a guarded submit stores; it does not claim the program took
/// the prompt.
pub const SUBMIT_KEY_DISPATCHED: &str =
    "pasted; submit key dispatched (application acceptance unconfirmed)";

/// A session by its Unix user and name; sockets are the node's business.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub session_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub unix_user: String,
}

impl Target {
    pub fn label(&self) -> String {
        if self.unix_user.is_empty() {
            self.session_name.clone()
        } else {
            format!("{}/{}", self.unix_user, self.session_name)
        }
    }
}

/// An interval or cron trigger, CHROTE's schema.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Schedule {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub expression: String,
    #[serde(default)]
    pub timezone: String,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub every_minutes: i64,
    /// An interval as a Go duration (`24h`, `1h30m`), as CHROTE's files
    /// may have it instead of `every_minutes`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub duration: String,
}

fn is_zero(n: &i64) -> bool {
    *n == 0
}

/// One target's part of a run.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TargetRun {
    pub session_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub unix_user: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub pane: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub submit_key_dispatched: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunEntry {
    pub id: String,
    pub trigger: String,
    pub started_at: Timestamp,
    pub finished_at: Timestamp,
    pub status: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<TargetRun>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AuditEntry {
    pub at: Timestamp,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub actor: String,
    pub action: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub message: String,
}

/// The persisted task document, also returned by the node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Task {
    pub id: String,
    pub name: String,
    pub prompt: String,
    pub targets: Vec<Target>,
    /// Which session each target meant, by its label, learned when the
    /// target was chosen: its id and the server it was on,
    /// `<session id> <server pid>:<server start>` (see [`tied_session`]).
    /// On the same server a session that has the name but not the id is
    /// another one, and the task is held rather than typed into it. On
    /// another server (tmux restarted, as at a reboot, and ids began
    /// again) the session that has the name is the one meant. Not
    /// CHROTE's; a target without one learns it at its next run.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub session_ids: BTreeMap<String, String>,
    pub schedule: Schedule,
    pub enabled: bool,
    pub paused: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_run: Option<Timestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run: Option<Timestamp>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub last_status: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub created_by: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub updated_by: String,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_runs: Vec<RunEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub audit: Vec<AuditEntry>,
    /// Why the node holds the task back and will not fire it (its run could
    /// not be written ahead or recorded, it was mid-run when the node
    /// stopped, or its file stopped reading) until it is edited or resumed.
    /// Not CHROTE's; said by the API, never written to the file.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub held: String,
    /// A run begun and not yet recorded, written before anything is sent:
    /// found on a task as the node starts, it means the node stopped
    /// mid-run, and the task is held rather than sent again. Not CHROTE's;
    /// CHROTE ignores it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt: Option<Attempt>,
}

impl Task {
    /// Whether the schedule is keeping it.
    pub fn live(&self) -> bool {
        self.enabled && !self.paused
    }
}

/// A run in progress, as written ahead of its delivery.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Attempt {
    pub run_id: String,
    pub trigger: String,
    pub started_at: Timestamp,
}

/// The session id of a [`Task::session_ids`] entry, without its server.
pub fn tied_session(tie: &str) -> &str {
    tie.split_once(' ').map_or(tie, |(session, _)| session)
}

/// Trimmed, without empties or repeats, in the caller's order.
pub fn normalize_targets(targets: impl IntoIterator<Item = Target>) -> Vec<Target> {
    let mut out: Vec<Target> = vec![];
    for t in targets {
        let t = Target {
            session_name: t.session_name.trim().into(),
            unix_user: t.unix_user.trim().into(),
        };
        if !t.session_name.is_empty() && !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

/// A target as a request names it. `socket` is refused: the node resolves
/// sockets itself.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetRequest {
    #[serde(default)]
    pub session_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub unix_user: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub socket: String,
}

/// `POST /api/scheduled-tasks`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateRequest {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub prompt: String,
    /// The retired single target, refused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<serde_json::Value>,
    #[serde(default)]
    pub targets: Vec<TargetRequest>,
    #[serde(default)]
    pub schedule: Schedule,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub paused: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub created_by: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub updated_by: String,
}

/// `PATCH /api/scheduled-tasks/{id}`: what is absent stays.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatchRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    /// The retired single target, refused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub targets: Option<Vec<TargetRequest>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<Schedule>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paused: Option<bool>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub updated_by: String,
}

/// Attribution for a pause, resume or immediate run.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionRequest {
    #[serde(default)]
    pub updated_by: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the API says of a task reaches the app: why it is held, and
    /// which session each target was chosen as.
    #[test]
    fn a_held_task_reads_back_as_held_with_its_sessions() {
        let said = r#"{"id":"tsk_1","name":"n","prompt":"p","targets":[{"session_name":"ops"}],
            "session_ids":{"ops":"$3"},"schedule":{"type":"interval","every_minutes":5},
            "enabled":true,"paused":false,"created_at":"2026-06-27T14:00:00Z",
            "updated_at":"2026-06-27T14:00:00Z","held":"session ops is another session now"}"#;
        let task: Task = serde_json::from_str(said).unwrap();
        assert_eq!(task.held, "session ops is another session now");
        assert_eq!(task.session_ids["ops"], "$3");
    }
}
