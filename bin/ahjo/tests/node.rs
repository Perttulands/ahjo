//! The whole binary: `ahjo serve` holding only terminals over a scratch
//! tmux, the CLI over its commands, and `ahjo mcp` relaying stdio. Linux
//! and macOS only; skips without tmux unless `AHJO_REQUIRE_TMUX=1`.
#![cfg(unix)]

use std::net::SocketAddr;
use std::process::{Output, Stdio};
use std::time::{Duration, Instant};

use ahjo_testkit::process::Node;
use ahjo_testkit::tmux::{Server, capture, eventually};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

const AHJO: &str = env!("CARGO_BIN_EXE_ahjo");

/// An `ahjo serve` whose only section is terminals on `tmux`'s server,
/// with its config folder in `tmux`'s directory.
fn serve(tmux: &Server) -> Node {
    let config = tmux.dir().join("config.toml");
    std::fs::write(
        &config,
        format!("[terminals]\nsocket = {:?}\n", tmux.socket_path()),
    )
    .unwrap();
    let mut command = std::process::Command::new(AHJO);
    command
        .args(["serve", "--config"])
        .arg(&config)
        .env("XDG_CONFIG_HOME", tmux.dir().join("xdg"));
    Node::start(&mut command, "127.0.0.1:0".parse().unwrap())
}

async fn ahjo(node: SocketAddr, args: &[&str]) -> Output {
    tokio::process::Command::new(AHJO)
        .args(args)
        .args(["--node", &format!("http://{node}")])
        .output()
        .await
        .unwrap()
}

/// Waits until the node lists session `name` with its panes. A node serves
/// at once and finds its sessions just after; until a command waits for
/// that itself (ahjo-iay), a test that uses one waits here.
async fn listed(node: SocketAddr, name: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let out = ahjo(node, &["session", "list", "--json"]).await;
        let list: Value = serde_json::from_slice(&out.stdout).unwrap_or_default();
        let ready = list["sessions"].as_array().is_some_and(|all| {
            all.iter()
                .any(|s| s["name"] == name && s["panes"].as_array().is_some_and(|p| !p.is_empty()))
        });
        if ready {
            return;
        }
        assert!(Instant::now() < deadline, "{name} was never listed: {list}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn get(node: SocketAddr, path: &str) -> Value {
    let mut stream = tokio::net::TcpStream::connect(node).await.unwrap();
    let request = format!("GET {path} HTTP/1.1\r\nHost: {node}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    let (head, body) = response.split_once("\r\n\r\n").unwrap();
    assert!(head.starts_with("HTTP/1.1 200"), "{response}");
    serde_json::from_str(body).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn serve_with_only_terminals_answers_health_and_the_session_commands() {
    let Some(tmux) = Server::start_in("ahjo-serve") else {
        return;
    };
    tmux.new_session("alpha", 100, 30);
    let node = serve(&tmux);
    let health = get(node.address, "/api/health").await;
    assert_eq!(health["status"], "ok", "{health}");
    listed(node.address, "alpha").await;

    let sent = ahjo(
        node.address,
        &["session", "send", "alpha", "echo cli-$((6*7))"],
    )
    .await;
    assert!(
        sent.status.success(),
        "{}",
        String::from_utf8_lossy(&sent.stderr)
    );
    eventually("the echo ran", || {
        capture(&tmux, "alpha")
            .lines()
            .any(|line| line.trim() == "cli-42")
            .then_some(())
    })
    .await;
    let listed = ahjo(node.address, &["session", "list", "--json"]).await;
    assert!(
        listed.status.success(),
        "{}",
        String::from_utf8_lossy(&listed.stderr)
    );
    let sessions: Value = serde_json::from_slice(&listed.stdout).unwrap();
    assert!(sessions.to_string().contains("\"alpha\""), "{sessions}");
    let verbs = ahjo(node.address, &["session"]).await;
    let verbs = String::from_utf8_lossy(&verbs.stdout);
    assert!(
        verbs.contains("ahjo session send <session> <text>"),
        "{verbs}"
    );
    let missing = ahjo(node.address, &["session", "send", "alpha"]).await;
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("missing <text>"));
    let unknown = ahjo(node.address, &["session", "read", "nobody-here"]).await;
    assert_eq!(unknown.status.code(), Some(1));
}

/// One JSON-RPC request to `path`: its result.
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
    let mut command = std::process::Command::new(AHJO);
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
            let name = format!("{path} {}", tool["name"].as_str().unwrap());
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

/// Linux only: the environment it reports is Linux or WSL.
#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread")]
async fn health_and_version_identify_the_serving_node() {
    let root = ahjo_testkit::fresh_dir("ahjo-health");
    let config = root.join("config.toml");
    std::fs::write(
        &config,
        format!(
            "[node]\ndevice = \"health-test\"\n[terminals]\nsocket = {:?}\n",
            root.join("tmux")
        ),
    )
    .unwrap();
    let mut command = std::process::Command::new(AHJO);
    command
        .args(["serve", "--config"])
        .arg(&config)
        .env("XDG_CONFIG_HOME", root.join("xdg"));
    let mut node = Node::start(&mut command, "127.0.0.1:0".parse().unwrap());
    let health = get(node.address, "/api/health").await;
    let version = get(node.address, "/api/version").await;
    assert_eq!(health["status"], "ok");
    assert_eq!(health["device"], "health-test");
    assert!(!health["node"].as_str().unwrap().is_empty());
    assert!(matches!(
        health["environment"].as_str(),
        Some("linux" | "wsl")
    ));
    assert_eq!(health["protocol"], ahjo_core::PROTOCOL);
    assert_eq!(health["version"], ahjo_node_kit::VERSION);
    assert!(ahjo_node_kit::VERSION.starts_with(env!("CARGO_PKG_VERSION")));
    assert_eq!(version["version"], health["version"]);
    assert!(
        health["timestamp"]
            .as_str()
            .unwrap()
            .parse::<jiff::Timestamp>()
            .is_ok()
    );
    node.stop();
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn ahjo_mcp_relays_stdio_to_the_node() {
    let Some(tmux) = Server::start_in("ahjo-stdio") else {
        return;
    };
    tmux.new_session("alpha", 100, 30);
    let node = serve(&tmux);
    listed(node.address, "alpha").await;
    let mut child = tokio::process::Command::new(AHJO)
        .args([
            "mcp",
            "--area",
            "terminals",
            "--node",
            &node.address.to_string(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap()).lines();
    let messages = [
        json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": { "protocolVersion": "2025-03-26", "capabilities": {},
                            "clientInfo": { "name": "test", "version": "1" } } }),
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
        json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call",
                "params": { "name": "session_list", "arguments": {} } }),
        json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/list" }),
    ];
    for message in messages {
        stdin
            .write_all(format!("{message}\n").as_bytes())
            .await
            .unwrap();
    }
    let next = async |lines: &mut tokio::io::Lines<_>| -> Value {
        let line = tokio::time::timeout(Duration::from_secs(10), lines.next_line())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        serde_json::from_str(&line).unwrap()
    };
    let answer = next(&mut stdout).await;
    assert_eq!(answer["result"]["protocolVersion"], "2025-03-26");
    let answer = next(&mut stdout).await;
    assert_eq!(answer["id"], 2, "the notification got no answer line");
    let sessions: Value =
        serde_json::from_str(answer["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert!(
        sessions["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["name"] == "alpha"),
        "{sessions}"
    );
    // --area terminals: the sessions' tools, and no other area's.
    let answer = next(&mut stdout).await;
    let tools = answer["result"]["tools"].as_array().unwrap();
    assert!(!tools.is_empty());
    let terminals = ahjo::AREAS.iter().find(|a| a.name == "terminals").unwrap();
    assert!(
        tools
            .iter()
            .all(|t| terminals.commands.contains(&t["name"].as_str().unwrap())),
        "{answer}"
    );

    drop(stdin);
    let status = child.wait().await.unwrap();
    assert!(status.success(), "ends with stdin");
}

#[tokio::test(flavor = "multi_thread")]
async fn ahjo_mcp_answers_requests_when_the_node_is_down() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let mut child = tokio::process::Command::new(AHJO)
        .args(["mcp", "--node", &addr.to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"ping\"}\n")
        .await
        .unwrap();
    drop(stdin);
    let mut out = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut out)
        .await
        .unwrap();
    let answer: Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(answer["id"], 7);
    assert!(
        answer["error"]["message"]
            .as_str()
            .unwrap()
            .contains("ahjo node at")
    );
}
