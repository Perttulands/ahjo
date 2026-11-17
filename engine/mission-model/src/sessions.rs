//! What a mission's agents are called where people see them: ordinary tmux
//! sessions named under the mission's prefix, `<prefix>-<agent>`, so the
//! session list groups them by that prefix.
//!
//! A step names its agent; a run uses that agent's session when it is live
//! and starts it when it is not, and the session stays for the next run. A
//! step that always starts fresh gets a session of its own for each
//! attempt, `<prefix>-<agent>-<run>-<attempt>`, which ends with the attempt.

use crate::{Formation, Mission, Slot};

/// The longest a prefix or an agent's name grows.
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

/// An agent's name as a step may set it: words of letters and digits joined
/// by single dashes.
pub fn valid_agent(agent: &str) -> bool {
    !agent.is_empty() && words(agent, "-") == agent
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

impl Formation {
    /// The agent a slot of this step names: the slot's own name, else the
    /// step's title (with the slot's label when the step has several).
    pub fn agent_of(&self, slot: &Slot) -> String {
        if !slot.agent.trim().is_empty() {
            return words(&slot.agent, "-");
        }
        let title = if self.title.trim().is_empty() {
            &self.id
        } else {
            &self.title
        };
        let label = if slot.label.trim().is_empty() {
            &slot.id
        } else {
            &slot.label
        };
        let name = if self.slots.len() > 1 {
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

/// The session an agent of `mission` works in.
pub fn agent_session(prefix: &str, agent: &str) -> String {
    format!("{prefix}-{agent}")
}

/// The session a fresh attempt works in, its own for that attempt.
pub fn fresh_session(prefix: &str, agent: &str, run: u64, attempt: i64) -> String {
    format!("{prefix}-{agent}-{run}-{attempt}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(title: &str, slots: &[(&str, &str)]) -> Formation {
        Formation {
            id: "fmn_a".into(),
            title: title.into(),
            slots: slots
                .iter()
                .map(|(label, agent)| Slot {
                    id: format!("s-{label}"),
                    label: (*label).into(),
                    agent: (*agent).into(),
                    ..Slot::default()
                })
                .collect(),
            ..Formation::default()
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
        assert_eq!(solo.agent_of(&solo.slots[0]), "write-the-draft");
        let named = step("Write", &[("", "Writer")]);
        assert_eq!(
            agent_session("bo3", &named.agent_of(&named.slots[0])),
            "bo3-writer"
        );
        let team = step("Review", &[("Lead", ""), ("Second", "")]);
        assert_eq!(team.agent_of(&team.slots[1]), "review-second");
        assert_eq!(fresh_session("bo3", "writer", 4, 2), "bo3-writer-4-2");
        // A name the list would split, or tmux refuse, is not a prefix.
        assert!(valid_prefix("bo3") && !valid_prefix("bo-3") && !valid_prefix("a:b"));
        assert!(valid_agent("lead-writer") && !valid_agent("Lead Writer"));
    }
}
