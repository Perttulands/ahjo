//! Tool sets: one MCP server shaped for one kind of client, at
//! `/mcp/<set>`, holding commands chosen from several areas and, if it
//! names them, three tools of its own that reach every other command. A
//! client such as ChatGPT pays for a dozen tools instead of every
//! command's, and loses none of them.
//!
//! The three exist only in a set: on the command line and on `/mcp`
//! every command is already its own verb or tool.
//!
//! - `swarm_commands` finds commands by words, with what each does.
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

use ahjo_core::{Error, NodeId};
use serde_json::{Value, json};

use crate::args::object;
use crate::{Command, Effect, Listing};

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

pub const COMMANDS: &str = "swarm_commands";
pub const READ: &str = "swarm_read";
pub const RUN: &str = "swarm_run";

/// The tools a set may name that are not commands.
pub const SET_TOOLS: &[&str] = &[COMMANDS, READ, RUN];

/// How many commands `swarm_commands` answers with at most.
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

/// The set's own tool `name`, reaching `every` command; `None` for a
/// command's name.
pub(super) fn own(name: &str, every: &Arc<[Command]>, node: &NodeId) -> Option<Command> {
    let every = every.clone();
    Some(match name {
        COMMANDS => Command::new(
            COMMANDS,
            Effect::Reads,
            "Find the Swarm commands this server has no tool of its own for: missions and \
             their runs and gates, scheduled prompts, sessions, files, services, roles, harness \
             settings and more. Use this when the user asks for something none of the other \
             tools does. Answers each command's name, what it does, and whether to run it \
             with swarm_read (it only reads) or swarm_run (it changes something); detail \
             full adds the arguments it takes. With no query, names every command.",
            || {
                object(
                    json!({
                        "query": {
                            "type": "string",
                            "description": "Words for what you want to do, e.g. \"approve gate\" or \"scheduled prompts\".",
                        },
                        "detail": {
                            "type": "string",
                            "enum": ["brief", "full"],
                            "default": "brief",
                            "description": "full adds each command's input schema.",
                        },
                    }),
                    &[],
                )
            },
            move |args: Value| {
                let answer = find(&every, &args);
                async move { answer }
            },
        ),
        READ | RUN => {
            let reads = name == READ;
            let node = node.clone();
            let (name, effect, description) = if reads {
                (
                    READ,
                    Effect::Reads,
                    "Run a Swarm command that only reads, as swarm_commands names it, with its \
                     arguments. Refused for a command that changes anything: run that with \
                     swarm_run.",
                )
            } else {
                (
                    RUN,
                    Effect::Destroys,
                    "Run a Swarm command that changes something (starts, edits, sends, \
                     deletes), as swarm_commands names it, with its arguments. Refused for a \
                     command that only reads: run that with swarm_read.",
                )
            };
            Command::new(
                name,
                effect,
                description,
                || {
                    object(
                        json!({
                            "command": {
                                "type": "string",
                                "description": "Its name, e.g. run_status.",
                            },
                            "arguments": {
                                "type": "object",
                                "description": "Its arguments by name, as swarm_commands with detail full lists them.",
                            },
                        }),
                        &["command"],
                    )
                },
                move |args: Value| {
                    let (every, node) = (every.clone(), node.clone());
                    async move { run(&every, &node, reads, args).await }
                },
            )
        }
        _ => return None,
    })
}

fn invalid(message: impl Into<String>) -> Error {
    Error::new("invalid_argument", message)
}

/// `swarm_read` and `swarm_run`: the command `args` names, run as
/// `tools/call` would run it, if it does what the tool is for.
async fn run(every: &[Command], node: &NodeId, reads: bool, args: Value) -> Result<Value, Error> {
    let name = args
        .get("command")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("command is required, as a string"))?;
    let command = every.iter().find(|c| c.name == name).ok_or_else(|| {
        Error::new(
            "unknown_command",
            format!("no command {name} on this node; swarm_commands finds them"),
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
    let mut inner = match args.get("arguments") {
        None | Some(Value::Null) => json!({}),
        Some(given @ Value::Object(_)) => given.clone(),
        Some(_) => return Err(invalid("arguments is an object of the command's arguments")),
    };
    super::target(node, &mut inner)?;
    let outcome = (command.call)(inner).await;
    super::call_log::relayed(
        if reads { READ } else { RUN },
        name,
        node.as_str(),
        if outcome.is_ok() { "ok" } else { "failed" },
    );
    outcome
}

/// `swarm_commands`: the commands whose name or description holds the
/// query's words, best first; every name with no query.
fn find(every: &[Command], args: &Value) -> Result<Value, Error> {
    let query = match args.get("query") {
        None | Some(Value::Null) => "",
        Some(Value::String(query)) => query.as_str(),
        Some(_) => return Err(invalid("query is a string")),
    };
    let full = match args.get("detail") {
        None | Some(Value::Null) => false,
        Some(Value::String(detail)) if detail == "brief" => false,
        Some(Value::String(detail)) if detail == "full" => true,
        Some(_) => return Err(invalid("detail is brief or full")),
    };
    let words = words(query);
    if words.is_empty() {
        let names: Vec<&str> = every.iter().map(|c| c.name).collect();
        return Ok(json!({
            "commands": names,
            "hint": "Ask again with words for what you want, to see what each does and how to run it.",
        }));
    }
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
    let commands: Vec<Value> = scored
        .iter()
        .take(FOUND)
        .map(|(_, _, command)| {
            let mut found = json!({
                "name": command.name,
                "does": command.effect.word(),
                "run_with": if command.effect.reads() { READ } else { RUN },
                "description": command.description,
            });
            if full {
                found["inputSchema"] = (command.schema)();
            }
            found
        })
        .collect();
    Ok(json!({ "commands": commands, "more": more }))
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

    fn command(name: &'static str, effect: Effect, description: &'static str) -> Command {
        Command::new(
            name,
            effect,
            description,
            || json!({ "type": "object" }),
            |args: Value| async move { Ok(args) },
        )
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
        let found = find(&every, &json!({ "query": "approve a gate" })).unwrap();
        let names: Vec<&str> = found["commands"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["gate_approve", "run_status"]);
        assert_eq!(found["commands"][0]["run_with"], "swarm_run");
        assert_eq!(found["commands"][1]["run_with"], "swarm_read");
        assert!(found["commands"][0].get("inputSchema").is_none());
        let full = find(&every, &json!({ "query": "missions", "detail": "full" })).unwrap();
        assert_eq!(full["commands"][0]["name"], "mission_list");
        assert_eq!(full["commands"][0]["inputSchema"]["type"], "object");
        let all = find(&every, &json!({})).unwrap();
        assert_eq!(
            all["commands"],
            json!(["run_status", "gate_approve", "mission_list"])
        );
        assert!(find(&every, &json!({ "detail": "most" })).is_err());
    }
}
