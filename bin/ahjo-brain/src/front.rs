//! A page's header (its front matter) apart from its words, and the fields
//! a note's header is given for the writer.
//!
//! Writing, the header is out of the way: the writer sees the words, and
//! the header is put back on save exactly as it was read, but for the
//! fields filled in for him. A note caught in the app is filed in the
//! brain's inbox (`knowledge/_inbox/`, Context Citadel's place for what
//! has no place yet), named by its first line, its header written for it:
//! `kind: note`, a one-line `summary` from its first words, `origin:
//! ahjo-brain` and the day it was `updated`.

/// Where notes are caught: the brain's inbox, which agents file into the
/// brain.
pub const INBOX: &str = "knowledge/_inbox";
/// Who wrote a note caught here, as its header says: the person at the app.
pub const ORIGIN: &str = "ahjo-brain";
/// What a new note holds before anything is written: its title's mark.
pub const NOTE_START: &str = "# ";

/// A page's header: the line that opens it, its fields as the writer sees
/// them (without the last line's end), and the line that closes it. A page
/// without one has all three empty.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Head {
    pub open: String,
    pub fields: String,
    pub close: String,
}

/// A page's text as its header and its words.
pub fn split(text: &str) -> (Head, &str) {
    let bom = if text.starts_with('\u{feff}') {
        "\u{feff}"
    } else {
        ""
    };
    let rest = &text[bom.len()..];
    let opener = if rest.starts_with("---\n") {
        "---\n"
    } else if rest.starts_with("---\r\n") {
        "---\r\n"
    } else {
        return (Head::default(), text);
    };
    let inner = &rest[opener.len()..];
    let mut at = 0;
    for line in inner.split_inclusive('\n') {
        if line.trim_end() == "---" {
            let fields = &inner[..at];
            let head = Head {
                open: format!("{bom}{opener}"),
                fields: fields.strip_suffix('\n').unwrap_or(fields).to_string(),
                close: line.to_string(),
            };
            return (head, &inner[at + line.len()..]);
        }
        at += line.len();
    }
    (Head::default(), text)
}

/// The page again from its header and its words: byte for byte what
/// [`split`] took apart when neither changed.
pub fn join(head: &Head, body: &str) -> String {
    if head.open.is_empty() && head.fields.trim().is_empty() {
        return body.to_string();
    }
    let (open, close) = if head.open.is_empty() {
        ("---\n", "---\n")
    } else {
        (head.open.as_str(), head.close.as_str())
    };
    let end = if head.fields.is_empty() { "" } else { "\n" };
    format!("{open}{}{end}{close}{body}", head.fields)
}

/// A field's value as written, its quotes taken off; none when the header
/// does not have it.
pub fn field<'a>(fields: &'a str, key: &str) -> Option<&'a str> {
    fields.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        (k == key).then(|| {
            let v = v.trim();
            v.strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .unwrap_or(v)
        })
    })
}

/// The fields with `key` set to `value`, in its own line where it was or
/// after the others.
pub fn set_field(fields: &str, key: &str, value: &str) -> String {
    let line = format!("{key}: {}", yaml_value(value));
    let mut out: Vec<String> = vec![];
    let mut done = false;
    for l in fields.lines() {
        if !done && l.split_once(':').is_some_and(|(k, _)| k == key) {
            out.push(line.clone());
            done = true;
        } else {
            out.push(l.to_string());
        }
    }
    if !done {
        out.push(line);
    }
    out.join("\n")
}

/// A value as the brain writes one (Citadel's `frontmatter`): a date or a
/// plain word bare, anything else quoted.
fn yaml_value(value: &str) -> String {
    let date = value.len() == 10
        && value.chars().enumerate().all(|(i, c)| {
            if i == 4 || i == 7 {
                c == '-'
            } else {
                c.is_ascii_digit()
            }
        });
    let word = !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if date || word {
        value.to_string()
    } else {
        serde_json::to_string(&value.split_whitespace().collect::<Vec<_>>().join(" "))
            .unwrap_or_default()
    }
}

/// A note's title: its first line with words, without a heading's marks.
pub fn title(body: &str) -> String {
    body.lines()
        .map(|l| l.trim_start_matches('#').trim())
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .to_string()
}

/// A note's summary: the first sentence after its title, else its title,
/// on one line and at most 200 letters, as Citadel keeps one. A list's
/// items read as one sentence, `a; b; c`.
pub fn summary(body: &str) -> String {
    let mut lines = body.lines().filter(|l| !l.trim().is_empty());
    let title = lines
        .next()
        .map(|l| l.trim_start_matches('#').trim())
        .unwrap_or("");
    let mut words = String::new();
    for line in lines.filter(|l| !l.trim_start().starts_with('#')) {
        let bare = line.trim_start().trim_start_matches('>').trim_start();
        // A list item, `- `, `* `, `+ ` or `1. `, and a task's box.
        let digits = bare.len() - bare.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        let marker = match bare.get(..2) {
            Some("- " | "* " | "+ ") => 2,
            _ if digits > 0 && bare[digits..].starts_with(". ") => digits + 2,
            _ => 0,
        };
        let item = marker > 0;
        let text = bare[marker..]
            .trim_start_matches("[ ] ")
            .trim_start_matches("[x] ")
            .trim();
        if text.is_empty() {
            continue;
        }
        if !words.is_empty() {
            let ended = words.ends_with(['.', '!', '?', ':', ';', ',']);
            words.push_str(if item && !ended { "; " } else { " " });
        }
        words.push_str(text);
    }
    let first = words
        .split_inclusive(['.', '!', '?'])
        .next()
        .unwrap_or("")
        .trim();
    let said = if first.is_empty() { title } else { first };
    let said: String = said.split_whitespace().collect::<Vec<_>>().join(" ");
    match said.char_indices().nth(199) {
        // Cut at the last word that fits.
        Some((at, _)) => {
            let cut = if said[at..].starts_with(' ') {
                at
            } else {
                said[..at].rfind(' ').unwrap_or(at)
            };
            format!("{}…", said[..cut].trim_end_matches([',', ';', ':']))
        }
        None => said,
    }
}

/// A title as a file name: lower case words joined by hyphens, as
/// Citadel's `slugify` makes them, at most sixty letters.
pub fn slug(title: &str) -> String {
    let mut out = String::new();
    for c in title.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let mut out: String = out.chars().take(60).collect();
    while out.ends_with('-') {
        out.pop();
    }
    out
}

/// Where a note of `title` caught on `day` is filed, before a number is
/// added for a name taken: `knowledge/_inbox/<day>-<title>`, without `.md`.
pub fn note_stem(day: &str, title: &str) -> String {
    match slug(title) {
        s if s.is_empty() => format!("{INBOX}/{day}-note"),
        s => format!("{INBOX}/{day}-{s}"),
    }
}

/// The day's note: one page a day in the inbox.
pub fn daily_path(day: &str) -> String {
    format!("{INBOX}/{day}-daily.md")
}

/// The day's note as it begins: its heading the day, then a list to jot in.
pub fn daily_start(heading: &str) -> String {
    format!("# {heading}\n\n- ")
}

/// A note's header before its first save.
pub fn note_head(day: &str) -> Head {
    Head {
        open: "---\n".into(),
        fields: format!("kind: note\nsummary:\norigin: {ORIGIN}\nupdated: {day}"),
        close: "---\n".into(),
    }
}

/// Whether a page is a note waiting in the inbox for an agent to file it,
/// caught here or anywhere else.
pub fn waiting(path: &str) -> bool {
    path.starts_with(INBOX) && path[INBOX.len()..].starts_with('/')
}

/// A note's header made right to save on `day`: a kind and an origin when
/// it has none, the day it was updated, and a summary from its words while
/// the summary is empty or still the one filled before (`filled`), so a
/// summary the writer wrote is his. Answers the fields and the summary
/// filled, if one was.
pub fn fill_note(
    fields: &str,
    body: &str,
    day: &str,
    filled: Option<&str>,
) -> (String, Option<String>) {
    let mut f = fields.to_string();
    if field(&f, "kind").is_none_or(str::is_empty) {
        f = set_field(&f, "kind", "note");
    }
    if field(&f, "origin").is_none() {
        f = set_field(&f, "origin", ORIGIN);
    }
    let now = field(&f, "summary").unwrap_or("").to_string();
    let mut auto = None;
    if now.is_empty() || filled == Some(now.as_str()) {
        let s = summary(body);
        if !s.is_empty() {
            f = set_field(&f, "summary", &s);
            auto = Some(s);
        }
    }
    (set_field(&f, "updated", day), auto)
}

/// A page's header made right to save on `day`: its `updated` day, when it
/// keeps one; nothing else of it is touched.
pub fn fill_page(fields: &str, day: &str) -> String {
    match field(fields, "updated") {
        Some(d) if d != day => set_field(fields, "updated", day),
        _ => fields.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_header_comes_back_byte_for_byte() {
        for text in [
            "---\nkind: note\nsummary: \"A, b.\"\nfacets: [x]\n---\n# T\n\nWords.\n",
            "\u{feff}---\r\nkind: note\r\n---\r\n# T\r\n",
            "---\n---\nbody",
            "# No header\n\nWords.",
            "---\nnot closed\n# T\n",
            "",
        ] {
            let (head, body) = split(text);
            assert_eq!(join(&head, body), text, "{text:?}");
        }
        let (head, body) = split("---\nkind: note\nupdated: 2026-11-01\n---\n# T\n");
        assert_eq!(head.fields, "kind: note\nupdated: 2026-11-01");
        assert_eq!(body, "# T\n");
        // A header written into a page without one gets its lines.
        let added = Head {
            fields: "kind: note".into(),
            ..Head::default()
        };
        assert_eq!(join(&added, "# T\n"), "---\nkind: note\n---\n# T\n");
    }

    #[test]
    fn fields_are_read_and_set_in_their_place() {
        let f = "kind: note\nsummary: \"Old.\"\nupdated: 2026-11-01";
        assert_eq!(field(f, "summary"), Some("Old."));
        assert_eq!(field(f, "origin"), None);
        assert_eq!(
            set_field(f, "summary", "New: words, \"quoted\"."),
            "kind: note\nsummary: \"New: words, \\\"quoted\\\".\"\nupdated: 2026-11-01"
        );
        assert_eq!(
            set_field(f, "origin", "ahjo-brain"),
            format!("{f}\norigin: ahjo-brain")
        );
        assert_eq!(
            fill_page(f, "2026-11-20"),
            "kind: note\nsummary: \"Old.\"\nupdated: 2026-11-20"
        );
        assert_eq!(fill_page("kind: note", "2026-11-20"), "kind: note");
    }

    #[test]
    fn a_note_is_named_and_summed_up_from_its_words() {
        let body = "# Fog on the harbour dial\n\nThe glass fogs at dawn. Try a vent.\n";
        assert_eq!(title(body), "Fog on the harbour dial");
        assert_eq!(summary(body), "The glass fogs at dawn.");
        assert_eq!(summary("# Only a title\n"), "Only a title");
        assert_eq!(
            summary("# T\n\n- [ ] Call the sailmaker\n- Ease"),
            "Call the sailmaker; Ease"
        );
        assert_eq!(title("Plain first line\nmore"), "Plain first line");
        assert_eq!(
            note_stem("2026-11-20", "Tide wheel: gearing, again!"),
            "knowledge/_inbox/2026-11-20-tide-wheel-gearing-again"
        );
        assert_eq!(
            note_stem("2026-11-20", "  "),
            "knowledge/_inbox/2026-11-20-note"
        );
        assert_eq!(
            daily_path("2026-11-20"),
            "knowledge/_inbox/2026-11-20-daily.md"
        );
    }

    #[test]
    fn a_notes_header_is_filled_but_what_he_wrote_is_kept() {
        let head = note_head("2026-11-19");
        let body = "# Fog\n\nThe glass fogs at dawn.\n";
        let (f, auto) = fill_note(&head.fields, body, "2026-11-20", None);
        assert_eq!(
            f,
            "kind: note\nsummary: \"The glass fogs at dawn.\"\norigin: ahjo-brain\nupdated: 2026-11-20"
        );
        assert_eq!(auto.as_deref(), Some("The glass fogs at dawn."));
        // The summary filled follows the words; one he wrote stays his.
        let more = "# Fog\n\nThe dial fogs at dawn.\n";
        let (again, _) = fill_note(&f, more, "2026-11-20", auto.as_deref());
        assert_eq!(field(&again, "summary"), Some("The dial fogs at dawn."));
        let his = set_field(&f, "summary", "Mine.");
        let (kept, none) = fill_note(&his, more, "2026-11-20", auto.as_deref());
        assert_eq!(field(&kept, "summary"), Some("Mine."));
        assert_eq!(none, None);
        // A page that came with an origin keeps it.
        let (f, _) = fill_note("kind: note\norigin: chatgpt", body, "2026-11-20", None);
        assert_eq!(field(&f, "origin"), Some("chatgpt"));
        // A day's list reads as one line.
        assert_eq!(
            summary("# Friday\n\n- Bench: vent test\n- Call the chandler\n"),
            "Bench: vent test; Call the chandler"
        );
        assert_eq!(
            summary("# Fog\n\nTo try:\n- a vent\n- a mesh. Then wait.\n"),
            "To try: a vent; a mesh."
        );
        assert_eq!(
            summary("# Plan\n\n1. **Brass** first\n2. [ ] then glass\n"),
            "**Brass** first; then glass"
        );
        // A long one is cut at a word.
        let long = format!("# T\n\n{}", "word ".repeat(60));
        assert_eq!(summary(&long), format!("{}…", ["word"; 40].join(" ")));
        assert!(waiting("knowledge/_inbox/2026-11-20-fog.md"));
        assert!(!waiting("knowledge/_inboxed/x.md"));
        assert!(!waiting("knowledge/garden/fog.md"));
    }
}
