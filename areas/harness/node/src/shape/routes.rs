//! The routes the page uses and the commands an agent uses, over the same
//! reads and edits.

use super::{Roots, edit};
use ahjo_core::Error;
use ahjo_harness_contract::FAILED;
use ahjo_harness_contract::commands::{
    AddHook, AtNode, HookEdited, ReadFile, ReadShape, RemoveHook, SetHook, SetSetting, SwitchHook,
};
use ahjo_harness_contract::shape::{
    FILE, HOOK, HookAt, HookChange, HookEdit, HookSpec, INVALID, NOT_FOUND, PATH, SETTING, STALE,
    SettingEdit, Write,
};
use ahjo_node_kit::{Command, Effect};
use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use schemars::JsonSchema;
use serde::{Serialize, de::DeserializeOwned};
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

pub fn commands(roots: Option<Arc<Roots>>, node: String) -> Vec<Command> {
    let cx = CommandContext { roots, node };
    vec![
        cx.command("harness_shape", Effect::Reads, |r, a: ReadShape| {
            super::read(r, &a.dir, a.harness.as_deref().unwrap_or("claude"))
        }),
        cx.command("harness_read", Effect::Reads, |r, a: ReadFile| {
            edit::read_text(r, &a.path)
        }),
        cx.command("harness_write", Effect::Destroys, |r, a: Write| {
            edit::write_text(r, &a)
        }),
        cx.command("harness_hook_add", Effect::Writes, |r, a: AddHook| {
            change_hook(
                r,
                a.harness,
                a.file,
                HookChange::Add {
                    hook: HookSpec {
                        event: a.event,
                        matcher: a.matcher,
                        command: a.command,
                        timeout: a.timeout,
                    },
                },
            )
        }),
        cx.command("harness_hook_set", Effect::Sets, |r, a: SetHook| {
            change_hook(
                r,
                a.harness,
                a.file,
                HookChange::Set {
                    at: HookAt {
                        event: a.event.clone(),
                        group: a.group,
                        index: a.index,
                        parked: a.parked.unwrap_or_default(),
                    },
                    was: a.was,
                    hook: HookSpec {
                        event: a.new_event.unwrap_or(a.event),
                        matcher: a.new_matcher,
                        command: a.new_command,
                        timeout: a.new_timeout,
                    },
                },
            )
        }),
        cx.command(
            "harness_hook_remove",
            Effect::Destroys,
            |r, a: RemoveHook| {
                change_hook(
                    r,
                    a.harness,
                    a.file,
                    HookChange::Remove {
                        at: HookAt {
                            event: a.event,
                            group: a.group,
                            index: a.index,
                            parked: a.parked.unwrap_or_default(),
                        },
                        was: a.was,
                    },
                )
            },
        ),
        cx.command("harness_hook_switch", Effect::Sets, |r, a: SwitchHook| {
            change_hook(
                r,
                a.harness,
                a.file,
                HookChange::Switch {
                    at: HookAt {
                        event: a.event,
                        group: a.group,
                        index: a.index,
                        parked: a.parked.unwrap_or_default(),
                    },
                    was: a.was,
                    on: a.on,
                },
            )
        }),
        cx.command("harness_setting_set", Effect::Sets, |r, a: SetSetting| {
            let value = if a.unset.unwrap_or_default() {
                Value::Null
            } else {
                let value = a
                    .value
                    .ok_or_else(|| Error::new(INVALID, "value is required, or unset"))?;
                match value {
                    Value::String(text) => {
                        serde_json::from_str(&text).unwrap_or(Value::String(text))
                    }
                    other => other,
                }
            };
            edit::setting_edit(
                r,
                &SettingEdit {
                    harness: a.harness,
                    file: a.file,
                    key: a.key,
                    value,
                },
            )
        }),
    ]
}

fn change_hook(
    roots: &Roots,
    harness: String,
    file: String,
    change: HookChange,
) -> Result<HookEdited, Error> {
    edit::hook_edit(
        roots,
        &HookEdit {
            harness,
            file,
            change,
        },
    )
}

struct CommandContext {
    roots: Option<Arc<Roots>>,
    node: String,
}

impl CommandContext {
    fn command<I, O>(
        &self,
        name: &'static str,
        effect: Effect,
        work: fn(&Roots, I) -> Result<O, Error>,
    ) -> Command
    where
        I: DeserializeOwned + JsonSchema + Send + 'static,
        O: Serialize + JsonSchema + Send + 'static,
    {
        let (roots, node) = (self.roots.clone(), self.node.clone());
        Command::typed(name, effect, move |input| {
            let (roots, node) = (roots.clone(), node.clone());
            async move {
                let roots =
                    roots.ok_or_else(|| Error::new(FAILED, "this node has no home folder"))?;
                let result = blocking(move || work(&roots, input)).await?;
                Ok(AtNode { result, node })
            }
        })
    }
}
