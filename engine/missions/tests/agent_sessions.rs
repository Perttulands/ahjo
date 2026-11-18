//! A step's agent works in its own session, named under the mission's
//! prefix: a live one is used where it already runs and outlives the run;
//! a dispatch that wants it while another holds it gets a session of its
//! own, ended with it.
use ahjo_mission_engine::{Dispatch, LocalSeats, TranscriptRoots};
use std::path::Path;

use ahjo_testkit::{
    node::Node,
    tmux::{Server, eventually, stubbed_path, write_script},
};

fn dispatch(id: &str, name: &str, keep: bool) -> Dispatch {
    Dispatch {
        id: id.into(),
        run_id: "run_1".into(),
        node_id: "fmn_write".into(),
        slot_id: "slot_1".into(),
        name: name.into(),
        harness: "pi".into(),
        model: String::new(),
        effort: String::new(),
        cwd: "/".into(),
        brief: "Write.".into(),
        reuse_dispatch: None,
        keep,
        place: String::new(),
        turn: None,
    }
}

fn live(server: &Server, name: &str) -> bool {
    server
        .command()
        .args(["has-session", "-t", &format!("={name}")])
        .status()
        .unwrap()
        .success()
}

#[tokio::test(flavor = "multi_thread")]
async fn an_agents_live_session_is_used_and_outlives_the_run() {
    let Some(server) = Server::start() else {
        return;
    };
    // A fake harness that just stays, found before anything real.
    let fakes = server.dir().join("fakes");
    std::fs::create_dir(&fakes).unwrap();
    write_script(&fakes.join("pi"), "#!/bin/sh\nexec /bin/sleep 600\n");
    // A session takes the PATH of the node that starts it, which is this
    // process: the fake must come first or the real harness would run.
    // SAFETY: this file's only test, before anything reads the environment.
    unsafe { std::env::set_var("PATH", stubbed_path(&fakes)) };
    let home = server.dir().join("home");
    std::fs::create_dir(&home).unwrap();
    server.run(&[
        "new-session",
        "-d",
        "-s",
        "demo-writer",
        "-c",
        &home.to_string_lossy(),
    ]);
    // tmux answers before the pane's child has moved into its folder; until
    // then the pane reports the folder the server was started in.
    eventually("demo-writer standing in its folder", || {
        (Path::new(server.display("demo-writer", "#{pane_current_path}").trim()) == home)
            .then_some(())
    })
    .await;
    let live_id = server
        .display("demo-writer", "#{session_id}")
        .trim()
        .to_owned();

    let config = format!("[terminals]\nsocket = {:?}\n", server.socket_path());
    let node = Node::start(&[ahjo_terminals::LISTING], &config)
        .await
        .unwrap();
    let seats = LocalSeats::new(
        node.sessions(),
        server.dir().join("seats"),
        "local".into(),
        TranscriptRoots::from_host(),
    );
    let seat = |id: &str| seats.records().seat(id).unwrap().unwrap();

    // The writer's session is live: the step uses it, where it runs.
    seats
        .dispatch(dispatch("dsp_1", "demo-writer", true))
        .await
        .unwrap();
    let used = seat("dsp_1");
    assert_eq!(used.session_name, "demo-writer");
    assert_eq!(used.session.session_id, live_id);
    assert_eq!(used.cwd.as_deref(), Some(home.as_path()));

    // Another attempt wants the writer while the first holds it: it gets a
    // session of its own, which ends with it.
    seats
        .dispatch(dispatch("dsp_2", "demo-writer", true))
        .await
        .unwrap();
    let own = seat("dsp_2");
    assert!(own.session_name.starts_with("demo-writer-"), "{own:?}");
    assert!(!seats.records().dispatch("dsp_2").unwrap().keep);
    assert!(live(&server, &own.session_name));
    assert_eq!(seats.end_seat("dsp_2", "run_final").await.unwrap(), "ended");
    assert!(!live(&server, &own.session_name));

    // The writer's session stays when the run is done with it, for the next
    // run to use again.
    assert_eq!(seats.end_seat("dsp_1", "run_final").await.unwrap(), "kept");
    assert!(live(&server, "demo-writer"));
    seats
        .dispatch(dispatch("dsp_3", "demo-writer", true))
        .await
        .unwrap();
    assert_eq!(seat("dsp_3").session.session_id, live_id);
    assert_eq!(seats.end_seat("dsp_3", "run_final").await.unwrap(), "kept");

    // An agent without a live session is started, in the run's folder, and
    // stays too.
    seats
        .dispatch(dispatch("dsp_4", "demo-reviewer", true))
        .await
        .unwrap();
    let started = seat("dsp_4");
    assert_eq!(started.session_name, "demo-reviewer");
    assert_eq!(started.cwd, None);
    // The session exists before its program does: tmux's child, then `env`
    // and the fake's shell each run there before the fake execs `sleep`.
    eventually("the fake harness running in demo-reviewer", || {
        (server.display("demo-reviewer", "#{pane_current_command}") == "sleep").then_some(())
    })
    .await;
    assert_eq!(seats.end_seat("dsp_4", "run_final").await.unwrap(), "kept");
    assert!(live(&server, "demo-reviewer"));
    node.stop().await;
}
