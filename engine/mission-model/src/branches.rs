//! The names a run's work goes under in a repository: short enough to read
//! in `git branch`, safe as refs, and derived from the run, so every node
//! and the app name the same branch.

/// A run's short name in git: the first eight characters of its id after
/// `run_`. Ids are random, so a repository's runs do not share one; a
/// branch that is somehow taken already fails its creation loudly.
pub fn run_tag(run_id: &str) -> &str {
    let id = run_id.strip_prefix("run_").unwrap_or(run_id);
    let end = id.char_indices().nth(8).map_or(id.len(), |(at, _)| at);
    &id[..end]
}

/// The branch a run's repository work is on: `ahjo/run-1a2b3c4d`.
pub fn run_branch(run_id: &str) -> String {
    format!("ahjo/run-{}", run_tag(run_id))
}

/// The branch an attempt's unpublished work is kept on, named by its step:
/// `ahjo/run-1a2b3c4d-kept/write-the-digest-2`. It stands beside the run's
/// own branch, which as a branch cannot also be a folder of them.
pub fn kept_branch(run_id: &str, step: &str, attempt: i64) -> String {
    let mut name = String::new();
    for c in step.chars() {
        if c.is_ascii_alphanumeric() {
            name.push(c.to_ascii_lowercase());
        } else if !name.is_empty() && !name.ends_with('-') {
            name.push('-');
        }
        if name.len() >= STEP_MAX {
            break;
        }
    }
    let name = name.trim_end_matches('-');
    let name = if name.is_empty() { "step" } else { name };
    format!("{}-kept/{name}-{attempt}", run_branch(run_id))
}

/// The longest a step's name runs in a kept branch.
const STEP_MAX: usize = 32;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_branch_is_short_and_a_kept_one_names_its_step() {
        let id = "run_1a2b3c4d5e6f708192a3b4c5d6e7f809";
        assert_eq!(run_branch(id), "ahjo/run-1a2b3c4d");
        assert_eq!(
            kept_branch(id, "Write the digest!", 2),
            "ahjo/run-1a2b3c4d-kept/write-the-digest-2"
        );
        assert_eq!(
            kept_branch(id, "Äö / ..", 1),
            "ahjo/run-1a2b3c4d-kept/step-1"
        );
        assert_eq!(run_branch("r1"), "ahjo/run-r1");
    }
}
