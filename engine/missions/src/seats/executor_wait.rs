//! A step placed on an environment that does not answer (#231): the wait
//! shows on the step, and stopping the run ends it, before the step's seat
//! exists and while it works.
use crate::{
    AgentSeat, BindingsSnapshot, Dispatch, DispatchResult, ExecutionEvent, ExecutionSink, Executor,
    PublishedTurn, SeatEvent, SeatExecutor, SeatFuture, SeatId, SeatSide, StepExecution,
    TurnWorkspace, executor::PendingEvent,
};
use ahjo_mission_model::{Seat, Step};
use serde_json::json;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, atomic::AtomicBool, atomic::Ordering, mpsc},
    time::Duration,
};
use tokio::sync::mpsc as stream;

/// Seats on an environment whose node never answers; with `seated`, it
/// answered until its seat was ready and its brief staged.
#[derive(Default)]
struct Away {
    seated: bool,
    started: Option<mpsc::Sender<()>>,
    proceed: Option<Arc<tokio::sync::Notify>>,
    streams: Mutex<Vec<stream::Sender<Result<SeatEvent, crate::Error>>>>,
}

fn never<'a, T: 'a>() -> SeatFuture<'a, T> {
    Box::pin(std::future::pending())
}

impl SeatSide for Away {
    fn dispatch(&self, dispatch: Dispatch) -> SeatFuture<'_, String> {
        if let Some(proceed) = &self.proceed {
            return Box::pin(async move {
                self.started.as_ref().unwrap().send(()).unwrap();
                proceed.notified().await;
                Ok(dispatch.id)
            });
        }
        if !self.seated {
            return never();
        }
        Box::pin(async move { Ok(dispatch.id) })
    }
    fn events(
        &self,
        _: &str,
        _: u64,
    ) -> Result<stream::Receiver<Result<SeatEvent, crate::Error>>, crate::Error> {
        let (sender, events) = stream::channel(4);
        let seat = SeatId {
            node: "n2".into(),
            session_name: "ahjo-run_1".into(),
            session: ahjo_term::SessionRef {
                session_id: "$1".into(),
                pane_id: 1,
                generation: ahjo_term::Generation {
                    pid: 1,
                    start_time: 1,
                },
            },
            cwd: None,
        };
        let prepared = json!({"seat":seat,"brief_path":"brief.md"});
        if self.proceed.is_some() {
            let created = json!({"node":seat.node,"session_name":seat.session_name,"session_id":seat.session.session_id,"pane_id":seat.session.pane_id,"generation":seat.session.generation,"run_owned":true});
            sender
                .try_send(Ok(SeatEvent {
                    key: "seat-created".into(),
                    seq: 1,
                    kind: "seat_created".into(),
                    data: created.as_object().unwrap().clone(),
                }))
                .unwrap();
        }
        sender
            .try_send(Ok(SeatEvent {
                key: "seat-prepared".into(),
                seq: 1,
                kind: "seat_prepared".into(),
                data: prepared.as_object().cloned().unwrap_or_default(),
            }))
            .unwrap();
        // The stream stays open: the node is away, not finished.
        self.streams.lock().unwrap().push(sender);
        Ok(events)
    }
    fn stage(&self, _: &str, _: &str) -> SeatFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }
    fn dispatch_info(&self, _: &str) -> Result<Dispatch, crate::Error> {
        Err(crate::Error::new("seat_error", "never dispatched"))
    }
    fn result(&self, _: &str) -> Result<Option<DispatchResult>, crate::Error> {
        Ok(None)
    }
    fn seat_alive(&self, _: &SeatId) -> SeatFuture<'_, bool> {
        never()
    }
    fn end_seat(&self, _: &SeatId, _: &str) -> SeatFuture<'_, String> {
        never()
    }
    fn seats(&self, _: &str) -> Result<Vec<AgentSeat>, crate::Error> {
        Ok(Vec::new())
    }
    fn prepare_turn(&self, _: &str, _: &TurnWorkspace) -> SeatFuture<'_, PathBuf> {
        never()
    }
    fn publish_turn(&self, _: &str, _: &TurnWorkspace) -> SeatFuture<'_, PublishedTurn> {
        never()
    }
    fn away(&self, environment: &str) -> Option<String> {
        if self.proceed.is_some() {
            return None;
        }
        Some(format!(
            "{environment} does not answer: no node of {environment} is known"
        ))
    }
}

#[test]
fn cancellation_waits_for_inflight_creation_and_reports_the_seat() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let (started, creation_started) = mpsc::channel();
    let proceed = Arc::new(tokio::sync::Notify::new());
    let seats = Away {
        started: Some(started),
        proceed: Some(proceed.clone()),
        ..Default::default()
    };
    let executor = SeatExecutor::new(Arc::new(seats), runtime.handle().clone());
    let canceled = Arc::new(AtomicBool::new(false));
    let request = execution(None, canceled.clone());
    let (sender, received) = mpsc::channel::<PendingEvent>();
    let (done, finished) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = done.send(executor.execute_step(request, ExecutionSink { sender }));
    });
    creation_started
        .recv_timeout(Duration::from_secs(10))
        .unwrap();
    canceled.store(true, Ordering::Release);
    assert!(matches!(
        finished.recv_timeout(Duration::from_millis(600)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    proceed.notify_one();
    let created = received.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(created.event.kind, "seat_created");
    assert_eq!(created.event.data["session_id"], "$1");
    created.ack.send(Ok(())).unwrap();
    let dispatch = received.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(dispatch.event.kind, "seat_dispatch");
    dispatch.ack.send(Err("run canceled".into())).unwrap();
    assert!(
        finished
            .recv_timeout(Duration::from_secs(10))
            .unwrap()
            .is_err()
    );
}

fn execution(turn: Option<TurnWorkspace>, canceled: Arc<AtomicBool>) -> StepExecution {
    StepExecution {
        run_id: "run_1".into(),
        prefix: String::new(),
        keep_sessions: false,
        step: Step {
            id: "fmn_build".into(),
            formation: ahjo_mission_model::Formation::Solo,
            environment: "box/windows".into(),
            seats: vec![Seat {
                id: "seat_1".into(),
                ..Default::default()
            }],
            ..Default::default()
        },
        bindings: BindingsSnapshot {
            schema: 1,
            run_id: "run_1".into(),
            mission_id: String::new(),
            mission_slug: String::new(),
            mission_rev: 1,
            input_card_id: String::new(),
            bindings: Vec::new(),
            gate_bindings: Vec::new(),
        },
        cwd: PathBuf::new(),
        turn,
        artifact_dir: PathBuf::new(),
        context_paths: Vec::new(),
        issue: String::new(),
        inputs: Vec::new(),
        input_text: String::new(),
        attempt: 1,
        judge: false,
        pick_attempts: Vec::new(),
        canceled,
        stopping: Arc::new(AtomicBool::new(false)),
        limits: Default::default(),
    }
}

/// Runs a step against `seats`, and stops it at the event of `kind`.
fn stop_at(
    seats: Away,
    turn: Option<TurnWorkspace>,
    kind: &str,
) -> (Vec<ExecutionEvent>, Result<(), String>) {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let executor = SeatExecutor::new(Arc::new(seats), runtime.handle().clone());
    let canceled = Arc::new(AtomicBool::new(false));
    let request = execution(turn, canceled.clone());
    let (sender, received) = mpsc::channel::<PendingEvent>();
    let (done, finished) = mpsc::channel();
    std::thread::spawn(move || {
        let result = executor.execute_step(request, ExecutionSink { sender });
        let _ = done.send(result.map(|_| ()));
    });
    let mut events = Vec::new();
    while let Ok(pending) = received.recv_timeout(Duration::from_secs(10)) {
        let _ = pending.ack.send(Ok(()));
        let stop = pending.event.kind == kind;
        events.push(pending.event);
        if stop {
            canceled.store(true, Ordering::Release);
        }
    }
    let result = finished
        .recv_timeout(Duration::from_secs(10))
        .expect("the stop ends the wait");
    (events, result.map_err(|error| error.message))
}

#[test]
fn a_step_waiting_for_its_place_says_so_and_a_stop_ends_the_wait() {
    // Before its turn is prepared there, and before its dispatch is taken.
    for turn in [Some(TurnWorkspace::default()), None] {
        let (events, result) = stop_at(Away::default(), turn, "seat_state");
        let [wait] = events.as_slice() else {
            panic!("one wait, then nothing: {events:?}");
        };
        assert_eq!(
            (wait.kind.as_str(), wait.seat_id.as_str()),
            ("seat_state", "seat_1")
        );
        assert_eq!(
            wait.data["detail"],
            "box/windows does not answer: no node of box/windows is known"
        );
        assert!(wait.data["since"].as_str().is_some_and(|s| !s.is_empty()));
        assert_eq!(result, Err("canceled".into()));
    }
}

#[test]
fn a_stop_leaves_the_seat_of_a_node_that_went_away() {
    let seated = Away {
        seated: true,
        ..Default::default()
    };
    let (events, result) = stop_at(seated, None, "seat_dispatch");
    assert_eq!(events.len(), 1, "{events:?} {result:?}");
    assert_eq!(
        result,
        Err("canceled while box/windows does not answer: no node of box/windows is known; its seat is left as it is".into())
    );
}
