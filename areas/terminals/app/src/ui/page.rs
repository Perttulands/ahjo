//! The page itself: its stores, the nodes' sessions and screens, and the
//! one place that decides what a frame, an act or a chord does on it. What
//! opens over it (Send, Overview, the scrollback search, Peek, the rename
//! fields) is a layer on the app's stack, drawn by [`super::surface`].

use super::launcher::Launcher;
use super::surface::{self, Naming, Surface};
use crate::remote::Notice;
use crate::remotes::{PaneAt, Remotes};
use crate::store::layout::Side;
use crate::store::sessions::{self, Key};
use crate::store::settings::{Kept, Settings, launched_in};
use crate::store::workspace::{Arrange, LinkState, MAX_WINDOWS, TileState};
use crate::term::screen::Screen;
use ahjo_app_kit::Host;
use ahjo_app_kit::chords::Chord;
use ahjo_app_kit::host::Status;
use ahjo_app_kit::menu::MenuRow;
use ahjo_app_kit::popup::{self, Popup, Spot};
use ahjo_app_kit::settings::Place;
use ahjo_app_kit::widgets::field::{self, FieldEvent, TextField};
use ahjo_app_kit::{Answers, Ask, Page, Setting};
use ahjo_core::NodeId;
use ahjo_core::frame::Kinds;
use ahjo_terminals_contract::{Launch, SessionList, decode_down};
use gpui::{
    AnyElement, App, AppContext, Bounds, Context, Entity, FocusHandle, Focusable, IntoElement,
    MouseDownEvent, Pixels, Point, Render, ScrollHandle, Task, WeakEntity, Window,
};
use parking_lot::Mutex;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::time::SystemTime;

/// What a menu row does.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    RenameTab(usize),
    /// Tab, and how many windows it shows.
    Windows(usize, usize),
    NewTab,
    CloseTab(usize),
    ClearTab(usize),
    /// Keeps the active tab's arrangement on a number key, asking its
    /// name.
    SaveView(u8),
    /// Lays the active tab out as the view on a number key keeps it.
    RecallView(u8),
    RenameView(u8),
    DeleteView(u8),
    /// The active tab's windows laid out afresh, or their sizes evened.
    Arrange(Arrange),
    EvenOut,
    Peek(Key),
    /// A terminal's own text size goes: a window's, or Peek's.
    ResetZoom(usize),
    /// A new empty window beside window `n`, on a side.
    Split(usize, Side),
    /// Closes a window of the shown tab; its sessions run on.
    CloseWindow(usize),
    /// Two windows of the shown tab trade places, with what they hold.
    Swap(usize, usize),
    /// Puts the session's folder on the clipboard.
    CopyPath(Key),
    /// Takes ended sessions' tags out of every window.
    ClearEnded(Vec<Key>),
    /// Picks these sessions in the rail, and only them.
    PickKeys(Vec<Key>),
    KillKeys(Vec<Key>),
    /// The offer to tidy up waits until there is more.
    TidyLater,
    Send(Key),
    Attach(Key, usize),
    Unassign(Key),
    Rename(Key, super::surface::NameAt),
    /// The same harness again, in the session's folder (CHROTE's Copy).
    Copy(Key),
    Kill(Key),
    /// What shapes the agent started in the session's folder, on the
    /// Harness page.
    Sees(Key),
    /// Kills the rail's picked rows that it shows.
    KillPicked,
    /// Takes a session's tag out of a window of the shown tab.
    Remove(usize, Key),
    /// Puts a session's tag on screen in its window.
    ShowTag(usize, Key),
    Pane(usize, u32),
    WindowList(usize, Key),
    WindowPane(usize, Key, u32),
    History(usize),
    /// Makes an ended session again, under its name, in its windows.
    Restart(Key),
}

/// A row of one of the page's menus, as its tests read it: what it says
/// and the [`Act`] it does. The app's menu gets it as a [`MenuRow`].
#[derive(Clone, Debug)]
pub struct MenuItem {
    pub label: String,
    pub chord: Option<String>,
    pub act: Option<Act>,
    /// Destroys something: the first click arms it, the second runs it.
    pub danger: bool,
    pub checked: bool,
    /// Offered and refused; its hint says why.
    pub refused: bool,
    /// A line under the label: what the row does, or why it is refused.
    pub hint: Option<String>,
    /// The rows it opens in its place.
    pub sub: Vec<MenuItem>,
}

impl MenuItem {
    pub fn new(label: impl Into<String>, act: Act) -> Self {
        Self {
            label: label.into(),
            chord: None,
            act: Some(act),
            danger: false,
            checked: false,
            refused: false,
            hint: None,
            sub: vec![],
        }
    }
    /// A row that opens `rows` in the menu's place; `state` reads beside it.
    pub fn sub(label: impl Into<String>, state: Option<String>, rows: Vec<MenuItem>) -> Self {
        let mut item = Self::note(label);
        item.chord = Some(match state {
            Some(s) => format!("{s} ›"),
            None => "›".into(),
        });
        item.sub = rows;
        item
    }
    /// A row that does nothing now, saying why.
    pub fn refused(mut self, why: impl Into<String>) -> Self {
        self.refused = true;
        self.hint = Some(why.into());
        self
    }
    pub fn hint(mut self, text: impl Into<String>) -> Self {
        self.hint = Some(text.into());
        self
    }
    /// A row that only says something.
    pub fn note(label: impl Into<String>) -> Self {
        let mut item = Self::rule();
        item.label = label.into();
        item
    }
    pub fn chord(mut self, c: &str) -> Self {
        self.chord = Some(c.into());
        self
    }
    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }
    pub fn checked(mut self, on: bool) -> Self {
        self.checked = on;
        self
    }
    pub fn rule() -> Self {
        Self {
            label: String::new(),
            chord: None,
            act: None,
            danger: false,
            checked: false,
            refused: false,
            hint: None,
            sub: vec![],
        }
    }

    /// The row for the app's menu: a click does its act on `page`.
    pub(super) fn row(self, page: &WeakEntity<Terminals>) -> MenuRow {
        let mut row = match self.act {
            Some(act) => {
                let page = page.clone();
                MenuRow::new(self.label, move |window, cx| {
                    let _ = page.update(cx, |p, cx| p.act(act.clone(), window, cx));
                })
            }
            None => MenuRow::note(self.label),
        };
        row.chord = self.chord;
        row.danger = self.danger;
        row.checked = self.checked;
        row.refused = self.refused;
        row.hint = self.hint;
        row.sub = self.sub.into_iter().map(|i| i.row(page)).collect();
        row
    }
}

/// A window reading back: on the pane's live screen, with its lines grown
/// above it, or on the alternate screen a read-only copy of them.
pub struct History {
    pub pane: PaneAt,
    pub screen: Arc<Mutex<Screen>>,
    /// Where the view starts, counted from the oldest line: it stays as
    /// output arrives below (`Screen::view`).
    pub top: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HistoryTarget {
    /// A window, or Peek at [`PEEK_SLOT`].
    Window { index: usize, line: Option<usize> },
}

/// Peek: one session over the page, used as in a window (typed into,
/// pasted, scrolled back, selected, clicked), and never sized: it shows the
/// grid the session has, at its own text size.
pub struct Peek {
    pub key: Key,
    pub pane: Option<u32>,
}

/// Peek's place among the windows' slots (their geometry, history, links):
/// it is a terminal as they are, after the last of them.
pub const PEEK_SLOT: usize = crate::store::workspace::MAX_WINDOWS;
/// The windows' slots and Peek's.
pub const SLOTS: usize = PEEK_SLOT + 1;

/// A terminal grid: columns, rows.
pub type Grid = (u16, u16);

/// Where each window's terminal was last painted, for the mouse.
#[derive(Clone, Copy, Default)]
pub struct Geo {
    pub body: Option<Bounds<Pixels>>,
    pub origin: Option<Point<Pixels>>,
    pub cell: Option<gpui::Size<Pixels>>,
    pub rows: usize,
    pub cols: usize,
    /// The text size it was drawn at: below its own when its grid, set
    /// elsewhere, only fits smaller.
    pub font: Option<f32>,
    pub display_offset: usize,
    /// Below a history: where the live input is docked.
    pub dock: Option<Bounds<Pixels>>,
    /// Reading back on the live screen: where its row 0 would lie, so a
    /// selection reaches the docked rows.
    pub dock_origin: Option<Point<Pixels>>,
    /// Peek zoomed past its room: the columns its view is moved right by,
    /// and the most it can be.
    pub pan: usize,
    pub pan_room: usize,
    /// Where the cursor was when the view last followed it: a pan by the
    /// wheel holds until the cursor moves.
    pub followed: Option<(usize, i32)>,
}

/// A launch awaiting its node's answer, including the status it started with.
pub struct PendingLaunch {
    launch: Launch,
    window: Option<usize>,
    tab: usize,
    status_revision: u64,
    /// A launcher's launch: its folder joins the recent ones once it
    /// started, so a folder that failed is not offered again.
    pub remember_folder: bool,
}

/// How many launched-in folders a node keeps.
const LAUNCHED_KEPT: usize = 5;

pub struct Terminals {
    pub host: Host,
    pub me: WeakEntity<Self>,
    pub focus: FocusHandle,
    pub term_focus: FocusHandle,
    /// Peek needs its own focus while another surface is underneath it.
    pub peek_focus: FocusHandle,
    pub fleet: Remotes,
    pub settings: Settings,
    /// The fleet as the app last saw it, by node.
    pub places: BTreeMap<String, Place>,
    /// The page is the one shown.
    pub shown: bool,
    /// The diagram last shown in the window's panel, for its chip to take
    /// away again.
    pub diagram: Option<gpui::WeakEntity<super::diagram::DiagramView>>,
    /// What the name field names, while it is open.
    pub naming: Option<Naming>,
    /// Status lines said since the page last handed them to the app.
    said: Vec<Status>,
    pub geo: Rc<RefCell<[Geo; SLOTS]>>,
    /// The small pictures each window last drew, for the pointer.
    pub chips: Rc<RefCell<[Vec<super::tiles::pictures::Chip>; SLOTS]>>,
    /// Pictures the sessions named, read from their devices.
    pub pictures: crate::store::pictures::Pictures<std::sync::Arc<gpui::RenderImage>>,
    /// The grid each window's body would hold at the terminal font,
    /// as last painted: what a launch into it asks the node for (#161).
    pub launch_grids: Rc<RefCell<[Option<Grid>; MAX_WINDOWS]>>,
    /// The window a drag selection started in.
    pub selecting: Option<usize>,
    /// Lines a selection drag held outside its grid scrolls each tick, and
    /// the ticks while it does.
    pub drag_scroll: i32,
    /// The part of a line the wheel has moved window `.0` and not yet
    /// scrolled: a touchpad sends fractions.
    pub scroll_rest: (usize, f32),
    pub drag_scroll_task: Option<Task<()>>,
    pub mouse_down_in: Option<(usize, u8)>,
    /// The cell (window, column, row on screen) the pointer rests on, when
    /// a link is drawn there.
    pub link_hover: Option<(usize, usize, usize)>,
    /// What the input method composes for the focused pane, not yet sent.
    pub composing: Option<String>,
    pub histories: HashMap<usize, History>,
    /// Live screens with lines grown above them, dropped when no window
    /// reads them back any more (`tidy_histories`).
    pub grown: Vec<Arc<Mutex<Screen>>>,
    /// Grids waiting to be sent for panes this app sizes, by pane: the grid
    /// and the wait's number, so only the last wait sends.
    pub resizing: HashMap<PaneAt, ((u16, u16), u64)>,
    pub resize_seq: u64,
    /// History requests in flight: request id → window, or Peek.
    pub history_wanted: HashMap<u32, HistoryTarget>,
    pub peek: Option<Peek>,
    /// The session Peek showed when a mouse-down outside closed it, and
    /// where: a click on that session's row, from that down, leaves Peek
    /// closed instead of opening it again.
    pub peek_left: Option<(Key, Point<Pixels>)>,
    pub launchers: Vec<Entity<Launcher>>,
    pub rail_search: Entity<TextField>,
    pub send_note: Entity<TextField>,
    /// Send's list of sessions to send to.
    pub send_finder: Entity<ahjo_app_kit::widgets::finder::Finder>,
    /// The sessions Send sends to, in the order they were chosen.
    pub send_targets: Vec<Key>,
    pub send: super::send::Draft,
    /// Send's area: a click in it that gives the keyboard to nothing
    /// there goes on with the draft.
    pub send_area: gpui::FocusHandle,
    pub find_query: Entity<TextField>,
    pub find_cursor: usize,
    /// The hit last opened, which Enter steps on from.
    pub find_shown: Option<usize>,
    pub rename_field: Entity<TextField>,
    pub overview_cursor: usize,
    /// Number of overview cards fitting in the last painted row.
    pub overview_columns: Rc<Cell<usize>>,
    /// The room Overview's cards were last laid out in, which sizes them.
    pub overview_room: Rc<Cell<gpui::Size<Pixels>>>,
    /// Overview's sessions in the rail's order, as last drawn: what the
    /// arrow keys and Enter count in.
    pub overview_order: Vec<Key>,
    pub overview_scroll: ScrollHandle,
    pub find_scroll: ScrollHandle,
    /// The Sessions tabs' strip, and the active tab, tab count and window
    /// width it last brought the active tab into view at.
    pub tab_scroll: ScrollHandle,
    pub tab_scrolled: Option<(usize, usize, i32)>,
    pub hover_row: Option<String>,
    /// Launches asked of a node, by node and request: the recipe, the window
    /// it goes to and that window's tab.
    pub launches: HashMap<(String, u32), PendingLaunch>,
    /// Changes whenever this page publishes a status: older async successes
    /// cannot replace a later status the operator has already been shown.
    pub status_revision: u64,
    /// Kills asked of nodes, answered together (`store::selection`).
    pub kills: crate::store::selection::Kills,
    /// Rail rows picked to kill together; on this device only, never kept.
    pub pick: crate::store::selection::Pick,
    /// The rail's rows as last drawn, in order: what a Shift+click range
    /// and Kill N count from.
    pub rail_shown: Vec<Key>,
    /// The rail takes focus on a pick, to hold Delete and Escape.
    pub rail_focus: FocusHandle,
    /// Mouse changes asked of nodes, by node and request: the device's name.
    pub mice: HashMap<(String, u32), String>,
    /// Renames asked of a node, by node and request: the session's key as
    /// it was, and its new name.
    pub renames: HashMap<(String, u32), (Key, String)>,
    /// Each window of the shown tab as last laid out.
    pub tile_bounds: Rc<RefCell<[Option<Bounds<Pixels>>; MAX_WINDOWS]>>,
    /// The window a drag is over, and where in it.
    pub drop_at: Option<(usize, super::tiles::Zone)>,
    /// What is being dragged over the windows.
    pub in_air: Option<super::tiles::Dragged>,
    /// A divider between windows being dragged.
    pub divider: Option<super::tiles::DividerDrag>,
    /// Each group of the layout as last laid out, by its path, which a
    /// divider's drag is a share of.
    pub group_bounds: Rc<RefCell<HashMap<Vec<usize>, Bounds<Pixels>>>>,
    /// The rail's list, whose scrollbar shows where it is.
    pub rail_scroll: gpui::ScrollHandle,
    /// Where the last menu a row was picked from stood.
    pub menu_at: Point<Pixels>,
    /// Ended sessions being made again, until their node answers.
    pub restarting: HashSet<Key>,
    /// The wheel with Ctrl held, between its steps.
    pub zoom_wheel: ahjo_app_kit::zoom::Wheel,
    /// The agent event each session was last told seen for, by key text.
    pub seen_sent: HashMap<String, u64>,
    /// The session whose report the status line says, and when it said it.
    pub told: Option<(Key, std::time::SystemTime)>,
    /// The operator pressed a key or a button on this page, or showed a
    /// session, since it last drew: he is looking at it.
    pub acted: bool,
    /// Wakes the page when the network thread fed a screen.
    _woken: Task<()>,
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl Terminals {
    pub fn new(host: Host, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut said = vec![];
        let settings = match Settings::load(&host, cx) {
            Ok(mut s) => {
                s.workspace.fold_empty();
                s
            }
            Err(e) => {
                said.push(Status::failure(format!(
                    "The Sessions settings did not read ({}): {}",
                    e.code, e.message
                )));
                Settings::default()
            }
        };
        let field = |cx: &mut Context<Self>, p: &str| cx.new(|cx| TextField::new(cx, p));
        let rail_search = field(cx, "Sessions…");
        let send_note = cx.new(|cx| {
            TextField::new(cx, "Message to send")
                .multiline()
                .attachments()
        });
        let send_finder = cx.new(|cx| {
            ahjo_app_kit::widgets::finder::Finder::new("send-target", "Find sessions…", cx)
                .several()
                .rows(crate::theme::size::SEND_TARGET_ROWS as usize)
        });
        let find_query = field(cx, "Search every session's output…");
        let rename_field = field(cx, "Name");
        let weak = cx.weak_entity();
        // One per window, and the Send drawer's last.
        let launchers = (0..=super::launcher::SEND)
            .map(|i| cx.new(|cx| Launcher::new(i, weak.clone(), host.clone(), window, cx)))
            .collect();
        let fleet = Remotes::default();
        let (wake, woken) = async_channel::bounded(1);
        host.tap(fleet.tap(wake), cx);
        let woken = cx.spawn(async move |this, cx| {
            while woken.recv().await.is_ok() {
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        });
        // What the page asks of the nodes, and says, leaves as it redraws.
        cx.observe_self(|p, cx| p.flush(cx)).detach();
        let places = host.places(cx);
        let mut s = Self {
            host,
            me: weak,
            focus: cx.focus_handle(),
            term_focus: cx.focus_handle(),
            peek_focus: cx.focus_handle(),
            fleet,
            settings,
            places,
            shown: false,
            diagram: None,
            send_area: cx.focus_handle(),
            naming: None,
            said,
            geo: Rc::new(RefCell::new([Geo::default(); SLOTS])),
            chips: Rc::default(),
            pictures: Default::default(),
            launch_grids: Rc::default(),
            selecting: None,
            drag_scroll: 0,
            scroll_rest: (0, 0.),
            drag_scroll_task: None,
            mouse_down_in: None,
            link_hover: None,
            composing: None,
            histories: HashMap::new(),
            grown: Vec::new(),
            resizing: HashMap::new(),
            resize_seq: 0,
            history_wanted: HashMap::new(),
            peek: None,
            peek_left: None,
            launchers,
            rail_search,
            send_note,
            send_finder,
            send_targets: vec![],
            send: Default::default(),
            find_query,
            find_cursor: 0,
            find_shown: None,
            rename_field,
            overview_cursor: 0,
            overview_columns: Rc::new(Cell::new(1)),
            overview_room: Default::default(),
            overview_order: Vec::new(),
            overview_scroll: ScrollHandle::new(),
            find_scroll: ScrollHandle::new(),
            tab_scroll: ScrollHandle::new(),
            tab_scrolled: None,
            hover_row: None,
            launches: HashMap::new(),
            status_revision: 0,
            kills: Default::default(),
            pick: Default::default(),
            rail_shown: vec![],
            rail_focus: cx.focus_handle(),
            mice: HashMap::new(),
            renames: HashMap::new(),
            tile_bounds: Rc::default(),
            drop_at: None,
            in_air: None,
            divider: None,
            group_bounds: Default::default(),
            rail_scroll: Default::default(),
            menu_at: Point::default(),
            restarting: HashSet::new(),
            zoom_wheel: Default::default(),
            seen_sent: HashMap::new(),
            told: None,
            acted: false,
            _woken: woken,
        };
        s.subscribe_fields(window, cx);
        // Nodes the app already reaches were opened before the page was made.
        for n in s.host.nodes(cx) {
            if let Some(state) = s.host.link(&n.id, cx) {
                s.link_changed(&n.id, Some(state), window, cx);
            }
        }
        // The rail is this page's section of the side panel, on every page.
        let page = cx.entity();
        let rail = cx.new(|cx| super::rail::Rail::new(&page, cx));
        s.host
            .lend_side(rail.into(), ahjo_app_kit::hub::SideOn::EveryPage, cx);
        s
    }

    fn subscribe_fields(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.subscribe_in(
            &self.rail_search.clone(),
            window,
            |s, _, ev, window, cx| match ev {
                FieldEvent::Changed => cx.notify(),
                FieldEvent::Submit => {
                    // Enter on a search peeks at the first session found.
                    let q = s.rail_search.read(cx).text.clone();
                    if let Some(k) = sessions::search(&s.fleet.all, &q)
                        .first()
                        .map(|x| Key::of(x))
                    {
                        // The words found the session; the list comes back
                        // whole for the next look.
                        s.clear_rail_search(cx);
                        s.open_peek(k, window, cx);
                    }
                }
                _ => {}
            },
        )
        .detach();
        cx.subscribe_in(
            &self.send_note.clone(),
            window,
            |s, _, ev, window, cx| match ev {
                FieldEvent::Submit => s.send_now(true, window, cx),
                FieldEvent::SubmitAlt => s.send_now(false, window, cx),
                // Tab goes to the sessions it is sent to, and back.
                FieldEvent::Tab => ahjo_app_kit::widgets::finder::focus(&s.send_finder, window, cx),
                FieldEvent::Changed => cx.notify(),
                _ => {}
            },
        )
        .detach();
        cx.subscribe_in(
            &self.send_note.clone(),
            window,
            |s, _, ev: &field::AttachmentEvent, _, cx| {
                s.send_clipboard(ev.0.clone(), cx);
            },
        )
        .detach();
        cx.subscribe_in(
            &self.send_finder.clone(),
            window,
            |s, _, ev: &ahjo_app_kit::widgets::finder::FinderEvent, window, cx| {
                s.send_finder_event(ev, window, cx);
            },
        )
        .detach();
        cx.subscribe_in(&self.find_query.clone(), window, |s, _, ev, window, cx| {
            let n = s.fleet.found().map_or(0, |(m, _)| m.len());
            match ev {
                FieldEvent::Changed => {
                    let q = s.find_query.read(cx).text.clone();
                    s.find_cursor = 0;
                    s.find_shown = None;
                    s.fleet.search(&q);
                }
                FieldEvent::Up => s.find_cursor = s.find_cursor.saturating_sub(1),
                FieldEvent::Down => s.find_cursor = (s.find_cursor + 1).min(n.saturating_sub(1)),
                FieldEvent::Submit => s.step_match(true, window, cx),
                _ => {}
            }
            cx.notify();
        })
        .detach();
        cx.subscribe_in(&self.rename_field.clone(), window, |s, _, ev, _, cx| {
            if !matches!(ev, FieldEvent::Submit) {
                return;
            }
            let name = s.rename_field.read(cx).text.clone();
            match s.naming.clone() {
                Some(Naming::Tab(_)) => s.keep_tab_name(cx),
                Some(Naming::Session(k, _)) => {
                    s.naming = None;
                    s.close(surface::RENAME_SESSION, cx);
                    s.rename_session(k, name.trim());
                }
                Some(Naming::View { popup, key, rename }) => {
                    s.naming = None;
                    s.host.close_popup(popup, cx);
                    if rename {
                        s.settings.workspace.rename_view(key, &name);
                        s.save(cx);
                        cx.notify();
                    } else {
                        s.keep_view(key, Some(&name), cx);
                    }
                }
                _ => {}
            }
        })
        .detach();
        // A tab's name is kept when its field loses focus, as CHROTE's input
        // commits on blur; Escape closes it first, keeping the old name.
        let rename_focus = self.rename_field.read(cx).focus_handle(cx);
        // A session's name is kept only by Enter: a click elsewhere leaves
        // it as it was.
        cx.on_blur(&rename_focus, window, |s, _, cx| match s.naming {
            Some(Naming::Tab(_)) => s.keep_tab_name(cx),
            Some(Naming::Session(..)) => s.close(surface::RENAME_SESSION, cx),
            _ => {}
        })
        .detach();
    }

    // ---- the nodes ----

    /// Hands the app what the page asked of the nodes and said since.
    pub fn flush(&mut self, cx: &mut App) {
        for (node, frames) in self.fleet.outbox() {
            let node = NodeId(node);
            for frame in frames {
                self.host.send(&node, frame, cx);
            }
        }
        for status in std::mem::take(&mut self.said) {
            self.host.report(status, cx);
        }
    }

    /// The link to `node` changed; `None` when the app no longer follows it.
    pub fn link_changed(
        &mut self,
        node: &NodeId,
        state: Option<LinkState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let first = self.host.nodes(cx).first().map(|n| n.id.0.clone());
        if state == Some(LinkState::Open) {
            // A look told while the link was down may not have arrived.
            self.seen_sent.clear();
        }
        self.places = self.host.places(cx);
        for n in self.fleet.link(node.as_str(), state, first.as_deref()) {
            self.notice(node.as_str(), n, window, cx);
        }
        self.after_news(cx);
    }

    /// A frame of the terminals' kinds from `node`.
    pub fn frame_from(
        &mut self,
        node: &NodeId,
        kind: u8,
        body: &[u8],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(down) = decode_down(kind, body) else {
            log::warn!("a terminals frame of kind {kind:#04x} from {node} did not read");
            return;
        };
        for n in self.fleet.frame(node.as_str(), down) {
            self.notice(node.as_str(), n, window, cx);
        }
        self.after_news(cx);
    }

    /// What follows anything a node said: its sessions remembered, a
    /// launched session's message delivered, and the panes watched.
    fn after_news(&mut self, cx: &mut Context<Self>) {
        self.remember_sessions(cx);
        self.send_waited(cx);
        self.sync(cx);
        cx.notify();
    }

    /// Tells other pages the sessions as now heard ([`SessionList`]).
    pub(super) fn publish_sessions(&mut self, cx: &mut Context<Self>) {
        let list = SessionList {
            sessions: self.fleet.all.clone(),
            focused: self
                .focused_key()
                .map(|k| ahjo_terminals_contract::FocusedSession {
                    node: k.node,
                    user: k.user,
                    name: k.name,
                }),
        };
        if let Err(e) = self.host.publish(list, cx) {
            self.report(&e.message, true, None);
        }
    }

    /// Where a node is, as it said last.
    pub fn where_is(&self, node: &str) -> Option<Place> {
        self.places.get(node).cloned()
    }

    /// When a node that does not answer now was last heard, as a clock
    /// time; `None` while it answers or if it never went.
    pub fn away_since(&self, node: &str) -> Option<String> {
        if self
            .fleet
            .get(node)
            .is_some_and(|c| c.state == LinkState::Open)
        {
            return None;
        }
        self.places.get(node)?.seen.map(ahjo_app_kit::clock::hhmm)
    }

    /// Keeps each node's session names, so an away device keeps its rows
    /// and windows, named, until it answers again; nodes the app no longer
    /// knows go.
    fn remember_sessions(&mut self, cx: &mut Context<Self>) {
        let kept = &mut self.settings.kept;
        let before = kept.clone();
        for c in self.fleet.clients().iter().filter(|c| c.known) {
            let list = c
                .sessions
                .iter()
                .map(|s| Kept {
                    name: s.name.clone(),
                    user: s.user.clone().unwrap_or_default(),
                })
                .collect();
            kept.insert(c.node.clone(), list);
        }
        if !self.places.is_empty() {
            kept.retain(|n, _| self.places.contains_key(n));
        }
        let changed = *kept != before;
        let before_restarts = self.settings.restarts.clone();
        for session in &self.fleet.all {
            let key = Key::of(session).text();
            let record = self.settings.restarts.entry(key).or_default();
            if record.id != session.id {
                *record = crate::store::settings::Restart {
                    id: session.id.clone(),
                    ..Default::default()
                };
            }
            if record.harness.is_none()
                && let Some(cwd) = &session.cwd
                && !cwd.is_empty()
            {
                record.folder.clone_from(cwd);
            }
        }
        if changed || self.settings.restarts != before_restarts {
            self.save(cx);
        }
    }

    fn notice(&mut self, node: &str, n: Notice, window: &mut Window, cx: &mut Context<Self>) {
        match n {
            Notice::Reported(list) => self.reported(list, window, cx),
            Notice::Waiting(names) => {
                let text = match names.as_slice() {
                    [one] => format!("{one} is waiting"),
                    many => format!("{} sessions are waiting", many.len()),
                };
                self.report(&text, false, None);
            }
            Notice::Launched { id, ok, text } => {
                self.send_launched(node, id, ok, &text, cx);
                let Some(PendingLaunch {
                    launch,
                    window: window_index,
                    tab,
                    status_revision,
                    remember_folder,
                }) = self.launches.remove(&(node.to_string(), id))
                else {
                    return;
                };
                let name = launch.name.clone();
                // A restart answered: its windows were bound all along.
                self.restarting
                    .retain(|k| !(k.node == node && k.name == name));
                if ok {
                    if remember_folder {
                        let launched = self
                            .settings
                            .launched_folders
                            .entry(launched_in(node, &launch.user))
                            .or_default();
                        sessions::remember(launched, &launch.folder, LAUNCHED_KEPT);
                    }
                    let key = Key {
                        node: node.to_string(),
                        user: launch.user.clone(),
                        name: name.clone(),
                    };
                    self.settings.restarts.insert(
                        key.text(),
                        crate::store::settings::Restart {
                            id: text.split('\0').next().unwrap_or_default().to_string(),
                            folder: launch.folder.clone(),
                            harness: Some(launch.harness.clone()),
                            flags: launch.flags.clone(),
                            install_completion_hooks: launch.install_completion_hooks,
                        },
                    );
                    self.save(cx);
                    if let Some(w) = window_index {
                        let node = node.to_string();
                        let user = self
                            .launchers
                            .get(w)
                            .map(|l| l.read(cx).user.clone())
                            .unwrap_or_default();
                        let key = Key {
                            node,
                            user,
                            name: name.clone(),
                        };
                        self.settings.workspace.bind_in(tab, w, key, None);
                        self.save(cx);
                    }
                    // After a NUL, what did not go as asked: no completion
                    // notices, or a harness that did not start.
                    match text.split_once('\0') {
                        Some((_, warning)) => {
                            self.report(&format!("Launched {name}, but {warning}"), true, None)
                        }
                        None if self.status_revision == status_revision => {
                            self.report(&format!("Launched {name}"), false, None)
                        }
                        None => {}
                    }
                    window.focus(&self.term_focus, cx);
                } else {
                    let why = if text.is_empty() {
                        "the node refused".to_string()
                    } else {
                        text
                    };
                    self.report(&format!("Could not launch {name}: {why}"), true, None);
                }
                if let Some(w) = window_index
                    && let Some(l) = self.launchers.get(w)
                {
                    l.update(cx, |l, cx| l.done(cx));
                }
            }
            Notice::Deleted { id, ok, text } => self.killed(node, id, ok, &text),
            Notice::Moused { id, ok, text } => self.moused(node, id, ok, &text),
            Notice::Renamed { id, ok, text } => {
                let Some((old, name)) = self.renames.remove(&(node.to_string(), id)) else {
                    return;
                };
                if ok {
                    self.settings.workspace.rename_session(&old, &name);
                    let new = Key {
                        name: name.clone(),
                        ..old.clone()
                    };
                    if let Some(record) = self.settings.restarts.remove(&old.text()) {
                        self.settings.restarts.insert(new.text(), record);
                    }
                    for k in self
                        .peek
                        .as_mut()
                        .map(|p| &mut p.key)
                        .into_iter()
                        .chain(self.send_targets.iter_mut())
                    {
                        if *k == old {
                            *k = new.clone();
                        }
                    }
                    self.save(cx);
                    self.report(&format!("Renamed {} to {name}", old.name), false, None);
                } else {
                    self.report(
                        &format!("Could not rename {}: {text}", old.name),
                        true,
                        None,
                    );
                }
            }
            Notice::Delivered { id, outcome } => self.send_delivered(id, outcome, cx),
            Notice::History { id, pane, text } => {
                let at = PaneAt {
                    node: node.to_string(),
                    pane,
                };
                self.history_arrived(id, at, text, cx)
            }
            Notice::ClaimRefused(why) => {
                self.report(&format!("tmux kept its size: {why}"), true, None)
            }
        }
    }

    /// Subscriptions follow the bindings (every tab, so a tab switch shows
    /// a live screen at once) and Peek.
    pub fn sync(&mut self, cx: &mut Context<Self>) {
        let bindings = self
            .settings
            .workspace
            .tabs
            .iter()
            .flat_map(|tab| &tab.windows)
            .flat_map(|window| &window.sessions)
            .map(|binding| binding.key.clone())
            .collect();
        self.fleet.retain_ended_screens(&bindings);
        let mut panes: HashMap<String, HashSet<u32>> = HashMap::new();
        let ws = &self.settings.workspace;
        for b in ws.on_screen() {
            if let Some(s) = self.fleet.session(&b.key)
                && let Some(p) = s
                    .panes
                    .iter()
                    .find(|p| Some(p.id) == b.pane)
                    .or(s.panes.first())
            {
                panes.entry(b.key.node.clone()).or_default().insert(p.id);
            }
        }
        if let Some(p) = &mut self.peek
            && let Some(s) = sessions::find(&self.fleet.all, &p.key)
        {
            let pane = p
                .pane
                .filter(|id| s.panes.iter().any(|x| x.id == *id))
                .or(s.panes.first().map(|x| x.id));
            p.pane = pane;
            if let Some(id) = pane {
                panes.entry(p.key.node.clone()).or_default().insert(id);
            }
        }
        // Overview draws every session as it is, colours and all: their
        // screens stream while it is open, sizing nothing, as Peek's does.
        if self.host.is_open(surface::OVERVIEW, cx) {
            for s in &self.fleet.all {
                if let Some(p) = s.panes.first() {
                    panes.entry(s.node.clone()).or_default().insert(p.id);
                }
            }
        }
        // A window whose session went stops sizing it.
        self.fleet.watch(&panes);
        self.publish_sessions(cx);
    }

    // ---- reporting ----

    /// Says something on the app's status line, with a button that does
    /// `action` on this page.
    pub fn report(&mut self, text: &str, failure: bool, action: Option<(String, Act)>) {
        self.say(text, failure, action);
    }

    /// Says `text` on the status line; returns when, to take it back by.
    pub fn say(
        &mut self,
        text: &str,
        failure: bool,
        action: Option<(String, Act)>,
    ) -> std::time::SystemTime {
        self.status_revision = self.status_revision.wrapping_add(1);
        let mut status = if failure {
            Status::failure(text)
        } else {
            Status::new(text)
        };
        if let Some((label, act)) = action {
            let page = self.me.clone();
            status = status.action(label, move |window, cx| {
                let _ = page.update(cx, |p, cx| p.act(act.clone(), window, cx));
            });
        }
        let at = status.at;
        self.said.push(status);
        at
    }

    pub(super) fn track_launch(
        &mut self,
        node: &str,
        id: u32,
        launch: Launch,
        window: Option<usize>,
        tab: usize,
    ) {
        self.launches.insert(
            (node.to_string(), id),
            PendingLaunch {
                launch,
                window,
                tab,
                status_revision: self.status_revision,
                remember_folder: false,
            },
        );
    }

    /// The launch `id` on `node` came from a launcher: see
    /// [`PendingLaunch::remember_folder`].
    pub(super) fn remember_folder_of(&mut self, node: &str, id: u32) {
        if let Some(p) = self.launches.get_mut(&(node.to_string(), id)) {
            p.remember_folder = true;
        }
    }

    pub fn toast(&mut self, text: &str, cx: &mut Context<Self>) {
        self.host.toast(text, cx);
    }

    /// Keeps the page's settings in the app's.
    pub fn save(&mut self, cx: &mut Context<Self>) {
        if let Err(e) = self.host.set_setting(&self.settings, cx) {
            self.report(
                &format!("The Sessions settings were not kept: {}", e.message),
                true,
                None,
            );
        }
    }

    // ---- surfaces ----

    /// This page's layer `name` is on the app's stack.
    pub fn is_open(&self, name: &str, cx: &App) -> bool {
        self.host.is_open(name, cx)
    }

    /// The name field now names `naming`; what it named before closes.
    fn name_in_field(&mut self, naming: Naming, cx: &mut Context<Self>) {
        match self.naming.replace(naming) {
            Some(Naming::Tab(_)) => self.host.close_layer(surface::RENAME_TAB, cx),
            Some(Naming::Session(..)) => self.host.close_layer(surface::RENAME_SESSION, cx),
            Some(Naming::View { popup, .. }) => self.host.close_popup(popup, cx),
            None => {}
        }
    }

    /// Opens `s` over the page, as a layer on the app's stack.
    pub fn open(&mut self, s: Surface, window: &mut Window, cx: &mut Context<Self>) {
        let name = s.name();
        let focus = match &s {
            Surface::Overview => self.focus.clone(),
            Surface::Peek => self.peek_focus.clone(),
            Surface::RenameTab(i) => {
                self.name_in_field(Naming::Tab(*i), cx);
                self.rename_field.focus_handle(cx)
            }
            Surface::RenameSession(key, at) => {
                self.name_in_field(Naming::Session(key.clone(), *at), cx);
                self.rename_field.focus_handle(cx)
            }
        };
        let view = surface::view(self.me.clone(), name, cx);
        let page = self.me.clone();
        let mut layer = ahjo_app_kit::host::Layer::new(name, view)
            .focus(focus.clone())
            .on_dismiss(move |_, cx| {
                let _ = page.update(cx, |p, cx| p.closed(name, cx));
            });
        // Peek is a terminal: Escape is its program's.
        if matches!(s, Surface::Peek) {
            layer = layer.keeps_escape();
        }
        self.host.layer(layer, cx);
        if matches!(s, Surface::Overview) {
            self.host
                .channel(ahjo_terminals_contract::OVERVIEW, true, cx);
            self.overview_cursor = 0;
            self.overview_scroll.scroll_to_item(1);
            self.sync(cx);
        }
        window.focus(&focus, cx);
        cx.notify();
    }

    /// Names the view kept on number `key` in a popup beside the menu it
    /// was asked for from: keeping the arrangement on screen there, or
    /// only renaming it.
    fn open_view_name(&mut self, key: u8, rename: bool, cx: &mut Context<Self>) {
        let view = popup::drawn(
            &cx.entity(),
            move |p: &mut Self, _, cx| p.render_view_name(key, rename, cx),
            cx,
        );
        let page = self.me.clone();
        let Some(id) = self.host.popup(
            Popup::beside(Spot::at(self.menu_at, cx), view)
                .focus(self.rename_field.focus_handle(cx))
                .on_dismiss(move |_, cx| {
                    let _ = page.update(cx, |p, cx| {
                        if matches!(p.naming, Some(Naming::View { key: k, .. }) if k == key) {
                            p.naming = None;
                            cx.notify();
                        }
                    });
                }),
            cx,
        ) else {
            return;
        };
        self.name_in_field(
            Naming::View {
                popup: id,
                key,
                rename,
            },
            cx,
        );
        cx.notify();
    }

    /// Closes the surface named `name`, and whatever opened above it. A tab
    /// rename open above it keeps what was typed, as on blur; only Esc,
    /// which closes the rename itself, drops it.
    pub fn close(&mut self, name: &'static str, cx: &mut Context<Self>) {
        if name != surface::RENAME_TAB
            && self.is_open(name, cx)
            && self.host.is_top(surface::RENAME_TAB, cx)
        {
            self.keep_tab_name(cx);
        }
        self.host.close_layer(name, cx);
    }

    /// Shows Send or the output search (`name`) in the window's panel,
    /// beside whatever page shows, its field taking the keys.
    pub fn show_in_panel(
        &mut self,
        name: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let focus = match name {
            surface::FIND => {
                self.find_cursor = 0;
                self.find_shown = None;
                self.find_query
                    .update(cx, |f, cx| f.set(&f.text.clone(), true, cx));
                self.find_query.focus_handle(cx)
            }
            _ if self.send_targets.is_empty() && !self.send.new_agent => {
                self.send_finder.read(cx).query.focus_handle(cx)
            }
            _ => self.send_note.focus_handle(cx),
        };
        let view = surface::view(self.me.clone(), name, cx);
        self.host
            .show_panel(ahjo_app_kit::panel::Panel::new(name, view), cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    /// Send or the search (`name`) shows in the window's panel.
    pub fn in_panel(&self, name: &str, cx: &gpui::App) -> bool {
        self.host.panel_shows(name, cx)
    }

    /// The app closed the surface named `name`.
    fn closed(&mut self, name: &str, cx: &mut Context<Self>) {
        match name {
            surface::OVERVIEW => {
                self.host
                    .channel(ahjo_terminals_contract::OVERVIEW, false, cx);
                self.fleet.overview_off();
                self.sync(cx);
            }
            surface::PEEK => {
                self.peek = None;
                self.leave_history(PEEK_SLOT, cx);
                if self.mouse_down_in.is_some_and(|(i, _)| i == PEEK_SLOT) {
                    self.mouse_down_in = None;
                }
                self.geo.borrow_mut()[PEEK_SLOT] = Geo::default();
                self.chips.borrow_mut()[PEEK_SLOT].clear();
                self.sync(cx);
            }
            surface::RENAME_TAB if matches!(self.naming, Some(Naming::Tab(_))) => {
                self.naming = None;
            }
            surface::RENAME_SESSION if matches!(self.naming, Some(Naming::Session(..))) => {
                self.naming = None;
            }
            _ => {}
        }
        cx.notify();
    }

    /// Opens the app's menu at `at` with these rows.
    pub fn menu(
        &mut self,
        items: Vec<MenuItem>,
        at: Point<Pixels>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.menu_at = at;
        let rows = items.into_iter().map(|i| i.row(&self.me)).collect();
        self.host.menu(rows, at, cx);
    }

    // ---- actions ----

    pub fn act(&mut self, act: Act, window: &mut Window, cx: &mut Context<Self>) {
        let ws = &mut self.settings.workspace;
        match act {
            Act::RenameTab(i) => {
                let name = ws
                    .tabs
                    .get(i)
                    .and_then(|t| t.name.clone())
                    .unwrap_or_default();
                self.rename_field.update(cx, |f, cx| f.set(&name, true, cx));
                self.open(Surface::RenameTab(i), window, cx);
                return;
            }
            Act::Windows(tab, n) => {
                ws.set_windows(tab, n);
                if tab == ws.active {
                    self.histories.retain(|w, _| *w < n);
                }
            }
            Act::NewTab => {
                if !ws.add_tab() {
                    self.report("Six Sessions tabs is the most.", false, None);
                }
                self.host.show(cx);
            }
            Act::CloseTab(i) => {
                ws.close_tab(i);
                self.histories.clear();
            }
            Act::ClearTab(i) => {
                if i == ws.active {
                    let shown: Vec<PaneAt> = self.window_panes().into_iter().flatten().collect();
                    for p in shown {
                        self.fleet.release(&p.node, p.pane);
                    }
                    self.histories.clear();
                }
                self.settings.workspace.clear(i);
            }
            Act::SaveView(key) | Act::RenameView(key) => {
                let rename = matches!(act, Act::RenameView(_));
                let name = match ws.view(key) {
                    Some(v) => v.name.clone(),
                    None => ws.view_name(),
                };
                self.rename_field.update(cx, |f, cx| f.set(&name, true, cx));
                self.open_view_name(key, rename, cx);
                return;
            }
            Act::RecallView(key) => {
                self.recall_view(key, cx);
                return;
            }
            Act::DeleteView(key) => {
                if let Some(name) = ws.delete_view(key) {
                    self.report(&format!("View {key} ({name}) deleted"), false, None);
                }
            }
            Act::Arrange(how) => {
                ws.arrange(how);
                self.histories.clear();
            }
            Act::EvenOut => ws.even_out(),
            Act::Split(w, side) => {
                if !ws.split(w, side) {
                    self.report("Eight windows is the most a tab holds.", false, None);
                }
            }
            Act::CloseWindow(w) => {
                if let Some(Some(p)) = self.window_panes().get(w) {
                    self.fleet.release(&p.node, p.pane);
                }
                if self.settings.workspace.close_window(w) {
                    self.histories.clear();
                }
            }
            Act::Swap(a, b) => {
                if self.settings.workspace.swap(a, b) {
                    self.histories.clear();
                }
            }
            Act::ClearEnded(keys) => {
                let n = ws.clear_ended(&keys);
                self.histories.clear();
                let text = match n {
                    1 => "Cleared 1 ended session from its window".to_string(),
                    n => format!("Cleared {n} ended sessions from their windows"),
                };
                self.report(&text, false, None);
            }
            Act::PickKeys(keys) => {
                self.pick.clear();
                for k in &keys {
                    self.pick.toggle(&k.text());
                }
                self.host.set_side_open(true, cx);
                window.focus(&self.rail_focus, cx);
                self.report(
                    &format!(
                        "Picked {} in the list: Delete offers to kill them",
                        keys.len()
                    ),
                    false,
                    None,
                );
                cx.notify();
                return;
            }
            Act::KillKeys(keys) => self.kill_sessions(&keys, true),
            Act::TidyLater => self.settings.tidy_seen = self.tidy().texts(),
            Act::CopyPath(k) => {
                match self.fleet.session(&k).and_then(|s| s.cwd.clone()) {
                    Some(path) => {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(path.clone()));
                        self.report(&format!("Copied {path}"), false, None);
                    }
                    None => self.report(&format!("{} has no folder to copy", k.name), true, None),
                }
                return;
            }
            Act::Peek(k) => {
                self.open_peek(k, window, cx);
                return;
            }
            Act::Send(k) => {
                self.open_send(k, window, cx);
                return;
            }
            Act::Sees(k) => {
                let Some(session) = self.fleet.session(&k) else {
                    return;
                };
                let Some(folder) = session.cwd.clone() else {
                    self.report(&format!("{} has no folder to read", k.name), true, None);
                    return;
                };
                // The Harness page names harnesses by the tools report's ids.
                let harness = match crate::store::sessions::running(session) {
                    crate::store::sessions::Running::Harness("claude-code") => Some("claude"),
                    crate::store::sessions::Running::Harness("codex") => Some("codex"),
                    _ => None,
                };
                let ask = ahjo_app_kit::Ask::Reveal {
                    what: "agent".into(),
                    id: folder,
                    node: Some(ahjo_core::NodeId(k.node.clone())),
                    part: harness.map(str::to_string),
                    beside: false,
                };
                if let Err(e) = self.host.ask(ask, cx) {
                    self.report(&e.message, true, None);
                }
                return;
            }
            Act::Attach(k, w) => self.attach(k, w, window, cx),
            Act::Unassign(k) => {
                self.settings.workspace.unbind_everywhere(&k);
                if let Some(s) = self.fleet.session(&k) {
                    let panes: Vec<u32> = s.panes.iter().map(|p| p.id).collect();
                    for p in panes {
                        self.fleet.release(&k.node, p);
                    }
                }
            }
            Act::Rename(k, at) => {
                self.rename_field
                    .update(cx, |f, cx| f.set(&k.name, true, cx));
                self.open(Surface::RenameSession(k, at), window, cx);
                return;
            }
            Act::Copy(k) => self.copy_session(&k),
            Act::Kill(k) => self.kill_sessions(&[k], false),
            Act::KillPicked => {
                let targets = self.pick_targets();
                self.kill_sessions(&targets, true);
            }
            Act::Remove(w, k) => {
                if self.settings.workspace.shown(w).is_some_and(|b| b.key == k) {
                    if let Some(Some(p)) = self.window_panes().get(w) {
                        self.fleet.release(&p.node, p.pane);
                    }
                    self.histories.remove(&w);
                }
                self.settings.workspace.unbind(w, &k);
            }
            Act::ShowTag(w, k) => {
                if let Some(Some(p)) = self.window_panes().get(w) {
                    self.fleet.release(&p.node, p.pane);
                }
                if self.settings.workspace.show_tag(w, &k) {
                    self.histories.remove(&w);
                }
                self.settings.workspace.focus = w;
            }
            Act::Pane(w, p) => {
                self.settings.workspace.choose_pane(w, p);
                self.histories.remove(&w);
            }
            Act::WindowList(w, key) => self.window_list(w, key, cx),
            Act::WindowPane(w, key, pane) => {
                if self
                    .settings
                    .workspace
                    .shown(w)
                    .is_some_and(|b| b.key == key)
                    && self
                        .fleet
                        .session(&key)
                        .is_some_and(|s| s.panes.iter().any(|p| p.id == pane))
                {
                    self.settings.workspace.choose_pane(w, pane);
                    self.histories.remove(&w);
                }
            }
            Act::History(w) => self.open_history(w, cx),
            Act::ResetZoom(w) => {
                self.zoom_slot(w, ahjo_app_kit::zoom::Zoom::Reset, 1, cx);
                return;
            }
            Act::Restart(k) => self.restart(k),
        }
        self.sync(cx);
        self.save(cx);
        cx.notify();
    }

    // ---- sessions ----

    pub fn focused_key(&self) -> Option<Key> {
        let ws = &self.settings.workspace;
        ws.shown(ws.focus).map(|b| b.key.clone())
    }

    /// Peek is open and on top: it has the keyboard.
    pub fn peek_on_top(&self, cx: &App) -> bool {
        self.peek.is_some() && self.host.is_top(surface::PEEK, cx)
    }

    /// The terminal that takes what is typed: Peek while it is on top,
    /// else the focused window.
    pub fn active_slot(&self, cx: &App) -> usize {
        if self.peek_on_top(cx) {
            PEEK_SLOT
        } else {
            self.settings.workspace.focus
        }
    }

    /// The live pane a slot shows: a window's, or Peek's.
    pub fn slot_pane(&self, i: usize) -> Option<PaneAt> {
        if i == PEEK_SLOT {
            let p = self.peek.as_ref()?;
            self.fleet.session(&p.key)?;
            return Some(PaneAt {
                node: p.key.node.clone(),
                pane: p.pane?,
            });
        }
        self.window_panes().get(i).cloned().flatten()
    }

    /// The session a slot shows.
    pub fn slot_key(&self, i: usize) -> Option<Key> {
        if i == PEEK_SLOT {
            return self.peek.as_ref().map(|p| p.key.clone());
        }
        self.settings.workspace.shown(i).map(|b| b.key.clone())
    }

    /// The pane each window of the active tab shows, if live.
    pub fn window_panes(&self) -> Vec<Option<PaneAt>> {
        self.settings
            .workspace
            .windows()
            .iter()
            .map(|w| match self.fleet.tile(w.shown()) {
                TileState::Live { session, pane } => Some(PaneAt {
                    node: session.node.clone(),
                    pane,
                }),
                _ => None,
            })
            .collect()
    }

    /// Keeps the name typed in the tab bar for the tab being renamed.
    fn keep_tab_name(&mut self, cx: &mut Context<Self>) {
        if let Some(Naming::Tab(i)) = self.naming {
            self.naming = None;
            let name = self.rename_field.read(cx).text.clone();
            self.settings.workspace.rename_tab(i, &name);
            self.close(surface::RENAME_TAB, cx);
            self.save(cx);
        }
    }

    /// Makes an ended session again under the same name and user. A witnessed
    /// launch keeps its recipe; other sessions use a shell in their last folder.
    fn restart(&mut self, k: Key) {
        if self.restarting.contains(&k) {
            return;
        }
        let ws = &self.settings.workspace;
        let target = ws
            .bound(&k)
            .filter(|(tab, _)| *tab == ws.active)
            .map_or(ws.focus, |(_, w)| w);
        let mut launch = restart_launch(&k, self.settings.restarts.get(&k.text()));
        launch.size = self.launch_grids.borrow()[target];
        match self.fleet.launch_on(&k.node, launch.clone()) {
            Some(id) => {
                let tab = self.settings.workspace.active;
                self.report(&format!("Restarting {}…", k.name), false, None);
                self.track_launch(&k.node, id, launch, None, tab);
                self.restarting.insert(k);
            }
            None => self.report(
                &format!("Could not restart {}: its device is away", k.name),
                true,
                None,
            ),
        }
    }

    /// Asks the session's node to rename it; its windows follow once the
    /// node answers. An empty or unchanged name changes nothing.
    fn rename_session(&mut self, k: Key, name: &str) {
        if name.is_empty() || name == k.name {
            return;
        }
        let Some(s) = self.fleet.session(&k).cloned() else {
            self.report(
                &format!("Could not rename {}: it is gone", k.name),
                true,
                None,
            );
            return;
        };
        match self.fleet.rename(&k.node, &s.id, name) {
            Some(id) => {
                self.report(&format!("Renaming {}…", k.name), false, None);
                self.renames
                    .insert((k.node.clone(), id), (k, name.to_string()));
            }
            None => self.report(
                &format!("Could not rename {}: its device is away", k.name),
                true,
                None,
            ),
        }
    }

    pub fn attach(&mut self, k: Key, w: usize, window: &mut Window, cx: &mut Context<Self>) {
        let ws = &mut self.settings.workspace;
        if w >= ws.windows().len() {
            return;
        }
        if let Some(Some(p)) = self.window_panes().get(w) {
            self.fleet.release(&p.node, p.pane);
        }
        self.settings.workspace.bind(w, k, None);
        self.histories.remove(&w);
        self.host.show(cx);
        self.sync(cx);
        self.save(cx);
        window.focus(&self.term_focus, cx);
        cx.notify();
    }

    /// A session goes where it is wanted: the focused window if empty, else
    /// the first empty one, else a new window, else the focused one.
    pub fn place(&mut self, k: Key, window: &mut Window, cx: &mut Context<Self>) {
        self.acted = true;
        if let Some((t, w)) = self.settings.workspace.bound(&k) {
            self.settings.workspace.show_tab(t);
            self.settings.workspace.focus = w;
            if self.settings.workspace.show_tag(w, &k) {
                self.histories.remove(&w);
            }
            self.sync(cx);
            self.host.show(cx);
            window.focus(&self.term_focus, cx);
            self.save(cx);
            cx.notify();
            return;
        }
        let ws = &mut self.settings.workspace;
        let w = match ws.empty_window() {
            Some(w) => w,
            None if ws.add_window() => ws.focus,
            None => ws.focus,
        };
        self.attach(k, w, window, cx);
    }

    /// Bring a session and optional immutable pane id into view. Resolve the
    /// whole request before changing the workspace, including a bad pane.
    pub(super) fn reveal_session(
        &mut self,
        id: &str,
        node: Option<NodeId>,
        part: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let node = node
            .map(|n| n.to_string())
            .or_else(|| self.fleet.here_id().map(str::to_owned))
            .unwrap_or_else(|| "here".into());
        let candidates: Vec<_> = self.fleet.all.iter().filter(|s| s.node == node).collect();
        let by_id = candidates.iter().copied().find(|s| s.id == id);
        let names: Vec<_> = candidates
            .iter()
            .copied()
            .filter(|s| s.name == id)
            .collect();
        let session = by_id.or_else(|| (names.len() == 1).then(|| names[0]));
        let Some(session) = session else {
            let why = if names.len() > 1 {
                format!("several sessions named {id} on {node}; use a session id")
            } else {
                format!("no session {id} on {node}")
            };
            self.report(&why, true, None);
            self.flush(cx);
            cx.notify();
            return;
        };
        let pane = match part {
            None => None,
            Some(part) => match part
                .strip_prefix('%')
                .unwrap_or(part)
                .parse::<u32>()
                .ok()
                .filter(|p| session.panes.iter().any(|pane| pane.id == *p))
            {
                Some(pane) => Some(pane),
                None => {
                    self.report(
                        &format!("no pane {part} in session {id} on {node}"),
                        true,
                        None,
                    );
                    self.flush(cx);
                    cx.notify();
                    return;
                }
            },
        };
        let key = Key::of(session);
        // Bindings in hidden windows and tabs remain the operator's intent.
        let bound = self
            .settings
            .workspace
            .tabs
            .iter()
            .enumerate()
            .find_map(|(tab, t)| {
                t.windows
                    .iter()
                    .enumerate()
                    .find(|(_, w)| w.sessions.iter().any(|b| b.key == key))
                    .map(|(w, _)| (tab, w))
            });
        let w = if let Some((tab, w)) = bound {
            let ws = &mut self.settings.workspace;
            ws.count = ws.count.max(tab + 1);
            if ws.tabs[tab].count <= w {
                ws.set_windows(tab, w + 1);
            }
            ws.show_tab(tab);
            ws.focus = w;
            if ws.show_tag(w, &key) {
                self.histories.remove(&w);
            }
            w
        } else {
            let w = self.settings.workspace.focus;
            self.attach(key, w, window, cx);
            w
        };
        if let Some(pane) = pane {
            self.settings.workspace.choose_pane(w, pane);
            self.histories.remove(&w);
        }
        self.sync(cx);
        self.save(cx);
        self.host.show(cx);
        window.focus(&self.term_focus, cx);
        cx.notify();
    }

    /// A mouse-down outside Peek's card, anywhere in the window, closes it
    /// while it is on top; whatever the down lands on still takes it.
    pub fn peek_down_outside(&mut self, e: &MouseDownEvent, cx: &mut Context<Self>) {
        if !self.host.is_top(surface::PEEK, cx) {
            return;
        }
        self.peek_left = self.peek.as_ref().map(|p| (p.key.clone(), e.position));
        self.close(surface::PEEK, cx);
    }

    pub fn open_peek(&mut self, k: Key, window: &mut Window, cx: &mut Context<Self>) {
        // Another session's history does not carry over.
        self.leave_history(PEEK_SLOT, cx);
        self.peek = Some(Peek { key: k, pane: None });
        self.open(Surface::Peek, window, cx);
        self.sync(cx);
    }

    pub fn open_match(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(m) = self
            .fleet
            .found()
            .and_then(|(matches, _)| matches.get(i).map(|m| (*m).clone()))
        else {
            return;
        };
        let Some(s) = self.fleet.session_of_pane(&m.node, m.pane) else {
            return;
        };
        let k = Key::of(s);
        self.place(k, window, cx);
        // The hits stay open, the query keeps the keys: Enter goes on to
        // the next, Shift+Enter back. From another page, the sessions
        // page shows the hit.
        if !self.shown {
            self.host.for_page(crate::PAGE, &[]).show(cx);
        }
        self.find_cursor = i;
        self.find_shown = Some(i);
        self.find_scroll.scroll_to_item(i);
        field::focus(&self.find_query, window, cx);
        let w = self.settings.workspace.focus;
        self.settings.workspace.choose_pane(w, m.pane);
        self.histories.remove(&w);
        if let Some(id) = self.fleet.history(&m.node, m.pane) {
            self.history_wanted.insert(
                id,
                HistoryTarget::Window {
                    index: w,
                    line: Some(m.line as usize),
                },
            );
        }
        self.sync(cx);
        cx.notify();
    }

    /// Empties the rail's filter, so the whole list shows again.
    pub fn clear_rail_search(&mut self, cx: &mut Context<Self>) {
        self.rail_search.update(cx, |f, cx| f.set("", false, cx));
    }

    /// Opens the scrollback search for `words`, from the session search.
    /// The words move there: the rail's filter empties.
    pub fn search_inside(&mut self, words: &str, window: &mut Window, cx: &mut Context<Self>) {
        let words = words.trim().to_string();
        self.clear_rail_search(cx);
        self.find_query.update(cx, |f, cx| f.set(&words, false, cx));
        self.show_in_panel(surface::FIND, window, cx);
        self.fleet.search(&words);
        cx.notify();
    }

    /// Enter (`forward`) or Shift+Enter in the scrollback search: the
    /// lit hit first, then the next or the one before, round the ends.
    pub fn step_match(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let n = self.fleet.found().map_or(0, |(m, _)| m.len());
        if let Some(i) = crate::store::find::step(n, self.find_cursor, self.find_shown, forward) {
            self.open_match(i, window, cx);
        }
    }

    // ---- history ----

    pub fn open_history(&mut self, w: usize, cx: &mut Context<Self>) {
        let Some(pane) = self.slot_pane(w) else {
            return;
        };
        if self.histories.get(&w).is_some_and(|h| h.pane == pane)
            || self
                .history_wanted
                .values()
                .any(|x| matches!(x, HistoryTarget::Window { index, .. } if *index == w))
        {
            return;
        }
        if let Some(id) = self.fleet.history(&pane.node, pane.pane) {
            self.history_wanted.insert(
                id,
                HistoryTarget::Window {
                    index: w,
                    line: None,
                },
            );
        }
        cx.notify();
    }

    fn history_arrived(
        &mut self,
        id: u32,
        pane: PaneAt,
        text: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(target) = self.history_wanted.remove(&id) else {
            return;
        };
        let Some(text) = text else {
            self.report("The pane is gone; its history went with it.", true, None);
            return;
        };
        let (cols, rows) = self
            .fleet
            .screen(&pane.node, pane.pane)
            .map(|s| {
                let s = s.lock();
                (s.cols, s.rows)
            })
            .or_else(|| {
                self.fleet
                    .session_of_pane(&pane.node, pane.pane)
                    .and_then(|s| s.panes.iter().find(|p| p.id == pane.pane))
                    .map(|p| (p.cols as usize, p.rows as usize))
            })
            .unwrap_or((80, 24));
        let query = self.find_query.read(cx).text.clone();
        match target {
            HistoryTarget::Window { index: w, line } => {
                if self.slot_pane(w).as_ref() != Some(&pane) {
                    return;
                }
                // On the live screen, so output goes on arriving below;
                // a window already reading it back keeps the lines it has,
                // unless a search result needs its line counted afresh.
                let live = self.fleet.screen(&pane.node, pane.pane).filter(|l| {
                    let mut l = l.lock();
                    (l.has_history() && line.is_none()) || l.grow_history(&text)
                });
                let screen = match live {
                    Some(l) => {
                        if !self.grown.iter().any(|g| Arc::ptr_eq(g, &l)) {
                            self.grown.push(l.clone());
                        }
                        l
                    }
                    None => Arc::new(Mutex::new(Screen::history(&text, cols, rows, None))),
                };
                let top = {
                    let mut s = screen.lock();
                    if let Some(line) = line {
                        select_line(&mut s, line, &query);
                    } else {
                        // One screen up from the bottom: the wheel asked to go back.
                        s.term
                            .scroll_display(alacritty_terminal::grid::Scroll::Delta(3));
                    }
                    let top = s.top();
                    s.live_view();
                    top
                };
                self.histories.insert(w, History { pane, screen, top });
            }
        }
        cx.notify();
    }
}

impl Page for Terminals {
    const ID: &'static str = crate::PAGE;
    const TITLE: &'static str = "Sessions";
    const KEY: Option<char> = Some('T');
    const AREA: Option<&'static str> = Some(crate::AREA);
    const KINDS: Kinds = ahjo_terminals_contract::KINDS;
    const ANSWERS: &'static [Answers] =
        &[Answers::Send, Answers::Launch, Answers::Reveal("session")];
    const SETTINGS: &'static [Setting] = crate::store::settings::DECLARED;

    /// The Settings page changed one of ours: taken, the rest as it is.
    fn settings_changed(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        match Settings::load(&self.host, cx) {
            Ok(fresh) => self.settings.take_declared(fresh),
            Err(e) => self.report(
                &format!("The Sessions settings did not read: {}", e.message),
                true,
                None,
            ),
        }
        self.flush(cx);
        cx.notify();
    }

    fn frame(
        &mut self,
        node: &NodeId,
        kind: u8,
        body: &[u8],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.frame_from(node, kind, body, window, cx);
    }

    fn link(
        &mut self,
        node: &NodeId,
        state: Option<LinkState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.link_changed(node, state, window, cx);
    }

    fn tabs(
        &mut self,
        shown: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        Some(self.render_tabs(shown, window, cx))
    }

    /// Peek and Overview are a glance at this page's sessions, so leaving
    /// the page closes them. Send and the search stand in the window's
    /// panel, beside any page.
    fn shown(&mut self, shown: bool, _: &mut Window, cx: &mut Context<Self>) {
        self.shown = shown;
        if !shown {
            for glance in [surface::PEEK, surface::OVERVIEW] {
                if self.is_open(glance, cx) {
                    self.close(glance, cx);
                }
            }
        }
        cx.notify();
    }

    /// Send to Session with what another page hands over, aimed at the
    /// session the ask names, else at the focused window's; or a launch
    /// another page asks for.
    fn answer(&mut self, ask: Ask, _: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        match ask {
            Ask::Send { reference, target } => {
                let named = target.map(|t| Key {
                    node: t.node.to_string(),
                    user: t.user,
                    name: t.name,
                });
                let target = crate::store::send::first_target(
                    &self.fleet.all,
                    named.as_ref(),
                    self.focused_key().as_ref(),
                );
                self.handoff(Some(reference), target, window, cx);
            }
            Ask::Reveal {
                what,
                id,
                node,
                part,
                ..
            } if what == "session" => {
                self.reveal_session(&id, node, part.as_deref(), window, cx);
            }
            Ask::Launch { node, folder, name } => {
                self.launch_asked(node.as_str(), &folder, &name);
                self.flush(cx);
                cx.notify();
            }
            _ => {}
        }
    }

    fn chords(&self, _cx: &App) -> Vec<Chord> {
        self.chord_list()
    }

    /// The terminal, or the rail while rows are picked there: it holds
    /// Delete and Escape, as CHROTE's panel does once its menu closes.
    fn focus(&self, cx: &App) -> Option<FocusHandle> {
        if !self.pick.keys.is_empty() && self.host.side_open(cx) {
            Some(self.rail_focus.clone())
        } else {
            Some(self.term_focus.clone())
        }
    }
}

impl Render for Terminals {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.see_focused(window);
        // An empty window's keys are its launcher's: the terminal would drop
        // them, having no session to type into.
        let w = self.settings.workspace.focus;
        let launcher = self
            .launchers
            .get(w)
            .filter(|_| self.settings.workspace.shown(w).is_none())
            .cloned();
        let nowhere = window.focused(cx).is_none();
        if self.shown && !self.host.has_open(cx) {
            match launcher {
                Some(l) if nowhere || self.term_focus.is_focused(window) => {
                    l.update(cx, |l, cx| l.focus_first(window, cx));
                }
                None if nowhere => window.focus(&self.term_focus, cx),
                _ => {}
            }
        }
        self.render_terminals(cx)
    }
}

/// What Restart asks the node for, retaining a witnessed launch recipe or
/// falling back to a shell in the last observed folder.
fn restart_launch(k: &Key, saved: Option<&crate::store::settings::Restart>) -> Launch {
    Launch {
        harness: saved
            .and_then(|s| s.harness.clone())
            .unwrap_or_else(|| "shell".into()),
        folder: saved.map(|s| s.folder.clone()).unwrap_or_default(),
        name: k.name.clone(),
        flags: saved
            .filter(|s| s.harness.is_some())
            .map(|s| s.flags.clone())
            .unwrap_or_default(),
        user: k.user.clone(),
        install_completion_hooks: saved
            .is_some_and(|s| s.harness.is_some() && s.install_completion_hooks),
        size: None,
    }
}

/// Marks a history line as selected, so a search result is found at once.
fn select_line(s: &mut Screen, line: usize, query: &str) {
    use alacritty_terminal::grid::Dimensions;
    use alacritty_terminal::grid::Scroll;
    use alacritty_terminal::index::{Column, Line, Point as GP, Side};
    use alacritty_terminal::selection::{Selection, SelectionType};
    let Some((start, end)) = s.history_rows(line) else {
        return;
    };
    let history = s.term.grid().history_size();
    let cols = s.term.grid().columns();
    // A joined tmux line can span several rows. Bring the row containing the
    // query into view while selecting the whole logical line.
    let needle = query.to_lowercase();
    let target = (start..=end)
        .find(|&row| {
            let text: String = (0..cols)
                .map(|col| s.term.grid()[GP::new(Line(row as i32 - history as i32), Column(col))].c)
                .collect();
            !needle.is_empty() && text.to_lowercase().contains(&needle)
        })
        .unwrap_or(start);
    let total = history + s.rows;
    let desired = total.saturating_sub(target + 1).saturating_sub(s.rows / 2);
    let current = s.term.grid().display_offset();
    s.term
        .scroll_display(Scroll::Delta(desired as i32 - current as i32));
    let start = start as i32 - history as i32;
    let end = end as i32 - history as i32;
    let mut sel = Selection::new(
        SelectionType::Lines,
        GP::new(Line(start), Column(0)),
        Side::Left,
    );
    sel.update(
        GP::new(Line(end), Column(cols.saturating_sub(1))),
        Side::Right,
    );
    s.term.selection = Some(sel);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restart_starts_the_same_name_and_user_again_as_a_shell_at_home() {
        let k = Key {
            node: "n".into(),
            user: "alice".into(),
            name: "build".into(),
        };
        let l = restart_launch(&k, None);
        assert_eq!((l.name.as_str(), l.user.as_str()), ("build", "alice"));
        assert_eq!(
            (l.harness.as_str(), l.folder.as_str(), l.flags.as_str()),
            ("shell", "", "")
        );
        assert!(!l.install_completion_hooks);
        let saved = crate::store::settings::Restart {
            id: "$1".into(),
            folder: "/tmp/work".into(),
            harness: Some("fake".into()),
            flags: "--chosen".into(),
            install_completion_hooks: true,
        };
        let l = restart_launch(&k, Some(&saved));
        assert_eq!(
            (
                l.harness.as_str(),
                l.folder.as_str(),
                l.flags.as_str(),
                l.install_completion_hooks
            ),
            ("fake", "/tmp/work", "--chosen", true)
        );
        let fallback = crate::store::settings::Restart {
            harness: None,
            ..saved
        };
        let l = restart_launch(&k, Some(&fallback));
        assert_eq!(
            (
                l.harness.as_str(),
                l.folder.as_str(),
                l.flags.as_str(),
                l.install_completion_hooks
            ),
            ("shell", "/tmp/work", "", false)
        );
    }
}
