//! Syntax colouring for the code editor ([`super::code`]): comments, strings, numbers and
//! keywords, line by line, from the file's name. Lexical only, no grammar:
//! enough to read code by, cheap enough to rescan on every edit. What a line
//! starts inside (a block comment, a string spanning lines) is carried from
//! the line before.

use std::ops::Range;

/// What a file's name says its type is, lower case: its extension, or
/// `dockerfile` and `env` for the files named so.
pub fn extension(name: &str) -> String {
    let lower = name.to_lowercase();
    if lower == "dockerfile" || lower.starts_with("dockerfile.") {
        return "dockerfile".into();
    }
    if lower.starts_with(".env") {
        return "env".into();
    }
    match lower.rsplit_once('.') {
        Some((_, ext)) => ext.to_string(),
        None => String::new(),
    }
}

/// What a stretch of a line is, for its colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Token {
    Comment,
    String,
    Number,
    Keyword,
    /// A link in prose (Markdown): drawn as the app draws links.
    Link,
}

/// What a line starts inside.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Carry {
    #[default]
    Nothing,
    /// The language's block at this index, still open.
    Block(usize),
    /// A string opened by this quote, still open.
    Quote(char),
}

/// A language as far as colouring goes.
pub struct Lang {
    /// What starts a comment to the line's end.
    line: &'static [&'static str],
    /// Delimited stretches that may span lines: block comments, and strings
    /// such as Python's triple quotes.
    blocks: &'static [(&'static str, &'static str, Token)],
    /// Quotes that open a string; a backslash escapes.
    quotes: &'static str,
    /// Of those, the ones whose strings run on past a line's end. Any other
    /// left open at the line's end is not a string (a stray apostrophe).
    spanning: &'static str,
    keywords: &'static [&'static str],
    /// Keywords match in any case (SQL).
    caseless: bool,
}

impl Lang {
    /// What starts a line comment, when the language has one.
    pub fn comment(&self) -> Option<&'static str> {
        self.line.first().copied()
    }
}

/// Larger text is shown plain: a rescan per keystroke stays well under a
/// frame up to here.
pub const MAX_BYTES: usize = 1 << 20;

const C_BLOCK: &[(&str, &str, Token)] = &[("/*", "*/", Token::Comment)];
const NONE: &[&str] = &[];

const RUST: Lang = Lang {
    line: &["//"],
    blocks: C_BLOCK,
    quotes: "\"",
    spanning: "\"",
    keywords: &[
        "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum",
        "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move",
        "mut", "pub", "ref", "return", "Self", "self", "static", "struct", "super", "trait",
        "true", "type", "unsafe", "use", "where", "while",
    ],
    caseless: false,
};

const GO: Lang = Lang {
    line: &["//"],
    blocks: C_BLOCK,
    quotes: "\"'`",
    spanning: "`",
    keywords: &[
        "break",
        "case",
        "chan",
        "const",
        "continue",
        "default",
        "defer",
        "else",
        "fallthrough",
        "false",
        "for",
        "func",
        "go",
        "goto",
        "if",
        "import",
        "interface",
        "map",
        "nil",
        "package",
        "range",
        "return",
        "select",
        "struct",
        "switch",
        "true",
        "type",
        "var",
    ],
    caseless: false,
};

const SCRIPT: Lang = Lang {
    line: &["//"],
    blocks: C_BLOCK,
    quotes: "\"'`",
    spanning: "`",
    keywords: &[
        "as",
        "async",
        "await",
        "break",
        "case",
        "catch",
        "class",
        "const",
        "continue",
        "default",
        "delete",
        "do",
        "else",
        "enum",
        "export",
        "extends",
        "false",
        "finally",
        "for",
        "from",
        "function",
        "if",
        "implements",
        "import",
        "in",
        "instanceof",
        "interface",
        "let",
        "new",
        "null",
        "of",
        "private",
        "protected",
        "public",
        "readonly",
        "return",
        "static",
        "super",
        "switch",
        "this",
        "throw",
        "true",
        "try",
        "type",
        "typeof",
        "undefined",
        "var",
        "void",
        "while",
        "yield",
    ],
    caseless: false,
};

/// C, C++, C#, Java, Kotlin, Swift, Dart, Scala and kin.
const C_FAMILY: Lang = Lang {
    line: &["//"],
    blocks: C_BLOCK,
    quotes: "\"'",
    spanning: "",
    keywords: &[
        "abstract",
        "auto",
        "bool",
        "break",
        "case",
        "catch",
        "char",
        "class",
        "const",
        "continue",
        "default",
        "delete",
        "do",
        "double",
        "else",
        "enum",
        "extends",
        "extern",
        "false",
        "final",
        "float",
        "for",
        "fun",
        "func",
        "goto",
        "if",
        "implements",
        "import",
        "include",
        "int",
        "interface",
        "let",
        "long",
        "namespace",
        "new",
        "null",
        "nullptr",
        "override",
        "package",
        "private",
        "protected",
        "public",
        "return",
        "short",
        "signed",
        "sizeof",
        "static",
        "struct",
        "switch",
        "template",
        "this",
        "throw",
        "throws",
        "true",
        "try",
        "typedef",
        "typename",
        "union",
        "unsigned",
        "using",
        "val",
        "var",
        "virtual",
        "void",
        "volatile",
        "while",
    ],
    caseless: false,
};

const CSS: Lang = Lang {
    line: NONE,
    blocks: C_BLOCK,
    quotes: "\"'",
    spanning: "",
    keywords: &["important", "inherit", "initial", "none", "unset"],
    caseless: false,
};

/// SCSS and Less take `//` comments where CSS does not.
const CSS_PRE: Lang = Lang {
    line: &["//"],
    ..CSS
};

const PYTHON: Lang = Lang {
    line: &["#"],
    blocks: &[
        ("\"\"\"", "\"\"\"", Token::String),
        ("'''", "'''", Token::String),
    ],
    quotes: "\"'",
    spanning: "",
    keywords: &[
        "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class",
        "continue", "def", "del", "elif", "else", "except", "finally", "for", "from", "global",
        "if", "import", "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return",
        "self", "try", "while", "with", "yield",
    ],
    caseless: false,
};

const SHELL: Lang = Lang {
    line: &["#"],
    blocks: &[],
    quotes: "\"'",
    spanning: "\"'",
    keywords: &[
        "case", "do", "done", "elif", "else", "esac", "export", "fi", "for", "function", "if",
        "in", "local", "return", "then", "until", "while",
    ],
    caseless: false,
};

const RUBY: Lang = Lang {
    line: &["#"],
    blocks: &[],
    quotes: "\"'",
    spanning: "",
    keywords: &[
        "begin", "class", "def", "do", "else", "elsif", "end", "ensure", "false", "if", "module",
        "nil", "require", "rescue", "return", "self", "then", "true", "unless", "until", "when",
        "while", "yield",
    ],
    caseless: false,
};

/// Settings files whose comments start with `#`: TOML, YAML, and the rest.
const HASH: Lang = Lang {
    line: &["#"],
    blocks: &[],
    quotes: "\"'",
    spanning: "",
    keywords: &["false", "no", "null", "off", "on", "true", "yes"],
    caseless: false,
};

const INI: Lang = Lang {
    line: &["#", ";"],
    blocks: &[],
    quotes: "\"",
    spanning: "",
    keywords: &["false", "no", "off", "on", "true", "yes"],
    caseless: false,
};

const SQL: Lang = Lang {
    line: &["--"],
    blocks: C_BLOCK,
    quotes: "'\"",
    spanning: "'",
    keywords: &[
        "add",
        "alter",
        "and",
        "as",
        "asc",
        "begin",
        "by",
        "case",
        "commit",
        "create",
        "default",
        "delete",
        "desc",
        "distinct",
        "drop",
        "else",
        "end",
        "exists",
        "foreign",
        "from",
        "group",
        "having",
        "if",
        "in",
        "index",
        "insert",
        "into",
        "is",
        "join",
        "key",
        "left",
        "limit",
        "not",
        "null",
        "on",
        "or",
        "order",
        "primary",
        "references",
        "returning",
        "select",
        "set",
        "table",
        "then",
        "union",
        "unique",
        "update",
        "values",
        "when",
        "where",
        "with",
    ],
    caseless: true,
};

const LUA: Lang = Lang {
    line: &["--"],
    blocks: &[("--[[", "]]", Token::Comment), ("[[", "]]", Token::String)],
    quotes: "\"'",
    spanning: "",
    keywords: &[
        "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "if", "in",
        "local", "nil", "not", "or", "repeat", "return", "then", "true", "until", "while",
    ],
    caseless: false,
};

const MARKUP: Lang = Lang {
    line: NONE,
    blocks: &[("<!--", "-->", Token::Comment)],
    quotes: "\"",
    spanning: "",
    keywords: &[],
    caseless: false,
};

const JSON: Lang = Lang {
    line: NONE,
    blocks: &[],
    quotes: "\"",
    spanning: "",
    keywords: &["false", "null", "true"],
    caseless: false,
};

/// JSON with comments (`.jsonc`, `.json5`).
const JSONC: Lang = Lang {
    line: &["//"],
    blocks: C_BLOCK,
    quotes: "\"'",
    spanning: "",
    keywords: &["false", "null", "true"],
    caseless: false,
};

/// Markdown, coloured by its marks rather than its words: headings, list
/// and quote marks, code, links, emphasis marks and front matter's keys.
/// Not chosen by a file's name (the Files app shows Markdown plain); an
/// editor of prose asks for it. A static, so `spans` knows it by address.
pub static MARKDOWN: Lang = Lang {
    line: NONE,
    blocks: &[("```", "```", Token::String), ("~~~", "~~~", Token::String)],
    quotes: "",
    spanning: "",
    keywords: NONE,
    caseless: false,
};

/// A Markdown line's coloured stretches (see `MARKDOWN`).
fn markdown_spans(line: &str, carry: Carry) -> (Vec<(Range<usize>, Token)>, Carry) {
    let whole = |token| (!line.is_empty()).then_some((0..line.len(), token));
    let trimmed = line.trim_start();
    let indent = line.len() - trimmed.len();
    // Inside a fence until its own mark closes it.
    if let Carry::Block(b) = carry {
        let next = if trimmed.starts_with(MARKDOWN.blocks[b].0) {
            Carry::Nothing
        } else {
            carry
        };
        return (whole(Token::String).into_iter().collect(), next);
    }
    if let Some(b) = MARKDOWN
        .blocks
        .iter()
        .position(|(o, ..)| trimmed.starts_with(o))
    {
        return (whole(Token::String).into_iter().collect(), Carry::Block(b));
    }
    let hashes = trimmed.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&hashes) && trimmed[hashes..].chars().next().is_none_or(|c| c == ' ') {
        return (whole(Token::Keyword).into_iter().collect(), Carry::Nothing);
    }
    let bare = trimmed.trim_end();
    if bare.len() >= 3
        && ["-", "*", "_"]
            .iter()
            .any(|m| bare.replace(' ', "").chars().all(|c| c.to_string() == *m))
    {
        return (whole(Token::Comment).into_iter().collect(), Carry::Nothing);
    }
    let mut out = vec![];
    let mut at = indent;
    // A quote's marks, then a list's.
    while line[at..].starts_with('>') {
        let end = at + 1 + usize::from(line[at + 1..].starts_with(' '));
        out.push((at..end, Token::Comment));
        at = end;
    }
    let rest = &line[at..];
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    let marker = if rest.starts_with("- ") || rest.starts_with("* ") || rest.starts_with("+ ") {
        Some(2)
    } else if digits > 0 && (rest[digits..].starts_with(". ") || rest[digits..].starts_with(") ")) {
        Some(digits + 2)
    } else {
        None
    };
    if let Some(len) = marker {
        let boxed = ["[ ] ", "[x] ", "[X] "]
            .iter()
            .any(|b| rest[len..].starts_with(b));
        let len = len + if boxed { 4 } else { 0 };
        out.push((at..at + len, Token::Number));
        at += len;
    } else if indent == 0 && out.is_empty() {
        // Front matter's `key:` (lower case, as the brain writes them).
        let key = rest
            .find(|c: char| !(c.is_ascii_lowercase() || c == '_' || c == '-'))
            .unwrap_or(rest.len());
        if key > 0
            && rest[key..].starts_with(':')
            && rest[key + 1..].chars().next().is_none_or(|c| c == ' ')
        {
            out.push((0..key + 1, Token::Keyword));
            at = key + 1;
        }
    }
    while at < line.len() {
        let rest = &line[at..];
        let close = |open: usize, end: &str| rest[open..].find(end).map(|k| open + k + end.len());
        let found = if rest.starts_with('`') {
            close(1, "`").map(|n| (n, Token::String))
        } else if rest.starts_with("[[") {
            close(2, "]]").map(|n| (n, Token::Link))
        } else if rest.starts_with('[') {
            // `[words](address)`: the words as a link, the address dim.
            close(1, "](").and_then(|mid| {
                close(mid, ")").map(|end| {
                    out.push((at..at + mid - 1, Token::Link));
                    out.push((at + mid - 1..at + end, Token::Comment));
                    (end, Token::Comment)
                })
            })
        } else if rest.starts_with("**") || rest.starts_with("__") {
            Some((2, Token::Comment))
        } else {
            None
        };
        match found {
            Some((n, token)) => {
                if !out.last().is_some_and(|(r, _)| r.end == at + n) {
                    out.push((at..at + n, token));
                }
                at += n;
            }
            None => at += rest.chars().next().map_or(1, char::len_utf8),
        }
    }
    (out, Carry::Nothing)
}

/// The language a file's name says, if it is one coloured.
pub fn lang(name: &str) -> Option<&'static Lang> {
    let lower = name.to_lowercase();
    if matches!(lower.as_str(), "makefile" | "justfile" | "brewfile") {
        return Some(&HASH);
    }
    if matches!(lower.as_str(), "gemfile" | "rakefile" | "vagrantfile") {
        return Some(&RUBY);
    }
    Some(match extension(name).as_str() {
        "rs" => &RUST,
        "go" => &GO,
        "js" | "jsx" | "mjs" | "cjs" | "ts" | "tsx" | "mts" | "cts" | "vue" | "svelte" => &SCRIPT,
        "c" | "h" | "cc" | "cpp" | "hpp" | "cs" | "java" | "kt" | "kts" | "swift" | "dart"
        | "scala" | "groovy" | "gradle" | "php" | "proto" | "m" => &C_FAMILY,
        "css" => &CSS,
        "scss" | "less" | "sass" => &CSS_PRE,
        "py" => &PYTHON,
        "sh" | "bash" | "zsh" | "fish" | "env" => &SHELL,
        "rb" => &RUBY,
        "toml" | "yaml" | "yml" | "conf" | "cfg" | "dockerfile" | "cmake" | "r" | "pl" | "ex"
        | "exs" | "nix" | "tf" | "hcl" | "tfvars" | "ps1" | "gitignore" | "dockerignore"
        | "gitattributes" | "editorconfig" | "properties" => &HASH,
        "ini" | "gitconfig" | "gitmodules" | "service" | "socket" | "timer" | "mount"
        | "target" | "path" | "desktop" | "npmrc" => &INI,
        "sql" => &SQL,
        "lua" => &LUA,
        "html" | "htm" | "xml" | "svg" => &MARKUP,
        "json" | "ndjson" | "jsonl" | "geojson" | "webmanifest" | "har" | "ipynb" => &JSON,
        "jsonc" | "json5" => &JSONC,
        _ => return None,
    })
}

fn ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Where the string opened by `quote` ends in `line[from..]`: just past its
/// closing quote, or `None` when it runs past the line's end.
fn string_end(line: &str, from: usize, quote: char) -> Option<usize> {
    let mut escaped = false;
    for (k, c) in line[from..].char_indices() {
        match c {
            _ if escaped => escaped = false,
            '\\' => escaped = true,
            c if c == quote => return Some(from + k + c.len_utf8()),
            _ => {}
        }
    }
    None
}

/// The coloured stretches of `line`, which starts inside `carry`, and what
/// the next line starts inside. Stretches not listed are plain.
pub fn spans(lang: &Lang, line: &str, carry: Carry) -> (Vec<(Range<usize>, Token)>, Carry) {
    if std::ptr::eq(lang, &MARKDOWN) {
        return markdown_spans(line, carry);
    }
    let mut out = vec![];
    let mut at = 0;
    // Finish what the line starts inside.
    match carry {
        Carry::Nothing => {}
        Carry::Block(b) => {
            let (_, close, token) = lang.blocks[b];
            match line.find(close) {
                Some(k) => {
                    at = k + close.len();
                    out.push((0..at, token));
                }
                None => {
                    if !line.is_empty() {
                        out.push((0..line.len(), token));
                    }
                    return (out, carry);
                }
            }
        }
        Carry::Quote(q) => match string_end(line, 0, q) {
            Some(end) => {
                at = end;
                out.push((0..end, Token::String));
            }
            None => {
                if !line.is_empty() {
                    out.push((0..line.len(), Token::String));
                }
                return (out, carry);
            }
        },
    }
    while at < line.len() {
        let rest = &line[at..];
        let c = rest.chars().next().expect("at is inside the line");
        if lang.line.iter().any(|l| rest.starts_with(l)) {
            out.push((at..line.len(), Token::Comment));
            return (out, Carry::Nothing);
        }
        if let Some((b, (open, close, token))) = lang
            .blocks
            .iter()
            .enumerate()
            .find(|(_, (open, ..))| rest.starts_with(open))
        {
            let body = at + open.len();
            match line[body..].find(close) {
                Some(k) => {
                    let end = body + k + close.len();
                    out.push((at..end, *token));
                    at = end;
                    continue;
                }
                None => {
                    out.push((at..line.len(), *token));
                    return (out, Carry::Block(b));
                }
            }
        }
        if lang.quotes.contains(c) {
            match string_end(line, at + c.len_utf8(), c) {
                Some(end) => {
                    out.push((at..end, Token::String));
                    at = end;
                    continue;
                }
                None if lang.spanning.contains(c) => {
                    out.push((at..line.len(), Token::String));
                    return (out, Carry::Quote(c));
                }
                None => {}
            }
        }
        let after_ident = line[..at].chars().next_back().is_some_and(ident_char);
        if !after_ident && c.is_ascii_digit() {
            let len = rest
                .find(|c: char| !(ident_char(c) || c == '.'))
                .unwrap_or(rest.len());
            out.push((at..at + len, Token::Number));
            at += len;
            continue;
        }
        if !after_ident && ident_char(c) {
            let len = rest.find(|c: char| !ident_char(c)).unwrap_or(rest.len());
            let word = &rest[..len];
            let keyword = if lang.caseless {
                lang.keywords.iter().any(|k| k.eq_ignore_ascii_case(word))
            } else {
                lang.keywords.contains(&word)
            };
            if keyword {
                out.push((at..at + len, Token::Keyword));
            }
            at += len;
            continue;
        }
        at += c.len_utf8();
    }
    (out, Carry::Nothing)
}

/// What each line of `lines` starts inside.
pub fn carries<'a>(lang: &Lang, lines: impl Iterator<Item = &'a str>) -> Vec<Carry> {
    let mut carry = Carry::Nothing;
    lines
        .map(|line| {
            let at = carry;
            carry = spans(lang, line, carry).1;
            at
        })
        .collect()
}

/// The language a Markdown fence names (```` ```rust ````), by its word or
/// the extension it stands for.
pub fn fence(word: &str) -> Option<&'static Lang> {
    let word = word.trim().to_lowercase();
    let ext = match word.as_str() {
        "" => return None,
        "rust" => "rs",
        "python" | "python3" => "py",
        "shell" | "console" | "shellscript" => "sh",
        "javascript" | "typescript" => "js",
        "golang" => "go",
        "c++" | "csharp" | "kotlin" => "cpp",
        "ruby" => "rb",
        "yml" => "yaml",
        other => other,
    };
    lang(&format!("x.{ext}"))
}

/// The coloured stretches of a whole text, as byte ranges into it.
pub fn highlight(lang: &Lang, text: &str) -> Vec<(Range<usize>, Token)> {
    let mut out = vec![];
    let mut carry = Carry::Nothing;
    let mut at = 0;
    for line in text.split('\n') {
        let (found, next) = spans(lang, line, carry);
        out.extend(
            found
                .into_iter()
                .map(|(r, t)| (at + r.start..at + r.end, t)),
        );
        carry = next;
        at += line.len() + 1;
    }
    out
}

/// The colour a token is drawn in.
pub fn colour(t: &crate::theme::Theme, token: Token) -> gpui::Hsla {
    match token {
        Token::Comment => t.syntax.comment,
        Token::String => t.syntax.string,
        Token::Number => t.syntax.number,
        Token::Keyword => t.syntax.keyword,
        Token::Link => t.accent,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fence_names_its_language_by_word_or_extension() {
        assert!(fence("rust").is_some());
        assert!(fence("rs").is_some());
        assert!(fence("bash").is_some());
        assert!(fence("").is_none());
        assert!(fence("text").is_none());
        let text = "let a = 1; // one\n\"two\"";
        let found: Vec<_> = highlight(fence("rust").unwrap(), text)
            .into_iter()
            .map(|(r, t)| (&text[r], t))
            .collect();
        assert!(found.contains(&("// one", Token::Comment)));
        assert!(found.contains(&("\"two\"", Token::String)));
    }

    fn tokens<'a>(lang: &Lang, line: &'a str) -> Vec<(&'a str, Token)> {
        spans(lang, line, Carry::Nothing)
            .0
            .into_iter()
            .map(|(r, t)| (&line[r], t))
            .collect()
    }

    #[test]
    fn markdown_is_coloured_by_its_marks() {
        let md = &MARKDOWN;
        assert_eq!(
            tokens(md, "## Where it goes"),
            [("## Where it goes", Token::Keyword)]
        );
        assert_eq!(tokens(md, "#hashtag"), []);
        assert_eq!(
            tokens(md, "- [ ] see [[sky-map]] and `code`"),
            [
                ("- [ ] ", Token::Number),
                ("[[sky-map]]", Token::Link),
                ("`code`", Token::String)
            ]
        );
        assert_eq!(
            tokens(md, "12. a [site](http://x) **bold**"),
            [
                ("12. ", Token::Number),
                ("[site]", Token::Link),
                ("(http://x)", Token::Comment),
                ("**", Token::Comment),
                ("**", Token::Comment)
            ]
        );
        assert_eq!(
            tokens(md, "summary: \"a: b\""),
            [("summary:", Token::Keyword)]
        );
        assert_eq!(tokens(md, "Note that: prose"), []);
        assert_eq!(
            tokens(md, "> - quoted"),
            [("> ", Token::Comment), ("- ", Token::Number)]
        );
        assert_eq!(tokens(md, "---"), [("---", Token::Comment)]);
        // A fence holds its lines until it closes; its words are not marks.
        let text = "```rust\n# not a heading\n```\n# Heading";
        let found: Vec<_> = highlight(md, text)
            .into_iter()
            .map(|(r, t)| (&text[r], t))
            .collect();
        assert_eq!(
            found,
            [
                ("```rust", Token::String),
                ("# not a heading", Token::String),
                ("```", Token::String),
                ("# Heading", Token::Keyword)
            ]
        );
        // Other languages are unchanged by it.
        assert!(lang("notes.md").is_none());
    }

    /// A link in a page's source is drawn as the app draws links, in the
    /// accent, which reads apart from the text in every theme; the string
    /// colour does not (github-light's is a navy near its text).
    #[test]
    fn links_are_drawn_in_the_accent() {
        for d in crate::theme::Direction::ALL {
            let t = crate::theme::Theme::new(d);
            assert_eq!(colour(&t, Token::Link), t.accent, "{}", d.label());
        }
    }

    #[test]
    fn languages_come_from_the_name() {
        // Constants have no one address, so a language is told by its words.
        let is = |name: &str, keywords: &[&str]| lang(name).unwrap().keywords == keywords;
        assert!(is("main.rs", RUST.keywords));
        assert!(is("App.TSX", SCRIPT.keywords));
        assert!(is("Dockerfile", HASH.keywords));
        assert!(is("Makefile", HASH.keywords));
        assert!(is(".env.local", SHELL.keywords));
        assert!(lang("notes.txt").is_none());
        assert!(lang("README").is_none());
    }

    #[test]
    fn a_line_has_its_keywords_strings_numbers_and_comment() {
        assert_eq!(
            tokens(&RUST, r#"let n = 0x1f; // "not a string""#),
            vec![
                ("let", Token::Keyword),
                ("0x1f", Token::Number),
                (r#"// "not a string""#, Token::Comment),
            ]
        );
        assert_eq!(
            tokens(&PYTHON, r#"print("a \" b", 'c')  # done"#),
            vec![
                (r#""a \" b""#, Token::String),
                ("'c'", Token::String),
                ("# done", Token::Comment),
            ]
        );
    }

    #[test]
    fn words_containing_keywords_or_digits_are_plain() {
        assert!(tokens(&RUST, "letter x1 format_if").is_empty());
    }

    #[test]
    fn a_stray_apostrophe_is_not_a_string_but_a_spanning_quote_is() {
        assert_eq!(tokens(&HASH, "name: it's 3"), vec![("3", Token::Number)]);
        let (s, carry) = spans(&SCRIPT, "const t = `one", Carry::Nothing);
        assert_eq!(carry, Carry::Quote('`'));
        assert_eq!(s.last().unwrap().1, Token::String);
        let (s, carry) = spans(&SCRIPT, "two` + 1", carry);
        assert_eq!(carry, Carry::Nothing);
        assert_eq!(s, vec![(0..4, Token::String), (7..8, Token::Number)]);
    }

    #[test]
    fn block_comments_and_triple_quotes_carry_across_lines() {
        let text = "a /* one\ntwo\nthree */ b\nc";
        assert_eq!(
            carries(&RUST, text.lines()),
            vec![
                Carry::Nothing,
                Carry::Block(0),
                Carry::Block(0),
                Carry::Nothing
            ]
        );
        let (s, _) = spans(&RUST, "three */ fn", Carry::Block(0));
        assert_eq!(s, vec![(0..8, Token::Comment), (9..11, Token::Keyword)]);
        let doc = "def f():\n    \"\"\"Doc\n    more\"\"\"\n    return 1";
        assert_eq!(
            carries(&PYTHON, doc.lines()),
            vec![
                Carry::Nothing,
                Carry::Nothing,
                Carry::Block(0),
                Carry::Nothing
            ]
        );
    }

    #[test]
    fn sql_keywords_match_in_any_case() {
        assert_eq!(
            tokens(&SQL, "Select id FROM t -- all"),
            vec![
                ("Select", Token::Keyword),
                ("FROM", Token::Keyword),
                ("-- all", Token::Comment),
            ]
        );
    }
}
