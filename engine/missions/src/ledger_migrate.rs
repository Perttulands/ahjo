//! Upgrade run ledgers and frozen documents in place, preserving arbitrary
//! authored data. Replacements are written atomically under the coordinator
//! lock, and invalid old records leave only their own run unavailable.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::document_migrate::{self, DocumentKind};

use serde_json::{Map, Value};

/// Migrates every ledger in `runs` with old fields, and
/// returns, by run id, why each one it could not migrate was left as it
/// was. The caller holds the coordinator lock, so nothing appends meanwhile.
pub(crate) fn migrate(runs: &Path) -> io::Result<BTreeMap<String, String>> {
    let mut refused = BTreeMap::new();
    for entry in fs::read_dir(runs)? {
        let path = entry?.path();
        let Some(id) = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_suffix(".ndjson"))
        else {
            continue;
        };
        if let Err(error) = migrate_run(runs, id, &path) {
            let reason = format!(
                "run records for {} could not be migrated: {error}",
                path.display()
            );
            eprintln!("ahjo: {reason}");
            refused.insert(id.to_owned(), reason);
        }
    }
    Ok(refused)
}

fn migrate_run(runs: &Path, id: &str, ledger: &Path) -> io::Result<()> {
    // Validate every file that needs an upgrade before replacing any of them.
    // Publish the ledger last. Startup retries the idempotent conversions if
    // the process stops between replacements.
    let snapshot = runs.join(format!("{id}.snapshot.toml"));
    let bindings = runs.join(format!("{id}.bindings.toml"));
    let frozen = match fs::read_to_string(&bindings) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    let upgraded_bindings = frozen
        .as_deref()
        .map(upgrade_bindings)
        .transpose()
        .map_err(|error| invalid(&format!("{}: {error}", bindings.display())))?
        .flatten();
    let frozen = upgraded_bindings
        .as_deref()
        .or(frozen.as_deref())
        .and_then(|text| toml::from_str::<crate::BindingsSnapshot>(text).ok());
    let changes = [
        upgrade_toml(snapshot, |text| {
            document_migrate::upgrade_text(text, DocumentKind::Mission, |node, seat, agent| {
                frozen
                    .as_ref()?
                    .bindings
                    .iter()
                    .find(|binding| {
                        binding.step_id == node && binding.seat_id == seat && binding.agent == agent
                    })?
                    .agent_definition()
                    .ok()
                    .flatten()
            })
        })?,
        upgraded_bindings.map(|text| (bindings, text)),
        upgrade_ledger(&fs::read_to_string(ledger)?)?.map(|text| (ledger.to_path_buf(), text)),
    ];
    for (path, contents) in changes.into_iter().flatten() {
        replace_private(&path, &contents)?;
    }
    Ok(())
}

fn upgrade_toml(
    path: PathBuf,
    upgrade: impl FnOnce(&str) -> Result<Option<String>, crate::Error>,
) -> io::Result<Option<(PathBuf, String)>> {
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let upgraded =
        upgrade(&text).map_err(|error| invalid(&format!("{}: {error}", path.display())))?;
    Ok(upgraded.map(|text| (path, text)))
}

fn upgrade_bindings(text: &str) -> Result<Option<String>, crate::Error> {
    use toml_edit::{Item, value};
    let error = |error: &dyn std::fmt::Display| {
        crate::Error::new("invalid_definition_source", error.to_string())
    };
    let mut document: toml_edit::DocumentMut = text.parse().map_err(|cause| error(&cause))?;
    if text.contains("runId") {
        document_migrate::rename_camel_keys(&mut document)?;
    }
    let schema = document.get("schema").and_then(Item::as_integer);
    if schema == Some(3) {
        crate::write::table_arrays(&mut document, &["binding"]);
        if let Some(bindings) = document
            .get_mut("binding")
            .and_then(Item::as_array_of_tables_mut)
        {
            for binding in bindings.iter_mut() {
                let id = binding
                    .get("agent_id")
                    .and_then(Item::as_str)
                    .unwrap_or_default()
                    .to_owned();
                if let Some(path) = binding.remove("card_path") {
                    binding.insert("agent_path", path);
                }
                let source = binding.remove("card_toml");
                let digest = binding.remove("card_sha256");
                if let Some(source) = source
                    .as_ref()
                    .and_then(Item::as_str)
                    .filter(|s| !s.is_empty())
                {
                    if digest.as_ref().and_then(Item::as_str)
                        != Some(&crate::run_bindings::hash(source))
                    {
                        return Err(crate::Error::new(
                            "persona_snapshot_invalid",
                            format!("frozen role {id:?} failed its integrity check"),
                        ));
                    }
                    let agent = crate::agents_migration::agent_from_role(&id, source)?;
                    let raw = crate::agents::encode_agent(&agent)?;
                    binding.insert("agent_sha256", value(crate::run_bindings::hash(&raw)));
                    binding.insert("agent_toml", value(raw));
                }
            }
        }
    }
    if matches!(schema, Some(3 | 4)) {
        crate::write::table_arrays(&mut document, &["binding"]);
        if let Some(bindings) = document
            .get_mut("binding")
            .and_then(Item::as_array_of_tables_mut)
        {
            for binding in bindings.iter_mut() {
                document_migrate::rename_field(binding, "slot_id", "seat_id")?;
                document_migrate::rename_field(binding, "agent_id", "agent")?;
            }
        }
    }
    if matches!(schema, Some(3..=5)) {
        crate::write::table_arrays(&mut document, &["binding"]);
        if let Some(bindings) = document
            .get_mut("binding")
            .and_then(Item::as_array_of_tables_mut)
        {
            for binding in bindings.iter_mut() {
                document_migrate::rename_field(binding, "node_id", "step_id")?;
            }
        }
        document["schema"] = value(i64::from(crate::run_bindings::BINDINGS_SCHEMA));
    }
    let updated = document.to_string();
    if updated == text {
        return Ok(None);
    }
    toml::from_str::<crate::BindingsSnapshot>(&updated).map_err(|cause| error(&cause))?;
    Ok(Some(updated))
}

fn upgrade_ledger(text: &str) -> io::Result<Option<String>> {
    let mut changed = false;
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let mut event: Map<String, Value> =
            serde_json::from_str(line).map_err(|e| invalid(&e.to_string()))?;
        let legacy_wire = event.contains_key("runId");
        changed |= rename(&mut event)?;
        let started = event.get("type").and_then(Value::as_str) == Some("run_started");
        if let Some(Value::Object(data)) = event.get_mut("data") {
            if started {
                changed |= rename(data)?;
            }
            changed |= migrate_session(data);
            if let Some(Value::Array(dispatches)) = data.get_mut("openDispatches") {
                for dispatch in dispatches.iter_mut().filter_map(Value::as_object_mut) {
                    changed |= migrate_session(dispatch);
                }
            }
        }
        let mut event = Value::Object(event);
        if legacy_wire {
            changed |= rename_json_keys(&mut event)?;
        }
        changed |= rename_step_event(&mut event)?;
        changed |= rename_seat_event(&mut event, "type")?;
        changed |= rename_card_event(&mut event, "type")?;
        out.push_str(&serde_json::to_string(&event)?);
        out.push('\n');
    }
    Ok(changed.then_some(out))
}

/// Rewrite the recorded graph identities while keeping user-authored text,
/// stable identifiers and named input and output ports intact.
pub(crate) fn rename_card_event(event: &mut Value, kind_field: &str) -> io::Result<bool> {
    let Some(event) = event.as_object_mut() else {
        return Ok(false);
    };
    let mut changed = rename_json_field(event, "node_id", "card_id")?;
    let kind = event.get(kind_field).and_then(Value::as_str).unwrap_or("");
    let new_kind = match kind {
        "node_started" => "card_started",
        "node_output" => "card_output",
        "node_output_pending" => "card_output_pending",
        "node_waiting" => "card_waiting",
        kind => kind,
    };
    let identity = match new_kind {
        "seat_dispatch" | "seat_result" | "adapter_send" | "pick_started" => "step_id",
        _ => "card_id",
    };
    if kind != new_kind {
        let new_kind = new_kind.to_owned();
        event.insert(kind_field.into(), Value::String(new_kind));
        changed = true;
    }
    if let Some(data) = event.get_mut("data") {
        changed |= rename_card_data(data, identity)?;
    }
    Ok(changed)
}

pub(crate) fn rename_dispatch_environment(dispatch: &mut Map<String, Value>) -> io::Result<bool> {
    let mut changed = rename_json_field(dispatch, "node_id", "step_id")?;
    changed |= rename_json_field(dispatch, "place", "environment")?;
    if let Some(turn) = dispatch.get_mut("turn").and_then(Value::as_object_mut) {
        changed |= rename_json_field(turn, "node_id", "step_id")?;
    }
    Ok(changed)
}

fn rename_card_data(value: &mut Value, identity: &str) -> io::Result<bool> {
    let mut changed = false;
    match value {
        Value::Array(items) => {
            for item in items {
                changed |= rename_card_data(item, identity)?;
            }
        }
        Value::Object(fields) => {
            for (old, new) in [
                ("node_id", identity),
                ("node_kind", "card_kind"),
                ("from_node_id", "from_card_id"),
                ("blocked_node_id", "blocked_card_id"),
                ("source_node_id", "source_card_id"),
                ("redispatch_node_id", "redispatch_step_id"),
                ("folder_node", "folder_step_id"),
                ("waiting_nodes", "waiting_cards"),
                ("place", "environment"),
            ] {
                changed |= rename_json_field(fields, old, new)?;
            }
            for (field, old, new) in [
                ("state", "waiting_for_device", "waiting_for_environment"),
                ("code", "reachable_node_starved", "reachable_card_starved"),
            ] {
                if let Some(value) = fields.get_mut(field)
                    && *value == old
                {
                    *value = Value::String(new.into());
                    changed = true;
                }
            }
            for (field, nested) in fields {
                // Named inputs and ports are dictionaries selected by the
                // mission author. Neither their keys nor their payloads are
                // graph identifiers.
                if matches!(field.as_str(), "inputs" | "outputs") && nested.is_object() {
                    continue;
                }
                let identity = match field.as_str() {
                    "open_dispatches" | "pending_output" | "pick" | "picks" | "turn" => "step_id",
                    _ => "card_id",
                };
                changed |= rename_card_data(nested, identity)?;
            }
        }
        _ => {}
    }
    Ok(changed)
}

fn rename_json_field(fields: &mut Map<String, Value>, old: &str, new: &str) -> io::Result<bool> {
    let Some(previous) = fields.remove(old) else {
        return Ok(false);
    };
    if fields.contains_key(new) {
        return Err(invalid(&format!("both {old} and {new} are present")));
    }
    fields.insert(new.into(), previous);
    Ok(true)
}

pub(crate) fn rename_seat_event(event: &mut Value, kind: &str) -> io::Result<bool> {
    let Some(event) = event.as_object_mut() else {
        return Ok(false);
    };
    let mut changed = rename_seat_fields(event)?;
    if let Some(field) = event.get_mut(kind) {
        let updated = match field.as_str() {
            Some("slot_dispatch") => Some("seat_dispatch"),
            Some("slot_result") => Some("seat_result"),
            _ => None,
        };
        if let Some(updated) = updated {
            *field = Value::String(updated.into());
            changed = true;
        }
    }
    let Some(data) = event.get_mut("data").and_then(Value::as_object_mut) else {
        return Ok(changed);
    };
    changed |= rename_seat_fields(data)?;
    if let Some(controller) = data.get_mut("controller").and_then(Value::as_object_mut) {
        changed |= rename_seat_fields(controller)?;
    }
    for field in ["workers", "open_dispatches"] {
        if let Some(entries) = data.get_mut(field).and_then(Value::as_array_mut) {
            for entry in entries.iter_mut().filter_map(Value::as_object_mut) {
                changed |= rename_seat_fields(entry)?;
            }
        }
    }
    Ok(changed)
}

pub(crate) fn rename_seat_fields(fields: &mut Map<String, Value>) -> io::Result<bool> {
    let mut changed = false;
    for (old, new) in [
        ("slot_id", "seat_id"),
        ("agent_id", "agent"),
        ("controller_slot", "controller_seat"),
    ] {
        if let Some(previous) = fields.remove(old) {
            if fields.contains_key(new) {
                return Err(invalid(&format!("both {old} and {new} are present")));
            }
            fields.insert(new.into(), previous);
            changed = true;
        }
    }
    Ok(changed)
}

/// Change only protocol fields, keeping IDs, inputs, outputs and prose exactly
/// as the recorded run wrote them.
fn rename_step_event(event: &mut Value) -> io::Result<bool> {
    let kind = event
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let Some(data) = event.get_mut("data").and_then(Value::as_object_mut) else {
        return Ok(false);
    };
    let mut changed = false;
    if kind == "run_started"
        && let Some(step) = data.remove("formation_id")
    {
        if data.contains_key("step_id") {
            return Err(invalid("run_started has both formation_id and step_id"));
        }
        data.insert("step_id".into(), step);
        changed = true;
    }
    if kind == "node_started"
        && data
            .get("node_kind")
            .is_some_and(|value| value == "formation")
    {
        data.insert("node_kind".into(), Value::String("step".into()));
        changed = true;
    }
    if kind == "gate_kind_result" && data.get("kind").is_some_and(|value| value == "formation") {
        data.insert("kind".into(), Value::String("step".into()));
        changed = true;
    }
    if matches!(
        kind.as_str(),
        "gate_kind_result" | "gate_verdict" | "human_input_requested"
    ) {
        changed |= rename_evidence(data.get_mut("evidence"));
        for field in ["kind_result_seqs", "per_kind"] {
            if let Some(kinds) = data.get_mut(field).and_then(Value::as_object_mut)
                && let Some(previous) = kinds.remove("formation")
            {
                if kinds.contains_key("step") {
                    return Err(invalid(&format!("{field} has both formation and step")));
                }
                kinds.insert("step".into(), previous);
                changed = true;
            }
        }
        if let Some(recommendation) = data
            .get_mut("pick")
            .and_then(Value::as_object_mut)
            .and_then(|pick| pick.get_mut("recommendation"))
            .and_then(Value::as_object_mut)
        {
            changed |= rename_evidence(recommendation.get_mut("evidence"));
        }
    }
    Ok(changed)
}

fn rename_evidence(evidence: Option<&mut Value>) -> bool {
    let Some(items) = evidence.and_then(Value::as_array_mut) else {
        return false;
    };
    let mut changed = false;
    for item in items.iter_mut().filter_map(Value::as_object_mut) {
        if item.get("kind").is_some_and(|value| value == "formation") {
            item.insert("kind".into(), Value::String("step".into()));
            changed = true;
        }
    }
    changed
}

/// Upgrade a protocol JSON object, retaining dictionary keys that name
/// ports or input values. The record types read the result strictly.
pub(crate) fn rename_json_keys(value: &mut Value) -> io::Result<bool> {
    fn rename(value: &mut Value, dictionary: bool) -> io::Result<bool> {
        let mut changed = false;
        match value {
            Value::Array(items) => {
                for item in items {
                    changed |= rename(item, false)?;
                }
            }
            Value::Object(fields) => {
                // Keep the order of untouched fields and reject a collision
                // rather than dropping one of the original values.
                let original = std::mem::take(fields);
                for (key, mut value) in original {
                    let updated = if dictionary
                        || !key.starts_with(|c: char| c.is_ascii_lowercase())
                        || !key.bytes().any(|b| b.is_ascii_uppercase())
                    {
                        key.clone()
                    } else {
                        let mut snake = String::with_capacity(key.len() + 2);
                        for c in key.chars() {
                            if c.is_ascii_uppercase() {
                                snake.push('_');
                                snake.push(c.to_ascii_lowercase());
                            } else {
                                snake.push(c);
                            }
                        }
                        snake
                    };
                    changed |= updated != key;
                    if !dictionary && key == "trigger" && value == "whenSpent" {
                        value = Value::String("when_spent".into());
                        changed = true;
                    }
                    let dynamic = !dictionary
                        && matches!(key.as_str(), "outputs" | "inputs")
                        && value.is_object();
                    changed |= rename(&mut value, dynamic)?;
                    if fields.insert(updated.clone(), value).is_some() {
                        return Err(invalid(&format!("more than one key maps to {updated:?}")));
                    }
                }
            }
            _ => (),
        }
        Ok(changed)
    }

    rename(value, false)
}

fn replace_private(path: &Path, out: &str) -> io::Result<()> {
    let mut name = path.as_os_str().to_owned();
    name.push(".migrating");
    let temporary = PathBuf::from(name);
    // A temporary left by a crash is replaced, never reused with its mode.
    match fs::remove_file(&temporary) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Private, as the ledger was created (ledger.rs).
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    file.write_all(out.as_bytes())?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temporary, path)?;
    #[cfg(unix)]
    fs::File::open(path.parent().unwrap_or(Path::new(".")))?.sync_all()?;
    Ok(())
}

/// Canonicalize a tracked issue without touching arbitrary event data.
fn rename(object: &mut Map<String, Value>) -> io::Result<bool> {
    let old = object.remove("work");
    let renamed = old.is_some();
    if old.is_some() && object.contains_key("issue") {
        return Err(invalid("both work and issue are present"));
    }
    let Some(value) = old.or_else(|| object.remove("issue")) else {
        return Ok(false);
    };
    let issue = value
        .as_str()
        .ok_or_else(|| invalid("issue is not a string"))?;
    let canonical = if issue.is_empty() {
        String::new()
    } else {
        ahjo_issue_ref::parse_resolved(issue)
            .map_err(|e| invalid(&e.to_string()))?
            .to_string()
    };
    let changed = canonical != issue || renamed;
    object.insert("issue".into(), Value::String(canonical));
    Ok(changed)
}

fn migrate_session(data: &mut Map<String, Value>) -> bool {
    let dispatch = data.remove("sessionStem").is_some();
    let Some(reference) = data.remove("sessionRef") else {
        return dispatch;
    };
    if let Some(name) = reference
        .as_str()
        .and_then(|value| value.strip_prefix("terminal:"))
    {
        if dispatch {
            data.entry("session")
                .or_insert_with(|| Value::String(name.into()));
        }
        data.entry("session_ref").or_insert(reference);
    }
    // Old lab references carried a slot or role id, with no session name
    // to recover from the reference.
    true
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_owned())
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    /// A migrated ledger stays private, even when the ledger was readable
    /// by others and a crash left a readable temporary behind.
    #[test]
    fn a_migrated_ledger_is_private() {
        let dir = std::env::temp_dir().join(format!("ahjo-180-mode-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let ledger = dir.join("run_a.ndjson");
        fs::write(&ledger, "{\"type\":\"run_started\",\"issue\":\"o/r#1\"}\n").unwrap();
        let stale = dir.join("run_a.ndjson.migrating");
        fs::write(&stale, "torn").unwrap();
        for path in [&ledger, &stale] {
            fs::set_permissions(path, fs::Permissions::from_mode(0o644)).unwrap();
        }

        assert!(migrate(&dir).unwrap().is_empty());
        let mode = fs::metadata(&ledger).unwrap().permissions().mode() & 0o777;
        let text = fs::read_to_string(&ledger).unwrap();
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(mode, 0o600);
        assert_eq!(text, "{\"type\":\"run_started\",\"issue\":\"gh:o/r#1\"}\n");
        assert!(!stale.exists());
    }
    #[test]
    fn formation_ledger_fields_upgrade_without_changing_authored_values() {
        let original = concat!(
            r#"{"type":"run_started","data":{"formation_id":"fmn_keep","inputs":{"formation_id":"formation","note":"formation"},"reason":"formation"}}"#,
            "\n",
            r#"{"type":"node_started","node_id":"fmn_keep","data":{"node_kind":"formation","brief":{"goal":"formation"}}}"#,
            "\n",
            r#"{"type":"gate_kind_result","data":{"kind":"formation","evidence":[{"kind":"formation","text":"formation"}]}}"#,
            "\n",
            r#"{"type":"human_input_requested","data":{"kind_result_seqs":{"formation":4},"per_kind":{"formation":"pass"},"pick":{"recommendation":{"evidence":[{"kind":"formation","text":"formation"}]}}}}"#,
            "\n",
        );
        let updated = upgrade_ledger(original).unwrap().unwrap();
        let events: Vec<Value> = updated
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(events[0]["data"]["step_id"], "fmn_keep");
        assert_eq!(events[0]["data"]["inputs"]["formation_id"], "formation");
        assert_eq!(events[0]["data"]["reason"], "formation");
        assert_eq!(events[1]["data"]["card_kind"], "step");
        assert_eq!(events[1]["data"]["brief"]["goal"], "formation");
        assert_eq!(events[2]["data"]["kind"], "step");
        assert_eq!(events[2]["data"]["evidence"][0]["kind"], "step");
        assert_eq!(events[2]["data"]["evidence"][0]["text"], "formation");
        assert_eq!(events[3]["data"]["kind_result_seqs"]["step"], 4);
        assert_eq!(events[3]["data"]["per_kind"]["step"], "pass");
        assert_eq!(
            events[3]["data"]["pick"]["recommendation"]["evidence"][0]["kind"],
            "step"
        );
        assert_eq!(
            events[3]["data"]["pick"]["recommendation"]["evidence"][0]["text"],
            "formation"
        );
        assert!(upgrade_ledger(&updated).unwrap().is_none());
    }

    #[test]
    fn card_upgrade_keeps_machine_placement_and_authored_port_keys() {
        let original = concat!(
            r#"{"type":"slot_dispatch","node_id":"fmn_work","data":{"node_id":"fmn_work","slot_id":"old-seat","agent_id":"builder"}}"#,
            "\n",
            r#"{"type":"run_blocked","node_id":"fmn_work","data":{"blocked_node_id":"fmn_work","open_dispatches":[{"node_id":"fmn_work","place":"box/linux"}],"limit":{"node_id":"inp_first"},"pending_output":{"node_id":"fmn_work","output":{"outputs":{"node_id":{"text":"node_id","place":"stage"}}}}}}"#,
            "\n",
        );
        let migrated = upgrade_ledger(original).unwrap().unwrap();
        let mut events = migrated
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap());
        let dispatch = events.next().unwrap();
        assert_eq!(dispatch["type"], "seat_dispatch");
        assert_eq!(dispatch["card_id"], "fmn_work");
        assert_eq!(dispatch["data"]["step_id"], "fmn_work");
        assert_eq!(dispatch["data"]["seat_id"], "old-seat");
        assert_eq!(dispatch["data"]["agent"], "builder");
        let blocked = events.next().unwrap();
        assert_eq!(blocked["card_id"], "fmn_work");
        assert_eq!(blocked["data"]["blocked_card_id"], "fmn_work");
        assert_eq!(blocked["data"]["open_dispatches"][0]["step_id"], "fmn_work");
        assert_eq!(
            blocked["data"]["open_dispatches"][0]["environment"],
            "box/linux"
        );
        assert_eq!(blocked["data"]["limit"]["card_id"], "inp_first");
        assert_eq!(blocked["data"]["pending_output"]["step_id"], "fmn_work");
        assert_eq!(
            blocked["data"]["pending_output"]["output"]["outputs"]["node_id"]["place"],
            "stage"
        );
        assert!(upgrade_ledger(&migrated).unwrap().is_none());
    }

    #[test]
    fn tracked_issue_upgrade_is_idempotent_and_leaves_dynamic_maps_alone() {
        for issue in ["", "bead:n1:/srv/x#x-1", "gh:o/r#1"] {
            let line = serde_json::json!({"type":"run_started", "issue":issue,
                "data":{"issue":issue, "inputs":{"work":"keep", "issue":"keep"}}})
            .to_string();
            assert!(upgrade_ledger(&line).unwrap().is_none());
        }
        let line = r#"{"type":"run_started","work":"o/r#1","data":{"work":"o/r#1","inputs":{"work":"keep"}}}"#;
        let migrated = upgrade_ledger(line).unwrap().unwrap();
        let event: Value = serde_json::from_str(&migrated).unwrap();
        assert_eq!(event["issue"], "gh:o/r#1");
        assert_eq!(event["data"]["issue"], "gh:o/r#1");
        assert_eq!(event["data"]["inputs"]["work"], "keep");
        assert!(upgrade_ledger(&migrated).unwrap().is_none());
    }

    #[test]
    fn conflicting_ledger_fields_are_refused() {
        let original = r#"{"type":"run_started","data":{"formation_id":"a","step_id":"b"}}"#;
        assert!(upgrade_ledger(original).is_err());
    }
}

#[cfg(test)]
mod binding_tests {
    use super::upgrade_bindings;

    const ROLE: &str = "schema = 1\n[card]\nid = \"writer\"\nkind = \"specialist\"\nsummary = \"Read the evidence.\"\n";

    #[test]
    fn frozen_roles_become_frozen_agents_without_rebinding_the_run() {
        let digest = crate::run_bindings::hash(ROLE);
        let original = format!(
            r#"schema = 3
run_id = "run_one"
mission_id = "mission_one"
mission_slug = "one"
mission_rev = 4
input_card_id = "input"

[[binding]]
node_id = "step"
slot_id = "writer"
agent_id = "writer"
harness = "openai-codex"
model = ""
effort = "ultra"
card_path = "/fixture/roles/writer.toml"
card_toml = '''
{ROLE}'''
card_sha256 = "{digest}"
"#
        );
        let upgraded = upgrade_bindings(&original).unwrap().unwrap();
        let snapshot: crate::BindingsSnapshot = toml::from_str(&upgraded).unwrap();
        assert_eq!(snapshot.schema, crate::run_bindings::BINDINGS_SCHEMA);
        assert_eq!(snapshot.run_id, "run_one");
        assert_eq!(snapshot.mission_rev, 4);
        let binding = &snapshot.bindings[0];
        assert_eq!(binding.harness, "openai-codex");
        assert_eq!(binding.effort, "ultra");
        assert_eq!(binding.agent_path, "/fixture/roles/writer.toml");
        assert_eq!(
            binding.instructions().as_deref(),
            Some("Read the evidence.")
        );
        assert_eq!(
            binding.agent_sha256,
            crate::run_bindings::hash(&binding.agent_toml)
        );
        assert!(upgrade_bindings(&upgraded).unwrap().is_none());

        let tampered = original.replace("Read the evidence.", "Trust the guess.");
        assert_eq!(
            upgrade_bindings(&tampered).unwrap_err().code,
            "persona_snapshot_invalid"
        );
    }
}
