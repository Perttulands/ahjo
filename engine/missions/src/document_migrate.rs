//! Stepwise upgrades for mission definitions and their layout and notes.
//! The caller validates the upgraded text before publishing it.

use ahjo_mission_model::{Agent, HARNESS_DEFAULT_MODEL, Layout, Mission, Notes};
use toml_edit::{DocumentMut, Item, TableLike, Value};

use crate::Error;

#[derive(Clone, Copy)]
pub(crate) enum DocumentKind {
    Mission,
    Layout,
    Notes,
}

impl DocumentKind {
    fn current_schema(self) -> i64 {
        match self {
            Self::Mission => ahjo_mission_model::MISSION_SCHEMA,
            Self::Layout => ahjo_mission_model::LAYOUT_SCHEMA,
            Self::Notes => ahjo_mission_model::NOTES_SCHEMA,
        }
    }

    fn validate(self, text: &str) -> Result<(), Error> {
        match self {
            Self::Mission => Mission::parse(text).map(|_| ()),
            Self::Layout => Layout::parse(text).map(|_| ()),
            Self::Notes => Notes::parse(text).map(|_| ()),
        }
    }
}

/// Returns upgraded and validated TOML, or None when no migration is needed.
pub(crate) fn upgrade_text(
    raw: &str,
    kind: DocumentKind,
    mut agent: impl FnMut(&str, &str, &str) -> Option<Agent>,
) -> Result<Option<String>, Error> {
    let mut document: DocumentMut = raw.parse().map_err(|error: toml_edit::TomlError| {
        Error::new("invalid_definition_source", error.to_string())
    })?;
    let schema = document
        .get("schema")
        .and_then(Item::as_integer)
        .ok_or_else(|| Error::new("unsupported_schema", "document has no schema"))?;
    let current = kind.current_schema();
    if schema == current {
        return Ok(None);
    }
    let mut version = schema;
    while version < current {
        version = match (kind, version) {
            (DocumentKind::Mission | DocumentKind::Layout, 1) => {
                rename_keys(document.as_table_mut(), false)?;
                2
            }
            (DocumentKind::Mission, 2) => {
                upgrade_steps(document.as_table_mut())?;
                3
            }
            (DocumentKind::Mission, 3) => {
                upgrade_seats(document.as_table_mut(), &mut agent)?;
                4
            }
            (DocumentKind::Mission, 4) => {
                upgrade_environment_and_scope(document.as_table_mut())?;
                5
            }
            (DocumentKind::Layout, 2) => {
                rename_field(document.as_table_mut(), "node", "card")?;
                3
            }
            (DocumentKind::Notes, 2) => {
                rename_keys(document.as_table_mut(), false)?;
                3
            }
            _ => {
                return Err(Error::new(
                    "unsupported_schema",
                    format!("no document upgrade from schema {version}"),
                ));
            }
        };
        let decor = document["schema"]
            .as_value()
            .map(|value| value.decor().clone());
        let mut replacement = toml_edit::Value::from(version);
        if let Some(decor) = decor {
            *replacement.decor_mut() = decor;
        }
        *document
            .as_table_mut()
            .get_mut("schema")
            .expect("schema exists") = Item::Value(replacement);
    }
    if version != current {
        return Err(Error::new(
            "unsupported_schema",
            format!("unsupported document schema {schema}"),
        ));
    }
    let upgraded = document.to_string();
    kind.validate(&upgraded)?;
    Ok(Some(upgraded))
}

/// Upgrade the graph's unit of work while retaining titles, ids and prose.
fn upgrade_steps(mission: &mut dyn TableLike) -> Result<(), Error> {
    if let Some(formations) = mission.get_mut("formation") {
        for_table_array(formations, &mut |step| {
            rename_field(step, "type", "formation")
        })?;
    }
    if let Some(gates) = mission.get_mut("gate") {
        for_table_array(gates, &mut |gate| {
            if let Some(kinds) = gate.get_mut("kinds").and_then(Item::as_array_mut) {
                let has_old = kinds.iter().any(|kind| kind.as_str() == Some("formation"));
                let has_new = kinds.iter().any(|kind| kind.as_str() == Some("step"));
                if has_old && has_new {
                    return Err(Error::new(
                        "invalid_definition_source",
                        "gate kinds contain both formation and step",
                    ));
                }
                for value in kinds.iter_mut() {
                    if value.as_str() == Some("formation") {
                        let decor = value.decor().clone();
                        *value = Value::from("step");
                        *value.decor_mut() = decor;
                    }
                }
            }
            Ok(())
        })?;
    }
    rename_field(mission, "formation", "step")
}

fn upgrade_seats(
    mission: &mut dyn TableLike,
    resolve: &mut impl FnMut(&str, &str, &str) -> Option<Agent>,
) -> Result<(), Error> {
    let Some(steps) = mission.get_mut("step") else {
        return Ok(());
    };
    for_table_array(steps, &mut |step| {
        let step_id = step
            .get("id")
            .and_then(Item::as_str)
            .unwrap_or("")
            .to_owned();
        if let Some(seats) = step.get_mut("slot") {
            for_table_array(seats, &mut |seat| {
                seat.remove("agent");
                rename_field(seat, "agent_id", "agent")?;
                let id = seat.get("id").and_then(Item::as_str).unwrap_or("");
                let agent_id = seat.get("agent").and_then(Item::as_str).unwrap_or("");
                if agent_id.is_empty() {
                    return Ok(());
                }
                let agent = resolve(&step_id, id, agent_id);
                // A blank old model always used the harness default. Preserve
                // that choice even when the named agent cannot be read today.
                if agent.as_ref().is_none_or(|agent| !agent.model.is_empty())
                    && seat
                        .get("model")
                        .is_none_or(|model| model.as_str() == Some(""))
                {
                    seat.insert("model", toml_edit::value(HARNESS_DEFAULT_MODEL));
                }
                let Some(agent) = agent else {
                    return Ok(());
                };
                for (field, default) in [
                    ("harness", agent.harness.as_str()),
                    ("effort", agent.effort.as_str()),
                    ("model", agent.model.as_str()),
                ] {
                    if seat.get(field).and_then(Item::as_str) == Some(default) {
                        seat.remove(field);
                    }
                }
                Ok(())
            })?;
        }
        rename_field(step, "slot", "seat")
    })
}

fn upgrade_environment_and_scope(mission: &mut dyn TableLike) -> Result<(), Error> {
    if let Some(steps) = mission.get_mut("step") {
        for_table_array(steps, &mut |step| {
            rename_field(step, "device", "environment")
        })?;
    }
    if let Some(limits) = mission.get_mut("limit") {
        for_table_array(limits, &mut |limit| rename_field(limit, "target", "scope"))?;
    }
    Ok(())
}

fn for_table_array(
    item: &mut Item,
    apply: &mut impl FnMut(&mut dyn TableLike) -> Result<(), Error>,
) -> Result<(), Error> {
    match item {
        Item::ArrayOfTables(tables) => {
            for table in tables.iter_mut() {
                apply(table)?;
            }
        }
        Item::Value(Value::Array(values)) => {
            for value in values.iter_mut() {
                if let Some(table) = value.as_inline_table_mut() {
                    apply(table)?;
                }
            }
        }
        _ => (),
    }
    Ok(())
}

pub(crate) fn rename_field(table: &mut dyn TableLike, old: &str, new: &str) -> Result<(), Error> {
    if !table.contains_key(old) {
        return Ok(());
    }
    if table.contains_key(new) {
        return Err(Error::new(
            "invalid_definition_source",
            format!("both {old} and {new} are present"),
        ));
    }
    let decor = table
        .get_key_value(old)
        .map(|(key, _)| (key.leaf_decor().clone(), key.dotted_decor().clone()));
    let value = table.remove(old).expect("key was present");
    table.insert(new, value);
    if let (Some((leaf, dotted)), Some((mut key, _))) = (decor, table.get_key_value_mut(new)) {
        *key.leaf_decor_mut() = leaf;
        *key.dotted_decor_mut() = dotted;
    }
    Ok(())
}

/// Only keys belonging to the document model are renamed. Values, including
/// text, names, references, and arbitrary user-authored strings, stay as-is.
/// Rename TOML keys recursively while preserving values and key decoration.
/// Use only on structures whose map keys belong to the protocol, rather than
/// free-form maps where a user chooses the keys.
pub(crate) fn rename_camel_keys(document: &mut DocumentMut) -> Result<(), Error> {
    rename_keys(document.as_table_mut(), true)
}

fn rename_keys(table: &mut dyn TableLike, all: bool) -> Result<(), Error> {
    const RENAMES: &[(&str, &str)] = &[
        ("inputCard", "input_card"),
        ("updatedBy", "updated_by"),
        ("updatedAt", "updated_at"),
        ("inputHint", "input_hint"),
        ("humanChannel", "human_channel"),
        ("agentId", "agent_id"),
        ("checkVersion", "check_version"),
        ("checkValue", "check_value"),
        ("warnSeconds", "warn_seconds"),
        ("whenSpent", "when_spent"),
        ("missionId", "mission_id"),
        ("missionRev", "mission_rev"),
        ("createdAt", "created_at"),
        ("editedAt", "edited_at"),
    ];
    let changes: Vec<_> = table
        .iter()
        .filter_map(|(key, _)| {
            let mapped = RENAMES
                .iter()
                .find_map(|(old, new)| (*old == key).then(|| (*new).to_owned()));
            let mapped = mapped.or_else(|| all.then(|| snake_case(key)));
            mapped
                .filter(|new| new != key)
                .map(|new| (key.to_owned(), new))
        })
        .collect();
    for (old, new) in changes {
        if table.contains_key(&new) {
            return Err(Error::new(
                "invalid_definition_source",
                format!("both {old} and {new} are present"),
            ));
        }
        let decor = table
            .get_key_value(&old)
            .map(|(key, _)| (key.leaf_decor().clone(), key.dotted_decor().clone()));
        let value = table.remove(&old).expect("key was present");
        table.insert(&new, value);
        if let (Some((leaf, dotted)), Some((mut key, _))) = (decor, table.get_key_value_mut(&new)) {
            *key.leaf_decor_mut() = leaf;
            *key.dotted_decor_mut() = dotted;
        }
    }
    for (_, item) in table.iter_mut() {
        rename_item(item, all)?;
    }
    Ok(())
}

fn rename_item(item: &mut Item, all: bool) -> Result<(), Error> {
    match item {
        Item::Table(table) => rename_keys(table, all),
        Item::ArrayOfTables(tables) => {
            for table in tables.iter_mut() {
                rename_keys(table, all)?;
            }
            Ok(())
        }
        Item::Value(value) => rename_value(value, all),
        Item::None => Ok(()),
    }
}

fn rename_value(value: &mut Value, all: bool) -> Result<(), Error> {
    match value {
        Value::InlineTable(table) => rename_keys(table, all),
        Value::Array(array) => {
            for value in array.iter_mut() {
                rename_value(value, all)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn snake_case(key: &str) -> String {
    let mut name = String::with_capacity(key.len());
    for c in key.chars() {
        if c.is_ascii_uppercase() {
            name.push('_');
            name.push(c.to_ascii_lowercase());
        } else {
            name.push(c);
        }
    }
    name
}
