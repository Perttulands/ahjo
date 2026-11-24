//! Moving through many files: tabs that preview then keep, the order they
//! were shown in (Ctrl+Tab), closing and opening again, going back and
//! forth to the place read, up to the folder, the find inside the open
//! file, going to a line or a symbol, what git says changed, and a second
//! group of tabs beside the first.
use super::*;
use crate::store::{symbols, tabs::Key};
use ahjo_app_kit::widgets::code::Place;
use std::ops::Range;
use std::time::Instant;

/// How long letters typed in the list add up to one name.
const TYPE_AHEAD: Duration = Duration::from_millis(900);
/// Places kept to go back to.
const HISTORY: usize = 100;

/// Where the reader was, to go back to.
#[derive(Clone, Debug, PartialEq)]
pub enum Spot {
    File {
        node: String,
        path: String,
        place: Option<Place>,
    },
    Folder(String),
}

/// A tab not shown, as it was left: the file as read, and where the
/// reader was in it.
pub(crate) struct TabKept {
    opened: Opened,
    place: Place,
    md: gpui::Point<gpui::Pixels>,
}

impl TabKept {
    /// The file as kept: a save that answers while it is not shown
    /// updates it here.
    pub(crate) fn opened_mut(&mut self) -> &mut Opened {
        &mut self.opened
    }
}

/// The find inside the open file: what it found, the one gone to, and
/// whether it replaces too.
#[derive(Clone, Debug, Default)]
pub struct InFile {
    pub found: Vec<Range<usize>>,
    pub at: Option<usize>,
    pub replace: bool,
}

/// The find box going somewhere in the open file instead of finding files:
/// `:42` to a line, `@name` to a symbol.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Jump {
    Line(Option<usize>),
    Symbol(String),
}

impl Jump {
    pub fn read(query: &str) -> Option<Jump> {
        let q = query.trim();
        if let Some(n) = q.strip_prefix(':') {
            return Some(Jump::Line(n.trim().parse().ok().filter(|&n| n > 0)));
        }
        q.strip_prefix('@').map(|s| Jump::Symbol(s.to_owned()))
    }
}

impl FilesPage {
    /// Where the reader is now.
    pub(crate) fn spot(&self, cx: &App) -> Option<Spot> {
        if let Some(folder) = &self.place {
            return Some(Spot::Folder(folder.clone()));
        }
        let o = self.opened.as_ref()?;
        Some(Spot::File {
            node: o.node.clone(),
            path: o.path.clone(),
            place: matches!(o.show, Show::Text { .. }).then(|| self.shown_code().read(cx).place()),
        })
    }

    /// Keeps where the reader is, to come back to; not while going back
    /// and forth.
    pub(crate) fn remember(&mut self, cx: &App) {
        if self.travelling {
            return;
        }
        let Some(spot) = self.spot(cx) else {
            return;
        };
        if self.back.last() != Some(&spot) {
            self.back.push(spot);
            if self.back.len() > HISTORY {
                self.back.remove(0);
            }
        }
        self.forward.clear();
    }

    /// Back to where the reader was before, or forward again: the file at
    /// the line it was read at, or the folder.
    pub fn travel(&mut self, back: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(spot) = (if back {
            self.back.pop()
        } else {
            self.forward.pop()
        }) else {
            return;
        };
        if let Some(now) = self.spot(cx) {
            if back {
                self.forward.push(now)
            } else {
                self.back.push(now)
            }
        }
        self.travelling = true;
        match spot {
            Spot::Folder(folder) => {
                self.show_in_tree(&folder, cx);
                self.show_place(folder, cx);
            }
            Spot::File { node, path, place } => {
                self.on_node(&node, cx);
                let same =
                    self.opened.as_ref().is_some_and(|o| o.path == path) && self.place.is_none();
                self.show_in_tree(&path, cx);
                if same {
                    if let Some(place) = place {
                        self.shown_code()
                            .update(cx, |c, cx| c.set_place(&place, cx));
                    }
                } else {
                    self.open(path, window, cx);
                    if let Some(place) = place {
                        // Restored from its tab, it is shown already.
                        if self
                            .opened
                            .as_ref()
                            .is_some_and(|o| !matches!(o.show, Show::Reading))
                        {
                            self.shown_code()
                                .update(cx, |c, cx| c.set_place(&place, cx));
                        } else {
                            self.pending_place = Some(place);
                        }
                    }
                }
            }
        }
        self.travelling = false;
        cx.notify();
    }

    pub fn can_travel(&self, back: bool) -> bool {
        !if back { &self.back } else { &self.forward }.is_empty()
    }

    /// Up from the file to its folder shown as a place, and from a place to
    /// the folder it is in; Back comes down again.
    pub fn up(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let here = match (&self.place, &self.opened) {
            (Some(folder), _) => folder.clone(),
            (None, Some(o)) => paths::join(&paths::parent(&o.path), "x"),
            (None, None) => match self.picked_path() {
                Some(p) => p,
                None => return,
            },
        };
        let folder = paths::parent(&here);
        // Not above the roots: they are where Files reaches.
        if folder.is_empty() || paths::root_of(&self.tree.roots, &folder).is_none() {
            return;
        }
        self.remember(cx);
        if std::mem::take(&mut self.palette) {
            window.focus(&self.focus, cx);
        }
        self.show_in_tree(&folder, cx);
        self.show_place(folder, cx);
    }

    /// The code view the open file shows in: its laid-out JSON, or its text.
    pub fn shown_code(&self) -> Entity<Code> {
        match &self.opened {
            Some(o)
                if o.pretty.is_some()
                    && !o.editing
                    && !o.source
                    && !self.drafts.contains_key(&o.key()) =>
            {
                self.pretty_code.clone()
            }
            _ => self.code.clone(),
        }
    }

    /// Keeps the open file's tab as it is, to come back to at once.
    pub(crate) fn stash(&mut self, cx: &App) {
        let Some(o) = self.opened.as_ref() else {
            return;
        };
        // A save on its way is kept too: its answer finds the tab here.
        if !matches!(o.show, Show::Text { .. }) || self.tabs.at(&o.key()).is_none() {
            return;
        }
        let kept = TabKept {
            opened: o.clone(),
            place: self.shown_code().read(cx).place(),
            md: self.md_scroll.offset(),
        };
        self.tab_kept.insert(o.key(), kept);
    }

    /// A tab comes back as it was left, then is read again in case the
    /// file changed meanwhile.
    pub(crate) fn restore(&mut self, kept: TabKept, window: &mut Window, cx: &mut Context<Self>) {
        let TabKept { opened, place, md } = kept;
        let key = opened.key();
        let draft = self.drafts.get(&key).map(|d| d.text.clone());
        let text = draft.clone().unwrap_or_else(|| opened.saved.clone());
        let lang = syntax::lang(&opened.name);
        let pretty = opened.pretty.clone();
        let editing = opened.editing;
        self.symbols = store::symbols::of(&opened.name, &text);
        self.pending_line = None;
        self.pending_column = None;
        self.confirm_discard = false;
        self.conflict = false;
        self.shown_newest = false;
        self.md_go.set(None);
        self.md_select.borrow_mut().selection.clear();
        self.follow_open(&opened.path);
        let (node, path) = (opened.node.clone(), opened.path.clone());
        self.opened = Some(opened);
        self.pretty_code.update(cx, |c, cx| {
            c.set_lang(lang, cx);
            c.set(pretty.as_deref().unwrap_or(&text), cx);
        });
        self.code.update(cx, |c, cx| {
            c.set_lang(lang, cx);
            c.set(&text, cx);
            c.edit(editing, window, cx);
        });
        self.shown_code()
            .update(cx, |c, cx| c.set_place(&place, cx));
        self.md_scroll.set_offset(md);
        self.apply_marks(cx);
        if self.infile.is_some() {
            self.refind(false, cx);
        }
        self.keep(cx);
        self.read(node, path, true, cx);
        // What changed may have, too, or not have been read before it was
        // left.
        self.read_changes(cx);
        cx.notify();
    }

    /// The tree marks the file shown, and the keyboard's row follows it
    /// there when the row shows (Ctrl+Shift+E opens the folders down to it).
    /// A find, or a row picked in Recent, keeps its own pick.
    pub(crate) fn follow_open(&mut self, path: &str) {
        self.tree.select(path);
        let in_tree = matches!(self.pick, None | Some(Pick::Node(_)));
        if self.find.is_none() && self.jump.is_none() && in_tree && self.tree.row(path).is_some() {
            self.pick = Some(Pick::Node(path.to_owned()));
            self.scroll_pending = !self.scroll_to_pick();
        }
    }

    /// Scrolls the tab strip to `key`'s tab.
    pub(crate) fn tab_into_view(&self, key: &Key) {
        if let Some(i) = self.tabs.at(key) {
            self.tabs_scroll.scroll_to_item(i);
        }
    }

    /// The same once the strip has been drawn: tabs just put there have
    /// no place to scroll to before that.
    pub(crate) fn tab_into_view_soon(&self, window: &mut Window, cx: &mut Context<Self>) {
        cx.on_next_frame(window, |page, _, cx| {
            if let Some(key) = page.opened.as_ref().map(|o| o.key()) {
                page.tab_into_view(&key);
                cx.notify();
            }
        });
    }

    /// Shows a tab's file, keeping the tab as it is (kept or a preview).
    pub fn show_tab(&mut self, key: Key, window: &mut Window, cx: &mut Context<Self>) {
        self.on_node(&key.0, cx);
        let keep = !self.tabs.is_preview(&key);
        self.open_as(key.1, keep, window, cx);
    }

    /// Closes a tab; one with unsaved changes asks first.
    pub fn close_tab(&mut self, key: Key, window: &mut Window, cx: &mut Context<Self>) {
        if self.drafts.contains_key(&key) {
            if self.opened.as_ref().is_none_or(|o| o.key() != key) {
                self.show_tab(key.clone(), window, cx);
            }
            self.closing = Some(key);
            cx.notify();
            return;
        }
        self.close_now(key, cx);
        self.after_close(window, cx);
    }

    /// The tab shown after one closed: the one shown before it.
    fn after_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.opened.is_none()
            && let Some(next) = self.tabs.recent().first().cloned()
        {
            self.show_tab(next, window, cx);
        }
        cx.notify();
    }

    /// Closes a tab now, its unsaved changes with it.
    pub(crate) fn close_now(&mut self, key: Key, cx: &mut Context<Self>) {
        self.tab_kept.remove(&key);
        self.tabs.close(&key);
        if self.closing.as_ref() == Some(&key) {
            self.closing = None;
        }
        if self.opened.as_ref().is_some_and(|o| o.key() == key) {
            self.opened = None;
            self.symbols.clear();
            self.code.update(cx, |c, cx| c.set("", cx));
            self.pretty_code.update(cx, |c, cx| c.set("", cx));
            if let Some(of) = self.split_of.clone()
                && self.tabs.open.is_empty()
            {
                // The second group goes when its last tab does.
                cx.defer(move |cx| {
                    let _ = of.update(cx, |p, cx| {
                        p.split = None;
                        cx.notify();
                    });
                });
            }
        }
        self.keep(cx);
        cx.notify();
    }

    /// The tab asked to close with unsaved changes: saved first, or not.
    pub fn close_saving(&mut self, save: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(key) = self.closing.take() else {
            return;
        };
        if save {
            self.close_after_save = Some(key);
            self.save(cx);
        } else {
            self.drafts.remove(&key);
            self.close_now(key, cx);
            self.after_close(window, cx);
        }
        cx.notify();
    }

    pub fn close_cancel(&mut self, cx: &mut Context<Self>) {
        self.closing = None;
        cx.notify();
    }

    /// Closes every other tab (or those right of it), but not one with
    /// unsaved changes.
    pub fn close_others(
        &mut self,
        key: Key,
        right: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let drafts = &self.drafts;
        let closing = if right {
            self.tabs.right_of(&key, |k| drafts.contains_key(k))
        } else {
            self.tabs.others(&key, |k| drafts.contains_key(k))
        };
        for k in closing {
            self.close_now(k, cx);
        }
        if self.opened.as_ref().is_none_or(|o| o.key() != key) {
            self.show_tab(key, window, cx);
        }
    }

    /// Closes every tab without unsaved changes.
    pub fn close_saved(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let saved: Vec<Key> = self
            .tabs
            .open
            .iter()
            .map(|t| t.key.clone())
            .filter(|k| !self.drafts.contains_key(k))
            .collect();
        for k in saved {
            self.close_now(k, cx);
        }
        self.after_close(window, cx);
    }

    /// Opens again the tab closed last.
    pub fn reopen_closed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(key) = self.tabs.reopen() {
            self.on_node(&key.0, cx);
            self.open_as(key.1, true, window, cx);
        }
    }

    /// The tab `by` places along from the one shown.
    pub fn step_tab(&mut self, by: isize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(key) = self
            .opened
            .as_ref()
            .and_then(|o| self.tabs.beside(&o.key(), by))
        {
            self.show_tab(key, window, cx);
        }
    }

    /// Ctrl+Tab: through the tabs in the order last shown, while Ctrl is
    /// held; letting go shows the one it is on.
    pub fn switch_step(&mut self, forward: bool, cx: &mut Context<Self>) {
        let n = self.tabs.recent().len();
        if n < 2 {
            return;
        }
        let at = match self.switching {
            None if forward => 1,
            None => n - 1,
            Some(i) if forward => (i + 1) % n,
            Some(i) => (i + n - 1) % n,
        };
        self.switching = Some(at);
        cx.notify();
    }

    pub fn switch_done(&mut self, show: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(at) = self.switching.take() else {
            return;
        };
        if show && let Some(key) = self.tabs.recent().get(at).cloned() {
            self.show_tab(key, window, cx);
            self.focus_file(window, cx);
        }
        cx.notify();
    }

    /// A tab's menu.
    pub fn tab_menu(&mut self, key: Key, at: gpui::Point<gpui::Pixels>, cx: &mut Context<Self>) {
        let me = self.me.clone();
        let row =
            |label: &str, f: fn(&mut FilesPage, Key, &mut Window, &mut Context<FilesPage>)| {
                let me = me.clone();
                let key = key.clone();
                MenuRow::new(label, move |w, cx| {
                    let _ = me.update(cx, |p, cx| f(p, key.clone(), w, cx));
                })
            };
        let preview = self.tabs.is_preview(&key);
        let mut rows = vec![
            row("Close", |p, k, w, cx| p.close_tab(k, w, cx)).chord("Ctrl+W"),
            row("Close others", |p, k, w, cx| {
                p.close_others(k, false, w, cx)
            }),
            row("Close to the right", |p, k, w, cx| {
                p.close_others(k, true, w, cx)
            }),
            row("Close saved", |p, _, w, cx| p.close_saved(w, cx)),
            MenuRow::rule(),
        ];
        if preview {
            rows.push(row("Keep open", |p, k, _, cx| {
                p.tabs.keep(&k);
                p.keep(cx);
                cx.notify();
            }));
        }
        if self.split_of.is_none() {
            rows.push(
                row("Open to the side", |p, k, w, cx| {
                    p.split_open(Some(k), w, cx)
                })
                .chord("Ctrl+\\"),
            );
        }
        rows.push(row("Show in the folders", |p, k, _, cx| {
            if p.side == Side::Hidden {
                p.side = p.side_before;
            }
            p.show_in_tree(&k.1, cx);
        }));
        rows.push(row("Copy path", |_, k, _, cx| {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(k.1))
        }));
        self.host.menu(rows, at, cx);
    }

    /// Ctrl+B: the side hidden, or shown again as it was.
    pub fn toggle_side(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let next = if self.side == Side::Hidden {
            self.side_before
        } else {
            Side::Hidden
        };
        self.set_side(next, window, cx);
    }

    /// Ctrl+Shift+E: the keyboard to the folders, on the open file's row.
    pub fn focus_folders(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_file_due = false;
        if self.side != Side::Folders {
            self.set_side(Side::Folders, window, cx);
        }
        if let Some(path) = self.opened.as_ref().map(|o| o.path.clone()) {
            self.show_in_tree(&path, cx);
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// The keyboard to the file shown.
    pub(crate) fn focus_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_file_due = false;
        if self.place.is_some() || self.reading() || self.runs_page() {
            window.focus(&self.focus, cx);
            return;
        }
        match self.opened.as_ref().map(|o| &o.show) {
            Some(Show::Text { .. }) => Code::focus(&self.shown_code(), window, cx),
            // Not read yet, so not shown: the page holds the keyboard
            // until the text is there to take it.
            Some(Show::Reading) => {
                window.focus(&self.focus, cx);
                self.focus_file_due = true;
            }
            _ => {}
        }
    }

    pub(crate) fn picked_folder(&self) -> bool {
        match &self.pick {
            Some(Pick::Node(p)) => self.tree.row(p).is_some_and(|r| r.folder),
            Some(Pick::Recent(p)) => self.recent_folder(p),
            Some(Pick::Group(_)) => true,
            _ => false,
        }
    }

    /// Rows a page of the list holds.
    pub(crate) fn list_page(&self) -> usize {
        let h = f32::from(self.list_scroll.0.borrow().base_handle.bounds().size.height);
        ((h / ahjo_app_kit::theme::size::ROW).floor() as usize).max(2) - 1
    }

    pub(crate) fn typing_ahead(&self) -> bool {
        self.typed_ahead
            .1
            .is_some_and(|at| at.elapsed() < TYPE_AHEAD)
            && !self.typed_ahead.0.is_empty()
    }

    /// Letters typed in the list: to the next row whose name starts with
    /// all typed so far, from the one picked.
    pub(crate) fn type_ahead(&mut self, ch: &str, cx: &mut Context<Self>) {
        if !self.typing_ahead() {
            self.typed_ahead.0.clear();
        }
        self.typed_ahead.0.push_str(&ch.to_lowercase());
        self.typed_ahead.1 = Some(Instant::now());
        let typed = self.typed_ahead.0.clone();
        let picks = self.picks();
        let at = self
            .pick
            .as_ref()
            .and_then(|p| picks.iter().position(|q| q == p))
            .unwrap_or(0);
        // A first letter looks from the row after; more letters from here.
        let from = if typed.chars().count() == 1 {
            at + 1
        } else {
            at
        };
        let name = |p: &Pick| -> Option<String> {
            match p {
                Pick::Node(path) | Pick::Recent(path) | Pick::Group(path) | Pick::HitFile(path) => {
                    Some(paths::base_name(path).to_lowercase())
                }
                Pick::Hit(i) => self
                    .find
                    .as_ref()
                    .and_then(|f| f.matches().get(*i).map(|m| m.name.to_lowercase())),
                _ => None,
            }
        };
        let n = picks.len();
        if let Some(found) = (0..n)
            .map(|k| &picks[(from + k) % n])
            .find(|p| name(p).is_some_and(|nm| nm.starts_with(&typed)))
        {
            let found = found.clone();
            self.set_pick(found, cx);
        }
    }

    /// The keys that move between files, wherever the keyboard is in the
    /// page. Answers whether the key was one.
    pub(crate) fn files_key(
        &mut self,
        e: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let k = &e.keystroke;
        let m = k.modifiers;
        let (ctrl, shift, alt) = (m.control || m.platform, m.shift, m.alt);
        match k.key.as_str() {
            "tab" if ctrl && !alt => self.switch_step(!shift, cx),
            "escape" if self.switching.is_some() => self.switch_done(false, window, cx),
            "escape" if self.closing.is_some() => self.close_cancel(cx),
            "pageup" | "pagedown" if ctrl && !alt && !shift => {
                self.step_tab(if k.key == "pageup" { -1 } else { 1 }, window, cx)
            }
            "w" if ctrl && !alt && !shift => match self.opened.as_ref().map(|o| o.key()) {
                Some(key) => self.close_tab(key, window, cx),
                None => return false,
            },
            "t" if ctrl && shift && !alt => self.reopen_closed(window, cx),
            "left" | "right" if alt && !ctrl && !shift => self.travel(k.key == "left", window, cx),
            "up" if alt && !ctrl && !shift => self.up(window, cx),
            "b" if ctrl && !alt && !shift && self.split_of.is_none() && !self.is_panel => {
                self.toggle_side(window, cx)
            }
            "e" if ctrl && shift && !alt && self.split_of.is_none() && !self.is_panel => {
                self.focus_folders(window, cx)
            }
            "p" | "e" if ctrl && !alt && !shift => {
                self.search_inside = false;
                self.reselect_query(cx);
                self.find_anywhere(window, cx)
            }
            "g" if ctrl && !alt && !shift => self.find_with(":", window, cx),
            "o" if ctrl && shift && !alt => self.find_with("@", window, cx),
            "f" if ctrl && shift && !alt => self.search_files(window, cx),
            "f" if ctrl && !alt && !shift && self.opened.is_some() => {
                self.find_in_file(false, window, cx)
            }
            "h" if ctrl && !alt && !shift && self.opened.is_some() => {
                self.find_in_file(true, window, cx)
            }
            "\\" if ctrl && !alt && !shift && self.split_of.is_none() && !self.is_panel => {
                self.split_open(None, window, cx)
            }
            "f3" if self.infile.is_some() => self.infile_step(!shift, cx),
            "escape" if self.infile.is_some() && !self.infile_focused(window, cx) => {
                // Esc in the file while its find is open closes the find.
                if self.opened.as_ref().is_some_and(|o| o.editing) {
                    return false;
                }
                self.close_infile(window, cx)
            }
            _ => return false,
        }
        true
    }

    /// Ctrl held up: the Ctrl+Tab switch shows the tab it is on.
    pub fn modifiers(
        &mut self,
        e: &gpui::ModifiersChangedEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.switching.is_some() && !e.modifiers.control && !e.modifiers.platform {
            self.switch_done(true, window, cx);
        }
    }

    /// What the find box still holds is selected, so typing starts a new
    /// find and Enter repeats the last.
    pub(crate) fn reselect_query(&mut self, cx: &mut Context<Self>) {
        self.query.update(cx, |f, cx| {
            let text = f.text.clone();
            f.set(&text, true, cx)
        });
    }

    /// The find box opened with `start` in it: `:` for a line, `@` for a
    /// symbol.
    pub fn find_with(&mut self, start: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.query.update(cx, |f, cx| f.set(start, false, cx));
        self.find_anywhere(window, cx);
        self.typed(start.to_owned(), cx);
    }

    /// Ctrl+Shift+F: the find box looks inside the files here.
    pub fn search_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_inside = true;
        let selected = self.code.read(cx).selected_text().to_owned();
        if !selected.is_empty() && !selected.contains('\n') {
            self.query.update(cx, |f, cx| f.set(&selected, true, cx));
            self.typed(selected, cx);
        } else {
            self.reselect_query(cx);
        }
        self.find_anywhere(window, cx);
        if self.find.is_some() {
            self.find_in_text(cx);
        }
    }

    /// Goes to line `n` (from 0) of the open file, and to a column (from 0)
    /// on it, the place left kept to come back to.
    pub fn go_to(
        &mut self,
        n: usize,
        column: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.opened.is_none() {
            return;
        }
        if self.place.is_none() {
            self.remember(cx);
        }
        // A rendered file or laid-out JSON goes to the line in its text.
        let source = self.reading()
            || self.runs_page()
            || self.opened.as_ref().is_some_and(|o| o.pretty.is_some());
        if let Some(o) = self.opened.as_mut() {
            o.source |= source;
            o.diff_open = false;
        }
        self.place = None;
        self.code.update(cx, |c, cx| match column {
            Some(column) => c.go_to_column(n, column, cx),
            None => c.go_to_line(n, cx),
        });
        self.palette = false;
        self.clear_find(cx);
        Code::focus(&self.code, window, cx);
        cx.notify();
    }

    /// The lines the find box shows while it goes to a line or a symbol.
    pub fn jump_lines(&self) -> Vec<crate::view::Line> {
        use crate::view::Line;
        let note = |text: String| Line::Note {
            text,
            depth: 0,
            error: false,
        };
        let Some(o) = self
            .opened
            .as_ref()
            .filter(|o| matches!(o.show, Show::Text { .. }))
        else {
            return vec![note("Open a file to go to its lines".into())];
        };
        match &self.jump {
            Some(Jump::Line(n)) => {
                let text = self.drafts.get(&o.key()).map_or(&o.saved, |d| &d.text);
                let lines = text.split('\n').count();
                match n.map(|n| n.min(lines)) {
                    Some(n) => vec![Line::Go(n - 1, format!("Go to line {n} of {lines}"))],
                    None => vec![note(format!("Type a line of {lines} in {}", o.name))],
                }
            }
            Some(Jump::Symbol(q)) => {
                let found = symbols::matching(&self.symbols, q);
                if found.is_empty() {
                    return vec![note(if self.symbols.is_empty() {
                        format!("No symbols found in {}", o.name)
                    } else {
                        format!("No symbol has \u{201c}{}\u{201d}", q.trim())
                    })];
                }
                found.into_iter().map(|s| Line::Symbol(s.clone())).collect()
            }
            None => vec![],
        }
    }

    /// The symbol the reader is in.
    pub fn symbol_here(&self, cx: &App) -> Option<&store::symbols::Symbol> {
        let code = self.shown_code();
        let c = code.read(cx);
        // Where the caret is, or reading on past it, the line at the top.
        let line = if self.opened.as_ref().is_some_and(|o| o.editing) {
            c.cursor_line()
        } else {
            c.cursor_line().max(c.top_line())
        };
        symbols::at(&self.symbols, line)
    }

    /// Ctrl+F (Ctrl+H to replace too): the find inside the open file, from
    /// what is selected.
    pub fn find_in_file(&mut self, replace: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(o) = self.opened.as_mut() else {
            return;
        };
        if !matches!(o.show, Show::Text { .. }) {
            return;
        }
        o.diff_open = false;
        // Rendered or running, its text is found in its source.
        if (self.reading() || self.runs_page())
            && let Some(o) = self.opened.as_mut()
        {
            o.source = true;
        }
        if replace
            && self
                .opened
                .as_ref()
                .is_some_and(|o| !o.editing && o.editable())
        {
            self.edit(window, cx);
        }
        let selected = self.shown_code().read(cx).selected_text().to_owned();
        // From what is selected, or the last query, all of it selected so
        // typing replaces it.
        let query = if !selected.is_empty() && !selected.contains('\n') {
            selected
        } else {
            self.infile_query.read(cx).text.clone()
        };
        self.infile_query
            .update(cx, |f, cx| f.set(&query, true, cx));
        let was = self.infile.as_ref().is_some_and(|f| f.replace);
        self.infile = Some(InFile {
            replace: replace || was,
            ..Default::default()
        });
        ahjo_app_kit::widgets::field::focus(&self.infile_query, window, cx);
        self.refind(true, cx);
        cx.notify();
    }

    fn infile_focused(&self, window: &Window, cx: &App) -> bool {
        self.infile_query.read(cx).is_focused(window)
            || self.infile_with.read(cx).is_focused(window)
    }

    pub fn close_infile(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.infile = None;
        self.code.update(cx, |c, cx| c.set_found(vec![], cx));
        self.pretty_code.update(cx, |c, cx| c.set_found(vec![], cx));
        self.focus_file(window, cx);
        cx.notify();
    }

    /// Finds the query in the open file again; `go` moves to the first
    /// found at or after the caret.
    pub fn refind(&mut self, go: bool, cx: &mut Context<Self>) {
        if self.infile.is_none() {
            return;
        }
        let q = self.infile_query.read(cx).text.clone();
        let code = self.shown_code();
        let (found, caret) = {
            let c = code.read(cx);
            (store::find::in_text(c.text(), &q), c.selection().start)
        };
        let Some(find) = self.infile.as_mut() else {
            return;
        };
        find.at = if found.is_empty() {
            None
        } else if go {
            Some(found.iter().position(|r| r.start >= caret).unwrap_or(0))
        } else {
            find.at.map(|a| a.min(found.len() - 1))
        };
        find.found = found.clone();
        let at = find.at.map(|a| found[a].clone());
        code.update(cx, |c, cx| {
            c.set_found(found, cx);
            if go && let Some(r) = at {
                c.select(r, cx);
            }
        });
        cx.notify();
    }

    /// To the next found (or the one before), round the ends.
    pub fn infile_step(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(find) = self.infile.as_mut() else {
            return;
        };
        let n = find.found.len();
        if n == 0 {
            return;
        }
        let at = match find.at {
            None => 0,
            Some(a) if forward => (a + 1) % n,
            Some(a) => (a + n - 1) % n,
        };
        find.at = Some(at);
        let r = find.found[at].clone();
        self.shown_code().update(cx, |c, cx| c.select(r, cx));
        cx.notify();
    }

    /// Puts the replacement in place of the found shown, and goes on.
    pub fn replace_one(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(r) = self
            .infile
            .as_ref()
            .and_then(|f| f.at.and_then(|a| f.found.get(a).cloned()))
        else {
            return;
        };
        if !self.editing_now(window, cx) {
            return;
        }
        let with = self.infile_with.read(cx).text.clone();
        self.code.update(cx, |c, cx| {
            c.replace_all(std::slice::from_ref(&r), &with, cx);
        });
        let at = r.start + with.len();
        self.code.update(cx, |c, cx| c.select(at..at, cx));
        self.refind(true, cx);
    }

    /// Replaces every found, as one step to undo.
    pub fn replace_every(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let found = self
            .infile
            .as_ref()
            .map(|f| f.found.clone())
            .unwrap_or_default();
        if found.is_empty() || !self.editing_now(window, cx) {
            return;
        }
        let with = self.infile_with.read(cx).text.clone();
        let n = found.len();
        self.code
            .update(cx, |c, cx| c.replace_all(&found, &with, cx));
        self.host.toast(&format!("Replaced {n}"), cx);
        self.refind(false, cx);
    }

    /// Editing the open file, begun now when it can be.
    fn editing_now(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        match self.opened.as_ref() {
            Some(o) if o.editing => true,
            Some(o) if o.editable() => {
                let focus_back = self.infile_focused(window, cx);
                self.edit(window, cx);
                if focus_back {
                    ahjo_app_kit::widgets::field::focus(&self.infile_with, window, cx);
                }
                true
            }
            _ => false,
        }
    }

    /// Asks what git says changed in the repository `folder` is in, unless
    /// that is known.
    pub(crate) fn ask_status(&mut self, folder: &str, cx: &mut Context<Self>) {
        let known = self.repos.keys().any(|r| paths::inside(r, folder))
            || self.no_repo.contains(folder)
            || !self.status_asked.insert(folder.to_owned());
        if known || self.tree.node.is_empty() {
            return;
        }
        self.read_status(folder.to_owned(), cx);
    }

    fn read_status(&mut self, folder: String, cx: &mut Context<Self>) {
        let node = self.tree.node.clone();
        self.call(
            NodeId(node.clone()),
            FileListChanges {
                path: folder.clone(),
            },
            move |p, answer, cx| {
                p.status_asked.remove(&folder);
                if p.tree.node != node {
                    return;
                }
                if answer.as_ref().is_err_and(|error| {
                    matches!(
                        error.code.as_str(),
                        "forbidden"
                            | "unknown_command"
                            | ahjo_files_contract::OUTSIDE_ROOTS
                            | ahjo_files_contract::NOT_FOUND
                    )
                }) {
                    // Gone, or outside what the node shows: no repository
                    // to ask about again.
                    p.no_repo.insert(folder.clone());
                }
                let Ok(status) = answer else {
                    return;
                };
                if status.repository.is_empty() {
                    p.no_repo.insert(folder.clone());
                    return;
                }
                p.repos.insert(status.repository, status.files);
                p.mark_states();
                cx.notify();
            },
            cx,
        );
    }

    /// Asks about the repository of every folder a session works in:
    /// Changes lists what is not committed there.
    pub(crate) fn ask_session_status(&mut self, cx: &mut Context<Self>) {
        let folders: Vec<String> = self
            .live
            .iter()
            .filter(|s| s.node == self.tree.node)
            .filter_map(|s| s.cwd.clone())
            .collect();
        for folder in folders {
            self.ask_status(&folder, cx);
        }
    }

    /// Whether a live session works in `repo` or under it.
    pub(crate) fn worked_in(&self, repo: &str) -> bool {
        self.live.iter().any(|s| {
            s.node == self.tree.node
                && s.cwd
                    .as_deref()
                    .is_some_and(|cwd| cwd == repo || paths::inside(repo, cwd))
        })
    }

    /// Asks every repository known again: something changed.
    pub(crate) fn refresh_status(&mut self, cx: &mut Context<Self>) {
        let repos: Vec<String> = self.repos.keys().cloned().collect();
        for repo in repos {
            if self.status_asked.insert(repo.clone()) {
                self.read_status(repo, cx);
            }
        }
    }

    /// Each changed file's state, both ends of a rename, and the folders holding changes.
    fn mark_states(&mut self) {
        self.states.clear();
        self.renames.clear();
        self.holding.clear();
        for (repo, files) in &self.repos {
            for f in files {
                self.states.insert(f.path.clone(), f.state);
                if let Some(source) = &f.renamed_from {
                    self.states.entry(source.clone()).or_insert('R');
                    self.renames
                        .entry(source.clone())
                        .or_insert_with(|| Rename::To(f.path.clone()));
                    self.renames
                        .insert(f.path.clone(), Rename::From(source.clone()));
                }
                let mut folder = paths::parent(&f.path);
                while paths::inside(repo, &folder) && self.holding.insert(folder.clone()) {
                    folder = paths::parent(&folder);
                }
            }
        }
    }

    /// Ctrl+\: the file shown (or `key`'s) opened in a second group of tabs
    /// beside the first.
    pub fn split_open(&mut self, key: Option<Key>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(key) = key.or_else(|| self.opened.as_ref().map(|o| o.key())) else {
            return;
        };
        let view = match &self.split {
            Some(v) => v.clone(),
            None => {
                let host = self.host.clone();
                let me = self.me.clone();
                let tree = self.tree.clone();
                let wrap = self.code.read(cx).wraps();
                let view = cx.new(|cx| {
                    let mut p = FilesPage::new(host, window, cx);
                    p.split_of = Some(me);
                    p.side = Side::Hidden;
                    p.restored = true;
                    p.tree = tree;
                    p.code.update(cx, |c, cx| c.set_wrap(wrap, cx));
                    p
                });
                view.update(cx, |p, cx| {
                    p.start(cx);
                    p.watch(window, cx);
                });
                self.split = Some(view.clone());
                view
            }
        };
        view.update(cx, |p, cx| {
            p.on_node(&key.0, cx);
            p.open_as(key.1, true, window, cx);
            p.focus_file(window, cx);
        });
        cx.notify();
    }
}
