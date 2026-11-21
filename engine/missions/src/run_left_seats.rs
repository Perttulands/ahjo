//! Finalize run-owned or detached seats by their recorded backend identity.
use crate::{Coordinator, LeftSeat, RunEvent};
use serde_json::json;
use std::{collections::BTreeMap, io};

pub(crate) fn keep_sessions(events: &[RunEvent]) -> bool {
    events
        .first()
        .and_then(|e| e.data.get("keep_sessions"))
        .and_then(serde_json::Value::as_bool)
        == Some(true)
}

fn run_owned(seat: &RunEvent) -> bool {
    seat.data
        .get("run_owned")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
}

fn identity(event: &RunEvent) -> String {
    json!([
        event.data.get("node"),
        event.data.get("session_id"),
        event.data.get("generation")
    ])
    .to_string()
}

pub(crate) fn end(c: &Coordinator, id: &str, node: Option<&str>, cause: &str) -> io::Result<()> {
    let events = c.ledger(id)?.read().map_err(crate::error::to_io)?;
    let finalized = events.last().is_some_and(|event| {
        matches!(
            event.kind.as_str(),
            "run_canceled" | "run_failed" | "run_picked" | "run_succeeded"
        )
    });
    let mut created = BTreeMap::<(String, String, String), RunEvent>::new();
    let mut left = BTreeMap::new();
    for event in events {
        let name = event
            .data
            .get("session_name")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if name.is_empty() {
            continue;
        }
        let key = (
            event.card_id.clone(),
            event.seat_id.clone(),
            name.to_owned(),
        );
        if event.kind == "seat_created" {
            if run_owned(&event) {
                left.insert((key.clone(), identity(&event)), event.clone());
            }
            created.insert(key, event);
        } else if event.kind == "seat_cleanup" {
            let Some(seat) = created.get(&key) else {
                continue;
            };
            match event.data.get("outcome").and_then(|v| v.as_str()) {
                Some("left_shutdown") => {
                    left.insert((key, identity(seat)), seat.clone());
                }
                Some("ended" | "gone" | "kept") => {
                    let recorded = if event.data.contains_key("session_id") {
                        &event
                    } else {
                        seat
                    };
                    left.remove(&(key, identity(recorded)));
                }
                _ => {}
            }
        }
    }
    let mut failures = Vec::new();
    for (_, seat) in left
        .into_iter()
        .filter(|(_, seat)| node.is_none_or(|node| seat.card_id == node))
        .filter(|(_, seat)| cause == "run_final" || !run_owned(seat))
    {
        let name = seat
            .data
            .get("session_name")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let result = if seat
            .data
            .get("session_id")
            .and_then(|v| v.as_str())
            .is_none_or(str::is_empty)
        {
            Err(crate::Error::new(
                "invalid_seat",
                "seat has no immutable session id",
            ))
        } else {
            c.inner.executor.end_left_seat(LeftSeat {
                run_id: id.into(),
                step_id: seat.card_id.clone(),
                seat_id: seat.seat_id.clone(),
                identity: seat.data.clone(),
                cause: cause.into(),
            })
        };
        let (outcome, detail) = match result {
            Ok(outcome) => (outcome, String::new()),
            Err(error) => ("left_cleanup_failed".into(), error.to_string()),
        };
        if !finalized {
            let mut event = crate::run_worker::event(
                "seat_cleanup",
                &seat.card_id,
                seat.attempt,
                json!({
                    "session_name":name,
                    "session_id":seat.data.get("session_id"),
                    "node":seat.data.get("node"),
                    "generation":seat.data.get("generation"),
                    "outcome":outcome,
                    "cause":cause,
                    "detail":detail,
                }),
            );
            event.seat_id = seat.seat_id;
            c.append(id, event)?;
        }
        if !["ended", "gone", "kept"].contains(&outcome.as_str()) {
            failures.push(format!("seat cleanup {outcome}: {detail}"));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "run_finalization_interrupted: {}",
            failures.join("; ")
        )))
    }
}
