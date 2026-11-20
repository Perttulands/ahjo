//! One JSON file per task in the node's own folder, written whole and at
//! once, as CHROTE's store keeps them (`tsk_*.json`).

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result, bail};

use ahjo_scheduled_contract::{SUBMIT_KEY_DISPATCHED, Task, normalize_targets};
use serde_json::{Value, json};

/// Task files that do not read, by id, each with why.
pub type Unreadable = Vec<(String, anyhow::Error)>;

pub struct Store {
    dir: PathBuf,
    files: Mutex<()>,
}

/// `tsk_` and letters, digits, `_` and `-`: the only names a task file has.
pub fn safe_id(id: &str) -> bool {
    id.strip_prefix("tsk_").is_some_and(|rest| {
        !rest.is_empty()
            && rest
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    })
}

impl Store {
    pub fn new(dir: PathBuf) -> Store {
        Store {
            dir,
            files: Mutex::new(()),
        }
    }

    fn path(&self, id: &str) -> Result<PathBuf> {
        if !safe_id(id) {
            bail!("invalid task id");
        }
        Ok(self.dir.join(format!("{id}.json")))
    }

    /// Every task that reads, oldest first, then by id, and apart from
    /// them each file that does not, by id with its error. Only a folder
    /// that cannot be read fails the whole.
    pub fn list_all(&self) -> Result<(Vec<Task>, Unreadable)> {
        let entries = match std::fs::read_dir(&self.dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((vec![], vec![])),
            Err(e) => return Err(e).with_context(|| format!("reading {}", self.dir.display())),
        };
        let mut tasks = vec![];
        let mut broken = vec![];
        for entry in entries {
            let name = entry
                .with_context(|| format!("reading {}", self.dir.display()))?
                .file_name()
                .to_string_lossy()
                .into_owned();
            let Some(id) = name.strip_suffix(".json") else {
                continue;
            };
            if safe_id(id) {
                match self.get(id) {
                    Ok(task) => tasks.extend(task),
                    Err(e) => broken.push((id.to_string(), e)),
                }
            }
        }
        tasks.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
        broken.sort_by(|a, b| a.0.cmp(&b.0));
        Ok((tasks, broken))
    }

    /// The task, rewriting a legacy document once on its first read.
    pub fn get(&self, id: &str) -> Result<Option<Task>> {
        let _files = self.files.lock().unwrap();
        let path = self.path(id)?;
        let raw = match std::fs::read(&path) {
            Ok(raw) => raw,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        let (mut task, migrated) = match serde_json::from_slice::<Task>(&raw) {
            Ok(task) => (task, false),
            Err(current_error) => {
                let task = migrate_legacy(&raw).with_context(|| {
                    format!(
                        "read scheduled task {} (current format: {current_error}; legacy format failed too)",
                        path.display()
                    )
                })?;
                (task, true)
            }
        };
        if task.id != id {
            bail!(
                "scheduled task {} has id {:?}, expected {id:?}",
                path.display(),
                task.id
            );
        }
        task.targets = normalize_targets(task.targets);
        if migrated {
            self.write(&path, &task)
                .with_context(|| format!("migrating scheduled task {}", path.display()))?;
        }
        Ok(Some(task))
    }

    /// Writes the task whole: a reader sees the old file or the new one.
    pub fn save(&self, task: &Task) -> Result<()> {
        let _files = self.files.lock().unwrap();
        self.write(&self.path(&task.id)?, task)
    }

    fn write(&self, path: &Path, task: &Task) -> Result<()> {
        std::fs::create_dir_all(&self.dir)
            .with_context(|| format!("creating {}", self.dir.display()))?;
        let mut raw = serde_json::to_vec_pretty(task)?;
        raw.push(b'\n');
        let tmp = self
            .dir
            .join(format!(".tmp-{}-{}.json", task.id, std::process::id()));
        write_private(&tmp, &raw)?;
        std::fs::rename(&tmp, path).inspect_err(|_| {
            let _ = std::fs::remove_file(&tmp);
        })?;
        Ok(())
    }

    /// Removes the task; `false` if there was none.
    pub fn delete(&self, id: &str) -> Result<bool> {
        let _files = self.files.lock().unwrap();
        match std::fs::remove_file(self.path(id)?) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e.into()),
        }
    }
}

/// Old Ahjo and CHROTE task files used camelCase. Convert the file here
/// so the wire types only accept the current spelling.
fn migrate_legacy(raw: &[u8]) -> Result<Task> {
    let mut old: Value = serde_json::from_slice(raw)?;
    let mut renamed = false;
    legacy_keys(&mut old, &mut renamed)?;
    let task = old.as_object_mut().context("task must be an object")?;

    if let Some(target) = task.shift_remove("target") {
        renamed = true;
        if !target.is_null() {
            task.entry("targets")
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .context("targets must be a list")?
                .push(target);
        }
    }
    if let Some(runs) = task.get_mut("recent_runs").and_then(Value::as_array_mut) {
        for run in runs {
            let Some(targets) = run.get_mut("targets").and_then(Value::as_array_mut) else {
                continue;
            };
            for target in targets {
                let target = target
                    .as_object_mut()
                    .context("run target must be an object")?;
                if let Some(submitted) = target.shift_remove("submitted") {
                    renamed = true;
                    let submitted = serde_json::from_value::<bool>(submitted)?;
                    let dispatched = match target.get("submit_key_dispatched") {
                        Some(value) => serde_json::from_value::<bool>(value.clone())?,
                        None => false,
                    };
                    target.insert(
                        "submit_key_dispatched".into(),
                        json!(submitted || dispatched),
                    );
                }
                if target.get("message").and_then(Value::as_str) == Some("pasted and submitted") {
                    target.insert("message".into(), json!(SUBMIT_KEY_DISPATCHED));
                }
                target.entry("status").or_insert_with(|| json!(""));
            }
        }
    }
    if !renamed {
        bail!("no legacy fields found");
    }
    // Older task documents omitted these fields. Keep the defaults used
    // when those files were read before this migration.
    for (key, default) in [
        ("name", json!("")),
        ("prompt", json!("")),
        ("targets", json!([])),
        ("schedule", json!({ "type": "" })),
        ("enabled", json!(false)),
        ("paused", json!(false)),
        ("created_at", json!(jiff::Timestamp::UNIX_EPOCH)),
        ("updated_at", json!(jiff::Timestamp::UNIX_EPOCH)),
    ] {
        task.entry(key).or_insert(default);
    }
    Ok(serde_json::from_value(old)?)
}

fn legacy_keys(value: &mut Value, renamed: &mut bool) -> Result<()> {
    match value {
        Value::Object(object) => {
            for (key, mut item) in std::mem::take(object) {
                if key.contains('_') {
                    bail!("legacy file contains snake_case field {key:?}");
                }
                // Keep the session labels exactly as stored.
                if key != "sessionIds" {
                    legacy_keys(&mut item, renamed)?;
                }
                let mut snake = String::with_capacity(key.len());
                for ch in key.chars() {
                    if ch.is_ascii_uppercase() {
                        snake.push('_');
                        *renamed = true;
                    }
                    snake.push(ch.to_ascii_lowercase());
                }
                object.insert(snake, item);
            }
        }
        Value::Array(items) => {
            for item in items {
                legacy_keys(item, renamed)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// A task holds prompts: readable by its owner only.
fn write_private(path: &Path, raw: &[u8]) -> Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options
        .open(path)
        .with_context(|| format!("writing {}", path.display()))?;
    file.write_all(raw)?;
    file.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_files_are_upgraded_once_without_losing_targets_or_run_history() {
        let dir = ahjo_testkit::fresh_dir("scheduled-migration");
        let store = Store::new(dir.clone());
        let path = dir.join("tsk_legacy.json");
        std::fs::write(
            &path,
            r#"{
            "id": "tsk_legacy", "name": "old prompt", "prompt": "nudge",
            "target": {"sessionName": " Ops_team ", "unixUser": "alice"},
            "sessionIds": {"alice/Ops_team": "$3 4200:99"},
            "schedule": {"type": "interval", "everyMinutes": 15, "timezone": "UTC"},
            "createdAt": "2026-06-27T14:00:00Z", "updatedAt": "2026-06-27T14:00:00.123456789Z",
            "recentRuns": [{
                "id": "run1", "trigger": "manual", "status": "success",
                "startedAt": "2026-06-27T14:00:01Z", "finishedAt": "2026-06-27T14:00:02Z",
                "targets": [{
                    "sessionName": "Ops_team", "unixUser": "alice", "status": "success",
                    "submitted": true, "message": "pasted and submitted"
                }]
            }],
            "attempt": {"runId": "run2", "trigger": "manual", "startedAt": "2026-06-27T15:00:00Z"}
        }"#,
        )
        .unwrap();

        let task = store.get("tsk_legacy").unwrap().unwrap();
        assert_eq!(task.targets[0].label(), "alice/Ops_team");
        assert_eq!(task.session_ids["alice/Ops_team"], "$3 4200:99");
        assert_eq!(
            task.updated_at.to_string(),
            "2026-06-27T14:00:00.123456789Z"
        );
        let receipt = &task.recent_runs[0].targets[0];
        assert!(receipt.submit_key_dispatched);
        assert_eq!(receipt.message, SUBMIT_KEY_DISPATCHED);
        assert_eq!(task.attempt.as_ref().unwrap().run_id, "run2");

        let saved = std::fs::read(&path).unwrap();
        let document: Value = serde_json::from_slice(&saved).unwrap();
        assert_eq!(document["targets"][0]["session_name"], "Ops_team");
        assert_eq!(document["schedule"]["every_minutes"], 15);
        assert_eq!(
            document["recent_runs"][0]["targets"][0]["submit_key_dispatched"],
            true
        );
        for old in [
            "target",
            "sessionName",
            "unixUser",
            "sessionIds",
            "everyMinutes",
            "updatedAt",
            "recentRuns",
            "submitted",
            "runId",
        ] {
            assert!(!String::from_utf8_lossy(&saved).contains(&format!("\"{old}\"")));
        }

        // A subsequent read preserves the formatting.
        let compact = document.to_string();
        std::fs::write(&path, &compact).unwrap();
        assert_eq!(store.get("tsk_legacy").unwrap(), Some(task));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), compact);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_file_in_neither_format_is_reported_by_name_and_left_intact() {
        let dir = ahjo_testkit::fresh_dir("scheduled-invalid");
        let store = Store::new(dir.clone());
        let path = dir.join("tsk_mixed.json");
        let mixed = r#"{"id":"tsk_mixed","createdAt":"2026-06-27T14:00:00Z","schedule":{"type":"interval","every_minutes":20}}"#;
        std::fs::write(&path, mixed).unwrap();
        let error = store.get("tsk_mixed").unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("tsk_mixed.json"), "{message}");
        assert!(message.contains("every_minutes"), "{message}");
        assert_eq!(std::fs::read_to_string(path).unwrap(), mixed);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn ids_are_task_names_only() {
        assert!(safe_id("tsk_mbx1_ab12"));
        assert!(!safe_id("tsk_"));
        assert!(!safe_id("tsk_../x"));
        assert!(!safe_id("run_1"));
    }
}
