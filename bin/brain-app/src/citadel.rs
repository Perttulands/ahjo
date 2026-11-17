//! Context Citadel's ranked search (`POST /v1/library/search`). Citadel owns
//! the index and the ranking: topics first, typos and half-typed words
//! forgiven. It trusts a tokenless caller on loopback, so the app asks a
//! Citadel on its own machine (or WSL's, forwarded to Windows' loopback)
//! over plain HTTP. One request is one short connection; the client is a
//! few lines of HTTP/1.1 so the app carries no HTTP stack.

use serde::Deserialize;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

/// Citadel's most results; it also stops at its response size.
const RESULTS: usize = 50;
const TIMEOUT: Duration = Duration::from_secs(5);
const ANSWER_LIMIT: u64 = 4 << 20;

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Hit {
    pub path: String,
    pub title: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub summary: String,
    /// Matched words sit between `»` and `«`.
    #[serde(default)]
    pub snippet: String,
}

#[derive(Deserialize)]
struct Answer {
    results: Vec<Hit>,
}

/// `http://host[:port][/prefix]` as the host to dial and the path prefix.
pub fn split_url(url: &str) -> Result<(String, String), String> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| format!("the Citadel URL {url:?} must start with http://"))?;
    let (authority, prefix) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    if authority.is_empty() || authority.contains('@') {
        return Err(format!("the Citadel URL {url:?} names no host"));
    }
    let authority = if authority
        .rsplit_once(':')
        .is_some_and(|(_, p)| !p.contains(']'))
    {
        authority.to_string()
    } else {
        format!("{authority}:80")
    };
    Ok((authority, prefix.trim_end_matches('/').to_string()))
}

/// The pages matching `query`, best first. Blocking: run it off the UI.
pub fn search(url: &str, query: &str) -> Result<Vec<Hit>, String> {
    let (authority, prefix) = split_url(url)?;
    let body = serde_json::json!({ "query": query, "limit": RESULTS }).to_string();
    let request = format!(
        "POST {prefix}/v1/library/search HTTP/1.1\r\nHost: {authority}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let addr = authority
        .to_socket_addrs()
        .map_err(|e| format!("Citadel at {authority}: {e}"))?
        .next()
        .ok_or_else(|| format!("Citadel at {authority} has no address"))?;
    let mut stream = TcpStream::connect_timeout(&addr, TIMEOUT)
        .map_err(|e| format!("Citadel did not answer at {authority}: {e}"))?;
    stream
        .set_read_timeout(Some(TIMEOUT))
        .and_then(|_| stream.set_write_timeout(Some(TIMEOUT)))
        .and_then(|_| stream.write_all(request.as_bytes()))
        .map_err(|e| format!("asking Citadel: {e}"))?;
    let mut answer = vec![];
    stream
        .take(ANSWER_LIMIT)
        .read_to_end(&mut answer)
        .map_err(|e| format!("reading Citadel's answer: {e}"))?;
    parse_answer(&answer)
}

fn parse_answer(raw: &[u8]) -> Result<Vec<Hit>, String> {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("Citadel's answer has no body")?;
    let head = String::from_utf8_lossy(&raw[..split]);
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);
    if status != 200 {
        return Err(format!("Citadel refused the search ({status})"));
    }
    if head
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        return Err("Citadel answered in chunks, which this client does not read".into());
    }
    serde_json::from_slice::<Answer>(&raw[split + 4..])
        .map(|a| a.results)
        .map_err(|_| "Citadel answered with something other than results".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_url() {
        assert_eq!(
            split_url("http://127.0.0.1:3200/").unwrap(),
            ("127.0.0.1:3200".into(), "".into())
        );
        assert_eq!(
            split_url("http://brain.local/citadel").unwrap(),
            ("brain.local:80".into(), "/citadel".into())
        );
        assert!(split_url("https://x").is_err());
    }

    #[test]
    fn asks_and_reads_a_ranked_answer() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let n = s.read(&mut buf).unwrap();
            let asked = String::from_utf8_lossy(&buf[..n]).to_string();
            let body = r#"{"results":[{"id":"a","slug":"a","title":"A","kind":"topic","collection":"knowledge","summary":"s","path":"knowledge/x/a.md","snippet":"»a«"}],"truncated":false}"#;
            write!(
                s,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
            asked
        });
        let hits = search(&format!("http://127.0.0.1:{port}"), "sky").unwrap();
        assert_eq!(hits[0].path, "knowledge/x/a.md");
        assert_eq!(hits[0].snippet, "»a«");
        let asked = server.join().unwrap();
        assert!(asked.starts_with("POST /v1/library/search HTTP/1.1"));
        assert!(
            asked.ends_with(r#"{"limit":50,"query":"sky"}"#) || asked.contains(r#""query":"sky""#)
        );
    }

    #[test]
    fn a_refusal_is_an_error_not_an_empty_answer() {
        assert!(parse_answer(b"HTTP/1.1 403 Forbidden\r\n\r\n{}").is_err());
    }
}
