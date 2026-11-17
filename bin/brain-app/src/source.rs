//! Where the app finds the brain and its service, and what it keeps between
//! launches.
//!
//! - The brain's files: `--brain <dir>` or `BRAIN_ROOT`; otherwise
//!   `/srv/data/context` on Linux, and on Windows the first WSL distribution
//!   that has it, through `\\wsl.localhost\<distro>\srv\data\context`.
//! - Context Citadel, for ranked search: `--citadel <url>` or
//!   `BRAIN_CITADEL`; otherwise `http://127.0.0.1:3200`, which on Windows
//!   reaches a WSL service bound to loopback through WSL's localhost
//!   forwarding.
//! - The cache: what the last scan read, and where the sky placed its
//!   systems, in `%LOCALAPPDATA%\brain-app` on Windows and
//!   `$XDG_CACHE_HOME/brain-app` (or `~/.cache/brain-app`) elsewhere;
//!   `BRAIN_APP_CACHE` moves it. It holds the brain's titles and summaries,
//!   so it stays on this machine.
//! - The app's own settings (its theme): `settings.json` in
//!   `%APPDATA%\brain-app` on Windows and `$XDG_CONFIG_HOME/brain-app` (or
//!   `~/.config/brain-app`) elsewhere; `BRAIN_APP_CONFIG` names the file.
//!   Until a theme is chosen here, the one chosen in Parvi is drawn.

use crate::corpus::Entry;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const DEFAULT_CITADEL: &str = "http://127.0.0.1:3200";
const LINUX_ROOT: &str = "/srv/data/context";
const WSL_ROOT: &str = r"srv\data\context";

pub struct Where {
    pub brain: Option<PathBuf>,
    pub citadel: String,
    pub cache: Option<PathBuf>,
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
    let cache = std::env::var_os("BRAIN_APP_CACHE")
        .map(PathBuf::from)
        .or_else(default_cache);
    Where {
        brain,
        citadel,
        cache,
    }
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
    base.map(|b| b.join("brain-app"))
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

/// What the app remembers of its own choices. A file that does not read is
/// reported and left as it is, never replaced; what it holds that this app
/// does not know is kept.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The theme's direction, by its key: `night`, `saga` or `signal`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub theme: String,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

pub fn settings_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("BRAIN_APP_CONFIG") {
        return Some(p.into());
    }
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    };
    base.map(|b| b.join("brain-app").join("settings.json"))
}

/// The settings in `path`; none written yet reads as the defaults.
pub fn read_settings(path: &Path) -> Result<Settings, String> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|e| format!("{} does not read: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Writes beside and renames, so a crash never leaves half a file.
pub fn write_settings(path: &Path, settings: &Settings) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!("json.{}", std::process::id()));
    let bytes = serde_json::to_vec_pretty(settings).map_err(std::io::Error::other)?;
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

/// When a file last changed, as a cheap sign to read it again.
pub fn changed(path: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
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
        let dir = std::env::temp_dir().join(format!("brain-app-kept-{}", std::process::id()));
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
    fn settings_keep_what_they_do_not_know_and_refuse_a_broken_file() {
        let dir = std::env::temp_dir().join(format!("brain-app-settings-{}", std::process::id()));
        let path = dir.join("settings.json");
        assert_eq!(read_settings(&path).unwrap(), Settings::default());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, r#"{"theme":"saga","later":1}"#).unwrap();
        let mut s = read_settings(&path).unwrap();
        assert_eq!(s.theme, "saga");
        s.theme = "signal".into();
        write_settings(&path, &s).unwrap();
        let back = read_settings(&path).unwrap();
        assert_eq!(back.theme, "signal");
        assert_eq!(back.rest["later"], 1);
        std::fs::write(&path, "{not json").unwrap();
        assert!(read_settings(&path).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn arguments_come_before_the_environment() {
        let w = find(&["x".into(), "--citadel".into(), "http://h:1".into()]);
        assert_eq!(w.citadel, "http://h:1");
    }
}
