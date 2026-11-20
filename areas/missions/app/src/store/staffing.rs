//! Clamp slot choices to the catalog published by the node's agent library.
use ahjo_harness_contract::agents::ListedAgents;
use ahjo_missions_contract::{Agent, Effort, Harness, HarnessChoices, MissionChange, Slot};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Staffing {
    pub agent_id: String,
    pub harness: Harness,
    pub model: String,
    pub effort: Effort,
}

impl From<&Slot> for Staffing {
    fn from(slot: &Slot) -> Self {
        Self {
            agent_id: slot.agent_id.clone(),
            harness: slot.harness,
            model: slot.model.clone(),
            effort: slot.effort,
        }
    }
}

impl Staffing {
    pub fn use_agent(&mut self, agent: &Agent) {
        self.agent_id.clone_from(&agent.id);
        self.harness = agent.harness;
        self.model.clone_from(&agent.model);
        self.effort = agent.effort;
    }

    pub fn assign(self, step_id: &str, slot_id: &str) -> MissionChange {
        MissionChange::AssignSlot {
            step_id: step_id.into(),
            slot_id: slot_id.into(),
            agent_id: self.agent_id,
            harness: self.harness,
            model: self.model,
            effort: self.effort,
        }
    }
}

fn rank(effort: Effort) -> usize {
    Effort::ALL
        .iter()
        .position(|item| *item == effort)
        .unwrap_or(Effort::ALL.len())
}
fn harness_choices(catalog: &ListedAgents, harness: Harness) -> Option<&HarnessChoices> {
    catalog.harnesses.iter().find(|choice| choice.id == harness)
}

fn available_efforts<'a>(catalog: &'a ListedAgents, staff: &Staffing) -> &'a [Effort] {
    let Some(harness) = harness_choices(catalog, staff.harness) else {
        return &[];
    };
    harness
        .models
        .iter()
        .find(|model| model.id == staff.model && !model.efforts.is_empty())
        .map_or(&harness.efforts, |model| &model.efforts)
}

fn clamp(catalog: &ListedAgents, staff: &mut Staffing) {
    let efforts = available_efforts(catalog, staff);
    if staff.effort.is_unset() || efforts.is_empty() || efforts.contains(&staff.effort) {
        return;
    }
    let want = rank(staff.effort);
    staff.effort = efforts
        .iter()
        .copied()
        .rfind(|effort| rank(*effort) <= want)
        .unwrap_or(efforts[0]);
}
pub fn pick_harness(catalog: &ListedAgents, staff: &mut Staffing, harness: Harness) {
    staff.harness = harness;
    if let Some(choices) = harness_choices(catalog, harness) {
        if !choices.models.iter().any(|model| model.id == staff.model) {
            staff.model.clear();
        }
        if staff.effort.is_unset() {
            staff.effort = catalog
                .agents
                .iter()
                .find(|agent| agent.id == staff.agent_id)
                .map_or(choices.default_effort, |agent| agent.effort);
        }
    }
    clamp(catalog, staff);
}

pub fn pick_model(catalog: &ListedAgents, staff: &mut Staffing, model: &str) {
    staff.model = model.to_owned();
    clamp(catalog, staff);
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
    let harnesses = c
        .harnesses
        .iter()
        .map(|h| choice(h.id.as_str(), &harness_name(h.id.as_str())))
        .collect();
    let mut models_list = vec![choice("", "default")];
    if let Some(harness) = harness_choices(c, staff.harness) {
        models_list.extend(
            harness
                .models
                .iter()
                .map(|model| choice(&model.id, &model.id)),
        );
    }
    Choices {
        agents: with_current(agents, &staff.agent_id, agent_name(&staff.agent_id)),
        harnesses: with_current(
            harnesses,
            staff.harness.as_str(),
            harness_name(staff.harness.as_str()),
        ),
        models: with_current(models_list, &staff.model, staff.model.clone()),
        efforts: with_current(
            available_efforts(c, staff)
                .iter()
                .map(|e| choice(e.as_str(), e.as_str()))
                .collect(),
            staff.effort.as_str(),
            if staff.effort.is_unset() {
                "no effort".into()
            } else {
                staff.effort.as_str().to_owned()
            },
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
    fn a_harness_chosen_first_uses_the_catalog_default_or_the_agents_effort() {
        let mut c = catalog();
        let mut s = Staffing::default();
        pick_harness(&c, &mut s, Harness::ClaudeCode);
        assert_eq!(s.effort, Effort::Medium);
        let mut s = Staffing {
            agent_id: "reviewer".into(),
            ..Staffing::default()
        };
        pick_harness(&c, &mut s, Harness::ClaudeCode);
        assert_eq!(s.effort, Effort::Xhigh);
        c.harnesses[1].efforts = vec![Effort::Low, Effort::High];
        let mut s = Staffing::default();
        pick_harness(&c, &mut s, Harness::OpenAiCodex);
        assert_eq!(s.effort, Effort::Low);
    }

    #[test]
    fn a_harness_change_keeps_what_it_can() {
        let c = catalog();
        let mut s = Staffing {
            agent_id: "planner".into(),
            harness: Harness::OpenAiCodex,
            model: "gpt-6".into(),
            effort: Effort::High,
        };
        pick_harness(&c, &mut s, Harness::ClaudeCode);
        assert!(s.model.is_empty());
        assert_eq!(s.effort, Effort::High);
        s.effort = Effort::Ultra;
        pick_harness(&c, &mut s, Harness::ClaudeCode);
        assert_eq!(s.effort, Effort::Max);
        pick_harness(&c, &mut s, Harness::OpenAiCodex);
        pick_model(&c, &mut s, "gpt-6");
        assert_eq!(s.effort, Effort::High);
    }

    #[test]
    fn picker_keeps_current_and_uses_sorted_agents_and_model_efforts() {
        let c = catalog();
        let staff = Staffing {
            agent_id: "archived".into(),
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
        assert_eq!(ids(choices.efforts), ["low", "high", "medium"]);
        assert_eq!(ids(choices.models), ["", "gpt-6"]);
    }
}
