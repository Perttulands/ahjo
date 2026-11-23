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
        let taken = [text_note("d1", 0, 1), text_note("d3", 0, 1)];
        assert_eq!(next_id(&taken), "d2");
        assert_eq!(next_id(&[]), "d1");
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
        let t = text_note("d3", 4, 9);
        assert_eq!(what(&t, Some("The clearing")), "“clear”");
    }

    fn text_note(id: &str, start: usize, end: usize) -> Mark {
        Mark {
            id: id.into(),
            kind: "note".into(),
            start: Some(start),
            end: Some(end),
            ..Default::default()
        }
    }
}
