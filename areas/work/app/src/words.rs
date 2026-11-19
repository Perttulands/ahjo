//! The tracker's words as a person reads them: what a state, a kind and a
//! priority are, said so that someone who has never used Beads or GitHub
//! issues knows at first sight what an item is and how much it matters.

use crate::store::work::{Row, Status};

/// A state in a word, as the facets and the item say it.
pub(crate) fn state(s: Status) -> &'static str {
    match s {
        Status::Open => "To do",
        Status::InProgress => "In progress",
        Status::Blocked => "Blocked",
        Status::Deferred => "On hold",
        Status::Closed => "Done",
    }
}

/// What a state means, for its tooltip.
pub(crate) fn state_means(s: Status) -> &'static str {
    match s {
        Status::Open => "Ready for someone to pick up",
        Status::InProgress => "Someone is working on it now",
        Status::Blocked => "Waits on other work before it can move",
        Status::Deferred => "Put off on purpose, for later",
        Status::Closed => "Finished, or closed without doing",
    }
}

/// A row's own state: the tracker's word where it says more than
/// done (GitHub's merged).
pub(crate) fn row_state(r: &Row) -> String {
    match (r.status, r.raw_status.as_str()) {
        (Status::Closed, "merged") => "Merged".into(),
        (s, _) => state(s).into(),
    }
}

/// A kind of work in a word, and what it is. A kind the tracker names
/// that is not one of these is said as it is written.
pub(crate) fn kind(kind: &str) -> (String, Option<&'static str>) {
    let means = match kind {
        "epic" => Some("A larger goal, made of the items under it"),
        "bug" => Some("Something that does not work as it should"),
        "feature" => Some("Something new to build"),
        "task" => Some("A piece of work to do"),
        "chore" => Some("Upkeep: tidying, updates, housekeeping"),
        "pull" => Some("A proposed change to the code, waiting to be merged"),
        "issue" => Some("A piece of work or a problem reported"),
        _ => None,
    };
    let word = match kind {
        "pull" => "pull request".to_string(),
        other => other.replace(['_', '-'], " "),
    };
    (word, means)
}

/// Whether the list says a row's kind: every kind but a tracker's one
/// plain kind (a GitHub issue), which would say the same on every row.
pub(crate) fn kind_in_list(kind: &str) -> bool {
    kind != "issue"
}

/// A priority in a word, and what it asks.
pub(crate) fn priority(p: u8) -> (&'static str, &'static str) {
    match p {
        0 => ("urgent", "P0, urgent: before anything else"),
        1 => ("high", "P1, high: next, after urgent work"),
        2 => ("normal", "P2, normal"),
        3 => ("low", "P3, low: when there is time"),
        _ => ("someday", "P4, someday: kept so it is not forgotten"),
    }
}

/// Whether the list says a priority: only what puts an item ahead of the
/// rest.
pub(crate) fn priority_in_list(p: u8) -> bool {
    p <= 1
}

/// How far a group of work has come: `3 of 7 done`.
pub(crate) fn progress(done: usize, all: usize) -> String {
    if done == all {
        format!("all {all} done")
    } else {
        format!("{done} of {all} done")
    }
}

/// The capital of a word that starts a line.
pub(crate) fn capital(word: &str) -> String {
    let mut c = word.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_trackers_words_read_as_plain_words() {
        assert_eq!(state(Status::Deferred), "On hold");
        assert_eq!(state(Status::Open), "To do");
        assert_eq!(kind("epic").0, "epic");
        assert!(kind("epic").1.unwrap().contains("items under it"));
        assert_eq!(kind("pull").0, "pull request");
        assert_eq!(kind("merge_request"), ("merge request".into(), None));
        assert_eq!(priority(0).0, "urgent");
        assert_eq!(priority(9).0, "someday");
        assert!(priority_in_list(1) && !priority_in_list(2));
        assert!(!kind_in_list("issue") && kind_in_list("bug"));
        assert_eq!(progress(3, 7), "3 of 7 done");
        assert_eq!(progress(2, 2), "all 2 done");
        assert_eq!(capital("bug"), "Bug");
    }
}
