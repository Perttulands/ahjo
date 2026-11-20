//! The Brain panel's state: the search as you type, what arrived lately,
//! the page in the reader and the trail that leads back. Plain data; the
//! panel draws it and makes the requests. Editing lives in Ahjo Brain.

/// How long typing must rest before a search goes out (CHROTE `SETTLE_MS`).
pub const SETTLE_MS: u64 = 200;
/// From this many results the list says it is the best of more.
pub const LIMIT: usize = 50;

pub use ahjo_brain_contract::{Found as Hit, Link, Listed, Page, PageRef};

use ahjo_brain_contract::commands::{BrainChangesResult, BrainPage, BrainSearchResults};
use ahjo_core::Error;

/// What the Brain panel keeps between app runs, in the `brain` section.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Kept {
    /// The size a page is read at, when zoomed from the theme's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read: Option<u8>,
}

#[derive(Default)]
pub struct Brain {
    /// The query the results answer, and each search's number: an answer
    /// to an older one is dropped.
    pub query: String,
    pub asked: u32,
    pub searching: bool,
    pub results: Vec<Hit>,
    pub selected: usize,
    pub search_error: Option<String>,
    /// The way back: (path, title) of every page reached by a link since a
    /// page was opened from the list, the reader's last.
    pub trail: Vec<(String, String)>,
    pub page: Option<Page>,
    pub reading: u32,
    /// The page asked for was chosen from the list: it starts a new trail.
    fresh: bool,
    pub page_error: Option<String>,
    /// Each page that arrived lately, once, newest first, and whether the
    /// node has answered yet.
    pub arrivals: Vec<Listed>,
    pub arrivals_read: bool,
    /// What git said when it would not read the corpus.
    pub git_error: Option<String>,
}

impl Brain {
    /// The query changed: a number for the search to come, or None when
    /// the query is empty and the results simply clear.
    pub fn typed(&mut self, query: &str) -> Option<u32> {
        self.asked += 1;
        self.query = query.trim().to_string();
        self.search_error = None;
        if self.query.is_empty() {
            self.results.clear();
            self.selected = 0;
            self.searching = false;
            return None;
        }
        self.searching = true;
        Some(self.asked)
    }

    /// A search answered. False if it was not the latest.
    pub fn found(&mut self, asked: u32, result: Result<BrainSearchResults, Error>) -> bool {
        if asked != self.asked {
            return false;
        }
        self.searching = false;
        match result {
            Ok(r) => {
                self.results = r.results;
                self.selected = 0;
            }
            Err(error) => self.search_error = Some(error.message),
        }
        true
    }

    /// The pages the list shows: the results while searching, else what
    /// arrived lately.
    pub fn listed(&self) -> Vec<&str> {
        if self.query.is_empty() {
            self.arrivals.iter().map(|a| a.page.path.as_str()).collect()
        } else {
            self.results.iter().map(|h| h.path.as_str()).collect()
        }
    }

    /// The listed page the list marks, which Enter opens: the one the keys
    /// or the mouse last chose, the first until then.
    pub fn chosen(&self) -> Option<usize> {
        (self.selected < self.listed().len()).then_some(self.selected)
    }

    /// Up or Down: moves the mark, held to the list.
    pub fn step(&mut self, by: isize) {
        if let Some(last) = self.listed().len().checked_sub(1) {
            self.selected = self.selected.saturating_add_signed(by).min(last);
        }
    }

    /// The marked page, if the list has one.
    pub fn marked(&self) -> Option<String> {
        self.chosen()
            .and_then(|at| self.listed().get(at).map(|p| p.to_string()))
    }

    /// The recent changes answered: each page once, at its newest change.
    pub fn changes_read(&mut self, result: Result<BrainChangesResult, Error>) {
        self.arrivals_read = true;
        match result {
            Ok(c) => {
                let mut arrivals: Vec<Listed> = vec![];
                for change in c.changes {
                    for p in change.pages {
                        if arrivals.iter().all(|a| a.page.path != p.path) {
                            arrivals.push(Listed {
                                page: p,
                                updated: change.commit.time.clone(),
                                author: change.commit.author.clone(),
                            });
                        }
                    }
                }
                self.arrivals = arrivals;
                self.git_error = c.error;
            }
            Err(error) => self.git_error = Some(error.message),
        }
    }

    /// Asking for a page, `fresh` from the list or else by a link: the
    /// number its answer must carry.
    pub fn open(&mut self, fresh: bool) -> u32 {
        self.fresh = fresh;
        self.reading += 1;
        self.reading
    }

    /// A page answered. False if another page was asked for since. A page
    /// from the list starts the trail; one reached by a link joins it, or
    /// the trail goes back to it.
    pub fn read(&mut self, reading: u32, result: Result<BrainPage, Error>) -> bool {
        if reading != self.reading {
            return false;
        }
        match result {
            Ok(answer) => {
                let p = answer.page;
                if self.fresh {
                    self.trail.clear();
                }
                match self.trail.iter().position(|(path, _)| *path == p.path) {
                    Some(at) => self.trail.truncate(at + 1),
                    None => self.trail.push((p.path.clone(), p.title.clone())),
                }
                self.page = Some(p);
                self.page_error = None;
            }
            Err(error) => self.page_error = Some(error.message),
        }
        true
    }

    pub fn close(&mut self) {
        self.page = None;
        self.trail.clear();
        self.page_error = None;
        self.reading += 1;
    }

    /// How many pages the search found, beside it. With none it says
    /// nothing: the list says so, once.
    pub fn count(&self) -> Option<String> {
        if self.searching {
            return Some("Searching…".into());
        }
        match self.results.len() {
            0 => None,
            1 => Some("1 page".into()),
            n if n >= LIMIT => Some(format!("{n} pages, best first")),
            n => Some(format!("{n} pages")),
        }
    }

    /// What Send hands a session, worded as `ahjo brain send` words it:
    /// the page's file, which an agent on its node can open.
    pub fn reference(&self) -> Option<String> {
        self.page.as_ref().map(|p| format!("brain page {}", p.file))
    }
}

/// When `iso` was, from `now`, as the Library says it: `3 days ago`.
pub fn when(iso: &str, now: jiff::Timestamp) -> String {
    let Ok(at) = iso.parse::<jiff::Timestamp>() else {
        return "never".into();
    };
    let s = (now.as_second() - at.as_second()).max(0);
    let say = |n: i64, unit: &str| format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" });
    const MINUTE: i64 = 60;
    const HOUR: i64 = 60 * MINUTE;
    const DAY: i64 = 24 * HOUR;
    const MONTH: i64 = 30 * DAY;
    const YEAR: i64 = 365 * DAY;
    match s {
        s if s < MINUTE => "just now".into(),
        s if s < HOUR => say(s / MINUTE, "minute"),
        s if s < DAY => say(s / HOUR, "hour"),
        s if s < MONTH => say(s / DAY, "day"),
        s if s < YEAR => say(s / MONTH, "month"),
        s => say(s / YEAR, "year"),
    }
}

/// A snippet's parts: (text, marked) with `»…«` marking the match.
pub fn marks(snippet: &str) -> Vec<(String, bool)> {
    let mut out = vec![];
    let mut rest = snippet;
    while let Some(open) = rest.find('»') {
        let Some(len) = rest[open..].find('«') else {
            break;
        };
        if open > 0 {
            out.push((rest[..open].to_string(), false));
        }
        let inner = &rest[open + '»'.len_utf8()..open + len];
        out.push((inner.to_string(), true));
        rest = &rest[open + len + '«'.len_utf8()..];
    }
    if !rest.is_empty() {
        out.push((rest.to_string(), false));
    }
    out
}

/// Display a snippet without Markdown syntax (headings, wiki and Markdown
/// links, emphasis, code marks), retaining match marks.
pub fn snippet_parts(snippet: &str) -> Vec<(String, bool)> {
    let chars: Vec<(char, bool)> = marks(snippet)
        .into_iter()
        .flat_map(|(text, marked)| text.chars().map(move |c| (c, marked)).collect::<Vec<_>>())
        .collect();
    let mut visible = Vec::new();
    let mut i = 0;
    let mut line_start = true;
    while i < chars.len() {
        if line_start {
            let mut end = i;
            while end < chars.len() && matches!(chars[end].0, ' ' | '\t') {
                end += 1;
            }
            let hashes = end;
            while end < chars.len() && chars[end].0 == '#' {
                end += 1;
            }
            if end > hashes && end < chars.len() && chars[end].0 == ' ' {
                i = end + 1;
                line_start = false;
                continue;
            }
        }
        // A heading's marks the snippet ran onto one line: `text ## Heading`.
        if chars[i].0 == '#' && i > 0 && chars[i - 1].0 == ' ' {
            let end = (i..chars.len().min(i + 6))
                .take_while(|&j| chars[j].0 == '#')
                .last()
                .map_or(i, |j| j + 1);
            if chars.get(end).is_some_and(|c| c.0 == ' ') {
                i = end + 1;
                continue;
            }
        }
        if chars[i].0 == '['
            && chars.get(i + 1).is_some_and(|c| c.0 == '[')
            && let Some(end) = (i + 2..chars.len().saturating_sub(1))
                .find(|&j| chars[j].0 == ']' && chars[j + 1].0 == ']')
        {
            let start = (i + 2..end)
                .find(|&j| chars[j].0 == '|')
                .map_or(i + 2, |j| j + 1);
            visible.extend_from_slice(&chars[start..end]);
            i = end + 2;
            line_start = false;
            continue;
        }
        // A link the snippet cut through shows its words: `[[` without its
        // end, an end without its start.
        if matches!(
            (chars[i].0, chars.get(i + 1).map(|c| c.0)),
            ('[', Some('[')) | (']', Some(']'))
        ) {
            i += 2;
            continue;
        }
        // A Markdown link shows its text, its target left out.
        if chars[i].0 == '['
            && let Some(close) = (i + 1..chars.len())
                .take_while(|&j| chars[j].0 != '\n' && chars[j].0 != '[')
                .find(|&j| chars[j].0 == ']')
            && chars.get(close + 1).is_some_and(|c| c.0 == '(')
        {
            visible.extend_from_slice(&chars[i + 1..close]);
            i = (close + 2..chars.len())
                .find(|&j| chars[j].0 == ')' || chars[j].0 == '\n')
                .map_or(chars.len(), |j| j + usize::from(chars[j].0 == ')'));
            line_start = false;
            continue;
        }
        // Emphasis and code marks.
        if chars[i].0 == '`' || (chars[i].0 == '*' && chars.get(i + 1).is_some_and(|c| c.0 == '*'))
        {
            i += if chars[i].0 == '`' { 1 } else { 2 };
            continue;
        }
        visible.push(chars[i]);
        line_start = chars[i].0 == '\n';
        i += 1;
    }
    let mut parts: Vec<(String, bool)> = Vec::new();
    for (ch, marked) in visible {
        if let Some((text, last)) = parts.last_mut()
            && *last == marked
        {
            text.push(ch);
        } else {
            parts.push((ch.to_string(), marked));
        }
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    use ahjo_brain_contract::{Commit, commands::BrainChange};

    fn results(paths: &[&str]) -> BrainSearchResults {
        BrainSearchResults {
            node: "n".into(),
            results: paths
                .iter()
                .map(|path| Hit {
                    path: (*path).into(),
                    title: (*path).into(),
                    ..Hit::default()
                })
                .collect(),
        }
    }

    fn page(path: &str, file: &str) -> BrainPage {
        BrainPage {
            node: "n".into(),
            page: Page {
                path: path.into(),
                title: path.into(),
                file: file.into(),
                ..Page::default()
            },
        }
    }

    fn changes(entries: &[(&str, &[&str])]) -> BrainChangesResult {
        BrainChangesResult {
            node: "n".into(),
            changes: entries
                .iter()
                .map(|(time, paths)| BrainChange {
                    commit: Commit {
                        time: (*time).into(),
                        ..Commit::default()
                    },
                    paths: paths.join(", "),
                    pages: paths
                        .iter()
                        .map(|path| PageRef {
                            path: (*path).into(),
                            title: (*path).into(),
                            ..PageRef::default()
                        })
                        .collect(),
                })
                .collect(),
            error: None,
        }
    }

    #[test]
    fn an_empty_query_clears_at_once_and_a_stale_answer_is_dropped() {
        let mut b = Brain::default();
        let first = b.typed("pa").unwrap();
        let second = b.typed("pace").unwrap();
        assert!(!b.found(first, Ok(results(&["a.md"]))));
        assert!(b.found(second, Ok(results(&["pace.md"]))));
        assert_eq!(b.results[0].path, "pace.md");
        assert_eq!(b.typed("  "), None);
        assert!(b.results.is_empty());
    }

    #[test]
    fn up_and_down_move_the_mark_held_to_the_list() {
        let mut b = Brain::default();
        let n = b.typed("x").unwrap();
        b.found(n, Ok(results(&["a", "b"])));
        assert_eq!(b.marked().as_deref(), Some("a"), "the first until moved");
        b.step(1);
        b.step(1);
        assert_eq!(b.marked().as_deref(), Some("b"));
        b.step(-5);
        assert_eq!(b.marked().as_deref(), Some("a"));
        let n = b.typed("y").unwrap();
        b.found(n, Ok(results(&[])));
        b.step(1);
        assert_eq!(b.marked(), None, "nothing to mark");
    }

    #[test]
    fn with_nothing_typed_the_keys_walk_the_arrivals() {
        let mut b = Brain::default();
        b.changes_read(Ok(changes(&[("t", &["a", "b", "c"])])));
        assert_eq!(b.marked().as_deref(), Some("a"));
        b.step(2);
        assert_eq!(b.marked().as_deref(), Some("c"));
        assert_eq!(b.typed(""), None);
        assert_eq!(
            b.marked().as_deref(),
            Some("a"),
            "a new list starts at its top"
        );
    }

    #[test]
    fn the_trail_goes_back_to_a_page_already_on_it_and_the_list_starts_a_new_one() {
        let mut b = Brain::default();
        for (p, fresh) in [
            ("z", true),
            ("a", true),
            ("b", false),
            ("c", false),
            ("b", false),
        ] {
            let r = b.open(fresh);
            b.read(r, Ok(page(p, "")));
        }
        let paths: Vec<&str> = b.trail.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(paths, vec!["a", "b"]);
        let r = b.open(false);
        assert!(!b.read(r - 1, Ok(page("c", ""))));
        assert!(b.read(r, Err(Error::new("not_found", "No such page"))));
        assert_eq!(b.page_error.as_deref(), Some("No such page"));
        assert_eq!(
            b.page.as_ref().unwrap().path,
            "b",
            "a failed read keeps the page"
        );
        let r = b.open(true);
        b.read(r, Ok(page("d", "")));
        assert_eq!(b.trail, vec![("d".to_string(), "d".to_string())]);
    }

    #[test]
    fn closing_rejects_an_in_flight_page_and_send_names_the_file() {
        let mut b = Brain::default();
        assert_eq!(b.reference(), None, "nothing read, nothing to send");
        let pending = b.open(true);
        b.close();
        assert!(!b.read(pending, Ok(page("preferences/tools.md", ""))));
        assert!(b.page.is_none());
        assert!(b.trail.is_empty());
        let selected = b.open(true);
        b.read(selected, Ok(page("a/b.md", "/srv/corpus/a/b.md")));
        assert_eq!(
            b.reference().as_deref(),
            Some("brain page /srv/corpus/a/b.md"),
            "as `ahjo brain send` words it"
        );
    }

    #[test]
    fn counts_as_chrote_does() {
        let mut b = Brain::default();
        let n = b.typed("x").unwrap();
        assert_eq!(b.count().as_deref(), Some("Searching…"));
        let paths: Vec<String> = (0..50).map(|i| i.to_string()).collect();
        let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
        b.found(n, Ok(results(&paths)));
        assert_eq!(b.count().as_deref(), Some("50 pages, best first"));
    }

    #[test]
    fn each_arrival_shows_once_at_its_newest_change() {
        let mut b = Brain::default();
        assert!(!b.arrivals_read);
        b.changes_read(Ok(changes(&[
            ("2026-11-02T00:00:00Z", &["self/a.md", "knowledge/b.md"]),
            ("2026-11-01T00:00:00Z", &["self/a.md"]),
        ])));
        assert!(b.arrivals_read);
        assert_eq!(b.arrivals.len(), 2, "each page once");
        assert_eq!(
            b.arrivals[0].updated, "2026-11-02T00:00:00Z",
            "at its newest"
        );
    }

    #[test]
    fn says_when_as_the_library_does() {
        let now: jiff::Timestamp = "2026-11-09T12:00:00Z".parse().unwrap();
        assert_eq!(when("2026-11-09T11:59:30Z", now), "just now");
        assert_eq!(when("2026-11-09T11:00:00+00:00", now), "1 hour ago");
        assert_eq!(when("2026-11-06T12:00:00+02:00", now), "3 days ago");
        assert_eq!(when("2025-10-01T00:00:00Z", now), "1 year ago");
        assert_eq!(when("", now), "never");
    }

    #[test]
    fn snippet_removes_markup_without_losing_match_marks() {
        assert_eq!(
            snippet_parts("## »Heading« [[Some Page|»shown«]] text"),
            vec![
                ("Heading".into(), true),
                (" ".into(), false),
                ("shown".into(), true),
                (" text".into(), false),
            ]
        );
        assert_eq!(
            snippet_parts("# One\n## [[Page]]"),
            vec![("One\nPage".into(), false)]
        );
        assert_eq!(
            snippet_parts("»[[Page|label]]«"),
            vec![("label".into(), true)]
        );
        let plain = |s: &str| -> String { snippet_parts(s).into_iter().map(|(t, _)| t).collect() };
        assert_eq!(plain("and [[resource …"), "and resource …");
        assert_eq!(
            plain("pacing ## Current understanding, C# and #1"),
            "pacing Current understanding, C# and #1"
        );
        assert_eq!(plain("page]] and more"), "page and more");
        assert_eq!(
            plain("its [installer](/srv/tools/heavy/install.sh) for"),
            "its installer for"
        );
        assert_eq!(plain("see [the doc](/srv/cut…"), "see the doc");
        assert_eq!(
            plain("**bold** and `code` [x] stays"),
            "bold and code [x] stays"
        );
    }
    #[test]
    fn marks_the_matched_words_in_a_snippet() {
        assert_eq!(
            marks("the »pace« of work"),
            vec![
                ("the ".into(), false),
                ("pace".into(), true),
                (" of work".into(), false)
            ]
        );
    }
}
