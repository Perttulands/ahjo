//! Staffing: every slot states a harness and effort a seat can start with,
//! and any agent it names is available.

use crate::AgentStore;
use crate::harness::Harnesses;
use ahjo_mission_model::{Code, Finding, Mission};

pub fn findings(mission: &Mission, agents: &AgentStore, harnesses: &Harnesses) -> Vec<Finding> {
    let mut findings = Vec::new();
    for step in &mission.steps {
        let id = &step.id;
        let mut add = |code, message| findings.push(Finding::new(code, id, message));
        for slot in &step.slots {
            let name = slot.name();
            if !slot.staffed() {
                add(
                    Code::UnstaffedSlot,
                    format!(
                        "step {id:?} slot {name} needs a harness and effort, and optionally an agent"
                    ),
                );
                continue;
            }
            // What the seat runs comes first, so a harness no seat can start
            // is named whatever role the slot has.
            let subject = format!("slot {name}");
            if let Err(problem) = harnesses.check(
                &subject,
                slot.harness.as_str(),
                &slot.model,
                slot.effort.as_str(),
            ) {
                add(
                    Code::InvalidSlotSettings,
                    format!("step {id:?} {}", problem.message),
                );
                continue;
            }
            if slot.agent_id.is_empty() {
                continue;
            }
            let agent_id = &slot.agent_id;
            if let Err(error) = agents.read(agent_id) {
                let (code, message) = if error.code == "not_found" {
                    if agents
                        .dir()
                        .join("archive")
                        .join(format!("{agent_id}.toml"))
                        .is_file()
                    {
                        (
                            Code::RetiredPersona,
                            format!(
                                "uses archived agent {agent_id:?}; restaff it or restore the agent file"
                            ),
                        )
                    } else {
                        (
                            Code::UnavailablePersona,
                            format!("names unknown agent {agent_id:?}"),
                        )
                    }
                } else {
                    (
                        Code::UnavailablePersona,
                        format!("cannot read agent {agent_id:?}: {error}"),
                    )
                };
                add(code, format!("step {id:?} slot {name} {message}"));
            }
        }
    }
    findings
}
