//! The decision desks this app answers for, each one project's desk server
//! (`/srv/desk/server.py`), reached over its HTTP API: `/api/desks` for the
//! other desks on its host, `/api/asks` for the asks, `/media/` and
//! `/thumb/` for what they show, `/api/asks/<id>/marks` for his marks and
//! `/api/asks/<id>/answer` to answer. The server validates every answer,
//! keeps it, and passes it on to Beads and the brain, so the app does none
//! of that and the agents' side never changes.

use crate::model::Ask;
use ahjo_app_kit::link::http;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// One desk: a name to show and where its server listens.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Place {
    #[serde(default)]
    pub name: String,
    pub url: String,
}

impl Place {
    /// `Name=http://host:port` or a bare URL.
    pub fn parse(s: &str) -> Self {
        match s.split_once('=') {
            Some((name, url)) if !name.contains("://") => Self {
                name: name.trim().into(),
                url: url.trim().trim_end_matches('/').into(),
            },
            _ => Self {
                name: String::new(),
                url: s.trim().trim_end_matches('/').into(),
            },
        }
    }
}

/// What the app keeps (`desk.json`): the desks it starts from and how he
/// set it up. What it does not know of the file is kept as it reads.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub desks: Vec<Place>,
    /// The theme's direction by its key; empty, the one chosen in Ahjo
    /// Swarm.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub theme: String,
    /// Which asks come first in the waiting list.
    pub order: Order,
    /// Enter sends; Shift+Enter starts a new line. Otherwise Enter starts
    /// a new line and Ctrl+Enter sends.
    pub enter_sends: bool,
    /// Tell the asking agent's session when an answer is sent, through
    /// Ahjo Swarm's node.
    pub tell_session: bool,
    /// The list of asks put away, for the answer's and the picture's room.
    pub fold_list: bool,
    /// The list's and the answer's widths as he dragged their edges; none,
    /// a share of the window.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list_w: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub talk_w: Option<f32>,
    /// The node that reaches the sessions; empty, the one Ahjo Swarm
    /// talks to.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub node: String,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            desks: vec![],
            theme: String::new(),
            order: Order::default(),
            enter_sends: false,
            tell_session: true,
            fold_list: false,
            list_w: None,
            talk_w: None,
            node: String::new(),
            rest: Default::default(),
        }
    }
}

/// How the waiting list is ordered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Order {
    /// The quickest to answer first, then the longest waiting: each answer
    /// frees an agent, and the quick ones free the most soonest.
    #[default]
    Quickest,
    /// The longest waiting first.
    Oldest,
    /// The newest first.
    Newest,
}

impl Order {
    pub const ALL: [Order; 3] = [Order::Quickest, Order::Oldest, Order::Newest];

    pub fn label(self) -> &'static str {
        match self {
            Order::Quickest => "Quickest first",
            Order::Oldest => "Oldest first",
            Order::Newest => "Newest first",
        }
    }
}

impl Config {
    /// The node that reaches the sessions: as set here, else Ahjo Swarm's.
    pub fn node(&self) -> String {
        if !self.node.trim().is_empty() {
            return self.node.trim().trim_end_matches('/').to_string();
        }
        ahjo_app_kit::settings::path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| ahjo_app_kit::settings::Settings::parse(&t))
            .map(|s| s.node)
            .unwrap_or_else(|| ahjo_app_kit::settings::DEFAULT_NODE.into())
            .trim_end_matches('/')
            .to_string()
    }
}

/// Writes the settings to `desk.json`, whole.
pub fn write_config(config: &Config) -> Result<(), String> {
    let Some(path) = config_path() else {
        return Err("there is no place for the Desk's settings".into());
    };
    let text = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    write_drafts(&path, &text)
}

/// `AHJO_DESK_CONFIG`, else `desk.json` beside Ahjo Swarm's `app.json`
/// (`%APPDATA%\Ahjo`, `$XDG_CONFIG_HOME/ahjo`).
pub fn config_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("AHJO_DESK_CONFIG") {
        return Some(p.into());
    }
    ahjo_app_kit::settings::path().map(|p| p.with_file_name("desk.json"))
}

/// His unsent answers and marks, beside `desk.json`, so closing the window
/// loses none of them.
pub fn drafts_path() -> Option<PathBuf> {
    config_path().map(|p| p.with_file_name("desk-drafts.json"))
}

/// The drafts kept at `path`; none when it was never written.
pub fn read_drafts(
    path: &std::path::Path,
) -> Result<std::collections::HashMap<crate::answer::DraftKey, crate::answer::Draft>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => crate::answer::kept(&text).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Default::default()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Writes the drafts whole, so a window closed mid-write leaves the last
/// whole file rather than half of one.
pub fn write_drafts(path: &std::path::Path, text: &str) -> Result<(), String> {
    let fail = |e: std::io::Error| format!("{}: {e}", path.display());
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(fail)?;
    }
    let part = path.with_extension("json.part");
    std::fs::write(&part, text).map_err(fail)?;
    std::fs::rename(&part, path).map_err(fail)
}

/// The whole app's size once zoomed here (`zoom::app`), beside `desk.json`:
/// the Desk opens at Ahjo Swarm's until it has its own.
pub fn size_path() -> Option<PathBuf> {
    config_path().map(|p| p.with_file_name("desk-size.json"))
}

#[derive(Default, Serialize, Deserialize)]
struct Kept {
    size: Option<u16>,
}

/// The size kept at `path`; none when it was never written.
pub fn read_size(path: &std::path::Path) -> Result<Option<u16>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str::<Kept>(&text)
            .map(|k| k.size)
            .map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Keeps `size` at `path`, whole.
pub fn write_size(path: &std::path::Path, size: u16) -> Result<(), String> {
    let text = serde_json::to_string(&Kept { size: Some(size) }).map_err(|e| e.to_string())?;
    write_drafts(path, &text)
}

/// What the Desk starts from: its settings as `desk.json` keeps them, the
/// desks it opens on, and whether the file read (or was never written), so
/// a change can be written back without losing anything.
pub struct Loaded {
    pub config: Config,
    pub places: Vec<Place>,
    pub notice: Option<String>,
    pub kept: bool,
}

/// The settings in `desk.json`, and the desks named by `--desk`
/// (repeated), else `AHJO_DESK_DESKS` (comma separated), else the file's.
pub fn load(args: &[String]) -> Loaded {
    let (config, notice, kept) = match config_path().map(std::fs::read_to_string) {
        Some(Ok(text)) => match serde_json::from_str::<Config>(&text) {
            Ok(c) => (c, None, true),
            Err(e) => (
                Config::default(),
                Some(format!(
                    "desk.json does not read, so settings are not kept: {e}"
                )),
                false,
            ),
        },
        _ => (Config::default(), None, true),
    };
    let given: Vec<Place> = args
        .windows(2)
        .filter(|w| w[0] == "--desk")
        .map(|w| Place::parse(&w[1]))
        .collect();
    let places = if !given.is_empty() {
        given
    } else if let Ok(list) = std::env::var("AHJO_DESK_DESKS") {
        list.split(',')
            .filter(|s| !s.trim().is_empty())
            .map(Place::parse)
            .collect()
    } else {
        config.desks.clone()
    };
    Loaded {
        config,
        places,
        notice,
        kept,
    }
}

/// The runtime the requests run on, off the UI thread.
pub fn runtime() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("desk-net")
            .enable_all()
            .build()
            .expect("the network runtime starts")
    })
}

/// A request's answer: its body, or the server's words for why not.
async fn request(
    base: &str,
    method: &str,
    path: &str,
    body: Option<Vec<u8>>,
    within: Duration,
) -> Result<Vec<u8>, String> {
    let body_ref = body.as_ref().map(|b| ("application/json", b.as_slice()));
    let (status, bytes) = http::send(base, method, path, &[], body_ref, within)
        .await
        .map_err(|e| {
            if e.maybe_done {
                format!("{e}; it may have reached the desk, so look before answering again")
            } else {
                format!("the desk did not answer: {e}")
            }
        })?;
    if (200..300).contains(&status) {
        Ok(bytes)
    } else {
        Err(http::error_text(status, &bytes))
    }
}

/// Why a desk's asks were not read.
#[derive(Clone, Debug, PartialEq)]
pub enum Unread {
    /// Nothing answered: the desk is not running or not reachable from here.
    Away(String),
    /// The desk answered, but not with its asks.
    Wrong(String),
}

pub async fn asks(base: String) -> Result<Vec<Ask>, Unread> {
    let (status, bytes) = http::send(
        &base,
        "GET",
        "/api/asks",
        &[],
        None,
        Duration::from_secs(10),
    )
    .await
    .map_err(|e| Unread::Away(e.to_string()))?;
    if !(200..300).contains(&status) {
        return Err(Unread::Wrong(http::error_text(status, &bytes)));
    }
    serde_json::from_slice(&bytes).map_err(|e| Unread::Wrong(format!("the asks do not read: {e}")))
}

/// A desk on another project, as `/api/desks` names it.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Listed {
    pub name: String,
    pub port: u16,
}

pub async fn desks(base: String) -> Result<Vec<Listed>, String> {
    let bytes = request(&base, "GET", "/api/desks", None, Duration::from_secs(10)).await?;
    serde_json::from_slice(&bytes).map_err(|e| format!("the desks do not read: {e}"))
}

/// The listed desks as places on the host `base` is on: the app reaches
/// them where it reaches that one.
pub fn siblings(base: &str, listed: &[Listed]) -> Vec<Place> {
    let (scheme, rest) = base.split_once("://").unwrap_or(("http", base));
    let host = rest.split('/').next().unwrap_or(rest);
    let host = match host.rsplit_once(':') {
        Some((h, port)) if port.chars().all(|c| c.is_ascii_digit()) => h,
        _ => host,
    };
    listed
        .iter()
        .filter(|d| d.port != 0)
        .map(|d| Place {
            name: d.name.clone(),
            url: format!("{scheme}://{host}:{}", d.port),
        })
        .collect()
}

/// Two addresses of the same desk (`localhost` is `127.0.0.1`).
pub fn same_place(a: &str, b: &str) -> bool {
    let norm = |u: &str| {
        u.trim_end_matches('/')
            .replace("://localhost", "://127.0.0.1")
            .to_ascii_lowercase()
    };
    norm(a) == norm(b)
}

/// Done on a hands job: the desk runs its check on `out/`; a pass
/// answers the ask, and the change feed says how it came out.
pub async fn done(base: String, id: String, round: u32, words: String) -> Result<(), String> {
    let body = serde_json::json!({"round": round, "comment": words});
    request(
        &base,
        "POST",
        &format!("/api/asks/{id}/done"),
        Some(serde_json::to_vec(&body).map_err(|e| e.to_string())?),
        Duration::from_secs(20),
    )
    .await
    .map(|_| ())
}

/// Keeps a picture he pasted on the desk, for his answer to ask `id`: the
/// desk answers with the name it keeps it under and where.
pub async fn attach(
    base: String,
    id: String,
    bytes: Vec<u8>,
) -> Result<crate::model::Attached, String> {
    let (status, body) = http::send(
        &base,
        "POST",
        &format!("/api/asks/{id}/attachments"),
        &[],
        Some(("application/octet-stream", &bytes)),
        Duration::from_secs(60),
    )
    .await
    .map_err(|e| format!("the desk did not answer: {e}"))?;
    match status {
        200..=299 => serde_json::from_slice(&body)
            .map_err(|e| format!("the desk's answer does not read: {e}")),
        // A desk server from before pictures: it says so in its own way.
        404 => Err("this desk takes no pictures yet: it needs restarting (desk@<project>)".into()),
        s => Err(http::error_text(s, &body)),
    }
}

pub fn attachment_path(ask: &str, id: &str) -> String {
    format!("/attachment/{ask}/{id}")
}

/// Types `note` into session `session` through the node at `node` and
/// presses Enter (the node's `session_send`).
pub async fn tell(node: String, session: String, note: String) -> Result<(), String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "session_send", "arguments": {"session": session, "text": note, "enter": true}},
    });
    let bytes = request(
        &node,
        "POST",
        "/mcp",
        Some(serde_json::to_vec(&body).map_err(|e| e.to_string())?),
        Duration::from_secs(10),
    )
    .await
    .map_err(|e| {
        e.replace(
            "the desk did not answer",
            "Ahjo Swarm's node did not answer",
        )
    })?;
    let v: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|e| format!("the node's answer does not read: {e}"))?;
    if let Some(e) = v.get("error") {
        return Err(e["message"]
            .as_str()
            .unwrap_or("the node refused")
            .to_string());
    }
    // A tool that failed answers with its words as the result.
    if v["result"]["isError"].as_bool() == Some(true) {
        let words = v["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or("the send failed");
        return Err(words.to_string());
    }
    Ok(())
}

/// Whether the node at `node` is there: the device it runs on.
pub async fn node_health(node: String) -> Result<String, String> {
    let bytes = request(&node, "GET", "/api/health", None, Duration::from_secs(5))
        .await
        .map_err(|e| e.replace("the desk did not answer", "the node did not answer"))?;
    let v: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|e| format!("the node's answer does not read: {e}"))?;
    match v["status"].as_str() {
        Some("ok") => Ok(v["device"].as_str().unwrap_or("").to_string()),
        _ => Err("that is not an Ahjo node".into()),
    }
}

/// The note an agent's session gets when he answers its ask.
pub fn note(ask: &Ask) -> String {
    let said = ask
        .answer
        .as_ref()
        .map(|a| match a.verdict.as_str() {
            "pick" => format!("picked {}", a.options.join(", ")),
            "accept" => "looks good".into(),
            "acknowledge" => "done; the check passed".into(),
            "send back" => "sent it back".into(),
            "elsewhere" => "answered elsewhere".into(),
            "grill" => format!(
                "answered {} of {} questions",
                a.answers.len(),
                ask.questions.len()
            ),
            _ => "answered in his words".into(),
        })
        .unwrap_or_else(|| "answered".into());
    format!(
        "Desk: WisQi answered {} ({said}). Read it with: desk show {}",
        ask.id, ask.id
    )
}

/// Puts his marks on an ask (the whole list), refused when the ask moved
/// on since `revision`.
pub async fn save_marks(
    base: String,
    id: String,
    marks: &[crate::model::Mark],
    revision: u64,
) -> Result<Ask, String> {
    let body = serde_json::to_vec(&serde_json::json!({"marks": marks, "revision": revision}))
        .map_err(|e| e.to_string())?;
    let bytes = request(
        &base,
        "POST",
        &format!("/api/asks/{id}/marks"),
        Some(body),
        Duration::from_secs(15),
    )
    .await?;
    serde_json::from_slice(&bytes).map_err(|e| format!("the desk's answer does not read: {e}"))
}

/// The bytes of one of an ask's media, whole (`/media/`) or as the desk's
/// JPEG preview (`/thumb/`, pictures only).
pub async fn media(base: String, path: String) -> Result<Vec<u8>, String> {
    request(&base, "GET", &path, None, Duration::from_secs(120)).await
}

pub fn media_path(ask: &str, round: u32, media: &str, thumb: bool) -> String {
    format!(
        "/{}/{ask}/r{round}/{media}",
        if thumb { "thumb" } else { "media" }
    )
}

/// Answers an ask; the desk answers with the ask as it now is.
pub async fn answer(base: String, id: String, payload: serde_json::Value) -> Result<Ask, String> {
    let body = serde_json::to_vec(&payload).map_err(|e| e.to_string())?;
    let bytes = request(
        &base,
        "POST",
        &format!("/api/asks/{id}/answer"),
        Some(body),
        Duration::from_secs(15),
    )
    .await?;
    serde_json::from_slice(&bytes).map_err(|e| format!("the desk's answer does not read: {e}"))
}

/// Follows a desk's `/events` while `live` holds `generation`: each change
/// it reports calls `changed`. The server ends a stream after a minute;
/// it is opened again until the generation moves on.
pub async fn follow(
    base: String,
    live: Arc<AtomicU64>,
    generation: u64,
    changed: impl Fn() + Send + 'static,
) {
    let host = base
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .to_string();
    while live.load(Ordering::Relaxed) == generation {
        let stream = async {
            let mut s = tokio::net::TcpStream::connect(&host).await.ok()?;
            let req = format!(
                "GET /events HTTP/1.1\r\nHost: {host}\r\nAccept: text/event-stream\r\n\r\n"
            );
            s.write_all(req.as_bytes()).await.ok()?;
            let mut lines = BufReader::new(s).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if live.load(Ordering::Relaxed) != generation {
                    return Some(());
                }
                if line.starts_with("event: change") {
                    changed();
                }
            }
            Some(())
        };
        if stream.await.is_none() {
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_keep_what_they_do_not_know_and_start_as_he_would_want() {
        let c: Config = serde_json::from_str("{}").unwrap();
        assert!(c.tell_session && !c.enter_sends && c.order == Order::Quickest);
        let text =
            r#"{"desks":[{"url":"http://h:1"}],"theme":"nord","order":"oldest","later":{"x":1}}"#;
        let c: Config = serde_json::from_str(text).unwrap();
        assert_eq!((c.theme.as_str(), c.order), ("nord", Order::Oldest));
        let back: serde_json::Value = serde_json::to_value(&c).unwrap();
        assert_eq!(
            back["later"]["x"], 1,
            "a setting from a later Desk survives"
        );
        assert_eq!(back["desks"][0]["url"], "http://h:1");
        let c = Config {
            node: " http://127.0.0.1:7310/ ".into(),
            ..Config::default()
        };
        assert_eq!(c.node(), "http://127.0.0.1:7310");
    }

    #[test]
    fn the_note_says_who_answered_what_and_how_to_read_it() {
        let ask: Ask = serde_json::from_str(
            r#"{"id":"AQ7","kind":"decide","status":"answered","session":"aq-lead","answer":{"verdict":"pick","options":["B"]}}"#,
        )
        .unwrap();
        let n = note(&ask);
        assert!(
            n.contains("AQ7") && n.contains("picked B") && n.contains("desk show AQ7"),
            "{n}"
        );
        assert!(!n.contains('\n'), "one line, so Enter sends it whole");
    }

    #[test]
    fn a_desk_is_named_or_bare() {
        assert_eq!(
            Place::parse("Korpi=http://127.0.0.1:8778/"),
            Place {
                name: "Korpi".into(),
                url: "http://127.0.0.1:8778".into()
            }
        );
        assert_eq!(Place::parse("http://127.0.0.1:8774").name, "");
        let listed = [
            Listed {
                name: "Korpi".into(),
                port: 8778,
            },
            Listed {
                name: "Broken".into(),
                port: 0,
            },
        ];
        let found = siblings("http://100.64.0.7:8774", &listed);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].url, "http://100.64.0.7:8778");
        assert!(same_place(
            "http://localhost:8778/",
            "http://127.0.0.1:8778"
        ));
        assert_eq!(
            media_path("KQ3", 2, "m1", true),
            "/thumb/KQ3/r2/m1".to_string()
        );
    }
}

#[cfg(test)]
mod size_tests {
    use super::*;

    #[test]
    fn the_app_size_is_kept_and_read_back() {
        let dir = std::env::temp_dir().join(format!("desk-size-{}", std::process::id()));
        let path = dir.join("desk-size.json");
        assert_eq!(read_size(&path), Ok(None));
        write_size(&path, 115).unwrap();
        assert_eq!(read_size(&path), Ok(Some(115)));
        std::fs::write(&path, "{not json").unwrap();
        assert!(read_size(&path).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A desk that is not running is away, which the app keeps quiet; one
    /// that answers wrongly is an error he sees.
    #[test]
    fn a_desk_that_does_not_answer_is_away_and_one_that_answers_wrongly_is_an_error() {
        use std::io::{Read, Write};
        let serve = |reply: &'static str| {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            std::thread::spawn(move || {
                let (mut s, _) = listener.accept().unwrap();
                let mut buf = [0u8; 1024];
                let _ = s.read(&mut buf);
                let _ = s.write_all(reply.as_bytes());
            });
            url
        };
        let read = |url: String| runtime().block_on(asks(url));
        // Accepted and closed without a word: nothing answered.
        assert!(matches!(read(serve("")), Err(Unread::Away(_))));
        let broken = "HTTP/1.1 500 Internal Server Error\r\ncontent-length: 4\r\nconnection: close\r\n\r\nboom";
        assert!(matches!(read(serve(broken)), Err(Unread::Wrong(_))));
        let garbled = "HTTP/1.1 200 OK\r\ncontent-length: 3\r\nconnection: close\r\n\r\n{x}";
        assert!(matches!(read(serve(garbled)), Err(Unread::Wrong(_))));
        let fine = "HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\n[]";
        assert_eq!(read(serve(fine)).unwrap().len(), 0);
    }
}
