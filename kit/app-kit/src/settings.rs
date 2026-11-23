//! What the app remembers on this device: the node it talks to, whether
//! keys are on, the fleet as last seen, and each page's own section, by
//! page id. One JSON file in the config folder (`%APPDATA%\Ahjo\app.json`,
//! `$XDG_CONFIG_HOME/ahjo/app.json`), or the file `AHJO_SWARM_CONFIG` names.
//! A file that doesn't read is reported and set aside, never silently
//! replaced; what it holds that this app does not know is kept as it reads.
//! The running app keeps it as a [`crate::settings_file::SettingsFile`]: an
//! edit made outside it applies at once (`Hub::watch`).

use ahjo_core::Error;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// The node of the app's own environment, on its default port: a Windows
/// node listens on 7301 so it can sit beside a WSL node on 7300.
#[cfg(not(windows))]
pub const DEFAULT_NODE: &str = "http://127.0.0.1:7300";
#[cfg(windows)]
pub const DEFAULT_NODE: &str = "http://127.0.0.1:7301";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub node: String,
    /// Keys on: chords and the leader are intercepted.
    pub keys: bool,
    /// A shortcut's keys show at the foot of the window as it runs: for
    /// showing someone the app, so off unless turned on.
    pub show_keys: bool,
    /// The theme's direction, by its key (`theme::Direction::key`).
    pub theme: String,
    /// The whole app's size, in percent (`zoom::app`).
    pub size: u16,
    /// The side panel (the sessions, on every page) is open; put
    /// away it is a narrow edge.
    pub side_open: bool,
    /// The edge of the window the panel stands at: `right` or `left`.
    pub panel_edge: String,
    /// The panel's width, as last dragged.
    pub panel_width: f32,
    /// The fleet as last seen: each node's device and environment, so an
    /// away device keeps its name until it answers again, across restarts
    /// too, and an address to serve through while the configured node is
    /// away.
    pub places: BTreeMap<String, Place>,
    /// Each page's section, by its id, and whatever else the file holds.
    #[serde(flatten)]
    pub sections: Map<String, Value>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Place {
    pub device: String,
    pub environment: String,
    /// When its link last went, Unix ms: an away device says since when.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seen: Option<u64>,
    /// Where its serving node last heard it on the tailnet: where the app
    /// dials it when the configured node goes away.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub address: String,
    /// What a page remembers of it, kept as it reads.
    #[serde(flatten)]
    pub rest: Map<String, Value>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            node: DEFAULT_NODE.into(),
            keys: true,
            show_keys: false,
            theme: crate::theme::Direction::default().key().into(),
            size: crate::zoom::app::DEFAULT,
            side_open: true,
            panel_edge: "right".into(),
            panel_width: crate::theme::size::PANEL,
            places: Default::default(),
            sections: Map::new(),
        }
    }
}

/// A panel width as kept: between the least and the most a panel takes.
pub fn panel_width(width: f32) -> f32 {
    use crate::theme::size;
    if !width.is_finite() {
        return size::PANEL;
    }
    width.clamp(size::PANEL_MIN, size::PANEL_MAX)
}

pub fn path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("AHJO_SWARM_CONFIG") {
        return Some(p.into());
    }
    let dir = if cfg!(windows) {
        std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("Ahjo"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .map(|d| d.join("ahjo"))
    }?;
    Some(dir.join("app.json"))
}

/// The settings Ahjo Swarm keeps on this device, as they read now: for
/// an app of the kit that runs on its own (Desk, Brain) and follows
/// Swarm's node, theme or size. None when there are none or they do not
/// read; the file is only read.
pub fn swarm() -> Option<Settings> {
    Settings::parse(&std::fs::read_to_string(path()?).ok()?)
}

/// The panel's first width before it opened wide enough to read a file.
const OLD_PANEL: f32 = 300.;

impl Settings {
    pub fn parse(text: &str) -> Option<Self> {
        let mut settings: Self = serde_json::from_str(text).ok()?;
        // Every save wrote the panel's first width, so a file that holds the
        // old one (300) was never dragged: it takes the new first width.
        if settings.panel_width == OLD_PANEL {
            settings.panel_width = crate::theme::size::PANEL;
        }
        Some(settings)
    }

    /// Page `id`'s section: its defaults while it has none.
    pub fn section<T: DeserializeOwned + Default>(&self, id: &str) -> Result<T, Error> {
        match self.sections.get(id) {
            None => Ok(T::default()),
            Some(v) => T::deserialize(v).map_err(|e| {
                Error::new(
                    "settings_unreadable",
                    format!("{id}'s settings do not read: {e}"),
                )
            }),
        }
    }

    /// Writes page `id`'s section.
    pub fn set_section<T: Serialize>(&mut self, id: &str, value: &T) -> Result<(), Error> {
        let v = serde_json::to_value(value).map_err(|e| {
            Error::new(
                "settings_unwritable",
                format!("{id}'s settings do not write: {e}"),
            )
        })?;
        self.sections.insert(id.to_string(), v);
        Ok(())
    }

    /// What page `id`'s section keeps under `key`.
    pub fn value(&self, id: &str, key: &str) -> Option<&Value> {
        self.sections.get(id)?.get(key)
    }

    /// Keeps `value` under `key` in page `id`'s section, leaving the rest
    /// of it as it is.
    pub fn set_value(&mut self, id: &str, key: &str, value: Value) {
        let section = self
            .sections
            .entry(id.to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        if !section.is_object() {
            *section = Value::Object(Map::new());
        }
        if let Value::Object(section) = section {
            section.insert(key.to_string(), value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_keeping_what_they_do_not_know() {
        let mut s = Settings {
            keys: false,
            ..Default::default()
        };
        s.places.insert(
            "n".into(),
            Place {
                device: "rig".into(),
                environment: "wsl".into(),
                ..Default::default()
            },
        );
        s.set_section("toy", &vec![1, 2]).unwrap();
        let back = Settings::parse(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
        assert_eq!(back.section::<Vec<u8>>("toy").unwrap(), [1, 2]);
        assert_eq!(back.section::<Vec<u8>>("other").unwrap(), Vec::<u8>::new());
        assert_eq!(
            back.section::<String>("toy").unwrap_err().code,
            "settings_unreadable"
        );
        // A kept width wins; the old first width was never a choice.
        let kept = |w: f32| Settings::parse(&format!(r#"{{"panel_width":{w}}}"#)).unwrap();
        assert_eq!(kept(300.).panel_width, crate::theme::size::PANEL);
        assert_eq!(kept(640.).panel_width, 640.);
        assert_eq!(
            Settings::parse("{}").unwrap().panel_width,
            crate::theme::size::PANEL
        );
        // What an older app wrote, and a page not built here, stays.
        let old = r#"{"node":"http://rig:7300","font_size":16,"places":{"n":{"device":"rig","environment":"wsl","sessions":[{"name":"a"}]}}}"#;
        let s = Settings::parse(old).unwrap();
        assert!(s.keys, "keys default on");
        let again: Value = serde_json::to_value(&s).unwrap();
        assert_eq!(again["font_size"], 16);
        assert_eq!(again["places"]["n"]["sessions"][0]["name"], "a");
        assert!(Settings::parse("[1]").is_none());
    }
}
