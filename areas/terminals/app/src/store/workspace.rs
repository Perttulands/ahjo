//! The terminal workspace, as CHROTE's (ADR-0017 decision 5): tabs, each a
//! layout of windows split side by side and stacked as the operator likes
//! (`layout::Split`), each window holding the sessions the operator bound
//! to it as tags, one of them on screen. A binding is intent, not a cache
//! of live sessions: it stays until the operator removes it, whatever
//! happens to the session. A session is bound in one window at a time;
//! binding it elsewhere moves it.
//!
//! Every tab keeps eight windows and every workspace six tabs; the counts
//! say how many are shown, and the shown windows are the layout's leaves.
//! Fewer windows hide the rest with their bindings and names, and more
//! bring them back: a layout shrinks, it never forgets, except a window
//! the operator closes. The app owns all of this; losing it loses a
//! layout, never work.

use super::layout::{Side, Split};
use super::sessions::{Key, find};
use super::views::View;
use ahjo_terminals_contract::Session;
use serde::{Deserialize, Serialize};

pub const MAX_TABS: usize = 6;
pub const MAX_WINDOWS: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    #[serde(flatten)]
    pub key: Key,
    /// The pane on screen, for sessions with more than one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<u32>,
}

/// A window: the sessions bound to it, one tag each, and the tag on screen.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "StoredWindow")]
pub struct Window {
    pub sessions: Vec<Binding>,
    /// The tag on screen.
    pub active: usize,
    /// Its own text size in pixels, zoomed with Ctrl+wheel; none is the
    /// Settings size.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font: Option<u8>,
}

/// A window as stored: today's, or one binding or none, as an app before
/// tags kept it.
#[derive(Deserialize)]
#[serde(untagged)]
enum StoredWindow {
    One(Binding),
    Tags {
        #[serde(default)]
        sessions: Vec<Binding>,
        #[serde(default)]
        active: usize,
        #[serde(default)]
        font: Option<u8>,
    },
    Empty(()),
}

impl From<StoredWindow> for Window {
    fn from(s: StoredWindow) -> Self {
        let (sessions, active, font) = match s {
            StoredWindow::One(b) => (vec![b], 0, None),
            StoredWindow::Tags {
                sessions,
                active,
                font,
            } => (sessions, active, font),
            StoredWindow::Empty(()) => (vec![], 0, None),
        };
        let active = active.min(sessions.len().saturating_sub(1));
        Self {
            sessions,
            active,
            font,
        }
    }
}

impl Window {
    /// The binding on screen.
    pub fn shown(&self) -> Option<&Binding> {
        self.sessions.get(self.active)
    }

    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    pub(super) fn holds(&self, key: &Key) -> Option<usize> {
        self.sessions.iter().position(|b| &b.key == key)
    }

    /// Drops a tag; the one on screen stays, or the first takes its place.
    fn drop_tag(&mut self, at: usize) {
        self.sessions.remove(at);
        if at == self.active {
            self.active = 0;
        } else if at < self.active {
            self.active -= 1;
        }
    }
}

fn windows() -> Vec<Window> {
    vec![Window::default(); MAX_WINDOWS]
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "StoredTab")]
pub struct Tab {
    /// The operator's name for it; unnamed tabs read Sessions, Sessions 2…
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Always eight; the first `count` are shown.
    pub windows: Vec<Window>,
    pub count: usize,
    /// How the shown windows share the tab: its leaves are `0..count`.
    pub layout: Split,
}

/// A tab as stored: today's, or one from before the layout tree, whose
/// windows stood in one row of up to three or two rows of two, with the
/// weights their dividers were dragged to for that many windows.
#[derive(Deserialize)]
struct StoredTab {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    windows: Vec<Window>,
    #[serde(default)]
    count: usize,
    #[serde(default)]
    layout: Option<Split>,
    #[serde(default)]
    sizes: Option<OldSizes>,
}

#[derive(Deserialize)]
struct OldSizes {
    count: usize,
    columns: Vec<u16>,
    #[serde(default = "half")]
    top: u16,
}

fn half() -> u16 {
    500
}

impl From<StoredTab> for Tab {
    fn from(s: StoredTab) -> Self {
        let count = shown_count(s.count, s.windows.len());
        let layout = s.layout.unwrap_or_else(|| {
            let dragged = s
                .sizes
                .as_ref()
                .filter(|z| z.count == count)
                .map(|z| (&z.columns[..], z.top));
            Split::from_columns(count, dragged)
        });
        let mut t = Self {
            name: s.name,
            windows: s.windows,
            count,
            layout,
        };
        mend(&mut t);
        t
    }
}

/// How many windows a stored tab shows: as it says, else every window it
/// kept, as before the count was kept.
fn shown_count(count: usize, kept: usize) -> usize {
    let n = if count == 0 { kept.max(1) } else { count };
    n.min(MAX_WINDOWS)
}

/// The least share of the room a divider leaves either window, and the
/// fewest pixels.
pub const LEAST_SHARE: f32 = 0.08;

impl Tab {
    pub fn new() -> Self {
        Self {
            name: None,
            windows: windows(),
            count: 1,
            layout: Split::Window(0),
        }
    }

    pub fn shown(&self) -> &[Window] {
        &self.windows[..self.count]
    }

    /// Its shown windows in reading order: left to right, top to bottom.
    pub fn order(&self) -> Vec<usize> {
        self.layout.windows()
    }

    /// Hides window `w` behind the shown ones, with what it holds: its
    /// room goes to the windows beside it, and the windows after it move
    /// down one. False for the last window.
    pub(super) fn hide(&mut self, w: usize) -> bool {
        if self.count <= 1 || w >= self.count || !self.layout.remove(w) {
            return false;
        }
        let gone = self.windows.remove(w);
        self.windows.insert(self.count - 1, gone);
        self.layout.renumber(&|x| if x > w { x - 1 } else { x });
        self.count -= 1;
        true
    }

    /// Shows hidden window `count` beside `at`, on `side`.
    fn reveal(&mut self, at: usize, side: Side) -> bool {
        if self.count >= MAX_WINDOWS || at >= self.count {
            return false;
        }
        let new = self.count;
        if !self.layout.insert(at, side, new) {
            return false;
        }
        self.count += 1;
        true
    }

    /// The window with the most room, and the side its longer edge splits
    /// on, in a tab as wide as a desktop.
    fn roomiest(&self) -> (usize, Side) {
        let mut rects = vec![];
        self.layout.rects(0., 0., 16., 9., &mut rects);
        rects
            .into_iter()
            .max_by(|a, b| (a.1[2] * a.1[3]).total_cmp(&(b.1[2] * b.1[3])))
            .map(|(w, r)| {
                (
                    w,
                    if r[2] >= r[3] {
                        Side::Right
                    } else {
                        Side::Bottom
                    },
                )
            })
            .unwrap_or((0, Side::Right))
    }
}

impl Default for Tab {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    /// Always six; the first `count` are shown.
    pub tabs: Vec<Tab>,
    #[serde(default)]
    pub count: usize,
    pub active: usize,
    /// The focused window in the active tab.
    pub focus: usize,
    /// Arrangements kept on the number keys, as an RTS keeps its control
    /// groups (`views`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub views: Vec<View>,
}

impl Default for Workspace {
    fn default() -> Self {
        Self {
            tabs: (0..MAX_TABS).map(|_| Tab::new()).collect(),
            count: 1,
            active: 0,
            focus: 0,
            views: vec![],
        }
    }
}

/// What a window shows.
#[derive(Clone, Debug, PartialEq)]
pub enum TileState<'a> {
    Empty,
    Live {
        session: &'a Session,
        pane: u32,
    },
    /// Bound, and the node has not said yet whether the session is there.
    Connecting(&'a str),
    /// The connection is gone; the last screen stays with Reconnect.
    Lost(&'a str),
    /// The session is gone from a list the node sent.
    Ended(&'a str),
}

pub use ahjo_app_kit::link::client::LinkState;

impl Workspace {
    /// The tabs on the bar.
    pub fn shown_tabs(&self) -> &[Tab] {
        &self.tabs[..self.count]
    }

    pub fn tab(&self) -> &Tab {
        &self.tabs[self.active]
    }

    pub(super) fn tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active]
    }

    /// The active tab's layout, to drag its dividers.
    pub fn layout_mut(&mut self) -> &mut Split {
        &mut self.tab_mut().layout
    }

    /// The windows of the active tab on screen.
    pub fn windows(&self) -> &[Window] {
        self.tab().shown()
    }

    /// The binding window `i` of the active tab shows.
    pub fn shown(&self, i: usize) -> Option<&Binding> {
        self.windows().get(i)?.shown()
    }

    /// Gives window `i` of the active tab its own text size, or the
    /// Settings size again (`None`).
    pub fn set_font(&mut self, i: usize, font: Option<u8>) {
        if let Some(w) = self.tab_mut().windows.get_mut(i) {
            w.font = font;
        }
    }

    /// Shows one more tab: the next hidden one, with what it held.
    pub fn add_tab(&mut self) -> bool {
        if self.count >= MAX_TABS {
            return false;
        }
        self.count += 1;
        self.active = self.count - 1;
        self.focus = 0;
        true
    }

    /// Hides a tab, with its name and bindings, behind the shown ones; the
    /// last one stays.
    pub fn close_tab(&mut self, index: usize) -> bool {
        if self.count <= 1 || index >= self.count {
            return false;
        }
        let tab = self.tabs.remove(index);
        self.tabs.insert(self.count - 1, tab);
        self.count -= 1;
        if self.active > index || self.active >= self.count {
            self.active = self.active.saturating_sub(1);
        }
        self.focus = self.focus.min(self.windows().len() - 1);
        true
    }

    pub fn show_tab(&mut self, index: usize) -> bool {
        if index >= self.count || index == self.active {
            return false;
        }
        self.active = index;
        self.focus = 0;
        true
    }

    /// Shows one more window: the next hidden one, with what it held, in
    /// the half of the roomiest window.
    pub fn add_window(&mut self) -> bool {
        let (at, side) = self.tab().roomiest();
        if !self.tab_mut().reveal(at, side) {
            return false;
        }
        self.focus = self.windows().len() - 1;
        true
    }

    /// Opens an empty window beside window `at`, on `side`, and focuses
    /// it; false when the tab holds all it can.
    pub fn split(&mut self, at: usize, side: Side) -> bool {
        let n = self.windows().len();
        if n >= MAX_WINDOWS || at >= n {
            return false;
        }
        self.tab_mut().windows[n] = Window::default();
        if !self.tab_mut().reveal(at, side) {
            return false;
        }
        self.focus = n;
        true
    }

    /// Closes window `w` of the active tab: its tags go with it (their
    /// sessions run on) and the windows beside it take its room. The last
    /// window stays.
    pub fn close_window(&mut self, w: usize) -> bool {
        let order = self.tab().order();
        let Some(at) = order.iter().position(|x| *x == w) else {
            return false;
        };
        // Focus goes to the window before it in reading order, else after.
        let next = if at > 0 {
            order[at - 1]
        } else {
            order.get(1).copied().unwrap_or(0)
        };
        let t = self.tab_mut();
        if !t.hide(w) {
            return false;
        }
        let n = t.count;
        t.windows[n] = Window::default();
        let was = self.focus;
        let focus = if was == w { next } else { was };
        self.focus = if focus > w { focus - 1 } else { focus };
        true
    }

    /// Closes the active tab's empty window: the focused one if it is
    /// empty, else the last; false when none is, or it is the only one.
    pub fn remove_empty_window(&mut self) -> bool {
        let windows = self.windows();
        let w = if windows.get(self.focus).is_some_and(Window::is_empty) {
            Some(self.focus)
        } else {
            windows.iter().rposition(Window::is_empty)
        };
        w.is_some_and(|w| self.close_window(w))
    }

    /// Moves window `w` beside window `to`, on `side`, with its tags.
    pub fn move_window(&mut self, w: usize, to: usize, side: Side) -> bool {
        let n = self.windows().len();
        if w >= n || to >= n || !self.tab_mut().layout.place(w, to, side) {
            return false;
        }
        self.focus = w;
        true
    }

    /// Windows `a` and `b` of the active tab trade places with what they
    /// hold; focus follows the focused one's sessions.
    pub fn swap(&mut self, a: usize, b: usize) -> bool {
        let n = self.windows().len();
        if a == b || a >= n || b >= n {
            return false;
        }
        self.tab_mut().windows.swap(a, b);
        if self.focus == a {
            self.focus = b;
        } else if self.focus == b {
            self.focus = a;
        }
        true
    }

    /// Lays the active tab's windows out afresh, in reading order: side by
    /// side, stacked, or as an even grid.
    pub fn arrange(&mut self, how: Arrange) {
        let order = self.tab().order();
        let n = order.len();
        let t = self.tab_mut();
        // The windows take the places their reading order says.
        let mut windows: Vec<Window> = order.iter().map(|w| t.windows[*w].clone()).collect();
        windows.extend(t.windows.drain(n..));
        t.windows = windows;
        t.layout = match how {
            Arrange::Row => line(super::layout::Axis::Row, n),
            Arrange::Column => line(super::layout::Axis::Column, n),
            Arrange::Grid => Split::even(n),
        };
        self.focus = order.iter().position(|w| *w == self.focus).unwrap_or(0);
    }

    /// Gives every part of the active tab's layout the same share.
    pub fn even_out(&mut self) {
        fn even(s: &mut Split) {
            if let Split::Group { parts, .. } = s {
                for (share, p) in parts.iter_mut() {
                    *share = super::layout::EVEN;
                    even(p);
                }
            }
        }
        even(&mut self.tab_mut().layout);
    }

    /// Sets how many windows tab `tab` shows: the last ones are hidden
    /// with their bindings, or the hidden ones come back, each in the half
    /// of the roomiest window (the tab menu's Windows).
    pub fn set_windows(&mut self, tab: usize, n: usize) {
        let Some(t) = self.tabs.get_mut(tab) else {
            return;
        };
        let n = n.clamp(1, MAX_WINDOWS);
        while t.count > n {
            let last = t.count - 1;
            t.hide(last);
        }
        while t.count < n {
            let (at, side) = t.roomiest();
            if !t.reveal(at, side) {
                break;
            }
        }
        if tab == self.active {
            self.focus = self.focus.min(t.count - 1);
        }
    }

    /// Focuses the window `by` steps along the reading order, round the
    /// ends.
    pub fn focus_step(&mut self, by: isize) {
        let order = self.tab().order();
        let at = order.iter().position(|w| *w == self.focus).unwrap_or(0) as isize;
        let n = order.len() as isize;
        self.focus = order[(at + by).rem_euclid(n) as usize];
    }

    /// Binds a session to window `window` of tab `tab` as its tag on
    /// screen, moving it from any other window it was bound to.
    pub fn bind_in(&mut self, tab: usize, window: usize, key: Key, pane: Option<u32>) {
        if tab >= MAX_TABS || window >= MAX_WINDOWS {
            return;
        }
        self.unbind_everywhere(&key);
        let w = &mut self.tabs[tab].windows[window];
        w.sessions.push(Binding { key, pane });
        w.active = w.sessions.len() - 1;
        if tab == self.active {
            self.focus = window;
        }
    }

    /// Binds in the active tab.
    pub fn bind(&mut self, window: usize, key: Key, pane: Option<u32>) {
        self.bind_in(self.active, window, key, pane);
    }

    /// Removes one tag from window `window` of the active tab.
    pub fn unbind(&mut self, window: usize, key: &Key) {
        if let Some(w) = self.tab_mut().windows.get_mut(window)
            && let Some(at) = w.holds(key)
        {
            w.drop_tag(at);
        }
    }

    /// Removes a session from every window it is bound to, shown or hidden.
    pub fn unbind_everywhere(&mut self, key: &Key) {
        for tab in &mut self.tabs {
            for w in &mut tab.windows {
                if let Some(at) = w.holds(key) {
                    w.drop_tag(at);
                }
            }
        }
    }

    /// Puts a bound session on screen in its window.
    pub fn show_tag(&mut self, window: usize, key: &Key) -> bool {
        match self.tab_mut().windows.get_mut(window) {
            Some(w) => match w.holds(key) {
                Some(at) if at != w.active => {
                    w.active = at;
                    true
                }
                _ => false,
            },
            None => false,
        }
    }

    /// Empties every window of tab `tab`, shown or hidden.
    pub fn clear(&mut self, tab: usize) {
        if let Some(t) = self.tabs.get_mut(tab) {
            t.windows = windows();
        }
    }

    pub fn choose_pane(&mut self, window: usize, pane: u32) {
        if let Some(w) = self.tab_mut().windows.get_mut(window)
            && let Some(b) = w.sessions.get_mut(w.active)
        {
            b.pane = Some(pane);
        }
    }

    pub fn rename_tab(&mut self, index: usize, name: &str) {
        if let Some(t) = self.tabs.get_mut(index) {
            let n = name.trim();
            t.name = (!n.is_empty()).then(|| n.to_string());
        }
    }

    /// A session the node renamed keeps its tag, in every tab, under its
    /// new name.
    pub fn rename_session(&mut self, old: &Key, name: &str) {
        for b in self
            .tabs
            .iter_mut()
            .flat_map(|t| t.windows.iter_mut())
            .flat_map(|w| w.sessions.iter_mut())
        {
            if &b.key == old {
                b.key.name = name.to_string();
            }
        }
    }

    /// The first empty window of the active tab: the focused one if it is.
    pub fn empty_window(&self) -> Option<usize> {
        let w = self.windows();
        if w.get(self.focus).is_some_and(Window::is_empty) {
            return Some(self.focus);
        }
        w.iter().position(Window::is_empty)
    }

    /// Where a session is bound in a window on screen: tab and window.
    pub fn bound(&self, key: &Key) -> Option<(usize, usize)> {
        self.shown_tabs().iter().enumerate().find_map(|(t, tab)| {
            tab.shown()
                .iter()
                .position(|w| w.holds(key).is_some())
                .map(|w| (t, w))
        })
    }

    /// The binding each shown window of each shown tab has on screen, for
    /// subscriptions: a tag behind another one stays idle.
    pub fn on_screen(&self) -> impl Iterator<Item = &Binding> {
        self.shown_tabs()
            .iter()
            .flat_map(|t| t.shown().iter().filter_map(Window::shown))
    }

    /// What the app opens with: each tab's empty windows put behind the
    /// ones that hold a session, so a restart shows terminals rather than
    /// empty frames; a tab keeps one window at least.
    pub fn fold_empty(&mut self) {
        let focused = self.shown(self.focus).map(|b| b.key.clone());
        for t in &mut self.tabs {
            for w in (0..t.count).rev() {
                if t.windows[w].is_empty() {
                    t.hide(w);
                }
            }
        }
        self.focus = focused
            .and_then(|k| self.windows().iter().position(|w| w.holds(&k).is_some()))
            .unwrap_or(0);
    }

    /// Reads a stored workspace, mending what is out of range; `None` if it
    /// isn't one. A workspace stored before hidden tabs and windows shows
    /// what it had.
    pub fn parse(v: serde_json::Value) -> Option<Self> {
        let presets = super::views::from_presets(&v);
        let mut ws: Workspace = serde_json::from_value(v).ok()?;
        if ws.views.is_empty() {
            ws.views = presets;
        }
        if ws.tabs.is_empty() {
            return None;
        }
        ws.tabs.truncate(MAX_TABS);
        if ws.count == 0 {
            ws.count = ws.tabs.len();
        }
        ws.tabs.resize_with(MAX_TABS, Tab::new);
        ws.count = ws.count.min(MAX_TABS);
        for t in &mut ws.tabs {
            mend(t);
        }
        ws.active = ws.active.min(ws.count - 1);
        ws.focus = ws.focus.min(ws.windows().len() - 1);
        super::views::mend(&mut ws.views);
        ws.dedupe();
        Some(ws)
    }

    /// Keeps each session in the first window that binds it.
    pub(super) fn dedupe(&mut self) {
        let mut seen = std::collections::HashSet::new();
        for w in self.tabs.iter_mut().flat_map(|t| t.windows.iter_mut()) {
            let mut at = 0;
            while at < w.sessions.len() {
                if seen.insert(w.sessions[at].key.clone()) {
                    at += 1;
                } else {
                    w.drop_tag(at);
                }
            }
        }
    }
}

/// Eight windows, one to eight of them shown, and a layout of exactly
/// those.
pub(super) fn mend(t: &mut Tab) {
    t.windows.truncate(MAX_WINDOWS);
    t.count = shown_count(t.count, t.windows.len());
    t.windows.resize_with(MAX_WINDOWS, Window::default);
    if !t.layout.holds_exactly(t.count) {
        t.layout = Split::even(t.count);
    }
}

/// How the tab menu's Arrange lays the windows out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arrange {
    Row,
    Column,
    Grid,
}

fn line(axis: super::layout::Axis, n: usize) -> Split {
    if n == 1 {
        return Split::Window(0);
    }
    Split::Group {
        axis,
        parts: (0..n)
            .map(|w| (super::layout::EVEN, Split::Window(w)))
            .collect(),
    }
}

/// The label a tab carries: its name as given, or Sessions, Sessions 2…
pub fn tab_label(tab: &Tab, index: usize) -> String {
    match &tab.name {
        Some(n) => n.clone(),
        None if index == 0 => "Sessions".into(),
        None => format!("Sessions {}", index + 1),
    }
}

/// What the node a window's session is on looks like from here: its
/// sessions, its link, and whether it has listed them on this link. `None`
/// when the app reaches no such node now.
pub type Reach<'a> = (&'a [Session], LinkState, bool);

/// What a window shows. Until the node has listed its sessions on this link,
/// a missing session isn't evidence of an ended one. A session mark-seen to end
/// stays ended when the connection drops.
pub fn tile_state<'a>(binding: Option<&'a Binding>, node: Option<Reach<'a>>) -> TileState<'a> {
    let Some(b) = binding else {
        return TileState::Empty;
    };
    // A device the app does not reach is away; its session hasn't ended.
    let Some((sessions, link, known)) = node else {
        return TileState::Lost(&b.key.name);
    };
    let session = find(sessions, &b.key);
    let gone = session.is_none_or(|s| s.panes.is_empty());
    if known && gone {
        return TileState::Ended(&b.key.name);
    }
    if matches!(link, LinkState::Lost | LinkState::Refused) {
        return TileState::Lost(&b.key.name);
    }
    match session {
        Some(s) if link == LinkState::Open && known => {
            let pane = s
                .panes
                .iter()
                .find(|p| Some(p.id) == b.pane)
                .unwrap_or(&s.panes[0]);
            TileState::Live {
                session: s,
                pane: pane.id,
            }
        }
        _ => TileState::Connecting(&b.key.name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ahjo_terminals_contract::Pane;

    fn key(name: &str) -> Key {
        Key {
            node: "n".into(),
            user: String::new(),
            name: name.into(),
        }
    }

    #[test]
    fn a_split_opens_an_empty_window_on_the_side_asked() {
        let mut ws = Workspace::default();
        ws.bind_in(0, 0, key("a"), None);
        assert!(ws.add_window());
        ws.bind_in(0, 1, key("b"), None);
        assert!(ws.split(0, Side::Bottom));
        assert_eq!(ws.focus, 2);
        assert!(ws.windows()[2].is_empty());
        assert_eq!(ws.tab().order(), [0, 2, 1], "below a, before b");
        let mut rects = vec![];
        ws.tab().layout.rects(0., 0., 2., 2., &mut rects);
        assert_eq!(rects[1], (2, [0., 1., 1., 1.]), "under a, in its column");
        while ws.split(0, Side::Right) {}
        assert_eq!(ws.windows().len(), MAX_WINDOWS, "eight is the most");
    }

    #[test]
    fn a_closed_window_gives_its_room_and_the_rest_keep_their_tags() {
        let mut ws = Workspace::default();
        ws.bind(0, key("a"), None);
        ws.split(0, Side::Right);
        ws.bind(1, key("b"), None);
        ws.split(1, Side::Bottom);
        ws.bind(2, key("c"), None);
        assert!(ws.close_window(1));
        assert_eq!(ws.windows().len(), 2);
        assert_eq!(ws.shown(0).unwrap().key, key("a"));
        assert_eq!(ws.shown(1).unwrap().key, key("c"), "c moved down one");
        assert_eq!(ws.bound(&key("b")), None, "its tags went with it");
        assert_eq!(ws.tab().order(), [0, 1]);
        assert_eq!(ws.focus, 1, "focus stays on c");
        assert!(ws.close_window(0));
        assert_eq!(ws.focus, 0);
        assert!(!ws.close_window(0), "the last window stays");
        assert!(ws.tab().windows[1..].iter().all(Window::is_empty));
    }

    #[test]
    fn a_window_moves_with_its_tags_and_the_layout_can_be_arranged() {
        let mut ws = Workspace::default();
        ws.set_windows(0, 3);
        ws.bind(0, key("a"), None);
        ws.bind(2, key("c"), None);
        assert!(ws.move_window(0, 2, Side::Bottom));
        assert_eq!(ws.tab().order(), [1, 2, 0]);
        assert_eq!(ws.focus, 0);
        assert!(!ws.move_window(1, 1, Side::Left));
        ws.arrange(Arrange::Column);
        assert_eq!(ws.tab().order(), [0, 1, 2]);
        assert_eq!(ws.shown(1).unwrap().key, key("c"));
        assert_eq!(ws.shown(2).unwrap().key, key("a"));
        assert_eq!(ws.focus, 2, "focus followed a");
    }

    #[test]
    fn dividers_are_kept_and_evened() {
        let mut ws = Workspace::default();
        ws.add_window();
        let (_, parts) = ws.layout_mut().group_mut(&[]).unwrap();
        let mut shares: Vec<u16> = parts.iter().map(|p| p.0).collect();
        crate::store::layout::drag(&mut shares, 0, 0.25, LEAST_SHARE);
        for (p, s) in parts.iter_mut().zip(&shares) {
            p.0 = *s;
        }
        let back = Workspace::parse(serde_json::to_value(&ws).unwrap()).unwrap();
        assert_eq!(back.tab().layout, ws.tab().layout, "kept across a restart");
        ws.even_out();
        assert_eq!(ws.tab().layout, Split::even(2));
    }

    fn tags(w: &Window) -> Vec<&str> {
        w.sessions.iter().map(|b| b.key.name.as_str()).collect()
    }

    fn session(name: &str, panes: &[u32]) -> Session {
        Session {
            node: "n".into(),
            id: "$1".into(),
            name: name.into(),
            panes: panes
                .iter()
                .map(|&id| Pane {
                    id,
                    cols: 80,
                    rows: 24,
                    waiting: None,
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn a_window_holds_several_tags_and_shows_the_newest() {
        let mut ws = Workspace::default();
        ws.bind(0, key("a"), None);
        ws.bind(0, key("b"), None);
        assert_eq!(tags(&ws.windows()[0]), ["a", "b"]);
        assert_eq!(ws.shown(0).unwrap().key, key("b"));
        assert!(ws.show_tag(0, &key("a")));
        assert!(!ws.show_tag(0, &key("a")), "already on screen");
        assert_eq!(ws.shown(0).unwrap().key, key("a"));
        ws.unbind(0, &key("a"));
        assert_eq!(
            ws.shown(0).unwrap().key,
            key("b"),
            "the first tag left takes the screen"
        );
        ws.bind(0, key("c"), None);
        ws.unbind(0, &key("b"));
        assert_eq!(tags(&ws.windows()[0]), ["c"]);
        assert_eq!(ws.shown(0).unwrap().key, key("c"));
    }

    #[test]
    fn binding_a_session_elsewhere_moves_it() {
        let mut ws = Workspace::default();
        ws.add_window();
        ws.bind(0, key("a"), None);
        ws.bind(0, key("b"), None);
        ws.show_tag(0, &key("a"));
        ws.bind(1, key("a"), None);
        assert_eq!(tags(&ws.windows()[0]), ["b"]);
        assert_eq!(ws.shown(0).unwrap().key, key("b"));
        assert_eq!(tags(&ws.windows()[1]), ["a"]);
        assert_eq!(ws.focus, 1);
        ws.add_tab();
        ws.bind(0, key("a"), None);
        assert!(ws.tabs[0].windows[1].is_empty(), "across tabs too");
        assert_eq!(ws.bound(&key("a")), Some((1, 0)));
    }

    #[test]
    fn fewer_windows_hide_the_rest_with_their_bindings() {
        let mut ws = Workspace::default();
        ws.set_windows(0, 4);
        assert_eq!(ws.windows().len(), 4);
        ws.bind(3, key("a"), None);
        ws.set_windows(0, 2);
        assert_eq!(ws.windows().len(), 2);
        assert_eq!(ws.focus, 1);
        assert!(ws.tab().layout.holds_exactly(2));
        assert_eq!(ws.bound(&key("a")), None, "hidden");
        assert_eq!(ws.on_screen().count(), 0);
        ws.add_window();
        ws.add_window();
        assert_eq!(ws.shown(3).unwrap().key, key("a"), "and back");
        ws.set_windows(0, 99);
        assert_eq!(ws.windows().len(), MAX_WINDOWS);
        assert!(ws.tab().layout.holds_exactly(MAX_WINDOWS));
    }

    #[test]
    fn a_closed_tab_is_hidden_with_its_name_and_bindings() {
        let mut ws = Workspace::default();
        while ws.add_tab() {}
        assert_eq!(ws.shown_tabs().len(), MAX_TABS);
        assert_eq!(tab_label(&ws.tabs[2], 2), "Sessions 3");
        ws.show_tab(1);
        ws.rename_tab(1, "ops");
        ws.bind(0, key("a"), None);
        assert!(ws.close_tab(1));
        assert_eq!(ws.shown_tabs().len(), 5);
        assert_eq!(ws.active, 1, "the next tab takes its place");
        assert_eq!(ws.bound(&key("a")), None);
        assert!(ws.add_tab());
        assert_eq!(ws.active, 5);
        assert_eq!(ws.tab().name.as_deref(), Some("ops"));
        assert_eq!(ws.shown(0).unwrap().key, key("a"));
        while ws.close_tab(0) {}
        assert_eq!(ws.shown_tabs().len(), 1);
        assert_eq!(ws.active, 0);
        assert!(!ws.close_tab(0), "the last tab stays");
    }

    #[test]
    fn only_the_tag_on_screen_of_a_shown_window_is_followed() {
        let mut ws = Workspace::default();
        ws.add_window();
        ws.bind(0, key("a"), None);
        ws.bind(0, key("b"), None);
        ws.bind(1, key("c"), None);
        ws.add_tab();
        ws.bind(0, key("d"), None);
        ws.set_windows(1, 1);
        ws.add_window();
        ws.bind(1, key("e"), None);
        ws.set_windows(1, 1);
        let on: Vec<&str> = ws.on_screen().map(|b| b.key.name.as_str()).collect();
        assert_eq!(on, ["b", "c", "d"]);
    }

    #[test]
    fn a_renamed_session_keeps_its_tags_in_every_tab() {
        let mut ws = Workspace::default();
        ws.add_window();
        ws.bind(0, key("a"), Some(7));
        ws.bind(1, key("b"), None);
        let theirs = Key {
            user: "alice".into(),
            ..key("a")
        };
        ws.bind(1, theirs.clone(), None);
        ws.rename_session(&key("a"), "z");
        assert_eq!(ws.bound(&key("a")), None);
        assert_eq!(ws.shown(0).unwrap().key, key("z"));
        assert_eq!(ws.shown(0).unwrap().pane, Some(7));
        assert_eq!(tags(&ws.windows()[1]), ["b", "a"]);
        assert_eq!(
            ws.shown(1).unwrap().key,
            theirs,
            "another user's session of that name stays"
        );
    }

    #[test]
    fn clearing_a_tab_empties_its_hidden_windows_too() {
        let mut ws = Workspace::default();
        ws.set_windows(0, 3);
        ws.bind(2, key("a"), None);
        ws.set_windows(0, 1);
        ws.clear(0);
        ws.set_windows(0, 3);
        assert!(ws.windows().iter().all(Window::is_empty));
    }

    #[test]
    fn focus_steps_in_reading_order_and_wraps() {
        let mut ws = Workspace::default();
        ws.set_windows(0, 2);
        ws.split(0, Side::Bottom);
        ws.focus = 0;
        ws.focus_step(1);
        assert_eq!(ws.focus, 2, "below the first, before the one beside it");
        ws.focus_step(1);
        assert_eq!(ws.focus, 1);
        ws.focus_step(1);
        assert_eq!(ws.focus, 0);
        ws.focus_step(-1);
        assert_eq!(ws.focus, 1);
    }

    #[test]
    fn a_tile_tells_ended_from_lost_from_connecting() {
        let live = [session("a", &[4, 5])];
        let b = Binding {
            key: key("a"),
            pane: Some(5),
        };
        assert_eq!(
            tile_state(Some(&b), Some((&live, LinkState::Open, true))),
            TileState::Live {
                session: &live[0],
                pane: 5
            }
        );
        assert_eq!(
            tile_state(Some(&b), Some((&[], LinkState::Open, true))),
            TileState::Ended("a")
        );
        assert_eq!(
            tile_state(Some(&b), Some((&live, LinkState::Lost, true))),
            TileState::Lost("a")
        );
        assert_eq!(
            tile_state(Some(&b), Some((&[], LinkState::Connecting, false))),
            TileState::Connecting("a")
        );
        assert_eq!(
            tile_state(None, Some((&live, LinkState::Open, true))),
            TileState::Empty
        );
        let other = Binding {
            key: key("a"),
            pane: Some(9),
        };
        assert_eq!(
            tile_state(Some(&other), Some((&live, LinkState::Open, true))),
            TileState::Live {
                session: &live[0],
                pane: 4
            }
        );
        assert_eq!(
            tile_state(Some(&b), None),
            TileState::Lost("a"),
            "another device's session waits for that device; it hasn't ended"
        );
    }

    #[test]
    fn the_app_opens_with_the_windows_that_hold_a_session() {
        let mut ws = Workspace::default();
        ws.set_windows(0, 4);
        ws.bind(1, key("a"), None);
        ws.bind(3, key("b"), None);
        ws.fold_empty();
        assert_eq!(ws.windows().len(), 2);
        assert_eq!(ws.shown(0).unwrap().key, key("a"));
        assert_eq!(ws.shown(1).unwrap().key, key("b"));
        assert_eq!(ws.focus, 1, "focus stays on the window it was on");

        let mut bare = Workspace::default();
        bare.set_windows(0, 3);
        bare.fold_empty();
        assert_eq!(bare.windows().len(), 1, "one empty window, not three");
        assert_eq!(bare.tabs[0].windows.len(), MAX_WINDOWS);
    }

    #[test]
    fn a_stored_workspace_is_mended() {
        let v = serde_json::json!({"tabs":[{"windows":[]},{"name":"ops","windows":[{"node":"n","user":"","name":"a"},null,null,null,null]}],"active":7,"focus":9});
        let ws = Workspace::parse(v).unwrap();
        assert_eq!(ws.shown_tabs().len(), 2);
        assert_eq!(ws.tabs.len(), MAX_TABS);
        assert_eq!(ws.tabs[0].windows.len(), MAX_WINDOWS);
        assert_eq!(ws.tabs[0].count, 1);
        assert_eq!(ws.tabs[1].count, 5, "every window it kept");
        assert!(ws.tabs[1].layout.holds_exactly(5));
        assert_eq!((ws.active, ws.focus), (1, 4));
        assert_eq!(tab_label(&ws.tabs[1], 1), "ops");
        assert_eq!(ws.shown(0).unwrap().key, key("a"));
        assert!(Workspace::parse(serde_json::json!({"tabs":[]})).is_none());

        let mut ws = Workspace::default();
        ws.add_window();
        ws.bind(0, key("a"), None);
        ws.bind(0, key("b"), None);
        ws.show_tag(0, &key("a"));
        let back = Workspace::parse(serde_json::to_value(&ws).unwrap()).unwrap();
        assert_eq!(back, ws);

        let twice = serde_json::json!({"tabs":[{"windows":[
            {"sessions":[{"node":"n","name":"a"},{"node":"n","name":"b"}],"active":5},
            {"sessions":[{"node":"n","name":"a"}]}
        ],"count":2}],"active":0,"focus":0});
        let ws = Workspace::parse(twice).unwrap();
        assert_eq!(tags(&ws.windows()[0]), ["a", "b"]);
        assert_eq!(ws.windows()[0].active, 1);
        assert!(
            ws.windows()[1].is_empty(),
            "a session is bound in one window"
        );
    }
}
