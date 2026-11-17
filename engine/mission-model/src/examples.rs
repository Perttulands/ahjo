//! Missions a person starts from: each is a whole graph, staffed and
//! wired, that `parvi mission new <slug> --example <name>` copies under a
//! new id.
pub struct Example {
    pub name: &'static str,
    pub title: &'static str,
    /// What it does, in a sentence, for a list to show.
    pub about: &'static str,
    pub definition: &'static str,
}

pub const EXAMPLES: &[Example] = &[
    Example {
        name: "best-of-three",
        title: "Best of three",
        about: "Three fresh attempts; a judge passes the first good one, else the best goes on.",
        definition: include_str!("examples/best-of-three.mission.toml"),
    },
    Example {
        name: "write-and-review",
        title: "Write and review",
        about: "A writer in its own session and a reviewer who sends it back, three rounds at most.",
        definition: include_str!("examples/write-and-review.mission.toml"),
    },
    Example {
        name: "plan-build-check",
        title: "Plan, build, check",
        about: "Plan, build, and checks that send a failure back to the builder, within an hour.",
        definition: include_str!("examples/plan-build-check.mission.toml"),
    },
];

/// The example of this name.
pub fn example(name: &str) -> Option<&'static Example> {
    EXAMPLES.iter().find(|e| e.name == name)
}
