//! Names for the tmux sessions associated with mission seats.
//!
//! A session component comes from the step title and, for a team, the seat
//! label. The mission prefix groups its sessions in the session list.
//!
//! A run reuses a live session prepared for that seat. If a different session
//! already has its name, the run opens one beside it. A step set to start
//! fresh has a session per attempt, which ends with that attempt.

use crate::{Mission, Seat, Step};

/// The longest a prefix or session component grows.
const MOST: usize = 24;

/// Lower-case letters and digits of `text`, words joined by `join`, at most
/// [`MOST`] characters and never ending in `join`.
fn words(text: &str, join: &str) -> String {
    let mut out = String::new();
    for word in text
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
    {
        let word = word.to_ascii_lowercase();
        let extra = if out.is_empty() { 0 } else { join.len() };
        if out.len() + extra + word.len() > MOST {
            if out.is_empty() {
                out.push_str(&word[..MOST]);
            }
            break;
        }
        if !out.is_empty() {
            out.push_str(join);
        }
        out.push_str(&word);
    }
    out
}

/// A prefix as a mission may set it: letters and digits only, since the
/// session list groups a name by what comes before its first `-`.
pub fn valid_prefix(prefix: &str) -> bool {
    !prefix.is_empty() && prefix.len() <= MOST && prefix.bytes().all(|b| b.is_ascii_alphanumeric())
}

impl Mission {
    /// The prefix its sessions are named under: the one it sets, else its
    /// slug's letters and digits.
    pub fn session_prefix(&self) -> String {
        match self.prefix.trim() {
            "" => match words(&self.slug, "") {
                prefix if prefix.is_empty() => "mission".into(),
                prefix => prefix,
            },
            prefix => prefix.to_ascii_lowercase(),
        }
    }
}

impl Step {
    /// The session name component from the step title and, for a team, seat label.
    pub fn session_part(&self, seat: &Seat) -> String {
        let title = if self.title.trim().is_empty() {
            &self.id
        } else {
            &self.title
        };
        let label = if seat.label.trim().is_empty() {
            &seat.id
        } else {
            &seat.label
        };
        let name = if self.seats.len() > 1 {
            words(&format!("{title} {label}"), "-")
        } else {
            words(title, "-")
        };
        if name.is_empty() {
            "agent".into()
        } else {
            name
        }
    }
}

/// The session for a mission seat.
pub fn seat_session(prefix: &str, part: &str) -> String {
    format!("{prefix}-{part}")
}

/// The session a fresh attempt works in, its own for that attempt.
pub fn fresh_session(prefix: &str, part: &str, run: u64, attempt: i64) -> String {
    format!("{prefix}-{part}-{run}-{attempt}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(title: &str, seats: &[(&str, &str)]) -> Step {
        Step {
            id: "fmn_a".into(),
            title: title.into(),
            seats: seats
                .iter()
                .map(|(label, agent)| Seat {
                    id: format!("s-{label}"),
                    label: (*label).into(),
                    agent: (*agent).into(),
                    ..Seat::default()
                })
                .collect(),
            ..Step::default()
        }
    }

    #[test]
    fn sessions_group_under_the_mission_prefix() {
        let mission = Mission {
            slug: "best-of-three".into(),
            ..Mission::default()
        };
        assert_eq!(mission.session_prefix(), "bestofthree");
        let set = Mission {
            prefix: "Bo3".into(),
            ..mission
        };
        assert_eq!(set.session_prefix(), "bo3");
        let solo = step("Write the draft!", &[("", "")]);
        assert_eq!(solo.session_part(&solo.seats[0]), "write-the-draft");
        let named = step("Write", &[("", "reviewer")]);
        assert_eq!(
            seat_session("bo3", &named.session_part(&named.seats[0])),
            "bo3-write"
        );
        let team = step("Review", &[("Lead", ""), ("Second", "")]);
        assert_eq!(team.session_part(&team.seats[1]), "review-second");
        assert_eq!(fresh_session("bo3", "writer", 4, 2), "bo3-writer-4-2");
        // A name the list would split, or tmux refuse, is not a prefix.
        assert!(valid_prefix("bo3") && !valid_prefix("bo-3") && !valid_prefix("a:b"));
    }
}
