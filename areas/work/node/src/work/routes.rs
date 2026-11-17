//! The Work module's HTTP route: one item with its family, read on demand
//! (`docs/work.md`, The seam).

use std::collections::HashMap;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;

use super::poller::Work;

/// Work's routes, for the server to merge with the other modules'.
pub fn routes<S: Clone + Send + Sync + 'static>(work: Work) -> Router<S> {
    Router::new()
        .route("/api/work/item", get(item))
        .route("/api/work/text", get(text))
        .route("/api/work/search", get(search))
        .route("/api/work/items", post(create))
        .with_state(work)
}

/// `GET /api/work/text?ref=<reference>`: `{"text": …}`, the item's text
/// that the channel's snapshot leaves out. From the snapshot as last read,
/// so it matches the rest of the item the page shows; an item the snapshot
/// does not hold is read as `/api/work/item` reads it.
async fn text(State(work): State<Work>, Query(params): Query<HashMap<String, String>>) -> Response {
    let written = params.get("ref").map(String::as_str).unwrap_or("");
    if let Some(text) = work.last().text(written) {
        return Json(json!({ "text": text })).into_response();
    }
    let reference = match work.resolve(written).await {
        Ok(reference) => reference,
        Err(error) => {
            return refuse(
                StatusCode::BAD_REQUEST,
                ahjo_work_contract::BAD_REQUEST,
                error,
            );
        }
    };
    match work.item(&reference).await {
        Ok(item) => Json(json!({ "text": item.body })).into_response(),
        Err(error) => {
            let (status, code) = if error.downcast_ref::<super::MissingItem>().is_some() {
                (StatusCode::NOT_FOUND, "not_found")
            } else {
                (StatusCode::BAD_GATEWAY, ahjo_work_contract::TRACKER_ERROR)
            };
            refuse(status, code, error)
        }
    }
}

/// `GET /api/work/search?q=<words>`: `{"references": […]}`, the items in
/// the snapshot as last read holding every word in their id, title,
/// labels or text, as the page's own search counts them.
async fn search(
    State(work): State<Work>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let terms = ahjo_work_contract::text::terms(params.get("q").map(String::as_str).unwrap_or(""));
    let snapshot = work.last();
    match tokio::task::spawn_blocking(move || snapshot.matching(&terms)).await {
        Ok(references) => Json(json!({ "references": references })).into_response(),
        Err(error) => refuse(
            StatusCode::INTERNAL_SERVER_ERROR,
            "search_failed",
            error.into(),
        ),
    }
}

/// `GET /api/work/item?ref=<reference>`: `{"item": …, "line": …,
/// "family": […]}`, where `line` names the item to an agent and `family`
/// holds the items its family names as last read, without their text; 400 for a reference that does not
/// resolve, 404 for a missing item, 502 for a tracker that failed to answer.
async fn item(State(work): State<Work>, Query(params): Query<HashMap<String, String>>) -> Response {
    let written = params.get("ref").map(String::as_str).unwrap_or("");
    let reference = match work.resolve(written).await {
        Ok(reference) => reference,
        Err(error) => {
            return refuse(
                StatusCode::BAD_REQUEST,
                ahjo_work_contract::BAD_REQUEST,
                error,
            );
        }
    };
    match work.item(&reference).await {
        Ok(item) => {
            let line = work.reference_line(&item).await;
            let family = work.family(&item);
            Json(json!({ "item": item, "line": line, "family": family })).into_response()
        }
        Err(error) => {
            let (status, code) = if error.downcast_ref::<super::MissingItem>().is_some() {
                (StatusCode::NOT_FOUND, "not_found")
            } else {
                (StatusCode::BAD_GATEWAY, ahjo_work_contract::TRACKER_ERROR)
            };
            refuse(status, code, error)
        }
    }
}

async fn create(
    State(work): State<Work>,
    request: Result<Json<ahjo_work_contract::Create>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(request) = match request {
        Ok(request) => request,
        Err(error) => {
            return refuse(
                StatusCode::BAD_REQUEST,
                "bad_request",
                anyhow::anyhow!(error.to_string()),
            );
        }
    };
    match work.create(&request).await {
        Ok(reference) => Json(json!({"reference": reference})).into_response(),
        Err(error) => {
            let status = match error.code.as_str() {
                "unknown_project" | "bad_request" => StatusCode::BAD_REQUEST,
                _ => StatusCode::BAD_GATEWAY,
            };
            (status, Json(json!({"error": error}))).into_response()
        }
    }
}

fn refuse(status: StatusCode, code: &str, error: anyhow::Error) -> Response {
    (
        status,
        Json(json!({ "error": ahjo_core::Error::new(code, format!("{error:#}")) })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{Body, to_bytes};
    use axum::http::Request;
    use tower::ServiceExt;

    async fn get_json(path: &str) -> (StatusCode, serde_json::Value) {
        let app: Router = routes(Work::default());
        let response = app
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), 8192).await.unwrap();
        (status, serde_json::from_slice(&body).unwrap())
    }

    #[tokio::test]
    async fn text_and_search_answer_from_the_snapshot() {
        let (status, value) = get_json("/api/work/search?q=anything").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(value, json!({"references": []}));
        let (status, value) = get_json("/api/work/text?ref=nowhere-1").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(value["error"]["code"], "bad_request");
    }

    #[tokio::test]
    async fn create_route_returns_the_work_error_shape() {
        let app: Router = routes(Work::default());
        let response = app
            .oneshot(
                Request::post("/api/work/items")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"project":"missing","title":"T","description":"D"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), 8192).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["error"]["code"], "unknown_project");
    }
}
