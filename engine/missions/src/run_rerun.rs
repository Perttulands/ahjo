//! Starting one step of an earlier run again as it started there: the input
//! its first attempt got and the commit that attempt started from, read from
//! the earlier run's ledger and recorded in the new run's `run_started`.
use crate::{AdmissionFailure, Coordinator};
use ahjo_mission_model::RunStart;
use serde_json::{Map, Value};

/// What the new run takes from the earlier one beyond its start request.
pub(crate) struct Rerun {
    /// The commit the step's first attempt started from.
    pub commit: String,
    /// The routed input that attempt got. `None` when the earlier run was
    /// itself started at the step, whose input was the run's inputs.
    pub input_refs: Option<Vec<Value>>,
}

impl Rerun {
    pub(crate) fn record(&self, rerun_of: &str, data: &mut Map<String, Value>) {
        data.insert("rerun_of".into(), rerun_of.into());
        data.insert("commit".into(), self.commit.clone().into());
        if let Some(refs) = &self.input_refs {
            data.insert("input_refs".into(), refs.clone().into());
        }
    }
}

/// The input refs a rerun records, if this run is one that has them.
pub(crate) fn recorded_input(started: &crate::RunEvent) -> Option<Vec<Value>> {
    started
        .data
        .get("input_refs")
        .and_then(Value::as_array)
        .cloned()
}

/// Fills `request` from the run it names in `rerun_of` and answers what the
/// new run records. The step is the request's `step_id`.
pub(crate) fn prepare(c: &Coordinator, request: &mut RunStart) -> Result<Rerun, AdmissionFailure> {
    let earlier = request.rerun_of.clone();
    if !request.inputs.is_empty()
        || !request.issue.is_empty()
        || !request.repository.is_empty()
        || !request.base.is_empty()
        || !request.cwd.is_empty()
        || !request.context_paths.is_empty()
    {
        return Err(AdmissionFailure::Invalid(format!(
            "a rerun takes its inputs, issue, repository, base and context paths from run {earlier}; leave them out"
        )));
    }
    let run = c
        .run(&earlier)
        .map_err(|error| AdmissionFailure::NotFound(error.message))?;
    let events = c.ledger(&earlier)?.read().map_err(crate::error::to_io)?;
    let step = request.step_id.as_str();
    let started = events
        .iter()
        .filter(|event| event.kind == "card_started" && event.card_id == step)
        .min_by_key(|event| event.attempt)
        .ok_or_else(|| {
            AdmissionFailure::NotFound(format!("step {step:?} never started in run {earlier}"))
        })?;
    if run.repository.is_empty() {
        return Err(AdmissionFailure::Invalid(format!(
            "run {earlier} worked in a folder, not a repository; Ahjo keeps no copy of the folder a step started from, so it cannot start step {step:?} again as it was"
        )));
    }
    let commit = events
        .iter()
        .find(|event| {
            event.kind == "workspace_started"
                && event.card_id == step
                && event.attempt == started.attempt
        })
        .and_then(|event| event.data.get("base"))
        .and_then(Value::as_str)
        .filter(|commit| !commit.is_empty())
        .ok_or_else(|| {
            AdmissionFailure::Invalid(format!(
                "run {earlier} recorded no commit for step {step:?}'s attempt {}",
                started.attempt
            ))
        })?
        .to_owned();
    let first = &events[0];
    let started_at_step = first
        .data
        .get("step_id")
        .and_then(Value::as_str)
        .is_some_and(|id| !id.is_empty());
    let input_refs = match recorded_input(first) {
        Some(refs) => Some(refs),
        None if started_at_step => None,
        None => Some(
            started
                .data
                .get("input_refs")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
        ),
    };
    request.repository = run.repository;
    request.base = run.base;
    request.issue = run.issue;
    request.context_paths = run.context_paths;
    request.inputs = run
        .inputs
        .into_iter()
        .map(|input| (input.name, input.value))
        .collect();
    Ok(Rerun { commit, input_refs })
}
