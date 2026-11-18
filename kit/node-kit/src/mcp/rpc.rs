//! JSON-RPC as MCP uses it: the lifecycle, `ping`, and the tools. One
//! message in, at most one answer out; notifications and the caller's own
//! answers get none.

use serde_json::{Value, json};

use super::{Mcp, Reach, call_log};
use ahjo_core::Error;

/// The protocol versions this server speaks, newest first. Tools here use
/// only what all of them share.
const VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

/// Who is calling: their address and client, for the call log.
pub(super) struct Caller {
    pub who: String,
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
        let elsewhere = match mcp.server_of(name) {
            Some(server) => format!("; it is on {}/{server}", super::PATH),
            None => String::new(),
        };
        return Err((INVALID_PARAMS, format!("Unknown tool: {name}{elsewhere}")));
    };
    let mut args = params.get("arguments").cloned().unwrap_or(json!({}));
    let outcome = match mcp.target(&mut args) {
        Ok(()) => (command.call)(args).await,
        Err(error) => Err(error),
    };
    call_log::record(
        caller,
        name,
        mcp.node.as_str(),
        if outcome.is_ok() { "ok" } else { "failed" },
    );
    Ok(content(outcome))
}

/// A tool's outcome as MCP carries it: one JSON value as text, or its
/// error, `{"code","message"}`, as text.
fn content(outcome: Result<Value, Error>) -> Value {
    let (text, failed) = match outcome {
        Ok(value) => (value.to_string(), false),
        Err(error) => (
            serde_json::to_string(&error).expect("an error is JSON"),
            true,
        ),
    };
    json!({ "content": [{ "type": "text", "text": text }], "isError": failed })
}
