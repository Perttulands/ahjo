//! Step types, their seat shapes, and seat ids.

use super::possessive;
use ahjo_mission_model::{Code, Finding, Formation, Mission, Step};

/// What a seat id may be, for messages: its seat's session is named after
/// it, and a relayed verdict names it.
const SEAT_ID_RULE: &str =
    "a letter or digit, then up to 63 letters, digits, underscores or hyphens";

pub fn valid_seat_id(id: &str) -> bool {
    let mut chars = id.chars();
    id.len() <= 64
        && chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Whether `environment` has a machine name and environment kind.
pub fn valid_environment(environment: &str) -> bool {
    environment
        .split_once('/')
        .is_some_and(|(name, environment)| {
            !name.is_empty()
                && !name.contains(char::is_whitespace)
                && ["linux", "wsl", "windows"].contains(&environment)
        })
}

/// A step environment names its machine and the environment kind.
pub fn environment_findings(mission: &Mission) -> Vec<Finding> {
    mission
        .steps
        .iter()
        .filter(|step| !step.environment.is_empty() && !valid_environment(&step.environment))
        .map(|step| {
            Finding::new(
                Code::UnknownEnvironment,
                &step.id,
                format!(
                    "step {:?} runs on {:?}; name an environment as <machine>/<linux|wsl|windows>, or clear it to run on the coordinator",
                    step.id, step.environment
                ),
            )
        })
        .collect()
}

pub fn shape_findings(mission: &Mission) -> Vec<Finding> {
    let mut findings = Vec::new();
    for step in &mission.steps {
        if step.formation == Formation::Peer {
            findings.push(Finding::new(
                Code::UnsupportedStep,
                &step.id,
                format!(
                    "peer step {:?} is not supported; use solo or orchestrated",
                    step.id
                ),
            ));
        }
        if step.formation == Formation::Unset {
            findings.push(Finding::new(
                Code::InvalidStepFormation,
                &step.id,
                format!("step {:?} needs a formation", step.id),
            ));
        } else {
            findings.extend(seat_shape_finding(step));
        }
    }
    findings
}

/// Solo has one seat; peer has two or more and no controller; orchestrated
/// has one controller and a worker beside it.
fn seat_shape_finding(step: &Step) -> Option<Finding> {
    let id = &step.id;
    let problem = |code, message| Some(Finding::new(code, id, message));
    let seats = step.seats.len();
    if seats == 0 {
        return problem(
            Code::StepWithoutSeats,
            format!("step {id:?} has no seats; add a seat and staff it"),
        );
    }
    let controllers = step.seats.iter().filter(|seat| seat.controller).count();
    match step.formation {
        Formation::Solo if seats != 1 => problem(
            Code::InvalidStepShape,
            format!("solo step {id:?} needs exactly one seat; it has {seats}"),
        ),
        Formation::Peer if seats < 2 || controllers != 0 => problem(
            Code::InvalidStepShape,
            format!(
                "peer step {id:?} needs at least two seats and no controllers; it has {seats} seats and {controllers} controllers"
            ),
        ),
        Formation::Orchestrated if controllers != 1 => problem(
            Code::OrchestratedController,
            format!(
                "orchestrated step {id:?} needs exactly one controller seat; it has {controllers}"
            ),
        ),
        Formation::Orchestrated if seats < 2 => problem(
            Code::OrchestratedController,
            format!("orchestrated step {id:?} needs a worker seat beside its controller"),
        ),
        _ => None,
    }
}

/// A seat id used by more than one seat, and one that breaks the seat id
/// rule. Two seats with one id would share a session name, and a verdict
/// relayed by that seat would name neither.
pub fn seat_id_findings(mission: &Mission) -> Vec<Finding> {
    let mut holders: Vec<(&str, Vec<&str>)> = Vec::new();
    for step in &mission.steps {
        for seat in mission_seats(step) {
            match holders.iter_mut().find(|(id, _)| *id == seat) {
                Some((_, steps)) => steps.push(&step.id),
                None => holders.push((seat, vec![&step.id])),
            }
        }
    }
    let mut findings = Vec::new();
    for (seat, steps) in holders.iter().filter(|(_, steps)| steps.len() > 1) {
        let mut cards: Vec<&str> = Vec::new();
        for card in steps {
            if !cards.contains(card) {
                cards.push(card);
            }
        }
        let described = match cards.as_slice() {
            [only] => format!("{} seats of step {only:?}", steps.len()),
            _ => {
                let quoted: Vec<_> = cards.iter().map(|card| format!("{card:?}")).collect();
                format!("steps {}", quoted.join(" and "))
            }
        };
        for card in &cards {
            findings.push(Finding::new(
                Code::DuplicateSeatId,
                card,
                format!("seat id {seat:?} is used by {described}; give every seat in the mission its own id"),
            ));
        }
    }
    for step in &mission.steps {
        for seat in mission_seats(step).filter(|seat| !valid_seat_id(seat)) {
            findings.push(Finding::new(
                Code::InvalidSeatId,
                &step.id,
                format!(
                    "{} seat id {seat:?} is not a seat id Ahjo can use: {SEAT_ID_RULE}; rename it in the mission file",
                    possessive(mission.card_name(&step.id))
                ),
            ));
        }
    }
    findings
}

/// A step's non-blank seat ids.
fn mission_seats(step: &Step) -> impl Iterator<Item = &str> {
    step.seats
        .iter()
        .map(|seat| seat.id.as_str())
        .filter(|id| !id.is_empty())
}

#[cfg(test)]
mod tests {
    use super::valid_seat_id;

    #[test]
    fn seat_ids_follow_the_rule() {
        assert!(valid_seat_id("w1"));
        assert!(valid_seat_id(&format!("a{}", "b".repeat(63))));
        assert!(!valid_seat_id(&format!("a{}", "b".repeat(64))));
        assert!(!valid_seat_id("_w"));
        assert!(!valid_seat_id("worker one"));
        assert!(!valid_seat_id("työ"));
    }
}
