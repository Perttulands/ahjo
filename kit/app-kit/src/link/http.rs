//! Plain HTTP/1.1 to the serving node, for what pages read and write over
//! HTTP (their areas' routes). No TLS: nodes listen on loopback or the
//! tailnet. A request for another node names it with `?node=`, and the
//! serving node passes it on.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

/// How long an ordinary request may take. An action that runs longer on
/// the node (a service's units) passes its own deadline.
pub const ANSWER: Duration = Duration::from_secs(5);

/// A request that got no answer.
#[derive(Clone, Debug, PartialEq)]
pub struct NoAnswer {
    pub message: String,
    /// The request may have reached the node, so what it asked for may
    /// have happened: a write's outcome is unknown, not failed.
    pub maybe_done: bool,
}

impl std::fmt::Display for NoAnswer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// `host:port` and the `Host` header for a node address.
fn address(base: &str) -> (String, String) {
    let host = base
        .trim()
        .trim_end_matches('/')
        .trim_start_matches("http://")
        .trim_start_matches("ws://")
        .to_string();
    let addr = if host.contains(':') {
        host.clone()
    } else {
        format!("{host}:80")
    };
    (addr, host)
}

/// Any request to the node: `body` is its content type and bytes (a Files
/// upload is not JSON). Answered `within`, or an error saying it was not.
pub async fn send(
    base: &str,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<(&str, &[u8])>,
    within: Duration,
) -> Result<(u16, Vec<u8>), NoAnswer> {
    // Set once any of the request may have left: from then on, a write
    // that gets no answer may still have happened.
    let sent = AtomicBool::new(false);
    let fetch = async {
        let mut answer = exchange(base, method, path, headers, body, &sent).await?;
        let body = answer.body.all().await?;
        Ok((answer.status, body))
    };
    let answer = match tokio::time::timeout(within, fetch).await {
        Ok(a) => a,
        Err(_) => Err(format!("{NO_ANSWER} {} s", within.as_secs())),
    };
    answer.map_err(|message| NoAnswer {
        message,
        maybe_done: sent.load(Ordering::Relaxed),
    })
}

/// A read whose body comes as it is sent: the status and headers once they
/// are in (`within`), then the body a piece at a time ([`Streamed::piece`]),
/// each piece also due `within`, so a body of any size never sits whole in
/// memory.
pub async fn open(
    base: &str,
    path: &str,
    headers: &[(&str, &str)],
    within: Duration,
) -> Result<Streamed, NoAnswer> {
    let sent = AtomicBool::new(false);
    match tokio::time::timeout(within, exchange(base, "GET", path, headers, None, &sent)).await {
        Ok(Ok(mut answer)) => {
            answer.body.within = within;
            Ok(answer)
        }
        Ok(Err(message)) => Err(NoAnswer {
            message,
            maybe_done: false,
        }),
        Err(_) => Err(NoAnswer {
            message: format!("{NO_ANSWER} {} s", within.as_secs()),
            maybe_done: false,
        }),
    }
}

/// An answer whose body is still to be read.
pub struct Streamed {
    pub status: u16,
    /// Its headers as sent, names in lower case.
    pub headers: Vec<(String, String)>,
    body: Body,
}

impl Streamed {
    /// The first header named `name` (any case).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// The body's next piece, or `None` once it has all come.
    pub async fn piece(&mut self) -> Result<Option<Vec<u8>>, String> {
        let within = self.body.within;
        tokio::time::timeout(within, self.body.piece())
            .await
            .map_err(|_| format!("{NO_ANSWER} {} s", within.as_secs()))?
    }

    /// The rest of the body, refused once it would pass `most` bytes.
    pub async fn up_to(&mut self, most: usize) -> Result<Vec<u8>, String> {
        let mut all = Vec::new();
        while let Some(piece) = self.piece().await? {
            if all.len() + piece.len() > most {
                return Err(format!("more than {most} bytes"));
            }
            all.extend_from_slice(&piece);
        }
        Ok(all)
    }
}

async fn exchange(
    base: &str,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<(&str, &[u8])>,
    sent: &AtomicBool,
) -> Result<Streamed, String> {
    let (addr, host) = address(base);
    let mut s = tokio::net::TcpStream::connect(&addr)
        .await
        .map_err(|e| e.to_string())?;
    sent.store(method != "GET", Ordering::Relaxed);
    let length = match body {
        Some((kind, b)) => format!("Content-Type: {kind}\r\nContent-Length: {}\r\n", b.len()),
        None if method == "GET" => String::new(),
        None => "Content-Length: 0\r\n".into(),
    };
    let extra: String = headers
        .iter()
        .map(|(k, v)| format!("{k}: {v}\r\n"))
        .collect();
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nAccept-Encoding: identity\r\n{extra}{length}\r\n"
    );
    s.write_all(req.as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    if let Some((_, b)) = body {
        s.write_all(b).await.map_err(|e| e.to_string())?;
    }
    let mut reader = BufReader::new(s);
    let mut lines = Vec::new();
    let mut read = 0;
    loop {
        let mut line = Vec::new();
        let n = reader
            .read_until(b'\n', &mut line)
            .await
            .map_err(|e| e.to_string())?;
        read += n;
        if n == 0 {
            return Err("no header end".into());
        }
        if read > MOST_HEAD {
            return Err("the answer's headers are too long".into());
        }
        let line = String::from_utf8_lossy(&line).trim_end().to_string();
        if line.is_empty() {
            break;
        }
        lines.push(line);
    }
    let mut lines = lines.into_iter();
    let status = lines
        .next()
        .and_then(|l| l.split_whitespace().nth(1)?.parse().ok())
        .ok_or("no status")?;
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| {
            let (name, value) = l.split_once(':')?;
            Some((name.trim().to_ascii_lowercase(), value.trim().to_string()))
        })
        .collect();
    let named = |name: &str| headers.iter().find(|(n, _)| n == name).map(|(_, v)| v);
    let framing = if method == "HEAD" || status == 204 || status == 304 {
        Framing::Length(0)
    } else if named("transfer-encoding").is_some_and(|v| v.to_ascii_lowercase().contains("chunked"))
    {
        Framing::Chunked(0)
    } else if let Some(n) = named("content-length").and_then(|v| v.parse().ok()) {
        Framing::Length(n)
    } else {
        Framing::Close
    };
    Ok(Streamed {
        status,
        headers,
        body: Body {
            reader,
            framing,
            within: Duration::MAX,
        },
    })
}

/// The most an answer's status line and headers may take.
const MOST_HEAD: usize = 64 << 10;
/// The most one piece of a body read takes.
const PIECE: usize = 256 << 10;

struct Body {
    reader: BufReader<tokio::net::TcpStream>,
    framing: Framing,
    within: Duration,
}

/// How the body's end is known.
#[derive(Clone, Copy)]
enum Framing {
    /// This many bytes are still to come.
    Length(u64),
    /// Chunks: this many bytes of the current one are still to come; `0`
    /// is before a chunk's size line.
    Chunked(u64),
    /// Until the node closes the connection.
    Close,
    Done,
}

impl Body {
    async fn piece(&mut self) -> Result<Option<Vec<u8>>, String> {
        loop {
            let most = match self.framing {
                Framing::Done | Framing::Length(0) => return Ok(None),
                Framing::Length(left) => left.min(PIECE as u64) as usize,
                Framing::Close => PIECE,
                Framing::Chunked(0) => {
                    let size = self.line().await?;
                    let size = u64::from_str_radix(size.split(';').next().unwrap_or("").trim(), 16)
                        .map_err(|_| "bad chunk size")?;
                    if size == 0 {
                        // Trailers, up to the empty line.
                        while !self.line().await?.is_empty() {}
                        self.framing = Framing::Done;
                        return Ok(None);
                    }
                    self.framing = Framing::Chunked(size);
                    continue;
                }
                Framing::Chunked(left) => left.min(PIECE as u64) as usize,
            };
            let mut piece = vec![0; most];
            let n = self
                .reader
                .read(&mut piece)
                .await
                .map_err(|e| e.to_string())?;
            piece.truncate(n);
            self.framing = match self.framing {
                Framing::Close if n == 0 => return Ok(None),
                _ if n == 0 => return Err("the answer ended early".into()),
                Framing::Length(left) => Framing::Length(left - n as u64),
                Framing::Chunked(left) if left == n as u64 => {
                    if !self.line().await?.is_empty() {
                        return Err("bad chunk end".into());
                    }
                    Framing::Chunked(0)
                }
                Framing::Chunked(left) => Framing::Chunked(left - n as u64),
                other => other,
            };
            return Ok(Some(piece));
        }
    }

    async fn line(&mut self) -> Result<String, String> {
        let mut line = Vec::new();
        let n = (&mut self.reader)
            .take(MOST_HEAD as u64)
            .read_until(b'\n', &mut line)
            .await
            .map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("the answer ended early".into());
        }
        Ok(String::from_utf8_lossy(&line).trim_end().to_string())
    }

    async fn all(&mut self) -> Result<Vec<u8>, String> {
        let mut all = Vec::new();
        while let Some(piece) = self.piece().await? {
            all.extend_from_slice(&piece);
        }
        Ok(all)
    }
}

/// How a request that timed out begins its reason, so a caller can tell a
/// silence (the node may still be acting) from a refusal.
pub const NO_ANSWER: &str = "no answer in";

/// The node's `{error}` words for a failed answer, or its status. The
/// error is the node's shape, `{"code","message"}`, or plain words.
pub fn error_text(status: u16, body: &[u8]) -> String {
    serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|v| match v.get("error")? {
            serde_json::Value::String(words) => Some(words.clone()),
            e => e.get("message")?.as_str().map(str::to_string),
        })
        .unwrap_or_else(|| match status {
            413 => "Too large".into(),
            0 => "Not reachable".into(),
            s => format!("Failed ({s})"),
        })
}

fn chunked(head: &str) -> bool {
    head.lines().any(|l| {
        let l = l.to_ascii_lowercase();
        l.starts_with("transfer-encoding:") && l.contains("chunked")
    })
}

/// A whole HTTP/1.1 answer as its status and body.
pub fn parse(all: &[u8]) -> Result<(u16, Vec<u8>), String> {
    let end = all
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("no header end")?;
    let head = String::from_utf8_lossy(&all[..end]);
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or("no status")?;
    let body = &all[end + 4..];
    Ok((
        status,
        if chunked(&head) {
            dechunk(body)?
        } else {
            body.to_vec()
        },
    ))
}

fn dechunk(mut b: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let line = b.windows(2).position(|w| w == b"\r\n").ok_or("bad chunk")?;
        let size = usize::from_str_radix(
            String::from_utf8_lossy(&b[..line])
                .split(';')
                .next()
                .unwrap_or("")
                .trim(),
            16,
        )
        .map_err(|_| "bad chunk size")?;
        b = &b[line + 2..];
        if size == 0 {
            return Ok(out);
        }
        out.extend_from_slice(b.get(..size).ok_or("short chunk")?);
        b = b.get(size + 2..).ok_or("short chunk")?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_plain_and_a_chunked_body() {
        assert_eq!(
            parse(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nhi").unwrap(),
            (200, b"hi".to_vec())
        );
        let chunked = b"HTTP/1.1 404 Not Found\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n";
        assert_eq!(parse(chunked).unwrap(), (404, b"abcde".to_vec()));
    }

    /// A node that answers each connection with `answer`, then holds the
    /// connection open (`hold`) or closes it.
    async fn node(answer: &'static [u8], hold: bool) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let at = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut s, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut head = [0; 4096];
                    let _ = s.read(&mut head).await;
                    s.write_all(answer).await.unwrap();
                    if hold {
                        tokio::time::sleep(Duration::from_secs(60)).await;
                    }
                });
            }
        });
        format!("http://{at}")
    }

    #[tokio::test]
    async fn a_streamed_answer_comes_before_its_body_ends() {
        // Headers and a first piece, then the node goes quiet with most of
        // the promised body unsent: the status, headers and that piece are
        // in hand already.
        let base = node(
            b"HTTP/1.1 206 Partial Content\r\nContent-Type: video/mp4\r\nContent-Range: bytes 0-9/100\r\nContent-Length: 1000\r\n\r\nfirst",
            true,
        )
        .await;
        let mut answer = open(
            &base,
            "/f",
            &[("Range", "bytes=0-9")],
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        assert_eq!(answer.status, 206);
        assert_eq!(answer.header("content-type"), Some("video/mp4"));
        assert_eq!(answer.header("Content-Range"), Some("bytes 0-9/100"));
        assert_eq!(
            answer.piece().await.unwrap().as_deref(),
            Some(&b"first"[..])
        );
    }

    #[tokio::test]
    async fn a_streamed_body_reads_by_length_by_chunks_or_to_the_close() {
        for (answer, body) in [
            (&b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello"[..], &b"hello"[..]),
            (
                b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2;x=y\r\nde\r\n0\r\n\r\n",
                b"abcde",
            ),
            (b"HTTP/1.1 200 OK\r\n\r\nto the end", b"to the end"),
        ] {
            let base = node(answer, false).await;
            let mut streamed = open(&base, "/", &[], Duration::from_secs(5)).await.unwrap();
            assert_eq!(streamed.up_to(100).await.unwrap(), body);
            let (status, whole) = send(&base, "GET", "/", &[], None, Duration::from_secs(5))
                .await
                .unwrap();
            assert_eq!((status, whole.as_slice()), (200, body));
        }
        let base = node(b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\nshort", false).await;
        let mut short = open(&base, "/", &[], Duration::from_secs(5)).await.unwrap();
        assert!(
            short.up_to(100).await.is_err(),
            "a body that ends early fails"
        );
    }

    #[test]
    fn a_failed_answer_reads_as_the_nodes_message() {
        let shaped = br#"{"error":{"code":"bad_request","message":"name is required"}}"#;
        assert_eq!(error_text(400, shaped), "name is required");
        assert_eq!(
            error_text(400, br#"{"error":"plain words"}"#),
            "plain words"
        );
        assert_eq!(error_text(400, b"not json"), "Failed (400)");
        assert_eq!(error_text(413, b"{}"), "Too large");
    }
}
