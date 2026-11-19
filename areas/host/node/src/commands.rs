//! The host and service commands shared by CLI and MCP.
use std::{sync::Arc, time::Duration};

use ahjo_core::Error;
use ahjo_host_contract::{BAD_REQUEST, SERVICE_FAILED};
use ahjo_node_kit::{
    Command, Effect,
    args::{object, text},
    events::Log,
};
use serde_json::{Value, json};

use crate::host::{self, services::Services};

pub(crate) fn commands(services: Arc<Services>, events: Arc<Log>, node: String) -> Vec<Command> {
    [
        (
            "host_show",
            Effect::Reads,
            "This node's host facts and current reading (CPU, memory, swap, disks, GPUs). \
             --full adds the heaviest processes and recent events.",
        ),
        ("service_list", Effect::Reads, "List this node's services and their state."),
        (
            "service_show",
            Effect::Reads,
            "Show one service and the state of its units.",
        ),
        ("service_start", Effect::Sets, "Start every unit of a service."),
        ("service_stop", Effect::Sets, "Stop every unit of a service."),
        (
            "service_restart",
            Effect::Writes,
            "Stop every unit of a service, then start them.",
        ),
        (
            "service_log",
            Effect::Reads,
            "The last lines a service's units logged, oldest first.",
        ),
    ]
    .into_iter()
    .map(|(name, effect, description)| {
        let (services, events, node) = (services.clone(), events.clone(), node.clone());
        let schema = move || {
            if name == "host_show" {
                object(
                    json!({"full": {"type": "boolean", "description": "Add the heaviest processes and recent events."}}),
                    &[],
                )
            } else if name == "service_list" {
                object(json!({}), &[])
            } else if name == "service_log" {
                object(
                    json!({
                        "id": {"type": "string", "description": "The service id."},
                        "lines": {"type": "integer", "description": "How many lines (40 if not said, at most 200)."}
                    }),
                    &["id"],
                )
            } else {
                object(
                    json!({"id": {"type": "string", "description": "The service id."}}),
                    &["id"],
                )
            }
        };
        Command::new(name, effect, description, schema, move |args: Value| {
            let (services, events, node) = (services.clone(), events.clone(), node.clone());
            async move {
                if name == "host_show" {
                    let mut state = host::now(Duration::from_millis(500), &events).await;
                    if !args["full"].as_bool().unwrap_or(false)
                        && let Some(fields) = state.as_object_mut()
                    {
                        fields.remove("events");
                        if let Some(latest) = fields.get_mut("latest").and_then(Value::as_object_mut) {
                            latest.remove("heavy");
                        }
                    }
                    state["node"] = json!(node);
                    return Ok(state);
                }
                if name == "service_list" {
                    let services = services.list().await.map_err(|e| e.error())?;
                    return Ok(json!({"node": node, "services": services}));
                }
                let id = text(&args, "id").map_err(|e| Error::new(BAD_REQUEST, e.to_string()))?;
                if name == "service_show" {
                    let service = services.one(id).await.map_err(|e| e.error())?;
                    return Ok(json!({"node": node, "service": service}));
                }
                if name == "service_log" {
                    let lines = args["lines"]
                        .as_u64()
                        .map_or(host::services::LOG_LINES, |n| n as usize);
                    let log = services.log(id, lines).await.map_err(|e| e.error())?;
                    return Ok(json!({"node": node, "lines": log.lines, "errors": log.errors}));
                }
                let action = name.strip_prefix("service_").expect("service command");
                let (service, errors, outcome) =
                    services.act(id, action).await.map_err(|e| e.error())?;
                if !errors.is_empty() {
                    return Err(Error::new(
                        SERVICE_FAILED,
                        format!("{action} {outcome:?}: {}", errors.join("; ")),
                    ));
                }
                Ok(json!({"node": node, "service": service, "errors": errors, "outcome": outcome}))
            }
        })
    })
    .collect()
}
