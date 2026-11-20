//! Mission edit envelopes and diagnostics for the shared mutation type.
use crate::MissionChange;
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};
use std::sync::OnceLock;

/// A single change sent by the app, carrying the revision it was read at.
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct SingleEdit<'a> {
    pub expected_rev: i64,
    pub updated_by: &'a str,
    #[serde(flatten)]
    pub change: &'a MissionChange,
}

pub struct EditRequest {
    pub rev: Option<i64>,
    pub updated_by: String,
    pub changes: Changes,
}

pub enum Changes {
    Single(Box<MissionChange>),
    Batch(Vec<MissionChange>),
}

impl<'de> Deserialize<'de> for EditRequest {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let body = Value::deserialize(deserializer)?;
        decode_request(&body).map_err(serde::de::Error::custom)
    }
}

impl Serialize for EditRequest {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match &self.changes {
            Changes::Single(change) => {
                #[derive(Serialize)]
                #[serde(rename_all = "snake_case")]
                struct Single<'a> {
                    #[serde(skip_serializing_if = "Option::is_none")]
                    expected_rev: Option<i64>,
                    #[serde(skip_serializing_if = "str::is_empty")]
                    updated_by: &'a str,
                    #[serde(flatten)]
                    change: &'a MissionChange,
                }
                Single {
                    expected_rev: self.rev,
                    updated_by: &self.updated_by,
                    change,
                }
                .serialize(serializer)
            }
            Changes::Batch(changes) => {
                #[derive(Serialize)]
                struct Batch<'a> {
                    rev: i64,
                    changes: &'a [MissionChange],
                }
                let rev = self.rev.ok_or_else(|| {
                    serde::ser::Error::custom("a batch needs the mission revision")
                })?;
                Batch { rev, changes }.serialize(serializer)
            }
        }
    }
}

impl JsonSchema for EditRequest {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "MissionEditRequest".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let operation = generator.subschema_for::<MissionChange>();
        schemars::json_schema!({
            "type": "object",
            "description": format!(
                "Send one operation, or a batch as {{\"rev\": n, \"changes\": [<operation>, ...]}}. Ids are those mission show --full gives. Operations and fields: {}.",
                operation_help()
            ),
            "anyOf": [
                operation,
                {
                    "type": "object",
                    "required": ["rev", "changes"],
                    "additionalProperties": false,
                    "properties": {
                        "rev": generator.subschema_for::<i64>(),
                        "changes": {"type": "array", "minItems": 1, "items": operation}
                    }
                }
            ]
        })
    }
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
                decode_operation(change)
                    .map_err(|error| format!("change at index {index}: {error}"))
            })
            .collect::<Result<_, _>>()?;
        return Ok(EditRequest {
            rev: Some(rev),
            updated_by: String::new(),
            changes: Changes::Batch(changes),
        });
    }

    let (change, metadata) = decode_single(map)?;
    Ok(EditRequest {
        rev: metadata.expected_rev,
        updated_by: metadata.updated_by.unwrap_or_default(),
        changes: Changes::Single(Box::new(change)),
    })
}

/// Deserialize one operation, with or without its single-edit metadata.
pub fn decode(body: &Value) -> Result<MissionChange, String> {
    let map = body.as_object().ok_or("expected an object")?;
    let (change, _) = decode_single(map)?;
    Ok(change)
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "snake_case")]
struct Metadata {
    expected_rev: Option<i64>,
    updated_by: Option<String>,
}

fn decode_single(map: &Map<String, Value>) -> Result<(MissionChange, Metadata), String> {
    fn metadata(map: &mut Map<String, Value>) -> Result<Metadata, String> {
        let metadata = ["expected_rev", "updated_by"]
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
            .unwrap_or_default()
    } else {
        Metadata::default()
    };
    let metadata = Metadata {
        expected_rev: outer.expected_rev.or(inner.expected_rev),
        updated_by: outer
            .updated_by
            .filter(|actor| !actor.is_empty())
            .or(inner.updated_by),
    };
    Ok((decode_operation(&Value::Object(change))?, metadata))
}

/// Explain an unknown field with the same schema as help.
fn decode_operation(body: &Value) -> Result<MissionChange, String> {
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
    fn a_single_edit_decodes_with_metadata_at_either_level() {
        for body in [
            json!({"expected_rev":7,"updated_by":"human:operator","create_gate":{"title":"Review"}}),
            json!({"create_gate":{"expected_rev":7,"updated_by":"human:operator","title":"Review"}}),
        ] {
            assert!(
                matches!(decode(&body).unwrap(), MissionChange::CreateGate { title, .. } if title == "Review")
            );
            let request = decode_request(&body).unwrap();
            assert_eq!(request.rev, Some(7));
            assert_eq!(request.updated_by, "human:operator");
            assert!(
                matches!(request.changes, Changes::Single(change) if matches!(change.as_ref(), MissionChange::CreateGate { title, .. } if title == "Review"))
            );
        }
    }

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
        let error = decode(&json!({"add_step": {}})).unwrap_err();
        assert!(error.contains("create_step, update_step"), "{error}");
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
        let error = decode(&json!({"staff_seat": {"device": "local"}})).unwrap_err();
        assert_eq!(
            error,
            "staff_seat: unknown field device; accepted fields: step_id, seat_id, agent, harness, model, effort",
        );
    }

    #[test]
    fn malformed_operation_fields_are_not_silently_dropped() {
        assert!(decode(&json!({"create_end":{"outcome":42}})).is_err());
        assert!(decode(&json!({"create_step":{"x":"100"}})).is_err());
        assert!(decode(&json!({"wire_connection":{"join_if_occupied":"yes"}})).is_err());
    }
    #[test]
    fn gate_without_kinds_defaults_to_human() {
        let MissionChange::CreateGate { kinds, .. } =
            decode(&json!({"create_gate":{"title":"Signoff"}})).unwrap()
        else {
            panic!("expected gate")
        };
        assert_eq!(kinds, [crate::GateKind::Human]);
    }
    #[test]
    fn restore_step_reads_the_stored_shape() {
        let body = json!({"restore_card":{"gate":null,"step":{"id":"f","formation":"solo","title":"Work","input":[{"id":"in","label":"Input"}],"output":[],"seat":[{"id":"s","label":"Agent","controller":false}]}}});
        let MissionChange::RestoreCard {
            card: crate::RestoredCard::Step(step),
            ..
        } = decode(&body).unwrap()
        else {
            panic!("expected a step restore")
        };
        assert_eq!(step.inputs[0].id, "in");
        assert_eq!(step.seats[0].id, "s");
        let mut invalid = body;
        invalid["restore_card"]["gate"] = json!({"id":"g"});
        assert!(decode(&invalid).is_err());
    }
}
