#[allow(dead_code)]
mod run_support;
use ahjo_mission_engine::{Coordinator, LabExecutor, RunSettings};
use ahjo_mission_model::RunStart;
use run_support::{Scratch, gated_mission, normalized_ledger};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

/// The test controls dispatch and completion independently. No wall-clock
/// delay decides which of two concurrent branches writes its next event.
type BranchGates = (
    std::sync::Mutex<BTreeMap<String, (bool, bool)>>,
    std::sync::Condvar,
);
#[derive(Clone, Default)]
struct BranchControl(std::sync::Arc<BranchGates>);
impl BranchControl {
    fn hold(&self, node: &str) {
        self.0.0.lock().unwrap().insert(node.into(), (false, false));
    }
    fn release(&self, node: &str, dispatch: bool, finish: bool) {
        let mut gates = self.0.0.lock().unwrap();
        let gate = gates.get_mut(node).unwrap();
        gate.0 |= dispatch;
        gate.1 |= finish;
        self.0.1.notify_all();
    }
    fn wait(&self, node: &str, finish: bool) {
        let gates = self.0.0.lock().unwrap();
        let (gates, timeout) = self
            .0
            .1
            .wait_timeout_while(gates, Duration::from_secs(20), |gates| {
                gates
                    .get(node)
                    .is_some_and(|gate| if finish { !gate.1 } else { !gate.0 })
            })
            .unwrap();
        assert!(
            !timeout.timed_out(),
            "branch {node} was not released: {gates:?}"
        );
    }
}
struct ControlledLab(BranchControl);
impl ahjo_mission_engine::Executor for ControlledLab {
    fn execute_step(
        &self,
        request: ahjo_mission_engine::StepExecution,
        sink: ahjo_mission_engine::ExecutionSink,
    ) -> Result<ahjo_mission_engine::StepResult, ahjo_mission_engine::Error> {
        let node = &request.step.id;
        let seat = &request.step.seats[0];
        self.0.wait(node, false);
        let dispatch = format!("dsp_{}_{}_{}", request.run_id, seat.id, request.attempt);
        sink.emit(ahjo_mission_engine::ExecutionEvent {
            kind: "seat_dispatch".into(),
            seat_id: seat.id.clone(),
            data: serde_json::json!({"dispatch_id":dispatch,"card_id":node,"seat_id":seat.id})
                .as_object()
                .unwrap()
                .clone(),
        })?;
        self.0.wait(node, true);
        sink.emit(ahjo_mission_engine::ExecutionEvent { kind: "seat_result".into(), seat_id: seat.id.clone(), data: serde_json::json!({"dispatch_id":dispatch,"card_id":node,"seat_id":seat.id,"status":"ok","sentinel":{"run_id":request.run_id,"status":"ok"}}).as_object().unwrap().clone() })?;
        let text = format!(
            "lab-fake output from {} using {} for {}\nseat: {}\ninput:{}{}",
            if seat.agent.is_empty() {
                "vanilla"
            } else {
                &seat.agent
            },
            seat.harness,
            request.step.title,
            seat.id,
            if request.input_text.starts_with("```") {
                "\n"
            } else {
                " "
            },
            request.input_text
        );
        let outputs = request
            .step
            .outputs
            .iter()
            .map(|port| (port.id.clone(), serde_json::json!({"text":text})))
            .collect();
        Ok(ahjo_mission_engine::StepResult {
            status: "done".into(),
            text,
            outputs,
        })
    }
}
fn wait_for(c: &Coordinator, id: &str, kind: &str, node: &str, deadline: Instant) -> u64 {
    loop {
        let projection = c.run(id).unwrap();
        if let Some(event) = projection
            .events
            .iter()
            .find(|event| event.kind == kind && event.card_id == node)
        {
            return event.seq;
        }
        assert!(
            Instant::now() < deadline,
            "missing {kind} for {node}: {:?}",
            projection.events
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn reattach_keeps_other_completed_dispatches_when_one_fails() {
    use ahjo_mission_engine::{
        ExecutionEvent, ExecutionSink, Executor, OpenDispatch, StepExecution, StepResult,
    };
    use ahjo_mission_model::{ResumeMode, ResumeRequest};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    struct Seats {
        release: Arc<AtomicBool>,
        fail: bool,
    }
    impl Executor for Seats {
        fn execute_step(
            &self,
            req: StepExecution,
            sink: ExecutionSink,
        ) -> Result<StepResult, ahjo_mission_engine::Error> {
            (|| -> Result<StepResult, String> {
            let seat = &req.step.seats[0];
            let dispatch = format!("dsp_{}_{}", req.step.id, req.attempt);
            sink.emit(ExecutionEvent { kind:"seat_dispatch".into(), seat_id:seat.id.clone(), data:serde_json::json!({"dispatch_id":dispatch,"card_id":req.step.id,"seat_id":seat.id}).as_object().unwrap().clone() }).map_err(|error| error.message)?;
            while !self.release.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(5));
            }
            sink.emit(ExecutionEvent { kind:"seat_result".into(), seat_id:seat.id.clone(), data:serde_json::json!({"dispatch_id":dispatch,"status":"ok","sentinel":{"run_id":req.run_id}}).as_object().unwrap().clone() }).map_err(|error| error.message)?;
            Ok(StepResult {
                status: "done".into(),
                text: "done".into(),
                outputs: req
                    .step
                    .outputs
                    .iter()
                    .map(|p| (p.id.clone(), serde_json::json!({"text":"done"})))
                    .collect(),
            })
         })().map_err(|message: String| ahjo_mission_engine::Error::new("executor_error", message))
        }
        fn reattach(&self, req: OpenDispatch) -> Result<StepResult, ahjo_mission_engine::Error> {
            (|| -> Result<StepResult, String> {
                if self.fail && req.step_id == "fmn_other" {
                    return Err("capture unavailable".into());
                }
                Ok(StepResult {
                    status: "done".into(),
                    text: "done".into(),
                    outputs: req
                        .step
                        .outputs
                        .iter()
                        .map(|p| (p.id.clone(), serde_json::json!({"text":"done"})))
                        .collect(),
                })
            })()
            .map_err(|message: String| ahjo_mission_engine::Error::new("executor_error", message))
        }
    }
    let root = Scratch::new();
    let solo = include_str!("parity/runs/missions/solo-lab.mission.toml");
    let other = solo
        .split("[[step]]")
        .nth(1)
        .unwrap()
        .split("[[end]]")
        .next()
        .unwrap()
        .replace("fmn_work", "fmn_other")
        .replace("seat_work", "seat_other");
    let mission = format!(
        "{solo}\n[[step]]{other}\n[[connection]]\nid = \"edge_other\"\nfrom = \"inp_hello:out\"\nto = \"fmn_other:port_in\"\n[[connection]]\nid = \"edge_other_end\"\nfrom = \"fmn_other:port_out\"\nto = \"end_done:in\"\n"
    );
    std::fs::write(root.0.join("missions/hello.mission.toml"), mission).unwrap();
    let release = Arc::new(AtomicBool::new(false));
    let original = Coordinator::open(
        &root.0,
        RunSettings::default(),
        Box::new(Seats {
            release: release.clone(),
            fail: false,
        }),
    )
    .unwrap();
    let id = original
        .start_mission(RunStart {
            mission: "hello".into(),
            input_card_id: "inp_hello".into(),
            expected_rev: 1,
            inputs: BTreeMap::from([("brief".into(), "hello".into())]),
            ..Default::default()
        })
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if original
            .run(&id)
            .unwrap()
            .events
            .iter()
            .filter(|e| e.kind == "seat_dispatch")
            .count()
            == 2
        {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    let scratch = Scratch::new();
    let source = root.0.join("runs");
    let target = scratch.0.join("runs");
    std::fs::create_dir_all(&target).unwrap();
    for suffix in ["ndjson", "snapshot.toml", "bindings.toml"] {
        std::fs::copy(
            source.join(format!("{id}.{suffix}")),
            target.join(format!("{id}.{suffix}")),
        )
        .unwrap();
    }
    release.store(true, Ordering::Release);
    while !original.run(&id).unwrap().r#final {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    drop(original);
    let c = Coordinator::open(
        &scratch.0,
        RunSettings::default(),
        Box::new(Seats {
            release,
            fail: true,
        }),
    )
    .unwrap();
    loop {
        let p = c.run(&id).unwrap();
        if p.block_code == "dispatch_reattach_failed" {
            assert!(
                p.events
                    .iter()
                    .any(|e| e.kind == "card_output" && e.card_id == "fmn_work")
            );
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    let events = ahjo_mission_engine::Ledger::new(target.join(format!("{id}.ndjson")), &id)
        .read()
        .unwrap();
    let block = events
        .iter()
        .rev()
        .find(|e| e.kind == "run_blocked")
        .unwrap();
    let remaining = block.data["open_dispatches"].as_array().unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0]["card_id"], "fmn_other");
    loop {
        match c.resume(
            &id,
            ResumeRequest {
                mode: ResumeMode::Redispatch,
                ..Default::default()
            },
        ) {
            Ok(_) => break,
            Err(e) if e.code == "conflict" && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(5))
            }
            Err(e) => panic!("resume: {e}"),
        }
    }
    loop {
        let p = c.run(&id).unwrap();
        if p.r#final {
            assert_eq!(p.status, "succeeded");
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn restart_cuts_preserve_branch_and_human_gate_outcomes() {
    use sha2::{Digest, Sha256};
    struct JudgeLab;
    impl ahjo_mission_engine::Executor for JudgeLab {
        fn execute_step(
            &self,
            request: ahjo_mission_engine::StepExecution,
            sink: ahjo_mission_engine::ExecutionSink,
        ) -> Result<ahjo_mission_engine::StepResult, ahjo_mission_engine::Error> {
            (|| -> Result<ahjo_mission_engine::StepResult, String> {
            let judge = request.step.id == "fmn_judge";
            let pass = request.input_text.contains("needs work");
            let mut result =
                ahjo_mission_engine::Executor::execute_step(&LabExecutor, request, sink).map_err(|error| error.message)?;
            if judge {
                result.text = if pass { "```ahjo-verdict\n{\"verdict\":\"pass\",\"reason\":\"fixed\",\"evidence\":[]}\n```" } else { "```ahjo-verdict\n{\"verdict\":\"fail\",\"reason\":\"needs work\",\"evidence\":[]}\n```" }.into();
                for output in result.outputs.values_mut() {
                    output["text"] = result.text.clone().into();
                }
            }
            Ok(result)
         })().map_err(|message: String| ahjo_mission_engine::Error::new("executor_error", message))
        }
    }
    fn drive(c: &Coordinator, id: &str, retry: bool) -> ahjo_mission_model::RunProjection {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let p = c.run(id).unwrap();
            if p.r#final {
                return p;
            }
            if p.status == "blocked" {
                assert!(p.resume_allowed, "{}", p.block_code);
                match c.resume(
                    id,
                    ahjo_mission_model::ResumeRequest {
                        mode: ahjo_mission_model::ResumeMode::Redispatch,
                        ..Default::default()
                    },
                ) {
                    Ok(_) => {}
                    Err(e) if e.code == "conflict" => {}
                    Err(e) => panic!("resume: {e}"),
                }
            } else if let Some(gate) = p.waiting_gates.first() {
                match c.record_verdict(
                    id,
                    &gate.gate_id,
                    ahjo_mission_model::HumanVerdict {
                        requested_seq: gate.requested_seq,
                        verdict: if retry
                            && !p.events.iter().any(|event| {
                                event.kind == "human_verdict_recorded"
                                    && event.gate_id == gate.gate_id
                            }) {
                            "fail"
                        } else {
                            "pass"
                        }
                        .into(),
                        ..Default::default()
                    },
                ) {
                    Ok(_) => {}
                    Err(e) if e.code == "conflict" => {}
                    Err(e) => panic!("verdict: {e}"),
                }
            }
            assert!(
                Instant::now() < deadline,
                "run {id} did not settle ({} {})",
                p.status,
                p.block_code
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    fn outcome(
        p: &ahjo_mission_model::RunProjection,
    ) -> (String, Vec<String>, BTreeMap<String, usize>) {
        let mut counts = BTreeMap::new();
        for event in &p.events {
            if event.kind == "card_output" {
                *counts.entry(event.card_id.clone()).or_default() += 1;
            }
        }
        (
            p.status.clone(),
            p.events.last().unwrap().end_ids.clone(),
            counts,
        )
    }
    for scenario in 0..3 {
        let retry = scenario == 1;
        let root = Scratch::new();
        let solo = include_str!("parity/runs/missions/solo-lab.mission.toml");
        let other = solo
            .split("[[step]]")
            .nth(1)
            .unwrap()
            .split("[[end]]")
            .next()
            .unwrap()
            .replace("fmn_work", "fmn_other")
            .replace("seat_work", "seat_other")
            .replace(
                "Return a short result",
                "archon-lab-work:40ms Return a short result",
            );
        let mut mission = format!(
            "{}\n[[step]]{other}\n[[connection]]\nid = \"edge_other\"\nfrom = \"inp_hello:out\"\nto = \"fmn_other:port_in\"\n[[connection]]\nid = \"edge_other_end\"\nfrom = \"fmn_other:port_out\"\nto = \"end_done:in\"\n",
            gated_mission("kinds = [\"human\"]")
        );
        if retry || scenario == 2 {
            mission = mission.replace("to = \"end_rejected:in\"", "to = \"fmn_work:port_in\"");
        }
        if scenario == 2 {
            mission = mission.replace("kinds = [\"human\"]", "kinds = [\"step\"]");
            let judge = solo
                .split("[[step]]")
                .nth(1)
                .unwrap()
                .split("[[end]]")
                .next()
                .unwrap()
                .replace("fmn_work", "fmn_judge")
                .replace("seat_work", "seat_judge");
            mission.push_str(&format!("\n[[step]]{judge}\n[[connection]]\nid = \"edge_judge\"\nfrom = \"gate_review:judge\"\nto = \"fmn_judge:port_in\"\n[[connection]]\nid = \"edge_judge_return\"\nfrom = \"fmn_judge:port_out\"\nto = \"gate_review:judge\"\n"));
        }
        std::fs::write(root.0.join("missions/hello.mission.toml"), mission).unwrap();
        let c = Coordinator::open(&root.0, RunSettings::default(), Box::new(JudgeLab)).unwrap();
        let id = c
            .start_mission(RunStart {
                mission: "hello".into(),
                input_card_id: "inp_hello".into(),
                expected_rev: 1,
                inputs: BTreeMap::from([("brief".into(), "hello".into())]),
                ..Default::default()
            })
            .unwrap();
        let expected = outcome(&drive(&c, &id, retry));
        drop(c);
        let runs = root.0.join("runs");
        let ledger = std::fs::read_to_string(runs.join(format!("{id}.ndjson"))).unwrap();
        let lines: Vec<_> = ledger.lines().collect();
        for cut in 1..lines.len() {
            let scratch = Scratch::new();
            let target = scratch.0.join("runs");
            std::fs::create_dir_all(&target).unwrap();
            for suffix in ["snapshot.toml", "bindings.toml"] {
                std::fs::copy(
                    runs.join(format!("{id}.{suffix}")),
                    target.join(format!("{id}.{suffix}")),
                )
                .unwrap();
            }
            std::fs::write(
                target.join(format!("{id}.ndjson")),
                lines[..cut].join("\n") + "\n",
            )
            .unwrap();
            // A real crash retains any copies recorded before the cut.
            // These fixture steps only write reports, so the folders are empty.
            for line in &lines[..cut] {
                let event: Value = serde_json::from_str(line).unwrap();
                if event["type"] == "folder_attempt_started" {
                    let node = event["card_id"].as_str().unwrap();
                    let attempt = event["attempt"].as_i64().unwrap();
                    let copy = scratch
                        .0
                        .join("folder-attempts")
                        .join(&id)
                        .join(hex::encode(Sha256::digest(node.as_bytes())))
                        .join(attempt.to_string());
                    std::fs::create_dir_all(copy.join("base")).unwrap();
                    std::fs::create_dir(copy.join("work")).unwrap();
                }
            }
            let c =
                Coordinator::open(&scratch.0, RunSettings::default(), Box::new(JudgeLab)).unwrap();
            assert_eq!(
                outcome(&drive(&c, &id, retry)),
                expected,
                "scenario {scenario}, cut {cut}"
            );
        }
    }
}

#[test]
fn fan_out_runs_concurrently_and_join_waits_for_both() {
    for b_first in [false, true] {
        let root = Scratch::new();
        let solo = include_str!("parity/runs/missions/solo-lab.mission.toml");
        let step = solo
            .split("[[step]]")
            .nth(1)
            .unwrap()
            .split("[[end]]")
            .next()
            .unwrap();
        let first = step
            .replace("fmn_work", "fmn_a")
            .replace("seat_work", "seat_a");
        let second = first.replace("fmn_a", "fmn_b").replace("seat_a", "seat_b");
        let join = step
            .replace("fmn_work", "fmn_join")
            .replace("seat_work", "seat_join")
            .replace(
                "[[step.output]]",
                "[[step.input]]\nid = \"port_second\"\nlabel = \"Second\"\n[[step.output]]",
            );
        let prefix = solo.split("[[step]]").next().unwrap();
        let end = solo
            .split("[[end]]")
            .nth(1)
            .unwrap()
            .split("[[connection]]")
            .next()
            .unwrap();
        let edges = [
            ("inp_hello:out", "fmn_a:port_in"),
            ("inp_hello:out", "fmn_b:port_in"),
            ("fmn_a:port_out", "fmn_join:port_in"),
            ("fmn_b:port_out", "fmn_join:port_second"),
            ("fmn_join:port_out", "end_done:in"),
        ];
        let mut mission =
            format!("{prefix}[[step]]{first}[[step]]{second}[[step]]{join}[[end]]{end}");
        for (i, (from, to)) in edges.iter().enumerate() {
            mission.push_str(&format!(
                "\n[[connection]]\nid = \"edge_{i}\"\nfrom = \"{from}\"\nto = \"{to}\"\n"
            ));
        }
        std::fs::write(root.0.join("missions/hello.mission.toml"), mission).unwrap();
        let control = BranchControl::default();
        control.hold("fmn_a");
        control.hold("fmn_b");
        let c = Coordinator::open(
            &root.0,
            RunSettings::default(),
            Box::new(ControlledLab(control.clone())),
        )
        .unwrap();
        let start = Instant::now();
        let id = c
            .start_mission(RunStart {
                mission: "hello".into(),
                input_card_id: "inp_hello".into(),
                expected_rev: 1,
                inputs: BTreeMap::from([("brief".into(), "hello".into())]),
                ..Default::default()
            })
            .unwrap();
        let deadline = start + Duration::from_secs(10);
        wait_for(&c, &id, "card_started", "fmn_a", deadline);
        wait_for(&c, &id, "card_started", "fmn_b", deadline);
        let (first, second) = if b_first {
            ("fmn_b", "fmn_a")
        } else {
            ("fmn_a", "fmn_b")
        };
        control.release(first, true, true);
        wait_for(&c, &id, "card_output", first, deadline);
        wait_for(&c, &id, "card_waiting", "fmn_join", deadline);
        control.release(second, true, true);
        let projection = loop {
            let p = c.run(&id).unwrap();
            if p.r#final || p.status == "blocked" {
                break p;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(projection.status, "succeeded");
        let events = &projection.events;
        let seq = |kind: &str, node: &str| {
            events
                .iter()
                .find(|e| e.kind == kind && e.card_id == node)
                .unwrap()
                .seq
        };
        assert!(seq("run_started", "") < seq("card_started", "inp_hello"));
        assert!(seq("card_output", "inp_hello") < seq("card_started", "fmn_a"));
        assert!(seq("card_output", "inp_hello") < seq("card_started", "fmn_b"));
        assert!(seq("card_output", first) < seq("card_waiting", "fmn_join"));
        assert!(seq("card_waiting", "fmn_join") < seq("card_output", second));
        assert!(seq("card_output", "fmn_join") < seq("run_succeeded", ""));
        assert!(seq("card_started", "fmn_a") < seq("card_output", "fmn_b"));
        assert!(seq("card_started", "fmn_b") < seq("card_output", "fmn_a"));
        assert!(
            seq("card_started", "fmn_join")
                > seq("card_output", "fmn_a").max(seq("card_output", "fmn_b"))
        );
        let expected: Vec<Value> = include_str!("parity/runs/archon-ledgers/fan-out-join.ndjson")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        run_support::assert_concurrent_ledger(&normalized_ledger(&root, &id), &expected);
        run_support::assert_concurrent_payload_parity(
            &root,
            &id,
            include_str!("parity/runs/archon-ledgers/fan-out-join.payload.ndjson"),
        );
    }
}

#[test]
fn human_gate_records_and_routes_the_exact_request() {
    use ahjo_mission_model::HumanVerdict;
    let root = Scratch::new();
    let solo = include_str!("parity/runs/missions/solo-lab.mission.toml");
    let mission = solo.replace("to = \"end_done:in\"", "to = \"gate_review:in\"");
    let mission = format!(
        "{mission}\n[[gate]]\nid = \"gate_review\"\ntitle = \"Review\"\nkinds = [\"human\"]\n\n[[end]]\nid = \"end_rejected\"\ntitle = \"Rejected\"\noutcome = \"rejected\"\n\n[[connection]]\nid = \"edge_pass\"\nfrom = \"gate_review:pass\"\nto = \"end_done:in\"\n\n[[connection]]\nid = \"edge_fail\"\nfrom = \"gate_review:fail\"\nto = \"end_rejected:in\"\n"
    );
    std::fs::write(root.0.join("missions/hello.mission.toml"), mission).unwrap();
    let c = Coordinator::open(&root.0, RunSettings::default(), Box::new(LabExecutor)).unwrap();
    let id = c
        .start_mission(RunStart {
            mission: "hello".into(),
            input_card_id: "inp_hello".into(),
            expected_rev: 1,
            inputs: BTreeMap::from([("brief".into(), "hello".into())]),
            ..Default::default()
        })
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let request = loop {
        let p = c.run(&id).unwrap();
        if let Some(gate) = p.waiting_gates.first() {
            break gate.requested_seq;
        }
        assert!(p.status != "blocked" && Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    };
    let verdict = HumanVerdict {
        requested_seq: request,
        verdict: "pass".into(),
        reason: " yes\n".into(),
        ..Default::default()
    };
    c.record_verdict(&id, "gate_review", verdict.clone())
        .unwrap();
    assert!(c.record_verdict(&id, "gate_review", verdict).is_err());
    loop {
        let p = c.run(&id).unwrap();
        if p.r#final {
            assert_eq!(
                p.status,
                "succeeded",
                "{}",
                std::fs::read_to_string(root.0.join("runs").join(format!("{id}.ndjson"))).unwrap()
            );
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    let actual = normalized_ledger(&root, &id);
    let expected: Vec<Value> = include_str!("parity/runs/archon-ledgers/human-approve.ndjson")
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(actual, expected);
    run_support::assert_payload_parity(
        &root,
        &id,
        include_str!("parity/runs/archon-ledgers/human-approve.payload.ndjson"),
    );
}

#[test]
fn human_answer_routes_while_another_branch_runs_and_rejection_waits() {
    use ahjo_mission_model::HumanVerdict;
    for verdict in ["fail", "pass"] {
        for dispatch_before in [false, true] {
            let root = Scratch::new();
            let solo = include_str!("parity/runs/missions/solo-lab.mission.toml");
            let other = solo
                .split("[[step]]")
                .nth(1)
                .unwrap()
                .split("[[end]]")
                .next()
                .unwrap()
                .replace("fmn_work", "fmn_other")
                .replace("seat_work", "seat_other");
            let mission = format!(
                "{}\n[[step]]{other}\n[[connection]]\nid = \"edge_other\"\nfrom = \"inp_hello:out\"\nto = \"fmn_other:port_in\"\n[[connection]]\nid = \"edge_other_done\"\nfrom = \"fmn_other:port_out\"\nto = \"end_done:in\"\n",
                gated_mission("kinds = [\"human\"]")
            );
            std::fs::write(root.0.join("missions/hello.mission.toml"), mission).unwrap();
            let control = BranchControl::default();
            control.hold("fmn_other");
            let c = Coordinator::open(
                &root.0,
                RunSettings::default(),
                Box::new(ControlledLab(control.clone())),
            )
            .unwrap();
            let id = c
                .start_mission(RunStart {
                    mission: "hello".into(),
                    input_card_id: "inp_hello".into(),
                    expected_rev: 1,
                    inputs: BTreeMap::from([("brief".into(), "hello".into())]),
                    ..Default::default()
                })
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(20);
            wait_for(&c, &id, "card_started", "fmn_other", deadline);
            let requested = wait_for(&c, &id, "human_input_requested", "gate_review", deadline);
            if dispatch_before {
                control.release("fmn_other", true, false);
                wait_for(&c, &id, "seat_dispatch", "fmn_other", deadline);
            }
            c.record_verdict(
                &id,
                "gate_review",
                HumanVerdict {
                    requested_seq: requested,
                    verdict: verdict.into(),
                    reason: if verdict == "fail" {
                        "needs work"
                    } else {
                        "yes"
                    }
                    .into(),
                    ..Default::default()
                },
            )
            .unwrap();
            let decision = wait_for(&c, &id, "gate_verdict", "gate_review", deadline);
            if !dispatch_before {
                control.release("fmn_other", true, false);
            }
            let dispatch = wait_for(&c, &id, "seat_dispatch", "fmn_other", deadline);
            assert_eq!(dispatch < decision, dispatch_before);
            control.release("fmn_other", false, true);
            let output = wait_for(&c, &id, "card_output", "fmn_other", deadline);
            assert!(decision < output);
            let terminal = wait_for(
                &c,
                &id,
                if verdict == "fail" {
                    "run_failed"
                } else {
                    "run_succeeded"
                },
                if verdict == "fail" {
                    "end_rejected"
                } else {
                    ""
                },
                deadline,
            );
            assert!(output < terminal);
            let input_start = wait_for(&c, &id, "card_started", "inp_hello", deadline);
            let input_output = wait_for(&c, &id, "card_output", "inp_hello", deadline);
            let work_start = wait_for(&c, &id, "card_started", "fmn_work", deadline);
            let work_output = wait_for(&c, &id, "card_output", "fmn_work", deadline);
            let gate_start = wait_for(&c, &id, "gate_evaluating", "gate_review", deadline);
            assert!(input_start < input_output && input_output < work_start);
            assert!(input_output < wait_for(&c, &id, "card_started", "fmn_other", deadline));
            assert!(work_output < gate_start && gate_start < requested && requested < decision);
            if verdict == "fail" {
                let lines =
                    std::fs::read_to_string(root.0.join("runs").join(format!("{id}.ndjson")))
                        .unwrap();
                let last: Value = serde_json::from_str(lines.lines().last().unwrap()).unwrap();
                assert_eq!(last["data"]["code"], "path_rejected");
                assert_eq!(last["data"]["reason"], "needs work");
                assert_eq!(last["actor"], "human:operator");
                let end_ids = last["data"]["end_ids"].as_array().unwrap();
                assert!(end_ids.contains(&"end_done".into()));
                assert!(end_ids.contains(&"end_rejected".into()));
            }
            let (expected, payload) = if verdict == "fail" {
                (
                    include_str!("parity/runs/archon-ledgers/rejected-end-on-one-branch.ndjson"),
                    include_str!(
                        "parity/runs/archon-ledgers/rejected-end-on-one-branch.payload.ndjson"
                    ),
                )
            } else {
                (
                    include_str!(
                        "parity/runs/archon-ledgers/human-approve-while-other-step-works.ndjson"
                    ),
                    include_str!(
                        "parity/runs/archon-ledgers/human-approve-while-other-step-works.payload.ndjson"
                    ),
                )
            };
            let expected: Vec<Value> = expected
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            run_support::assert_concurrent_ledger(&normalized_ledger(&root, &id), &expected);
            run_support::assert_concurrent_payload_parity(&root, &id, payload);
        }
    }
}

#[test]
fn a_newer_human_request_makes_the_old_sequence_stale() {
    let root = Scratch::new();
    let solo = include_str!("parity/runs/missions/solo-lab.mission.toml");
    let other = solo
        .split("[[step]]")
        .nth(1)
        .unwrap()
        .split("[[end]]")
        .next()
        .unwrap()
        .replace("fmn_work", "fmn_other")
        .replace("seat_work", "seat_other")
        .replace(
            "Return a short result",
            "archon-lab-work:400ms Return a short result",
        );
    let base = gated_mission("kinds = [\"human\"]").replace(
        "Return a short result",
        "archon-lab-work:50ms Return a short result",
    );
    let mission = format!(
        "{base}\n[[step]]{other}\n[[gate]]\nid = \"gate_check\"\ntitle = \"Check\"\nkinds = [\"code\"]\ncheck = \"output_contains\"\ncheck_version = \"1\"\ncheck_value = \"never present\"\n[[connection]]\nid = \"edge_other\"\nfrom = \"inp_hello:out\"\nto = \"fmn_other:port_in\"\n[[connection]]\nid = \"edge_check\"\nfrom = \"fmn_other:port_out\"\nto = \"gate_check:in\"\n[[connection]]\nid = \"edge_check_pass\"\nfrom = \"gate_check:pass\"\nto = \"end_done:in\"\n[[connection]]\nid = \"edge_check_fail\"\nfrom = \"gate_check:fail\"\nto = \"gate_review:in\"\n"
    );
    std::fs::write(root.0.join("missions/hello.mission.toml"), mission).unwrap();
    let c = Coordinator::open(&root.0, RunSettings::default(), Box::new(LabExecutor)).unwrap();
    let id = c
        .start_mission(RunStart {
            mission: "hello".into(),
            input_card_id: "inp_hello".into(),
            expected_rev: 1,
            inputs: BTreeMap::from([("brief".into(), "hello".into())]),
            ..Default::default()
        })
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let first = loop {
        let p = c.run(&id).unwrap();
        if let Some(gate) = p.waiting_gates.first() {
            break gate.requested_seq;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    };
    let second = loop {
        let p = c.run(&id).unwrap();
        if let Some(gate) = p.waiting_gates.first()
            && gate.requested_seq != first
        {
            break gate.requested_seq;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    };
    let stale = c
        .record_verdict(
            &id,
            "gate_review",
            ahjo_mission_model::HumanVerdict {
                requested_seq: first,
                verdict: "pass".into(),
                ..Default::default()
            },
        )
        .unwrap_err();
    assert_eq!(stale.code, "conflict");
    c.record_verdict(
        &id,
        "gate_review",
        ahjo_mission_model::HumanVerdict {
            requested_seq: second,
            verdict: "pass".into(),
            ..Default::default()
        },
    )
    .unwrap();
    loop {
        let p = c.run(&id).unwrap();
        if p.r#final {
            assert_eq!(p.status, "succeeded");
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    let expected: Vec<Value> = include_str!("parity/runs/archon-ledgers/human-stale-409.ndjson")
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(normalized_ledger(&root, &id), expected);
    run_support::assert_payload_parity(
        &root,
        &id,
        include_str!("parity/runs/archon-ledgers/human-stale-409.payload.ndjson"),
    );
}

#[test]
fn separate_human_gates_can_be_answered_in_either_order() {
    let root = Scratch::new();
    let solo = include_str!("parity/runs/missions/solo-lab.mission.toml");
    let other = solo
        .split("[[step]]")
        .nth(1)
        .unwrap()
        .split("[[end]]")
        .next()
        .unwrap()
        .replace("fmn_work", "fmn_other")
        .replace("seat_work", "seat_other");
    let mission = format!(
        "{}\n[[step]]{other}\n[[gate]]\nid = \"gate_other\"\ntitle = \"Other\"\nkinds = [\"human\"]\n\n[[connection]]\nid = \"edge_other\"\nfrom = \"inp_hello:out\"\nto = \"fmn_other:port_in\"\n[[connection]]\nid = \"edge_other_gate\"\nfrom = \"fmn_other:port_out\"\nto = \"gate_other:in\"\n[[connection]]\nid = \"edge_other_pass\"\nfrom = \"gate_other:pass\"\nto = \"end_done:in\"\n[[connection]]\nid = \"edge_other_fail\"\nfrom = \"gate_other:fail\"\nto = \"end_rejected:in\"\n",
        gated_mission("kinds = [\"human\"]")
    );
    std::fs::write(root.0.join("missions/hello.mission.toml"), mission).unwrap();
    let c = Coordinator::open(&root.0, RunSettings::default(), Box::new(LabExecutor)).unwrap();
    let id = c
        .start_mission(RunStart {
            mission: "hello".into(),
            input_card_id: "inp_hello".into(),
            expected_rev: 1,
            inputs: BTreeMap::from([("brief".into(), "hello".into())]),
            ..Default::default()
        })
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let gates = loop {
        let p = c.run(&id).unwrap();
        if p.waiting_gates.len() == 2 {
            break p.waiting_gates;
        }
        assert!(p.status != "blocked" && Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    };
    for gate in gates.iter().rev() {
        c.record_verdict(
            &id,
            &gate.gate_id,
            ahjo_mission_model::HumanVerdict {
                requested_seq: gate.requested_seq,
                verdict: "pass".into(),
                ..Default::default()
            },
        )
        .unwrap();
    }
    loop {
        let p = c.run(&id).unwrap();
        if p.r#final || p.status == "blocked" {
            assert_eq!(p.status, "succeeded");
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_blocked_run_accepts_another_gates_verdict_and_routes_it_on_resume() {
    let root = Scratch::new();
    let solo = include_str!("parity/runs/missions/solo-lab.mission.toml");
    let judge = solo
        .split("[[step]]")
        .nth(1)
        .unwrap()
        .split("[[end]]")
        .next()
        .unwrap()
        .replace("fmn_work", "fmn_judge")
        .replace("seat_work", "seat_judge");
    let other = solo
        .split("[[step]]")
        .nth(1)
        .unwrap()
        .split("[[end]]")
        .next()
        .unwrap()
        .replace("fmn_work", "fmn_other")
        .replace("seat_work", "seat_other")
        .replace(
            "Return a short result",
            "archon-lab-work:400ms Return a short result",
        );
    let mission = format!(
        "{}\n[[step]]{judge}\n[[step]]{other}\n[[gate]]\nid = \"gate_human\"\ntitle = \"Human\"\nkinds = [\"human\"]\n",
        gated_mission("kinds = [\"step\"]")
    ) + "\n[[connection]]\nid = \"edge_judge\"\nfrom = \"gate_review:judge\"\nto = \"fmn_judge:port_in\"\n[[connection]]\nid = \"edge_return\"\nfrom = \"fmn_judge:port_out\"\nto = \"gate_review:judge\"\n[[connection]]\nid = \"edge_other\"\nfrom = \"inp_hello:out\"\nto = \"fmn_other:port_in\"\n[[connection]]\nid = \"edge_human\"\nfrom = \"fmn_other:port_out\"\nto = \"gate_human:in\"\n[[connection]]\nid = \"edge_human_pass\"\nfrom = \"gate_human:pass\"\nto = \"end_done:in\"\n[[connection]]\nid = \"edge_human_fail\"\nfrom = \"gate_human:fail\"\nto = \"end_rejected:in\"\n";
    std::fs::write(root.0.join("missions/hello.mission.toml"), mission).unwrap();
    let c = Coordinator::open(&root.0, RunSettings::default(), Box::new(LabExecutor)).unwrap();
    let id = c
        .start_mission(RunStart {
            mission: "hello".into(),
            input_card_id: "inp_hello".into(),
            expected_rev: 1,
            inputs: BTreeMap::from([("brief".into(), "no verdict".into())]),
            ..Default::default()
        })
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let requested = loop {
        let p = c.run(&id).unwrap();
        if p.status == "blocked" {
            assert_eq!(p.block_code, "invalid_judge_result");
            break p
                .waiting_gates
                .iter()
                .find(|gate| gate.gate_id == "gate_human")
                .unwrap()
                .requested_seq;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    };
    let view = c
        .record_verdict(
            &id,
            "gate_human",
            ahjo_mission_model::HumanVerdict {
                requested_seq: requested,
                verdict: "pass".into(),
                reason: "approved while blocked".into(),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(view.status, "blocked");
    loop {
        match c.resume(
            &id,
            ahjo_mission_model::ResumeRequest {
                take_fail: true,
                actor: "human:operator".into(),
                ..Default::default()
            },
        ) {
            Ok(_) => break,
            Err(e) if e.code == "conflict" => {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(e) => panic!("resume: {e}"),
        }
    }
    loop {
        let p = c.run(&id).unwrap();
        if p.r#final || p.status == "blocked" {
            assert_eq!(p.status, "sent_back");
            assert!(p.events.iter().any(|event| event.kind == "gate_verdict"
                && event.gate_id == "gate_human"
                && event.verdict == "pass"));
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    let id = c
        .start_mission(RunStart {
            mission: "hello".into(),
            input_card_id: "inp_hello".into(),
            expected_rev: 1,
            inputs: BTreeMap::from([("brief".into(), "no verdict".into())]),
            ..Default::default()
        })
        .unwrap();
    let requested = loop {
        let p = c.run(&id).unwrap();
        if p.status == "blocked" {
            break p
                .waiting_gates
                .iter()
                .find(|gate| gate.gate_id == "gate_human")
                .unwrap()
                .requested_seq;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    };
    loop {
        match c.resume(
            &id,
            ahjo_mission_model::ResumeRequest {
                take_fail: true,
                actor: "human:operator".into(),
                ..Default::default()
            },
        ) {
            Ok(_) => break,
            Err(error) if error.code == "conflict" => {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("resume: {error}"),
        }
    }
    loop {
        let p = c.run(&id).unwrap();
        if p.status == "waiting_human" {
            assert!(!p.r#final);
            assert_eq!(p.waiting_gates[0].requested_seq, requested);
            break;
        }
        assert!(!p.r#final && p.status != "blocked" && Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    c.record_verdict(
        &id,
        "gate_human",
        ahjo_mission_model::HumanVerdict {
            requested_seq: requested,
            verdict: "pass".into(),
            ..Default::default()
        },
    )
    .unwrap();
    loop {
        let p = c.run(&id).unwrap();
        if p.r#final {
            assert_eq!(p.status, "sent_back");
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn reopening_retries_an_open_attempt_on_its_original_inputs() {
    let root = Scratch::new();
    let c = Coordinator::open(&root.0, RunSettings::default(), Box::new(LabExecutor)).unwrap();
    let id = c
        .start_mission(RunStart {
            mission: "hello".into(),
            input_card_id: "inp_hello".into(),
            expected_rev: 1,
            inputs: BTreeMap::from([("brief".into(), "original".into())]),
            ..Default::default()
        })
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !c.run(&id).unwrap().r#final {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    drop(c);
    let path = root.0.join("runs").join(format!("{id}.ndjson"));
    let text = std::fs::read_to_string(&path).unwrap();
    let events: Vec<Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let start = events
        .iter()
        .position(|event| event["type"] == "card_started" && event["card_id"] == "fmn_work")
        .unwrap();
    let kept = text.lines().take(start + 1).collect::<Vec<_>>().join("\n") + "\n";
    std::fs::write(&path, kept).unwrap();
    let c = loop {
        match Coordinator::open(&root.0, RunSettings::default(), Box::new(LabExecutor)) {
            Ok(c) => break c,
            Err(_) => {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    };
    loop {
        let p = c.run(&id).unwrap();
        if p.r#final || p.status == "blocked" {
            assert_eq!(p.status, "succeeded");
            let retry = p
                .events
                .iter()
                .find(|event| {
                    event.kind == "card_started"
                        && event.card_id == "fmn_work"
                        && event.attempt == 2
                })
                .unwrap();
            assert_eq!(retry.input_refs.len(), 1);
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn replay_blocks_instead_of_succeeding_when_an_output_payload_is_missing() {
    let root = Scratch::new();
    let c = Coordinator::open(&root.0, RunSettings::default(), Box::new(LabExecutor)).unwrap();
    let id = c
        .start_mission(RunStart {
            mission: "hello".into(),
            input_card_id: "inp_hello".into(),
            expected_rev: 1,
            inputs: BTreeMap::from([("brief".into(), "hello".into())]),
            ..Default::default()
        })
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !c.run(&id).unwrap().r#final {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    drop(c);
    let path = root.0.join("runs").join(format!("{id}.ndjson"));
    let mut events: Vec<Value> = std::fs::read_to_string(&path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let output = events
        .iter()
        .position(|event| event["type"] == "card_output" && event["card_id"] == "fmn_work")
        .unwrap();
    events.truncate(output + 1);
    events[output]["data"]["outputs"] = serde_json::json!({});
    std::fs::write(
        &path,
        events
            .iter()
            .map(|event| serde_json::to_string(event).unwrap())
            .collect::<Vec<_>>()
            .join("\n")
            + "\n",
    )
    .unwrap();
    let c = loop {
        match Coordinator::open(&root.0, RunSettings::default(), Box::new(LabExecutor)) {
            Ok(c) => break c,
            Err(_) => {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    };
    loop {
        let p = c.run(&id).unwrap();
        if p.r#final || p.status == "blocked" && p.block_code != "coordinator_interrupted" {
            assert_eq!(p.status, "succeeded");
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    // Replay refused the payload-less output: the restart blocked the run
    // (and resumed it on its own) instead of succeeding from the replay.
    let events = c.run(&id).unwrap().events;
    let block = events.iter().position(|e| e.kind == "run_blocked").unwrap();
    assert!(events[block..].iter().any(|e| e.kind == "run_resumed"));
}

struct FailingLab;
impl ahjo_mission_engine::Executor for FailingLab {
    fn execute_step(
        &self,
        request: ahjo_mission_engine::StepExecution,
        events: ahjo_mission_engine::ExecutionSink,
    ) -> Result<ahjo_mission_engine::StepResult, ahjo_mission_engine::Error> {
        (|| -> Result<ahjo_mission_engine::StepResult, String> {
            let fail = request.step.id == "fmn_work" && request.attempt == 1;
            let result = ahjo_mission_engine::Executor::execute_step(&LabExecutor, request, events)
                .map_err(|error| error.message)?;
            if fail {
                Err("deliberate step error".into())
            } else {
                Ok(result)
            }
        })()
        .map_err(|message: String| ahjo_mission_engine::Error::new("executor_error", message))
    }
}

#[test]
fn a_step_error_is_recorded_while_other_work_finishes_and_resume_retries_it() {
    let root = Scratch::new();
    let solo = include_str!("parity/runs/missions/solo-lab.mission.toml");
    let other = solo
        .split("[[step]]")
        .nth(1)
        .unwrap()
        .split("[[end]]")
        .next()
        .unwrap()
        .replace("fmn_work", "fmn_other")
        .replace("seat_work", "seat_other")
        .replace(
            "Return a short result",
            "archon-lab-work:400ms Return a short result",
        );
    let mission = solo.replace(
        "Return a short result",
        "archon-lab-work:50ms Return a short result",
    ) + &format!(
        "\n[[step]]{other}\n[[connection]]\nid = \"edge_other\"\nfrom = \"inp_hello:out\"\nto = \"fmn_other:port_in\"\n[[connection]]\nid = \"edge_other_end\"\nfrom = \"fmn_other:port_out\"\nto = \"end_done:in\"\n"
    );
    std::fs::write(root.0.join("missions/hello.mission.toml"), mission).unwrap();
    let c = Coordinator::open(&root.0, RunSettings::default(), Box::new(FailingLab)).unwrap();
    let id = c
        .start_mission(RunStart {
            mission: "hello".into(),
            input_card_id: "inp_hello".into(),
            expected_rev: 1,
            inputs: BTreeMap::from([("brief".into(), "hello".into())]),
            ..Default::default()
        })
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let blocked = loop {
        let p = c.run(&id).unwrap();
        if p.status == "blocked" {
            break p;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    };
    assert!(blocked.resume_allowed);
    let failure = blocked
        .events
        .iter()
        .find(|event| event.kind == "error" && event.card_id == "fmn_work")
        .unwrap()
        .seq;
    let other = blocked
        .events
        .iter()
        .find(|event| event.kind == "card_output" && event.card_id == "fmn_other")
        .unwrap()
        .seq;
    assert!(failure < other);
    loop {
        match c.resume(&id, Default::default()) {
            Ok(_) => break,
            Err(error) if error.code == "conflict" => {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("resume: {error}"),
        }
    }
    loop {
        let p = c.run(&id).unwrap();
        if p.r#final || p.status == "blocked" {
            assert_eq!(p.status, "succeeded");
            assert_eq!(
                p.events
                    .iter()
                    .filter(|event| event.kind == "card_started" && event.card_id == "fmn_other")
                    .count(),
                1
            );
            assert!(p.events.iter().any(|event| event.kind == "card_started"
                && event.card_id == "fmn_work"
                && event.attempt == 2));
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn recovery_drains_started_work_without_retrying_a_recorded_error() {
    let root = Scratch::new();
    let solo = include_str!("parity/runs/missions/solo-lab.mission.toml");
    let other = solo
        .split("[[step]]")
        .nth(1)
        .unwrap()
        .split("[[end]]")
        .next()
        .unwrap()
        .replace("fmn_work", "fmn_other")
        .replace("seat_work", "seat_other")
        .replace(
            "Return a short result",
            "archon-lab-work:300ms Return a short result",
        );
    let mission = solo.replace(
        "Return a short result",
        "archon-lab-work:50ms Return a short result",
    ) + &format!(
        "\n[[step]]{other}\n[[connection]]\nid = \"edge_other\"\nfrom = \"inp_hello:out\"\nto = \"fmn_other:port_in\"\n[[connection]]\nid = \"edge_other_end\"\nfrom = \"fmn_other:port_out\"\nto = \"end_done:in\"\n"
    );
    std::fs::write(root.0.join("missions/hello.mission.toml"), mission).unwrap();
    let c = Coordinator::open(&root.0, RunSettings::default(), Box::new(FailingLab)).unwrap();
    let id = c
        .start_mission(RunStart {
            mission: "hello".into(),
            input_card_id: "inp_hello".into(),
            expected_rev: 1,
            inputs: BTreeMap::from([("brief".into(), "hello".into())]),
            ..Default::default()
        })
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    while c.run(&id).unwrap().status != "blocked" {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    drop(c);
    let path = root.0.join("runs").join(format!("{id}.ndjson"));
    let text = std::fs::read_to_string(&path).unwrap();
    let events: Vec<Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let failure = events
        .iter()
        .position(|e| e["type"] == "error" && e["card_id"] == "fmn_work")
        .unwrap();
    assert!(
        events[..failure]
            .iter()
            .any(|e| e["type"] == "card_started" && e["card_id"] == "fmn_other")
    );
    std::fs::write(
        &path,
        text.lines()
            .take(failure + 1)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n",
    )
    .unwrap();
    let c = loop {
        match Coordinator::open(&root.0, RunSettings::default(), Box::new(LabExecutor)) {
            Ok(c) => break c,
            Err(_) => {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    };
    loop {
        let p = c.run(&id).unwrap();
        if p.r#final {
            assert!(
                p.events
                    .iter()
                    .any(|e| e.kind == "card_output" && e.card_id == "fmn_other")
            );
            assert_eq!(
                p.events
                    .iter()
                    .filter(|e| e.kind == "card_started" && e.card_id == "fmn_work")
                    .count(),
                2,
                "status {} events {:?}",
                p.status,
                p.events
                    .iter()
                    .map(|e| (&e.kind, &e.card_id, e.attempt))
                    .collect::<Vec<_>>()
            );
            break;
        }
        assert!(!p.r#final && Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn human_response_is_delivered_with_its_original_input_like_archon() {
    let root = Scratch::new();
    let solo = include_str!("parity/runs/missions/solo-lab.mission.toml");
    let after = solo
        .split("[[step]]")
        .nth(1)
        .unwrap()
        .split("[[end]]")
        .next()
        .unwrap()
        .replace("fmn_work", "fmn_after")
        .replace("seat_work", "seat_after");
    let mission = gated_mission("kinds = [\"human\"]")
        .replace("to = \"end_done:in\"", "to = \"fmn_after:port_in\"")
        + &format!(
            "\n[[step]]{after}\n[[connection]]\nid = \"edge_after\"\nfrom = \"fmn_after:port_out\"\nto = \"end_done:in\"\n"
        );
    std::fs::write(root.0.join("missions/hello.mission.toml"), mission).unwrap();
    let c = Coordinator::open(&root.0, RunSettings::default(), Box::new(LabExecutor)).unwrap();
    let id = c
        .start_mission(RunStart {
            mission: "hello".into(),
            input_card_id: "inp_hello".into(),
            expected_rev: 1,
            inputs: BTreeMap::from([("brief".into(), "hello".into())]),
            ..Default::default()
        })
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let seq = loop {
        let p = c.run(&id).unwrap();
        if let Some(gate) = p.waiting_gates.first() {
            break gate.requested_seq;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    };
    c.record_verdict(
        &id,
        "gate_review",
        ahjo_mission_model::HumanVerdict {
            requested_seq: seq,
            verdict: "pass".into(),
            reason: "yes".into(),
            ..Default::default()
        },
    )
    .unwrap();
    loop {
        let p = c.run(&id).unwrap();
        if p.r#final || p.status == "blocked" {
            assert_eq!(p.status, "succeeded");
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    let expected: Vec<Value> = include_str!("parity/runs/archon-ledgers/human-response.ndjson")
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(normalized_ledger(&root, &id), expected);
    run_support::assert_payload_parity(
        &root,
        &id,
        include_str!("parity/runs/archon-ledgers/human-response.payload.ndjson"),
    );
}
