//! Clamp slot choices to the catalog published by the node's role library.
use ahjo_harness_contract::roles::Roles;
use ahjo_missions_contract::{Effort, Harness, MissionChange, Slot};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Staffing {
    pub role: String,
    pub harness: Harness,
    pub model: String,
    pub effort: Effort,
}

impl From<&Slot> for Staffing {
    fn from(slot: &Slot) -> Self {
        Self {
            role: slot.agent_id.clone(),
            harness: slot.harness,
            model: slot.model.clone(),
            effort: slot.effort,
        }
    }
}

impl Staffing {
    pub fn assign(self, step_id: &str, slot_id: &str) -> MissionChange {
        MissionChange::AssignSlot {
            step_id: step_id.into(),
            slot_id: slot_id.into(),
            agent_id: self.role,
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
fn models<'a>(catalog: &'a Roles, harness: &str) -> &'a [serde_json::Value] {
    catalog
        .harnesses
        .iter()
        .find(|h| h.id == harness)
        .map(|h| h.models.as_slice())
        .unwrap_or_default()
}
fn available_efforts(catalog: &Roles, staff: &Staffing) -> Vec<Effort> {
    let model = models(catalog, staff.harness.as_str())
        .iter()
        .find(|m| m["id"].as_str() == Some(staff.model.as_str()));
    let model_efforts = model
        .and_then(|m| m["efforts"].as_array())
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str()?.parse().ok())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if !model_efforts.is_empty() {
        return model_efforts;
    }
    catalog
        .harnesses
        .iter()
        .find(|harness| harness.id == staff.harness.as_str())
        .into_iter()
        .flat_map(|harness| &harness.efforts)
        .filter_map(|effort| effort.parse().ok())
        .collect()
}

fn clamp(catalog: &Roles, staff: &mut Staffing) {
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
pub fn pick_harness(catalog: &Roles, staff: &mut Staffing, harness: Harness) {
    staff.harness = harness;
    if !models(catalog, harness.as_str())
        .iter()
        .any(|m| m["id"].as_str() == Some(staff.model.as_str()))
    {
        staff.model.clear();
    }
    // A harness never stands without an effort: the role's usual one, so
    // the step is whole the moment a harness is chosen.
    if staff.effort.is_unset() && !harness.is_unset() {
        staff.effort = usual_effort(&staff.role);
    }
    clamp(catalog, staff);
}

/// The effort a role usually works at, after the node's effort policy: low
/// for errands, medium for making things, xhigh for architecture and
/// review. Max is only ever chosen by hand.
pub fn usual_effort(role: &str) -> Effort {
    match role {
        "verifier" | "scout" => Effort::Low,
        "reviewer" | "judge" | "architect" | "designer" | "planner" | "orchestrator" => {
            Effort::Xhigh
        }
        _ => Effort::Medium,
    }
}
pub fn pick_model(catalog: &Roles, staff: &mut Staffing, model: &str) {
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
    pub roles: Vec<Choice>,
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
/// roles by name; the existing value always remains available, even if the
/// node no longer offers it. Models and efforts follow the chosen harness.
pub fn choices(catalog: Option<&Roles>, staff: &Staffing) -> Choices {
    use super::words::{harness_name, role_name};
    let empty = Roles::default();
    let c = catalog.unwrap_or(&empty);
    let mut roles: Vec<_> = c
        .roles
        .iter()
        .filter(|r| r.status != "retired")
        .map(|r| {
            let label = if r.display_name.is_empty() {
                role_name(&r.id)
            } else {
                r.display_name.clone()
            };
            choice(&r.id, &label)
        })
        .collect();
    roles.sort_by_key(|r| r.label.to_lowercase());
    roles.insert(0, choice("", "vanilla"));
    let harnesses = c
        .harnesses
        .iter()
        .filter(|h| h.id.parse::<Harness>().is_ok())
        .map(|h| choice(&h.id, &harness_name(&h.id)))
        .collect();
    let mut models_list = vec![choice("", "default")];
    models_list.extend(
        models(c, staff.harness.as_str())
            .iter()
            .filter_map(|m| m["id"].as_str())
            .map(|id| choice(id, id)),
    );
    Choices {
        roles: with_current(roles, &staff.role, role_name(&staff.role)),
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
    #[test]
    fn a_harness_chosen_first_comes_with_its_roles_usual_effort() {
        let c: Roles = serde_json::from_str(r#"{"harnesses":[{"id":"claude-code","efforts":["low","medium","high","xhigh","max"]},{"id":"openai-codex","efforts":["low","high"]}]}"#).unwrap();
        let mut s = Staffing::default();
        pick_harness(&c, &mut s, Harness::ClaudeCode);
        assert_eq!(s.effort, Effort::Medium);
        let mut s = Staffing {
            role: "reviewer".into(),
            ..Staffing::default()
        };
        pick_harness(&c, &mut s, Harness::ClaudeCode);
        assert_eq!(s.effort, Effort::Xhigh);
        // An effort the harness lacks gives way to the nearest below it.
        let mut s = Staffing::default();
        pick_harness(&c, &mut s, Harness::OpenAiCodex);
        assert_eq!(s.effort, Effort::Low);
    }
    #[test]
    fn a_harness_change_keeps_what_it_can() {
        let c: Roles = serde_json::from_str(r#"{"roles":[{"id":"planner","displayName":"Planner"}],"harnesses":[{"id":"claude-code","efforts":["low","medium","high","xhigh","max"],"models":[{"id":"opus"}]},{"id":"openai-codex","efforts":["low","medium","high","xhigh","max","ultra"],"models":[{"id":"gpt-6","efforts":["low","high"]}]}]}"#).unwrap();
        let mut s = Staffing {
            role: "planner".into(),
            harness: Harness::OpenAiCodex,
            model: "gpt-6".into(),
            effort: Effort::High,
        };
        pick_harness(&c, &mut s, Harness::ClaudeCode);
        assert_eq!(s.model, "");
        assert_eq!(s.effort, Effort::High);
        s.effort = Effort::Ultra;
        pick_harness(&c, &mut s, Harness::ClaudeCode);
        assert_eq!(s.effort, Effort::Max);
        pick_harness(&c, &mut s, Harness::OpenAiCodex);
        pick_model(&c, &mut s, "gpt-6");
        assert_eq!(s.effort, Effort::High);
    }
    #[test]
    fn picker_keeps_current_and_uses_sorted_active_roles_and_model_efforts() {
        let c: Roles = serde_json::from_str(r#"{"roles":[{"id":"reviewer","displayName":"Zed"},{"id":"retired","displayName":"Old","status":"retired"},{"id":"planner","displayName":"Alpha"}],"harnesses":[{"id":"openai-codex","efforts":["low","medium","high"],"models":[{"id":"gpt-6","efforts":["low","high"]}]}]}"#).unwrap();
        let staff = Staffing {
            role: "retired".into(),
            harness: Harness::OpenAiCodex,
            model: "gpt-6".into(),
            effort: Effort::Medium,
        };
        let choices = choices(Some(&c), &staff);
        assert_eq!(
            choices
                .roles
                .iter()
                .map(|c| c.id.as_str())
                .collect::<Vec<_>>(),
            ["", "planner", "reviewer", "retired"]
        );
        assert_eq!(
            choices
                .efforts
                .iter()
                .map(|c| c.id.as_str())
                .collect::<Vec<_>>(),
            ["low", "high", "medium"]
        );
        assert_eq!(
            choices
                .models
                .iter()
                .map(|c| c.id.as_str())
                .collect::<Vec<_>>(),
            ["", "gpt-6"]
        );
    }
}
