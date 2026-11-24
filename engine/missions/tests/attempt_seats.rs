//! A run's attempts record who worked each seat and what it spent, read
//! from the harness's own transcript: a seat left on the harness default
//! learns its model only there.
use ahjo_mission_engine::{Coordinator, LocalSeats, RunSettings, SeatExecutor, TranscriptRoots};
use ahjo_mission_model::{AttemptTokens, RunStart};
use ahjo_testkit::{
    node::Node,
    tmux::{Server, stubbed_path, write_script},
};
use std::{collections::BTreeMap, sync::Arc, time::Duration};

/// A fake Claude: an empty prompt, and once a brief pointer is entered, a
/// transcript of two assistant messages on a model the seat never named,
/// the last one with the run's output and done line.
fn fake_claude(transcripts: &std::path::Path) -> String {
    let file = transcripts.join("project/fake-session.jsonl");
    let usage = |input, write, read, output| {
        format!(
            r#""usage":{{"input_tokens":{input},"cache_creation_input_tokens":{write},"cache_read_input_tokens":{read},"output_tokens":{output}}}"#
        )
    };
    format!(
        r#"#!/bin/sh
printf '❯ '
IFS= read -r line
brief=$(printf '%s' "$line" | sed 's/^Read the file \(.*\) and execute it exactly.*$/\1/')
run=$(grep -o 'run-id=[A-Za-z0-9_-]*' "$brief" | head -n 1 | cut -d= -f2)
now=$(date -u +%Y-%m-%dT%H:%M:%SZ)
mkdir -p "{dir}"
{{
printf '{{"type":"user","origin":{{"kind":"human"}},"promptSource":"typed","timestamp":"%s","cwd":"%s","sessionId":"fake-conversation","message":{{"role":"user","content":"%s"}}}}\n' "$now" "$PWD" "$line"
printf '{{"type":"assistant","timestamp":"%s","cwd":"%s","sessionId":"fake-conversation","message":{{"id":"msg_1","role":"assistant","model":"claude-fable-9","stop_reason":"tool_use","content":[{{"type":"tool_use","id":"tool_1","name":"Bash","input":{{"command":"true"}}}}],{first}}}}}\n' "$now" "$PWD"
printf '{{"type":"assistant","timestamp":"%s","cwd":"%s","sessionId":"fake-conversation","message":{{"id":"msg_2","role":"assistant","model":"claude-fable-9","stop_reason":"end_turn","content":[{{"type":"text","text":"```ahjo-outputs\\n{{\\"port_out\\":{{\\"text\\":\\"done\\"}}}}\\n```\\n<<<AHJO-DONE run-id=%s status=ok artifact=port_out>>>"}}],{second}}}}}\n' "$now" "$PWD" "$run"
}} >> "{file}"
printf '\n❯ '
exec sleep 600
"#,
        dir = file.parent().unwrap().display(),
        file = file.display(),
        first = usage(11, 2200, 33000, 44),
        second = usage(5, 100, 34000, 120),
    )
}

const MISSION: &str = r#"schema = 6
id = "msn_spend"
slug = "spend"
title = "Spend"
rev = 1

[[input_card]]
id = "inp_spend"
title = "Spend"
goal = "Say done"

[[step]]
id = "fmn_work"
formation = "solo"
title = "Work"
[step.brief]
goal = "Say done."
[[step.input]]
id = "port_in"
label = "Input"
[[step.output]]
id = "port_out"
label = "Result"
[[step.seat]]
id = "seat_work"
label = "Worker"
agent = "builder"
harness = "claude-code"
model = "default"
effort = "high"

[[end]]
id = "end_done"
title = "Done"
outcome = "done"

[[connection]]
id = "edge_start"
from = "inp_spend:out"
to = "fmn_work:port_in"

[[connection]]
id = "edge_done"
from = "fmn_work:port_out"
to = "end_done:in"
"#;

#[tokio::test(flavor = "multi_thread")]
async fn an_attempt_records_the_model_its_transcript_names_and_the_four_token_parts() {
    let Some(server) = Server::start() else {
        return;
    };
    let transcripts = server.dir().join("claude-projects");
    let fakes = server.dir().join("fakes");
    std::fs::create_dir(&fakes).unwrap();
    write_script(&fakes.join("claude"), fake_claude(&transcripts));
    // A session takes the PATH of the node that starts it, which is this
    // process: the fake must come first or the real harness would run.
    // SAFETY: this file's only test, before anything reads the environment.
    unsafe { std::env::set_var("PATH", stubbed_path(&fakes)) };

    let config = format!("[terminals]\nsocket = {:?}\n", server.socket_path());
    let node = Node::start(&[ahjo_terminals::LISTING], &config)
        .await
        .unwrap();
    let roots = TranscriptRoots {
        claude: transcripts,
        codex: server.dir().join("codex-sessions"),
        pi: server.dir().join("pi-sessions"),
    };
    let seats = LocalSeats::new(
        node.sessions(),
        server.dir().join("seats"),
        "local".into(),
        roots,
    );
    let root = server.dir().join("missions-root");
    std::fs::create_dir_all(root.join("missions")).unwrap();
    std::fs::write(root.join("missions/spend.mission.toml"), MISSION).unwrap();
    let coordinator = Coordinator::open(
        &root,
        RunSettings::default(),
        Box::new(SeatExecutor::new(
            Arc::new(seats),
            tokio::runtime::Handle::current(),
        )),
    )
    .unwrap();
    let workspace = server.dir().join("work");
    std::fs::create_dir(&workspace).unwrap();
    let run = coordinator
        .start_mission(RunStart {
            mission: "spend".into(),
            input_card_id: "inp_spend".into(),
            expected_rev: 1,
            cwd: workspace.to_string_lossy().into(),
            inputs: BTreeMap::from([("brief".into(), "go".into())]),
            ..Default::default()
        })
        .unwrap();
    let projection = tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            let projection = coordinator.run(&run).unwrap();
            if projection.r#final || projection.status == "blocked" {
                break projection;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap_or_else(|_| {
        // What the seat showed and what its records hold say where it stuck.
        let screens: Vec<String> = server
            .run(&["list-sessions", "-F", "#{session_name}"])
            .lines()
            .map(|s| server.run(&["capture-pane", "-p", "-t", s]))
            .collect();
        let records = std::process::Command::new("sh")
            .args(["-c", "find \"$1\" -name '*.json' -exec cat {} +", "sh"])
            .arg(server.dir().join("seats"))
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned());
        panic!("the run did not end:\n{}\n{records:?}", screens.join("\n"))
    });
    assert_eq!(projection.status, "succeeded", "{projection:?}");

    let attempts = coordinator.attempts(&run, None).unwrap();
    let attempt = &attempts.steps[0].attempts[0];
    assert!(attempt.wall_ms > 0, "{attempt:?}");
    let [seat] = attempt.seats.as_slice() else {
        panic!("one seat: {attempt:?}");
    };
    assert_eq!(
        (
            seat.seat_id.as_str(),
            seat.agent.as_str(),
            seat.harness.as_str(),
            seat.model.as_str(),
            seat.effort.as_str(),
            seat.harness_session.as_str(),
        ),
        (
            "seat_work",
            "builder",
            "claude-code",
            "claude-fable-9",
            "high",
            "fake-conversation"
        )
    );
    assert!(!seat.session.is_empty(), "{seat:?}");
    assert_eq!(
        seat.tokens,
        Some(AttemptTokens {
            input: 16,
            cache_write: 2300,
            cache_read: 67000,
            output: 164,
        })
    );
    assert!(seat.wall_ms > 0, "{seat:?}");
    node.stop().await;
}
