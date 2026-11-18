//! `ahjo <noun> <verb> [args]`: every command of the node's areas. The
//! command line is a client of the node's MCP tools: it reads `tools/list`
//! for the tool `<noun>_<verb>` and its arguments, and calls it through
//! `tools/call`, so the CLI and MCP cannot drift apart. Required arguments
//! may be given in order; every argument is also `--name value`.

use ahjo_core::Error;
use anyhow::{Context, Result, bail};
use axum::http::Method;
use serde_json::{Map, Value, json};
use std::io::Write;

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

/// Runs one command, writing its answer to `out`. The exit code is 1 when
/// the tool failed. `node`, when given, is the node to talk to; else the
/// one this machine runs, or `--node` among `args`.
pub async fn run(noun: &str, args: &[String], out: &mut impl Write) -> Result<i32> {
    let (node, args) = take_node(args)?;
    // An address is the node the command talks to; an id is the node the
    // tool runs on, reached through the one it talks to.
    let (url, on) = match node {
        Some(node) if crate::client::is_address(&node) => (Some(node), None),
        on => (None, on),
    };
    let client = NodeClient::new(url.as_deref())?;
    let tools = list(&client).await?;
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
        let known = nouns(&tools);
        let nearest = match nearest(noun, known.iter().map(String::as_str)) {
            Some(near) => format!(" (did you mean {near}?)"),
            None => String::new(),
        };
        return Err(usage_error(format!(
            "no command {noun}{nearest}; this node's commands are {}{off}",
            known.join(", ")
        )));
    }
    let Some(verb) = args.first().filter(|v| !v.starts_with('-')) else {
        writeln!(out, "{}", verbs(noun, &ours))?;
        return Ok(0);
    };
    let tool_name = format!("{noun}_{}", verb.replace('-', "_"));
    let Some(tool) = ours.iter().find(|t| name(t) == tool_name) else {
        let names: Vec<String> = ours.iter().map(|t| verb_of(noun, t)).collect();
        let nearest = match nearest(verb, names.iter().map(String::as_str)) {
            Some(near) => format!(" (did you mean ahjo {noun} {near}?)"),
            None => String::new(),
        };
        return Err(usage_error(format!(
            "no command {noun} {verb}{nearest}; ahjo {noun} lists its verbs"
        )));
    };
    let rest = &args[1..];
    if rest.iter().any(|a| a == "--help" || a == "-h") {
        writeln!(out, "{}", help(noun, verb, tool))?;
        return Ok(0);
    }
    let json_out = rest.iter().any(|a| a == "--json" || a == "--json=true");
    let rest: Vec<String> = rest
        .iter()
        .filter(|a| *a != "--json" && *a != "--json=true")
        .cloned()
        .collect();
    let mut arguments = parse(&tool["inputSchema"], &rest)
        .map_err(|error| usage_error(format!("{error:#}\nusage: {}", usage(noun, verb, tool))))?;
    if let Some(on) = &on {
        arguments["node"] = Value::String(on.clone());
    }
    let answer = call(&client, &tool_name, arguments).await?;
    let failed = answer["isError"].as_bool().unwrap_or(false);
    let text = answer["content"][0]["text"].as_str().unwrap_or("");
    if failed {
        match serde_json::from_str::<Error>(text) {
            Ok(error) if json_out => eprintln!("{}", json!({ "error": error })),
            Ok(error) => eprintln!("ahjo {noun} {verb}: {error}"),
            Err(_) => eprintln!("ahjo {noun} {verb}: {text}"),
        }
        return Ok(1);
    }
    let mut value: Value =
        serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.into()));
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

fn take_node(args: &[String]) -> Result<(Option<String>, Vec<String>)> {
    let mut node = None;
    let mut rest = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--node" {
            node = Some(it.next().context("--node needs a node id or URL")?.clone());
        } else if let Some(v) = a.strip_prefix("--node=") {
            node = Some(v.to_owned());
        } else {
            rest.push(a.clone());
        }
    }
    Ok((node, rest))
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
        match spec["type"].as_str() {
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
        tool["description"].as_str().unwrap_or("")
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
            spec["type"].as_str().unwrap_or("value")
        );
        if let Some(d) = spec["description"].as_str() {
            line.push_str(&format!(": {d}"));
        }
        if let Some(values) = spec["enum"].as_array() {
            let values: Vec<String> = values.iter().map(|v| v.to_string()).collect();
            line.push_str(&format!(" (one of {})", values.join(", ")));
        }
        if let Some(d) = spec.get("default") {
            line.push_str(&format!(" (default {d})"));
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
    match properties(schema).as_slice() {
        [(key, spec)] if required(schema).is_empty() && spec["type"] == "string" => {
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

/// The tool's arguments from the command line, typed by its schema.
fn parse(schema: &Value, args: &[String]) -> Result<Value> {
    let props = properties(schema);
    let spec = |key: &str| props.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
    let mut out = Map::new();
    let mut positional = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let Some(flag) = a.strip_prefix("--") else {
            positional.push(a.clone());
            continue;
        };
        let (key, inline) = match flag.split_once('=') {
            Some((k, v)) => (k.to_owned(), Some(v.to_owned())),
            None => (flag.to_owned(), None),
        };
        let key = key.replace('-', "_");
        if let Some(negated) = key.strip_prefix("no_")
            && spec(negated).is_some_and(|s| s["type"] == "boolean")
            && inline.is_none()
        {
            out.insert(negated.to_owned(), Value::Bool(false));
            continue;
        }
        let s = spec(&key).with_context(|| format!("unknown flag --{key}"))?;
        let raw = match (inline, s["type"].as_str()) {
            (Some(v), _) => v,
            (None, Some("boolean")) => "true".into(),
            (None, _) => it
                .next()
                .with_context(|| format!("--{key} needs a value"))?
                .clone(),
        };
        let value = typed(&key, &s, &raw)?;
        match (s["type"].as_str(), out.get_mut(&key)) {
            (Some("array"), Some(Value::Array(items))) => {
                items.extend(value.as_array().cloned().unwrap_or_default())
            }
            _ => {
                out.insert(key, value);
            }
        }
    }
    let needed = required(schema).len();
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
                items.push(Value::String(p));
            }
            continue;
        }
        let key = waiting
            .next()
            .with_context(|| format!("unexpected argument {p:?}"))?;
        let s = spec(&key).unwrap_or(Value::Null);
        if s["type"] == "array" && waiting.peek().is_none() {
            list = Some(key.clone());
        }
        out.insert(key.clone(), typed(&key, &s, &p)?);
    }
    if let Some(missing) = waiting.next().filter(|_| needed > 0) {
        bail!("missing <{missing}>");
    }
    Ok(Value::Object(out))
}

fn typed(key: &str, spec: &Value, raw: &str) -> Result<Value> {
    Ok(match spec["type"].as_str() {
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
        Some("array") => json!([raw]),
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
            "agents_hook_add",
            "session_read",
            "agents_show",
        ]
        .iter()
        .map(|n| json!({"name": n, "description": "Does it."}))
        .collect();
        assert_eq!(
            noun_index(&tools),
            "  session  list read\n  agents   hook-add show"
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
