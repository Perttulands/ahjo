//! HTTP for Send attachments: a file read from the node it is on, and
//! written on the target's.

use ahjo_app_kit::host::{Answer, Request};
use ahjo_core::{Error, NodeId};
use std::time::Duration;

/// The most one attachment carries, as the node takes it.
pub const MAX_DROP: usize = 256 << 20;

/// How long a drop may take to be written.
const DROP_WITHIN: Duration = Duration::from_secs(120);

/// A query value, percent-encoded.
pub fn enc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Writes `bytes` as `name` on `node`, readable by the session's Unix
/// `user` (empty is the node's own).
pub fn drop_file(node: &NodeId, user: &str, name: &str, bytes: Vec<u8>) -> Request {
    let mut path = format!("/api/drops?name={}", enc(name));
    if !user.is_empty() {
        path.push_str(&format!("&user={}", enc(user)));
    }
    let mut r = Request::new("POST", node.clone(), path).within(DROP_WITHIN);
    r.body = Some(("application/octet-stream".into(), bytes));
    r
}

/// Reads the file at `path` on `node` (Files' own route).
pub fn file_bytes(node: &NodeId, path: &str) -> Request {
    let route = format!(
        "{}?path={}",
        ahjo_files_contract::FILE_BYTES_PATH,
        enc(path)
    );
    Request::get(node.clone(), route).within(DROP_WITHIN)
}

/// The bytes read, or why there are none.
pub fn bytes_read(answer: Result<Answer, Error>) -> Result<Vec<u8>, String> {
    match answer {
        Ok(a) if a.ok() => Ok(a.body),
        Ok(a) => Err(a.error_text()),
        Err(e) => Err(e.message),
    }
}

/// The dropped file's path on its node, or why there is none.
pub fn dropped(answer: Result<Answer, Error>) -> Result<String, String> {
    match answer {
        Ok(a) if a.status == 201 => serde_json::from_slice::<serde_json::Value>(&a.body)
            .ok()
            .and_then(|v| v.get("path")?.as_str().map(str::to_string))
            .ok_or_else(|| "the node's answer did not read".into()),
        Ok(a) => Err(a.error_text()),
        Err(e) => Err(e.message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drop_names_the_file_and_its_user_on_the_target_node() {
        let r = drop_file(
            &NodeId("other-node".into()),
            "agent",
            "clipboard.png",
            b"image bytes".to_vec(),
        );
        assert_eq!(r.method, "POST");
        assert_eq!(r.node.as_str(), "other-node");
        assert_eq!(r.path, "/api/drops?name=clipboard.png&user=agent");
        assert_eq!(
            r.body,
            Some((
                "application/octet-stream".to_string(),
                b"image bytes".to_vec()
            ))
        );
        let answer = Answer {
            status: 201,
            body: br#"{"path":"/target/drops/clipboard.png"}"#.to_vec(),
        };
        assert_eq!(
            dropped(Ok(answer)).as_deref(),
            Ok("/target/drops/clipboard.png")
        );
    }
}
