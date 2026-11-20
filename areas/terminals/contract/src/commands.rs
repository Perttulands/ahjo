//! Requests and responses for session, handoff and project commands.

use std::num::{NonZeroU16, NonZeroUsize};

use ahjo_core::NodeId;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// List the sessions on this node with their harness, state, panes and activity.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionList {}

/// Read a pane's screen, or its scrollback and screen with --lines or --all.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionRead {
    /// A session id or name.
    pub session: String,
    /// A pane id. Defaults to the session's active pane.
    pub pane: Option<u32>,
    /// Keep this many of the newest lines, including scrollback.
    pub lines: Option<usize>,
    /// Return the whole scrollback and screen.
    pub all: Option<bool>,
}

/// Paste text into a session's pane and press Enter unless disabled.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionSend {
    /// A session id or name.
    pub session: String,
    /// The text to paste in one go.
    pub text: String,
    /// A pane id. Defaults to the session's active pane.
    pub pane: Option<u32>,
    /// Press Enter after the paste. Defaults to true.
    pub enter: Option<bool>,
}

/// Start a new session running a harness in a folder.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionLaunch {
    /// A harness id from the node's launcher, such as shell, claude or codex.
    pub harness: String,
    /// An absolute folder or one under ~.
    pub folder: String,
    /// Letters, digits, _ and -, at most 50.
    pub name: String,
    /// The whole flag line. Defaults to the harness's configured flags.
    pub flags: Option<String>,
    /// Initial terminal columns. Supply with rows.
    pub cols: Option<NonZeroU16>,
    /// Initial terminal rows. Supply with cols.
    pub rows: Option<NonZeroU16>,
    /// Install hooks that report completion and requests for input.
    pub install_completion_hooks: Option<bool>,
    /// Whose tmux server runs it. Defaults to the node's own.
    pub user: Option<String>,
}

/// Copy a session's harness and folder on the same server under the next free name.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionCopy {
    /// A session id or name.
    pub session: String,
    /// Install completion hooks. Defaults to false.
    pub install_completion_hooks: Option<bool>,
}

/// Search all sessions' scrollback and return matching lines with their session names.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionSearch {
    /// Literal text. Case matters when the query contains a capital letter.
    pub query: String,
}

/// Press named keys or type literal text into a session's pane.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionKeys {
    /// A session id or name.
    pub session: String,
    /// Enter, Tab, Escape, BSpace, Space, arrows, Home, End, PageUp, PageDown, Delete, C-<letter>, or literal text.
    pub keys: Vec<String>,
    /// A pane id. Defaults to the session's active pane.
    pub pane: Option<u32>,
}

/// Rename a session after checking that its tmux server has no session with that name.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionRename {
    /// A session id or name.
    pub session: String,
    /// The new name with letters, digits, _ and -, at most 50.
    pub name: String,
}

/// End a session and everything running in it.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionKill {
    /// A session id or name.
    pub session: String,
}

/// Set mouse mode on all tmux servers this node follows.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionMouse {
    /// On lets tmux scroll and select. Off passes the wheel and clicks to the program.
    pub mouse: MouseMode,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MouseMode {
    On,
    Off,
}

/// Mark a session's last report as seen.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionSeen {
    /// A session id or name.
    pub session: String,
}

/// Find project folders on this node by name or path, best match first.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectFind {
    /// A fragment of the project's name or words in its path.
    pub query: String,
}

/// Start a coding agent in a project with a brief, and wait up to a minute for it to begin.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HandoffStart {
    /// The project's absolute folder or a fragment as project find takes it.
    pub project: String,
    /// The first message. Include the goal, decisions, constraints, what counts as done and links.
    pub brief: String,
    /// A configured agent harness. Defaults to the first agent in the launcher.
    pub harness: Option<String>,
    /// Letters, digits, _ and -. Defaults to a free name based on the folder.
    pub name: Option<String>,
    /// A work item the agent should own, added at the end of the brief.
    pub work: Option<String>,
}

/// Show a handoff session's state, question or last report, and the bottom of its screen.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HandoffStatus {
    /// A session id or name.
    pub session: String,
    /// Wait up to this many seconds while the agent works. Defaults to zero and is capped at 90.
    pub wait: Option<u64>,
    /// How many of the screen's bottom lines to return. Defaults to 40.
    pub lines: Option<NonZeroUsize>,
}

/// Reply to an agent's menu with an option key or send the text as a follow-up.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HandoffReply {
    /// A session id or name.
    pub session: String,
    /// The option key or the text to paste and submit.
    pub text: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SessionsListed {
    pub node: NodeId,
    pub sessions: Vec<SessionSummary>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SessionSummary {
    pub id: String,
    pub name: String,
    pub user: Option<String>,
    pub active: bool,
    /// Seconds since the Unix epoch.
    pub last_output: Option<u64>,
    pub harness: SessionHarness,
    pub state: SessionState,
    pub panes: Vec<PaneSummary>,
    pub report: Option<SessionReport>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SessionHarness {
    ClaudeCode,
    OpenaiCodex,
    Pi,
    None,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SessionState {
    Working,
    NeedsInput,
    Idle,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct PaneSummary {
    pub id: u32,
    pub cols: u16,
    pub rows: u16,
    pub waiting: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SessionReport {
    #[serde(flatten)]
    pub report: HandoffReport,
    pub seen: bool,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SessionReadout {
    pub node: NodeId,
    pub session: String,
    pub pane: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cut: Option<String>,
    pub text: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SessionSent {
    pub node: NodeId,
    pub session: String,
    pub pane: u32,
    pub sent: bool,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SessionLaunched {
    pub node: NodeId,
    pub session: String,
    pub warning: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SessionsSearched {
    pub node: NodeId,
    pub matches: Vec<SessionMatch>,
    pub panes: u32,
    pub truncated: bool,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SessionMatch {
    /// The name of the session holding the pane.
    pub session: String,
    pub pane: u32,
    pub line: u32,
    pub text: String,
    pub before: String,
    pub after: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SessionRenamed {
    pub node: NodeId,
    pub session: String,
    pub name: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SessionKilled {
    pub node: NodeId,
    pub session: String,
    pub killed: bool,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct MouseSet {
    pub node: NodeId,
    pub mouse: MouseMode,
    pub applied: usize,
    pub total: usize,
    pub failures: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SessionSeenResult {
    pub node: NodeId,
    pub session: String,
    pub report: bool,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ProjectsFound {
    pub node: NodeId,
    pub projects: Vec<ProjectSummary>,
    pub more: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub none: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ProjectSummary {
    pub path: String,
    pub name: String,
    pub git: bool,
    pub beads: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum HandoffState {
    Working,
    NeedsInput,
    Finished,
    Idle,
    Gone,
}

impl HandoffState {
    pub fn word(self) -> &'static str {
        match self {
            Self::Working => "working",
            Self::NeedsInput => "needs-input",
            Self::Finished => "finished",
            Self::Idle => "idle",
            Self::Gone => "gone",
        }
    }
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct HandoffReport {
    pub kind: ReportKind,
    pub summary: String,
    pub at: u64,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ReportKind {
    Finished,
    NeedsInput,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct HandoffStatusResult {
    pub node: NodeId,
    pub session: String,
    pub state: HandoffState,
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub detail: Option<HandoffDetail>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub say: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub waited_seconds: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct HandoffDetail {
    pub name: String,
    pub question: Option<String>,
    pub report: Option<HandoffReport>,
    pub last_output_seconds_ago: Option<u64>,
    pub screen: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct HandoffStarted {
    pub node: NodeId,
    pub session: String,
    pub state: HandoffState,
    #[serde(flatten)]
    pub detail: HandoffDetail,
    pub folder: String,
    pub harness: String,
    pub warning: Option<String>,
    pub brief_sent: bool,
    pub say: String,
    pub follow: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct HandoffReplied {
    pub node: NodeId,
    pub session: String,
    pub state: HandoffState,
    #[serde(flatten)]
    pub detail: HandoffDetail,
    pub sent: ReplyKind,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReplyKind {
    Key,
    Text,
}
