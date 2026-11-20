//! Each type reads back what it writes, so the node and its clients can
//! share one declaration. Responses omit empty fields when written; requests
//! are written whole.

use ahjo_mission_model::{
    Code, Escalation, GateRequest, HumanPick, HumanVerdict, MissionListing, Report, ResumeMode,
    ResumeRequest, RunAttempts, RunProjection, RunStart, RunSummary,
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

fn reads_back<T: Serialize + DeserializeOwned>(written: Value) -> T {
    let read: T = serde_json::from_value(written.clone()).unwrap();
    assert_eq!(serde_json::to_value(&read).unwrap(), written);
    read
}

#[test]
fn responses_read_back_with_their_empty_fields_left_out() {
    let run = reads_back::<RunProjection>(json!({
        "run_id": "run_a", "status": "waiting_human", "final": false,
        "resume_allowed": false, "epoch": 1, "mission_slug": "hello",
        "mission_id": "msn_a", "mission_rev": 2, "input_card_id": "inp_a",
        "work": "", "cwd": "/work", "repository": "", "base": "",
        "context_paths": [], "inputs": [{"name": "brief", "kind": "text", "value": "hi"}],
        "outputs": [{"name": "plan", "kind": "text", "value": "done", "node_id": "fmn_a",
                     "port_id": "out", "attempt": 1}],
        "event_count": 2, "started_by": "human:operator",
        "started_at": "2026-11-11T00:00:00Z", "updated_at": "2026-11-11T00:00:01Z",
        "waiting_gates": [{"gate_id": "gte_a", "requested_seq": 2,
                          "requested_at": "2026-11-11T00:00:01Z"}],
        "events": [
            {"seq": 1, "type": "run_started"},
            {"seq": 2, "type": "human_input_requested", "gate_id": "gte_a", "attempt": 1,
             "input_refs": [{"edge_id": "edg_a", "from_node_id": "fmn_a", "from_port_id": "out",
                            "to_port_id": "in", "origin": null}]}
        ]
    }));
    assert_eq!(run.events[1].gate_id, "gte_a");

    reads_back::<Vec<RunSummary>>(json!([{
        "run_id": "run_a", "number": 1, "status": "blocked", "final": false,
        "mission_slug": "hello", "mission_id": "msn_a", "mission_rev": 2,
        "input_card_id": "inp_a", "work": "", "event_count": 3, "resume_allowed": true,
        "block_code": "limit_spent", "started_by": "human:operator",
        "started_at": "2026-11-11T00:00:00Z", "updated_at": "2026-11-11T00:00:01Z",
        "input_excerpt": "hi", "outputs": [],
        "sessions": [{"session_name": "ahjo-a", "node_id": "fmn_a", "slot_id": "w1"}]
    }]));
    reads_back::<RunAttempts>(json!({"run_id": "run_a", "steps": [{
        "node_id": "fmn_a", "title": "Plan",
        "outputs": [{"id": "out", "label": "Plan", "kind": "text"}],
        "attempts": [{
            "attempt": 1, "status": "done", "started_seq": 3,
            "started_at": "2026-11-11T00:00:00Z", "output_seq": 5,
            "outputs": [{"port_id": "out", "kind": "text", "text": {"text": "plan", "bytes": 4}}],
            "verdicts": [{"gate_id": "gte_a", "evaluation_seq": 6, "pending": true,
                          "reason": {"text": "", "bytes": 0}, "evidence": []}]
        }]
    }]}));
    reads_back::<GateRequest>(json!({
        "gate_id": "gte_a", "requested_seq": 2, "criterion": "Is it right?",
        "input": {"text": "plan", "truncated": false}
    }));
    reads_back::<Vec<Escalation>>(json!([{
        "run_id": "run_a", "seq": 4, "severity": "high", "reason": "stuck",
        "source": "agent", "trigger": "sentinel", "blocks": true
    }]));
    reads_back::<Vec<MissionListing>>(json!([{"slug": "broken", "broken": "dangling link"}]));
}

#[test]
fn findings_read_back_by_their_codes() {
    let report = reads_back::<Report>(json!({
        "errors": [
            {"code": "missing_input", "node_id": "inp_a", "message": "brief is required"},
            {"code": "missing_file", "node_id": "fmn_a", "message": "no such file",
             "path": "/work/brief.md"}
        ],
        "warnings": []
    }));
    assert_eq!(report.errors[1].code, Code::MissingFile);
}

#[test]
fn requests_read_back_whole() {
    let start = reads_back::<RunStart>(json!({
        "mission": "hello", "input_card_id": "inp_a", "formation_id": "",
        "expected_rev": 2, "expected_etag": "abc", "cwd": "/work", "repository": "",
        "base": "", "context_paths": [], "work": "", "actor": "human:operator",
        "inputs": {"brief": "hi"}, "request_id": "req_1"
    }));
    assert_eq!(start.known_devices, None);
    let resume = reads_back::<ResumeRequest>(json!({
        "mode": "redispatch", "take_fail": false, "grant": true, "pick_best": false,
        "reason": "one more round", "actor": "human:operator"
    }));
    assert_eq!(resume.mode, ResumeMode::Redispatch);
    let verdict = reads_back::<HumanVerdict>(json!({
        "requested_seq": 2, "verdict": "pass", "reason": "fine", "relayed_by": "",
        "actor": "human:operator"
    }));
    assert_eq!(verdict.reason, "fine");
    reads_back::<HumanPick>(json!({
        "requested_seq": 2, "attempt": 1, "reason": "best", "relayed_by": "w1",
        "actor": "human:operator"
    }));
}
