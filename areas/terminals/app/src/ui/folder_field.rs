//! The launcher's Folder field, as CHROTE's FolderField: one input that
//! takes a folder three ways. Untouched, the list under it offers the
//! launcher's folders; a fragment ranks them with every project on the
//! node; a path lists the folders
//! under what is typed, on the launcher's node. Tab completes the lit row
//! and, once the folder is written out, goes on; the arrows move, Enter takes the lit row or the typed path. The list
//! takes the room its rows need until typing starts, then keeps the room of
//! its most rows while the launcher is open, so nothing moves as it fills.
//! Its rules live in `store::launch`.

use crate::store::launch::{self, FolderList, Suggestion};
use crate::theme::theme;
use ahjo_app_kit::Host;
use ahjo_app_kit::widgets::field::{FieldEvent, TextField};
use ahjo_app_kit::widgets::finder::marked_letters;
use ahjo_app_kit::widgets::{kit, pane};
use ahjo_core::NodeId;
use ahjo_terminals_contract::commands::{ListProjects, SessionListFolders};
use gpui::{Context, Entity, EventEmitter, Window, div, prelude::*, px};

/// Rows the list shows at once, at most.
const ROWS: usize = 6;
const ROW_H: f32 = 20.;

pub enum FolderEvent {
    /// The folder changed, typed or picked.
    Changed,
    /// Enter: launch with the folder as it now stands.
    Submit,
    /// Tab with nothing left to complete: on to the next field.
    Next,
}

/// One folder listing, or the node's reason it could not be read.
struct Listing {
    node: String,
    dir: String,
    folders: Result<Vec<String>, String>,
}

pub struct FolderField {
    id: usize,
    pub field: Entity<TextField>,
    /// The node paths are listed on.
    node: String,
    host: Host,
    /// What the list offers before anything is typed.
    offered: Vec<String>,
    /// Typing has given the list its full room, which it keeps until the
    /// launcher starts afresh.
    grown: bool,
    offered_user: String,
    /// The node's home folder for that user, when the node said.
    home: Option<String>,
    /// Typing turns the list into suggestions; a folder picked, launched
    /// or left alone is not being typed.
    editing: bool,
    /// The lit row, for the value it was moved on.
    lit: Option<(String, Option<usize>)>,
    /// The folders read since the field took the keyboard, so a folder
    /// typed again, or one read ahead, lists at once.
    listings: Vec<Listing>,
    /// The (node, folder)s being read now.
    reading: Vec<(String, String)>,
    /// Every project on the node, which a fragment searches with what is
    /// offered: the node's last walk, for the node it came from.
    projects: Option<(String, Vec<String>)>,
    /// The node whose projects are being asked for.
    asking: Option<String>,
}

/// Keep existing rows in place while a launcher is open; new choices join
/// the end rather than moving underneath the pointer.
fn keep_recent_order(current: &mut Vec<String>, offered: Vec<String>) {
    for folder in offered {
        if !current.contains(&folder) {
            current.push(folder);
        }
    }
}

impl EventEmitter<FolderEvent> for FolderField {}

impl FolderField {
    pub fn new(id: usize, host: Host, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let field = cx.new(|cx| TextField::new(cx, "~"));
        cx.subscribe_in(&field, window, |f: &mut Self, _, ev, window, cx| {
            f.on_field(ev, window, cx)
        })
        .detach();
        Self {
            id,
            field,
            node: String::new(),
            host,
            offered: vec![],
            grown: false,
            offered_user: String::new(),
            home: None,
            editing: false,
            lit: None,
            listings: Vec::new(),
            reading: Vec::new(),
            projects: None,
            asking: None,
        }
    }

    pub fn text(&self, cx: &gpui::App) -> String {
        self.field.read(cx).text.clone()
    }

    /// Writes a folder without it counting as typed.
    pub fn set(&mut self, value: &str, cx: &mut Context<Self>) {
        self.field.update(cx, |f, cx| f.set(value, false, cx));
        self.editing = false;
        cx.notify();
    }

    /// Where paths are listed and what is offered: the launcher's node now.
    pub fn serve(
        &mut self,
        node: &str,
        user: &str,
        offered: Vec<String>,
        home: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if self.node != node || self.offered_user != user {
            self.listings.clear();
            self.offered.clear();
        }
        let asked = self.projects.as_ref().map(|p| p.0.as_str()) == Some(node)
            || self.asking.as_deref() == Some(node);
        if !asked {
            self.ask_projects(node, cx);
        }
        self.node = node.to_string();
        self.offered_user = user.to_string();
        self.home = home;
        keep_recent_order(&mut self.offered, offered);
    }

    pub fn reset_recent(&mut self) {
        self.offered.clear();
        self.grown = false;
    }

    fn value(&self, cx: &gpui::App) -> String {
        self.field.read(cx).text.clone()
    }

    fn list(&self, cx: &gpui::App) -> FolderList {
        launch::folder_list(&self.value(cx), self.editing)
    }

    /// The rows for what is typed now.
    fn suggestions(&self, list: &FolderList, cx: &gpui::App) -> Vec<Suggestion> {
        match list {
            FolderList::Offered => self
                .offered
                .iter()
                .map(|p| Suggestion {
                    path: p.clone(),
                    complete: p.clone(),
                    at: vec![],
                })
                .collect(),
            FolderList::Matching => {
                let projects = match &self.projects {
                    Some((node, projects)) if *node == self.node => &projects[..],
                    _ => &[],
                };
                launch::matching(
                    &self.value(cx),
                    &launch::searched(&self.offered, projects, self.home.as_deref()),
                )
            }
            FolderList::Inside { dir, segment } => match self.listed(dir) {
                Some(l) => launch::inside(dir, segment, l.folders.as_deref().unwrap_or_default()),
                None => vec![],
            },
        }
    }

    fn lit(&self, list: &FolderList, rows: usize, cx: &gpui::App) -> Option<usize> {
        match &self.lit {
            Some((v, i)) if *v == self.value(cx) => i.map(|i| i.min(rows.saturating_sub(1))),
            _ => launch::first_lit(list, rows),
        }
    }

    /// The folder a launch takes now: what Enter would take, so the lit
    /// match of a fragment rather than the fragment.
    pub fn chosen(&self, cx: &gpui::App) -> String {
        self.chosen_for_enter(cx)
    }

    fn on_field(&mut self, ev: &FieldEvent, _window: &mut Window, cx: &mut Context<Self>) {
        match ev {
            FieldEvent::Changed => {
                self.editing = true;
                // The list keeps its height while you type, so rows do not
                // jump under the pointer; emptied, it is Recent again and
                // only as tall as what it holds.
                self.grown = !self.field.read(cx).text.is_empty();
                self.read(cx);
                cx.emit(FolderEvent::Changed);
            }
            // Folders and projects made since the last look show on the next.
            FieldEvent::Focused => {
                // The folder standing there was not typed: the first key
                // replaces it, as in an address bar.
                if !self.editing {
                    self.field.update(cx, |f, cx| f.select_everything(cx));
                }
                self.listings.clear();
                let node = self.node.clone();
                self.ask_projects(&node, cx);
            }
            FieldEvent::Up | FieldEvent::Down => {
                let list = self.list(cx);
                let rows = self.suggestions(&list, cx).len();
                let lit = self.lit(&list, rows, cx);
                let next = launch::step(lit, rows, matches!(ev, FieldEvent::Down), &list);
                self.lit = Some((self.value(cx), next));
            }
            FieldEvent::Tab => {
                let list = self.list(cx);
                let rows = self.suggestions(&list, cx);
                let Some(row) = self
                    .lit(&list, rows.len(), cx)
                    .or(Some(0))
                    .and_then(|i| rows.get(i))
                else {
                    cx.emit(FolderEvent::Next);
                    return;
                };
                let complete = row.complete.clone();
                // The folder is written out already: Tab is the way on, as
                // anywhere. A separator typed after it lists inside.
                let value = self.value(cx);
                if complete == value || row.path == value {
                    cx.emit(FolderEvent::Next);
                    return;
                }
                self.field.update(cx, |f, cx| f.set(&complete, false, cx));
                self.editing = true;
                self.read(cx);
                cx.emit(FolderEvent::Changed);
            }
            FieldEvent::Submit => {
                // An empty field launches where its placeholder and the
                // launch button say, as the button does.
                let chosen = self.chosen(cx);
                if !chosen.is_empty() {
                    self.set(&chosen, cx);
                }
                cx.emit(FolderEvent::Submit);
            }
            _ => {}
        }
        cx.notify();
    }

    /// Enter takes the lit row, or the typed path when none is lit.
    fn chosen_for_enter(&self, cx: &gpui::App) -> String {
        let list = self.list(cx);
        let rows = self.suggestions(&list, cx);
        match self.lit(&list, rows.len(), cx) {
            Some(i) => rows[i].path.clone(),
            None => self.value(cx).trim().to_string(),
        }
    }

    fn pick(&mut self, path: &str, cx: &mut Context<Self>) {
        self.set(path, cx);
        cx.emit(FolderEvent::Changed);
    }

    /// What the node said is in `dir`, when it has.
    fn listed(&self, dir: &str) -> Option<&Listing> {
        self.listings
            .iter()
            .find(|l| l.node == self.node && l.dir == dir)
    }

    /// Reads the folder typed so far on the node, once per folder, and
    /// the one folder in it the typing leads to, so the separator after
    /// its name lists it at once.
    fn read(&mut self, cx: &mut Context<Self>) {
        let FolderList::Inside { dir, segment } = self.list(cx) else {
            return;
        };
        let ahead = self
            .listed(&dir)
            .and_then(|l| launch::leads_to(&dir, &segment, l.folders.as_deref().ok()?));
        for dir in std::iter::once(dir).chain(ahead) {
            self.read_dir(dir, cx);
        }
    }

    fn read_dir(&mut self, dir: String, cx: &mut Context<Self>) {
        let at = (self.node.clone(), dir);
        if self.listed(&at.1).is_some() || self.reading.contains(&at) || self.node.is_empty() {
            return;
        }
        self.reading.push(at.clone());
        let task = self.host.call(
            NodeId(at.0.clone()),
            SessionListFolders { path: at.1.clone() },
            cx,
        );
        cx.spawn(async move |this, cx| {
            let answer = task.await.map(|a| a.folders).map_err(|e| e.message);
            let _ = this.update(cx, |f, cx| {
                f.reading.retain(|r| *r != at);
                f.listings.push(Listing {
                    node: at.0,
                    dir: at.1,
                    folders: answer,
                });
                f.read(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Asks the node for its projects. Its answer is its last walk, so it
    /// comes at once; one made since shows on the next ask.
    fn ask_projects(&mut self, node: &str, cx: &mut Context<Self>) {
        if node.is_empty() || self.asking.as_deref() == Some(node) {
            return;
        }
        self.asking = Some(node.to_string());
        let task = self
            .host
            .call(NodeId(node.to_string()), ListProjects {}, cx);
        let node = node.to_string();
        cx.spawn(async move |this, cx| {
            let answer = task.await;
            let _ = this.update(cx, |f, cx| {
                if f.asking.as_deref() == Some(node.as_str()) {
                    f.asking = None;
                }
                // A node that could not say keeps what it said last.
                if let Ok(answer) = answer {
                    let projects = answer.projects;
                    f.projects = Some((node, projects));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// What the list says when it holds nothing.
    fn note(&self, list: &FolderList) -> String {
        match list {
            FolderList::Offered => "Nothing recent".into(),
            FolderList::Matching => "No folder matches".into(),
            FolderList::Inside { dir, .. } => match self.listed(dir) {
                Some(l) => match l.folders {
                    Err(ref why) => why.clone(),
                    Ok(_) => "No folders here".into(),
                },
                _ => "Reading…".into(),
            },
        }
    }
}

impl Render for FolderField {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx).clone();
        // Leaving the field ends the typing, as CHROTE's blur does.
        if self.editing && !self.field.read(cx).is_focused(window) {
            self.editing = false;
        }
        let list = self.list(cx);
        let rows = self.suggestions(&list, cx);
        let lit = self.lit(&list, rows.len(), cx);
        let value = self.value(cx);
        let label = match list {
            FolderList::Offered => "Recent",
            FolderList::Matching => "Matching",
            FolderList::Inside { .. } => "Folders",
        };
        let shown_rows = if self.grown {
            ROWS
        } else {
            rows.len().clamp(1, ROWS)
        };
        let mut body = pane::scroll(("folder-rows", self.id)).h(px(ROW_H * shown_rows as f32));
        if rows.is_empty() {
            body = body.child(kit::note(&t, self.note(&list), None));
        }
        for (n, row) in rows.iter().enumerate() {
            let path = row.path.clone();
            let shown = row.path == value;
            body = body.child(
                div()
                    .id(("folder-row", self.id * 1000 + n))
                    .debug_selector({
                        let id = self.id;
                        let path = path.clone();
                        move || format!("folder-row-{id}-{path}")
                    })
                    .flex_none()
                    .h(px(ROW_H))
                    .px(px(4.))
                    .flex()
                    .items_center()
                    .rounded(px(3.))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_size(px(12.))
                    .text_color(if shown || lit == Some(n) {
                        t.text
                    } else {
                        t.text2
                    })
                    .when(lit == Some(n), |d| d.bg(t.accent_soft()))
                    .cursor_pointer()
                    .hover(|s| s.text_color(t.text))
                    // Its middle gives way: the end of a path says which
                    // folder it is.
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis_middle()
                            .child(marked_letters(&row.path, &row.at, &t)),
                    )
                    .on_click(cx.listener(move |f, _, _, cx| f.pick(&path, cx))),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(self.field.clone())
            .child(kit::label(&t, label))
            .child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::keep_recent_order;

    #[test]
    fn recent_folders_do_not_move_when_session_activity_changes() {
        let mut rows = vec!["/first".into(), "/second".into()];
        keep_recent_order(
            &mut rows,
            vec!["/second".into(), "/first".into(), "/third".into()],
        );
        assert_eq!(rows, ["/first", "/second", "/third"]);
    }
}
