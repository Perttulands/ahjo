//! What shapes an agent started in a folder: the instruction files that
//! load, the skills it sees, its hooks, the settings that change how it
//! behaves and the servers that give it tools, each with the file it comes
//! from. Also the edits the page makes to them. HTTP wire data; no I/O.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// `GET ?dir=<folder>&harness=<id>`: a [`Shape`].
pub const PATH: &str = "/api/harness/shape";
/// `GET ?path=` reads a [`Text`]; `PUT` a [`Write`] saves one.
pub const FILE: &str = "/api/harness/file";
/// `POST` a [`HookEdit`]: add, change, remove, switch a hook.
pub const HOOK: &str = "/api/harness/hook";
/// `POST` a [`SettingEdit`]: set or unset one setting.
pub const SETTING: &str = "/api/harness/setting";

/// A file changed on disk since it was read: read it again.
pub const STALE: &str = "harness_stale";
/// A request names something that is not there (a hook, a file, a key).
pub const NOT_FOUND: &str = "harness_not_found";
/// A request that cannot be done as asked.
pub const INVALID: &str = "harness_invalid";

/// The harnesses this page reads, by the ids the tools report uses.
pub const HARNESSES: &[(&str, &str)] = &[("claude", "Claude Code"), ("codex", "Codex")];

/// Where a thing comes from, as the operator thinks of it.
#[derive(
    Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord, JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    /// Set by the machine's administrator (`/etc`).
    Managed,
    /// The operator's own, for every folder (`~/.claude`, `~/.codex`).
    #[default]
    User,
    /// A folder above the project, on the way down to it.
    Ancestor,
    /// The project the folder belongs to: its root down to the folder.
    Project,
    /// The operator's own for this project only, kept out of Git.
    Local,
    /// What the harness wrote down for itself about this project.
    Memory,
    /// Pulled in by another instruction file (`@path`).
    Import,
    /// Shipped with the harness or a plugin.
    Plugin,
}

impl Origin {
    pub fn word(self) -> &'static str {
        match self {
            Origin::Managed => "managed",
            Origin::User => "user",
            Origin::Ancestor => "ancestor",
            Origin::Project => "project",
            Origin::Local => "local",
            Origin::Memory => "memory",
            Origin::Import => "import",
            Origin::Plugin => "plugin",
        }
    }
}

/// Whether a thing reaches the agent, and when.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Load {
    /// At the start, before the first word.
    #[default]
    Start,
    /// Later, when the agent touches a file it applies to.
    Later,
    /// Present but kept out (excluded, switched off, shadowed, over a cap).
    Off,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, JsonSchema)]
#[serde(default)]
pub struct Shape {
    /// The folder, as given and as the node resolved it.
    pub dir: String,
    pub harness: String,
    /// The project root the harness finds for the folder, if any.
    pub root: Option<String>,
    pub home: String,
    pub instructions: Vec<Instruction>,
    pub skills: Vec<SeenSkill>,
    pub hooks: Vec<Hook>,
    pub settings: Vec<Setting>,
    pub servers: Vec<Server>,
    /// The files read for hooks and settings, present or not: where a new
    /// hook or setting can go.
    pub files: Vec<Source>,
    /// What the node could not read or is not sure of, in words.
    pub notes: Vec<String>,
}

/// An instruction file, in the order the harness reads them.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, JsonSchema)]
#[serde(default)]
pub struct Instruction {
    /// The path the harness reads.
    pub path: String,
    /// Where a link leads, when the path is one: what an edit changes.
    pub target: Option<String>,
    pub origin: Origin,
    pub load: Load,
    pub bytes: u64,
    pub lines: u32,
    /// Why it loads later or not at all, or what it is part of.
    pub why: Option<String>,
    /// The file that imports it, for an import.
    pub from: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, JsonSchema)]
#[serde(default)]
pub struct SeenSkill {
    pub name: String,
    pub description: String,
    /// Its `SKILL.md`.
    pub path: String,
    /// Where a linked skill's folder leads.
    pub target: Option<String>,
    pub origin: Origin,
    pub load: Load,
    pub why: Option<String>,
}

/// One hook: when `event` happens (and `matcher` matches), run `command`.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, JsonSchema)]
#[serde(default)]
pub struct Hook {
    pub event: String,
    pub matcher: Option<String>,
    /// `command`, `prompt`, `http`, …: what the harness runs.
    pub kind: String,
    /// The command, prompt or URL.
    pub run: String,
    pub timeout: Option<u64>,
    pub status: Option<String>,
    pub file: String,
    pub origin: Origin,
    pub on: bool,
    /// What the operator should know: it waits to be trusted, say.
    pub note: Option<String>,
    pub at: HookAt,
}

/// Where a hook sits in its file: the event, its group and its place.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq, JsonSchema)]
#[serde(default)]
pub struct HookAt {
    pub event: String,
    pub group: usize,
    pub index: usize,
    /// Switched off: kept aside by Ahjo (`index` in its list), for a
    /// harness that has no switch of its own.
    pub parked: bool,
}

/// A setting as one file sets it.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, JsonSchema)]
#[serde(default)]
pub struct Setting {
    /// Its key, dotted: `permissions.deny`, `model`.
    pub key: String,
    pub value: serde_json::Value,
    pub file: String,
    pub origin: Origin,
    /// A file read later sets it again, and wins.
    pub overridden: bool,
}

/// A server that gives the agent tools (MCP).
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, JsonSchema)]
#[serde(default)]
pub struct Server {
    pub name: String,
    /// Its command line, or its URL.
    pub run: String,
    pub file: String,
    pub origin: Origin,
    pub on: bool,
    pub why: Option<String>,
}

/// A file the harness reads hooks or settings from.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, JsonSchema)]
#[serde(default)]
pub struct Source {
    pub path: String,
    pub origin: Origin,
    pub present: bool,
    /// Hooks can be added here.
    pub hooks: bool,
}

/// A file's text and its version, as read.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, JsonSchema)]
#[serde(default)]
pub struct Text {
    pub path: String,
    /// Where the bytes are, when the path is a link.
    pub target: Option<String>,
    pub text: String,
    /// What a write must name to replace this text; empty for a file that
    /// is not there yet.
    pub etag: String,
}

/// Replace a file’s text if its version still matches.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Write {
    /// The file to save.
    pub path: String,
    /// The complete replacement text.
    pub text: String,
    /// The version returned by read. Use an empty string to create a file.
    pub etag: String,
}

/// A hook as the page writes it.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, JsonSchema)]
#[serde(default)]
pub struct HookSpec {
    pub event: String,
    pub matcher: Option<String>,
    pub command: String,
    pub timeout: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum HookChange {
    Add {
        hook: HookSpec,
    },
    /// `was` is what the hook ran when it was read: a hook that has moved
    /// or changed since is not touched.
    Set {
        at: HookAt,
        was: String,
        hook: HookSpec,
    },
    Remove {
        at: HookAt,
        was: String,
    },
    /// Off keeps the hook where Ahjo can put it back.
    Switch {
        at: HookAt,
        was: String,
        on: bool,
    },
}

/// A change to the hooks in one settings file of one harness.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, JsonSchema)]
pub struct HookEdit {
    pub harness: String,
    pub file: String,
    pub change: HookChange,
}

/// Set `key` in `file` to `value`, or take it out with `null`.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, JsonSchema)]
#[serde(default)]
pub struct SettingEdit {
    pub harness: String,
    pub file: String,
    pub key: String,
    pub value: serde_json::Value,
}

/// What a hook can run on, for each harness: the events it names.
pub fn events(harness: &str) -> &'static [&'static str] {
    match harness {
        "codex" => &[
            "SessionStart",
            "UserPromptSubmit",
            "PreModelRequest",
            "PostModelRequest",
            "PreToolUse",
            "PermissionRequest",
            "PostToolUse",
            "SubagentStart",
            "SubagentStop",
            "Stop",
            "Interrupt",
            "PreCompact",
            "PostCompact",
            "SessionEnd",
        ],
        _ => &[
            "SessionStart",
            "UserPromptSubmit",
            "PreToolUse",
            "PermissionRequest",
            "PostToolUse",
            "PostToolUseFailure",
            "Notification",
            "SubagentStart",
            "SubagentStop",
            "Stop",
            "PreCompact",
            "SessionEnd",
        ],
    }
}
