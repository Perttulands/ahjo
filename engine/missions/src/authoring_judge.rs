//! Judge chain attachment and detachment.
use crate::Error;
use ahjo_mission_model::{GateKind, Mission, MissionChange};
use std::collections::BTreeSet;
use toml_edit::{DocumentMut, Item, value};
fn missing() -> Error {
    Error::new("not_found", "gate or judge step")
}
pub(crate) fn apply(
    doc: &mut DocumentMut,
    mission: &Mission,
    patch: MissionChange,
) -> Result<bool, Error> {
    let (gate_id, chain) = match patch {
        MissionChange::SetGateJudge { gate_id, chain } => (gate_id, Some(chain)),
        MissionChange::DetachGateJudge { gate_id } => (gate_id, None),
        _ => unreachable!(),
    };
    let gate = mission.gate(&gate_id).ok_or_else(missing)?;
    let endpoint = format!("{gate_id}:judge");
    let mut kinds = gate.kinds.clone();
    let mut pairs = Vec::new();
    if let Some(chain) = chain {
        if chain.is_empty() {
            return Err(missing());
        }
        if !kinds.contains(&GateKind::Step) {
            kinds.push(GateKind::Step);
        }
        let mut previous = endpoint.clone();
        for id in &chain {
            let step = mission.step(id).ok_or_else(missing)?;
            let input = step.inputs.first().ok_or_else(missing)?;
            let output = step.outputs.first().ok_or_else(missing)?;
            pairs.push((previous, format!("{id}:{}", input.id)));
            previous = format!("{id}:{}", output.id);
        }
        pairs.push((previous, endpoint.clone()));
        let existing: BTreeSet<_> = mission
            .connections
            .iter()
            .filter(|c| c.from == endpoint || c.to == endpoint)
            .map(|c| (c.from.clone(), c.to.clone()))
            .collect();
        let expected: BTreeSet<_> = pairs
            .iter()
            .filter(|(a, b)| a == &endpoint || b == &endpoint)
            .cloned()
            .collect();
        if existing == expected
            && gate.kinds == kinds
            && pairs.iter().all(|(a, b)| {
                mission
                    .connections
                    .iter()
                    .any(|c| &c.from == a && &c.to == b)
            })
        {
            return Ok(false);
        }
    } else {
        kinds.retain(|k| *k != GateKind::Step);
        if kinds.is_empty() {
            kinds.push(GateKind::Human);
        }
        if kinds == gate.kinds
            && !mission
                .connections
                .iter()
                .any(|c| c.from == endpoint || c.to == endpoint)
        {
            return Ok(false);
        }
    }
    let gate_table = doc["gate"]
        .as_array_of_tables_mut()
        .and_then(|a| {
            a.iter_mut()
                .find(|t| t.get("id").and_then(Item::as_str) == Some(gate_id.as_str()))
        })
        .ok_or_else(missing)?;
    let mut array = toml_edit::Array::new();
    for k in &kinds {
        array.push(k.as_str());
    }
    crate::write::set(gate_table, "kinds", value(array));
    if let Some(wires) = doc
        .get_mut("connection")
        .and_then(Item::as_array_of_tables_mut)
    {
        let indices: Vec<_> = wires
            .iter()
            .enumerate()
            .filter(|(_, t)| {
                ["from", "to"]
                    .iter()
                    .any(|key| t.get(key).and_then(Item::as_str) == Some(endpoint.as_str()))
            })
            .map(|(i, _)| i)
            .collect();
        for i in indices.into_iter().rev() {
            wires.remove(i);
        }
    }
    let mut current = Mission::parse(&doc.to_string())?;
    for (from, to) in pairs {
        if current
            .connections
            .iter()
            .any(|c| c.from == from && c.to == to)
        {
            continue;
        }
        crate::authoring_wire::apply(
            doc,
            &current,
            MissionChange::WireConnection {
                id: String::new(),
                from,
                to,
                join_if_occupied: false,
            },
        )?;
        current = Mission::parse(&doc.to_string())?;
    }
    Ok(true)
}
