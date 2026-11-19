//! Links in a pane's text, found by shape alone, as CHROTE's terminal finds
//! them (`terminal/pathLinks.ts` and xterm's web-links addon): a web address
//! opens in the browser, an absolute path opens in Files on the pane's
//! device. Nothing is asked of a node to find one.
//!
//! The text is read the way it reads on screen: rows the terminal wrapped
//! are one line, a wide character is one character in its two cells.
//!
//! Claude Code and Codex break long lines themselves, so a path they print
//! often goes on over several rows with a hanging indentation: Claude Code
//! at the edge of its box, in the middle of a name; Codex after a `/` or a
//! `-`. Such a break is taken out when the rows read as one path by the
//! rules of [`Text::continues`], strict enough that prose after a path
//! stays prose. A path either agent shortened with `…` names no file that
//! can be found from it, so neither it nor a part of it is a link; nor is
//! a path cut off by the rows read.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::Term;
use alacritty_terminal::term::cell::Flags;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Url(String),
    Path(String),
    WorkItem(String),
    /// A mermaid diagram's source, drawn on its header.
    Diagram(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub target: Target,
    /// The cells the link is drawn on, in grid lines.
    pub cells: Vec<Point>,
}

/// How many rows either side of the pointer are read: enough for any path
/// an agent prints, bounded for a screen of unbroken output.
const RADIUS: i32 = 32;

/// The link drawn on cell `p`, if any.
pub fn at<T>(term: &Term<T>, p: Point) -> Option<Link> {
    let grid = term.grid();
    if p.line < grid.topmost_line() || p.line > grid.bottommost_line() {
        return None;
    }
    let mut p = p;
    if p.column.0 > 0 && grid[p].flags.contains(Flags::WIDE_CHAR_SPACER) {
        p.column -= 1;
    }
    let text = Text::read(term, p.line - RADIUS, p.line + RADIUS);
    text.links().into_iter().find(|l| l.cells.contains(&p))
}

/// A path as the session's device reads it: one relative to the session's
/// folder `cwd` is joined to it, `~` is the home the folder is in (`/root`
/// or `/home/<name>`), and `.` and `..` are taken out. A `~` path from a
/// folder outside a home stays as it is, for Files to say it is not found.
pub fn resolve(cwd: Option<&str>, path: &str) -> String {
    let joined = match (cwd, path.strip_prefix('~')) {
        (_, _) if path.starts_with('/') => path.to_string(),
        (Some(cwd), Some(rest)) if rest.is_empty() || rest.starts_with('/') => match home_of(cwd) {
            Some(home) => format!("{home}{rest}"),
            None => return path.to_string(),
        },
        (Some(cwd), _) => format!("{}/{path}", cwd.trim_end_matches('/')),
        (None, _) => return path.to_string(),
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in joined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    format!("/{}", parts.join("/"))
}

/// The home folder `cwd` is in.
fn home_of(cwd: &str) -> Option<String> {
    if cwd == "/root" || cwd.starts_with("/root/") {
        return Some("/root".into());
    }
    let name = cwd.strip_prefix("/home/")?.split('/').next()?;
    (!name.is_empty()).then(|| format!("/home/{name}"))
}

/// The links with a cell on grid lines `first..=last`.
pub fn in_lines<T>(term: &Term<T>, first: Line, last: Line) -> Vec<Link> {
    // A path an agent broke over a few rows may begin above the first; a
    // diagram's header in view has its lines below it.
    const ABOVE: i32 = 8;
    const BELOW: i32 = 64;
    let mut links: Vec<Link> = Text::read(term, first - ABOVE, last + BELOW)
        .links()
        .into_iter()
        .filter(|l| l.cells.iter().any(|c| c.line >= first && c.line <= last))
        .collect();
    links.sort_by_key(|l| l.cells.first().copied());
    links
}

/// Rows read as one text, each character with the cell it is drawn in.
struct Text {
    chars: Vec<char>,
    /// The cell of each character; `None` for the break between two rows.
    cells: Vec<Option<Point>>,
    /// Where each row read, from `first`, has its text: its first column
    /// that is not a space, and the column of the text after a bullet
    /// there, if it starts with one ([`margin`]).
    margins: Vec<(usize, Option<usize>)>,
    first: Line,
    cols: usize,
    /// A link starting at this index began above the rows read.
    cut_before: Option<usize>,
    /// The text ends partway through a line or a path.
    cut_after: bool,
}

/// The row continues on the next one: the terminal wrapped it.
fn wrapped<T>(term: &Term<T>, line: Line) -> bool {
    let grid = term.grid();
    line >= grid.topmost_line()
        && line <= grid.bottommost_line()
        && grid[line][Column(term.columns() - 1)]
            .flags
            .contains(Flags::WRAPLINE)
}

/// How far short of the window's edge a program's own box may stop: Claude
/// Code's boxes stop a few columns short, about as far as they are
/// indented.
const BOX_MARGIN: usize = 5;

impl Text {
    fn read<T>(term: &Term<T>, first: Line, last: Line) -> Self {
        let grid = term.grid();
        let (top, bottom) = (grid.topmost_line(), grid.bottommost_line());
        let first = first.max(top);
        let last = last.min(bottom);
        let mut t = Self::rows(term, first, last);
        t.join();
        // A path that goes on past the rows read is cut.
        if wrapped(term, first - 1) {
            t.cut_before = Some(0);
        } else if first > top && Self::breaks_into(term, first - 1) {
            t.cut_before = t.chars.iter().position(|c| *c != ' ');
        }
        t.cut_after = wrapped(term, last) || (last < bottom && Self::breaks_into(term, last));
        t
    }

    /// Whether the line ending on row `line` goes on into the next row as
    /// one path.
    fn breaks_into<T>(term: &Term<T>, line: Line) -> bool {
        let top = term.grid().topmost_line();
        let mut from = line;
        while from > top && line.0 - from.0 < 8 && wrapped(term, from - 1) {
            from -= 1;
        }
        let mut t = Self::rows(term, from, line + 1);
        let breaks = t.chars.iter().filter(|c| **c == '\n').count();
        t.join();
        t.chars.iter().filter(|c| **c == '\n').count() < breaks
    }

    /// Rows `first..=last` as they read: a row the terminal wrapped runs
    /// on into the next, every other one ends in a break.
    fn rows<T>(term: &Term<T>, first: Line, last: Line) -> Self {
        let grid = term.grid();
        let cols = term.columns();
        let mut t = Text {
            chars: Vec::new(),
            cells: Vec::new(),
            margins: Vec::new(),
            first,
            cols,
            cut_before: None,
            cut_after: false,
        };
        let mut line = first;
        while line <= last {
            let row = &grid[line];
            let start = t.chars.len();
            for col in 0..cols {
                let cell = &row[Column(col)];
                if cell
                    .flags
                    .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
                {
                    continue;
                }
                let at = Point::new(line, Column(col));
                for c in
                    std::iter::once(cell.c).chain(cell.zerowidth().into_iter().flatten().copied())
                {
                    t.chars.push(c);
                    t.cells.push(Some(at));
                }
            }
            t.margins.push(margin(&t.chars[start..], &t.cells[start..]));
            // Spaces before a wrap are part of the line; a row's own end is not.
            if !wrapped(term, line) {
                while t.chars.len() > start && t.chars.last() == Some(&' ') {
                    t.chars.pop();
                    t.cells.pop();
                }
                if line < last {
                    t.chars.push('\n');
                    t.cells.push(None);
                }
            }
            line += 1;
        }
        t
    }

    /// Takes out each break inside a path that a program broke over rows
    /// itself, and the indentation after it, so the path reads whole.
    fn join(&mut self) {
        let mut chars = Vec::with_capacity(self.chars.len());
        let mut cells = Vec::with_capacity(self.cells.len());
        let mut k = 0;
        while k < self.chars.len() {
            if self.chars[k] == '\n'
                && let Some(next) = self.continues(&chars, &cells, k)
            {
                k = next;
                continue;
            }
            chars.push(self.chars[k]);
            cells.push(self.cells[k]);
            k += 1;
        }
        self.chars = chars;
        self.cells = cells;
    }

    /// Where the path ending the text so far (`before`, with its `cells`)
    /// goes on after the break at `k`, when the next row holds the rest of
    /// it. Every one of these holds:
    ///
    /// - the row ends in a path with a slash in it, not shortened by `…`;
    /// - the next row starts, after its indentation, with more of a path,
    ///   not shortened by `…` either;
    /// - that indentation is where the row's text starts, where its text
    ///   after a bullet starts, or where the path starts;
    /// - the row could not have held the next row's first piece (up to
    ///   its first `/` or `-`, where programs break a path), give or take
    ///   a program's own box ([`BOX_MARGIN`]);
    /// - the rest has a slash in it, or completes the name of a file that
    ///   the row alone did not (`…_tes` then `ts.rs`, `out.lo` then `g`),
    ///   so a file named at the end of a row is not joined to the word that
    ///   starts the next.
    fn continues(&self, before: &[char], cells: &[Option<Point>], k: usize) -> Option<usize> {
        let line_start = before.iter().rposition(|c| *c == '\n').map_or(0, |i| i + 1);
        let r1 = (line_start..before.len())
            .rev()
            .take_while(|&i| path_char(before[i]))
            .last()?;
        if r1 > 0 && before[r1 - 1] == '…' {
            return None;
        }
        let head = &before[r1..];
        if !head.contains(&'/') {
            return None;
        }
        let t = &self.chars;
        let from = k + 1 + t[k + 1..].iter().take_while(|c| **c == ' ').count();
        let to = run(t, from);
        if to == from || t.get(to) == Some(&'…') {
            return None;
        }
        let rest = &t[from..to];
        let end = cells[before.len() - 1]?;
        let indent = self.cells[from]?.column.0;
        let (lead, after_bullet) = self.margins[(end.line.0 - self.first.0) as usize];
        let path_at = cells[r1]?.column.0;
        if indent != lead && Some(indent) != after_bullet && indent != path_at {
            return None;
        }
        let piece = rest
            .iter()
            .position(|c| *c == '/' || *c == '-')
            .map_or(rest.len(), |i| i + 1);
        let slack = BOX_MARGIN.max(indent);
        if end.column.0 + 1 + piece <= self.cols.saturating_sub(slack) {
            return None;
        }
        if !rest.contains(&'/') {
            let whole: Vec<char> = head.iter().chain(rest).copied().collect();
            let common = |p: &[char]| extension(p).is_some_and(|e| COMMON.contains(&e.as_str()));
            let completes = match extension(head) {
                None => extension(&whole).is_some(),
                Some(_) => !common(head) && common(&whole),
            };
            if !completes {
                return None;
            }
        }
        Some(from)
    }

    fn cells(&self, from: usize, len: usize) -> Vec<Point> {
        self.cells[from..from + len]
            .iter()
            .flatten()
            .copied()
            .collect()
    }

    /// A link at `i` of `n` characters goes on past the rows read: only
    /// what a path's end is trimmed of (`/`, `.`) is after it.
    fn cut(&self, i: usize, n: usize) -> bool {
        self.cut_before == Some(i)
            || (self.cut_after && self.chars[i + n..].iter().all(|c| TRAILING.contains(*c)))
    }

    fn links(&self) -> Vec<Link> {
        let mut links: Vec<Link> = urls(&self.chars)
            .into_iter()
            .map(|(i, n)| Link {
                target: Target::Url(self.chars[i..i + n].iter().collect()),
                cells: self.cells(i, n),
            })
            .collect();
        links.extend(
            super::mermaid::blocks(&self.chars)
                .into_iter()
                .map(|b| Link {
                    target: Target::Diagram(b.source),
                    cells: self.cells(b.at, b.len),
                }),
        );
        // A path cut off by the rows read is not offered in part.
        for (i, n) in paths(&self.chars) {
            if !self.cut(i, n) {
                links.push(Link {
                    target: Target::Path(self.chars[i..i + n].iter().collect()),
                    cells: self.cells(i, n),
                });
            }
        }
        // A path relative to the pane's folder, where nothing above is.
        for (i, n) in relative_paths(&self.chars) {
            let cells = self.cells(i, n);
            if !self.cut(i, n)
                && !links
                    .iter()
                    .any(|link| cells.iter().any(|cell| link.cells.contains(cell)))
            {
                links.push(Link {
                    target: Target::Path(self.chars[i..i + n].iter().collect()),
                    cells,
                });
            }
        }
        // A URL or path takes precedence over a work-like substring.
        for (i, n) in work_items(&self.chars) {
            let cells = self.cells(i, n);
            if !self.cut(i, n)
                && !links
                    .iter()
                    .any(|link| cells.iter().any(|cell| link.cells.contains(cell)))
            {
                links.push(Link {
                    target: Target::WorkItem(self.chars[i..i + n].iter().collect()),
                    cells,
                });
            }
        }
        links
    }
}

/// Where a row's text starts, and where its text after a bullet starts if
/// it has one: a mark that is not part of a word (`-`, `*`, `•`, `●`,
/// `⎿`, `└`, `❯`), or a number and `.` or `)`, then spaces.
fn margin(chars: &[char], cells: &[Option<Point>]) -> (usize, Option<usize>) {
    let col = |i: usize| cells.get(i).copied().flatten().map(|p| p.column.0);
    let lead = chars.iter().take_while(|c| **c == ' ').count();
    let Some(at) = col(lead) else {
        return (0, None);
    };
    let mut i = lead;
    if chars[i].is_ascii_digit() {
        i += chars[i..].iter().take_while(|c| c.is_ascii_digit()).count();
        if !matches!(chars.get(i), Some('.' | ')')) {
            return (at, None);
        }
    } else if chars[i].is_alphanumeric() || "/.~_\"'([`".contains(chars[i]) {
        return (at, None);
    }
    i += 1;
    let spaces = chars[i.min(chars.len())..]
        .iter()
        .take_while(|c| **c == ' ')
        .count();
    if spaces == 0 {
        return (at, None);
    }
    (at, col(i + spaces))
}

/// Extensions a file's name ends in often enough that a name broken just
/// after one is taken as whole.
const COMMON: &[&str] = &[
    "rs", "md", "txt", "toml", "json", "jsonl", "yaml", "yml", "lock", "ts", "tsx", "js", "jsx",
    "mjs", "py", "go", "sh", "c", "h", "cpp", "hpp", "java", "kt", "rb", "swift", "sql", "csv",
    "xml", "html", "css", "svg", "png", "jpg", "jpeg", "gif", "webp", "pdf", "log", "ini", "cfg",
    "conf", "env", "mp4", "wav", "zip", "gz",
];

/// The extension of the last name in `path`, by the rule a relative path
/// names a file by ([`names_a_file`]).
fn extension(path: &[char]) -> Option<String> {
    let word: String = path.iter().collect();
    let last = word.rsplit('/').next().unwrap_or(&word);
    if !names_a_file(last) {
        return None;
    }
    last.rsplit_once('.').map(|(_, e)| e.to_lowercase())
}

/// A character of a path: a file name's, or a slash.
fn path_char(c: char) -> bool {
    name_char(c) || c == '/'
}

/// A character of a file name.
fn name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || "_.@%+~-".contains(c)
}

/// Where a run of path characters (names and slashes) from `i` ends.
fn run(t: &[char], i: usize) -> usize {
    let mut j = i;
    while j < t.len() && (name_char(t[j]) || t[j] == '/') {
        j += 1;
    }
    j
}

const TRAILING: &str = ".,;:)]}'\"/";

/// The length of `t` without the punctuation that ends a sentence.
fn trailing_trimmed(t: &[char]) -> usize {
    let mut n = t.len();
    while n > 0 && TRAILING.contains(t[n - 1]) {
        n -= 1;
    }
    n
}

/// The absolute paths in `t`, as (start, length): a slash that begins a
/// word, then names and slashes, less the punctuation after it. The slash
/// inside a web address follows a letter or a slash, so it is not one.
fn paths(t: &[char]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < t.len() {
        let follows_word =
            i > 0 && (t[i - 1].is_ascii_alphanumeric() || "_./~:@%+-".contains(t[i - 1]));
        if t[i] != '/' || follows_word {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        loop {
            let s = j;
            while j < t.len() && name_char(t[j]) {
                j += 1;
            }
            if j > s && j < t.len() && t[j] == '/' {
                j += 1;
            } else {
                break;
            }
        }
        let n = trailing_trimmed(&t[i..j]);
        // A slash on its own is not somewhere to go, nor a path shortened
        // with `…`.
        if n > 1 && !elided(t, i, j) {
            out.push((i, n));
        }
        i = j;
    }
    out
}

/// The run of path characters at `i..j` touches a `…` that shortened it:
/// `areas/termin…/x.rs`, `/tmp/a/b…`.
fn elided(t: &[char], i: usize, j: usize) -> bool {
    (i > 0 && t[i - 1] == '…') || t.get(j) == Some(&'…')
}

/// The paths relative to the pane's folder in `t`, as (start, length):
/// a word that names a file by its extension (`notes.txt`,
/// `src/ui/rail.rs:12`) or starts at `./` or `../`. A word with slashes but
/// no extension (`and/or`) or a number (`v2.1.3`) is not one, nor a repo
/// before its `#42`.
fn relative_paths(t: &[char]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < t.len() {
        let starts = (name_char(t[i]) || t[i] == '.')
            && (i == 0 || !(name_char(t[i - 1]) || "/:#".contains(t[i - 1])));
        if !starts {
            i += 1;
            continue;
        }
        let j = run(t, i);
        let n = trailing_trimmed(&t[i..j]);
        let word: String = t[i..i + n].iter().collect();
        if t.get(j) != Some(&'#') && !elided(t, i, j) && names_a_file(&word) {
            out.push((i, n));
        }
        i = j.max(i + 1);
    }
    out
}

/// A relative word that reads as a file: `./` or `../` before it, or a
/// last name with an extension of letters and digits after a name with a
/// letter in it.
fn names_a_file(word: &str) -> bool {
    if word.starts_with('/') || word.contains("//") {
        return false;
    }
    if (word.starts_with("./") || word.starts_with("../")) && word.len() > 3 {
        return true;
    }
    let last = word.rsplit('/').next().unwrap_or(word);
    let Some((base, ext)) = last.rsplit_once('.') else {
        return false;
    };
    base.chars().filter(|c| c.is_ascii_alphabetic()).count() >= 1
        && base.trim_start_matches('.').chars().count() >= 2
        && (1..=8).contains(&ext.len())
        && ext.chars().all(|c| c.is_ascii_alphanumeric())
        && ext.chars().any(|c| c.is_ascii_alphabetic())
}

/// Recognize complete tokens, so a path or a larger word is never offered as
/// a partial item id. Soft-wrapped rows have already been joined by `Text`.
fn work_items(t: &[char]) -> Vec<(usize, usize)> {
    use ahjo_work_contract::work_ref::{is_bead_id, is_repo_part};
    let token = |c: char| c.is_ascii_alphanumeric() || "_./-#".contains(c);
    let mut found = Vec::new();
    let mut i = 0;
    while i < t.len() {
        if !token(t[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < t.len() && token(t[i]) {
            i += 1;
        }
        let mut end = i;
        while end > start && t[end - 1] == '.' {
            end -= 1;
        }
        let candidate: String = t[start..end].iter().collect();
        let issue = candidate.rsplit_once('#').is_some_and(|(repo, n)| {
            let number = !n.is_empty()
                && n.bytes().all(|b| b.is_ascii_digit())
                && n.parse::<u64>().is_ok_and(|n| n > 0);
            number
                && (repo.is_empty()
                    || repo
                        .split_once('/')
                        .is_some_and(|(owner, name)| is_repo_part(owner) && is_repo_part(name)))
        });
        if is_bead_id(&candidate) || issue {
            found.push((start, end - start));
        }
    }
    found
}

/// The web addresses in `t`, as (start, length), by xterm's web-links rule:
/// `http(s)://`, then anything but spaces, quotes and brackets, ending on a
/// character that does not end a sentence.
fn urls(t: &[char]) -> Vec<(usize, usize)> {
    let body = |c: char| !c.is_whitespace() && !"\"'!*(){}|\\^<>`".contains(c);
    let end = |c: char| body(c) && !":,.?~[]".contains(c);
    let mut out = Vec::new();
    let mut i = 0;
    while i < t.len() {
        let Some(scheme) = scheme(&t[i..]) else {
            i += 1;
            continue;
        };
        let from = i + scheme;
        let mut j = from;
        while j < t.len() && body(t[j]) {
            j += 1;
        }
        match (from..j).rev().find(|&k| end(t[k])) {
            Some(k) => {
                out.push((i, k + 1 - i));
                i = k + 1;
            }
            None => i += 1,
        }
    }
    out
}

/// The length of the `http://` or `https://` that `t` starts with.
fn scheme(t: &[char]) -> Option<usize> {
    ["http://", "https://", "HTTP://", "HTTPS://"]
        .iter()
        .find(|s| t.len() >= s.len() && s.chars().zip(t).all(|(a, b)| a == *b))
        .map(|s| s.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::term::screen::Screen;

    fn link(s: &Screen, line: i32, col: usize) -> Option<Link> {
        at(&s.term, Point::new(Line(line), Column(col)))
    }

    fn screen(cols: usize, rows: usize, text: &str) -> Screen {
        let mut s = Screen::new(cols, rows);
        s.feed(text.replace('\n', "\r\n").as_bytes());
        s
    }

    #[test]
    fn the_paths_in_view_are_found_with_their_rows() {
        let s = screen(
            40,
            6,
            "made out/shot.png\nno path here\nand /tmp/a/b/Plan.SVG then\n/x/y.png",
        );
        let paths: Vec<(String, i32)> = in_lines(&s.term, Line(0), Line(2))
            .into_iter()
            .filter_map(|l| match l.target {
                Target::Path(p) => Some((p, l.cells[0].line.0)),
                _ => None,
            })
            .collect();
        assert_eq!(
            paths,
            vec![("out/shot.png".into(), 0), ("/tmp/a/b/Plan.SVG".into(), 2)]
        );
    }

    #[test]
    fn a_url_opens_and_ends_before_its_full_stop() {
        let s = screen(60, 3, "see https://example.com/a?b=1. next");
        let l = link(&s, 0, 10).expect("a link on the address");
        assert_eq!(l.target, Target::Url("https://example.com/a?b=1".into()));
        assert_eq!(l.cells.first(), Some(&Point::new(Line(0), Column(4))));
        assert_eq!(l.cells.len(), 25);
        assert_eq!(link(&s, 0, 2), None);
        // The path inside an address is the address's.
        assert!(matches!(link(&s, 0, 24).unwrap().target, Target::Url(_)));
    }

    #[test]
    fn a_url_the_terminal_wrapped_is_one_link() {
        let s = screen(20, 3, "go https://example.com/long/path ok");
        let want = Target::Url("https://example.com/long/path".into());
        assert_eq!(link(&s, 0, 5).unwrap().target, want);
        assert_eq!(link(&s, 1, 3).unwrap().target, want);
    }

    #[test]
    fn a_path_opens_without_its_line_number_or_punctuation() {
        let s = screen(60, 2, "wrote /srv/ahjo/src/main.rs:12, done");
        let l = link(&s, 0, 8).unwrap();
        assert_eq!(l.target, Target::Path("/srv/ahjo/src/main.rs".into()));
        assert_eq!(link(&s, 0, 3), None);
        // A lone slash, or a word with slashes but no file in it, is not a link.
        let s = screen(60, 2, "a/b and / alone");
        assert_eq!(link(&s, 0, 1), None);
        assert_eq!(link(&s, 0, 8), None);
    }

    #[test]
    fn a_file_named_relative_to_the_pane_opens() {
        let s = screen(
            80,
            2,
            "edited src/ui/rail.rs:12 and notes.txt. see ./run and v2.1.3 e.g. x",
        );
        let at = |col| link(&s, 0, col).map(|l| l.target);
        assert_eq!(at(9), Some(Target::Path("src/ui/rail.rs".into())));
        assert_eq!(at(31), Some(Target::Path("notes.txt".into())));
        assert_eq!(at(47), Some(Target::Path("./run".into())));
        // A version, an abbreviation and a plain word are not files.
        assert_eq!(at(56), None);
        assert_eq!(at(63), None);
        assert_eq!(at(0), None);
    }

    #[test]
    fn a_path_after_wide_characters_is_found_at_its_cells() {
        let s = screen(40, 2, "日本 👍 /tmp/x.txt");
        // 日本 takes four cells, the space one, 👍 two, the space one.
        let l = link(&s, 0, 8).expect("the path starts at column 8");
        assert_eq!(l.target, Target::Path("/tmp/x.txt".into()));
        assert_eq!(l.cells.first(), Some(&Point::new(Line(0), Column(8))));
        assert_eq!(l.cells.last(), Some(&Point::new(Line(0), Column(17))));
        // The right half of a wide character is that character's cell.
        assert_eq!(link(&s, 0, 1), None);
    }

    #[test]
    fn a_quoted_path_broken_at_the_margin_opens_whole() {
        // Claude Code breaks a long line itself and indents what follows.
        let s = screen(19, 4, "  Read(\"/srv/ahjo/c\n  rates/app/x.rs\")\n");
        let want = Target::Path("/srv/ahjo/crates/app/x.rs".into());
        assert_eq!(link(&s, 0, 10).unwrap().target, want);
        let l = link(&s, 1, 4).unwrap();
        assert_eq!(l.target, want);
        assert_eq!(l.cells.len(), "/srv/ahjo/crates/app/x.rs".chars().count());
        // The indentation is not part of it.
        assert_eq!(link(&s, 1, 0), None);
    }

    #[test]
    fn a_hard_break_not_at_the_margin_is_not_joined() {
        let s = screen(30, 4, "  Read(\"/srv/ahjo/c\n  rates/app/x.rs\")\n");
        assert_eq!(
            link(&s, 0, 10).unwrap().target,
            Target::Path("/srv/ahjo/c".into())
        );
        assert_eq!(link(&s, 1, 4), None);
    }

    #[test]
    fn a_path_the_terminal_wrapped_is_one_link() {
        let s = screen(12, 3, "x /home/user/notes.md");
        let want = Target::Path("/home/user/notes.md".into());
        assert_eq!(link(&s, 0, 4).unwrap().target, want);
        assert_eq!(link(&s, 1, 2).unwrap().target, want);
    }

    #[test]
    fn work_item_ids_and_issues_are_links_including_soft_wraps() {
        let s = screen(
            80,
            3,
            "ahjo-abc parvi-pnu.7.3 #42 octo/repo#42 word /srv/ahjo-abc",
        );
        for (col, id) in [
            (0, "ahjo-abc"),
            (11, "parvi-pnu.7.3"),
            (25, "#42"),
            (30, "octo/repo#42"),
        ] {
            assert_eq!(
                link(&s, 0, col).unwrap().target,
                Target::WorkItem(id.into())
            );
        }
        assert!(link(&s, 0, 43).is_none());
        assert!(matches!(link(&s, 0, 50).unwrap().target, Target::Path(_)));
        let s = screen(12, 3, "x parvi-pnu.7.3");
        let want = Target::WorkItem("parvi-pnu.7.3".into());
        assert_eq!(link(&s, 0, 4).unwrap().target, want);
        assert_eq!(link(&s, 1, 1).unwrap().target, want);
    }

    // The fixtures below are rows Claude Code (2.1) and Codex printed in a
    // tmux pane 60 and 50 columns wide, as `capture-pane -p` gave them.

    const DEEP: &str =
        "/tmp/claude-1000/-srv-parvi/4bb7cac5-c5e6-4cf2-b148-2088dc57b999/scratchpad/links/proj";
    const LONG: &str =
        "areas/terminals/app/src/term/deeply/nested/folder/long_file_name_for_wrapping_tests.rs";
    const MD: &str = "areas/terminals/app/src/term/deeply/nested/folder/another_quite_long_name.md";

    /// The path on every cell of rows `rows` that is not indentation, in
    /// columns from `col` on.
    fn path_on(s: &Screen, rows: &[(i32, usize)]) -> Vec<Option<Target>> {
        rows.iter()
            .map(|&(row, col)| link(s, row, col).map(|l| l.target))
            .collect()
    }

    #[test]
    fn claude_codes_list_of_paths_broken_at_the_edge_opens_whole() {
        let s = screen(
            60,
            12,
            "● Both files are at:\n\
             \n  - /tmp/claude-1000/-srv-parvi/4bb7cac5-c5e6-4cf2-b148-2088\n    \
             dc57b999/scratchpad/links/proj/areas/terminals/app/src/t\n    \
             erm/deeply/nested/folder/long_file_name_for_wrapping_tes\n    \
             ts.rs\n  - /tmp/claude-1000/-srv-parvi/4bb7cac5-c5e6-4cf2-b148-2088\n    \
             dc57b999/scratchpad/links/proj/notes.txt\n\
             \n  Relative path: areas/terminals/app/src/term/deeply/nested/\n  \
             folder/long_file_name_for_wrapping_tests.rs:2:5",
        );
        let long = Some(Target::Path(format!("{DEEP}/{LONG}")));
        assert_eq!(
            path_on(&s, &[(2, 4), (2, 59), (3, 4), (4, 30), (5, 6)]),
            vec![long.clone(); 5]
        );
        let notes = Some(Target::Path(format!("{DEEP}/notes.txt")));
        assert_eq!(path_on(&s, &[(6, 10), (7, 20)]), vec![notes; 2]);
        let relative = Some(Target::Path(LONG.into()));
        assert_eq!(path_on(&s, &[(9, 20), (10, 2)]), vec![relative; 2]);
        // The indentation and the line number are not the path.
        assert_eq!(link(&s, 3, 1), None);
        assert_eq!(link(&s, 10, 46), None);
        let l = link(&s, 4, 4).unwrap();
        assert_eq!(l.cells.len(), DEEP.len() + 1 + LONG.len());
    }

    #[test]
    fn claude_codes_box_breaks_a_name_short_of_the_edge() {
        let s = screen(
            50,
            8,
            "● Write(areas/termina…/another_quite_long_name.md)\n  \
             ⎿  Wrote 1 line to\n     \
             areas/terminals/app/src/term/deeply/nest\n     \
             ed/folder/another_quite_long_name.md\n      \
             1 This is a single-line markdown file",
        );
        let md = Some(Target::Path(MD.into()));
        assert_eq!(
            path_on(&s, &[(2, 5), (2, 44), (3, 5), (3, 30)]),
            vec![md; 4]
        );
        // The word before it is not part of it.
        assert_eq!(link(&s, 1, 17), None);
    }

    #[test]
    fn a_typed_prompt_joins_its_path_but_not_the_words_after_it() {
        let s = screen(
            50,
            4,
            "❯ Create the file areas/terminals/app/src/term/de\n  \
             eply/nested/folder/another_quite_long_name.md\n  \
             with one line, then list every file you touched",
        );
        let l = link(&s, 0, 20).unwrap();
        assert_eq!(l.target, Target::Path(MD.into()));
        assert!(l.cells.iter().all(|c| c.line.0 <= 1));
        assert_eq!(link(&s, 2, 3), None);
    }

    #[test]
    fn codex_breaks_after_a_slash_or_a_dash_and_hangs_the_rest() {
        let s = screen(
            50,
            10,
            "  • (/tmp/claude-1000/-srv-parvi/4bb7cac5-c5e6-\n    \
             4cf2-b148-2088dc57b999/scratchpad/links/proj/\n    \
             areas/terminals/app/src/term/deeply/nested/\n    \
             folder/another_quite_long_name.md)\n  \
             • /tmp/claude-1000/-srv-parvi/4bb7cac5-c5e6-\n    \
             4cf2-b148-2088dc57b999/scratchpad/links/proj/\n    \
             notes.txt",
        );
        let md = Some(Target::Path(format!("{DEEP}/{MD}")));
        assert_eq!(path_on(&s, &[(0, 6), (1, 4), (2, 10), (3, 4)]), vec![md; 4]);
        let notes = Some(Target::Path(format!("{DEEP}/notes.txt")));
        assert_eq!(path_on(&s, &[(4, 5), (6, 6)]), vec![notes; 2]);
        // The bracket around it is not part of it.
        assert_eq!(link(&s, 3, 37), None);

        let s = screen(
            60,
            3,
            "• Edited areas/terminals/app/src/term/deeply/nested/folder/\n\
             long_file_name_for_wrapping_tests.rs (+1 -0)",
        );
        let long = Some(Target::Path(LONG.into()));
        assert_eq!(path_on(&s, &[(0, 12), (1, 0)]), vec![long; 2]);
        assert_eq!(link(&s, 1, 39), None);
    }

    #[test]
    fn a_path_shortened_with_an_ellipsis_is_no_link_at_all() {
        let s = screen(
            60,
            4,
            "● Update(areas/termin…/long_file_name_for_wrapping_tests.rs)\n\
             • Ran tail -n 2 areas/terminals/app/src/term/deeply/nested/…\n    \
             /tmp/claude-1000/-srv-parvi/4bb7cac5-c5e6-4cf…\n \
             ▝▝   ▝▝   /…/scratchpad/links/proj",
        );
        for (row, col) in [(0, 12), (0, 30), (1, 20), (2, 10), (3, 14)] {
            assert_eq!(link(&s, row, col), None, "row {row}, column {col}");
        }
    }

    #[test]
    fn a_file_at_the_end_of_a_row_keeps_the_word_after_it_out() {
        // Prose broken after a path is not joined to it ...
        let s = screen(
            30,
            6,
            "  it wrote /tmp/out/report.md\n  which is long\n  \
             in the folder /tmp/out/build\n  which holds it",
        );
        let at = |row, col| link(&s, row, col).map(|l| l.target);
        assert_eq!(at(0, 12), Some(Target::Path("/tmp/out/report.md".into())));
        assert_eq!(at(1, 3), None);
        assert_eq!(at(2, 18), Some(Target::Path("/tmp/out/build".into())));
        assert_eq!(at(3, 3), None);
        // ... but a name broken just after its first letters is.
        let s = screen(20, 3, "  saved /tmp/a/out.t\n  xt and more");
        let want = Some(Target::Path("/tmp/a/out.txt".into()));
        assert_eq!(path_on(&s, &[(0, 9), (1, 2)]), vec![want; 2]);
    }

    #[test]
    fn a_path_going_on_above_the_rows_read_is_not_offered_in_part() {
        let s = screen(
            50,
            6,
            "  • /tmp/claude-1000/-srv-parvi/4bb7cac5-c5e6-\n    \
             4cf2-b148-2088dc57b999/scratchpad/links/proj/\n    \
             notes.txt",
        );
        let paths = |first, last| -> Vec<String> {
            Text::read(&s.term, Line(first), Line(last))
                .links()
                .into_iter()
                .filter_map(|l| match l.target {
                    Target::Path(p) => Some(p),
                    _ => None,
                })
                .collect()
        };
        assert_eq!(paths(0, 2), vec![format!("{DEEP}/notes.txt")]);
        assert!(paths(1, 2).is_empty(), "{:?}", paths(1, 2));
        assert!(paths(0, 1).is_empty(), "{:?}", paths(0, 1));
    }

    #[test]
    fn a_path_resolves_in_the_panes_folder_and_home() {
        let cwd = Some("/home/ana/proj");
        assert_eq!(resolve(cwd, "src/a.rs"), "/home/ana/proj/src/a.rs");
        assert_eq!(resolve(cwd, "./run"), "/home/ana/proj/run");
        assert_eq!(resolve(cwd, "../other/b.md"), "/home/ana/other/b.md");
        assert_eq!(resolve(cwd, "~/notes/c.txt"), "/home/ana/notes/c.txt");
        assert_eq!(resolve(Some("/root/x"), "~/d.txt"), "/root/d.txt");
        assert_eq!(resolve(cwd, "/srv/e.rs"), "/srv/e.rs");
        // A home that cannot be told from the folder is left to Files.
        assert_eq!(resolve(Some("/srv/x"), "~/f.txt"), "~/f.txt");
        assert_eq!(resolve(None, "g.txt"), "g.txt");
    }
}
