//! Command transports on the node's own port. Apps post a typed argument
//! object to `/api/call/{command}`. MCP takes one JSON-RPC
//! message per POST to `/mcp` (every area) or `/mcp/<area>[,<area>...]`,
//! answered with JSON. The server opens no streams of its own, so GET is
//! refused, as the transport allows.

use std::net::SocketAddr;
use std::sync::Arc;

use ahjo_core::Error;
use axum::body::Body;
use axum::extract::{ConnectInfo, Path, Request, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde_json::Value;

use super::rpc::{self, Caller};
use super::{Mcp, Reach, call_log};
use crate::Failure;

/// The path every area's tools answer on; one area's are under it.
pub const PATH: &str = "/mcp";

/// The largest MCP message taken: a 64 KiB text with room for JSON's escapes.
const MAX_MESSAGE: usize = 1024 * 1024;

/// Typed HTTP arguments have the same 2 MiB budget as Axum JSON routes.
const MAX_COMMAND_ARGS: usize = 2 * 1024 * 1024;

/// Set by Tailscale Funnel on requests from the public internet.
const FUNNEL: &str = "tailscale-funnel-request";

pub(super) fn routes(mcp: Arc<Mcp>) -> Router {
    Router::new()
        .route(PATH, post(every).get(no_stream).delete(no_stream))
        .route(
            &format!("{PATH}/{{areas}}"),
            post(some).get(no_stream).delete(no_stream),
        )
        .route("/api/call/{command}", post(call))
        .with_state(mcp)
}

async fn every(State(mcp): State<Arc<Mcp>>, request: Request) -> Response {
    let reach = mcp.everything();
    message(&mcp, &reach, request).await
}

async fn some(
    State(mcp): State<Arc<Mcp>>,
    Path(areas): Path<String>,
    request: Request,
) -> Response {
    match mcp.reach(&areas) {
        Ok(reach) => message(&mcp, &reach, request).await,
        // A public request learns nothing, not even which areas there are.
        Err(_) if request.headers().contains_key(FUNNEL) => refused(),
        Err(why) => (StatusCode::NOT_FOUND, why).into_response(),
    }
}

async fn message(mcp: &Mcp, reach: &Reach, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    // There is no public endpoint (VISION.md): a Funnel request gets
    // nothing, whatever the node's serve settings.
    if parts.headers.contains_key(FUNNEL) {
        return refused();
    }
    let Ok(body) = axum::body::to_bytes(body, MAX_MESSAGE).await else {
        return (StatusCode::PAYLOAD_TOO_LARGE, "the message is too large").into_response();
    };
    let caller = caller(&parts, "mcp");
    let answer = match serde_json::from_slice::<Value>(&body) {
        Ok(message) => rpc::answer(mcp, reach, &caller, &message).await,
        Err(error) => Some(rpc::unreadable(&error)),
    };
    match answer {
        Some(answer) => (
            [(header::CONTENT_TYPE, "application/json")],
            Body::from(answer.to_string()),
        )
            .into_response(),
        None => StatusCode::ACCEPTED.into_response(),
    }
}

async fn call(
    State(mcp): State<Arc<Mcp>>,
    Path(name): Path<String>,
    request: Request,
) -> Result<Json<Value>, Failure> {
    let (parts, body) = request.into_parts();
    if parts.headers.contains_key(FUNNEL) {
        return Err(Failure(
            StatusCode::FORBIDDEN,
            Error::new(
                "forbidden",
                "the node does not accept public command requests",
            ),
        ));
    }
    let caller = caller(&parts, "api");
    let reach = mcp.everything();
    let Some(command) = mcp.command(&reach, &name) else {
        call_log::record(&caller, "<unknown>", mcp.node.as_str(), "unknown");
        return Err(Failure(
            StatusCode::NOT_FOUND,
            Error::new("unknown_command", format!("Unknown command: {name}")),
        ));
    };
    let body = axum::body::to_bytes(body, MAX_COMMAND_ARGS)
        .await
        .map_err(|_| {
            Failure(
                StatusCode::PAYLOAD_TOO_LARGE,
                Error::new("too_large", "the command arguments are too large"),
            )
        })?;
    let args = serde_json::from_slice(&body).map_err(|error| {
        Failure(
            StatusCode::BAD_REQUEST,
            Error::new(
                "invalid_argument",
                format!("the command arguments are not JSON: {error}"),
            ),
        )
    })?;
    mcp.invoke(&caller, command, args)
        .await
        .map(Json)
        .map_err(|error| {
            let status = (command.error_status)(&error).unwrap_or_else(|| status(&error));
            Failure(status, error)
        })
}

/// Statuses for error codes shared by areas. A command can supply the status
/// for its own codes.
fn status(error: &Error) -> StatusCode {
    match error.code.as_str() {
        "bad_request" | "invalid_argument" => StatusCode::BAD_REQUEST,
        "forbidden" | "permission_denied" => StatusCode::FORBIDDEN,
        "not_found" | "unknown_section" | "no_page" => StatusCode::NOT_FOUND,
        "conflict" | "already_exists" => StatusCode::CONFLICT,
        "unknown_node" => StatusCode::MISDIRECTED_REQUEST,
        "too_large" => StatusCode::PAYLOAD_TOO_LARGE,
        "unavailable" | "no_app" => StatusCode::SERVICE_UNAVAILABLE,
        "timeout" => StatusCode::GATEWAY_TIMEOUT,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

fn caller(parts: &Parts, transport: &'static str) -> Caller {
    let peer = parts
        .extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|info| info.0);
    Caller {
        who: who(peer, &parts.headers),
        transport,
    }
}

fn refused() -> Response {
    (StatusCode::FORBIDDEN, "the public MCP door is not open").into_response()
}

async fn no_stream() -> Response {
    (StatusCode::METHOD_NOT_ALLOWED, [(header::ALLOW, "POST")]).into_response()
}

/// The caller as the call log names it: its address and what it says it is.
fn who(peer: Option<SocketAddr>, headers: &HeaderMap) -> String {
    let peer = peer.map_or_else(|| "?".to_owned(), |peer| peer.to_string());
    match headers
        .get(header::USER_AGENT)
        .and_then(|ua| ua.to_str().ok())
    {
        Some(agent) => format!("{peer} {agent}"),
        None => peer,
    }
}
