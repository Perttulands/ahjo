//! A web page on a node shown running (ADR 0006): the app's own scheme
//! serves the folder the page was opened in, and nothing else, each request
//! read from the node through the app's link.
//!
//! Each folder is a site of its own (an origin), named by a hash of its node
//! and path, so a page keeps its storage and reaches no other folder's. The
//! node confines each read to the folder again, links followed
//! ([`FOLDER_FILE_PATH`]).
//!
//! The system web view takes each answer whole. A file asked for whole (a
//! script, a stylesheet, an image) comes whole, however long; a range, as a
//! player asks for, holds at most [`MOST`] bytes from where it was asked,
//! said so, and the player asks for the rest as it plays.

use ahjo_app_kit::host::{Reach, Request};
use ahjo_app_kit::web_view::{Respond, Scheme, SchemeRequest, SchemeResponse, scheme_url};
use ahjo_core::NodeId;
use ahjo_files_contract::FOLDER_FILE_PATH;
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};

/// The scheme's name.
pub const SCHEME: &str = "ahjo-file";

/// The most bytes of a file one answer to a range holds.
pub const MOST: u64 = 8 << 20;

/// What the headers of a node's answer may tell the page.
const PASSED: &[&str] = &["content-type", "content-range", "accept-ranges"];

/// The folder a page serves from, now.
#[derive(Clone)]
struct Served {
    node: String,
    folder: String,
    site: String,
    reach: Reach,
}

/// The folder one view serves pages from. Clones share it.
#[derive(Clone, Default)]
pub struct Live {
    served: Arc<Mutex<Option<Served>>>,
}

impl Live {
    /// Serves `name` in `folder` on `node` from now on, instead of any
    /// folder before: the address the view loads it from.
    pub fn serve(&self, node: &str, folder: &str, name: &str, reach: Reach) -> String {
        let site = site(node, folder);
        let url = scheme_url(SCHEME, &site, name);
        let mut served = self.served.lock().unwrap_or_else(|e| e.into_inner());
        if !served
            .as_ref()
            .is_some_and(|s| s.node == node && s.folder == folder)
        {
            *served = Some(Served {
                node: node.into(),
                folder: folder.into(),
                site,
                reach,
            });
        }
        url
    }

    /// Whether the file at `path` on `node` is in the folder served now, so
    /// a page shown from it may load it.
    pub fn serves(&self, node: &str, path: &str) -> bool {
        let served = self.served.lock().unwrap_or_else(|e| e.into_inner());
        served.as_ref().is_some_and(|s| {
            s.node == node
                && path
                    .strip_prefix(s.folder.as_str())
                    .is_some_and(|rest| rest.starts_with('/') || s.folder.ends_with('/'))
        })
    }

    /// The scheme a view loads its pages from.
    pub fn scheme(&self) -> Scheme {
        let live = self.clone();
        Scheme {
            name: SCHEME,
            serve: Arc::new(move |asked, respond| live.answer(asked, respond)),
        }
    }

    /// Answers one request of the page.
    pub fn answer(&self, asked: SchemeRequest, respond: Respond) {
        let served = self
            .served
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let Some(served) = served.filter(|s| s.site == asked.site) else {
            return respond(SchemeResponse::refused(
                403,
                "That address is not the open page's folder.",
            ));
        };
        if asked.method != "GET" {
            return respond(SchemeResponse::refused(405, "Pages here only read."));
        }
        let path = asked.path.trim_start_matches('/');
        if path
            .split('/')
            .any(|part| part == ".." || part == "." || part.contains('\\'))
        {
            return respond(SchemeResponse::refused(
                403,
                "That is outside the page's folder.",
            ));
        }
        if path.is_empty() {
            return respond(SchemeResponse::refused(404, "No file is named."));
        }
        let uri = format!(
            "{FOLDER_FILE_PATH}?folder={}&path={}",
            enc(&served.folder),
            enc(path)
        );
        let mut request = Request::get(NodeId(served.node.clone()), uri);
        let range = asked.header("range").and_then(bounded);
        let ranged = range.is_some();
        if let Some(range) = range {
            request = request.header("Range", range);
        }
        let opened = served.reach.open(request);
        served.reach.spawn(async move {
            respond(match opened.await {
                Ok(answer) => read(answer, ranged).await,
                Err(e) => SchemeResponse::refused(502, e.message),
            })
        });
    }
}

/// The node's answer as the page gets it: the whole file when it was asked
/// whole, else the range it was asked, at most [`MOST`] bytes of it.
async fn read(mut answer: ahjo_app_kit::link::http::Streamed, ranged: bool) -> SchemeResponse {
    let mut headers: Vec<(String, String)> = answer
        .headers
        .iter()
        .filter(|(n, _)| PASSED.contains(&n.as_str()))
        .cloned()
        .collect();
    headers.push(("Cache-Control".into(), "no-store".into()));
    if !(200..300).contains(&answer.status) {
        let body = read_all(&mut answer).await.unwrap_or_default();
        let why = serde_json::from_slice::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v["message"].as_str().map(str::to_string))
            .unwrap_or_else(|| String::from_utf8_lossy(&body).into_owned());
        let mut refused = SchemeResponse::refused(answer.status, why);
        refused.headers.extend(
            headers
                .into_iter()
                .filter(|(n, _)| n == "content-range" || n == "Cache-Control"),
        );
        return refused;
    }
    // The node bounds a range already; this only guards against more.
    let most = if ranged { MOST as usize } else { usize::MAX };
    let mut body = Vec::new();
    loop {
        match answer.piece().await {
            Ok(Some(piece)) => {
                let room = most - body.len();
                body.extend_from_slice(&piece[..piece.len().min(room)]);
                if body.len() >= most {
                    break;
                }
            }
            Ok(None) => break,
            Err(e) => return SchemeResponse::refused(502, e),
        }
    }
    headers.push(("Content-Length".into(), body.len().to_string()));
    SchemeResponse {
        status: answer.status,
        headers,
        body,
    }
}

async fn read_all(answer: &mut ahjo_app_kit::link::http::Streamed) -> Result<Vec<u8>, String> {
    answer.up_to(64 << 10).await
}

/// One range a page asked for, made to hold at most [`MOST`] bytes; `None`
/// for anything but one byte range, which asks for the whole.
fn bounded(asked: &str) -> Option<String> {
    let spec = asked.trim().strip_prefix("bytes=")?.trim();
    if spec.contains(',') {
        return None;
    }
    let (from, to) = spec.split_once('-')?;
    let (from, to) = (from.trim(), to.trim());
    if from.is_empty() {
        let last: u64 = to.parse().ok()?;
        return Some(format!("bytes=-{}", last.min(MOST)));
    }
    let from: u64 = from.parse().ok()?;
    let most = from.saturating_add(MOST - 1);
    let to = if to.is_empty() {
        most
    } else {
        to.parse::<u64>().ok()?.min(most)
    };
    Some(format!("bytes={from}-{to}"))
}

/// The site a folder serves as: a hash of its node and path, so no page
/// learns either from its address.
fn site(node: &str, folder: &str) -> String {
    let hash = Sha256::digest(format!("{node}\n{folder}").as_bytes());
    hex::encode(&hash[..10])
}

fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_range_holds_at_most_one_answer() {
        assert_eq!(bounded("bytes=0-99").as_deref(), Some("bytes=0-99"));
        assert_eq!(
            bounded("bytes=10-").as_deref(),
            Some(format!("bytes=10-{}", 10 + MOST - 1).as_str())
        );
        assert_eq!(
            bounded(&format!("bytes=0-{}", 4 * MOST)).as_deref(),
            Some(format!("bytes=0-{}", MOST - 1).as_str())
        );
        assert_eq!(
            bounded(&format!("bytes=-{}", 2 * MOST)).as_deref(),
            Some(format!("bytes=-{MOST}").as_str())
        );
        assert_eq!(bounded("bytes=0-1,5-6"), None);
        assert_eq!(bounded("items=0-1"), None);
    }

    #[test]
    fn each_folder_is_a_site_of_its_own() {
        let a = site("box", "/work/a");
        assert_eq!(a.len(), 20);
        assert!(a.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_ne!(a, site("box", "/work/b"));
        assert_ne!(a, site("other", "/work/a"));
    }
}
