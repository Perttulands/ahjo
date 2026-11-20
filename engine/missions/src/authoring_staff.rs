//! Step titles, staffing and controller selection.
use crate::Error;
use ahjo_mission_model::{Mission, MissionChange};
use toml_edit::{DocumentMut, Item, TableLike, Value, value};
fn not_found() -> Error {
    Error::new("not_found", "step or seat")
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
pub(crate) fn seat_table<'a>(item: &'a mut Item, id: &str) -> Result<&'a mut dyn TableLike, Error> {
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
    agents: &crate::AgentStore,
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
        MissionChange::StaffSeat {
            step_id,
            seat_id,
            agent,
            harness,
            model,
            effort,
        } => {
            let step = mission.step(&step_id).ok_or_else(not_found)?;
            let seat = step
                .seats
                .iter()
                .find(|s| s.id == seat_id)
                .ok_or_else(not_found)?;
            let (agent, model) = (agent.trim(), model.trim());
            if !agent.is_empty() && crate::check_agent_id(agent).is_err() {
                return Err(Error::new("invalid_seat_settings", "invalid agent id"));
            }
            let setup = if agent.is_empty() {
                None
            } else {
                Some(
                    agents
                        .read(agent)
                        .map_err(|error| Error::new("invalid_seat_settings", error.message))?,
                )
            };
            let next = ahjo_mission_model::Seat {
                agent: agent.to_owned(),
                harness,
                model: model.to_owned(),
                effort,
                ..Default::default()
            };
            if next.staffed() {
                let resolved = next.settings(setup.as_ref());
                let harnesses = crate::Harnesses::from_host();
                let check = if model.is_empty() && setup.is_some() {
                    harnesses.check(
                        "seat",
                        resolved.harness.as_str(),
                        &resolved.model,
                        resolved.effort.as_str(),
                    )
                } else {
                    harnesses.check_offered(
                        "seat",
                        resolved.harness.as_str(),
                        &resolved.model,
                        resolved.effort.as_str(),
                    )
                };
                check.map_err(|error| Error::new("invalid_seat_settings", error.message))?;
            }
            if (
                seat.agent.as_str(),
                seat.harness,
                seat.model.as_str(),
                seat.effort,
            ) == (agent, harness, model, effort)
            {
                return Ok(false);
            }
            let table = step_table(doc, &step_id)?;
            let item = table.get_mut("seat").ok_or_else(not_found)?;
            let seat = seat_table(item, &seat_id)?;
            for (key, val) in [
                ("agent", agent),
                ("harness", harness.as_str()),
                ("model", model),
                ("effort", effort.as_str()),
            ] {
                set(seat, key, val);
            }
        }
        MissionChange::SwapSeats {
            from_step,
            from_seat,
            to_step,
            to_seat,
        } => {
            let find = |step: &str, seat: &str| {
                mission
                    .step(step)
                    .and_then(|f| f.seats.iter().find(|s| s.id == seat))
                    .ok_or_else(not_found)
            };
            let from = find(&from_step, &from_seat)?;
            let to = find(&to_step, &to_seat)?;
            let settings = |s: &ahjo_mission_model::Seat| {
                [
                    s.agent.clone(),
                    s.harness.as_str().to_owned(),
                    s.model.clone(),
                    s.effort.as_str().to_owned(),
                ]
            };
            let first = settings(from);
            let second = settings(to);
            if first == second {
                return Ok(false);
            }
            for (step, id, values) in [
                (&from_step, &from_seat, &second),
                (&to_step, &to_seat, &first),
            ] {
                let table = step_table(doc, step)?;
                let seat = seat_table(table.get_mut("seat").ok_or_else(not_found)?, id)?;
                for (key, value) in ["agent", "harness", "model", "effort"].iter().zip(values) {
                    set(seat, key, value);
                }
            }
        }
        MissionChange::MakeController { step_id, seat_id } => {
            let step = mission.step(&step_id).ok_or_else(not_found)?;
            if step.formation != ahjo_mission_model::Formation::Orchestrated {
                return Err(Error::new(
                    "invalid_controller_role",
                    "step is not orchestrated",
                ));
            }
            if !step.seats.iter().any(|s| s.id == seat_id) {
                return Err(not_found());
            }
            if step.seats.iter().all(|s| s.controller == (s.id == seat_id)) {
                return Ok(false);
            }
            let table = step_table(doc, &step_id)?;
            let item = table.get_mut("seat").ok_or_else(not_found)?;
            for seat in &step.seats {
                crate::write::set(
                    seat_table(item, &seat.id)?,
                    "controller",
                    value(seat.id == seat_id),
                );
            }
        }
        _ => unreachable!(),
    }
    Ok(true)
}
