//! The brain as the app holds it: every page's name, kind, summary, date and
//! links, read from the brain's own Markdown files.
//!
//! The brain is files (Context Citadel's `docs/ARCHITECTURE.md`); the
//! service is a convenience over them that answers a bounded search and a
//! bounded read, never the whole graph. So the sky is drawn from the files,
//! by the rules `lib/brain.js` uses: the four collections, a project's one
//! home page, generated maps left out, a `README.md` named for its folder,
//! and links as `[[slug]]` or relative `.md` links outside code.
//!
//! A scan reuses what an earlier scan read of every file whose size and
//! modification time are unchanged, so after the first launch only what
//! changed is read again. That matters on Windows, where the brain is read
//! across the WSL file share at a few milliseconds a file.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::UNIX_EPOCH;

/// The page folders, as Context Citadel lists them.
pub const COLLECTIONS: [&str; 4] = ["knowledge", "evidence", "projects", "self"];

/// Which collection a filed record's subject is drawn with when several
/// collections have a subject of that name.
const HOME_ORDER: [&str; 3] = ["knowledge", "projects", "self"];

/// One file as a scan read it, kept between launches.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub path: String,
    pub size: u64,
    pub modified: u64,
    pub slug: String,
    pub title: String,
    pub kind: String,
    pub summary: String,
    /// The frontmatter's `updated`, as a Julian day.
    pub updated: Option<i32>,
    pub words: u32,
    pub aliases: Vec<String>,
    /// The slugs it links to, as written.
    pub refs: Vec<String>,
    /// Who wrote it, where its header says (`origin`).
    #[serde(default)]
    pub origin: String,
}

#[derive(Clone, Debug)]
pub struct Page {
    pub path: String,
    pub slug: String,
    pub title: String,
    pub kind: String,
    pub summary: String,
    pub updated: Option<i32>,
    /// When its file last changed, in nanoseconds since 1970.
    pub modified: u64,
    pub words: u32,
    pub aliases: Vec<String>,
    pub subject: usize,
    /// Who wrote it, where its header says.
    pub origin: String,
}

impl Page {
    /// A record kept as evidence rather than understanding.
    pub fn is_evidence(&self) -> bool {
        self.kind == "source" || self.path.starts_with("evidence/")
    }
}

#[derive(Clone, Debug)]
pub struct Subject {
    /// `collection/name`, or a collection for pages at its top.
    pub id: String,
    /// Said as a reader says it: `Game systems`.
    pub name: String,
    pub pages: Vec<usize>,
}

/// Every page, the links between them resolved, and the subjects they fall in.
#[derive(Clone, Debug, Default)]
pub struct Corpus {
    pub pages: Vec<Page>,
    pub subjects: Vec<Subject>,
    /// `(from, to)`, each pair once, no page to itself.
    pub links: Vec<(usize, usize)>,
    pub out: Vec<Vec<usize>>,
    pub inc: Vec<Vec<usize>>,
    by_slug: HashMap<String, usize>,
    by_path: HashMap<String, usize>,
}

impl Corpus {
    pub fn new(mut entries: Vec<Entry>) -> Corpus {
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        let subject_ids = subjects_of(entries.iter().map(|e| e.path.as_str()));
        let mut subjects: Vec<Subject> = vec![];
        let mut subject_at: HashMap<String, usize> = HashMap::new();
        let mut sorted_ids = subject_ids.clone();
        sorted_ids.sort();
        sorted_ids.dedup();
        for id in sorted_ids {
            subject_at.insert(id.clone(), subjects.len());
            subjects.push(Subject {
                name: subject_name(&id),
                id,
                pages: vec![],
            });
        }
        let mut pages = Vec::with_capacity(entries.len());
        let mut by_slug = HashMap::new();
        let mut by_path = HashMap::new();
        for (i, (e, sid)) in entries.iter().zip(&subject_ids).enumerate() {
            let subject = subject_at[sid];
            subjects[subject].pages.push(i);
            by_slug.entry(e.slug.clone()).or_insert(i);
            by_path.insert(e.path.clone(), i);
            pages.push(Page {
                path: e.path.clone(),
                slug: e.slug.clone(),
                title: e.title.clone(),
                kind: e.kind.clone(),
                summary: e.summary.clone(),
                updated: e.updated,
                modified: e.modified,
                words: e.words,
                aliases: e.aliases.clone(),
                subject,
                origin: e.origin.clone(),
            });
        }
        let mut corpus = Corpus {
            out: vec![vec![]; pages.len()],
            inc: vec![vec![]; pages.len()],
            pages,
            subjects,
            links: vec![],
            by_slug,
            by_path,
        };
        for (from, e) in entries.iter().enumerate() {
            for r in &e.refs {
                let Some(to) = corpus.resolve(r) else {
                    continue;
                };
                if to != from && !corpus.out[from].contains(&to) {
                    corpus.out[from].push(to);
                    corpus.inc[to].push(from);
                    corpus.links.push((from, to));
                }
            }
        }
        corpus
    }

    /// The page a link names: a slug, a path, or a title or alias.
    pub fn resolve(&self, written: &str) -> Option<usize> {
        let written = written.trim();
        let target = written.split('#').next().unwrap_or(written).trim();
        if target.is_empty() {
            return None;
        }
        let slug = slug_of_ref(target);
        if let Some(&i) = self.by_slug.get(&slug) {
            return Some(i);
        }
        if let Some(&i) = self.by_path.get(target) {
            return Some(i);
        }
        let lower = slug.to_lowercase();
        self.pages.iter().position(|p| {
            p.title.to_lowercase() == lower || p.aliases.iter().any(|a| a.to_lowercase() == lower)
        })
    }

    pub fn by_path(&self, path: &str) -> Option<usize> {
        self.by_path.get(path).copied()
    }

    /// The pages whose title, aliases or name hold every word asked, best
    /// first: a title that starts with the words, then one that holds them,
    /// then an alias, then the file's name; understanding before evidence,
    /// shorter titles before longer. The search for when Citadel is away.
    pub fn local_search(&self, query: &str, most: usize) -> Vec<usize> {
        let query = query.trim().to_lowercase();
        let words: Vec<&str> = query.split_whitespace().collect();
        if words.is_empty() {
            return vec![];
        }
        let holds = |text: &str| {
            let text = text.to_lowercase();
            words.iter().all(|w| text.contains(w))
        };
        let mut found: Vec<(u8, bool, usize, usize)> = self
            .pages
            .iter()
            .enumerate()
            .filter_map(|(i, p)| {
                let rank = if p.title.to_lowercase().starts_with(&query) {
                    0
                } else if holds(&p.title) {
                    1
                } else if p.aliases.iter().any(|a| holds(a)) {
                    2
                } else if holds(&p.slug.replace('-', " ")) {
                    3
                } else {
                    return None;
                };
                Some((rank, p.is_evidence(), p.title.len(), i))
            })
            .collect();
        found.sort();
        found.into_iter().take(most).map(|f| f.3).collect()
    }

    /// The pages one link away either way, out first, each once.
    pub fn neighbours(&self, page: usize) -> Vec<usize> {
        let mut all = self.out[page].clone();
        for &i in &self.inc[page] {
            if !all.contains(&i) {
                all.push(i);
            }
        }
        all
    }
}

/// The subject each path is drawn in, in the order given.
///
/// A page sits in `collection/folder`. Records are kept by who produced them
/// (`evidence/sources/<subject>/…`, `evidence/desk/<project>/…`), so the
/// folder under the producer names the subject the record supports; where
/// another collection has a subject of that name the record is drawn with
/// it, and otherwise it stays with its producer.
pub fn subjects_of<'a>(paths: impl Iterator<Item = &'a str>) -> Vec<String> {
    let paths: Vec<&str> = paths.collect();
    let own = |path: &str| -> String {
        let parts: Vec<&str> = path.split('/').collect();
        if parts.len() > 2 {
            format!("{}/{}", parts[0], parts[1])
        } else {
            parts[0].to_string()
        }
    };
    let rank = |collection: &str| {
        HOME_ORDER
            .iter()
            .position(|c| *c == collection)
            .unwrap_or(HOME_ORDER.len())
    };
    let mut homes: HashMap<String, String> = HashMap::new();
    for path in &paths {
        let id = own(path);
        let Some((collection, name)) = id.split_once('/') else {
            continue;
        };
        let better = match homes.get(name) {
            Some(held) => rank(collection) < rank(held.split('/').next().unwrap_or("")),
            None => true,
        };
        if better {
            homes.insert(name.to_string(), id.clone());
        }
    }
    paths
        .iter()
        .map(|path| {
            let parts: Vec<&str> = path.split('/').collect();
            if parts.len() >= 4
                && let Some(home) = homes.get(parts[2])
                && home.split('/').next() != Some(parts[0])
            {
                return home.clone();
            }
            own(path)
        })
        .collect()
}

/// `knowledge/game-systems` as `Game systems`.
pub fn subject_name(id: &str) -> String {
    let last = id.rsplit('/').next().unwrap_or(id).replace(['-', '_'], " ");
    let mut chars = last.trim().chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => id.to_string(),
    }
}

/// A file the walk found, with what tells whether it changed.
struct Found {
    path: String,
    full: PathBuf,
    size: u64,
    modified: u64,
}

/// Every page file under the collections, as `lib/brain.js` walks them.
fn walk(root: &Path) -> std::io::Result<Vec<Found>> {
    let mut found = vec![];
    for top in COLLECTIONS {
        let start = root.join(top);
        if !start.is_dir() {
            continue;
        }
        let mut stack = vec![(start.clone(), top.to_string())];
        while let Some((dir, rel)) = stack.pop() {
            for entry in std::fs::read_dir(&dir)? {
                let entry = entry?;
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with('.') {
                    continue;
                }
                let full = entry.path();
                let child = format!("{rel}/{name}");
                let ty = entry.file_type()?;
                if top == "projects" {
                    // A registered folder has one home; its README and
                    // repository links stay out.
                    if dir == start && ty.is_dir() {
                        stack.push((full, child));
                    } else if dir != start
                        && ty.is_file()
                        && Some(name.as_str())
                            == dir
                                .file_name()
                                .and_then(|d| d.to_str())
                                .map(|d| format!("project-{d}.md"))
                                .as_deref()
                    {
                        found.push(found_file(full, child)?);
                    }
                } else if ty.is_dir() {
                    stack.push((full, child));
                } else if ty.is_file() && name.ends_with(".md") && !is_meta(&child) {
                    found.push(found_file(full, child)?);
                }
            }
        }
    }
    Ok(found)
}

fn found_file(full: PathBuf, path: String) -> std::io::Result<Found> {
    let meta = std::fs::metadata(&full)?;
    let modified = meta
        .modified()
        .ok()
        .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    Ok(Found {
        path,
        full,
        size: meta.len(),
        modified,
    })
}

/// `MAP.md`, `FACETS.md` and the generated maps describe the brain; they
/// are not pages.
fn is_meta(path: &str) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    parts[0] == "knowledge" && (parts.len() == 2 || parts[1] == "maps")
}

/// Reads every page under `root`, reusing `known` for files that have not
/// changed. Reads run on several threads, which is what makes a first scan
/// across a network share bearable.
pub fn scan(root: &Path, known: &[Entry]) -> std::io::Result<Vec<Entry>> {
    if !root.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("{} is not a folder", root.display()),
        ));
    }
    let found = walk(root)?;
    let known: HashMap<&str, &Entry> = known.iter().map(|e| (e.path.as_str(), e)).collect();
    let mut entries: Vec<Option<Entry>> = found
        .iter()
        .map(|f| {
            known
                .get(f.path.as_str())
                .filter(|e| e.size == f.size && e.modified == f.modified)
                .map(|e| (*e).clone())
        })
        .collect();
    let todo: Vec<usize> = (0..found.len()).filter(|&i| entries[i].is_none()).collect();
    let next = AtomicUsize::new(0);
    let read: Mutex<Vec<(usize, Entry)>> = Mutex::new(vec![]);
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(2, 16);
    std::thread::scope(|s| {
        for _ in 0..threads.min(todo.len().max(1)) {
            s.spawn(|| {
                loop {
                    let k = next.fetch_add(1, Ordering::Relaxed);
                    let Some(&i) = todo.get(k) else { break };
                    let f = &found[i];
                    // A file that went between the walk and the read is
                    // simply not a page any more.
                    let Ok(bytes) = std::fs::read(&f.full) else {
                        continue;
                    };
                    let text = String::from_utf8_lossy(&bytes);
                    let mut entry = parse(&f.path, &text);
                    entry.size = f.size;
                    entry.modified = f.modified;
                    read.lock().unwrap().push((i, entry));
                }
            });
        }
    });
    for (i, entry) in read.into_inner().unwrap() {
        entries[i] = Some(entry);
    }
    Ok(entries.into_iter().flatten().collect())
}

/// A page's text without its frontmatter.
pub fn body(text: &str) -> &str {
    split_frontmatter(text).1
}

fn split_frontmatter(text: &str) -> (&str, &str) {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let Some(rest) = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
    else {
        return ("", text);
    };
    let mut at = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end() == "---" {
            return (&rest[..at], &rest[at + line.len()..]);
        }
        at += line.len();
    }
    ("", text)
}

/// One page's fields and links, from its path and text.
pub fn parse(path: &str, text: &str) -> Entry {
    let (front, body) = split_frontmatter(text);
    let fields = frontmatter(front);
    let one = |key: &str| {
        fields
            .get(key)
            .and_then(|v| v.first().cloned())
            .unwrap_or_default()
    };
    let slug = slug_of_path(path);
    let title = body
        .lines()
        .find_map(|l| l.strip_prefix("# ").or_else(|| l.strip_prefix("#\t")))
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| slug.clone());
    let kind = match one("kind") {
        k if k.is_empty() => "note".to_string(),
        k => k,
    };
    Entry {
        path: path.to_string(),
        size: 0,
        modified: 0,
        title,
        kind,
        summary: one("summary"),
        updated: julian(&one("updated")),
        words: body.split_whitespace().count() as u32,
        aliases: fields.get("aliases").cloned().unwrap_or_default(),
        refs: links_of(body, path),
        origin: one("origin"),
        slug,
    }
}

/// `2026-10-22` as a Julian day.
pub fn julian(date: &str) -> Option<i32> {
    let mut parts = date.trim().get(..10)?.split('-');
    let y: i32 = parts.next()?.parse().ok()?;
    let m: u8 = parts.next()?.parse().ok()?;
    let d: u8 = parts.next()?.parse().ok()?;
    let month = time::Month::try_from(m).ok()?;
    time::Date::from_calendar_date(y, month, d)
        .ok()
        .map(|d| d.to_julian_day())
}

/// The fields of a frontmatter block: each key's value, or its list.
fn frontmatter(front: &str) -> HashMap<String, Vec<String>> {
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    let mut list_key: Option<String> = None;
    for line in front.lines() {
        if let Some(key) = &list_key
            && let Some(item) = line.trim_start().strip_prefix("- ")
            && line.starts_with([' ', '-'])
        {
            out.entry(key.clone()).or_default().push(unquote(item));
            continue;
        }
        list_key = None;
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        if key.starts_with(' ') || key.trim().is_empty() {
            continue;
        }
        let key = key.trim().to_string();
        let value = value.trim();
        if value.is_empty() {
            list_key = Some(key);
            continue;
        }
        let values = if value.starts_with('[') {
            serde_json::from_str::<Vec<String>>(value).unwrap_or_else(|_| {
                value
                    .trim_start_matches('[')
                    .trim_end_matches(']')
                    .split(',')
                    .map(unquote)
                    .filter(|v| !v.is_empty())
                    .collect()
            })
        } else {
            vec![unquote(value)]
        };
        out.insert(key, values);
    }
    out
}

fn unquote(value: &str) -> String {
    let v = value.trim();
    if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
        return serde_json::from_str::<String>(v).unwrap_or_else(|_| v[1..v.len() - 1].into());
    }
    if v.len() >= 2 && v.starts_with('\'') && v.ends_with('\'') {
        return v[1..v.len() - 1].replace("''", "'");
    }
    v.to_string()
}

/// A page's slug: its file name, or its folder's for a `README.md`.
pub fn slug_of_path(path: &str) -> String {
    let mut parts = path.rsplit('/');
    let file = parts.next().unwrap_or(path);
    let stem = file.strip_suffix(".md").unwrap_or(file);
    if stem == "README" {
        parts.next().unwrap_or(stem).to_string()
    } else {
        stem.to_string()
    }
}

fn slug_of_ref(written: &str) -> String {
    let last = written.trim().rsplit('/').next().unwrap_or(written);
    last.strip_suffix(".md").unwrap_or(last).to_string()
}

/// The text with fenced blocks and inline code blanked: code shows syntax,
/// it does not link.
fn without_code(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut fence: Option<&str> = None;
    for line in body.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if let Some(open) = fence {
            if trimmed.starts_with(open) {
                fence = None;
            }
            out.push('\n');
            continue;
        }
        if let Some(open) = ["```", "~~~"].into_iter().find(|o| trimmed.starts_with(o)) {
            fence = Some(open);
            out.push('\n');
            continue;
        }
        let mut in_code = false;
        for c in line.chars() {
            if c == '`' {
                in_code = !in_code;
                out.push(' ');
            } else if in_code && c != '\n' {
                out.push(' ');
            } else {
                out.push(c);
            }
        }
    }
    out
}

/// The slugs a page links to: `[[slug#part|label]]`, and relative `.md`
/// links resolved against the page's folder.
pub fn links_of(body: &str, path: &str) -> Vec<String> {
    let text = without_code(body);
    let mut out: Vec<String> = vec![];
    let mut add = |slug: String| {
        if !slug.is_empty() && !out.contains(&slug) {
            out.push(slug);
        }
    };
    let mut rest = text.as_str();
    while let Some(at) = rest.find("[[") {
        let after = &rest[at + 2..];
        match after.find("]]") {
            Some(end) => {
                let inner = &after[..end];
                let target = inner.split(['|', '#']).next().unwrap_or("");
                if !target.trim().is_empty() && !inner.contains('[') {
                    add(slug_of_ref(target));
                }
                rest = &after[end + 2..];
            }
            None => break,
        }
    }
    let dir: Vec<&str> = path.split('/').collect();
    let dir = &dir[..dir.len().saturating_sub(1)];
    let mut rest = text.as_str();
    while let Some(at) = rest.find("](") {
        let after = rest[at + 2..].trim_start_matches('<');
        rest = &rest[at + 2..];
        let end = after
            .find(|c: char| c == ')' || c == '>' || c.is_whitespace())
            .unwrap_or(after.len());
        let target = after[..end].split('#').next().unwrap_or("");
        if !target.ends_with(".md")
            || target.contains("://")
            || target.starts_with('/')
            || target.starts_with('~')
        {
            continue;
        }
        let mut parts: Vec<String> = dir.iter().map(|s| s.to_string()).collect();
        for seg in percent_decode(target).split('/') {
            match seg {
                "" | "." => {}
                ".." => {
                    parts.pop();
                }
                s => parts.push(s.to_string()),
            }
        }
        add(slug_of_path(&parts.join("/")));
    }
    out
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_search_ranks_titles_then_aliases_then_names() {
        let corpus = Corpus::new(vec![
            parse(
                "evidence/sources/sea/tide-log.md",
                "---\nkind: source\n---\n# Tide log",
            ),
            parse("knowledge/sea/tide-tables.md", "# Tables of the tide"),
            parse(
                "knowledge/sea/moon.md",
                "---\naliases:\n  - tide maker\n---\n# Moon",
            ),
            parse("knowledge/sea/tide.md", "# Tide"),
            parse("knowledge/sea/low-water.md", "# Ebb"),
        ]);
        let titles = |q: &str| -> Vec<String> {
            corpus
                .local_search(q, 10)
                .into_iter()
                .map(|i| corpus.pages[i].title.clone())
                .collect()
        };
        assert_eq!(
            titles("TIDE"),
            ["Tide", "Tide log", "Tables of the tide", "Moon"]
        );
        assert_eq!(titles("water low"), ["Ebb"]);
        assert!(titles("  ").is_empty());
    }

    #[test]
    fn reads_the_fields_a_page_carries() {
        let text = "---\nkind: \"topic\"\nsummary: \"Sky \\\"lit\\\" at night.\"\nfacets: [\"a\", \"b\"]\naliases:\n  - star map\n  - 'night sky'\nupdated: 2026-10-22\n---\n\n# Night sky\n\nOne two three.\n";
        let e = parse("knowledge/sky/night-sky.md", text);
        assert_eq!(e.slug, "night-sky");
        assert_eq!(e.title, "Night sky");
        assert_eq!(e.kind, "topic");
        assert_eq!(e.summary, "Sky \"lit\" at night.");
        assert_eq!(e.aliases, ["star map", "night sky"]);
        assert_eq!(e.updated, julian("2026-10-22"));
        // The title's line counts too: "# Night sky" is three.
        assert_eq!(e.words, 6);
    }

    #[test]
    fn a_page_without_frontmatter_is_a_note_named_by_its_file() {
        let e = parse("knowledge/sky/README.md", "no heading here");
        assert_eq!(
            (e.slug.as_str(), e.title.as_str(), e.kind.as_str()),
            ("sky", "sky", "note")
        );
    }

    #[test]
    fn links_are_wiki_and_relative_markdown_outside_code() {
        let body = "See [[alpha]], [[folder/beta.md#part|the beta]] and [g](../other/gamma.md).\n`[[not-a-link]]`\n```\n[[steps]]\n```\n[web](https://x.org/a.md) [[alpha]] [d](./d%20e.md)";
        assert_eq!(
            links_of(body, "knowledge/sky/page.md"),
            ["alpha", "beta", "gamma", "d e"]
        );
    }

    #[test]
    fn records_are_drawn_with_the_subject_they_support() {
        let paths = [
            "knowledge/sky/a.md",
            "projects/lantern/project-lantern.md",
            "evidence/sources/sky/s1.md",
            "evidence/desk/lantern/answer.md",
            "evidence/sources/lonely/s2.md",
            "evidence/catalogue/card.md",
            "self/pace.md",
        ];
        assert_eq!(
            subjects_of(paths.into_iter()),
            [
                "knowledge/sky",
                "projects/lantern",
                "knowledge/sky",
                "projects/lantern",
                "evidence/sources",
                "evidence/catalogue",
                "self",
            ]
        );
        assert_eq!(subject_name("knowledge/game-systems"), "Game systems");
    }

    #[test]
    fn a_corpus_resolves_links_by_slug_path_and_alias_each_once() {
        let mut a = parse(
            "knowledge/sky/a.md",
            "# A\n[[b]] [[b]] [[Gamma star]] [[a]] [[missing]]",
        );
        a.aliases = vec![];
        let b = parse("knowledge/sky/b.md", "# B\n");
        let c = parse(
            "knowledge/other/c.md",
            "---\naliases: [\"gamma star\"]\n---\n# C\n[[knowledge/sky/a.md]]",
        );
        let corpus = Corpus::new(vec![c, b, a]);
        let at = |p: &str| corpus.by_path(p).unwrap();
        let (a, b, c) = (
            at("knowledge/sky/a.md"),
            at("knowledge/sky/b.md"),
            at("knowledge/other/c.md"),
        );
        assert_eq!(corpus.out[a], [b, c]);
        assert_eq!(corpus.out[c], [a]);
        assert_eq!(corpus.inc[a], [c]);
        assert_eq!(corpus.neighbours(a), [b, c]);
        assert_eq!(corpus.subjects.len(), 2);
    }

    #[test]
    fn a_scan_reads_only_what_changed_and_skips_what_is_not_a_page() {
        let root = std::env::temp_dir().join(format!("ahjo-brain-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let w = |p: &str, t: &str| {
            let f = root.join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, t).unwrap();
        };
        w("knowledge/sky/a.md", "# A\n[[b]]");
        w("knowledge/sky/b.md", "# B");
        w("knowledge/MAP.md", "# Map");
        w("knowledge/maps/x.md", "# X");
        w("knowledge/.hidden/h.md", "# H");
        w("projects/lantern/project-lantern.md", "# Lantern");
        w("projects/lantern/README.md", "# Readme");
        w("other/z.md", "# Z");
        let first = scan(&root, &[]).unwrap();
        let mut paths: Vec<&str> = first.iter().map(|e| e.path.as_str()).collect();
        paths.sort();
        assert_eq!(
            paths,
            [
                "knowledge/sky/a.md",
                "knowledge/sky/b.md",
                "projects/lantern/project-lantern.md"
            ]
        );
        // A known entry with the same size and time is reused as it was.
        let mut known = first.clone();
        for e in &mut known {
            e.title = format!("cached {}", e.title);
        }
        let second = scan(&root, &known).unwrap();
        assert!(second.iter().all(|e| e.title.starts_with("cached ")));
        std::fs::remove_dir_all(&root).unwrap();
        // A brain that is not there is an error, not an empty sky.
        assert!(scan(&root, &[]).is_err());
    }
}
