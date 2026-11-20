//! Brings run ledgers written before runs carried any work item (#180) up
//! to date, once, when a coordinator opens its state directory. Those
//! ledgers name a GitHub issue as `issue: "owner/repo#N"` on each event and
//! in `run_started`'s data; they now name it as `work`, in the canonical
//! form (`gh:owner/repo#N`). The next steps upgrade dispatch session fields
//! and snake-case the keys in the ledger and its frozen documents.
//! A ledger is rewritten whole, through a synced
//! private temporary file renamed over it, so a crash leaves either the old
//! ledger or the new one.
//!
//! The old admission accepted some issues that name no GitHub issue (`#0`,
//! numbers past `u64`, an owner starting with `-`). Their ledgers are left
//! byte for byte as they were, and only those runs refuse to open, saying
//! why; the rest of the coordinator works.

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
    let changes = [
        upgrade_toml(snapshot, |text| {
            document_migrate::upgrade_text(text, DocumentKind::Mission)
        })?,
        upgrade_toml(bindings, upgrade_bindings)?,
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
    if !text.contains("runId") {
        return Ok(None);
    }
    let error = |error: &dyn std::fmt::Display| {
        crate::Error::new("invalid_definition_source", error.to_string())
    };
    let mut document: toml_edit::DocumentMut = text.parse().map_err(|cause| error(&cause))?;
    document_migrate::rename_camel_keys(&mut document)?;
    let updated = document.to_string();
    if updated == text {
        return Ok(None);
    }
    toml::from_str::<crate::BindingsSnapshot>(&updated).map_err(|cause| error(&cause))?;
    Ok(Some(updated))
}

fn upgrade_ledger(text: &str) -> io::Result<Option<String>> {
    if ![
        "\"runId\"",
        "\"issue\"",
        "\"sessionStem\"",
        "\"sessionRef\"",
    ]
    .iter()
    .any(|key| text.contains(key))
    {
        return Ok(None);
    }
    let mut changed = false;
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let mut event: Map<String, Value> =
            serde_json::from_str(line).map_err(|e| invalid(&e.to_string()))?;
        let legacy_wire = event.contains_key("runId");
        changed |= rename(&mut event)?;
        if let Some(Value::Object(data)) = event.get_mut("data") {
            changed |= rename(data)?;
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
        out.push_str(&serde_json::to_string(&event)?);
        out.push('\n');
    }
    Ok(changed.then_some(out))
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

/// Moves an object's `issue` to `work` as a canonical reference. An empty
/// issue meant none, and so is dropped.
fn rename(object: &mut Map<String, Value>) -> io::Result<bool> {
    let Some(issue) = object.remove("issue") else {
        return Ok(false);
    };
    let issue = issue
        .as_str()
        .ok_or_else(|| invalid("issue is not a string"))?;
    if !issue.is_empty() {
        let work = ahjo_work_ref::parse(issue)
            .ok()
            .and_then(|written| match written {
                ahjo_work_ref::Written::Ref(reference) => Some(reference.to_string()),
                ahjo_work_ref::Written::Bead(_) => None,
            })
            .ok_or_else(|| {
                invalid(&format!(
                    "issue {issue:?} names no GitHub issue; correct it by hand and reopen"
                ))
            })?;
        object.insert("work".into(), Value::String(work));
    }
    Ok(true)
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
        assert_eq!(text, "{\"type\":\"run_started\",\"work\":\"gh:o/r#1\"}\n");
        assert!(!stale.exists());
    }
}
