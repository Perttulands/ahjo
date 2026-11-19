//! Send, through Ahjo Swarm. The Brain app has no sessions of its own: a
//! page's reference goes to the node Swarm is set to (its `app.json`), as
//! `view_send`, and the node hands it to the Swarm app connected to it,
//! which opens Send to Session holding it, so the session is picked where
//! the sessions are.

use crate::citadel::{post_to, split_answer};
use ahjo_app_kit::settings::{self, DEFAULT_NODE, Settings};
use serde_json::{Value, json};
use std::time::Duration;

/// The node the view command waits on answers within five seconds.
const WAIT: Duration = Duration::from_secs(8);

/// The node Ahjo Swarm on this device is set to.
pub fn node() -> String {
    settings::path()
        .filter(|p| p.exists())
        .map(|p| Settings::load(&p).0.node)
        .unwrap_or_else(|| DEFAULT_NODE.to_string())
}

/// What one sends of a page: what an agent reads it by.
pub fn reference(path: &str) -> String {
    format!("brain page {path}")
}

/// Hands `text` to Swarm's Send. Blocking: run it off the UI.
pub fn send(node: &str, text: &str) -> Result<(), String> {
    let call = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": { "name": "view_send", "arguments": { "text": text } },
    });
    let raw = post_to("Ahjo Swarm's node", node, "/mcp", &call, WAIT)?;
    let (status, body) = split_answer(&raw)?;
    if status != 200 {
        return Err(format!("Ahjo Swarm's node refused it ({status})"));
    }
    read_answer(body)
}

/// A tool call's answer: done, or the node's words for why not.
fn read_answer(body: &[u8]) -> Result<(), String> {
    let answer: Value =
        serde_json::from_slice(body).map_err(|_| "the node answered in a way not understood")?;
    let result = &answer["result"];
    if result["isError"] == false {
        return Ok(());
    }
    if let Some(m) = answer["error"]["message"].as_str() {
        return Err(if m.starts_with("Unknown tool") {
            "Ahjo Swarm's node is older than this Send; update Ahjo.".into()
        } else {
            m.to_string()
        });
    }
    let said: Value = result["content"][0]["text"]
        .as_str()
        .and_then(|t| serde_json::from_str(t).ok())
        .unwrap_or(Value::Null);
    Err(match said["code"].as_str() {
        Some("no_app") => "Ahjo Swarm is not open on its node; open it and Send again.".into(),
        _ => said["message"]
            .as_str()
            .unwrap_or("the node refused it")
            .to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_nodes_answer_says_whether_swarm_took_it() {
        let ok = br#"{"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"{}"}],"isError":false}}"#;
        assert_eq!(read_answer(ok), Ok(()));
        let none = br#"{"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"{\"code\":\"no_app\",\"message\":\"No Ahjo Swarm app is connected to this node.\"}"}],"isError":true}}"#;
        assert!(
            read_answer(none)
                .unwrap_err()
                .starts_with("Ahjo Swarm is not open")
        );
        let other = br#"{"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"{\"code\":\"timeout\",\"message\":\"The app did not answer within 5 s.\"}"}],"isError":true}}"#;
        assert_eq!(
            read_answer(other).unwrap_err(),
            "The app did not answer within 5 s."
        );
        let old = br#"{"jsonrpc":"2.0","id":1,"error":{"code":-32602,"message":"Unknown tool: view_send"}}"#;
        assert!(read_answer(old).unwrap_err().contains("older"));
        assert_eq!(reference("knowledge/a/b.md"), "brain page knowledge/a/b.md");
    }
}
