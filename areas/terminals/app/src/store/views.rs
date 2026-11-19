//! Views, kept on the number keys as an RTS keeps its control groups: the
//! active tab's arrangement (its layout and what each window holds) under
//! a number from 1 to 9, with a name. A free number takes the arrangement
//! on screen; a kept one lays it out again in the active tab, its sessions
//! leaving any other window they were bound to. Views replace CHROTE's
//! named presets, which stored every tab: an old preset becomes the view
//! of its first tab that held a session.

use super::workspace::{MAX_TABS, MAX_WINDOWS, Tab, Window, Workspace, mend as mend_tab};
use serde::{Deserialize, Serialize};

/// The number keys views are kept on: 1 to 9.
pub const VIEW_KEYS: u8 = 9;
/// The longest name a view takes.
pub const NAME_MAX: usize = 30;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct View {
    /// Its number key, 1 to 9.
    pub key: u8,
    pub name: String,
    /// The arrangement: its shown windows and layout, unnamed.
    pub tab: Tab,
}

impl Workspace {
    pub fn view(&self, key: u8) -> Option<&View> {
        self.views.iter().find(|v| v.key == key)
    }

    /// The name a view of the active tab would take unasked: its sessions
    /// on screen, in reading order.
    pub fn view_name(&self) -> String {
        let tab = self.tab();
        let names: Vec<&str> = tab
            .order()
            .into_iter()
            .filter_map(|w| tab.windows[w].shown().map(|b| b.key.name.as_str()))
            .collect();
        let name = match names.as_slice() {
            [] => "Empty".to_string(),
            names => names.join(" · "),
        };
        clip(&name)
    }

    /// Keeps the active tab's arrangement as view `key`, named `name` or,
    /// with none, after its sessions; a view kept there before is
    /// replaced, its name kept unless another is given. Returns the name.
    pub fn save_view(&mut self, key: u8, name: Option<&str>) -> Option<String> {
        if !(1..=VIEW_KEYS).contains(&key) {
            return None;
        }
        let given = name.map(clip).filter(|n| !n.is_empty());
        let before = self.view(key).map(|v| v.name.clone());
        let name = given.or(before).unwrap_or_else(|| self.view_name());
        let t = self.tab();
        let mut windows: Vec<Window> = t.shown().to_vec();
        windows.resize_with(MAX_WINDOWS, Window::default);
        let tab = Tab {
            name: None,
            windows,
            count: t.count,
            layout: t.layout.clone(),
        };
        self.views.retain(|v| v.key != key);
        self.views.push(View {
            key,
            name: name.clone(),
            tab,
        });
        self.views.sort_by_key(|v| v.key);
        Some(name)
    }

    /// Lays the active tab out as view `key` keeps it: its windows, their
    /// tags and sizes; the tab keeps its name. Its sessions leave any other
    /// window. Returns its name.
    pub fn recall_view(&mut self, key: u8) -> Option<String> {
        let v = self.view(key)?.clone();
        for b in v.tab.windows.iter().flat_map(|w| &w.sessions) {
            self.unbind_everywhere(&b.key);
        }
        let t = self.tab_mut();
        t.windows = v.tab.windows;
        t.count = v.tab.count;
        t.layout = v.tab.layout;
        mend_tab(t);
        self.focus = self.tab().order().first().copied().unwrap_or(0);
        Some(v.name)
    }

    pub fn rename_view(&mut self, key: u8, name: &str) -> bool {
        let name = clip(name);
        match self.views.iter_mut().find(|v| v.key == key) {
            Some(v) if !name.is_empty() => {
                v.name = name;
                true
            }
            _ => false,
        }
    }

    pub fn delete_view(&mut self, key: u8) -> Option<String> {
        let at = self.views.iter().position(|v| v.key == key)?;
        Some(self.views.remove(at).name)
    }
}

fn clip(name: &str) -> String {
    let name = name.trim();
    if name.chars().count() <= NAME_MAX {
        return name.to_string();
    }
    let mut out: String = name.chars().take(NAME_MAX - 1).collect();
    out.push('…');
    out
}

/// Mends stored views as the workspace is mended: one per key, in order.
pub(super) fn mend(views: &mut Vec<View>) {
    views.retain(|v| (1..=VIEW_KEYS).contains(&v.key));
    views.sort_by_key(|v| v.key);
    views.dedup_by_key(|v| v.key);
    for v in views {
        v.tab.name = None;
        mend_tab(&mut v.tab);
    }
}

/// A preset as CHROTE's menu kept it: a name and every tab.
#[derive(Deserialize)]
struct Preset {
    name: String,
    #[serde(default)]
    tabs: Vec<Tab>,
}

/// The views a workspace stored before views had: each named preset, on
/// the next number, as its first tab that held a session.
pub(super) fn from_presets(v: &serde_json::Value) -> Vec<View> {
    let Some(list) = v.get("presets").cloned() else {
        return vec![];
    };
    let presets: Vec<Preset> = serde_json::from_value(list).unwrap_or_default();
    presets
        .into_iter()
        .take(VIEW_KEYS as usize)
        .enumerate()
        .filter_map(|(k, p)| {
            let tab = p
                .tabs
                .iter()
                .take(MAX_TABS)
                .find(|t| t.shown().iter().any(|w| !w.is_empty()))
                .or(p.tabs.first())?
                .clone();
            Some(View {
                key: k as u8 + 1,
                name: clip(&p.name),
                tab,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::layout::Side;
    use crate::store::sessions::Key;

    fn key(name: &str) -> Key {
        Key {
            node: "n".into(),
            user: String::new(),
            name: name.into(),
        }
    }

    #[test]
    fn a_view_keeps_the_arrangement_and_lays_it_out_again() {
        let mut ws = Workspace::default();
        ws.bind(0, key("a"), None);
        ws.split(0, Side::Bottom);
        ws.bind(1, key("b"), None);
        ws.rename_tab(0, "Ops");
        assert_eq!(ws.save_view(3, None), Some("a · b".into()));
        let kept = ws.tab().layout.clone();

        // Elsewhere: another layout, and a held elsewhere.
        ws.close_window(1);
        ws.split(0, Side::Right);
        ws.bind(1, key("c"), None);
        ws.add_tab();
        ws.bind(0, key("a"), None);
        ws.show_tab(0);
        assert_eq!(ws.recall_view(3), Some("a · b".into()));
        assert_eq!(ws.tab().layout, kept);
        assert_eq!(ws.shown(0).unwrap().key, key("a"));
        assert_eq!(ws.shown(1).unwrap().key, key("b"));
        assert_eq!(
            ws.tab().name.as_deref(),
            Some("Ops"),
            "the tab keeps its name"
        );
        assert!(ws.tabs[1].windows[0].is_empty(), "a left the other tab");
        assert_eq!(ws.bound(&key("c")), None, "not in the view");
        assert_eq!(ws.recall_view(4), None);
    }

    #[test]
    fn a_view_saved_again_keeps_its_name_unless_given_one() {
        let mut ws = Workspace::default();
        ws.bind(0, key("a"), None);
        ws.save_view(1, Some("  Focus  "));
        ws.bind(0, key("b"), None);
        assert_eq!(ws.save_view(1, None), Some("Focus".into()));
        assert_eq!(ws.views.len(), 1);
        assert!(ws.rename_view(1, "Deep work"));
        assert!(!ws.rename_view(1, "   "));
        assert_eq!(ws.save_view(0, None), None, "1 to 9 only");
        assert_eq!(ws.save_view(10, None), None);
        let long = "x".repeat(40);
        ws.save_view(2, Some(&long));
        assert_eq!(ws.view(2).unwrap().name.chars().count(), NAME_MAX);
        assert_eq!(ws.delete_view(1), Some("Deep work".into()));
        assert_eq!(ws.delete_view(1), None);
        let back = Workspace::parse(serde_json::to_value(&ws).unwrap()).unwrap();
        assert_eq!(back.views, ws.views);
    }

    #[test]
    fn old_presets_become_views_of_their_first_tab_that_held_a_session() {
        let v = serde_json::json!({
            "tabs": [{"windows": [null]}],
            "active": 0, "focus": 0,
            "presets": [
                {"name": "old", "tabs": [
                    {"windows": [null], "count": 1},
                    {"name": "x", "windows": [{"node":"n","name":"a"}, {"node":"n","name":"b"}], "count": 2,
                     "sizes": {"count": 2, "columns": [1500, 500]}}
                ]},
                {"name": "bare", "tabs": [{"windows": []}]}
            ]
        });
        let ws = Workspace::parse(v).unwrap();
        assert_eq!(ws.views.len(), 2);
        let old = ws.view(1).unwrap();
        assert_eq!(old.name, "old");
        assert_eq!(old.tab.count, 2);
        assert_eq!(old.tab.name, None);
        assert_eq!(old.tab.windows[1].shown().unwrap().key, key("b"));
        let (_, parts) = old.tab.layout.group(&[]).unwrap();
        assert_eq!((parts[0].0, parts[1].0), (1500, 500), "its dividers too");
        assert_eq!(ws.view(2).unwrap().tab.count, 1);
        let saved = serde_json::to_value(&ws).unwrap();
        assert!(saved.get("presets").is_none(), "kept as views from now on");
    }
}
