//! End seats detached at shutdown only by their recorded backend identity.
use crate::{Coordinator, LeftSeat, RunEvent};
use serde_json::json;
use std::{collections::BTreeMap, io};

pub(crate) fn end(c: &Coordinator, id: &str, node: Option<&str>, cause: &str) -> io::Result<()> {
    let events = c.ledger(id)?.read().map_err(crate::error::to_io)?;
    let mut created = BTreeMap::<(String, String, String), RunEvent>::new();
    let mut left = BTreeMap::<(String, String, String), RunEvent>::new();
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
            created.insert(key, event);
        } else if event.kind == "seat_cleanup" {
            if event.data.get("outcome").and_then(|v| v.as_str()) == Some("left_shutdown") {
                if let Some(seat) = created.get(&key) {
                    left.insert(key, seat.clone());
                }
            } else if matches!(
                event.data.get("outcome").and_then(|v| v.as_str()),
                Some("ended" | "gone" | "kept")
            ) {
                left.remove(&key);
            }
        }
    }
    let mut failures = Vec::new();
    for (_, seat) in left
        .into_iter()
        .filter(|(_, seat)| node.is_none_or(|node| seat.card_id == node))
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
                "detached seat has no immutable session id",
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
        let mut event = crate::run_worker::event(
            "seat_cleanup",
            &seat.card_id,
            seat.attempt,
            json!({"session_name":name,"outcome":outcome,"cause":cause,"detail":detail}),
        );
        event.seat_id = seat.seat_id;
        c.append(id, event)?;
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
