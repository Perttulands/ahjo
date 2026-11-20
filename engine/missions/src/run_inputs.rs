//! Admission and rendering of values supplied to a mission run.

use ahjo_mission_model::{Code, Finding, Mission, MissionInput, PortKind, RunInput};
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

pub fn mission_run_inputs(mission: &Mission) -> Vec<MissionInput> {
    if let Some(card) = mission.input_card()
        && !card.inputs.is_empty()
    {
        return card
            .inputs
            .iter()
            .cloned()
            .map(|mut input| {
                input.kind = input.kind.effective();
                input
            })
            .collect();
    }
    vec![MissionInput {
        name: "brief".into(),
        kind: PortKind::Text,
        required: true,
        description: mission
            .input_card()
            .map(|card| card.input_hint.clone())
            .unwrap_or_default(),
    }]
}

pub fn input_findings(mission: &Mission, supplied: &BTreeMap<String, String>) -> Vec<Finding> {
    let declared = mission_run_inputs(mission);
    let node = mission
        .input_card()
        .map(|card| card.id.as_str())
        .unwrap_or("");
    let mut findings = Vec::new();
    for input in &declared {
        let value = supplied.get(&input.name).map(|s| s.trim()).unwrap_or("");
        if value.is_empty() {
            if input.required {
                let suffix = if input.description.is_empty() {
                    String::new()
                } else {
                    format!(": {}", input.description)
                };
                findings.push(Finding::new(
                    Code::MissingInput,
                    node,
                    format!("input {} is required{}", input.name, suffix),
                ));
            }
            continue;
        }
        if input.kind == PortKind::File || input.kind == PortKind::Folder {
            let path = Path::new(value);
            let problem = if !path.is_absolute() {
                Some(format!("{value:?} is a relative path"))
            } else {
                match path.metadata() {
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                        Some(format!("{value} does not exist"))
                    }
                    Err(err) => Some(format!("{value} cannot be read: {err}")),
                    Ok(meta) if input.kind == PortKind::File && !meta.is_file() => {
                        Some(format!("{value} is not a regular file"))
                    }
                    Ok(meta) if input.kind == PortKind::Folder && !meta.is_dir() => {
                        Some(format!("{value} is not a directory"))
                    }
                    Ok(_) => None,
                }
            };
            if let Some(problem) = problem {
                let kind = if input.kind == PortKind::File {
                    "file"
                } else {
                    "directory"
                };
                findings.push(Finding::new(
                    Code::InvalidInput,
                    node,
                    format!(
                        "input {} must be the absolute path of an existing {kind}; {problem}",
                        input.name
                    ),
                ));
            }
        }
    }
    let names = declared
        .iter()
        .map(|input| input.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    for name in supplied
        .keys()
        .filter(|name| !declared.iter().any(|input| &input.name == *name))
    {
        findings.push(Finding::new(
            Code::UnknownInput,
            node,
            format!(
                "mission {} has no input named {name:?}; its inputs are {names}",
                mission.slug
            ),
        ));
    }
    findings.sort_by(|a, b| {
        (a.code.as_str(), &a.card_id, &a.message).cmp(&(b.code.as_str(), &b.card_id, &b.message))
    });
    findings
}

pub fn resolve_run_inputs(mission: &Mission, supplied: &BTreeMap<String, String>) -> Vec<RunInput> {
    mission_run_inputs(mission)
        .into_iter()
        .filter_map(|input| {
            let value = supplied.get(&input.name)?;
            if value.trim().is_empty() {
                return None;
            }
            Some(RunInput {
                name: input.name,
                value: if input.kind == PortKind::Text {
                    value.clone()
                } else {
                    value.trim().to_owned()
                },
                kind: input.kind.as_str().to_owned(),
            })
        })
        .collect()
}

pub fn render_run_inputs(mission: &Mission, inputs: &[RunInput]) -> String {
    if mission
        .input_card()
        .is_none_or(|card| card.inputs.is_empty())
        && inputs.len() == 1
        && inputs[0].name == "brief"
    {
        return inputs[0].value.clone();
    }
    inputs
        .iter()
        .map(|input| format!("{}: {}", input.name, input.value))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The placeholder for the run's tracked issue.
pub const ISSUE_REFERENCE: &str = "issue";

/// The short form substituted for a run's canonical issue reference.
pub fn issue_placeholder(issue: &str) -> String {
    ahjo_issue_ref::parse_resolved(issue)
        .map(|r| r.short())
        .unwrap_or_default()
}

/// Replace only valid input references. Inserted values are never interpreted again.
/// `{issue}` takes the run issue unless a declared input takes precedence.
/// `{issue.ref}` always takes the run issue and preserves migrated missions.
pub fn substitute_run_inputs(
    text: &str,
    declared: &[MissionInput],
    inputs: &[RunInput],
    issue: &str,
) -> String {
    let known: HashSet<&str> = declared.iter().map(|input| input.name.as_str()).collect();
    let mut result = String::new();
    let mut rest = text;
    while let Some(pos) = rest.find('{') {
        result.push_str(&rest[..pos]);
        rest = &rest[pos..];
        if let Some(tail) = rest.strip_prefix("{{issue.ref}}") {
            result.push_str("{issue.ref}");
            rest = tail;
            continue;
        }
        if let Some(tail) = rest.strip_prefix("{issue.ref}") {
            result.push_str(issue);
            rest = tail;
            continue;
        }
        let escaped = rest.starts_with("{{");
        let prefix = if escaped { 2 } else { 1 };
        let after = &rest[prefix..];
        let len = after
            .bytes()
            .take_while(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_')
            .count();
        let name = &after[..len];
        let suffix = if escaped { "}}" } else { "}" };
        if len > 0
            && len <= 64
            && name.as_bytes()[0].is_ascii_lowercase()
            && after[len..].starts_with(suffix)
        {
            if escaped {
                result.push('{');
                result.push_str(name);
                result.push('}');
            } else if name == ISSUE_REFERENCE && !known.contains(name) {
                result.push_str(issue);
            } else if known.contains(name) {
                result.push_str(
                    inputs
                        .iter()
                        .find(|input| input.name == name)
                        .map(|input| input.value.as_str())
                        .unwrap_or("(not supplied)"),
                );
            } else {
                result.push('{');
                result.push_str(name);
                result.push('}');
            }
            rest = &after[len + suffix.len()..];
        } else {
            result.push('{');
            rest = &rest[1..];
        }
    }
    result.push_str(rest);
    result
}
