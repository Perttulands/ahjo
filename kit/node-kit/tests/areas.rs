//! An area defined here, in the test, reaches the client every way the node
//! offers: its route over HTTP, its channel on `/ws` (a frame from the
//! client and a push), and its command as an MCP tool and a CLI verb. And
//! areas that would share a name, a frame kind or a command refuse to start.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use ahjo_core::Error;
use ahjo_core::Hello;
use ahjo_core::frame::Kinds;
use ahjo_node_kit::mcp::ToolSet;
use ahjo_node_kit::{
    Area, BoxFuture, Build, Built, Channel, Client, Command, Config, Effect, Listing, NodeClient,
    NodeError, Options, Running, Setup,
};
use axum::Router;
use axum::http::Method;
use axum::routing::get;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

/// A lamp: `/api/lights` says how bright, `lights_switch` sets it, and a
/// client's frame 0x60 asks for a push 0x61 of the brightness it carries.
struct Lights {
    command: &'static str,
}

const LIGHTS: Kinds = &[0x60..=0x61];

/// The lights as a binary lists them.
const LIGHTS_LISTED: Listing = listed("lights", LIGHTS, &["lights_switch"], lights);

const fn listed(
    name: &'static str,
    kinds: Kinds,
    commands: &'static [&'static str],
    build: Build,
) -> Listing {
    Listing {
        name,
        kinds,
        commands,
        build,
    }
}

/// Switch the lights. Answers with how bright they are.
#[derive(serde::Deserialize, schemars::JsonSchema)]
struct SwitchLights {
    level: Option<u64>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
struct LightLevel {
    level: u64,
}

impl Area for Lights {
    fn routes(&self) -> Router {
        Router::new().route(
            "/api/lights",
            get(|| async { axum::Json(json!({ "brightness": "dim" })) }),
        )
    }

    fn commands(&self) -> Vec<Command> {
        vec![Command::typed(
            self.command,
            Effect::Sets,
            |args: SwitchLights| async move {
                match args.level {
                    Some(level) => Ok(LightLevel { level }),
                    None => Err(Error::new("no_level", "level is required")),
                }
            },
        )]
    }

    fn channel(&self, _client: Client) -> Option<Box<dyn Channel>> {
        let (pushes, pushed) = mpsc::channel(4);
        Some(Box::new(Lamp { pushes, pushed }))
    }
}

struct Lamp {
    pushes: mpsc::Sender<Vec<u8>>,
    pushed: mpsc::Receiver<Vec<u8>>,
}

impl Channel for Lamp {
    fn receive(&mut self, frame: Vec<u8>) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            assert_eq!(frame[0], 0x60);
            let _ = self.pushes.send(vec![0x61, frame[1]]).await;
        })
    }

    fn next(&mut self) -> BoxFuture<'_, Option<Vec<u8>>> {
        Box::pin(self.pushed.recv())
    }
}

fn lights(_: &Setup) -> anyhow::Result<Built> {
    Built::now(Lights {
        command: "lights_switch",
    })
}

async fn start(areas: &[Listing], config: &str) -> anyhow::Result<Running> {
    start_with(areas, &[], config).await
}

async fn start_with(areas: &[Listing], sets: &[ToolSet], config: &str) -> anyhow::Result<Running> {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "ahjo-areas-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir)?;
    let config = Config::parse(config, dir.clone())?;
    ahjo_node_kit::start_with(
        Options {
            dir,
            config,
            listen: Some("127.0.0.1:0".into()),
        },
        areas,
        sets,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn an_area_defined_in_a_test_answers_every_way() {
    let node = start(&[LIGHTS_LISTED], "").await.unwrap();
    let url = format!("http://{}/", node.address);
    let client = NodeClient::new(Some(&url)).unwrap();

    // Its route, over HTTP.
    let (dim, _) = client
        .request(Method::GET, "/api/lights", None, None)
        .await
        .unwrap();
    assert_eq!(dim, json!({ "brightness": "dim" }));

    // Its channel, on /ws: after the hello, a frame and its push.
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{}/ws", node.address))
        .await
        .unwrap();
    let hello = ws.next().await.unwrap().unwrap().into_data();
    let hello = Hello::decode(&hello).unwrap();
    assert_eq!(hello.node, node.node.id.to_string());
    assert!(hello.off.is_empty());
    ws.send(Message::Binary(vec![0x60, 7].into()))
        .await
        .unwrap();
    let pushed = ws.next().await.unwrap().unwrap().into_data();
    assert_eq!(&pushed[..], &[0x61, 7]);

    // Its command, as an MCP tool.
    let call = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": { "name": "lights_switch", "arguments": { "level": 3 } },
    });
    let (answer, _) = client
        .request(Method::POST, "/mcp", Some(call), None)
        .await
        .unwrap();
    assert_eq!(answer["result"]["isError"], false);
    assert_eq!(answer["result"]["content"][0]["text"], r#"{"level":3}"#);
    let failing = json!({
        "jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": { "name": "lights_switch", "arguments": {} },
    });
    let (answer, _) = client
        .request(Method::POST, "/mcp", Some(failing), None)
        .await
        .unwrap();
    assert_eq!(answer["result"]["isError"], true);
    let error: Error =
        serde_json::from_str(answer["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(error.code, "no_level");
    // `node` names where a tool runs: this node, and no other yet.
    for (node, code) in [
        (node.node.id.to_string(), None),
        ("elsewhere".into(), Some("unknown_node")),
    ] {
        let call = json!({
            "jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": { "name": "lights_switch", "arguments": { "level": 1, "node": node } },
        });
        let (answer, _) = client
            .request(Method::POST, "/mcp", Some(call), None)
            .await
            .unwrap();
        let text = answer["result"]["content"][0]["text"].as_str().unwrap();
        match code {
            None => assert_eq!(text, r#"{"level":1}"#),
            Some(code) => {
                let error: Error = serde_json::from_str(text).unwrap();
                assert_eq!(error.code, code);
            }
        }
    }

    // And as a CLI verb.
    let mut out = Vec::new();
    let args: Vec<String> = ["switch", "--level", "5", "--json", "--node", &url]
        .map(String::from)
        .to_vec();
    let code = ahjo_node_kit::cli::run("lights", &args, &mut out)
        .await
        .unwrap();
    assert_eq!(code, 0);
    let printed: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(printed, json!({ "level": 5 }));

    drop(ws);
    node.stop().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_area_its_section_turns_off_is_not_built_and_says_so() {
    let node = start(&[LIGHTS_LISTED], "[lights]\nenabled = false\n")
        .await
        .unwrap();
    assert_eq!(node.off(), ["lights"]);
    let url = format!("http://{}/", node.address);
    let client = NodeClient::new(Some(&url)).unwrap();
    let (health, _) = client
        .request(Method::GET, "/api/health", None, None)
        .await
        .unwrap();
    assert_eq!(health["off"], json!(["lights"]));
    let args: Vec<String> = ["switch", "--node", &url].map(String::from).to_vec();
    let refused = ahjo_node_kit::cli::run("lights", &args, &mut Vec::new())
        .await
        .unwrap_err();
    assert!(
        format!("{refused:#}").contains("turned off on this node: lights"),
        "{refused:#}"
    );
    node.stop().await.unwrap();
}

/// Starts the lights and `second` together and answers with the refusal.
async fn refusal(second: Listing) -> String {
    match start(&[LIGHTS_LISTED, second], "").await {
        Ok(_) => panic!("conflicting areas started"),
        Err(error) => format!("{error:#}"),
    }
}

/// The node checks its list before building anything (`listing::check`
/// holds each conflict), and refuses an area whose built commands are not
/// the ones it is listed with.
#[tokio::test(flavor = "multi_thread")]
async fn areas_that_would_share_a_kind_a_name_or_a_command_refuse_to_start() {
    let error = refusal(listed("lamps", &[0x61..=0x62], &["lamps_switch"], lights)).await;
    assert!(
        error.contains("lights") && error.contains("lamps") && error.contains("frame kinds"),
        "{error}"
    );
    let error = refusal(listed("lamps", &[0x70..=0x70], &["lights_switch"], lights)).await;
    assert!(
        error.contains("lights") && error.contains("lamps") && error.contains("lights_switch"),
        "{error}"
    );
    let error = refusal(LIGHTS_LISTED).await;
    assert!(error.contains("two areas are named lights"), "{error}");
    let error = refusal(listed("lamps", &[0x00..=0x00], &[], lights)).await;
    assert!(
        error.contains("lamps") && error.contains("hello"),
        "{error}"
    );

    fn lamps(_: &Setup) -> anyhow::Result<Built> {
        Built::now(Lights {
            command: "lamps_dim",
        })
    }
    // Listed with one command, it has another.
    let error = refusal(listed("lamps", &[0x70..=0x70], &["lamps_switch"], lamps)).await;
    assert!(
        error.contains("lamps") && error.contains("lamps_switch") && error.contains("lamps_dim"),
        "{error}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_section_no_area_reads_refuses_to_start() {
    let error = match start(&[LIGHTS_LISTED], "[lamps]\nlevel = 3\n").await {
        Ok(_) => panic!("an unknown section started"),
        Err(error) => format!("{error:#}"),
    };
    assert!(error.contains("lamps"), "{error}");
}

/// What one area offers others: a word to say.
trait Word: Send + Sync {
    fn word(&self) -> &'static str;
}

impl Word for Lights {
    fn word(&self) -> &'static str {
        "bright"
    }
}

/// Lights that offer their word to other areas.
fn offering(_: &Setup) -> anyhow::Result<Built> {
    Ok(Built::Now(Arc::new(Lights {
        command: "lights_switch",
    })))
}

fn offering_word(setup: &Setup) -> anyhow::Result<Built> {
    setup.provided.provide::<dyn Word>(Arc::new(Lights {
        command: "lights_switch",
    }))?;
    offering(setup)
}

/// Lamps that read the word lights offer, and answer it as their command.
fn reading_word(_: &Setup) -> anyhow::Result<Built> {
    Built::then(|provided| {
        let word = provided.get::<dyn Word>()?.word();
        Ok(Arc::new(Said { word }) as Arc<dyn Area>)
    })
}

const LIGHTS_OFFERING: Listing = listed("lights", LIGHTS, &["lights_switch"], offering_word);
const LAMPS_READING: Listing = listed("lamps", &[], &["lamps_say"], reading_word);

struct Said {
    word: &'static str,
}

impl Area for Said {
    fn commands(&self) -> Vec<Command> {
        let word = self.word;
        /// Says the word the lights offer.
        #[derive(serde::Deserialize, schemars::JsonSchema)]
        struct Say {}

        vec![Command::typed(
            "lamps_say",
            Effect::Reads,
            move |_: Say| async move { Ok(word) },
        )]
    }
}

/// An area reads what another provides wherever the two are listed; read
/// while areas are still being built it is refused, and with no provider
/// it is `not_provided`.
#[tokio::test(flavor = "multi_thread")]
async fn areas_read_what_others_provide_in_any_order() {
    for areas in [
        [LAMPS_READING, LIGHTS_OFFERING],
        [LIGHTS_OFFERING, LAMPS_READING],
    ] {
        let node = start(&areas, "").await.unwrap();
        let said = node.provided.get::<dyn Word>().unwrap().word();
        assert_eq!(said, "bright");
        let url = format!("http://{}/", node.address);
        let call = json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": "lamps_say", "arguments": {} },
        });
        let (answer, _) = NodeClient::new(Some(&url))
            .unwrap()
            .request(Method::POST, "/mcp", Some(call), None)
            .await
            .unwrap();
        assert_eq!(answer["result"]["content"][0]["text"], r#""bright""#);
        node.stop().await.unwrap();
    }

    let error = match start(&[LAMPS_READING, LIGHTS_LISTED], "").await {
        Ok(_) => panic!("an area read what nobody provides"),
        Err(error) => format!("{error:#}"),
    };
    assert!(error.contains("starting lamps"), "{error}");
    assert!(error.contains("no area on this node provides"), "{error}");

    fn reading_early(setup: &Setup) -> anyhow::Result<Built> {
        setup.provided.get::<dyn Word>()?;
        offering(setup)
    }
    let error = match start(
        &[
            LIGHTS_OFFERING,
            listed("lamps", &[], &["lamps_switch"], reading_early),
        ],
        "",
    )
    .await
    {
        Ok(_) => panic!("an area read a interface while areas were being built"),
        Err(error) => format!("{error:#}"),
    };
    assert!(error.contains("Built::then"), "{error}");
}

const UPGRADE: &str = "Connection: Upgrade\r\nUpgrade: websocket\r\n\
    Sec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n";

/// Every way in, the node's own and an area's, admits only requests
/// addressed to this node from its own pages or from no page at all. The
/// HTTPS rule for tailnet names is `serve::trust`'s own test: no node learns
/// tailnet names yet.
#[tokio::test(flavor = "multi_thread")]
async fn foreign_origins_and_rebinding_hosts_are_refused() {
    let node = start(&[LIGHTS_LISTED], "").await.unwrap();
    let here = node.address.to_string();
    let own = format!("http://{here}");
    let cases = [
        (here.as_str(), None, true, "without Origin"),
        (&here, Some(own.as_str()), true, "from its own origin"),
        (
            &here,
            Some("http://evil.example"),
            false,
            "from a foreign origin",
        ),
        (
            &here,
            Some("http://localhost:1"),
            false,
            "from another port",
        ),
        ("evil.example:7300", None, false, "rebinding without Origin"),
        (
            "evil.example:7300",
            Some("http://evil.example:7300"),
            false,
            "rebinding with Origin",
        ),
    ];
    for (path, upgrade, served) in [
        ("/api/health", "", 200),
        ("/ws", UPGRADE, 101),
        ("/mcp", "", 405),
        ("/mcp/lights", "", 405),
        ("/api/lights", "", 200),
    ] {
        for (host, origin, admitted, case) in cases {
            let expected = if admitted { served } else { 403 };
            let got = status(node.address, path, upgrade, host, origin).await;
            assert_eq!(got, expected, "{path} {case}");
        }
    }
    node.stop().await.unwrap();
}

/// A request naming another node with `?node=` is refused with the error
/// shape, on an area's route, on `/ws` and on `/mcp`, and never runs here;
/// one naming this node runs as one naming none.
#[tokio::test(flavor = "multi_thread")]
async fn a_request_for_another_node_is_refused_not_run_here() {
    let node = start(&[LIGHTS_LISTED], "").await.unwrap();
    let here = node.address.to_string();
    let own = node.node.id.to_string();
    for (path, upgrade, served) in [
        ("/api/lights", "", 200),
        ("/ws", UPGRADE, 101),
        ("/mcp", "", 405),
    ] {
        let mine = format!("{path}?node={own}");
        assert_eq!(
            status(node.address, &mine, upgrade, &here, None).await,
            served
        );
        let theirs = format!("{path}?node=elsewhere");
        let (code, body) = response(node.address, &theirs, upgrade).await;
        assert_eq!(code, 421, "{theirs}");
        let refused: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(refused["error"]["code"], "unknown_node", "{theirs}");
        assert!(
            refused["error"]["message"]
                .as_str()
                .unwrap()
                .contains("elsewhere")
        );
    }
    node.stop().await.unwrap();
}

/// The status and body a request for `path` gets.
async fn response(addr: std::net::SocketAddr, path: &str, upgrade: &str) -> (u16, String) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    let request = format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\n{upgrade}\r\n");
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut text = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let read = stream.read(&mut chunk).await.unwrap();
        text.extend_from_slice(&chunk[..read]);
        let whole = String::from_utf8_lossy(&text).into_owned();
        if let Some((head, body)) = whole.split_once("\r\n\r\n") {
            let length: usize = head
                .lines()
                .find_map(|l| {
                    l.to_ascii_lowercase()
                        .strip_prefix("content-length: ")
                        .map(str::to_owned)
                })
                .and_then(|n| n.trim().parse().ok())
                .unwrap_or(0);
            if body.len() >= length || read == 0 {
                let code = head.split(' ').nth(1).and_then(|s| s.parse().ok()).unwrap();
                return (code, body.to_owned());
            }
        }
        assert!(read > 0, "the node closed before answering: {whole}");
    }
}

/// The status a request for `path` gets with these headers.
async fn status(
    addr: std::net::SocketAddr,
    path: &str,
    upgrade: &str,
    host: &str,
    origin: Option<&str>,
) -> u16 {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    let origin = origin
        .map(|o| format!("Origin: {o}\r\n"))
        .unwrap_or_default();
    let request = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\n{origin}{upgrade}\r\n");
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).await.unwrap();
    line.split(' ')
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("status line {line:?}"))
}

/// An area that reads its section and does nothing else.
fn files(_: &Setup) -> anyhow::Result<Built> {
    struct Files;
    impl Area for Files {}
    Built::now(Files)
}

/// `ahjo <noun> <args>` against the node at `url`: its exit code and what
/// it printed.
async fn ahjo(noun: &str, args: &[&str], url: &str) -> (i32, Value) {
    let mut args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    args.extend(["--json".into(), "--node".into(), url.into()]);
    let mut out = Vec::new();
    let code = ahjo_node_kit::cli::run(noun, &args, &mut out)
        .await
        .unwrap();
    (code, serde_json::from_slice(&out).unwrap_or(Value::Null))
}

/// Shades: a second area, with one command and nothing else.
struct Shades;

impl Area for Shades {
    fn commands(&self) -> Vec<Command> {
        /// Close the shades.
        #[derive(serde::Deserialize, schemars::JsonSchema)]
        struct Close {}

        vec![Command::typed(
            "shades_close",
            Effect::Sets,
            |_: Close| async { Ok("closed") },
        )]
    }
}

fn shades(_: &Setup) -> anyhow::Result<Built> {
    Built::now(Shades)
}

/// One JSON-RPC message posted to `path`: the HTTP status and the answer,
/// over a plain socket so a refused path's status and text are seen.
async fn rpc(node: &Running, path: &str, message: Value) -> (u16, String) {
    post(node, path, message, "").await
}

async fn post(node: &Running, path: &str, message: Value, headers: &str) -> (u16, String) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let (address, body) = (node.address, message.to_string());
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    let (head, body) = response.split_once("\r\n\r\n").unwrap();
    let status = head.split(' ').nth(1).unwrap().parse().unwrap();
    (status, body.to_owned())
}

/// A node-local delete reaches the same handler and authority via both
/// transports, with the counter proving which requests actually ran.
#[tokio::test(flavor = "multi_thread")]
async fn typed_calls_share_mcp_authority_arguments_and_execution() {
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};

    /// Delete one entry from the test vault.
    #[derive(Deserialize, JsonSchema)]
    #[serde(deny_unknown_fields)]
    struct Delete {
        entry: u32,
    }

    #[derive(Serialize, JsonSchema)]
    struct Deleted {
        entry: u32,
        count: u32,
    }

    #[derive(Default)]
    struct Vault(Arc<AtomicU32>);
    impl Area for Vault {
        fn commands(&self) -> Vec<Command> {
            let counter = self.0.clone();
            vec![Command::typed(
                "vault_delete",
                Effect::Destroys,
                move |args: Delete| {
                    let count = counter.fetch_add(1, Ordering::SeqCst) + 1;
                    async move {
                        Ok(Deleted {
                            entry: args.entry,
                            count,
                        })
                    }
                },
            )]
        }
    }
    fn vault(_: &Setup) -> anyhow::Result<Built> {
        Built::now(Vault::default())
    }
    let node = start(
        &[
            LIGHTS_LISTED,
            listed("vault", &[], &["vault_delete"], vault),
        ],
        "",
    )
    .await
    .unwrap();
    let path = "/api/call/vault_delete";
    let call = |arguments: Value| {
        json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": "vault_delete", "arguments": arguments }
        })
    };
    let input = json!({"entry": 9});
    for headers in [
        "Origin: http://foreign.invalid\r\n",
        "Tailscale-Funnel-Request: true\r\n",
    ] {
        let (api, body) = post(&node, path, input.clone(), headers).await;
        let (mcp, _) = post(&node, "/mcp", call(input.clone()), headers).await;
        assert_eq!((api, mcp), (403, 403), "{headers}");
        assert_eq!(
            serde_json::from_str::<Value>(&body).unwrap()["error"]["code"],
            "forbidden"
        );
    }
    let (status, body) = post(&node, path, json!({"extra": true}), "").await;
    assert_eq!(status, 400, "{body}");
    let refusal: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(refusal["error"]["code"], "invalid_argument");
    let message = refusal["error"]["message"].as_str().unwrap();
    assert!(
        message.contains("extra") && message.contains("accepted fields: entry"),
        "{message}"
    );
    for (path, args) in [
        (format!("{path}?node=elsewhere"), input.clone()),
        (path.into(), json!({"node":"elsewhere","entry":9})),
    ] {
        let (status, body) = post(&node, &path, args, "").await;
        assert_eq!(status, 421, "{body}");
        assert_eq!(
            serde_json::from_str::<Value>(&body).unwrap()["error"]["code"],
            "unknown_node"
        );
    }
    {
        let name = "vault_absent";
        let (status, body) = post(&node, &format!("/api/call/{name}"), json!({}), "").await;
        assert_eq!(status, 404, "{body}");
        let refusal: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(refusal["error"]["code"], "unknown_command");
        assert!(refusal["error"]["message"].as_str().unwrap().contains(name));
    }
    let (status, body) = post(&node, path, input.clone(), "").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap(),
        json!({"entry":9,"count":1})
    );
    let (status, body) = rpc(&node, "/mcp", call(input)).await;
    assert_eq!(status, 200, "{body}");
    let body: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        body["result"]["structuredContent"],
        json!({"entry":9,"count":2})
    );
    node.stop().await.unwrap();
}

fn tool_names(answer: &str) -> Vec<String> {
    let answer: Value = serde_json::from_str(answer).unwrap();
    answer["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_owned())
        .collect()
}

/// Each area is an MCP server of its own, the node's own commands another,
/// and `/mcp` every one: a client pays only for the tools it asked for, and
/// the command line still sees every command.
#[tokio::test(flavor = "multi_thread")]
async fn each_area_is_an_mcp_server_and_the_cli_sees_them_all() {
    let shades_listed = listed("shades", &[], &["shades_close"], shades);
    let node = start(&[LIGHTS_LISTED, shades_listed], "").await.unwrap();
    let list = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" });
    let names = async |path: &str| {
        let (status, body) = rpc(&node, path, list.clone()).await;
        assert_eq!(status, 200, "{path}: {body}");
        tool_names(&body)
    };
    assert_eq!(names("/mcp/lights").await, ["lights_switch"]);
    assert_eq!(names("/mcp/shades").await, ["shades_close"]);
    assert_eq!(
        names("/mcp/node").await,
        ["config_show", "view_show", "view_open", "view_send"]
    );
    assert_eq!(
        names("/mcp/shades,lights").await,
        ["lights_switch", "shades_close"],
        "several, in the node's order"
    );
    assert_eq!(
        names("/mcp").await,
        [
            "lights_switch",
            "shades_close",
            "config_show",
            "view_show",
            "view_open",
            "view_send"
        ]
    );

    // No tool lists `node`, nor an empty `required`.
    let (_, body) = rpc(&node, "/mcp", list.clone()).await;
    let tools: Value = serde_json::from_str(&body).unwrap();
    for tool in tools["result"]["tools"].as_array().unwrap() {
        let schema = &tool["inputSchema"];
        assert!(schema["properties"].get("node").is_none(), "{tool}");
        assert!(schema.get("required") != Some(&json!([])), "{tool}");
    }

    // A server says which it is and where the others are.
    let initialize = json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} });
    let (_, body) = rpc(&node, "/mcp/lights", initialize).await;
    let said: Value = serde_json::from_str(&body).unwrap();
    let instructions = said["result"]["instructions"].as_str().unwrap();
    assert!(instructions.contains("lights tools"), "{instructions}");
    assert!(
        instructions.contains("/mcp/shades, /mcp/node"),
        "{instructions}"
    );

    // A tool asked of another server is refused, naming its own.
    let call = json!({
        "jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": { "name": "shades_close", "arguments": {} },
    });
    let (_, body) = rpc(&node, "/mcp/lights", call.clone()).await;
    let refused: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        refused["error"]["message"],
        "Unknown tool: shades_close; it is on /mcp/shades"
    );
    let (_, body) = rpc(&node, "/mcp/shades", call).await;
    let answered: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(answered["result"]["content"][0]["text"], r#""closed""#);

    // A server there is not is 404, naming those there are.
    let (status, body) = rpc(&node, "/mcp/lights,nope", list.clone()).await;
    assert_eq!(status, 404);
    assert_eq!(
        body,
        "no MCP server nope; this node's are lights, shades, node"
    );

    // The command line reads every server in one list.
    let url = format!("http://{}/", node.address);
    let index = ahjo_node_kit::cli::index(Some(&url)).await.unwrap();
    for noun in ["lights", "shades", "config", "view"] {
        assert!(index.contains(&format!("  {noun} ")), "{noun}: {index}");
    }
    node.stop().await.unwrap();
}

/// A clock: one command that reads and one that destroys.
struct Clock;

impl Area for Clock {
    fn commands(&self) -> Vec<Command> {
        /// Read the clock's time.
        #[derive(serde::Deserialize, schemars::JsonSchema)]
        struct Read {}

        /// Reset the clock, losing its time.
        #[derive(serde::Deserialize, schemars::JsonSchema)]
        struct Reset {}

        vec![
            Command::typed("clock_read", Effect::Reads, |_: Read| async { Ok("noon") }),
            Command::typed("clock_reset", Effect::Destroys, |_: Reset| async {
                Ok("reset")
            }),
        ]
    }
}

fn clock(_: &Setup) -> anyhow::Result<Built> {
    Built::now(Clock)
}

const CLOCK_LISTED: Listing = listed("clock", &[], &["clock_read", "clock_reset"], clock);

/// A set for a client that wants the clock and to reach the rest.
const HOME: ToolSet = ToolSet {
    name: "home",
    tools: &["clock_read", "swarm_search", "swarm_read", "swarm_run"],
    instructions: "The home: read the clock, find the rest.",
};

/// One `tools/call` on `path`: the tool's answer, or its error's code.
async fn called(node: &Running, path: &str, tool: &str, arguments: Value) -> Result<Value, String> {
    let call = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": { "name": tool, "arguments": arguments },
    });
    let (status, body) = rpc(node, path, call).await;
    assert_eq!(status, 200, "{body}");
    let answer: Value = serde_json::from_str(&body).unwrap();
    let result = &answer["result"];
    let text: Value = serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    if result["isError"] == true {
        Err(text["code"].as_str().unwrap().to_owned())
    } else {
        Ok(text)
    }
}

/// Every tool says what it does to the world, from its command's effect,
/// so a client asks before a write and not before a read.
#[tokio::test(flavor = "multi_thread")]
async fn every_tool_is_annotated_from_its_commands_effect() {
    let node = start(&[LIGHTS_LISTED, CLOCK_LISTED], "").await.unwrap();
    let list = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" });
    let (_, body) = rpc(&node, "/mcp", list).await;
    let answer: Value = serde_json::from_str(&body).unwrap();
    let tools = answer["result"]["tools"].as_array().unwrap();
    let tool = |name: &str| tools.iter().find(|t| t["name"] == name).unwrap().clone();
    assert_eq!(tool("clock_read")["title"], "Clock read");
    assert_eq!(
        tool("clock_read")["annotations"],
        json!({ "readOnlyHint": true, "openWorldHint": false })
    );
    assert_eq!(
        tool("lights_switch")["annotations"],
        json!({ "readOnlyHint": false, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false })
    );
    assert_eq!(
        tool("clock_reset")["annotations"],
        json!({ "readOnlyHint": false, "destructiveHint": true, "openWorldHint": false })
    );
    for tool in tools {
        assert!(tool["annotations"]["readOnlyHint"].is_boolean(), "{tool}");
    }
    node.stop().await.unwrap();
}

/// A tool set is an MCP server of chosen commands from any area, with
/// tools of its own that find and run every other command, each only for
/// what it is: a read through `swarm_read`, a change through `swarm_run`.
#[tokio::test(flavor = "multi_thread")]
async fn a_tool_set_serves_its_commands_and_reaches_the_rest_as_they_are() {
    let node = start_with(&[LIGHTS_LISTED, CLOCK_LISTED], &[HOME], "")
        .await
        .unwrap();
    let list = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" });
    let (status, body) = rpc(&node, "/mcp/home", list.clone()).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        tool_names(&body),
        ["clock_read", "swarm_search", "swarm_read", "swarm_run"]
    );
    let answer: Value = serde_json::from_str(&body).unwrap();
    let annotations = |at: usize| answer["result"]["tools"][at]["annotations"].clone();
    assert_eq!(annotations(2)["readOnlyHint"], true, "swarm_read");
    assert_eq!(annotations(3)["destructiveHint"], true, "swarm_run");
    let (_, every) = rpc(&node, "/mcp", list.clone()).await;
    assert!(
        !tool_names(&every).iter().any(|t| t.starts_with("swarm_")),
        "the set's own tools are only in the set"
    );
    let initialize = json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} });
    let (_, body) = rpc(&node, "/mcp/home", initialize).await;
    let said: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        said["result"]["instructions"],
        "The home: read the clock, find the rest."
    );
    let (status, body) = rpc(&node, "/mcp/home,clock", list.clone()).await;
    assert_eq!(status, 404, "{body}");

    // What it lists it runs; what it does not, it says how to reach.
    assert_eq!(
        called(&node, "/mcp/home", "clock_read", json!({})).await,
        Ok(json!("noon"))
    );
    let call = json!({
        "jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": { "name": "clock_reset", "arguments": {} },
    });
    let (_, body) = rpc(&node, "/mcp/home", call).await;
    assert!(
        body.contains("Unknown tool: clock_reset; run it here with swarm_read or swarm_run"),
        "{body}"
    );

    // Found by words, with how to run each.
    let found = called(
        &node,
        "/mcp/home",
        "swarm_search",
        json!({ "query": "reset clock" }),
    )
    .await
    .unwrap();
    assert_eq!(found["commands"][0]["name"], "clock_reset", "{found}");
    assert_eq!(found["commands"][0]["run_with"], "swarm_run");

    // A read runs as a read, a change as a change, never the other way.
    let read = async |tool: &str, command: &str, arguments: Value| {
        called(
            &node,
            "/mcp/home",
            tool,
            json!({ "command": command, "arguments": arguments }),
        )
        .await
    };
    assert_eq!(
        read("swarm_read", "clock_read", json!({})).await,
        Ok(json!({"result": "noon"}))
    );
    assert_eq!(
        read("swarm_read", "clock_reset", json!({})).await,
        Err("not_read_only".into())
    );
    assert_eq!(
        read("swarm_read", "lights_switch", json!({ "level": 2 })).await,
        Err("not_read_only".into())
    );
    assert_eq!(
        read("swarm_run", "clock_read", json!({})).await,
        Err("read_only".into())
    );
    assert_eq!(
        read("swarm_run", "lights_switch", json!({ "level": 2 })).await,
        Ok(json!({ "level": 2 }))
    );
    // The command's own checks and the node's still hold.
    assert_eq!(
        read("swarm_run", "lights_switch", json!({})).await,
        Err("no_level".into())
    );
    assert_eq!(
        read("swarm_run", "clock_reset", json!({ "node": "elsewhere" })).await,
        Err("unknown_node".into())
    );
    assert_eq!(
        read("swarm_run", "config_show", json!({})).await,
        Err("read_only".into()),
        "the node's own commands are reached too"
    );
    assert_eq!(
        read("swarm_read", "nope", json!({})).await,
        Err("unknown_command".into())
    );
    node.stop().await.unwrap();
}

/// A set that would shadow a server or name a tool no area has refuses to
/// start.
#[tokio::test(flavor = "multi_thread")]
async fn a_tool_set_naming_what_is_not_there_refuses_to_start() {
    for (set, says) in [
        (
            ToolSet {
                name: "clock",
                ..HOME
            },
            "has the name of another MCP server",
        ),
        (
            ToolSet {
                tools: &["clock_read", "clock_wind"],
                ..HOME
            },
            "names clock_wind, which no area lists",
        ),
        (
            ToolSet {
                tools: &["clock_read", "clock_read"],
                ..HOME
            },
            "names clock_read twice",
        ),
    ] {
        let error = match start_with(&[CLOCK_LISTED], &[set], "").await {
            Ok(node) => {
                node.stop().await.unwrap();
                panic!("{} started", set.name);
            }
            Err(error) => format!("{error:#}"),
        };
        assert!(error.contains(says), "{error}");
    }
    // A set whose area is turned off serves what remains.
    let node = start_with(&[CLOCK_LISTED], &[HOME], "[clock]\nenabled = false\n")
        .await
        .unwrap();
    let list = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" });
    let (_, body) = rpc(&node, "/mcp/home", list).await;
    assert_eq!(
        tool_names(&body),
        ["swarm_search", "swarm_read", "swarm_run"]
    );
    assert_eq!(
        called(
            &node,
            "/mcp/home",
            "swarm_read",
            json!({ "command": "clock_read" })
        )
        .await,
        Err("unknown_command".into())
    );
    node.stop().await.unwrap();
}

/// An area turned off is said to be, not unknown.
#[tokio::test(flavor = "multi_thread")]
async fn an_area_turned_off_is_no_mcp_server_and_says_so() {
    let node = start(&[LIGHTS_LISTED], "[lights]\nenabled = false\n")
        .await
        .unwrap();
    let list = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" });
    let (status, body) = rpc(&node, "/mcp/lights", list).await;
    assert_eq!(
        (status, body.as_str()),
        (404, "the area lights is turned off on this node")
    );
    node.stop().await.unwrap();
}

/// The node's typed config command shows the file as it loaded it.
#[tokio::test(flavor = "multi_thread")]
async fn config_show_answers_the_file_and_each_section() {
    let node = start(
        &[listed("files", &[], &[], files), LIGHTS_LISTED],
        "[files]\nroots = [\"/srv/notes\"]\n[lights]\nenabled = true\n",
    )
    .await
    .unwrap();
    let url = format!("http://{}/", node.address);
    let (code, all) = ahjo("config", &["show"], &url).await;
    assert_eq!(code, 0);
    let path = all["path"].as_str().unwrap();
    assert!(path.ends_with("config.toml"), "{path}");
    assert_eq!(all["sections"]["files"]["roots"], json!(["/srv/notes"]));
    assert_eq!(all["sections"]["lights"]["enabled"], json!(true));

    let (code, files) = ahjo("config", &["show", "--section", "files"], &url).await;
    assert_eq!(code, 0);
    assert_eq!(
        files["sections"],
        json!({"files": {"roots": ["/srv/notes"]}})
    );
    let (code, _) = ahjo("config", &["show", "--section", "nope"], &url).await;
    assert_eq!(code, 1);

    let client = NodeClient::new(Some(&url)).unwrap();
    let (answer, _) = client
        .request(
            Method::POST,
            "/api/call/config_show",
            Some(json!({"section":"files"})),
            None,
        )
        .await
        .unwrap();
    assert_eq!(answer, files);
    let refused = client
        .request(
            Method::POST,
            "/api/call/config_show",
            Some(json!({"section":"nope"})),
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(
        refused.downcast_ref::<NodeError>().unwrap().code,
        "unknown_section"
    );

    let list = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"});
    let (tools, _) = client
        .request(Method::POST, "/mcp", Some(list), None)
        .await
        .unwrap();
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    for own in ["config_show", "view_show", "view_open", "view_send"] {
        assert!(names.contains(&own), "{names:?}");
    }
    node.stop().await.unwrap();
}

/// An MCP tool call's answer: its value, or its error's code.
async fn tool(client: &NodeClient, name: &str, arguments: Value) -> Result<Value, String> {
    let call = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": { "name": name, "arguments": arguments },
    });
    let (answer, _) = client
        .request(Method::POST, "/mcp", Some(call), None)
        .await
        .unwrap();
    let text = answer["result"]["content"][0]["text"].as_str().unwrap();
    match answer["result"]["isError"].as_bool() {
        Some(false) => Ok(serde_json::from_str(text).unwrap()),
        _ => Err(serde_json::from_str::<Error>(text).unwrap().code),
    }
}

/// View control, the node's side, with a socket client standing in for the
/// app: it is an app once it says what it shows, and `view_open` reaches it
/// as a view command whose answer is the command's.
#[tokio::test(flavor = "multi_thread")]
async fn view_open_reaches_the_app_and_answers_its_answer() {
    let node = start(&[], "").await.unwrap();
    let url = format!("http://{}/", node.address);
    let client = NodeClient::new(Some(&url)).unwrap();
    assert_eq!(
        ahjo("view", &["show"], &url).await,
        (0, json!({"apps": []}))
    );
    assert_eq!(
        ahjo("view", &["open", "--page", "files"], &url).await.0,
        1,
        "no app"
    );
    let args = json!({ "page": "files" });
    assert_eq!(
        tool(&client, "view_open", args).await.unwrap_err(),
        "no_app"
    );
    // The brain's page moved to Ahjo Brain: said with or without an app.
    let call = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": { "name": "view_open", "arguments": { "page": "brain" } },
    });
    let (answer, _) = client
        .request(Method::POST, "/mcp", Some(call), None)
        .await
        .unwrap();
    let error: Error =
        serde_json::from_str(answer["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(error.code, "no_page");
    assert!(
        error
            .message
            .contains("the brain lives in the Ahjo Brain app"),
        "{}",
        error.message
    );

    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{}/ws", node.address))
        .await
        .unwrap();
    ws.next().await.unwrap().unwrap(); // the hello
    // Not an app until it says what it shows.
    assert_eq!(
        tool(&client, "view_show", json!({})).await,
        Ok(json!({"apps": []}))
    );
    ws.send(Message::Binary(
        [&[0xb0][..], br#"{"page":"sessions"}"#].concat().into(),
    ))
    .await
    .unwrap();
    let shown = loop {
        let shown = tool(&client, "view_show", json!({})).await.unwrap();
        if shown != json!({"apps": []}) {
            break shown;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    };
    assert_eq!(shown["apps"][0]["page"], "sessions");

    // Two things at once are refused before reaching the app.
    let two = json!({ "page": "files", "path": "/a", "item": "x" });
    assert_eq!(
        tool(&client, "view_open", two).await.unwrap_err(),
        "invalid_argument"
    );

    // The app is told, and its answer is the command's.
    let opening = {
        let client = NodeClient::new(Some(&url)).unwrap();
        tokio::spawn(async move {
            let args = json!({ "page": "files", "path": "/tmp/x" });
            tool(&client, "view_open", args).await
        })
    };
    let told = ws.next().await.unwrap().unwrap().into_data();
    assert_eq!(told[0], 0xb8);
    let command: Value = serde_json::from_slice(&told[5..]).unwrap();
    assert_eq!(
        command,
        json!({ "do": "open", "page": "files", "path": "/tmp/x", "node": node.node.id })
    );
    let answer = br#"{"ok":true,"view":{"page":"files"}}"#;
    ws.send(Message::Binary(
        [&[0xb1][..], &told[1..5], answer].concat().into(),
    ))
    .await
    .unwrap();
    assert_eq!(opening.await.unwrap(), Ok(json!({ "page": "files" })));

    // Text to send reaches the app as a send, answered as an open is.
    assert_eq!(
        tool(&client, "view_send", json!({ "text": " " }))
            .await
            .unwrap_err(),
        "invalid_argument"
    );
    let sending = {
        let client = NodeClient::new(Some(&url)).unwrap();
        tokio::spawn(async move {
            let args = json!({ "text": "brain page knowledge/a/b.md" });
            tool(&client, "view_send", args).await
        })
    };
    let told = ws.next().await.unwrap().unwrap().into_data();
    let command: Value = serde_json::from_slice(&told[5..]).unwrap();
    assert_eq!(
        command,
        json!({ "do": "send", "reference": "brain page knowledge/a/b.md" })
    );
    let answer = br#"{"ok":true,"view":{"page":"sessions"}}"#;
    ws.send(Message::Binary(
        [&[0xb1][..], &told[1..5], answer].concat().into(),
    ))
    .await
    .unwrap();
    assert_eq!(sending.await.unwrap(), Ok(json!({ "page": "sessions" })));

    // An app that does not answer.
    let args = json!({ "page": "files" });
    assert_eq!(
        tool(&client, "view_open", args).await.unwrap_err(),
        "timeout"
    );

    // Gone, it is no app.
    drop(ws);
    let mut shown = json!(null);
    for _ in 0..100 {
        shown = tool(&client, "view_show", json!({})).await.unwrap();
        if shown == json!({"apps": []}) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(shown, json!({"apps": []}));
    node.stop().await.unwrap();
}
