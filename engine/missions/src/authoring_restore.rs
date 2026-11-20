//! Restore a deleted card, its wires and its canvas position in one edit.
use crate::Error;
use ahjo_mission_model::{
    Connection, EndOutcome, Formation, LayoutCard, Mission, MissionChange, PortKind, RestoredCard,
};
use serde::Serialize;
use std::collections::HashSet;
use toml_edit::{ArrayOfTables, DocumentMut, Item};
fn invalid(message: &str) -> Error {
    Error::new("invalid_card_restore", message)
}
fn put<T: Serialize>(
    doc: &mut DocumentMut,
    key: &str,
    card: &T,
    index: Option<usize>,
) -> Result<(), Error> {
    let table = toml_edit::ser::to_document(card)
        .map_err(|e| invalid(&e.to_string()))?
        .into_table();
    if !doc.contains_key(key) {
        doc[key] = Item::ArrayOfTables(ArrayOfTables::new());
    }
    let array = doc[key]
        .as_array_of_tables_mut()
        .ok_or_else(|| invalid("card list"))?;
    array.insert(index.unwrap_or(array.len()).min(array.len()), table);
    Ok(())
}
pub(crate) fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.')
}
pub(crate) fn apply(
    doc: &mut DocumentMut,
    mission: &Mission,
    mut card: RestoredCard,
    connections: Vec<Connection>,
    index: Option<usize>,
    x: i64,
    y: i64,
) -> Result<LayoutCard, Error> {
    let id = match &card {
        RestoredCard::InputCard(n) => &n.id,
        RestoredCard::Step(n) => &n.id,
        RestoredCard::Gate(n) => &n.id,
        RestoredCard::End(n) => &n.id,
        RestoredCard::Limit(n) => &n.id,
    }
    .clone();
    if !valid_id(&id)
        || mission.input_cards.iter().any(|n| n.id == id)
        || mission.step(&id).is_some()
        || mission.gate(&id).is_some()
        || mission.end(&id).is_some()
        || mission.limits.iter().any(|n| n.id == id)
    {
        return Err(invalid("card id is invalid or already taken"));
    }
    match &mut card {
        RestoredCard::InputCard(n) => {
            crate::authoring_create::file_refs(&mut n.files)?;
            put(doc, "input_card", n, index)?;
            let current = Mission::parse(&doc.to_string())?;
            crate::authoring_input::apply(
                doc,
                &current,
                ahjo_mission_model::InputCardUpdate {
                    id: n.id.clone(),
                    human_channel: Some(n.human_channel),
                    inputs: Some(n.inputs.clone()),
                    outputs: Some(n.outputs.clone()),
                    ..Default::default()
                },
            )?;
        }
        RestoredCard::Step(n) => {
            if n.formation == Formation::Unset {
                return Err(Error::new(
                    "unsupported_step_formation",
                    "invalid step formation",
                ));
            }
            let mut ports = HashSet::new();
            for port in n.inputs.iter().chain(&n.outputs) {
                if !valid_id(&port.id) || !ports.insert(&port.id) {
                    return Err(invalid("invalid or duplicate port id"));
                }
            }
            if n.inputs.iter().any(|p| p.kind() != PortKind::Text) {
                return Err(Error::new("invalid_port", "invalid port kind"));
            }
            for port in n.inputs.iter_mut().chain(&mut n.outputs) {
                if port.kind == PortKind::Text {
                    port.kind = PortKind::Unset;
                }
            }
            let controllers = n.seats.iter().filter(|s| s.controller).count();
            if match n.formation {
                Formation::Solo => n.seats.len() != 1,
                Formation::Peer => n.seats.len() < 2 || controllers != 0,
                _ => n.seats.len() < 2 || controllers != 1,
            } {
                return Err(invalid("invalid step shape"));
            }
            let mut seen = HashSet::new();
            for seat in &n.seats {
                if seat.id.is_empty()
                    || seat.id.len() > 64
                    || !seat.id.as_bytes()[0].is_ascii_alphanumeric()
                    || !seat
                        .id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                    || !seen.insert(&seat.id)
                    || mission
                        .steps
                        .iter()
                        .any(|f| f.seats.iter().any(|s| s.id == seat.id))
                {
                    return Err(invalid("invalid or duplicate seat id"));
                }
            }
            if let Some(brief) = &mut n.brief {
                crate::authoring_create::file_refs(&mut brief.files)?;
                crate::authoring_staff::check_work(&brief.work)?;
            }
            put(doc, "step", n, index)?;
        }
        RestoredCard::Gate(n) => {
            crate::authoring_controls::check_gate(n)?;
            put(doc, "gate", n, index)?;
        }
        RestoredCard::End(n) => {
            if n.outcome == EndOutcome::Unset {
                n.outcome = EndOutcome::Done;
            }
            if n.title.trim().is_empty() {
                n.title = if n.outcome == EndOutcome::Done {
                    "Done"
                } else {
                    "Rejected"
                }
                .into();
            }
            put(doc, "end", n, index)?;
        }
        RestoredCard::Limit(n) => {
            if n.title.trim().is_empty() {
                n.title = "Limit".into();
            }
            if [n.rounds, n.seconds, n.warn_seconds, n.tokens]
                .into_iter()
                .flatten()
                .any(|v| v <= 0)
            {
                return Err(Error::new("invalid_limit", "limit knobs must be positive"));
            }
            put(doc, "limit", n, index)?;
        }
    }
    let mut current = Mission::parse(&doc.to_string())?;
    for mut wire in connections {
        if crate::graph::endpoint_card(&wire.from) != id.as_str()
            && crate::graph::endpoint_card(&wire.to) != id.as_str()
        {
            return Err(invalid("connection does not touch restored card"));
        }
        if !valid_id(&wire.id) || current.connections.iter().any(|c| c.id == wire.id) {
            wire.id = crate::authoring::id("edge")?;
        }
        crate::authoring_wire::apply(
            doc,
            &current,
            MissionChange::WireConnection {
                id: wire.id,
                from: wire.from,
                to: wire.to,
                join_if_occupied: false,
            },
        )
        .map_err(|e| {
            if ["not_found", "input_occupied"].contains(&e.code.as_str()) {
                invalid(&e.to_string())
            } else {
                e
            }
        })?;
        current = Mission::parse(&doc.to_string())?;
    }
    Ok(LayoutCard { id, x, y })
}
