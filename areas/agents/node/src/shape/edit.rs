//! Changes the page makes to what shapes an agent, each saved to the file
//! it came from: a file's text, a hook, a setting. A write replaces the
//! file whole (a new file beside it, renamed over it), keeps its
//! permissions, and goes through a link to the file it leads to. A text
//! write names the version it replaces; a hook names what it ran when it
//! was read, so nothing changed meanwhile is overwritten.

use super::{Roots, claude, codex, shown};
use parvi_agents_contract::shape::Hook;
use parvi_agents_contract::shape::{
    HookAt, HookChange, HookEdit, HookSpec, INVALID, NOT_FOUND, Origin, STALE, SettingEdit, Text,
    Write,
};
use parvi_core::Error;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// One change at a time on this node.
static WRITING: Mutex<()> = Mutex::new(());

fn lock() -> std::sync::MutexGuard<'static, ()> {
    WRITING.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn etag(bytes: &[u8]) -> String {
    let d = Sha256::digest(bytes);
    d[..12].iter().map(|b| format!("{b:02x}")).collect()
}

fn invalid(m: impl Into<String>) -> Error {
    Error::new(INVALID, m.into())
}

/// The path as given, `~/` meaning the home folder; it must be full.
pub fn path(roots: &Roots, written: &str) -> Result<PathBuf, Error> {
    let p = match written.strip_prefix("~/") {
        Some(rest) => roots.home.dir.join(rest),
        None => PathBuf::from(written),
    };
    if !p.is_absolute() {
        return Err(invalid(format!("{written:?} is not a full path")));
    }
    Ok(p)
}

/// Where the bytes of `p` are: through every link, or `p` for a file
/// that is not there yet.
fn real(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_owned())
}

pub fn read_text(roots: &Roots, written: &str) -> Result<Text, Error> {
    let p = path(roots, written)?;
    let target = super::target(&p).map(|t| shown(&t));
    match std::fs::read(&p) {
        Ok(bytes) => Ok(Text {
            path: shown(&p),
            target,
            etag: etag(&bytes),
            text: String::from_utf8(bytes)
                .map_err(|_| invalid(format!("{} is not text", p.display())))?,
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Text {
            path: shown(&p),
            target,
            ..Text::default()
        }),
        Err(e) => Err(Error::new(NOT_FOUND, format!("{}: {e}", p.display()))),
    }
}

pub fn write_text(roots: &Roots, w: &Write) -> Result<Text, Error> {
    let p = path(roots, &w.path)?;
    let _guard = lock();
    let now = match std::fs::read(&p) {
        Ok(bytes) => etag(&bytes),
        Err(_) => String::new(),
    };
    if now != w.etag {
        return Err(Error::new(
            STALE,
            format!("{} changed since it was read; read it again", p.display()),
        ));
    }
    replace(&p, w.text.as_bytes())?;
    read_text(roots, &w.path)
}

/// Writes `bytes` as the whole of the file `p` leads to.
fn replace(p: &Path, bytes: &[u8]) -> Result<(), Error> {
    let to = real(p);
    let failed = |e: std::io::Error| Error::new(INVALID, format!("{}: {e}", to.display()));
    let dir = to
        .parent()
        .ok_or_else(|| invalid("no folder to write in"))?;
    std::fs::create_dir_all(dir).map_err(failed)?;
    let name = to.file_name().unwrap_or_default().to_string_lossy();
    let temp = dir.join(format!(".{name}.parvi-{}", std::process::id()));
    std::fs::write(&temp, bytes).map_err(failed)?;
    if let Ok(meta) = std::fs::metadata(&to) {
        let _ = std::fs::set_permissions(&temp, meta.permissions());
    }
    std::fs::rename(&temp, &to).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        failed(e)
    })
}

/// A JSON file's value, `{}` when it is not there.
fn json_value(p: &Path) -> Result<Value, Error> {
    match std::fs::read_to_string(p) {
        Ok(text) if text.trim().is_empty() => Ok(json!({})),
        Ok(text) => serde_json::from_str(&text)
            .map_err(|e| invalid(format!("{} does not read as JSON: {e}", p.display()))),
        Err(_) => Ok(json!({})),
    }
}

fn save_json(p: &Path, v: &Value) -> Result<(), Error> {
    let mut text = serde_json::to_string_pretty(v).map_err(|e| invalid(e.to_string()))?;
    text.push('\n');
    replace(p, text.as_bytes())
}

/// Hooks switched off where the harness has no switch: kept aside in the
/// node's folder with the file they came from, to be put back.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
struct Parked {
    harness: String,
    file: String,
    event: String,
    matcher: Option<String>,
    hook: Value,
}

fn parked_list(roots: &Roots) -> Vec<Parked> {
    std::fs::read_to_string(&roots.parked)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_parked(roots: &Roots, list: &[Parked]) -> Result<(), Error> {
    save_json(&roots.parked, &json!(list))
}

/// The hooks of `harness` kept aside, as hooks that are off.
pub fn parked(roots: &Roots, harness: &str) -> Vec<Hook> {
    parked_list(roots)
        .iter()
        .enumerate()
        .filter(|(_, p)| p.harness == harness)
        .map(|(i, p)| {
            let mut h = claude::hook(
                &p.file,
                origin_of(roots, &p.file),
                &p.event,
                p.matcher.clone(),
                &p.hook,
                false,
                0,
                i,
            );
            h.at.parked = true;
            h
        })
        .collect()
}

fn origin_of(roots: &Roots, file: &str) -> Origin {
    if Path::new(file).starts_with(&roots.home.dir)
        && file.contains("/.claude/settings.json")
        && Path::new(file).parent() == Some(&roots.home.dir.join(".claude"))
    {
        Origin::User
    } else if file.ends_with("settings.local.json") {
        Origin::Local
    } else {
        Origin::Project
    }
}

fn handler(spec: &HookSpec, old: Option<&Value>) -> Result<Value, Error> {
    if spec.command.trim().is_empty() {
        return Err(invalid("a hook needs a command"));
    }
    let mut h = old.cloned().unwrap_or_else(|| json!({"type": "command"}));
    h["type"] = json!("command");
    h["command"] = json!(spec.command.trim());
    match spec.timeout {
        Some(t) => h["timeout"] = json!(t),
        None => {
            if let Some(o) = h.as_object_mut() {
                o.remove("timeout");
            }
        }
    }
    Ok(h)
}

fn matcher_of(spec: &HookSpec) -> Option<String> {
    spec.matcher
        .as_deref()
        .map(str::trim)
        .filter(|m| !m.is_empty() && *m != "*")
        .map(str::to_owned)
}

/// Puts `h` under `event` in a settings value: in the group with the same
/// matcher, or a new group at the end.
pub fn insert(v: &mut Value, event: &str, matcher: Option<&str>, h: Value) {
    if !v.is_object() {
        *v = json!({});
    }
    if !v["hooks"].is_object() {
        v["hooks"] = json!({});
    }
    if !v["hooks"][event].is_array() {
        v["hooks"][event] = json!([]);
    }
    let groups = v["hooks"][event].as_array_mut().expect("an array");
    let same = |g: &Value| g["matcher"].as_str().filter(|m| !m.is_empty() && *m != "*") == matcher;
    if let Some(g) = groups.iter_mut().find(|g| same(g)) {
        if !g["hooks"].is_array() {
            g["hooks"] = json!([]);
        }
        g["hooks"].as_array_mut().expect("an array").push(h);
    } else {
        let mut g = json!({"hooks": [h]});
        if let Some(m) = matcher {
            g = json!({"matcher": m, "hooks": g["hooks"].take()});
        }
        groups.push(g);
    }
}

/// Takes the hook at `at` out of a settings value, if it still runs
/// `was`, with its matcher. Empty groups and events go with it.
pub fn take(v: &mut Value, at: &HookAt, was: &str) -> Result<(Option<String>, Value), Error> {
    let gone = || {
        Error::new(
            STALE,
            format!(
                "the {} hook changed since it was read; read it again",
                at.event
            ),
        )
    };
    let groups = v["hooks"][&at.event].as_array_mut().ok_or_else(gone)?;
    let group = groups.get_mut(at.group).ok_or_else(gone)?;
    let matcher = group["matcher"]
        .as_str()
        .filter(|m| !m.is_empty() && *m != "*")
        .map(str::to_owned);
    let list = group["hooks"].as_array_mut().ok_or_else(gone)?;
    let h = list.get(at.index).ok_or_else(gone)?;
    if claude::hook("", Origin::User, &at.event, None, h, true, 0, 0).run != was {
        return Err(gone());
    }
    let h = list.remove(at.index);
    if list.is_empty() {
        groups.remove(at.group);
    }
    if groups.is_empty()
        && let Some(o) = v["hooks"].as_object_mut()
    {
        o.remove(&at.event);
    }
    if v["hooks"].as_object().is_some_and(|o| o.is_empty())
        && let Some(o) = v.as_object_mut()
    {
        o.remove("hooks");
    }
    Ok((matcher, h))
}

pub fn hook_edit(roots: &Roots, edit: &HookEdit) -> Result<Value, Error> {
    let _guard = lock();
    match edit.harness.as_str() {
        "claude" => claude_hook(roots, edit),
        "codex" => codex::hook_edit(roots, edit),
        other => Err(invalid(format!("no harness {other:?}"))),
    }
}

fn claude_hook(roots: &Roots, edit: &HookEdit) -> Result<Value, Error> {
    let file = path(roots, &edit.file)?;
    let parked_at = |at: &HookAt, was: &str, list: &[Parked]| -> Result<usize, Error> {
        let p = list.get(at.index).filter(|p| {
            p.harness == "claude"
                && p.file == edit.file
                && claude::hook("", Origin::User, &p.event, None, &p.hook, false, 0, 0).run == was
        });
        p.map(|_| at.index).ok_or_else(|| {
            Error::new(
                STALE,
                "the switched-off hook changed since it was read; read it again",
            )
        })
    };
    match &edit.change {
        HookChange::Add { hook } => {
            known_event("claude", &hook.event)?;
            let mut v = json_value(&file)?;
            insert(
                &mut v,
                &hook.event,
                matcher_of(hook).as_deref(),
                handler(hook, None)?,
            );
            save_json(&file, &v)?;
        }
        HookChange::Set { at, was, hook } if at.parked => {
            let mut list = parked_list(roots);
            let i = parked_at(at, was, &list)?;
            list[i].hook = handler(hook, Some(&list[i].hook))?;
            list[i].matcher = matcher_of(hook);
            list[i].event = hook.event.clone();
            save_parked(roots, &list)?;
        }
        HookChange::Set { at, was, hook } => {
            known_event("claude", &hook.event)?;
            let mut v = json_value(&file)?;
            let (_, old) = take(&mut v, at, was)?;
            insert(
                &mut v,
                &hook.event,
                matcher_of(hook).as_deref(),
                handler(hook, Some(&old))?,
            );
            save_json(&file, &v)?;
        }
        HookChange::Remove { at, was } if at.parked => {
            let mut list = parked_list(roots);
            let i = parked_at(at, was, &list)?;
            list.remove(i);
            save_parked(roots, &list)?;
        }
        HookChange::Remove { at, was } => {
            let mut v = json_value(&file)?;
            take(&mut v, at, was)?;
            save_json(&file, &v)?;
        }
        HookChange::Switch { at, was, on: true } if at.parked => {
            let mut list = parked_list(roots);
            let i = parked_at(at, was, &list)?;
            let p = list.remove(i);
            let mut v = json_value(&file)?;
            insert(&mut v, &p.event, p.matcher.as_deref(), p.hook);
            save_json(&file, &v)?;
            save_parked(roots, &list)?;
        }
        HookChange::Switch { at, was, on: false } if !at.parked => {
            let mut v = json_value(&file)?;
            let (matcher, h) = take(&mut v, at, was)?;
            let mut list = parked_list(roots);
            list.push(Parked {
                harness: "claude".into(),
                file: edit.file.clone(),
                event: at.event.clone(),
                matcher,
                hook: h,
            });
            save_parked(roots, &list)?;
            save_json(&file, &v)?;
        }
        HookChange::Switch { .. } => {}
    }
    Ok(json!({"file": edit.file}))
}

pub fn known_event(harness: &str, event: &str) -> Result<(), Error> {
    if parvi_agents_contract::shape::events(harness).contains(&event) {
        Ok(())
    } else {
        Err(invalid(format!("{harness} has no hook event {event:?}")))
    }
}

pub fn setting_edit(roots: &Roots, edit: &SettingEdit) -> Result<Value, Error> {
    let file = path(roots, &edit.file)?;
    let keys: Vec<&str> = edit.key.split('.').filter(|k| !k.is_empty()).collect();
    if keys.is_empty() {
        return Err(invalid("a setting needs a key"));
    }
    let _guard = lock();
    match edit.harness.as_str() {
        "claude" => {
            if keys[0] == "hooks" {
                return Err(invalid("hooks are changed as hooks"));
            }
            let mut v = json_value(&file)?;
            set_json(&mut v, &keys, &edit.value);
            save_json(&file, &v)?;
        }
        "codex" => codex::setting_edit(&file, &keys, &edit.value)?,
        other => return Err(invalid(format!("no harness {other:?}"))),
    }
    Ok(json!({"file": edit.file, "key": edit.key}))
}

/// Sets the value at `keys`, making the objects on the way; `null` takes
/// it out, and objects it leaves empty with it.
pub fn set_json(v: &mut Value, keys: &[&str], value: &Value) {
    if !v.is_object() {
        *v = json!({});
    }
    let (last, path) = keys.split_last().expect("a key");
    if value.is_null() {
        fn remove(v: &mut Value, path: &[&str], last: &str) {
            match path.split_first() {
                None => {
                    if let Some(o) = v.as_object_mut() {
                        o.remove(last);
                    }
                }
                Some((first, rest)) => {
                    if let Some(inner) = v.get_mut(*first) {
                        remove(inner, rest, last);
                        if inner.as_object().is_some_and(|o| o.is_empty())
                            && let Some(o) = v.as_object_mut()
                        {
                            o.remove(*first);
                        }
                    }
                }
            }
        }
        remove(v, path, last);
        return;
    }
    let mut at = v;
    for k in path {
        if !at[*k].is_object() {
            at[*k] = json!({});
        }
        at = &mut at[*k];
    }
    at[*last] = value.clone();
}

pub(super) fn replace_file(p: &Path, bytes: &[u8]) -> Result<(), Error> {
    replace(p, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hook_goes_in_and_out_leaving_nothing_empty() {
        let mut v = json!({"model": "x"});
        insert(
            &mut v,
            "PreToolUse",
            Some("Bash"),
            json!({"type":"command","command":"a"}),
        );
        insert(
            &mut v,
            "PreToolUse",
            Some("Bash"),
            json!({"type":"command","command":"b"}),
        );
        insert(
            &mut v,
            "PreToolUse",
            None,
            json!({"type":"command","command":"c"}),
        );
        assert_eq!(v["hooks"]["PreToolUse"][0]["hooks"][1]["command"], "b");
        assert_eq!(v["hooks"]["PreToolUse"][1]["hooks"][0]["command"], "c");
        let at = |group, index| HookAt {
            event: "PreToolUse".into(),
            group,
            index,
            parked: false,
        };
        assert_eq!(take(&mut v, &at(0, 0), "b").unwrap_err().code, STALE);
        take(&mut v, &at(1, 0), "c").unwrap();
        take(&mut v, &at(0, 1), "b").unwrap();
        let (m, h) = take(&mut v, &at(0, 0), "a").unwrap();
        assert_eq!((m.as_deref(), &h["command"]), (Some("Bash"), &json!("a")));
        assert_eq!(v, json!({"model": "x"}));
    }

    #[test]
    fn settings_are_set_deep_and_taken_out_with_null() {
        let mut v = json!({});
        set_json(&mut v, &["permissions", "deny"], &json!(["Bash"]));
        set_json(&mut v, &["model"], &json!("opus"));
        assert_eq!(
            v,
            json!({"permissions": {"deny": ["Bash"]}, "model": "opus"})
        );
        set_json(&mut v, &["permissions", "deny"], &Value::Null);
        assert_eq!(v, json!({"model": "opus"}));
    }
}
