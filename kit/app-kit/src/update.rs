//! An installed app updating itself from the node it was installed from
//! (ARCHITECTURE.md, Install and upgrade). The install script leaves
//! `install.json` beside the apps, naming that node; an app with none
//! beside it (the operator's own, a build run where cargo put it) never
//! offers anything.
//!
//! The node hands out the builds of its own commit (`/apps/<platform>`).
//! When their version is not this app's, [`Updates`] offers them: every
//! file is fetched and checked against its SHA-256 before any is put in
//! place, each is then swapped in by renaming (a running app keeps running
//! from its old file, so Brain and Desk need not close for Swarm to
//! update), and the app starts its new build with the arguments it was
//! given and quits. The old files are removed the next time an app starts.

use crate::link::http::{error_text, parse};
use gpui::{App, Context, Window};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

/// What the install script writes beside the apps.
pub const INSTALL: &str = "install.json";

/// This platform's name in the node's `/apps/<platform>`.
pub const PLATFORM: &str = if cfg!(windows) { "windows" } else { "linux" };

/// An install this app runs from.
#[derive(Clone, Debug, PartialEq)]
pub struct Install {
    /// The folder holding the apps and `install.json`.
    pub dir: PathBuf,
    /// The node the apps came from: `http://rig.tail1.ts.net:7300`.
    pub from: String,
    /// This app's file and the arguments it was started with, as at start:
    /// once swapped, the running file has another name.
    exe: PathBuf,
    args: Vec<String>,
}

#[derive(Deserialize)]
struct Written {
    from: String,
}

impl Install {
    /// The install beside this app's file, if there is one.
    pub fn here() -> Option<Install> {
        let exe = std::env::current_exe().ok()?;
        let install = Install::at(exe.parent()?)?;
        Some(Install {
            exe,
            args: std::env::args().skip(1).collect(),
            ..install
        })
    }

    /// The install in `dir`, if `install.json` there reads.
    pub fn at(dir: &Path) -> Option<Install> {
        let text = std::fs::read(dir.join(INSTALL)).ok()?;
        let written: Written = serde_json::from_slice(&text).ok()?;
        Some(Install {
            dir: dir.to_owned(),
            from: written.from.trim_end_matches('/').to_owned(),
            exe: PathBuf::new(),
            args: vec![],
        })
    }

    /// The node's name, as the tooltip says it: `rig` for
    /// `http://rig.tail1.ts.net:7300`, an address whole.
    pub fn node_name(&self) -> &str {
        let host = self.from.trim_start_matches("http://");
        let host = host.split([':', '/']).next().unwrap_or(host);
        if host.parse::<std::net::IpAddr>().is_ok() {
            return host;
        }
        host.split('.').next().unwrap_or(host)
    }
}

/// What the node hands out for this platform.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Manifest {
    pub version: String,
    pub files: Vec<File>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct File {
    pub name: String,
    pub size: u64,
    pub sha256: String,
}

impl Manifest {
    pub fn bytes(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }
}

/// How long a stalled read waits; a download as a whole may take as long
/// as it keeps moving.
const STALL: Duration = Duration::from_secs(30);

/// One GET of `path` from `base` (`http://host:port`), its whole answer.
fn get(base: &str, path: &str) -> Result<Vec<u8>, String> {
    let host = base.trim_start_matches("http://").trim_end_matches('/');
    let addr = if host.contains(':') {
        host.to_owned()
    } else {
        format!("{host}:80")
    };
    let to = addr
        .to_socket_addrs()
        .map_err(|e| format!("{host}: {e}"))?
        .next()
        .ok_or_else(|| format!("{host} has no address"))?;
    let mut s = TcpStream::connect_timeout(&to, Duration::from_secs(10))
        .map_err(|e| format!("{host}: {e}"))?;
    s.set_read_timeout(Some(STALL)).map_err(|e| e.to_string())?;
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nAccept-Encoding: identity\r\n\r\n"
    )
    .map_err(|e| format!("{host}: {e}"))?;
    let mut all = Vec::new();
    s.read_to_end(&mut all)
        .map_err(|e| format!("{host}: {e}"))?;
    let (status, body) = parse(&all)?;
    if status == 200 {
        Ok(body)
    } else {
        Err(error_text(status, &body))
    }
}

/// What the install's node hands out now.
pub fn offered(install: &Install) -> Result<Manifest, String> {
    let body = get(&install.from, &format!("/apps/{PLATFORM}"))?;
    serde_json::from_slice(&body).map_err(|e| format!("the node's list of apps does not read: {e}"))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Fetches every file `manifest` names and puts them in place in
/// `install.dir`. Nothing in place changes unless every file arrived whole;
/// a swap that fails half way puts back what it moved.
pub fn fetch(install: &Install, manifest: &Manifest) -> Result<(), String> {
    let staging = install.dir.join(format!(".update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| format!("{}: {e}", staging.display()))?;
    let done = (|| {
        for f in &manifest.files {
            if f.name.contains(['/', '\\']) || f.name.starts_with('.') {
                return Err(format!("the node names an odd file, {}", f.name));
            }
            let bytes = get(&install.from, &format!("/apps/{PLATFORM}/{}", f.name))?;
            if bytes.len() as u64 != f.size || hex(&Sha256::digest(&bytes)) != f.sha256 {
                return Err(format!("{} arrived damaged; nothing was changed", f.name));
            }
            let at = staging.join(&f.name);
            std::fs::write(&at, &bytes).map_err(|e| format!("{}: {e}", at.display()))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&at, std::fs::Permissions::from_mode(0o755))
                    .map_err(|e| format!("{}: {e}", at.display()))?;
            }
        }
        swap(&install.dir, &staging, manifest)
    })();
    let _ = std::fs::remove_dir_all(&staging);
    done
}

/// Moves each file from `staging` into `dir`, the one there renamed aside
/// first (Windows lets a running program's file be renamed, not replaced).
fn swap(dir: &Path, staging: &Path, manifest: &Manifest) -> Result<(), String> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    let mut moved: Vec<(PathBuf, Option<PathBuf>)> = vec![];
    for f in &manifest.files {
        let at = dir.join(&f.name);
        let aside = at
            .exists()
            .then(|| dir.join(format!("{}.{stamp}.old", f.name)));
        let step = aside
            .as_ref()
            .map_or(Ok(()), |aside| std::fs::rename(&at, aside))
            .and_then(|()| std::fs::rename(staging.join(&f.name), &at));
        if let Err(e) = step {
            // Back as it was: this file's old one back, and for those
            // already moved, the new ones out and the old ones in.
            if let Some(aside) = &aside
                && !at.exists()
            {
                let _ = std::fs::rename(aside, &at);
            }
            for (at, aside) in moved.iter().rev() {
                let _ = std::fs::remove_file(at);
                if let Some(aside) = aside {
                    let _ = std::fs::rename(aside, at);
                }
            }
            return Err(format!("{}: {e}; the apps are as they were", at.display()));
        }
        moved.push((at, aside));
    }
    Ok(())
}

/// Removes the files earlier updates set aside; one still running stays.
pub fn tidy(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().ends_with(".old") {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Starts the app's file as it was started.
pub fn relaunch(install: &Install) -> Result<(), String> {
    std::process::Command::new(&install.exe)
        .args(&install.args)
        .current_dir(&install.dir)
        .spawn()
        .map(drop)
        .map_err(|e| format!("starting {}: {e}", install.exe.display()))
}

/// Where an update stands.
#[derive(Clone, Debug, PartialEq)]
pub enum State {
    /// Nothing to offer: this app is the node's build, or none is known.
    Current,
    /// The node hands out another build.
    Offered(Manifest),
    Fetching(Manifest),
    /// In place; the app restarts when nothing holds it.
    Ready(Manifest),
    Failed(Manifest, String),
}

/// Asked before restarting: whether the window may close now. A page
/// holding unsaved work says no, and asks its own way.
pub type Guard = Rc<dyn Fn(&mut Window, &mut App) -> bool>;

/// An app's updates: what the node offers and the one action on it.
pub struct Updates {
    install: Option<Install>,
    own: &'static str,
    pub state: State,
    checking: bool,
    guard: Option<Guard>,
}

/// What the app's chrome shows of an update.
pub struct Badge {
    pub words: &'static str,
    pub tip: String,
    pub tone: Tone,
    /// A click acts: update, restart or try again.
    pub acts: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Action,
    Busy,
    Failed,
}

impl Updates {
    /// This app's updates: `own` is its build (`AHJO_VERSION`).
    pub fn new(own: &'static str) -> Self {
        let install = Install::here();
        if let Some(install) = &install {
            tidy(&install.dir);
        }
        Self::with(install, own)
    }

    pub fn with(install: Option<Install>, own: &'static str) -> Self {
        Self {
            install,
            own,
            state: State::Current,
            checking: false,
            guard: None,
        }
    }

    pub fn installed(&self) -> bool {
        self.install.is_some()
    }

    pub fn set_guard(&mut self, guard: Guard) {
        self.guard = Some(guard);
    }

    /// Asks the node what it hands out, off the UI thread. Cheap: the app
    /// asks when it starts, when its window comes forward and when its
    /// node comes back.
    pub fn check(&mut self, cx: &mut Context<Self>) {
        let Some(install) = self.install.clone() else {
            return;
        };
        if self.checking || matches!(self.state, State::Fetching(_) | State::Ready(_)) {
            return;
        }
        self.checking = true;
        let asked = cx
            .background_executor()
            .spawn(async move { offered(&install) });
        cx.spawn(async move |this, cx| {
            let answer = asked.await;
            let _ = this.update(cx, |me, cx| {
                me.checking = false;
                if matches!(me.state, State::Fetching(_) | State::Ready(_)) {
                    return;
                }
                me.state = match answer {
                    Ok(m) if m.version != me.own => State::Offered(m),
                    // Unreachable or nothing kept: nothing to offer, and
                    // the app's own link says when the node is away.
                    _ => State::Current,
                };
                cx.notify();
            });
        })
        .detach();
    }

    /// The one action: fetch and swap in what is offered, then restart;
    /// once in place, restart; after a failure, try again.
    pub fn act(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(install) = self.install.clone() else {
            return;
        };
        let manifest = match &self.state {
            State::Offered(m) | State::Failed(m, _) => m.clone(),
            State::Ready(_) => {
                self.restart(window, cx);
                return;
            }
            State::Current | State::Fetching(_) => return,
        };
        self.state = State::Fetching(manifest.clone());
        cx.notify();
        let fetched = {
            let manifest = manifest.clone();
            cx.background_executor()
                .spawn(async move { fetch(&install, &manifest) })
        };
        cx.spawn_in(window, async move |this, cx| {
            let done = fetched.await;
            let _ = this.update_in(cx, |me, window, cx| {
                match done {
                    Ok(()) => {
                        me.state = State::Ready(manifest);
                        me.restart(window, cx);
                    }
                    Err(why) => me.state = State::Failed(manifest, why),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn restart(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(install) = &self.install else {
            return;
        };
        if let Some(guard) = self.guard.clone()
            && !guard(window, cx)
        {
            return;
        }
        match relaunch(install) {
            Ok(()) => cx.quit(),
            Err(why) => {
                if let State::Ready(m) = &self.state {
                    self.state = State::Failed(m.clone(), why);
                }
                cx.notify();
            }
        }
    }

    /// What the chrome shows, if anything.
    pub fn badge(&self) -> Option<Badge> {
        let install = self.install.as_ref()?;
        let node = install.node_name();
        let mb = |m: &Manifest| (m.bytes() as f64 / 1_048_576.).round();
        Some(match &self.state {
            State::Current => return None,
            State::Offered(m) => Badge {
                words: "Update",
                tip: format!(
                    "{node} runs Ahjo {}; this app is {}. Update fetches it ({} MB) and restarts.",
                    m.version,
                    self.own,
                    mb(m)
                ),
                tone: Tone::Action,
                acts: true,
            },
            State::Fetching(m) => Badge {
                words: "Updating…",
                tip: format!("Fetching Ahjo {} from {node} ({} MB)", m.version, mb(m)),
                tone: Tone::Busy,
                acts: false,
            },
            State::Ready(m) => Badge {
                words: "Restart",
                tip: format!("Ahjo {} is in place; restart to use it.", m.version),
                tone: Tone::Action,
                acts: true,
            },
            State::Failed(m, why) => Badge {
                words: "Update failed",
                tip: format!("Ahjo {} from {node}: {why}. Click to try again.", m.version),
                tone: Tone::Failed,
                acts: true,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch folder, removed when the test ends.
    fn dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn manifest(names: &[&str]) -> Manifest {
        Manifest {
            version: "next".into(),
            files: names
                .iter()
                .map(|n| File {
                    name: (*n).into(),
                    size: 0,
                    sha256: String::new(),
                })
                .collect(),
        }
    }

    #[test]
    fn an_install_names_its_node() {
        let scratch = dir();
        let d = scratch.path();
        assert_eq!(Install::at(d), None);
        std::fs::write(d.join(INSTALL), "\u{feff}{}").unwrap();
        assert_eq!(Install::at(d), None, "a byte-order mark does not read");
        std::fs::write(
            d.join(INSTALL),
            r#"{ "from": "http://rig.tail1.ts.net:7300/" }"#,
        )
        .unwrap();
        let install = Install::at(d).unwrap();
        assert_eq!(install.from, "http://rig.tail1.ts.net:7300");
        assert_eq!(install.node_name(), "rig");
        let local = Install {
            from: "http://127.0.0.1:7310".into(),
            ..install
        };
        assert_eq!(local.node_name(), "127.0.0.1");
    }

    #[test]
    fn a_swap_puts_every_file_in_place_and_the_old_aside() {
        let scratch = dir();
        let d = scratch.path();
        let staging = d.join(".update-1");
        std::fs::create_dir_all(&staging).unwrap();
        for (name, old, new) in [("a.exe", "old a", "new a"), ("b.exe", "old b", "new b")] {
            std::fs::write(d.join(name), old).unwrap();
            std::fs::write(staging.join(name), new).unwrap();
        }
        std::fs::write(staging.join("c.exe"), "new c").unwrap();
        swap(d, &staging, &manifest(&["a.exe", "b.exe", "c.exe"])).unwrap();
        for name in ["a", "b", "c"] {
            assert_eq!(
                std::fs::read_to_string(d.join(format!("{name}.exe"))).unwrap(),
                format!("new {name}")
            );
        }
        let aside = |d: &Path| {
            std::fs::read_dir(d)
                .unwrap()
                .filter(|e| {
                    e.as_ref()
                        .unwrap()
                        .file_name()
                        .to_string_lossy()
                        .ends_with(".old")
                })
                .count()
        };
        assert_eq!(aside(d), 2);
        tidy(d);
        assert_eq!(aside(d), 0);
    }

    #[test]
    fn a_swap_that_fails_half_way_leaves_the_apps_as_they_were() {
        let scratch = dir();
        let d = scratch.path();
        let staging = d.join(".update-1");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(d.join("a.exe"), "old a").unwrap();
        std::fs::write(staging.join("a.exe"), "new a").unwrap();
        std::fs::write(d.join("b.exe"), "old b").unwrap();
        // b.exe never arrived in staging.
        let failed = swap(d, &staging, &manifest(&["a.exe", "b.exe"]));
        assert!(failed.is_err());
        assert_eq!(std::fs::read_to_string(d.join("a.exe")).unwrap(), "old a");
        assert_eq!(std::fs::read_to_string(d.join("b.exe")).unwrap(), "old b");
    }

    #[test]
    fn nothing_is_offered_outside_an_install() {
        let u = Updates::with(None, "1");
        assert!(u.badge().is_none());
        let mut u = Updates::with(Install::at(dir().path()), "1");
        assert!(!u.installed());
        u.state = State::Offered(manifest(&[]));
        assert!(u.badge().is_none());
    }
}
