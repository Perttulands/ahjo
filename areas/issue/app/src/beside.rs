//! An item in the window's panel, beside the page that named it (a
//! terminal's Bead id): from the board the page holds when it holds the
//! item, at once; else read from its node with its family's titles.
use crate::Issue;
use crate::store::issue::Board;
use crate::view::Place;
use ahjo_app_kit::theme::{FAMILY, theme};
use ahjo_app_kit::widgets::pane;
use ahjo_core::NodeId;
use ahjo_issue_contract::{Item, ProjectView, Snapshot, StoreView, commands::ShowIssue};
use gpui::{AppContext, Context, Entity, IntoElement, Render, Window, div, prelude::*};
use std::rc::Rc;

/// The item's content in the window's panel.
pub(crate) const CARD: &str = "item";

struct Card {
    work: Entity<Issue>,
    error: Option<(String, bool)>,
    loading: bool,
}

impl Card {
    /// What the card says while it has no item to show.
    fn words(&self) -> Option<(&'static str, &'static str)> {
        match &self.error {
            Some((_, true)) => Some(("issue-beside-not-found", "Item not found")),
            Some((_, false)) => Some(("issue-beside-failed", "Could not read item")),
            None if self.loading => Some(("issue-beside-loading", "Reading item…")),
            None => None,
        }
    }
}

impl ahjo_app_kit::panel::PanelView for Card {
    fn head(&mut self, _: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let t = theme(cx).clone();
        let words = self.words();
        self.work.update(cx, |work, cx| match work.shown_row() {
            Some(row) => work
                .detail_head(&t, row, Place::Beside, cx)
                .into_any_element(),
            None => {
                let (selector, words) = words.unwrap_or(("issue-beside-none", ""));
                pane::title(&t, words)
                    .debug_selector(move || selector.into())
                    .into_any_element()
            }
        })
    }
    fn can_go_back(&self, cx: &gpui::App) -> bool {
        !self.work.read(cx).trail.is_empty()
    }
    fn go_back(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.work.update(cx, |work, cx| work.back(cx));
    }
}

impl Render for Card {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx).clone();
        let detail = self.work.update(cx, |work, cx| {
            work.shown_row().map(|row| {
                work.want_text(row, cx);
                work.detail(&t, row, Place::Beside, cx)
            })
        });
        let words = self.words();
        div()
            .id("issue-beside")
            .debug_selector(|| "issue-beside".into())
            // The page that shows it beside decides the width.
            .size_full()
            .overflow_hidden()
            .flex()
            .flex_col()
            .bg(t.surface)
            .font_family(FAMILY)
            .text_color(t.text)
            .when(words.is_some() && detail.is_none(), |d| {
                let error = self.error.clone().map(|(e, _)| e);
                let detail_error = error.clone();
                d.child(
                    pane::empty(&t, error.unwrap_or_default(), None)
                        .when_some(detail_error, |d, e| {
                            d.debug_selector(move || format!("issue-beside-error-{e}"))
                        }),
                )
            })
            .children(detail)
    }
}

impl Issue {
    /// Shows item `id` in the window's panel, beside the page that named
    /// it.
    pub(crate) fn open_beside(
        &mut self,
        id: String,
        node: Option<NodeId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let node = node
            .or_else(|| self.host.nodes(cx).first().map(|n| n.id.clone()))
            .unwrap_or_else(|| NodeId(String::new()));
        let host = self.host.clone();
        let card_work = cx.new(|cx| Issue::new(host.clone(), window, cx));
        // The page's board holds the item: it shows now, family and all.
        if let Some(row) = self.find_on(&id, Some(node.as_str())) {
            let (board, snapshots) = (self.board.clone(), self.snapshots.clone());
            card_work.update(cx, |w, cx| {
                w.board = board;
                w.snapshots = snapshots;
                w.select(row, cx);
            });
            let card = cx.new(|cx| {
                cx.observe(&card_work, |_, _, cx| cx.notify()).detach();
                Card {
                    work: card_work.clone(),
                    error: None,
                    loading: false,
                }
            });
            host.show_panel(ahjo_app_kit::panel::Panel::new(CARD, card), cx);
            return;
        }
        let card = cx.new(|cx| {
            cx.observe(&card_work, |_, _, cx| cx.notify()).detach();
            Card {
                work: card_work.clone(),
                error: None,
                loading: true,
            }
        });
        host.show_panel(ahjo_app_kit::panel::Panel::new(CARD, card.clone()), cx);
        let task = host.call(
            node.clone(),
            ShowIssue {
                reference: id.clone(),
            },
            cx,
        );
        // Whatever the page knows of the node's projects names a GitHub
        // item's checkout.
        let mut projects: Vec<ProjectView> = self
            .snapshots
            .get(node.as_str())
            .map(|s| s.projects.clone())
            .unwrap_or_default();
        cx.spawn(async move |_, cx| {
            let result = task
                .await
                .map(|answer| (answer.item, answer.line, answer.family))
                .map_err(|error| (error.message, error.code == "not_found"));
            card.update(cx, |card, cx| {
                card.loading = false;
                match result {
                    Err(error) => card.error = Some(error),
                    Ok((item, line, family)) => {
                        let unread: Vec<ProjectView> = family_stores(family)
                            .into_iter()
                            .filter(|f| {
                                !projects.iter().any(|p| match (p, f) {
                                    (ProjectView::Beads(p), ProjectView::Beads(f)) => {
                                        p.folder == f.folder && p.items.is_some()
                                    }
                                    _ => false,
                                })
                            })
                            .collect();
                        projects.extend(unread);
                        card.work.update(cx, |w, cx| {
                            w.board = Rc::new(Board::from_item(item, &projects));
                            // All rows in this board came from the answering node.
                            w.snapshots.insert(
                                node.to_string(),
                                std::sync::Arc::new(Snapshot {
                                    refreshed: 0,
                                    projects: vec![
                                        ProjectView::Beads(StoreView::default());
                                        w.board.projects.len()
                                    ],
                                }),
                            );
                            w.card_line = Some((w.board.rows[0].reference.clone(), line));
                            w.select(0, cx);
                        });
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}

/// The family's items as the stores that hold them, by the folder their
/// references name.
fn family_stores(family: Vec<Item>) -> Vec<ProjectView> {
    let mut stores: Vec<StoreView> = Vec::new();
    for item in family {
        let Some(folder) = item
            .reference
            .strip_prefix("bead:")
            .and_then(|r| r.split_once(':'))
            .and_then(|(_, r)| r.rsplit_once('#'))
            .map(|(f, _)| f.to_owned())
        else {
            continue;
        };
        match stores.iter_mut().find(|s| s.folder == folder) {
            Some(s) => s.items.get_or_insert_with(Vec::new).push(item),
            None => stores.push(StoreView {
                folder,
                items: Some(vec![item]),
                ..Default::default()
            }),
        }
    }
    stores.into_iter().map(ProjectView::Beads).collect()
}
