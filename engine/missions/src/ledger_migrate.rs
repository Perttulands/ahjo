//! Brings run ledgers written before runs carried any work item (#180) up
//! to date, once, when a coordinator opens its state directory. Those
//! ledgers name a GitHub issue as `issue: "owner/repo#N"` on each event and
//! in `run_started`'s data; they now name it as `work`, in the canonical
//! form (`gh:owner/repo#N`). Also updates old dispatch session fields.
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
use std::path::Path;

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
        if let Err(error) = migrate_one(&path) {
            let reason = format!(
                "run ledger {} could not be migrated, \
                 so it was left as it was: {error}",
                path.display()
            );
            eprintln!("ahjo: {reason}");
            refused.insert(id.to_owned(), reason);
        }
    }
    Ok(refused)
}

fn migrate_one(path: &Path) -> io::Result<()> {
    let text = fs::read_to_string(path)?;
    if !["\"issue\"", "\"sessionStem\"", "\"sessionRef\""]
        .iter()
        .any(|key| text.contains(key))
    {
        return Ok(());
    }
    let mut changed = false;
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let mut event: Map<String, Value> =
            serde_json::from_str(line).map_err(|e| invalid(&e.to_string()))?;
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
        out.push_str(&serde_json::to_string(&event)?);
        out.push('\n');
    }
    if !changed {
        return Ok(());
    }
    let temporary = path.with_extension("ndjson.migrating");
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
