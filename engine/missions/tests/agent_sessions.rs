//! A step's agent works in its own session, named under the mission's
//! prefix: one a run started is used where it runs and outlives the run; a
//! dispatch that wants it while another holds it gets a session of its
//! own, ended with it. A session nobody's run started is never taken over,
//! whatever its name.
use ahjo_mission_engine::{Dispatch, LocalSeats, TranscriptRoots};
use ahjo_testkit::{
    node::Node,
    tmux::{Server, eventually, stubbed_path, write_script},
};

fn dispatch(id: &str, name: &str, keep: bool) -> Dispatch {
    dispatch_in(id, name, keep, "/")
}

fn dispatch_in(id: &str, name: &str, keep: bool, cwd: impl Into<std::path::PathBuf>) -> Dispatch {
    Dispatch {
        id: id.into(),
        run_id: "run_1".into(),
        node_id: "fmn_write".into(),
        seat_id: "seat_1".into(),
        name: name.into(),
        harness: "pi".into(),
        model: String::new(),
        effort: String::new(),
        cwd: cwd.into(),
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
async fn an_agents_session_is_used_again_and_a_namesake_is_left_alone() {
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
    // The operator's own session, started for something else, happens to
    // have the name the writer's session would get.
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
    let theirs = server
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

    // The run did not start that session, so the step leaves it alone and
    // works in one of its own beside it, which ends with it.
    seats
        .dispatch(dispatch("dsp_1", "demo-writer", true))
        .await
        .unwrap();
    let beside = seat("dsp_1");
    assert!(
        beside.session_name.starts_with("demo-writer-"),
        "{beside:?}"
    );
    assert_ne!(beside.session.session_id, theirs);
    assert!(!seats.records().dispatch("dsp_1").unwrap().keep);
    // Asked again, as after a crash, it is the same seat, not the namesake.
    seats
        .dispatch(dispatch("dsp_1", "demo-writer", true))
        .await
        .unwrap();
    assert_eq!(seat("dsp_1"), beside);
    assert_eq!(seats.end_seat("dsp_1", "run_final").await.unwrap(), "ended");
    assert!(!live(&server, &beside.session_name));
    assert_eq!(
        server.display("demo-writer", "#{session_id}").trim(),
        theirs
    );

    // An agent without a session is started, in the run's folder.
    seats
        .dispatch(dispatch("dsp_2", "demo-reviewer", true))
        .await
        .unwrap();
    let started = seat("dsp_2");
    assert_eq!(started.session_name, "demo-reviewer");
    assert_eq!(started.cwd, None);
    // The session exists before its program does: tmux's child, then `env`
    // and the fake's shell each run there before the fake execs `sleep`.
    eventually("the fake harness running in demo-reviewer", || {
        (server.display("demo-reviewer", "#{pane_current_command}") == "sleep").then_some(())
    })
    .await;

    // Another attempt wants the reviewer while the first holds it: it gets
    // a session of its own, which ends with it.
    seats
        .dispatch(dispatch("dsp_3", "demo-reviewer", true))
        .await
        .unwrap();
    let own = seat("dsp_3");
    assert!(own.session_name.starts_with("demo-reviewer-"), "{own:?}");
    assert!(!seats.records().dispatch("dsp_3").unwrap().keep);
    assert!(live(&server, &own.session_name));
    assert_eq!(seats.end_seat("dsp_3", "run_final").await.unwrap(), "ended");
    assert!(!live(&server, &own.session_name));

    // The reviewer's session stays when the run is done with it, and the
    // next run uses it, where it runs.
    assert_eq!(seats.end_seat("dsp_2", "run_final").await.unwrap(), "kept");
    assert!(live(&server, "demo-reviewer"));
    seats
        .dispatch(dispatch("dsp_4", "demo-reviewer", true))
        .await
        .unwrap();
    let again = seat("dsp_4");
    assert_eq!(again.session, started.session);
    assert_eq!(again.cwd.as_deref(), Some(std::path::Path::new("/")));
    assert_eq!(seats.end_seat("dsp_4", "run_final").await.unwrap(), "kept");

    // An agent started in a checkout that is then removed, as a repository
    // run's attempt is, still stands under that folder's own name, which is
    // where its harness keeps the record of its turns.
    let checkout = server.dir().join("turn-1");
    std::fs::create_dir(&checkout).unwrap();
    seats
        .dispatch(dispatch_in("dsp_6", "demo-builder", true, &checkout))
        .await
        .unwrap();
    eventually("the fake harness running in demo-builder", || {
        (server.display("demo-builder", "#{pane_current_command}") == "sleep").then_some(())
    })
    .await;
    assert_eq!(seats.end_seat("dsp_6", "run_final").await.unwrap(), "kept");
    std::fs::remove_dir(&checkout).unwrap();
    seats
        .dispatch(dispatch("dsp_7", "demo-builder", true))
        .await
        .unwrap();
    assert_eq!(seat("dsp_7").cwd.as_deref(), Some(checkout.as_path()));
    assert_eq!(seats.end_seat("dsp_7", "run_final").await.unwrap(), "kept");

    // Renamed by a person, it is no longer the reviewer's: a session that
    // took the name since is not either.
    server.run(&["rename-session", "-t", "demo-reviewer", "mine"]);
    server.run(&["rename-session", "-t", "demo-writer", "demo-reviewer"]);
    seats
        .dispatch(dispatch("dsp_5", "demo-reviewer", true))
        .await
        .unwrap();
    assert_ne!(seat("dsp_5").session.session_id, theirs);
    assert!(seat("dsp_5").session_name.starts_with("demo-reviewer-"));
    assert_eq!(seats.end_seat("dsp_5", "run_final").await.unwrap(), "ended");
    node.stop().await;
}
