//! The issue area exposes tracker commands and only pushes while watched.
use ahjo_node_kit::{Built, Listing, Setup};
use ahjo_testkit::node::Node;
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use std::sync::OnceLock;
use std::time::Duration;
static BD: OnceLock<String> = OnceLock::new();
fn build_work(setup: &Setup) -> anyhow::Result<Built> {
    ahjo_issue::build_with(
        setup,
        ahjo_issue::Bd::new(BD.get().unwrap()),
        ahjo_issue::Gh::default(),
    )
}
const WORK: Listing = Listing {
    build: build_work,
    ..ahjo_issue::LISTING
};
use tokio_tungstenite::tungstenite::Message;

#[test]
fn commands_are_registered_for_cli_and_mcp() {
    assert_eq!(
        ahjo_issue::LISTING.commands,
        &[
            "issue_list",
            "issue_search",
            "issue_show",
            "issue_create",
            "issue_snapshot_text",
            "issue_snapshot_search"
        ]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn work_is_pushed_only_while_watched() {
    let node = Node::start(&[ahjo_issue::LISTING], "").await.unwrap();
    let client = ahjo_node_kit::NodeClient::new(Some(&node.url())).unwrap();
    let (tools, _) = client
        .request(
            axum::http::Method::POST,
            "/mcp/issue",
            Some(json!({"jsonrpc":"2.0","id":1,"method":"tools/list"})),
            None,
        )
        .await
        .unwrap();
    for name in ahjo_issue::LISTING.commands {
        let tool = tools["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == *name)
            .unwrap();
        assert_eq!(tool["inputSchema"]["additionalProperties"], false, "{name}");
        assert!(tool["outputSchema"].is_object(), "{name}");
    }
    let answer = node.call("issue_list", json!({})).await.unwrap();
    assert_eq!(answer["projects"], json!([]));
    // An empty node says how a project is added.
    let none = answer["none"].as_str().unwrap();
    assert!(
        none.contains(r#"[issue] projects = ["beads /abs/folder"]"#),
        "{none}"
    );
    assert!(none.contains("ahjo config show"), "{none}");
    assert!(node.call("issue_show", json!({"ref":"bad"})).await.is_err());
    let (mut ws, _) = tokio_tungstenite::connect_async(node.ws()).await.unwrap();
    ws.next().await.unwrap().unwrap(); // hello
    assert!(
        tokio::time::timeout(Duration::from_millis(50), ws.next())
            .await
            .is_err()
    );
    ws.send(Message::Binary(vec![0x98, 1].into()))
        .await
        .unwrap();
    let frame = tokio::time::timeout(Duration::from_secs(5), ws.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap()
        .into_data();
    assert_eq!(&frame[..], b"\x90{\"projects\":[],\"refreshed\":0}");
    ws.send(Message::Binary(vec![0x98, 0].into()))
        .await
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(50), ws.next())
            .await
            .is_err()
    );
    drop(ws);
    node.stop().await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn cli_lists_shows_and_creates_a_work_item() {
    let dir = ahjo_testkit::fresh_dir("ahjo-work-create");
    std::fs::create_dir(dir.join(".beads")).unwrap();
    std::fs::write(dir.join(".beads/prefix"), "pwc").unwrap();
    let program = dir.join("fake-bd");
    ahjo_testkit::tmux::write_script(&program, include_str!("fixtures/work/fake-bd.sh"));
    BD.set(program.display().to_string()).unwrap();
    let config = format!("[issue]\nprojects = [\"beads {}\"]\n", dir.display());
    let node = Node::start(&[WORK], &config).await.unwrap();
    let missing: Vec<String> = [
        "create",
        "--project",
        "missing",
        "--title",
        "T",
        "--description",
        "D",
        "--node",
        &node.url(),
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    assert_eq!(
        ahjo_node_kit::cli::run("issue", &missing, &mut Vec::new())
            .await
            .unwrap(),
        1
    );
    let unknown = node
        .call(
            "issue_create",
            json!({"project":"missing", "title":"T", "description":"D"}),
        )
        .await
        .unwrap_err();
    assert_eq!(unknown.code, "unknown_project");
    // It names the projects there are, as work list shows them.
    assert!(
        unknown.message.contains(&dir.display().to_string()),
        "{}",
        unknown.message
    );
    let created = node.call("issue_create", json!({"project": dir.display().to_string(), "title":"Via HTTP", "description":"Body", "type":"feature"})).await.unwrap();
    let http_id = created["reference"]
        .as_str()
        .unwrap()
        .rsplit('#')
        .next()
        .unwrap();
    let http = dir.join(".beads").join(http_id);
    assert_eq!(
        std::fs::read_to_string(http.join("title")).unwrap(),
        "Via HTTP"
    );
    assert_eq!(
        std::fs::read_to_string(http.join("description")).unwrap(),
        "Body"
    );
    assert_eq!(
        std::fs::read_to_string(http.join("type")).unwrap(),
        "feature"
    );
    let args: Vec<String> = vec![
        "create".into(),
        "--node".into(),
        node.url(),
        "--project".into(),
        dir.display().to_string(),
        "--title".into(),
        "T".into(),
        "--description".into(),
        "D".into(),
    ];
    let mut out = Vec::new();
    assert_eq!(
        ahjo_node_kit::cli::run("issue", &args, &mut out)
            .await
            .unwrap(),
        0
    );
    let printed = String::from_utf8(out).unwrap();
    let id = printed.trim().rsplit('#').next().unwrap();
    assert!(id.starts_with("pwc-"), "{printed}");
    let filed = dir.join(".beads").join(id);
    assert_eq!(std::fs::read_to_string(filed.join("title")).unwrap(), "T");
    assert_eq!(
        std::fs::read_to_string(filed.join("description")).unwrap(),
        "D"
    );
    assert_eq!(std::fs::read_to_string(filed.join("type")).unwrap(), "bug");
    let list = vec!["list".into(), "--node".into(), node.url()];
    let mut out = Vec::new();
    assert_eq!(
        ahjo_node_kit::cli::run("issue", &list, &mut out)
            .await
            .unwrap(),
        0
    );
    assert!(String::from_utf8(out).unwrap().contains(id));
    let show = vec![
        "show".into(),
        "--node".into(),
        node.url(),
        "--ref".into(),
        id.into(),
    ];
    let mut out = Vec::new();
    assert_eq!(
        ahjo_node_kit::cli::run("issue", &show, &mut out)
            .await
            .unwrap(),
        0
    );
    let shown = String::from_utf8(out).unwrap();
    assert!(shown.contains("T") && shown.contains(id));

    node.stop().await;
    std::fs::remove_dir_all(dir).unwrap();
}
