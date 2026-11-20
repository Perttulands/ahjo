//! Typed commands for the harness setup area.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Report this node’s harnesses, skills and checkouts.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Show {}

/// Sync a skills checkout in a terminal session.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Sync {
    /// The checkout path listed by harness show.
    pub checkout: String,
}

/// Link a checkout’s skills into the skills folders missing them.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Link {
    /// The checkout path listed by harness show.
    pub checkout: String,
}

/// Show a folder’s instructions, skills, hooks, settings and servers in the harness that reads them.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadShape {
    /// The folder the agent starts in.
    pub dir: String,
    /// The harness id. Defaults to claude.
    pub harness: Option<String>,
}

/// Read an instruction, skill or settings file and its version.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadFile {
    /// The file path listed by harness shape.
    pub path: String,
}

/// Add a hook to a harness settings file.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddHook {
    /// The harness id, claude or codex.
    pub harness: String,
    /// The file listed by harness shape.
    pub file: String,
    /// The event that triggers the hook.
    pub event: String,
    /// The optional match condition for the event.
    pub matcher: Option<String>,
    /// The command to run.
    pub command: String,
    /// The hook timeout in seconds.
    pub timeout: Option<u64>,
}

/// Change an existing hook.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetHook {
    /// The harness id, claude or codex.
    pub harness: String,
    /// The file listed by harness shape.
    pub file: String,
    /// The hook’s current event.
    pub event: String,
    /// The hook’s current group index.
    pub group: usize,
    /// The hook’s index within its group.
    pub index: usize,
    /// The command the hook ran when it was read.
    pub was: String,
    /// Whether the hook was switched off and kept aside.
    pub parked: Option<bool>,
    /// The replacement event. Omit it to keep the current event.
    pub new_event: Option<String>,
    /// The optional match condition for the event.
    pub new_matcher: Option<String>,
    /// The command to run.
    pub new_command: String,
    /// The hook timeout in seconds.
    pub new_timeout: Option<u64>,
}

/// Remove a hook.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveHook {
    /// The harness id, claude or codex.
    pub harness: String,
    /// The file listed by harness shape.
    pub file: String,
    /// The hook’s current event.
    pub event: String,
    /// The hook’s current group index.
    pub group: usize,
    /// The hook’s index within its group.
    pub index: usize,
    /// The command the hook ran when it was read.
    pub was: String,
    /// Whether the hook was switched off and kept aside.
    pub parked: Option<bool>,
}

/// Switch a hook off or on.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SwitchHook {
    /// The harness id, claude or codex.
    pub harness: String,
    /// The file listed by harness shape.
    pub file: String,
    /// The hook’s current event.
    pub event: String,
    /// The hook’s current group index.
    pub group: usize,
    /// The hook’s index within its group.
    pub index: usize,
    /// The command the hook ran when it was read.
    pub was: String,
    /// Whether the hook was switched off and kept aside.
    pub parked: Option<bool>,
    /// Whether the hook should be on.
    pub on: bool,
}

/// Set or unset a harness setting.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetSetting {
    /// The harness id, claude or codex.
    pub harness: String,
    /// The file listed by harness shape.
    pub file: String,
    /// The dotted setting key.
    pub key: String,
    /// The new value. On the command line, pass JSON or plain words.
    pub value: Option<serde_json::Value>,
    /// Remove this key from the file.
    pub unset: Option<bool>,
}

/// A result from the node named in the receipt.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct AtNode<T> {
    #[serde(flatten)]
    pub result: T,
    pub node: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SyncStarted {
    pub node: String,
    pub session: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct LinksMade {
    pub node: String,
    pub linked: Vec<Linked>,
}

/// A skill and the folder that received a link to it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub struct Linked {
    pub skill: String,
    pub folder: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct HookEdited {
    pub file: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SettingEdited {
    pub file: String,
    pub key: String,
}

impl ahjo_core::TypedCommand for ReadShape {
    const NAME: &'static str = "harness_shape";
    type Output = AtNode<crate::shape::Shape>;
}
impl ahjo_core::TypedCommand for ReadFile {
    const NAME: &'static str = "harness_read";
    type Output = AtNode<crate::shape::Text>;
}
impl ahjo_core::TypedCommand for crate::shape::Write {
    const NAME: &'static str = "harness_write";
    type Output = AtNode<crate::shape::Text>;
}
impl ahjo_core::TypedCommand for AddHook {
    const NAME: &'static str = "harness_hook_add";
    type Output = AtNode<HookEdited>;
}
impl ahjo_core::TypedCommand for SetHook {
    const NAME: &'static str = "harness_hook_set";
    type Output = AtNode<HookEdited>;
}
impl ahjo_core::TypedCommand for RemoveHook {
    const NAME: &'static str = "harness_hook_remove";
    type Output = AtNode<HookEdited>;
}
impl ahjo_core::TypedCommand for SwitchHook {
    const NAME: &'static str = "harness_hook_switch";
    type Output = AtNode<HookEdited>;
}
impl ahjo_core::TypedCommand for SetSetting {
    const NAME: &'static str = "harness_setting_set";
    type Output = AtNode<SettingEdited>;
}
