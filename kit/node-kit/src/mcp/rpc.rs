//! JSON-RPC as MCP uses it: the lifecycle, `ping`, and the tools. One
//! message in, at most one answer out; notifications and the caller's own
//! answers get none.

use serde_json::{Value, json};

use super::{Mcp, Reach, call_log};
use ahjo_core::Error;

/// The protocol versions this server speaks, newest first. Typed tools include
/// their structured results alongside text for older clients.
const VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

/// Who is calling: their address and client, for the call log.
pub(super) struct Caller {
    pub who: String,
    pub transport: &'static str,
}

type Answer = Result<Value, (i64, String)>;

/// The answer to one message to the servers `reach` names, if it wants one.
pub(super) async fn answer(
    mcp: &Mcp,
    reach: &Reach,
    caller: &Caller,
    message: &Value,
) -> Option<Value> {
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        // An answer from the caller needs none; anything else is malformed.
        return (message.get("result").is_none() && message.get("error").is_none())
            .then(|| reply(&Value::Null, Err((INVALID_REQUEST, "not a request".into()))));
    };
    let id = message.get("id")?;
    let params = message.get("params").unwrap_or(&Value::Null);
    let answer = match method {
        "initialize" => Ok(initialize(params, &mcp.instructions(reach))),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(mcp.list(reach)),
        "tools/call" => call(mcp, reach, caller, params).await,
        _ => Err((METHOD_NOT_FOUND, format!("no method {method}"))),
    };
    Some(reply(id, answer))
}

/// An error answer for a message that could not be read.
pub(super) fn unreadable(error: &serde_json::Error) -> Value {
    reply(&Value::Null, Err((-32700, format!("not JSON: {error}"))))
}

fn reply(id: &Value, answer: Answer) -> Value {
    match answer {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err((code, message)) => {
            json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
        }
    }
}

fn initialize(params: &Value, instructions: &str) -> Value {
    let asked = params.get("protocolVersion").and_then(Value::as_str);
    let version = asked
        .and_then(|asked| VERSIONS.iter().find(|v| **v == asked))
        .unwrap_or(&VERSIONS[0]);
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": { "name": "ahjo", "version": crate::VERSION },
        "instructions": instructions,
    })
}

async fn call(mcp: &Mcp, reach: &Reach, caller: &Caller, params: &Value) -> Answer {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or((INVALID_PARAMS, "name is required".to_owned()))?;
    let Some(command) = mcp.command(reach, name) else {
        // An unknown name could be anything, so the log doesn't echo it.
        call_log::record(caller, "<unknown>", mcp.node.as_str(), "unknown");
        let elsewhere = mcp.elsewhere(reach, name);
        return Err((INVALID_PARAMS, format!("Unknown tool: {name}{elsewhere}")));
    };
    let args = params.get("arguments").cloned().unwrap_or(json!({}));
    let image = mcp
        .answering(command, &args)
        .and_then(|answering| super::image_field(&answering.output_schema))
        .map(str::to_owned);
    let outcome = mcp.invoke(caller, command, args).await;
    Ok(content(outcome, image.as_deref()))
}

/// A tool's outcome as JSON text, plus the matching structured value when
/// the call succeeded. The field `image` names ([`super::image_field`]) is
/// given as image content after the text, and not in the text or the
/// structured value.
fn content(mut outcome: Result<Value, Error>, image: Option<&str>) -> Value {
    let picture = match (&mut outcome, image) {
        (Ok(Value::Object(fields)), Some(field)) => fields.shift_remove(field),
        _ => None,
    };
    let (text, failed) = match &outcome {
        Ok(value) => (value.to_string(), false),
        Err(error) => (
            serde_json::to_string(error).expect("an error is JSON"),
            true,
        ),
    };
    let mut result = json!({ "content": [{ "type": "text", "text": text }], "isError": failed });
    if let Some(picture) = picture {
        result["content"]
            .as_array_mut()
            .expect("content is a list")
            .push(json!({
                "type": "image",
                "data": picture["data"],
                "mimeType": picture["mime_type"],
            }));
    }
    if let Ok(value) = outcome {
        result["structuredContent"] = value;
    }
    result
}
