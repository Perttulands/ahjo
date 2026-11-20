//! What the page remembers on this device, in its section of the app's
//! settings: the workspace, the terminal's preferences, and the sessions
//! each node listed last.

use super::workspace::Workspace;
use crate::theme::size;
use ahjo_app_kit::zoom::{self, Zoom};
use ahjo_app_kit::{Error, Host, Setting};
use gpui::App;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub font_size: f32,
    /// Peek's own text size, zoomed with Ctrl+wheel; none is `font_size`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peek_font: Option<u8>,
    /// Copy a selection to the clipboard as the mouse is released.
    pub copy_on_select: bool,
    pub collapsed_groups: Vec<String>,
    pub recent_targets: Vec<String>,
    /// The Send drawer's width, as last dragged on this device.
    pub send_width: Option<f32>,
    /// The width of a view beside the windows, as last dragged here.
    pub beside_width: Option<f32>,
    pub last_harness: Option<String>,
    /// The folders launched in, the latest first, by node and user
    /// ([`launched_in`]).
    pub launched_folders: BTreeMap<String, Vec<String>>,
    /// Flags typed per harness, when they differ from the node's.
    pub flags: BTreeMap<String, String>,
    /// The launcher asks a harness to report when it finishes.
    pub install_completion_hooks: bool,
    /// A report marks its session's rail row and the tab holding it.
    pub event_marks: bool,
    /// A report pops the toast; the status line keeps it either way.
    pub event_toast: bool,
    /// A report plays its tone (`store::tones`).
    pub event_tone: bool,
    /// A report posts a desktop notification while the window is not active.
    pub event_desktop: bool,
    #[serde(deserialize_with = "mended")]
    pub workspace: Workspace,
    /// The sessions each node listed last, by node, so an away device keeps
    /// its windows and rail rows, named, until it answers again, across
    /// restarts too.
    pub kept: BTreeMap<String, Vec<Kept>>,
    /// The last known incarnation and restart recipe of each session.
    pub restarts: BTreeMap<String, Restart>,
    /// What the offer to tidy up held when it was waved away
    /// (`store::tidy`), so it stays quiet until something new joins it.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tidy_seen: Vec<String>,
}

/// What the Settings page lists for the terminals, each under its field's
/// name here.
pub const DECLARED: &[Setting] = &[
    Setting::number(
        "font_size",
        "Font size",
        size::TERM_DEFAULT as f64,
        (size::TERM_MIN as f64, size::TERM_MAX as f64, 1.),
    ),
    Setting::toggle("copy_on_select", "Copy on select", true)
        .note("A selection is copied as the mouse is released."),
    Setting::toggle("install_completion_hooks", "Report when done", true)
        .note("The launcher asks a harness to report when it finishes."),
    Setting::toggle("event_marks", "Mark reports", true)
        .note("A report marks its session's row and the tab holding it."),
    Setting::toggle("event_toast", "Toast reports", true),
    Setting::toggle("event_tone", "Play a tone", false),
    Setting::toggle("event_desktop", "Desktop notifications", true)
        .note("While the window is not active."),
];

/// A session as remembered: enough to list it while its device is away.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Kept {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub user: String,
}

/// What can be used to start an ended session again.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Restart {
    pub id: String,
    pub folder: String,
    /// Absent for sessions whose launch the app did not witness.
    pub harness: Option<String>,
    pub flags: String,
    pub install_completion_hooks: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            font_size: size::TERM_DEFAULT,
            peek_font: None,
            copy_on_select: true,
            collapsed_groups: vec![],
            recent_targets: vec![],
            send_width: None,
            beside_width: None,
            last_harness: None,
            launched_folders: Default::default(),
            flags: Default::default(),
            install_completion_hooks: true,
            event_marks: true,
            event_toast: true,
            event_tone: false,
            event_desktop: true,
            workspace: Workspace::default(),
            kept: Default::default(),
            restarts: Default::default(),
            tidy_seen: vec![],
        }
    }
}

/// A workspace that does not read is the default one, not a broken section.
fn mended<'de, D: Deserializer<'de>>(d: D) -> Result<Workspace, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(Workspace::parse(v).unwrap_or_default())
}

impl Settings {
    /// Both startup and live settings reloads use the same, one-time rewrite.
    pub fn load(host: &Host, cx: &mut App) -> Result<Self, Error> {
        let mut section: Map<String, Value> = host.setting(cx)?;
        let migrated = migrate_completion_hooks(&mut section);
        let settings = Self::deserialize(Value::Object(section.clone())).map_err(|error| {
            Error::new("settings_unreadable", format!("Sessions settings: {error}"))
        })?;
        if migrated {
            host.set_setting(&section, cx)?;
        }
        Ok(settings.mend())
    }

    /// Takes what the Settings page sets ([`DECLARED`]) from `fresh`,
    /// keeping the rest as the page has it.
    pub fn take_declared(&mut self, fresh: Settings) {
        self.font_size = fresh.font_size;
        self.copy_on_select = fresh.copy_on_select;
        self.install_completion_hooks = fresh.install_completion_hooks;
        self.event_marks = fresh.event_marks;
        self.event_toast = fresh.event_toast;
        self.event_tone = fresh.event_tone;
        self.event_desktop = fresh.event_desktop;
    }

    /// A terminal's text size: its own, else the Settings size.
    pub fn font(&self, own: Option<u8>) -> f32 {
        own.map_or(self.font_size, f32::from)
            .clamp(size::TERM_MIN, size::TERM_MAX)
    }

    /// A terminal's own text size after `n` steps of a zoom, as browsers
    /// and terminals step it: a pixel each, within the range. A reset, or
    /// a zoom that lands on the Settings size, leaves it none, so it
    /// follows Settings again. `None` when nothing changes.
    pub fn zoom_font(&self, own: Option<u8>, z: Zoom, n: i32) -> Option<Option<u8>> {
        let to = match z {
            Zoom::Reset => return own.is_some().then_some(None),
            Zoom::In => zoom::step(self.font(own), n, 1., size::TERM_MIN, size::TERM_MAX)?,
            Zoom::Out => zoom::step(self.font(own), -n, 1., size::TERM_MIN, size::TERM_MAX)?,
        };
        let own_now = (to != self.font_size).then_some(to.round() as u8);
        (own_now != own).then_some(own_now)
    }

    /// Brings what was read within bounds.
    pub fn mend(mut self) -> Self {
        self.font_size = self.font_size.clamp(size::TERM_MIN, size::TERM_MAX);
        self
    }
}

/// Move the old setting in the launcher and in each saved restart recipe.
fn migrate_completion_hooks(section: &mut Map<String, Value>) -> bool {
    fn rename(object: &mut Map<String, Value>) -> bool {
        if object.contains_key("install_completion_hooks") {
            // A conflicting old key is rejected by the normal decoder.
            return false;
        }
        if let Some(value) = object.remove("notify") {
            object.insert("install_completion_hooks".into(), value);
            return true;
        }
        false
    }

    let mut migrated = rename(section);
    if let Some(Value::Object(restarts)) = section.get_mut("restarts") {
        for restart in restarts.values_mut().filter_map(Value::as_object_mut) {
            migrated |= rename(restart);
        }
    }
    migrated
}

/// Where [`Settings::launched_folders`] keeps a node's folders for `user`
/// (empty: the node's own).
pub fn launched_in(node: &str, user: &str) -> String {
    if user.is_empty() {
        node.to_string()
    } else {
        format!("{node}@{user}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zoom_steps_a_terminals_own_size_within_its_range() {
        let s = Settings {
            font_size: 14.,
            ..Settings::default()
        };
        assert_eq!(s.zoom_font(None, Zoom::In, 1), Some(Some(15)));
        assert_eq!(s.zoom_font(None, Zoom::Out, 2), Some(Some(12)));
        assert_eq!(s.zoom_font(Some(18), Zoom::In, 3), Some(Some(21)));
        // Back on the Settings size it follows Settings again.
        assert_eq!(s.zoom_font(Some(15), Zoom::Out, 1), Some(None));
        assert_eq!(s.zoom_font(Some(18), Zoom::Reset, 1), Some(None));
        assert_eq!(s.zoom_font(None, Zoom::Reset, 1), None);
        let top = size::TERM_MAX as u8;
        assert_eq!(s.zoom_font(Some(top), Zoom::In, 1), None);
        let bottom = size::TERM_MIN as u8;
        assert_eq!(s.zoom_font(Some(bottom), Zoom::Out, 1), None);
        assert_eq!(s.font(Some(18)), 18.);
        assert_eq!(s.font(None), 14.);
    }

    fn parse(text: &str) -> Option<Settings> {
        serde_json::from_str::<Settings>(text)
            .ok()
            .map(Settings::mend)
    }

    /// The declarations name these fields, with their defaults, and a
    /// change to any of them is taken; nothing else is.
    #[test]
    fn what_the_settings_page_lists_is_these_fields() {
        let defaults = serde_json::to_value(Settings::default()).unwrap();
        let mut changed = defaults.clone();
        for setting in DECLARED {
            let kept = &defaults[setting.key];
            assert_eq!(*kept, setting.default_value(), "{}'s default", setting.key);
            changed[setting.key] = match kept {
                serde_json::Value::Bool(b) => (!b).into(),
                _ => setting.stepped(kept, 1.),
            };
        }
        changed["collapsed_groups"] = serde_json::json!(["mine"]);
        let fresh: Settings = serde_json::from_value(changed.clone()).unwrap();
        let mut mine = Settings::default();
        mine.take_declared(fresh);
        let taken = serde_json::to_value(&mine).unwrap();
        for setting in DECLARED {
            assert_eq!(taken[setting.key], changed[setting.key], "{}", setting.key);
        }
        assert_eq!(
            taken["collapsed_groups"],
            serde_json::json!([]),
            "not declared, not taken"
        );
    }

    #[test]
    fn settings_round_trip_and_mend() {
        let mut s = Settings {
            font_size: 16.,
            ..Default::default()
        };
        s.workspace.add_window();
        let back = parse(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
        let odd = parse(r#"{"font_size":40,"workspace":{"tabs":[]}}"#).unwrap();
        assert_eq!(odd.font_size, size::TERM_MAX);
        assert_eq!(odd.workspace, Workspace::default());
        // A device plays tones only once asked to, as CHROTE's.
        assert!(!odd.event_tone && odd.event_toast);
        // A settings file that is not an object does not read; the app's
        // settings, which hold this section, are where that is decided.
        assert!(ahjo_app_kit::settings::Settings::parse("[1]").is_none());
    }
}
