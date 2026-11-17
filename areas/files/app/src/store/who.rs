//! Which live sessions work where a file is: those whose folder holds it,
//! the nearest first. A session's folder is where its agent writes, so
//! this is who most likely changed the file; it is a guess the page shows
//! as one, never a record.
use super::paths;
use ahjo_terminals_contract::Session;

/// The sessions on `node` whose folder holds `path` (or is it), the
/// deepest folder first, then the one active most recently.
pub fn working_at<'a>(sessions: &'a [Session], node: &str, path: &str) -> Vec<&'a Session> {
    let mut found: Vec<&Session> = sessions
        .iter()
        .filter(|s| s.node == node)
        .filter(|s| {
            s.cwd
                .as_deref()
                .is_some_and(|cwd| cwd == path || paths::inside(cwd, path))
        })
        .collect();
    found.sort_by_key(|s| {
        (
            std::cmp::Reverse(s.cwd.as_deref().map_or(0, str::len)),
            std::cmp::Reverse(s.last.unwrap_or(0)),
        )
    });
    found
}

/// The folder a changed path is grouped under: the nearest session folder
/// holding it, or else the folder one level under its root.
pub fn group_of(sessions: &[Session], node: &str, root: Option<&str>, path: &str) -> String {
    if let Some(s) = working_at(sessions, node, path).first()
        && let Some(cwd) = &s.cwd
    {
        return cwd.clone();
    }
    match root {
        Some(root) if paths::inside(root, path) && root != path => {
            let rest = &path[paths::join(root, "").len()..];
            let first = rest
                .split(paths::separator(root))
                .find(|p| !p.is_empty())
                .unwrap_or_default();
            if rest == first {
                // A file straight under the root is its own group's root.
                root.to_owned()
            } else {
                paths::join(root, first)
            }
        }
        _ => paths::parent(path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(name: &str, cwd: &str, last: u64) -> Session {
        Session {
            node: "n".into(),
            name: name.into(),
            cwd: Some(cwd.into()),
            last: Some(last),
            ..Default::default()
        }
    }

    #[test]
    fn the_nearest_session_folder_holding_a_file_comes_first() {
        let all = [
            session("wide", "/srv", 9),
            session("near", "/srv/ahjo", 1),
            session("near-later", "/srv/ahjo", 5),
            session("other", "/srv/korpi", 9),
        ];
        let names: Vec<_> = working_at(&all, "n", "/srv/ahjo/DESIGN.md")
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(names, ["near-later", "near", "wide"]);
        assert!(working_at(&all, "elsewhere", "/srv/ahjo/DESIGN.md").is_empty());
        assert!(working_at(&all, "n", "/srv/parvi-archive/x").len() == 1);
    }

    #[test]
    fn a_change_is_grouped_by_its_session_or_its_project_under_the_root() {
        let all = [session("a", "/srv/ahjo", 1)];
        assert_eq!(
            group_of(&all, "n", Some("/srv"), "/srv/ahjo/a/b.rs"),
            "/srv/ahjo"
        );
        assert_eq!(
            group_of(&all, "n", Some("/srv"), "/srv/korpi/docs/x.md"),
            "/srv/korpi"
        );
        assert_eq!(group_of(&all, "n", Some("/srv"), "/srv/notes.md"), "/srv");
        assert_eq!(group_of(&all, "n", None, "/a/b/c.md"), "/a/b");
    }
}
