//! Partial Input card edits and declaration normalization.
use crate::Error;
use ahjo_mission_model::{HumanChannel, InputCardUpdate, Mission, PortKind};
use serde::Serialize;
use std::collections::HashSet;
use toml_edit::{DocumentMut, Item, value};

fn normalize_name(
    name: &mut String,
    kind: &mut PortKind,
    seen: &mut HashSet<String>,
    code: &'static str,
) -> Result<(), Error> {
    *name = name.trim().into();
    *kind = kind.effective();
    if !crate::validate::inputs::valid_name(name) || !seen.insert(name.clone()) {
        return Err(Error::new(code, "invalid or duplicate name"));
    }
    Ok(())
}
fn list_value<T: Serialize>(values: &[T]) -> Result<Item, Error> {
    #[derive(Serialize)]
    struct Wrapper<'a, T> {
        list: &'a [T],
    }
    let mut doc = toml_edit::ser::to_document(&Wrapper { list: values })
        .map_err(|e| Error::new("invalid_definition", e.to_string()))?;
    Ok(doc.remove("list").unwrap())
}
pub(crate) fn apply(
    doc: &mut DocumentMut,
    mission: &Mission,
    update: InputCardUpdate,
) -> Result<bool, Error> {
    let old = mission
        .input_cards
        .iter()
        .find(|c| c.id == update.id)
        .ok_or_else(|| Error::new("not_found", "Input card"))?;
    let mut next = old.clone();
    if let Some(title) = update.title {
        next.title = title;
    }
    if let Some(goal) = update.goal {
        next.goal = goal;
    }
    if let Some(mut files) = update.files {
        crate::authoring_create::file_refs(&mut files)?;
        next.files = files;
    }
    if let Some(hint) = update.input_hint {
        next.input_hint = hint;
    }
    if let Some(channel) = update.human_channel {
        next.human_channel = match channel {
            HumanChannel::Notify => HumanChannel::Unset,
            channel => channel,
        };
    }
    if let Some(mut inputs) = update.inputs {
        let mut seen = HashSet::new();
        for input in &mut inputs {
            input.description = input.description.trim().into();
            normalize_name(
                &mut input.name,
                &mut input.kind,
                &mut seen,
                "invalid_mission_input",
            )?;
        }
        next.inputs = inputs;
    }
    if let Some(mut outputs) = update.outputs {
        let mut seen = HashSet::new();
        for output in &mut outputs {
            output.description = output.description.trim().into();
            output.from = output.from.trim().into();
            normalize_name(
                &mut output.name,
                &mut output.kind,
                &mut seen,
                "invalid_mission_output",
            )?;
            if !output.from.is_empty() && crate::graph::split_endpoint(&output.from).is_none() {
                return Err(Error::new(
                    "invalid_mission_output",
                    "from must name a card and port",
                ));
            }
        }
        next.outputs = outputs;
    }
    if old == &next {
        return Ok(false);
    }
    let table = doc["input_card"]
        .as_array_of_tables_mut()
        .and_then(|a| {
            a.iter_mut()
                .find(|t| t.get("id").and_then(Item::as_str) == Some(update.id.as_str()))
        })
        .ok_or_else(|| Error::new("not_found", "Input card"))?;
    for (key, before, after, optional) in [
        ("title", old.title.as_str(), next.title.as_str(), false),
        ("goal", old.goal.as_str(), next.goal.as_str(), false),
        (
            "input_hint",
            old.input_hint.as_str(),
            next.input_hint.as_str(),
            true,
        ),
        (
            "human_channel",
            old.human_channel.as_str(),
            next.human_channel.as_str(),
            true,
        ),
    ] {
        if before != after {
            if optional && after.is_empty() {
                table.remove(key);
            } else {
                crate::write::set(table, key, value(after));
            }
        }
    }
    if old.files != next.files {
        if next.files.is_empty() {
            table.remove("files");
        } else {
            crate::write::set(table, "files", list_value(&next.files)?);
        }
    }
    if old.inputs != next.inputs {
        if next.inputs.is_empty() {
            table.remove("inputs");
        } else {
            crate::write::set(table, "inputs", list_value(&next.inputs)?);
        }
    }
    if old.outputs != next.outputs {
        if next.outputs.is_empty() {
            table.remove("outputs");
        } else {
            crate::write::set(table, "outputs", list_value(&next.outputs)?);
        }
    }
    Ok(true)
}
