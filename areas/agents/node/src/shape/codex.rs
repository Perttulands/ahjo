//! What Codex reads, from its source at `rust-v0.169.0` (the installed
//! release; `codex-rs/core/src/agents_md.rs`, `config/src/loader`,
//! `ext/skills/src/loader`, `hooks/src/engine/discovery.rs`).
//!
//! Instructions: `~/.codex/AGENTS.override.md`, or `AGENTS.md` when the
//! override is missing or blank; then, from the project root (the nearest
//! folder up holding a `project_root_markers` entry, `.git` by default) down
//! to the folder, one file per folder: `AGENTS.override.md`, `AGENTS.md` or
//! the first of `project_doc_fallback_filenames`. Project files share
//! `project_doc_max_bytes` (32 KiB): the one that crosses it is cut, later
//! ones are left out. A project marked `untrusted` gets none. With
//! `features.memories` on, `memories/memory_summary.md` (about 2,500
//! tokens of it).
//!
//! Configuration: `/etc/codex/config.toml`, `~/.codex/config.toml`, then
//! each `.codex/config.toml` from the project root down, only where the
//! project is trusted; later wins. Hooks come from `hooks.json` beside each
//! of those and from their `[hooks]`; `[hooks.state]` in the user's file
//! switches one off and holds the hash it was trusted at.

use super::{Roots, ancestors, edit, instruction, shown, source};
use ahjo_agents_contract::shape::{
    Hook, HookChange, HookEdit, INVALID, Instruction, Load, Origin, STALE, SeenSkill, Server,
    Setting, Shape, Source,
};
use ahjo_core::Error;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const DOC_BUDGET: u64 = 32 * 1024;
const SCAN_DEPTH: usize = 6;
const DEFAULT_TIMEOUT: u64 = 600;
const DEFAULT_CONTEXT_LIMIT: u64 = 2500;

fn codex_home(roots: &Roots) -> PathBuf {
    roots.home.dir.join(".codex")
}

/// A TOML file as JSON; none when it is not there, a note when it does
/// not read.
fn toml_file(path: &Path, notes: &mut Vec<String>) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    match toml::from_str::<toml::Value>(&text) {
        Ok(v) => serde_json::to_value(v).ok(),
        Err(e) => {
            notes.push(format!("{} does not read as TOML: {e}", path.display()));
            None
        }
    }
}

/// A configuration layer: its file, where it comes from, its value, and
/// whether Codex uses it (an untrusted project's is read but not used).
struct Layer {
    file: Source,
    value: Value,
    on: bool,
}

/// The project root: the nearest folder at or above `dir` with a marker.
/// A `.git` folder counts only with a `HEAD` in it.
fn project_root(dir: &Path, markers: &[String]) -> Option<PathBuf> {
    dir.ancestors()
        .find(|d| {
            markers.iter().any(|m| {
                let p = d.join(m);
                if m == ".git" && p.is_dir() {
                    p.join("HEAD").exists()
                } else {
                    p.exists()
                }
            })
        })
        .map(Path::to_owned)
}

fn strings(v: &Value) -> Option<Vec<String>> {
    v.as_array().map(|a| {
        a.iter()
            .filter_map(|x| x.as_str().map(str::to_owned))
            .collect()
    })
}

/// The trust the user gave the folder, its project root or the main
/// repository: `trusted`, `untrusted` or none.
fn trust(user: &Value, keys: &[&Path]) -> Option<String> {
    keys.iter().find_map(|k| {
        let canon = k.canonicalize().unwrap_or_else(|_| k.to_path_buf());
        [shown(&canon), shown(k)].iter().find_map(|key| {
            user["projects"][key]["trust_level"]
                .as_str()
                .map(str::to_owned)
        })
    })
}

pub fn read(roots: &Roots, dir: &Path) -> Shape {
    let mut shape = Shape::default();
    let home = codex_home(roots);
    let mut notes = vec![];
    let system_file = roots.codex_system.join("config.toml");
    let user_file = home.join("config.toml");
    let system = toml_file(&system_file, &mut notes);
    let user = toml_file(&user_file, &mut notes).unwrap_or(json!({}));
    let markers = strings(&user["project_root_markers"])
        .or_else(|| {
            system
                .as_ref()
                .and_then(|s| strings(&s["project_root_markers"]))
        })
        .unwrap_or_else(|| vec![".git".into()]);
    let root = project_root(dir, &markers);
    shape.root = root.as_ref().map(|r| shown(r));
    let main = root.as_deref().map(super::main_root);
    let trust_keys: Vec<&Path> = [Some(dir), root.as_deref(), main.as_deref()]
        .into_iter()
        .flatten()
        .collect();
    let trust = trust(&user, &trust_keys);
    let trusted = trust.as_deref() == Some("trusted");

    let mut layers = vec![];
    if let Some(v) = system {
        layers.push(Layer {
            file: source(&system_file, Origin::Managed, false),
            value: v,
            on: true,
        });
    }
    layers.push(Layer {
        file: source(&user_file, Origin::User, true),
        value: user.clone(),
        on: true,
    });
    let project_dirs: Vec<PathBuf> = match &root {
        Some(r) => ancestors(dir)
            .into_iter()
            .filter(|d| d.starts_with(r))
            .collect(),
        None => vec![dir.to_owned()],
    };
    for d in &project_dirs {
        let f = d.join(".codex/config.toml");
        if d.join(".codex") == home {
            continue;
        }
        if let Some(v) = toml_file(&f, &mut notes) {
            layers.push(Layer {
                file: source(&f, Origin::Project, true),
                value: v,
                on: trusted,
            });
        }
    }
    if !trusted && layers.iter().any(|l| l.file.origin == Origin::Project) {
        notes.push(
            "The project is not trusted: its .codex/config.toml and hooks are not used".into(),
        );
    }
    // Where hooks and settings can go: the user's files, and the folder's
    // own project files.
    shape.files = vec![
        source(&user_file, Origin::User, false),
        source(&home.join("hooks.json"), Origin::User, true),
        source(&dir.join(".codex/config.toml"), Origin::Project, false),
        source(&dir.join(".codex/hooks.json"), Origin::Project, true),
    ];
    let effective = |key: &[&str]| -> Option<Value> {
        layers
            .iter()
            .filter(|l| l.on)
            .filter_map(|l| {
                let v = key.iter().fold(&l.value, |v, k| &v[*k]);
                (!v.is_null()).then(|| v.clone())
            })
            .next_back()
    };

    shape.instructions = instructions(roots, dir, &project_dirs, &trust, &effective);
    shape.skills = skills(roots, &project_dirs, &user, &effective);
    shape.hooks = hooks(roots, &layers, &user, &mut notes);
    if effective(&["features", "hooks"]) == Some(Value::Bool(false))
        || effective(&["features", "codex_hooks"]) == Some(Value::Bool(false))
    {
        for h in &mut shape.hooks {
            h.on = false;
        }
        notes.push("features.hooks is off: no hook runs".into());
    }
    shape.settings = settings(&layers, dir, trust.as_deref());
    shape.servers = servers(&layers);
    if user["plugins"]
        .as_object()
        .is_some_and(|p| p.values().any(|v| v["enabled"] != Value::Bool(false)))
    {
        notes.push("Plugins are on: their skills, hooks and servers are not shown".into());
    }
    shape.notes = notes;
    shape
}

fn instructions(
    roots: &Roots,
    dir: &Path,
    project_dirs: &[PathBuf],
    trust: &Option<String>,
    effective: &dyn Fn(&[&str]) -> Option<Value>,
) -> Vec<Instruction> {
    let home = codex_home(roots);
    let mut out = vec![];
    // The user's file: the override when it says something.
    let blank = |p: &Path| {
        std::fs::read_to_string(p)
            .map(|t| t.trim().is_empty())
            .unwrap_or(true)
    };
    let over = home.join("AGENTS.override.md");
    let main = home.join("AGENTS.md");
    let override_wins = over.is_file() && !blank(&over);
    if let Some(mut i) = instruction(&over, Origin::User, Load::Start) {
        if !override_wins {
            i.load = Load::Off;
            i.why = Some("blank, so AGENTS.md is read".into());
        }
        out.push(i);
    }
    if let Some(mut i) = instruction(&main, Origin::User, Load::Start) {
        if override_wins {
            i.load = Load::Off;
            i.why = Some("AGENTS.override.md is read instead".into());
        } else if blank(&main) {
            i.load = Load::Off;
            i.why = Some("blank".into());
        }
        out.push(i);
    }
    if let Some(Value::String(file)) = effective(&["model_instructions_file"])
        && let Some(mut i) = instruction(Path::new(&file), Origin::User, Load::Start)
    {
        i.why = Some("model_instructions_file: in place of the model's own".into());
        out.push(i);
    }
    // The project's files, one per folder, sharing a budget.
    let fallbacks: Vec<String> = effective(&["project_doc_fallback_filenames"])
        .and_then(|v| strings(&v))
        .unwrap_or_default()
        .into_iter()
        .filter(|n| !n.contains('/') && !n.contains('\0') && n != "." && n != "..")
        .collect();
    let budget = effective(&["project_doc_max_bytes"])
        .and_then(|v| v.as_u64())
        .unwrap_or(DOC_BUDGET);
    let untrusted = trust.as_deref() == Some("untrusted");
    let mut used = 0u64;
    for d in project_dirs {
        let names = ["AGENTS.override.md".to_owned(), "AGENTS.md".to_owned()]
            .into_iter()
            .chain(fallbacks.iter().cloned());
        let Some(path) = names.map(|n| d.join(n)).find(|p| p.is_file()) else {
            continue;
        };
        let Some(mut i) = instruction(&path, Origin::Project, Load::Start) else {
            continue;
        };
        let file_name = path.file_name().unwrap_or_default().to_string_lossy();
        if untrusted {
            i.load = Load::Off;
            i.why = Some("the project is marked untrusted".into());
        } else if budget == 0 {
            i.load = Load::Off;
            i.why = Some("project_doc_max_bytes is 0".into());
        } else if blank(&path) {
            i.load = Load::Off;
            i.why = Some(if file_name == "AGENTS.override.md" {
                "blank, and AGENTS.md here is not read".into()
            } else {
                "blank".into()
            });
        } else if used >= budget {
            i.load = Load::Off;
            i.why = Some(format!(
                "past the {} KiB the project's files share",
                budget / 1024
            ));
        } else {
            if used + i.bytes > budget {
                i.why = Some(format!(
                    "cut after {} bytes: the project's files share {} KiB",
                    budget - used,
                    budget / 1024
                ));
            } else if file_name != "AGENTS.md" {
                i.why = Some(format!("{file_name} stands in for AGENTS.md here"));
            }
            used += i.bytes;
        }
        if d.as_path() == dir && i.origin == Origin::Project {
            i.origin = Origin::Project;
        }
        out.push(i);
    }
    // Memories.
    let memories_on = effective(&["features", "memories"]) == Some(Value::Bool(true))
        || effective(&["features", "memory_tool"]) == Some(Value::Bool(true));
    let use_them = effective(&["memories", "use_memories"]) != Some(Value::Bool(false));
    let folder = if effective(&["memories", "version"]).and_then(|v| v.as_str().map(str::to_owned))
        == Some("v2".into())
    {
        "memories_v2"
    } else {
        "memories"
    };
    if let Some(mut i) = instruction(
        &home.join(folder).join("memory_summary.md"),
        Origin::Memory,
        Load::Start,
    ) {
        if memories_on && use_them {
            i.why = Some("about 2,500 tokens of it".into());
        } else {
            i.load = Load::Off;
            i.why = Some(if memories_on {
                "memories.use_memories is off".into()
            } else {
                "features.memories is off".into()
            });
        }
        out.push(i);
    }
    out
}

/// Every `SKILL.md` under `folder`, at most six folders down, leaving out
/// hidden folders below it.
fn scan(folder: &Path, origin: Origin, follow: bool) -> Vec<SeenSkill> {
    let mut out = vec![];
    let mut dirs = vec![(folder.to_owned(), 0usize)];
    let mut seen = 0usize;
    while let Some((d, depth)) = dirs.pop() {
        seen += 1;
        if seen > 2000 {
            break;
        }
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries.iter().rev() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let is_link = e.file_type().is_ok_and(|t| t.is_symlink());
            if e.path().is_dir() && (follow || !is_link) && depth + 1 < SCAN_DEPTH {
                dirs.push((e.path(), depth + 1));
            }
        }
        if d != folder && d.join("SKILL.md").is_file() {
            let parent = d.parent().map(Path::to_owned).unwrap_or_default();
            if let Some(mut s) = super::skills_in(&parent, origin, "")
                .into_iter()
                .find(|s| Path::new(&s.path) == d.join("SKILL.md"))
            {
                s.why = None;
                if let Ok(rest) = d.strip_prefix(folder)
                    && rest.components().count() > 1
                {
                    s.why = Some(format!("found in {}", rest.display()));
                }
                out.push(s);
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn skills(
    roots: &Roots,
    project_dirs: &[PathBuf],
    user: &Value,
    effective: &dyn Fn(&[&str]) -> Option<Value>,
) -> Vec<SeenSkill> {
    let home = codex_home(roots);
    let mut all = vec![];
    for d in project_dirs.iter().rev() {
        let f = d.join(".codex");
        if f != home {
            all.extend(scan(&f.join("skills"), Origin::Project, true));
        }
    }
    all.extend(scan(&home.join("skills"), Origin::User, true));
    all.extend(scan(
        &roots.home.dir.join(".agents/skills"),
        Origin::User,
        true,
    ));
    if effective(&["skills", "bundled", "enabled"]) != Some(Value::Bool(false)) {
        let mut bundled = scan(&home.join("skills/.system"), Origin::Plugin, false);
        for s in &mut bundled {
            s.why = Some("comes with Codex".into());
        }
        all.extend(bundled);
    }
    all.extend(scan(
        &roots.codex_system.join("skills"),
        Origin::Managed,
        true,
    ));
    for d in project_dirs {
        all.extend(scan(&d.join(".agents/skills"), Origin::Project, true));
    }
    // The same files reached two ways are one skill.
    let mut real_seen: Vec<PathBuf> = vec![];
    all.retain(|s| {
        let real = Path::new(&s.path)
            .canonicalize()
            .unwrap_or_else(|_| PathBuf::from(&s.path));
        if real_seen.contains(&real) {
            false
        } else {
            real_seen.push(real);
            true
        }
    });
    // [[skills.config]] in the user's file switches skills by name or path.
    for entry in user["skills"]["config"].as_array().into_iter().flatten() {
        let on = entry["enabled"].as_bool().unwrap_or(true);
        for s in all.iter_mut() {
            let named = entry["name"].as_str() == Some(s.name.as_str());
            let pathed = entry["path"].as_str() == Some(s.path.as_str());
            if named || pathed {
                if on {
                    if s.why.as_deref() == Some("switched off in skills.config") {
                        s.load = Load::Start;
                        s.why = None;
                    }
                } else {
                    s.load = Load::Off;
                    s.why = Some("switched off in skills.config".into());
                }
            }
        }
    }
    all
}

/// The snake-case name Codex keys a hook's state by.
fn snake(event: &str) -> String {
    let mut out = String::new();
    for (i, c) in event.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

fn state_key(file: &str, event: &str, group: usize, index: usize) -> String {
    format!("{file}:{}:{group}:{index}", snake(event))
}

/// The hash Codex trusts a command hook at: its event, matcher and the
/// handler as Codex reads it, as sorted, compact JSON.
pub fn trust_hash(event: &str, matcher: Option<&str>, h: &Value) -> Option<String> {
    if h["type"].as_str().unwrap_or("command") != "command" {
        return None;
    }
    let timeout = h["timeout"].as_u64().or_else(|| h["timeoutSec"].as_u64());
    let timeout = match event {
        "SessionEnd" | "Interrupt" => timeout.unwrap_or(1).clamp(1, 3),
        _ => timeout.unwrap_or(DEFAULT_TIMEOUT).max(1),
    };
    let mut handler = json!({
        "type": "command",
        "command": h["command"].as_str()?,
        "timeout": timeout,
        "async": h["async"].as_bool().unwrap_or(false),
    });
    if let Some(s) = h["statusMessage"].as_str() {
        handler["statusMessage"] = json!(s);
    }
    let context = matches!(
        event,
        "PreToolUse" | "PostToolUse" | "SessionStart" | "UserPromptSubmit" | "SubagentStart"
    );
    if let Some(n) = h["additionalContextLimit"].as_u64()
        && context
        && n != DEFAULT_CONTEXT_LIMIT
    {
        handler["additionalContextLimit"] = json!(n);
    }
    let mut identity = json!({"event_name": snake(event), "hooks": [handler]});
    if let Some(m) = matcher {
        identity["matcher"] = json!(m);
    }
    identity.sort_all_objects();
    let bytes = serde_json::to_vec(&identity).ok()?;
    let hex: String = Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Some(format!("sha256:{hex}"))
}

fn hooks(roots: &Roots, layers: &[Layer], user: &Value, notes: &mut Vec<String>) -> Vec<Hook> {
    let state = &user["hooks"]["state"];
    let mut out = vec![];
    for layer in layers {
        let folder = Path::new(&layer.file.path)
            .parent()
            .map(Path::to_owned)
            .unwrap_or_default();
        let json_file = folder.join("hooks.json");
        let mut from: Vec<(String, Value)> = vec![];
        if let Some(v) = super::json_file(&json_file, notes) {
            from.push((shown(&json_file), v));
        }
        if layer.value["hooks"]
            .as_object()
            .is_some_and(|o| o.keys().any(|k| k != "state"))
        {
            from.push((layer.file.path.clone(), layer.value.clone()));
        }
        for (file, v) in from {
            for mut h in super::claude::hooks_in(&file, layer.file.origin, &v, layer.on) {
                if h.at.event == "state" {
                    continue;
                }
                let key = state_key(&file, &h.event, h.at.group, h.at.index);
                let s = &state[&key];
                if s["enabled"] == Value::Bool(false) {
                    h.on = false;
                }
                let handler = &v["hooks"][&h.event][h.at.group]["hooks"][h.at.index];
                let managed = layer.file.origin == Origin::Managed;
                if !managed && h.on && layer.on {
                    let current = trust_hash(&h.event, h.matcher.as_deref(), handler);
                    let trusted = s["trusted_hash"].as_str();
                    h.note = match (current, trusted) {
                        (Some(c), Some(t)) if c == t => None,
                        (Some(_), Some(_)) => {
                            Some("changed since it was trusted: review it in Codex's /hooks".into())
                        }
                        (Some(_), None) => {
                            Some("not trusted yet: review it in Codex's /hooks".into())
                        }
                        (None, _) => Some("Codex does not run this kind of hook".into()),
                    };
                }
                if !layer.on {
                    h.note = Some("the project is not trusted".into());
                }
                out.push(h);
            }
        }
    }
    let _ = roots;
    out
}

fn settings(layers: &[Layer], dir: &Path, trust: Option<&str>) -> Vec<Setting> {
    const SHOWN_ELSEWHERE: &[&str] = &["projects", "hooks", "mcp_servers", "notice", "skills"];
    let mut out: Vec<Setting> = vec![];
    for layer in layers {
        let Some(map) = layer.value.as_object() else {
            continue;
        };
        let mut add = |key: String, value: &Value| {
            out.push(Setting {
                key,
                value: value.clone(),
                file: layer.file.path.clone(),
                origin: layer.file.origin,
                overridden: !layer.on,
            })
        };
        for (key, value) in map {
            if SHOWN_ELSEWHERE.contains(&key.as_str()) {
                continue;
            }
            match value.as_object() {
                Some(inner) if !inner.is_empty() => {
                    for (k, x) in inner {
                        add(format!("{key}.{k}"), x);
                    }
                }
                _ => add(key.clone(), value),
            }
        }
        if layer.file.origin == Origin::User
            && let Some(t) = trust
        {
            let _ = dir;
            add("trust_level".into(), &json!(t));
        }
    }
    let n = out.len();
    for i in 0..n {
        if out[i].overridden {
            continue;
        }
        out[i].overridden = out[i + 1..]
            .iter()
            .any(|s| s.key == out[i].key && !s.overridden);
    }
    out
}

fn servers(layers: &[Layer]) -> Vec<Server> {
    let mut out: Vec<Server> = vec![];
    for layer in layers {
        for (name, v) in layer.value["mcp_servers"].as_object().into_iter().flatten() {
            let on = layer.on && v["enabled"] != Value::Bool(false);
            for earlier in out.iter_mut().filter(|s| s.name == *name) {
                if layer.on {
                    earlier.on = false;
                    earlier.why = Some(format!("{} sets it again", layer.file.path));
                }
            }
            out.push(Server {
                name: name.clone(),
                run: super::claude::server_run(v),
                file: layer.file.path.clone(),
                origin: layer.file.origin,
                on,
                why: if !layer.on {
                    Some("the project is not trusted".into())
                } else if !on {
                    Some("enabled = false".into())
                } else {
                    None
                },
            });
        }
    }
    out
}

/// A `hooks.json`'s value, `{"hooks":{}}` when it is not there.
fn hooks_file(p: &Path) -> Result<Value, Error> {
    match std::fs::read_to_string(p) {
        Ok(t) if !t.trim().is_empty() => serde_json::from_str(&t).map_err(|e| {
            Error::new(
                INVALID,
                format!("{} does not read as JSON: {e}", p.display()),
            )
        }),
        _ => Ok(json!({"hooks": {}})),
    }
}

/// Changes a hook in a `hooks.json`. Codex keys each hook's switch and
/// trust by its place in the file, so when hooks move, their entries in
/// `[hooks.state]` move with them.
pub fn hook_edit(roots: &Roots, e: &HookEdit) -> Result<Value, Error> {
    let file = edit::path(roots, &e.file)?;
    if file.file_name().is_none_or(|n| n != "hooks.json") {
        return Err(Error::new(
            INVALID,
            "Codex hooks are changed in a hooks.json",
        ));
    }
    let config = codex_home(roots).join("config.toml");
    let mut v = hooks_file(&file)?;
    let mut doc = read_doc(&config)?;
    let key_file = shown(&file);
    // Each handler of the events touched, with its state, by place.
    let mut events: Vec<String> = match &e.change {
        HookChange::Add { hook } => vec![hook.event.clone()],
        HookChange::Set { at, hook, .. } => vec![at.event.clone(), hook.event.clone()],
        HookChange::Remove { at, .. } | HookChange::Switch { at, .. } => vec![at.event.clone()],
    };
    events.dedup();
    let mut states: Vec<(String, Value, Option<toml_edit::Item>)> = vec![];
    for event in &events {
        let groups = v["hooks"][event].as_array().cloned().unwrap_or_default();
        for (g, group) in groups.iter().enumerate() {
            for (i, h) in group["hooks"].as_array().into_iter().flatten().enumerate() {
                let key = state_key(&key_file, event, g, i);
                let item = take_state(&mut doc, &key);
                let mut tagged = h.clone();
                tagged["__ahjo"] = json!(states.len());
                states.push((event.clone(), h.clone(), item));
                v["hooks"][event][g]["hooks"][i] = tagged;
            }
        }
    }
    let mut switch: Option<(usize, bool)> = None;
    match &e.change {
        HookChange::Add { hook } => {
            edit::known_event("codex", &hook.event)?;
            let mut h = json!({"type": "command", "command": hook.command.trim()});
            if hook.command.trim().is_empty() {
                return Err(Error::new(INVALID, "a hook needs a command"));
            }
            if let Some(t) = hook.timeout {
                h["timeout"] = json!(t);
            }
            let matcher = hook
                .matcher
                .as_deref()
                .map(str::trim)
                .filter(|m| !m.is_empty());
            edit::insert(&mut v, &hook.event, matcher, h);
        }
        HookChange::Set { at, was, hook } => {
            edit::known_event("codex", &hook.event)?;
            let (_, mut old) = edit::take(&mut v, at, was)?;
            if hook.command.trim().is_empty() {
                return Err(Error::new(INVALID, "a hook needs a command"));
            }
            old["command"] = json!(hook.command.trim());
            match hook.timeout {
                Some(t) => old["timeout"] = json!(t),
                None => {
                    if let Some(o) = old.as_object_mut() {
                        o.remove("timeout");
                    }
                }
            }
            let matcher = hook
                .matcher
                .as_deref()
                .map(str::trim)
                .filter(|m| !m.is_empty());
            edit::insert(&mut v, &hook.event, matcher, old);
        }
        HookChange::Remove { at, was } => {
            edit::take(&mut v, at, was)?;
        }
        HookChange::Switch { at, was, on } => {
            let h = &v["hooks"][&at.event][at.group]["hooks"][at.index];
            if super::claude::hook("", Origin::User, &at.event, None, h, true, 0, 0).run != *was {
                return Err(Error::new(
                    STALE,
                    "the hook changed since it was read; read it again",
                ));
            }
            switch = h["__ahjo"].as_u64().map(|n| (n as usize, *on));
        }
    }
    // Put the states back at each handler's new place.
    for event in &events {
        let groups = v["hooks"][event].as_array().cloned().unwrap_or_default();
        for (g, group) in groups.iter().enumerate() {
            for (i, h) in group["hooks"].as_array().into_iter().flatten().enumerate() {
                let key = state_key(&key_file, event, g, i);
                if let Some(n) = h["__ahjo"].as_u64().map(|n| n as usize) {
                    let mut item = states[n].2.clone();
                    if let Some((m, on)) = switch
                        && m == n
                    {
                        let table = item
                            .get_or_insert_with(|| toml_edit::Item::Table(toml_edit::Table::new()));
                        if on {
                            if let Some(t) = table.as_table_like_mut() {
                                t.remove("enabled");
                            }
                        } else {
                            table["enabled"] = toml_edit::value(false);
                        }
                        if table.as_table_like().is_some_and(|t| t.is_empty()) {
                            item = None;
                        }
                    }
                    if let Some(item) = item {
                        put_state(&mut doc, &key, item);
                    }
                }
                if let Some(o) = v["hooks"][event][g]["hooks"][i].as_object_mut() {
                    o.remove("__ahjo");
                }
            }
        }
    }
    let mut text =
        serde_json::to_string_pretty(&v).map_err(|e| Error::new(INVALID, e.to_string()))?;
    text.push('\n');
    edit::replace_file(&file, text.as_bytes())?;
    if let Some(d) = doc {
        edit::replace_file(&config, d.to_string().as_bytes())?;
    }
    Ok(json!({"file": e.file}))
}

fn read_doc(p: &Path) -> Result<Option<toml_edit::DocumentMut>, Error> {
    match std::fs::read_to_string(p) {
        Ok(t) => t.parse::<toml_edit::DocumentMut>().map(Some).map_err(|e| {
            Error::new(
                INVALID,
                format!("{} does not read as TOML: {e}", p.display()),
            )
        }),
        Err(_) => Ok(None),
    }
}

fn take_state(doc: &mut Option<toml_edit::DocumentMut>, key: &str) -> Option<toml_edit::Item> {
    let d = doc.as_mut()?;
    let state = d.get_mut("hooks")?.get_mut("state")?.as_table_like_mut()?;
    state.remove(key)
}

fn put_state(doc: &mut Option<toml_edit::DocumentMut>, key: &str, item: toml_edit::Item) {
    let d = doc.get_or_insert_with(toml_edit::DocumentMut::new);
    if d.get("hooks").and_then(|h| h.as_table_like()).is_none() {
        d["hooks"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    if d["hooks"]
        .get("state")
        .and_then(|s| s.as_table_like())
        .is_none()
    {
        let mut t = toml_edit::Table::new();
        t.set_implicit(false);
        d["hooks"]["state"] = toml_edit::Item::Table(t);
    }
    if let Some(t) = d["hooks"]["state"].as_table_like_mut() {
        t.insert(key, item);
    }
}

/// Sets a key in a Codex `config.toml`, keeping the rest of the file as
/// written; `null` takes it out.
pub fn setting_edit(file: &Path, keys: &[&str], value: &Value) -> Result<(), Error> {
    let mut doc = read_doc(file)?.unwrap_or_default();
    let (last, path) = keys
        .split_last()
        .ok_or_else(|| Error::new(INVALID, "no key"))?;
    if path.first().is_some_and(|k| *k == "hooks") {
        return Err(Error::new(INVALID, "hooks are changed as hooks"));
    }
    let mut table: &mut dyn toml_edit::TableLike = doc.as_table_mut();
    for k in path {
        if table.get(k).and_then(|i| i.as_table_like()).is_none() {
            if value.is_null() {
                return Ok(());
            }
            table.insert(k, toml_edit::Item::Table(toml_edit::Table::new()));
        }
        table = table
            .get_mut(k)
            .and_then(|i| i.as_table_like_mut())
            .expect("a table");
    }
    if value.is_null() {
        table.remove(last);
    } else {
        table.insert(last, toml_edit::Item::Value(toml_value(value)?));
    }
    edit::replace_file(file, doc.to_string().as_bytes())
}

fn toml_value(v: &Value) -> Result<toml_edit::Value, Error> {
    Ok(match v {
        Value::Bool(b) => (*b).into(),
        Value::Number(n) if n.is_i64() => n.as_i64().unwrap_or_default().into(),
        Value::Number(n) => n.as_f64().unwrap_or_default().into(),
        Value::String(s) => s.as_str().into(),
        Value::Array(a) => {
            let mut arr = toml_edit::Array::new();
            for x in a {
                arr.push(toml_value(x)?);
            }
            toml_edit::Value::Array(arr)
        }
        Value::Object(o) => {
            let mut t = toml_edit::InlineTable::new();
            for (k, x) in o {
                t.insert(k, toml_value(x)?);
            }
            toml_edit::Value::InlineTable(t)
        }
        Value::Null => return Err(Error::new(INVALID, "null is not a TOML value")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Codex 0.169 hashes a hook's canonical JSON: its event in snake
    /// case, its matcher, and its handler with the defaults filled in, keys
    /// sorted. The expected values were computed apart from this code, and
    /// the same computation matches the hashes Codex wrote for real hooks.
    #[test]
    fn hashes_a_hook_as_codex_does() {
        let session = json!({"type":"command","command":"~/bin/show-tasks --short","statusMessage":"Reading the tasks","additionalContextLimit":5000});
        let guard = json!({"type":"command","command":"~/bin/guard-shell","statusMessage":"Checking the command"});
        let a = trust_hash("SessionStart", Some("startup|resume"), &session).unwrap();
        let b = trust_hash("PreToolUse", None, &guard).unwrap();
        assert_eq!(
            a,
            "sha256:63fb9324c4587801027bb17cbb5da059bf912f856c51abdf7e2261ee6a705dc5"
        );
        assert_eq!(
            b,
            "sha256:a85d9941ae4978f0564bbcb80c43d190f3d7e0b292f79a59c75c986b27984d60"
        );
        assert_eq!(snake("PreToolUse"), "pre_tool_use");
        assert_eq!(
            state_key("/h/.codex/hooks.json", "SessionStart", 0, 1),
            "/h/.codex/hooks.json:session_start:0:1"
        );
    }
}
