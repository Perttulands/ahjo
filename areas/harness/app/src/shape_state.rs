//! What the Agent view knows: the folder and harness asked about, what
//! shapes an agent started there, and the one thing opened from it. Plain
//! data and the readings the view draws; nothing here draws or sends.

pub use ahjo_harness_contract::shape::*;
use serde::{Deserialize, Serialize};

/// The most folders the view remembers.
pub const RECENT: usize = 12;

/// What the page keeps in its section of `app.json`.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct Kept {
    /// Folders looked at here, the last first.
    pub recent: Vec<String>,
    pub harness: String,
}

/// The one thing opened beside the anatomy.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Pick {
    #[default]
    None,
    /// An instruction or skill file, by path.
    File(String),
    /// A hook, by its place in its file.
    Hook(String, HookAt),
    /// A new hook, to go in this file.
    NewHook(String),
    /// A setting, by key and file.
    Setting(String, String),
    /// A new setting, to go in this file.
    NewSetting(String),
}

#[derive(Default)]
pub struct ShapeView {
    pub node: String,
    pub dir: String,
    pub harness: String,
    pub shape: Option<Shape>,
    pub error: Option<String>,
    /// A read of `dir` on its way.
    pub asking: Option<(String, String)>,
    pub pick: Pick,
    /// The opened file's text, as read.
    pub file: Option<Text>,
    pub editing: bool,
    pub saving: bool,
    /// The editor leaves editing at the next draw, after a save.
    pub close_editor: bool,
    /// The skills list shows only those the agent does not see.
    pub show_off_skills: bool,
    /// Where the page has room for one column only, the part picked is
    /// read in place of the anatomy until Back.
    pub reading: bool,
}

impl ShapeView {
    pub fn harness_name(&self) -> &'static str {
        HARNESSES
            .iter()
            .find(|(id, _)| *id == self.harness)
            .map_or("Claude Code", |(_, n)| n)
    }

    pub fn hook(&self) -> Option<&Hook> {
        let Pick::Hook(file, at) = &self.pick else {
            return None;
        };
        self.shape
            .as_ref()?
            .hooks
            .iter()
            .find(|h| &h.file == file && &h.at == at)
    }

    pub fn setting(&self) -> Option<&Setting> {
        let Pick::Setting(key, file) = &self.pick else {
            return None;
        };
        self.shape
            .as_ref()?
            .settings
            .iter()
            .find(|s| &s.key == key && &s.file == file)
    }
}

/// About how many tokens a file of `bytes` costs: four bytes a token.
pub fn tokens(bytes: u64) -> u64 {
    bytes.div_ceil(4)
}

/// A count of tokens as read at a glance: `840`, `9.8k`, `31k`.
pub fn tokens_words(n: u64) -> String {
    match n {
        0..1000 => n.to_string(),
        1000..10_000 => format!("{:.1}k", n as f64 / 1000.),
        _ => format!("{}k", (n + 500) / 1000),
    }
}

/// The instruction tokens an agent reads before its first word: whole
/// files that load at the start, the memory index at most 25 KB.
pub fn start_tokens(shape: &Shape) -> u64 {
    shape
        .instructions
        .iter()
        .filter(|i| i.load == Load::Start)
        .map(|i| {
            let bytes = if i.origin == Origin::Memory {
                i.bytes.min(25 * 1024)
            } else {
                i.bytes
            };
            tokens(bytes)
        })
        .sum()
}

/// A path as the operator reads it: under the project as `./…`, under
/// the home folder as `~/…`.
pub fn short(path: &str, root: Option<&str>, home: &str) -> String {
    // A memory's folder is named for the whole project path: said once.
    if let Some((before, after)) = path.split_once("/.claude/projects/")
        && let Some((_, rest)) = after.split_once('/')
    {
        let shown = short(before, root, home);
        return format!("{shown}/.claude/…/{rest}");
    }
    if let Some(r) = root
        && let Some(rest) = path.strip_prefix(r)
        && let Some(rest) = rest.strip_prefix('/')
    {
        return format!("./{rest}");
    }
    if !home.is_empty()
        && let Some(rest) = path.strip_prefix(home)
    {
        if rest.is_empty() {
            return "~".into();
        }
        if let Some(rest) = rest.strip_prefix('/') {
            return format!("~/{rest}");
        }
    }
    path.to_owned()
}

/// The hooks by event, in the order the harness's events come in a
/// session, then any other events in the files.
pub fn by_event<'a>(harness: &str, hooks: &'a [Hook]) -> Vec<(String, Vec<&'a Hook>)> {
    let mut order: Vec<String> = events(harness).iter().map(|e| (*e).to_owned()).collect();
    for h in hooks {
        if !order.contains(&h.event) {
            order.push(h.event.clone());
        }
    }
    order
        .into_iter()
        .filter_map(|e| {
            let mine: Vec<&Hook> = hooks.iter().filter(|h| h.event == e).collect();
            (!mine.is_empty()).then_some((e, mine))
        })
        .collect()
}

/// What happens when, in words: `before a tool runs`.
pub fn event_words(event: &str) -> &'static str {
    match event {
        "SessionStart" => "when a session starts",
        "SessionEnd" => "when a session ends",
        "UserPromptSubmit" => "when you send a prompt",
        "PreToolUse" => "before a tool runs",
        "PostToolUse" => "after a tool runs",
        "PostToolUseFailure" => "after a tool fails",
        "PermissionRequest" => "when it asks permission",
        "Notification" => "when it notifies you",
        "SubagentStart" => "when a subagent starts",
        "SubagentStop" => "when a subagent stops",
        "Stop" => "when it stops to wait for you",
        "PreCompact" => "before it compacts",
        "PostCompact" => "after it compacts",
        _ => "",
    }
}

/// A setting's value as one line: strings bare, the rest as JSON.
pub fn value_words(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// A skill's or document's text without its front matter, to read.
pub fn without_front_matter(text: &str) -> &str {
    let Some(rest) = text.strip_prefix("---\n") else {
        return text;
    };
    match rest.find("\n---") {
        Some(end) => {
            let after = &rest[end + 4..];
            after.strip_prefix('\n').unwrap_or(after)
        }
        None => text,
    }
}

/// The file the reader opens on when nothing is picked: the instruction
/// file closest to the folder that the harness reads at the start (the
/// project's own, not one it imports), else the first one read.
pub fn opening(shape: &Shape) -> Option<String> {
    let within = shape.root.as_deref().unwrap_or(&shape.dir);
    shape
        .instructions
        .iter()
        .rfind(|i| {
            i.load == Load::Start && i.from.is_none() && i.path.starts_with(&format!("{within}/"))
        })
        .or_else(|| shape.instructions.iter().find(|i| i.load == Load::Start))
        .map(|i| i.path.clone())
}

/// Whether what is picked is part of `shape`: a file it reads or a skill
/// it sees, a hook or setting it has, or a new one for a file it reads.
/// What another harness or folder showed is not, and is not kept open.
pub fn holds(shape: &Shape, pick: &Pick) -> bool {
    let has_file = |path: &str| shape.files.iter().any(|f| f.path == path);
    match pick {
        Pick::None => true,
        Pick::File(path) => {
            shape.instructions.iter().any(|i| &i.path == path)
                || shape.skills.iter().any(|s| &s.path == path)
        }
        Pick::Hook(file, at) => shape.hooks.iter().any(|h| &h.file == file && &h.at == at),
        Pick::Setting(key, file) => shape
            .settings
            .iter()
            .any(|s| &s.key == key && &s.file == file),
        Pick::NewHook(file) | Pick::NewSetting(file) => has_file(file),
    }
}

/// The folders the list offers: running agents' first, then recent ones,
/// each once.
pub fn offered(running: &[(String, String)], recent: &[String]) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for f in running.iter().map(|(f, _)| f).chain(recent) {
        if !out.contains(f) {
            out.push(f.clone());
        }
    }
    out
}

/// `dir` first among the recent folders, at most [`RECENT`] of them.
pub fn remember(recent: &mut Vec<String>, dir: &str) {
    recent.retain(|d| d != dir);
    recent.insert(0, dir.to_owned());
    recent.truncate(RECENT);
}

/// The harness a session runs, from its command: `claude` or `codex`.
pub fn harness_of(command: &str) -> Option<&'static str> {
    let c = command.to_ascii_lowercase();
    if c.contains("claude") {
        Some("claude")
    } else if c.contains("codex") {
        Some("codex")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_read_from_the_project_or_home() {
        assert_eq!(
            short("/srv/p/AGENTS.md", Some("/srv/p"), "/home/u"),
            "./AGENTS.md"
        );
        assert_eq!(
            short("/home/u/.claude/CLAUDE.md", Some("/srv/p"), "/home/u"),
            "~/.claude/CLAUDE.md"
        );
        assert_eq!(
            short("/srv/AGENTS.md", Some("/srv/p"), "/home/u"),
            "/srv/AGENTS.md"
        );
        assert_eq!(short("/home/user2/x", None, "/home/u"), "/home/user2/x");
        assert_eq!(
            short(
                "/home/u/.claude/projects/-srv-p/memory/MEMORY.md",
                Some("/srv/p"),
                "/home/u"
            ),
            "~/.claude/…/memory/MEMORY.md"
        );
    }

    #[test]
    fn tokens_read_at_a_glance() {
        assert_eq!(tokens_words(tokens(3000)), "750");
        assert_eq!(tokens_words(9800), "9.8k");
        assert_eq!(tokens_words(31_400), "31k");
    }

    #[test]
    fn the_reader_opens_on_the_folders_own_instructions() {
        let file = |path: &str, load, from: Option<&str>| Instruction {
            path: path.into(),
            load,
            from: from.map(str::to_owned),
            ..Instruction::default()
        };
        let mut shape = Shape {
            dir: "/w/garden/beds".into(),
            root: Some("/w/garden".into()),
            instructions: vec![
                file("/home/u/.claude/CLAUDE.md", Load::Start, None),
                file("/w/garden/CLAUDE.md", Load::Start, None),
                file(
                    "/w/garden/AGENTS.md",
                    Load::Start,
                    Some("/w/garden/CLAUDE.md"),
                ),
                file("/w/garden/beds/deep/CLAUDE.md", Load::Later, None),
            ],
            ..Shape::default()
        };
        assert_eq!(opening(&shape).as_deref(), Some("/w/garden/CLAUDE.md"));
        shape.instructions.truncate(1);
        assert_eq!(
            opening(&shape).as_deref(),
            Some("/home/u/.claude/CLAUDE.md")
        );
        shape.instructions.clear();
        assert_eq!(opening(&shape), None);
    }

    #[test]
    fn recent_folders_keep_the_last_first_once() {
        let mut r = vec!["/a".to_string(), "/b".to_string()];
        remember(&mut r, "/b");
        assert_eq!(r, ["/b", "/a"]);
        assert_eq!(
            offered(
                &[
                    ("/c".into(), "claude".into()),
                    ("/a".into(), "codex".into())
                ],
                &r
            ),
            ["/c", "/a", "/b"]
        );
    }

    #[test]
    fn hooks_come_in_session_order() {
        let hook = |e: &str| Hook {
            event: e.into(),
            ..Hook::default()
        };
        let hooks = [hook("PreToolUse"), hook("Custom"), hook("SessionStart")];
        let order: Vec<String> = by_event("claude", &hooks)
            .into_iter()
            .map(|(e, _)| e)
            .collect();
        assert_eq!(order, ["SessionStart", "PreToolUse", "Custom"]);
    }
}
