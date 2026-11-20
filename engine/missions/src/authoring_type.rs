//! Change a step's shape without silently discarding staffed seats.
use crate::Error;
use ahjo_mission_model::{Formation, Mission, MissionChange, Seat};
use std::collections::HashSet;
use toml_edit::{DocumentMut, Item, value};
fn invalid(message: &str) -> Error {
    Error::new("invalid_type_change", message)
}
fn valid_id(id: &str) -> bool {
    id.len() <= 64
        && id.as_bytes().first().is_some_and(u8::is_ascii_alphanumeric)
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
fn new_seat(label: &str, controller: bool) -> Result<Seat, Error> {
    Ok(Seat {
        id: crate::authoring::id("seat")?,
        label: label.into(),
        controller,
        ..Seat::default()
    })
}
pub(crate) fn apply(
    doc: &mut DocumentMut,
    mission: &Mission,
    patch: MissionChange,
) -> Result<bool, Error> {
    let MissionChange::SetStepFormation {
        id,
        formation,
        keep_seat_id,
        seats,
    } = patch
    else {
        unreachable!()
    };
    if formation == Formation::Unset {
        return Err(Error::new(
            "unsupported_step_formation",
            "the step needs a formation",
        ));
    }
    if !keep_seat_id.is_empty() && formation != Formation::Solo {
        return Err(invalid("keep_seat_id applies only to solo"));
    }
    let current = mission
        .step(&id)
        .ok_or_else(|| Error::new("not_found", "step"))?;
    let next = if let Some(snapshot) = seats {
        let mut seen = HashSet::new();
        let controllers = snapshot.iter().filter(|s| s.controller).count();
        if snapshot
            .iter()
            .any(|s| !valid_id(&s.id) || !seen.insert(s.id.clone()))
            || match formation {
                Formation::Solo => snapshot.len() != 1,
                Formation::Peer => snapshot.len() < 2 || controllers != 0,
                _ => snapshot.len() < 2 || controllers != 1,
            }
        {
            return Err(invalid("seat snapshot does not fit step formation"));
        }
        if !keep_seat_id.is_empty()
            && (!current.seats.iter().any(|s| s.id == keep_seat_id)
                || snapshot[0].id != keep_seat_id)
        {
            return Err(invalid("keep_seat_id must name the retained current seat"));
        }
        if current
            .seats
            .iter()
            .any(|s| s.staffed() && !seen.contains(&s.id))
            && keep_seat_id.is_empty()
        {
            return Err(Error::new(
                "seat_choice_required",
                "snapshot would remove a staffed seat",
            ));
        }
        if mission
            .steps
            .iter()
            .any(|f| f.id != id && f.seats.iter().any(|s| seen.contains(&s.id)))
        {
            return Err(invalid("seat id already used"));
        }
        snapshot
    } else {
        let mut seats = current.seats.clone();
        if !keep_seat_id.is_empty() && !seats.iter().any(|s| s.id == keep_seat_id) {
            return Err(invalid("seat to keep does not exist"));
        }
        match formation {
            Formation::Solo => {
                if seats.is_empty() {
                    seats.push(new_seat("Agent", false)?);
                }
                let staffed: Vec<_> = seats.iter().filter(|s| s.staffed()).collect();
                if keep_seat_id.is_empty() && staffed.len() > 1 {
                    return Err(Error::new(
                        "seat_choice_required",
                        "choose the staffed seat to keep",
                    ));
                }
                let keep = if !keep_seat_id.is_empty() {
                    &keep_seat_id
                } else if let Some(seat) = staffed.first() {
                    &seat.id
                } else {
                    &seats[0].id
                };
                let mut seat = seats.iter().find(|s| &s.id == keep).unwrap().clone();
                seat.controller = false;
                vec![seat]
            }
            Formation::Peer => {
                for s in &mut seats {
                    s.controller = false;
                }
                while seats.len() < 2 {
                    seats.push(new_seat("Peer", false)?);
                }
                seats
            }
            _ => {
                if seats.is_empty() {
                    seats.push(new_seat("Orchestrator", true)?);
                    seats.push(new_seat("Agent", false)?);
                    seats.push(new_seat("Agent", false)?);
                }
                let controllers: Vec<_> = seats
                    .iter()
                    .filter(|s| s.controller)
                    .map(|s| s.id.clone())
                    .collect();
                let controller = if controllers.len() == 1 {
                    &controllers[0]
                } else {
                    &seats[0].id
                }
                .clone();
                for seat in &mut seats {
                    seat.controller = seat.id == controller;
                }
                if seats.len() == 1 {
                    seats.push(new_seat("Agent", false)?);
                }
                seats
            }
        }
    };
    if current.formation == formation && current.seats == next {
        return Ok(false);
    }
    let table = doc["step"]
        .as_array_of_tables_mut()
        .and_then(|a| {
            a.iter_mut()
                .find(|t| t.get("id").and_then(Item::as_str) == Some(id.as_str()))
        })
        .ok_or_else(|| invalid("step source"))?;
    crate::write::set(table, "formation", value(formation.as_str()));
    // Retain the source table of each kept seat, including its comments.
    let old = table.remove("seat");
    let mut source = std::collections::HashMap::new();
    if let Some(Item::ArrayOfTables(tables)) = old {
        for table in tables.iter() {
            if let Some(id) = table.get("id").and_then(Item::as_str) {
                source.insert(id.to_owned(), table.clone());
            }
        }
    } else if let Some(Item::Value(toml_edit::Value::Array(array))) = old {
        for val in array.iter() {
            if let Some(t) = val.as_inline_table()
                && let Some(id) = t.get("id").and_then(|v| v.as_str())
            {
                source.insert(id.to_owned(), t.clone().into_table());
            }
        }
    }
    let mut result = toml_edit::ArrayOfTables::new();
    for seat in &next {
        let mut table = source.remove(&seat.id).unwrap_or_else(|| {
            toml_edit::ser::to_document(seat)
                .expect("seat serializes")
                .into_table()
        });
        for (key, field) in [
            ("id", seat.id.as_str()),
            ("label", seat.label.as_str()),
            ("agent", seat.agent.as_str()),
            ("harness", seat.harness.as_str()),
            ("model", seat.model.as_str()),
            ("effort", seat.effort.as_str()),
        ] {
            if field.is_empty() && !["id", "label"].contains(&key) {
                table.remove(key);
            } else if table.get(key).and_then(Item::as_str) != Some(field) {
                crate::write::set(&mut table, key, value(field));
            }
        }
        if table.get("controller").and_then(Item::as_bool) != Some(seat.controller) {
            crate::write::set(&mut table, "controller", value(seat.controller));
        }
        result.push(table);
    }
    table["seat"] = Item::ArrayOfTables(result);
    Ok(true)
}
