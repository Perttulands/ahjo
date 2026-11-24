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
//! `-`, or inside a name too long for a row. Such a break is taken out
//! when the rows read as one path by the rules of [`Text::continues`],
//! strict enough that prose after a path stays prose. A path cut off by
//! the rows read is no link.
//!
//! Some paths name their file only with the device's help, which the click
//! asks for: one an agent shortened with `…` (`areas/termin…/x.rs`) is
//! found by its ends, and one followed by more words may go on over the
//! spaces (`/tmp/space name.txt`, [`Link::spaced`]). A path in quotes is
//! the whole quoted text, spaces and all.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::Term;
use alacritty_terminal::term::cell::Flags;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Url(String),
    /// A path, and the line named after it (`main.rs:12`, `main.rs:12:5`).
    Path(String, Option<u32>),
    WorkItem(String),
    /// A mermaid diagram's source, drawn on its header.
    Diagram(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub target: Target,
    /// The cells the link is drawn on, in grid lines.
    pub cells: Vec<Point>,
    /// The column named after a path's line (`main.rs:12:5`).
    pub column: Option<u32>,
    /// Longer readings of a path, over the spaces after it or before it
    /// (`/tmp/space name.txt` clicked on either word), longest first: a
    /// file's name may hold spaces, and only the device can say whether it
    /// does.
    pub spaced: Vec<String>,
}

impl Link {
    fn new(target: Target, cells: Vec<Point>) -> Self {
        Link {
            target,
            cells,
            column: None,
            spaced: Vec::new(),
        }
    }
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
/// folder outside a home stays as it is, for [`in_home`].
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

/// A `~` path the folder did not tell the home of ([`resolve`]), in the
/// node's own home: the session runs as the node's user unless its folder
/// says otherwise.
pub fn in_home(path: &str, home: Option<&str>) -> String {
    match (path.strip_prefix('~'), home) {
        (Some(rest), Some(home)) if rest.is_empty() || rest.starts_with('/') => {
            resolve(None, &format!("{}{rest}", home.trim_end_matches('/')))
        }
        _ => path.to_string(),
    }
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
    /// - the rest has a slash in it, completes the name of a file that the
    ///   row alone did not (`…_tes` then `ts.rs`, `out.lo` then `g`),
    ///   fills its own row to the edge, a name too long for one row that
    ///   the program broke inside (Codex's `long-directory-` then a row of
    ///   `aaaa`), or is all of its row and breaks again after a dash
    ///   (Codex's `more-` then `directory-` then `bbbb/long-`), so a file
    ///   named at the end of a row is not joined to the word that starts
    ///   the next.
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
            let fills_row = t.get(to).is_none_or(|c| *c == '\n')
                && rest.iter().any(|c| c.is_alphanumeric())
                && self.cells[to - 1]?.column.0 + 1 >= self.cols.saturating_sub(slack);
            let breaks_again = t.get(to).is_none_or(|c| *c == '\n') && rest.last() == Some(&'-');
            if !completes && !fills_row && !breaks_again {
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
        let t = &self.chars;
        let mut links: Vec<Link> = urls(t)
            .into_iter()
            .map(|(i, n)| Link::new(Target::Url(t[i..i + n].iter().collect()), self.cells(i, n)))
            .collect();
        links.extend(
            super::mermaid::blocks(t)
                .into_iter()
                .map(|b| Link::new(Target::Diagram(b.source), self.cells(b.at, b.len))),
        );
        let overlaps = |links: &[Link], cells: &[Point]| {
            links
                .iter()
                .any(|link| cells.iter().any(|cell| link.cells.contains(cell)))
        };
        // A path in quotes is all of it, and a path relative to the pane's
        // folder is one where nothing above is.
        let quoted = quoted_paths(t);
        let absolute = paths(t);
        let from = links.len();
        let found = quoted
            .iter()
            .chain(&absolute)
            .chain(&relative_paths(t))
            .copied()
            .collect::<Vec<_>>();
        for (i, n) in found {
            let cells = self.cells(i, n);
            let above = if absolute.contains(&(i, n)) {
                &links[from..]
            } else {
                &links[..]
            };
            if self.cut(i, n) || overlaps(above, &cells) {
                continue;
            }
            let after = &t[i + n..];
            let after = if quoted.contains(&(i, n)) {
                &after[1..]
            } else {
                after
            };
            let mut link = Link::new(
                Target::Path(t[i..i + n].iter().collect(), line_after(after)),
                cells,
            );
            link.column = column_after(after);
            if !quoted.contains(&(i, n)) {
                link.spaced = spaced(t, i, n);
            }
            links.push(link);
        }
        // A URL or path takes precedence over a work-like substring.
        for (i, n) in work_items(t) {
            let cells = self.cells(i, n);
            if !self.cut(i, n) && !overlaps(&links, &cells) {
                links.push(Link::new(
                    Target::WorkItem(t[i..i + n].iter().collect()),
                    cells,
                ));
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

/// A character of a file name: a letter or digit of any script, a mark
/// combined with one (`u` and `\u{308}` for `ü`), or `_.@%+~-`.
fn name_char(c: char) -> bool {
    c.is_alphanumeric() || "_.@%+~-".contains(c) || combining(c)
}

/// A letter of a script written without spaces between words: Chinese,
/// Japanese and Korean.
fn unspaced(c: char) -> bool {
    matches!(c, '\u{1100}'..='\u{11ff}' | '\u{2e80}'..='\u{a4cf}' | '\u{ac00}'..='\u{d7af}' | '\u{f900}'..='\u{faff}' | '\u{ff00}'..='\u{ffef}')
}

/// A mark the terminal draws over the character before it.
fn combining(c: char) -> bool {
    matches!(c, '\u{300}'..='\u{36f}' | '\u{1ab0}'..='\u{1aff}' | '\u{1dc0}'..='\u{1dff}' | '\u{20d0}'..='\u{20ff}' | '\u{fe20}'..='\u{fe2f}')
}

/// Where a run of path characters (names and slashes) from `i` ends.
fn run(t: &[char], i: usize) -> usize {
    let mut j = i;
    while j < t.len() && (name_char(t[j]) || t[j] == '/') {
        j += 1;
    }
    j
}

/// Where a run of path characters from `i` ends, through one `…` an agent
/// shortened it with (`areas/termin…/x.rs`, `/…/proj`, `/tmp/4cf…`).
fn shortened_run(t: &[char], i: usize) -> usize {
    let j = run(t, i);
    if t.get(j) == Some(&'…') {
        run(t, j + 1)
    } else {
        j
    }
}

/// Whether a path holding a `…` still names something by its ends: a
/// name after it, or the start of a name before it.
fn names_by_its_ends(path: &[char]) -> bool {
    let Some(at) = path.iter().position(|c| *c == '…') else {
        return true;
    };
    let (head, tail) = (&path[..at], &path[at + 1..]);
    tail.iter().any(|c| *c != '/') || head.last().is_some_and(|c| *c != '/')
}

const TRAILING: &str = ".,;:)]}'\"/";

/// The line a path names just after it, as compilers and agents write it:
/// `:12`, or `:12:5` with a column ([`column_after`]).
fn line_after(t: &[char]) -> Option<u32> {
    let digits: String = t
        .strip_prefix(&[':'])?
        .iter()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok().filter(|n| *n > 0)
}

/// The column after a path's line: the `5` of `:12:5`.
fn column_after(t: &[char]) -> Option<u32> {
    line_after(t)?;
    let rest = &t[1 + t[1..].iter().take_while(|c| c.is_ascii_digit()).count()..];
    line_after(rest)
}

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
/// One `…` may stand inside it, where an agent shortened it, and a slash
/// after a `…` is that path's. A slash after a letter of a script written
/// without spaces (`已写入/tmp/a.txt`) does begin one.
fn paths(t: &[char]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < t.len() {
        let follows_word =
            i > 0 && ((name_char(t[i - 1]) && !unspaced(t[i - 1])) || "/:…".contains(t[i - 1]));
        if t[i] != '/' || follows_word {
            i += 1;
            continue;
        }
        let j = shortened_run(t, i);
        // A run with an empty name in it (`//`) ends before that.
        let j = (i + 1..j)
            .find(|&k| t[k] == '/' && t[k - 1] == '/')
            .unwrap_or(j);
        let n = trailing_trimmed(&t[i..j]);
        // A slash on its own is not somewhere to go, nor a path shortened
        // where it names nothing (`/tmp/…`).
        if n > 1 && names_by_its_ends(&t[i..i + n]) {
            out.push((i, n));
        }
        i = j.max(i + 1);
    }
    out
}

/// The paths inside quotes or backticks in `t`, as (start, length), where
/// a name has a space in it: `"/tmp/space name.txt"`. The text in quotes
/// starts as a path does (`/`, `~/`, `./`, `../`) and ends on its line.
fn quoted_paths(t: &[char]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < t.len() {
        let quote = t[i];
        if !"\"'`".contains(quote) {
            i += 1;
            continue;
        }
        let Some(len) = t[i + 1..]
            .iter()
            .position(|c| *c == quote || *c == '\n')
            .filter(|&k| t[i + 1 + k] == quote)
        else {
            i += 1;
            continue;
        };
        let word: String = t[i + 1..i + 1 + len].iter().collect();
        let starts = ["/", "~/", "./", "../"].iter().any(|s| word.starts_with(s));
        if starts && word.contains(' ') && !word.ends_with(' ') && !word.contains("  ") {
            out.push((i + 1, len));
            i += len + 2;
        } else {
            i += 1;
        }
    }
    out
}

/// The longer readings of the path at `i..i + n` over the spaces around it
/// ([`Link::spaced`]), longest first: up to three words after it, ending on
/// one that names a file (`/tmp/space` then `name.txt`), and, for a
/// relative path, the words before it back to one that starts a path
/// (`name.txt` after `/tmp/space`). Each word is made of path characters
/// and set off by one space, on the same line.
fn spaced(t: &[char], i: usize, n: usize) -> Vec<String> {
    const WORDS: usize = 3;
    let mut out = Vec::new();
    let mut k = i + n;
    for _ in 0..WORDS {
        if t.get(k) != Some(&' ') {
            break;
        }
        let w = run(t, k + 1);
        if w == k + 1 {
            break;
        }
        let end = k + 1 + trailing_trimmed(&t[k + 1..w]);
        let word: String = t[k + 1..end].iter().collect();
        if names_a_file(&word) {
            out.push(t[i..end].iter().collect());
        }
        if end < w {
            break;
        }
        k = w;
    }
    out.reverse();
    if t[i] != '/' && t[i] != '~' {
        let mut k = i;
        for _ in 0..WORDS {
            if k < 2 || t[k - 1] != ' ' {
                break;
            }
            let start = (0..k - 1)
                .rev()
                .take_while(|&j| path_char(t[j]))
                .last()
                .unwrap_or(k - 1);
            if start == k - 1 {
                break;
            }
            if t[start] == '/' || (t[start] == '~' && t.get(start + 1) == Some(&'/')) {
                out.insert(0, t[start..i + n].iter().collect());
                break;
            }
            k = start;
        }
    }
    out
}

/// The paths relative to the pane's folder in `t`, as (start, length):
/// a word that names a file by its extension (`notes.txt`,
/// `src/ui/rail.rs:12`, `areas/termin…/x.rs`) or starts at `./` or `../`.
/// A word with slashes but no extension (`and/or`) or a number (`v2.1.3`)
/// is not one, nor a repo before its `#42`.
fn relative_paths(t: &[char]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < t.len() {
        let starts = (name_char(t[i]) || t[i] == '.')
            && (i == 0 || !(name_char(t[i - 1]) || "/:#…".contains(t[i - 1])));
        if !starts {
            i += 1;
            continue;
        }
        let j = shortened_run(t, i);
        let n = trailing_trimmed(&t[i..j]);
        let word: String = t[i..i + n].iter().collect();
        let names = match word.split_once('…') {
            Some((_, file)) => file.starts_with('/') && names_a_file(&file[1..]),
            None => names_a_file(&word),
        };
        if t.get(j) != Some(&'#') && names {
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
    base.chars().filter(|c| c.is_alphabetic()).count() >= 1
        && base.trim_start_matches('.').chars().count() >= 2
        && (1..=8).contains(&ext.len())
        && ext.chars().all(|c| c.is_ascii_alphanumeric())
        && ext.chars().any(|c| c.is_ascii_alphabetic())
}

/// Recognize complete tokens, so a path or a larger word is never offered as
/// a partial item id. Soft-wrapped rows have already been joined by `Text`.
fn work_items(t: &[char]) -> Vec<(usize, usize)> {
    use ahjo_issue_contract::issue_ref::{is_bead_id, is_repo_part};
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
                Target::Path(p, _) => Some((p, l.cells[0].line.0)),
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
    fn a_path_opens_at_its_line_without_its_punctuation() {
        let s = screen(60, 2, "wrote /srv/ahjo/src/main.rs:12, done");
        let l = link(&s, 0, 8).unwrap();
        assert_eq!(
            l.target,
            Target::Path("/srv/ahjo/src/main.rs".into(), Some(12))
        );
        assert_eq!(link(&s, 0, 3), None);
        // A lone slash, or a word with slashes but no file in it, is not a link.
        let s = screen(60, 2, "a/b and / alone");
        assert_eq!(link(&s, 0, 1), None);
        assert_eq!(link(&s, 0, 8), None);
    }

    #[test]
    fn the_line_after_a_path_is_carried_and_anything_else_is_not() {
        let s = screen(
            80,
            2,
            "at /a/b.rs:7:3 and cc.rs:0 and dd.rs:x and /e/f.md: done, gg.py:12.",
        );
        let at = |col| link(&s, 0, col).map(|l| l.target);
        assert_eq!(at(4), Some(Target::Path("/a/b.rs".into(), Some(7))));
        assert_eq!(at(20), Some(Target::Path("cc.rs".into(), None)));
        assert_eq!(at(32), Some(Target::Path("dd.rs".into(), None)));
        assert_eq!(at(44), Some(Target::Path("/e/f.md".into(), None)));
        assert_eq!(at(59), Some(Target::Path("gg.py".into(), Some(12))));
        // The line number is not drawn as the link.
        assert_eq!(link(&s, 0, 12), None);
    }

    #[test]
    fn a_file_named_relative_to_the_pane_opens() {
        let s = screen(
            80,
            2,
            "edited src/ui/rail.rs:12 and notes.txt. see ./run and v2.1.3 e.g. x",
        );
        let at = |col| link(&s, 0, col).map(|l| l.target);
        assert_eq!(at(9), Some(Target::Path("src/ui/rail.rs".into(), Some(12))));
        assert_eq!(at(31), Some(Target::Path("notes.txt".into(), None)));
        assert_eq!(at(47), Some(Target::Path("./run".into(), None)));
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
        assert_eq!(l.target, Target::Path("/tmp/x.txt".into(), None));
        assert_eq!(l.cells.first(), Some(&Point::new(Line(0), Column(8))));
        assert_eq!(l.cells.last(), Some(&Point::new(Line(0), Column(17))));
        // The right half of a wide character is that character's cell.
        assert_eq!(link(&s, 0, 1), None);
    }

    #[test]
    fn a_quoted_path_broken_at_the_margin_opens_whole() {
        // Claude Code breaks a long line itself and indents what follows.
        let s = screen(19, 4, "  Read(\"/srv/ahjo/c\n  rates/app/x.rs\")\n");
        let want = Target::Path("/srv/ahjo/crates/app/x.rs".into(), None);
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
            Target::Path("/srv/ahjo/c".into(), None)
        );
        assert_eq!(link(&s, 1, 4), None);
    }

    #[test]
    fn a_path_the_terminal_wrapped_is_one_link() {
        let s = screen(12, 3, "x /home/user/notes.md");
        let want = Target::Path("/home/user/notes.md".into(), None);
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
        assert!(matches!(link(&s, 0, 50).unwrap().target, Target::Path(..)));
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
        let long = Some(Target::Path(format!("{DEEP}/{LONG}"), None));
        assert_eq!(
            path_on(&s, &[(2, 4), (2, 59), (3, 4), (4, 30), (5, 6)]),
            vec![long.clone(); 5]
        );
        let notes = Some(Target::Path(format!("{DEEP}/notes.txt"), None));
        assert_eq!(path_on(&s, &[(6, 10), (7, 20)]), vec![notes; 2]);
        let relative = Some(Target::Path(LONG.into(), Some(2)));
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
        let md = Some(Target::Path(MD.into(), None));
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
        assert_eq!(l.target, Target::Path(MD.into(), None));
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
        let md = Some(Target::Path(format!("{DEEP}/{MD}"), None));
        assert_eq!(path_on(&s, &[(0, 6), (1, 4), (2, 10), (3, 4)]), vec![md; 4]);
        let notes = Some(Target::Path(format!("{DEEP}/notes.txt"), None));
        assert_eq!(path_on(&s, &[(4, 5), (6, 6)]), vec![notes; 2]);
        // The bracket around it is not part of it.
        assert_eq!(link(&s, 3, 37), None);

        let s = screen(
            60,
            3,
            "• Edited areas/terminals/app/src/term/deeply/nested/folder/\n\
             long_file_name_for_wrapping_tests.rs (+1 -0)",
        );
        let long = Some(Target::Path(LONG.into(), None));
        assert_eq!(path_on(&s, &[(0, 12), (1, 0)]), vec![long; 2]);
        assert_eq!(link(&s, 1, 39), None);
    }

    #[test]
    fn a_path_shortened_with_an_ellipsis_is_one_link_for_the_device_to_find() {
        // The rows Claude Code and Codex printed: the click finds the file
        // by the ends either side of the `…` (`printed_path`).
        let s = screen(
            60,
            4,
            "● Update(areas/termin…/long_file_name_for_wrapping_tests.rs)\n\
             • Ran tail -n 2 areas/terminals/app/src/term/deeply/nested/…\n    \
             /tmp/claude-1000/-srv-parvi/4bb7cac5-c5e6-4cf…\n \
             ▝▝   ▝▝   /…/scratchpad/links/proj",
        );
        let at = |row, col| link(&s, row, col).map(|l| l.target);
        let shortened = Some(Target::Path(
            "areas/termin…/long_file_name_for_wrapping_tests.rs".into(),
            None,
        ));
        assert_eq!(at(0, 12), shortened);
        assert_eq!(at(0, 30), shortened);
        assert_eq!(
            at(2, 10),
            Some(Target::Path(
                "/tmp/claude-1000/-srv-parvi/4bb7cac5-c5e6-4cf…".into(),
                None
            ))
        );
        assert_eq!(
            at(3, 14),
            Some(Target::Path("/…/scratchpad/links/proj".into(), None))
        );
        // A relative folder cut where nothing of a name is left names
        // nothing, and neither half of a shortened path is a link alone.
        assert_eq!(at(1, 20), None);
        assert_eq!(at(1, 59), None);
        let s = screen(60, 2, "  /tmp/a/… and areas/termin… then …/x.rs");
        for col in [3, 9, 18, 34] {
            assert_eq!(link(&s, 0, col), None, "column {col}");
        }

        // Codex's own header, as `capture-pane` gave it (0.171.0).
        let s = screen(
            84,
            3,
            "  >_ OpenAI Codex (v0.171.0)\n     \
             /tmp/claude-1000/-srv-ahjo/…/scratchpad/codex\n",
        );
        assert_eq!(
            link(&s, 1, 30).map(|l| l.target),
            Some(Target::Path(
                "/tmp/claude-1000/-srv-ahjo/…/scratchpad/codex".into(),
                None
            ))
        );
    }

    #[test]
    fn the_column_after_a_paths_line_goes_with_it() {
        // The audit's printed `./line.txt:82:4` (ahjo-3v0).
        let s = screen(60, 2, "./line.txt:82:4 and /a/b.rs:7 and cc.md:3:x");
        let at = |col| link(&s, 0, col).map(|l| (l.target, l.column));
        assert_eq!(
            at(3),
            Some((Target::Path("./line.txt".into(), Some(82)), Some(4)))
        );
        assert_eq!(
            at(22),
            Some((Target::Path("/a/b.rs".into(), Some(7)), None))
        );
        assert_eq!(at(35), Some((Target::Path("cc.md".into(), Some(3)), None)));
    }

    // Interactive Codex 0.171.0 in a tmux pane 84 columns wide, as
    // `capture-pane -p` gave it (ahjo-4zw.11): the prompt as typed, then
    // its answer, a bullet Codex broke itself, first after a `-` well
    // short of the edge, then inside each name too long for a row.
    const CODEX_84: &str = "\
› Print this path as a bullet point exactly, without other words or tools: /tmp/
  claude-1000/ahjo-file-lane/4zw-r1/paths/long-directory-
  aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  aaaaaaaaa/more-directory-
  bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
  bbbbbbbbb/long-path-target.txt


• • /tmp/claude-1000/ahjo-file-lane/4zw-r1/paths/long-directory-
    aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
    aaaaaaaaaa/more-directory-
    bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
    bbbbbbbbbb/long-path-target.txt

  Worked for 3s • 11:00 PM";

    #[test]
    fn interactive_codex_breaks_a_bullet_inside_names_too_long_for_a_row() {
        let s = screen(84, 20, CODEX_84);
        let path = format!(
            "/tmp/claude-1000/ahjo-file-lane/4zw-r1/paths/long-directory-{}/more-directory-{}/long-path-target.txt",
            "a".repeat(90),
            "b".repeat(90)
        );
        let whole = Some(Target::Path(path.clone(), None));
        // The answer: every row of it, the first where the click failed.
        assert_eq!(
            path_on(
                &s,
                &[
                    (8, 4),
                    (8, 20),
                    (8, 63),
                    (9, 4),
                    (9, 83),
                    (10, 10),
                    (11, 50),
                    (12, 30)
                ]
            ),
            vec![whole.clone(); 8]
        );
        let l = link(&s, 8, 20).unwrap();
        assert_eq!(l.cells.len(), path.len());
        // The prompt as typed, broken after its `/tmp/` at the edge.
        assert_eq!(
            path_on(&s, &[(0, 78), (1, 2), (2, 82), (3, 2), (4, 40), (5, 20)]),
            vec![whole; 6]
        );
        // Neither the bullets, the indentation nor the words after it.
        for (row, col) in [(8, 0), (8, 2), (9, 1), (14, 4)] {
            assert_eq!(link(&s, row, col), None, "row {row}, column {col}");
        }
    }

    // The same answer once a panel opened beside narrowed the pane to 104
    // columns and Codex drew it again: `directory-` alone on its row.
    const CODEX_104: &str = "\
\u{2022} \u{2022} /tmp/claude-1000/ahjo-file-lane/4zw-r1/paths/long-directory-
    aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/more-
    directory-
    bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb/long-
    path-target.txt

  Worked for 4s \u{2022} 11:50 PM";

    #[test]
    fn interactive_codex_breaks_again_after_a_dash_on_a_row_of_its_own() {
        let s = screen(104, 8, CODEX_104);
        let path = format!(
            "/tmp/claude-1000/ahjo-file-lane/4zw-r1/paths/long-directory-{}/more-directory-{}/long-path-target.txt",
            "a".repeat(90),
            "b".repeat(90)
        );
        let whole = Some(Target::Path(path, None));
        assert_eq!(
            path_on(&s, &[(0, 20), (1, 50), (1, 98), (2, 6), (3, 50), (4, 8)]),
            vec![whole; 6]
        );
        assert_eq!(link(&s, 6, 4), None);
    }

    #[test]
    fn a_word_filling_the_row_after_prose_is_not_joined() {
        // A folder named near the end of a row, then a rule of dashes, or
        // a row of one long word that does not hang where the path or its
        // row's text starts.
        let s = screen(
            30,
            4,
            "see the folder /tmp/out/\n------------------------------\n  and /tmp/a/build-\n    yyyyyyyyyyyyyyyyyyyyyyyyyy",
        );
        assert_eq!(
            link(&s, 0, 17).map(|l| l.target),
            Some(Target::Path("/tmp/out".into(), None))
        );
        assert_eq!(link(&s, 1, 3), None);
        assert_eq!(
            link(&s, 2, 8).map(|l| l.target),
            Some(Target::Path("/tmp/a/build-".into(), None))
        );
        assert_eq!(link(&s, 3, 6), None);
    }

    // The audit's printed paths (ahjo-4zw.7, .8), as `cat` printed them.
    const AUDIT: &str = "/tmp/claude-1000/ahjo-file-lane/4zw-r1/paths";

    #[test]
    fn a_quoted_path_with_spaces_is_all_of_it() {
        let s = screen(
            100,
            2,
            &format!("\"{AUDIT}/space name.txt\"\n`{AUDIT}/My Files/a b.md`:12"),
        );
        let quoted = Some(Target::Path(format!("{AUDIT}/space name.txt"), None));
        assert_eq!(
            path_on(&s, &[(0, 1), (0, 47), (0, 52), (0, 59)]),
            vec![quoted; 4]
        );
        let l = link(&s, 0, 52).unwrap();
        assert_eq!(l.cells.len(), AUDIT.len() + "/space name.txt".len());
        assert!(l.spaced.is_empty());
        // The quotes are not the path.
        assert_eq!(link(&s, 0, 0), None);
        assert_eq!(link(&s, 0, 60), None);
        assert_eq!(
            link(&s, 1, 50).map(|l| l.target),
            Some(Target::Path(format!("{AUDIT}/My Files/a b.md"), Some(12)))
        );
        // Prose in quotes is not a path.
        let s = screen(60, 1, "said \"it's done\" and './a b' then");
        assert_eq!(link(&s, 0, 7), None);
        assert_eq!(
            link(&s, 0, 24).map(|l| l.target),
            Some(Target::Path("./a b".into(), None))
        );
    }

    #[test]
    fn a_bare_path_with_spaces_offers_the_longer_reading_from_either_word() {
        let s = screen(100, 2, &format!("{AUDIT}/space name.txt\n"));
        let spaced = vec![format!("{AUDIT}/space name.txt")];
        // Either word: the shape it has alone, and the reading over the
        // space for the device to choose.
        let l = link(&s, 0, 10).unwrap();
        assert_eq!(l.target, Target::Path(format!("{AUDIT}/space"), None));
        assert_eq!(l.spaced, spaced);
        let l = link(&s, 0, 52).unwrap();
        assert_eq!(l.target, Target::Path("name.txt".into(), None));
        assert_eq!(l.spaced, spaced);
        // Up to three words, each one that names a file, longest first;
        // and prose after a path is a reading only where it names a file.
        let s = screen(
            80,
            2,
            "/srv/My Big Report.pdf\n/tmp/out then run it, and /tmp/x see.md",
        );
        assert_eq!(
            link(&s, 0, 2).unwrap().spaced,
            vec!["/srv/My Big Report.pdf".to_string()]
        );
        assert!(link(&s, 1, 2).unwrap().spaced.is_empty());
        assert_eq!(
            link(&s, 1, 28).unwrap().spaced,
            vec!["/tmp/x see.md".to_string()]
        );
    }

    #[test]
    fn a_unicode_file_name_is_one_path_wherever_it_is_clicked() {
        let printed = format!("{AUDIT}/üñicode.txt");
        // Composed, as the audit printed it, and decomposed, as some file
        // systems give names (`u` and a combining diaeresis).
        let decomposed = format!("{AUDIT}/u\u{308}n\u{303}icode.txt");
        let s = screen(
            100,
            4,
            &format!("{printed}\n{decomposed}\nluotu ./päivä/yö.md.\n已写入/tmp/a.txt"),
        );
        for col in [0, 30, 45, 46, 47, 50, 55] {
            assert_eq!(
                link(&s, 0, col).map(|l| l.target),
                Some(Target::Path(printed.clone(), None)),
                "column {col}"
            );
        }
        assert_eq!(
            link(&s, 1, 47).map(|l| l.target),
            Some(Target::Path(decomposed, None))
        );
        assert_eq!(
            link(&s, 2, 10).map(|l| l.target),
            Some(Target::Path("./päivä/yö.md".into(), None))
        );
        // A path right after Chinese text, which has no spaces, is still one.
        assert_eq!(
            link(&s, 3, 8).map(|l| l.target),
            Some(Target::Path("/tmp/a.txt".into(), None))
        );
    }

    #[test]
    fn a_home_path_the_folder_does_not_tell_is_in_the_nodes_home() {
        // ahjo-4zw.9: the same `~` path from the home folder, then from
        // /tmp after a `cd`, opens the same file.
        let printed = "~/.cache/ahjo-4zw-path-audit/home.txt";
        let node_home = Some("/home/ana");
        let from = |cwd| in_home(&resolve(Some(cwd), printed), node_home);
        let want = "/home/ana/.cache/ahjo-4zw-path-audit/home.txt";
        assert_eq!(from("/home/ana/.cache/ahjo-4zw-path-audit"), want);
        assert_eq!(from("/tmp/claude-1000/ahjo-file-lane/4zw-r1/paths"), want);
        // A folder in another user's home still says whose `~` it is.
        assert_eq!(
            from("/home/bo/x"),
            "/home/bo/.cache/ahjo-4zw-path-audit/home.txt"
        );
        // With no home known it is left for Files to refuse.
        assert_eq!(in_home(printed, None), printed);
        assert_eq!(in_home("~bo/x", node_home), "~bo/x");
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
        assert_eq!(
            at(0, 12),
            Some(Target::Path("/tmp/out/report.md".into(), None))
        );
        assert_eq!(at(1, 3), None);
        assert_eq!(at(2, 18), Some(Target::Path("/tmp/out/build".into(), None)));
        assert_eq!(at(3, 3), None);
        // ... but a name broken just after its first letters is.
        let s = screen(20, 3, "  saved /tmp/a/out.t\n  xt and more");
        let want = Some(Target::Path("/tmp/a/out.txt".into(), None));
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
                    Target::Path(p, _) => Some(p),
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
