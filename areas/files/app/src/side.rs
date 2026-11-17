//! The Files section of the side panel, on every page: the folder of the
//! session in the focused terminal window as a tree, or the node's roots
//! while no session is focused. A folder opens in place; a file opens on
//! the Files page.

use crate::store::tree::{Row, Tree};
use gpui::{AnyElement, Context, Entity, Subscription, Window, div, prelude::*, px};
use parvi_app_kit::host::Request;
use parvi_app_kit::theme::{Theme, size, theme};
use parvi_app_kit::widgets::{kit, pane};
use parvi_app_kit::{Ask, Host};
use parvi_core::NodeId;
use parvi_files_contract::{Listing, NodeRoots, Root};
use parvi_terminals_contract::SessionList;
use std::collections::HashSet;

/// How far in a row moves for each folder it is inside.
const INDENT: f32 = 12.;
/// A folder's open or shut mark, as the sessions' group headers draw it.
const MARK: f32 = 8.;

/// `s` as a query value: unreserved bytes as they are, the rest escaped.
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

pub struct FilesSide {
    host: Host,
    tree: Tree,
    /// The focused session's node and folder the tree is rooted at; `None`
    /// while it shows the node's roots.
    at: Option<(String, String)>,
    /// Folders being read.
    reading: HashSet<String>,
    /// Why the tree could not be read, while it could not.
    trouble: Option<String>,
    sessions: Option<Entity<SessionList>>,
    _sessions_changed: Option<Subscription>,
}

impl FilesSide {
    pub fn new(host: Host, cx: &mut Context<Self>) -> Self {
        let sessions = host.model::<SessionList>(cx);
        let changed = sessions
            .as_ref()
            .map(|s| cx.observe(s, |me: &mut Self, _, cx| me.follow(cx)));
        let mut me = Self {
            host,
            tree: Tree::default(),
            at: None,
            reading: HashSet::new(),
            trouble: None,
            sessions,
            _sessions_changed: changed,
        };
        me.root(cx);
        me
    }

    /// The focused session's node and folder, when it has one.
    fn focused(&self, cx: &gpui::App) -> Option<(String, String)> {
        let list = self.sessions.as_ref()?.read(cx);
        let f = list.focused.as_ref()?;
        list.sessions
            .iter()
            .find(|s| {
                s.node == f.node && s.name == f.name && s.user.as_deref().unwrap_or("") == f.user
            })
            .and_then(|s| Some((s.node.clone(), s.cwd.clone()?)))
    }

    /// The sessions changed: the tree follows the focused session's folder
    /// when that is another folder.
    fn follow(&mut self, cx: &mut Context<Self>) {
        let at = self.focused(cx);
        if at != self.at || (at.is_none() && self.tree.roots.is_empty()) {
            self.root(cx);
        }
    }

    /// Roots the tree at the focused session's folder, else at the node's
    /// roots.
    fn root(&mut self, cx: &mut Context<Self>) {
        self.at = self.focused(cx);
        self.trouble = None;
        self.reading.clear();
        match self.at.clone() {
            Some((node, folder)) => {
                self.tree = Tree::default();
                self.tree.choose_node(&node);
                let name = folder
                    .trim_end_matches(['/', '\\'])
                    .rsplit(['/', '\\'])
                    .next()
                    .filter(|n| !n.is_empty())
                    .unwrap_or(&folder)
                    .to_string();
                self.tree.rooted(vec![Root { name, path: folder }]);
                self.read_unread(cx);
            }
            None => {
                let Some(node) = self.host.nodes(cx).first().map(|n| n.id.to_string()) else {
                    return;
                };
                self.tree = Tree::default();
                self.tree.choose_node(&node);
                let task = self
                    .host
                    .request(Request::get(NodeId(node.clone()), "/api/files/roots"), cx);
                cx.spawn(async move |me, cx| {
                    let answer = task.await;
                    let _ = me.update(cx, |me, cx| {
                        if me.at.is_some() || me.tree.node != node {
                            return;
                        }
                        match answer.map_err(|e| e.message).and_then(|a| {
                            if a.ok() {
                                serde_json::from_slice::<NodeRoots>(&a.body)
                                    .map_err(|e| e.to_string())
                            } else {
                                Err(a.error_text())
                            }
                        }) {
                            Ok(r) => {
                                me.tree.rooted(r.roots);
                                me.read_unread(cx);
                            }
                            Err(why) => me.trouble = Some(why),
                        }
                        cx.notify();
                    });
                })
                .detach();
            }
        }
        cx.notify();
    }

    fn read_unread(&mut self, cx: &mut Context<Self>) {
        for folder in self.tree.unread() {
            if self.reading.insert(folder.clone()) {
                self.list(folder, cx);
            }
        }
    }

    fn list(&mut self, folder: String, cx: &mut Context<Self>) {
        let node = self.tree.node.clone();
        let path = format!("/api/files/folder?path={}", enc(&folder));
        let task = self
            .host
            .request(Request::get(NodeId(node.clone()), path), cx);
        cx.spawn(async move |me, cx| {
            let answer = task.await;
            let _ = me.update(cx, |me, cx| {
                me.reading.remove(&folder);
                if me.tree.node != node {
                    return;
                }
                let read = answer.map_err(|e| e.message).and_then(|a| {
                    if a.ok() {
                        serde_json::from_slice::<Listing>(&a.body).map_err(|e| e.to_string())
                    } else {
                        Err(a.error_text())
                    }
                });
                match read {
                    Ok(l) => me.tree.listed(folder.clone(), l.entries),
                    Err(why) => me.tree.failed(folder.clone(), why),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// A folder opens or closes in place; a file opens on the Files page.
    fn pick(&mut self, row: &Row, cx: &mut Context<Self>) {
        self.tree.select(&row.path);
        if row.folder {
            if self.tree.toggle(&row.path) {
                self.read_unread(cx);
            }
            cx.notify();
            return;
        }
        let ask = Ask::OpenPath {
            node: NodeId(self.tree.node.clone()),
            path: row.path.clone(),
            beside: false,
        };
        if let Err(e) = self.host.ask(ask, cx) {
            self.trouble = Some(e.message);
        }
        cx.notify();
    }

    fn row(&self, t: &Theme, row: Row, cx: &mut Context<Self>) -> AnyElement {
        let chosen = self.tree.selected.as_deref() == Some(row.path.as_str());
        let selector = format!("side-file-{}", row.path);
        let mark = match (row.folder, row.open) {
            (true, true) => "▼",
            (true, false) => "▶",
            (false, _) => "",
        };
        let failed = self.tree.failed.get(&row.path).cloned();
        let r = row.clone();
        let line = pane::row(t, gpui::SharedString::from(selector.clone()), chosen)
            .debug_selector(move || selector)
            .pl(px(size::ROW_PAD + INDENT * row.depth as f32))
            .gap(px(6.))
            .child(
                div()
                    .flex_none()
                    .w(px(INDENT))
                    .text_size(px(MARK))
                    .text_color(t.dim)
                    .child(mark),
            )
            .child(pane::cell(row.name.clone()).text_color(if row.folder {
                t.text
            } else {
                t.text2
            }))
            .on_click(cx.listener(move |me, _, _, cx| me.pick(&r, cx)));
        // A folder that could not be read says why under its name.
        div()
            .flex()
            .flex_col()
            .child(line)
            .children(failed.map(|why| {
                div()
                    .pl(px(size::ROW_PAD + INDENT * (row.depth + 1) as f32 + INDENT))
                    .pr(px(size::ROW_PAD))
                    .pb(px(4.))
                    .text_size(px(size::SMALL))
                    .text_color(t.dim)
                    .child(why)
            }))
            .into_any_element()
    }
}

impl Render for FilesSide {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx).clone();
        let rows = self.tree.rows();
        let mut list = div()
            .id("side-files")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .pb(px(8.))
            .flex()
            .flex_col();
        for row in rows {
            list = list.child(self.row(&t, row, cx));
        }
        if let Some(why) = &self.trouble {
            list = list.child(
                div()
                    .px(px(size::ROW_PAD))
                    .text_size(px(size::SMALL))
                    .text_color(t.dim)
                    .child(why.clone()),
            );
        }
        div()
            .debug_selector(|| "side-files-section".into())
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .px(px(size::ROW_PAD))
                    .pt(px(size::LABEL_TOP))
                    .pb(px(size::LABEL_BOTTOM))
                    .child(kit::group_label(&t, "Files").text_color(t.text)),
            )
            .child(list)
    }
}
