//! Page state, file requests and user actions.
mod nav;
use crate::{
    open::{self, Draft, Opened, Show},
    store::{self, Side, find::Find, paths, tree::Tree},
};
use ahjo_app_kit::{
    Answers, Ask, Host, Page,
    frame::Status,
    host::Request,
    link::client::LinkState,
    menu::MenuRow,
    widgets::{
        code::{Code, CodeEvent},
        field::{FieldEvent, TextField},
        inline_edit::{InlineEdit, InlineEditEvent},
        syntax,
    },
};
use ahjo_core::{Error, NodeId, TypedCommand};
use ahjo_files_contract::{
    Changed, FILE_BYTES_PATH, Matches,
    commands::{
        FileCreateFolder, FileDelete, FileDiff, FileList, FileListChanges, FileListRecent,
        FileMove, FileSearch, FileSearchPaths, FileStat, FileWrite, ListContent,
    },
};
use gpui::{
    App, AppContext, Context, Entity, FocusHandle, KeyDownEvent, UniformListScrollHandle,
    WeakEntity, Window,
};
pub use nav::{InFile, Jump, Spot};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::time::Duration;

/// How often a shown page asks whether what it shows changed on disk.
const WATCH: Duration = Duration::from_secs(2);
/// Files' content in the window's panel.
const PANEL: &str = "files";
/// Changes seen on disk ask for recent changes again at most this often.
const RECENT_AGAIN: Duration = Duration::from_secs(60);
/// A pause in typing this long sends the find.
const TYPED: Duration = Duration::from_millis(90);

pub(crate) enum Rename {
    From(String),
    To(String),
}

struct Upload {
    source: PathBuf,
    node: NodeId,
    folder: String,
}
struct UploadConflict {
    upload: Upload,
    name: String,
    bytes: Vec<u8>,
}

/// What the name field is open for, and where.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Naming {
    File(String),
    Folder(String),
    Rename(String),
}
impl Naming {
    pub fn verb(&self) -> &'static str {
        match self {
            Naming::File(_) | Naming::Folder(_) => "Create",
            Naming::Rename(_) => "Rename",
        }
    }
    fn placeholder(&self) -> &'static str {
        match self {
            Naming::File(_) => "Name of the new file",
            Naming::Folder(_) => "Name of the new folder",
            Naming::Rename(_) => "New name",
        }
    }
}

/// Which part of a find an answer is for: the files beside the asker,
/// the names under every root, or the text inside files.
#[derive(Clone, Copy)]
enum Part {
    Beside,
    Names,
    Inside,
}

/// Where a rename edits the name: the list's line it was asked from, or
/// the open file's name over the viewer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenameIn {
    Line(Pick),
    Viewer,
}

/// The row the keyboard is on in the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pick {
    Recent(String),
    More,
    Node(String),
    Hit(usize),
    InText,
    /// Review: a folder where things changed, and who works there.
    Group(String),
    /// The find going to a line of the open file, or to a symbol (by its
    /// line).
    Go(usize),
    Symbol(usize),
    /// A file that lines inside files were found in.
    HitFile(String),
}

/// What changed, as the node answered, or why it could not say.
pub type Changes = Result<Vec<Changed>, String>;

pub struct FilesPage {
    pub host: Host,
    pub focus: FocusHandle,
    pub tree: Tree,
    /// No node has answered since the app started, and none answers now:
    /// the tree says so instead of reading. Kept as each draw begins.
    pub no_node: bool,
    pub query: Entity<TextField>,
    pub find: Option<Find>,
    find_revision: u64,
    pub pick: Option<Pick>,
    pub recent: Option<Changes>,
    pub recent_more: bool,
    /// The Recent folder line opened to the files changed in it; none
    /// while the node is looking.
    pub recent_inside: Option<(String, Option<Changes>)>,
    recent_asked: bool,
    recent_at: Option<std::time::Instant>,
    pub opened: Option<Opened>,
    pub code: Entity<Code>,
    pub pretty_code: Entity<Code>,
    pub room: crate::picture::Room,
    pending_line: Option<usize>,
    /// The column on [`Self::pending_line`] a printed `path:12:5` names.
    pending_column: Option<usize>,
    pub drafts: HashMap<(String, String), Draft>,
    pub naming: Option<Naming>,
    /// Where the name being renamed is edited.
    pub naming_in: Option<RenameIn>,
    /// What the menu open now was opened from.
    menu_from: Option<RenameIn>,
    /// A new file's or folder's name.
    pub name_field: Entity<TextField>,
    /// A name renamed where it is shown.
    pub name_edit: Entity<InlineEdit>,
    pub confirm_discard: bool,
    pub conflict: bool,
    copy_ordinal: usize,
    /// The panel asked to close with unsaved edits: what to do with them.
    pub confirm_close_panel: bool,
    pub drawn: Vec<std::sync::Arc<gpui::RenderImage>>,
    /// The pictures the open Markdown shows, by node, the Markdown's path
    /// and the picture's, read once while it is open.
    pub md_pictures: std::cell::RefCell<HashMap<[String; 3], crate::picture::Inline>>,
    /// The opened file's header and the room its name has there, and the
    /// list's width, as last laid out: names shorten in the middle to fit.
    pub(crate) head_room: crate::view::Width,
    pub(crate) name_room: crate::view::Width,
    pub(crate) list_room: crate::view::Width,
    pub list_scroll: UniformListScrollHandle,
    /// A folder Down was pressed on before its contents came: the pick
    /// steps into it when they do.
    step_due: Option<String>,
    /// A row to scroll to once its folder is read.
    scroll_pending: bool,
    /// The pick is a folder shown for what is in it: it scrolls to the
    /// list's top, its rows under it.
    pick_at_top: bool,
    /// The first Recent arrived with no file open: the newest opens at the
    /// next draw, where there is a window to open it in.
    pub(crate) newest_due: bool,
    /// The keyboard goes to the file once it has been read and shows.
    pub(crate) focus_file_due: bool,
    /// The find looks inside the files as well as at their names
    /// (Ctrl+Shift+F), until it ends.
    pub(crate) search_inside: bool,
    /// The open file is the newest, shown unasked: not kept as the file
    /// to come back to.
    shown_newest: bool,
    /// The pick, the list's place and the file open when a find began, to
    /// go back to.
    before_find: Option<(Option<Pick>, gpui::Point<gpui::Pixels>, Option<String>)>,
    /// A path to open the tree down to once the roots are known.
    reveal_pending: Option<String>,
    /// Folders asked for and not answered yet.
    listing: std::collections::HashSet<String>,
    /// This is the window's Files panel, not the page.
    pub is_panel: bool,
    pub shown: bool,
    /// The page's: the Files panel it lends the other pages.
    pub panel: Option<Entity<FilesPage>>,
    restored: bool,
    pub(crate) me: WeakEntity<FilesPage>,
    pub(crate) sessions: Option<Entity<ahjo_terminals_contract::SessionList>>,
    /// The sessions as last told, for the views that group by them.
    pub(crate) live: Vec<ahjo_terminals_contract::Session>,
    uploads: VecDeque<Upload>,
    uploading: bool,
    upload_conflict: Option<UploadConflict>,
    /// The watch is running, and when each watched path last changed.
    watching: bool,
    seen: HashMap<String, Option<u64>>,
    /// What the side shows (or that it is hidden), and what it showed
    /// before it was hidden.
    pub side: Side,
    side_before: Side,
    /// Reader: the find lies open over the file.
    pub palette: bool,
    /// Where the reader was before and after now, to go back and forth.
    back: Vec<Spot>,
    forward: Vec<Spot>,
    travelling: bool,
    /// The files open in tabs, and each tab not shown as it was left.
    pub tabs: store::tabs::Tabs,
    tab_kept: HashMap<store::tabs::Key, nav::TabKept>,
    pub tabs_scroll: gpui::ScrollHandle,
    /// The find box has the keyboard or shows what it found: the list
    /// takes the room its lines need. Set as the page is drawn.
    pub(crate) finding: std::cell::Cell<bool>,
    /// The code outline's scroll, and the symbol it last followed the
    /// reader to: it moves only when the reader comes to another symbol.
    pub(crate) outline_scroll: gpui::ScrollHandle,
    pub(crate) outline_here: std::cell::Cell<Option<usize>>,
    /// Ctrl+Tab held: the tab it is on among those shown last.
    pub switching: Option<usize>,
    /// A tab with unsaved changes asked to close: what to do with them.
    pub closing: Option<store::tabs::Key>,
    close_after_save: Option<store::tabs::Key>,
    /// Esc pressed while a save was on its way: editing ends when the save
    /// is answered, unless it failed or more was typed.
    pub(crate) leave_after_save: bool,
    /// A place to go to once the file is read.
    pending_place: Option<ahjo_app_kit::widgets::code::Place>,
    /// The open file's symbols.
    pub symbols: Vec<store::symbols::Symbol>,
    /// The find inside the open file, and its two fields.
    pub infile: Option<InFile>,
    pub infile_query: Entity<TextField>,
    pub infile_with: Entity<TextField>,
    /// The find box going to a line or a symbol instead.
    pub jump: Option<Jump>,
    /// Letters typed in the list to go to a name, and when.
    typed_ahead: (String, Option<std::time::Instant>),
    /// What git says differs from HEAD: by repository, then by path, and
    /// the folders holding any of it.
    pub(crate) repos: HashMap<String, Vec<ahjo_files_contract::FileState>>,
    no_repo: std::collections::HashSet<String>,
    status_asked: std::collections::HashSet<String>,
    pub states: HashMap<String, char>,
    pub(crate) renames: HashMap<String, Rename>,
    pub holding: std::collections::HashSet<String>,
    /// The second group of tabs beside the first, and (in it) the page it
    /// is beside.
    pub split: Option<Entity<FilesPage>>,
    pub split_of: Option<WeakEntity<FilesPage>>,
    /// The diff's list, and whether it is shown inline when there is room
    /// for both sides.
    pub diff_scroll: UniformListScrollHandle,
    /// How far the changes view's lines are scrolled sideways, in pixels.
    pub diff_left: std::cell::Cell<f32>,
    pub diff_inline: bool,
    /// Review and Reader: a folder shown as a place (its sessions, what
    /// changed in it, its documents) instead of a file.
    pub place: Option<String>,
    /// What changed under the place, once read.
    pub place_recent: Option<Changes>,
    /// The rendered Markdown's scroll and where each block was laid out,
    /// for the outline; its words, to be selected.
    pub md_scroll: gpui::ScrollHandle,
    pub md_blocks: std::rc::Rc<std::cell::RefCell<Vec<gpui::Bounds<gpui::Pixels>>>>,
    pub md_select: std::rc::Rc<std::cell::RefCell<ahjo_app_kit::widgets::select::Selectable>>,
    /// A block to scroll to once laid out.
    pub md_go: std::rc::Rc<std::cell::Cell<Option<usize>>>,
    /// The viewer's body as last laid out: room for the outline or not.
    pub(crate) body_room: crate::view::Width,
    /// Review: the file opened goes to its first change once read.
    jump_due: bool,
    /// The place's README, rendered, when it has one.
    pub place_readme: Option<Vec<ahjo_app_kit::widgets::markdown::Block>>,
    /// Ctrl+wheel over the open file, a step not yet taken.
    pub(crate) zoom_wheel: std::rc::Rc<std::cell::Cell<ahjo_app_kit::zoom::Wheel>>,
    /// The web page the open file runs as, when it is one (ADR 0006).
    pub web: ahjo_app_kit::web_view::WebSurface,
    /// The folder that page is served from.
    pub served: crate::live::Live,
    /// The file opening now was not asked for as a page to see: shown at
    /// start or unasked, or asked at a line. A web page does not open in
    /// the system's browser for it.
    unasked: bool,
}

/// The line, and the column if any, a revealed path's part names: `12` or
/// `12:5`, both from 1.
fn line_and_column(part: &str) -> Option<(usize, Option<usize>)> {
    let (line, column) = match part.split_once(':') {
        Some((line, column)) => (line, Some(column.parse().ok()?)),
        None => (part, None),
    };
    Some((line.parse().ok()?, column))
}

fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}
pub(crate) fn bytes_uri(path: &str) -> String {
    format!("{FILE_BYTES_PATH}?path={}", enc(path))
}
fn upload_copy_name(name: &str, ordinal: usize) -> String {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() => {
            format!("{stem} ({ordinal}).{ext}")
        }
        _ => format!("{name} ({ordinal})"),
    }
}
fn message(status: u16, body: &[u8]) -> String {
    serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().map(str::to_owned))
        .unwrap_or_else(|| {
            let said = String::from_utf8_lossy(body);
            match (said.trim(), status) {
                // No route at all: the node runs without its files area.
                ("", 404) => "This node does not serve files.".to_string(),
                ("", _) => format!("The node answered {status} and gave no reason."),
                (said, _) => format!("Request failed ({status}): {said}"),
            }
        })
}
/// Where a relative link from a file in `folder` leads.
pub(crate) fn resolve(folder: &str, link: &str) -> String {
    let mut at = folder.to_owned();
    for part in link.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => at = paths::parent(&at),
            p => at = paths::join(&at, p),
        }
    }
    at
}
fn code_of(body: &[u8]) -> Option<String> {
    serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v["error"]["code"].as_str().map(str::to_owned))
}

impl Page for FilesPage {
    const ID: &'static str = "files";
    const TITLE: &'static str = "Files";
    const KEY: Option<char> = Some('F');
    const AREA: Option<&'static str> = Some("files");
    const ANSWERS: &'static [Answers] = &[Answers::OpenPath, Answers::Reveal("path")];
    const SETTINGS: &'static [ahjo_app_kit::Setting] = &[
        ahjo_app_kit::Setting::text("download_folder", "Download folder", "")
            .note("Leave empty to use Downloads in your home folder."),
        ahjo_app_kit::Setting::choice(
            "side",
            "Side",
            "folders",
            &[
                ("folders", "Folders"),
                ("changes", "Changes"),
                ("hidden", "Hidden"),
            ],
        )
        .note("Beside the open files: the folder tree, what changed by session, or nothing."),
    ];
    fn chords(&self, _: &App) -> Vec<ahjo_app_kit::chords::Chord> {
        let me = self.me.clone();
        vec![
            ahjo_app_kit::chords::Chord::new("o", "Files panel, to find a file", {
                let me = me.clone();
                move |window, cx| {
                    let _ = me.update(cx, |page, cx| page.toggle_panel(true, window, cx));
                }
            })
            .alt("o", Some(false))
            .everywhere(),
            // VS Code's Explorer key; Ctrl+B too, where a terminal does
            // not have the keyboard.
            ahjo_app_kit::chords::Chord::new("e", "Files panel", move |window, cx| {
                let _ = me.update(cx, |page, cx| page.toggle_panel(false, window, cx));
            })
            .alt("e", Some(false))
            .everywhere(),
        ]
    }
    fn focus(&self, _: &App) -> Option<FocusHandle> {
        Some(self.focus.clone())
    }
    fn settings_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let kept: store::Kept = self.host.setting(cx).unwrap_or_default();
        if kept.side != self.side {
            self.side = kept.side;
            self.laid_out(window, cx);
        }
    }
    fn shown(&mut self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.shown = on;
        if on {
            self.start(cx);
            self.refresh_recent(cx);
            self.watch(window, cx);
            self.tab_into_view_soon(window, cx);
        }
    }
    fn link(
        &mut self,
        node: &NodeId,
        state: Option<LinkState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if state != Some(LinkState::Open) {
            if self.tree.node == node.as_str() {
                self.tree.away = true;
            }
            cx.notify();
            return;
        }
        if let Some(view) = &self.panel {
            view.update(cx, |page, cx| {
                page.start(cx);
                page.refresh_recent(cx);
                if let Some(path) = page
                    .opened
                    .as_ref()
                    .filter(|o| o.node == node.as_str() && matches!(o.show, Show::Failed(_)))
                    .map(|o| o.path.clone())
                {
                    page.open_unasked(path, true, window, cx);
                }
            });
        }
        if self.shown {
            self.start(cx);
            self.refresh_recent(cx);
        }
        if !self.restored {
            self.restored = true;
            let kept: store::Kept = self.host.setting(cx).unwrap_or_default();
            let here = self.tree.node.clone();
            for tab in &kept.tabs {
                self.tabs.show((here.clone(), tab.clone()), true, None);
            }
            if let Some(file) = kept.file.clone() {
                self.show_in_tree(&file, cx);
                self.open_unasked(file, true, window, cx);
                self.tab_into_view_soon(window, cx);
            }
            if let (Some(panel), Some(file)) = (&self.panel, kept.panel_file) {
                panel.update(cx, |p, cx| {
                    p.open_unasked(file, true, window, cx);
                    p.watch(window, cx);
                });
            }
        }
    }
    fn answer(&mut self, ask: Ask, _: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        match ask {
            Ask::OpenPath { node, path, beside } => {
                if beside {
                    self.open_in_panel(node, path, None, None, window, cx);
                } else {
                    self.on_node(node.as_str(), cx);
                    self.show_in_tree(&path, cx);
                    self.open(path, window, cx)
                }
            }
            // A file revealed at a part of it, its line and maybe column
            // (a path a session printed as `main.rs:12` or `main.rs:12:5`),
            // opens there.
            Ask::Reveal {
                what,
                id,
                node,
                part: Some(part),
                beside,
            } if what == "path" && line_and_column(&part).is_some() => {
                let (line, column) = line_and_column(&part).expect("checked");
                if beside {
                    let node = node.unwrap_or_else(|| NodeId(self.tree.node.clone()));
                    self.open_in_panel(node, id, Some(line), column, window, cx);
                } else {
                    if let Some(n) = node {
                        self.on_node(n.as_str(), cx);
                    }
                    self.show_in_tree(&id, cx);
                    self.open_at(id, Some(line), column, window, cx);
                }
            }
            Ask::Reveal { what, id, node, .. } if what == "path" => {
                if let Some(n) = node {
                    self.on_node(n.as_str(), cx);
                }
                self.reveal(id, window, cx);
            }
            _ => {}
        }
    }
}

impl FilesPage {
    pub fn new(host: Host, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let kept: store::Kept = host.setting(cx).unwrap_or_default();
        let sessions = host.model::<ahjo_terminals_contract::SessionList>(cx);
        if let Some(sessions) = &sessions {
            cx.observe(sessions, |p: &mut Self, list, cx| {
                p.live = list.read(cx).sessions.clone();
                if p.shown && p.side == Side::Changes {
                    p.ask_session_status(cx);
                }
                cx.notify()
            })
            .detach();
        }
        let wrap = !kept.unwrapped;
        let text = kept.text.map_or(ahjo_app_kit::theme::size::TEXT, f32::from);
        let code = cx.new(|cx| {
            let mut c = Code::new(cx);
            c.set_wrap(wrap, cx);
            c.set_text_size(text, cx);
            c
        });
        let pretty_code = cx.new(|cx| {
            let mut c = Code::new(cx);
            c.set_wrap(wrap, cx);
            c.set_text_size(text, cx);
            c
        });
        let query = cx.new(|cx| TextField::new(cx, "Find a file  (: a line, @ a symbol)"));
        let infile_query = cx.new(|cx| TextField::new(cx, "Find in the file"));
        let infile_with = cx.new(|cx| TextField::new(cx, "Replace with"));
        cx.subscribe_in(
            &infile_query,
            window,
            move |page, _, event, window, cx| match event {
                FieldEvent::Changed => page.refind(true, cx),
                FieldEvent::Submit | FieldEvent::Down => page.infile_step(true, cx),
                FieldEvent::Up => page.infile_step(false, cx),
                FieldEvent::Tab if page.infile.as_ref().is_some_and(|f| f.replace) => {
                    ahjo_app_kit::widgets::field::focus(&page.infile_with, window, cx);
                }
                _ => {}
            },
        )
        .detach();
        cx.subscribe_in(
            &infile_with,
            window,
            move |page, _, event, window, cx| match event {
                FieldEvent::Submit => page.replace_one(window, cx),
                FieldEvent::SubmitAlt => page.replace_every(window, cx),
                _ => {}
            },
        )
        .detach();
        let name_field = cx.new(|cx| TextField::new(cx, "New name"));
        let name_edit = cx.new(|cx| InlineEdit::new("New name", window, cx));
        cx.subscribe_in(
            &code,
            window,
            move |page, _, event, window, cx| match event {
                CodeEvent::Save => page.save(cx),
                CodeEvent::Changed => page.changed(cx),
                CodeEvent::Escape => page.finish_edit(window, cx),
                CodeEvent::Moved => cx.notify(),
            },
        )
        .detach();
        cx.subscribe_in(&query, window, move |page, field, event, window, cx| {
            match event {
                FieldEvent::Changed => {
                    let q = field.read(cx).text.clone();
                    page.typed(q, cx);
                }
                FieldEvent::Submit => {
                    page.activate(window, cx);
                }
                FieldEvent::Down => page.step(1, cx),
                FieldEvent::Up => page.step(-1, cx),
                _ => {}
            }
            cx.notify();
        })
        .detach();
        cx.subscribe_in(&name_field, window, move |page, _, event, window, cx| {
            if matches!(event, FieldEvent::Submit) {
                page.apply_naming(window, cx);
            }
        })
        .detach();
        // A rename keeps what was typed on Enter and on a click elsewhere
        // (the keyboard stays where that click put it); Esc lets it go.
        cx.subscribe_in(&name_edit, window, |page, _, event, window, cx| {
            if !matches!(page.naming, Some(Naming::Rename(_))) {
                return;
            }
            match event {
                InlineEditEvent::Committed(name) => page.give_name(name.trim(), window, cx),
                InlineEditEvent::Reverted => page.stop_naming(window, cx),
            }
        })
        .detach();
        host.guard_close(
            &cx.entity(),
            |page: &FilesPage, _| {
                let mut names: Vec<_> = page
                    .drafts
                    .keys()
                    .map(|(_, path)| paths::base_name(path).to_owned())
                    .collect();
                names.sort();
                names.dedup();
                (!names.is_empty()).then(|| names.join(", "))
            },
            cx,
        );
        Self {
            focus: cx.focus_handle().tab_stop(true),
            host,
            tree: Tree::from_kept(&kept.tree),
            query,
            find: None,
            find_revision: 0,
            pick: None,
            recent: None,
            recent_more: false,
            recent_inside: None,
            recent_asked: false,
            recent_at: None,
            opened: None,
            code,
            pretty_code,
            room: Default::default(),
            pending_line: None,
            pending_column: None,
            drafts: HashMap::new(),
            naming: None,
            naming_in: None,
            menu_from: None,
            name_field,
            name_edit: name_edit.clone(),
            confirm_discard: false,
            conflict: false,
            copy_ordinal: 1,
            confirm_close_panel: false,
            drawn: Vec::new(),
            md_pictures: Default::default(),
            head_room: Default::default(),
            name_room: Default::default(),
            list_room: Default::default(),
            list_scroll: UniformListScrollHandle::new(),
            step_due: None,
            scroll_pending: false,
            pick_at_top: false,
            newest_due: false,
            focus_file_due: false,
            search_inside: false,
            shown_newest: false,
            before_find: None,
            reveal_pending: None,
            listing: Default::default(),
            is_panel: false,
            shown: false,
            panel: None,
            no_node: false,
            restored: false,
            me: cx.weak_entity(),
            live: sessions
                .as_ref()
                .map(|s| s.read(cx).sessions.clone())
                .unwrap_or_default(),
            sessions,
            uploads: VecDeque::new(),
            uploading: false,
            upload_conflict: None,
            watching: false,
            seen: HashMap::new(),
            side: kept.side,
            side_before: if kept.side == Side::Hidden {
                Side::Folders
            } else {
                kept.side
            },
            palette: false,
            back: vec![],
            forward: vec![],
            travelling: false,
            tabs: Default::default(),
            tab_kept: HashMap::new(),
            tabs_scroll: gpui::ScrollHandle::new(),
            finding: std::cell::Cell::new(false),
            outline_scroll: gpui::ScrollHandle::new(),
            outline_here: std::cell::Cell::new(None),
            switching: None,
            closing: None,
            close_after_save: None,
            leave_after_save: false,
            pending_place: None,
            symbols: vec![],
            infile: None,
            infile_query,
            infile_with,
            jump: None,
            typed_ahead: (String::new(), None),
            repos: HashMap::new(),
            no_repo: Default::default(),
            status_asked: Default::default(),
            states: HashMap::new(),
            renames: HashMap::new(),
            holding: Default::default(),
            split: None,
            split_of: None,
            diff_scroll: UniformListScrollHandle::new(),
            diff_left: Default::default(),
            diff_inline: false,
            place: None,
            place_recent: None,
            md_scroll: gpui::ScrollHandle::new(),
            md_blocks: Default::default(),
            md_select: Default::default(),
            md_go: Default::default(),
            body_room: Default::default(),
            zoom_wheel: Default::default(),
            jump_due: false,
            place_readme: None,
            web: Default::default(),
            served: Default::default(),
            unasked: false,
        }
    }

    fn fail(&self, text: String, cx: &mut Context<Self>) {
        self.host.report(Status::failure(text), cx);
    }

    /// The window's Files panel: a Files of its own in one column (the
    /// list, or the file opened from it), which the page lends the other
    /// pages at the edge its setting says.
    pub fn panel(host: Host, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let kept: store::Kept = host.setting(cx).unwrap_or_default();
        let mut panel = Self::new(host, window, cx);
        panel.is_panel = true;
        panel.tree = Tree::from_kept(&kept.panel_tree);
        panel
    }

    /// Alt+E, and Alt+O for its find: the Files panel opens, or is put
    /// away. Opened for its find, it shows the focused session's folder
    /// and the find has the keyboard. On the Files page, whose list holds
    /// the same, Alt+O finds there.
    fn toggle_panel(&mut self, find: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(panel) = self.panel.clone() else {
            return;
        };
        if find && self.host.shown_page(cx) == Some(<Self as Page>::ID) {
            ahjo_app_kit::widgets::field::focus(&self.query, window, cx);
            return;
        }
        if self.host.panel_shows(PANEL, cx) && !find {
            panel.update(cx, |p, cx| p.close_panel(window, cx));
            return;
        }
        // Open with its find in hand, the find's key puts it away; open
        // with the keyboard elsewhere, it takes the keyboard to the find.
        if find && panel.read(cx).seen_now(cx) && panel.read(cx).query.read(cx).is_focused(window) {
            panel.update(cx, |p, cx| p.close_panel(window, cx));
            return;
        }
        // A session outside the roots leaves the panel where it was.
        let folder = self.session_folder(cx).filter(|(_, f)| {
            self.tree.roots.is_empty() || paths::root_of(&self.tree.roots, f).is_some()
        });
        panel.update(cx, |p, cx| {
            if let Some((node, folder)) = folder.filter(|_| find || p.opened.is_none()) {
                p.on_node(&node, cx);
                p.opened = None;
                p.show_folder(folder, cx);
            } else if find {
                // The find stands over the list, not the file.
                p.back_to_list(cx);
            }
            p.refresh_recent(cx);
            p.watch(window, cx);
        });
        self.show_panel(cx);
        let (query, focus) = {
            let p = panel.read(cx);
            (p.query.clone(), p.focus.clone())
        };
        if find {
            ahjo_app_kit::widgets::field::focus(&query, window, cx);
        } else {
            window.focus(&focus, cx);
        }
    }

    /// Shows Files in the window's panel, on the other pages.
    fn show_panel(&self, cx: &mut App) {
        let Some(panel) = self.panel.clone() else {
            return;
        };
        self.host.show_panel(
            ahjo_app_kit::panel::Panel::new(PANEL, panel).off_its_page(),
            cx,
        );
    }

    /// The node and folder of the session in the focused terminal tile.
    fn session_folder(&self, cx: &App) -> Option<(String, String)> {
        let list = self.sessions.as_ref()?.read(cx);
        let focused = list.focused.as_ref()?;
        list.sessions
            .iter()
            .find(|s| {
                s.node == focused.node
                    && s.name == focused.name
                    && s.user.as_deref().unwrap_or_default() == focused.user
            })
            .and_then(|s| Some((s.node.clone(), s.cwd.clone()?)))
    }

    /// A file asked for beside another page (a path clicked in a
    /// terminal, evidence in a run): it opens in the Files panel, at
    /// `line` when one is named, and the panel opens, the page beside
    /// making room for it.
    fn open_in_panel(
        &mut self,
        node: NodeId,
        path: String,
        line: Option<usize>,
        column: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(panel) = self.panel.clone() else {
            return;
        };
        panel.update(cx, |p, cx| {
            p.on_node(node.as_str(), cx);
            p.show_in_tree(&path, cx);
            p.open_at(path, line, column, window, cx);
            p.watch(window, cx);
        });
        self.show_panel(cx);
    }
    /// Esc and Alt+E in the panel: put away, its unsaved edits asked
    /// about first.
    pub fn close_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if ahjo_app_kit::panel::PanelView::may_close(self, window, cx) {
            self.host.put_panel_away(cx);
        }
    }
    pub fn cancel_close_panel(&mut self, cx: &mut Context<Self>) {
        self.confirm_close_panel = false;
        cx.notify();
    }
    pub fn discard_and_close_panel(&mut self, cx: &mut Context<Self>) {
        self.drafts.clear();
        self.confirm_close_panel = false;
        self.host.put_panel_away(cx);
    }
    fn request(
        &self,
        r: Request,
        done: impl FnOnce(&mut Self, u16, Vec<u8>, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let task = self.host.request(r, cx);
        cx.spawn(async move |this, cx| {
            let (s, b) = match task.await {
                Ok(a) => (a.status, a.body),
                Err(e) => (0, e.message.into_bytes()),
            };
            let _ = this.update(cx, |p, cx| done(p, s, b, cx));
        })
        .detach();
    }
    fn call<C: TypedCommand>(
        &self,
        node: NodeId,
        input: C,
        done: impl FnOnce(&mut Self, Result<C::Output, Error>, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let task = self.host.call(node, input, cx);
        cx.spawn(async move |this, cx| {
            let answer = task.await;
            let _ = this.update(cx, |p, cx| done(p, answer, cx));
        })
        .detach();
    }
    fn node(&self) -> NodeId {
        NodeId(self.tree.node.clone())
    }
    /// Shows `node`'s files from here on.
    fn on_node(&mut self, node: &str, cx: &mut Context<Self>) {
        if self.tree.node != node {
            self.tree.choose_node(node);
            self.recent = None;
            self.roots(cx);
            self.refresh_recent(cx);
        }
    }
    pub fn start(&mut self, cx: &mut Context<Self>) {
        let Some(node) = self.host.nodes(cx).first().map(|n| n.id.to_string()) else {
            return;
        };
        if self.tree.node.is_empty() {
            self.tree.node = node;
        }
        self.roots(cx);
    }
    fn roots(&mut self, cx: &mut Context<Self>) {
        let node = self.tree.node.clone();
        self.call(
            NodeId(node.clone()),
            FileList { path: None },
            move |p, answer, cx| {
                if p.tree.node != node {
                    return;
                }
                match answer.map(|answer| answer.content) {
                    Ok(ListContent::Roots { roots }) => {
                        p.tree.rooted(roots);
                        if let Some(path) = p.reveal_pending.take() {
                            p.tree.reveal(&path);
                            p.scroll_pending = true;
                        }
                        p.read_unread(cx);
                    }
                    Ok(ListContent::Folder(_)) => {
                        p.fail("The node answered with a folder.".into(), cx)
                    }
                    Err(error) => {
                        p.tree.away = true;
                        if error.code == "unknown_command" {
                            p.fail("This node does not serve files.".into(), cx);
                        } else {
                            p.fail(error.message, cx);
                        }
                    }
                }
                cx.notify()
            },
            cx,
        )
    }
    /// Reads every open folder not yet read.
    fn read_unread(&mut self, cx: &mut Context<Self>) {
        for folder in self.tree.unread() {
            if !self.listing.contains(&folder) {
                self.list(folder, cx);
            }
        }
    }
    pub fn refresh_recent(&mut self, cx: &mut Context<Self>) {
        if self.recent_asked || self.tree.node.is_empty() {
            return;
        }
        if self.side == Side::Changes && !self.is_panel {
            self.ask_session_status(cx);
        }
        self.recent_asked = true;
        self.recent_at = Some(std::time::Instant::now());
        let node = self.tree.node.clone();
        self.call(
            NodeId(node.clone()),
            FileListRecent { path: None },
            move |p, answer, cx| {
                p.recent_asked = false;
                if p.tree.node != node {
                    return;
                }
                // The first Recent lands above the tree: a row picked in
                // the tree stays in view.
                let first = p.recent.is_none();
                p.recent = Some(answer.map(|r| r.files).map_err(|error| error.message));
                if first && matches!(p.pick, Some(Pick::Node(_))) {
                    p.scroll_to_pick();
                }
                p.newest_due = first && !p.is_panel && p.opened.is_none();
                match p.recent_inside.as_ref().map(|(folder, _)| folder.clone()) {
                    Some(folder) if p.recent_folder(&folder) => p.read_recent_inside(folder, cx),
                    _ => p.recent_inside = None,
                }
                cx.notify();
            },
            cx,
        );
    }
    /// Whether `path` is a Recent line standing for many files of a folder.
    fn recent_folder(&self, path: &str) -> bool {
        matches!(&self.recent, Some(Ok(r)) if r.iter().any(|c| c.path == path && c.files.is_some()))
    }
    /// Reads the files changed under a Recent folder line.
    fn read_recent_inside(&mut self, folder: String, cx: &mut Context<Self>) {
        let node = self.tree.node.clone();
        self.call(
            NodeId(node.clone()),
            FileListRecent {
                path: Some(folder.clone()),
            },
            move |p, answer, cx| {
                let Some((open, found)) = p.recent_inside.as_mut() else {
                    return;
                };
                if p.tree.node != node || *open != folder {
                    return;
                }
                *found = Some(answer.map(|r| r.files).map_err(|error| error.message));
                cx.notify();
            },
            cx,
        );
    }
    /// Opens the file an Enter pressed before any answer waited for.
    pub(crate) fn open_first_found(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(path) = self.find.as_mut().and_then(|f| f.take_first()) {
            self.open(path, window, cx);
            cx.notify();
        }
    }
    /// With nothing open, the page shows the file changed last: the first
    /// screen answers what just changed with the file itself.
    pub(crate) fn open_newest(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !std::mem::take(&mut self.newest_due) || self.opened.is_some() || self.find.is_some() {
            return;
        }
        let newest = match &self.recent {
            Some(Ok(recent)) => recent.iter().find(|c| c.files.is_none()),
            _ => None,
        };
        if let Some(path) = newest.map(|c| c.path.clone()) {
            self.pick = Some(Pick::Recent(path.clone()));
            // Shown unasked, so a preview the next file takes the place of.
            self.open_unasked(path, false, window, cx);
            self.shown_newest = true;
            self.keep(cx);
        }
    }
    /// A path asked for from elsewhere: a folder opens in the tree, a file
    /// opens.
    fn reveal(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        let task = self.host.call(
            self.node(),
            FileList {
                path: Some(path.clone()),
            },
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let answer = task.await;
            let _ = this.update_in(cx, |page, window, cx| {
                match answer {
                    Ok(_) => page.show_folder(path, cx),
                    Err(error) if error.code == ahjo_files_contract::NOT_FOLDER => {
                        page.show_in_tree(&path, cx);
                        page.open(path, window, cx);
                    }
                    Err(error) => page.fail(error.message, cx),
                }
                cx.notify();
            });
        })
        .detach();
    }
    /// Opens the tree down to `folder`, opens it and picks it.
    pub fn show_folder(&mut self, folder: String, cx: &mut Context<Self>) {
        self.clear_find(cx);
        self.reveal_in_tree(&paths::join(&folder, "x"), cx);
        self.tree.select(&folder);
        self.pick = Some(Pick::Node(folder));
        self.pick_at_top = true;
        // Rows landing above it move it: follow it until they have.
        self.scroll_pending = !self.scroll_to_pick() || !self.listing.is_empty();
        self.keep(cx);
        cx.notify();
    }
    /// In the panel: from the file back to the list, the file
    /// picked in it.
    pub fn back_to_list(&mut self, cx: &mut Context<Self>) {
        if let Some(o) = self.opened.take() {
            self.pick = Some(Pick::Node(o.path));
            self.scroll_pending = !self.scroll_to_pick();
        }
        self.keep(cx);
        cx.notify();
    }
    /// Opens the tree down to `path` and picks its row: what was asked
    /// for from elsewhere shows where it is.
    pub fn show_in_tree(&mut self, path: &str, cx: &mut Context<Self>) {
        self.clear_find(cx);
        self.reveal_in_tree(path, cx);
        self.set_pick(Pick::Node(path.to_owned()), cx);
        self.scroll_pending = !self.scroll_to_pick();
    }
    /// Opens the folders down to `path`, now or once the roots are known.
    fn reveal_in_tree(&mut self, path: &str, cx: &mut Context<Self>) {
        if self.tree.roots.is_empty() {
            self.reveal_pending = Some(path.to_owned());
            return;
        }
        for unread in self.tree.reveal(path) {
            self.list(unread, cx);
        }
    }
    pub fn list(&mut self, path: String, cx: &mut Context<Self>) {
        let node = self.tree.node.clone();
        self.listing.insert(path.clone());
        self.call(
            NodeId(node.clone()),
            FileList {
                path: Some(path.clone()),
            },
            move |p, answer, cx| {
                p.listing.remove(&path);
                let step_due = p.step_due.take_if(|due| *due == path).is_some();
                if p.tree.node != node {
                    return;
                }
                match answer.map(|answer| answer.content) {
                    Ok(ListContent::Folder(l)) => {
                        p.tree.listed(path.clone(), l.entries);
                        p.ask_status(&path, cx);
                        // Folders kept or asked open inside it show now: read them.
                        p.read_unread(cx);
                        if step_due && p.pick == Some(Pick::Node(path.clone())) {
                            p.step(1, cx);
                        }
                        // Listings landing above the pick move it: follow it
                        // until every folder asked for has answered.
                        if p.scroll_pending && p.scroll_to_pick() && p.listing.is_empty() {
                            p.scroll_pending = false;
                        }
                    }
                    Ok(ListContent::Roots { .. }) => {
                        p.tree
                            .failed(path.clone(), "The node answered with its roots.".into());
                    }
                    Err(error)
                        if matches!(
                            error.code.as_str(),
                            "unreachable"
                                | "unanswered"
                                | "unknown_node"
                                | "unavailable"
                                | "no_app"
                        ) =>
                    {
                        p.tree.away = true;
                    }
                    Err(error) => {
                        if error.code == ahjo_files_contract::NOT_FOUND {
                            p.tree.open.remove(&path);
                            p.tree.listings.remove(&path);
                        }
                        p.tree.failed(path.clone(), error.message);
                    }
                }
                cx.notify()
            },
            cx,
        )
    }

    /// The query changed: a find of names across every root, answered as
    /// you type.
    fn typed(&mut self, query: String, cx: &mut Context<Self>) {
        self.find_revision += 1;
        let jump = Jump::read(&query);
        if query.trim().is_empty() {
            self.find = None;
            self.jump = None;
            self.back_from_find();
            return;
        }
        if self.find.is_none() && self.jump.is_none() {
            let place = self.list_scroll.0.borrow().base_handle.offset();
            let open = self.opened.as_ref().map(|o| o.path.clone());
            self.before_find = Some((self.pick.take(), place, open));
        }
        if let Some(jump) = jump {
            self.find = None;
            self.jump = Some(jump);
            self.pick = self.picks().into_iter().next();
            cx.notify();
            return;
        }
        self.jump = None;
        let revision = self.find_revision;
        self.find = Some(Find::new(&query, revision));
        self.pick = Some(Pick::Hit(0));
        let node = self.tree.node.clone();
        let near = self.text_scope().unwrap_or_default();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(TYPED).await;
            let _ = this.update(cx, |page, cx| {
                if page.find_revision == revision {
                    let near = (!near.is_empty()).then_some(near);
                    // What is beside you is asked for alone as well: it
                    // answers at once, the find under every root after.
                    if let Some(near) = near.clone() {
                        let beside = page.host.call(
                            NodeId(node.clone()),
                            FileSearchPaths {
                                query: query.clone(),
                                near: Some(near),
                                beside: Some(true),
                                under: None,
                            },
                            cx,
                        );
                        page.ask_find(beside, revision, Part::Beside, cx);
                    }
                    let names = page.host.call(
                        NodeId(node),
                        FileSearchPaths {
                            query,
                            near,
                            beside: None,
                            under: None,
                        },
                        cx,
                    );
                    page.ask_find(names, revision, Part::Names, cx);
                    // Ctrl+Shift+F looks inside the files as you type.
                    if page.search_inside {
                        page.find_in_text(cx);
                    }
                }
            });
        })
        .detach();
    }
    fn ask_find(
        &mut self,
        task: impl Future<Output = Result<Matches, Error>> + 'static,
        revision: u64,
        part: Part,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            let found = task.await.map_err(|error| error.message);
            let _ = this.update(cx, |p, cx| {
                if let Some(find) = p.find.as_mut() {
                    match part {
                        Part::Beside => find.answer_beside(revision, found),
                        Part::Names => find.answer(revision, found),
                        Part::Inside => find.answer_inside(revision, found),
                    }
                }
                cx.notify()
            });
        })
        .detach();
    }
    /// The folder a find looks in first, and whose files' text it
    /// searches: a folder picked in the tree; else the workspace of the
    /// file open, the repository it is in (or its folder outside one);
    /// else the folder of the row picked.
    pub fn text_scope(&self) -> Option<String> {
        if let Some(row) = self.tree.selection().filter(|r| r.folder) {
            return Some(row.path);
        }
        match self.opened.as_ref().filter(|o| o.node == self.tree.node) {
            Some(o) => Some(
                self.repository_of(&o.path)
                    .unwrap_or_else(|| paths::parent(&o.path)),
            ),
            None => self.tree.folder_here(),
        }
    }
    /// The repository `path` is in, as far as git has said.
    pub fn repository_of(&self, path: &str) -> Option<String> {
        self.repos
            .keys()
            .filter(|r| paths::inside(r, path))
            .max_by_key(|r| r.len())
            .cloned()
    }
    /// Looks for the query inside the files under the text scope; the
    /// names found stay above.
    pub fn find_in_text(&mut self, cx: &mut Context<Self>) {
        let Some(folder) = self.text_scope() else {
            return;
        };
        let Some(find) = self.find.as_mut() else {
            return;
        };
        find.inside = Some(store::find::Inside {
            folder: folder.clone(),
            found: None,
        });
        let (query, revision) = (find.query.clone(), find.revision);
        let first = find.matches().len();
        self.pick = Some(Pick::Hit(first));
        let task = self.host.call(
            self.node(),
            FileSearch {
                path: folder,
                query,
            },
            cx,
        );
        self.ask_find(
            async move { task.await.map(|answer| answer.results) },
            revision,
            Part::Inside,
            cx,
        );
        cx.notify();
    }
    fn clear_find(&mut self, cx: &mut Context<Self>) {
        if self.find.is_some() || !self.query.read(cx).text.is_empty() {
            self.query.update(cx, |field, cx| field.set("", false, cx));
            self.find = None;
            self.jump = None;
            self.search_inside = false;
            self.find_revision += 1;
            self.back_from_find();
        }
    }
    /// The find is over: the list is where it was when the find began, or
    /// on the file the find opened.
    fn back_from_find(&mut self) {
        let Some((pick, place, open)) = self.before_find.take() else {
            self.pick = None;
            return;
        };
        self.pick = pick;
        self.list_scroll.0.borrow().base_handle.set_offset(place);
        if let Some(path) = self.opened.as_ref().map(|o| o.path.clone())
            && open.as_ref() != Some(&path)
        {
            self.follow_open(&path);
        }
    }

    /// The rows the keyboard steps through, as the list shows them.
    pub fn picks(&self) -> Vec<Pick> {
        crate::view::lines(self)
            .into_iter()
            .filter_map(|line| line.pick())
            .collect()
    }
    pub fn step(&mut self, by: isize, cx: &mut Context<Self>) {
        let picks = self.picks();
        if picks.is_empty() {
            return;
        }
        let at = self
            .pick
            .as_ref()
            .and_then(|p| picks.iter().position(|q| q == p));
        let last = picks.len() - 1;
        let next = match (at, by) {
            (_, isize::MAX) | (None, isize::MIN..0) => last,
            (_, isize::MIN) | (None, _) => 0,
            (Some(i), by) => (i as isize).saturating_add(by).clamp(0, last as isize) as usize,
        };
        self.set_pick(picks[next].clone(), cx);
    }
    pub fn set_pick(&mut self, pick: Pick, cx: &mut Context<Self>) {
        self.pick_at_top = false;
        if let Pick::Node(path) = &pick {
            self.tree.select(path);
            self.keep(cx);
        }
        self.pick = Some(pick);
        self.scroll_to_pick();
        cx.notify();
    }
    /// Scrolls the list to the picked row; false while it is not there yet.
    fn scroll_to_pick(&self) -> bool {
        let Some(pick) = &self.pick else { return false };
        let Some(at) = crate::view::lines(self)
            .iter()
            .position(|l| l.pick().as_ref() == Some(pick))
        else {
            return false;
        };
        if self.pick_at_top {
            self.list_scroll
                .scroll_to_item_strict(at, gpui::ScrollStrategy::Top);
        } else {
            self.list_scroll
                .scroll_to_item(at, gpui::ScrollStrategy::Nearest);
        }
        true
    }
    /// Enter on the picked row: a file opens in a tab of its own.
    pub fn activate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.activate_as(true, window, cx)
    }
    /// The picked row opened: a file kept in its tab, or (`keep` false, a
    /// click or Space) previewed.
    pub fn activate_as(&mut self, keep: bool, window: &mut Window, cx: &mut Context<Self>) {
        let pick = match &self.pick {
            Some(p) => p.clone(),
            None if self.find.is_some() => Pick::Hit(0),
            None => return,
        };
        match pick {
            // A folder line opens to the files changed in it, and closes.
            Pick::Recent(path) if self.recent_folder(&path) => {
                if self
                    .recent_inside
                    .take()
                    .is_none_or(|(open, _)| open != path)
                {
                    self.recent_inside = Some((path.clone(), None));
                    self.read_recent_inside(path, cx);
                }
            }
            Pick::Recent(path) => self.open_as(path, keep, window, cx),
            Pick::More => {
                self.recent_more = true;
                cx.notify();
            }
            Pick::Node(path) => match self.tree.row(&path) {
                Some(row) if row.folder => {
                    if self.tree.toggle(&path) {
                        self.list(path, cx);
                    }
                    self.keep(cx);
                }
                Some(_) => self.open_as(path, keep, window, cx),
                None => {}
            },
            Pick::Hit(i) => {
                let hit = self.find.as_ref().and_then(|f| f.matches().get(i).cloned());
                match (hit, self.find.as_mut()) {
                    (Some(hit), _) => self.open_at(hit.path, hit.line, None, window, cx),
                    // Enter before any answer: the first file found opens.
                    (None, Some(find)) if i == 0 && find.widening => find.open_first = true,
                    _ => {}
                }
            }
            Pick::InText => self.find_in_text(cx),
            Pick::Group(folder) => {
                self.remember(cx);
                self.show_place(folder, cx)
            }
            Pick::Go(line) | Pick::Symbol(line) => self.go_to(line, None, window, cx),
            Pick::HitFile(path) => self.open_as(path, keep, window, cx),
        }
        cx.notify();
    }

    /// Opens a file at its line (from 1), and at a column (from 1) on it:
    /// at once when its text is already shown (open, or its tab back as it
    /// was left), else once it is read.
    pub fn open_at(
        &mut self,
        path: String,
        line: Option<usize>,
        column: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = (self.tree.node.clone(), path.clone());
        // At a line it opens to its source.
        if line.is_some() {
            self.open_unasked(path, true, window, cx);
        } else {
            self.open(path, window, cx);
        }
        let shown = self
            .opened
            .as_ref()
            .is_some_and(|o| o.key() == key && matches!(o.show, Show::Text { .. }));
        let column = column.filter(|_| line.is_some());
        match line {
            Some(line) if shown => self.go_to(
                line.saturating_sub(1),
                column.map(|c| c.saturating_sub(1)),
                window,
                cx,
            ),
            _ => {
                self.pending_line = line;
                self.pending_column = column;
            }
        }
    }
    /// Opens a file in a tab of its own, and shows it in the tree.
    pub fn open(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        self.open_as(path, true, window, cx)
    }
    /// Opens a file the operator did not ask to see as a page now.
    fn open_unasked(
        &mut self,
        path: String,
        keep: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.unasked = true;
        self.open_as(path, keep, window, cx);
        self.unasked = false;
    }
    /// Opens a file in a tab, kept or (`keep` false) a preview the next
    /// one previewed replaces; a tab left before comes back as it was.
    pub fn open_as(
        &mut self,
        path: String,
        keep: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = (self.tree.node.clone(), path.clone());
        let same = self.opened.as_ref().is_some_and(|o| o.key() == key);
        if !same {
            self.remember(cx);
            self.stash(cx);
        }
        let after = self.opened.as_ref().and_then(|o| self.tabs.at(&o.key()));
        self.tabs.show(key.clone(), keep, after);
        let tabs = &self.tabs;
        self.tab_kept.retain(|k, _| tabs.at(k).is_some());
        self.tab_into_view(&key);
        if std::mem::take(&mut self.palette) {
            window.focus(&self.focus, cx);
        }
        self.jump = None;
        self.place = None;
        if same {
            // Asked for now, so it is the file to come back to.
            self.run_outside(cx);
            self.shown_newest = false;
            self.follow_open(&path);
            self.keep(cx);
            cx.notify();
            return;
        }
        if let Some(kept) = self.tab_kept.remove(&key) {
            self.restore(kept, window, cx);
            return;
        }
        self.jump_due = self.side == Side::Changes && !self.is_panel;
        self.md_go.set(None);
        self.md_scroll.set_offset(gpui::Point::default());
        self.md_select.borrow_mut().selection.clear();
        self.pending_line = None;
        self.pending_column = None;
        self.confirm_discard = false;
        self.leave_after_save = false;
        self.conflict = false;
        self.shown_newest = false;
        self.code
            .update(cx, |code, cx| code.edit(false, window, cx));
        let node = self.tree.node.clone();
        let name = paths::base_name(&path).to_string();
        self.seen.remove(&path);
        self.symbols.clear();
        self.opened = Some(Opened::reading(node.clone(), path.clone()));
        // In the panel, the file takes the list's place: the keys go with it.
        if self.is_panel && self.query.read(cx).is_focused(window) {
            window.focus(&self.focus, cx);
        }
        self.follow_open(&path);
        self.ask_status(&paths::parent(&path), cx);
        self.keep(cx);
        let kind = store::kinds::kind(&name);
        if matches!(
            kind,
            store::kinds::Kind::Audio | store::kinds::Kind::Video | store::kinds::Kind::Pdf
        ) {
            self.opened.as_mut().unwrap().show = Show::Media(kind);
            cx.notify();
            return;
        }
        self.run_outside(cx);
        self.read(node, path, false, cx);
        cx.notify();
    }
    /// Reads the open file; `again` keeps the place read and any edit.
    fn read(&mut self, node: String, path: String, again: bool, cx: &mut Context<Self>) {
        let name = paths::base_name(&path).to_string();
        self.request(
            Request::get(NodeId(node.clone()), bytes_uri(&path)),
            move |p, s, b, cx| {
                if !p
                    .opened
                    .as_ref()
                    .is_some_and(|o| o.node == node && o.path == path)
                {
                    return;
                }
                if s != 200 {
                    if code_of(&b).as_deref() == Some(ahjo_files_contract::NOT_FILE) {
                        p.opened = None;
                        p.show_folder(path, cx);
                        return;
                    }
                    if let Some(o) = p.opened.as_mut() {
                        o.show = Show::Failed(message(s, &b));
                    }
                    // A file deleted since the last commit still has what
                    // went to show.
                    if code_of(&b).as_deref() == Some(ahjo_files_contract::NOT_FOUND) {
                        p.read_changes(cx);
                    }
                    cx.notify();
                    return;
                }
                p.opened.as_mut().unwrap().size = Some(b.len() as u64);
                if store::kinds::kind(&name) == store::kinds::Kind::Model {
                    p.decode(node, path, cx, move || {
                        crate::model::Model::load(&b)
                            .map(crate::model::Preview::new)
                            .map(Show::Model)
                    });
                    return;
                }
                if let Some(format) = open::picture_format(&name) {
                    let svg = cx.svg_renderer();
                    p.decode(node, path, cx, move || {
                        let image = gpui::Image::from_bytes(format, b)
                            .to_image_data(svg)
                            .map_err(|e| e.to_string())?;
                        let dim = image.size(0);
                        Ok(Show::Picture {
                            natural: store::zoom::Px {
                                w: dim.width.0 as f32,
                                h: dim.height.0 as f32,
                            },
                            image,
                        })
                    });
                    return;
                }
                let (show, text, pretty) = open::show(&name, &b);
                let version = open::version(&b);
                let key = (node.clone(), path.clone());
                let o = p.opened.as_mut().unwrap();
                if again && o.base == version {
                    return;
                }
                let draft = p.drafts.get(&key).cloned();
                if again && draft.is_some() {
                    // Theirs changed under an edit: Save will ask.
                    o.changed_on_disk = true;
                    cx.notify();
                    return;
                }
                let is_text = matches!(show, Show::Text { .. });
                o.show = show;
                o.saved = text.clone();
                o.base = draft.as_ref().map_or(version, |d| d.base.clone());
                o.pretty = pretty.clone();
                o.changed_on_disk = false;
                let lang = syntax::lang(&name);
                let line = p.pending_line.take();
                let column = p.pending_column.take();
                let place = p.pending_place.take();
                let log = store::kinds::extension(&name) == "log";
                p.symbols = store::symbols::of(&name, draft.as_ref().map_or(&text, |d| &d.text));
                p.pretty_code.update(cx, |code, cx| {
                    code.set_lang(lang, cx);
                    if again {
                        code.reload(pretty.as_deref().unwrap_or(&text), cx);
                    } else {
                        code.set(pretty.as_deref().unwrap_or(&text), cx);
                    }
                });
                p.code.update(cx, |code, cx| {
                    code.set_lang(lang, cx);
                    let shown = draft.as_ref().map_or(&text, |d| &d.text);
                    if again {
                        code.reload(shown, cx);
                    } else {
                        code.set(shown, cx);
                        if let Some(place) = place.as_ref() {
                            code.set_place(place, cx);
                        } else if let (Some(line), Some(column)) = (line, column) {
                            code.go_to_column(line.saturating_sub(1), column.saturating_sub(1), cx);
                        } else if let Some(line) = line {
                            code.go_to_line(line.saturating_sub(1), cx);
                        } else if log {
                            code.show_end(cx);
                        }
                    }
                });
                if line.is_some()
                    && let Some(o) = p.opened.as_mut()
                {
                    o.source = true;
                }
                if is_text && !again {
                    p.read_changes(cx);
                }
                if p.infile.is_some() {
                    p.refind(false, cx);
                }
                cx.notify()
            },
            cx,
        )
    }
    /// Decodes off the UI thread what a file shows as.
    fn decode(
        &mut self,
        node: String,
        path: String,
        cx: &mut Context<Self>,
        work: impl FnOnce() -> Result<Show, String> + Send + 'static,
    ) {
        let task = cx.background_executor().spawn(async move { work() });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |p, cx| {
                if let Some(o) = p
                    .opened
                    .as_mut()
                    .filter(|o| o.node == node && o.path == path)
                {
                    o.show = result.unwrap_or_else(Show::Failed);
                    cx.notify();
                }
            });
        })
        .detach();
    }
    /// What the open file's repository says changed in it, so Changes is
    /// offered only when there are some.
    fn read_changes(&mut self, cx: &mut Context<Self>) {
        let Some(o) = self.opened.as_ref() else {
            return;
        };
        let node = o.node.clone();
        let path = o.path.clone();
        self.call(
            NodeId(node.clone()),
            FileDiff {
                path: path.clone(),
                whole: Some(true),
            },
            move |page, answer, cx| {
                let Some(o) = page
                    .opened
                    .as_mut()
                    .filter(|o| o.node == node && o.path == path)
                else {
                    return;
                };
                o.diff = answer
                    .ok()
                    .filter(|d| d.error.is_none() && !d.diff.trim().is_empty());
                o.changes = o.diff.as_ref().map(|d| store::marks::of_diff(&d.diff));
                o.sides = o
                    .diff
                    .as_ref()
                    .map(|d| std::rc::Rc::new(open::Sides::of(&o.name, &d.diff)));
                if o.diff.is_none() {
                    o.diff_open = false;
                } else if matches!(o.show, Show::Failed(_) | Show::Gone) {
                    o.show = Show::Gone;
                    o.diff_open = true;
                }
                page.apply_marks(cx);
                if std::mem::take(&mut page.jump_due) {
                    page.first_change(cx);
                }
                cx.notify();
            },
            cx,
        );
    }
    pub fn toggle_diff(&mut self, cx: &mut Context<Self>) {
        if let Some(o) = self.opened.as_mut() {
            o.diff_open = !o.diff_open && o.diff.is_some();
        }
        if self.opened.as_ref().is_some_and(|o| o.diff_open) {
            self.diff_opened(cx);
        }
        cx.notify();
    }
    fn changed(&mut self, cx: &mut Context<Self>) {
        let Some(o) = self.opened.as_ref() else {
            return;
        };
        let text = self.code.read(cx).text().to_owned();
        let key = o.key();
        self.tabs.keep(&key);
        self.symbols = store::symbols::of(&o.name, &text);
        if self.infile.is_some() {
            self.refind(false, cx);
        }
        let Some(o) = self.opened.as_ref() else {
            return;
        };
        if text != o.saved {
            self.drafts.insert(
                o.key(),
                Draft {
                    text,
                    base: o.base.clone(),
                },
            );
        } else {
            self.drafts.remove(&o.key());
        }
        cx.notify()
    }
    pub fn edit(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        if let Some(o) = self.opened.as_mut().filter(|o| o.editable()) {
            o.editing = true;
            o.diff_open = false;
            self.code.update(cx, |c, cx| c.edit(true, w, cx));
        }
        self.apply_marks(cx);
        cx.notify()
    }
    pub(crate) fn finish_edit(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        if self.opened.as_ref().is_some_and(|o| !o.editing) {
            if self.is_panel {
                self.close_panel(w, cx);
            } else {
                w.focus(&self.focus, cx);
            }
            cx.notify();
            return;
        }
        let dirty = self
            .opened
            .as_ref()
            .is_some_and(|o| self.drafts.contains_key(&o.key()));
        if self.confirm_discard {
            // Esc on the question is its safe answer.
            self.keep_editing(w, cx);
        } else if self.conflict {
            self.cancel_conflict(w, cx);
        } else if dirty && self.opened.as_ref().is_some_and(|o| o.saving) {
            // The edit is on its way to disk: nothing to ask about yet.
            self.leave_after_save = true;
        } else if dirty {
            self.confirm_discard = true;
        } else if let Some(o) = self.opened.as_mut() {
            o.editing = false;
            self.code.update(cx, |c, cx| c.edit(false, w, cx));
            self.apply_marks(cx);
        }
        cx.notify()
    }
    pub fn keep_editing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.confirm_discard) {
            Code::focus(&self.code, window, cx);
        }
        cx.notify();
    }
    pub fn save(&mut self, cx: &mut Context<Self>) {
        self.write(open::Write::Save, cx);
    }
    pub fn cancel_conflict(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.conflict) {
            Code::focus(&self.code, window, cx)
        }
        cx.notify();
    }
    pub fn write(&mut self, mode: open::Write, cx: &mut Context<Self>) {
        let Some(opened) = self.opened.as_mut() else {
            return;
        };
        if opened.saving {
            return;
        }
        opened.saving = true;
        let node = opened.node.clone();
        let path = opened.path.clone();
        let base = opened.base.clone();
        let name = opened.name.clone();
        let folder = paths::parent(&path);
        let sent = self.code.read(cx).text().to_owned();
        let taken: Vec<String> = self
            .tree
            .listings
            .get(&folder)
            .map(|l| l.iter().map(|e| e.name.clone()).collect())
            .unwrap_or_default();
        let (target, ordinal) = if mode == open::Write::Copy {
            let mut n = self.copy_ordinal;
            while taken.contains(&open::copy_name(&name, n)) {
                n += 1;
            }
            (paths::join(&folder, &open::copy_name(&name, n)), n)
        } else {
            (path.clone(), 0)
        };
        let (method, uri, success) = match mode {
            open::Write::Save => (
                "PUT",
                format!("{}&base={}", bytes_uri(&target), enc(&base)),
                204,
            ),
            open::Write::Overwrite => ("PUT", bytes_uri(&target), 204),
            open::Write::Copy => ("POST", bytes_uri(&target), 201),
        };
        let mut request = Request::new(method, NodeId(node.clone()), uri);
        request.body = Some(("application/octet-stream".into(), sent.as_bytes().to_vec()));
        self.request(
            request,
            move |p, status, body, cx| {
                let key = (node.clone(), path.clone());
                // The answer belongs to the file it was sent for, shown or not.
                let here = p.opened.as_ref().is_some_and(|o| o.key() == key);
                if status != success {
                    if !here {
                        p.not_saved_away(&key, status, &body, cx);
                    } else {
                        p.leave_after_save = false;
                        p.opened.as_mut().unwrap().saving = false;
                        if status == 412 {
                            p.conflict = true;
                            p.copy_ordinal = 1;
                        } else if status == 409 && mode == open::Write::Copy {
                            p.copy_ordinal = ordinal + 1;
                            p.write(mode, cx);
                        } else {
                            p.fail(message(status, &body), cx)
                        }
                    }
                    cx.notify();
                    return;
                }
                p.host
                    .toast(&format!("Saved {}", paths::base_name(&target)), cx);
                let new_key = (node.clone(), target.clone());
                if here {
                    p.conflict = false;
                    let now = p.code.read(cx).text().to_owned();
                    let o = p.opened.as_mut().unwrap();
                    o.wrote(&target, &sent);
                    let pretty = o.pretty.clone();
                    p.pretty_code.update(cx, |code, cx| {
                        code.reload(pretty.as_deref().unwrap_or(&sent), cx)
                    });
                    p.drafts.remove(&key);
                    if let Some(d) = open::acknowledged(&now, &sent) {
                        p.drafts.insert(new_key.clone(), d);
                        p.leave_after_save = false;
                    }
                } else {
                    // Saved while another file is shown: its tab and its
                    // edit learn the new version, so it is neither unsaved
                    // nor in conflict with this write when it comes back.
                    if let Some(mut kept) = p.tab_kept.remove(&key) {
                        kept.opened_mut().wrote(&target, &sent);
                        p.tab_kept.insert(new_key.clone(), kept);
                    }
                    if let Some(d) = p.drafts.remove(&key)
                        && let Some(d) = open::acknowledged(&d.text, &sent)
                    {
                        p.drafts.insert(new_key.clone(), d);
                    }
                }
                p.tabs.moved(&key, new_key.clone());
                // Our own write is not a change to show.
                p.seen.remove(&target);
                p.seen.remove(&folder);
                p.refresh_status(cx);
                if p.close_after_save.take() == Some(new_key.clone())
                    && !p.drafts.contains_key(&new_key)
                {
                    p.close_now(new_key, cx);
                    cx.notify();
                    return;
                }
                if p.tree.listings.contains_key(&folder) {
                    p.list(folder.clone(), cx);
                }
                p.read_changes(cx);
                cx.notify();
            },
            cx,
        )
    }
    /// A save that failed after another file was shown: said with the
    /// file's name, its edit kept, and its tab knowing it is not saving.
    fn not_saved_away(
        &mut self,
        key: &store::tabs::Key,
        status: u16,
        body: &[u8],
        cx: &mut Context<Self>,
    ) {
        if let Some(kept) = self.tab_kept.get_mut(key) {
            let o = kept.opened_mut();
            o.saving = false;
            o.changed_on_disk |= status == 412;
        }
        if self.close_after_save.as_ref() == Some(key) {
            self.close_after_save = None;
        }
        let name = paths::base_name(&key.1);
        self.fail(
            if status == 412 {
                format!(
                    "{name} was not saved: it changed on disk since it was opened. Open it to keep yours or theirs."
                )
            } else {
                format!("{name} was not saved: {}", message(status, body))
            },
            cx,
        );
    }
    pub fn search_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        // In the panel, Esc in an empty find puts the panel away.
        if e.keystroke.key == "escape"
            && self.is_panel
            && self.query.read(cx).is_focused(window)
            && self.query.read(cx).text.is_empty()
        {
            self.close_panel(window, cx);
            cx.stop_propagation();
            return;
        }
        if e.keystroke.key == "escape" && self.palette {
            self.close_palette(window, cx);
            cx.stop_propagation();
            return;
        }
        if e.keystroke.key == "escape" && self.query.read(cx).is_focused(window) {
            self.clear_find(cx);
            window.focus(&self.focus, cx);
            cx.stop_propagation();
            cx.notify();
        }
    }
    /// The page's keys: those that move between files wherever the
    /// keyboard is in the page, then the list's.
    pub fn key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.files_key(e, window, cx) {
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if self.opened.is_some()
            && let Some(z) = ahjo_app_kit::zoom::from_key(&e.keystroke)
        {
            self.zoom_file(z, 1, cx);
            cx.stop_propagation();
            return;
        }
        let reading = !self.opened.as_ref().is_some_and(|o| o.editing)
            && (self.code.read(cx).is_focused(window)
                || self.pretty_code.read(cx).is_focused(window));
        if !self.focus.is_focused(window) && !reading {
            return;
        }
        let key = &e.keystroke;
        let m = key.modifiers;
        if m.control
            && !m.shift
            && key.key == "c"
            && let Some(text) = self.md_select.borrow().copied()
        {
            ahjo_app_kit::widgets::select::copy(text, cx);
            cx.stop_propagation();
            return;
        }
        let plain = !m.alt && !m.control && !m.platform;
        // Reading: the next change in the file, or the one before.
        let in_diff = self.opened.as_ref().is_some_and(|o| o.diff_open);
        if (reading || self.focus.is_focused(window) && (self.reading() || in_diff))
            && plain
            && key.key.eq_ignore_ascii_case("n")
        {
            self.step_change(!m.shift, cx);
            cx.stop_propagation();
            return;
        }
        if reading {
            return;
        }
        if m.alt || m.platform || (m.control && key.key != "c") {
            return;
        }
        match key.key.as_str() {
            "up" => self.step(-1, cx),
            // Down on a folder just opened whose contents are still on
            // their way goes into it once they come.
            "down" => match self.tree.selection().filter(|r| r.reading) {
                Some(r) if self.pick == Some(Pick::Node(r.path.clone())) => {
                    self.step_due = Some(r.path)
                }
                _ => self.step(1, cx),
            },
            "pageup" => self.step(-(self.list_page() as isize), cx),
            "pagedown" => self.step(self.list_page() as isize, cx),
            "home" => self.step(isize::MIN, cx),
            "end" => self.step(isize::MAX, cx),
            "left" if matches!(self.pick, Some(Pick::Node(_))) => {
                self.tree.left();
                let path = self.tree.selected.clone().unwrap_or_default();
                self.set_pick(Pick::Node(path), cx);
            }
            "right" if matches!(self.pick, Some(Pick::Node(_))) => {
                if self.tree.right()
                    && let Some(path) = self.tree.selected.clone()
                {
                    self.list(path, cx);
                }
                let path = self.tree.selected.clone().unwrap_or_default();
                self.set_pick(Pick::Node(path), cx);
            }
            // Enter opens to read; Space previews and keeps the keyboard
            // in the list.
            "enter" => {
                self.activate(window, cx);
                if self.opened.is_some() && self.place.is_none() && !self.picked_folder() {
                    self.focus_file(window, cx);
                }
            }
            "space" => {
                self.typed_ahead.0.clear();
                self.activate_as(false, window, cx)
            }
            "f5" => {
                for folder in self.tree.open_folders() {
                    self.list(folder, cx);
                }
                self.refresh_recent(cx);
                self.refresh_status(cx);
            }
            "/" => {
                self.reselect_query(cx);
                self.find_anywhere(window, cx)
            }
            "f2" => self.start_rename(window, cx),
            // In the panel, Esc puts it away (its drafts are
            // asked about first), with a find under the file shown too.
            "escape" if self.is_panel && (self.find.is_none() || self.opened.is_some()) => {
                self.close_panel(window, cx);
            }
            "escape" => {
                if self.find.is_some() {
                    self.clear_find(cx);
                } else {
                    return;
                }
            }
            "c" if m.control && m.shift => {
                if let Some(path) = self.picked_path() {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(path))
                }
            }
            "backspace" => self.up(window, cx),
            _ => {
                // Letters typed go to the next row whose name starts so.
                match key.key_char.as_deref() {
                    Some(ch) if !m.control && ch.chars().all(|c| !c.is_control()) => {
                        self.type_ahead(ch, cx)
                    }
                    _ => return,
                }
            }
        }
        cx.stop_propagation();
        cx.notify();
    }
    /// The path of the picked row.
    pub fn picked_path(&self) -> Option<String> {
        match self.pick.as_ref()? {
            Pick::Recent(p) | Pick::Node(p) => Some(p.clone()),
            Pick::Hit(i) => self
                .find
                .as_ref()
                .and_then(|f| f.matches().get(*i).map(|m| m.path.clone())),
            Pick::Group(p) | Pick::HitFile(p) => Some(p.clone()),
            Pick::More | Pick::InText | Pick::Go(_) | Pick::Symbol(_) => None,
        }
    }
    pub fn discard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.confirm_discard = false;
        if let Some(opened) = self.opened.as_mut() {
            self.drafts.remove(&opened.key());
            let saved = opened.saved.clone();
            let stale = std::mem::take(&mut opened.changed_on_disk);
            opened.editing = false;
            let (node, path) = (opened.node.clone(), opened.path.clone());
            self.symbols = store::symbols::of(&opened.name, &saved);
            self.code.update(cx, |code, cx| {
                code.reload(&saved, cx);
                code.edit(false, window, cx)
            });
            if self.infile.is_some() {
                self.refind(false, cx);
            }
            if stale {
                // What is on disk now, not what the edit began from.
                self.read(node, path, true, cx);
            }
        }
        self.apply_marks(cx);
        cx.notify();
    }

    /// Opens the name field for a new file or folder in `folder`.
    pub fn start_naming(&mut self, naming: Naming, window: &mut Window, cx: &mut Context<Self>) {
        if !self.tree.writable() {
            return;
        }
        let text = match &naming {
            Naming::Rename(path) => paths::base_name(path).to_owned(),
            _ => String::new(),
        };
        if matches!(naming, Naming::Rename(_)) {
            self.name_edit
                .update(cx, |edit, cx| edit.start(&text, window, cx));
        } else {
            self.name_edit.update(cx, |edit, cx| edit.revert(cx));
            self.name_field.update(cx, |field, cx| {
                field.set_placeholder(naming.placeholder());
                field.set(&text, true, cx)
            });
            ahjo_app_kit::widgets::field::focus(&self.name_field, window, cx);
        }
        self.naming = Some(naming);
        cx.notify();
    }
    pub fn start_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(path) = self
            .picked_path()
            .filter(|p| self.tree.row(p).is_some_and(|r| r.depth > 0))
        {
            let picked = self.pick.clone().map(RenameIn::Line);
            self.start_rename_in(path, picked, window, cx);
        }
    }
    /// Renames `path` where `place` shows its name.
    fn start_rename_in(
        &mut self,
        path: String,
        place: Option<RenameIn>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.naming_in = place;
        self.start_naming(Naming::Rename(path), window, cx);
    }
    /// A rename the node or the name refused goes on as typed.
    fn resume_rename(&mut self, naming: &Naming, cx: &mut Context<Self>) {
        if matches!(naming, Naming::Rename(_)) && self.naming.as_ref() == Some(naming) {
            self.name_edit.update(cx, |edit, cx| edit.resume(cx));
        }
    }
    pub fn stop_naming(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.naming = None;
        self.name_edit.update(cx, |edit, cx| edit.revert(cx));
        self.name_field
            .update(cx, |field, cx| field.set("", false, cx));
        window.focus(&self.focus, cx);
        cx.notify();
    }
    /// Gives the name typed: a rename ends as Enter ends it.
    pub fn apply_naming(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.naming {
            Some(Naming::Rename(_)) => self.name_edit.update(cx, |edit, cx| edit.commit(cx)),
            Some(_) => {
                let name = self.name_field.read(cx).text.trim().to_owned();
                self.give_name(&name, window, cx);
            }
            None => {}
        }
    }

    /// Makes the new file or folder, or renames, under `name`. A rename
    /// to its own name, or to none, changes nothing; a name the node
    /// refuses keeps the rename open, saying why.
    fn give_name(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(naming) = self.naming.clone() else {
            return;
        };
        let name = name.to_owned();
        if let Naming::Rename(from) = &naming
            && (name.is_empty() || name == paths::base_name(from))
        {
            self.naming = None;
            cx.notify();
            return;
        }
        if let Some(reason) = paths::bad_name(&name) {
            self.fail(reason.into(), cx);
            self.resume_rename(&naming, cx);
            return;
        }
        let node = self.tree.node.clone();
        let folder = match &naming {
            Naming::File(folder) | Naming::Folder(folder) => folder.clone(),
            Naming::Rename(from) => paths::parent(from),
        };
        let made = paths::join(&folder, &name);
        let file = matches!(naming, Naming::File(_));
        let target = NodeId(node.clone());
        let task = if file {
            let answer = self.host.call(
                target,
                FileWrite {
                    path: made.clone(),
                    text: String::new(),
                    base: None,
                    new: Some(true),
                    base64: false,
                    replace: false,
                },
                cx,
            );
            cx.background_executor()
                .spawn(async move { answer.await.map(drop) })
        } else {
            let answer = match &naming {
                Naming::Rename(from) => self.host.call(
                    target,
                    FileMove {
                        path: from.clone(),
                        to: made.clone(),
                    },
                    cx,
                ),
                _ => self
                    .host
                    .call(target, FileCreateFolder { path: made.clone() }, cx),
            };
            cx.background_executor()
                .spawn(async move { answer.await.map(drop) })
        };
        cx.spawn_in(window, async move |this, cx| {
            let answer = task.await;
            let _ = this.update_in(cx, |p, window, cx| {
                if let Err(error) = answer {
                    p.fail(error.message, cx);
                    p.resume_rename(&naming, cx);
                    cx.notify();
                    return;
                }
                if let Naming::Rename(from) = &naming {
                    let to = &made;
                    store::drafts::move_drafts(&mut p.drafts, &node, from, to);
                    // Tabs follow what they show.
                    let under: Vec<store::tabs::Key> = p
                        .tabs
                        .open
                        .iter()
                        .map(|t| t.key.clone())
                        .filter(|k| k.0 == node && paths::inside(from, &k.1))
                        .collect();
                    for k in under {
                        p.tab_kept.remove(&k);
                        let path = format!("{to}{}", &k.1[from.len()..]);
                        p.tabs.moved(&k, (node.clone(), path));
                    }
                    if let Some(opened) = p
                        .opened
                        .as_mut()
                        .filter(|o| o.node == node && paths::inside(from, &o.path))
                    {
                        opened.path = format!("{to}{}", &opened.path[from.len()..]);
                        opened.name = paths::base_name(&opened.path).to_owned();
                    }
                    if p.tree.open.remove(from) {
                        p.tree.open.insert(to.clone());
                    }
                }
                p.naming = None;
                p.name_field
                    .update(cx, |field, cx| field.set("", false, cx));
                p.tree.open.insert(folder.clone());
                p.list(folder.clone(), cx);
                p.tree.select(&made);
                p.pick = Some(Pick::Node(made.clone()));
                p.refresh_recent(cx);
                p.keep(cx);
                // A new file opens, to be written.
                if file {
                    p.open(made.clone(), window, cx);
                }
                cx.notify();
            });
        })
        .detach();
        window.focus(&self.focus, cx);
    }
    pub fn delete(&mut self, path: String, cx: &mut Context<Self>) {
        if !self.tree.writable() {
            return;
        }
        let node = self.tree.node.clone();
        self.call(
            NodeId(node.clone()),
            FileDelete { path: path.clone() },
            move |p, answer, cx| {
                if let Err(error) = answer {
                    p.fail(error.message, cx);
                } else {
                    let gone: Vec<store::tabs::Key> = p
                        .tabs
                        .open
                        .iter()
                        .map(|t| t.key.clone())
                        .filter(|k| k.0 == node && paths::inside(&path, &k.1))
                        .collect();
                    for k in gone {
                        p.drafts.remove(&k);
                        p.close_now(k, cx);
                    }
                    if p.opened
                        .as_ref()
                        .is_some_and(|o| o.node == node && paths::inside(&path, &o.path))
                    {
                        p.opened = None;
                    }
                    p.tree.open.remove(&path);
                    p.list(paths::parent(&path), cx);
                    p.refresh_recent(cx);
                }
                cx.notify()
            },
            cx,
        );
    }
    pub(crate) fn pick_uploads(
        &mut self,
        folder: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let node = self.node();
        ahjo_app_kit::widgets::path_picker::pick(
            self.host.clone(),
            "Upload files",
            window,
            cx,
            move |page, paths, cx| page.queue_uploads(node, folder, &paths, cx),
        );
    }
    pub(crate) fn upload_paths(
        &mut self,
        folder: String,
        paths: &[PathBuf],
        cx: &mut Context<Self>,
    ) {
        let node = self.node();
        self.queue_uploads(node, folder, paths, cx);
    }
    fn queue_uploads(
        &mut self,
        node: NodeId,
        folder: String,
        paths: &[PathBuf],
        cx: &mut Context<Self>,
    ) {
        self.uploads
            .extend(paths.iter().cloned().map(|source| Upload {
                source,
                node: node.clone(),
                folder: folder.clone(),
            }));
        self.next_upload(cx);
    }
    fn next_upload(&mut self, cx: &mut Context<Self>) {
        if self.uploading {
            return;
        }
        let Some(upload) = self.uploads.pop_front() else {
            return;
        };
        self.uploading = true;
        cx.spawn(async move |this, cx| {
            let source = upload.source.clone();
            let task = cx.background_executor().spawn(async move {
                let name = source
                    .file_name()
                    .and_then(|s| s.to_str())
                    .ok_or_else(|| "The file name is not valid UTF-8.".to_owned())?
                    .to_owned();
                let metadata = std::fs::metadata(&source)
                    .map_err(|e| format!("Cannot read {}: {e}", source.display()))?;
                if !metadata.is_file() {
                    return Err(format!("{} is not a file.", source.display()));
                }
                if metadata.len() > 64 * 1024 * 1024 {
                    return Err(format!("{} is larger than 64 MiB.", source.display()));
                }
                let bytes = std::fs::read(&source)
                    .map_err(|e| format!("Cannot read {}: {e}", source.display()))?;
                Ok((name, bytes))
            });
            let result = task.await;
            let _ = this.update(cx, |page, cx| match result {
                Ok((name, bytes)) => page.write_upload(upload, name, bytes, false, 0, cx),
                Err(error) => {
                    page.fail(error, cx);
                    page.uploading = false;
                    page.next_upload(cx);
                    cx.notify();
                }
            });
        })
        .detach();
    }
    fn write_upload(
        &mut self,
        upload: Upload,
        name: String,
        bytes: Vec<u8>,
        replace: bool,
        ordinal: usize,
        cx: &mut Context<Self>,
    ) {
        let target_name = if ordinal == 0 {
            name.clone()
        } else {
            upload_copy_name(&name, ordinal)
        };
        let path = paths::join(&upload.folder, &target_name);
        let mut request = Request::new(
            if replace { "PUT" } else { "POST" },
            upload.node.clone(),
            bytes_uri(&path),
        );
        request.body = Some(("application/octet-stream".into(), bytes.clone()));
        self.request(
            request,
            move |page, status, body, cx| {
                if (200..300).contains(&status) {
                    if page.tree.node == upload.node.as_str() {
                        page.tree.open.insert(upload.folder.clone());
                        page.list(upload.folder.clone(), cx);
                        page.tree.select(&path);
                        page.pick = Some(Pick::Node(path.clone()));
                    }
                    page.uploading = false;
                    page.next_upload(cx);
                } else if status == 409
                    && code_of(&body).as_deref() == Some(ahjo_files_contract::ALREADY_EXISTS)
                {
                    if ordinal > 0 {
                        page.write_upload(upload, name, bytes, false, ordinal + 1, cx);
                    } else {
                        page.upload_conflict = Some(UploadConflict {
                            upload,
                            name,
                            bytes,
                        });
                    }
                } else {
                    page.fail(message(status, &body), cx);
                    page.uploading = false;
                    page.next_upload(cx);
                }
                cx.notify();
            },
            cx,
        );
    }
    pub(crate) fn upload_conflict_name(&self) -> Option<&str> {
        self.upload_conflict.as_ref().map(|c| c.name.as_str())
    }
    pub(crate) fn resolve_upload(&mut self, replace: Option<bool>, cx: &mut Context<Self>) {
        let Some(conflict) = self.upload_conflict.take() else {
            return;
        };
        match replace {
            Some(replace) => self.write_upload(
                conflict.upload,
                conflict.name,
                conflict.bytes,
                replace,
                if replace { 0 } else { 2 },
                cx,
            ),
            None => {
                self.uploading = false;
                self.next_upload(cx);
            }
        }
        cx.notify();
    }

    /// The menu on a row: what can be done with that file or folder.
    pub fn row_menu(
        &mut self,
        path: String,
        folder: bool,
        at: gpui::Point<gpui::Pixels>,
        from: Option<Pick>,
        cx: &mut Context<Self>,
    ) {
        self.menu_from = from.map(RenameIn::Line);
        let rows = self.menu_rows(path, folder);
        self.host.menu(rows, at, cx);
    }

    /// The viewer's "…": how long lines show, then the file's own menu.
    pub fn file_menu(&mut self, at: gpui::Point<gpui::Pixels>, cx: &mut Context<Self>) {
        let Some(path) = self.opened.as_ref().map(|o| o.path.clone()) else {
            return;
        };
        self.menu_from = Some(RenameIn::Viewer);
        let me = self.me.clone();
        let wraps = self.code.read(cx).wraps();
        let mut rows = vec![
            MenuRow::new(
                if wraps {
                    "Let long lines run on"
                } else {
                    "Wrap long lines"
                },
                move |_, cx| {
                    let _ = me.update(cx, |p, cx| p.set_wrap(!wraps, cx));
                },
            ),
            MenuRow::rule(),
        ];
        rows.extend(self.menu_rows(path, false));
        self.host.menu(rows, at, cx);
    }

    pub fn set_wrap(&mut self, on: bool, cx: &mut Context<Self>) {
        self.code.update(cx, |c, cx| c.set_wrap(on, cx));
        self.pretty_code.update(cx, |c, cx| c.set_wrap(on, cx));
        let mut k: store::Kept = self.host.setting(cx).unwrap_or_default();
        k.unwrapped = !on;
        let _ = self.host.set_setting(&k, cx);
        cx.notify();
    }

    fn menu_rows(&self, path: String, folder: bool) -> Vec<MenuRow> {
        let me = self.me.clone();
        let node = self.node();
        let root = self.tree.roots.iter().any(|r| r.path == path);
        let writable = self.tree.writable();
        let with = |f: fn(&mut FilesPage, String, &mut Window, &mut Context<FilesPage>)| {
            let me = me.clone();
            let path = path.clone();
            move |window: &mut Window, cx: &mut App| {
                let _ = me.update(cx, |p, cx| f(p, path.clone(), window, cx));
            }
        };
        let mut rows = vec![
            MenuRow::new(
                "Send to session",
                with(|p, path, _, cx| p.send_path(path, cx)),
            ),
            MenuRow::new("Copy path", {
                let path = path.clone();
                move |_, cx| cx.write_to_clipboard(gpui::ClipboardItem::new_string(path.clone()))
            }),
            MenuRow::new("Show in tree", {
                with(|p, path, _, cx| {
                    if p.tree.row(&path).is_some_and(|r| r.folder) {
                        p.show_folder(path, cx)
                    } else {
                        p.show_in_tree(&path, cx);
                    }
                })
            }),
        ];
        if !folder {
            rows.push(MenuRow::new("Download", {
                let path = path.clone();
                let me = me.clone();
                move |_, cx| {
                    let _ = me.update(cx, |p, cx| p.download_path(node.clone(), path.clone(), cx));
                }
            }));
        }
        if writable {
            rows.push(MenuRow::rule());
            let here = if folder {
                path.clone()
            } else {
                paths::parent(&path)
            };
            rows.extend(self.new_rows(here));
            if !root {
                rows.push(MenuRow::new(
                    "Rename",
                    with(|p, path, w, cx| {
                        let from = p.menu_from.clone();
                        p.start_rename_in(path, from, w, cx)
                    }),
                ));
                rows.push(
                    MenuRow::new("Delete", with(|p, path, _, cx| p.delete(path, cx))).danger(),
                );
            }
        }
        rows
    }

    /// What can be made in `folder`: a file, a folder, an upload.
    fn new_rows(&self, folder: String) -> Vec<MenuRow> {
        let me = self.me.clone();
        let in_folder = |f: fn(&mut FilesPage, String, &mut Window, &mut Context<FilesPage>)| {
            let me = me.clone();
            let folder = folder.clone();
            move |window: &mut Window, cx: &mut App| {
                let _ = me.update(cx, |p, cx| f(p, folder.clone(), window, cx));
            }
        };
        vec![
            MenuRow::new(
                "New file",
                in_folder(|p, here, w, cx| p.start_naming(Naming::File(here), w, cx)),
            ),
            MenuRow::new(
                "New folder",
                in_folder(|p, here, w, cx| p.start_naming(Naming::Folder(here), w, cx)),
            ),
            MenuRow::new(
                "Upload…",
                in_folder(|p, here, w, cx| p.pick_uploads(here, w, cx)),
            ),
        ]
    }
    /// New, over the list: what can be made in the folder picked, and
    /// nothing done to that folder itself.
    pub fn new_menu(
        &mut self,
        folder: String,
        at: gpui::Point<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.menu_from = None;
        let rows = self.new_rows(folder);
        self.host.menu(rows, at, cx);
    }

    pub(crate) fn download_path(&mut self, node: NodeId, path: String, cx: &mut Context<Self>) {
        let kept: store::Kept = self.host.setting(cx).unwrap_or_default();
        let folder = if kept.download_folder.trim().is_empty() {
            std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
                .map(|p| std::path::PathBuf::from(p).join("Downloads"))
        } else {
            Some(std::path::PathBuf::from(kept.download_folder))
        };
        let Some(folder) = folder else {
            self.fail(
                "Cannot find your Downloads folder; set a download folder in Settings.".into(),
                cx,
            );
            return;
        };
        let destination = folder.join(paths::base_name(&path));
        self.request(
            Request::get(node, bytes_uri(&path)),
            move |p, status, body, cx| {
                if status != 200 {
                    p.fail(message(status, &body), cx);
                    return;
                }
                cx.spawn(async move |this, cx| {
                    let task = cx.background_executor().spawn(async move {
                        use std::io::Write;
                        let result = std::fs::create_dir_all(&folder).and_then(|_| {
                            let mut file = std::fs::OpenOptions::new()
                                .write(true)
                                .create_new(true)
                                .open(&destination)?;
                            file.write_all(&body)
                        });
                        (destination, result)
                    });
                    let (destination, result) = task.await;
                    let _ = this.update(cx, |page, cx| match result {
                        Ok(()) => page.host.report(
                            Status::new(format!("Downloaded to {}", destination.display())),
                            cx,
                        ),
                        Err(e) => page.fail(
                            format!("Could not download to {}: {e}", destination.display()),
                            cx,
                        ),
                    });
                })
                .detach();
            },
            cx,
        );
    }
    /// Where the app cannot run a page inside its window: the open web
    /// page opens in the system's browser, from its own folder so what it
    /// loads comes with it, once for each time it is asked for. Only this
    /// computer's own files can.
    fn run_outside(&mut self, cx: &mut Context<Self>) {
        if ahjo_app_kit::web_view::SHOWS_INSIDE || self.unasked {
            return;
        }
        let local = |node: &str, cx: &mut Context<Self>| {
            self.host
                .reach(cx)
                .is_some_and(|r| r.on_this_computer(node))
        };
        let Some(o) = self
            .opened
            .as_ref()
            .filter(|o| !o.ran_outside && store::kinds::kind(&o.name) == store::kinds::Kind::Html)
        else {
            return;
        };
        let file = std::path::PathBuf::from(&o.path);
        if local(&o.node.clone(), cx) && file.is_file() {
            cx.open_with_system(&file);
        }
        if let Some(o) = self.opened.as_mut() {
            o.ran_outside = true;
        }
    }
    /// The open file runs as a web page: one, not asked for at a line, not
    /// being edited.
    pub(crate) fn runs_page(&self) -> bool {
        self.opened.as_ref().is_some_and(|o| {
            store::kinds::kind(&o.name) == store::kinds::Kind::Html
                && !o.editing
                && !o.source
                && !o.diff_open
                && !self.drafts.contains_key(&o.key())
                && !matches!(o.show, Show::Failed(_) | Show::Gone)
        })
    }
    /// Between the page running and its source.
    pub fn toggle_source(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(o) = self.opened.as_mut() else {
            return;
        };
        o.source = !o.source;
        if o.source {
            self.focus_file(window, cx);
        }
        cx.notify();
    }
    pub fn play(&mut self, cx: &mut Context<Self>) {
        let Some(opened) = self.opened.as_ref() else {
            return;
        };
        let node = opened.node.clone();
        let path = opened.path.clone();
        let name = opened.name.clone();
        self.request(
            Request::get(NodeId(node.clone()), bytes_uri(&path)),
            move |p, s, b, cx| {
                if s != 200 {
                    p.fail(message(s, &b), cx)
                } else {
                    let key = open::version(format!("{node}\n{path}").as_bytes());
                    let dir = std::env::temp_dir().join("ahjo-open").join(&key[..16]);
                    let dest = dir.join(&name);
                    match std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&dest, b)) {
                        Ok(()) => cx.open_with_system(&dest),
                        Err(e) => p.fail(e.to_string(), cx),
                    };
                }
                cx.notify();
            },
            cx,
        )
    }
    pub fn follow_link(
        &mut self,
        node: &str,
        folder: &str,
        link: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if link.starts_with("http://")
            || link.starts_with("https://")
            || link.starts_with("mailto:")
        {
            cx.open_url(link);
            return;
        }
        let link = link.split('#').next().unwrap_or_default();
        if link.is_empty() || self.tree.node != node {
            return;
        }
        let at = resolve(folder, link);
        if !self.tree.roots.is_empty() && paths::root_of(&self.tree.roots, &at).is_none() {
            self.fail("That link is outside the roots.".into(), cx);
            return;
        }
        self.show_in_tree(&at, cx);
        self.open(at, window, cx);
    }
    pub fn send_path(&self, path: String, cx: &mut Context<Self>) {
        let ask = Ask::Send {
            reference: paths::reference(&path),
            target: None,
        };
        if let Err(e) = self.host.ask(ask, cx) {
            self.fail(format!("Not sent: {}", e.message), cx);
        }
    }

    /// Sends the open file to a session. Lines selected in it are named
    /// after the path (`path:12-18`).
    pub fn send(&self, cx: &mut Context<Self>) {
        let Some(o) = self.opened.as_ref() else {
            return;
        };
        let lines = matches!(o.show, Show::Text { .. })
            .then(|| self.code.read(cx).selected_lines())
            .flatten();
        let path = match lines {
            Some((first, last)) if first == last => format!("{}:{first}", o.path),
            Some((first, last)) => format!("{}:{first}-{last}", o.path),
            None => o.path.clone(),
        };
        self.send_path(path, cx);
    }
    pub(crate) fn keep(&self, cx: &mut Context<Self>) {
        if self.split_of.is_some() {
            return;
        }
        let mut k: store::Kept = self.host.setting(cx).unwrap_or_default();
        if self.is_panel {
            k.panel_tree = self.tree.kept();
            k.panel_file = self.opened.as_ref().map(|o| o.path.clone());
        } else {
            k.tree = self.tree.kept();
            k.file = self
                .opened
                .as_ref()
                .filter(|_| !self.shown_newest)
                .map(|o| o.path.clone());
            k.tabs = self
                .tabs
                .open
                .iter()
                .filter(|t| !t.preview && t.key.0 == self.tree.node)
                .map(|t| t.key.1.clone())
                .collect();
        }
        if k != self.host.setting(cx).unwrap_or_default() {
            let _ = self.host.set_setting(&k, cx);
        }
    }

    /// Whether anyone sees this view now.
    fn seen_now(&self, cx: &App) -> bool {
        if let Some(of) = &self.split_of {
            return of.upgrade().is_some_and(|p| {
                let p = p.read(cx);
                p.split.is_some() && p.seen_now(cx)
            });
        }
        if self.is_panel {
            self.host.panel_shows(PANEL, cx) && self.host.shown_page(cx) != Some(<Self as Page>::ID)
        } else {
            self.shown
        }
    }

    /// While the view is seen and the window active, asks every few
    /// seconds whether the open file or an open folder changed on disk, and
    /// reads again what did. Asking is cheap: when each last changed.
    pub fn watch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.watching {
            return;
        }
        self.watching = true;
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(WATCH).await;
                let go = this.update_in(cx, |p, window, cx| {
                    if !p.seen_now(cx) {
                        p.watching = false;
                        p.seen.clear();
                        return false;
                    }
                    if window.is_window_active() {
                        p.check(cx);
                    }
                    true
                });
                if !matches!(go, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }
    fn check(&mut self, cx: &mut Context<Self>) {
        let mut asked: Vec<String> = Vec::new();
        if let Some(o) = self
            .opened
            .as_ref()
            .filter(|o| !matches!(o.show, Show::Reading | Show::Media(_)) && !o.saving)
        {
            asked.push(o.path.clone());
        }
        // In the panel, the list is seen only while no file is open.
        if !self.is_panel || self.opened.is_none() {
            asked.extend(
                self.tree
                    .open_folders()
                    .into_iter()
                    .filter(|f| self.tree.listings.contains_key(f)),
            );
        }
        if asked.is_empty() || self.tree.node.is_empty() {
            return;
        }
        let node = self.tree.node.clone();
        self.call(
            NodeId(node.clone()),
            FileStat {
                paths: asked.clone(),
            },
            move |p, answer, cx| {
                if p.tree.node != node {
                    return;
                }
                let Ok(answer) = answer else {
                    return;
                };
                let mut any = false;
                for (path, time) in asked.into_iter().zip(answer.times) {
                    // The first answer is checked too: the file may have
                    // changed between its read and this look.
                    if p.seen.insert(path.clone(), time) == Some(time) {
                        continue;
                    }
                    any = true;
                    if let Some(o) = p.opened.as_ref().filter(|o| o.path == path) {
                        if time.is_some() {
                            let node = o.node.clone();
                            p.read(node, path, true, cx);
                        }
                    } else if p.tree.listings.contains_key(&path) {
                        p.list(path, cx);
                    }
                }
                // Recent changes walk every root: a change seen here asks
                // for them again only now and then.
                if any {
                    p.refresh_status(cx);
                }
                if any && p.recent_at.is_none_or(|at| at.elapsed() >= RECENT_AGAIN) {
                    p.refresh_recent(cx);
                }
            },
            cx,
        );
    }
}

/// The layouts being compared, and what reading needs in each of them:
/// the find over the file, going back and forth, the changes in a file and
/// a folder shown as a place.
impl FilesPage {
    /// The layout changed: start it as it starts.
    pub(crate) fn laid_out(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.palette = false;
        if self.side != Side::Hidden {
            self.side_before = self.side;
        }
        if self.side == Side::Hidden && self.opened.is_none() && self.place.is_none() {
            self.find_anywhere(window, cx);
        }
        cx.notify();
    }

    pub fn set_side(&mut self, layout: Side, window: &mut Window, cx: &mut Context<Self>) {
        self.side = layout;
        if layout == Side::Changes {
            self.ask_session_status(cx);
        }
        let mut kept: store::Kept = self.host.setting(cx).unwrap_or_default();
        if kept.side != layout {
            kept.side = layout;
            let _ = self.host.set_setting(&kept, cx);
        }
        self.laid_out(window, cx);
    }

    /// The find takes the keyboard: in the list, or (Reader) opened over
    /// the file.
    pub fn find_anywhere(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.side == Side::Hidden && !self.is_panel {
            self.palette = true;
        }
        ahjo_app_kit::widgets::field::focus(&self.query, window, cx);
        cx.notify();
    }

    pub fn close_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.palette = false;
        self.clear_find(cx);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// The open file's change marks on its lines, none while it is being
    /// edited (its lines move as you type).
    pub(crate) fn apply_marks(&self, cx: &mut Context<Self>) {
        let marks = match &self.opened {
            Some(o) if !o.editing => {
                let last = o.saved.lines().count().saturating_sub(1);
                o.changes.as_ref().map_or(vec![], |c| {
                    c.marks.iter().map(|&(l, m)| (l.min(last), m)).collect()
                })
            }
            _ => vec![],
        };
        self.code.update(cx, |c, cx| c.set_marks(marks, cx));
    }

    /// The rendered Markdown's blocks with what changed in each.
    pub fn block_marks(&self) -> Vec<Option<ahjo_app_kit::widgets::code::Mark>> {
        let Some(o) = &self.opened else {
            return vec![];
        };
        let Some(changes) = &o.changes else {
            return vec![];
        };
        let ranges: Vec<_> = ahjo_app_kit::widgets::markdown::parse_lines(&o.saved)
            .into_iter()
            .map(|(_, r)| r)
            .collect();
        ahjo_app_kit::widgets::markdown::block_marks(&ranges, &changes.marks)
    }

    /// Scrolls to the next change (or the one before): in the code, or to
    /// the next changed block of rendered Markdown.
    pub fn step_change(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(o) = &self.opened else {
            return;
        };
        if o.diff_open {
            self.step_diff(forward, cx);
            return;
        }
        if o.changes.as_ref().is_none_or(|c| c.marks.is_empty()) {
            return;
        }
        if !crate::view::rendered(self) {
            self.code.update(cx, |c, cx| {
                c.step_change(forward, cx);
            });
            return;
        }
        let marked: Vec<usize> = self
            .block_marks()
            .iter()
            .enumerate()
            .filter_map(|(i, m)| m.map(|_| i))
            .collect();
        let top = self.md_scroll.bounds().top();
        // The block at the top of the view now, a little room given.
        let here = self
            .md_blocks
            .borrow()
            .iter()
            .position(|b| b.bottom() > top + gpui::px(crate::view::GO_ROOM + 1.))
            .unwrap_or(0);
        let next = if forward {
            marked.into_iter().find(|&i| i > here)
        } else {
            marked.into_iter().rev().find(|&i| i < here)
        };
        if let Some(i) = next {
            self.md_go.set(Some(i));
            cx.notify();
        }
    }

    /// Scrolls to the open file's first change.
    fn first_change(&mut self, cx: &mut Context<Self>) {
        if crate::view::rendered(self) {
            if let Some(i) = self.block_marks().iter().position(Option::is_some) {
                self.md_go.set(Some(i));
            }
            return;
        }
        let first = self
            .code
            .read(cx)
            .marks()
            .first()
            .map(|m| m.0.saturating_sub(crate::view::GO_LINES));
        if let Some(line) = first {
            self.code.update(cx, |c, cx| c.show_line(line, cx));
        }
    }

    /// Shows a folder as a place: who works there, what changed in it, its
    /// documents.
    pub fn show_place(&mut self, folder: String, cx: &mut Context<Self>) {
        if self.place.as_deref() == Some(folder.as_str()) {
            return;
        }
        self.place = Some(folder.clone());
        self.place_recent = None;
        self.place_readme = None;
        self.palette = false;
        let readme = paths::join(&folder, "README.md");
        self.request(
            Request::get(self.node(), bytes_uri(&readme)),
            move |p, s, b, cx| {
                if s == 200 && p.place.as_deref() == Some(paths::parent(&readme).as_str()) {
                    let text = String::from_utf8_lossy(&b);
                    p.place_readme = Some(ahjo_app_kit::widgets::markdown::parse(&text));
                    cx.notify();
                }
            },
            cx,
        );
        if !self.tree.listings.contains_key(&folder) {
            self.list(folder.clone(), cx);
        }
        let node = self.tree.node.clone();
        self.call(
            NodeId(node.clone()),
            FileListRecent {
                path: Some(folder.clone()),
            },
            move |p, answer, cx| {
                if p.tree.node != node || p.place.as_deref() != Some(folder.as_str()) {
                    return;
                }
                p.place_recent = Some(answer.map(|r| r.files).map_err(|error| error.message));
                cx.notify();
            },
            cx,
        );
        cx.notify();
    }

    /// The live sessions on this node working where `path` is.
    pub fn sessions_at(&self, path: &str) -> Vec<&ahjo_terminals_contract::Session> {
        store::who::working_at(&self.live, &self.tree.node, path)
    }

    /// Brings a session forward in the terminals.
    pub fn reveal_session(&self, s: &ahjo_terminals_contract::Session, cx: &mut Context<Self>) {
        let ask = Ask::Reveal {
            what: "session".into(),
            id: s.name.clone(),
            node: Some(NodeId(s.node.clone())),
            part: None,
            beside: false,
        };
        if let Err(e) = self.host.ask(ask, cx) {
            self.fail(e.message, cx);
        }
    }

    /// Starts a session in `folder`.
    pub fn launch_in(&self, folder: &str, cx: &mut Context<Self>) {
        let ask = Ask::Launch {
            node: self.node(),
            folder: folder.to_owned(),
            name: paths::base_name(folder).to_owned(),
        };
        if let Err(e) = self.host.ask(ask, cx) {
            self.fail(e.message, cx);
        }
    }
}
