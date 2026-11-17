//! What the operator has said on an ask so far, and the answer it makes:
//! the JSON the desk's `/api/asks/<id>/answer` takes. Pure, so the payload
//! for each kind of ask is tested here and nowhere needs a server.

use crate::model::{Ask, Form, Question};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// The words a question gets when it does not say enough to be answered:
/// it goes back to the agent as his answer to that question.
pub const UNCLEAR: &str = "Unclear: this question is not complete in itself. Ask it again with what it refers to, what is being decided and why it matters now.";

#[derive(Clone, Debug, Default, PartialEq)]
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
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Draft {
    /// The option picked, for an ask with options.
    pub pick: Option<usize>,
    /// The note on the whole ask.
    pub note: String,
    /// Each question's answer, by question id.
    pub grill: BTreeMap<String, Said>,
    /// The question in front of the operator.
    pub at: usize,
}

/// What the operator chose to send.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Looks good, Done, the picked option or the recommendation.
    Yes,
    /// Needs work, Not yet, None of these: words required.
    No,
    /// The grill as answered so far.
    Grill,
    /// Every question left gets its recommendation, then the grill goes.
    GrillRest,
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
}

/// The answer's JSON for the desk, or the words saying what is missing.
pub fn payload(ask: &Ask, draft: &Draft, verdict: Verdict) -> Result<Value, String> {
    let note = draft.note.trim();
    let round = ask.round();
    let words_needed = |what: &str| {
        if note.is_empty() {
            Err(format!("{what} needs a word on what should change"))
        } else {
            Ok(())
        }
    };
    let (v, options) = match (ask.form(), verdict) {
        (Form::Web, _) => return Err("This ask is answered on the web desk".into()),
        (Form::Grill, Verdict::Grill | Verdict::GrillRest) => {
            let mut answers = serde_json::Map::new();
            for q in &ask.questions {
                let mut said = draft.said(q);
                if verdict == Verdict::GrillRest && !said.answered() {
                    said.pick = crate::model::recommended(&q.options, &q.recommend);
                    said.rec = said.pick.is_some();
                }
                let mut words = said.words.trim().to_string();
                if said.unclear {
                    words = if words.is_empty() {
                        UNCLEAR.into()
                    } else {
                        format!("{UNCLEAR} {words}")
                    };
                }
                let pick = said.pick.and_then(|i| q.options.get(i)).map(|o| &o.name);
                if pick.is_none() && words.is_empty() {
                    continue;
                }
                let mut a = json!({"pick": pick, "comment": words});
                if said.rec && pick.is_some() {
                    a["rec"] = json!(true);
                }
                answers.insert(q.id.clone(), a);
            }
            if answers.is_empty() && note.is_empty() {
                return Err("Answer a question, or write a word on the round".into());
            }
            return Ok(json!({"round": round, "verdict": "grill", "options": [],
                "comment": note, "answers": answers}));
        }
        (Form::Grill, _) => return Err("A grilling round is sent as a whole".into()),
        (Form::Pick, Verdict::Yes) => {
            let i = draft
                .pick
                .or_else(|| ask.recommended())
                .ok_or("Pick an option first")?;
            ("pick", vec![ask.options[i].name.clone()])
        }
        (Form::Pick, _) => {
            words_needed("None of these")?;
            ("send back", vec![])
        }
        (Form::Look, Verdict::Yes) => ("accept", vec![]),
        (Form::Do, Verdict::Yes) => ("acknowledge", vec![]),
        (Form::Look | Form::Do, _) => {
            words_needed(if ask.form() == Form::Do {
                "Not yet"
            } else {
                "Needs work"
            })?;
            ("send back", vec![])
        }
    };
    Ok(json!({"round": round, "verdict": v, "options": options, "comment": note}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask(json: &str) -> Ask {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn a_pick_takes_the_recommendation_when_nothing_is_picked() {
        let a = ask(
            r#"{"id":"AQ2","kind":"decide","status":"open","round":1,"options":[{"name":"Quiet"},{"name":"Bright"}],"recommend":"Quiet: calm"}"#,
        );
        let mut d = Draft::default();
        let p = payload(&a, &d, Verdict::Yes).unwrap();
        assert_eq!(p["verdict"], "pick");
        assert_eq!(p["options"], json!(["Quiet"]));
        assert_eq!(p["round"], 1);
        d.pick = Some(1);
        assert_eq!(
            payload(&a, &d, Verdict::Yes).unwrap()["options"],
            json!(["Bright"])
        );
        assert!(
            payload(&a, &d, Verdict::No).is_err(),
            "None of these needs words"
        );
        d.note = "Neither; darker".into();
        let p = payload(&a, &d, Verdict::No).unwrap();
        assert_eq!(
            (p["verdict"].as_str(), p["comment"].as_str()),
            (Some("send back"), Some("Neither; darker"))
        );
    }

    #[test]
    fn a_look_is_accepted_or_sent_back_with_words() {
        let a = ask(r#"{"id":"AQ1","kind":"look","status":"open","round":3}"#);
        let mut d = Draft::default();
        assert_eq!(payload(&a, &d, Verdict::Yes).unwrap()["verdict"], "accept");
        assert!(payload(&a, &d, Verdict::No).is_err());
        d.note = "The rim floats".into();
        let p = payload(&a, &d, Verdict::No).unwrap();
        assert_eq!(
            (p["verdict"].as_str(), p["round"].as_u64()),
            (Some("send back"), Some(3))
        );
    }

    #[test]
    fn a_grill_sends_what_was_said_and_lgtm_fills_the_rest() {
        let a = ask(
            r#"{"id":"BQ1","kind":"grill","status":"open","round":1,"questions":[
            {"id":"q1","title":"Sky?","options":[{"name":"Ember"},{"name":"Moss"}],"recommend":"Moss: older"},
            {"id":"q2","title":"Grid?","options":[{"name":"Always"},{"name":"Never"}],"recommend":"Never"},
            {"id":"q3","title":"Else?"},
            {"id":"q4","title":"Hands?","options":[{"name":"Hammer"}]}]}"#,
        );
        let mut d = Draft::default();
        assert!(payload(&a, &d, Verdict::Grill).is_err(), "nothing said yet");
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
        let p = payload(&a, &d, Verdict::Grill).unwrap();
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
        let p = payload(&a, &d, Verdict::GrillRest).unwrap();
        assert_eq!(
            p["answers"]["q2"],
            json!({"pick": "Never", "comment": "", "rec": true})
        );
        assert_eq!(p["answers"]["q1"]["pick"], "Ember", "a pick stands");
        assert!(p["answers"].get("q4").is_none(), "no recommendation: left");
    }
}
