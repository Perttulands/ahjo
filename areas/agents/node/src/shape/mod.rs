//! What shapes an agent started in a folder, read the way each harness
//! reads it (`claude`, `codex`), and the edits the page makes to it
//! (`edit`). The rules and where each comes from are written beside the
//! code that follows them.

mod claude;
mod codex;
pub mod edit;
pub mod routes;

use ahjo_agents_contract::shape::{Instruction, Load, Origin, SeenSkill, Shape, Source};
use ahjo_core::Error;
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use super::Home;

/// Where the administrator's files are. Tests leave them where they are:
/// a scratch home never has them.
pub struct Roots {
    pub home: Home,
    pub claude_managed: PathBuf,
    pub codex_system: PathBuf,
    /// Hooks switched off where the harness has no switch of its own.
    pub parked: PathBuf,
}

impl Roots {
    /// `node` is the node's own folder, where switched-off hooks wait.
    pub fn new(home: Home, node: &Path) -> Self {
        let (claude_managed, codex_system) = if cfg!(windows) {
            (
                PathBuf::from(r"C:\Program Files\ClaudeCode"),
                PathBuf::from(r"C:\ProgramData\codex"),
            )
        } else {
            (
                PathBuf::from("/etc/claude-code"),
                PathBuf::from("/etc/codex"),
            )
        };
        Self {
            home,
            claude_managed,
            codex_system,
            parked: node.join("agents").join("hooks-off.json"),
        }
    }
}

/// What shapes `harness` started in `dir`.
pub fn read(roots: &Roots, dir: &str, harness: &str) -> Result<Shape, Error> {
    let dir = folder(&roots.home, dir)?;
    let mut shape = match harness {
        "claude" => claude::read(roots, &dir),
        "codex" => codex::read(roots, &dir),
        other => {
            return Err(Error::new(
                ahjo_agents_contract::shape::INVALID,
                format!("no harness {other:?}: claude or codex"),
            ));
        }
    };
    shape.dir = dir.to_string_lossy().into_owned();
    shape.harness = harness.to_owned();
    shape.home = roots.home.dir.to_string_lossy().into_owned();
    Ok(shape)
}

/// The folder as an absolute path: `~` is the home folder, `.` and `..`
/// are taken out. It must be a folder that is there.
pub fn folder(home: &Home, dir: &str) -> Result<PathBuf, Error> {
    let dir = dir.trim();
    let path = if dir == "~" {
        home.dir.clone()
    } else if let Some(rest) = dir.strip_prefix("~/") {
        home.dir.join(rest)
    } else {
        PathBuf::from(dir)
    };
    if !path.is_absolute() {
        return Err(Error::new(
            ahjo_agents_contract::shape::INVALID,
            format!("{dir:?} is not a full path"),
        ));
    }
    let mut clean = PathBuf::new();
    for part in path.components() {
        match part {
            Component::ParentDir => {
                clean.pop();
            }
            Component::CurDir => {}
            other => clean.push(other),
        }
    }
    if !clean.is_dir() {
        return Err(Error::new(
            ahjo_agents_contract::shape::NOT_FOUND,
            format!("{} is not a folder", clean.display()),
        ));
    }
    Ok(clean)
}

/// The folder and every folder above it, the top first.
pub fn ancestors(dir: &Path) -> Vec<PathBuf> {
    let mut all: Vec<PathBuf> = dir.ancestors().map(Path::to_owned).collect();
    all.reverse();
    all
}

/// The nearest folder at or above `dir` holding `.git`.
pub fn git_root(dir: &Path) -> Option<PathBuf> {
    dir.ancestors()
        .find(|d| d.join(".git").exists())
        .map(Path::to_owned)
}

/// The repository a worktree belongs to: a `.git` file naming
/// `<repo>/.git/worktrees/<name>` gives `<repo>`; a `.git` folder gives
/// its own folder.
pub fn main_root(root: &Path) -> PathBuf {
    let git = root.join(".git");
    if git.is_file()
        && let Ok(text) = std::fs::read_to_string(&git)
        && let Some(dir) = text.trim().strip_prefix("gitdir:")
    {
        let dir = root.join(dir.trim());
        if let Some(repo) = dir
            .ancestors()
            .find(|a| a.file_name().is_some_and(|n| n == ".git"))
            .and_then(Path::parent)
        {
            return repo.to_owned();
        }
    }
    root.to_owned()
}

/// Where a path leads when it, or a folder on the way to it, is a link.
pub fn target(path: &Path) -> Option<PathBuf> {
    let real = path.canonicalize().ok()?;
    (real != path).then_some(real)
}

pub fn shown(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// An instruction file as found, if it is there.
pub fn instruction(path: &Path, origin: Origin, load: Load) -> Option<Instruction> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let lines = std::fs::read(path)
        .map(|b| {
            let breaks = b.iter().filter(|&&c| c == b'\n').count();
            (breaks + usize::from(b.last().is_some_and(|&c| c != b'\n'))) as u32
        })
        .unwrap_or(0);
    Some(Instruction {
        path: shown(path),
        target: target(path).map(|p| shown(&p)),
        origin,
        load,
        bytes: meta.len(),
        lines,
        why: None,
        from: None,
    })
}

/// A file read for hooks or settings.
pub fn source(path: &Path, origin: Origin, hooks: bool) -> Source {
    Source {
        path: shown(path),
        origin,
        present: path.is_file(),
        hooks,
    }
}

/// The front matter of a `SKILL.md`: its `key: value` lines, a folded or
/// literal block (`>` or `|`) joined into one line.
pub fn front_matter(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") {
        return out;
    }
    let body: Vec<&str> = lines.take_while(|l| l.trim() != "---").collect();
    let mut i = 0;
    while i < body.len() {
        let line = body[i];
        i += 1;
        if line.starts_with([' ', '\t']) {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        let value = if value.is_empty() || value.starts_with(['>', '|']) {
            let mut parts = vec![];
            while i < body.len() && (body[i].starts_with([' ', '\t']) || body[i].trim().is_empty())
            {
                parts.push(body[i].trim());
                i += 1;
            }
            parts.join(" ").trim().to_owned()
        } else {
            value.trim_matches(['"', '\'']).to_owned()
        };
        out.insert(key.trim().to_owned(), value);
    }
    out
}

/// The skills in a skills folder: each entry holding a `SKILL.md`,
/// following links. `prefix` names a plugin's skills (`plugin:`).
pub fn skills_in(folder: &Path, origin: Origin, prefix: &str) -> Vec<SeenSkill> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return vec![];
    };
    let mut out: Vec<SeenSkill> = entries
        .flatten()
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .filter_map(|e| {
            let file = e.path().join("SKILL.md");
            let text = std::fs::read_to_string(&file).ok()?;
            let front = front_matter(&text);
            let dir_name = e.file_name().to_string_lossy().into_owned();
            let name = front
                .get("name")
                .filter(|n| !n.is_empty())
                .cloned()
                .unwrap_or(dir_name);
            Some(SeenSkill {
                name: format!("{prefix}{name}"),
                description: front.get("description").cloned().unwrap_or_default(),
                path: shown(&file),
                target: target(&e.path()).map(|p| shown(&p)),
                origin,
                load: Load::Start,
                why: (front.get("disable-model-invocation").map(String::as_str) == Some("true"))
                    .then(|| "only when asked for by name".to_owned()),
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Marks each skill whose name an earlier one already took as shadowed by
/// it: `skills` comes in the harness's order of precedence.
pub fn shadow(skills: &mut [SeenSkill]) {
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    for s in skills.iter_mut() {
        if let Some(first) = seen.get(&s.name) {
            if s.load != Load::Off {
                s.load = Load::Off;
                s.why = Some(format!("shadowed by {first}"));
            }
        } else {
            seen.insert(s.name.clone(), s.path.clone());
        }
    }
}

/// `pattern` matches `path`: `**` crosses folders, `*` and `?` do not.
pub fn glob(pattern: &str, path: &str) -> bool {
    fn go(p: &[u8], s: &[u8]) -> bool {
        match p {
            [] => s.is_empty(),
            [b'*', b'*', rest @ ..] => {
                let rest = rest.strip_prefix(b"/").unwrap_or(rest);
                (0..=s.len()).any(|i| go(rest, &s[i..]))
            }
            [b'*', rest @ ..] => (0..=s.len())
                .take_while(|&i| i == 0 || s[i - 1] != b'/')
                .any(|i| go(rest, &s[i..])),
            [b'?', rest @ ..] => !s.is_empty() && s[0] != b'/' && go(rest, &s[1..]),
            [c, rest @ ..] => s.first() == Some(c) && go(rest, &s[1..]),
        }
    }
    go(pattern.as_bytes(), path.as_bytes())
}

/// Folders under `dir`, at most `depth` down and `most` in all, leaving out
/// hidden ones and build output: where instructions that load later are.
pub fn below(dir: &Path, depth: usize, most: usize) -> Vec<PathBuf> {
    const SKIP: &[&str] = &[
        "node_modules",
        "target",
        "dist",
        "build",
        "vendor",
        "__pycache__",
    ];
    let mut out = vec![];
    let mut level = vec![dir.to_owned()];
    for _ in 0..depth {
        let mut next = vec![];
        for d in &level {
            let Ok(entries) = std::fs::read_dir(d) else {
                continue;
            };
            let mut found: Vec<PathBuf> = entries
                .flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .filter(|e| {
                    let n = e.file_name().to_string_lossy().into_owned();
                    !n.starts_with('.') && !SKIP.contains(&n.as_str())
                })
                .map(|e| e.path())
                .collect();
            found.sort();
            next.extend(found);
            if out.len() + next.len() >= most {
                break;
            }
        }
        next.truncate(most.saturating_sub(out.len()));
        out.extend(next.iter().cloned());
        if out.len() >= most || next.is_empty() {
            break;
        }
        level = next;
    }
    out
}

/// A JSON file as a value: none when it is not there; a note when it does
/// not read.
pub fn json_file(path: &Path, notes: &mut Vec<String>) -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str(&text) {
        Ok(v) => Some(v),
        Err(e) => {
            notes.push(format!("{} does not read as JSON: {e}", path.display()));
            None
        }
    }
}

/// The `@path` imports in an instruction file's text, outside code: a
/// path after `@` that starts with `/`, `~/`, `./`, `../` or a word, as
/// written.
pub fn imports(text: &str) -> Vec<String> {
    let mut out = vec![];
    let mut fenced = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        let mut in_code = false;
        let bytes: Vec<char> = line.chars().collect();
        let mut i = 0;
        while i < bytes.len() {
            let c = bytes[i];
            if c == '`' {
                in_code = !in_code;
            } else if c == '@' && !in_code && (i == 0 || bytes[i - 1].is_whitespace()) {
                let mut j = i + 1;
                let mut word = String::new();
                while j < bytes.len() {
                    let ch = bytes[j];
                    if ch == '\\' && j + 1 < bytes.len() && bytes[j + 1] == ' ' {
                        word.push(' ');
                        j += 2;
                        continue;
                    }
                    if ch.is_whitespace() {
                        break;
                    }
                    word.push(ch);
                    j += 1;
                }
                let word = word
                    .trim_end_matches([',', ';', ')', ':'])
                    .trim_end_matches('.')
                    .to_owned();
                if word.contains('/') || word.contains('.') {
                    out.push(word);
                }
                i = j;
                continue;
            }
            i += 1;
        }
    }
    out
}

/// Each file `file` imports, then what they import, `hops` deep, after it.
pub fn with_imports(home: &Home, file: Instruction, hops: usize, out: &mut Vec<Instruction>) {
    let path = PathBuf::from(&file.path);
    let load = file.load;
    let text = if load == Load::Off {
        String::new()
    } else {
        std::fs::read_to_string(&path).unwrap_or_default()
    };
    out.push(file);
    if hops == 0 {
        return;
    }
    let base = path.parent().map(Path::to_owned).unwrap_or_default();
    for written in imports(&text) {
        let to = if let Some(rest) = written.strip_prefix("~/") {
            home.dir.join(rest)
        } else if written.starts_with('/') {
            PathBuf::from(&written)
        } else {
            base.join(&written)
        };
        if out.iter().any(|i| Path::new(&i.path) == to) {
            continue;
        }
        if let Some(mut i) = instruction(&to, Origin::Import, load) {
            i.from = Some(shown(&path));
            with_imports(home, i, hops - 1, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs_cross_folders_only_with_two_stars() {
        assert!(glob("/home/p/AGENTS.md", "/home/p/AGENTS.md"));
        assert!(glob("**/vendor/**", "/srv/x/vendor/a/CLAUDE.md"));
        assert!(glob("/srv/*/CLAUDE.md", "/srv/a/CLAUDE.md"));
        assert!(!glob("/srv/*/CLAUDE.md", "/srv/a/b/CLAUDE.md"));
        assert!(glob("/srv/?/x", "/srv/a/x"));
    }

    #[test]
    fn imports_skip_code_and_addresses() {
        let text = "See @docs/a.md and @~/notes.md.\n`@not/this.md`\n```\n@nor/this.md\n```\nmail me@host.com @./b.md";
        assert_eq!(imports(text), ["docs/a.md", "~/notes.md", "./b.md"]);
    }

    #[test]
    fn front_matter_reads_plain_quoted_and_folded_values() {
        let text = "---\nname: x\ndescription: >\n  Two\n  lines.\nuser-invocable: \"false\"\n---\nbody: no\n";
        let f = front_matter(text);
        assert_eq!(f["name"], "x");
        assert_eq!(f["description"], "Two lines.");
        assert_eq!(f["user-invocable"], "false");
        assert!(!f.contains_key("body"));
    }
}
