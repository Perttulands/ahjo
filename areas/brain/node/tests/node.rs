//! The Brain area through a real testkit node and a stand-in Citadel.
use ahjo_node_kit::{NodeClient, NodeError};
use ahjo_testkit::{fresh_dir, node::Node};
use axum::{Json, http::Method, routing::post};
use serde_json::{Value, json};
use std::{path::PathBuf, process::Command};

struct Corpus(PathBuf);
impl Corpus {
    fn new() -> Self {
        let root = fresh_dir("brain-node-corpus");
        std::fs::create_dir(root.join("notes")).unwrap();
        std::fs::write(root.join("notes/first.md"), "# First\n\nSee [[second]].\n").unwrap();
        std::fs::write(root.join("notes/second.md"), "# Second\n").unwrap();
        let corpus = Self(root);
        corpus.git(&["init", "-q"]);
        corpus.git(&["config", "user.name", "Test Author"]);
        corpus.git(&["config", "user.email", "author@example.org"]);
        corpus.git(&["add", "."]);
        corpus.git(&["commit", "-qm", "Initial pages"]);
        corpus
    }
    fn git(&self, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.0)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
impl Drop for Corpus {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn node_serves_brain_routes_commands_and_coded_errors() {
    let corpus = Corpus::new();
    let citadel = axum::Router::new().route(
        "/v1/library/search",
        post(|Json(query): Json<Value>| async move {
            assert_eq!(query["query"], "first");
            Json(json!({"results": [{"path":"notes/first.md", "title":"First"}]}))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, citadel).await.unwrap() });
    let config = format!(
        "[brain]\ncorpus = {:?}\ncitadel = {:?}\nauthor = {:?}\n",
        corpus.0.to_str().unwrap(),
        format!("http://{address}"),
        "Test Author <author@example.org>"
    );
    let node = Node::start(&[ahjo_brain::LISTING], &config).await.unwrap();
    let client = NodeClient::new(Some(&format!("{}/", node.url()))).unwrap();
    async fn call(client: &NodeClient, name: &str, input: Value) -> Value {
        client
            .request(
                Method::POST,
                &format!("/api/call/{name}"),
                Some(input),
                None,
            )
            .await
            .unwrap()
            .0
    }
    let search = call(&client, "brain_search", json!({"query":"first"})).await;
    assert_eq!(search["results"][0]["path"], "notes/first.md");
    let page = call(&client, "brain_read", json!({"path":"notes/first.md"})).await;
    assert_eq!(page["links"][0]["page"]["path"], "notes/second.md");
    assert_eq!(
        call(&client, "brain_list_collections", json!({})).await["collections"][0]["name"],
        "notes"
    );
    assert_eq!(
        call(&client, "brain_list_collection", json!({"name":"notes"})).await["pages"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        call(&client, "brain_list_changes", json!({"limit":1})).await["changes"][0]["message"],
        "Initial pages"
    );
    let save =
        json!({"path":"notes/first.md", "text":"# First\n\nUpdated.\n", "summary":"Update first"});
    let saved = call(&client, "brain_write", save).await;
    assert_eq!(saved["commit"]["message"], "Update first");
    assert!(
        call(&client, "brain_read", json!({"path":"notes/first.md"})).await["body"]
            .as_str()
            .unwrap()
            .contains("Updated.")
    );
    let error = client
        .request(
            Method::POST,
            "/api/call/brain_read",
            Some(json!({"path":"missing.md"})),
            None,
        )
        .await
        .unwrap_err();
    let error = error.downcast_ref::<NodeError>().unwrap();
    assert_eq!(error.code, "not_found");
    assert_eq!(error.message, "No such page");
    let remote = client
        .request(
            Method::POST,
            "/api/call/brain_read?node=another-node",
            Some(json!({"path":"notes/first.md"})),
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(
        remote.downcast_ref::<NodeError>().unwrap().code,
        "unknown_node"
    );
    let rpc = json!({"jsonrpc":"2.0", "id":1, "method":"tools/list", "params":{}});
    let tools = client
        .request(Method::POST, "/mcp", Some(rpc), None)
        .await
        .unwrap()
        .0;
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .filter(|name| !ahjo_node_kit::own::COMMANDS.contains(name))
        .collect();
    assert_eq!(
        names,
        [
            "brain_search",
            "brain_read",
            "brain_write",
            "brain_list_collections",
            "brain_list_changes",
            "brain_list_files",
            "brain_read_texts",
            "brain_list_collection",
            "brain_send"
        ]
    );
    let read = node
        .call("brain_read", json!({"path":"notes/second.md"}))
        .await
        .unwrap();
    assert_eq!(read["title"], "Second");
    let no_sessions = node
        .call(
            "brain_send",
            json!({"path":"notes/second.md", "session":"alpha"}),
        )
        .await
        .unwrap_err();
    assert_eq!(no_sessions.code, "unavailable");
    let searched = node
        .call("brain_search", json!({"query":"first"}))
        .await
        .unwrap();
    assert_eq!(searched["results"][0]["title"], "First");
    let refused = node
        .call("brain_read", json!({"path":"missing.md"}))
        .await
        .unwrap_err();
    assert_eq!(refused.code, "not_found");
    node.stop().await;
    server.abort();
}

#[tokio::test]
async fn disabled_brain_has_no_routes_or_tools() {
    let node = Node::start(&[ahjo_brain::LISTING], "[brain]\nenabled = false\n")
        .await
        .unwrap();
    let client = NodeClient::new(Some(&format!("{}/", node.url()))).unwrap();
    let health = client
        .request(Method::GET, "/api/health", None, None)
        .await
        .unwrap()
        .0;
    assert_eq!(health["off"], json!(["brain"]));
    let missing = client
        .request(
            Method::POST,
            "/api/call/brain_search",
            Some(json!({"query":"first"})),
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(
        missing.downcast_ref::<NodeError>().unwrap().status,
        axum::http::StatusCode::NOT_FOUND
    );
    let rpc = json!({"jsonrpc":"2.0", "id":1, "method":"tools/list", "params":{}});
    let tools = client
        .request(Method::POST, "/mcp", Some(rpc), None)
        .await
        .unwrap()
        .0;
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ahjo_node_kit::own::COMMANDS, "only the node's own");
    node.stop().await;
}
