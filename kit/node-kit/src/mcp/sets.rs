//! Tool sets: one MCP server shaped for one kind of client, at
//! `/mcp/<set>`, holding commands chosen from several areas and, if it
//! names them, three tools of its own that reach every other command. A
//! client such as ChatGPT pays for a dozen tools instead of every
//! command's, and loses none of them.
//!
//! The three exist only in a set: on the command line and on `/mcp`
//! every command is already its own verb or tool.
//!
//! - `swarm_search` finds commands by words, with what each does.
//! - `swarm_read` runs one that only reads, so a client need not ask the
//!   operator first; it refuses one that changes anything.
//! - `swarm_run` runs one that changes something, and is marked as a tool
//!   that may destroy, so the client asks; it refuses one that only reads.
//!
//! Both run a command the way `tools/call` does: the same `node` check,
//! the command's own argument checks and errors, and a call-log line
//! naming the command. A command the node does not serve (its area turned
//! off) is not there to run.

use std::sync::Arc;

use crate::{Command, Effect, Listing};
use ahjo_core::{Error, NodeId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// A tool set as a binary lists it.
#[derive(Clone, Copy)]
pub struct ToolSet {
    /// Its path under `/mcp`; no area may have the name.
    pub name: &'static str,
    /// Its tools in order: commands by name, and any of [`SET_TOOLS`].
    pub tools: &'static [&'static str],
    /// What `initialize` tells a client. Its first 512 characters stand on
    /// their own: ChatGPT may show no more.
    pub instructions: &'static str,
}

pub const SEARCH: &str = "swarm_search";
pub const READ: &str = "swarm_read";
pub const RUN: &str = "swarm_run";

/// The tools a set may name that are not commands.
pub const SET_TOOLS: &[&str] = &[SEARCH, READ, RUN];

/// How many commands `swarm_search` answers with at most.
const FOUND: usize = 12;

/// Refuses sets that share a name with each other, an area or the node's
/// own server, that name a tool no listing has, or name one twice.
pub fn check(sets: &[ToolSet], areas: &[Listing]) -> Result<(), Error> {
    let conflict = |message: String| Error::new("area_conflict", message);
    for (i, set) in sets.iter().enumerate() {
        if set.name == super::NODE
            || areas.iter().any(|a| a.name == set.name)
            || sets[..i].iter().any(|s| s.name == set.name)
        {
            return Err(conflict(format!(
                "the tool set {} has the name of another MCP server",
                set.name
            )));
        }
        for (at, tool) in set.tools.iter().enumerate() {
            let known = SET_TOOLS.contains(tool)
                || crate::own::COMMANDS.contains(tool)
                || areas.iter().any(|a| a.commands.contains(tool));
            if !known {
                return Err(conflict(format!(
                    "the tool set {} names {tool}, which no area lists",
                    set.name
                )));
            }
            if set.tools[..at].contains(tool) {
                return Err(conflict(format!(
                    "the tool set {} names {tool} twice",
                    set.name
                )));
            }
        }
    }
    Ok(())
}

/// Find Swarm commands by words and see how to call them through this server.
#[derive(Deserialize, JsonSchema)]
struct SearchCommands {
    /// Words for what you want to do, e.g. "approve gate" or "scheduled prompts". Omit to list every command.
    query: Option<String>,
    /// Full adds each command's input schema. Defaults to brief.
    detail: Option<SearchDetail>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum SearchDetail {
    Brief,
    Full,
}

#[derive(Serialize, JsonSchema)]
struct CommandsFound {
    commands: Vec<CommandMatch>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hint: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    more: Option<usize>,
}

#[derive(Serialize, JsonSchema)]
#[serde(untagged)]
enum CommandMatch {
    Name(&'static str),
    Match {
        name: &'static str,
        does: &'static str,
        run_with: &'static str,
        description: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        input_schema: Option<Value>,
    },
}

#[derive(Deserialize, JsonSchema)]
struct Relay {
    /// The command's name, e.g. run_show.
    command: String,
    /// Its arguments by name, as swarm_search with detail full lists them.
    arguments: Option<Map<String, Value>>,
}

/// Run a Swarm command that only reads, as swarm_search names it. Use swarm_run for commands that change something.
#[derive(Deserialize, JsonSchema)]
struct ReadCommand {
    #[serde(flatten)]
    relay: Relay,
}

/// Run a Swarm command that changes something, as swarm_search names it. Use swarm_read for commands that only read.
#[derive(Deserialize, JsonSchema)]
struct RunCommand {
    #[serde(flatten)]
    relay: Relay,
}

/// The set's own tool, if the name is one of its relays or its search.
pub(super) fn own(name: &str, every: &Arc<[Command]>, node: &NodeId) -> Option<Command> {
    let (every, node) = (every.clone(), node.clone());
    Some(match name {
        SEARCH => Command::typed(SEARCH, Effect::Reads, move |args: SearchCommands| {
            let found = find(&every, args);
            async move { Ok(found) }
        }),
        READ => Command::typed(READ, Effect::Reads, move |args: ReadCommand| {
            let (every, node) = (every.clone(), node.clone());
            async move { run(&every, &node, true, args.relay).await }
        }),
        RUN => Command::typed(RUN, Effect::Destroys, move |args: RunCommand| {
            let (every, node) = (every.clone(), node.clone());
            async move { run(&every, &node, false, args.relay).await }
        }),
        _ => return None,
    })
}

/// Run the named command as tools/call would, if its effect matches the relay.
async fn run(
    every: &[Command],
    node: &NodeId,
    reads: bool,
    args: Relay,
) -> Result<Map<String, Value>, Error> {
    let name = args.command.as_str();
    let command = every.iter().find(|c| c.name == name).ok_or_else(|| {
        Error::new(
            "unknown_command",
            format!("no command {name} on this node; swarm_search finds them"),
        )
    })?;
    match (reads, command.effect.reads()) {
        (true, false) => {
            return Err(Error::new(
                "not_read_only",
                format!(
                    "{name} {} something: run it with swarm_run",
                    if command.effect == Effect::Destroys {
                        "deletes or overwrites"
                    } else {
                        "changes"
                    }
                ),
            ));
        }
        (false, true) => {
            return Err(Error::new(
                "read_only",
                format!("{name} only reads: run it with swarm_read, which needs no confirmation"),
            ));
        }
        _ => {}
    }
    let mut inner = Value::Object(args.arguments.unwrap_or_default());
    super::target(node, &mut inner)?;
    let outcome = (command.call)(inner).await;
    super::call_log::relayed(
        if reads { READ } else { RUN },
        name,
        node.as_str(),
        if outcome.is_ok() { "ok" } else { "failed" },
    );
    outcome.map(|value| match value {
        Value::Object(fields) => fields,
        // MCP structured results are objects. Preserve the command's value inside one.
        other => Map::from_iter([("result".into(), other)]),
    })
}

/// `swarm_search`: the commands whose name or description holds the
/// query's words, best first; every name with no query.
fn find(every: &[Command], args: SearchCommands) -> CommandsFound {
    let words = words(args.query.as_deref().unwrap_or_default());
    if words.is_empty() {
        return CommandsFound {
            commands: every
                .iter()
                .map(|command| CommandMatch::Name(command.name))
                .collect(),
            hint: Some(
                "Ask again with words for what you want, to see what each does and how to run it.",
            ),
            more: None,
        };
    }
    let full = matches!(args.detail, Some(SearchDetail::Full));
    let mut scored: Vec<(usize, usize, &Command)> = every
        .iter()
        .enumerate()
        .filter_map(|(at, command)| {
            let name = command.name.replace('_', " ");
            let description = command.description.to_lowercase();
            let score: usize = words
                .iter()
                .map(|word| {
                    if found(&name, word) {
                        3
                    } else if found(&description, word) {
                        1
                    } else {
                        0
                    }
                })
                .sum();
            (score > 0).then_some((score, at, command))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let more = scored.len().saturating_sub(FOUND);
    let commands = scored
        .iter()
        .take(FOUND)
        .map(|(_, _, command)| CommandMatch::Match {
            name: command.name,
            does: command.effect.word(),
            run_with: if command.effect.reads() { READ } else { RUN },
            description: command.description.to_string(),
            input_schema: full.then(|| (command.schema)()),
        })
        .collect();
    CommandsFound {
        commands,
        hint: None,
        more: Some(more),
    }
}

/// The words a query looks for: lowercase, without the short ones that
/// every description holds, unless they are all it has.
fn words(query: &str) -> Vec<String> {
    let all: Vec<String> = query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect();
    let long: Vec<String> = all.iter().filter(|w| w.len() > 2).cloned().collect();
    if long.is_empty() { all } else { long }
}

/// Whether `text` holds `word`, or its singular (`missions`, `mission`).
fn found(text: &str, word: &str) -> bool {
    text.contains(word)
        || (word.len() > 3
            && word
                .strip_suffix('s')
                .is_some_and(|stem| text.contains(stem)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn command(name: &'static str, effect: Effect, description: &'static str) -> Command {
        #[derive(Deserialize, JsonSchema)]
        struct Empty {}

        let mut command = Command::typed(name, effect, |_: Empty| async { Ok(json!({})) });
        command.description = description.into();
        command
    }

    fn search(every: &[Command], args: Value) -> Value {
        json!(find(every, serde_json::from_value(args).unwrap()))
    }

    #[test]
    fn commands_are_found_by_their_words_names_first() {
        let every = [
            command("run_status", Effect::Reads, "Read a run and its gates."),
            command(
                "gate_approve",
                Effect::Writes,
                "Approve a pending human gate.",
            ),
            command("mission_list", Effect::Reads, "List the missions."),
        ];
        let found = search(&every, json!({ "query": "approve a gate" }));
        let names: Vec<&str> = found["commands"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["gate_approve", "run_status"]);
        assert_eq!(found["commands"][0]["run_with"], "swarm_run");
        assert_eq!(found["commands"][1]["run_with"], "swarm_read");
        assert!(found["commands"][0].get("input_schema").is_none());
        let full = search(&every, json!({ "query": "missions", "detail": "full" }));
        assert_eq!(full["commands"][0]["name"], "mission_list");
        assert_eq!(full["commands"][0]["input_schema"]["type"], "object");
        let all = search(&every, json!({}));
        assert_eq!(
            all["commands"],
            json!(["run_status", "gate_approve", "mission_list"])
        );
        assert!(serde_json::from_value::<SearchCommands>(json!({ "detail": "most" })).is_err());
    }
}
