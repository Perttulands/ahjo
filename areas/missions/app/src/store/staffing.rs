//! Clamp seat choices to the catalog published by the node's agent library.
use ahjo_harness_contract::agents::ListedAgents;
use ahjo_missions_contract::{Agent, Effort, Harness, HarnessChoices, MissionChange, Seat};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Staffing {
    pub agent: String,
    pub harness: Harness,
    pub model: String,
    pub effort: Effort,
}

impl From<&Seat> for Staffing {
    fn from(seat: &Seat) -> Self {
        Self {
            agent: seat.agent.clone(),
            harness: seat.harness,
            model: seat.model.clone(),
            effort: seat.effort,
        }
    }
}

impl Staffing {
    pub fn assign(self, step_id: &str, seat_id: &str) -> MissionChange {
        MissionChange::StaffSeat {
            step_id: step_id.into(),
            seat_id: seat_id.into(),
            agent: self.agent,
            harness: self.harness,
            model: self.model,
            effort: self.effort,
        }
    }
}

fn harness_choices(catalog: &ListedAgents, harness: Harness) -> Option<&HarnessChoices> {
    catalog.harnesses.iter().find(|choice| choice.id == harness)
}

fn agent<'a>(catalog: &'a ListedAgents, staff: &Staffing) -> Option<&'a Agent> {
    catalog.agents.iter().find(|agent| agent.id == staff.agent)
}

fn settings(staff: &Staffing, agent: Option<&Agent>) -> ahjo_missions_contract::SeatSettings {
    Seat {
        agent: staff.agent.clone(),
        harness: staff.harness,
        model: staff.model.clone(),
        effort: staff.effort,
        ..Default::default()
    }
    .settings(agent)
}

/// A picker choice retains the machine value separately from its label.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub id: String,
    pub label: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Choices {
    pub agents: Vec<Choice>,
    pub harnesses: Vec<Choice>,
    pub models: Vec<Choice>,
    pub efforts: Vec<Choice>,
}

fn choice(id: &str, label: &str) -> Choice {
    Choice {
        id: id.into(),
        label: label.into(),
    }
}
fn with_current(mut choices: Vec<Choice>, id: &str, label: String) -> Vec<Choice> {
    if !choices.iter().any(|c| c.id == id) {
        choices.push(Choice {
            id: id.into(),
            label,
        });
    }
    choices
}

/// Catalog order follows the original staffing picker: vanilla then active
/// agents by name; the existing value always remains available, even if the
/// node no longer offers it. Models and efforts follow the chosen harness.
pub fn choices(catalog: Option<&ListedAgents>, staff: &Staffing) -> Choices {
    use super::words::{agent_name, harness_name};
    let empty = ListedAgents {
        node: String::new(),
        agents: vec![],
        unreadable: vec![],
        harnesses: vec![],
        effort_policy: vec![],
    };
    let c = catalog.unwrap_or(&empty);
    let mut agents: Vec<_> = c
        .agents
        .iter()
        .map(|agent| choice(&agent.id, &agent.name))
        .collect();
    agents.sort_by_key(|agent| agent.label.to_lowercase());
    agents.insert(0, choice("", "vanilla"));
    let current_agent = agent(c, staff);
    let effective = settings(staff, current_agent);
    let inherited_harness =
        current_agent.map_or("none".into(), |agent| harness_name(agent.harness.as_str()));
    let mut harnesses = vec![choice("", &format!("Agent default ({inherited_harness})"))];
    harnesses.extend(
        c.harnesses
            .iter()
            .map(|h| choice(h.id.as_str(), &harness_name(h.id.as_str()))),
    );
    let inherited_model = current_agent
        .filter(|agent| !agent.model.is_empty())
        .map_or("harness default", |agent| agent.model.as_str());
    let mut models_list = vec![
        choice("", &format!("Agent default ({inherited_model})")),
        choice(
            ahjo_missions_contract::HARNESS_DEFAULT_MODEL,
            "Harness default",
        ),
    ];
    let mut efforts = vec![choice(
        "",
        &format!(
            "Agent default ({})",
            current_agent.map_or("none", |agent| agent.effort.as_str())
        ),
    )];
    if let Some(harness) = harness_choices(c, effective.harness) {
        models_list.extend(
            harness
                .models
                .iter()
                .map(|model| choice(&model.id, &model.id)),
        );
        let supported = harness
            .models
            .iter()
            .find(|model| model.id == effective.model && !model.efforts.is_empty())
            .map_or(&harness.efforts, |model| &model.efforts);
        efforts.extend(
            supported
                .iter()
                .map(|effort| choice(effort.as_str(), effort.as_str())),
        );
    }
    Choices {
        agents: with_current(agents, &staff.agent, agent_name(&staff.agent)),
        harnesses: with_current(
            harnesses,
            staff.harness.as_str(),
            harness_name(staff.harness.as_str()),
        ),
        models: with_current(models_list, &staff.model, staff.model.clone()),
        efforts: with_current(
            efforts,
            staff.effort.as_str(),
            staff.effort.as_str().to_owned(),
        ),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use ahjo_missions_contract::HarnessModel;

    fn catalog() -> ListedAgents {
        ListedAgents {
            node: "scratch".into(),
            agents: vec![
                Agent {
                    id: "reviewer".into(),
                    name: "Zed".into(),
                    effort: Effort::Xhigh,
                    ..Default::default()
                },
                Agent {
                    id: "planner".into(),
                    name: "Alpha".into(),
                    ..Default::default()
                },
            ],
            unreadable: vec![],
            harnesses: vec![
                HarnessChoices {
                    id: Harness::ClaudeCode,
                    efforts: vec![
                        Effort::Low,
                        Effort::Medium,
                        Effort::High,
                        Effort::Xhigh,
                        Effort::Max,
                    ],
                    default_effort: Effort::Medium,
                    models: vec![HarnessModel {
                        id: "opus".into(),
                        efforts: vec![],
                    }],
                },
                HarnessChoices {
                    id: Harness::OpenAiCodex,
                    efforts: vec![
                        Effort::Low,
                        Effort::Medium,
                        Effort::High,
                        Effort::Xhigh,
                        Effort::Max,
                        Effort::Ultra,
                    ],
                    default_effort: Effort::Medium,
                    models: vec![HarnessModel {
                        id: "gpt-6".into(),
                        efforts: vec![Effort::Low, Effort::High],
                    }],
                },
            ],
            effort_policy: vec![],
        }
    }

    #[test]
    fn choices_show_inherited_values_and_keep_explicit_overrides() {
        let mut c = catalog();
        c.agents[1].harness = Harness::OpenAiCodex;
        c.agents[1].model = "gpt-6".into();
        c.agents[1].effort = Effort::High;
        let mut staff = Staffing {
            agent: "planner".into(),
            ..Default::default()
        };
        let inherited = choices(Some(&c), &staff);
        assert!(
            inherited
                .harnesses
                .iter()
                .any(|choice| choice.id.is_empty() && choice.label.contains("Codex"))
        );
        assert!(
            inherited
                .models
                .iter()
                .any(|choice| choice.id.is_empty() && choice.label.contains("gpt-6"))
        );
        assert!(
            inherited
                .efforts
                .iter()
                .any(|choice| choice.id.is_empty() && choice.label.contains("high"))
        );
        assert!(inherited.efforts.iter().any(|choice| choice.id == "low"));
        assert!(!inherited.efforts.iter().any(|choice| choice.id == "medium"));
        staff.model = ahjo_missions_contract::HARNESS_DEFAULT_MODEL.into();
        let explicit = choices(Some(&c), &staff);
        assert!(
            explicit
                .models
                .iter()
                .any(|choice| choice.id == "default" && choice.label == "Harness default")
        );
        assert!(explicit.efforts.iter().any(|choice| choice.id == "medium"));
        assert_eq!(staff.harness, Harness::Unset);
        assert_eq!(staff.effort, Effort::Unset);
    }

    #[test]
    fn picker_keeps_unavailable_agent_and_override() {
        let c = catalog();
        let staff = Staffing {
            agent: "archived".into(),
            harness: Harness::OpenAiCodex,
            model: "gpt-6".into(),
            effort: Effort::Medium,
        };
        let choices = choices(Some(&c), &staff);
        let ids = |choices: Vec<Choice>| {
            choices
                .into_iter()
                .map(|choice| choice.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(choices.agents), ["", "planner", "reviewer", "archived"]);
        assert_eq!(ids(choices.efforts), ["", "low", "high", "medium"]);
        assert_eq!(ids(choices.models), ["", "default", "gpt-6"]);
    }
}
