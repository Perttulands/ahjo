//! One JSON settings file an app keeps (Swarm's `app.json`, Desk's
//! `desk.json`, Brain's `settings.json`), the same way in each: read at
//! start (a missing file is the defaults; one that does not read is set
//! aside beside itself, named in the notice, never silently replaced),
//! written whole through a temporary file a moment after a burst of
//! changes, and read again when the file changes on disk, as an agent
//! edits it. Only the file's own events wake the app; its own writes are
//! not edits, and an edit that does not read is reported while the last
//! good settings stay.
//!
//! The app's root entity holds the file and says how to reach it
//! ([`KeepsSettings`]); the file reaches back to it to write and to tell
//! it what an outside edit changed.

use ahjo_core::Error;
use gpui::{Context, Task};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// What a settings file holds: plain data that reads and writes as JSON.
pub trait SettingsValue: Serialize + DeserializeOwned + Default + PartialEq + 'static {}

impl<T: Serialize + DeserializeOwned + Default + PartialEq + 'static> SettingsValue for T {}

/// The entity that holds a settings file: the app's root, or Swarm's hub.
pub trait KeepsSettings: Sized + 'static {
    type Value: SettingsValue;

    fn settings_file(&mut self) -> &mut SettingsFile<Self::Value>;

    /// The file could not be written, or an edit made outside the app
    /// could not be read: say so; the settings stay as they are.
    fn settings_failed(&mut self, error: Error, cx: &mut Context<Self>);

    /// An edit made outside the app replaced the settings, which were
    /// `old`: apply what changed.
    fn settings_reloaded(&mut self, old: Self::Value, cx: &mut Context<Self>);
}

/// The settings as the app holds them, and the file they are kept in.
/// Reads as the settings themselves.
pub struct SettingsFile<T> {
    value: T,
    path: Option<PathBuf>,
    /// The file's text as the app last wrote or read it: an event that
    /// finds it so is the app's own write.
    known: Option<String>,
    /// The app is writing the file: what is there until it is done is
    /// not an edit.
    writing: bool,
    save_task: Option<Task<()>>,
    /// Watches the file's folder while the file is held.
    watcher: Option<notify::RecommendedWatcher>,
    watch_task: Option<Task<()>>,
}

impl<T> Deref for SettingsFile<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.value
    }
}

impl<T> DerefMut for SettingsFile<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.value
    }
}

/// How long a burst of changes is waited out before the one write.
const SETTLE: Duration = Duration::from_millis(250);

impl<T: SettingsValue> SettingsFile<T> {
    /// The settings at `path`, and what to say about reading them. None
    /// at `path` is the defaults; a file that does not read is moved to
    /// `<name>.broken` and the defaults stand. With no path nothing is
    /// kept.
    pub fn load(path: Option<PathBuf>) -> (Self, Option<Error>) {
        let (value, known, notice) = match &path {
            None => (T::default(), None, None),
            Some(p) => match read::<T>(p) {
                Ok(Some((value, text))) => (value, Some(text), None),
                Ok(None) => (T::default(), None, None),
                Err(e) if e.code == "settings_unreadable" => {
                    let aside = broken(p);
                    let notice = match std::fs::rename(p, &aside) {
                        Ok(()) => format!("{}; kept as {}", e.message, aside.display()),
                        Err(why) => format!("{}; it could not be set aside: {why}", e.message),
                    };
                    (T::default(), None, Some(Error::new(e.code, notice)))
                }
                Err(e) => (T::default(), None, Some(e)),
            },
        };
        let file = Self {
            value,
            path,
            known,
            writing: false,
            save_task: None,
            watcher: None,
            watch_task: None,
        };
        (file, notice)
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// The file's text for these settings.
    pub fn text(&self) -> String {
        text(&self.value)
    }
}

/// Writes `owner`'s settings soon: a burst of changes is one write.
pub fn save<E: KeepsSettings>(owner: &mut E, cx: &mut Context<E>) {
    let file = owner.settings_file();
    if file.path.is_none() {
        return;
    }
    file.save_task = Some(cx.spawn(async move |this, cx| {
        cx.background_executor().timer(SETTLE).await;
        let Ok(Some((path, text))) = this.update(cx, |e, _| {
            let file = e.settings_file();
            let path = file.path.clone()?;
            let text = file.text();
            file.known = Some(text.clone());
            file.writing = true;
            Some((path, text))
        }) else {
            return;
        };
        let saved = cx
            .background_executor()
            .spawn({
                let path = path.clone();
                async move { write(&path, &text) }
            })
            .await;
        let _ = this.update(cx, |e, cx| {
            e.settings_file().writing = false;
            if let Err(why) = saved {
                let error = Error::new(
                    "settings_unwritable",
                    format!("{} could not be saved: {why}", path.display()),
                );
                e.settings_failed(error, cx);
            }
        });
    }));
}

/// Reads `owner`'s settings again whenever their file changes on disk, as
/// an agent edits it, for as long as `owner` holds the file. Only the
/// file's own events wake it. A folder that cannot be watched is reported:
/// edits then apply at the next start.
pub fn watch<E: KeepsSettings>(owner: &mut E, cx: &mut Context<E>) {
    use notify::Watcher;
    let Some(path) = owner.settings_file().path.clone() else {
        return;
    };
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
        return;
    };
    let name = name.to_os_string();
    // One pending wake is enough: the reload reads the file as it is.
    let (tx, rx) = async_channel::bounded::<()>(1);
    let watched = std::fs::create_dir_all(dir)
        .map_err(notify::Error::io)
        .and_then(|_| {
            let mut w = notify::recommended_watcher(move |ev: notify::Result<notify::Event>| {
                // Reading the file is an event too: only a change wakes it.
                let changed = |ev: &notify::Event| {
                    !ev.kind.is_access() && ev.paths.iter().any(|p| p.file_name() == Some(&name))
                };
                if ev.is_ok_and(|ev| changed(&ev)) {
                    let _ = tx.try_send(());
                }
            })?;
            w.watch(dir, notify::RecursiveMode::NonRecursive)?;
            Ok(w)
        });
    let watcher = match watched {
        Ok(w) => w,
        Err(e) => {
            let error = Error::new(
                "settings_unwatched",
                format!(
                    "{} is not watched ({e}); edits to it apply at the next start",
                    path.display()
                ),
            );
            return owner.settings_failed(error, cx);
        }
    };
    let file = owner.settings_file();
    file.watcher = Some(watcher);
    file.watch_task = Some(cx.spawn(async move |this, cx| {
        while rx.recv().await.is_ok() {
            if this.update(cx, |e, cx| reload(e, cx)).is_err() {
                break;
            }
        }
    }));
}

/// Reads `owner`'s settings file again. An edit made outside the app
/// replaces the settings, and the owner hears what they were; a file
/// that does not read is reported and the settings stay as they were.
pub fn reload<E: KeepsSettings>(owner: &mut E, cx: &mut Context<E>) {
    let file = owner.settings_file();
    let Some(path) = file.path.clone().filter(|_| !file.writing) else {
        return;
    };
    let (new, text) = match read::<E::Value>(&path) {
        Ok(Some(read)) => read,
        // Gone, or between a write and its rename: nothing to read.
        Ok(None) => return,
        Err(e) => {
            let error = Error::new(
                e.code,
                format!("{}; the settings stay as they were", e.message),
            );
            return owner.settings_failed(error, cx);
        }
    };
    if file.known.as_deref() == Some(text.as_str()) {
        return;
    }
    file.known = Some(text);
    if new == file.value {
        return;
    }
    let old = std::mem::replace(&mut file.value, new);
    owner.settings_reloaded(old, cx);
}

/// `<name>.broken` beside `path`.
fn broken(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".broken");
    path.with_file_name(name)
}

/// The file's text for `value`.
pub fn text<T: Serialize>(value: &T) -> String {
    serde_json::to_string_pretty(value).expect("settings hold only JSON values")
}

/// The settings at `path` and the text they read from; none when there is
/// no file. A file that does not read is `settings_unreadable`.
pub fn read<T: DeserializeOwned>(path: &Path) -> Result<Option<(T, String)>, Error> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(Error::new(
                "settings_unreadable_file",
                format!("{} could not be read: {e}", path.display()),
            ));
        }
    };
    match serde_json::from_str(&text) {
        Ok(value) => Ok(Some((value, text))),
        Err(e) => Err(Error::new(
            "settings_unreadable",
            format!("{} does not read ({e})", path.display()),
        )),
    }
}

/// Writes `text` to `path` through a temporary file beside it, so a crash
/// never leaves half a file.
pub fn write(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{}.tmp", std::process::id()));
    let tmp = path.with_file_name(name);
    std::fs::write(&tmp, text)?;
    std::fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    #[test]
    fn a_broken_file_is_set_aside_not_lost_and_a_kept_one_reads_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.json");
        let (file, notice) = SettingsFile::<Settings>::load(Some(path.clone()));
        assert!(notice.is_none(), "no file is the defaults");
        assert_eq!(*file, Settings::default());
        std::fs::write(&path, "{not json").unwrap();
        let (file, notice) = SettingsFile::<Settings>::load(Some(path.clone()));
        assert_eq!(*file, Settings::default());
        let notice = notice.unwrap();
        assert_eq!(notice.code, "settings_unreadable");
        assert!(notice.message.contains("app.json.broken"), "{notice:?}");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("app.json.broken")).unwrap(),
            "{not json"
        );
        let kept = Settings {
            keys: false,
            ..Default::default()
        };
        write(&path, &text(&kept)).unwrap();
        let (file, notice) = SettingsFile::<Settings>::load(Some(path));
        assert!(notice.is_none());
        assert_eq!(*file, kept);
    }
}
