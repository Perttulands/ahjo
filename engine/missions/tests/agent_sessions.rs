//! Exercise session ownership and finalization on an isolated tmux server.
use ahjo_mission_engine::{
    Coordinator, Dispatch, Error, ExecutionEvent, ExecutionSink, Executor, LabExecutor, LeftSeat,
    LocalSeats, RunSettings, StepExecution, StepResult, TranscriptRoots,
};
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
        step_id: "fmn_write".into(),
        seat_id: "seat_1".into(),
        name: name.into(),
        harness: "pi".into(),
        model: String::new(),
        effort: String::new(),
        cwd: cwd.into(),
        brief: "Write.".into(),
        reuse_dispatch: None,
        keep,
        run_owned: false,
        environment: String::new(),
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

    // A run owns one live session across attempts. A retry rejected before
    // its creation event reaches the ledger must not kill the first attempt's
    // already recorded session.
    let mut first = dispatch("dsp_run_1", "demo-owned", false);
    first.run_owned = true;
    seats.dispatch(first).await.unwrap();
    let original = seat("dsp_run_1");
    assert_eq!(
        seats
            .end_seat("dsp_run_1", "attempt_finished")
            .await
            .unwrap(),
        "released"
    );
    let mut retry = dispatch("dsp_run_2", "demo-owned", false);
    retry.run_owned = true;
    seats.dispatch(retry).await.unwrap();
    assert_eq!(seat("dsp_run_2").session, original.session);
    assert_eq!(
        seats.end_seat("dsp_run_2", "unrecorded").await.unwrap(),
        "released"
    );
    assert!(live(&server, "demo-owned"));
    assert_eq!(
        seats.end_seat("dsp_run_2", "run_final").await.unwrap(),
        "ended"
    );
    assert!(!live(&server, "demo-owned"));

    let mut renamed = dispatch("dsp_renamed", "demo-renamed", false);
    renamed.run_owned = true;
    seats.dispatch(renamed).await.unwrap();
    server.run(&["rename-session", "-t", "demo-renamed", "demo-renamed-live"]);
    server.run(&["new-session", "-d", "-s", "demo-renamed"]);
    assert_eq!(
        seats.end_seat("dsp_renamed", "run_final").await.unwrap(),
        "ended"
    );
    assert!(!live(&server, "demo-renamed-live"));
    assert!(live(&server, "demo-renamed"));

    // An entirely new seat with no recorded creation must be cleaned up.
    let mut unrecorded = dispatch("dsp_unrecorded", "demo-unrecorded", true);
    unrecorded.run_owned = true;
    seats.dispatch(unrecorded).await.unwrap();
    assert_eq!(
        seats
            .end_seat("dsp_unrecorded", "unrecorded")
            .await
            .unwrap(),
        "ended"
    );
    assert!(!live(&server, "demo-unrecorded"));

    let mut kept = dispatch("dsp_run_kept", "demo-kept", true);
    kept.run_owned = true;
    seats.dispatch(kept).await.unwrap();
    assert_eq!(
        seats
            .end_seat("dsp_run_kept", "attempt_finished")
            .await
            .unwrap(),
        "released"
    );
    assert_eq!(
        seats.end_seat("dsp_run_kept", "run_final").await.unwrap(),
        "kept"
    );
    assert!(live(&server, "demo-kept"));
    run_owned_seats_end_with_the_run(&server, &node, theirs).await;
    node.stop().await;
}

struct TmuxLab {
    seats: LocalSeats,
    runtime: tokio::runtime::Handle,
}

impl Executor for TmuxLab {
    fn execute_step(
        &self,
        request: StepExecution,
        sink: ExecutionSink,
    ) -> Result<StepResult, Error> {
        let name = format!("lab-{}", request.run_id);
        let id = format!("dsp_{}", request.run_id);
        let mut dispatch = dispatch_in(&id, &name, request.keep_sessions, &request.cwd);
        dispatch.run_id.clone_from(&request.run_id);
        dispatch.step_id.clone_from(&request.step.id);
        dispatch.seat_id.clone_from(&request.step.seats[0].id);
        dispatch.run_owned = true;
        self.runtime.block_on(self.seats.dispatch(dispatch))?;
        let created = self
            .seats
            .records()
            .events(&id, 0)?
            .into_iter()
            .find(|event| event.kind == "seat_created")
            .expect("local seats record their real tmux identity");
        sink.emit(ExecutionEvent {
            kind: created.kind,
            seat_id: request.step.seats[0].id.clone(),
            data: created.data,
        })?;
        LabExecutor.execute_step(request, sink)
    }

    fn end_left_seat(&self, seat: LeftSeat) -> Result<String, Error> {
        let id = seat.identity["dispatch_id"].as_str().unwrap();
        assert_eq!(
            self.seats.records().seat(id)?.unwrap().session.session_id,
            seat.identity["session_id"]
        );
        self.runtime.block_on(self.seats.end_seat(id, &seat.cause))
    }
}

async fn run_owned_seats_end_with_the_run(server: &Server, node: &Node, operator_id: String) {
    let root = server.dir().join("coordinator");
    std::fs::create_dir_all(root.join("missions")).unwrap();
    std::fs::write(
        root.join("missions/hello.mission.toml"),
        include_str!("parity/runs/missions/solo-lab.mission.toml"),
    )
    .unwrap();
    let coordinator = Coordinator::open(
        &root,
        RunSettings::default(),
        Box::new(TmuxLab {
            seats: LocalSeats::new(
                node.sessions(),
                &root,
                "local".into(),
                TranscriptRoots::from_host(),
            ),
            runtime: tokio::runtime::Handle::current(),
        }),
    )
    .unwrap();
    for keep_sessions in [false, true] {
        let id = coordinator
            .start_mission(ahjo_mission_model::RunStart {
                mission: "hello".into(),
                input_card_id: "inp_hello".into(),
                expected_rev: 1,
                inputs: std::collections::BTreeMap::from([(
                    "brief".into(),
                    "Exercise seats.".into(),
                )]),
                keep_sessions,
                ..Default::default()
            })
            .unwrap();
        let run = eventually("run to settle", || {
            let projection = coordinator.run(&id).unwrap();
            (projection.r#final || projection.status == "blocked").then_some(projection)
        })
        .await;
        assert_eq!(run.status, "succeeded", "{run:?}");
        let name = format!("lab-{id}");
        assert_eq!(live(server, &name), keep_sessions);
        let cleanup = run
            .events
            .iter()
            .find(|event| event.kind == "seat_cleanup")
            .unwrap();
        assert_eq!(
            cleanup.outcome,
            if keep_sessions { "kept" } else { "ended" }
        );
        let owner = coordinator.clone();
        tokio::task::spawn_blocking(move || owner.delete_run(&id).unwrap())
            .await
            .unwrap();
        assert_eq!(live(server, &name), keep_sessions);
    }
    assert_eq!(
        server.display("demo-reviewer", "#{session_id}").trim(),
        operator_id
    );
}
