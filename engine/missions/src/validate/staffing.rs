//! Staffing: every seat states a harness and effort a seat can start with,
//! and any agent it names is available.

use crate::AgentStore;
use crate::harness::Harnesses;
use ahjo_mission_model::{Code, Finding, Mission};

pub fn findings(mission: &Mission, agents: &AgentStore, harnesses: &Harnesses) -> Vec<Finding> {
    let mut findings = Vec::new();
    for step in &mission.steps {
        let id = &step.id;
        let mut add = |code, message| findings.push(Finding::new(code, id, message));
        for seat in &step.seats {
            let name = seat.name();
            if !seat.staffed() {
                add(
                    Code::UnstaffedSeat,
                    format!(
                        "step {id:?} seat {name} needs a harness and effort, and optionally an agent"
                    ),
                );
                continue;
            }
            let agent = if seat.agent.is_empty() {
                None
            } else {
                match agents.read(&seat.agent) {
                    Ok(agent) => Some(agent),
                    Err(error) => {
                        let agent = &seat.agent;
                        let (code, message) = if error.code == "not_found" {
                            if agents
                                .dir()
                                .join("archive")
                                .join(format!("{agent}.toml"))
                                .is_file()
                            {
                                (
                                    Code::RetiredPersona,
                                    format!(
                                        "uses archived agent {agent:?}; restaff it or restore the agent file"
                                    ),
                                )
                            } else {
                                (
                                    Code::UnavailablePersona,
                                    format!("names unknown agent {agent:?}"),
                                )
                            }
                        } else {
                            (
                                Code::UnavailablePersona,
                                format!("cannot read agent {agent:?}: {error}"),
                            )
                        };
                        add(code, format!("step {id:?} seat {name} {message}"));
                        // A missing definition only prevents checking settings it would supply.
                        if seat.harness.is_unset() || seat.effort.is_unset() {
                            continue;
                        }
                        None
                    }
                }
            };
            let settings = seat.settings(agent.as_ref());
            let subject = format!("seat {name}");
            if let Err(problem) = harnesses.check(
                &subject,
                settings.harness.as_str(),
                &settings.model,
                settings.effort.as_str(),
            ) {
                add(
                    Code::InvalidSeatSettings,
                    format!("step {id:?} {}", problem.message),
                );
            }
        }
    }
    findings
}
