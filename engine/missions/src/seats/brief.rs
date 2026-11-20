//! Render the complete, private instructions a seat reads by pointer.
use crate::StepExecution;
use ahjo_mission_model::Seat;
use std::fmt::Write;

pub(super) fn render(request: &StepExecution, seat: &Seat) -> String {
    render_for(request, seat, true)
}

/// A worker has the same context, but the controller owns the final output.
pub(super) fn render_worker(request: &StepExecution, seat: &Seat) -> String {
    let mut brief = render_for(request, seat, false);
    let _ = writeln!(
        brief,
        "You are a worker on this team. Work on the controller's assigned task, save your output in the attempt directory as worker-{}.md, and report its path and findings to the controller. The controller owns the step's final output.",
        seat.id
    );
    brief
}

fn render_for(request: &StepExecution, seat: &Seat, final_output: bool) -> String {
    let mut brief = String::new();
    let _ = writeln!(
        brief,
        "Run: {}\nStep: {} ({})\nSeat: {}\nAgent: {}\nHarness: {}\nWorking directory: {}\nAttempt directory: {}\n",
        request.run_id,
        request.step.title,
        request.step.id,
        seat.id,
        if seat.agent.is_empty() {
            "vanilla (no role)"
        } else {
            &seat.agent
        },
        seat.harness,
        request.cwd.display(),
        request.artifact_dir.display()
    );
    let _ = writeln!(
        brief,
        "Work in the working directory and write what you make there, unless the goal names another place. The attempt directory is Ahjo's record of this attempt, not where results go.\n"
    );
    // A repository run's attempt works in a checkout of its own, here
    // ([`crate::run_workspace::turn_path`]) or on the environment it is
    // placed on.
    let own_checkout = request.turn.is_some()
        || request
            .cwd
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with(".turn-"));
    if own_checkout {
        let _ = writeln!(
            brief,
            "The working directory is a checkout made for this attempt: go there first, wherever your session started. What you leave in it is committed to the run's branch and the checkout is then removed, so name a file by its path inside it, not by an absolute path.\n"
        );
    }
    if let Some(turn) = &request.turn {
        let _ = writeln!(brief, "Starting commit: {}", turn.commit);
    }
    if let Ok(work) = ahjo_work_ref::parse(&request.work) {
        let _ = writeln!(brief, "Mission {}: {}", work.noun(), work.describe());
    }
    if let Some(details) = &request.step.brief {
        let _ = writeln!(brief, "Goal:\n{}\n", details.goal);
        if let Ok(work) = ahjo_work_ref::parse(&details.work) {
            let _ = writeln!(brief, "Brief {}: {}", work.noun(), work.describe());
        }
        for file in &details.files {
            let _ = writeln!(brief, "Reference file: {file}");
        }
        for link in &details.links {
            let _ = writeln!(brief, "Reference link: {link}");
        }
    }
    if let Some(binding) = request
        .bindings
        .bindings
        .iter()
        .find(|binding| binding.step_id == request.step.id && binding.seat_id == seat.id)
        && let Some(instructions) = binding.instructions()
        && !instructions.is_empty()
    {
        let _ = writeln!(brief, "\nAgent instructions:\n{instructions}");
    }
    for path in &request.context_paths {
        let _ = writeln!(brief, "Context path: {}", path.display());
    }
    let _ = writeln!(
        brief,
        "\nRouted input and feedback:\n{}\n",
        request.input_text
    );
    if final_output && !request.step.outputs.is_empty() {
        brief.push_str("Return all and only these output ports in one fenced `ahjo-outputs` JSON block. Each port maps to an object with `text`; file and folder outputs must also carry an absolute `ref` path.\n");
        for port in &request.step.outputs {
            let _ = writeln!(brief, "- {} ({})", port.id, port.kind());
        }
        let example: serde_json::Map<String, serde_json::Value> = request
            .step
            .outputs
            .iter()
            .map(|port| {
                let payload = if port.kind() == ahjo_mission_model::PortKind::File
                    || port.kind() == ahjo_mission_model::PortKind::Folder
                {
                    serde_json::json!({"text":"result", "ref":"/absolute/path/to/artifact"})
                } else {
                    serde_json::json!({"text":"result"})
                };
                (port.id.clone(), payload)
            })
            .collect();
        let _ = writeln!(
            brief,
            "Example shape (replace values):\n```ahjo-outputs\n{}\n```",
            serde_json::Value::Object(example)
        );
    }
    if final_output && !request.pick_attempts.is_empty() {
        brief.push_str(&crate::run_pick::contract(
            request.judge,
            &request.pick_attempts,
        ));
    } else if final_output && request.judge {
        brief.push_str("Return exactly one fenced `ahjo-verdict` block containing JSON with exactly `verdict` (pass or fail), `reason` (string), and `evidence` (array of strings).\n");
    }
    let _ = writeln!(
        brief,
        "\nWhen finished, end your response with this line, ARTIFACT replaced by the path or port id you produced and the closing >>> kept:\n<<<AHJO-DONE run-id={} status=ok artifact=ARTIFACT>>>",
        request.run_id
    );
    brief
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BindingsSnapshot;
    use ahjo_mission_model::{Brief, Step};

    fn execution(run_work: &str, brief_work: &str) -> StepExecution {
        StepExecution {
            run_id: "run_1".into(),
            prefix: String::new(),
            run_number: 0,
            step: Step {
                id: "fmn_work".into(),
                title: "Work".into(),
                brief: Some(Brief {
                    goal: "Ship".into(),
                    work: brief_work.into(),
                    ..Brief::default()
                }),
                ..Step::default()
            },
            bindings: BindingsSnapshot {
                schema: 1,
                run_id: "run_1".into(),
                mission_id: String::new(),
                mission_slug: String::new(),
                mission_rev: 0,
                input_card_id: String::new(),
                bindings: Vec::new(),
                gate_bindings: Vec::new(),
            },
            cwd: "/w".into(),
            turn: None,
            artifact_dir: "/a".into(),
            context_paths: Vec::new(),
            work: run_work.into(),
            inputs: Vec::new(),
            input_text: String::new(),
            attempt: 1,
            judge: false,
            pick_attempts: Vec::new(),
            canceled: Default::default(),
            stopping: Default::default(),
            limits: Default::default(),
        }
    }

    /// Archon's brief names the mission bead and the brief bead; Ahjo says
    /// bead or issue by tracker, with a Bead's store so bd runs in it.
    #[test]
    fn names_the_runs_work_item_and_the_briefs_own() {
        let text = render(
            &execution("bead:n1:/srv/x#x-1", "gh:a/b#2"),
            &Seat::default(),
        );
        assert!(
            text.contains("Mission bead: x-1 (store: /srv/x)\n"),
            "{text}"
        );
        assert!(text.contains("Brief issue: a/b#2\n"), "{text}");
        let text = render(&execution("gh:a/b#2", "x-9"), &Seat::default());
        assert!(text.contains("Mission issue: a/b#2\n"), "{text}");
        assert!(text.contains("Brief bead: x-9\n"), "{text}");
        let text = render(&execution("", ""), &Seat::default());
        assert!(
            !text.contains("Mission ") && !text.contains("Brief "),
            "{text}"
        );
    }
}
