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

use axum::Router;
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::routing::get;

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

struct Search {
    launcher: Arc<Launcher>,
    last: Mutex<Option<Walked>>,
    /// A walk behind an answer is running.
    walking: Mutex<bool>,
}

pub fn routes(launcher: Arc<Launcher>) -> Router {
    Router::new()
        .route("/api/terminal/projects", get(projects))
        .with_state(Arc::new(Search {
            launcher,
            last: Mutex::new(None),
            walking: Mutex::new(false),
        }))
}

/// `GET /api/terminal/projects`: every project under the search roots.
async fn projects(State(search): State<Arc<Search>>) -> Response {
    search.launcher.look();
    let roots = search
        .launcher
        .get()
        .search_roots(crate::launch::own_home().as_deref());
    let (known, stale) = {
        let last = search.last.lock().unwrap();
        match last.as_ref().filter(|w| w.roots == roots) {
            Some(w) => (Some(w.projects.clone()), w.at.elapsed() >= FRESH),
            None => (None, true),
        }
    };
    if let Some(projects) = known {
        if stale && !std::mem::replace(&mut *search.walking.lock().unwrap(), true) {
            let search = search.clone();
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
        return axum::Json(&*projects).into_response();
    }
    let walked = {
        let roots = roots.clone();
        tokio::task::spawn_blocking(move || find(&roots)).await
    };
    let projects = Arc::new(walked.unwrap_or_default());
    *search.last.lock().unwrap() = Some(Walked {
        roots,
        at: Instant::now(),
        projects: projects.clone(),
    });
    axum::Json(&*projects).into_response()
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
    fn set_aside_words_are_whole_words() {
        assert!(skipped("chrote-archive"));
        assert!(skipped("retired-2026-10-19"));
        assert!(skipped("AppData"));
        assert!(skipped("ahjo-worktrees"));
        assert!(!skipped("tmpl"));
        assert!(!skipped("korpi"));
    }
}
