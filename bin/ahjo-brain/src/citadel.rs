//! Context Citadel's ranked search (`POST /v1/library/search`). Citadel owns
//! the index and the ranking: topics first, typos and half-typed words
//! forgiven. It trusts a tokenless caller on loopback, so the app asks a
//! Citadel on its own machine (or WSL's, forwarded to Windows' loopback)
//! over plain HTTP. One request is one short connection; the client is a
//! few lines of HTTP/1.1 so the app carries no HTTP stack.

use ahjo_app_kit::Error;
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
pub fn split_url(url: &str) -> Result<(String, String), Error> {
    let rest = url.strip_prefix("http://").ok_or_else(|| {
        Error::new(
            "bad_url",
            format!("the URL {url:?} must start with http://"),
        )
    })?;
    let (authority, prefix) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    if authority.is_empty() || authority.contains('@') {
        return Err(Error::new(
            "bad_url",
            format!("the URL {url:?} names no host"),
        ));
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
pub fn search(url: &str, query: &str) -> Result<Vec<Hit>, Error> {
    let body = serde_json::json!({ "query": query, "limit": RESULTS });
    let answer = post(url, "search", &body, TIMEOUT)?;
    parse_answer(&answer)
}

/// What Citadel says of a page written.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Written {
    /// False when the page already read that way.
    pub committed: bool,
    #[serde(default)]
    pub revision: String,
}

/// Writes a page through Citadel, the brain's one writer: it refuses a
/// page another writer has changed and not committed, and runs `brain
/// check` on it before it commits, putting the page back when the check
/// refuses. Blocking: run it off the UI.
pub fn write(url: &str, path: &str, content: &str, message: &str) -> Result<Written, Error> {
    let body = serde_json::json!({ "path": path, "content": content, "message": message });
    let raw = post(url, "write", &body, WRITE_TIMEOUT)?;
    let (status, body) = split_answer(&raw)?;
    if status != 200 {
        return Err(refusal(status, body));
    }
    serde_json::from_slice(body).map_err(|_| {
        Error::new(
            "citadel_unreadable",
            "Citadel answered the write in a way not understood",
        )
    })
}

/// Citadel's own words for a refusal, or its status.
fn refusal(status: u16, body: &[u8]) -> Error {
    #[derive(Deserialize)]
    struct Refused {
        error: String,
    }
    let message = match serde_json::from_slice::<Refused>(body) {
        Ok(r) => r.error.trim().to_string(),
        Err(_) => format!("Citadel refused ({status})"),
    };
    Error::new("citadel_refused", message)
}

/// A commit runs `brain check`, which Citadel gives two minutes.
const WRITE_TIMEOUT: Duration = Duration::from_secs(130);

/// Posts `body` to `/v1/library/<route>` and reads the whole answer.
fn post(
    url: &str,
    route: &str,
    body: &serde_json::Value,
    wait: Duration,
) -> Result<Vec<u8>, Error> {
    post_to("Citadel", url, &format!("/v1/library/{route}"), body, wait)
}

/// Posts JSON to `path` under `url` (Citadel's, or a node's: `who`, for
/// what an error says) and reads the whole answer.
pub fn post_to(
    who: &str,
    url: &str,
    path: &str,
    body: &serde_json::Value,
    wait: Duration,
) -> Result<Vec<u8>, Error> {
    ask(who, "POST", url, path, Some(body), wait, ANSWER_LIMIT)
}

/// One request to `path` under `url`, with a JSON body or none, and its
/// whole answer, read up to `limit` bytes.
pub fn ask(
    who: &str,
    method: &str,
    url: &str,
    path: &str,
    body: Option<&serde_json::Value>,
    wait: Duration,
    limit: u64,
) -> Result<Vec<u8>, Error> {
    let (authority, prefix) = split_url(url)?;
    let body = body.map(|b| b.to_string()).unwrap_or_default();
    let request = format!(
        "{method} {prefix}{path} HTTP/1.1\r\nHost: {authority}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let addr = authority
        .to_socket_addrs()
        .map_err(|e| Error::new("unreachable", format!("{who} at {authority}: {e}")))?
        .next()
        .ok_or_else(|| {
            Error::new(
                "unreachable",
                format!("{who} at {authority} has no address"),
            )
        })?;
    let mut stream = TcpStream::connect_timeout(&addr, TIMEOUT).map_err(|e| {
        Error::new(
            "unreachable",
            format!("{who} did not answer at {authority}: {e}"),
        )
    })?;
    stream
        .set_read_timeout(Some(wait))
        .and_then(|_| stream.set_write_timeout(Some(TIMEOUT)))
        .and_then(|_| stream.write_all(request.as_bytes()))
        .map_err(|e| Error::new("unreachable", format!("asking {who}: {e}")))?;
    let mut answer = vec![];
    stream
        .take(limit)
        .read_to_end(&mut answer)
        .map_err(|e| Error::new("unreachable", format!("reading {who}'s answer: {e}")))?;
    Ok(answer)
}

/// An answer's status and body.
pub fn split_answer(raw: &[u8]) -> Result<(u16, &[u8]), Error> {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| Error::new("unreadable_answer", "the answer has no body"))?;
    let head = String::from_utf8_lossy(&raw[..split]);
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);
    if head
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        return Err(Error::new(
            "unreadable_answer",
            "the answer came in chunks, which this client does not read",
        ));
    }
    Ok((status, &raw[split + 4..]))
}

fn parse_answer(raw: &[u8]) -> Result<Vec<Hit>, Error> {
    let (status, body) = split_answer(raw)?;
    if status != 200 {
        return Err(Error::new(
            "citadel_refused",
            format!("Citadel refused the search ({status})"),
        ));
    }
    serde_json::from_slice::<Answer>(body)
        .map(|a| a.results)
        .map_err(|_| {
            Error::new(
                "citadel_unreadable",
                "Citadel answered with something other than results",
            )
        })
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

    #[test]
    fn a_write_says_why_the_brain_refused_it() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let n = s.read(&mut buf).unwrap();
            let body =
                r#"{"error":"brain check refuses this commit:\nbroken link [[nope]] in a.md"}"#;
            write!(
                s,
                "HTTP/1.1 422 Unprocessable\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
            String::from_utf8_lossy(&buf[..n]).to_string()
        });
        let said = write(&format!("http://127.0.0.1:{port}"), "a.md", "x", "m").unwrap_err();
        assert_eq!(said.code, "citadel_refused");
        assert!(
            said.message.ends_with("broken link [[nope]] in a.md"),
            "{said}"
        );
        let asked = server.join().unwrap();
        assert!(asked.starts_with("POST /v1/library/write HTTP/1.1"));
        assert!(asked.contains(r#""path":"a.md""#));
    }
}
