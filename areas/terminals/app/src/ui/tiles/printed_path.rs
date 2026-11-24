//! A path a session printed, opened from a click on it ([`links`]): in
//! Files in the window's panel, on the session's device, at the line and
//! column named after it.
//!
//! Most paths open as printed. Two kinds ask the device first:
//!
//! - a path an agent shortened with `…` is found by its ends, under the
//!   folder its head names, or the pane's folder when its head names none;
//! - a path with longer readings over spaces ([`links::Link::spaced`]) is
//!   the reading that names something there.
//!
//! One place found opens; several are offered in a menu at the click; none
//! is said on the status line, except that a path with spaced readings of
//! which none stands opens as printed, for Files to say it is not found.

use super::super::page::{Act, MenuItem, Terminals};
use crate::remotes::PaneAt;
use crate::term::links::{self, Link, Target};
use ahjo_app_kit::Ask;
use ahjo_core::NodeId;
use ahjo_files_contract::commands::{FileSearchPaths, FileStat};
use ahjo_terminals_contract::commands::SessionListWindows;
use gpui::{AsyncApp, Context, Pixels, Point, WeakEntity};

/// A file or folder a printed path opens, on its device, at the line and
/// column named after it (from 1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Opening {
    pub node: String,
    pub path: String,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

impl Opening {
    /// The ask that opens it beside the terminals.
    pub(in crate::ui) fn ask(self) -> Ask {
        let node = NodeId(self.node);
        match (self.line, self.column) {
            (None, _) => Ask::OpenPath {
                node,
                path: self.path,
                beside: true,
            },
            (Some(line), column) => Ask::Reveal {
                what: "path".into(),
                id: self.path,
                node: Some(node),
                part: Some(match column {
                    Some(column) => format!("{line}:{column}"),
                    None => line.to_string(),
                }),
                beside: true,
            },
        }
    }
}

/// The most places a menu offers for one printed path.
const OFFERED: usize = 8;

/// How a path shortened with `…` is found: the folder searched, the text
/// looked for, and the ends a place found keeps (`head` its start, `tail`
/// its end).
#[derive(Debug, PartialEq, Eq)]
struct Ends {
    under: String,
    query: String,
    head: String,
    tail: String,
}

impl Ends {
    /// The search for `printed`, its head read by `place`; a head that
    /// names no folder (`/…/proj`) is searched for in `folder`, the
    /// pane's.
    fn of(printed: &str, folder: Option<&str>, place: impl Fn(&str) -> String) -> Option<Ends> {
        let (head, tail) = printed.split_once('…')?;
        let (under, start) = if head.trim_matches('/').is_empty() {
            let folder = folder?.trim_end_matches('/').to_string();
            (folder.clone(), format!("{folder}/"))
        } else if head.ends_with('/') {
            let under = place(head);
            (under.clone(), format!("{}/", under.trim_end_matches('/')))
        } else {
            let start = place(head);
            let under = start.rsplit_once('/')?.0.to_string();
            (if under.is_empty() { "/".into() } else { under }, start)
        };
        let query = match tail.trim_matches('/') {
            "" => start.rsplit('/').next().unwrap_or_default().to_string(),
            name => name.to_string(),
        };
        (!query.is_empty()).then(|| Ends {
            under,
            query,
            head: start,
            tail: tail.to_string(),
        })
    }

    /// The places `found` (paths the search answered) show that start
    /// with the head and end with the tail, each once: the file itself,
    /// or the folder it is in that the tail names. With no tail, the
    /// shortened name is completed and nothing below it taken.
    fn named(&self, found: &[String]) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for path in found {
            let ends = path
                .char_indices()
                .filter(|(_, c)| *c == '/')
                .map(|(i, _)| i)
                .chain([path.len()]);
            for end in ends {
                let place = &path[..end];
                let middle = place.len() >= self.head.len() + self.tail.len();
                if middle
                    && place.starts_with(&self.head)
                    && place.ends_with(&self.tail)
                    && (!self.tail.is_empty() || !place[self.head.len()..].contains('/'))
                {
                    if !out.iter().any(|p| p == place) {
                        out.push(place.to_string());
                    }
                    break;
                }
            }
        }
        out
    }
}

/// What the device answered for a printed path.
enum Found {
    /// The places it names.
    Places(Vec<String>),
    /// Nothing; the words say where it was looked for.
    Nothing(String),
}

impl Terminals {
    /// Opens the path link `link` clicked at `at` in the window showing
    /// `pane`. A relative path is the folder's of the pane the window
    /// shows, which tmux is asked for, as the session's own folder is its
    /// active pane's; a `~` its folder does not tell is the node's home.
    pub(super) fn open_printed(
        &mut self,
        pane: PaneAt,
        link: Link,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let Target::Path(printed, line) = link.target else {
            return;
        };
        let session = self.fleet.session_of_pane(&pane.node, pane.pane);
        let cwd = session.and_then(|s| s.cwd.clone());
        let id = session.map(|s| s.id.clone()).unwrap_or_default();
        let home = self.home_of(&pane.node);
        let node = NodeId(pane.node.clone());
        let relative = std::iter::once(&printed)
            .chain(&link.spaced)
            .any(|p| !p.starts_with(['/', '~']));
        let windows = (relative && !id.is_empty()).then(|| {
            self.host
                .call(node.clone(), SessionListWindows { session: id }, cx)
        });
        let asks = printed.contains('…') || !link.spaced.is_empty();
        if windows.is_none() && !asks {
            let path = links::in_home(&links::resolve(cwd.as_deref(), &printed), home.as_deref());
            let opening = Opening {
                node: pane.node,
                path,
                line,
                column: link.column,
            };
            return self.ask_link(opening.ask(), cx);
        }
        self.menu_at = at;
        let column = link.column;
        cx.spawn(async move |this, cx| {
            let folder = match windows {
                Some(task) => task
                    .await
                    .ok()
                    .and_then(|a| a.windows.into_iter().find(|w| w.pane == pane.pane))
                    .map(|w| w.cwd)
                    .or(cwd),
                None => cwd,
            };
            let place =
                |p: &str| links::in_home(&links::resolve(folder.as_deref(), p), home.as_deref());
            let found = if printed.contains('…') {
                match Ends::of(&printed, folder.as_deref(), place) {
                    Some(ends) => find(&this, &node, &printed, ends, cx).await,
                    None => Found::Nothing(format!("{printed} names no file")),
                }
            } else if link.spaced.is_empty() {
                Found::Places(vec![place(&printed)])
            } else {
                // The word as printed is a reading too: where both stand,
                // both are offered.
                let readings: Vec<String> = link
                    .spaced
                    .iter()
                    .chain([&printed])
                    .map(|p| place(p))
                    .collect();
                match stat(&this, &node, readings, cx).await {
                    found if found.is_empty() => Found::Places(vec![place(&printed)]),
                    found => Found::Places(found),
                }
            };
            let opening = |path| Opening {
                node: pane.node.clone(),
                path,
                line,
                column,
            };
            let _ = this.update(cx, |s, cx| match found {
                Found::Nothing(words) => {
                    s.report(&words, true, None);
                    s.flush(cx);
                    cx.notify();
                }
                Found::Places(places) if places.len() == 1 => {
                    let path = places.into_iter().next().expect("one");
                    s.ask_link(opening(path).ask(), cx)
                }
                Found::Places(places) => {
                    let mut items: Vec<MenuItem> = places
                        .iter()
                        .take(OFFERED)
                        .map(|p| MenuItem::new(p.clone(), Act::OpenPrinted(opening(p.clone()))))
                        .collect();
                    if places.len() > OFFERED {
                        let more = places.len() - OFFERED;
                        items.push(MenuItem::note(format!("and {more} more")));
                    }
                    let rows = items.into_iter().map(|i| i.row(&s.me)).collect();
                    s.host.menu(rows, s.menu_at, cx);
                }
            });
        })
        .detach();
    }
}

/// The places a shortened path names, found on its device.
async fn find(
    this: &WeakEntity<Terminals>,
    node: &NodeId,
    printed: &str,
    ends: Ends,
    cx: &mut AsyncApp,
) -> Found {
    let search = FileSearchPaths {
        query: ends.query.clone(),
        under: Some(ends.under.clone()),
        near: None,
        beside: None,
    };
    let Ok(task) = this.update(cx, |s, cx| s.host.call(node.clone(), search, cx)) else {
        return Found::Places(Vec::new());
    };
    match task.await {
        Ok(found) => {
            let paths: Vec<String> = found
                .matches
                .into_iter()
                .map(|m| m.path.replace('\\', "/"))
                .collect();
            match ends.named(&paths) {
                places if places.is_empty() => {
                    Found::Nothing(format!("{printed} names no file under {}", ends.under))
                }
                places => Found::Places(places),
            }
        }
        Err(e) => Found::Nothing(format!("Could not look for {printed}: {}", e.message)),
    }
}

/// Which of `readings` stand on the device, in their order.
async fn stat(
    this: &WeakEntity<Terminals>,
    node: &NodeId,
    readings: Vec<String>,
    cx: &mut AsyncApp,
) -> Vec<String> {
    let asked = FileStat {
        paths: readings.clone(),
    };
    let Ok(task) = this.update(cx, |s, cx| s.host.call(node.clone(), asked, cx)) else {
        return Vec::new();
    };
    match task.await {
        Ok(stated) => readings
            .into_iter()
            .zip(stated.times)
            .filter_map(|(path, time)| time.map(|_| path))
            .collect(),
        Err(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ends(printed: &str, folder: &str) -> Ends {
        Ends::of(printed, Some(folder), |p| links::resolve(Some(folder), p)).expect("a search")
    }

    #[test]
    fn a_shortened_path_is_looked_for_under_the_folder_its_head_names() {
        // Claude Code's tool line, from the pane's folder.
        let e = ends(
            "areas/termin…/long_file_name_for_wrapping_tests.rs",
            "/w/proj",
        );
        assert_eq!(
            e,
            Ends {
                under: "/w/proj/areas".into(),
                query: "long_file_name_for_wrapping_tests.rs".into(),
                head: "/w/proj/areas/termin".into(),
                tail: "/long_file_name_for_wrapping_tests.rs".into(),
            }
        );
        let found = [
            "/w/proj/areas/terminals/app/src/term/deeply/nested/folder/long_file_name_for_wrapping_tests.rs",
            "/w/proj/areas/files/long_file_name_for_wrapping_tests.rs",
            "/w/proj/areas/terminals/long_file_name_for_wrapping_tests.rs.bak",
        ]
        .map(String::from);
        assert_eq!(e.named(&found), vec![found[0].clone()]);

        // Codex's header: a folder in the middle left out.
        let e = ends("/tmp/claude-1000/-srv-ahjo/…/scratchpad/codex", "/srv");
        assert_eq!(e.under, "/tmp/claude-1000/-srv-ahjo");
        assert_eq!(e.query, "scratchpad/codex");
        let found = [
            "/tmp/claude-1000/-srv-ahjo/aee9/scratchpad/codex/out84.txt",
            "/tmp/claude-1000/-srv-ahjo/aee9/scratchpad/codex/tmux.log",
            "/tmp/claude-1000/-srv-ahjo/b2c4/scratchpad/codex-old/x.txt",
        ]
        .map(String::from);
        assert_eq!(
            e.named(&found),
            vec!["/tmp/claude-1000/-srv-ahjo/aee9/scratchpad/codex".to_string()]
        );
    }

    #[test]
    fn a_name_cut_at_its_end_is_completed_and_two_completions_are_both_offered() {
        let e = ends("/tmp/c/-srv-parvi/4bb7cac5-c5e6-4cf…", "/srv");
        assert_eq!(e.under, "/tmp/c/-srv-parvi");
        assert_eq!(e.query, "4bb7cac5-c5e6-4cf");
        let found = [
            "/tmp/c/-srv-parvi/4bb7cac5-c5e6-4cf2-b148/scratchpad/a.txt",
            "/tmp/c/-srv-parvi/4bb7cac5-c5e6-4cf2-b148/notes.txt",
        ]
        .map(String::from);
        assert_eq!(
            e.named(&found),
            vec!["/tmp/c/-srv-parvi/4bb7cac5-c5e6-4cf2-b148".to_string()]
        );
        let found = [
            "/tmp/c/-srv-parvi/4bb7cac5-c5e6-4cf2/a.txt",
            "/tmp/c/-srv-parvi/4bb7cac5-c5e6-4cf9/b.txt",
        ]
        .map(String::from);
        assert_eq!(e.named(&found).len(), 2);
    }

    #[test]
    fn a_head_that_names_no_folder_is_looked_for_in_the_panes() {
        let e = ends("/…/scratchpad/links/proj", "/w/claude");
        assert_eq!(e.under, "/w/claude");
        assert_eq!(e.head, "/w/claude/");
        let found = ["/w/claude/4bb7/scratchpad/links/proj/notes.txt".to_string()];
        assert_eq!(
            e.named(&found),
            vec!["/w/claude/4bb7/scratchpad/links/proj".to_string()]
        );
        // Nothing found says so: no place, not a guess.
        assert!(e.named(&["/w/claude/other/x.txt".to_string()]).is_empty());
    }

    #[test]
    fn a_line_and_column_go_with_the_path() {
        let opening = Opening {
            node: "n".into(),
            path: "/a/b.rs".into(),
            line: Some(7),
            column: Some(3),
        };
        assert!(matches!(
            opening.ask(),
            Ask::Reveal { part: Some(p), .. } if p == "7:3"
        ));
    }
}
