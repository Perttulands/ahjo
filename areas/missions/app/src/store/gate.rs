//! Words for the routes a human gate may take.
use ahjo_missions_contract::flow::Target;

/// What an answer does, in the operator's words: the line under the
/// answer and the button. `alone` says nothing else in the run is open, so
/// a route that only ends paths ends the run (Archon's `gateRouteWords`).
pub fn route(targets: &[Target], answer: &str, alone: bool) -> (String, String) {
    let ends: Vec<&str> = targets
        .iter()
        .filter_map(|t| match t {
            Target::End { outcome, .. } => Some(outcome.as_str()),
            _ => None,
        })
        .collect();
    let steps: Vec<String> = targets
        .iter()
        .filter_map(|t| match t {
            Target::Step { number, title, .. } => Some(match number {
                Some(n) => format!("{n} {title}"),
                None => title.clone(),
            }),
            _ => None,
        })
        .collect();
    if steps.is_empty() && ends.is_empty() {
        return (format!("{answer}: this route is not wired."), answer.into());
    }
    if steps.is_empty() {
        let ended = ends
            .iter()
            .map(|o| format!("this path ends ({o})"))
            .collect::<Vec<_>>()
            .join(" and ");
        let fails = ends.contains(&"rejected");
        return match (alone, fails) {
            (true, true) => (
                format!("{answer}: {ended}, and with nothing else to run, the run is sent back."),
                format!("{answer} and end the run"),
            ),
            (true, false) => (
                format!("{answer}: {ended}, and with nothing else to run, the run succeeds."),
                format!("{answer} and end the run"),
            ),
            (false, true) => (
                format!(
                    "{answer}: {ended}, so the run is sent back once its other open work ends."
                ),
                answer.into(),
            ),
            (false, false) => (
                format!("{answer}: {ended}; the run goes on with its other work."),
                answer.into(),
            ),
        };
    }
    let back = targets
        .iter()
        .any(|t| matches!(t, Target::Step { back: true, .. }));
    let how = if back { "goes back to" } else { "goes on to" };
    (
        format!("{answer}: the run {how} {}.", steps.join(", ")),
        answer.into(),
    )
}

/// Names the card whose output the gate is considering.
pub fn source(mission: &ahjo_missions_contract::Mission, id: &str) -> String {
    let title = mission.card_title(id).trim();
    if title.is_empty() {
        "upstream step".into()
    } else {
        title.into()
    }
}
/// One candidate of a pick request: the step to name when several steps
/// feed the gate, the attempt, and its button's words.
#[derive(Debug, PartialEq)]
pub struct PickChoice {
    pub step: Option<String>,
    pub attempt: i64,
    pub label: String,
}

/// The candidates of a pick request: a step's own attempts by number, or
/// the outputs of several steps feeding the gate by step, titled by `name`.
pub fn pick_choices(pick: &serde_json::Value, name: impl Fn(&str) -> String) -> Vec<PickChoice> {
    (pick
        .get("candidates")
        .and_then(|v| v.as_array())
        .into_iter())
    .flatten()
    .filter_map(|candidate| {
        if let Some(attempt) = candidate.as_i64() {
            return Some(PickChoice {
                step: None,
                attempt,
                label: format!("Pick {attempt}"),
            });
        }
        let step = candidate.get("step_id")?.as_str()?;
        Some(PickChoice {
            step: Some(step.to_owned()),
            attempt: candidate.get("attempt")?.as_i64()?,
            label: format!("Pick {}", name(step)),
        })
    })
    .collect()
}

/// Names candidates and the judge's recommendation in a pick request.
pub fn picking_words(
    pick: &serde_json::Value,
    goes: &str,
    name: impl Fn(&str) -> String,
) -> Vec<String> {
    let choices = pick_choices(pick, &name);
    let gathered = choices.iter().any(|choice| choice.step.is_some());
    let mut words = vec![if gathered {
        let steps: Vec<_> = (choices.iter())
            .filter_map(|choice| choice.step.as_deref())
            .map(&name)
            .collect();
        let steps = match steps.as_slice() {
            [rest @ .., last] if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
            _ => steps.concat(),
        };
        format!(
            "{steps} did this work side by side. Take the best output; the others stay in the attempts."
        )
    } else {
        let attempts: Vec<_> = choices.iter().map(|c| c.attempt.to_string()).collect();
        format!(
            "The step's limit is spent. Its gate judged attempts {}; take the best one.",
            attempts.join(", ")
        )
    }];
    if let Some(rec) = pick.get("recommendation")
        && let Some(attempt) = rec.get("attempt").and_then(|v| v.as_i64())
    {
        let why = rec
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        let what = match rec.get("step_id").and_then(|v| v.as_str()) {
            Some(step) if gathered => name(step),
            _ => format!("attempt {attempt}"),
        };
        words.push(if why.is_empty() {
            format!("The judge recommends {what}.")
        } else {
            format!("The judge recommends {what}: {why}")
        });
    }
    words.push(goes.to_owned());
    words
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_gate_quote_names_the_feeding_card_or_a_plain_fallback() {
        let mission = ahjo_missions_contract::Mission {
            steps: vec![
                ahjo_missions_contract::Step {
                    id: "named".into(),
                    title: "Plan".into(),
                    ..Default::default()
                },
                ahjo_missions_contract::Step {
                    id: "untitled".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        assert_eq!(source(&mission, "named"), "Plan");
        assert_eq!(source(&mission, "untitled"), "upstream step");
        assert_eq!(source(&mission, "missing"), "upstream step");
        assert_eq!(source(&mission, ""), "upstream step");
    }
    #[test]
    fn a_pick_among_steps_names_each_step_and_picks_by_step() {
        let pick = serde_json::json!({"candidates":[{"step_id":"fmn_a","attempt":1},{"step_id":"fmn_b","attempt":2}], "recommendation":{"step_id":"fmn_b","attempt":2,"reason":"tighter"}});
        let name = |id: &str| id.trim_start_matches("fmn_").to_uppercase();
        assert_eq!(
            pick_choices(&pick, name),
            [
                PickChoice {
                    step: Some("fmn_a".into()),
                    attempt: 1,
                    label: "Pick A".into()
                },
                PickChoice {
                    step: Some("fmn_b".into()),
                    attempt: 2,
                    label: "Pick B".into()
                }
            ]
        );
        assert_eq!(
            picking_words(&pick, "goes", name),
            [
                "A and B did this work side by side. Take the best output; the others stay in the attempts.",
                "The judge recommends B: tighter",
                "goes"
            ]
        );
    }
    #[test]
    fn a_pick_request_names_its_attempts_and_the_judges_choice() {
        let pick = serde_json::json!({"candidates":[1,2], "recommendation":{"attempt":2,"reason":"tighter"}});
        assert_eq!(
            picking_words(&pick, "goes", str::to_owned),
            [
                "The step's limit is spent. Its gate judged attempts 1, 2; take the best one.",
                "The judge recommends attempt 2: tighter",
                "goes"
            ]
        );
    }
    #[test]
    fn a_route_says_where_an_answer_leads() {
        use ahjo_missions_contract::EndOutcome;
        let end = |outcome| Target::End {
            card_id: "e".into(),
            title: "End".into(),
            outcome,
        };
        assert_eq!(
            route(&[end(EndOutcome::Done)], "Approve", true).1,
            "Approve and end the run"
        );
        assert_eq!(
            route(&[end(EndOutcome::Rejected)], "Send back", true).1,
            "Send back and end the run"
        );
        assert_eq!(
            route(&[end(EndOutcome::Rejected)], "Send back", false).0,
            "Send back: this path ends (rejected), so the run is sent back once its other open work ends."
        );
        let back = Target::Step {
            card_id: "b".into(),
            number: Some(2),
            title: "Work".into(),
            back: true,
        };
        assert_eq!(
            route(&[back], "Send back", true).0,
            "Send back: the run goes back to 2 Work."
        );
        assert!(
            route(&[Target::Nowhere], "Approve", true)
                .0
                .contains("not wired")
        );
    }
}
