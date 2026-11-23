//! Issues in the window's panel (Alt+B), beside any page: the focused
//! session's project's open work as the page's tree, or an item from it
//! read in place, Back going back to the tree.
use crate::Issue;
use crate::store::issue::outline::{self, Line};
use crate::tokens::*;
use crate::view::{Place, status_meta};
use ahjo_app_kit::theme::{FAMILY, Theme, size, theme};
use ahjo_app_kit::widgets::field::{self, FieldEvent};
use ahjo_app_kit::widgets::{kit, pane};
use gpui::{
    AnyElement, AppContext, Context, Div, Entity, KeyDownEvent, Render, Window, div, prelude::*, px,
};

/// Issues' work in the window's panel.
pub(crate) const PANEL: &str = "issues";

pub(crate) struct Panel {
    work: Entity<Issue>,
}
impl Panel {
    fn new(work: Entity<Issue>, cx: &mut Context<Self>) -> Self {
        cx.observe(&work, |_, _, cx| cx.notify()).detach();
        Self { work }
    }
}
impl Render for Panel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.work.update(cx, |work, cx| work.render_panel(cx))
    }
}
impl ahjo_app_kit::panel::PanelView for Panel {
    fn head(&mut self, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        self.work.update(cx, |work, cx| work.panel_head(cx))
    }
    fn can_go_back(&self, cx: &gpui::App) -> bool {
        let work = self.work.read(cx);
        work.panel_item.is_some() || work.panel_picking
    }
    fn go_back(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.work.update(cx, |work, cx| {
            if work.panel_picking {
                work.panel_picking = false;
            } else {
                work.panel_item = None;
            }
            cx.notify();
        });
    }
    fn focus(&self, cx: &gpui::App) -> Option<gpui::FocusHandle> {
        Some(self.work.read(cx).panel_focus.clone())
    }
}

impl Issue {
    /// Whether Issues' work shows in the window's panel.
    pub(crate) fn panel_open(&self, cx: &gpui::App) -> bool {
        self.host.panel_shows(PANEL, cx)
    }

    /// Shows the work in the window's panel, its find holding the
    /// keyboard, or puts the panel away.
    pub(crate) fn toggle_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.panel_open(cx) {
            self.close_panel(cx);
            return;
        }
        let Some(work) = self.me.upgrade() else {
            return;
        };
        let panel = self
            .panel
            .get_or_insert_with(|| cx.new(|cx| Panel::new(work, cx)))
            .clone();
        self.panel_item = None;
        self.panel_picking = false;
        self.panel_project = None;
        self.panel_search.update(cx, |f, cx| f.set("", false, cx));
        self.host
            .show_panel(ahjo_app_kit::panel::Panel::new(PANEL, panel), cx);
        field::focus(&self.panel_search, window, cx);
    }

    /// Typing in the panel's find narrows the tree to the matches; Enter
    /// reads the first.
    pub(crate) fn panel_search_event(
        &mut self,
        ev: FieldEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if ev == FieldEvent::Changed {
            let query = self.panel_search.read(cx).text.clone();
            self.search_text(&query, cx);
        }
        if ev == FieldEvent::Submit
            && let Some(&row) = self.panel_rows(cx).first()
        {
            self.panel_item = self
                .board
                .rows
                .get(row)
                .map(crate::store::issue::RowKey::of);
            window.focus(&self.panel_focus, cx);
        }
        cx.notify();
    }

    /// The rows the panel lists: what its find matches, here first, or
    /// none while it is empty.
    fn panel_rows(&self, cx: &gpui::App) -> Vec<usize> {
        let found = outline::search(
            &self.board,
            self.panel_project(cx),
            &self.panel_search.read(cx).text,
            self.hits_for(&self.panel_search.read(cx).text),
        );
        found.here.into_iter().chain(found.elsewhere).collect()
    }

    /// Esc in the panel's find clears it; with nothing to clear, Esc is
    /// the panel's and puts it away.
    fn panel_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if e.keystroke.key != "escape" || e.keystroke.modifiers.modified() {
            return;
        }
        let search = self.panel_search.read(cx);
        if search.is_focused(window) && !search.text.is_empty() {
            self.panel_search.update(cx, |f, cx| f.set("", false, cx));
            cx.stop_propagation();
            cx.notify();
        }
    }

    pub(crate) fn close_panel(&mut self, cx: &mut Context<Self>) {
        if self.panel_open(cx) {
            self.host.put_panel_away(cx);
        }
    }

    /// The panel's header: the item's head, else what the panel lists
    /// (the project, chosen there).
    fn panel_head(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let t = theme(cx).clone();
        if let Some(row) = self
            .panel_item
            .as_ref()
            .and_then(|key| self.board.position(key))
        {
            return self
                .detail_head(&t, row, Place::Panel, cx)
                .into_any_element();
        }
        if self.panel_picking {
            return pane::title(&t, "Project").into_any_element();
        }
        let label = self
            .panel_project(cx)
            .map(|p| self.board.projects[p].label.clone())
            .unwrap_or_else(|| "Project".into());
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .child(
                pane::act(&t, "issue-panel-project", label)
                    .debug_selector(|| "issue-panel-project".into())
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(t.text)
                    .tooltip(kit::tip("Choose the project"))
                    .on_click(cx.listener(|w, _, _, cx| {
                        w.panel_picking = true;
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    fn render_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let t = theme(cx).clone();
        let item = self
            .panel_item
            .as_ref()
            .and_then(|key| self.board.position(key));
        let inner = match item {
            Some(row) => {
                self.want_text(row, cx);
                self.detail(&t, row, Place::Panel, cx)
            }
            None if self.panel_picking => self.panel_projects(&t, cx),
            None => {
                let project = self.panel_project(cx);
                let finding = !self.panel_search.read(cx).text.trim().is_empty();
                let lines = if finding {
                    self.panel_rows(cx)
                        .into_iter()
                        .map(|row| Line { row, depth: 0 })
                        .collect()
                } else {
                    project
                        .map(|p| outline::tree(&self.board, p))
                        .unwrap_or_default()
                };
                let mut rows = div().flex().flex_col().py(px(size::ROW_INSET));
                if lines.is_empty() {
                    let why = if finding {
                        "Nothing matches.".into()
                    } else {
                        self.why_empty(cx)
                    };
                    rows = rows.child(pane::empty(&t, why, None));
                }
                for (at, line) in lines.into_iter().enumerate() {
                    let r = &self.board.rows[line.row];
                    let selector = format!("issue-panel-row-{}", r.id);
                    let key = crate::store::issue::RowKey::of(r);
                    rows = rows.child(
                        pane::row(&t, ("issue-panel-row", at), false)
                            .debug_selector(move || selector)
                            .pl(px(size::ROW_PAD - size::ROW_INSET
                                + line.depth.min(MAX_DEPTH) as f32 * INDENT))
                            .child(pane::cell(r.title.clone()))
                            .children(status_meta(&t, r).map(|(s, c)| pane::meta(&t, s, Some(c))))
                            .on_click(cx.listener(move |w, _, _, cx| {
                                w.panel_item = Some(key.clone());
                                cx.notify();
                            })),
                    );
                }
                let find = pane::list_head(
                    &t,
                    Some(
                        div()
                            .debug_selector(|| "issue-panel-search".into())
                            .child(self.panel_search.clone())
                            .into_any_element(),
                    ),
                );
                panel_frame(find, rows)
            }
        };
        // The terminals decide its width.
        div()
            .id("issue-panel")
            .debug_selector(|| "issue-panel".into())
            .track_focus(&self.panel_focus)
            .on_key_down(cx.listener(Self::panel_key))
            .size_full()
            .flex()
            .flex_col()
            .bg(t.surface)
            .font_family(FAMILY)
            .text_color(t.text)
            .child(inner)
            .into_any_element()
    }

    /// The projects to choose from in the panel, as the page lists them:
    /// most open work first, each with how much is open. The one chosen
    /// shows in the panel now and is the page's from then on.
    fn panel_projects(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let open = outline::open_counts(&self.board);
        let current = self.panel_project(cx);
        let mut rows = div().flex().flex_col().py(px(size::ROW_INSET));
        for (at, p) in self.projects_named("").into_iter().enumerate() {
            let info = &self.board.projects[p];
            let selector = format!("issue-panel-pick-{}", info.label);
            rows = rows.child(
                pane::row(t, ("issue-panel-pick", at), Some(p) == current)
                    .debug_selector(move || selector)
                    .child(pane::cell(info.label.clone()))
                    .child(pane::meta(t, open[p].to_string(), None))
                    .on_click(cx.listener(move |w, _, _, cx| {
                        w.panel_project = w.board.projects.get(p).map(|p| p.path.clone());
                        w.panel_picking = false;
                        w.keep_project(p, cx);
                    })),
            );
        }
        panel_frame(div(), rows)
    }
}

/// The panel's find (or nothing) over its rows, which scroll.
fn panel_frame(head: Div, rows: Div) -> Div {
    div().size_full().flex().flex_col().child(head).child(
        div()
            .id("issue-panel-rows")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(rows),
    )
}
