use ahjo_missions_contract::{Layout, Mission};
use ahjo_node_kit::{Listing, cli};
use ahjo_testkit::{node::Node, sessions};
use serde_json::{Value, json};
use std::path::PathBuf;

const AREAS: &[Listing] = &[
    sessions::LISTING,
    ahjo_work::LISTING,
    ahjo_missions::LISTING,
];

async fn cli_mission(node: &Node, args: &[&str]) -> Vec<u8> {
    let mut args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
    args.extend(["--node".into(), format!("{}/", node.url())]);
    let mut output = Vec::new();
    assert_eq!(
        cli::run_with_files(
            "mission",
            &args,
            &mut output,
            ahjo_missions::DOCUMENT_FILE_ARGUMENTS
        )
        .await
        .unwrap(),
        0,
        "{args:?}",
    );
    output
}

fn mission_path(node: &Node, slug: &str) -> PathBuf {
    node.dir()
        .join("missions/missions")
        .join(format!("{slug}.mission.toml"))
}

async fn stored_layout(node: &Node, slug: &str) -> Layout {
    let response = node
        .call("layout_show", json!({"mission":slug}))
        .await
        .unwrap();
    serde_json::from_value(response["layout"].clone()).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn document_commands_keep_bytes_and_reject_a_missing_or_stale_revision() {
    let node = Node::start(AREAS, "").await.unwrap();
    let example = cli_mission(&node, &["export", "--example", "write-and-review"]).await;
    assert_eq!(
        example,
        ahjo_missions_contract::example("write-and-review")
            .unwrap()
            .definition
            .as_bytes()
    );
    let source = String::from_utf8(example)
        .unwrap()
        .replace("slug = \"write-and-review\"", "slug = \"post\"");
    let file = node.dir().join("post document.toml");
    std::fs::write(&file, &source).unwrap();
    let applied: Value = serde_json::from_slice(
        &cli_mission(&node, &["apply", file.to_str().unwrap(), "--json"]).await,
    )
    .unwrap();
    assert_eq!(applied["mission"], "post");
    assert_eq!(applied["rev"], 1);
    assert_eq!(applied["assigned_ids"], json!([]));
    assert_eq!(applied["validation"]["errors"], json!([]));
    assert!(
        applied["next"]
            .as_str()
            .unwrap()
            .contains("ahjo mission run")
    );
    let path = mission_path(&node, "post");
    assert_eq!(std::fs::read(&path).unwrap(), source.as_bytes());
    let model = Mission::parse(&source).unwrap();
    assert_eq!(model.id, "msn_example");
    assert_eq!(model.formations[0].slots[0].id, "slot_write");
    let layout = stored_layout(&node, "post").await;
    assert!(layout.nodes.iter().any(|n| n.id == "fmn_write"));

    let exported = cli_mission(&node, &["export", "post"]).await;
    assert_eq!(exported, source.as_bytes());
    let json_export: Value =
        serde_json::from_slice(&cli_mission(&node, &["export", "post", "--json"]).await).unwrap();
    assert_eq!(json_export["rev"], 1);
    assert_eq!(json_export["formation"][0]["slot"][0]["id"], "slot_write");
    assert!(json_export.get("inputCard").is_some());
    assert!(json_export.get("document").is_none());
    std::fs::write(&file, &exported).unwrap();
    let unchanged: Value = serde_json::from_slice(
        &cli_mission(&node, &["apply", file.to_str().unwrap(), "--json"]).await,
    )
    .unwrap();
    assert_eq!(unchanged["rev"], 1);
    assert_eq!(std::fs::read(&path).unwrap(), exported);
    assert_eq!(stored_layout(&node, "post").await, layout);

    let new_source = source.replace(
        "title = \"Write and review\"",
        "title = \"A revised mission\"",
    );
    let updated = node
        .call("mission_apply", json!({"document":new_source}))
        .await
        .unwrap();
    assert_eq!(updated["rev"], 2);
    assert_eq!(updated["changed"]["changed"], json!(["msn_example"]));
    let current = std::fs::read(&path).unwrap();
    for unversioned in [source.clone(), source.replace("rev = 1\n", "")] {
        let error = node
            .call("mission_apply", json!({"document":unversioned}))
            .await
            .unwrap_err();
        assert_eq!(error.code, "conflict");
        assert!(error.message.contains("export it again"));
        assert_eq!(std::fs::read(&path).unwrap(), current);
    }
    node.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn apply_reports_assigned_ids_and_draft_errors_and_previews_without_writes() {
    let node = Node::start(AREAS, "").await.unwrap();
    let document = r#"schema = 1
slug = "draft"
title = "Sample draft"
formation = [
    {title = "First draft", type = "solo", input = [{label = "Brief"}]},
    {title = "First draft", type = "solo"},
]
"#;
    let preview = node
        .call("mission_apply", json!({"document":document,"dry_run":true}))
        .await
        .unwrap();
    assert_eq!(preview["rev"], 1);
    assert_eq!(
        preview["assigned_ids"],
        json!([
            "msn_sample_draft",
            "fmn_first_draft",
            "port_brief",
            "fmn_first_draft_2"
        ])
    );
    assert!(
        !preview["validation"]["errors"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(preview["next"], "ahjo mission apply -");
    let path = mission_path(&node, "draft");
    assert!(!path.exists());
    assert!(
        !node
            .dir()
            .join("missions/layout/draft.layout.toml")
            .exists()
    );

    let applied = node
        .call("mission_apply", json!({"document":document}))
        .await
        .unwrap();
    assert_eq!(applied["assigned_ids"], preview["assigned_ids"]);
    assert_eq!(applied["validation"], preview["validation"]);
    let original = std::fs::read_to_string(&path).unwrap();
    let model = Mission::parse(&original).unwrap();
    assert_eq!(model.formations[0].inputs[0].id, "port_brief");
    assert_eq!(model.formations[0].id, "fmn_first_draft");
    assert_eq!(model.rev, 1);
    let placed = stored_layout(&node, "draft").await;
    assert_eq!(placed.nodes.len(), 2);
    assert_ne!(
        (placed.nodes[0].x, placed.nodes[0].y),
        (placed.nodes[1].x, placed.nodes[1].y)
    );
    let add = format!("{original}\n[[end]]\ntitle = \"Unexpected end\"\n");
    let preview = node
        .call("mission_apply", json!({"document":add,"dry_run":true}))
        .await
        .unwrap();
    assert_eq!(preview["rev"], 2);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    assert_eq!(stored_layout(&node, "draft").await, placed);

    node.call("mission_apply", json!({"document":add}))
        .await
        .unwrap();
    let layout = stored_layout(&node, "draft").await;
    assert!(placed.nodes.iter().all(|old| layout.nodes.contains(old)));
    assert!(layout.nodes.iter().any(|n| n.id == "end_unexpected_end"));
    node.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn apply_keeps_a_card_with_an_open_note_and_removes_its_position_after_resolution() {
    let node = Node::start(AREAS, "").await.unwrap();
    let document = r#"schema = 1
id = "mission_notes"
slug = "noted"
title = "Noted"
[[end]]
id = "end_old"
title = "Old card"
"#;
    node.call("mission_apply", json!({"document":document}))
        .await
        .unwrap();
    let notes = node.call("mission_note", json!({
        "mission":"noted","target":"end_old","author":"document-test","text":"Check this ending"
    })).await.unwrap();
    let note = notes["note"].as_str().unwrap();
    let removal =
        "schema = 1\nid = \"mission_notes\"\nslug = \"noted\"\nrev = 1\ntitle = \"Noted\"\n";
    let error = node
        .call("mission_apply", json!({"document":removal}))
        .await
        .unwrap_err();
    assert_eq!(error.code, "conflict");
    assert!(error.message.contains(note) && error.message.contains("end_old"));
    let path = mission_path(&node, "noted");
    let saved = std::fs::read_to_string(&path).unwrap();
    assert_eq!(Mission::parse(&saved).unwrap().rev, 1);
    let layout = stored_layout(&node, "noted").await;
    assert!(layout.nodes.iter().any(|n| n.id == "end_old"));

    node.call(
        "mission_resolve",
        json!({
            "mission":"noted","note":note,"author":"document-test","text":"Answered"
        }),
    )
    .await
    .unwrap();
    let applied = node
        .call("mission_apply", json!({"document":removal}))
        .await
        .unwrap();
    assert_eq!(applied["changed"]["removed"], json!(["end_old"]));
    assert_eq!(applied["rev"], 2);
    assert!(
        !stored_layout(&node, "noted")
            .await
            .nodes
            .iter()
            .any(|n| n.id == "end_old")
    );
    node.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_port_choice_names_ids_even_when_labels_repeat() {
    let node = Node::start(AREAS, "").await.unwrap();
    let document = r#"schema = 1
slug = "ports"
inputCard = [{ id = "start", title = "Start" }]
formation = [
    { id = "merge", title = "Merge", type = "solo", input = [
        { id = "first", label = "Input" },
        { id = "second", label = "Input" },
    ] },
]
"#;
    node.call("mission_apply", json!({"document":document}))
        .await
        .unwrap();
    for to in ["Merge", "Merge:Input"] {
        let error = node
            .call(
                "formation_wire",
                json!({"mission":"ports","from":"Start","to":to}),
            )
            .await
            .unwrap_err();
        assert!(error.message.contains("merge:first"), "{error}");
        assert!(error.message.contains("merge:second"), "{error}");
    }
    node.stop().await;
}
