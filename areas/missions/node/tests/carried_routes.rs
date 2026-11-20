//! Route regressions carried from the former Missions node.
use ahjo_node_kit::{NodeClient, NodeError};
use ahjo_testkit::{node::Node, sessions};
use axum::http::{Method, StatusCode};
use serde_json::{Value, json};

async fn start() -> (Node, NodeClient) {
    let node = Node::start(
        &[
            sessions::LISTING,
            ahjo_work::LISTING,
            ahjo_missions::LISTING,
        ],
        "",
    )
    .await
    .unwrap();
    let client = NodeClient::new(Some(&format!("{}/", node.url()))).unwrap();
    (node, client)
}
async fn send(
    client: &NodeClient,
    method: Method,
    path: &str,
    body: Value,
    etag: Option<&str>,
) -> (Value, String) {
    client
        .request(method, path, Some(body), etag)
        .await
        .unwrap()
}
fn assert_error(error: anyhow::Error, status: StatusCode, code: &str) {
    let error = error.downcast_ref::<NodeError>().unwrap();
    assert_eq!((error.status, error.code.as_str()), (status, code));
}

#[tokio::test(flavor = "multi_thread")]
async fn http_restores_a_deleted_formation_and_fences_notes_and_layout() {
    let (node, c) = start().await;
    let (_, tag) = send(
        &c,
        Method::POST,
        "/api/missions",
        json!({"slug":"demo","title":"Demo"}),
        None,
    )
    .await;
    let (made, tag) = send(
        &c,
        Method::PATCH,
        "/api/missions/demo",
        json!({"expected_rev":1,"create_formation":{"type":"solo","title":"Work"}}),
        Some(&tag),
    )
    .await;
    let formation = made["formation"].clone();
    let id = formation["id"].as_str().unwrap();
    let (deleted, tag) = send(
        &c,
        Method::PATCH,
        "/api/missions/demo",
        json!({"expected_rev":2,"delete_formation":{"id":id}}),
        Some(&tag),
    )
    .await;
    assert_eq!(deleted["mission"]["formation"], json!([]));
    let (restored, _) = send(
        &c,
        Method::PATCH,
        "/api/missions/demo",
        json!({"expected_rev":3,"restore_node":{"formation":formation,"x":112,"y":112}}),
        Some(&tag),
    )
    .await;
    assert_eq!(restored["mission"]["formation"][0]["id"], id);
    let (_, notes_tag) = c
        .request(Method::GET, "/api/missions/demo/notes", None, None)
        .await
        .unwrap();
    let note = json!({"target":"mission","author":"human:test","text":"Keep this"});
    send(
        &c,
        Method::PATCH,
        "/api/missions/demo/notes",
        note.clone(),
        Some(&notes_tag),
    )
    .await;
    assert_error(
        c.request(
            Method::PATCH,
            "/api/missions/demo/notes",
            Some(note),
            Some(&notes_tag),
        )
        .await
        .unwrap_err(),
        StatusCode::CONFLICT,
        "conflict",
    );
    let (_, layout_tag) = c
        .request(Method::GET, "/api/missions/demo/layout", None, None)
        .await
        .unwrap();
    let position = json!({"nodes":[{"id":id,"x":400,"y":500}]});
    send(
        &c,
        Method::PATCH,
        "/api/missions/demo/layout",
        position.clone(),
        Some(&layout_tag),
    )
    .await;
    assert_error(
        c.request(
            Method::PATCH,
            "/api/missions/demo/layout",
            Some(position),
            Some(&layout_tag),
        )
        .await
        .unwrap_err(),
        StatusCode::CONFLICT,
        "conflict",
    );
    node.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn derives_named_slug_and_rejects_duplicate_without_clobbering() {
    let (node, c) = start().await;
    let body = json!({"title":"  Release Plan!  "});
    let (first, tag) = send(&c, Method::POST, "/api/missions", body.clone(), None).await;
    assert_eq!(first["mission"]["slug"], "release-plan");
    assert_eq!(first["mission"]["title"], "Release Plan!");
    assert_eq!(first["etag"], tag);
    assert_error(
        c.request(Method::POST, "/api/missions", Some(body), None)
            .await
            .unwrap_err(),
        StatusCode::CONFLICT,
        "mission_exists",
    );
    node.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn default_gate_is_human_and_a_code_check_requires_code_kind() {
    let (node, c) = start().await;
    let (_, tag) = send(
        &c,
        Method::POST,
        "/api/missions",
        json!({"slug":"gate","title":"Gate"}),
        None,
    )
    .await;
    let bad=c.request(Method::PATCH,"/api/missions/gate",Some(json!({"expected_rev":1,"create_gate":{"check":"output_contains","check_version":"1","check_value":"OK"}})),Some(&tag)).await.unwrap_err();
    let err = bad.downcast_ref::<NodeError>().unwrap();
    assert_eq!(err.status, StatusCode::UNPROCESSABLE_ENTITY, "{err}");
    let (saved, _) = send(
        &c,
        Method::PATCH,
        "/api/missions/gate",
        json!({"expected_rev":1,"create_gate":{"title":"Signoff"}}),
        Some(&tag),
    )
    .await;
    assert_eq!(saved["gate"]["kinds"], json!(["human"]));
    node.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn edits_are_checked_and_pushed_to_subscribers() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::{connect_async, tungstenite::Message};
    let (node, c) = start().await;
    let (mut socket, _) = connect_async(node.ws()).await.unwrap();
    socket
        .send(Message::Binary(
            vec![ahjo_missions_contract::WATCH, 1].into(),
        ))
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Some(Ok(message)) = socket.next().await {
            if let Message::Binary(frame) = message
                && frame.first() == Some(&ahjo_missions_contract::SNAPSHOT)
            {
                break;
            }
        }
    })
    .await
    .unwrap();
    let (_, tag) = send(
        &c,
        Method::POST,
        "/api/missions",
        json!({"slug":"test","title":"Test"}),
        None,
    )
    .await;
    let (_, same) = send(
        &c,
        Method::PATCH,
        "/api/missions/test",
        json!({"expected_rev":1,"title":"Test"}),
        Some(&tag),
    )
    .await;
    assert_eq!(same, tag);
    let error = c
        .request(
            Method::PATCH,
            "/api/missions/test",
            Some(json!({"expected_rev":1,"title":"X","create_end":{"title":"End"}})),
            Some(&tag),
        )
        .await
        .unwrap_err();
    assert_eq!(
        error.downcast_ref::<NodeError>().unwrap().status,
        StatusCode::BAD_REQUEST
    );
    let update = json!({"expected_rev":1,"title":"Renamed"});
    send(
        &c,
        Method::PATCH,
        "/api/missions/test",
        update.clone(),
        Some(&tag),
    )
    .await;
    let stale = c
        .request(
            Method::PATCH,
            "/api/missions/test",
            Some(update),
            Some(&tag),
        )
        .await
        .unwrap_err();
    assert_eq!(
        stale.downcast_ref::<NodeError>().unwrap().status,
        StatusCode::CONFLICT
    );
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Some(Ok(message)) = socket.next().await {
            if let Message::Binary(frame) = message
                && frame.first() == Some(&ahjo_missions_contract::SNAPSHOT)
            {
                let data: Value = serde_json::from_slice(&frame[1..]).unwrap();
                if data.to_string().contains("Renamed") {
                    return;
                }
            }
        }
        panic!("subscription ended before the edit");
    })
    .await
    .expect("subscriber observes edit");
    drop(socket);
    node.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn http_patch_uses_the_same_schema_and_stages_a_batch() {
    let (node, client) = start().await;
    let (_, tag) = send(
        &client,
        Method::POST,
        "/api/missions",
        json!({"slug":"batch","title":"Original"}),
        None,
    )
    .await;
    let path = "/api/missions/batch";
    let error = client
        .request(
            Method::PATCH,
            path,
            Some(json!({"rev":1,"changes":[{"title":"Changed"},{"create_end":{"bogus":42}}]})),
            Some(&tag),
        )
        .await
        .unwrap_err();
    let error = error.downcast_ref::<NodeError>().unwrap();
    assert_eq!(error.status, StatusCode::BAD_REQUEST);
    assert_eq!(
        error.message,
        "change at index 1: create_end: unknown field bogus; accepted fields: title, outcome, x, y"
    );
    let (after, _) = send(
        &client,
        Method::PATCH,
        path,
        json!({"rev":1,"changes":[{"create_formation":{"title":"Work"}},{"title":"Changed"}]}),
        Some(&tag),
    )
    .await;
    assert_eq!(after["mission"]["rev"], 2);
    assert_eq!(after["mission"]["title"], "Changed");
    assert_eq!(after["mission"]["formation"][0]["title"], "Work");
    assert_eq!(after["layout"]["mission_rev"], 2);
    node.stop().await;
}
