//! The node's real list of areas holds together: no two share a name, a
//! frame kind or a command, none takes a reserved kind or one of the
//! binary's own verbs, `ahjo kinds` prints who owns which kinds, and the
//! shipped skill names every command. A scratch node supplies the schemas.

use std::collections::BTreeMap;
#[cfg(unix)]
use std::net::SocketAddr;
use std::process::Command;

#[cfg(unix)]
use ahjo_testkit::{process::Node, tmux::Server};
#[cfg(unix)]
use serde_json::{Value, json};
#[cfg(unix)]
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use ahjo_node_kit::listing;

#[test]
fn the_listed_areas_do_not_conflict() {
    listing::check(ahjo::AREAS).unwrap();
    ahjo_node_kit::mcp::check(ahjo::TOOL_SETS, ahjo::AREAS).unwrap();
    for area in ahjo::AREAS {
        for command in area.commands {
            let noun = command.split('_').next().unwrap();
            assert!(
                !ahjo::VERBS.contains(&noun),
                "{}'s command {command} takes the binary's verb {noun}",
                area.name
            );
        }
    }
}

#[test]
fn ahjo_kinds_prints_the_table_of_the_listed_areas() {
    let out = Command::new(env!("CARGO_BIN_EXE_ahjo"))
        .arg("kinds")
        .output()
        .unwrap();
    assert!(out.status.success());
    let printed = String::from_utf8(out.stdout).unwrap();
    assert_eq!(printed, listing::kinds_table(ahjo::AREAS));
    for area in ahjo::AREAS.iter().filter(|a| !a.kinds.is_empty()) {
        assert!(
            printed
                .lines()
                .any(|l| l.ends_with(&format!("  {}", area.name))),
            "{} has no row in\n{printed}",
            area.name
        );
    }
    assert!(printed.contains("the node's hello"), "{printed}");
}

/// An agent learns Ahjo's commands from `skills/ahjo`: a command the skill
/// does not name is one no agent will find.
#[test]
fn the_ahjo_skill_names_every_command() {
    let skill = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills/ahjo");
    let mut text = std::fs::read_to_string(skill.join("SKILL.md")).unwrap();
    for file in std::fs::read_dir(skill.join("references")).unwrap() {
        text.push_str(&std::fs::read_to_string(file.unwrap().path()).unwrap());
    }
    let commands = ahjo::AREAS
        .iter()
        .flat_map(|area| area.commands.iter())
        .chain(ahjo_node_kit::own::COMMANDS);
    let unnamed: Vec<String> = commands
        .map(|command| {
            let (noun, verb) = command.split_once('_').unwrap();
            format!("`ahjo {noun} {}`", verb.replace('_', "-"))
        })
        .filter(|written| !text.contains(written))
        .collect();
    assert!(
        unnamed.is_empty(),
        "skills/ahjo names no {}: add each to skills/ahjo/references/commands.md, and to \
         SKILL.md what an agent should know about it",
        unnamed.join(", ")
    );
}

// Shared verbs and the domain actions exposed by `ahjo help --all`.
const VERBS: &[&str] = &[
    "list",
    "show",
    "read",
    "create",
    "edit",
    "delete",
    "abort",
    "apply",
    "approve",
    "arrange",
    "copy",
    "diff",
    "export",
    "land",
    "launch",
    "link",
    "move",
    "open",
    "pause",
    "pick",
    "reject",
    "reply",
    "resolve",
    "restart",
    "resume",
    "run",
    "search",
    "send",
    "start",
    "stop",
    "sync",
    "validate",
    "wait",
    "write",
    "edit_hook",
    "create_hook",
    "delete_hook",
    "switch_hook",
    "edit_setting",
    "list_launch_flags",
    "list_folders",
    "list_windows",
    "send_keys",
    "edit_mouse",
    "mark_seen",
    "read_snapshot",
    "search_snapshot",
    "list_collections",
    "list_changes",
    "list_files",
    "read_texts",
    "list_collection",
    "search_paths",
    "create_folder",
    "list_recent",
    "list_images",
    "read_image",
    "stat",
    "show_transfer",
    "list_repositories",
    "list_archives",
    "list_examples",
    "list_history",
    "delete_archive",
    "show_shape",
];

#[test]
fn command_names_use_the_vocabulary_and_each_noun_has_one_owner() {
    let listings = ahjo::AREAS
        .iter()
        .map(|area| (area.name, area.commands))
        .chain([
            ("node", ahjo_node_kit::own::COMMANDS),
            ("tool set", ahjo_node_kit::mcp::SET_TOOLS),
        ]);
    let mut owners = BTreeMap::new();
    let mut found = Vec::new();
    for (area, commands) in listings {
        for &command in commands {
            let Some((noun, verb)) = command.split_once('_') else {
                found.push(format!("{area}: {command} is not noun_verb"));
                continue;
            };
            if !snake_case(command) || retired_word(command) || noun == "formation" {
                found.push(format!("{area}: {command} has an invalid command name"));
            }
            let area_verb =
                area == ahjo_missions::LISTING.name && ahjo_missions::NOTE_VERBS.contains(&verb);
            if !VERBS.contains(&verb) && !area_verb {
                found.push(format!("{area}: {command} uses an unlisted verb {verb}"));
            }
            if let Some(owner) = owners.insert(noun, area)
                && owner != area
            {
                found.push(format!("{noun} is a noun in both {owner} and {area}"));
            }
        }
    }
    assert!(found.is_empty(), "{}", found.join("\n"));
}

fn snake_case(name: &str) -> bool {
    name.starts_with(|ch: char| ch.is_ascii_lowercase())
        && name.split('_').all(|word| {
            !word.is_empty()
                && word
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        })
}

fn retired_word(name: &str) -> bool {
    let retired = [
        "slot",
        "slots",
        "role",
        "roles",
        "character",
        "characters",
        "device",
        "devices",
        "board",
        "boards",
        "flow",
        "flows",
        "place",
        "places",
        "task",
        "tasks",
        "response",
        "responses",
    ];
    name.split('_').any(|word| retired.contains(&word))
        || matches!(
            name,
            "work" | "work_item" | "work_items" | "session_stem" | "home"
        )
}

/// Check declared fields even inside arrays, alternatives, maps and definitions.
#[cfg(unix)]
fn schema_names(at: &str, schema: &Value, found: &mut Vec<String>) {
    if let Some(fields) = schema.as_object() {
        for (key, value) in fields {
            match key.as_str() {
                "properties" | "patternProperties" | "$defs" | "definitions"
                | "dependentSchemas" => {
                    for (name, property) in value.as_object().into_iter().flatten() {
                        let path = format!("{at}.{name}");
                        if key == "properties" && (!snake_case(name) || retired_word(name)) {
                            found.push(format!("{path} is not an allowed property name"));
                        }
                        schema_names(&path, property, found);
                    }
                }
                "items"
                | "additionalItems"
                | "additionalProperties"
                | "contains"
                | "not"
                | "if"
                | "then"
                | "else"
                | "propertyNames"
                | "unevaluatedItems"
                | "unevaluatedProperties" => schema_names(at, value, found),
                "anyOf" | "oneOf" | "allOf" | "prefixItems" => {
                    for branch in value.as_array().into_iter().flatten() {
                        schema_names(at, branch, found);
                    }
                }
                _ => {}
            }
        }
    }
}

/// One JSON-RPC request to `path`: its result.
#[cfg(unix)]
async fn rpc(node: SocketAddr, path: &str, method: &str) -> Value {
    let body = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": {} }).to_string();
    let mut stream = tokio::net::TcpStream::connect(node).await.unwrap();
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {node}\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    let (head, body) = response.split_once("\r\n\r\n").unwrap();
    assert!(head.starts_with("HTTP/1.1 200"), "{path}: {response}");
    let answer: Value = serde_json::from_str(body).unwrap();
    answer["result"].clone()
}

/// What in a JSON schema no client can satisfy or read: an empty `enum`
/// takes no value, a `required` field no property describes, an array
/// without `items`, a type JSON Schema does not have.
#[cfg(unix)]
fn unsatisfiable(at: &str, schema: &Value, found: &mut Vec<String>) {
    const TYPES: &[&str] = &[
        "string", "integer", "number", "boolean", "object", "array", "null",
    ];
    let Some(fields) = schema.as_object() else {
        return found.push(format!("{at} is not a schema"));
    };
    if let Some(choices) = fields.get("enum")
        && choices.as_array().is_none_or(Vec::is_empty)
    {
        found.push(format!("{at} has an empty enum"));
    }
    let types: Vec<&str> = match fields.get("type") {
        Some(Value::String(one)) => vec![one],
        Some(Value::Array(many)) => many.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    };
    for kind in &types {
        if !TYPES.contains(kind) {
            found.push(format!("{at} has the type {kind}"));
        }
    }
    if types.contains(&"array") {
        match fields.get("items") {
            Some(items) => unsatisfiable(&format!("{at}[]"), items, found),
            None => found.push(format!("{at} is an array without items")),
        }
    }
    let properties = fields.get("properties").and_then(Value::as_object);
    if let Some(properties) = properties {
        for (name, property) in properties {
            unsatisfiable(&format!("{at}.{name}"), property, found);
        }
    }
    for required in fields
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let name = required.as_str().unwrap_or_default();
        if !properties.is_some_and(|p| p.contains_key(name)) {
            found.push(format!("{at} requires {name}, which it does not describe"));
        }
    }
    for key in ["anyOf", "oneOf", "allOf"] {
        for (i, branch) in fields
            .get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            unsatisfiable(&format!("{at}.{key}[{i}]"), branch, found);
        }
    }
}

/// Every tool the whole node serves, on `/mcp` and in each tool set, takes
/// arguments a client can give and says whether it changes anything. A
/// fresh node is the hard case: no other users, no folders, no missions.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn every_tool_has_a_satisfiable_schema_and_says_what_it_does() {
    let Some(tmux) = Server::start_in("ahjo-schemas") else {
        return;
    };
    let config = tmux.dir().join("config.toml");
    std::fs::write(
        &config,
        format!(
            "[terminals]\nsocket = {:?}\n[files]\nroots = [{:?}]\n",
            tmux.socket_path(),
            tmux.dir()
        ),
    )
    .unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_ahjo"));
    command
        .args(["serve", "--config"])
        .arg(&config)
        .env("HOME", tmux.dir())
        .env("XDG_CONFIG_HOME", tmux.dir().join("xdg"))
        .env("XDG_DATA_HOME", tmux.dir().join("data"))
        .env("XDG_STATE_HOME", tmux.dir().join("state"));
    let node = Node::start(&mut command, "127.0.0.1:0".parse().unwrap());
    let mut found = Vec::new();
    let paths = std::iter::once("/mcp".to_owned()).chain(
        ahjo::TOOL_SETS
            .iter()
            .map(|set| format!("/mcp/{}", set.name)),
    );
    for path in paths {
        let listed = rpc(node.address, &path, "tools/list").await;
        let tools = listed["tools"].as_array().unwrap();
        assert!(!tools.is_empty(), "{path}");
        for tool in tools {
            let command = tool["name"].as_str().unwrap();
            let name = format!("{path} {command}");
            if !snake_case(command) || retired_word(command) {
                found.push(format!("{name} has an invalid command name"));
            }
            for key in ["inputSchema", "outputSchema"] {
                if let Some(schema) = tool.get(key).filter(|schema| schema.is_object()) {
                    schema_names(&format!("{name} {key}"), schema, &mut found);
                } else {
                    found.push(format!("{name} has no {key}"));
                }
            }
            let schema = &tool["inputSchema"];
            if schema["type"] != "object" {
                found.push(format!("{name} takes no object"));
            }
            unsatisfiable(&name, schema, &mut found);
            if !tool["annotations"]["readOnlyHint"].is_boolean() {
                found.push(format!("{name} does not say whether it only reads"));
            }
            if !tool["title"].as_str().is_some_and(|t| !t.is_empty()) {
                found.push(format!("{name} has no title"));
            }
        }
    }
    assert!(found.is_empty(), "{}", found.join("\n"));

    // A set lists its tools in its order, and the part of its instructions
    // a client may show alone stands alone.
    for set in ahjo::TOOL_SETS {
        let path = format!("/mcp/{}", set.name);
        let listed = rpc(node.address, &path, "tools/list").await;
        let names: Vec<&str> = listed["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, set.tools, "{path}");
        let said = rpc(node.address, &path, "initialize").await;
        let first = said["instructions"]
            .as_str()
            .unwrap()
            .split("\n\n")
            .next()
            .unwrap();
        assert!(first.chars().count() <= 512, "{path}: {first}");
    }
}
