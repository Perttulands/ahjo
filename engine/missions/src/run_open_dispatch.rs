//! Dispatches whose completion has not been recorded, in dispatch order.
use crate::RunEvent;

pub(crate) fn open(events: &[RunEvent]) -> Vec<RunEvent> {
    let mut dispatches: Vec<RunEvent> = Vec::new();
    for event in events {
        let id = event
            .data
            .get("dispatch_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if id.is_empty() {
            continue;
        }
        match event.kind.as_str() {
            "seat_dispatch" => dispatches.push(event.clone()),
            "seat_result" => dispatches
                .retain(|d| d.data.get("dispatch_id").and_then(|v| v.as_str()) != Some(id)),
            _ => {}
        }
    }
    dispatches
}

/// Open dispatches and completed dispatches whose output was never retained.
pub(crate) fn unresolved(events: &[RunEvent]) -> Vec<RunEvent> {
    let mut dispatches = open(events);
    for dispatch in events.iter().filter(|event| event.kind == "seat_dispatch") {
        let id = dispatch.data.get("dispatch_id").and_then(|v| v.as_str());
        let completed = events.iter().any(|event| {
            event.kind == "seat_result"
                && event.data.get("dispatch_id").and_then(|v| v.as_str()) == id
                && event.data.get("status").and_then(|v| v.as_str()) == Some("ok")
        });
        let abandoned = events.iter().any(|event| {
            event.kind == "seat_result"
                && event.data.get("dispatch_id").and_then(|v| v.as_str()) == id
                && event.data.get("status").and_then(|v| v.as_str()) == Some("abandoned")
        });
        let retained = events.iter().any(|event| {
            matches!(
                event.kind.as_str(),
                "card_output" | "card_output_pending" | "output_capture_pending"
            ) && event.card_id == dispatch.card_id
                && event.attempt == dispatch.attempt
        });
        if id.is_some()
            && completed
            && !abandoned
            && !retained
            && !dispatches.iter().any(|event| event.seq == dispatch.seq)
        {
            dispatches.push(dispatch.clone());
        }
    }
    dispatches.sort_by_key(|event| event.seq);
    dispatches
}

/// A completed seat followed by a recorded step error is not evidence of a
/// successful step. Its dispatch must be abandoned before a retry.
pub(crate) fn failed_completion(events: &[RunEvent], dispatch: &RunEvent) -> bool {
    let id = dispatch.data.get("dispatch_id").and_then(|v| v.as_str());
    events.iter().any(|event| {
        event.kind == "seat_result"
            && event.data.get("dispatch_id").and_then(|v| v.as_str()) == id
            && event.data.get("status").and_then(|v| v.as_str()) == Some("ok")
    }) && events.iter().any(|event| {
        event.kind == "error"
            && event.card_id == dispatch.card_id
            && event.seq > dispatch.seq
            && !events.iter().any(|later| {
                later.kind == "card_started"
                    && later.card_id == dispatch.card_id
                    && later.seq > dispatch.seq
                    && later.seq < event.seq
            })
    })
}
