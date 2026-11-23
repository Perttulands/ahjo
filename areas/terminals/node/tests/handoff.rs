//! A handoff end to end, as ChatGPT drives it over MCP: find the project,
//! start an agent on it with a brief, follow it with the long poll, answer
//! its menu and send it more. The agent is a stand-in script; a real
//! Claude Code or Codex needs a sign-in. Linux and macOS only; skips
//! without tmux unless `AHJO_REQUIRE_TMUX=1`.
#![cfg(unix)]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use ahjo_node_kit::{Built, Listing, Setup};
use ahjo_testkit::node::Node;
use ahjo_testkit::tmux::{Server, write_script};
use serde_json::{Value, json};

/// A stand-in coding agent. At its prompt it has bracketed paste on, as
/// Claude Code and Codex have; a message is worked on for a second of
/// output, and one that says "menu" is first asked about with a
/// numbered menu, answered by a key, and drawn over once answered as
/// theirs are. With `--ask-first` it asks before its
/// first prompt, as a harness asks to trust a folder.
const STAND_IN: &str = r#"#!/bin/bash
menu() {
  printf 'Do you want to proceed?\n\xe2\x9d\xaf 1. Yes\n  2. No\n\nEsc to cancel\n'
  read -rsn1 key
  printf '\033[5A\033[J'
  echo "picked $key"
}
prompt() { printf '> \033[?2004h'; }
[ "$1" = --ask-first ] && menu
prompt
while IFS= read -r line; do
  printf '\033[?2004l'
  line=${line//$'\e[200~'/}
  line=${line//$'\e[201~'/}
  echo "got: $line"
  [[ $line == *menu* ]] && menu
  for i in 1 2 3 4 5 6; do echo "working $i"; sleep 0.2; done
  echo "done: $line"
  prompt
done
"#;

/// The area with its waits shortened to what the stand-in needs: it
/// writes at least every 0.2 s while it works, so a second's quiet is idle.
const TERMINALS: Listing = Listing {
    build,
    ..ahjo_terminals::LISTING
};

fn build(setup: &Setup) -> anyhow::Result<Built> {
    let timings = ahjo_terminals::Timings {
        engine: ahjo_terminals::EngineTimings {
            active_for: Duration::from_secs(1),
        },
        handoff: ahjo_terminals::HandoffTimings {
            asks_for: Duration::from_millis(500),
            look_every: Duration::from_millis(100),
        },
    };
    ahjo_terminals::build_timed(setup, timings)
}

struct Handoff {
    node: Node,
    projects: PathBuf,
    _tmux: Server,
}

impl Handoff {
    async fn start(name: &str) -> Option<Handoff> {
        let tmux = Server::start_in(name)?;
        let dir = tmux.dir().canonicalize().unwrap();
        let projects = dir.join("projects");
        for project in ["games/korpi", "korpikuusi", "games/kauri"] {
            std::fs::create_dir_all(projects.join(project).join(".git")).unwrap();
        }
        let stand_in = dir.join("stand-in");
        write_script(&stand_in, STAND_IN);
        let launch = dir.join("launch.conf");
        std::fs::write(
            &launch,
            format!(
                "harness stand-in {0}\nharness asker {0} --ask-first\nroot {1}\nsize 100x30\n",
                stand_in.display(),
                projects.display()
            ),
        )
        .unwrap();
        let config = format!(
            "[terminals]\nsocket = {:?}\nlaunch = {:?}\n",
            tmux.socket_path(),
            launch
        );
        let node = Node::start(&[TERMINALS], &config).await.unwrap();
        Some(Handoff {
            node,
            projects,
            _tmux: tmux,
        })
    }

    async fn call(&self, tool: &str, arguments: Value) -> Result<Value, String> {
        self.node
            .call(tool, arguments)
            .await
            .map_err(|error| error.message)
    }

    fn project(&self, path: &str) -> String {
        self.projects.join(path).display().to_string()
    }
}

fn screen(answer: &Value) -> &str {
    answer["screen"].as_str().unwrap_or_default()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_brief_is_handed_to_an_agent_followed_and_answered() {
    let Some(h) = Handoff::start("handoff").await else {
        return;
    };

    // A fragment finds the project; its own name comes first.
    let found = h
        .call("project_search", json!({ "query": "korpi" }))
        .await
        .unwrap();
    let paths: Vec<&str> = found["projects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["path"].as_str().unwrap())
        .collect();
    assert_eq!(
        paths,
        [h.project("games/korpi"), h.project("korpikuusi")],
        "{found}"
    );
    assert_eq!(found["projects"][0]["git"], true);
    let none = h
        .call("project_search", json!({ "query": "nowhere" }))
        .await
        .unwrap();
    assert!(none["none"].as_str().unwrap().contains("absolute path"));
    let fragment = h
        .call("project_search", json!({ "query": "kor" }))
        .await
        .unwrap();
    assert_eq!(
        fragment["projects"][0]["path"],
        h.project("games/korpi"),
        "{fragment}"
    );

    // A fragment two projects answer as well is refused, naming both.
    let tied = h
        .call(
            "handoff_launch",
            json!({ "project": "kor", "brief": "Fix the build." }),
        )
        .await
        .unwrap_err();
    assert!(
        tied.contains("games/korpi") && tied.contains("korpikuusi"),
        "{tied}"
    );

    // The agent starts in the project, takes the brief and works on it.
    let started = h
        .call(
            "handoff_launch",
            json!({ "project": "korpi", "brief": "Fix the build.", "issue": "korpi-7" }),
        )
        .await
        .unwrap();
    assert_eq!(started["name"], "korpi", "{started}");
    assert_eq!(started["folder"], h.project("games/korpi"));
    assert_eq!(started["harness"], "stand-in");
    assert_eq!(started["brief_sent"], true, "{started}");
    assert_eq!(started["state"], "working", "{started}");
    assert!(
        started["say"]
            .as_str()
            .unwrap()
            .starts_with("Opened korpi in Swarm"),
        "{started}"
    );
    assert!(
        screen(&started).contains("got: Fix the build."),
        "{started}"
    );

    // The long poll answers once the agent stops working, not at its end.
    let begun = Instant::now();
    let done = h
        .call("handoff_show", json!({ "session": "korpi", "wait": 60 }))
        .await
        .unwrap();
    assert_eq!(done["state"], "idle", "{done}");
    assert!(
        screen(&done).contains("done: Your issue: korpi-7"),
        "the issue ends the brief: {done}"
    );
    assert!(begun.elapsed() < Duration::from_secs(30), "{done}");
    let quick = Instant::now();
    let again = h
        .call("handoff_show", json!({ "session": "korpi", "wait": 60 }))
        .await
        .unwrap();
    assert_eq!(again["state"], "idle");
    assert!(
        quick.elapsed() < Duration::from_secs(5),
        "an agent not working is not waited on"
    );

    // A follow-up that brings a menu: the poll stops at the question.
    let replied = h
        .call(
            "handoff_reply",
            json!({ "session": "korpi", "text": "show me the menu" }),
        )
        .await
        .unwrap();
    assert_eq!(replied["sent"], "text");
    let asked = h
        .call("handoff_show", json!({ "session": "korpi", "wait": 30 }))
        .await
        .unwrap();
    assert_eq!(asked["state"], "needs-input", "{asked}");
    assert_eq!(asked["question"], "Do you want to proceed?", "{asked}");

    // A menu takes an option's number, not words.
    let words = h
        .call(
            "handoff_reply",
            json!({ "session": "korpi", "text": "yes, go on" }),
        )
        .await
        .unwrap_err();
    assert!(words.contains("number of an option"), "{words}");
    let picked = h
        .call("handoff_reply", json!({ "session": "korpi", "text": "1" }))
        .await
        .unwrap();
    assert_eq!(picked["sent"], "key");
    let after = h
        .call("handoff_show", json!({ "session": "korpi", "wait": 30 }))
        .await
        .unwrap();
    assert_eq!(after["state"], "idle", "{after}");
    assert!(screen(&after).contains("picked 1"), "{after}");

    // A second handoff to the project gets a session of its own.
    let second = h
        .call(
            "handoff_launch",
            json!({ "project": h.project("games/korpi"), "brief": "Write the notes." }),
        )
        .await
        .unwrap();
    assert_eq!(second["name"], "korpi-2", "{second}");

    // A session that ended is gone, not an error.
    h.call("session_delete", json!({ "session": "korpi-2" }))
        .await
        .unwrap();
    let gone = h
        .call("handoff_show", json!({ "session": "korpi-2" }))
        .await
        .unwrap();
    assert_eq!(gone["state"], "gone");
    h.node.stop().await;
}

/// An agent that asks something before its first prompt, as a harness asks
/// to trust a folder, is not sent the brief: the answer is the operator's.
#[tokio::test(flavor = "multi_thread")]
async fn a_brief_waits_for_no_question_and_says_so() {
    let Some(h) = Handoff::start("handoff-ask").await else {
        return;
    };
    let begun = Instant::now();
    let started = h
        .call(
            "handoff_launch",
            json!({ "project": "kauri", "brief": "Tidy the island.", "harness": "asker", "name": "kauri-ask" }),
        )
        .await
        .unwrap();
    assert_eq!(started["brief_sent"], false, "{started}");
    assert_eq!(started["state"], "needs-input", "{started}");
    assert_eq!(started["question"], "Do you want to proceed?");
    assert!(
        started["say"].as_str().unwrap().contains("not sent"),
        "{started}"
    );
    assert!(
        begun.elapsed() < Duration::from_secs(30),
        "a question ends the wait"
    );
    assert!(!screen(&started).contains("got:"), "{started}");

    // Answered, the agent comes to its prompt and the brief goes as a reply.
    h.call(
        "handoff_reply",
        json!({ "session": "kauri-ask", "text": "1" }),
    )
    .await
    .unwrap();
    let ready = h
        .call("handoff_show", json!({ "session": "kauri-ask" }))
        .await
        .unwrap();
    assert_ne!(ready["state"], "needs-input", "{ready}");
    h.call(
        "handoff_reply",
        json!({ "session": "kauri-ask", "text": "Tidy the island." }),
    )
    .await
    .unwrap();
    let done = h
        .call(
            "handoff_show",
            json!({ "session": "kauri-ask", "wait": 30 }),
        )
        .await
        .unwrap();
    assert!(screen(&done).contains("done: Tidy the island."), "{done}");
    h.node.stop().await;
}
