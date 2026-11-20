//! The launcher's folder search: every project under the search roots,
//! so a fragment of a name finds it (`kor` is `/srv/games/korpi`). A
//! project is a folder holding `.git` or a Beads store, found three levels
//! deep, as CHROTE's workspace walk finds them.
//!
//! Walked when asked, never on a timer: the first ask walks; a later one
//! answers the last walk at once and, if it is older than [`FRESH`], walks
//! again behind it, so a project made since shows on the next ask.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::launcher::Launcher;

/// How far under a root the walk looks: where a project's own folder sits
/// under a root such as /srv or a home. Deeper is the project's business.
const DEPTH: usize = 3;

/// How long a walk answers without another behind it.
const FRESH: Duration = Duration::from_secs(10);

/// Words that, anywhere in a folder's name, say it was set aside: a copy
/// kept for safety, retired work, a throwaway, another project's code, or
/// worktrees (checkouts of a project already listed).
const SET_ASIDE: &[&str] = &[
    "backup",
    "backups",
    "archive",
    "archived",
    "attic",
    "retired",
    "scratch",
    "tmp",
    "upstream",
    "upstreams",
    "vendor",
    "worktree",
    "worktrees",
];

/// Whether the walk neither lists nor enters a folder by this name:
/// dot-folders, dependencies, Windows' AppData and set-aside folders,
/// worktrees among them (checkouts of a project already listed).
fn skipped(name: &str) -> bool {
    if name.starts_with('.') || name == "node_modules" || name.eq_ignore_ascii_case("appdata") {
        return true;
    }
    name.to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|word| SET_ASIDE.contains(&word))
}

fn holds_project(dir: &Path) -> bool {
    dir.join(".git").symlink_metadata().is_ok() || dir.join(".beads").is_dir()
}

/// Every project from `dir` down to the depth limit. A symlinked folder is
/// not entered: it is walked where it really is, or is outside the roots.
fn walk(dir: &Path, depth: usize, found: &mut Vec<PathBuf>) {
    if holds_project(dir) {
        found.push(dir.to_path_buf());
    }
    if depth == DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
        if is_dir && !skipped(&entry.file_name().to_string_lossy()) {
            walk(&entry.path(), depth + 1, found);
        }
    }
}

/// The projects under `roots`, each once, by path.
pub fn find(roots: &[PathBuf]) -> Vec<String> {
    let mut found = Vec::new();
    for root in roots {
        walk(root, 0, &mut found);
    }
    let mut paths: Vec<String> = found
        .into_iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    paths.sort();
    paths.dedup();
    paths
}

struct Walked {
    roots: Vec<PathBuf>,
    at: Instant,
    projects: Arc<Vec<String>>,
}

/// The projects under the launcher's search roots, as last walked.
pub struct Projects {
    launcher: Arc<Launcher>,
    last: Mutex<Option<Walked>>,
    /// A walk behind an answer is running.
    walking: Mutex<bool>,
}

impl Projects {
    pub fn new(launcher: Arc<Launcher>) -> Projects {
        Projects {
            launcher,
            last: Mutex::new(None),
            walking: Mutex::new(false),
        }
    }

    /// The roots the walk looks under now.
    pub fn roots(&self) -> Vec<PathBuf> {
        self.launcher.look();
        self.launcher
            .get()
            .search_roots(crate::launch::own_home().as_deref())
    }

    /// Every project under the search roots: the last walk if there was
    /// one of these roots, walking again behind it once it is older than
    /// [`FRESH`]; else a walk now.
    pub async fn list(self: &Arc<Self>) -> Arc<Vec<String>> {
        let roots = self.roots();
        let (known, stale) = {
            let last = self.last.lock().unwrap();
            match last.as_ref().filter(|w| w.roots == roots) {
                Some(w) => (Some(w.projects.clone()), w.at.elapsed() >= FRESH),
                None => (None, true),
            }
        };
        if let Some(projects) = known {
            if stale && !std::mem::replace(&mut *self.walking.lock().unwrap(), true) {
                let search = self.clone();
                tokio::task::spawn_blocking(move || {
                    let projects = Arc::new(find(&roots));
                    *search.last.lock().unwrap() = Some(Walked {
                        roots,
                        at: Instant::now(),
                        projects,
                    });
                    *search.walking.lock().unwrap() = false;
                });
            }
            return projects;
        }
        let walked = {
            let roots = roots.clone();
            tokio::task::spawn_blocking(move || find(&roots)).await
        };
        let projects = Arc::new(walked.unwrap_or_default());
        *self.last.lock().unwrap() = Some(Walked {
            roots,
            at: Instant::now(),
            projects: projects.clone(),
        });
        projects
    }
}

/// How well a project's path answers a query, best first; `None` if not
/// at all. Its folder's name equal to the query, then starting with it,
/// then holding it, then its whole path holding every word.
pub fn rank(path: &str, query: &str) -> Option<u8> {
    let query = query.trim().to_lowercase();
    let lower = path.to_lowercase();
    let name = lower
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default();
    if query.is_empty() {
        return Some(4);
    }
    if name == query {
        Some(0)
    } else if name.starts_with(&query) {
        Some(1)
    } else if name.contains(&query) {
        Some(2)
    } else if query.split_whitespace().all(|word| lower.contains(word)) {
        Some(3)
    } else {
        None
    }
}

/// The projects among `paths` that answer `query`, best first, then
/// shortest path, with each one's rank.
pub fn matching<'a>(paths: &'a [String], query: &str) -> Vec<(u8, &'a str)> {
    let mut found: Vec<(u8, &str)> = paths
        .iter()
        .filter_map(|path| Some((rank(path, query)?, path.as_str())))
        .collect();
    // Among equals, the shortest folder name is nearest what was typed.
    let name = |path: &str| path.rsplit(['/', '\\']).next().unwrap_or(path).len();
    found.sort_by_key(|&(rank, path)| (rank, name(path), path.len(), path));
    found
}

/// Whether a folder holds a project: Git, Beads, or both.
pub fn kinds(path: &Path) -> (bool, bool) {
    (
        path.join(".git").symlink_metadata().is_ok(),
        path.join(".beads").is_dir(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_projects_three_deep_and_skips_what_was_set_aside() {
        let root = std::env::temp_dir().join(format!("ahjo-projects-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (dir, marker) in [
            ("games/korpi", ".git"),
            ("wealth", ".beads"),
            ("a/b/c", ".git"),
            ("a/b/c/d", ".git"),
            ("old-backup/thing", ".git"),
            ("web/node_modules/pkg", ".git"),
            (".cache/thing", ".git"),
            ("plain/folder", "readme"),
        ] {
            std::fs::create_dir_all(root.join(dir).join(marker)).unwrap();
        }
        let found = find(std::slice::from_ref(&root));
        let names: Vec<String> = found
            .iter()
            .map(|p| {
                Path::new(p)
                    .strip_prefix(&root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        assert_eq!(names, ["a/b/c", "games/korpi", "wealth"]);
        assert_eq!(
            find(&[root.clone(), root.join("games")]),
            found,
            "a project under two roots is listed once"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_query_finds_its_name_first_then_names_holding_it_then_paths() {
        let paths: Vec<String> = [
            "/srv/games/korpi-old",
            "/srv/games/korpi",
            "/srv/korpikuusi",
            "/home/me/notes",
            "/srv/games/kauri",
        ]
        .map(String::from)
        .to_vec();
        let found: Vec<&str> = matching(&paths, "Korpi").iter().map(|f| f.1).collect();
        assert_eq!(
            found,
            [
                "/srv/games/korpi",
                "/srv/games/korpi-old",
                "/srv/korpikuusi"
            ],
            "its own name first, then the shortest names holding it"
        );
        assert_eq!(matching(&paths, "korpi")[0].0, 0);
        let games: Vec<&str> = matching(&paths, "games kau").iter().map(|f| f.1).collect();
        assert_eq!(games, ["/srv/games/kauri"]);
        assert!(matching(&paths, "nothing").is_empty());
        assert_eq!(matching(&paths, "").len(), paths.len());
    }

    #[test]
    fn set_aside_words_are_whole_words() {
        assert!(skipped("chrote-archive"));
        assert!(skipped("retired-2026-10-19"));
        assert!(skipped("AppData"));
        assert!(skipped("ahjo-worktrees"));
        assert!(!skipped("tmpl"));
        assert!(!skipped("korpi"));
    }
}
