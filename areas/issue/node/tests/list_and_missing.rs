//! Issues commands filter their own response without changing the page's board.
#![cfg(unix)]
use ahjo_node_kit::{Built, Listing, Setup};
use ahjo_testkit::node::Node;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::sync::OnceLock;
use tokio_tungstenite::tungstenite::Message;

static BD: OnceLock<String> = OnceLock::new();
fn build(setup: &Setup) -> anyhow::Result<Built> {
    ahjo_issue::build_with(
        setup,
        ahjo_issue::Bd::new(BD.get().unwrap()),
        ahjo_issue::Gh::default(),
    )
}
const WORK: Listing = Listing {
    build,
    ..ahjo_issue::LISTING
};

#[tokio::test(flavor = "multi_thread")]
async fn list_filters_closed_without_changing_the_page_and_missing_is_not_found() {
    let dir = ahjo_testkit::fresh_dir("ahjo-work-filter");
    std::fs::create_dir(dir.join(".beads")).unwrap();
    std::fs::write(
        dir.join(".beads/list.json"),
        include_str!("fixtures/work/beads.json"),
    )
    .unwrap();
    let program = dir.join("fake-bd");
    ahjo_testkit::tmux::write_script(
        &program,
        "#!/bin/sh\ncase \"$2\" in config) printf '{\"value\":\"tst\"}';; list) cat .beads/list.json;; *) exit 1;; esac\n",
    );
    BD.set(program.display().to_string()).unwrap();
    let node = Node::start(
        &[WORK],
        &format!("[issue]\nprojects = [\"beads {}\"]\n", dir.display()),
    )
    .await
    .unwrap();

    for (command, args) in [
        ("issue_list", json!({"unknown": true})),
        ("issue_search", json!({"query": 7})),
        ("issue_show", json!({"ref": true})),
        (
            "issue_create",
            json!({"project":"p","title":"t","description":"d","type":"invalid"}),
        ),
    ] {
        let error = node.call(command, args).await.unwrap_err();
        assert_eq!(error.code, "invalid_argument", "{command}: {error}");
    }
    let list = node.call("issue_list", json!({})).await.unwrap();
    let ids = |v: &Value| {
        v.as_array()
            .unwrap()
            .iter()
            .map(|v| v["id"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    // Each item once, under its project, without its text or whole family.
    assert!(list.get("items").is_none(), "{list}");
    let items = &list["projects"][0]["items"];
    assert_eq!(ids(items).len(), 3);
    assert!(!ids(items).contains(&"tst-1mt".to_owned()));
    for item in items.as_array().unwrap() {
        assert!(
            item.get("body").is_none() && item.get("family").is_none(),
            "{item}"
        );
    }
    let blocked = items
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == "tst-qq9.2")
        .unwrap();
    assert_eq!(blocked["parent"], "tst-qq9", "{blocked}");
    assert!(
        blocked["blocked_by"]
            .as_array()
            .unwrap()
            .contains(&json!("tst-qq9.1")),
        "{blocked}"
    );
    assert!(
        items
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["status"] == "in_progress")
    );
    let mut out = Vec::new();
    let args = ["list", "--all", "--json", "--node", &node.url()].map(str::to_owned);
    assert_eq!(
        ahjo_node_kit::cli::run("issue", &args, &mut out)
            .await
            .unwrap(),
        0
    );
    let all: Value = serde_json::from_slice(&out).unwrap();
    assert!(ids(&all["projects"][0]["items"]).contains(&"tst-1mt".to_owned()));

    // A search reads every word, in titles and text, closed items too.
    let found = |query: &'static str| {
        let node = &node;
        async move {
            let found = node
                .call("issue_search", json!({"query": query}))
                .await
                .unwrap();
            let mut found = ids(&found["items"]);
            found.sort();
            found
        }
    };
    assert_eq!(found("store folder").await, ["tst-qq9", "tst-qq9.1"]);
    assert_eq!(found("old idea").await, ["tst-1mt"]);
    assert!(found("no such words").await.is_empty());

    let (mut ws, _) = tokio_tungstenite::connect_async(node.ws()).await.unwrap();
    ws.next().await.unwrap().unwrap();
    ws.send(Message::Binary(vec![0x98, 1].into()))
        .await
        .unwrap();
    let frame = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let frame = ws.next().await.unwrap().unwrap().into_data();
            if frame.first() == Some(&0x90) {
                let board: Value = serde_json::from_slice(&frame[1..]).unwrap();
                if board["projects"][0]["items"].is_array() {
                    break board;
                }
            }
        }
    })
    .await
    .unwrap();
    assert!(ids(&frame["projects"][0]["items"]).contains(&"tst-1mt".to_owned()));
    // Snapshot text and matching use the same raw snapshot as the lean push.
    let reference = frame["projects"][0]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == "tst-qq9.1")
        .unwrap()["reference"]
        .as_str()
        .unwrap();
    let text = node
        .call("issue_read_snapshot", json!({"reference": reference}))
        .await
        .unwrap();
    assert_eq!(text["text"], "Run bd list in the store folder.");
    let matches = node
        .call("issue_search_snapshot", json!({"query":"store folder"}))
        .await
        .unwrap();
    assert!(
        matches["references"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r == reference)
    );
    assert!(
        frame["projects"][0]["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["body"] == "")
    );
    let error = node
        .call("issue_show", json!({"ref":"tst-missing"}))
        .await
        .unwrap_err();
    assert_eq!(error.code, "not_found");
    assert!(error.message.contains("tst-missing"));
    let missing_text = node
        .call("issue_read_snapshot", json!({"reference":"tst-missing"}))
        .await
        .unwrap_err();
    assert_eq!(missing_text.code, "not_found");
    drop(ws);
    node.stop().await;
    std::fs::remove_dir_all(dir).unwrap();
}
