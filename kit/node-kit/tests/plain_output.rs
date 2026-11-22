//! Plain and JSON CLI output of representative node responses.
use ahjo_node_kit::{Area, Built, Command, Config, Listing, Options, Setup, cli};
use serde_json::{Value, json};

/// Test response.
#[derive(serde::Deserialize, schemars::JsonSchema)]
struct Show {}

struct Responses;

fn answer(name: &str) -> Value {
    match name {
        "host_show" => {
            json!({"host": {"at": 1700000000123_i64, "cpu": {"percent": 12.3}, "gpu": {"name": "GPU", "memory": {"used": 42}}}, "events": [{"at": 1700000000123_i64, "text": "ready"}]})
        }
        "issue_show" => {
            json!({"item": {"id": "work-1", "title": "Fix it", "children": [{"id": "work-2", "status": "open"}]}, "warning": null})
        }
        "scheduled_list" => {
            json!({"scheduled_prompts": [{"id": "task-1", "next_run": "2026-11-12T12:00:00Z", "sessions": ["worker"], "schedule": {"type": "interval", "every_minutes": 15}, "paused": false}]})
        }
        "session_launch" => {
            json!({"node": "n1", "session": "worker", "warning": null, "pane": 3})
        }
        _ => unreachable!(),
    }
}
impl Area for Responses {
    fn commands(&self) -> Vec<Command> {
        NAMES
            .iter()
            .map(|&name| {
                Command::typed(
                    name,
                    ahjo_node_kit::Effect::Reads,
                    move |_: Show| async move { Ok(answer(name)) },
                )
            })
            .collect()
    }
}
fn build(_: &Setup) -> anyhow::Result<Built> {
    Built::now(Responses)
}
const NAMES: &[&str] = &[
    "host_show",
    "issue_show",
    "scheduled_list",
    "session_launch",
];
const AREA: Listing = Listing {
    name: "fixtures",
    kinds: &[],
    commands: NAMES,
    build,
};

#[tokio::test(flavor = "multi_thread")]
async fn plain_cli_formats_nested_responses_and_json_stays_the_same() {
    let dir = std::env::temp_dir().join(format!("ahjo-plain-output-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let node = ahjo_node_kit::start(
        Options {
            config: Config::parse("", dir.clone()).unwrap(),
            dir: dir.clone(),
            listen: Some("127.0.0.1:0".into()),
        },
        &[AREA],
    )
    .await
    .unwrap();
    let url = format!("http://{}/", node.address);
    for (noun, verb, name) in [
        ("host", "show", "host_show"),
        ("issue", "show", "issue_show"),
        ("scheduled", "list", "scheduled_list"),
        ("session", "launch", "session_launch"),
    ] {
        let args = [verb, "--node", &url].map(str::to_owned);
        let mut plain = Vec::new();
        assert_eq!(cli::run(noun, &args, &mut plain).await.unwrap(), 0);
        let plain = String::from_utf8(plain).unwrap();
        if name == "host_show" {
            let local = jiff::Timestamp::from_millisecond(1700000000123)
                .unwrap()
                .to_zoned(jiff::tz::TimeZone::system());
            let expected = format!("{}{}", local.datetime(), local.offset());
            assert!(plain.contains(&format!("at: {expected}")), "{plain}");
            assert!(plain.contains(&format!("at={expected}")), "{plain}");
        }
        // The node answering is the one asked: said only in JSON.
        if name == "session_launch" {
            assert_eq!(plain, "session: worker\npane: 3\n", "in the node's order");
        }
        if name == "scheduled_list" {
            assert!(plain.contains("next_run=2026-11-12T12:00:00Z"), "{plain}");
        }
        assert!(
            !plain.contains('{') && !plain.contains("null"),
            "{name}: {plain}"
        );
        assert!(
            plain.lines().all(|line| line.chars().count() <= 200),
            "{name}: {plain}"
        );
        let args = [verb, "--node", &url, "--json"].map(str::to_owned);
        let mut json = Vec::new();
        assert_eq!(cli::run(noun, &args, &mut json).await.unwrap(), 0);
        assert_eq!(
            serde_json::from_slice::<Value>(&json).unwrap(),
            answer(name)
        );
    }
    node.stop().await.unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}
