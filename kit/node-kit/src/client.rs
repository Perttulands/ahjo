//! The CLI's HTTP connection to a node. An id is routed through the local node.
use anyhow::{Context, Result, bail};
use axum::{
    body::Body,
    http::{Method, Request, header},
};
use http_body_util::BodyExt;
use hyper_util::rt::TokioIo;
use serde_json::Value;
use tokio::net::TcpStream;

/// An error returned by a node, retaining its code for CLI callers.
#[derive(Debug)]
pub struct NodeError {
    pub status: axum::http::StatusCode,
    pub code: String,
    pub message: String,
    pub findings: Option<Value>,
}

/// How long a connection may take. A closed port refuses at once on most
/// hosts, but WSL with mirrored networking lets the attempt hang for
/// minutes, so a node that is not running must be given up on here.
const CONNECT: std::time::Duration = std::time::Duration::from_secs(5);
/// On this machine itself a listener answers at once.
const CONNECT_LOCAL: std::time::Duration = std::time::Duration::from_secs(2);

/// Nothing accepted the connection: no node runs there, or it cannot be
/// reached.
#[derive(Debug)]
pub struct Unreachable {
    /// `host:port` as it was asked.
    pub authority: String,
    pub why: String,
}

impl std::fmt::Display for Unreachable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "no node answers on {} ({})", self.authority, self.why)
    }
}
impl std::error::Error for Unreachable {}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_port_nobody_listens_on_is_unreachable_and_says_where() {
        // Bound and dropped, so the port is known and nothing listens.
        let address = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap()
            .local_addr()
            .unwrap();
        let error = NodeClient::new(Some(&address.to_string()))
            .unwrap()
            .request(Method::GET, "/api/health", None, None)
            .await
            .unwrap_err();
        let unreachable = error.downcast_ref::<Unreachable>().unwrap();
        assert_eq!(unreachable.authority, address.to_string());
        assert!(
            error
                .to_string()
                .starts_with("no node answers on 127.0.0.1:")
        );
    }

    #[test]
    fn a_node_is_a_url_an_address_or_an_id() {
        for written in ["http://127.0.0.1:7320", "127.0.0.1:7320", "127.0.0.1:7320/"] {
            let client = NodeClient::new(Some(written)).unwrap();
            assert_eq!(client.authority, "127.0.0.1:7320", "{written}");
            assert_eq!(client.node, None);
        }
        let routed = NodeClient::new(Some("landmass-win")).unwrap();
        assert_eq!(routed.node.as_deref(), Some("landmass-win"));
        assert!(!is_address("landmass-win"));
        assert!(NodeClient::new(Some("https://h:1")).is_err());
    }
    #[tokio::test]
    async fn plain_text_peer_failures_keep_status_and_message() {
        let app = axum::Router::new().route(
            "/",
            axum::routing::get(|| async {
                (axum::http::StatusCode::BAD_GATEWAY, "peer is offline")
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let error = NodeClient::new(Some(&format!("http://{address}")))
            .unwrap()
            .request(Method::GET, "/", None, None)
            .await
            .unwrap_err();
        let error = error.downcast_ref::<NodeError>().unwrap();
        assert_eq!(error.status, axum::http::StatusCode::BAD_GATEWAY);
        assert_eq!(error.message, "peer is offline");
        task.abort();
    }
    #[tokio::test]
    async fn a_rejection_keeps_the_findings_beside_its_error() {
        let app = axum::Router::new().route(
            "/",
            axum::routing::get(|| async {
                (
                    axum::http::StatusCode::UNPROCESSABLE_ENTITY,
                    axum::Json(serde_json::json!({
                        "error": {"code": "not_admitted", "message": "two findings"},
                        "findings": [{"code": "no_seat"}, {"code": "no_role"}],
                    })),
                )
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let error = NodeClient::new(Some(&format!("http://{address}")))
            .unwrap()
            .request(Method::GET, "/", None, None)
            .await
            .unwrap_err();
        let error = error.downcast_ref::<NodeError>().unwrap();
        assert_eq!(error.code, "not_admitted");
        assert_eq!(error.message, "two findings");
        assert_eq!(
            error.findings,
            Some(serde_json::json!([{"code": "no_seat"}, {"code": "no_role"}]))
        );
        task.abort();
    }
}
impl std::fmt::Display for NodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "node HTTP {}: {}: {}",
            self.status, self.code, self.message
        )
    }
}
impl std::error::Error for NodeError {}

#[derive(Clone, Debug)]
pub struct NodeClient {
    authority: String,
    node: Option<String>,
}
/// Whether `--node` names a node by its address (a URL, or `host:port`)
/// rather than by id: ids have no colon or dot.
pub fn is_address(node: &str) -> bool {
    node.contains([':', '.'])
}

/// A node id as `--node` takes it.
fn node_id(id: &str) -> Result<String> {
    if id.is_empty()
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        bail!("invalid node id");
    }
    Ok(id.into())
}

impl NodeClient {
    pub fn websocket_url(&self) -> String {
        match &self.node {
            Some(node) => format!("ws://{}/ws?node={node}", self.authority),
            None => format!("ws://{}/ws", self.authority),
        }
    }
    pub fn new(node: Option<&str>) -> Result<Self> {
        let default = crate::address::local();
        match node {
            None => Ok(Self {
                authority: default,
                node: None,
            }),
            Some(v) if v.starts_with("http://") => {
                let uri: axum::http::Uri = v.parse().context("invalid node URL")?;
                let node = match uri.query() {
                    None => None,
                    Some(query) => Some(node_id(
                        query
                            .strip_prefix("node=")
                            .context("a node URL's only query is node=<id>")?,
                    )?),
                };
                if uri.path() != "/" {
                    bail!("node URL must have no path");
                }
                Ok(Self {
                    authority: uri
                        .authority()
                        .context("node URL needs a host")?
                        .as_str()
                        .to_owned(),
                    node,
                })
            }
            Some(v) if v.contains("://") => bail!("node URL must use http"),
            Some(v) if is_address(v) => Self::new(Some(&format!("http://{v}"))),
            Some(v) => Ok(Self {
                authority: default,
                node: Some(node_id(v)?),
            }),
        }
    }
    pub async fn request(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
        etag: Option<&str>,
    ) -> Result<(Value, String)> {
        let path = if let Some(node) = &self.node {
            format!(
                "{path}{}node={node}",
                if path.contains('?') { '&' } else { '?' }
            )
        } else {
            path.to_owned()
        };
        let mut builder = Request::builder()
            .method(method)
            .uri(path)
            .header(header::HOST, &self.authority);
        if let Some(etag) = etag {
            builder = builder.header(header::IF_MATCH, etag);
        }
        let bytes = match body {
            Some(body) => {
                builder = builder.header(header::CONTENT_TYPE, "application/json");
                serde_json::to_vec(&body)?
            }
            None => Vec::new(),
        };
        let request = builder.body(Body::from(bytes))?;
        let unreachable = |why: String| Unreachable {
            authority: self.authority.clone(),
            why,
        };
        let local = ["127.", "localhost:", "[::1]:"]
            .iter()
            .any(|own| self.authority.starts_with(own));
        let within = if local { CONNECT_LOCAL } else { CONNECT };
        let stream = match tokio::time::timeout(within, TcpStream::connect(&self.authority)).await {
            Ok(Ok(stream)) => stream,
            Ok(Err(error)) => return Err(unreachable(error.to_string()).into()),
            Err(_) => {
                return Err(unreachable(format!(
                    "no answer to a connection within {} s",
                    within.as_secs()
                ))
                .into());
            }
        };
        let (mut sender, connection) =
            hyper::client::conn::http1::handshake(TokioIo::new(stream)).await?;
        tokio::spawn(async move {
            let _ = connection.await;
        });
        let response = sender.send_request(request).await?;
        let status = response.status();
        let tag = response
            .headers()
            .get(header::ETAG)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_owned();
        let bytes = response.into_body().collect().await?.to_bytes();
        if !status.is_success() {
            let value: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
            let plain = String::from_utf8_lossy(&bytes);
            return Err(NodeError {
                status,
                code: value["error"]["code"].as_str().unwrap_or("ERROR").into(),
                message: value["error"]["message"]
                    .as_str()
                    .unwrap_or(if plain.trim().is_empty() {
                        "request failed"
                    } else {
                        plain.trim()
                    })
                    .into(),
                findings: value.get("findings").cloned(),
            }
            .into());
        }
        let value: Value = serde_json::from_slice(&bytes).context("invalid JSON from node")?;
        Ok((value, tag))
    }
}
