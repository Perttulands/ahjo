//! The inbox notes that agents have filed, and where they went, as the
//! brain's history tells it.
//!
//! An agent files a note from the inbox by moving it to its place, by
//! folding its words into pages already there and taking it out, or by
//! taking it out alone. Each is one commit that deletes the inbox file, so
//! the log of deletions in `knowledge/_inbox` finds them; the note as it
//! was gives its title and where it came from (`origin`), and the commit's
//! other pages say where it went.

use crate::front;
use crate::history;
use ahjo_app_kit::Error;
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

/// The filings looked back over, newest first.
const LOOK_BACK: &str = "-n200";

/// An inbox note that an agent filed.
#[derive(Clone, Debug, PartialEq)]
pub struct Filed {
    /// Its file in the inbox, as it was.
    pub path: String,
    pub title: String,
    /// Where it was caught: `ahjo-brain` here, `chatgpt`, or none said.
    pub origin: String,
    /// When it was filed, in seconds since 1970.
    pub when: i64,
    /// The pages it went to: where it was moved, or the pages its words
    /// were folded into. None: it was taken out alone.
    pub to: Vec<String>,
    /// It was moved whole to `to`'s one page.
    pub moved: bool,
}

/// One commit that took notes out of the inbox.
#[derive(Clone, Debug, PartialEq)]
struct Out {
    hash: String,
    when: i64,
    gone: Vec<String>,
}

/// The inbox notes that agents filed, newest first. Blocking: run it
/// off the UI.
pub fn read(root: &Path) -> Result<Vec<Filed>, Error> {
    let out = run(
        history::git(root).args([
            "log",
            LOOK_BACK,
            "--format=%x01%H %at",
            "--name-status",
            "--diff-filter=D",
            "--",
            front::INBOX,
        ]),
        None,
    )?;
    let outs = parse_log(&out);
    if outs.is_empty() {
        return Ok(vec![]);
    }
    // Each note as it was just before it went: its title and origin.
    let asked: String = outs
        .iter()
        .flat_map(|o| o.gone.iter().map(move |p| format!("{}^:{p}\n", o.hash)))
        .collect();
    let texts = parse_batch(&run(
        history::git(root).args(["cat-file", "--batch"]),
        Some(asked.as_bytes()),
    )?);
    let mut notes: Vec<(&Out, &String, String, String)> = vec![];
    let mut k = 0;
    for o in &outs {
        for p in &o.gone {
            let text = texts.get(k).cloned().flatten().unwrap_or_default();
            k += 1;
            let (head, body) = front::split(&text);
            let origin = front::field(&head.fields, "origin").unwrap_or_default();
            notes.push((o, p, front::title(body), origin.to_string()));
        }
    }
    // Where the commits that filed them put them.
    let mut hashes: Vec<&str> = notes.iter().map(|m| m.0.hash.as_str()).collect();
    hashes.dedup();
    let shown = run(
        history::git(root)
            .args(["show", "-M", "--format=%x01%H", "--name-status"])
            .args(&hashes),
        None,
    )?;
    let changes = parse_show(&shown);
    Ok(notes
        .into_iter()
        .map(|(o, p, title, origin)| {
            let (to, moved) = changes.get(&o.hash).map(|c| went(c, p)).unwrap_or_default();
            Filed {
                path: p.clone(),
                title,
                origin,
                when: o.when,
                to,
                moved,
            }
        })
        .collect())
}

fn run(cmd: &mut std::process::Command, input: Option<&[u8]>) -> Result<String, Error> {
    use std::process::Stdio;
    let mut child = cmd
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Error::new("git_unavailable", format!("git did not run: {e}")))?;
    if let (Some(bytes), Some(mut stdin)) = (input, child.stdin.take()) {
        // Written from another thread, so a long answer cannot stall it.
        let bytes = bytes.to_vec();
        std::thread::spawn(move || {
            let _ = stdin.write_all(&bytes);
        });
    }
    let out = child
        .wait_with_output()
        .map_err(|e| Error::new("git_failed", format!("git did not answer: {e}")))?;
    if !out.status.success() {
        return Err(Error::new(
            "git_failed",
            format!(
                "git could not read the brain's history: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ),
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `git log --format=%x01%H %at --name-status --diff-filter=D`.
fn parse_log(out: &str) -> Vec<Out> {
    out.split('\u{1}')
        .filter_map(|block| {
            let mut lines = block.lines();
            let (hash, when) = lines.next()?.split_once(' ')?;
            let gone = lines
                .filter_map(|l| l.strip_prefix("D\t"))
                .filter(|p| p.ends_with(".md"))
                .map(str::to_string)
                .collect::<Vec<_>>();
            (!gone.is_empty()).then(|| Out {
                hash: hash.to_string(),
                when: when.trim().parse().unwrap_or(0),
                gone,
            })
        })
        .collect()
}

/// `git cat-file --batch`'s answers, in the order asked: each object's
/// text, or none where it was missing.
fn parse_batch(out: &str) -> Vec<Option<String>> {
    let mut found = vec![];
    let mut rest = out;
    while let Some(nl) = rest.find('\n') {
        let header = &rest[..nl];
        rest = &rest[nl + 1..];
        if header.ends_with(" missing") || header.ends_with(" ambiguous") {
            found.push(None);
            continue;
        }
        let size: usize = header
            .rsplit(' ')
            .next()
            .and_then(|n| n.parse().ok())
            .unwrap_or(0);
        let end = size.min(rest.len());
        // A size counts bytes; a text cut inside a letter is read up to it.
        let end = (0..=end)
            .rev()
            .find(|&i| rest.is_char_boundary(i))
            .unwrap_or(0);
        found.push(Some(rest[..end].to_string()));
        rest = rest.get(end..).unwrap_or("");
        rest = rest.strip_prefix('\n').unwrap_or(rest);
    }
    found
}

/// One change of a commit, as `--name-status` tells it.
#[derive(Clone, Debug, PartialEq)]
enum Change {
    Moved(String, String),
    Changed(String),
    Gone(String),
}

/// `git show -M --format=%x01%H --name-status <commits>`, by commit.
fn parse_show(out: &str) -> HashMap<String, Vec<Change>> {
    out.split('\u{1}')
        .filter_map(|block| {
            let mut lines = block.lines();
            let hash = lines.next()?.trim().to_string();
            let changes = lines
                .filter_map(|l| {
                    let mut f = l.split('\t');
                    let status = f.next()?;
                    let a = f.next()?.to_string();
                    Some(match status.chars().next()? {
                        'R' => Change::Moved(a, f.next()?.to_string()),
                        'D' => Change::Gone(a),
                        _ => Change::Changed(a),
                    })
                })
                .collect();
            (!hash.is_empty()).then_some((hash, changes))
        })
        .collect()
}

/// Where the note at `path` went in a commit's changes: the page it was
/// moved to, or the pages changed beside its going (the inbox's and the
/// brain's maps left out).
fn went(changes: &[Change], path: &str) -> (Vec<String>, bool) {
    if let Some(to) = changes.iter().find_map(|c| match c {
        Change::Moved(from, to) if from == path => Some(to.clone()),
        _ => None,
    }) {
        return (vec![to], true);
    }
    let pages = changes
        .iter()
        .filter_map(|c| match c {
            Change::Changed(p) | Change::Moved(_, p) => Some(p),
            Change::Gone(_) => None,
        })
        .filter(|p| p.ends_with(".md") && !p.starts_with(front::INBOX))
        .filter(|p| !p.starts_with("knowledge/maps/") && *p != "knowledge/MAP.md")
        .cloned()
        .collect();
    (pages, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_finds_notes_taken_out_of_the_inbox() {
        let log = "\u{1}b7d4 1794636300\n\nD\tknowledge/_inbox/basil.md\n\u{1}bb71 1794636000\n\nD\tknowledge/_inbox/kelp.md\nD\tknowledge/_inbox/kelp.png\n";
        assert_eq!(
            parse_log(log),
            vec![
                Out {
                    hash: "b7d4".into(),
                    when: 1794636300,
                    gone: vec!["knowledge/_inbox/basil.md".into()],
                },
                Out {
                    hash: "bb71".into(),
                    when: 1794636000,
                    gone: vec!["knowledge/_inbox/kelp.md".into()],
                },
            ]
        );
    }

    #[test]
    fn a_batch_answer_is_cut_by_its_sizes() {
        let out = "aaa blob 13\n---\nkind: é\n\nbbb missing\nccc blob 3\nabc\n";
        assert_eq!(
            parse_batch(out),
            vec![Some("---\nkind: é\n".into()), None, Some("abc".into())]
        );
    }

    #[test]
    fn a_note_went_where_it_was_moved_or_folded() {
        let shown = "\u{1}bb71\n\nR100\tknowledge/_inbox/kelp.md\tknowledge/sailing/kelp.md\n\u{1}b7d4\n\nD\tknowledge/_inbox/basil.md\nM\tknowledge/garden/herbs.md\nM\tknowledge/MAP.md\nD\tknowledge/_inbox/other.md\n";
        let c = parse_show(shown);
        assert_eq!(
            went(&c["bb71"], "knowledge/_inbox/kelp.md"),
            (vec!["knowledge/sailing/kelp.md".to_string()], true)
        );
        assert_eq!(
            went(&c["b7d4"], "knowledge/_inbox/basil.md"),
            (vec!["knowledge/garden/herbs.md".to_string()], false)
        );
        assert_eq!(
            went(&[Change::Gone("x.md".into())], "x.md"),
            (vec![], false)
        );
    }

    #[test]
    fn a_brains_history_says_which_notes_were_filed_and_where() {
        let dir = std::env::temp_dir().join(format!("ahjo-brain-inbox-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let git = |args: &[&str]| {
            let ok = std::process::Command::new("git")
                .arg("-C")
                .arg(&dir)
                .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"])
                .args(args)
                .output()
                .unwrap()
                .status
                .success();
            assert!(ok, "git {args:?}");
        };
        let write = |path: &str, text: &str| {
            let file = dir.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, text).unwrap();
        };
        std::fs::create_dir_all(&dir).unwrap();
        git(&["init", "-q"]);
        let note = |title: &str, origin: &str| {
            format!("---\nkind: note\norigin: {origin}\n---\n# {title}\n\nWords.\n")
        };
        write("knowledge/_inbox/kelp.md", &note("Kelp", front::ORIGIN));
        write("knowledge/_inbox/basil.md", &note("Basil", front::ORIGIN));
        write("knowledge/_inbox/theirs.md", &note("Theirs", "chatgpt"));
        write("knowledge/garden/herbs.md", "# Herbs\n");
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "caught"]);
        std::fs::create_dir_all(dir.join("knowledge/sailing")).unwrap();
        git(&[
            "mv",
            "knowledge/_inbox/kelp.md",
            "knowledge/sailing/kelp.md",
        ]);
        git(&["commit", "-q", "-m", "file kelp"]);
        git(&[
            "rm",
            "-q",
            "knowledge/_inbox/basil.md",
            "knowledge/_inbox/theirs.md",
        ]);
        write("knowledge/garden/herbs.md", "# Herbs\n\nBasil too.\n");
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "fold basil"]);
        let filed = read(&dir).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        let seen: Vec<(&str, &str, &str, Vec<&str>, bool)> = filed
            .iter()
            .map(|f| {
                (
                    f.path.as_str(),
                    f.title.as_str(),
                    f.origin.as_str(),
                    f.to.iter().map(String::as_str).collect(),
                    f.moved,
                )
            })
            .collect();
        assert_eq!(
            seen,
            vec![
                (
                    "knowledge/_inbox/basil.md",
                    "Basil",
                    front::ORIGIN,
                    vec!["knowledge/garden/herbs.md"],
                    false
                ),
                (
                    "knowledge/_inbox/theirs.md",
                    "Theirs",
                    "chatgpt",
                    vec!["knowledge/garden/herbs.md"],
                    false
                ),
                (
                    "knowledge/_inbox/kelp.md",
                    "Kelp",
                    front::ORIGIN,
                    vec!["knowledge/sailing/kelp.md"],
                    true
                ),
            ]
        );
    }
}
