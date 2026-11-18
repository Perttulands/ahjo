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

/// What the app keeps: the desks it starts from.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub desks: Vec<Place>,
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

/// The desks named by `--desk` (repeated), else `AHJO_DESK_DESKS` (comma
/// separated), else the config file's.
pub fn load(args: &[String]) -> (Config, Option<String>) {
    let (mut config, notice) = match config_path().map(std::fs::read_to_string) {
        Some(Ok(text)) => match serde_json::from_str::<Config>(&text) {
            Ok(c) => (c, None),
            Err(e) => (Config::default(), Some(format!("desk.json: {e}"))),
        },
        _ => (Config::default(), None),
    };
    let given: Vec<Place> = args
        .windows(2)
        .filter(|w| w[0] == "--desk")
        .map(|w| Place::parse(&w[1]))
        .collect();
    if !given.is_empty() {
        config.desks = given;
    } else if let Ok(list) = std::env::var("AHJO_DESK_DESKS") {
        config.desks = list
            .split(',')
            .filter(|s| !s.trim().is_empty())
            .map(Place::parse)
            .collect();
    }
    (config, notice)
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

pub async fn asks(base: String) -> Result<Vec<Ask>, String> {
    let bytes = request(&base, "GET", "/api/asks", None, Duration::from_secs(10)).await?;
    serde_json::from_slice(&bytes).map_err(|e| format!("the asks do not read: {e}"))
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
