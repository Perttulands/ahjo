//! An ask as a desk's `/api/asks` returns it (the store's shape in
//! `/srv/desk/store.py`), read leniently: a field the store adds later is
//! ignored, a missing one is empty. Nothing here does I/O.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Media {
    pub id: String,
    pub path: String,
    pub caption: String,
}

/// What a media file is, by its name, as the web desk tells them apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaKind {
    Image,
    /// H.264 in MP4, which the app decodes itself.
    Video,
    /// Another clip (WebM, MOV) the app hands to the system's player.
    OtherVideo,
    /// A page the agent made (a prototype), rendered from the desk.
    Page,
    Other,
}

impl Media {
    pub fn kind(&self) -> MediaKind {
        let p = self.path.to_ascii_lowercase();
        let ext = p.rsplit('.').next().unwrap_or("");
        match ext {
            "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" => MediaKind::Image,
            "mp4" | "m4v" => MediaKind::Video,
            "webm" | "mov" | "ogv" => MediaKind::OtherVideo,
            "html" | "htm" => MediaKind::Page,
            _ => MediaKind::Other,
        }
    }

    pub fn file_name(&self) -> &str {
        self.path.rsplit(['/', '\\']).next().unwrap_or(&self.path)
    }
}

/// One of an ask's options ("option" in the store).
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Choice {
    pub name: String,
    pub why: String,
    pub media: Vec<Media>,
}

/// A question of a grilling round.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Question {
    pub id: String,
    pub title: String,
    pub question: String,
    pub recommend: String,
    pub media: Vec<Media>,
    pub options: Vec<Choice>,
    /// The previous round's question this one follows up.
    pub was: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct GrillAnswer {
    pub pick: Option<String>,
    pub comment: String,
    /// Given by taking the recommendation, not chosen.
    pub rec: bool,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Answer {
    pub verdict: String,
    pub options: Vec<String>,
    pub comment: String,
    pub answers: BTreeMap<String, GrillAnswer>,
    pub at: String,
    /// The pictures he pasted into it.
    pub attachments: Vec<Attached>,
}

/// The check a hands job's Done runs on its `out/`.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Check {
    pub running: bool,
    pub ok: bool,
    pub summary: String,
    pub lines: Vec<CheckLine>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct CheckLine {
    pub ok: bool,
    pub text: String,
}

/// A picture he pasted into an answer, as the desk keeps it.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Attached {
    pub id: String,
    pub path: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Comment {
    pub author: String,
    pub text: String,
    pub at: String,
}

/// A mark the operator made, in the store's one format the web desk and the
/// agents read: on a picture, a clip's frame or a page's element (`media`,
/// `points` from 0 to 1 across it), or on a passage of the document
/// (`start` and `end`, characters into its text). Fields this app does not
/// know (the store's anchors) are kept as they came and sent back.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct Mark {
    pub id: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub points: Vec<[f64; 2]>,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Seconds into the clip, and its frame, when marked on a paused one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame: Option<u64>,
    /// The page's element: a CSS selector, and its words.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quote: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<usize>,
    #[serde(flatten)]
    pub rest: Map<String, Value>,
}

impl Mark {
    /// On the document's text rather than on a picture, clip or page.
    pub fn on_text(&self) -> bool {
        matches!(self.kind.as_str(), "tick" | "strike" | "rewrite" | "note")
    }
}

/// A document the ask is about: read here, its passages marked.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Doc {
    pub name: String,
    /// The page's words, as the desk sends them to read.
    pub text: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Ask {
    pub id: String,
    pub bead: String,
    pub kind: String,
    pub title: String,
    pub question: String,
    pub recommend: String,
    pub project: String,
    pub asked_by: String,
    /// The tmux session the ask was posted from: told when he answers.
    pub session: String,
    pub status: String,
    pub media: Vec<Media>,
    pub options: Vec<Choice>,
    pub questions: Vec<Question>,
    pub doc: Option<Doc>,
    pub folder: Option<String>,
    /// A hands job's folders in CHROTE Files (`in`, `out`) and its guide.
    pub links: BTreeMap<String, String>,
    /// A hands job's check: running, or how it came out.
    pub check: Option<Check>,
    pub answer: Option<Answer>,
    pub comments: Vec<Comment>,
    pub marks: Vec<Mark>,
    pub round: u32,
    pub history: Vec<Ask>,
    pub revision: u64,
    pub created_at: String,
    pub updated_at: String,
}

/// How the app answers an ask. Every one is answered in words first; what
/// differs is what may come with them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Form {
    /// Words and marks, with a pick when it has options.
    Open,
    /// A grilling round: each question picked, written or left.
    Grill,
    /// A job for his hands: Done is checked where the job is.
    Hands,
}

impl Ask {
    pub fn round(&self) -> u32 {
        self.round.max(1)
    }

    pub fn is_open(&self) -> bool {
        self.status == "open"
    }

    pub fn form(&self) -> Form {
        if !self.questions.is_empty() {
            Form::Grill
        } else if self.kind == "hands" {
            Form::Hands
        } else {
            Form::Open
        }
    }

    /// The options' index of the one the agent recommends, if any names one.
    pub fn recommended(&self) -> Option<usize> {
        recommended(&self.options, &self.recommend)
    }

    /// The previous round when it was a grill, which this round's questions
    /// follow up.
    pub fn previous_grill(&self) -> Option<&Ask> {
        self.history.iter().rev().find(|r| !r.questions.is_empty())
    }

    /// The question of the round before that this one follows (named by
    /// `was`, or the same title), and what he answered then.
    pub fn followed_up(&self, q: &Question) -> Option<(u32, &Question, Option<&GrillAnswer>)> {
        let prev = self.previous_grill()?;
        let old = match &q.was {
            Some(was) => prev.questions.iter().find(|o| &o.id == was),
            None => prev
                .questions
                .iter()
                .find(|o| o.title.trim() == q.title.trim()),
        }?;
        let answer = prev.answer.as_ref().and_then(|a| a.answers.get(&old.id));
        Some((prev.round(), old, answer))
    }

    /// Every media item of the ask, its own first, then each option's.
    pub fn all_media(&self) -> Vec<&Media> {
        let mut all: Vec<&Media> = self.media.iter().collect();
        for o in &self.options {
            all.extend(o.media.iter());
        }
        for q in &self.questions {
            all.extend(q.media.iter());
            for o in &q.options {
                all.extend(o.media.iter());
            }
        }
        all
    }
}

const LETTERS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";

/// Whether the name brings its own letter or number ("B: a place",
/// "2. Vessels"), and which.
fn own_label(name: &str) -> Option<(&str, &str)> {
    let mut chars = name.char_indices();
    let (_, first) = chars.next()?;
    let digits = name
        .char_indices()
        .take_while(|(_, c)| c.is_ascii_digit())
        .count();
    let end = if first.is_ascii_uppercase() {
        1
    } else if (1..=2).contains(&digits) {
        digits
    } else {
        return None;
    };
    let rest = &name[end..];
    let sep = rest.chars().next()?;
    if !matches!(sep, ':' | '.' | ')') {
        return None;
    }
    Some((&name[..end], rest[1..].trim_start()))
}

/// The option's letter as the operator sees it: its own when its name
/// brings one, else A, B, C by place.
pub fn letter(options: &[Choice], i: usize) -> String {
    match own_label(&options[i].name) {
        Some((own, _)) => own.to_string(),
        None => LETTERS
            .get(i)
            .map(|b| (*b as char).to_string())
            .unwrap_or_else(|| "?".into()),
    }
}

/// The option's name without the letter it brings.
pub fn label(options: &[Choice], i: usize) -> String {
    let name = options[i].name.as_str();
    match own_label(name) {
        Some((_, rest)) if !rest.is_empty() => rest.to_string(),
        _ => name.to_string(),
    }
}

/// Which option a recommendation names: the whole name, or its start
/// ("Quiet: keeps the floor readable" names "Quiet"), or its letter
/// ("B: ..."), and only when exactly one option fits.
pub fn recommended(options: &[Choice], recommend: &str) -> Option<usize> {
    let r = recommend.trim();
    if r.is_empty() || options.is_empty() {
        return None;
    }
    let fits: Vec<usize> = (0..options.len())
        .filter(|&i| {
            let name = options[i].name.as_str();
            let label = label(options, i);
            names(r, name) || (!label.is_empty() && names(r, &label))
        })
        .collect();
    if let [one] = fits.as_slice() {
        return Some(*one);
    }
    if fits.len() > 1 {
        // The longest name that fits is the one meant ("Only while
        // holding" over "Only").
        return fits.into_iter().max_by_key(|&i| options[i].name.len());
    }
    // "B: because" or "B." names option B.
    let (head, sep) = r.split_at(r.char_indices().nth(1).map_or(r.len(), |(i, _)| i));
    if head.len() == 1 && (sep.is_empty() || sep.starts_with([':', '.', ')', ','])) {
        return (0..options.len()).find(|&i| letter(options, i) == head);
    }
    None
}

/// `r` starts with `name` and the name ends there (not "Quietly" for
/// "Quiet").
fn names(r: &str, name: &str) -> bool {
    let name = name.trim();
    if name.is_empty() || !r.starts_with(name) {
        return false;
    }
    r[name.len()..]
        .chars()
        .next()
        .is_none_or(|c| !c.is_alphanumeric())
}

/// How long an ask takes to answer, roughly, in taps: the queue answers
/// the quick ones first, because every answer frees an agent and clearing
/// the quick ones first frees the most agents soonest.
pub fn effort(ask: &Ask) -> u32 {
    let clips = ask
        .all_media()
        .iter()
        .filter(|m| m.kind() == MediaKind::Video)
        .count() as u32;
    let base = match ask.form() {
        Form::Open => 2 + ask.options.len() as u32 + if ask.doc.is_some() { 6 } else { 0 },
        Form::Grill => 3 * ask.questions.len() as u32,
        // Done where the job is: after everything answered here.
        Form::Hands => 1000,
    };
    base + 2 * clips
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(names: &[&str]) -> Vec<Choice> {
        names
            .iter()
            .map(|n| Choice {
                name: n.to_string(),
                ..Default::default()
            })
            .collect()
    }

    #[test]
    fn a_recommendation_names_its_option_by_name_start_or_letter() {
        let o = opts(&["Quiet", "Bright"]);
        assert_eq!(
            recommended(&o, "Quiet: the cavern is for walking through"),
            Some(0)
        );
        assert_eq!(recommended(&o, "Bright"), Some(1));
        assert_eq!(recommended(&o, "B: it reads from afar"), Some(1));
        assert_eq!(recommended(&o, "Quietly"), None);
        assert_eq!(recommended(&o, ""), None);
        let o = opts(&["Always", "Only while holding a piece", "Never"]);
        assert_eq!(recommended(&o, "Only while holding a piece"), Some(1));
        let o = opts(&["A: a place", "B: a road"]);
        assert_eq!(recommended(&o, "B"), Some(1));
        assert_eq!(recommended(&o, "a road, because"), Some(1));
    }

    #[test]
    fn an_option_that_brings_its_letter_shows_it_once() {
        let o = opts(&["B: a place", "2. Vessels", "Plain"]);
        assert_eq!(letter(&o, 0), "B");
        assert_eq!(label(&o, 0), "a place");
        assert_eq!(letter(&o, 1), "2");
        assert_eq!(label(&o, 1), "Vessels");
        assert_eq!(letter(&o, 2), "C");
        assert_eq!(label(&o, 2), "Plain");
    }

    #[test]
    fn the_store_shape_reads_and_forms_follow_it() {
        let json = r#"[{"id":"AQ1","kind":"look","title":"t","status":"open","media":[{"path":"/w/a.png","id":"m1"}],"round":1,"history":[],"extra":1},
            {"id":"AQ2","kind":"grill","title":"g","status":"open","questions":[{"id":"q1","title":"x","options":[{"name":"A"}]}]},
            {"id":"AQ3","kind":"look","title":"d","status":"open","doc":{"name":"V.md","text":"..."}}]"#;
        let asks: Vec<Ask> = serde_json::from_str(json).unwrap();
        assert_eq!(asks[0].form(), Form::Open);
        assert_eq!(asks[0].media[0].kind(), MediaKind::Image);
        assert_eq!(asks[1].form(), Form::Grill);
        assert_eq!(asks[1].round(), 1);
        assert_eq!(asks[2].form(), Form::Open);
        assert!(effort(&asks[0]) < effort(&asks[1]));
        assert!(effort(&asks[0]) < effort(&asks[2]));
    }

    #[test]
    fn a_mark_goes_back_as_it_came_with_what_the_app_does_not_know() {
        let json = r#"{"id":"t1","kind":"note","start":3,"end":9,"text":"why?","quote":"lorem ","line":2,"before":"ab"}"#;
        let m: Mark = serde_json::from_str(json).unwrap();
        assert!(m.on_text());
        let back = serde_json::to_value(&m).unwrap();
        assert_eq!(back, serde_json::from_str::<Value>(json).unwrap());
        let pin = Mark {
            id: "n1".into(),
            kind: "pin".into(),
            media: Some("m1".into()),
            points: vec![[0.5, 0.25]],
            frame: Some(33),
            ..Default::default()
        };
        let v = serde_json::to_value(&pin).unwrap();
        assert_eq!(v["points"][0][1], 0.25);
        assert_eq!(v["frame"], 33);
        assert!(v.get("selector").is_none() && v.get("start").is_none());
    }

    #[test]
    fn a_question_follows_the_previous_rounds_by_was_or_title() {
        let json = r#"{"id":"BQ1","kind":"grill","round":2,"questions":[{"id":"q1","title":"Sky?"},{"id":"q2","title":"New","was":"q2"}],
            "history":[{"id":"BQ1","round":1,"questions":[{"id":"q1","title":"Sky?"},{"id":"q2","title":"Grid?"}],
            "answer":{"verdict":"grill","answers":{"q1":{"pick":"Moss","comment":""}}}}]}"#;
        let ask: Ask = serde_json::from_str(json).unwrap();
        let (round, old, answer) = ask.followed_up(&ask.questions[0]).unwrap();
        assert_eq!((round, old.id.as_str()), (1, "q1"));
        assert_eq!(answer.unwrap().pick.as_deref(), Some("Moss"));
        let (_, old, answer) = ask.followed_up(&ask.questions[1]).unwrap();
        assert_eq!(old.title, "Grid?");
        assert!(answer.is_none());
    }
}
