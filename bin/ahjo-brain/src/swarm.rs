//! Send, through Ahjo Swarm. The Brain app has no sessions of its own: a
//! page's reference goes to the node Swarm is set to (its `app.json`), as
//! `view_send`, and the node hands it to the Swarm app connected to it,
//! which opens Send to Session holding it, so the session is picked where
//! the sessions are.
//!
//! The other machines whose brains can be read are Swarm's too: the places
//! its fleet last saw, by device, each with the address its node answered at.

use crate::citadel::{post_to, split_answer};
use crate::mirror::Remote;
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

/// The other machines Ahjo Swarm knows, one a device, by name: each read
/// through the first of its nodes with an address.
pub fn places() -> Vec<Remote> {
    let Some(path) = settings::path().filter(|p| p.exists()) else {
        return vec![];
    };
    others(&Settings::load(&path).0, this_machine().as_deref())
}

fn others(s: &Settings, here: Option<&str>) -> Vec<Remote> {
    let mut out: Vec<Remote> = vec![];
    for place in s.places.values() {
        let device = place.device.trim();
        if device.is_empty()
            || place.address.is_empty()
            || here.is_some_and(|h| h.eq_ignore_ascii_case(device))
            || out.iter().any(|r| r.name.eq_ignore_ascii_case(device))
        {
            continue;
        }
        out.push(Remote {
            name: device.to_string(),
            url: format!("http://{}", place.address),
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// This machine's name, as its node names its device: the hostname, in
/// lower case (WSL takes Windows' name).
fn this_machine() -> Option<String> {
    let name = if cfg!(windows) {
        std::env::var("COMPUTERNAME").ok()
    } else {
        std::fs::read_to_string("/proc/sys/kernel/hostname").ok()
    }?;
    Some(name.trim().to_lowercase()).filter(|n| !n.is_empty())
}

/// What one sends of a page: its file, as the Brain tab sent it, which an
/// agent on the node opens.
pub fn reference(file: &str) -> String {
    format!("brain page {file}")
}

/// The file of the page at `path` under `root` as an agent on the node
/// reads it: the brain through a WSL share (Windows) is the distribution's
/// own path.
pub fn file_on_node(root: &str, path: &str) -> String {
    for share in [r"\\wsl.localhost\", r"\\wsl$\"] {
        if let Some((_, inside)) = root.strip_prefix(share).and_then(|r| r.split_once('\\')) {
            let inside = inside.replace('\\', "/");
            return format!("/{}/{path}", inside.trim_end_matches('/'));
        }
    }
    format!("{}/{path}", root.trim_end_matches(['/', '\\']))
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
        assert_eq!(
            reference("/srv/data/context/knowledge/a/b.md"),
            "brain page /srv/data/context/knowledge/a/b.md"
        );
        assert_eq!(
            file_on_node(
                r"\\wsl.localhost\Ubuntu\srv\data\context",
                "knowledge/a/b.md"
            ),
            "/srv/data/context/knowledge/a/b.md"
        );
        assert_eq!(
            file_on_node(r"\\wsl$\Ubuntu\srv\data\context\", "knowledge/a/b.md"),
            "/srv/data/context/knowledge/a/b.md"
        );
        assert_eq!(
            file_on_node("/srv/data/context/", "knowledge/a/b.md"),
            "/srv/data/context/knowledge/a/b.md"
        );
    }

    #[test]
    fn the_other_machines_are_swarms_places_but_this_one() {
        let s: Settings = serde_json::from_value(json!({"places": {
            "n1": {"device": "landmass", "environment": "wsl", "address": "100.64.0.1:7300"},
            "n2": {"device": "rig", "environment": "linux", "address": "100.64.0.2:7300"},
            "n3": {"device": "rig", "environment": "windows", "address": "100.64.0.3:7300"},
            "n4": {"device": "away", "environment": "linux"},
        }}))
        .unwrap();
        let names: Vec<(String, String)> = others(&s, Some("landmass"))
            .into_iter()
            .map(|r| (r.name, r.url))
            .collect();
        assert_eq!(
            names,
            [("rig".to_string(), "http://100.64.0.2:7300".to_string())]
        );
    }
}
