//! The routes the page uses and the commands an agent uses, over the same
//! reads and edits.

use super::{Roots, edit};
use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use parvi_agents_contract::FAILED;
use parvi_agents_contract::shape::{
    FILE, HOOK, HookAt, HookChange, HookEdit, HookSpec, INVALID, NOT_FOUND, PATH, SETTING, STALE,
    SettingEdit, Write,
};
use parvi_core::Error;
use parvi_node_kit::Command;
use parvi_node_kit::args::object;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;

/// Runs `work` off the async threads: it reads and writes files.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, Error> + Send + 'static,
) -> Result<T, Error> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| Error::new(FAILED, e.to_string()))?
}

fn reply<T: serde::Serialize>(result: Result<T, Error>) -> Response {
    match result {
        Ok(v) => Json(json!(v)).into_response(),
        Err(e) => {
            let status = match e.code.as_str() {
                NOT_FOUND => StatusCode::NOT_FOUND,
                STALE => StatusCode::CONFLICT,
                _ => StatusCode::BAD_REQUEST,
            };
            (status, Json(json!({"error": e}))).into_response()
        }
    }
}

fn arg(q: &HashMap<String, String>, key: &str) -> Result<String, Error> {
    q.get(key)
        .cloned()
        .ok_or_else(|| Error::new(INVALID, format!("{key} is required")))
}

pub fn routes(roots: Arc<Roots>) -> Router {
    Router::new()
        .route(PATH, get(shape))
        .route(FILE, get(read).put(write))
        .route(HOOK, post(hook))
        .route(SETTING, post(setting))
        .with_state(roots)
}

async fn shape(State(r): State<Arc<Roots>>, Query(q): Query<HashMap<String, String>>) -> Response {
    reply(
        blocking(move || {
            let dir = arg(&q, "dir")?;
            let harness = q.get("harness").cloned().unwrap_or_else(|| "claude".into());
            super::read(&r, &dir, &harness)
        })
        .await,
    )
}

async fn read(State(r): State<Arc<Roots>>, Query(q): Query<HashMap<String, String>>) -> Response {
    reply(blocking(move || edit::read_text(&r, &arg(&q, "path")?)).await)
}

async fn write(State(r): State<Arc<Roots>>, Json(w): Json<Write>) -> Response {
    reply(blocking(move || edit::write_text(&r, &w)).await)
}

async fn hook(State(r): State<Arc<Roots>>, Json(e): Json<HookEdit>) -> Response {
    reply(blocking(move || edit::hook_edit(&r, &e)).await)
}

async fn setting(State(r): State<Arc<Roots>>, Json(e): Json<SettingEdit>) -> Response {
    reply(blocking(move || edit::setting_edit(&r, &e)).await)
}

fn text(args: &Value, key: &str) -> Result<String, Error> {
    args[key]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| Error::new(INVALID, format!("{key} is required, as a string")))
}

fn number(args: &Value, key: &str) -> Result<usize, Error> {
    args[key]
        .as_u64()
        .map(|n| n as usize)
        .ok_or_else(|| Error::new(INVALID, format!("{key} is required, as a number")))
}

fn at(args: &Value) -> Result<HookAt, Error> {
    Ok(HookAt {
        event: text(args, "event")?,
        group: number(args, "group")?,
        index: number(args, "index")?,
        parked: args["parked"].as_bool().unwrap_or(false),
    })
}

fn spec(args: &Value, prefix: &str) -> Result<HookSpec, Error> {
    let key = |k: &str| format!("{prefix}{k}");
    Ok(HookSpec {
        event: text(args, &key("event")).or_else(|_| text(args, "event"))?,
        matcher: args[key("matcher")].as_str().map(str::to_owned),
        command: text(args, &key("command"))?,
        timeout: args[key("timeout")].as_u64(),
    })
}

/// A value as written on a command line: JSON when it reads as JSON,
/// otherwise the words themselves.
fn written(v: &Value) -> Value {
    match v {
        Value::String(s) => serde_json::from_str(s).unwrap_or_else(|_| Value::String(s.clone())),
        other => other.clone(),
    }
}

pub fn commands(roots: Option<Arc<Roots>>, node: String) -> Vec<Command> {
    let place = json!({"harness":{"type":"string","description":"claude or codex"},"file":{"type":"string","description":"the settings or hooks file, as agents shape lists it"}});
    let where_ = json!({"event":{"type":"string"},"group":{"type":"integer"},"index":{"type":"integer"},"parked":{"type":"boolean","description":"a hook Parvi switched off"},"was":{"type":"string","description":"what the hook runs now, as agents shape shows it"}});
    let merge = |a: &Value, b: Value| {
        let mut m = a.as_object().cloned().unwrap_or_default();
        m.extend(b.as_object().cloned().unwrap_or_default());
        Value::Object(m)
    };
    let defs: Vec<(&'static str, &'static str, Value, Vec<&'static str>)> = vec![
        (
            "agents_shape",
            "Show what shapes an agent started in a folder: its instruction files in load order, skills, hooks, settings and servers, each with where it comes from.",
            json!({"dir":{"type":"string"},"harness":{"type":"string","description":"claude (default) or codex"}}),
            vec!["dir"],
        ),
        (
            "agents_read",
            "Read an instruction, skill or settings file, with the etag a write names.",
            json!({"path":{"type":"string"}}),
            vec!["path"],
        ),
        (
            "agents_write",
            "Replace a file's text if it is still the version read (its etag; empty for a new file).",
            json!({"path":{"type":"string"},"text":{"type":"string"},"etag":{"type":"string"}}),
            vec!["path", "text", "etag"],
        ),
        (
            "agents_hook_add",
            "Add a hook: when the event happens (and the matcher matches), run the command.",
            merge(
                &place,
                json!({"event":{"type":"string"},"matcher":{"type":"string"},"command":{"type":"string"},"timeout":{"type":"integer"}}),
            ),
            vec!["harness", "file", "event", "command"],
        ),
        (
            "agents_hook_set",
            "Change a hook: its event, matcher, command and timeout (new_event, new_matcher, new_command, new_timeout).",
            merge(
                &merge(&place, where_.clone()),
                json!({"new_event":{"type":"string"},"new_matcher":{"type":"string"},"new_command":{"type":"string"},"new_timeout":{"type":"integer"}}),
            ),
            vec![
                "harness",
                "file",
                "event",
                "group",
                "index",
                "was",
                "new_command",
            ],
        ),
        (
            "agents_hook_remove",
            "Remove a hook.",
            merge(&place, where_.clone()),
            vec!["harness", "file", "event", "group", "index", "was"],
        ),
        (
            "agents_hook_switch",
            "Switch a hook off, keeping it to switch back on, or on again.",
            merge(&merge(&place, where_), json!({"on":{"type":"boolean"}})),
            vec!["harness", "file", "event", "group", "index", "was", "on"],
        ),
        (
            "agents_setting_set",
            "Set a setting in a settings file (key dotted, value as JSON or words), or take it out with unset.",
            merge(
                &place,
                json!({"key":{"type":"string"},"value":{"type":"string"},"unset":{"type":"boolean"}}),
            ),
            vec!["harness", "file", "key"],
        ),
    ];
    defs.into_iter()
        .map(|(name, description, props, required)| {
            let (roots, node) = (roots.clone(), node.clone());
            Command::new(
                name,
                description,
                move || object(props.clone(), &required),
                move |args| {
                    let (roots, node) = (roots.clone(), node.clone());
                    async move {
                        let r = roots
                            .ok_or_else(|| Error::new(FAILED, "this node has no home folder"))?;
                        let mut out = blocking(move || run(name, &r, &args)).await?;
                        out["node"] = json!(node);
                        Ok(out)
                    }
                },
            )
        })
        .collect()
}

fn to_json<T: serde::Serialize>(v: Result<T, Error>) -> Result<Value, Error> {
    v.and_then(|x| serde_json::to_value(x).map_err(|e| Error::new(FAILED, e.to_string())))
}

fn run(name: &str, r: &Roots, args: &Value) -> Result<Value, Error> {
    let hook = |change| -> Result<Value, Error> {
        edit::hook_edit(
            r,
            &HookEdit {
                harness: text(args, "harness")?,
                file: text(args, "file")?,
                change,
            },
        )
    };
    match name {
        "agents_shape" => to_json(super::read(
            r,
            &text(args, "dir")?,
            args["harness"].as_str().unwrap_or("claude"),
        )),
        "agents_read" => to_json(edit::read_text(r, &text(args, "path")?)),
        "agents_write" => to_json(edit::write_text(
            r,
            &Write {
                path: text(args, "path")?,
                text: text(args, "text")?,
                etag: args["etag"].as_str().unwrap_or_default().to_owned(),
            },
        )),
        "agents_hook_add" => hook(HookChange::Add {
            hook: spec(args, "")?,
        }),
        "agents_hook_set" => hook(HookChange::Set {
            at: at(args)?,
            was: text(args, "was")?,
            hook: spec(args, "new_")?,
        }),
        "agents_hook_remove" => hook(HookChange::Remove {
            at: at(args)?,
            was: text(args, "was")?,
        }),
        "agents_hook_switch" => hook(HookChange::Switch {
            at: at(args)?,
            was: text(args, "was")?,
            on: args["on"]
                .as_bool()
                .ok_or_else(|| Error::new(INVALID, "on is true or false"))?,
        }),
        "agents_setting_set" => edit::setting_edit(
            r,
            &SettingEdit {
                harness: text(args, "harness")?,
                file: text(args, "file")?,
                key: text(args, "key")?,
                value: if args["unset"].as_bool() == Some(true) {
                    Value::Null
                } else if args["value"].is_null() {
                    return Err(Error::new(INVALID, "value is required, or unset"));
                } else {
                    written(&args["value"])
                },
            },
        ),
        _ => Err(Error::new(FAILED, format!("no command {name}"))),
    }
}
