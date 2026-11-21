//! What the operator has said on an ask so far, and the answer it makes:
//! the JSON the desk's `/api/asks/<id>/answer` takes. His words are the
//! answer; a pick, a "looks good" or a grill's picks come with them when he
//! wants. Pure, so each payload is tested here and nowhere needs a server.

use crate::model::{Ask, Form, Mark, Question};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};

/// The words a question gets when it does not say enough to be answered:
/// it goes back to the agent as his answer to that question.
pub const UNCLEAR: &str = "Unclear: this question is not complete in itself. Ask it again with what it refers to, what is being decided and why it matters now.";

/// What he said on one question of a grill.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Said {
    pub pick: Option<usize>,
    pub words: String,
    /// Taken as recommended rather than chosen.
    pub rec: bool,
    pub unclear: bool,
}

impl Said {
    pub fn answered(&self) -> bool {
        self.pick.is_some() || !self.words.trim().is_empty() || self.unclear
    }
}

/// A draft answer to one round of one ask.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Draft {
    /// The option he leans to, when the ask has options: never needed.
    pub pick: Option<usize>,
    /// His words on the whole ask (on a grill, on the whole round).
    pub words: String,
    /// Each question's answer, by question id.
    pub grill: BTreeMap<String, Said>,
    /// The grill's question his words go to; none, the round.
    pub at: Option<usize>,
    /// The marks as he has them now; none, as the desk has them.
    pub marks: Option<Vec<Mark>>,
    /// The pictures he pasted, as the desk keeps them for this ask
    /// (`pasted-1.png`): they go with his answer.
    pub pictures: Vec<String>,
}

impl Draft {
    pub fn said(&self, q: &Question) -> Said {
        self.grill.get(&q.id).cloned().unwrap_or_default()
    }

    pub fn answered(&self, ask: &Ask) -> usize {
        ask.questions
            .iter()
            .filter(|q| self.said(q).answered())
            .count()
    }

    /// The marks he would send: his, else the desk's.
    pub fn marks<'a>(&'a self, ask: &'a Ask) -> &'a [Mark] {
        self.marks.as_deref().unwrap_or(&ask.marks)
    }

    /// Whether anything is written, picked or marked that sending would
    /// lose.
    pub fn touched(&self) -> bool {
        self.pick.is_some()
            || !self.words.trim().is_empty()
            || self.grill.values().any(Said::answered)
            || self.marks.is_some()
            || !self.pictures.is_empty()
    }
}

/// What he has said on an ask, as its row in the list shows it: the
/// options he chose, in his order, his words, and how many marks he made.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Gist {
    pub picks: Vec<String>,
    pub words: String,
    pub marks: usize,
}

impl Gist {
    pub fn is_empty(&self) -> bool {
        self.picks.is_empty() && self.words.is_empty() && self.marks == 0
    }

    /// A draft's: on a grill, each question's pick, and the round's words or
    /// else the first question's he wrote on.
    pub fn of_draft(d: &Draft, ask: &Ask) -> Gist {
        let names = |options: &[crate::model::Choice], i: Option<usize>| {
            i.filter(|&i| i < options.len())
                .map(|i| crate::model::label(options, i))
        };
        let (picks, first): (Vec<String>, Option<String>) = if ask.form() == Form::Grill {
            let said: Vec<(Said, &Question)> =
                ask.questions.iter().map(|q| (d.said(q), q)).collect();
            (
                said.iter()
                    .filter_map(|(s, q)| names(&q.options, s.pick))
                    .collect(),
                said.iter()
                    .map(|(s, _)| s.words.trim().to_string())
                    .find(|w| !w.is_empty()),
            )
        } else {
            (names(&ask.options, d.pick).into_iter().collect(), None)
        };
        let words = Some(d.words.trim().to_string())
            .filter(|w| !w.is_empty())
            .or(first)
            .unwrap_or_default();
        Gist {
            picks,
            words,
            marks: d.marks(ask).len(),
        }
    }

    /// An answer's, as the desk keeps it.
    pub fn of_answer(a: &crate::model::Answer, ask: &Ask) -> Gist {
        // A chosen name as its option shows it, without the letter.
        let shown = |options: &[crate::model::Choice], name: &str| {
            options
                .iter()
                .position(|o| o.name == name)
                .map_or_else(|| name.to_string(), |i| crate::model::label(options, i))
        };
        let picks = if a.answers.is_empty() {
            a.options.iter().map(|n| shown(&ask.options, n)).collect()
        } else {
            ask.questions
                .iter()
                .filter_map(|q| {
                    let g = a.answers.get(&q.id)?;
                    g.pick.as_deref().map(|n| shown(&q.options, n))
                })
                .collect()
        };
        let words = Some(a.comment.trim().to_string())
            .filter(|w| !w.is_empty())
            .or_else(|| {
                ask.questions
                    .iter()
                    .filter_map(|q| a.answers.get(&q.id))
                    .map(|g| g.comment.trim().to_string())
                    .find(|w| !w.is_empty())
            })
            .unwrap_or_default();
        Gist {
            picks,
            words,
            marks: ask.marks.len(),
        }
    }
}

/// Which draft: the desk (where it listens, which outlives the window's
/// order of desks), the ask and its round.
pub type DraftKey = (String, String, u32);

/// One draft as it is kept between launches.
#[derive(Serialize, Deserialize)]
struct Kept {
    desk: String,
    ask: String,
    round: u32,
    draft: Draft,
}

/// The drafts worth keeping, as the file holds them: only what sending
/// would lose, so an ask opened and left leaves nothing behind.
pub fn keep(drafts: &HashMap<DraftKey, Draft>) -> String {
    let mut kept: Vec<Kept> = drafts
        .iter()
        .filter(|(_, d)| d.touched())
        .map(|((desk, ask, round), draft)| Kept {
            desk: desk.clone(),
            ask: ask.clone(),
            round: *round,
            draft: draft.clone(),
        })
        .collect();
    kept.sort_by(|a, b| (&a.desk, &a.ask, a.round).cmp(&(&b.desk, &b.ask, b.round)));
    serde_json::to_string_pretty(&kept).unwrap_or_else(|_| "[]".into())
}

/// The drafts a file kept.
pub fn kept(text: &str) -> Result<HashMap<DraftKey, Draft>, String> {
    let kept: Vec<Kept> = serde_json::from_str(text).map_err(|e| e.to_string())?;
    Ok(kept
        .into_iter()
        .map(|k| ((k.desk, k.ask, k.round), k.draft))
        .collect())
}

/// How he sends what he has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// His words and marks, with his pick if he made one; a grill as
    /// answered so far.
    Send,
    /// The work is right as it is (a document: move on), words optional.
    LooksGood,
    /// A grill: every question he left takes its recommendation.
    GrillRest,
    /// He answered in the agent's session or by voice; any words say how.
    Elsewhere,
}

/// The answer's JSON for the desk, or the words saying what is missing.
/// `marks` is how many marks go with it (already on the desk).
pub fn payload(ask: &Ask, draft: &Draft, verdict: Verdict, marks: usize) -> Result<Value, String> {
    let words = draft.words.trim();
    let round = ask.round();
    let pictures = &draft.pictures;
    let with = |v: &str, options: Vec<String>| {
        let mut p = json!({"round": round, "verdict": v, "options": options, "comment": words});
        if !pictures.is_empty() {
            p["attachments"] = json!(pictures);
        }
        p
    };
    // A picture says something, as a mark does.
    let marks = marks + pictures.len();
    if verdict == Verdict::Elsewhere {
        return Ok(with("elsewhere", vec![]));
    }
    match ask.form() {
        Form::Grill => {
            if verdict == Verdict::LooksGood {
                return Err("A grilling round is answered question by question".into());
            }
            let mut answers = serde_json::Map::new();
            for q in &ask.questions {
                let mut said = draft.said(q);
                if verdict == Verdict::GrillRest && !said.answered() {
                    said.pick = crate::model::recommended(&q.options, &q.recommend);
                    said.rec = said.pick.is_some();
                }
                let mut text = said.words.trim().to_string();
                if said.unclear {
                    text = if text.is_empty() {
                        UNCLEAR.into()
                    } else {
                        format!("{UNCLEAR} {text}")
                    };
                }
                let pick = said.pick.and_then(|i| q.options.get(i)).map(|o| &o.name);
                if pick.is_none() && text.is_empty() {
                    continue;
                }
                let mut a = json!({"pick": pick, "comment": text});
                if said.rec && pick.is_some() {
                    a["rec"] = json!(true);
                }
                answers.insert(q.id.clone(), a);
            }
            if answers.is_empty() && words.is_empty() && marks == 0 {
                return Err("Answer a question, mark something or write a word first".into());
            }
            let mut p = with("grill", vec![]);
            p["answers"] = Value::Object(answers);
            Ok(p)
        }
        Form::Hands => match verdict {
            Verdict::Send if !words.is_empty() || !pictures.is_empty() => {
                Ok(with("send back", vec![]))
            }
            Verdict::Send => {
                Err("Press Done once out/ holds the work, or say what stops the job".into())
            }
            _ => Err("A job for your hands is done with Done: the desk checks out/".into()),
        },
        Form::Open => match verdict {
            Verdict::LooksGood => Ok(with("accept", vec![])),
            _ => match draft.pick.and_then(|i| ask.options.get(i)) {
                Some(o) => Ok(with("pick", vec![o.name.clone()])),
                None if !words.is_empty() || marks > 0 => Ok(with("marks", vec![])),
                None => Err("Write what you think, or mark something, first".into()),
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask(json: &str) -> Ask {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn a_draft_and_its_marks_outlive_the_window() {
        let mut drafts = HashMap::new();
        let mark: Mark = serde_json::from_str(
            r#"{"id":"k1","kind":"pin","media":"m1","points":[[0.5,0.25]],"text":"the rim","anchor":{"x":1}}"#,
        )
        .unwrap();
        let mut grill = BTreeMap::new();
        grill.insert(
            "q2".to_string(),
            Said {
                pick: Some(1),
                words: "the second".into(),
                ..Said::default()
            },
        );
        let written = Draft {
            pick: Some(0),
            words: "Quieter, and the rim wetter".into(),
            grill,
            at: Some(1),
            marks: Some(vec![mark]),
            pictures: vec!["pasted-1.png".into()],
        };
        let desk = "http://127.0.0.1:8774".to_string();
        drafts.insert((desk.clone(), "AQ2".into(), 2), written.clone());
        // Opened and left: nothing to lose, nothing kept.
        drafts.insert((desk.clone(), "AQ3".into(), 1), Draft::default());
        let back = kept(&keep(&drafts)).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[&(desk, "AQ2".to_string(), 2)], written);
        assert!(kept("not json").is_err());
    }

    #[test]
    fn words_are_the_answer_and_a_pick_only_comes_with_them() {
        let a = ask(
            r#"{"id":"AQ2","kind":"decide","status":"open","round":2,"options":[{"name":"Quiet"},{"name":"Bright"}],"recommend":"Quiet: calm"}"#,
        );
        let mut d = Draft::default();
        assert!(
            payload(&a, &d, Verdict::Send, 0).is_err(),
            "nothing said, nothing sent: the recommendation is never taken for him"
        );
        d.words = "Neither yet: try the quiet one at dusk".into();
        let p = payload(&a, &d, Verdict::Send, 0).unwrap();
        assert_eq!(
            (
                p["verdict"].as_str(),
                p["comment"].as_str(),
                p["round"].as_u64()
            ),
            (
                Some("marks"),
                Some("Neither yet: try the quiet one at dusk"),
                Some(2)
            )
        );
        d.pick = Some(1);
        let p = payload(&a, &d, Verdict::Send, 0).unwrap();
        assert_eq!(
            (p["verdict"].as_str(), &p["options"]),
            (Some("pick"), &json!(["Bright"]))
        );
        assert_eq!(p["comment"], "Neither yet: try the quiet one at dusk");
    }

    #[test]
    fn marks_alone_answer_and_looks_good_needs_no_words() {
        let a = ask(r#"{"id":"AQ1","kind":"look","status":"open","round":1}"#);
        let d = Draft::default();
        assert_eq!(
            payload(&a, &d, Verdict::Send, 2).unwrap()["verdict"],
            "marks"
        );
        assert_eq!(
            payload(&a, &d, Verdict::LooksGood, 0).unwrap()["verdict"],
            "accept"
        );
    }

    #[test]
    fn any_ask_can_be_answered_elsewhere_with_or_without_a_note() {
        let grill = ask(
            r#"{"id":"BQ1","kind":"grill","status":"open","questions":[{"id":"q1","title":"Sky?"}]}"#,
        );
        let hands = ask(r#"{"id":"BQ2","kind":"hands","status":"open"}"#);
        let mut d = Draft::default();
        for a in [&grill, &hands] {
            let p = payload(a, &d, Verdict::Elsewhere, 0).unwrap();
            assert_eq!(
                (p["verdict"].as_str(), p["comment"].as_str()),
                (Some("elsewhere"), Some(""))
            );
        }
        d.words = "Told it in the session".into();
        d.pick = Some(0);
        let p = payload(&hands, &d, Verdict::Elsewhere, 0).unwrap();
        assert_eq!(p["comment"], "Told it in the session");
        assert_eq!(p["options"], json!([]), "a pick does not go with it");
    }

    #[test]
    fn a_pasted_picture_goes_with_the_answer_and_is_enough_alone() {
        let a = ask(r#"{"id":"AQ1","kind":"look","status":"open","round":1}"#);
        let mut d = Draft::default();
        assert!(payload(&a, &d, Verdict::Send, 0).is_err());
        d.pictures = vec!["pasted-1.png".into()];
        assert!(d.touched(), "a picture is kept with the draft");
        let p = payload(&a, &d, Verdict::Send, 0).unwrap();
        assert_eq!(p["verdict"], "marks");
        assert_eq!(p["attachments"], json!(["pasted-1.png"]));
        let g = ask(
            r#"{"id":"BQ1","kind":"grill","status":"open","questions":[{"id":"q1","title":"Sky?"}]}"#,
        );
        assert_eq!(
            payload(&g, &d, Verdict::Send, 0).unwrap()["attachments"],
            json!(["pasted-1.png"])
        );
        d.pictures.clear();
        assert!(payload(&a, &d, Verdict::Send, 0).is_err());
        assert!(
            payload(&a, &d, Verdict::LooksGood, 0)
                .unwrap()
                .get("attachments")
                .is_none()
        );
    }

    #[test]
    fn a_hands_job_is_sent_back_in_words_only() {
        let a = ask(r#"{"id":"BQ2","kind":"hands","status":"open"}"#);
        let mut d = Draft::default();
        assert!(payload(&a, &d, Verdict::Send, 0).is_err());
        assert!(payload(&a, &d, Verdict::LooksGood, 0).is_err());
        d.words = "The laptop is away this week".into();
        assert_eq!(
            payload(&a, &d, Verdict::Send, 0).unwrap()["verdict"],
            "send back"
        );
    }

    #[test]
    fn a_grill_sends_what_was_said_and_the_rest_can_take_its_recommendations() {
        let a = ask(
            r#"{"id":"BQ1","kind":"grill","status":"open","round":1,"questions":[
            {"id":"q1","title":"Sky?","options":[{"name":"Ember"},{"name":"Moss"}],"recommend":"Moss: older"},
            {"id":"q2","title":"Grid?","options":[{"name":"Always"},{"name":"Never"}],"recommend":"Never"},
            {"id":"q3","title":"Else?"},
            {"id":"q4","title":"Hands?","options":[{"name":"Hammer"}]}]}"#,
        );
        let mut d = Draft::default();
        assert!(
            payload(&a, &d, Verdict::Send, 0).is_err(),
            "nothing said yet"
        );
        assert!(
            payload(&a, &d, Verdict::Send, 1).is_ok(),
            "a mark is something"
        );
        d.grill.insert(
            "q1".into(),
            Said {
                pick: Some(0),
                ..Default::default()
            },
        );
        d.grill.insert(
            "q3".into(),
            Said {
                unclear: true,
                ..Default::default()
            },
        );
        let p = payload(&a, &d, Verdict::Send, 0).unwrap();
        assert_eq!(p["verdict"], "grill");
        assert_eq!(p["answers"]["q1"], json!({"pick": "Ember", "comment": ""}));
        assert_eq!(p["answers"]["q3"]["pick"], Value::Null);
        assert!(
            p["answers"]["q3"]["comment"]
                .as_str()
                .unwrap()
                .starts_with("Unclear")
        );
        assert!(p["answers"].get("q2").is_none(), "skipped");
        let p = payload(&a, &d, Verdict::GrillRest, 0).unwrap();
        assert_eq!(
            p["answers"]["q2"],
            json!({"pick": "Never", "comment": "", "rec": true})
        );
        assert_eq!(p["answers"]["q1"]["pick"], "Ember", "a pick stands");
        assert!(p["answers"].get("q4").is_none(), "no recommendation: left");
        assert!(payload(&a, &d, Verdict::LooksGood, 0).is_err());
    }

    #[test]
    fn a_row_shows_what_he_said_not_a_label() {
        let decide = ask(
            r#"{"id":"AQ1","kind":"decide","status":"open","options":[{"name":"B: Inter"},{"name":"Quattro"}]}"#,
        );
        assert!(Gist::of_draft(&Draft::default(), &decide).is_empty());
        let d = Draft {
            pick: Some(0),
            words: "  Inter, with more leading ".into(),
            ..Draft::default()
        };
        let g = Gist::of_draft(&d, &decide);
        assert_eq!(
            g.picks,
            vec!["Inter".to_string()],
            "the name, not its letter"
        );
        assert_eq!(g.words, "Inter, with more leading");
        let grill = ask(r#"{"id":"AQ5","kind":"grill","status":"open","questions":[
                {"id":"q1","title":"Lay","options":[{"name":"Point"},{"name":"Carry"}]},
                {"id":"q2","title":"Fall","options":[{"name":"Tumble"},{"name":"Puff"}]}]}"#);
        let mut d = Draft::default();
        d.grill.insert(
            "q2".into(),
            Said {
                pick: Some(1),
                words: "softly".into(),
                ..Said::default()
            },
        );
        d.grill.insert(
            "q1".into(),
            Said {
                pick: Some(0),
                ..Said::default()
            },
        );
        let g = Gist::of_draft(&d, &grill);
        assert_eq!(
            g.picks,
            vec!["Point".to_string(), "Puff".to_string()],
            "in the questions' order"
        );
        assert_eq!(
            g.words, "softly",
            "the round has no words: the first question's he wrote on"
        );
    }
}
