//! One-time conversion of saved roles and character setups.
use std::collections::{HashMap, HashSet};
use std::path::Path;

use ahjo_mission_model::{AGENT_SCHEMA, Agent, AgentNote, Effort, Harness};
use serde::Deserialize;

use crate::{Error, links};

const BUILTIN: &[(&str, &str, &str, &[&str])] = &[
    (
        "scout",
        "Scout",
        "Explores a codebase read-only before changes begin: maps files, callers, tests, risks and unknowns, and reports the evidence.",
        &["research", "inspect", "read-only"],
    ),
    (
        "planner",
        "Planner",
        "Turns grounded requirements into an ordered implementation plan with explicit verification.",
        &["planning", "design"],
    ),
    (
        "builder",
        "Builder",
        "Implements scoped changes, exercises them, and leaves a working artifact.",
        &["implement", "test"],
    ),
    (
        "judge",
        "Judge",
        "Judges work against its acceptance criteria, scope, safety and evidence, and returns a clear pass or fail verdict with the blocking findings.",
        &["judge", "verify", "review"],
    ),
    (
        "orchestrator",
        "Orchestrator",
        "Coordinates role-specialized agents, dependencies, handoffs, and completion gates.",
        &["orchestrate", "coordinate"],
    ),
    (
        "debugger",
        "Debugger",
        "Diagnoses failures from evidence, isolates root causes, and verifies repairs.",
        &["debug", "diagnose"],
    ),
    (
        "reviewer",
        "Reviewer",
        "Reviews changes independently for correctness, regressions, and maintainability.",
        &["review", "audit"],
    ),
];

#[derive(Default, Deserialize)]
#[serde(default)]
struct OldCard {
    id: String,
    display_name: String,
    kind: String,
    summary: String,
    tags: Vec<String>,
    status: String,
    #[serde(flatten)]
    extra: std::collections::BTreeMap<String, toml::Value>,
}
#[derive(Default, Deserialize)]
#[serde(default)]
struct OldNote {
    ts: String,
    actor: String,
    text: String,
}
#[derive(Deserialize)]
struct OldFile {
    schema: i64,
    card: OldCard,
    #[serde(default, rename = "note")]
    notes: Vec<OldNote>,
}
#[derive(Default, Deserialize)]
#[serde(default)]
struct OldSetup {
    id: String,
    name: String,
    description: String,
    picture: String,
    #[serde(alias = "agentId")]
    agent_id: String,
    harness: String,
    model: String,
    effort: String,
}
#[derive(Deserialize)]
struct OldSetups {
    schema: i64,
    #[serde(default)]
    characters: Vec<OldSetup>,
}

pub(crate) fn upgrade(dir: &Path) -> Result<(), Error> {
    let root = dir.parent().unwrap_or(dir);
    let lock = crate::write::locks(root, "agents-migration");
    let _guard = lock
        .document
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let marker = dir.join(".migrated");
    if marker.is_file() {
        return Ok(());
    }
    let old_dir = root.join("roles");
    let mut roles = HashMap::new();
    let mut retired = HashSet::new();
    let mut unreadable = HashMap::new();
    for (id, name, summary, _) in BUILTIN {
        roles.insert(
            (*id).to_owned(),
            Agent {
                schema: AGENT_SCHEMA,
                id: (*id).to_owned(),
                name: (*name).to_owned(),
                instructions: (*summary).to_owned(),
                harness: Harness::ClaudeCode,
                model: String::new(),
                effort: crate::harness::starter_effort(id),
                notes: Vec::new(),
            },
        );
    }
    for id in links::list_names(&old_dir, ".toml")? {
        let source = match links::read_definition(&old_dir.join(format!("{id}.toml"))) {
            Ok(source) => source,
            Err(error) => {
                roles.remove(&id);
                unreadable.insert(id, format!("role source unavailable: {error}"));
                continue;
            }
        };
        match parse_role(&id, &source, crate::harness::starter_effort(&id)) {
            Ok((agent, is_retired)) => {
                roles.insert(id.clone(), agent);
                if is_retired {
                    retired.insert(id);
                }
            }
            Err(_) => {
                roles.remove(&id);
                unreadable.insert(id, source);
            }
        }
    }

    let setups_path = old_dir.join("characters.json");
    let mut setups_source = match links::read_definition(&setups_path) {
        Ok(source) => Some(source),
        Err(error) if error.code == "not_found" => None,
        Err(error) => {
            unreadable.insert(
                "characters-unreadable".into(),
                format!("characters.json: {error}"),
            );
            None
        }
    };
    let mut setups = Vec::new();
    if let Some(source) = &setups_source {
        match serde_json::from_str::<OldSetups>(source) {
            Ok(file) if [1, 2].contains(&file.schema) => setups = file.characters,
            Ok(file) => {
                unreadable.insert(
                    "characters-unreadable".into(),
                    format!("characters.json schema {}", file.schema),
                );
                setups_source = None;
            }
            Err(error) => {
                unreadable.insert(
                    "characters-unreadable".into(),
                    format!("characters.json: {error}"),
                );
                setups_source = None;
            }
        }
    }

    std::fs::create_dir_all(dir.join("archive")).map_err(crate::error::io)?;
    let mut ids: HashSet<String> = roles.keys().chain(unreadable.keys()).cloned().collect();
    for (id, source) in &unreadable {
        let target = dir.join(format!("{id}.toml"));
        if std::fs::symlink_metadata(&target).is_err() {
            crate::write::atomic(&target, source.as_bytes()).map_err(crate::error::io)?;
        }
    }
    for agent in roles.values() {
        let parent = if retired.contains(&agent.id) {
            dir.join("archive")
        } else {
            dir.to_path_buf()
        };
        let active = dir.join(format!("{}.toml", agent.id));
        let archived = dir.join("archive").join(format!("{}.toml", agent.id));
        if retired.contains(&agent.id) && std::fs::symlink_metadata(&active).is_ok() {
            return Err(Error::new(
                "conflict",
                format!("retired role {:?} conflicts with an active agent", agent.id),
            ));
        }
        if std::fs::symlink_metadata(&active).is_err()
            && std::fs::symlink_metadata(&archived).is_err()
        {
            write_new(&parent.join(format!("{}.toml", agent.id)), agent)?;
        }
    }
    for setup in setups {
        crate::agents::check_agent_id(&setup.id)?;
        let mut id = setup.id.clone();
        if ids.contains(&id) {
            id = format!("{id}-character");
            let mut suffix = 2;
            while ids.contains(&id) {
                id = format!("{}-character-{suffix}", setup.id);
                suffix += 1;
            }
        }
        ids.insert(id.clone());
        let unavailable = !setup.agent_id.is_empty()
            && (!roles.contains_key(&setup.agent_id) || retired.contains(&setup.agent_id));
        let mut instructions = if setup.agent_id.is_empty() {
            String::new()
        } else if let Some(role) = roles.get(&setup.agent_id) {
            role.instructions.clone()
        } else {
            format!("Original role {} is unavailable.", setup.agent_id)
        };
        if !setup.description.is_empty() {
            if !instructions.is_empty() {
                instructions.push_str("\n\n");
            }
            instructions.push_str(&setup.description);
        }
        // The original JSON backup retains image data and all original metadata.
        let _ = &setup.picture;
        let harness = setup
            .harness
            .parse()
            .map_err(|error: String| Error::new("invalid_definition_source", error))?;
        let effort = setup
            .effort
            .parse()
            .map_err(|error: String| Error::new("invalid_definition_source", error))?;
        let agent = Agent {
            schema: AGENT_SCHEMA,
            id: id.clone(),
            name: setup.name,
            instructions,
            harness,
            model: setup.model,
            effort,
            notes: Vec::new(),
        };
        let target = if unavailable {
            dir.join("archive").join(format!("{id}.toml"))
        } else {
            dir.join(format!("{id}.toml"))
        };
        let other = if unavailable {
            dir.join(format!("{id}.toml"))
        } else {
            dir.join("archive").join(format!("{id}.toml"))
        };
        if std::fs::symlink_metadata(&other).is_ok() {
            return Err(Error::new(
                "conflict",
                format!("agent {id:?} exists in the wrong location during character migration"),
            ));
        }
        if std::fs::symlink_metadata(&target).is_ok() {
            let existing = links::read_definition(&target)?;
            let parsed: Agent = toml::from_str(&existing)
                .map_err(|error| Error::new("invalid_definition_source", error.to_string()))?;
            if parsed != agent {
                return Err(Error::new(
                    "conflict",
                    format!("agent {id:?} already exists during character migration"),
                ));
            }
        } else {
            write_new(&target, &agent)?;
        }
    }
    if let Some(source) = setups_source {
        let backup = old_dir.join("characters.json.bak");
        match std::fs::symlink_metadata(&backup) {
            Ok(_) => {
                let existing = links::read_definition(&backup)?;
                if existing != source {
                    return Err(Error::new(
                        "conflict",
                        "characters.json.bak already holds other data",
                    ));
                }
                std::fs::remove_file(&setups_path).map_err(crate::error::io)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                std::fs::rename(&setups_path, backup).map_err(crate::error::io)?;
            }
            Err(error) => return Err(crate::error::io(error)),
        }
    }
    crate::write::atomic(&marker, b"1\n").map_err(crate::error::io)?;
    Ok(())
}

/// Convert a frozen role source without touching the filesystem.
pub(crate) fn agent_from_role(id: &str, raw: &str) -> Result<Agent, Error> {
    // Retain the original conversion of frozen run snapshots.
    let effort = match id {
        "scout" => Effort::Low,
        "reviewer" | "judge" | "planner" | "orchestrator" => Effort::Xhigh,
        _ => Effort::Medium,
    };
    parse_role(id, raw, effort).map(|(agent, _)| agent)
}

fn parse_role(id: &str, source: &str, effort: Effort) -> Result<(Agent, bool), Error> {
    let file: OldFile = toml::from_str(source).map_err(|error| {
        Error::new("invalid_definition_source", format!("role {id:?}: {error}"))
    })?;
    if file.schema != 1 || file.card.id != id {
        return Err(Error::new(
            "invalid_definition_source",
            format!("role {id:?} has an unsupported schema or mismatched id"),
        ));
    }
    crate::agents::check_agent_id(id)?;
    let mut instructions = file.card.summary.clone();
    for (key, value) in &file.card.extra {
        if !instructions.is_empty() {
            instructions.push_str("\n\n");
        }
        instructions.push_str(key);
        instructions.push_str(": ");
        match value.as_str() {
            Some(text) => instructions.push_str(text),
            None => instructions.push_str(&value.to_string()),
        }
    }
    let _ = (&file.card.kind, &file.card.tags);
    let name = if file.card.display_name.is_empty() {
        id.to_owned()
    } else {
        file.card.display_name
    };
    let is_retired = file.card.status == "retired";
    Ok((
        Agent {
            schema: AGENT_SCHEMA,
            id: id.to_owned(),
            name,
            instructions,
            harness: Harness::ClaudeCode,
            model: String::new(),
            effort,
            notes: file
                .notes
                .into_iter()
                .map(|note| AgentNote {
                    at: note.ts,
                    by: note.actor,
                    text: note.text,
                })
                .collect(),
        },
        is_retired,
    ))
}

fn write_new(path: &Path, agent: &Agent) -> Result<(), Error> {
    let text = crate::agents::encode_agent(agent)?;
    crate::write::atomic(path, text.as_bytes()).map_err(crate::error::io)
}
