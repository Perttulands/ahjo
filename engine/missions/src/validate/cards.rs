//! Card ids End cards and Limit cards share with other cards, and End
//! outcomes.

use std::collections::HashMap;

use ahjo_mission_model::{Code, EndOutcome, Finding, Mission};

pub fn findings(mission: &Mission) -> Vec<Finding> {
    let mut seen: HashMap<&str, &str> = HashMap::new();
    let cards = mission
        .input_cards
        .iter()
        .map(|n| (n.id.as_str(), "Mission"));
    let steps = mission.steps.iter().map(|n| (n.id.as_str(), "Step"));
    let gates = mission.gates.iter().map(|n| (n.id.as_str(), "Gate"));
    seen.extend(cards.chain(steps).chain(gates));

    let mut findings = Vec::new();
    for end in &mission.ends {
        findings.extend(claim(&mut seen, &end.id, "End", "End card"));
        if end.outcome == EndOutcome::Unset {
            findings.push(Finding::new(
                Code::InvalidEnd,
                &end.id,
                format!("End card {} needs an outcome", mission.card_name(&end.id)),
            ));
        }
    }
    for limit in &mission.limits {
        findings.extend(claim(&mut seen, &limit.id, "Limit", "Limit card"));
    }
    findings
}

/// Claims a card id for an End card or Limit card, or says whose it is.
fn claim<'a>(
    seen: &mut HashMap<&'a str, &'static str>,
    id: &'a str,
    kind: &'static str,
    noun: &str,
) -> Option<Finding> {
    if id.is_empty() {
        return None;
    }
    if let Some(first) = seen.get(id) {
        return Some(Finding::new(
            Code::DuplicateCardId,
            id,
            format!("{noun} id {id:?} duplicates an existing {first} card id"),
        ));
    }
    seen.insert(id, kind);
    None
}
