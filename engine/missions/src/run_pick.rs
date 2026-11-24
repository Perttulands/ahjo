//! Best of N (Archon `run_pick.go`). A step whose own Limit card is spent
//! has attempts its gate already judged. A card whose when_spent is best, or a
//! resume with `--pick-best`, picks the best of them instead of stopping: the
//! gate's judge chain compares them side by side, or the operator picks, and
//! the picked attempt goes on through the gate's pass route with its output,
//! as if the gate had passed it.
//!
//! The ledger records a pick in two events. `pick_started` names the step,
//! its selecting gate, the spent limit and the candidate attempts; it settles
//! the step's pending send-back, as the step's output would. The pick ends in
//! a `gate_verdict` pass whose `pick` names the attempt, the candidates, why
//! and who picked. Between the two a judge chain runs, anchored at
//! `pick_started`, or a `human_input_requested` with a `pick` waits for the
//! operator. Replay finishes a pick the ledger started and did not end.
//!
//! A gate fed by several steps picks the same way among their outputs, once
//! each has one: its `pick_started` belongs to the gate, its trigger is
//! `gathered` and each candidate names its step. Every feeding step's work
//! waits for the pick (`run_workspace::review_gates`), so only the picked
//! output reaches the run branch; the others stay in the run's attempts.
use crate::coordinator::Control;
use crate::limits::LimitUse;
use crate::run_limit::LimitStop;
use crate::run_worker::event;
use crate::{Coordinator, RoutedInput, RunEvent};
use ahjo_mission_model::{Gate, HumanPick, Mission, RunProjection, RunStart};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, io};

pub(crate) const PICK_STARTED: &str = "pick_started";
/// The block a judge's malformed pick records; a plain resume asks the final
/// judge again.
pub(crate) const INVALID_JUDGE_PICK: &str = "invalid_judge_pick";
/// Who decides a pick with only one candidate (Archon says `archon`).
const DECIDED_BY_AHJO: &str = "ahjo";
/// The trigger of a pick among the outputs of several steps feeding a gate.
pub(crate) const GATHERED: &str = "gathered";

/// An attempt a pick may take: the step's output was accepted (`output_seq`)
/// and its selecting gate judged it (`verdict_seq`). An attempt a limit cut
/// off, or a crash ended, has no output and is never one. A gathered pick's
/// candidate names its step and carries the output the gate was handed, which
/// no verdict judged.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct Candidate {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub step_id: String,
    pub attempt: i64,
    pub output_seq: u64,
    #[serde(default)]
    pub verdict_seq: u64,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub input: Value,
}

/// What picking a step's best attempt takes: its selecting gate and the
/// attempts that gate judged.
pub(crate) struct StepPick<'a> {
    pub gate: &'a Gate,
    pub candidates: Vec<Candidate>,
}

/// A decided pick.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Choice {
    /// The picked step: empty for one of a step's own attempts.
    pub step_id: String,
    pub attempt: i64,
    pub reason: String,
    pub evidence: Vec<String>,
    pub decided_by: String,
}
impl Choice {
    fn data(&self) -> Value {
        let evidence: Vec<_> = (self.evidence.iter())
            .map(|text| json!({"kind":"step","text":text}))
            .collect();
        let mut data = json!({"attempt":self.attempt,"reason":self.reason,"evidence":evidence,"decided_by":self.decided_by});
        if !self.step_id.is_empty() {
            data["step_id"] = self.step_id.clone().into();
        }
        data
    }
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}
fn data(event: &RunEvent) -> &serde_json::Map<String, Value> {
    &event.data
}

/// Whether an event belongs to a pick: a pick's gate verdict, its human
/// request, or the operator's pick.
pub(crate) fn is_pick(event: &RunEvent) -> bool {
    event.data.contains_key("pick")
}
/// The `pick_started` a pick's verdict or request answers.
pub(crate) fn pick_seq(event: &RunEvent) -> u64 {
    (event.data.get("pick"))
        .and_then(|pick| pick.get("pick_seq"))
        .and_then(Value::as_u64)
        .unwrap_or(0)
}

/// The step's attempts whose output the gate judged with a verdict of its
/// own, by attempt; a pick's verdict judges nothing. Each verdict names the
/// output it judged (its input_ref's origin). An attempt's latest verdict
/// counts.
pub(crate) fn candidates(events: &[RunEvent], step: &str, gate: &str) -> Vec<Candidate> {
    let mut outputs = BTreeMap::new();
    let mut judged = BTreeMap::new();
    for event in events {
        match event.kind.as_str() {
            "card_output" if event.card_id == step => {
                outputs.insert(event.attempt, event.seq);
            }
            "gate_verdict" if event.gate_id == gate && !is_pick(event) => {
                let port = data(event).get("route_port").and_then(Value::as_str);
                if port != Some("pass") && port != Some("fail") {
                    continue;
                }
                let origin = (data(event).get("input_ref"))
                    .and_then(|input| input.get("origin"))
                    .and_then(|origin| {
                        serde_json::from_value::<ahjo_mission_model::PublicOrigin>(origin.clone())
                            .ok()
                    });
                let Some(origin) = origin.filter(|origin| origin.card_id == step) else {
                    continue;
                };
                let Some(output_seq) = outputs.get(&origin.attempt) else {
                    continue;
                };
                judged.insert(
                    origin.attempt,
                    Candidate {
                        step_id: String::new(),
                        attempt: origin.attempt,
                        output_seq: *output_seq,
                        verdict_seq: event.seq,
                        input: Value::Null,
                    },
                );
            }
            _ => {}
        }
    }
    judged.into_values().collect()
}

/// Whether the spent limit `use_`, met by `step`, can pick: it is the step's
/// own card, the step has a selecting gate, and that gate judged at least one
/// of its attempts. A mission card never picks.
pub(crate) fn pick_for<'a>(
    mission: &'a Mission,
    events: &[RunEvent],
    step: &str,
    use_: &LimitUse,
) -> Option<StepPick<'a>> {
    let card = crate::limits::card(mission, &use_.limit_id)?;
    if card.scope != step || use_.card_id != step {
        return None;
    }
    let gate = crate::graph::selecting_gate(mission, step).ok()?;
    let last = crate::run_folder::last_selection(events, step);
    let candidates = candidates(events, step, &gate.id)
        .into_iter()
        .filter(|candidate| candidate.output_seq > last)
        .collect::<Vec<_>>();
    (!candidates.is_empty()).then_some(StepPick { gate, candidates })
}

/// Whether the gate judged one of the candidates after the step's latest
/// pick. A card picks by itself only then, so a pass route that leads back
/// to the step cannot pick the same attempts forever.
pub(crate) fn judged_since_last_pick(
    events: &[RunEvent],
    step: &str,
    candidates: &[Candidate],
) -> bool {
    let last = (events.iter())
        .filter(|e| e.kind == PICK_STARTED && e.card_id == step)
        .map(|e| e.seq)
        .next_back()
        .unwrap_or(0);
    candidates.iter().any(|c| c.verdict_seq > last)
}

/// The spent limit a resume with `--pick-best` asked to pick at, while that
/// resume is the run's latest and no pick started since.
fn pending_resume_pick(events: &[RunEvent]) -> Option<LimitUse> {
    for event in events.iter().rev() {
        match event.kind.as_str() {
            PICK_STARTED => return None,
            "run_resumed" => {
                if event.data.get("pick_best").and_then(Value::as_bool) != Some(true) {
                    return None;
                }
                let from = event.data.get("resumed_from_seq").and_then(Value::as_u64)?;
                let block = events.iter().find(|e| e.seq == from)?;
                return LimitUse::recorded(block);
            }
            _ => {}
        }
    }
    None
}

/// Record `pick_started`, which settles the step's pending send-back as its
/// output would; the run's worker decides the pick.
#[allow(clippy::too_many_arguments)]
pub(crate) fn start(
    c: &Coordinator,
    id: &str,
    control: &Control,
    mission: &Mission,
    step: &str,
    pick: &StepPick<'_>,
    use_: &LimitUse,
    trigger: &str,
) -> io::Result<()> {
    let mut data = json!({"step_id":step,"gate_id":pick.gate.id,"trigger":trigger,"limit":use_,"candidates":pick.candidates});
    if pick
        .gate
        .kinds
        .contains(&ahjo_mission_model::GateKind::Step)
    {
        let chain: Vec<_> = (crate::graph::judge_chain(mission, &pick.gate.id).into_iter())
            .map(|judge| judge.id.clone())
            .collect();
        data["judge_chain"] = json!(chain);
    }
    let mut started = event(PICK_STARTED, step, 0, data);
    started.gate_id = pick.gate.id.clone();
    crate::run_worker::append_live(c, id, control, started).map(|_| ())
}

/// Start the pick of each gate fed by several steps once every one of them
/// has handed it an output since its last pick. Says whether one started.
fn gather(
    context: &crate::run_gate::GateContext<'_>,
    running: &std::collections::HashSet<String>,
) -> io::Result<bool> {
    let crate::run_gate::GateContext {
        c,
        id,
        control,
        mission,
        events,
        replay,
        ..
    } = *context;
    for gate in &mission.gates {
        if replay.picking.contains(&gate.id)
            || running.contains(&format!("gate:{}", gate.id))
            || !crate::graph::gathers(mission, &gate.id)
        {
            continue;
        }
        let feeders = crate::graph::gate_feeders(mission, &gate.id);
        let Some(held) = (replay.gathered.get(&gate.id))
            .filter(|held| feeders.iter().all(|step| held.contains_key(*step)))
        else {
            continue;
        };
        let mut candidates = Vec::new();
        for step in feeders {
            let input = &held[step];
            let attempt = input.origin.as_ref().map_or(0, |origin| origin.attempt);
            let output_seq = (events.iter())
                .rev()
                .find(|e| e.kind == "card_output" && e.card_id == step && e.attempt == attempt)
                .map_or(0, |e| e.seq);
            candidates.push(Candidate {
                step_id: step.to_owned(),
                attempt,
                output_seq,
                verdict_seq: 0,
                input: serde_json::to_value(input)?,
            });
        }
        let mut data = json!({"gate_id":gate.id,"trigger":GATHERED,"candidates":candidates});
        if gate.kinds.contains(&ahjo_mission_model::GateKind::Step) {
            let chain: Vec<_> = (crate::graph::judge_chain(mission, &gate.id).into_iter())
                .map(|judge| judge.id.clone())
                .collect();
            data["judge_chain"] = json!(chain);
        }
        let mut started = event(PICK_STARTED, &gate.id, 0, data);
        started.gate_id = gate.id.clone();
        crate::run_worker::append_live(c, id, control, started)?;
        return Ok(true);
    }
    Ok(false)
}

/// Start the pick a resume with `--pick-best` asked for, whatever the card's
/// when_spent; where the step can no longer pick, the run stops at the limit
/// again.
pub(crate) fn on_resume(
    c: &Coordinator,
    id: &str,
    control: &Control,
    mission: &Mission,
) -> io::Result<Option<LimitStop>> {
    let events = c.ledger(id)?.read().map_err(crate::error::to_io)?;
    let Some(use_) = pending_resume_pick(&events) else {
        return Ok(None);
    };
    let node = use_.card_id.clone();
    if let Some(pick) = pick_for(mission, &events, &node, &use_) {
        start(c, id, control, mission, &node, &pick, &use_, "resume")?;
        return Ok(None);
    }
    crate::run_limit::append_limit_error(c, id, control, mission, &node, &use_)?;
    Ok(Some(LimitStop {
        step_id: node,
        use_,
        fail: false,
    }))
}

/// The picks the ledger started and neither ended nor put to the operator.
pub(crate) fn unsettled(events: &[RunEvent]) -> Vec<&RunEvent> {
    (events.iter())
        .filter(|started| started.kind == PICK_STARTED)
        .filter(|started| {
            !events.iter().any(|e| {
                (e.kind == "gate_verdict" || e.kind == "human_input_requested")
                    && pick_seq(e) == started.seq
            })
        })
        .collect()
}

/// A candidate as the ledger shows it: its step, the verdict its gate gave
/// it and the gate input that verdict judged, which a pick carries on. A
/// gathered candidate has no verdict and carries the output its step handed
/// the gate.
struct Detail {
    step: String,
    attempt: i64,
    workspace: Option<std::path::PathBuf>,
    verdict: String,
    reason: String,
    input: Value,
}

/// Whether a pick chooses among several steps feeding its gate.
pub(crate) fn is_gathered(started: &RunEvent) -> bool {
    started.data.get("trigger").and_then(Value::as_str) == Some(GATHERED)
}

fn details(
    c: &Coordinator,
    id: &str,
    events: &[RunEvent],
    started: &RunEvent,
) -> io::Result<Vec<Detail>> {
    let candidates: Vec<Candidate> =
        serde_json::from_value(started.data.get("candidates").cloned().unwrap_or_default())
            .map_err(|e| invalid(&format!("pick {} candidates: {e}", started.seq)))?;
    let gathered = is_gathered(started);
    let mut details = Vec::new();
    for candidate in candidates {
        let step = if gathered {
            candidate.step_id.clone()
        } else {
            started.card_id.clone()
        };
        let (verdict, reason, input) = if gathered {
            (String::new(), String::new(), candidate.input.clone())
        } else {
            let verdict = (events.iter())
                .find(|e| e.seq == candidate.verdict_seq && e.kind == "gate_verdict")
                .ok_or_else(|| {
                    invalid(&format!(
                        "pick candidate attempt {} names verdict {}",
                        candidate.attempt, candidate.verdict_seq
                    ))
                })?;
            let data = Value::Object(verdict.data.clone());
            (
                text(&data, "verdict").into(),
                text(&data, "reason").into(),
                verdict.data.get("input_ref").cloned().unwrap_or_default(),
            )
        };
        details.push(Detail {
            workspace: crate::run_folder::attempt_path(c, id, events, &step, candidate.attempt)
                .or_else(|| {
                    crate::run_workspace::attempt_path(c, id, events, &step, candidate.attempt)
                }),
            step,
            attempt: candidate.attempt,
            verdict,
            reason,
            input,
        });
    }
    if details.is_empty() {
        return Err(invalid(&format!("pick {} has no candidates", started.seq)));
    }
    Ok(details)
}

/// What a picker names its pick by: the attempt numbers, and in a gathered
/// pick the step of each, in the same order (empty otherwise).
fn keys(started: &RunEvent, details: &[Detail]) -> (Vec<i64>, Vec<String>) {
    let attempts = details.iter().map(|detail| detail.attempt).collect();
    let steps = if is_gathered(started) {
        details.iter().map(|detail| detail.step.clone()).collect()
    } else {
        Vec::new()
    };
    (attempts, steps)
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_owned())
}

/// What a picker reads: every candidate side by side, with the verdict the
/// gate gave it and the output it judged, as text or as the path of the kept
/// file or folder.
fn pick_input(
    id: &str,
    mission: &Mission,
    gate: &Gate,
    started: &RunEvent,
    details: &[Detail],
) -> RoutedInput {
    let gathered = is_gathered(started);
    let mut body = if gathered {
        format!(
            "Pick the best output. {} steps did this work side by side and each handed its output to {}; the output picked goes on through it as if it had passed, and the others stay in the run's attempts.\n",
            details.len(),
            mission.card_name(&gate.id)
        )
    } else {
        format!(
            "Pick the best attempt of {}. Its Limit card is spent, so it runs no more; the attempt picked goes on through {} as if it had passed.\n",
            mission.card_name(&started.card_id),
            mission.card_name(&gate.id)
        )
    };
    if !gate.criterion.trim().is_empty() {
        body += &format!("criterion: {}\n", gate.criterion.trim());
    }
    for detail in details {
        if gathered {
            body += &format!(
                "\nstep {} ({}) attempt {}\n",
                detail.step,
                mission.card_name(&detail.step),
                detail.attempt
            );
        } else {
            body += &format!(
                "\nattempt {}\nverdict: {}\nreason: {}\n",
                detail.attempt, detail.verdict, detail.reason
            );
        }
        if let Some(path) = &detail.workspace {
            body += &format!("attempt workspace: {}\n", path.display());
        }
        let kind = text(&detail.input, "kind");
        let reference = text(&detail.input, "ref");
        if (kind == "file" || kind == "folder") && !reference.is_empty() {
            body += &format!("output {kind}: {reference}\n");
        }
        let output = text(&detail.input, "text");
        if !output.is_empty() {
            body += &format!("output:\n{output}\n");
        }
    }
    RoutedInput {
        from_card_id: started.card_id.clone(),
        r#ref: format!("ledger://{id}/pick/{}", started.seq),
        text: body,
        ..Default::default()
    }
}

/// Attempt numbers said plainly: "1, 2 or 3".
pub(crate) fn attempt_words(attempts: &[i64]) -> String {
    let words: Vec<_> = attempts.iter().map(i64::to_string).collect();
    or_words(&words)
}

fn or_words(words: &[String]) -> String {
    match words {
        [] => "none".into(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} or {last}", rest.join(", ")),
    }
}

/// The candidates said plainly: "attempts 1, 2 or 3", or in a gathered pick
/// "step a attempt 1 or step b attempt 2".
pub(crate) fn candidate_words(attempts: &[i64], steps: &[String]) -> String {
    if steps.is_empty() {
        return format!("attempts {}", attempt_words(attempts));
    }
    let words: Vec<_> = (steps.iter().zip(attempts))
        .map(|(step, attempt)| format!("step {step} attempt {attempt}"))
        .collect();
    or_words(&words)
}

/// What a judge in a pick returns: the final judge picks with a fenced
/// `ahjo-pick` block, the judges before it recommend in plain output. In a
/// gathered pick (`steps` not empty) the pick names a step and its attempt.
pub(crate) fn contract(final_judge: bool, attempts: &[i64], steps: &[String]) -> String {
    let pick = crate::contract_names::PICK;
    let what = if steps.is_empty() {
        "attempt"
    } else {
        "output"
    };
    let words = candidate_words(attempts, steps);
    if !final_judge {
        return format!(
            "judge recommend contract:\nThis is a pick, not a verdict, and a later judge in the chain decides it: compare the {what}s below side by side and recommend the best one. Write your recommendation as your output: the {what} you recommend (one of {words}), why it is the best, and what decided it. The judges after you read it beside the {what}s. Emit no {pick} or {} block.\n",
            crate::contract_names::VERDICT
        );
    }
    let first = attempts.first().copied().unwrap_or(1);
    let (count, example, names) = match steps.first() {
        None => (
            "three",
            format!("\"attempt\":{first}"),
            "attempt must be one of",
        ),
        Some(step) => (
            "four",
            format!("\"step\":{},\"attempt\":{first}", json!(step)),
            "step and attempt must name one of",
        ),
    };
    format!(
        "judge pick contract:\nThis is a pick, not a verdict: compare the {what}s below side by side and pick the best one. Before the completion sentinel, emit exactly one fenced {pick} JSON block, separate from {}, with exactly these {count} keys:\n```{pick}\n{{{example},\"reason\":\"Why this {what} is the best\",\"evidence\":[\"what decided it\"]}}\n```\n{names} {words}, reason a string, and evidence an array of strings (use [] when there are no references). Do not omit evidence or add keys. Your pick decides; any recommendation from an earlier judge follows the {what}s.\n",
        crate::contract_names::OUTPUTS,
    )
}

/// Accept one explicitly fenced pick of a candidate, never prose. In a
/// gathered pick (`steps` not empty) it names a step and its attempt.
pub(crate) fn parse_pick(text: &str, attempts: &[i64], steps: &[String]) -> Result<Choice, String> {
    let lang = crate::contract_names::PICK;
    let marker = format!("```{lang}");
    let mut body = String::new();
    let (mut blocks, mut inside, mut closed) = (0, false, false);
    for line in text.lines().map(str::trim) {
        if line.starts_with(&marker) {
            blocks += 1;
            if line != marker || inside || blocks != 1 {
                return Err(format!("expected exactly one {lang} block"));
            }
            inside = true;
        } else if inside && line == "```" {
            inside = false;
            closed = true;
        } else if inside {
            body.push_str(line);
            body.push('\n');
        }
    }
    if blocks != 1 || inside || !closed {
        return Err(format!("missing or unterminated {lang} block"));
    }
    let (names, keys): (&str, &[&str]) = if steps.is_empty() {
        (
            "requires exactly attempt, reason and evidence",
            &["attempt", "reason", "evidence"],
        )
    } else {
        (
            "requires exactly step, attempt, reason and evidence",
            &["step", "attempt", "reason", "evidence"],
        )
    };
    let fields: serde_json::Map<String, Value> =
        serde_json::from_str(&body).map_err(|_| "judge pick must be a JSON object".to_owned())?;
    if let Some(extra) = fields.keys().find(|key| !keys.contains(&key.as_str())) {
        return Err(format!("judge pick has extra key {extra:?}; it {names}"));
    }
    let missing: Vec<_> = (keys.iter())
        .filter(|key| !fields.contains_key(**key))
        .copied()
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "judge pick is missing {}; it {names}",
            missing.join(" and ")
        ));
    }
    let step = fields.get("step").and_then(Value::as_str).unwrap_or("");
    let attempt = (fields["attempt"].as_i64())
        .filter(|attempt| {
            if steps.is_empty() {
                attempts.contains(attempt)
            } else {
                (steps.iter().zip(attempts)).any(|(s, a)| s == step && a == attempt)
            }
        })
        .ok_or_else(|| {
            let what = if steps.is_empty() {
                "attempt"
            } else {
                "step and attempt"
            };
            format!(
                "judge pick {what} must name one of {}",
                candidate_words(attempts, steps)
            )
        })?;
    let reason = (fields["reason"].as_str())
        .ok_or("judge pick reason must be a string")?
        .to_owned();
    let evidence = (fields["evidence"].as_array())
        .ok_or("judge pick evidence must be an array of strings")?
        .iter()
        .map(|item| item.as_str().map(str::to_owned))
        .collect::<Option<Vec<_>>>()
        .ok_or("judge pick evidence must contain only strings")?;
    Ok(Choice {
        step_id: step.to_owned(),
        attempt,
        reason,
        evidence,
        decided_by: String::new(),
    })
}

fn gate_attempt(events: &[RunEvent], gate: &str) -> i64 {
    (events.iter())
        .filter(|e| e.kind == "gate_evaluating" && e.gate_id == gate)
        .count() as i64
}

/// End a pick: a gate_verdict pass that carries the picked attempt's gate
/// input down the gate's pass route, as the gate's pass would have. Replay
/// delivers it. `answer` is the operator's recorded pick and the request it
/// answers, if any.
fn resolve(
    c: &Coordinator,
    id: &str,
    control: &Control,
    mission: &Mission,
    started: &RunEvent,
    choice: &Choice,
    answer: Option<(&RunEvent, &RunEvent)>,
) -> io::Result<()> {
    let events = c.ledger(id)?.read().map_err(crate::error::to_io)?;
    let details = details(c, id, &events, started)?;
    let step = if choice.step_id.is_empty() {
        &started.card_id
    } else {
        &choice.step_id
    };
    let picked = (details.iter())
        .find(|detail| &detail.step == step && detail.attempt == choice.attempt)
        .ok_or_else(|| {
            invalid(&format!(
                "pick {} names step {step} attempt {}, which is not a candidate",
                started.seq, choice.attempt
            ))
        })?;
    let (attempts, steps) = keys(started, &details);
    let mut pick = json!({"step_id":picked.step,"attempt":choice.attempt,"pick_seq":started.seq,"candidates":candidates_value(&attempts, &steps),"decided_by":choice.decided_by,"trigger":text(&Value::Object(started.data.clone()), "trigger")});
    let evidence: Vec<_> = (choice.evidence.iter())
        .map(|text| json!({"kind":"step","text":text}))
        .collect();
    let routes: Vec<_> = crate::graph::outgoing_from_port(mission, &started.gate_id, "pass")
        .map(|edge| edge.id.clone())
        .collect();
    let mut data = json!({"verdict":"pass","route_port":"pass","routed_edges":routes,"reason":choice.reason,"evidence":evidence,"input_ref":picked.input});
    let mut actor = String::new();
    if let Some((request, recorded)) = answer {
        data["requested_seq"] = request.seq.into();
        if let Some(recommendation) = request
            .data
            .get("pick")
            .and_then(|p| p.get("recommendation"))
        {
            pick["recommendation"] = recommendation.clone();
        }
        actor.clone_from(&recorded.actor);
    }
    data["pick"] = pick;
    let mut verdict = event(
        "gate_verdict",
        &started.gate_id,
        gate_attempt(&events, &started.gate_id),
        data,
    );
    verdict.gate_id.clone_from(&started.gate_id);
    verdict.actor = actor;
    crate::run_workspace::record_verdict(c, id, control, &events, verdict)
}

/// The candidates as a pick records them: attempt numbers, or in a gathered
/// pick each step and its attempt.
fn candidates_value(attempts: &[i64], steps: &[String]) -> Value {
    if steps.is_empty() {
        return json!(attempts);
    }
    (steps.iter().zip(attempts))
        .map(|(step, attempt)| json!({"step_id":step,"attempt":attempt}))
        .collect()
}

/// Ask the operator to pick the best attempt, with the judge's pick as a
/// recommendation when the gate has a judge. Only this path waits for the
/// pick; the run goes on with every other branch, as with a verdict. A
/// gathered pick names no one step: its candidates name theirs.
fn ask(
    gate: &Gate,
    started: &RunEvent,
    input: &RoutedInput,
    (attempts, steps): (&[i64], &[String]),
    recommendation: Option<&Choice>,
) -> RunEvent {
    let mut pick = json!({"pick_seq":started.seq,"candidates":candidates_value(attempts, steps)});
    if steps.is_empty() {
        pick["step_id"] = started.card_id.clone().into();
    }
    if let Some(recommendation) = recommendation {
        pick["recommendation"] = recommendation.data();
    }
    let choices: Vec<_> = if steps.is_empty() {
        attempts.iter().map(i64::to_string).collect()
    } else {
        (steps.iter().zip(attempts))
            .map(|(step, attempt)| format!("{step} attempt {attempt}"))
            .collect()
    };
    let mut request = event(
        "human_input_requested",
        &gate.id,
        0,
        json!({"gate_id":gate.id,"card_id":gate.id,"prompt":gate.criterion,"choices":choices,"requested_by":gate.id,"input_ref":input,"pick":pick}),
    );
    request.gate_id = gate.id.clone();
    request
}

/// What `advance` did with a pick.
pub(crate) enum PickAction {
    None,
    Acted,
    /// The gate's judge chain picks on a thread of its own.
    Judging(String),
}

/// Go on with the first unsettled pick whose gate is not busy: the only
/// candidate is taken without a picker; otherwise the gate's judge chain
/// picks, on a thread, and a human gate asks the operator.
pub(crate) fn advance(
    context: &crate::run_gate::GateContext<'_>,
    running: &std::collections::HashSet<String>,
) -> io::Result<PickAction> {
    let crate::run_gate::GateContext {
        c,
        id,
        control,
        mission,
        events,
        ..
    } = *context;
    if gather(context, running)? {
        return Ok(PickAction::Acted);
    }
    for started in unsettled(events) {
        if running.contains(&format!("gate:{}", started.gate_id)) {
            continue;
        }
        let gate = mission.gate(&started.gate_id).ok_or_else(|| {
            invalid(&format!(
                "pick {} names gate {:?}",
                started.seq, started.gate_id
            ))
        })?;
        let details = details(c, id, events, started)?;
        if let [only] = details.as_slice() {
            let choice = Choice {
                step_id: only.step.clone(),
                attempt: only.attempt,
                reason: format!("the only attempt {} judged", mission.card_name(&gate.id)),
                evidence: Vec::new(),
                decided_by: DECIDED_BY_AHJO.into(),
            };
            resolve(c, id, control, mission, started, &choice, None)?;
            return Ok(PickAction::Acted);
        }
        let input = pick_input(id, mission, gate, started, &details);
        let keys = keys(started, &details);
        if gate.kinds.contains(&ahjo_mission_model::GateKind::Step) {
            if crate::graph::judge_chain(mission, &gate.id).is_empty() {
                return Err(invalid(&format!("gate {:?} has no judge chain", gate.id)));
            }
            spawn_judges(context, gate.clone(), started.clone(), input, keys);
            return Ok(PickAction::Judging(gate.id.clone()));
        }
        if !gate.kinds.contains(&ahjo_mission_model::GateKind::Human) {
            return Err(invalid(&format!(
                "gate {:?} has no judge or human to pick",
                gate.id
            )));
        }
        let request = ask(gate, started, &input, (&keys.0, &keys.1), None);
        crate::run_worker::append_live(c, id, control, request)?;
        return Ok(PickAction::Acted);
    }
    Ok(PickAction::None)
}

fn spawn_judges(
    context: &crate::run_gate::GateContext<'_>,
    gate: Gate,
    started: RunEvent,
    input: RoutedInput,
    keys: (Vec<i64>, Vec<String>),
) {
    let (c, id, mission, req, workspace, control, inputs, done, wake) = (
        context.c.clone(),
        context.id.to_owned(),
        context.mission.clone(),
        context.req.clone(),
        context.workspace.to_path_buf(),
        context.control.clone(),
        context.inputs.to_vec(),
        context.done.clone(),
        context.wake.clone(),
    );
    std::thread::spawn(move || {
        let judged = Judging {
            c: &c,
            id: &id,
            mission: &mission,
            req: &req,
            workspace: &workspace,
            control: &control,
            inputs: &inputs,
        };
        let result = judged.pick(&gate, &started, input, (&keys.0, &keys.1));
        let _ = done.send(crate::run_scheduler::Finished::Pick(
            gate.id.clone(),
            result,
        ));
        let _ = wake.send(());
    });
}

struct Judging<'a> {
    c: &'a Coordinator,
    id: &'a str,
    mission: &'a Mission,
    req: &'a RunStart,
    workspace: &'a std::path::Path,
    control: &'a Control,
    inputs: &'a [ahjo_mission_model::RunInput],
}

impl Judging<'_> {
    /// Run the gate's judge chain to pick: every judge reads the attempts
    /// side by side, the judges before the last recommend and the last picks.
    /// Judge outputs the ledger holds since `pick_started` are reused, except
    /// the final judge's after its malformed pick blocked the run.
    fn pick(
        &self,
        gate: &Gate,
        started: &RunEvent,
        input: RoutedInput,
        (attempts, steps): (&[i64], &[String]),
    ) -> io::Result<()> {
        let Judging {
            c,
            id,
            mission,
            control,
            ..
        } = *self;
        let chain = crate::graph::judge_chain(mission, &gate.id);
        let ledger = c.ledger(id)?.read().map_err(crate::error::to_io)?;
        let invalidated = ledger.iter().any(|e| {
            e.seq > started.seq
                && e.kind == "run_blocked"
                && e.gate_id == gate.id
                && e.data.get("code").and_then(Value::as_str) == Some(INVALID_JUDGE_PICK)
        });
        let mut current = input;
        let count = chain.len();
        let mut final_text = String::new();
        for (index, judge) in chain.into_iter().enumerate() {
            let last = index + 1 == count;
            let events = c.ledger(id)?.read().map_err(crate::error::to_io)?;
            let previous = (!(invalidated && last))
                .then(|| {
                    events.iter().rev().find(|e| {
                        e.seq > started.seq
                            && e.kind == "card_output"
                            && e.card_id == judge.id
                            && e.data.get("reason").and_then(Value::as_str) == Some("judge")
                    })
                })
                .flatten();
            let (text, outputs) = if let Some(output) = previous {
                (
                    output
                        .data
                        .get("text")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned(),
                    output.data.get("outputs").cloned().unwrap_or_default(),
                )
            } else {
                let attempt = (events.iter())
                    .filter(|e| e.kind == "card_started" && e.card_id == judge.id)
                    .map(|e| e.attempt)
                    .max()
                    .unwrap_or(0)
                    + 1;
                let result = crate::run_job::execute_step(
                    c,
                    id,
                    mission,
                    self.req,
                    self.workspace,
                    control,
                    judge.id.clone(),
                    attempt,
                    vec![serde_json::to_value(&current)?],
                    current.text.clone(),
                    self.inputs,
                    "judge",
                    last,
                    None,
                    (attempts.to_vec(), steps.to_vec()),
                )?;
                crate::run_judges::record_output(
                    c,
                    id,
                    self.req,
                    self.workspace,
                    control,
                    &judge.id,
                    attempt,
                    &result,
                )?;
                (result.text.clone(), Value::Object(result.outputs))
            };
            if last {
                final_text = text;
                break;
            }
            let recommendation = (judge.outputs.first())
                .and_then(|port| outputs.get(&port.id))
                .and_then(|payload| {
                    let text = payload
                        .get("text")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .trim();
                    let kind = payload.get("kind").and_then(Value::as_str).unwrap_or("");
                    let reference = payload.get("ref").and_then(Value::as_str).unwrap_or("");
                    if !text.is_empty() {
                        Some(text.to_owned())
                    } else if (kind == "file" || kind == "folder") && !reference.is_empty() {
                        Some(format!("{kind}: {reference}"))
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| text.trim().to_owned());
            current.text += &format!("\nrecommendation from {}:\n{recommendation}\n", judge.title);
        }
        let judge = crate::graph::judge_chain(mission, &gate.id)
            .last()
            .map(|judge| judge.id.clone())
            .unwrap_or_default();
        match parse_pick(&final_text, attempts, steps) {
            Ok(mut choice) => {
                choice.decided_by = judge;
                if gate.kinds.contains(&ahjo_mission_model::GateKind::Human) {
                    let events = c.ledger(id)?.read().map_err(crate::error::to_io)?;
                    let details = details(c, id, &events, started)?;
                    let input = pick_input(id, mission, gate, started, &details);
                    let request = ask(gate, started, &input, (attempts, steps), Some(&choice));
                    crate::run_worker::append_live(c, id, control, request).map(|_| ())
                } else {
                    resolve(c, id, control, mission, started, &choice, None)
                }
            }
            Err(error) => {
                let events = c.ledger(id)?.read().map_err(crate::error::to_io)?;
                let mut failed = event(
                    "judge_attempt_failed",
                    &gate.id,
                    gate_attempt(&events, &gate.id),
                    json!({"code":INVALID_JUDGE_PICK,"reason":error}),
                );
                failed.gate_id = gate.id.clone();
                crate::run_worker::append_live(c, id, control, failed)?;
                Err(io::Error::other(format!(
                    "{INVALID_JUDGE_PICK}[{}]: {error}",
                    gate.id
                )))
            }
        }
    }
}

/// Route the operator's recorded pick of a pending pick request.
pub(crate) fn route_human(
    c: &Coordinator,
    id: &str,
    control: &Control,
    mission: &Mission,
    events: &[RunEvent],
    request: &RunEvent,
    recorded: &RunEvent,
) -> io::Result<()> {
    let seq = pick_seq(request);
    let started = (events.iter())
        .find(|e| {
            e.seq == seq
                && e.kind == PICK_STARTED
                && e.gate_id == request.gate_id
                && seq < request.seq
        })
        .ok_or_else(|| {
            invalid(&format!(
                "gate {:?} pick request names invalid pick {seq}",
                request.gate_id
            ))
        })?;
    let picked = recorded.data.get("pick");
    let choice = Choice {
        step_id: (picked.and_then(|pick| pick.get("step_id")))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned(),
        attempt: (picked.and_then(|pick| pick.get("attempt")))
            .and_then(Value::as_i64)
            .unwrap_or(0),
        reason: text(&Value::Object(recorded.data.clone()), "reason").into(),
        evidence: Vec::new(),
        decided_by: text(&Value::Object(recorded.data.clone()), "decided_by").into(),
    };
    resolve(
        c,
        id,
        control,
        mission,
        started,
        &choice,
        Some((request, recorded)),
    )
}

/// The candidates a pending human pick request asks among, each as its step
/// and attempt; `None` when the request asks for a verdict.
pub(crate) fn request_candidates(request: &RunEvent) -> Option<Vec<(String, i64)>> {
    let pick = request.data.get("pick")?;
    let step = pick.get("step_id").and_then(Value::as_str).unwrap_or("");
    Some(
        (pick.get("candidates").and_then(Value::as_array).into_iter())
            .flatten()
            .filter_map(|candidate| match candidate.as_i64() {
                Some(attempt) => Some((step.to_owned(), attempt)),
                None => Some((
                    candidate.get("step_id")?.as_str()?.to_owned(),
                    candidate.get("attempt")?.as_i64()?,
                )),
            })
            .collect(),
    )
}

impl Coordinator {
    /// Record the operator's pick on a gate's pending pick. Like a verdict it
    /// routes nothing: the run's worker routes it, so a pick is accepted
    /// while other seats work, and on a blocked run it waits for the resume.
    pub fn record_pick(
        &self,
        run: &str,
        gate: &str,
        pick: HumanPick,
    ) -> Result<RunProjection, crate::Error> {
        (|| -> io::Result<RunProjection> {
        let relay = pick.relayed_by.as_bytes();
        if pick.requested_seq == 0
            || (!relay.is_empty()
                && (relay.len() > 64
                    || !relay[0].is_ascii_alphanumeric()
                    || !relay
                        .iter()
                        .all(|c| c.is_ascii_alphanumeric() || *c == b'_' || *c == b'-')))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "requested_seq, an attempt and valid relayed_by required",
            ));
        }
        let control = self.inner.controls.lock().unwrap().get(run).cloned();
        let guard = control.as_ref().map(|control| control.done.lock().unwrap());
        if control
            .as_ref()
            .is_some_and(|control| control.canceled.load(std::sync::atomic::Ordering::Acquire))
        {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "run is final or aborting",
            ));
        }
        let actor = if pick.actor.is_empty() {
            "human:operator".to_owned()
        } else {
            pick.actor.clone()
        };
        let mut data = json!({"gate_id":gate,"card_id":gate,"verdict":"pass","reason":pick.reason,"requested_seq":pick.requested_seq,"decided_by":actor,"pick":{"attempt":pick.attempt}});
        if !pick.step.is_empty() {
            data["pick"]["step_id"] = pick.step.clone().into();
        }
        if !pick.relayed_by.is_empty() {
            data["relayed_by"] = pick.relayed_by.clone().into();
        }
        let event = RunEvent {
            kind: "human_verdict_recorded".into(),
            gate_id: gate.into(),
            card_id: gate.into(),
            timestamp: crate::run_storage::now(),
            actor: actor.clone(),
            data: data.as_object().unwrap().clone(),
            ..Default::default()
        };
        let appended = self.ledger(run)?.append_checked(event, |events| {
            let projection = crate::project_run(events, run).map_err(crate::error::to_io)?;
            if projection.r#final
                || !projection.waiting_gates.iter().any(|pending| {
                    pending.gate_id == gate && pending.requested_seq == pick.requested_seq
                })
            {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "human gate request is no longer pending",
                ));
            }
            let request = (events.iter())
                .find(|e| e.seq == pick.requested_seq)
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "human gate request"))?;
            let Some(candidates) = request_candidates(request) else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "this gate waits for a verdict, not a pick: answer it with ahjo run approve or ahjo run reject",
                ));
            };
            let steps: Vec<String> = candidates.iter().map(|(step, _)| step.clone()).collect();
            let attempts: Vec<i64> = candidates.iter().map(|(_, attempt)| *attempt).collect();
            let one_step = steps.iter().all(|step| *step == steps[0]);
            if one_step {
                if !attempts.contains(&pick.attempt)
                    || (!pick.step.is_empty() && steps.first() != Some(&pick.step))
                {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!(
                            "not a candidate attempt: attempt {}; pick one of {}",
                            pick.attempt,
                            attempt_words(&attempts)
                        ),
                    ));
                }
            } else if !candidates.contains(&(pick.step.clone(), pick.attempt)) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "not a candidate: step {:?} attempt {}; pick one of {}, naming the step and its attempt",
                        pick.step,
                        pick.attempt,
                        candidate_words(&attempts, &steps)
                    ),
                ));
            }
            Ok(())
        })?;
        drop(guard);
        self.changed(run, appended.seq);
        if let Some(control) = self.inner.controls.lock().unwrap().get(run)
            && let Some(wake) = control.wake.lock().unwrap().as_ref()
        {
            let _ = wake.send(());
        }
        self.run(run).map_err(crate::error::to_io)
     })().map_err(crate::error::io)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_judge_pick_is_one_fenced_block_naming_a_candidate() {
        let block = |body: &str| format!("```ahjo-pick\n{body}\n```");
        let choice = parse_pick(
            &block(r#"{"attempt":2,"reason":"tests pass","evidence":["retry"]}"#),
            &[1, 2, 3],
            &[],
        )
        .unwrap();
        assert_eq!((choice.attempt, choice.reason.as_str()), (2, "tests pass"));
        assert_eq!(choice.evidence, ["retry"]);
        assert!(
            parse_pick(
                &block(r#"{"attempt":4,"reason":"","evidence":[]}"#),
                &[1, 2, 3],
                &[]
            )
            .unwrap_err()
            .contains("one of attempts 1, 2 or 3")
        );
        assert!(
            parse_pick(&block(r#"{"attempt":1,"reason":""}"#), &[1], &[])
                .unwrap_err()
                .contains("missing evidence")
        );
        assert!(
            parse_pick(
                &block(r#"{"attempt":1,"reason":"","evidence":[],"x":1}"#),
                &[1],
                &[]
            )
            .unwrap_err()
            .contains("extra key")
        );
        assert!(parse_pick("attempt 2 is best", &[1, 2], &[]).is_err());
    }

    #[test]
    fn a_gathered_pick_names_the_step_and_its_attempt() {
        let block = |body: &str| format!("```ahjo-pick\n{body}\n```");
        let steps = ["fmn_a".to_owned(), "fmn_b".to_owned()];
        let choice = parse_pick(
            &block(r#"{"step":"fmn_b","attempt":1,"reason":"tighter","evidence":[]}"#),
            &[1, 1],
            &steps,
        )
        .unwrap();
        assert_eq!((choice.step_id.as_str(), choice.attempt), ("fmn_b", 1));
        assert!(
            parse_pick(
                &block(r#"{"step":"fmn_b","attempt":2,"reason":"","evidence":[]}"#),
                &[1, 1],
                &steps
            )
            .unwrap_err()
            .contains("one of step fmn_a attempt 1 or step fmn_b attempt 1")
        );
        assert!(
            parse_pick(
                &block(r#"{"attempt":1,"reason":"","evidence":[]}"#),
                &[1, 1],
                &steps
            )
            .unwrap_err()
            .contains("missing step")
        );
        assert!(contract(true, &[1, 1], &steps).contains(
            "step and attempt must name one of step fmn_a attempt 1 or step fmn_b attempt 1"
        ));
    }

    #[test]
    fn contracts_name_the_candidates() {
        assert!(
            contract(true, &[1, 2, 3], &[]).contains("attempt must be one of attempts 1, 2 or 3")
        );
        assert!(contract(false, &[1, 2], &[]).starts_with("judge recommend contract:"));
    }
}
