//! Remove a node, its wires and dependent Limit cards as one edit.
use crate::Error;
use ahjo_mission_model::{Mission, MissionChange};
use std::collections::HashSet;
use toml_edit::DocumentMut;

pub(crate) fn remove(doc: &mut DocumentMut, key: &str, ids: &HashSet<String>) -> bool {
    let Some(tables) = doc
        .get_mut(key)
        .and_then(|item| item.as_array_of_tables_mut())
    else {
        return false;
    };
    let mut indices = Vec::new();
    for (index, table) in tables.iter().enumerate() {
        if table
            .get("id")
            .and_then(|item| item.as_str())
            .is_some_and(|id| ids.contains(id))
        {
            indices.push(index);
        }
    }
    let found = !indices.is_empty();
    for index in indices.into_iter().rev() {
        tables.remove(index);
    }
    found
}

pub(crate) fn apply(
    doc: &mut DocumentMut,
    mission: &Mission,
    patch: MissionChange,
) -> Result<HashSet<String>, Error> {
    let (key, id, remove_limits) = match patch {
        MissionChange::DeleteInputCard { id } => ("inputCard", id, false),
        MissionChange::DeleteFormation { id } => ("formation", id, true),
        MissionChange::DeleteGate { id } => ("gate", id, false),
        MissionChange::DeleteEnd { id } => ("end", id, false),
        MissionChange::DeleteLimit { id } => ("limit", id, false),
        _ => unreachable!(),
    };
    let mut ids = HashSet::from([id.clone()]);
    if !remove(doc, key, &ids) {
        return Err(Error::new("not_found", format!("node {id:?}")));
    }
    if remove_limits {
        for limit in &mission.limits {
            if limit.target == id {
                ids.insert(limit.id.clone());
            }
        }
        remove(doc, "limit", &ids);
    }
    let mut wires = HashSet::new();
    for wire in &mission.connections {
        if [&wire.from, &wire.to]
            .iter()
            .any(|endpoint| ids.contains(endpoint.split(':').next().unwrap_or("")))
        {
            wires.insert(wire.id.clone());
        }
    }
    remove(doc, "connection", &wires);
    Ok(ids)
}
