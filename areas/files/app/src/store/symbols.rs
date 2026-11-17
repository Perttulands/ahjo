//! The symbols a file defines, found line by line: functions, types and
//! their kin in code, headings in Markdown. A light scan, not a parser: it
//! finds what starts a line, which is where most definitions stand.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    /// What it is, in a word: "fn", "struct", "class", "#"…
    pub kind: &'static str,
    /// Its line, from 0.
    pub line: usize,
    /// How deep it stands: its indent's steps, or a heading's level less one.
    pub depth: usize,
}

/// The words that open a definition, by the file's extension, and what
/// each one is called.
fn openers(ext: &str) -> &'static [(&'static str, &'static str)] {
    match ext {
        "rs" => &[
            ("fn ", "fn"),
            ("struct ", "struct"),
            ("enum ", "enum"),
            ("trait ", "trait"),
            ("impl ", "impl"),
            ("impl<", "impl"),
            ("mod ", "mod"),
            ("type ", "type"),
            ("const ", "const"),
            ("static ", "static"),
            ("macro_rules! ", "macro"),
        ],
        "py" => &[("def ", "def"), ("class ", "class")],
        "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" => &[
            ("function ", "function"),
            ("function* ", "function"),
            ("class ", "class"),
            ("interface ", "interface"),
            ("type ", "type"),
            ("enum ", "enum"),
        ],
        "go" => &[("func ", "func"), ("type ", "type")],
        "sh" | "bash" => &[("function ", "function")],
        _ => &[],
    }
}

/// Words that may stand before an opener.
const LEADS: [&str; 12] = [
    "pub ",
    "pub(crate) ",
    "pub(super) ",
    "async ",
    "unsafe ",
    "export ",
    "default ",
    "extern ",
    "abstract ",
    "declare ",
    "static ",
    "const ",
];

pub fn of(file_name: &str, text: &str) -> Vec<Symbol> {
    let ext = super::kinds::extension(file_name);
    if matches!(ext.as_str(), "md" | "markdown") {
        return headings(text);
    }
    let openers = openers(&ext);
    if openers.is_empty() {
        return vec![];
    }
    // The indent one step is, found from the first indented definition.
    let mut step = 0;
    let mut out = vec![];
    for (n, line) in text.lines().enumerate() {
        let body = line.trim_start();
        let indent = line.len() - body.len();
        let mut rest = body;
        // Leads go; one that is an opener itself (`const`) stays unless
        // another opener follows it (`const fn`).
        while let Some(lead) = LEADS.iter().find(|l| rest.starts_with(**l)) {
            let after = &rest[lead.len()..];
            let opens = |t: &str| openers.iter().any(|(o, _)| t.starts_with(o));
            if opens(rest) && !opens(after) {
                break;
            }
            rest = after;
        }
        // `pub(in path) fn`.
        if rest.starts_with("pub(")
            && let Some(close) = rest.find(") ")
        {
            rest = &rest[close + 2..];
        }
        let Some((opener, kind)) = openers.iter().find(|(o, _)| rest.starts_with(o)) else {
            continue;
        };
        // An impl's generics are read past, so it keeps its `<`.
        let after = if *kind == "impl" {
            &rest[4..]
        } else {
            &rest[opener.len()..]
        };
        let name = name_of(after.trim_start(), kind);
        if name.is_empty() {
            continue;
        }
        if indent > 0 && step == 0 {
            step = indent;
        }
        out.push(Symbol {
            name,
            kind,
            line: n,
            depth: indent.checked_div(step).unwrap_or(0),
        });
    }
    out
}

/// The name a definition gives: up to what ends a name. An `impl` names
/// what it is for, `impl Trait for Type` as written.
fn name_of(after: &str, kind: &str) -> String {
    if kind == "impl" {
        // `impl<T> …`: past its generics.
        let mut after = after;
        if let Some(inner) = after.strip_prefix('<') {
            let mut depth = 1;
            let end = inner.char_indices().find_map(|(i, c)| {
                depth += match c {
                    '<' => 1,
                    '>' => -1,
                    _ => 0,
                };
                (depth == 0).then_some(i + 1)
            });
            after = end.map_or("", |e| &inner[e..]);
        }
        let head = after.split(['{', '\n']).next().unwrap_or("");
        let head = head.split(" where").next().unwrap_or(head);
        return head.trim().to_owned();
    }
    // Go methods: `func (r *T) Name(`.
    let after = if kind == "func" && after.starts_with('(') {
        after.split_once(") ").map_or(after, |(_, rest)| rest)
    } else {
        after
    };
    after
        .trim_start_matches('*')
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$' || *c == '!')
        .collect()
}

/// A Markdown file's headings, outside its fenced code.
fn headings(text: &str) -> Vec<Symbol> {
    let mut out = vec![];
    let mut fence: Option<&str> = None;
    for (n, line) in text.lines().enumerate() {
        let t = line.trim_start();
        for f in ["```", "~~~"] {
            if t.starts_with(f) {
                fence = match fence {
                    Some(open) if open == f => None,
                    None => Some(f),
                    other => other,
                };
            }
        }
        if fence.is_some() || line.starts_with(' ') {
            continue;
        }
        let level = t.chars().take_while(|&c| c == '#').count();
        if (1..=6).contains(&level) && t[level..].starts_with(' ') {
            out.push(Symbol {
                name: t[level..].trim().trim_end_matches('#').trim().to_owned(),
                kind: "#",
                line: n,
                depth: level - 1,
            });
        }
    }
    out
}

/// The symbol a line is in: the last one at or above it.
pub fn at(symbols: &[Symbol], line: usize) -> Option<&Symbol> {
    symbols.iter().rev().find(|s| s.line <= line)
}

/// The symbols whose name has `query` in it, closest first: a name that
/// starts with it before one that only has it.
pub fn matching<'a>(symbols: &'a [Symbol], query: &str) -> Vec<&'a Symbol> {
    let q = query.trim().to_lowercase();
    let mut found: Vec<(usize, &Symbol)> = symbols
        .iter()
        .filter_map(|s| {
            let name = s.name.to_lowercase();
            if q.is_empty() {
                Some((1, s))
            } else if name.starts_with(&q) {
                Some((0, s))
            } else {
                name.contains(&q).then_some((1, s))
            }
        })
        .collect();
    if !q.is_empty() {
        found.sort_by_key(|(rank, s)| (*rank, s.line));
    }
    found.into_iter().map(|(_, s)| s).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(file: &str, text: &str) -> Vec<(String, &'static str, usize, usize)> {
        of(file, text)
            .into_iter()
            .map(|s| (s.name, s.kind, s.line, s.depth))
            .collect()
    }

    #[test]
    fn rust_definitions_and_their_depth() {
        let text = "use x;\npub struct Page {\n}\nimpl<T> Page for Thing<T> {\n    pub(crate) async fn open(&self) {}\n    const fn size() {}\n}\npub(in crate::a) fn far() {}\nconst MAX: usize = 3;\n";
        assert_eq!(
            names("a.rs", text),
            [
                ("Page".into(), "struct", 1, 0),
                ("Page for Thing<T>".into(), "impl", 3, 0),
                ("open".into(), "fn", 4, 1),
                ("size".into(), "fn", 5, 1),
                ("far".into(), "fn", 7, 0),
                ("MAX".into(), "const", 8, 0),
            ]
        );
    }

    #[test]
    fn other_languages_and_markdown() {
        assert_eq!(
            names(
                "a.py",
                "class A:\n    def run(self):\n        pass\nasync def go():\n"
            ),
            [
                ("A".into(), "class", 0, 0),
                ("run".into(), "def", 1, 1),
                ("go".into(), "def", 3, 0)
            ]
        );
        assert_eq!(
            names(
                "a.ts",
                "export default function main() {}\nexport class View {}\n"
            ),
            [
                ("main".into(), "function", 0, 0),
                ("View".into(), "class", 1, 0)
            ]
        );
        assert_eq!(
            names("a.go", "func (s *Server) Serve() {}\ntype T struct{}\n"),
            [("Serve".into(), "func", 0, 0), ("T".into(), "type", 1, 0)]
        );
        assert_eq!(
            names("a.md", "# Title\n```\n# not\n```\n## Part ##\n"),
            [("Title".into(), "#", 0, 0), ("Part".into(), "#", 4, 1)]
        );
        assert!(names("a.txt", "fn x() {}").is_empty());
    }

    #[test]
    fn a_line_is_in_the_symbol_above_it_and_a_query_finds_starts_first() {
        let s = of("a.rs", "fn open() {}\n\nfn reopen() {}\nfn close() {}\n");
        assert_eq!(at(&s, 1).map(|s| s.name.as_str()), Some("open"));
        assert_eq!(at(&s, 3).map(|s| s.name.as_str()), Some("close"));
        let found: Vec<_> = matching(&s, "OPEN")
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(found, ["open", "reopen"]);
    }
}
