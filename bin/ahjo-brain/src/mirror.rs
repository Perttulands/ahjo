//! Another machine's brain, read through its Ahjo node and kept in this
//! machine's cache, using the node's typed file, text, and search commands.
//! The copy is read as a brain, read only.
//!
//! The copy is the node's corpus as it last answered; a page the node no
//! longer lists is removed from it. Each file keeps the time it changed on
//! its own machine, so what changed lately reads as it does there.

use crate::citadel::{self, Hit};
use ahjo_brain_contract::commands::{BrainFiles, BrainSearch, BrainTexts};
use ahjo_brain_contract::{File, TypedCommand};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

/// The machine a brain is read from, by the name it goes by.
#[derive(Clone, Debug, PartialEq)]
pub struct Remote {
    pub name: String,
    /// Its node, `http://host:port`.
    pub url: String,
}

const WHO: &str = "The other machine's node";
const WAIT: Duration = Duration::from_secs(30);
/// The node reads at most this many pages at a time, and a batch is cut
/// sooner by its size so its answer stays under `ANSWER`.
const BATCH: usize = 200;
const BATCH_BYTES: u64 = 4 << 20;
const ANSWER: u64 = 64 << 20;
const MANIFEST: &str = "manifest.json";

/// Where the copy of `url`'s brain is kept under the cache.
pub fn home(cache: &Path, url: &str) -> PathBuf {
    let name: String = url
        .trim_start_matches("http://")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    cache.join("from").join(name)
}

/// The copy's pages, the brain root the app reads.
pub fn pages(home: &Path) -> PathBuf {
    home.join("pages")
}

/// What a copy took.
#[derive(Debug, Default, PartialEq)]
pub struct Synced {
    pub fetched: usize,
    pub removed: usize,
}

impl Synced {
    pub fn changed(&self) -> bool {
        self.fetched + self.removed > 0
    }
}

/// Brings the copy in `home` up to the node at `url`. Blocking: run it off
/// the UI.
pub fn sync(url: &str, home: &Path) -> Result<Synced, String> {
    let listed = call(url, BrainFiles {})?;
    let root = pages(home);
    std::fs::create_dir_all(&root).map_err(|e| format!("{}: {e}", root.display()))?;
    let mut kept = read_manifest(home);
    let wanted: BTreeMap<&str, &File> = listed
        .files
        .iter()
        .filter(|f| safe(&f.path))
        .map(|f| (f.path.as_str(), f))
        .collect();
    let mut synced = Synced::default();
    let gone: Vec<String> = kept
        .keys()
        .filter(|p| !wanted.contains_key(p.as_str()))
        .cloned()
        .collect();
    for path in gone {
        let _ = std::fs::remove_file(root.join(&path));
        kept.remove(&path);
        synced.removed += 1;
    }
    let stale: Vec<&File> = wanted
        .values()
        .filter(|f| kept.get(&f.path) != Some(f) || !root.join(&f.path).is_file())
        .copied()
        .collect();
    for batch in batches(&stale) {
        let read = call(
            url,
            BrainTexts {
                paths: batch.iter().map(|f| f.path.clone()).collect(),
            },
        )?;
        for text in read.texts {
            let Some(file) = wanted.get(text.path.as_str()) else {
                continue;
            };
            write(&root, file, &text.text)?;
            kept.insert(file.path.clone(), (*file).clone());
            synced.fetched += 1;
        }
        write_manifest(home, &kept)?;
    }
    if synced.removed > 0 {
        write_manifest(home, &kept)?;
    }
    Ok(synced)
}

/// The pages matching `query` on the node at `url`, best first, as its
/// Citadel ranks them.
pub fn search(url: &str, query: &str) -> Result<Vec<Hit>, String> {
    let answer = call(
        url,
        BrainSearch {
            query: query.to_owned(),
        },
    )?;
    Ok(answer
        .results
        .into_iter()
        .map(|hit| Hit {
            path: hit.path,
            title: hit.title,
            kind: hit.kind,
            summary: hit.summary,
            snippet: hit.snippet,
        })
        .collect())
}

/// The stale files in batches the node takes and answers within `ANSWER`.
fn batches<'a>(stale: &[&'a File]) -> Vec<Vec<&'a File>> {
    let mut out: Vec<Vec<&File>> = vec![];
    let mut bytes = 0;
    for f in stale {
        match out.last_mut() {
            Some(last) if last.len() < BATCH && bytes + f.size <= BATCH_BYTES => {
                last.push(f);
                bytes += f.size;
            }
            _ => {
                out.push(vec![f]);
                bytes = f.size;
            }
        }
    }
    out
}

/// A page path stays inside the copy: plain names, none hidden.
fn safe(path: &str) -> bool {
    !path.is_empty()
        && !path.contains('\\')
        && path.ends_with(".md")
        && Path::new(path).components().all(|c| match c {
            Component::Normal(part) => !part.to_string_lossy().starts_with('.'),
            _ => false,
        })
}

fn write(root: &Path, file: &File, text: &str) -> Result<(), String> {
    let to = root.join(&file.path);
    let fail = |e: std::io::Error| format!("{}: {e}", to.display());
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir).map_err(fail)?;
    }
    std::fs::write(&to, text).map_err(fail)?;
    let when = UNIX_EPOCH + Duration::from_nanos(file.modified);
    std::fs::File::options()
        .write(true)
        .open(&to)
        .and_then(|f| f.set_modified(when))
        .map_err(fail)
}

fn read_manifest(home: &Path) -> BTreeMap<String, File> {
    std::fs::read(home.join(MANIFEST))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// Writes beside and renames, so a crash never leaves half a file.
fn write_manifest(home: &Path, kept: &BTreeMap<String, File>) -> Result<(), String> {
    let tmp = home.join(format!("{MANIFEST}.{}", std::process::id()));
    let bytes = serde_json::to_vec(kept).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, bytes)
        .and_then(|_| std::fs::rename(&tmp, home.join(MANIFEST)))
        .map_err(|e| format!("{}: {e}", home.display()))
}

/// The node's answer as `T`, or its own words for a refusal.
fn parse<T: serde::de::DeserializeOwned>(raw: &[u8]) -> Result<T, String> {
    let (status, body) = citadel::split_answer(raw)?;
    if status == 200 {
        return serde_json::from_slice(body)
            .map_err(|_| format!("{WHO} answered in a way not understood"));
    }
    #[derive(Deserialize)]
    struct Refused {
        error: Said,
    }
    #[derive(Deserialize)]
    struct Said {
        message: String,
    }
    match serde_json::from_slice::<Refused>(body) {
        Ok(r) => Err(format!("The other machine: {}", r.error.message)),
        Err(_) if status == 404 => Err(
            "The other machine's node is older and cannot share its brain; update Ahjo there."
                .into(),
        ),
        Err(_) => Err(format!("{WHO} refused ({status})")),
    }
}

/// Call one typed command on the other node.
fn call<C: TypedCommand>(url: &str, input: C) -> Result<C::Output, String> {
    let body = serde_json::to_value(input).map_err(|error| error.to_string())?;
    let raw = citadel::ask(
        WHO,
        "POST",
        url,
        &format!("/api/call/{}", C::NAME),
        Some(&body),
        WAIT,
        ANSWER,
    )?;
    parse(&raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    /// A stand-in node serving `pages` (path → text, changed at `modified`)
    /// and counting the texts asked for.
    fn node(pages: Arc<Mutex<Vec<(String, String, u64)>>>, asked: Arc<Mutex<usize>>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let mut length = 0;
                loop {
                    let mut h = String::new();
                    reader.read_line(&mut h).unwrap();
                    if h == "\r\n" {
                        break;
                    }
                    if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = v.trim().parse().unwrap();
                    }
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                let pages = pages.lock().unwrap().clone();
                let answer = if line.starts_with("POST /api/call/brain_files") {
                    let files: Vec<_> = pages
                        .iter()
                        .map(|(p, t, m)| serde_json::json!({"path": p, "size": t.len(), "modified": m}))
                        .collect();
                    serde_json::json!({ "node":"n", "files": files })
                } else if line.starts_with("POST /api/call/brain_texts") {
                    let asked_for: serde_json::Value = serde_json::from_slice(&body).unwrap();
                    let paths = asked_for["paths"].as_array().unwrap();
                    *asked.lock().unwrap() += paths.len();
                    let texts: Vec<_> = pages
                        .iter()
                        .filter(|(p, _, _)| paths.iter().any(|a| a == p))
                        .map(|(p, t, _)| serde_json::json!({"path": p, "text": t}))
                        .collect();
                    serde_json::json!({ "node":"n", "texts": texts })
                } else {
                    assert!(line.starts_with("POST /api/call/brain_search "), "{line}");
                    let query: serde_json::Value = serde_json::from_slice(&body).unwrap();
                    assert_eq!(query["query"], "tide & wheel");
                    serde_json::json!({ "node":"n", "results": [{"path": "knowledge/a.md", "title": "A", "collection": "knowledge", "snippet": "»tide«"}] })
                };
                let body = answer.to_string();
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        url
    }

    #[test]
    fn a_real_node_shares_its_brain_through_typed_commands() {
        let root = ahjo_testkit::fresh_dir("brain-mirror-real-corpus");
        let cache = ahjo_testkit::fresh_dir("brain-mirror-real-cache");
        std::fs::create_dir_all(root.join("knowledge")).unwrap();
        let page = root.join("knowledge/a & b.md");
        std::fs::write(&page, "# First\n").unwrap();
        let (ready, address) = std::sync::mpsc::channel();
        let (stop, stopped) = std::sync::mpsc::channel();
        let corpus = root.clone();
        let server = std::thread::spawn(move || {
            tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(async move {
                    let config = format!("[brain]\ncorpus = {:?}\n", corpus.to_str().unwrap());
                    let node = ahjo_testkit::node::Node::start(&[ahjo_brain::LISTING], &config)
                        .await
                        .unwrap();
                    ready.send(node.url()).unwrap();
                    while stopped.try_recv().is_err() {
                        tokio::time::sleep(Duration::from_millis(20)).await;
                    }
                    node.stop().await;
                });
        });
        let url = address.recv().unwrap();
        assert_eq!(
            sync(&url, &cache).unwrap(),
            Synced {
                fetched: 1,
                removed: 0
            }
        );
        let local = pages(&cache).join("knowledge/a & b.md");
        assert_eq!(std::fs::read_to_string(&local).unwrap(), "# First\n");
        assert_eq!(sync(&url, &cache).unwrap(), Synced::default());
        std::fs::write(&page, "# Second, longer\n").unwrap();
        assert_eq!(
            sync(&url, &cache).unwrap(),
            Synced {
                fetched: 1,
                removed: 0
            }
        );
        assert_eq!(
            std::fs::read_to_string(&local).unwrap(),
            "# Second, longer\n"
        );
        std::fs::remove_file(page).unwrap();
        assert_eq!(
            sync(&url, &cache).unwrap(),
            Synced {
                fetched: 0,
                removed: 1
            }
        );
        assert!(!local.exists());
        stop.send(()).unwrap();
        server.join().unwrap();
        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(cache).unwrap();
    }

    #[test]
    fn a_copy_takes_only_what_changed_and_drops_what_went() {
        let home = std::env::temp_dir().join(format!("ahjo-brain-mirror-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let pages = Arc::new(Mutex::new(vec![
            (
                "knowledge/a.md".to_string(),
                "# A\n".to_string(),
                1_700_000_000_000_000_000,
            ),
            (
                "knowledge/b & c.md".to_string(),
                "# B\n".to_string(),
                1_700_000_000_000_000_000,
            ),
            ("../outside.md".to_string(), "# No\n".to_string(), 1),
        ]));
        let asked = Arc::new(Mutex::new(0));
        let url = node(pages.clone(), asked.clone());

        let first = sync(&url, &home).unwrap();
        assert_eq!(
            first,
            Synced {
                fetched: 2,
                removed: 0
            }
        );
        let a = pages_file(&home, "knowledge/a.md");
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "# A\n");
        let when = std::fs::metadata(&a).unwrap().modified().unwrap();
        assert_eq!(
            when,
            UNIX_EPOCH + Duration::from_secs(1_700_000_000),
            "it keeps the other machine's time"
        );
        assert!(
            !home.join("outside.md").exists(),
            "a path out of the copy is never written"
        );

        assert_eq!(
            sync(&url, &home).unwrap(),
            Synced::default(),
            "nothing changed, nothing asked"
        );
        assert_eq!(*asked.lock().unwrap(), 2);

        {
            let mut p = pages.lock().unwrap();
            p[0] = (
                "knowledge/a.md".into(),
                "# A again\n".into(),
                1_700_000_100_000_000_000,
            );
            p.remove(1);
        }
        assert_eq!(
            sync(&url, &home).unwrap(),
            Synced {
                fetched: 1,
                removed: 1
            }
        );
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "# A again\n");
        assert!(!pages_file(&home, "knowledge/b & c.md").exists());

        let hits = search(&url, "tide & wheel").unwrap();
        assert_eq!(
            (hits[0].path.as_str(), hits[0].snippet.as_str()),
            ("knowledge/a.md", "»tide«")
        );
        std::fs::remove_dir_all(&home).unwrap();
    }

    fn pages_file(home: &Path, path: &str) -> PathBuf {
        pages(home).join(path)
    }

    #[test]
    fn batches_stay_within_their_count_and_size() {
        let f = |size| File {
            path: String::new(),
            size,
            modified: 0,
        };
        let files = [f(3 << 20), f(2 << 20), f(1), f(5 << 20)];
        let refs: Vec<&File> = files.iter().collect();
        let sizes: Vec<usize> = batches(&refs).iter().map(Vec::len).collect();
        assert_eq!(sizes, [1, 2, 1]);
    }

    #[test]
    fn a_node_url_names_its_copy() {
        let home = home(Path::new("/c"), "http://100.64.0.9:7300");
        assert_eq!(home, Path::new("/c/from/100.64.0.9_7300"));
    }
}
