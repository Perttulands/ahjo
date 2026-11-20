//! Typed commands for reading and editing harness configuration.

use super::{Roots, edit};
use ahjo_core::Error;
use ahjo_harness_contract::FAILED;
use ahjo_harness_contract::commands::{
    AddHook, AtNode, HookEdited, ReadFile, ReadShape, RemoveHook, SetHook, SetSetting, SwitchHook,
};
use ahjo_harness_contract::shape::{
    HookAt, HookChange, HookEdit, HookSpec, INVALID, NOT_FOUND, STALE, SettingEdit, Write,
};
use ahjo_node_kit::{Command, Effect};
use axum::http::StatusCode;
use schemars::JsonSchema;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::sync::Arc;

/// Runs `work` off the async threads: it reads and writes files.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, Error> + Send + 'static,
) -> Result<T, Error> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| Error::new(FAILED, e.to_string()))?
}

fn failure_status(error: &Error) -> Option<StatusCode> {
    Some(match error.code.as_str() {
        NOT_FOUND => StatusCode::NOT_FOUND,
        STALE => StatusCode::CONFLICT,
        INVALID | FAILED => StatusCode::BAD_REQUEST,
        _ => return None,
    })
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
        .with_error_status(failure_status)
    }
}
