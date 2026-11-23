//! The Issue page (DESIGN.md, Issue): one project's open work as a tree with
//! the item beside it, the whole store a search away, an item handed to a
//! session in one action; the same item beside a terminal that named it,
//! and the project's tree in a panel over the terminals.
mod beside;
mod chart;
mod layouts;
mod natural;
mod panel;
mod report;
pub mod store;
#[cfg(test)]
mod tests;
mod text;
pub(crate) mod tokens;
mod view;
mod words;

use ahjo_app_kit::ask::{Answers, Ask};
use ahjo_app_kit::text::fuzzy;
use ahjo_app_kit::widgets::field::{self, FieldEvent, TextField};
use ahjo_app_kit::widgets::select::Selectable;
use ahjo_app_kit::{Host, Mounted, Page, mount};
use ahjo_core::NodeId;
use ahjo_issue_contract::{SNAPSHOT, Snapshot, WATCH};
use ahjo_terminals_contract::SessionList;
use gpui::{App, AppContext, Context, Entity, FocusHandle, KeyDownEvent, WeakEntity, Window};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use store::issue::outline::{self, Found, Line};
use store::issue::timeline::{self, Clock, Dated, Day, Movement, Range, Timeline};
use store::issue::{Board, RowKey, Status, prose};

pub const NAME: &str = "issue";
pub fn build(host: Host, window: &mut Window, cx: &mut App) -> Mounted {
    mount(host, window, cx, Issue::new)
}

/// What the list shows, in order: the rows the keys walk, and the labels
/// between them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    /// A row of the board, this deep in the tree; `away` when it is
    /// another project's.
    Item {
        row: usize,
        depth: usize,
        away: bool,
    },
    Label(String),
    /// Matches left out of a long search.
    More(usize),
}

/// How the page lays out the work (DESIGN.md, Issue): the strip first, the
/// time page second, and the bare tree for those who want only that.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    /// The tree under a strip of the last weeks, the projects and states
    /// counted beside it; a range on the strip lists what moved in it.
    Strip,
    /// The chart is the page: a range says what opened, finished and sat
    /// idle, across every project or one.
    Time,
    /// One project's open work as a tree, the item beside it.
    Tree,
}

impl Layout {
    pub const ALL: [Layout; 3] = [Layout::Strip, Layout::Time, Layout::Tree];
    pub fn key(self) -> &'static str {
        match self {
            Layout::Tree => "tree",
            Layout::Strip => "strip",
            Layout::Time => "time",
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Layout::Tree => "Tree",
            Layout::Strip => "Strip",
            Layout::Time => "Time",
        }
    }
    /// What it shows, for its button's tooltip.
    pub fn what(self) -> &'static str {
        match self {
            Layout::Strip => "The list under a time strip (V for the next)",
            Layout::Time => "The chart as the page (V for the next)",
            Layout::Tree => "One project's tree (V for the next)",
        }
    }
    fn of(key: &str) -> Layout {
        Layout::ALL
            .into_iter()
            .find(|l| l.key() == key)
            .unwrap_or(Layout::Strip)
    }
    fn next(self) -> Layout {
        match self {
            Layout::Strip => Layout::Time,
            Layout::Time => Layout::Tree,
            Layout::Tree => Layout::Strip,
        }
    }
}

/// Which of a range's movements the time page lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Moved {
    Opened,
    Closed,
    Idle,
}

pub struct Issue {
    host: Host,
    me: WeakEntity<Issue>,
    focus: FocusHandle,
    /// The search, or the project's name while a project is being chosen.
    search: Entity<TextField>,
    report: Option<Entity<report::Report>>,
    snapshots: BTreeMap<String, Arc<Snapshot>>,
    /// A node's snapshot that arrived while another was being read.
    pending: BTreeMap<String, Vec<u8>>,
    frame_errors: BTreeMap<String, String>,
    reading: bool,
    /// Nodes whose link closed while their snapshot was being read: the
    /// read is dropped when it lands.
    gone: std::collections::HashSet<String>,
    /// Nodes still reading for a Refresh press, with the count of
    /// refreshes they had finished when it was pressed.
    refreshing: BTreeMap<String, u64>,
    board: Rc<Board>,
    /// The project chosen last, by its folder or repository; kept.
    project: Option<String>,
    /// With no project chosen and none named by a session: the project
    /// whose work moved last, settled once every project has been read so
    /// the page never shows one project and then another.
    settled: Option<String>,
    query: String,
    /// Choosing a project: the search names one, the list holds them.
    picking: bool,
    picked: usize,
    entries: Vec<Entry>,
    selected: Option<RowKey>,
    selected_node: Option<String>,
    card_line: Option<(String, String)>,
    pub(crate) trail: Vec<(Option<String>, RowKey)>,
    prose: Rc<RefCell<Selectable>>,
    wanted: Option<(String, Option<String>)>,
    sessions: Option<Entity<SessionList>>,
    /// The list's scroll, and whether the row the keys reached is to be
    /// brought into view at the next draw.
    list_scroll: gpui::ScrollHandle,
    reveal: bool,
    /// The item's finished family shows, not one line saying how many.
    show_finished: bool,
    /// The panel over the terminals, made the first time it opens, and the
    /// item it shows instead of its tree.
    panel: Option<Entity<panel::Panel>>,
    pub(crate) panel_item: Option<RowKey>,
    /// The panel lists the projects to choose from, and the one chosen in
    /// it since it opened.
    pub(crate) panel_picking: bool,
    panel_project: Option<String>,
    /// The panel's find, which has the keyboard when Alt+B opens it, and
    /// what holds the keyboard in the panel once the find is left.
    panel_search: Entity<TextField>,
    pub(crate) panel_focus: FocusHandle,
    /// Items' text, which the snapshot leaves out, as read from their
    /// node: by reference, with the update it was read for.
    texts: HashMap<String, (Option<u64>, text::Text)>,
    /// The words last searched, and the items the nodes say hold them.
    text_hits: (String, HashSet<String>),
    /// The take on the page in use; kept.
    layout: Layout,
    /// The days chosen on the chart, and the day a drag started from
    /// while the mouse is held.
    range: Option<Range>,
    anchor: Option<Day>,
    /// The day under the mouse on the chart.
    hover: Option<Day>,
    clock: Clock,
    today: Day,
    /// Each row's days, counted once per board.
    dated: Rc<Vec<Dated>>,
    /// How many days untouched makes open work idle on the time page; kept.
    idle_days: i64,
    /// The list's width once dragged at its edge, kept; until then a
    /// share of the room ([`tokens::LIST_SHARE_OPEN`]), so a wide window
    /// widens it. Where it starts and its width as last drawn: the room
    /// is measured from there.
    list_w: Option<f32>,
    list_drawn: Rc<std::cell::Cell<(f32, f32)>>,
    /// The strip and the time page show every project, not the chosen one.
    every: bool,
    /// The strip's chart is folded away to one line; kept.
    chart_folded: bool,
    /// The strip's state facet: only work in this state.
    state: Option<Status>,
    /// What the time page lists of the range.
    moved: Moved,
    /// The chosen work over time, and what moved in the range.
    timeline: Rc<Timeline>,
    movement: Movement,
    /// Per project, and per state, how much of the work in view counts:
    /// open work, or with a range the work that moved in it.
    project_counts: Vec<usize>,
    state_counts: [usize; 5],
    /// The list's scroll for the strip and time pages.
    rows_scroll: gpui::UniformListScrollHandle,
    /// What the timeline was counted for: the project, every project, and
    /// the board.
    counted: (Option<usize>, bool, *const Board),
    /// The timeline and range the time page last chose its list for.
    moved_for: (*const Timeline, Option<Range>),
}

impl Page for Issue {
    const ID: &'static str = NAME;
    const TITLE: &'static str = "Issues";
    const KEY: Option<char> = Some('O');
    const AREA: Option<&'static str> = Some(NAME);
    const KINDS: ahjo_core::frame::Kinds = &[SNAPSHOT..=SNAPSHOT];
    const CHANNELS: &'static [u8] = &[WATCH];
    const ANSWERS: &'static [Answers] = &[Answers::Reveal("item")];
    const SETTINGS: &'static [ahjo_app_kit::declared::Setting] =
        &[ahjo_app_kit::declared::Setting::choice(
            "layout",
            "Layout",
            "strip",
            &[("strip", "Strip"), ("time", "Time"), ("tree", "Tree")],
        )
        .note(
            "The list under a time strip, the chart as the page, or the tree alone (V on the page)",
        )];
    fn settings_changed(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let settings: serde_json::Value = self.host.setting(cx).unwrap_or_default();
        let layout = Layout::of(settings["layout"].as_str().unwrap_or(""));
        if layout != self.layout {
            self.set_layout(layout, false, cx);
        }
    }
    fn frame(&mut self, node: &NodeId, _: u8, body: &[u8], _: &mut Window, cx: &mut Context<Self>) {
        self.gone.remove(node.as_str());
        self.pending.insert(node.as_str().into(), body.to_vec());
        self.read_pending(cx);
    }
    fn link(
        &mut self,
        node: &NodeId,
        state: Option<ahjo_app_kit::link::client::LinkState>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if state.is_none() {
            self.snapshots.remove(node.as_str());
            self.pending.remove(node.as_str());
            self.frame_errors.remove(node.as_str());
            self.refreshing.remove(node.as_str());
            self.gone.insert(node.as_str().into());
            self.rebuild(cx);
        }
    }
    fn answer(&mut self, ask: Ask, _: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        if let Ask::Reveal {
            id,
            node,
            beside: true,
            ..
        } = ask
        {
            self.open_beside(id, node, window, cx);
        } else if let Ask::Reveal { id, node, .. } = ask {
            self.wanted = Some((id, node.map(|n| n.to_string())));
            self.open_wanted(cx);
        }
    }
    fn chords(&self, _: &App) -> Vec<ahjo_app_kit::chords::Chord> {
        let me = self.me.clone();
        let close = self.me.clone();
        let report = self.me.clone();
        let (layout, search, pick) = (self.me.clone(), self.me.clone(), self.me.clone());
        let fold = self.me.clone();
        vec![
            ahjo_app_kit::chords::Chord::new("r", "Report", move |window, cx| {
                let _ = report.update(cx, |work, cx| work.open_report(window, cx));
            })
            .alt("r", Some(false))
            .everywhere(),
            ahjo_app_kit::chords::Chord::new("i", "Close the item", move |_, cx| {
                let _ = close.update(cx, |work, cx| {
                    if work.host.panel_shows(beside::CARD, cx) {
                        work.host.put_panel_away(cx);
                    }
                });
            })
            .alt("i", Some(false))
            .everywhere(),
            ahjo_app_kit::chords::Chord::new("b", "Issues beside", move |window, cx| {
                let _ = me.update(cx, |work, cx| work.toggle_panel(window, cx));
            })
            .alt("b", Some(false))
            .everywhere(),
            // The keys the page takes itself, so the keys panel lists them.
            ahjo_app_kit::chords::Chord::on_page(
                "V",
                "Layout: strip, time or tree",
                move |_, cx| {
                    let _ =
                        layout.update(cx, |work, cx| work.set_layout(work.layout.next(), true, cx));
                },
            ),
            ahjo_app_kit::chords::Chord::on_page("/", "Search issues", move |window, cx| {
                let _ = search.update(cx, |work, cx| field::focus(&work.search, window, cx));
            }),
            ahjo_app_kit::chords::Chord::on_page("P", "Pick a project", move |window, cx| {
                let _ = pick.update(cx, |work, cx| work.start_picking(window, cx));
            }),
            ahjo_app_kit::chords::Chord::on_page(
                "C",
                "Fold the chart away, or show it",
                move |_, cx| {
                    let _ = fold.update(cx, |work, cx| {
                        if work.layout == Layout::Strip {
                            work.fold_chart(!work.chart_folded, cx);
                        }
                    });
                },
            ),
        ]
    }
    fn focus(&self, _: &App) -> Option<FocusHandle> {
        Some(self.focus.clone())
    }
}

impl Issue {
    pub fn new(host: Host, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| TextField::new(cx, SEARCH));
        cx.subscribe_in(&search, window, |w: &mut Self, _, ev, window, cx| {
            w.search_event(*ev, window, cx)
        })
        .detach();
        let panel_search = cx.new(|cx| TextField::new(cx, PANEL_SEARCH));
        cx.subscribe_in(&panel_search, window, |w: &mut Self, _, ev, window, cx| {
            w.panel_search_event(*ev, window, cx)
        })
        .detach();
        let sessions = host.model::<SessionList>(cx);
        if let Some(s) = &sessions {
            cx.observe(s, |_, _, cx| cx.notify()).detach();
        }
        let settings: serde_json::Value = host.setting(cx).unwrap_or_default();
        let now = ahjo_app_kit::hub::now_ms();
        let clock = Clock::local();
        let today = clock.day(now);
        let layout = Layout::of(settings["layout"].as_str().unwrap_or(""));
        Self {
            host,
            me: cx.weak_entity(),
            focus: cx.focus_handle(),
            search,
            report: None,
            snapshots: BTreeMap::new(),
            pending: BTreeMap::new(),
            frame_errors: BTreeMap::new(),
            reading: false,
            gone: Default::default(),
            refreshing: BTreeMap::new(),
            board: Rc::default(),
            project: settings["project"].as_str().map(str::to_owned),
            settled: None,
            query: String::new(),
            picking: false,
            picked: 0,
            entries: Vec::new(),
            selected: None,
            selected_node: None,
            card_line: None,
            trail: Vec::new(),
            prose: Rc::default(),
            wanted: None,
            sessions,
            panel: None,
            panel_item: None,
            panel_picking: false,
            panel_project: None,
            panel_search,
            panel_focus: cx.focus_handle(),
            show_finished: false,
            list_scroll: gpui::ScrollHandle::new(),
            reveal: false,
            texts: HashMap::new(),
            text_hits: Default::default(),
            layout,
            range: opening_range(layout, today),
            anchor: None,
            hover: None,
            clock,
            today,
            dated: Rc::default(),
            idle_days: settings["idle_days"]
                .as_i64()
                .filter(|d| tokens::IDLE_DAYS.contains(d))
                .unwrap_or(tokens::IDLE_DEFAULT),
            list_w: settings["list_width"].as_f64().map(|w| w as f32),
            list_drawn: Rc::new(std::cell::Cell::new((0., tokens::LIST_W))),
            every: layout == Layout::Time,
            chart_folded: settings["chart_folded"].as_bool().unwrap_or(false),
            state: None,
            moved: Moved::Opened,
            timeline: Rc::default(),
            movement: Movement::default(),
            project_counts: Vec::new(),
            state_counts: [0; 5],
            rows_scroll: gpui::UniformListScrollHandle::new(),
            counted: (None, false, std::ptr::null()),
            moved_for: (std::ptr::null(), None),
        }
    }

    /// Reads the newest snapshot waiting, off the window's thread: a node
    /// with thousands of items sends megabytes, and a snapshot that comes
    /// while one is read replaces any older one still waiting.
    fn read_pending(&mut self, cx: &mut Context<Self>) {
        if self.reading {
            return;
        }
        let Some((node, body)) = self.pending.pop_first() else {
            return;
        };
        self.reading = true;
        let others: Vec<(String, Arc<Snapshot>)> = self
            .snapshots
            .iter()
            .filter(|(n, _)| **n != node)
            .map(|(n, s)| (n.clone(), s.clone()))
            .collect();
        let snapshot_node = node.clone();
        let read = cx.background_spawn(async move {
            let snapshot = Arc::new(serde_json::from_slice::<Snapshot>(&body)?);
            let mut all: BTreeMap<String, Arc<Snapshot>> = others.into_iter().collect();
            all.insert(snapshot_node, snapshot.clone());
            Ok::<_, serde_json::Error>((snapshot, Board::from_snapshot(&combined(&all))))
        });
        cx.spawn(async move |this, cx| {
            let read = read.await;
            let _ = this.update(cx, |w, cx| {
                w.reading = false;
                // A node gone meanwhile stays gone.
                if !w.gone.contains(&node) {
                    match read {
                        Ok((snapshot, board)) => {
                            if w.refreshing.get(&node).is_some_and(|&at| snapshot.refreshed > at) {
                                w.refreshing.remove(&node);
                            }
                            w.frame_errors.remove(&node);
                            w.snapshots.insert(node, snapshot);
                            w.set_board(board, cx);
                        }
                        Err(error) => {
                            log::warn!("Issue snapshot from {node} did not read: {error}");
                            w.frame_errors.insert(node, format!(
                                "The node sent issue data this app cannot read: {error}. Update the app."
                            ));
                            cx.notify();
                        }
                    }
                }
                w.read_pending(cx);
            });
        })
        .detach();
    }

    /// The board again from the snapshots held, at once.
    fn rebuild(&mut self, cx: &mut Context<Self>) {
        let board = Board::from_snapshot(&combined(&self.snapshots));
        self.set_board(board, cx);
    }

    fn set_board(&mut self, board: Board, cx: &mut Context<Self>) {
        self.dated = Rc::new(timeline::dated(&board, &self.clock));
        self.board = Rc::new(board);
        self.today = self.clock.day(ahjo_app_kit::hub::now_ms());
        if !self.text_hits.0.is_empty() {
            self.search_text(&self.text_hits.0.clone(), cx);
        }
        self.settle();
        self.relist(cx);
        self.open_wanted(cx);
        cx.notify();
    }

    /// Project indices for a node in the combined board, which is built in
    /// the same sorted node order as the snapshot map.
    fn projects_on(&self, node: &str) -> Option<std::ops::Range<usize>> {
        let mut first = 0;
        for (id, snapshot) in &self.snapshots {
            let end = first + snapshot.projects.len();
            if id == node {
                return Some(first..end);
            }
            first = end;
        }
        None
    }
    fn node_for_project(&self, project: usize) -> Option<String> {
        self.snapshots
            .keys()
            .find(|node| self.projects_on(node).is_some_and(|r| r.contains(&project)))
            .cloned()
    }
    fn find_on(&self, reference: &str, node: Option<&str>) -> Option<usize> {
        if let Some(node) = node {
            let projects = self.projects_on(node)?;
            if let Some((index, _)) = self
                .board
                .rows
                .iter()
                .enumerate()
                .find(|(_, r)| projects.contains(&r.project) && r.reference == reference)
            {
                return Some(index);
            }
            let mut matches = self
                .board
                .rows
                .iter()
                .enumerate()
                .filter(|(_, r)| projects.contains(&r.project) && r.id == reference);
            let first = matches.next()?.0;
            return matches.next().is_none().then_some(first);
        }
        self.board.find(reference)
    }
    fn open_wanted(&mut self, cx: &mut Context<Self>) {
        let found = self
            .wanted
            .as_ref()
            .and_then(|(r, n)| self.find_on(r, n.as_deref()));
        if let Some(row) = found {
            self.wanted = None;
            self.open(row, cx);
        }
    }
    fn refresh(&mut self, cx: &mut Context<Self>) {
        // A text that could not be read is tried again.
        self.texts
            .retain(|_, (_, text)| !matches!(text, text::Text::Failed(_)));
        for node in self.host.nodes(cx) {
            let done = self
                .snapshots
                .get(node.id.as_str())
                .map_or(0, |s| s.refreshed);
            self.refreshing.insert(node.id.as_str().into(), done);
            self.host.send(&node.id, vec![WATCH, 2], cx);
        }
        cx.notify();
    }

    /// The project of the session in the focused terminal, by its folder.
    fn session_project(&self, cx: &App) -> Option<usize> {
        let list = self.sessions.as_ref()?.read(cx);
        let focused = list.focused.as_ref()?;
        let session = list.sessions.iter().find(|s| {
            s.node == focused.node
                && s.name == focused.name
                && s.user.as_deref().unwrap_or("") == focused.user
        })?;
        outline::project_of_folder(&self.board, session.cwd.as_deref()?)
    }

    /// Once every project has been read (or could not be), names the one
    /// whose open work moved last; it stays for as long as the app runs.
    fn settle(&mut self) {
        let projects = &self.board.projects;
        if self.settled.is_some()
            || projects.is_empty()
            || !projects.iter().all(|p| p.read || p.error.is_some())
        {
            return;
        }
        self.settled = (0..self.board.rows.len())
            .filter(|&i| outline::open(&self.board, i))
            .max_by_key(|&i| self.board.rows[i].updated)
            .map(|i| projects[self.board.rows[i].project].path.clone());
    }

    fn project_at(&self, path: Option<&String>) -> Option<usize> {
        path.and_then(|p| self.board.projects.iter().position(|x| &x.path == p))
    }

    /// The project the page shows: the one chosen last, else the focused
    /// session's, else the one whose work moved last (none until every
    /// project has been read).
    pub fn project(&self, cx: &App) -> Option<usize> {
        self.project_at(self.project.as_ref())
            .or_else(|| self.session_project(cx))
            .or_else(|| self.project_at(self.settled.as_ref()))
    }

    /// The project beside a terminal: the one chosen in the panel since
    /// it opened, else the focused session's, else the page's.
    fn panel_project(&self, cx: &App) -> Option<usize> {
        self.project_at(self.panel_project.as_ref())
            .or_else(|| self.session_project(cx))
            .or_else(|| self.project(cx))
    }

    /// Chooses a project on the page, and keeps the choice.
    pub fn choose_project(&mut self, project: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.keep_project(project, cx);
        window.focus(&self.focus, cx);
    }

    /// The chosen project becomes the page's, kept for the next run.
    fn keep_project(&mut self, project: usize, cx: &mut Context<Self>) {
        let Some(path) = self.board.projects.get(project).map(|p| p.path.clone()) else {
            return;
        };
        self.keep("project", serde_json::json!(path), cx);
        self.project = Some(path);
        self.stop_picking(cx);
        self.selected = None;
        self.selected_node = None;
        self.trail.clear();
        self.relist(cx);
        cx.notify();
    }

    pub fn start_picking(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.picking = true;
        self.picked = self
            .project(cx)
            .and_then(|p| self.projects_listed(cx).iter().position(|&x| x == p))
            .unwrap_or(0);
        self.search.update(cx, |f, cx| {
            f.set_placeholder(PICK);
            f.set("", false, cx);
        });
        field::focus(&self.search, window, cx);
        self.reveal = true;
        cx.notify();
    }

    fn stop_picking(&mut self, cx: &mut Context<Self>) {
        if !self.picking {
            return;
        }
        self.picking = false;
        let query = self.query.clone();
        self.search.update(cx, |f, cx| {
            f.set_placeholder(SEARCH);
            f.set(&query, false, cx);
        });
    }

    /// The projects to choose from, most open work first, those the typed
    /// name matches.
    pub fn projects_listed(&self, cx: &App) -> Vec<usize> {
        let needle = self.search.read(cx).text.trim().to_lowercase();
        self.projects_named(&needle)
    }

    /// Every project `needle` names (by its name or its folder, a slipped
    /// letter forgiven), the best match first, then most open work.
    fn projects_named(&self, needle: &str) -> Vec<usize> {
        let open = outline::open_counts(&self.board);
        let projects = &self.board.projects;
        let mut found = fuzzy::filter(needle, projects, |info| {
            vec![info.label.clone(), info.path.clone()]
        });
        found.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| open[b.index].cmp(&open[a.index]))
                .then_with(|| projects[a.index].label.cmp(&projects[b.index].label))
        });
        found.into_iter().map(|f| f.index).collect()
    }

    /// What the list holds now: a search's matches, else what the layout
    /// lists; and, for the strip and the time page, the chart and the
    /// counts beside it.
    fn relist(&mut self, cx: &App) {
        let project = self.project(cx);
        if self.layout != Layout::Tree {
            self.recount(project);
        }
        self.entries = if !self.query.trim().is_empty() {
            self.found(project)
        } else {
            match self.layout {
                Layout::Tree => project.map_or_else(Vec::new, |p| self.tree_of(p)),
                Layout::Strip => self.strip_entries(project),
                Layout::Time => {
                    let rows = match self.moved {
                        Moved::Opened => &self.movement.opened,
                        Moved::Closed => &self.movement.closed,
                        Moved::Idle => &self.movement.idle,
                    };
                    rows.iter()
                        .map(|&row| Entry::Item {
                            row,
                            depth: 0,
                            away: self.every || Some(self.board.rows[row].project) != project,
                        })
                        .collect()
                }
            }
        };
    }

    fn tree_of(&self, p: usize) -> Vec<Entry> {
        outline::tree(&self.board, p)
            .into_iter()
            .map(|Line { row, depth }| Entry::Item {
                row,
                depth,
                away: self.board.rows[row].project != p,
            })
            .collect()
    }

    fn found(&self, project: Option<usize>) -> Vec<Entry> {
        let Found {
            here,
            elsewhere,
            more,
        } = outline::search(
            &self.board,
            project,
            &self.query,
            self.hits_for(&self.query),
        );
        let mut entries: Vec<Entry> = here
            .into_iter()
            .map(|row| Entry::Item {
                row,
                depth: 0,
                away: false,
            })
            .collect();
        if !elsewhere.is_empty() {
            entries.push(Entry::Label("Other projects".into()));
            entries.extend(elsewhere.into_iter().map(|row| Entry::Item {
                row,
                depth: 0,
                away: true,
            }));
        }
        if more > 0 {
            entries.push(Entry::More(more));
        }
        entries
    }

    /// The rows in view on the strip and the time page: the chosen
    /// project's, or every project's.
    fn scoped(&self, project: Option<usize>) -> Vec<usize> {
        (0..self.board.rows.len())
            .filter(|&i| self.every || Some(self.board.rows[i].project) == project)
            .collect()
    }

    /// The chart, the range's movement and the facets' counts, again.
    fn recount(&mut self, project: Option<usize>) {
        let rows = self.scoped(project);
        let days = match self.layout {
            Layout::Time => tokens::CHART_DAYS,
            _ => tokens::STRIP_DAYS,
        };
        if self.timeline.first + self.timeline.opened.len() as i64 - 1 != self.today
            || self.timeline.opened.len() != days
            || self.counted != (project, self.every, Rc::as_ptr(&self.board))
        {
            self.timeline = Rc::new(Timeline::of(&self.dated, &rows, self.today, days));
            self.counted = (project, self.every, Rc::as_ptr(&self.board));
        }
        self.movement = match self.range {
            Some(range) => {
                timeline::movement(&self.board, &self.dated, &rows, range, self.idle_days)
            }
            None => Movement::default(),
        };
        // New days or another scope: the time page lists the first of the
        // range's movements that holds anything, rather than an empty one.
        let moved = (Rc::as_ptr(&self.timeline), self.range);
        if self.layout == Layout::Time && moved != self.moved_for {
            self.moved_for = moved;
            let m = &self.movement;
            let held = |which: Moved| match which {
                Moved::Opened => !m.opened.is_empty(),
                Moved::Closed => !m.closed.is_empty(),
                Moved::Idle => !m.idle.is_empty(),
            };
            if !held(self.moved)
                && let Some(first) = [Moved::Opened, Moved::Closed, Moved::Idle]
                    .into_iter()
                    .find(|&w| held(w))
            {
                self.moved = first;
            }
        }
        // What counts: work moved in the range, else open work; a project
        // counts only work in the state chosen, a state only the work in
        // view.
        let counts = |i: usize| match self.range {
            Some(range) => timeline::touched(&self.dated[i], range),
            None => self.state.is_some() || outline::open(&self.board, i),
        };
        self.project_counts = vec![0; self.board.projects.len()];
        for (i, r) in self.board.rows.iter().enumerate() {
            if self.state.is_none_or(|s| r.status == s) && counts(i) {
                self.project_counts[r.project] += 1;
            }
        }
        self.state_counts = [0; 5];
        for &i in &rows {
            if self
                .range
                .is_none_or(|range| timeline::touched(&self.dated[i], range))
            {
                self.state_counts[self.board.rows[i].status as usize] += 1;
            }
        }
    }

    /// The strip's list: with a range or a state chosen, the work in view
    /// that matches, latest move first; else the open work as trees, the
    /// chosen project's or every project's under its name.
    fn strip_entries(&self, project: Option<usize>) -> Vec<Entry> {
        if self.range.is_none() && self.state.is_none() {
            if !self.every {
                return project.map_or_else(Vec::new, |p| self.tree_of(p));
            }
            let mut out = Vec::new();
            for p in self.projects_named("") {
                if self.project_counts.get(p).copied().unwrap_or(0) == 0 {
                    continue;
                }
                out.push(Entry::Label(self.board.projects[p].label.clone()));
                out.extend(self.tree_of(p));
            }
            return out;
        }
        let mut rows: Vec<usize> = self
            .scoped(project)
            .into_iter()
            .filter(|&i| {
                let r = &self.board.rows[i];
                self.state.is_none_or(|s| r.status == s)
                    && self
                        .range
                        .is_none_or(|range| timeline::touched(&self.dated[i], range))
            })
            .collect();
        let last = |i: usize| {
            let r = &self.board.rows[i];
            r.closed.max(r.updated).max(r.created)
        };
        rows.sort_by_key(|&i| std::cmp::Reverse(last(i)));
        rows.into_iter()
            .map(|row| Entry::Item {
                row,
                depth: 0,
                away: self.every,
            })
            .collect()
    }

    /// Uses another layout, kept when `keep`.
    pub fn set_layout(&mut self, layout: Layout, keep: bool, cx: &mut Context<Self>) {
        self.layout = layout;
        if layout == Layout::Time && self.range.is_none() {
            self.range = opening_range(layout, self.today);
        }
        if keep {
            self.keep("layout", serde_json::json!(layout.key()), cx);
        }
        self.relist(cx);
        cx.notify();
    }

    /// A press on the chart: the range starts at that day. A press on the
    /// one day already chosen lets the range go.
    pub fn range_press(&mut self, day: Day, cx: &mut Context<Self>) {
        if self.range == Some(Range::between(day, day)) {
            self.range = None;
            self.anchor = None;
        } else {
            self.anchor = Some(day);
            self.range = Some(Range::between(day, day));
        }
        self.range_changed(cx);
    }

    /// The mouse moved: over the chart at `over`, or dragging at `held`.
    pub fn range_move(&mut self, over: Option<Day>, held: Option<Day>, cx: &mut Context<Self>) {
        let mut changed = false;
        if let (Some(anchor), Some(day)) = (self.anchor, held) {
            let range = Some(Range::between(anchor, day));
            if range != self.range {
                self.range = range;
                self.range_changed(cx);
                changed = true;
            }
        }
        if over != self.hover {
            self.hover = over;
            if !changed {
                cx.notify();
            }
        }
    }

    pub fn range_release(&mut self, cx: &mut Context<Self>) {
        if self.anchor.take().is_some() {
            cx.notify();
        }
    }

    pub fn clear_range(&mut self, cx: &mut Context<Self>) {
        if self.range.take().is_some() {
            self.range_changed(cx);
        }
    }

    fn range_changed(&mut self, cx: &mut Context<Self>) {
        self.relist(cx);
        self.rows_scroll
            .scroll_to_item(0, gpui::ScrollStrategy::Top);
        cx.notify();
    }

    /// The strip's and time page's scope: every project, or the one chosen.
    pub fn show_every(&mut self, every: bool, cx: &mut Context<Self>) {
        self.every = every;
        self.relist(cx);
        cx.notify();
    }

    pub fn show_state(&mut self, state: Option<Status>, cx: &mut Context<Self>) {
        self.state = state;
        self.relist(cx);
        cx.notify();
    }

    /// How many days untouched makes open work idle; kept.
    pub fn set_idle_days(&mut self, days: i64, cx: &mut Context<Self>) {
        self.idle_days = days;
        self.keep("idle_days", serde_json::json!(days), cx);
        self.moved = Moved::Idle;
        self.selected = None;
        self.relist(cx);
        cx.notify();
    }

    /// The list's edge dragged to `width`, held between its least and the
    /// most it may take; kept when the drag ends.
    pub fn drag_list(&mut self, width: f32, cx: &mut Context<Self>) {
        let width = width.clamp(tokens::LIST_MIN, tokens::LIST_MAX);
        if Some(width) != self.list_w {
            self.list_w = Some(width);
            cx.notify();
        }
    }

    /// Folds the strip's chart away to one line, or shows it; kept.
    pub fn fold_chart(&mut self, folded: bool, cx: &mut Context<Self>) {
        self.chart_folded = folded;
        self.keep("chart_folded", serde_json::json!(folded), cx);
        cx.notify();
    }

    /// The list's width as it was until dragged: a share of the room.
    pub fn reset_list(&mut self, cx: &mut Context<Self>) {
        self.list_w = None;
        self.keep("list_width", serde_json::Value::Null, cx);
        cx.notify();
    }

    pub fn keep_list(&mut self, cx: &mut Context<Self>) {
        if let Some(w) = self.list_w {
            self.keep("list_width", serde_json::json!(w.round()), cx);
        }
    }

    /// One of the page's settings, kept for the next run.
    fn keep(&self, key: &str, value: serde_json::Value, cx: &mut Context<Self>) {
        let mut settings: serde_json::Value = self.host.setting(cx).unwrap_or_default();
        if !settings.is_object() {
            settings = serde_json::json!({});
        }
        settings[key] = value;
        if let Err(e) = self.host.set_setting(&settings, cx) {
            self.host.toast(&e.message, cx);
        }
    }

    pub fn show_moved(&mut self, moved: Moved, cx: &mut Context<Self>) {
        self.moved = moved;
        self.selected = None;
        self.relist(cx);
        self.rows_scroll
            .scroll_to_item(0, gpui::ScrollStrategy::Top);
        cx.notify();
    }

    /// The rows the list shows, in order.
    pub fn listed(&self) -> Vec<usize> {
        self.entries
            .iter()
            .filter_map(|e| match e {
                Entry::Item { row, .. } => Some(*row),
                _ => None,
            })
            .collect()
    }

    /// The selected row's index in the board, if it is still there.
    pub fn selected_row(&self) -> Option<usize> {
        let key = self.selected.as_ref()?;
        let node = self.selected_node.as_deref();
        let range = match node {
            Some(n) => Some(self.projects_on(n)?),
            None => None,
        };
        self.board.rows.iter().position(|row| {
            range.as_ref().is_none_or(|r| r.contains(&row.project))
                && row.reference == key.reference
                && row.pull.is_some() == key.pull
        })
    }

    /// The row the item view shows: the one chosen, else the list's first;
    /// none while a search matches nothing.
    pub fn shown_row(&self) -> Option<usize> {
        let listed = self.listed();
        if listed.is_empty() && !self.query.trim().is_empty() {
            return None;
        }
        self.selected_row().or_else(|| listed.first().copied())
    }

    pub fn select(&mut self, row: usize, cx: &mut Context<Self>) {
        self.trail.clear();
        self.select_keeping_trail(row, cx);
    }

    fn select_keeping_trail(&mut self, row: usize, cx: &mut Context<Self>) {
        let selected = self.board.rows.get(row).map(RowKey::of);
        let selected_node = self
            .board
            .rows
            .get(row)
            .and_then(|r| self.node_for_project(r.project));
        if selected != self.selected || selected_node != self.selected_node {
            self.prose.borrow_mut().selection.clear();
            self.show_finished = false;
        }
        self.selected = selected;
        self.selected_node = selected_node;
        cx.notify();
    }

    /// Shows a row reached from anywhere: an item of another project
    /// shows without changing the project.
    pub fn open(&mut self, row: usize, cx: &mut Context<Self>) {
        if self.board.rows.get(row).is_none() {
            return;
        }
        self.select(row, cx);
    }

    fn step(&mut self, by: isize, cx: &mut Context<Self>) {
        let listed = self.listed();
        if listed.is_empty() {
            return;
        }
        let n = listed.len() as isize;
        // The row shown is where the walk starts, chosen or not.
        let at = match self
            .shown_row()
            .and_then(|r| listed.iter().position(|&x| x == r))
        {
            Some(c) => (c as isize + by).clamp(0, n - 1),
            None => 0,
        };
        self.select(listed[at as usize], cx);
        self.reveal = true;
    }

    fn search_event(&mut self, ev: FieldEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.picking {
            let n = self.projects_listed(cx).len();
            match ev {
                FieldEvent::Changed => self.picked = 0,
                FieldEvent::Up => {
                    self.picked = self.picked.saturating_sub(1);
                    self.reveal = true;
                }
                FieldEvent::Down => {
                    self.picked = (self.picked + 1).min(n.saturating_sub(1));
                    self.reveal = true;
                }
                FieldEvent::Submit => {
                    if let Some(&p) = self.projects_listed(cx).get(self.picked) {
                        self.choose_project(p, window, cx);
                    }
                }
                _ => {}
            }
            cx.notify();
            return;
        }
        match ev {
            // Typing narrows the list; the item in hand stays until a
            // match is moved to, clicked or taken with Enter.
            FieldEvent::Changed => {
                let held = self.shown_row();
                self.query = self.search.read(cx).text.clone();
                self.search_text(&self.query.clone(), cx);
                self.relist(cx);
                if let Some(row) = held.filter(|_| self.selected_row().is_none()) {
                    self.select(row, cx);
                }
            }
            FieldEvent::Down | FieldEvent::Submit => {
                if let Some(&first) = self.listed().first() {
                    self.select(first, cx);
                    self.reveal = true;
                }
                window.focus(&self.focus, cx);
            }
            _ => {}
        }
        cx.notify();
    }

    fn clear_search(&mut self, cx: &mut Context<Self>) {
        self.query.clear();
        self.search_text("", cx);
        self.search.update(cx, |f, cx| f.set("", false, cx));
        self.relist(cx);
    }

    /// Esc in the page's search: the project choice ends, else the search
    /// clears, else the keys go back to the list. Whether it was used.
    pub fn escape_search(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.search.read(cx).is_focused(window) {
            return false;
        }
        if self.picking {
            self.stop_picking(cx);
        } else if !self.query.is_empty() {
            self.clear_search(cx);
            return true;
        }
        window.focus(&self.focus, cx);
        cx.notify();
        true
    }

    /// The list's keys: ↑↓ or j/k walk it, `/` searches, `p` chooses the
    /// project, Esc clears the search; Alt+S hands over and Alt+Left goes
    /// back along the trail.
    pub fn key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &e.keystroke;
        let m = k.modifiers;
        if self.alt_key(e, window, cx) {
            return;
        }
        if m.control && !m.alt && k.key == "c" && self.copy_prose(cx) {
            cx.stop_propagation();
            return;
        }
        if m.alt || m.control || m.platform {
            return;
        }
        let handled = match k.key.as_str() {
            "up" | "k" => {
                self.step(-1, cx);
                true
            }
            "down" | "j" => {
                self.step(1, cx);
                true
            }
            "escape" if !self.query.is_empty() => {
                self.clear_search(cx);
                true
            }
            "escape" if self.range.is_some() && self.layout != Layout::Tree => {
                self.clear_range(cx);
                true
            }
            "v" => {
                self.set_layout(self.layout.next(), true, cx);
                true
            }
            "/" => {
                field::focus(&self.search, window, cx);
                true
            }
            "p" => {
                self.start_picking(window, cx);
                true
            }
            "c" if self.layout == Layout::Strip => {
                self.fold_chart(!self.chart_folded, cx);
                true
            }
            _ => false,
        };
        if handled {
            cx.stop_propagation();
            cx.notify();
        }
    }

    /// The page's Alt keys, wherever focus is on it: Alt+S sends, Alt+Left
    /// goes back. Whether the key was one.
    pub fn alt_key(
        &mut self,
        e: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let k = &e.keystroke;
        let m = k.modifiers;
        if !m.alt || m.control || m.platform || m.shift {
            return false;
        }
        match k.key.as_str() {
            "s" => self.send(window, cx),
            "left" => self.back(cx),
            _ => return false,
        }
        cx.stop_propagation();
        true
    }

    /// Shows a row reached from the item in hand, which joins the trail.
    pub fn follow_row(&mut self, row: usize, cx: &mut Context<Self>) {
        let from = self
            .shown_row()
            .map(|r| (self.selected_node.clone(), RowKey::of(&self.board.rows[r])));
        let mut trail = std::mem::take(&mut self.trail);
        self.select_keeping_trail(row, cx);
        if let Some(from) = from
            && (self.selected.as_ref() != Some(&from.1) || self.selected_node != from.0)
        {
            trail.push(from);
        }
        self.trail = trail;
    }

    /// Back along the trail to the item the last link was followed from;
    /// nothing with nothing behind. An item gone from the board is passed.
    pub fn back(&mut self, cx: &mut Context<Self>) {
        while let Some((node, key)) = self.trail.pop() {
            if let Some(row) = self.find_on(&key.reference, node.as_deref()) {
                self.select_keeping_trail(row, cx);
                cx.notify();
                return;
            }
        }
    }

    pub(crate) fn item_menu(&self, row: usize) -> Vec<ahjo_app_kit::menu::MenuRow> {
        use ahjo_app_kit::menu::MenuRow;
        let r = &self.board.rows[row];
        let id = r.written_id(&self.board);
        let title = format!("{id}: {}", r.title);
        let mut rows = Vec::new();
        if self.board.projects[r.project].checkout.is_some() {
            let me = self.me.clone();
            rows.push(MenuRow::new("Start a session on it", move |_, cx| {
                let _ = me.update(cx, |w, cx| w.start_session_for(row, cx));
            }));
        }
        rows.extend([
            MenuRow::new("Copy id", move |_, cx| {
                ahjo_app_kit::widgets::select::copy(id.clone(), cx)
            }),
            MenuRow::new("Copy id and title", move |_, cx| {
                ahjo_app_kit::widgets::select::copy(title.clone(), cx)
            }),
        ]);
        if let Some(url) = r.url.clone() {
            rows.push(MenuRow::new("Open on GitHub", move |_, cx| {
                cx.open_url(&url)
            }));
        }
        rows
    }
    fn copy_prose(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(text) = self.prose.borrow().copied() else {
            return false;
        };
        ahjo_app_kit::widgets::select::copy(text, cx);
        true
    }
    /// The line that names the item shown to an agent.
    pub fn line(&self) -> Option<String> {
        Some(self.line_of(self.shown_row()?))
    }
    /// The line that names a row to an agent: the node's own words for the
    /// item it was asked for, else the board's.
    pub fn line_of(&self, row: usize) -> String {
        let item = &self.board.rows[row];
        self.card_line
            .as_ref()
            .filter(|(reference, _)| reference == &item.reference)
            .map(|(_, line)| line.clone())
            .unwrap_or_else(|| item.line(&self.board))
    }
    pub fn send(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(line) = self.line() {
            // The terminals choose the target by their own rule (the
            // terminal this item sits beside, else the last one sent to),
            // never by guessing from the item's folder. Issue stays shown,
            // with Send in the panel beside it.
            let _ = self.host.ask(
                Ask::Send {
                    reference: line,
                    target: None,
                },
                cx,
            );
        }
    }
    /// Asks the Sessions page to start a session in the shown item's
    /// checkout.
    pub fn start_session(&mut self, cx: &mut Context<Self>) {
        if let Some(row) = self.shown_row() {
            self.start_session_for(row, cx);
        }
    }
    /// Asks the Sessions page to start a session in a row's checkout.
    pub fn start_session_for(&mut self, row: usize, cx: &mut Context<Self>) {
        let item = &self.board.rows[row];
        let project = &self.board.projects[item.project];
        let (Some(folder), Some(node)) = (
            project.checkout.clone(),
            self.node_for_project(item.project),
        ) else {
            return;
        };
        let _ = self.host.ask(
            Ask::Launch {
                node: NodeId(node),
                folder,
                name: session_name(&project.label, &item.id),
            },
            cx,
        );
    }

    /// A link in an item's text: a Bead id shows that Bead, `#n` its issue
    /// or pull request (or GitHub's page when not read), anything else
    /// opens in the browser.
    pub fn follow_link(&mut self, link: &str, project: usize, cx: &mut Context<Self>) {
        if let Some(id) = link.strip_prefix(prose::BEAD_LINK) {
            if let Some(row) = self.board.find_from(id, project) {
                self.follow_row(row, cx);
            }
        } else if let Some(n) = link.strip_prefix('#').and_then(|n| n.parse::<u32>().ok()) {
            let p = &self.board.projects[project];
            let reference = format!("gh:{}#{n}", p.path);
            match self
                .board
                .find(&reference)
                .or_else(|| self.board.pull(project, n))
            {
                Some(row) => self.follow_row(row, cx),
                None => {
                    if let Some(u) = &p.url {
                        cx.open_url(&format!("{u}/issues/{n}"));
                    }
                }
            }
        } else {
            cx.open_url(link);
        }
    }

    /// A family link: the row if the node read it, else nothing to open.
    pub fn follow(&mut self, reference: &str, cx: &mut Context<Self>) {
        if let Some(row) = self.board.find(reference) {
            self.follow_row(row, cx);
        }
    }
}

/// The range a layout opens on: the time page the last week, the others
/// none.
fn opening_range(layout: Layout, today: Day) -> Option<Range> {
    (layout == Layout::Time).then(|| Range::between(today - tokens::OPENING_RANGE + 1, today))
}

const SEARCH: &str = "Search…";
const PANEL_SEARCH: &str = "Find issues";
const PICK: &str = "Project…";

/// Every node's projects in one snapshot, in node order.
fn combined(snapshots: &BTreeMap<String, Arc<Snapshot>>) -> Snapshot {
    Snapshot {
        refreshed: 0,
        projects: snapshots
            .values()
            .flat_map(|s| s.projects.iter().cloned())
            .collect(),
    }
}

/// Suggest a tmux-safe name; the answering page resolves collisions.
fn session_name(project: &str, id: &str) -> String {
    let short = project.rsplit('/').next().unwrap_or(project);
    let id = id.trim_start_matches('#');
    let raw = if id.starts_with(short) {
        id.to_string()
    } else {
        format!("{short}-{id}")
    };
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .take(48)
        .collect()
}
