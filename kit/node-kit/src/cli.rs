//! `ahjo <noun> <verb> [args]`: every command of the node's areas. The
//! command line is a client of the node's MCP tools: it reads `tools/list`
//! for the tool `<noun>_<verb>` and its arguments, and calls it through
//! `tools/call`, so the CLI and MCP cannot drift apart. Required arguments
//! may be given in order; every argument is also `--name value`.

use ahjo_core::Error;
use anyhow::{Context, Result, bail};
use axum::http::Method;
use serde_json::{Map, Value, json};
use std::io::{Read, Write};

use crate::client::NodeClient;

/// The command line itself was wrong: no such command, an unknown flag, a
/// missing argument. Nothing was asked of the node's tool. `ahjo` exits 2
/// on it, and 1 when a command ran and failed.
#[derive(Debug)]
pub struct Usage(pub String);

impl std::fmt::Display for Usage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Usage {}

fn usage_error(message: String) -> anyhow::Error {
    Usage(message).into()
}

/// The node a command line names and what the command line says of it.
struct Reached {
    client: NodeClient,
    tools: Vec<Value>,
    /// The `--node` given, as given.
    named: Option<String>,
    /// The node the tool runs on, when `--node` named an id.
    on: Option<String>,
}

/// The node's commands, from the node `node` names (or this machine's).
async fn reach(node: Option<String>) -> Result<Reached> {
    // An address is the node the command talks to; an id is the node the
    // tool runs on, reached through the one it talks to.
    let (url, on) = match &node {
        Some(node) if crate::client::is_address(node) => (Some(node.clone()), None),
        on => (None, on.clone()),
    };
    let client = NodeClient::new(url.as_deref())?;
    let tools = list(&client).await?;
    Ok(Reached {
        client,
        tools,
        named: node,
        on,
    })
}

/// Runs one command, writing its answer to `out`. The exit code is 1 when
/// the tool failed. The node is the one `--node` among `args` names, else
/// the one this machine runs.
pub async fn run(noun: &str, args: &[String], out: &mut impl Write) -> Result<i32> {
    run_with_files(noun, args, out, &[]).await
}

/// The local binary supplies the command and field pairs that may read local files.
pub async fn run_with_files(
    noun: &str,
    args: &[String],
    out: &mut impl Write,
    local_files: &[(&str, &str)],
) -> Result<i32> {
    let (lead, args) = leading_node(args)?;
    let verb = args
        .first()
        .filter(|v| !v.starts_with('-'))
        .map(String::as_str);
    let rest = match verb {
        Some(_) => &args[1..],
        None => &[][..],
    };
    // Which flags take a value is known only from the node's commands, and
    // which node to ask is a flag: ask the node `--node` seems to name, then
    // read the command line once with each flag's arity, and ask again in
    // the rare case that reading names another.
    let mut node = guess_node(rest).or_else(|| lead.clone());
    let mut again = false;
    let (reached, tool, line, verb) = loop {
        let reached = reach(node.clone()).await?;
        let (Some(tool), Some(verb)) = (find(noun, verb, &reached, out).await?, verb) else {
            return Ok(0);
        };
        let line = read(&tool["inputSchema"], rest).map_err(|error| {
            usage_error(format!("{error:#}\nusage: {}", usage(noun, verb, &tool)))
        })?;
        let named = line.node.clone().or_else(|| lead.clone());
        if named == node {
            break (reached, tool, line, verb);
        }
        if again {
            return Err(usage_error(
                "the command line names its node in a way that reads two ways; \
                 give it as --node=<id|url>"
                    .into(),
            ));
        }
        (node, again) = (named, true);
    };
    let Reached {
        client,
        tools,
        named,
        on,
    } = reached;
    if line.help {
        writeln!(out, "{}", help(noun, verb, &tool))?;
        return Ok(0);
    }
    let json_out = line.json;
    let mut arguments = line.arguments;
    let tool_name = name(&tool).to_owned();
    let allowed = |field: &str| local_files.contains(&(tool_name.as_str(), field));
    read_file_arguments(
        &tool["inputSchema"],
        &mut arguments,
        &mut std::io::stdin(),
        allowed,
    )?;
    if let Some(on) = &on {
        arguments["node"] = Value::String(on.clone());
    }
    let answer = call(&client, &tool_name, arguments).await?;
    let failed = answer["isError"].as_bool().unwrap_or(false);
    let text = answer["content"][0]["text"].as_str().unwrap_or("");
    if failed {
        // A suggested command reaches the node this one was sent to.
        let aim = |text: &str| aimed(text, named.as_deref(), &tools);
        match serde_json::from_str::<Error>(text) {
            Ok(mut error) => {
                error.message = aim(&error.message);
                match json_out {
                    true => eprintln!("{}", json!({ "error": error })),
                    false => eprintln!("ahjo {noun} {verb}: {error}"),
                }
            }
            Err(_) => eprintln!("ahjo {noun} {verb}: {}", aim(text)),
        }
        return Ok(1);
    }
    let mut value: Value =
        serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.into()));
    if let Some(field) = toml_output_field(&tool["outputSchema"]) {
        let document = value[field]
            .as_str()
            .with_context(|| format!("command returned no TOML document in {field:?}"))?;
        write_toml_document(document, json_out, out)?;
        return Ok(0);
    }
    aim(&mut value, named.as_deref(), &tools);
    if json_out {
        writeln!(out, "{}", serde_json::to_string_pretty(&value)?)?;
    } else {
        // Which node answered is the one asked, named or this machine's:
        // said only where it is another than the one named.
        if let Some(fields) = value.as_object_mut()
            && fields
                .get("node")
                .and_then(Value::as_str)
                .is_some_and(|node| on.as_deref().is_none_or(|on| on == node))
        {
            fields.shift_remove("node");
        }
        write!(out, "{}", render(&value))?;
    }
    Ok(0)
}

/// Read CLI file arguments only after parsing has selected a node and ruled out help.
fn read_file_arguments(
    schema: &Value,
    arguments: &mut Value,
    stdin: &mut impl Read,
    allowed: impl Fn(&str) -> bool,
) -> Result<()> {
    for (key, spec) in properties(schema) {
        if spec["x-ahjo-cli-file"] != true || !allowed(&key) {
            continue;
        }
        let Some(value) = arguments.get_mut(&key) else {
            continue;
        };
        let path = value
            .as_str()
            .with_context(|| format!("{key} must name a file"))?;
        let document = if path == "-" {
            let mut document = String::new();
            stdin
                .read_to_string(&mut document)
                .context("reading standard input")?;
            document
        } else {
            std::fs::read_to_string(path).with_context(|| format!("reading {path:?}"))?
        };
        *value = Value::String(document);
    }
    Ok(())
}

fn write_toml_document(document: &str, json_out: bool, out: &mut impl Write) -> Result<()> {
    if json_out {
        let parsed: toml::Value =
            toml::from_str(document).context("command returned invalid TOML")?;
        writeln!(out, "{}", serde_json::to_string_pretty(&parsed)?)?;
    } else {
        out.write_all(document.as_bytes())?;
    }
    Ok(())
}

fn toml_output_field(schema: &Value) -> Option<&str> {
    schema["properties"]
        .as_object()?
        .iter()
        .find_map(|(name, spec)| (spec["x-ahjo-cli-toml"] == true).then_some(name.as_str()))
}

/// The tool `ahjo <noun> <verb>` names on the node reached. Without a verb
/// it writes the noun's verbs to `out` and answers `None`; a noun or verb
/// the node does not have is a usage error saying what it has.
async fn find(
    noun: &str,
    verb: Option<&str>,
    reached: &Reached,
    out: &mut impl Write,
) -> Result<Option<Value>> {
    let Reached {
        client,
        tools,
        named,
        ..
    } = reached;
    let ours: Vec<&Value> = tools
        .iter()
        .filter(|t| {
            name(t)
                .strip_prefix(noun)
                .is_some_and(|v| v.starts_with('_'))
        })
        .collect();
    if ours.is_empty() {
        // An area the node turned off has no commands; say which are off.
        let off = match client.request(Method::GET, "/api/health", None, None).await {
            Ok((health, _)) => health["off"]
                .as_array()
                .map(|off| {
                    off.iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default(),
            Err(_) => String::new(),
        };
        let off = match off.as_str() {
            "" => String::new(),
            off => format!("; turned off on this node: {off}"),
        };
        let known = nouns(tools);
        let nearest = match nearest(noun, known.iter().map(String::as_str)) {
            Some(near) => format!(" (did you mean {near}?)"),
            None => String::new(),
        };
        return Err(usage_error(aimed(
            &format!(
                "no command {noun}{nearest}; this node's commands are {}{off}",
                known.join(", ")
            ),
            named.as_deref(),
            tools,
        )));
    }
    let Some(verb) = verb else {
        writeln!(out, "{}", verbs(noun, &ours))?;
        return Ok(None);
    };
    let tool_name = format!("{noun}_{}", verb.replace('-', "_"));
    let Some(tool) = ours.iter().find(|t| name(t) == tool_name) else {
        let names: Vec<String> = ours.iter().map(|t| verb_of(noun, t)).collect();
        let nearest = match nearest(verb, names.iter().map(String::as_str)) {
            Some(near) => format!(" (did you mean ahjo {noun} {near}?)"),
            None => String::new(),
        };
        return Err(usage_error(aimed(
            &format!("no command {noun} {verb}{nearest}; ahjo {noun} lists its verbs"),
            named.as_deref(),
            tools,
        )));
    };
    Ok(Some((*tool).clone()))
}

/// The commands, one line each, for `ahjo help --all`.
pub async fn overview(node: Option<&str>) -> Result<String> {
    Ok(listing(&list(&NodeClient::new(node)?).await?))
}

/// The nouns, each on one line with its verbs, for `ahjo help`.
pub async fn index(node: Option<&str>) -> Result<String> {
    Ok(noun_index(&list(&NodeClient::new(node)?).await?))
}

/// One line per noun: the noun, then its verbs, nouns in the order the
/// node first names them.
fn noun_index(tools: &[Value]) -> String {
    let mut nouns: Vec<(&str, Vec<String>)> = Vec::new();
    for t in tools {
        let (noun, verb) = name(t).split_once('_').unwrap_or((name(t), ""));
        let verb = verb.replace('_', "-");
        match nouns.iter_mut().find(|(n, _)| *n == noun) {
            Some((_, verbs)) => verbs.push(verb),
            None => nouns.push((noun, vec![verb])),
        }
    }
    let width = nouns.iter().map(|(n, _)| n.len()).max().unwrap_or(0);
    nouns
        .iter()
        .map(|(noun, verbs)| format!("  {noun:width$}  {}", verbs.join(" ")))
        .collect::<Vec<_>>()
        .join("\n")
}

/// One line per command, each noun's commands together, nouns in the order
/// the node first names them.
fn listing(tools: &[Value]) -> String {
    let mut nouns: Vec<(&str, Vec<(String, &str)>)> = Vec::new();
    for t in tools {
        let (noun, verb) = name(t).split_once('_').unwrap_or((name(t), ""));
        let command = format!("ahjo {noun} {}", verb.replace('_', "-"));
        match nouns.iter_mut().find(|(n, _)| *n == noun) {
            Some((_, lines)) => lines.push((command, first_line(t))),
            None => nouns.push((noun, vec![(command, first_line(t))])),
        }
    }
    // What each command does starts in one column within its noun, and a
    // blank line parts the nouns.
    nouns
        .into_iter()
        .map(|(_, lines)| {
            let width = lines.iter().map(|(c, _)| c.len()).max().unwrap_or(0);
            lines
                .iter()
                .map(|(command, does)| format!("  {command:width$}  {does}"))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Every string in `value` with its suggested commands aimed at `node`.
fn aim(value: &mut Value, node: Option<&str>, tools: &[Value]) {
    match value {
        Value::String(text) => *text = aimed(text, node, tools),
        Value::Array(items) => items.iter_mut().for_each(|v| aim(v, node, tools)),
        Value::Object(fields) => fields.values_mut().for_each(|v| aim(v, node, tools)),
        _ => {}
    }
}

/// `text` with `--node <node>` after each `ahjo <noun> <verb>` it suggests
/// (or `ahjo <noun>`), so following a suggestion reaches the node the
/// command named. Without a named node the text is as it was.
fn aimed(text: &str, node: Option<&str>, tools: &[Value]) -> String {
    let Some(node) = node else {
        return text.to_owned();
    };
    let flag = if node
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"_-.:/".contains(&b))
    {
        format!(" --node {node}")
    } else {
        format!(" --node '{}'", node.replace('\'', "'\\''"))
    };
    let known = nouns(tools);
    let word = |s: &str| {
        s.find(|c: char| !(c.is_ascii_lowercase() || c == '-' || c == '_'))
            .unwrap_or(s.len())
    };
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("ahjo ") {
        let starts = rest[..at]
            .chars()
            .next_back()
            .is_none_or(|c| !(c.is_alphanumeric() || c == '_' || c == '-'));
        let after = &rest[at + 5..];
        let noun = &after[..word(after)];
        let mut end = None;
        if starts && known.iter().any(|k| k == noun) {
            end = Some(at + 5 + noun.len());
            if let Some(tail) = after[noun.len()..].strip_prefix(' ') {
                let verb = &tail[..word(tail)];
                let tool = format!("{noun}_{}", verb.replace('-', "_"));
                if !verb.is_empty() && tools.iter().any(|t| name(t) == tool) {
                    end = Some(at + 5 + noun.len() + 1 + verb.len());
                }
            }
        }
        match end {
            Some(end) => {
                out.push_str(&rest[..end]);
                out.push_str(&flag);
                rest = &rest[end..];
            }
            None => {
                out.push_str(&rest[..at + 5]);
                rest = &rest[at + 5..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// `--node` given before the verb, as `ahjo session --node lab list`, and
/// the words from the verb on.
fn leading_node(args: &[String]) -> Result<(Option<String>, &[String])> {
    let mut node = None;
    let mut at = 0;
    while let Some(a) = args.get(at) {
        if a == "--node" {
            let value = args
                .get(at + 1)
                .ok_or_else(|| usage_error("--node needs a node id or URL".into()))?;
            node = Some(value.clone());
            at += 2;
        } else if let Some(v) = a.strip_prefix("--node=") {
            node = Some(v.to_owned());
            at += 1;
        } else {
            break;
        }
    }
    Ok((node, &args[at..]))
}

/// The node `--node` seems to name before the command's flags are known: a
/// `--node` followed by a word that is not itself a flag. Only which node to
/// ask for the commands; `read` decides.
fn guess_node(args: &[String]) -> Option<String> {
    let mut node = None;
    let mut it = args.iter().peekable();
    while let Some(a) = it.next() {
        if a == "--" {
            break;
        }
        if a == "--node" {
            if let Some(value) = it.next_if(|v| !v.starts_with('-')) {
                node = Some(value.clone());
            }
        } else if let Some(v) = a.strip_prefix("--node=") {
            node = Some(v.to_owned());
        }
    }
    node
}

async fn rpc(client: &NodeClient, method: &str, params: Value) -> Result<Value> {
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
    let (answer, _) = client
        .request(Method::POST, "/mcp", Some(body), None)
        .await?;
    if let Some(error) = answer.get("error") {
        bail!(
            "{}",
            error["message"].as_str().unwrap_or("the node refused")
        );
    }
    Ok(answer["result"].clone())
}

async fn list(client: &NodeClient) -> Result<Vec<Value>> {
    let result = rpc(client, "tools/list", json!({})).await?;
    Ok(result["tools"].as_array().cloned().unwrap_or_default())
}

async fn call(client: &NodeClient, tool: &str, arguments: Value) -> Result<Value> {
    rpc(
        client,
        "tools/call",
        json!({"name": tool, "arguments": arguments}),
    )
    .await
}

fn name(tool: &Value) -> &str {
    tool["name"].as_str().unwrap_or("")
}

fn first_line(tool: &Value) -> &str {
    tool["description"]
        .as_str()
        .unwrap_or("")
        .split(". ")
        .next()
        .unwrap_or("")
        .trim_end_matches('.')
}

fn nouns(tools: &[Value]) -> Vec<String> {
    let mut nouns: Vec<String> = tools
        .iter()
        .filter_map(|t| name(t).split_once('_').map(|(n, _)| n.to_owned()))
        .collect();
    let mut seen = std::collections::HashSet::new();
    nouns.retain(|noun| seen.insert(noun.clone()));
    nouns
}

fn verb_of(noun: &str, tool: &Value) -> String {
    name(tool)[noun.len() + 1..].replace('_', "-")
}

fn verbs(noun: &str, tools: &[&Value]) -> String {
    tools
        .iter()
        .map(|t| {
            let verb = verb_of(noun, t);
            format!("  {}\n      {}", usage(noun, &verb, t), first_line(t))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Words that ask for the same thing, as a caller from another tool might
/// say it: the first of a group the noun has answers for any of them.
const ALIKE: &[&[&str]] = &[
    &[
        "show", "inspect", "status", "read", "get", "view", "info", "cat",
    ],
    &["list", "ls", "all"],
    &["delete", "remove", "rm", "kill", "retire"],
    &["create", "new", "add", "make"],
    &["edit", "update", "set", "change"],
    &["run", "start", "launch"],
    &["search", "find", "grep", "query", "locate"],
];

/// The known word `typed` most likely meant: one asking for the same
/// thing, one it begins or that begins it, or one a slip or two away.
fn nearest<'a>(typed: &str, known: impl Iterator<Item = &'a str> + Clone) -> Option<&'a str> {
    let typed = typed.to_lowercase();
    let alike = ALIKE
        .iter()
        .filter(|group| group.contains(&typed.as_str()))
        .flat_map(|group| group.iter())
        .find_map(|word| known.clone().find(|k| k == word));
    if alike.is_some() {
        return alike;
    }
    let mut prefixed = known
        .clone()
        .filter(|k| typed.len() >= 2 && (k.starts_with(&typed) || typed.starts_with(*k)));
    if let (Some(only), None) = (prefixed.next(), prefixed.next()) {
        return Some(only);
    }
    known
        .map(|k| (distance(&typed, k), k))
        .filter(|(d, k)| *d <= 2 && *d < k.len().max(typed.len()) / 2 + 1)
        .min_by_key(|(d, _)| *d)
        .map(|(_, k)| k)
}

/// Edits (insert, delete, replace) that turn `a` into `b`.
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = (above + 1)
                .min(row[j] + 1)
                .min(diagonal + usize::from(ca != *cb));
            diagonal = above;
        }
    }
    row[b.len()]
}

/// `ahjo noun verb <required>... [--optional value]...`
fn usage(noun: &str, verb: &str, tool: &Value) -> String {
    let schema = &tool["inputSchema"];
    let mut line = format!("ahjo {noun} {verb}");
    for r in required(schema) {
        line.push_str(&format!(" <{r}>"));
    }
    let lone = lone(schema);
    if let Some(lone) = &lone {
        line.push_str(&format!(" [<{lone}>]"));
    }
    for (key, spec) in properties(schema) {
        if required(schema).contains(&key.as_str()) || lone.as_ref() == Some(&key) {
            continue;
        }
        let flag = key.replace('_', "-");
        match schema_type(&spec) {
            Some("boolean") => line.push_str(&format!(" [--{flag}|--no-{flag}]")),
            _ => line.push_str(&format!(" [--{flag} {key}]")),
        }
    }
    line + " [--json] [--node id|url]"
}

fn help(noun: &str, verb: &str, tool: &Value) -> String {
    let mut text = format!(
        "usage: {}\n{}\n",
        usage(noun, verb, tool),
        tool["inputSchema"]["description"]
            .as_str()
            .or_else(|| tool["description"].as_str())
            .unwrap_or("")
    );
    // The required arguments first, in the order they are given without
    // their flags, then the rest.
    let schema = &tool["inputSchema"];
    let needed = required(schema);
    let mut all = properties(schema);
    all.sort_by_key(|(key, _)| needed.iter().position(|r| r == key).unwrap_or(needed.len()));
    let lone = lone(schema);
    let label = |key: &str| match needed.contains(&key) || lone.as_deref() == Some(key) {
        true => format!("<{key}>"),
        false => format!("--{}", key.replace('_', "-")),
    };
    let width = all.iter().map(|(k, _)| label(k).len()).max().unwrap_or(0);
    for (key, spec) in all {
        let mut line = format!(
            "\n  {:width$}  {}",
            label(&key),
            schema_type(&spec).unwrap_or("value")
        );
        if let Some(d) = spec["description"].as_str() {
            line.push_str(&format!(": {d}"));
        }
        if let Some(values) = spec["enum"].as_array() {
            let choices: Vec<_> = values
                .iter()
                .filter(|value| !value.is_null())
                .map(Value::to_string)
                .collect();
            if !choices.is_empty() {
                line.push_str(&format!(" (one of {})", choices.join(", ")));
            }
        }
        if let Some(d) = spec.get("default") {
            line.push_str(&format!(" (default {d})"));
        }
        // An object's keys, each under it, as the tool's schema names them.
        if let Some(keys) = spec["properties"].as_object() {
            let width = keys.keys().map(String::len).max().unwrap_or(0);
            for (key, inner) in keys {
                line.push_str(&format!(
                    "\n      {key:width$}  {}",
                    schema_type(inner).unwrap_or("value")
                ));
                if let Some(d) = inner["description"].as_str() {
                    line.push_str(&format!(": {d}"));
                }
            }
        }
        text.push_str(&line);
    }
    text
}

/// The tool's arguments but `node`, which is the command line's `--node`.
fn properties(schema: &Value) -> Vec<(String, Value)> {
    schema["properties"]
        .as_object()
        .map(|p| {
            p.iter()
                .filter(|(k, _)| *k != "node")
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        })
        .unwrap_or_default()
}

/// A command's one argument, when it is optional text: given without its
/// flag too, as `ahjo file list <path>`, since nothing else could be meant.
fn lone(schema: &Value) -> Option<String> {
    let properties = properties(schema);
    if let Some((key, _)) = properties.iter().find(|(key, spec)| {
        spec["x-ahjo-cli-positional"] == true && !required(schema).contains(&key.as_str())
    }) {
        return Some(key.clone());
    }
    match properties.as_slice() {
        [(key, spec)] if required(schema).is_empty() && schema_type(spec) == Some("string") => {
            Some(key.clone())
        }
        _ => None,
    }
}

fn required(schema: &Value) -> Vec<&str> {
    schema["required"]
        .as_array()
        .map(|r| r.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

/// What a command line says after its verb.
#[derive(Debug, Default)]
struct Line {
    /// The tool's arguments, typed by its schema.
    arguments: Value,
    /// `--node`: the node to ask.
    node: Option<String>,
    /// `--json`: the answer as JSON.
    json: bool,
    /// `--help` or `-h` where a flag stands: the command's help, not a call.
    help: bool,
}

/// The command line after the verb, read once, left to right, with each
/// flag's arity from the tool's schema and the command line's own flags
/// (`--node <node>`, `--json`, `--help`). A flag that takes a value takes the
/// next word whatever it looks like, so `--text --json --new` is the text
/// `--json` and the flag `--new`; `--text=--json` says the same. After `--`
/// every word is a value. Help asked for where a flag stands is answered
/// even when the rest is wrong.
fn read(schema: &Value, args: &[String]) -> Result<Line> {
    let props = properties(schema);
    let spec = |key: &str| props.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
    let mut line = Line::default();
    let mut out = Map::new();
    let mut positional = Vec::new();
    // The first thing wrong, kept while the rest is read for `--help`.
    let mut wrong: Option<anyhow::Error> = None;
    // Words taken for unknown flags, with the number of values before each.
    let mut stray = Vec::new();
    let mut options = true;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if !options {
            positional.push(a.clone());
            continue;
        }
        if a == "--" {
            options = false;
            continue;
        }
        if a == "-h" {
            line.help = true;
            continue;
        }
        let Some(flag) = a.strip_prefix("--") else {
            positional.push(a.clone());
            continue;
        };
        let (key, inline) = match flag.split_once('=') {
            Some((k, v)) => (k.to_owned(), Some(v.to_owned())),
            None => (flag.to_owned(), None),
        };
        let key = key.replace('-', "_");
        let mut value_of = |key: &str| -> Result<String> {
            match &inline {
                Some(v) => Ok(v.clone()),
                None => it
                    .next()
                    .cloned()
                    .with_context(|| format!("--{key} needs a value")),
            }
        };
        let step = match key.as_str() {
            "help" if inline.is_none() => {
                line.help = true;
                Ok(())
            }
            "json" => match &inline {
                None => {
                    line.json = true;
                    Ok(())
                }
                Some(raw) => typed("json", &json!({"type": "boolean"}), raw)
                    .map(|v| line.json = v == Value::Bool(true)),
            },
            "node" => value_of("node").and_then(|node| match node.is_empty() {
                true => bail!("--node needs a node id or URL"),
                false => {
                    line.node = Some(node);
                    Ok(())
                }
            }),
            _ => {
                if let Some(negated) = key.strip_prefix("no_")
                    && spec(negated).is_some_and(|s| schema_type(&s) == Some("boolean"))
                    && inline.is_none()
                {
                    out.insert(negated.to_owned(), Value::Bool(false));
                    continue;
                }
                let Some(s) = spec(&key) else {
                    stray.push((a, positional.len()));
                    continue;
                };
                let raw = match schema_type(&s) {
                    Some("boolean") if inline.is_none() => Ok("true".to_owned()),
                    _ => value_of(&key),
                };
                raw.and_then(|raw| typed(&key, &s, &raw)).map(|value| {
                    match (schema_type(&s), out.get_mut(&key)) {
                        (Some("array"), Some(Value::Array(items))) => {
                            items.extend(value.as_array().cloned().unwrap_or_default())
                        }
                        _ => {
                            out.insert(key, value);
                        }
                    }
                })
            }
        };
        if let Err(error) = step {
            wrong.get_or_insert(error);
        }
    }
    if line.help {
        return Ok(line);
    }
    if !stray.is_empty() {
        let unfixed = required(schema)
            .into_iter()
            .map(str::to_owned)
            .chain(lone(schema))
            .filter(|r| !out.contains_key(r))
            .collect::<Vec<_>>();
        let hints = stray
            .into_iter()
            .map(|(word, before)| {
                unknown_flag(word, &props, unfixed.get(before).map(String::as_str))
            })
            .collect::<Vec<_>>()
            .join("; ");
        let flags = props
            .iter()
            .map(|(key, _)| format!("--{}", key.replace('_', "-")))
            .collect::<Vec<_>>();
        let accepted = if flags.is_empty() {
            "none".into()
        } else {
            flags.join(", ")
        };
        bail!("{hints}; accepted fields: {accepted}");
    }
    if let Some(wrong) = wrong {
        return Err(wrong);
    }
    let mut waiting = required(schema)
        .into_iter()
        .map(str::to_owned)
        .chain(lone(schema))
        .filter(|r| !out.contains_key(r))
        .collect::<Vec<_>>()
        .into_iter()
        .peekable();
    // The last required argument, if it is a list, takes every value left.
    let mut list: Option<String> = None;
    for p in positional {
        if let Some(key) = &list {
            if let Some(Value::Array(items)) = out.get_mut(key) {
                let element = spec(key).unwrap_or(Value::Null);
                items.push(typed(key, &element["items"], &p)?);
            }
            continue;
        }
        let key = waiting
            .next()
            .with_context(|| format!("unexpected argument {p:?}"))?;
        let s = spec(&key).unwrap_or(Value::Null);
        if schema_type(&s) == Some("array") && waiting.peek().is_none() {
            list = Some(key.clone());
        }
        out.insert(key.clone(), typed(&key, &s, &p)?);
    }
    if let Some(missing) = required(schema)
        .into_iter()
        .find(|key| !out.contains_key(*key))
    {
        bail!("missing <{missing}>");
    }
    line.arguments = Value::Object(out);
    Ok(line)
}

/// Why `word`, read where a flag stands, is not one: the flag it was likely
/// meant as, or, when it reads as a value (front matter, a dash, text), how
/// to give it as the value `slot` it would have been.
fn unknown_flag(word: &str, props: &[(String, Value)], slot: Option<&str>) -> String {
    let flag_like = word[2..].split('=').next().is_some_and(|k| {
        k.starts_with(|c: char| c.is_ascii_alphanumeric())
            && k.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
    });
    let shown: String = match word.lines().next().unwrap_or("") {
        first if first.chars().count() > 30 || first.len() < word.len() => {
            format!("{}…", first.chars().take(30).collect::<String>())
        }
        first => first.to_owned(),
    };
    let as_value = slot.map(|slot| {
        format!(
            "a value that starts with - goes after -- or as --{}=<value>",
            slot.replace('_', "-")
        )
    });
    if flag_like {
        let key = word[2..].split('=').next().unwrap_or("").replace('-', "_");
        if let Some(near) = nearest(&key, props.iter().map(|(key, _)| key.as_str())) {
            return format!(
                "unknown flag {shown} (did you mean --{}?)",
                near.replace('_', "-")
            );
        }
        return match as_value {
            Some(hint) => format!("unknown flag {shown}; {hint}"),
            None => format!("unknown flag {shown}"),
        };
    }
    match as_value {
        Some(hint) => format!("{shown:?} reads as a flag; {hint}"),
        None => format!("{shown:?} reads as a flag; a value that starts with - goes after --"),
    }
}

/// The CLI supplies one non-null value at a time. Derived schemas may
/// include null alongside the type to represent an optional field.
fn schema_type(spec: &Value) -> Option<&str> {
    let kind = &spec["type"];
    if let Some(alternatives) = kind.as_array() {
        let mut non_null = alternatives
            .iter()
            .filter_map(Value::as_str)
            .filter(|kind| *kind != "null");
        return non_null.next().filter(|_| non_null.next().is_none());
    }
    kind.as_str()
}

fn typed(key: &str, spec: &Value, raw: &str) -> Result<Value> {
    Ok(match schema_type(spec) {
        Some("boolean") => Value::Bool(match raw {
            "true" | "yes" | "1" => true,
            "false" | "no" | "0" => false,
            _ => bail!("--{key} is true or false"),
        }),
        Some("integer") => json!(
            raw.parse::<i64>()
                .with_context(|| format!("--{key} is a whole number"))?
        ),
        Some("number") => json!(
            raw.parse::<f64>()
                .with_context(|| format!("--{key} is a number"))?
        ),
        Some("array") => json!([typed(key, &spec["items"], raw)?]),
        Some("object") => serde_json::from_str(raw).with_context(|| format!("--{key} is JSON"))?,
        _ => Value::String(raw.to_owned()),
    })
}

/// A readable answer. An object is `key: value` lines, with a nested value
/// indented under its key. A list of objects is one row per item, its plain
/// fields as tab-separated `key=value`, and whatever an item nests indented
/// under its row. Text of several lines follows its key as it is. Nulls and
/// empty values are left out. Epoch milliseconds in timestamp fields are
/// rendered in local time. A list the answer is about (a top-level one) that
/// is empty reads `key: none`, so an answer with nothing in it still says
/// what there is none of. `--json` prints the value itself instead.
fn render(value: &Value) -> String {
    let mut text = String::new();
    render_value(value, 0, &mut text);
    if value.as_array().is_some_and(Vec::is_empty) {
        text.push_str("none\n");
    }
    text
}

/// Nulls and empty values at any depth do not need a heading of their own.
fn has_content(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::String(s) => !s.trim().is_empty(),
        Value::Array(items) => items.iter().any(has_content),
        Value::Object(fields) => fields.values().any(has_content),
        Value::Bool(_) | Value::Number(_) => true,
    }
}

/// A value that fits on its key's line: not nested, and one line of text.
fn plain(value: &Value) -> bool {
    match value {
        Value::Array(_) | Value::Object(_) => false,
        Value::String(s) => s.trim_end().lines().count() <= 1,
        _ => true,
    }
}

fn scalar(value: &Value) -> String {
    match value {
        Value::String(s) => s.trim_end().to_owned(),
        _ => value.to_string(),
    }
}

/// Render timestamp fields without changing the value sent to JSON clients.
fn field_scalar(key: &str, value: &Value) -> String {
    let timed = matches!(
        key,
        "at" | "started" | "updated" | "modified" | "synced" | "last_output"
    ) || key.ends_with("_at");
    // Below this a count of milliseconds would be a day in 1973: the field
    // counts seconds.
    const SECONDS_BELOW: i64 = 100_000_000_000;
    if timed
        && let Some(count) = value.as_i64()
        && let Ok(at) = match count.abs() < SECONDS_BELOW {
            true => jiff::Timestamp::from_second(count),
            false => jiff::Timestamp::from_millisecond(count),
        }
    {
        let local = at.to_zoned(jiff::tz::TimeZone::system());
        return format!("{}{}", local.datetime(), local.offset());
    }
    scalar(value)
}

fn pad(indent: usize, out: &mut String) {
    out.push_str(&" ".repeat(indent));
}

fn render_value(value: &Value, indent: usize, out: &mut String) {
    match value {
        Value::Object(fields) => {
            for (key, value) in fields {
                if has_content(value) {
                    render_field(key, value, indent, out);
                } else if indent == 0 && value.as_array().is_some_and(Vec::is_empty) {
                    out.push_str(&format!("{key}: none\n"));
                }
            }
        }
        Value::Array(items) => {
            for item in items.iter().filter(|v| has_content(v)) {
                pad(indent, out);
                match item {
                    Value::Object(fields) => {
                        let (row, nested): (Vec<_>, Vec<_>) = fields
                            .iter()
                            .filter(|(_, v)| has_content(v))
                            .partition(|(_, v)| plain(v));
                        let row: Vec<String> = row
                            .iter()
                            .map(|(key, value)| format!("{key}={}", field_scalar(key, value)))
                            .collect();
                        out.push_str(if row.is_empty() { "-" } else { "" });
                        out.push_str(&row.join("\t"));
                        out.push('\n');
                        for (key, value) in nested {
                            render_field(key, value, indent + 2, out);
                        }
                    }
                    Value::Array(_) => {
                        out.push_str("-\n");
                        render_value(item, indent + 2, out);
                    }
                    _ => {
                        out.push_str("- ");
                        render_text(&scalar(item), indent + 2, out);
                    }
                }
            }
        }
        _ if has_content(value) => {
            pad(indent, out);
            render_text(&scalar(value), indent, out);
        }
        _ => {}
    }
}

/// One field of an object at `indent`.
fn render_field(key: &str, value: &Value, indent: usize, out: &mut String) {
    pad(indent, out);
    out.push_str(key);
    if plain(value) {
        out.push_str(": ");
        out.push_str(&field_scalar(key, value));
        out.push('\n');
    } else if value.is_string() {
        // Text of several lines (a diff, a file) stays as it was written,
        // so its lines can be read and matched as they are.
        out.push_str(":\n");
        for line in scalar(value).lines() {
            pad(indent, out);
            out.push_str(line);
            out.push('\n');
        }
    } else {
        out.push_str(":\n");
        render_value(value, indent + 2, out);
    }
}

/// Text whose first line follows what is already written and whose other
/// lines start at `indent`.
fn render_text(text: &str, indent: usize, out: &mut String) {
    let mut lines = text.lines();
    if let Some(first) = lines.next() {
        out.push_str(first);
        out.push('\n');
    }
    for line in lines {
        pad(indent, out);
        out.push_str(line);
        out.push('\n');
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(schema: &Value, args: &[String]) -> Result<Value> {
        read(schema, args).map(|line| line.arguments)
    }

    #[test]
    fn help_lists_each_nouns_commands_together() {
        let tools: Vec<Value> = ["mission_list", "run_list", "mission_create", "run_wait"]
            .iter()
            .map(|n| json!({"name": n, "description": "Does it."}))
            .collect();
        let listed = listing(&tools);
        assert_eq!(listed.matches("\n\n").count(), 1, "{listed}");
        let verbs: Vec<String> = listed
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| l.split("  ").nth(1).unwrap_or("").trim().to_owned())
            .collect();
        assert_eq!(
            verbs,
            [
                "ahjo mission list",
                "ahjo mission create",
                "ahjo run list",
                "ahjo run wait"
            ]
        );
    }

    #[test]
    fn help_names_each_noun_once_with_its_verbs() {
        let tools: Vec<Value> = [
            "session_list",
            "harness_hook_add",
            "session_read",
            "harness_show",
        ]
        .iter()
        .map(|n| json!({"name": n, "description": "Does it."}))
        .collect();
        assert_eq!(
            noun_index(&tools),
            "  session  list read\n  harness  hook-add show"
        );
    }

    #[test]
    fn a_mistyped_word_finds_the_one_meant_or_none() {
        let verbs = ["list", "inspect", "status", "history", "hook-add"];
        let near = |typed| nearest(typed, verbs.iter().copied());
        assert_eq!(near("show"), Some("inspect"), "asks for the same");
        assert_eq!(near("lst"), Some("list"), "a slip");
        assert_eq!(near("hist"), Some("history"), "the start of it");
        assert_eq!(near("hook_add"), Some("hook-add"));
        assert_eq!(near("nonsense"), None);
        assert_eq!(near("x"), None);
        let nouns = ["session", "file", "scheduled"];
        assert_eq!(nearest("sessions", nouns.iter().copied()), Some("session"));
    }

    #[test]
    fn a_lone_optional_argument_may_go_without_its_flag() {
        let one = json!({"type": "object", "properties": {"path": {"type": "string"}}});
        assert_eq!(
            parse(&one, &args(&["/srv"])).unwrap(),
            json!({"path": "/srv"})
        );
        assert_eq!(parse(&one, &args(&[])).unwrap(), json!({}));
        assert!(parse(&one, &args(&["/srv", "/tmp"])).is_err());
        let tool = json!({"name": "file_list", "inputSchema": one});
        assert_eq!(
            usage("file", "list", &tool),
            "ahjo file list [<path>] [--json] [--node id|url]"
        );
        let two = json!({"type": "object", "properties": {
            "path": {"type": "string"}, "all": {"type": "boolean"}}});
        assert!(parse(&two, &args(&["/srv"])).is_err(), "two could be meant");
    }

    #[test]
    fn a_suggested_command_reaches_the_node_that_was_named() {
        let tools = [
            json!({"name": "session_list"}),
            json!({"name": "mission_run"}),
            json!({"name": "hook_add"}),
        ];
        let said = "no session named x; ahjo session list shows them";
        assert_eq!(aimed(said, None, &tools), said, "no --node, no change");
        assert_eq!(
            aimed(said, Some("http://127.0.0.1:7399"), &tools),
            "no session named x; ahjo session list --node http://127.0.0.1:7399 shows them"
        );
        assert_eq!(
            aimed("ahjo mission run demo --inputs '{}'", Some("lab"), &tools),
            "ahjo mission run --node lab demo --inputs '{}'"
        );
        assert_eq!(
            aimed("`ahjo hook add` or ahjo session", Some("lab"), &tools),
            "`ahjo hook add --node lab` or ahjo session --node lab"
        );
        let untouched = "ahjo serve, or ahjo nouns, or myahjo session list";
        assert_eq!(aimed(untouched, Some("lab"), &tools), untouched);
        let mut answer = json!({"next": ["ahjo session list"], "valid": true});
        aim(&mut answer, Some("lab"), &tools);
        assert_eq!(answer["next"][0], "ahjo session list --node lab");
    }

    #[test]
    fn an_objects_keys_are_listed_under_it() {
        let tool = json!({"name": "role_edit", "inputSchema": {"type": "object",
            "properties": {"change": {"type": "object", "properties": {
                "summary": {"type": "string"},
                "retire": {"type": "boolean", "description": "Retire it."}}}}}});
        assert!(
            help("role", "edit", &tool).ends_with(
                "  --change  object\n      summary  string\n      retire   boolean: Retire it."
            ),
            "{}",
            help("role", "edit", &tool)
        );
    }

    #[test]
    fn an_empty_choice_is_not_listed() {
        let tool = json!({"name": "session_launch", "inputSchema": {"type": "object",
            "properties": {"user": {"type": "string", "enum": []}}}});
        assert!(!help("session", "launch", &tool).contains("one of"));
    }

    fn schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "session": {"type": "string"},
                "text": {"type": "string"},
                "enter": {"type": "boolean", "default": true},
                "pane": {"type": "integer"},
            },
            "required": ["session", "text"],
        })
    }

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn explicit_optional_positional_works_with_other_flags_and_required_arguments() {
        let export = json!({"type":"object","properties":{
            "mission":{"type":["string","null"],"x-ahjo-cli-positional":true},
            "example":{"type":["string","null"]}
        }});
        assert_eq!(
            parse(&export, &args(&["post"])).unwrap(),
            json!({"mission":"post"})
        );
        assert_eq!(
            parse(&export, &args(&["--example", "write-and-review"])).unwrap(),
            json!({"example":"write-and-review"})
        );
        assert!(
            usage("mission", "export", &json!({"inputSchema":export}))
                .contains("[<mission>] [--example example]")
        );
        let mixed = json!({"type":"object","properties":{
            "document":{"type":"string"},
            "note":{"type":"string","x-ahjo-cli-positional":true}
        },"required":["document"]});
        assert_eq!(
            parse(&mixed, &args(&["file"])).unwrap(),
            json!({"document":"file"})
        );
        assert_eq!(
            parse(&mixed, &args(&["file", "note"])).unwrap(),
            json!({"document":"file","note":"note"})
        );
        assert!(
            parse(&mixed, &[])
                .unwrap_err()
                .to_string()
                .contains("missing <document>")
        );
    }

    #[test]
    fn file_arguments_read_the_local_file_or_standard_input() {
        let schema = json!({"type":"object","properties":{
            "document":{"type":"string","x-ahjo-cli-file":true},
            "other":{"type":"string"}
        }});
        let path = std::env::temp_dir().join(format!("ahjo-cli-document-{}", std::process::id()));
        let source = "slug = \"one\"\n# keep this comment\n";
        std::fs::write(&path, source).unwrap();
        let mut arguments = json!({"document":path.to_str().unwrap(),"other":"unchanged"});
        read_file_arguments(&schema, &mut arguments, &mut std::io::empty(), |_| true).unwrap();
        assert_eq!(arguments, json!({"document":source,"other":"unchanged"}));
        std::fs::remove_file(path).unwrap();
        let mut arguments = json!({"document":"-"});
        read_file_arguments(&schema, &mut arguments, &mut source.as_bytes(), |_| true).unwrap();
        assert_eq!(arguments["document"], source);
        let mut missing = json!({"document":"/nonexistent/ahjo-cli-document"});
        assert!(
            read_file_arguments(&schema, &mut missing, &mut std::io::empty(), |_| true).is_err()
        );
        let untouched = missing.clone();
        read_file_arguments(&schema, &mut missing, &mut std::io::empty(), |_| false).unwrap();
        assert_eq!(missing, untouched);
    }

    #[test]
    fn toml_output_writes_exact_bytes_and_json_preserves_keys() {
        let schema = json!({"properties":{"document":{"type":"string","x-ahjo-cli-toml":true}}});
        assert_eq!(toml_output_field(&schema), Some("document"));
        let document = "# Header\nslug = \"demo\"\n[inputCard]\nid = \"i\"";
        let mut output = Vec::new();
        write_toml_document(document, false, &mut output).unwrap();
        assert_eq!(output, document.as_bytes());
        output.clear();
        write_toml_document(document, true, &mut output).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&output).unwrap(),
            json!({"slug":"demo","inputCard":{"id":"i"}})
        );
        assert!(write_toml_document("bad = [", true, &mut Vec::new()).is_err());
    }

    #[test]
    fn required_arguments_go_in_order_and_flags_are_typed() {
        let v = parse(
            &schema(),
            &args(&["main", "hello there", "--pane", "2", "--no-enter"]),
        )
        .unwrap();
        assert_eq!(
            v,
            json!({"session": "main", "text": "hello there", "pane": 2, "enter": false})
        );
        let v = parse(&schema(), &args(&["--text=hi", "main", "--enter"])).unwrap();
        assert_eq!(v, json!({"session": "main", "text": "hi", "enter": true}));
    }

    #[tokio::test]
    async fn derived_optional_and_enum_fields_project_to_cli_arguments() {
        use crate::{Command, Effect};
        use schemars::JsonSchema;
        use serde::{Deserialize, Serialize};

        #[derive(Debug, Deserialize, Serialize, JsonSchema)]
        #[serde(rename_all = "snake_case")]
        enum Mode {
            First,
            Second,
        }

        /// Project typed fields.
        #[derive(Debug, Deserialize, Serialize, JsonSchema)]
        #[serde(deny_unknown_fields)]
        struct Arguments {
            mode: Option<Mode>,
            weight: Option<f64>,
            enabled: Option<bool>,
            counts: Option<Vec<i64>>,
        }

        let command = Command::typed(
            "test_project",
            Effect::Reads,
            |args: Arguments| async move { Ok(args) },
        );
        let schema = (command.schema)();
        assert_eq!(
            schema["properties"]["mode"]["enum"],
            json!(["first", "second", null])
        );
        let line = read(
            &schema,
            &args(&[
                "--mode",
                "second",
                "--weight",
                "2.5",
                "--no-enabled",
                "--counts",
                "2",
                "--counts",
                "7",
            ]),
        )
        .unwrap();
        let expected = json!({"mode":"second", "weight":2.5, "enabled":false, "counts":[2,7]});
        assert_eq!(line.arguments, expected);
        assert_eq!((command.call)(line.arguments).await.unwrap(), expected);
    }

    #[test]
    fn help_names_required_arguments_first_in_the_order_they_are_given() {
        let tool = json!({"name": "session_send", "description": "Type text.", "inputSchema": {
            "type": "object",
            "properties": {
                "text": {"type": "string"},
                "enter": {"type": "boolean", "default": true},
                "session": {"type": "string", "description": "A session id or name."},
            },
            "required": ["session", "text"],
        }});
        assert_eq!(
            help("session", "send", &tool),
            concat!(
                "usage: ahjo session send <session> <text> [--enter|--no-enter] [--json] [--node id|url]\n",
                "Type text.\n\n",
                "  <session>  string: A session id or name.\n",
                "  <text>     string\n",
                "  --enter    boolean (default true)",
            )
        );
    }

    #[test]
    fn an_empty_list_says_none_and_a_count_of_seconds_is_a_time_too() {
        assert_eq!(
            render(&json!({"node": "n", "missions": [], "inner": {"runs": []}})),
            "node: n\nmissions: none\n"
        );
        assert_eq!(render(&json!([])), "none\n");
        let seconds = 1_700_000_000_i64;
        assert_eq!(
            render(&json!({"last_output": seconds})),
            render(&json!({"last_output": seconds * 1000}))
        );
    }

    #[test]
    fn kebab_and_underscore_flags_resolve_to_schema_keys() {
        let schema = json!({"properties":{"to_node":{"type":"string"},"display_name":{"type":"string"},"context_paths":{"type":"array"},"copy_on_select":{"type":"boolean"}}});
        assert_eq!(
            parse(
                &schema,
                &args(&[
                    "--to-node",
                    "n",
                    "--display-name=x",
                    "--context-paths",
                    "a",
                    "--context_paths",
                    "b",
                    "--no-copy-on-select"
                ])
            )
            .unwrap(),
            json!({"to_node":"n","display_name":"x","context_paths":["a","b"],"copy_on_select":false})
        );
    }

    #[test]
    fn a_last_required_list_takes_every_value_left() {
        let keys = json!({
            "type": "object",
            "properties": {
                "session": {"type": "string"},
                "keys": {"type": "array", "items": {"type": "string"}},
            },
            "required": ["session", "keys"],
        });
        let v = parse(&keys, &args(&["main", "echo hi", "Enter"])).unwrap();
        assert_eq!(v, json!({"session": "main", "keys": ["echo hi", "Enter"]}));
        let v = parse(&keys, &args(&["--keys", "C-c", "main"])).unwrap();
        assert_eq!(v, json!({"session": "main", "keys": ["C-c"]}));
    }

    fn write_schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "text": {"type": "string"},
                "new": {"type": "boolean", "default": false},
            },
            "required": ["path", "text"],
        })
    }

    #[test]
    fn a_value_that_looks_like_the_command_lines_own_flag_stays_a_value() {
        // `--json` and `--node` were once taken out before the flags were
        // read, so `--new` became the text and the file was written over.
        for own in ["--json", "--node", "--help", "-h", "--"] {
            let line = read(&write_schema(), &args(&["p", "--text", own, "--new"])).unwrap();
            assert_eq!(
                line.arguments,
                json!({"path": "p", "text": own, "new": true}),
                "{own}"
            );
            assert!(!line.json && !line.help && line.node.is_none(), "{own}");
        }
        let line = read(
            &write_schema(),
            &args(&["p", "--text=--help", "--node", "lab", "--json", "--new"]),
        )
        .unwrap();
        assert_eq!(
            line.arguments,
            json!({"path": "p", "text": "--help", "new": true})
        );
        assert_eq!(line.node.as_deref(), Some("lab"));
        assert!(line.json && !line.help);
        let line = read(
            &write_schema(),
            &args(&["--node=http://h:1", "--json=false"]),
        )
        .unwrap_err();
        assert!(line.to_string().contains("missing <path>"), "{line}");
    }

    #[test]
    fn help_where_a_flag_stands_wins_even_over_a_wrong_line() {
        for asked in [
            &["--help"][..],
            &["-h"],
            &["p", "--colour", "red", "--help"],
            &["--text", "x", "--help"],
        ] {
            assert!(
                read(&write_schema(), &args(asked)).unwrap().help,
                "{asked:?}"
            );
        }
        assert!(
            !read(&write_schema(), &args(&["p", "--", "--help"]))
                .unwrap()
                .help
        );
    }

    #[test]
    fn after_a_double_dash_every_word_is_a_value() {
        let line = read(
            &write_schema(),
            &args(&["--new", "--", "notes.md", "---\ntitle: x\n---\n"]),
        )
        .unwrap();
        assert_eq!(
            line.arguments,
            json!({"path": "notes.md", "text": "---\ntitle: x\n---\n", "new": true})
        );
        // Without it, the error says how to give such a value.
        let error = read(&write_schema(), &args(&["notes.md", "---\ntitle: x"]))
            .unwrap_err()
            .to_string();
        assert!(error.contains("after --"), "{error}");
        assert!(error.contains("--text=<value>"), "{error}");
        assert!(!error.contains('\n'), "one line: {error}");
        let error = read(&write_schema(), &args(&["p", "t", "--neww"]))
            .unwrap_err()
            .to_string();
        assert!(error.contains("did you mean --new?"), "{error}");
    }

    #[test]
    fn wrong_or_missing_arguments_are_refused() {
        assert!(parse(&schema(), &args(&["main"])).is_err());
        assert!(parse(&schema(), &args(&["main", "hi", "extra"])).is_err());
        assert!(parse(&schema(), &args(&["main", "hi", "--colour", "red"])).is_err());
        assert!(parse(&schema(), &args(&["main", "hi", "--pane", "two"])).is_err());
    }

    #[test]
    fn nested_objects_and_lists_keep_their_values_without_json_or_empty_fields() {
        let answer = json!({
            "node": "local", "warning": null, "empty": "", "none": [],
            "host": {"cpu": {"percent": 0}, "gpu": {"reason": "unavailable"}},
            "tasks": [
                {"name": "check", "targets": [{"name": "agent", "paused": false}]},
                {"name": "review", "schedule": {"every": 15}},
                null, {}, []
            ],
            "hidden": {"nested": [null, "", {}]}
        });
        assert_eq!(
            render(&answer),
            concat!(
                "node: local\n",
                "none: none\n",
                "host:\n  cpu:\n    percent: 0\n  gpu:\n    reason: unavailable\n",
                "tasks:\n  name=check\n    targets:\n      name=agent\tpaused=false\n",
                "  name=review\n    schedule:\n      every: 15\n",
            )
        );
    }

    #[test]
    fn renders_mixed_arrays_multiline_text_and_empty_values() {
        assert_eq!(
            render(&json!(["one", null, ["two", 0], {"text": "first\nsecond", "off": false}])),
            "- one\n-\n  - two\n  - 0\noff=false\n  text:\n  first\n  second\n"
        );
        assert_eq!(
            render(&json!({"warning": null, "empty": [null, {}], "blank": "  "})),
            ""
        );
        assert_eq!(render(&json!("hello")), "hello\n");
    }

    #[test]
    fn timestamp_fields_render_as_local_times_in_objects_and_rows() {
        let ms = 1_700_000_000_123_i64;
        let local = jiff::Timestamp::from_millisecond(ms)
            .unwrap()
            .to_zoned(jiff::tz::TimeZone::system());
        let expected = format!("{}{}", local.datetime(), local.offset());
        assert_eq!(
            expected
                .parse::<jiff::Timestamp>()
                .unwrap()
                .as_millisecond(),
            ms
        );
        let answer = json!({
            "host": {"at": ms, "started": ms, "updated": ms, "modified": ms,
                     "synced": ms, "created_at": ms, "count": ms},
            "tasks": [{"finished_at": ms, "next_run": "2026-11-12T12:00:00Z"}],
            "invalid": {"at": u64::MAX, "started": 9223372036854775807_i64,
                        "updated": 1.5, "modified": "not a timestamp"}
        });
        let rendered = render(&answer);
        for key in [
            "at",
            "started",
            "updated",
            "modified",
            "synced",
            "created_at",
        ] {
            assert!(
                rendered.contains(&format!("{key}: {expected}\n")),
                "{rendered}"
            );
        }
        assert!(rendered.contains(&format!(
            "finished_at={expected}\tnext_run=2026-11-12T12:00:00Z"
        )));
        assert!(rendered.contains(&format!("count: {ms}\n")));
        assert!(rendered.contains("at: 18446744073709551615\n"));
        assert!(rendered.contains("started: 9223372036854775807\n"));
        assert!(rendered.contains("updated: 1.5\n"));
        assert!(rendered.contains("modified: not a timestamp\n"));
    }

    #[test]
    fn a_list_reads_as_one_row_per_item_and_long_text_as_it_was_written() {
        assert_eq!(
            render(&json!({"sessions": [
                {"id": "$1", "name": "main", "question": null},
                {"id": "$2", "name": "build"}
            ]})),
            "sessions:\n  id=$1\tname=main\n  id=$2\tname=build\n"
        );
        assert_eq!(
            render(&json!({"path": "a.txt", "diff": "-before\n+after\n"})),
            "path: a.txt\ndiff:\n-before\n+after\n"
        );
    }
}
