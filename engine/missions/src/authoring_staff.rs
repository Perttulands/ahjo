//! Step titles, staffing and controller selection.
use crate::Error;
use ahjo_mission_model::{Mission, MissionChange};
use toml_edit::{DocumentMut, Item, TableLike, Value, value};
fn not_found() -> Error {
    Error::new("not_found", "step or slot")
}
pub(crate) fn step_table<'a>(
    doc: &'a mut DocumentMut,
    id: &str,
) -> Result<&'a mut toml_edit::Table, Error> {
    doc["step"]
        .as_array_of_tables_mut()
        .and_then(|arr| {
            arr.iter_mut()
                .find(|t| t.get("id").and_then(Item::as_str) == Some(id))
        })
        .ok_or_else(not_found)
}
pub(crate) fn slot_table<'a>(item: &'a mut Item, id: &str) -> Result<&'a mut dyn TableLike, Error> {
    match item {
        Item::ArrayOfTables(tables) => tables
            .iter_mut()
            .find(|t| t.get("id").and_then(Item::as_str) == Some(id))
            .map(|t| t as &mut dyn TableLike)
            .ok_or_else(not_found),
        Item::Value(Value::Array(array)) => array
            .iter_mut()
            .filter_map(Value::as_inline_table_mut)
            .find(|t| t.get("id").and_then(Value::as_str) == Some(id))
            .map(|t| t as &mut dyn TableLike)
            .ok_or_else(not_found),
        _ => Err(not_found()),
    }
}
pub(crate) fn set(table: &mut dyn TableLike, key: &str, next: &str) {
    if next.is_empty() {
        table.remove(key);
    } else if table.get(key).and_then(Item::as_str) != Some(next) {
        crate::write::set(table, key, value(next));
    }
}
/// A brief's work item, if any, must be a reference in a written form.
pub(crate) fn check_work(work: &str) -> Result<(), Error> {
    if work.is_empty() {
        return Ok(());
    }
    ahjo_work_ref::parse(work)
        .map(|_| ())
        .map_err(|invalid| Error::new("invalid_bead_id", invalid.to_string()))
}
pub(crate) fn apply(
    doc: &mut DocumentMut,
    mission: &Mission,
    patch: MissionChange,
) -> Result<bool, Error> {
    match patch {
        MissionChange::SetBrief {
            step_id,
            goal,
            repository,
            work,
            files,
            links,
        } => {
            let mut brief = ahjo_mission_model::Brief {
                goal,
                repository,
                work,
                files,
                links,
            };
            crate::authoring_create::file_refs(&mut brief.files)?;
            check_work(&brief.work)?;
            let step = mission.step(&step_id).ok_or_else(not_found)?;
            if step.brief.as_ref() == Some(&brief) {
                return Ok(false);
            }
            let next = toml_edit::ser::to_document(&brief)
                .map_err(|e| Error::new("invalid_definition", e.to_string()))?
                .into_table();
            step_table(doc, &step_id)?["brief"] = Item::Table(next);
        }
        MissionChange::ClearBrief { step_id } => {
            let step = mission.step(&step_id).ok_or_else(not_found)?;
            if step.brief.is_none() {
                return Err(not_found());
            }
            step_table(doc, &step_id)?.remove("brief");
        }
        MissionChange::EditStep {
            id,
            title,
            mut brief,
        } => {
            if let Some(brief) = &mut brief {
                crate::authoring_create::file_refs(&mut brief.files)?;
                check_work(&brief.work)?;
            }
            let step = mission.step(&id).ok_or_else(not_found)?;
            if step.title == title && step.brief == brief {
                return Ok(false);
            }
            let table = step_table(doc, &id)?;
            crate::write::set(table, "title", value(title));
            if let Some(brief) = brief {
                let next = toml_edit::ser::to_document(&brief)
                    .map_err(|e| Error::new("invalid_definition", e.to_string()))?
                    .into_table();
                table["brief"] = Item::Table(next);
            } else {
                table.remove("brief");
            }
        }
        MissionChange::UpdateStep {
            id,
            title,
            device,
            fresh,
        } => {
            let step = mission.step(&id).ok_or_else(not_found)?;
            let title = title.filter(|t| *t != step.title);
            let device = device
                .map(|d| d.trim().to_string())
                .filter(|d| *d != step.device);
            let fresh = fresh.filter(|f| *f != step.fresh);
            if title.is_none() && device.is_none() && fresh.is_none() {
                return Ok(false);
            }
            let table = step_table(doc, &id)?;
            if let Some(title) = title {
                // An empty title is an explicit value, not an absent field.
                crate::write::set(table, "title", value(title));
            }
            match device.as_deref() {
                Some("") => {
                    table.remove("device");
                }
                Some(d) => crate::write::set(table, "device", value(d)),
                None => {}
            }
            match fresh {
                Some(true) => crate::write::set(table, "fresh", value(true)),
                Some(false) => {
                    table.remove("fresh");
                }
                None => {}
            }
        }
        MissionChange::NameAgent {
            step_id,
            slot_id,
            agent,
        } => {
            let step = mission.step(&step_id).ok_or_else(not_found)?;
            let slot = step
                .slots
                .iter()
                .find(|s| s.id == slot_id)
                .ok_or_else(not_found)?;
            let agent = agent.trim();
            if !agent.is_empty() && !ahjo_mission_model::valid_agent(agent) {
                return Err(Error::new(
                    "invalid_agent",
                    "an agent's name is lower-case words of letters and digits joined by dashes",
                ));
            }
            if slot.agent == agent {
                return Ok(false);
            }
            let table = step_table(doc, &step_id)?;
            let item = table.get_mut("slot").ok_or_else(not_found)?;
            set(slot_table(item, &slot_id)?, "agent", agent);
        }
        MissionChange::AssignSlot {
            step_id,
            slot_id,
            agent_id,
            harness,
            model,
            effort,
        } => {
            let step = mission.step(&step_id).ok_or_else(not_found)?;
            let slot = step
                .slots
                .iter()
                .find(|s| s.id == slot_id)
                .ok_or_else(not_found)?;
            let harness = crate::harness::harness_id(&harness);
            let (agent_id, harness, model, effort) = (
                agent_id.trim(),
                harness.as_str(),
                model.trim(),
                effort.trim(),
            );
            if !agent_id.is_empty() && crate::roles::check_role_id(agent_id).is_err() {
                return Err(Error::new("invalid_slot_settings", "invalid role id"));
            }
            if [agent_id, harness, model, effort]
                .iter()
                .any(|s| !s.is_empty())
            {
                crate::Harnesses::from_host()
                    .check_offered("slot", harness, model, effort)
                    .map_err(|e| Error::new("invalid_slot_settings", e.message))?;
            }
            if (
                slot.agent_id.as_str(),
                slot.harness.as_str(),
                slot.model.as_str(),
                slot.effort.as_str(),
            ) == (agent_id, harness, model, effort)
            {
                return Ok(false);
            }
            let table = step_table(doc, &step_id)?;
            let item = table.get_mut("slot").ok_or_else(not_found)?;
            let slot = slot_table(item, &slot_id)?;
            for (key, val) in [
                ("agent_id", agent_id),
                ("harness", harness),
                ("model", model),
                ("effort", effort),
            ] {
                set(slot, key, val);
            }
        }
        MissionChange::SwapSlots {
            from_step,
            from_slot,
            to_step,
            to_slot,
        } => {
            let find = |step: &str, slot: &str| {
                mission
                    .step(step)
                    .and_then(|f| f.slots.iter().find(|s| s.id == slot))
                    .ok_or_else(not_found)
            };
            let from = find(&from_step, &from_slot)?;
            let to = find(&to_step, &to_slot)?;
            let settings = |s: &ahjo_mission_model::Slot| {
                [
                    s.agent_id.clone(),
                    s.harness.clone(),
                    s.model.clone(),
                    s.effort.clone(),
                ]
            };
            let first = settings(from);
            let second = settings(to);
            if first == second {
                return Ok(false);
            }
            for (step, id, values) in [
                (&from_step, &from_slot, &second),
                (&to_step, &to_slot, &first),
            ] {
                let table = step_table(doc, step)?;
                let slot = slot_table(table.get_mut("slot").ok_or_else(not_found)?, id)?;
                for (key, value) in ["agent_id", "harness", "model", "effort"]
                    .iter()
                    .zip(values)
                {
                    set(slot, key, value);
                }
            }
        }
        MissionChange::MakeController { step_id, slot_id } => {
            let step = mission.step(&step_id).ok_or_else(not_found)?;
            if step.formation != "orchestrated" {
                return Err(Error::new(
                    "invalid_controller_role",
                    "step is not orchestrated",
                ));
            }
            if !step.slots.iter().any(|s| s.id == slot_id) {
                return Err(not_found());
            }
            if step.slots.iter().all(|s| s.controller == (s.id == slot_id)) {
                return Ok(false);
            }
            let table = step_table(doc, &step_id)?;
            let item = table.get_mut("slot").ok_or_else(not_found)?;
            for slot in &step.slots {
                crate::write::set(
                    slot_table(item, &slot.id)?,
                    "controller",
                    value(slot.id == slot_id),
                );
            }
        }
        _ => unreachable!(),
    }
    Ok(true)
}
