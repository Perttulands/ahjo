//! Mission edit envelopes and diagnostics for the shared mutation type.
use crate::MissionChange;
use serde::Deserialize;
use serde_json::{Map, Value};
use std::sync::OnceLock;

pub struct EditRequest {
    pub rev: Option<i64>,
    pub updated_by: String,
    pub changes: Changes,
}

pub enum Changes {
    Single(Box<MissionChange>),
    Batch(Vec<MissionChange>),
}

/// Accept the historic single-operation form, and the revision-checked batch form.
pub fn decode_request(body: &Value) -> Result<EditRequest, String> {
    let map = body.as_object().ok_or("expected an object")?;
    if map.contains_key("changes") || map.contains_key("rev") {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Batch {
            rev: i64,
            changes: Vec<Value>,
        }
        let Batch { rev, changes } =
            serde_json::from_value(body.clone()).map_err(|error| error.to_string())?;
        if changes.is_empty() {
            return Err("changes must contain at least one operation".into());
        }
        let changes = changes
            .iter()
            .enumerate()
            .map(|(index, change)| {
                decode(change).map_err(|error| format!("change at index {index}: {error}"))
            })
            .collect::<Result<_, _>>()?;
        return Ok(EditRequest {
            rev: Some(rev),
            updated_by: String::new(),
            changes: Changes::Batch(changes),
        });
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Metadata {
        expected_rev: Option<i64>,
        updated_by: Option<String>,
    }
    fn metadata(map: &mut Map<String, Value>) -> Result<Metadata, String> {
        let metadata = ["expectedRev", "updatedBy"]
            .into_iter()
            .filter_map(|key| map.remove(key).map(|value| (key.into(), value)))
            .collect();
        serde_json::from_value(Value::Object(metadata)).map_err(|error| error.to_string())
    }

    let mut change = map.clone();
    let outer = metadata(&mut change)?;
    let inner = if change.len() == 1 {
        change
            .values_mut()
            .next()
            .and_then(Value::as_object_mut)
            .map(metadata)
            .transpose()?
    } else {
        None
    };
    let updated_by = outer.updated_by.filter(|actor| !actor.is_empty());
    Ok(EditRequest {
        rev: outer
            .expected_rev
            .or_else(|| inner.as_ref().and_then(|inner| inner.expected_rev)),
        updated_by: updated_by
            .or_else(|| inner.and_then(|inner| inner.updated_by))
            .unwrap_or_default(),
        changes: Changes::Single(Box::new(decode(&Value::Object(change))?)),
    })
}

/// Deserialize one operation, explaining an unknown field with the same schema as help.
pub fn decode(body: &Value) -> Result<MissionChange, String> {
    serde_json::from_value(body.clone()).map_err(|error| {
        let Some(map) = body.as_object() else {
            return "expected an object".into();
        };
        if map.len() != 1 {
            let names: Vec<_> = map.keys().map(String::as_str).collect();
            let names = if names.is_empty() { "none".into() } else { names.join(", ") };
            return format!("a mission patch names one operation, and this one names {names}: send them one at a time");
        }
        let (name, value) = map.iter().next().expect("one operation");
        let Some(operation) = operations().iter().find(|operation| operation.name == name) else {
            let known: Vec<_> = operations().iter().map(|operation| operation.name).collect();
            return format!("unknown operation {name}; the operations are {}", known.join(", "));
        };
        if let Some(fields) = value.as_object()
            && let Some(field) = fields.keys().find(|field| !operation.fields.contains(&field.as_str()))
        {
            let accepted = if operation.fields.is_empty() {
                "none (expects a string)".into()
            } else {
                operation.fields.join(", ")
            };
            return format!("{name}: unknown field {field}; accepted fields: {accepted}");
        }
        format!("{name}: {error}")
    })
}

pub fn operation_help() -> String {
    operations()
        .iter()
        .map(|operation| {
            if operation.fields.is_empty() {
                format!("{} <string>", operation.name)
            } else {
                format!("{} {{{}}}", operation.name, operation.field_help)
            }
        })
        .collect::<Vec<_>>()
        .join("; ")
}

struct Operation {
    name: &'static str,
    fields: Vec<&'static str>,
    field_help: String,
}

fn operations() -> &'static [Operation] {
    static SCHEMA: OnceLock<Value> = OnceLock::new();
    static HELP: OnceLock<Vec<Operation>> = OnceLock::new();
    HELP.get_or_init(|| {
        let root = SCHEMA.get_or_init(|| schemars::schema_for!(MissionChange).to_value());
        root["oneOf"]
            .as_array()
            .expect("externally tagged enum schema")
            .iter()
            .map(|variant| {
                let (name, schema) = variant["properties"]
                    .as_object()
                    .expect("operation tag")
                    .iter()
                    .next()
                    .expect("single operation tag");
                let mut fields = Vec::new();
                schema_fields(root, schema, &mut fields);
                let field_help = fields
                    .iter()
                    .map(|(field, schema)| field_help(root, field, schema))
                    .collect::<Vec<_>>()
                    .join(", ");
                Operation {
                    name,
                    fields: fields.into_iter().map(|(field, _)| field).collect(),
                    field_help,
                }
            })
            .collect()
    })
}

fn referenced<'a>(root: &'a Value, schema: &'a Value) -> &'a Value {
    match schema["$ref"].as_str() {
        Some(reference) => root
            .pointer(reference.strip_prefix('#').expect("local schema reference"))
            .expect("operation schema exists"),
        None => schema,
    }
}

fn schema_fields<'a>(root: &'a Value, schema: &'a Value, fields: &mut Vec<(&'a str, &'a Value)>) {
    let schema = referenced(root, schema);
    if let Some(properties) = schema["properties"].as_object() {
        for (field, schema) in properties {
            if !fields.iter().any(|(name, _)| name == field) {
                fields.push((field, schema));
            }
        }
    }
    for keyword in ["allOf", "oneOf", "anyOf"] {
        if let Some(parts) = schema[keyword].as_array() {
            for part in parts {
                schema_fields(root, part, fields);
            }
        }
    }
}

fn field_help(root: &Value, field: &str, schema: &Value) -> String {
    let schema = referenced(root, schema);
    if schema.get("items").is_some() {
        return format!("{field}[]");
    }
    let mut fields = Vec::new();
    schema_fields(root, schema, &mut fields);
    if fields.is_empty() {
        return field.to_owned();
    }
    let fields = fields
        .iter()
        .map(|(name, schema)| field_help(root, name, schema))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{field}{{{fields}}}")
}

/// Keep duplicate operation keys visible: a JSON object naming the same edit
/// twice still names two operations and must never silently discard one.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(transparent)]
pub struct UniquePatch(pub Value);
impl<'de> serde::Deserialize<'de> for UniquePatch {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = UniquePatch;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a mission patch object")
            }
            fn visit_map<M: serde::de::MapAccess<'de>>(
                self,
                mut map: M,
            ) -> Result<Self::Value, M::Error> {
                use serde::de::Error;
                let mut result = serde_json::Map::new();
                while let Some((key, value)) = map.next_entry::<String, Value>()? {
                    if result.contains_key(&key) {
                        return Err(M::Error::custom(format!(
                            "a mission patch repeats {key}: send one operation at a time"
                        )));
                    }
                    result.insert(key, value);
                }
                Ok(UniquePatch(Value::Object(result)))
            }
        }
        deserializer.deserialize_map(Visitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn every_operation_described_is_one_decode_takes() {
        for operation in operations() {
            let op = operation.name;
            let value = match op {
                "title" | "prefix" => json!("x"),
                _ => json!({}),
            };
            if let Err(error) = decode(&json!({ op: value })) {
                assert!(!error.contains("unknown operation"), "{op}: {error}");
            }
        }
        let error = decode(&json!({"addFormation": {}})).unwrap_err();
        assert!(
            error.contains("createFormation, updateFormation"),
            "{error}"
        );
    }
    #[test]
    fn every_operation_rejects_unknown_fields_even_if_null() {
        for operation in operations() {
            let op = operation.name;
            let error = decode(&json!({op: {"unexpected": null}})).unwrap_err();
            assert!(
                error.contains(&format!("{op}: unknown field unexpected")),
                "{error}"
            );
            let fields = &operation.fields;
            let expected = if fields.is_empty() {
                "none (expects a string)".to_owned()
            } else {
                fields.join(", ")
            };
            assert!(
                error.ends_with(&format!("accepted fields: {expected}")),
                "{error}"
            );
        }
    }

    #[test]
    fn fields_from_other_operations_are_rejected() {
        let error = decode(&json!({"assignSlot": {"device": "local"}})).unwrap_err();
        assert_eq!(
            error,
            "assignSlot: unknown field device; accepted fields: formationId, slotId, agentId, harness, model, effort",
        );
    }

    #[test]
    fn malformed_operation_fields_are_not_silently_dropped() {
        assert!(decode(&json!({"createEnd":{"outcome":42}})).is_err());
        assert!(decode(&json!({"createFormation":{"x":"100"}})).is_err());
        assert!(decode(&json!({"wireConnection":{"joinIfOccupied":"yes"}})).is_err());
    }
    #[test]
    fn gate_without_kinds_defaults_to_human() {
        let MissionChange::CreateGate { kinds, .. } =
            decode(&json!({"createGate":{"title":"Signoff"}})).unwrap()
        else {
            panic!("expected gate")
        };
        assert_eq!(kinds, ["human"]);
    }
    #[test]
    fn restore_formation_reads_the_public_json_shape() {
        let body = json!({"restoreNode":{"gate":null,"formation":{"id":"f","type":"solo","title":"Work","inputs":[{"id":"in","label":"Input"}],"outputs":[],"slots":[{"id":"s","label":"Agent","controller":false}]}}});
        let MissionChange::RestoreNode {
            node: crate::RestoredNode::Formation(formation),
            ..
        } = decode(&body).unwrap()
        else {
            panic!("expected a formation restore")
        };
        assert_eq!(formation.inputs[0].id, "in");
        assert_eq!(formation.slots[0].id, "s");
        let mut invalid = body;
        invalid["restoreNode"]["gate"] = json!({"id":"g"});
        assert!(decode(&invalid).is_err());
    }
}
