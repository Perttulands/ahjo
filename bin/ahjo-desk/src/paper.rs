//! A document as a paper to grade, as the web desk draws it
//! (`/srv/desk/web/app.js`, "Paper"): its Markdown read into blocks whose
//! every shown character knows where it is in the agent's text, so a tap,
//! a drag and every mark convert between the page and the text exactly;
//! the passage a tap takes (a phrase, a sentence, a paragraph); what
//! changed since the round he marked; and the grader's hand, drawn. Pure:
//! `stage` draws it.
//!
//! Offsets are characters into the document's text, as the store keeps a
//! mark's `start` and `end`.

use crate::model::Mark;
use std::ops::Range;

pub const TEXT_MARKS: [&str; 4] = ["tick", "strike", "rewrite", "note"];

/// What each pen is called, as the web desk calls it.
pub fn pen_name(kind: &str) -> &'static str {
    match kind {
        "tick" => "Right",
        "strike" => "Cut",
        "rewrite" => "Rewrite",
        _ => "Note",
    }
}

/// A pen's key while a passage is chosen.
pub fn pen_key(kind: &str) -> &'static str {
    match kind {
        "tick" => "T",
        "strike" => "X",
        "rewrite" => "R",
        _ => "N",
    }
}

// ---- Blocks ----

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Heading(u8),
    Para,
    /// A list item: its number if numbered, and how deep.
    Item {
        number: Option<String>,
        depth: usize,
    },
    Quote,
    Code,
    /// Rows of cells, each cell one of `segs`, in order.
    Table(Vec<usize>),
    Rule,
}

/// A block and the stretches of the text it shows (`segs`).
#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub kind: Kind,
    pub start: usize,
    pub end: usize,
    pub segs: Vec<Range<usize>>,
}

struct Line {
    s: usize,
    e: usize,
    text: Vec<char>,
}

fn lines_of(src: &[char]) -> Vec<Line> {
    let mut out = vec![];
    let mut at = 0;
    for piece in src.split(|c| *c == '\n') {
        out.push(Line {
            s: at,
            e: at + piece.len(),
            text: piece.to_vec(),
        });
        at += piece.len() + 1;
    }
    out
}

fn indent(t: &[char]) -> usize {
    t.iter().take_while(|c| **c == ' ' || **c == '\t').count()
}

fn blank(t: &[char]) -> bool {
    t.iter().all(|c| c.is_whitespace())
}

/// A fence (``` or ~~~, three or more) after at most three spaces.
fn fence(t: &[char]) -> Option<(char, usize)> {
    let i = indent(t);
    if i > 3 {
        return None;
    }
    let c = *t.get(i)?;
    if c != '`' && c != '~' {
        return None;
    }
    let n = t[i..].iter().take_while(|x| **x == c).count();
    (n >= 3).then_some((c, n))
}

/// `#`s and the space after them: the level and where the words start.
fn heading(t: &[char]) -> Option<(u8, usize)> {
    let i = indent(t);
    if i > 3 {
        return None;
    }
    let n = t[i..].iter().take_while(|c| **c == '#').count();
    if !(1..=6).contains(&n) {
        return None;
    }
    let after = i + n;
    match t.get(after) {
        None => Some((n as u8, after)),
        Some(c) if c.is_whitespace() => Some((
            n as u8,
            after + t[after..].iter().take_while(|c| c.is_whitespace()).count(),
        )),
        _ => None,
    }
}

fn rule(t: &[char]) -> bool {
    if indent(t) > 3 {
        return false;
    }
    let s: Vec<char> = t.iter().copied().filter(|c| !c.is_whitespace()).collect();
    s.len() >= 3 && ['-', '*', '_'].iter().any(|m| s.iter().all(|c| c == m))
}

fn table(t: &[char]) -> bool {
    let s: String = t.iter().collect();
    let s = s.trim();
    s.len() > 1 && s.starts_with('|') && s.ends_with('|')
}

fn quote_mark(t: &[char]) -> Option<usize> {
    let i = indent(t);
    (i <= 3 && t.get(i) == Some(&'>')).then(|| i + 1 + usize::from(t.get(i + 1) == Some(&' ')))
}

/// A list marker (`-`, `*`, `+` or `1.` / `1)`) and the whitespace after:
/// the indent, the number if any, and where the words start.
fn list_item(t: &[char]) -> Option<(usize, Option<String>, usize)> {
    let i = indent(t);
    let c = *t.get(i)?;
    let (number, after) = if matches!(c, '-' | '*' | '+') {
        (None, i + 1)
    } else {
        let n = t[i..].iter().take_while(|c| c.is_ascii_digit()).count();
        if n == 0 || n > 9 || !matches!(t.get(i + n), Some('.') | Some(')')) {
            return None;
        }
        (Some(t[i..i + n + 1].iter().collect()), i + n + 1)
    };
    match t.get(after) {
        None => Some((i, number, after)),
        Some(c) if c.is_whitespace() => Some((
            i,
            number,
            after + t[after..].iter().take_while(|c| c.is_whitespace()).count(),
        )),
        _ => None,
    }
}

fn starts_block(t: &[char]) -> bool {
    heading(t).is_some() || fence(t).is_some() || quote_mark(t).is_some() || rule(t) || table(t)
}

/// The document's blocks, in order, each with where its words are.
pub fn blocks(src: &[char]) -> Vec<Block> {
    let lines = lines_of(src);
    let mut out: Vec<Block> = vec![];
    let mut i = 0;
    while i < lines.len() {
        let l = &lines[i];
        let t = &l.text;
        if blank(t) {
            i += 1;
            continue;
        }
        if let Some((c, n)) = fence(t) {
            let mut j = i + 1;
            while j < lines.len() && fence(&lines[j].text).is_none_or(|(d, m)| d != c || m < n) {
                j += 1;
            }
            let s = lines.get(i + 1).map_or(l.e, |x| x.s);
            let e = if j > i + 1 { lines[j - 1].e } else { s };
            out.push(Block {
                kind: Kind::Code,
                start: l.s,
                end: lines[j.min(lines.len() - 1)].e,
                segs: if e > s { vec![s..e] } else { vec![] },
            });
            i = j + 1;
        } else if let Some((level, from)) = heading(t) {
            // Closing #s are not words.
            let mut end = t.len();
            while end > from && t[end - 1].is_whitespace() {
                end -= 1;
            }
            let hashes = t[from..end].iter().rev().take_while(|c| **c == '#').count();
            if hashes > 0 && (end - hashes == from || t[end - hashes - 1].is_whitespace()) {
                end -= hashes;
                while end > from && t[end - 1].is_whitespace() {
                    end -= 1;
                }
            }
            out.push(Block {
                kind: Kind::Heading(level),
                start: l.s,
                end: l.e,
                segs: if end > from {
                    vec![l.s + from..l.s + end]
                } else {
                    vec![]
                },
            });
            i += 1;
        } else if rule(t) {
            out.push(Block {
                kind: Kind::Rule,
                start: l.s,
                end: l.e,
                segs: vec![],
            });
            i += 1;
        } else if quote_mark(t).is_some() {
            let mut segs = vec![];
            let start = l.s;
            while i < lines.len()
                && let Some(q) = quote_mark(&lines[i].text)
            {
                if lines[i].e > lines[i].s + q {
                    segs.push(lines[i].s + q..lines[i].e);
                }
                i += 1;
            }
            out.push(Block {
                kind: Kind::Quote,
                start,
                end: lines[i - 1].e,
                segs,
            });
        } else if table(t) {
            let start = l.s;
            let mut segs = vec![];
            let mut rows = vec![];
            while i < lines.len() && table(&lines[i].text) {
                let r = &lines[i];
                i += 1;
                let divider = r
                    .text
                    .iter()
                    .all(|c| matches!(c, '|' | ':' | '-') || c.is_whitespace());
                if divider {
                    continue;
                }
                let bars: Vec<usize> = (0..r.text.len())
                    .filter(|&k| r.text[k] == '|' && (k == 0 || r.text[k - 1] != '\\'))
                    .collect();
                let mut cells = 0;
                for w in bars.windows(2) {
                    let (mut a, mut b) = (w[0] + 1, w[1]);
                    while a < b && r.text[a].is_whitespace() {
                        a += 1;
                    }
                    while b > a && r.text[b - 1].is_whitespace() {
                        b -= 1;
                    }
                    segs.push(r.s + a..r.s + b);
                    cells += 1;
                }
                rows.push(cells);
            }
            out.push(Block {
                kind: Kind::Table(rows),
                start,
                end: lines[i - 1].e,
                segs,
            });
        } else {
            let item = list_item(t);
            let ind = indent(t);
            let s = match &item {
                Some((_, _, from)) => l.s + from,
                None => l.s + ind,
            };
            let mut e = l.e;
            i += 1;
            while i < lines.len()
                && !blank(&lines[i].text)
                && !starts_block(&lines[i].text)
                && list_item(&lines[i].text).is_none()
            {
                e = lines[i].e;
                i += 1;
            }
            let prev = out.last().map(|b| b.kind.clone());
            let segs = if e > s { vec![s..e] } else { vec![] };
            let kind = match (item, prev) {
                (Some((ind, number, _)), _) => Kind::Item {
                    number,
                    depth: ind / 2,
                },
                (None, Some(Kind::Item { depth, .. })) if ind >= 2 => Kind::Item {
                    number: Some(String::new()),
                    depth,
                },
                _ => Kind::Para,
            };
            out.push(Block {
                kind,
                start: l.s,
                end: e,
                segs,
            });
        }
    }
    out
}

// ---- Inline: what a stretch shows, and where each shown byte came from ----

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    Bold,
    Em,
    Code,
    Struck,
    Link,
}

/// Words shown that are not the document's own: his rewrite, written in
/// after the words it replaces, or words the agent took out since the
/// round he marked.
#[derive(Clone, Debug, PartialEq)]
pub enum Extra {
    Rewrite,
    Gone,
}

/// What one text on the page shows: its words, the document's character
/// each byte shows (`None` for words that are not the document's), its
/// styles, and the words written in.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Piece {
    pub text: String,
    pub src: Vec<Option<usize>>,
    pub styles: Vec<(Range<usize>, Style)>,
    pub extras: Vec<(Range<usize>, Extra)>,
}

impl Piece {
    fn push(&mut self, c: char, at: Option<usize>) {
        let n = c.len_utf8();
        self.text.push(c);
        self.src.extend(std::iter::repeat_n(at, n));
    }

    /// The document's character at byte `i`, or the nearest one shown
    /// before it.
    pub fn source_at(&self, i: usize) -> Option<usize> {
        let i = i.min(self.src.len().saturating_sub(1));
        self.src[..=i]
            .iter()
            .rev()
            .flatten()
            .next()
            .copied()
            .or_else(|| self.src.iter().flatten().next().copied())
    }

    /// The bytes showing the document's characters `range` (start of the
    /// first to the end of the last), if any show here.
    pub fn bytes_of(&self, range: &Range<usize>) -> Option<Range<usize>> {
        let mut first = None;
        let mut last = None;
        for (i, s) in self.src.iter().enumerate() {
            if let Some(s) = s
                && range.contains(s)
            {
                first.get_or_insert(i);
                last = Some(i);
            }
        }
        let last = last?;
        let end = last + self.text[last..].chars().next().map_or(1, char::len_utf8);
        Some(first?..end)
    }

    /// The document's characters this piece shows, first to last.
    pub fn span(&self) -> Option<Range<usize>> {
        let first = self.src.iter().flatten().next()?;
        let last = self.src.iter().rev().flatten().next()?;
        Some(*first..last + 1)
    }
}

fn starts_with(src: &[char], at: usize, pat: &str) -> bool {
    pat.chars()
        .enumerate()
        .all(|(k, c)| src.get(at + k) == Some(&c))
}

fn word(c: Option<&char>) -> bool {
    c.is_some_and(|c| c.is_alphanumeric())
}

/// Where the delimiter `pat` closes a span opened just before `from`:
/// the closer must follow a non-space, and the span hold something.
fn closer(src: &[char], from: usize, end: usize, pat: &str, underscore: bool) -> Option<usize> {
    if src.get(from).is_none_or(|c| c.is_whitespace()) {
        return None;
    }
    let n = pat.chars().count();
    let mut k = from + 1;
    while k + n <= end {
        if starts_with(src, k, pat) && !src[k - 1].is_whitespace() {
            let after = src.get(k + n);
            let fine = if underscore {
                !word(after) && after != Some(&'_')
            } else if pat == "*" {
                after != Some(&'*')
            } else {
                true
            };
            if fine {
                return Some(k);
            }
        }
        k += 1;
    }
    None
}

/// The earliest inline span starting in `s..e`: its kind, where it
/// starts and ends, and the words inside it.
fn next_span(
    src: &[char],
    s: usize,
    e: usize,
) -> Option<(Option<Style>, usize, usize, Range<usize>)> {
    for a in s..e {
        let c = src[a];
        if c == '\\' && a + 1 < e && src[a + 1].is_ascii_punctuation() {
            return Some((None, a, a + 2, a + 1..a + 2));
        }
        if c == '`' {
            if let Some(k) = (a + 1..e).find(|&k| src[k] == '`' || src[k] == '\n')
                && src[k] == '`'
                && k > a + 1
            {
                return Some((Some(Style::Code), a, k + 1, a + 1..k));
            }
            continue;
        }
        if c == '[' || (c == '!' && src.get(a + 1) == Some(&'[')) {
            let open = if c == '!' { a + 1 } else { a };
            if let Some(close) = (open + 1..e).find(|&k| src[k] == ']' || src[k] == '\n')
                && src[close] == ']'
                && close > open + 1
                && src.get(close + 1) == Some(&'(')
                && let Some(paren) = (close + 2..e).find(|&k| src[k] == ')' || src[k] == '\n')
                && src[paren] == ')'
            {
                return Some((Some(Style::Link), a, paren + 1, open + 1..close));
            }
            continue;
        }
        for (pat, style, under) in [
            ("**", Style::Bold, false),
            ("__", Style::Bold, true),
            ("~~", Style::Struck, false),
            ("*", Style::Em, false),
            ("_", Style::Em, true),
        ] {
            if !starts_with(src, a, pat) {
                continue;
            }
            if under && a > 0 && (word(src.get(a - 1)) || src[a - 1] == '_') {
                continue;
            }
            let n = pat.chars().count();
            if let Some(k) = closer(src, a + n, e, pat, under) {
                return Some((Some(style), a, k + n, a + n..k));
            }
            break;
        }
    }
    None
}

/// Builds pieces: the document's characters through `inline`, and words
/// written in after a character.
struct Builder<'a> {
    src: &'a [char],
    /// Words to write in after a character: (the character, words, kind).
    after: &'a [(usize, String, Extra)],
    /// Collapse runs of whitespace into one space (all but code).
    prose: bool,
}

impl Builder<'_> {
    fn chars(&self, piece: &mut Piece, s: usize, e: usize, style: &[Style]) {
        let begin = piece.text.len();
        let mut space = false;
        for k in s..e {
            let c = self.src[k];
            if self.prose && c.is_whitespace() {
                if !space && !piece.text.is_empty() {
                    piece.push(' ', Some(k));
                }
                space = true;
            } else {
                space = false;
                piece.push(c, Some(k));
            }
            self.extras(piece, k);
        }
        for st in style {
            if piece.text.len() > begin {
                piece.styles.push((begin..piece.text.len(), *st));
            }
        }
    }

    fn extras(&self, piece: &mut Piece, k: usize) {
        for (_, words, kind) in self.after.iter().filter(|(at, ..)| *at == k) {
            let from = piece.text.len();
            let lead = if *kind == Extra::Rewrite {
                "\u{2038}"
            } else {
                " "
            };
            for c in lead.chars().chain(words.chars()) {
                piece.push(c, None);
            }
            if matches!(kind, Extra::Gone) {
                piece.push(' ', None);
            }
            piece.extras.push((from..piece.text.len(), kind.clone()));
        }
    }

    fn inline(&self, piece: &mut Piece, s: usize, e: usize, style: &mut Vec<Style>) {
        let mut pos = s;
        while pos < e {
            let Some((kind, a, z, inner)) = next_span(self.src, pos, e) else {
                break;
            };
            self.chars(piece, pos, a, style);
            match kind {
                None => self.chars(piece, inner.start, inner.end, style),
                Some(Style::Code) => {
                    style.push(Style::Code);
                    self.chars(piece, inner.start, inner.end, style);
                    style.pop();
                }
                Some(st) => {
                    style.push(st);
                    self.inline(piece, inner.start, inner.end, style);
                    style.pop();
                }
            }
            pos = z;
        }
        self.chars(piece, pos, e, style);
    }
}

/// What the page shows of the text: each block's pieces (a heading, a
/// paragraph, an item or a quote is one; a table one a cell).
#[derive(Clone, Debug, PartialEq)]
pub struct Shown {
    pub block: Block,
    pub pieces: Vec<Piece>,
}

/// The document as the page shows it. `after` writes words in after a
/// document character (his rewrites, and words taken out since the round
/// he marked); words taken out whole lines at a time are in `gone`
/// (shown as blocks of their own before the block at their place).
pub fn show(src: &[char], after: &[(usize, String, Extra)]) -> Vec<Shown> {
    let mut out = vec![];
    for block in blocks(src) {
        let prose = block.kind != Kind::Code;
        let b = Builder { src, after, prose };
        let pieces = match &block.kind {
            Kind::Rule => vec![],
            Kind::Table(_) => block
                .segs
                .iter()
                .map(|r| {
                    let mut p = Piece::default();
                    b.inline(&mut p, r.start, r.end, &mut vec![]);
                    p
                })
                .collect(),
            Kind::Code => {
                let mut p = Piece::default();
                for r in &block.segs {
                    b.chars(&mut p, r.start, r.end, &[]);
                }
                vec![p]
            }
            _ => {
                let mut p = Piece::default();
                for (n, r) in block.segs.iter().enumerate() {
                    if n > 0 {
                        p.push(' ', None);
                    }
                    b.inline(&mut p, r.start, r.end, &mut vec![]);
                }
                vec![p]
            }
        };
        out.push(Shown { block, pieces });
    }
    out
}

/// The last character of the document shown before `at`: where words
/// written in at `at` go.
pub fn anchor(shown: &[Shown], at: usize) -> Option<usize> {
    let mut best = None;
    for s in shown {
        for p in &s.pieces {
            for c in p.src.iter().flatten() {
                if *c < at && best.is_none_or(|b| *c > b) {
                    best = Some(*c);
                }
            }
        }
    }
    best
}

// ---- Passages ----

/// A tap's passage: a phrase, a sentence or the paragraph around it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Level {
    Phrase,
    #[default]
    Sentence,
    Paragraph,
}

impl Level {
    pub const ALL: [Level; 3] = [Level::Phrase, Level::Sentence, Level::Paragraph];

    pub fn name(self) -> &'static str {
        match self {
            Level::Phrase => "Phrase",
            Level::Sentence => "Sentence",
            Level::Paragraph => "Paragraph",
        }
    }
}

/// The sentences of `text` (chars), each with the space after it: up to
/// and with its `.`, `!` or `?` (and any closing quote or bracket).
pub fn sentences(text: &[char]) -> Vec<Range<usize>> {
    let mut out = vec![];
    let mut start = 0;
    let mut i = 0;
    while i < text.len() {
        if matches!(text[i], '.' | '!' | '?') {
            let mut j = i + 1;
            while j < text.len() && matches!(text[j], '.' | '!' | '?') {
                j += 1;
            }
            while j < text.len() && matches!(text[j], ')' | '"' | '\'' | '”' | '’' | ']') {
                j += 1;
            }
            if j == text.len() || text[j].is_whitespace() {
                while j < text.len() && text[j].is_whitespace() {
                    j += 1;
                }
                out.push(start..j);
                start = j;
            }
            i = j.max(i + 1);
        } else {
            i += 1;
        }
    }
    if start < text.len() {
        out.push(start..text.len());
    }
    out
}

/// The unit of `level` around character `i` of `text` (chars), without
/// the space at its ends.
pub fn unit_at(text: &[char], i: usize, level: Level) -> Range<usize> {
    let (mut a, mut b) = (0, text.len());
    if level != Level::Paragraph {
        if let Some(r) = sentences(text).into_iter().find(|r| r.contains(&i)) {
            (a, b) = (r.start, r.end);
        }
        if level == Level::Phrase {
            let (mut pa, mut pb) = (a, b);
            let mut k = a;
            while k < b {
                let c = text[k];
                let dash = c == '-' && k > 0 && text[k - 1] == ' ' && text.get(k + 1) == Some(&' ');
                if matches!(c, ',' | ';' | ':' | '(' | ')' | '–' | '—') || dash {
                    if k < i {
                        pa = k + 1;
                    } else {
                        pb = k;
                        break;
                    }
                }
                k += 1;
            }
            (a, b) = (pa, pb);
        }
    }
    while a < b && text[a].is_whitespace() {
        a += 1;
    }
    while b > a && text[b - 1].is_whitespace() {
        b -= 1;
    }
    a..b
}

/// The passage of `level` around the document's character `at` in
/// `piece`, as the document's characters.
pub fn passage_at(piece: &Piece, at: usize, level: Level) -> Option<Range<usize>> {
    // The piece's own words (none written in), with each one's place.
    let mut text = vec![];
    let mut map = vec![];
    for (i, c) in piece.text.char_indices() {
        if let Some(s) = piece.src[i] {
            text.push(if c.is_whitespace() { ' ' } else { c });
            map.push(s);
        }
    }
    let i = map
        .iter()
        .position(|s| *s >= at)
        .unwrap_or(map.len().saturating_sub(1));
    let r = unit_at(&text, i, level);
    (r.end > r.start).then(|| map[r.start]..map[r.end - 1] + 1)
}

/// `start..end` without whitespace at either end, in the document.
pub fn trimmed(src: &[char], r: Range<usize>) -> Range<usize> {
    let (mut a, mut b) = (r.start.min(src.len()), r.end.min(src.len()));
    while a < b && src[a].is_whitespace() {
        a += 1;
    }
    while b > a && src[b - 1].is_whitespace() {
        b -= 1;
    }
    a..b
}

/// The document's words from `start` to `end`, on one line.
pub fn quote(src: &[char], r: &Range<usize>) -> String {
    let s: String = src[r.start.min(src.len())..r.end.min(src.len())]
        .iter()
        .collect();
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ---- Grading: what he has in hand ----

/// What he is doing on the paper: the passage he chose and how, the mark
/// on it he is looking at, the pen he is writing with, and a press not
/// yet let go.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Grading {
    /// The passage chosen, as the document's characters.
    pub chosen: Option<Range<usize>>,
    /// Where a tap chose it, so it widens and narrows (none for a drag).
    pub tapped: Option<usize>,
    pub level: Level,
    /// His mark on the chosen passage, when he chose a mark.
    pub mark: Option<String>,
    /// The pen he is writing with: a rewrite or a note.
    pub writing: Option<&'static str>,
    /// A press on the text: where it went down and where it is now.
    pub press: Option<(usize, usize)>,
    /// What changed since the round he marked is shown on the page.
    pub hide_changes: bool,
}

impl Grading {
    /// Lets go of the passage, its mark and the pen.
    pub fn close(&mut self) {
        let keep = (self.level, self.hide_changes);
        *self = Grading::default();
        (self.level, self.hide_changes) = keep;
    }

    /// The passage under the press now (start to end, either way).
    pub fn pressed(&self) -> Option<Range<usize>> {
        let (a, b) = self.press?;
        Some(a.min(b)..a.max(b) + 1)
    }
}

// ---- Marks on the text ----

pub fn is_text(m: &Mark) -> bool {
    TEXT_MARKS.contains(&m.kind.as_str())
}

pub fn range(m: &Mark) -> Option<Range<usize>> {
    Some(m.start?..m.end?)
}

/// His marks on the text, in reading order.
pub fn text_marks(marks: &[Mark]) -> Vec<Mark> {
    let mut v: Vec<Mark> = marks
        .iter()
        .filter(|m| is_text(m) && range(m).is_some())
        .cloned()
        .collect();
    v.sort_by_key(|m| (m.start, m.end));
    v
}

/// Puts `mark` among `marks`: one verdict to a passage, so a tick, cut or
/// rewrite on exactly the same words replaces the one there; a note is a
/// word beside them and replaces only an earlier note.
pub fn place(marks: &mut Vec<Mark>, mark: Mark) {
    let note = mark.kind == "note";
    marks.retain(|m| {
        !is_text(m) || m.start != mark.start || m.end != mark.end || (m.kind == "note") != note
    });
    marks.push(mark);
}

/// The innermost of his marks under the document's character `at`.
pub fn mark_at(marks: &[Mark], at: usize) -> Option<Mark> {
    text_marks(marks)
        .into_iter()
        .filter(|m| range(m).is_some_and(|r| r.contains(&at)))
        .min_by_key(|m| range(m).map_or(usize::MAX, |r| r.len()))
}

// ---- What changed since the round he marked ----

/// A stretch of the difference between the old text and the new:
/// kept (`p` in the old, `o` in the new, `n` long), taken out (`p`, `n`;
/// it stood where `at` is in the new; whole lines when `block`), or put in
/// (`o`, `n`). Characters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seg {
    Kept {
        p: usize,
        o: usize,
        n: usize,
    },
    Gone {
        p: usize,
        n: usize,
        at: usize,
        block: bool,
    },
    New {
        o: usize,
        n: usize,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Same(usize, usize),
    Del(usize),
    Add(usize),
}

/// The longest common subsequence's edit script of two lists.
fn lcs<T: PartialEq>(a: &[T], b: &[T]) -> Vec<Op> {
    let mut p = 0;
    while p < a.len() && p < b.len() && a[p] == b[p] {
        p += 1;
    }
    let mut q = 0;
    while q < a.len() - p && q < b.len() - p && a[a.len() - 1 - q] == b[b.len() - 1 - q] {
        q += 1;
    }
    let (n, m) = (a.len() - p - q, b.len() - p - q);
    let mut ops: Vec<Op> = (0..p).map(|i| Op::Same(i, i)).collect();
    if n * m > 4_000_000 {
        ops.extend((0..n).map(|i| Op::Del(p + i)));
        ops.extend((0..m).map(|j| Op::Add(p + j)));
    } else {
        let w = m + 1;
        let mut l = vec![0u32; (n + 1) * w];
        for i in (0..n).rev() {
            for j in (0..m).rev() {
                l[i * w + j] = if a[p + i] == b[p + j] {
                    l[(i + 1) * w + j + 1] + 1
                } else {
                    l[(i + 1) * w + j].max(l[i * w + j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < n && j < m {
            if a[p + i] == b[p + j] {
                ops.push(Op::Same(p + i, p + j));
                i += 1;
                j += 1;
            } else if l[(i + 1) * w + j] >= l[i * w + j + 1] {
                ops.push(Op::Del(p + i));
                i += 1;
            } else {
                ops.push(Op::Add(p + j));
                j += 1;
            }
        }
        ops.extend((i..n).map(|i| Op::Del(p + i)));
        ops.extend((j..m).map(|j| Op::Add(p + j)));
    }
    ops.extend((1..=q).rev().map(|k| Op::Same(a.len() - k, b.len() - k)));
    ops
}

#[derive(Clone, Copy, Debug)]
struct Tok {
    s: usize,
    e: usize,
}

/// Words, runs of space and single marks.
fn tokens(t: &[char], s: usize, e: usize) -> Vec<Tok> {
    let mut out = vec![];
    let mut k = s;
    while k < e {
        let c = t[k];
        let start = k;
        if c.is_whitespace() {
            while k < e && t[k].is_whitespace() {
                k += 1;
            }
        } else if c.is_alphanumeric() {
            k += 1;
            while k < e && (t[k].is_alphanumeric() || matches!(t[k], '\'' | '’' | '_' | '-')) {
                k += 1;
            }
        } else {
            k += 1;
        }
        out.push(Tok { s: start, e: k });
    }
    out
}

/// What changed from `old` to `now`: lines first, then the words inside
/// changed lines; a sentence rewritten through reads whole.
pub fn diff(old: &[char], now: &[char]) -> Vec<Seg> {
    let lines = |t: &[char]| {
        let mut at = 0;
        let mut v: Vec<(usize, usize)> = t
            .split(|c| *c == '\n')
            .map(|l| {
                let r = (at, at + l.len() + 1);
                at = r.1;
                r
            })
            .collect();
        if let Some(last) = v.last_mut() {
            last.1 -= 1;
        }
        v
    };
    let (la, lb) = (lines(old), lines(now));
    let text = |t: &[char], r: (usize, usize)| {
        t[r.0..r.1]
            .iter()
            .collect::<String>()
            .trim_end_matches('\n')
            .to_string()
    };
    let ta: Vec<String> = la.iter().map(|r| text(old, *r)).collect();
    let tb: Vec<String> = lb.iter().map(|r| text(now, *r)).collect();
    let mut segs: Vec<Seg> = vec![];
    let mut cursor = 0;
    let keep = |segs: &mut Vec<Seg>, cursor: &mut usize, p: usize, o: usize, n: usize| {
        if let Some(Seg::Kept {
            p: gp,
            o: go,
            n: gn,
        }) = segs.last_mut()
            && *gp + *gn == p
            && *go + *gn == o
        {
            *gn += n;
        } else {
            segs.push(Seg::Kept { p, o, n });
        }
        *cursor = o + n;
    };
    let ops = lcs(&ta, &tb);
    let mut i = 0;
    while i < ops.len() {
        if let Op::Same(x, y) = ops[i] {
            let (a, b) = (la[x], lb[y]);
            keep(&mut segs, &mut cursor, a.0, b.0, (a.1 - a.0).min(b.1 - b.0));
            i += 1;
            continue;
        }
        let mut dels = vec![];
        let mut adds = vec![];
        while i < ops.len() && !matches!(ops[i], Op::Same(..)) {
            match ops[i] {
                Op::Del(x) => dels.push(la[x]),
                Op::Add(y) => adds.push(lb[y]),
                Op::Same(..) => {}
            }
            i += 1;
        }
        if adds.is_empty() {
            let (ps, pe) = (dels[0].0, dels[dels.len() - 1].1);
            segs.push(Seg::Gone {
                p: ps,
                n: pe - ps,
                at: cursor,
                block: true,
            });
            continue;
        }
        let (os, oe) = (adds[0].0, adds[adds.len() - 1].1);
        if dels.is_empty() {
            segs.push(Seg::New { o: os, n: oe - os });
            cursor = oe;
            continue;
        }
        let (ps, pe) = (dels[0].0, dels[dels.len() - 1].1);
        let wa = tokens(old, ps, pe);
        let wb = tokens(now, os, oe);
        let sa: Vec<String> = wa.iter().map(|t| old[t.s..t.e].iter().collect()).collect();
        let sb: Vec<String> = wb.iter().map(|t| now[t.s..t.e].iter().collect()).collect();
        let mut wops = lcs(&sa, &sb);
        let same: usize = wops
            .iter()
            .filter_map(|o| match o {
                Op::Same(x, _) => Some(sa[*x].trim().chars().count()),
                _ => None,
            })
            .sum();
        let solid =
            |t: &[char], s: usize, e: usize| t[s..e].iter().filter(|c| !c.is_whitespace()).count();
        let size = solid(old, ps, pe).min(solid(now, os, oe));
        if (same as f32) < size as f32 * 0.5 {
            segs.push(Seg::Gone {
                p: ps,
                n: pe - ps,
                at: os,
                block: true,
            });
            segs.push(Seg::New { o: os, n: oe - os });
            cursor = oe;
            continue;
        }
        // A lone short word or mark kept between two changes is folded in.
        let mut k = 0;
        while k < wops.len() {
            if !matches!(wops[k], Op::Same(..)) {
                k += 1;
                continue;
            }
            let mut z = k;
            while z + 1 < wops.len() && matches!(wops[z + 1], Op::Same(..)) {
                z += 1;
            }
            let kept: Vec<&String> = wops[k..=z]
                .iter()
                .filter_map(|o| {
                    if let Op::Same(x, _) = o {
                        Some(&sa[*x])
                    } else {
                        None
                    }
                })
                .collect();
            let words = kept.iter().filter(|t| !t.trim().is_empty()).count();
            let len: usize = kept.iter().map(|t| t.chars().count()).sum::<usize>();
            let joined: String = kept.iter().map(|t| t.as_str()).collect();
            if k > 0
                && z < wops.len() - 1
                && words <= 1
                && joined.trim().chars().count() <= 4
                && len > 0
            {
                let run: Vec<Op> = wops[k..=z].to_vec();
                let mut repl: Vec<Op> = run
                    .iter()
                    .map(|o| {
                        if let Op::Same(x, _) = o {
                            Op::Del(*x)
                        } else {
                            *o
                        }
                    })
                    .collect();
                repl.extend(run.iter().map(|o| {
                    if let Op::Same(_, y) = o {
                        Op::Add(*y)
                    } else {
                        *o
                    }
                }));
                let added = repl.len();
                wops.splice(k..=z, repl);
                z = k + added - 1;
            }
            k = z + 1;
        }
        let mut k = 0;
        while k < wops.len() {
            if let Op::Same(x, y) = wops[k] {
                let (a, b) = (wa[x], wb[y]);
                keep(&mut segs, &mut cursor, a.s, b.s, a.e - a.s);
                k += 1;
                continue;
            }
            let mut d = vec![];
            let mut add = vec![];
            while k < wops.len() && !matches!(wops[k], Op::Same(..)) {
                match wops[k] {
                    Op::Del(x) => d.push(wa[x]),
                    Op::Add(y) => add.push(wb[y]),
                    Op::Same(..) => {}
                }
                k += 1;
            }
            let at = add.first().map_or(cursor, |t| t.s);
            if let (Some(f), Some(l)) = (d.first(), d.last()) {
                segs.push(Seg::Gone {
                    p: f.s,
                    n: l.e - f.s,
                    at,
                    block: false,
                });
            }
            if let (Some(f), Some(l)) = (add.first(), add.last()) {
                segs.push(Seg::New {
                    o: f.s,
                    n: l.e - f.s,
                });
                cursor = l.e;
            }
        }
    }
    segs
}

/// Where a stretch of the old text is now: its kept parts in the new
/// text; whether any of it was taken out.
pub fn map_old(segs: &[Seg], r: &Range<usize>) -> (Vec<Range<usize>>, bool) {
    let mut kept = vec![];
    let mut touched = false;
    for g in segs {
        match *g {
            Seg::Kept { p, o, n } if p < r.end && p + n > r.start => {
                let (a, b) = (r.start.max(p), r.end.min(p + n));
                kept.push(o + a - p..o + b - p);
            }
            Seg::Gone { p, n, .. } if p < r.end && p + n > r.start => touched = true,
            _ => {}
        }
    }
    (kept, touched)
}

/// Where the old text's character `q` is in the new.
pub fn anchor_old(segs: &[Seg], q: usize) -> usize {
    let mut at = 0;
    for g in segs {
        match *g {
            Seg::Kept { p, o, n } => {
                if q >= p && q <= p + n {
                    return o + q - p;
                }
                if p > q {
                    break;
                }
                at = o + n;
            }
            Seg::Gone {
                p, n, at: there, ..
            } => {
                if q >= p && q <= p + n {
                    return there;
                }
            }
            Seg::New { o, n } => at = o + n,
        }
    }
    at
}

/// Did the agent act on an old mark: the passage changed, a rewrite's
/// words are in the new text, or not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Followed {
    AsWritten,
    Changed,
    Unchanged,
}

impl Followed {
    pub fn words(self) -> &'static str {
        match self {
            Followed::AsWritten => "as written",
            Followed::Changed => "changed",
            Followed::Unchanged => "unchanged",
        }
    }
}

pub fn followed(m: &Mark, segs: &[Seg], now: &[char]) -> Followed {
    let one = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    let now: String = now.iter().collect();
    if m.kind == "rewrite" && !m.text.trim().is_empty() && one(&now).contains(&one(&m.text)) {
        return Followed::AsWritten;
    }
    match range(m) {
        Some(r) if map_old(segs, &r).1 => Followed::Changed,
        _ => Followed::Unchanged,
    }
}

// ---- The paper: a round's document, read once ----

/// The round he marked before this one, as it changed since.
#[derive(Clone, Debug, PartialEq)]
pub struct Before {
    pub round: u32,
    pub text: Vec<char>,
    pub segs: Vec<Seg>,
    /// His marks on its text.
    pub marks: Vec<Mark>,
}

impl Before {
    /// How many of the passages he marked the agent changed.
    pub fn acted_on(&self, now: &[char]) -> usize {
        self.marks
            .iter()
            .filter(|m| followed(m, &self.segs, now) != Followed::Unchanged)
            .count()
    }
}

/// A round's document: its characters, its blocks as shown without
/// anything written in, and the round he marked before it.
#[derive(Clone, Debug, PartialEq)]
pub struct Paper {
    pub text: Vec<char>,
    pub plain: Vec<Shown>,
    pub before: Option<Before>,
}

impl Paper {
    /// `before`: the last earlier round with a document he marked on: its
    /// number, text and marks.
    pub fn new(text: &str, before: Option<(u32, &str, &[Mark])>) -> Self {
        let text: Vec<char> = text.chars().collect();
        let plain = show(&text, &[]);
        let before = before.and_then(|(round, old, marks)| {
            let marks = text_marks(marks);
            if marks.is_empty() {
                return None;
            }
            let old: Vec<char> = old.chars().collect();
            Some(Before {
                round,
                segs: diff(&old, &text),
                text: old,
                marks,
            })
        });
        Paper {
            text,
            plain,
            before,
        }
    }

    /// The page as he sees it now: his rewrites written in, and with
    /// `changes` the words taken out since the round he marked; and the
    /// blocks taken out whole, each with the block it stood before.
    pub fn shown(&self, marks: &[Mark], changes: bool) -> (Vec<Shown>, Vec<(usize, String)>) {
        let mut after = vec![];
        for m in text_marks(marks) {
            if m.kind == "rewrite"
                && !m.text.trim().is_empty()
                && let Some(r) = range(&m)
                && let Some(at) = anchor(&self.plain, r.end)
            {
                after.push((at, m.text.trim().to_string(), Extra::Rewrite));
            }
        }
        let mut gone_blocks = vec![];
        if changes && let Some(b) = &self.before {
            for g in &b.segs {
                if let Seg::Gone { p, n, at, block } = *g {
                    let words = quote(&b.text, &(p..p + n));
                    if words.is_empty() {
                        continue;
                    }
                    if block {
                        let i = self
                            .plain
                            .iter()
                            .position(|s| s.block.end >= at)
                            .unwrap_or(self.plain.len());
                        gone_blocks.push((i, words));
                    } else if let Some(c) = anchor(&self.plain, at) {
                        after.push((c, words, Extra::Gone));
                    }
                }
            }
        }
        (show(&self.text, &after), gone_blocks)
    }

    /// The new text since the round he marked, as the document's
    /// characters.
    pub fn new_since(&self) -> Vec<Range<usize>> {
        let Some(b) = &self.before else {
            return vec![];
        };
        b.segs
            .iter()
            .filter_map(|g| match *g {
                Seg::New { o, n } => Some(o..o + n),
                _ => None,
            })
            .collect()
    }
}

// ---- The grader's hand ----

/// A small seeded random source, so a mark's stroke is the same each time
/// it is drawn and differs from the next mark's.
pub struct Hand(u32);

impl Hand {
    pub fn new(seed: &str) -> Self {
        let mut h: u32 = 2166136261;
        for b in seed.bytes() {
            h = (h ^ b as u32).wrapping_mul(16777619);
        }
        Hand(h)
    }

    pub fn draw(&mut self) -> f32 {
        self.0 = self.0.wrapping_add(0x6D2B79F5);
        let mut t = self.0;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        ((t ^ (t >> 14)) as f64 / 4294967296.) as f32
    }
}

/// A line's box on the page: x, y, width, height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// A stroke as cubic pieces: the start and each (control, control, end).
pub type Stroke = ((f32, f32), Vec<[(f32, f32); 3]>);

/// Points joined by a smooth curve through each (Catmull-Rom as Béziers).
pub fn smooth(ps: &[(f32, f32)]) -> Stroke {
    let mut out = vec![];
    for i in 0..ps.len().saturating_sub(1) {
        let p0 = if i > 0 { ps[i - 1] } else { ps[i] };
        let (p1, p2) = (ps[i], ps[i + 1]);
        let p3 = ps.get(i + 2).copied().unwrap_or(p2);
        out.push([
            (p1.0 + (p2.0 - p0.0) / 6., p1.1 + (p2.1 - p0.1) / 6.),
            (p2.0 - (p3.0 - p1.0) / 6., p2.1 - (p3.1 - p1.1) / 6.),
            p2,
        ]);
    }
    (ps.first().copied().unwrap_or_default(), out)
}

/// A line drawn by hand along `r` at height `y`: a little past each end,
/// a little tilted, never quite straight.
pub fn hand_line(r: Rect, y: f32, hand: &mut Hand) -> Stroke {
    let n = ((r.w / 70.).round() as usize).max(2);
    let tilt = (hand.draw() - 0.5) * (r.w / 30.).min(4.);
    let ps: Vec<(f32, f32)> = (0..=n)
        .map(|k| {
            let f = k as f32 / n as f32;
            (
                r.x - 3. + (r.w + 6.) * f,
                y + tilt * (f - 0.5) + (hand.draw() - 0.5) * 1.5,
            )
        })
        .collect();
    smooth(&ps)
}

/// A loop drawn around `r`, its end running a little past its start.
pub fn hand_loop(r: Rect, hand: &mut Hand) -> Stroke {
    let (cx, cy) = (r.x + r.w / 2., r.y + r.h / 2.);
    let (rx, ry) = (r.w / 2. + 8., r.h / 2. + 5.);
    let a0 = -std::f32::consts::PI * (0.6 + hand.draw() * 0.15);
    let turn = std::f32::consts::TAU * (1.1 + hand.draw() * 0.01);
    let ps: Vec<(f32, f32)> = (0..=30)
        .map(|k| {
            let f = k as f32 / 30.;
            let t = a0 + turn * f;
            let grow = 1. + 0.1 * f;
            let wob = 1. + (hand.draw() - 0.5) * 0.05;
            (
                cx + t.cos() * rx * wob * grow,
                cy + t.sin() * ry * wob * grow,
            )
        })
        .collect();
    smooth(&ps)
}

/// A tick from `(x, y)`, `h` tall.
pub fn hand_tick(x: f32, y: f32, h: f32) -> Stroke {
    let s = h / 20.;
    smooth(&[
        (x, y),
        (x + 3. * s, y + 4. * s),
        (x + 6. * s, y + 8. * s),
        (x + 11. * s, y - s),
        (x + 19. * s, y - 11. * s),
    ])
}

/// The strokes of a mark on lines `lines` (top to bottom): a cut or a
/// rewrite strikes each line through, a note on one line is looped and on
/// several underlined, a tick stands after the last.
pub fn ink(kind: &str, id: &str, lines: &[Rect]) -> Vec<Stroke> {
    let mut hand = Hand::new(id);
    let mut out = vec![];
    match kind {
        "strike" | "rewrite" => {
            for r in lines {
                let y = r.y + r.h * (0.55 + (hand.draw() - 0.5) * 0.08);
                out.push(hand_line(*r, y, &mut hand));
            }
        }
        "note" if lines.len() == 1 => out.push(hand_loop(lines[0], &mut hand)),
        "note" => {
            for r in lines {
                out.push(hand_line(*r, r.y + r.h + 1.5, &mut hand));
            }
        }
        "tick" => {
            if let Some(l) = lines.last() {
                out.push(hand_tick(l.x + l.w + 3., l.y + l.h * 0.55, l.h));
            }
        }
        _ => {}
    }
    out
}

/// Boxes on one line (their middles within half the shorter's height)
/// joined into one, top to bottom.
pub fn join_lines(mut rects: Vec<Rect>) -> Vec<Rect> {
    rects.sort_by(|p, q| p.y.total_cmp(&q.y).then(p.x.total_cmp(&q.x)));
    let mut lines: Vec<Rect> = vec![];
    for r in rects {
        let mid = r.y + r.h / 2.;
        match lines
            .iter_mut()
            .find(|l| (l.y + l.h / 2. - mid).abs() < l.h.min(r.h) / 2.)
        {
            Some(l) => {
                let (x0, x1) = (l.x.min(r.x), (l.x + l.w).max(r.x + r.w));
                let (y0, y1) = (l.y.min(r.y), (l.y + l.h).max(r.y + r.h));
                *l = Rect {
                    x: x0,
                    y: y0,
                    w: x1 - x0,
                    h: y1 - y0,
                };
            }
            None => lines.push(r),
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    fn shown_text(src: &str) -> Vec<String> {
        show(&chars(src), &[])
            .into_iter()
            .flat_map(|s| s.pieces.into_iter().map(|p| p.text))
            .collect()
    }

    #[test]
    fn markdown_reads_into_blocks_that_know_their_words() {
        let src = "# The *board*\n\nA gate is **one** thing,\nwith `exits`.\n\n- first\n- [link](x.md) second\n\n| A | B |\n|---|---|\n| c | d |\n\n> quoted\n\n```\ncode  here\n```\n";
        assert_eq!(
            shown_text(src),
            [
                "The board",
                "A gate is one thing, with exits.",
                "first",
                "link second",
                "A",
                "B",
                "c",
                "d",
                "quoted",
                "code  here"
            ]
        );
        // Every shown character is the document's own, where it stands.
        let c = chars(src);
        for s in show(&c, &[]) {
            for p in s.pieces {
                for (i, ch) in p.text.char_indices() {
                    let at = p.src[i].expect("the document's");
                    if !ch.is_whitespace() {
                        assert_eq!(c[at], ch);
                    }
                }
            }
        }
    }

    #[test]
    fn a_tap_takes_a_sentence_and_narrows_or_widens() {
        let src = "Gates decide. A gate, once passed, opens the next step! The end";
        let s = show(&chars(src), &[]);
        let p = &s[0].pieces[0];
        let at = src.find("once").unwrap();
        let q = |r: Range<usize>| src.chars().skip(r.start).take(r.len()).collect::<String>();
        assert_eq!(
            q(passage_at(p, at, Level::Sentence).unwrap()),
            "A gate, once passed, opens the next step!"
        );
        assert_eq!(q(passage_at(p, at, Level::Phrase).unwrap()), "once passed");
        assert_eq!(q(passage_at(p, at, Level::Paragraph).unwrap()), src);
    }

    #[test]
    fn a_rewrite_is_written_in_after_its_words() {
        let src = "Keep this. Cut that.";
        let c = chars(src);
        let end = src.find("that.").unwrap() + 4;
        let s = show(&c, &[(end, "this other".into(), Extra::Rewrite)]);
        let p = &s[0].pieces[0];
        assert_eq!(p.text, "Keep this. Cut that.\u{2038}this other");
        let (r, kind) = &p.extras[0];
        assert_eq!(kind, &Extra::Rewrite);
        assert!(p.src[r.clone()].iter().all(Option::is_none));
    }

    #[test]
    fn one_verdict_to_a_passage_and_a_note_beside_it() {
        let m = |id: &str, kind: &str| Mark {
            id: id.into(),
            kind: kind.into(),
            start: Some(3),
            end: Some(9),
            ..Default::default()
        };
        let mut marks = vec![m("a", "tick"), m("b", "note")];
        place(&mut marks, m("c", "strike"));
        let kinds: Vec<&str> = marks.iter().map(|m| m.kind.as_str()).collect();
        assert_eq!(kinds, ["note", "strike"]);
        place(&mut marks, m("d", "note"));
        let ids: Vec<&str> = marks.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["c", "d"]);
    }

    #[test]
    fn changes_since_the_last_round_read_as_words_and_lines() {
        let old = chars("Intro line.\nThe gate is fine as it was.\nA line taken out.\nEnd.");
        let now = chars("Intro line.\nThe gate is one element now.\nEnd.\nA new last line.");
        let segs = diff(&old, &now);
        let gone: Vec<String> = segs
            .iter()
            .filter_map(|g| match g {
                Seg::Gone { p, n, .. } => Some(old[*p..p + n].iter().collect()),
                _ => None,
            })
            .collect();
        assert!(
            gone.iter().any(|g| g.contains("A line taken out")),
            "{gone:?}"
        );
        let new: Vec<String> = segs
            .iter()
            .filter_map(|g| match g {
                Seg::New { o, n } => Some(now[*o..o + n].iter().collect()),
                _ => None,
            })
            .collect();
        assert!(new.iter().any(|g| g.contains("A new last line")), "{new:?}");
        // A mark on a kept passage maps to where it is now, untouched.
        let at = |s: &[char], w: &str| s.iter().collect::<String>().find(w).unwrap();
        let intro = at(&old, "Intro")..at(&old, "Intro") + 5;
        let (kept, touched) = map_old(&segs, &intro);
        assert_eq!(kept, vec![at(&now, "Intro")..at(&now, "Intro") + 5]);
        assert!(!touched);
        let cut = Mark {
            id: "m".into(),
            kind: "strike".into(),
            start: Some(at(&old, "A line taken")),
            end: Some(at(&old, "A line taken") + 17),
            ..Default::default()
        };
        assert_eq!(followed(&cut, &segs, &now), Followed::Changed);
    }

    #[test]
    fn the_hand_is_the_same_each_time_and_differs_by_mark() {
        let r = [Rect {
            x: 10.,
            y: 20.,
            w: 200.,
            h: 18.,
        }];
        assert_eq!(ink("strike", "d1", &r), ink("strike", "d1", &r));
        assert_ne!(ink("strike", "d1", &r), ink("strike", "d2", &r));
        assert_eq!(ink("tick", "d1", &r).len(), 1);
    }
}
