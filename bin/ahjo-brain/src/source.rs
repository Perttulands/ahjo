//! Where the app finds the brain and its service, and what it keeps between
//! launches.
//!
//! - The brain's files: `--brain <dir>` or `BRAIN_ROOT`; otherwise
//!   `/srv/data/context` on Linux, and on Windows the first WSL distribution
//!   that has it, through `\\wsl.localhost\<distro>\srv\data\context`.
//! - Another machine's brain: `--from <node url>` (`BRAIN_FROM`, `from` in
//!   the settings, which the Windows installer writes), or one of Ahjo
//!   Swarm's places chosen in the app, read through its node into a copy
//!   kept in the cache (`mirror`), read only.
//! - Context Citadel, for ranked search: `--citadel <url>` or
//!   `BRAIN_CITADEL`; otherwise `http://127.0.0.1:3200`, which on Windows
//!   reaches a WSL service bound to loopback through WSL's localhost
//!   forwarding.
//! - The cache: what the last scan read, and where the sky placed its
//!   systems, in `%LOCALAPPDATA%\ahjo-brain` on Windows and
//!   `$XDG_CACHE_HOME/ahjo-brain` (or `~/.cache/ahjo-brain`) elsewhere;
//!   `AHJO_BRAIN_CACHE` moves it. It holds the brain's titles and summaries,
//!   so it stays on this machine.
//! - The app's own settings (its theme): `brain.json` beside Swarm's
//!   `app.json` and Desk's `desk.json`, in `%APPDATA%\Ahjo` on Windows and
//!   `$XDG_CONFIG_HOME/ahjo` (or `~/.config/ahjo`) elsewhere;
//!   `AHJO_BRAIN_CONFIG` names the file. The first start after the move
//!   carries over the old `ahjo-brain/settings.json`. Until a theme is
//!   chosen here, the one chosen in Ahjo Swarm is drawn.

use crate::corpus::Entry;
use crate::mirror::{self, Remote};
use ahjo_app_kit::Error;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const DEFAULT_CITADEL: &str = "http://127.0.0.1:3200";
const LINUX_ROOT: &str = "/srv/data/context";
const WSL_ROOT: &str = r"srv\data\context";

#[derive(Clone)]
pub struct Where {
    pub brain: Option<PathBuf>,
    pub citadel: String,
    pub cache: Option<PathBuf>,
    /// The other machine the brain is read from; none is this one's.
    pub from: Option<Remote>,
}

impl Where {
    /// Where `remote`'s brain is read, from this launch's cache: its copy,
    /// which keeps its own scan beside it.
    pub fn reading(&self, remote: Remote) -> Where {
        let home = self.cache.as_ref().map(|c| mirror::home(c, &remote.url));
        Where {
            brain: home.as_deref().map(mirror::pages),
            citadel: self.citadel.clone(),
            cache: home,
            from: Some(remote),
        }
    }
}

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

pub fn find(args: &[String]) -> Where {
    let brain = arg(args, "--brain")
        .or_else(|| std::env::var("BRAIN_ROOT").ok())
        .map(PathBuf::from)
        .or_else(default_root);
    let citadel = arg(args, "--citadel")
        .or_else(|| std::env::var("BRAIN_CITADEL").ok())
        .unwrap_or_else(|| DEFAULT_CITADEL.to_string());
    let cache = std::env::var_os("AHJO_BRAIN_CACHE")
        .map(PathBuf::from)
        .or_else(default_cache);
    Where {
        brain,
        citadel,
        cache,
        from: None,
    }
}

/// The other machine asked for: `--from <node url>`, else `BRAIN_FROM`,
/// else the settings' `from`, named by its host: `rig` for
/// `http://rig.tail1.ts.net:7300`. Kept in the settings, it holds however
/// the app is started, from a shortcut or a taskbar pin of the bare exe.
pub fn asked_from(args: &[String], settings: &Settings) -> Option<Remote> {
    let url = arg(args, "--from")
        .or_else(|| std::env::var("BRAIN_FROM").ok())
        .or_else(|| settings.from.clone())?;
    Some(Remote {
        name: host_name(&url),
        url,
    })
}

fn host_name(url: &str) -> String {
    let host = url.trim_start_matches("http://");
    let host = host.split([':', '/']).next().unwrap_or(host);
    if host.parse::<std::net::IpAddr>().is_ok() {
        return host.to_string();
    }
    host.split('.').next().unwrap_or(host).to_string()
}

fn default_root() -> Option<PathBuf> {
    if cfg!(windows) {
        // `\\wsl.localhost` itself cannot be listed, only a distribution's
        // share under it, so ask WSL for their names, the default first.
        let distros = wsl_distros();
        for share in [r"\\wsl.localhost", r"\\wsl$"] {
            for d in &distros {
                let root = Path::new(share).join(d).join(WSL_ROOT);
                if root.join("knowledge").is_dir() {
                    return Some(root);
                }
            }
        }
        None
    } else {
        let root = PathBuf::from(LINUX_ROOT);
        root.is_dir().then_some(root)
    }
}

/// The WSL distributions installed, from `wsl.exe --list --quiet`.
#[cfg(windows)]
fn wsl_distros() -> Vec<String> {
    use std::os::windows::process::CommandExt;
    // No console window flashes up for it.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let Ok(out) = std::process::Command::new("wsl.exe")
        .args(["--list", "--quiet"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    else {
        return vec![];
    };
    distro_names(&out.stdout)
}

#[cfg(not(windows))]
fn wsl_distros() -> Vec<String> {
    vec![]
}

/// `wsl.exe` answers in UTF-16, one name a line.
#[cfg(any(windows, test))]
fn distro_names(raw: &[u8]) -> Vec<String> {
    let wide: Vec<u16> = raw
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .collect();
    String::from_utf16_lossy(&wide)
        .lines()
        .map(|l| {
            l.trim_matches(|c: char| c.is_whitespace() || c == '\0')
                .to_string()
        })
        .filter(|l| !l.is_empty())
        .collect()
}

fn default_cache() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
    };
    base.map(|b| b.join("ahjo-brain"))
}

/// What one launch leaves the next: the scan, and the systems' places.
#[derive(Default, Serialize, Deserialize)]
pub struct Kept {
    pub root: String,
    pub entries: Vec<Entry>,
    pub placement: HashMap<String, (f32, f32)>,
}

const KEPT: &str = "brain.json";
/// Bumped when `Entry` changes shape, so an old file is read as none.
const VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct File {
    version: u32,
    #[serde(flatten)]
    kept: Kept,
}

pub fn load(cache: &Path, root: &Path) -> Kept {
    let read = std::fs::read(cache.join(KEPT))
        .ok()
        .and_then(|b| serde_json::from_slice::<File>(&b).ok());
    match read {
        Some(f) if f.version == VERSION && f.kept.root == root.display().to_string() => f.kept,
        _ => Kept {
            root: root.display().to_string(),
            ..Kept::default()
        },
    }
}

/// Writes beside and renames, so a crash never leaves half a file.
pub fn save(cache: &Path, kept: Kept) -> std::io::Result<()> {
    std::fs::create_dir_all(cache)?;
    let tmp = cache.join(format!("{KEPT}.{}", std::process::id()));
    let bytes = serde_json::to_vec(&File {
        version: VERSION,
        kept,
    })
    .map_err(std::io::Error::other)?;
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, cache.join(KEPT))
}

/// What the app remembers of its own choices, kept in their file as the
/// kit keeps every app's (`settings_file`); what it holds that this app
/// does not know is kept.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The theme's direction, by its key: `night`, `saga` or `signal`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub theme: String,
    /// The reader's body size as zoomed; none is its own size.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reader_text: Option<f32>,
    /// The whole app's size in percent (`zoom::app`), once zoomed here;
    /// none opens at Ahjo Swarm's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_size: Option<u16>,
    /// The other machine's node the brain is read from on every start;
    /// none is this one's brain.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

/// `AHJO_BRAIN_CONFIG`, else `brain.json` in the folder of Ahjo Swarm's
/// `app.json` and Desk's `desk.json` (`%APPDATA%\Ahjo`, or
/// `$XDG_CONFIG_HOME/ahjo`).
pub fn settings_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("AHJO_BRAIN_CONFIG") {
        return Some(p.into());
    }
    ahjo_app_kit::settings::path().map(|p| p.with_file_name("brain.json"))
}

/// Where Brain kept its settings before they moved beside Swarm's and
/// Desk's: `settings.json` in its own `ahjo-brain` folder.
fn former_settings_path() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    };
    base.map(|b| b.join("ahjo-brain").join("settings.json"))
}

/// The settings file to use now. A file at the former place moves to the
/// new one once: read, written whole, and only then removed. Nothing is
/// moved over a file already at the new place, or when `AHJO_BRAIN_CONFIG`
/// names the file. A move that fails leaves the old file in use for this
/// launch, and the error says why; the next launch tries again.
pub fn settings_path_moved() -> (Option<PathBuf>, Option<Error>) {
    let path = settings_path();
    if std::env::var_os("AHJO_BRAIN_CONFIG").is_some() {
        return (path, None);
    }
    let (Some(new), Some(old)) = (path.clone(), former_settings_path()) else {
        return (path, None);
    };
    match move_settings(&old, &new) {
        Ok(_) => (path, None),
        Err(e) => (Some(old), Some(e)),
    }
}

/// Moves the settings at `old` to `new` unless `new` exists or `old` does
/// not; whether it did.
fn move_settings(old: &Path, new: &Path) -> Result<bool, Error> {
    let fail = |what: &str, at: &Path, e: std::io::Error| {
        Error::new(
            "settings_not_moved",
            format!(
                "Brain's settings could not be moved to {}: {what} {}: {e}",
                new.display(),
                at.display()
            ),
        )
    };
    if new.exists() || !old.exists() {
        return Ok(false);
    }
    let text = std::fs::read(old).map_err(|e| fail("reading", old, e))?;
    if let Some(dir) = new.parent() {
        std::fs::create_dir_all(dir).map_err(|e| fail("making", dir, e))?;
    }
    let part = new.with_extension("json.part");
    std::fs::write(&part, &text).map_err(|e| fail("writing", &part, e))?;
    std::fs::rename(&part, new).map_err(|e| fail("writing", new, e))?;
    std::fs::remove_file(old).map_err(|e| fail("removing", old, e))?;
    // The old folder goes with it when nothing else is in it.
    if let Some(dir) = old.parent() {
        let _ = std::fs::remove_dir(dir);
    }
    Ok(true)
}

/// A cheap sign that the brain changed: its Git index moves with every
/// commit. Read when the window comes forward, never on a timer.
pub fn marker(root: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(root.join(".git").join("index"))
        .and_then(|m| m.modified())
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ahjo-brain-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn settings_move_beside_swarms_once_and_the_old_file_goes_after_the_write() {
        let home = scratch("moves");
        let (old, new) = (
            home.join("ahjo-brain").join("settings.json"),
            home.join("Ahjo").join("brain.json"),
        );
        std::fs::create_dir_all(old.parent().unwrap()).unwrap();
        std::fs::write(&old, r#"{"from":"http://rig:7300","theme":"night"}"#).unwrap();
        assert!(move_settings(&old, &new).unwrap());
        assert_eq!(
            std::fs::read_to_string(&new).unwrap(),
            r#"{"from":"http://rig:7300","theme":"night"}"#
        );
        assert!(!old.exists(), "the old file is gone");
        assert!(!old.parent().unwrap().exists(), "so is its empty folder");
        // A second start finds nothing to move, and moves nothing over a file.
        assert!(!move_settings(&old, &new).unwrap());
        std::fs::create_dir_all(old.parent().unwrap()).unwrap();
        std::fs::write(&old, "{}").unwrap();
        assert!(!move_settings(&old, &new).unwrap());
        assert!(old.exists(), "an old file beside a new one stays untouched");
        assert!(std::fs::read_to_string(&new).unwrap().contains("rig"));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_move_that_cannot_write_keeps_the_old_file_and_says_so() {
        let home = scratch("keeps");
        let old = home.join("ahjo-brain").join("settings.json");
        std::fs::create_dir_all(old.parent().unwrap()).unwrap();
        std::fs::write(&old, "{}").unwrap();
        // The new place's folder is a file: nothing can be written there.
        let blocked = home.join("Ahjo");
        std::fs::write(&blocked, "").unwrap();
        let err = move_settings(&old, &blocked.join("brain.json")).unwrap_err();
        assert_eq!(err.code, "settings_not_moved");
        assert!(old.exists(), "the old file is removed only after the write");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn another_machine_is_named_by_its_host() {
        assert_eq!(host_name("http://rig.tail1.ts.net:7300"), "rig");
        assert_eq!(host_name("http://100.64.0.2:7300"), "100.64.0.2");
        assert_eq!(host_name("rig:7300"), "rig");
    }

    #[test]
    fn the_settings_name_the_other_machine_when_the_launch_does_not() {
        let kept = Settings {
            from: Some("http://rig.tail1.ts.net:7300".into()),
            ..Settings::default()
        };
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        if std::env::var_os("BRAIN_FROM").is_none() {
            let bare = asked_from(&args(&["ahjo-brain"]), &kept).unwrap();
            assert_eq!(
                (bare.name.as_str(), bare.url.as_str()),
                ("rig", "http://rig.tail1.ts.net:7300")
            );
            assert!(asked_from(&args(&["ahjo-brain"]), &Settings::default()).is_none());
        }
        let asked = asked_from(&args(&["ahjo-brain", "--from", "http://box:7300"]), &kept).unwrap();
        assert_eq!(asked.url, "http://box:7300");
    }

    #[test]
    fn wsl_names_its_distributions_in_utf16() {
        let raw: Vec<u8> = "Ubuntu\r\nDebian\r\n\r\n"
            .encode_utf16()
            .flat_map(|u| u.to_le_bytes())
            .collect();
        assert_eq!(distro_names(&raw), ["Ubuntu", "Debian"]);
    }

    #[test]
    fn kept_state_returns_for_the_same_brain_only() {
        let dir = std::env::temp_dir().join(format!("ahjo-brain-kept-{}", std::process::id()));
        let root = Path::new("/somewhere/brain");
        let mut kept = load(&dir, root);
        assert!(kept.entries.is_empty());
        kept.placement.insert("knowledge/sky".into(), (1., 2.));
        save(&dir, kept).unwrap();
        assert_eq!(load(&dir, root).placement["knowledge/sky"], (1., 2.));
        assert!(load(&dir, Path::new("/elsewhere")).placement.is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn settings_keep_what_they_do_not_know() {
        use ahjo_app_kit::settings_file::{read, text, write};
        let dir = std::env::temp_dir().join(format!("ahjo-brain-settings-{}", std::process::id()));
        let path = dir.join("settings.json");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, r#"{"theme":"saga","later":1}"#).unwrap();
        let (mut s, _) = read::<Settings>(&path).unwrap().unwrap();
        assert_eq!(s.theme, "saga");
        assert_eq!(s.reader_text, None);
        s.theme = "signal".into();
        s.reader_text = Some(18.5);
        s.app_size = Some(115);
        write(&path, &text(&s)).unwrap();
        let (back, _) = read::<Settings>(&path).unwrap().unwrap();
        assert_eq!(back.theme, "signal");
        assert_eq!(back.reader_text, Some(18.5));
        assert_eq!(back.app_size, Some(115));
        assert_eq!(back.rest["later"], 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn arguments_come_before_the_environment() {
        let w = find(&["x".into(), "--citadel".into(), "http://h:1".into()]);
        assert_eq!(w.citadel, "http://h:1");
    }
}
