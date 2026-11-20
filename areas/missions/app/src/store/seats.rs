//! Owned terminal seats reconstructed from a run's public lifecycle events.
use ahjo_missions_contract::RunProjection;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seat {
    pub run: String,
    pub step: String,
    pub seat: String,
    pub seq: u64,
    pub node: String,
    pub session: String,
    pub name: String,
    pub state: State,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Live,
    Ended,
    Unavailable,
}

/// A cleanup can only close the named seat in its own step and seat. Never
/// use the coordinator's node as a substitute for a missing seat identity.
pub fn seats(run: &RunProjection) -> Vec<Seat> {
    let mut events: Vec<_> = run.events.iter().collect();
    events.sort_by_key(|event| event.seq);
    let mut seats: Vec<Seat> = Vec::new();
    for event in events {
        match event.kind.as_str() {
            "seat_created"
                if !event.card_id.is_empty()
                    && !event.seat_node.is_empty()
                    && !event.session_id.is_empty()
                    && !event.session_name.is_empty() =>
            {
                if !seats.iter().any(|seat| {
                    seat.node == event.seat_node
                        && seat.session == event.session_id
                        && seat.name == event.session_name
                }) {
                    seats.push(Seat {
                        run: run.run_id.clone(),
                        step: event.card_id.clone(),
                        seat: event.seat_id.clone(),
                        seq: event.seq,
                        node: event.seat_node.clone(),
                        session: event.session_id.clone(),
                        name: event.session_name.clone(),
                        state: State::Live,
                    });
                }
            }
            "seat_cleanup" => {
                if let Some(seat) = seats.iter_mut().rev().find(|seat| {
                    seat.step == event.card_id
                        && seat.seat == event.seat_id
                        && !event.session_name.is_empty()
                        && seat.name == event.session_name
                }) {
                    seat.state = match event.outcome.as_str() {
                        "ended" | "gone" => State::Ended,
                        // A stopping coordinator leaves the seat running for
                        // the run to reattach, as the engine counts it.
                        "left_shutdown" => State::Live,
                        _ => State::Unavailable,
                    };
                }
            }
            _ => (),
        }
    }
    // A final run without a cleanup record cannot prove its seat still exists.
    if run.r#final {
        for seat in &mut seats {
            if seat.state == State::Live {
                seat.state = State::Unavailable;
            }
        }
    }
    seats
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn lifecycle_keeps_each_seat_identity_and_reports_cleanup() {
        let run: RunProjection = serde_json::from_value(json!({"run_id":"run_1","status":"working","final":false,"resume_allowed":false,"epoch":1,"mission_slug":"m","mission_id":"m","mission_rev":1,"input_card_id":"i","work":"","cwd":"","repository":"","base":"","context_paths":[],"inputs":[],"outputs":[],"event_count":3,"started_by":"","started_at":"","updated_at":"","waiting_gates":[],"events":[{"seq":1,"type":"seat_created","card_id":"work","seat_id":"a","seat_node":"n1","session_id":"$1","session_name":"same"},{"seq":2,"type":"seat_created","card_id":"work","seat_id":"b","seat_node":"n2","session_id":"$2","session_name":"same"},{"seq":3,"type":"seat_cleanup","card_id":"work","seat_id":"a","session_name":"same","outcome":"ended"}]})).unwrap();
        let seats = seats(&run);
        assert_eq!(seats.len(), 2);
        assert_eq!(
            (seats[0].state, seats[1].state),
            (State::Ended, State::Live)
        );
        assert_eq!(
            (&seats[1].node, &seats[1].session),
            (&"n2".into(), &"$2".into())
        );
    }
    #[test]
    fn a_seat_left_running_by_a_coordinator_shutdown_can_still_be_opened() {
        let run: RunProjection = serde_json::from_value(json!({"run_id":"run_1","status":"blocked","final":false,"resume_allowed":true,"epoch":1,"mission_slug":"m","mission_id":"m","mission_rev":1,"input_card_id":"i","work":"","cwd":"","repository":"","base":"","context_paths":[],"inputs":[],"outputs":[],"event_count":2,"started_by":"","started_at":"","updated_at":"","waiting_gates":[],"events":[{"seq":1,"type":"seat_created","card_id":"work","seat_id":"a","seat_node":"n1","session_id":"$1","session_name":"seat"},{"seq":2,"type":"seat_cleanup","card_id":"work","seat_id":"a","session_name":"seat","outcome":"left_shutdown"}]})).unwrap();
        assert_eq!(seats(&run)[0].state, State::Live);
    }
}
