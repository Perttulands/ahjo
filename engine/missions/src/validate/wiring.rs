//! Wires and routes: every connection joins existing ports, each input has
//! one producer, every gate can route, every route leads somewhere, and the
//! Input card reaches the rest.

use std::collections::{HashMap, HashSet};

use super::possessive;
use crate::code_check::{code_check, code_check_names};
use crate::graph::{self, Direction};
use ahjo_mission_model::{
    Code, Finding, GATE_FAIL, GATE_PASS, Gate, GateKind, HumanChannel, Mission,
};

pub fn connection_findings(mission: &Mission) -> Vec<Finding> {
    let mut findings = Vec::new();
    for connection in &mission.connections {
        if !graph::endpoint_allows(mission, &connection.from, Direction::Output) {
            findings.push(Finding::new(
                Code::DanglingConnection,
                &connection.id,
                format!(
                    "connection {:?} has a broken 'from' endpoint {:?}: it does not reference an existing node output",
                    connection.id, connection.from
                ),
            ));
        }
        if !graph::endpoint_allows(mission, &connection.to, Direction::Input) {
            findings.push(Finding::new(
                Code::DanglingConnection,
                &connection.id,
                format!(
                    "connection {:?} has a broken 'to' endpoint {:?}: it does not reference an existing node input",
                    connection.id, connection.to
                ),
            ));
        }
    }
    // A gate's fail route into a fed input is the pushback route, and an End
    // takes any number of routes; neither counts as a second producer.
    let mut producers: HashMap<&str, &str> = HashMap::new();
    for connection in &mission.connections {
        if graph::is_gate_fail(mission, &connection.from)
            || graph::is_end_input(mission, &connection.to)
        {
            continue;
        }
        if let Some(first) = producers.get(connection.to.as_str()) {
            findings.push(Finding::new(
                Code::DuplicateInputProducer,
                &connection.id,
                format!(
                    "connection {:?} is a second producer for input {:?}; it is already produced by connection {first:?}",
                    connection.id, connection.to
                ),
            ));
            continue;
        }
        producers.insert(&connection.to, &connection.id);
    }
    findings
}

pub fn gate_findings(mission: &Mission) -> Vec<Finding> {
    (mission.gates.iter())
        .filter_map(|gate| {
            let gaps = route_gaps(mission, gate);
            (!gaps.is_empty()).then(|| {
                Finding::new(
                    Code::GateNotRoutable,
                    &gate.id,
                    format!("gate {:?} needs {}", gate.id, gaps.join("; ")),
                )
            })
        })
        .collect()
}

/// What a gate still needs before a run can route through it.
fn route_gaps(mission: &Mission, gate: &Gate) -> Vec<String> {
    if gate.kinds.is_empty() {
        return vec!["at least one kind".to_owned()];
    }
    let mut gaps = Vec::new();
    let mut seen = HashSet::new();
    for kind in &gate.kinds {
        if !seen.insert(*kind) {
            continue;
        }
        match kind {
            GateKind::Code => gaps.extend(code_check_gap(gate)),
            GateKind::Step if graph::judge_chain(mission, &gate.id).is_empty() => {
                gaps.push(
                    "a judge chain wired from its judge port through a step and back".to_owned(),
                );
            }
            GateKind::Step | GateKind::Human => {}
        }
    }
    gaps
}

/// Names the missing or malformed part of a gate's code check.
fn code_check_gap(gate: &Gate) -> Option<String> {
    let (check, version) = (gate.check.trim(), gate.check_version.trim());
    let registered = code_check_names().join(" or ");
    if check.is_empty() && version.is_empty() {
        return Some(format!("a code check ({registered})"));
    }
    let Some(profile) = code_check(check, version) else {
        return Some(if version.is_empty() {
            format!("a version for code check {check:?}")
        } else if check.is_empty() {
            format!("a code check for version {version:?}")
        } else {
            format!("a registered code check in place of unknown {check}@{version} ({registered})")
        });
    };
    gate.check_value.trim().is_empty().then(|| {
        format!(
            "{} for code check {}@{}",
            profile.parameter.to_lowercase(),
            profile.id,
            profile.version
        )
    })
}

/// Every step output and gate pass or fail route without a wire.
pub fn route_findings(mission: &Mission) -> Vec<Finding> {
    let wired: HashSet<&str> = mission
        .connections
        .iter()
        .map(|c| c.from.as_str())
        .collect();
    let mut findings = Vec::new();
    let mut add = |node: &str, route: String| {
        findings.push(Finding::new(
            Code::RouteLeadsNowhere,
            node,
            format!(
                "{} {route} leads nowhere: wire it to a step or an End node",
                possessive(mission.node_name(node))
            ),
        ));
    };
    for step in &mission.steps {
        for port in &step.outputs {
            if !wired.contains(format!("{}:{}", step.id, port.id).as_str()) {
                let route = if step.outputs.len() > 1 {
                    format!("{:?} output", port.name())
                } else {
                    "output".to_owned()
                };
                add(&step.id, route);
            }
        }
    }
    for gate in &mission.gates {
        for port in [GATE_PASS, GATE_FAIL] {
            if !wired.contains(format!("{}:{port}", gate.id).as_str()) {
                add(&gate.id, format!("{port} route"));
            }
        }
    }
    findings
}

/// Every input of a step a run reaches that no wire feeds. A step runs once
/// each of its inputs holds work, so a run that reaches it would wait there
/// and stop. A step no run reaches is warned of as unreachable instead.
pub fn unfed_input_findings(mission: &Mission) -> Vec<Finding> {
    let fed: HashSet<&str> = mission.connections.iter().map(|c| c.to.as_str()).collect();
    let reached: HashSet<String> = (mission.input_cards.iter())
        .flat_map(|card| graph::reachable(mission, &card.id))
        .collect();
    let mut findings = Vec::new();
    for step in mission.steps.iter().filter(|f| reached.contains(&f.id)) {
        for port in &step.inputs {
            if !fed.contains(format!("{}:{}", step.id, port.id).as_str()) {
                findings.push(Finding::new(
                    Code::InputWithoutProducer,
                    &step.id,
                    format!(
                        "{} input {:?} ({}) has no wire into it, so a run that reaches the step \
                         waits there and stops; wire an earlier step's output to it or remove the input",
                        possessive(mission.node_name(&step.id)),
                        port.name(),
                        port.id
                    ),
                ));
            }
        }
    }
    findings
}

/// One Input card per mission, on a channel the operator can be reached by.
pub fn input_card_findings(mission: &Mission) -> Vec<Finding> {
    let mut findings = Vec::new();
    match mission.input_cards.len() {
        0 => findings.push(Finding::new(
            Code::MissionCount,
            "",
            "a mission needs its Input card to run; add one on the canvas".to_owned(),
        )),
        1 => {}
        count => {
            let cards: Vec<_> = (mission.input_cards.iter())
                .map(|card| format!("{:?} ({})", card.title, card.id))
                .collect();
            findings.push(Finding::new(
                Code::SeveralInputCards,
                "",
                format!(
                    "mission {slug:?} holds {count} Input cards, {}; a mission has one, so no run can start from it. \
                     Split it: copy {slug}.mission.toml beside itself under a new slug and give the copy a new id, slug and title, \
                     then delete from each file the Input cards, and the steps only they reach, that belong to the other",
                    cards.join(", "),
                    slug = mission.slug,
                ),
            ));
        }
    }
    findings
}

/// Human gates reached from a session-channel Input card cannot be served.
pub fn unsupported_human_channels(mission: &Mission) -> Vec<Finding> {
    let reached: std::collections::HashSet<String> = mission
        .input_cards
        .iter()
        .filter(|card| card.human_channel == HumanChannel::Session)
        .flat_map(|card| graph::reachable(mission, &card.id))
        .collect();
    mission.gates.iter().filter(|gate| reached.contains(&gate.id) && gate.kinds.contains(&GateKind::Human))
        .map(|gate| Finding::new(Code::UnsupportedHumanChannel, &gate.id, format!("human gate {:?} uses a session channel, which is not supported; set the Input card channel to notify", gate.id)))
        .collect()
}

/// An Input card with no wire out cannot start a run.
pub fn not_runnable_findings(mission: &Mission) -> Vec<Finding> {
    (mission.input_cards.iter())
        .filter(|card| graph::outgoing(mission, &card.id).next().is_none())
        .map(|card| {
            Finding::new(
                Code::MissionNotRunnable,
                &card.id,
                format!(
                    "Input card {:?} has no outgoing connection, so it cannot start a run; wire it to a first step",
                    card.id
                ),
            )
        })
        .collect()
}

/// Every step, gate and End node no path from the Input card reaches. A
/// judge is reached through its gate's judge port.
pub fn unreachable_findings(mission: &Mission) -> Vec<Finding> {
    if mission.input_cards.is_empty() {
        return Vec::new();
    }
    let reached: HashSet<String> = (mission.input_cards.iter())
        .flat_map(|card| graph::reachable(mission, &card.id))
        .collect();
    let nodes = (mission.steps.iter().map(|n| (&n.id, "step")))
        .chain(mission.gates.iter().map(|n| (&n.id, "gate")))
        .chain(mission.ends.iter().map(|n| (&n.id, "End node")));
    nodes
        .filter(|(id, _)| !reached.contains(id.as_str()))
        .map(|(id, kind)| {
            Finding::new(
                Code::UnreachableNode,
                id,
                format!(
                    "No path from the Input card reaches {kind} {}, so no run will get there; wire a route into it or delete it",
                    mission.node_name(id)
                ),
            )
        })
        .collect()
}
