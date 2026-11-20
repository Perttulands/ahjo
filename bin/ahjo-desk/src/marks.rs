//! Marks as the operator makes them: a pin, a box, an arrow or a line drawn
//! on a picture or a paused clip's frame, a click on a page's element, a
//! passage chosen in the document. Each becomes one [`Mark`] in the store's
//! format, so the web desk shows it and agents read it unchanged. Pure.

use crate::model::Mark;

/// What a press on the surface does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    /// Look: drag moves, the wheel zooms.
    Look,
    Pin,
    Box,
    Arrow,
    Pen,
}

impl Tool {
    pub const DRAW: [Tool; 4] = [Tool::Pin, Tool::Box, Tool::Arrow, Tool::Pen];

    pub fn name(self) -> &'static str {
        match self {
            Tool::Look => "Look",
            Tool::Pin => "Pin",
            Tool::Box => "Box",
            Tool::Arrow => "Arrow",
            Tool::Pen => "Pen",
        }
    }

    /// Its key while the surface has the keyboard.
    pub fn key(self) -> &'static str {
        match self {
            Tool::Look => "V",
            Tool::Pin => "P",
            Tool::Box => "B",
            Tool::Arrow => "A",
            Tool::Pen => "D",
        }
    }

    /// The store's kind for what it draws.
    fn kind(self) -> &'static str {
        match self {
            Tool::Look | Tool::Pin => "pin",
            Tool::Box => "box",
            Tool::Arrow => "arrow",
            Tool::Pen => "pen",
        }
    }
}

/// A free id for a new mark among `marks` (`[A-Za-z0-9_-]`, as the store
/// takes them).
pub fn next_id(marks: &[Mark]) -> String {
    (1..)
        .map(|n| format!("d{n}"))
        .find(|id| !marks.iter().any(|m| &m.id == id))
        .expect("a free id")
}

/// Less than this far apart (as a share of the picture), a box or an arrow
/// was a click: it becomes a pin there.
const CLICK: f64 = 0.008;

/// The mark `tool` drew through `points` (0 to 1 across the media), on a
/// clip's frame when `at` says which (seconds, frame).
pub fn drawn(
    id: String,
    tool: Tool,
    media: &str,
    points: &[[f64; 2]],
    at: Option<(f64, u64)>,
) -> Option<Mark> {
    let first = *points.first()?;
    let last = *points.last()?;
    let far = (first[0] - last[0]).hypot(first[1] - last[1]) >= CLICK;
    let (kind, points) = match tool {
        Tool::Box | Tool::Arrow if far => (tool.kind(), vec![first, last]),
        Tool::Pen if points.len() > 1 && far => ("pen", points.to_vec()),
        _ => ("pin", vec![first]),
    };
    Some(Mark {
        id,
        kind: kind.into(),
        media: Some(media.into()),
        points: points
            .iter()
            .map(|p| [p[0].clamp(0., 1.), p[1].clamp(0., 1.)])
            .collect(),
        time: at.map(|a| (a.0 * 1000.).round() / 1000.),
        frame: at.map(|a| a.1),
        ..Default::default()
    })
}

/// A note on the whole of what is shown (a picture, a clip's frame, a
/// page) rather than on a point of it: in the store's terms a region over
/// all of it, so the web desk and the agents read it unchanged.
pub fn whole(id: String, media: &str, at: Option<(f64, u64)>) -> Mark {
    Mark {
        id,
        kind: "region".into(),
        media: Some(media.into()),
        points: WHOLE.to_vec(),
        time: at.map(|a| (a.0 * 1000.).round() / 1000.),
        frame: at.map(|a| a.1),
        ..Default::default()
    }
}

const WHOLE: [[f64; 2]; 2] = [[0., 0.], [1., 1.]];

/// Whether a mark is a note on the whole of its media.
pub fn is_whole(m: &Mark) -> bool {
    m.kind == "region" && m.points == WHOLE
}

/// A page's element he clicked: where on the page, and which element.
pub fn on_page(id: String, media: &str, selector: &str, quote: &str, x: f64, y: f64) -> Mark {
    Mark {
        id,
        kind: "pin".into(),
        media: Some(media.into()),
        points: vec![[x.clamp(0., 1.), y.clamp(0., 1.)]],
        selector: Some(selector.into()).filter(|s: &String| !s.is_empty()),
        quote: Some(quote.chars().take(300).collect()).filter(|s: &String| !s.is_empty()),
        ..Default::default()
    }
}

/// A note on the document's passage from `start` to `end` (characters).
pub fn on_text(id: String, start: usize, end: usize) -> Mark {
    Mark {
        id,
        kind: "note".into(),
        start: Some(start),
        end: Some(end),
        ..Default::default()
    }
}

/// Whether the mark shows on this media now: a clip's mark on its frame
/// only.
pub fn shows_on(m: &Mark, media: &str, frame: Option<usize>) -> bool {
    m.media.as_deref() == Some(media)
        && match (m.frame, frame) {
            (Some(f), Some(now)) => f as usize == now,
            _ => true,
        }
}

/// `m:ss.s`, as the store prints a clip's time.
pub fn clock(seconds: f64) -> String {
    let s = seconds.max(0.);
    format!("{}:{:04.1}", (s / 60.).floor() as u64, s % 60.)
}

/// What a mark is, in a few words, for the list beside it.
pub fn what(m: &Mark, doc: Option<&str>) -> String {
    if m.on_text() {
        let quote = m.quote.clone().or_else(|| {
            let (s, e) = (m.start?, m.end?);
            Some(doc?.chars().skip(s).take(e.saturating_sub(s)).collect())
        });
        return format!("“{}”", clip(&quote.unwrap_or_default(), 60));
    }
    let mut w = match m.kind.as_str() {
        _ if is_whole(m) => "Note",
        "pin" => "Pin",
        "box" | "region" => "Box",
        "arrow" => "Arrow",
        "pen" | "ink" => "Line",
        "text" => "Note",
        _ => "Mark",
    }
    .to_string();
    if let Some(sel) = &m.selector {
        w = match &m.quote {
            Some(q) => format!("On “{}”", clip(q, 40)),
            None => format!("On {}", clip(sel.rsplit(" > ").next().unwrap_or(sel), 40)),
        };
    }
    if let Some(t) = m.time {
        w.push_str(&format!(" at {}", clock(t)));
    }
    w
}

fn clip(s: &str, n: usize) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= n {
        s
    } else {
        format!("{}…", s.chars().take(n).collect::<String>().trim_end())
    }
}

/// Where the words he selected in the rendered document are in its text,
/// as characters (`start..end`), though the rendering dropped the markup
/// and joined lines: matched with markup and runs of space set aside, and
/// a long passage by its first and last words when the middle differs.
pub fn locate(doc: &str, selected: &str) -> Option<(usize, usize)> {
    let (plain, at) = flatten(doc);
    let (want, _) = flatten(selected);
    let want: Vec<char> = trim(&want);
    if want.is_empty() {
        return None;
    }
    let span = |from: usize, len: usize| (at[from], at[from + len - 1] + 1);
    if let Some(i) = find(&plain, &want, 0) {
        return Some(span(i, want.len()));
    }
    const ENDS: usize = 24;
    if want.len() < 2 * ENDS {
        return None;
    }
    let head = &want[..ENDS];
    let tail = &want[want.len() - ENDS..];
    let i = find(&plain, head, 0)?;
    let j = find(&plain, tail, i + ENDS)?;
    Some((at[i], at[j + ENDS - 1] + 1))
}

/// The text without markup, one space for any run of space, with each
/// kept character's place in the original.
fn flatten(s: &str) -> (Vec<char>, Vec<usize>) {
    let mut out = vec![];
    let mut at = vec![];
    let mut space = false;
    for (i, c) in s.chars().enumerate() {
        if matches!(c, '*' | '_' | '`' | '#' | '>' | '[' | ']') {
            continue;
        }
        if c.is_whitespace() {
            space = true;
            continue;
        }
        if space && !out.is_empty() {
            out.push(' ');
            at.push(i - 1);
        }
        space = false;
        out.push(c);
        at.push(i);
    }
    (out, at)
}

fn trim(s: &[char]) -> Vec<char> {
    let start = s.iter().position(|c| *c != ' ').unwrap_or(s.len());
    let end = s.iter().rposition(|c| *c != ' ').map_or(start, |e| e + 1);
    s[start..end].to_vec()
}

fn find(hay: &[char], needle: &[char], from: usize) -> Option<usize> {
    if needle.len() > hay.len() {
        return None;
    }
    (from..=hay.len() - needle.len()).find(|&i| hay[i..i + needle.len()] == *needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_drag_is_a_pin_and_a_clip_mark_keeps_its_frame() {
        let pin = drawn(
            "d1".into(),
            Tool::Box,
            "m1",
            &[[0.5, 0.5], [0.502, 0.5]],
            None,
        )
        .unwrap();
        assert_eq!((pin.kind.as_str(), pin.points.len()), ("pin", 1));
        let b = drawn(
            "d2".into(),
            Tool::Box,
            "m3",
            &[[0.1, 0.2], [0.4, 1.3]],
            Some((1.0999999, 33)),
        )
        .unwrap();
        assert_eq!(b.kind, "box");
        assert_eq!(
            b.points,
            vec![[0.1, 0.2], [0.4, 1.0]],
            "kept on the picture"
        );
        assert_eq!((b.time, b.frame), (Some(1.1), Some(33)));
        let pen = drawn(
            "d3".into(),
            Tool::Pen,
            "m1",
            &[[0.1, 0.1], [0.2, 0.2], [0.3, 0.1]],
            None,
        )
        .unwrap();
        assert_eq!((pen.kind.as_str(), pen.points.len()), ("pen", 3));
        assert!(shows_on(&b, "m3", Some(33)) && !shows_on(&b, "m3", Some(34)));
        assert!(shows_on(&pin, "m1", None) && !shows_on(&pin, "m2", None));
    }

    #[test]
    fn a_note_covers_the_whole_and_reads_as_a_note() {
        let n = whole("d1".into(), "m2", Some((2.00049, 60)));
        // As the store takes a media mark: a kind it knows and points on it.
        assert_eq!(n.kind, "region");
        assert_eq!(n.points, vec![[0., 0.], [1., 1.]]);
        assert_eq!((n.time, n.frame), (Some(2.0), Some(60)));
        assert!(is_whole(&n) && shows_on(&n, "m2", Some(60)));
        assert_eq!(what(&n, None), "Note at 0:02.0");
        // A box he drew over part of it is not a note.
        let b = drawn("d2".into(), Tool::Box, "m2", &[[0., 0.], [0.5, 1.]], None).unwrap();
        assert!(!is_whole(&b));
        assert_eq!(what(&b, None), "Box");
    }

    #[test]
    fn ids_are_free_and_in_the_stores_alphabet() {
        let taken = [on_text("d1".into(), 0, 1), on_text("d3".into(), 0, 1)];
        assert_eq!(next_id(&taken), "d2");
        assert_eq!(next_id(&[]), "d1");
    }

    #[test]
    fn a_selected_passage_is_found_in_the_markdown_it_was_rendered_from() {
        let doc = "# Clearing\n\nThe clearing is **where the\nwolves** rest at dusk.\n\n- A `fire` burns low\n";
        let (s, e) = locate(doc, "where the wolves rest").unwrap();
        let found: String = doc.chars().skip(s).take(e - s).collect();
        assert_eq!(found, "where the\nwolves** rest");
        let (s, e) = locate(doc, "  A fire burns ").unwrap();
        assert_eq!(
            doc.chars().skip(s).take(e - s).collect::<String>(),
            "A `fire` burns"
        );
        assert_eq!(locate(doc, "not in it"), None);
        assert_eq!(locate(doc, "   "), None);
        // Characters, not bytes, as the store counts them.
        let (s, _) = locate("Ää ö clearing", "clearing").unwrap();
        assert_eq!(s, 5);
    }

    #[test]
    fn a_long_passage_is_found_by_its_ends_when_its_middle_was_drawn_otherwise() {
        let doc = "Start. The old road runs along the lake | a table cell | and ends at the ferry landing by the shed. End.";
        let picked = "The old road runs along the lake\ta table cell\tand ends at the ferry landing by the shed";
        let (s, e) = locate(doc, picked).unwrap();
        let found: String = doc.chars().skip(s).take(e - s).collect();
        assert!(found.starts_with("The old road") && found.ends_with("by the shed"));
    }

    #[test]
    fn a_mark_says_what_and_when_in_a_few_words() {
        let b = drawn(
            "d1".into(),
            Tool::Arrow,
            "m1",
            &[[0.1, 0.1], [0.5, 0.5]],
            Some((61.5, 70)),
        )
        .unwrap();
        assert_eq!(what(&b, None), "Arrow at 1:01.5");
        let p = on_page("d2".into(), "m4", "main > h2", "Choose your path", 0.5, 0.2);
        assert_eq!(what(&p, None), "On “Choose your path”");
        let t = on_text("d3".into(), 4, 9);
        assert_eq!(what(&t, Some("The clearing")), "“clear”");
    }
}
