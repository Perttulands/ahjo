//! What Claude Code reads, from its documentation (code.claude.com/docs/en:
//! memory, skills, hooks, settings, mcp) and checked against Claude Code
//! 2.1.326 on the operator's machine.
//!
//! Instructions, in this order: the managed `CLAUDE.md`; `~/.claude/CLAUDE.md`;
//! `~/.claude/rules/**/*.md`; then each folder from the filesystem's root
//! down to the folder, its `CLAUDE.md`, `.claude/CLAUDE.md` and
//! `CLAUDE.local.md`, and the project's `.claude/rules`. `AGENTS.md` stands
//! in for `CLAUDE.md` only where no `CLAUDE.md`, `.claude/CLAUDE.md` or
//! `CLAUDE.local.md` is in the folder or above it. Each file's `@path`
//! imports follow it, four hops deep. Files matching `claudeMdExcludes`
//! (absolute-path globs, every settings file's together) are left out,
//! except the managed one. Last, the project's auto memory: `MEMORY.md`
//! under `~/.claude/projects/<repo path, every other character a dash>/memory`,
//! its first 200 lines or 25 KB. Instruction files in folders below load
//! when the agent works there.
//!
//! The documentation does not say where the walk up stops; the operator's
//! own `claudeMdExcludes` leaves out `~/AGENTS.md`, which only matters if
//! it reaches the home folder, so it is read to the root.

use super::{
    Roots, ancestors, below, git_root, glob, instruction, json_file, main_root, shadow, shown,
    skills_in, source, with_imports,
};
use ahjo_agents_contract::shape::{
    Hook, HookAt, Instruction, Load, Origin, SeenSkill, Server, Setting, Shape, Source,
};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// The largest instruction file that loads.
const MAX_BYTES: u64 = 4 << 20;
const IMPORT_HOPS: usize = 4;
const MEMORY_LINES: usize = 200;
const MEMORY_BYTES: u64 = 25 * 1024;

/// The settings files, lowest precedence first, with whether hooks may be
/// added there.
pub fn settings_files(roots: &Roots, dir: &Path) -> Vec<Source> {
    let user = roots.home.dir.join(".claude");
    let mut files = vec![
        source(&user.join("settings.json"), Origin::User, true),
        source(&dir.join(".claude/settings.json"), Origin::Project, true),
        source(
            &dir.join(".claude/settings.local.json"),
            Origin::Local,
            true,
        ),
    ];
    let managed = roots.claude_managed.join("managed-settings.json");
    if managed.is_file() {
        files.push(source(&managed, Origin::Managed, false));
    }
    if let Ok(entries) = std::fs::read_dir(roots.claude_managed.join("managed-settings.d")) {
        let mut more: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        more.sort();
        files.extend(more.iter().map(|p| source(p, Origin::Managed, false)));
    }
    files
}

pub fn read(roots: &Roots, dir: &Path) -> Shape {
    let mut shape = Shape::default();
    let root = git_root(dir);
    shape.root = root.as_ref().map(|r| shown(r));
    let project = root.clone().unwrap_or_else(|| dir.to_owned());
    shape.files = settings_files(roots, dir);
    let layers: Vec<(Source, Value)> = shape
        .files
        .iter()
        .filter_map(|f| {
            let v = json_file(Path::new(&f.path), &mut shape.notes)?;
            Some((f.clone(), v))
        })
        .collect();
    shape.instructions = instructions(roots, dir, &project, &layers);
    shape.skills = skills(roots, dir, root.as_deref(), &layers);
    shape.hooks = hooks(&layers, roots);
    if layers
        .iter()
        .any(|(_, v)| v["disableAllHooks"] == Value::Bool(true))
    {
        for h in &mut shape.hooks {
            h.on = false;
        }
        shape
            .notes
            .push("disableAllHooks is set: no hook runs".into());
    }
    shape.settings = settings(&layers);
    shape.servers = servers(roots, dir, &project, &layers, &mut shape.notes);
    if layers.iter().any(|(_, v)| {
        v["enabledPlugins"]
            .as_object()
            .is_some_and(|p| !p.is_empty())
    }) {
        shape
            .notes
            .push("Plugins are on: their skills, hooks and servers are not shown".into());
    }
    shape
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// Which instruction files count: `claude-md-or-agents-md` unless the
/// user's settings name another (`pluginConfigs`, `cc-plugin-agents-md@builtin`).
fn instruction_files(layers: &[(Source, Value)]) -> String {
    layers
        .iter()
        .filter(|(s, _)| matches!(s.origin, Origin::User | Origin::Managed))
        .filter_map(|(_, v)| {
            let c = &v["pluginConfigs"]["cc-plugin-agents-md@builtin"];
            c["options"]["instructionFiles"]
                .as_str()
                .or_else(|| c["instructionFiles"].as_str())
                .map(str::to_owned)
        })
        .next_back()
        .unwrap_or_else(|| "claude-md-or-agents-md".into())
}

fn instructions(
    roots: &Roots,
    dir: &Path,
    project: &Path,
    layers: &[(Source, Value)],
) -> Vec<Instruction> {
    let home = &roots.home;
    let excludes: Vec<String> = layers
        .iter()
        .flat_map(|(_, v)| strings(&v["claudeMdExcludes"]))
        .collect();
    let mode = instruction_files(layers);
    let mut found: Vec<Instruction> = vec![];
    let add = |i: Option<Instruction>, found: &mut Vec<Instruction>| {
        if let Some(i) = i {
            found.push(i);
        }
    };
    add(
        instruction(
            &roots.claude_managed.join("CLAUDE.md"),
            Origin::Managed,
            Load::Start,
        ),
        &mut found,
    );
    if mode != "managed-only" {
        let user = home.dir.join(".claude");
        add(
            instruction(&user.join("CLAUDE.md"), Origin::User, Load::Start),
            &mut found,
        );
        for rule in rules(&user.join("rules")) {
            add(rule_file(&rule, Origin::User), &mut found);
        }
        let folders = ancestors(dir);
        // The home folder's .claude/CLAUDE.md is the operator's own file,
        // read once, as theirs, and not a project's above the folder.
        let users = user.join("CLAUDE.md");
        let claude_names = |d: &Path| {
            [
                d.join("CLAUDE.md"),
                d.join(".claude/CLAUDE.md"),
                d.join("CLAUDE.local.md"),
            ]
            .map(|p| if p == users { PathBuf::new() } else { p })
        };
        let any_claude = folders
            .iter()
            .any(|d| claude_names(d).iter().any(|p| p.is_file()));
        let agents = match mode.as_str() {
            "claude-md" => false,
            "claude-md-and-agents-md" => true,
            _ => !any_claude,
        };
        for d in &folders {
            let origin = if d.starts_with(project) {
                Origin::Project
            } else {
                Origin::Ancestor
            };
            let [main, dotted, local] = claude_names(d);
            add(instruction(&main, origin, Load::Start), &mut found);
            add(instruction(&dotted, origin, Load::Start), &mut found);
            if agents {
                add(
                    instruction(&d.join("AGENTS.md"), origin, Load::Start),
                    &mut found,
                );
            }
            add(instruction(&local, Origin::Local, Load::Start), &mut found);
            if d.starts_with(project) {
                for rule in rules(&d.join(".claude/rules")) {
                    add(rule_file(&rule, Origin::Project), &mut found);
                }
            }
        }
        // Folders below load when the agent reads or writes in them.
        for d in below(dir, 3, 4000) {
            for name in ["CLAUDE.md", ".claude/CLAUDE.md", "CLAUDE.local.md"]
                .into_iter()
                .chain(agents.then_some("AGENTS.md"))
            {
                if let Some(mut i) = instruction(&d.join(name), Origin::Project, Load::Later) {
                    i.why = Some(format!(
                        "when it works in {}",
                        d.strip_prefix(dir).unwrap_or(&d).display()
                    ));
                    found.push(i);
                }
            }
        }
    }
    let mut out = vec![];
    for mut i in found {
        let target = i.target.clone().unwrap_or_default();
        if i.origin != Origin::Managed
            && let Some(pattern) = excludes
                .iter()
                .find(|p| glob(p, &i.path) || glob(p, &target))
        {
            i.load = Load::Off;
            i.why = Some(format!("left out by claudeMdExcludes {pattern}"));
        } else if i.bytes > MAX_BYTES {
            i.load = Load::Off;
            i.why = Some("over 4 MiB".into());
        }
        with_imports(home, i, IMPORT_HOPS, &mut out);
    }
    if let Some(memory) = memory(roots, project, layers) {
        out.extend(memory);
    }
    out
}

fn rules(folder: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    let Ok(entries) = std::fs::read_dir(folder) else {
        return out;
    };
    let mut entries: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            out.extend(rules(&p));
        } else if p.extension().is_some_and(|x| x == "md") {
            out.push(p);
        }
    }
    out
}

/// A rule loads at the start unless its front matter names `paths:`.
fn rule_file(path: &Path, origin: Origin) -> Option<Instruction> {
    let mut i = instruction(path, origin, Load::Start)?;
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let mut lines = text.lines();
    if lines.next().map(str::trim) == Some("---")
        && let Some(paths) = lines
            .take_while(|l| l.trim() != "---")
            .find(|l| l.starts_with("paths:"))
    {
        i.load = Load::Later;
        let written = paths.trim_start_matches("paths:").trim();
        i.why = Some(if written.is_empty() {
            "when it works on the files its paths name".into()
        } else {
            format!("when it works on {written}")
        });
    } else {
        i.why = Some("a rule".into());
    }
    Some(i)
}

/// The folder name Claude Code keeps a project's files under: its path
/// with every character that is not a letter or digit a dash.
pub fn project_key(path: &Path) -> String {
    shown(path)
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

fn memory(roots: &Roots, project: &Path, layers: &[(Source, Value)]) -> Option<Vec<Instruction>> {
    let mut on = true;
    let mut place: Option<PathBuf> = None;
    for (_, v) in layers {
        if let Some(b) = v["autoMemoryEnabled"].as_bool() {
            on = b;
        }
        if let Some(d) = v["autoMemoryDirectory"].as_str() {
            place = Some(match d.strip_prefix("~/") {
                Some(rest) => roots.home.dir.join(rest),
                None => PathBuf::from(d),
            });
        }
    }
    let folder = place.unwrap_or_else(|| {
        roots
            .home
            .dir
            .join(".claude/projects")
            .join(project_key(&main_root(project)))
            .join("memory")
    });
    let index = folder.join("MEMORY.md");
    let mut first = instruction(
        &index,
        Origin::Memory,
        if on { Load::Start } else { Load::Off },
    )?;
    if !on {
        first.why = Some("autoMemoryEnabled is off".into());
    } else if first.lines as usize > MEMORY_LINES || first.bytes > MEMORY_BYTES {
        first.why = Some("its first 200 lines or 25 KB".into());
    }
    let mut out = vec![first];
    if let Ok(entries) = std::fs::read_dir(&folder) {
        let mut topics: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "md") && *p != index)
            .collect();
        topics.sort();
        for t in topics {
            if let Some(mut i) = instruction(&t, Origin::Memory, Load::Later) {
                i.why = Some("when it looks it up".into());
                out.push(i);
            }
        }
    }
    Some(out)
}

fn skills(
    roots: &Roots,
    dir: &Path,
    root: Option<&Path>,
    layers: &[(Source, Value)],
) -> Vec<SeenSkill> {
    let user = roots.home.dir.join(".claude/skills");
    let mut all = skills_in(&roots.claude_managed.join("skills"), Origin::Managed, "");
    all.extend(skills_in(&user, Origin::User, ""));
    // Project skills: the folder and each one above it to the repository's
    // root, the nearest first.
    let stop = root.unwrap_or(dir);
    for d in dir.ancestors() {
        all.extend(skills_in(&d.join(".claude/skills"), Origin::Project, ""));
        if d == stop {
            break;
        }
    }
    // Skills synced from claude.ai, as a plugin of their own.
    if let Ok(entries) = std::fs::read_dir(user.join("synced")) {
        for e in entries.flatten() {
            all.extend(skills_in(&e.path(), Origin::Plugin, "anthropic-skills:"));
        }
    }
    shadow(&mut all);
    let mut overrides: std::collections::BTreeMap<String, String> = Default::default();
    for (_, v) in layers {
        if let Some(o) = v["skillOverrides"].as_object() {
            for (k, x) in o {
                if let Some(x) = x.as_str() {
                    overrides.insert(k.clone(), x.to_owned());
                }
            }
        }
    }
    for s in &mut all {
        match overrides.get(&s.name).map(String::as_str) {
            Some("off") => {
                s.load = Load::Off;
                s.why = Some("switched off in skillOverrides".into());
            }
            Some("user-invocable-only") => {
                s.load = Load::Off;
                s.why = Some("only when you type its name".into());
            }
            Some("name-only") if s.load == Load::Start => {
                s.why = Some("listed by name only".into());
            }
            _ => {}
        }
    }
    all
}

fn hooks(layers: &[(Source, Value)], roots: &Roots) -> Vec<Hook> {
    let mut out = vec![];
    for (file, v) in layers {
        out.extend(hooks_in(&file.path, file.origin, v, true));
    }
    out.extend(super::edit::parked(roots, "claude"));
    out
}

/// The hooks a settings value holds: `hooks.<event>[group].hooks[index]`.
pub fn hooks_in(file: &str, origin: Origin, v: &Value, on: bool) -> Vec<Hook> {
    let mut out = vec![];
    let Some(events) = v["hooks"].as_object() else {
        return out;
    };
    for (event, groups) in events {
        for (g, group) in groups.as_array().into_iter().flatten().enumerate() {
            let matcher = group["matcher"]
                .as_str()
                .filter(|m| !m.is_empty() && *m != "*")
                .map(str::to_owned);
            for (i, h) in group["hooks"].as_array().into_iter().flatten().enumerate() {
                out.push(hook(file, origin, event, matcher.clone(), h, on, g, i));
            }
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
pub fn hook(
    file: &str,
    origin: Origin,
    event: &str,
    matcher: Option<String>,
    h: &Value,
    on: bool,
    group: usize,
    index: usize,
) -> Hook {
    let kind = h["type"].as_str().unwrap_or("command").to_owned();
    let run = ["command", "prompt", "url"]
        .iter()
        .find_map(|k| h[*k].as_str())
        .map(str::to_owned)
        .or_else(|| {
            let (s, t) = (h["server"].as_str()?, h["tool"].as_str()?);
            Some(format!("{s} {t}"))
        })
        .unwrap_or_default();
    Hook {
        event: event.to_owned(),
        matcher,
        kind,
        run,
        timeout: h["timeout"].as_u64(),
        status: h["statusMessage"].as_str().map(str::to_owned),
        file: file.to_owned(),
        origin,
        on,
        note: None,
        at: HookAt {
            event: event.to_owned(),
            group,
            index,
            parked: false,
        },
    }
}

/// Every setting each file makes, but hooks (shown as hooks). Objects are
/// opened one level (`permissions.deny`, `env.X`). A later file's value
/// wins over an earlier one's; lists are joined, so none is overridden.
fn settings(layers: &[(Source, Value)]) -> Vec<Setting> {
    let mut out: Vec<Setting> = vec![];
    for (file, v) in layers {
        let Some(map) = v.as_object() else { continue };
        for (key, value) in map {
            if key == "hooks" {
                continue;
            }
            match value.as_object() {
                Some(inner) if !inner.is_empty() => {
                    for (k, x) in inner {
                        out.push(setting(file, &format!("{key}.{k}"), x));
                    }
                }
                _ => out.push(setting(file, key, value)),
            }
        }
    }
    let n = out.len();
    for i in 0..n {
        if out[i].value.is_array() {
            continue;
        }
        let later = out[i + 1..].iter().any(|s| s.key == out[i].key);
        out[i].overridden = later;
    }
    out
}

fn setting(file: &Source, key: &str, value: &Value) -> Setting {
    Setting {
        key: key.to_owned(),
        value: value.clone(),
        file: file.path.clone(),
        origin: file.origin,
        overridden: false,
    }
}

/// A server's command line or URL, never its environment or headers.
pub fn server_run(v: &Value) -> String {
    if let Some(url) = v["url"].as_str() {
        return url.to_owned();
    }
    let mut parts = vec![v["command"].as_str().unwrap_or_default().to_owned()];
    parts.extend(strings(&v["args"]));
    parts.join(" ").trim().to_owned()
}

fn servers(
    roots: &Roots,
    dir: &Path,
    project: &Path,
    layers: &[(Source, Value)],
    notes: &mut Vec<String>,
) -> Vec<Server> {
    let mut out: Vec<Server> = vec![];
    // ~/.claude.json holds much else; only its servers are read.
    let state = roots.home.dir.join(".claude.json");
    let state_value = json_file(&state, notes);
    let named = |file: &Path, origin, map: &Value, why: Option<String>| -> Vec<Server> {
        map.as_object()
            .map(|m| {
                m.iter()
                    .map(|(name, v)| Server {
                        name: name.clone(),
                        run: server_run(v),
                        file: shown(file),
                        origin,
                        on: true,
                        why: why.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    if let Some(s) = &state_value {
        out.extend(named(&state, Origin::User, &s["mcpServers"], None));
        for key in [dir, project] {
            let p = &s["projects"][shown(key)]["mcpServers"];
            for server in named(
                &state,
                Origin::Local,
                p,
                Some(format!("for {}", key.display())),
            ) {
                if !out
                    .iter()
                    .any(|o| o.name == server.name && o.origin == Origin::Local)
                {
                    out.push(server);
                }
            }
            if key == project {
                break;
            }
        }
    }
    let mcp_file = project.join(".mcp.json");
    if let Some(v) = json_file(&mcp_file, notes) {
        // Approved or refused in settings, or at Claude Code's own prompt,
        // which keeps the answer with the project in ~/.claude.json.
        let mut places: Vec<&Value> = layers.iter().map(|(_, v)| v).collect();
        if let Some(s) = &state_value {
            for key in [dir, project] {
                places.push(&s["projects"][shown(key)]);
            }
        }
        let disabled: Vec<String> = places
            .iter()
            .flat_map(|v| strings(&v["disabledMcpjsonServers"]))
            .collect();
        let enabled: Vec<String> = places
            .iter()
            .flat_map(|v| strings(&v["enabledMcpjsonServers"]))
            .collect();
        let all = layers
            .iter()
            .any(|(_, v)| v["enableAllProjectMcpServers"] == Value::Bool(true));
        for mut s in named(&mcp_file, Origin::Project, &v["mcpServers"], None) {
            if disabled.contains(&s.name) {
                s.on = false;
                s.why = Some("refused in disabledMcpjsonServers".into());
            } else if !all && !enabled.contains(&s.name) {
                s.why = Some("asks before its first use".into());
            }
            out.push(s);
        }
    }
    // A name set in several places takes the most local one.
    let rank = |o: Origin| match o {
        Origin::Local => 0,
        Origin::Project => 1,
        _ => 2,
    };
    let mut seen: Vec<(String, Origin)> = vec![];
    let mut sorted = out.clone();
    sorted.sort_by_key(|s| rank(s.origin));
    for s in &sorted {
        if !seen.iter().any(|(n, _)| *n == s.name) {
            seen.push((s.name.clone(), s.origin));
        }
    }
    for s in &mut out {
        if let Some((_, o)) = seen.iter().find(|(n, _)| *n == s.name)
            && *o != s.origin
        {
            s.on = false;
            s.why = Some(format!("a {} server of the same name wins", o.word()));
        }
    }
    out
}
