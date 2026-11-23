//! Drawing the Issue page on the kit's skeleton (DESIGN.md, Issue): the
//! project's open work as a tree in the list, or a search's matches, or the
//! projects to choose from; the item beside it, read as a page reads. The
//! same item view stands beside a terminal and in the panel.

use crate::store::issue::timeline::{Range, range_label};
use crate::store::issue::{Row, Status, Tracker, prose, when};
use crate::text::Text;
use crate::tokens::*;
use crate::words;
use crate::{Entry, Issue, Layout, Moved};
use ahjo_app_kit::hub::now_ms;
use ahjo_app_kit::theme::{FAMILY, Theme, size, theme};
use ahjo_app_kit::widgets::kit;
use ahjo_app_kit::widgets::kit::{GripEdge, Phase};
use ahjo_app_kit::widgets::markdown;
use ahjo_app_kit::widgets::pane;
use ahjo_app_kit::widgets::select;
use gpui::{
    AnyElement, ClickEvent, Context, Div, FontWeight, Hsla, MouseButton, MouseDownEvent,
    SharedString, Stateful, Window, div, prelude::*, px,
};
use std::rc::Rc;

/// Where an item view stands: on the page, beside the page that named it,
/// or in the Issue panel over the terminals.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Place {
    Page,
    Beside,
    Panel,
}

/// A status as the operator needs to see it: under way, blocked, on hold
/// and done are said, to do is not.
pub(crate) fn status_meta(t: &Theme, r: &Row) -> Option<(String, Hsla)> {
    let word = words::row_state(r).to_lowercase();
    match r.status {
        Status::InProgress => Some((word, t.running)),
        Status::Blocked => Some((word, t.text2)),
        Status::Deferred | Status::Closed => Some((word, t.dim)),
        Status::Open => None,
    }
}

impl Render for Issue {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx).clone();
        let viewport = f32::from(window.viewport_size().width);
        let list = self.list_column(&t, viewport, window, cx);
        let detail = match self.shown_row() {
            Some(row) => {
                self.want_text(row, cx);
                self.detail(&t, row, Place::Page, cx)
            }
            // The list says why it holds nothing; the detail stays clear.
            None => pane::detail(&t, "issue-detail", pane::head(&t), div()),
        };
        // A node with no projects has nothing to chart, facet or list:
        // the page says how one is added, across its whole width.
        let unconfigured = !self.snapshots.is_empty() && self.board.projects.is_empty();
        let page = match self.layout {
            _ if self.snapshots.is_empty() && !self.frame_errors.is_empty() => {
                div().size_full().bg(t.bg)
            }
            _ if unconfigured => {
                div()
                    .size_full()
                    .bg(t.bg)
                    .child(pane::vacant(&t, self.why_empty(cx), None))
            }
            Layout::Tree => pane::page(&t, list, detail),
            Layout::Strip => self.strip_page(&t, list, detail, cx),
            Layout::Time => self.time_page(&t, list, detail, cx),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .children(self.frame_errors.values().map(|error| {
                div()
                    .debug_selector(|| "issue-frame-error".into())
                    .flex_none()
                    .px(px(size::ROW_PAD))
                    .py(px(size::HEAD_GAP))
                    .text_color(t.error)
                    .child(error.clone())
            }))
            .child(div().flex_1().min_h_0().child(page))
            .id("issue")
            .key_context("Issue")
            .track_focus(&self.focus)
            .font_family(FAMILY)
            .text_color(t.text)
            .on_key_down(cx.listener(|w, e: &gpui::KeyDownEvent, window, cx| {
                if w.focus.is_focused(window) {
                    w.key(e, window, cx);
                } else if !w.alt_key(e, window, cx)
                    && e.keystroke.key == "escape"
                    && w.escape_search(window, cx)
                {
                    cx.stop_propagation();
                }
            }))
    }
}

impl Issue {
    /// Why the list holds nothing, in one line.
    pub(crate) fn why_empty(&self, cx: &gpui::App) -> String {
        if self.snapshots.is_empty() {
            return "Reading issues…".into();
        }
        if self.board.projects.is_empty() {
            return "No projects are configured on this node: add a `beads` or `github` line to its issue settings.".into();
        }
        if !self.query.trim().is_empty() {
            return format!("Nothing matches “{}”.", self.query.trim());
        }
        if self.layout != Layout::Tree && (self.range.is_some() || self.state.is_some()) {
            let what = match (self.layout, self.moved) {
                (Layout::Time, Moved::Opened) => "Nothing was opened",
                (Layout::Time, Moved::Closed) => "Nothing was finished",
                (Layout::Time, Moved::Idle) => "Nothing sat idle",
                _ => "Nothing moved",
            };
            return match self.range {
                Some(r) => format!("{what} {}.", on_days(r)),
                None => "Nothing here is in that state.".into(),
            };
        }
        if self.every && self.layout != Layout::Tree {
            return "Nothing is open.".into();
        }
        let Some(p) = self.project(cx) else {
            return "Reading issues…".into();
        };
        let info = &self.board.projects[p];
        if let Some(e) = &info.error {
            // Why, in the tracker's first line, and what to do.
            let why = first_line(e);
            match info.tracker {
                Tracker::Beads => format!(
                    "{} could not be read: {why}. Run `bd list` in {} to see what Beads says, or take the line out of the node's issue settings.",
                    info.label, info.path
                ),
                Tracker::Github => format!(
                    "{} could not be read: {why}. Check `gh auth status` where the node runs.",
                    info.label
                ),
            }
        } else if !info.read {
            format!("Reading {}…", info.label)
        } else {
            format!("Nothing open in {}.", info.label)
        }
    }

    /// The list column: its head, a line saying the project could not be
    /// read when so, and the rows.
    fn list_column(
        &mut self,
        t: &Theme,
        viewport: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let head = self.list_head(t, cx);
        let mut body = div().flex_1().min_h_0().flex().flex_col();
        if !self.picking
            && let Some(p) = self
                .project(cx)
                .filter(|_| !self.every || self.layout == Layout::Tree)
        {
            let info = &self.board.projects[p];
            // One line in the column's own width that reads the store
            // again when clicked; the full cause, which names a whole
            // path, is in its tooltip.
            let trouble = info
                .error
                .as_ref()
                .map(|e| (format!("Not read: {}", first_line(e)), e.trim(), t.error))
                // Not watched is a warning: what is shown was read.
                .or_else(|| {
                    info.unwatched.as_ref().map(|w| {
                        (
                            "Not watched for changes: click to read again".into(),
                            w.trim(),
                            t.needs,
                        )
                    })
                });
            if let Some((short, full, colour)) = trouble.filter(|_| info.read) {
                body = body.child(
                    div()
                        .id("issue-trouble")
                        .debug_selector(|| "issue-trouble".into())
                        .flex_none()
                        .px(px(size::ROW_PAD))
                        .py(px(SPACE_XS))
                        .text_size(px(size::SMALL))
                        .text_color(colour)
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .cursor_pointer()
                        .child(short)
                        .tooltip(kit::tip(full.to_owned()))
                        .on_click(cx.listener(|w, _, _, cx| w.refresh(cx))),
                );
            }
        }
        if self.picking {
            let (rows, chosen) = self.project_rows(t, cx);
            if std::mem::take(&mut self.reveal)
                && let Some(at) = chosen
            {
                self.list_scroll.scroll_to_item(at);
            }
            body = body.child(
                pane::scroll("issue-list")
                    .flex_1()
                    .track_scroll(&self.list_scroll)
                    .children(rows),
            );
        } else if self.entries.is_empty() {
            body = body.child(pane::empty(t, self.why_empty(cx), None));
        } else {
            let shown = self.shown_row();
            if std::mem::take(&mut self.reveal)
                && let Some(at) = self
                    .entries
                    .iter()
                    .position(|e| matches!(e, Entry::Item { row, .. } if Some(*row) == shown))
            {
                self.rows_scroll
                    .scroll_to_item(at, gpui::ScrollStrategy::Nearest);
            }
            let n = self.entries.len();
            body = body.child(
                pane::long_list(
                    "issue-list",
                    n,
                    &self.rows_scroll,
                    cx.processor(move |w, range: std::ops::Range<usize>, _, cx| {
                        let t = theme(cx).clone();
                        let shown = w.shown_row();
                        range
                            .filter_map(|at| Some((at, w.entries.get(at)?.clone())))
                            .map(|(at, entry)| match entry {
                                Entry::Item { row, .. } => {
                                    w.item_row(&t, at, &entry, shown == Some(row), cx)
                                }
                                Entry::Label(text) => list_label(&t, text).into_any_element(),
                                Entry::More(n) => {
                                    list_label(&t, format!("{n} more: add a word to narrow it"))
                                        .into_any_element()
                                }
                            })
                            // Each row takes the list's whole width.
                            .map(|row| {
                                div()
                                    .w_full()
                                    .flex()
                                    .flex_col()
                                    .child(row)
                                    .into_any_element()
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .flex_1(),
            );
        }
        // As wide as the operator dragged it, never more than most of
        // the room beside the facets; until dragged, a share of that room
        // (measured where the list starts, as last drawn), so a
        // full-screen window shows whole titles and leaves the item room
        // for two columns.
        let drawn = self.list_drawn.clone();
        let me = cx.entity().downgrade();
        let width = self
            .list_w
            .unwrap_or_else(|| open_width(viewport - drawn.get().0));
        pane::list_wide_frame(t, head, body)
            .w(px(width))
            .max_w(gpui::relative(LIST_SHARE))
            .relative()
            .child(
                gpui::canvas(
                    move |b, _, cx| {
                        let was = drawn.replace((f32::from(b.origin.x), f32::from(b.size.width)));
                        // A new start arranges the next frame: drawn now,
                        // since a refresh asked while drawing is not one.
                        if (was.0 - f32::from(b.origin.x)).abs() > 0.5 {
                            cx.defer(move |cx| {
                                let _ = me.update(cx, |_, cx| cx.notify());
                            });
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .child(self.grip(t, width, window, cx))
    }

    /// The list's edge, dragged to widen or narrow it and kept; a double
    /// click gives it back the width it has until dragged.
    fn grip(
        &self,
        t: &Theme,
        width: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let page = cx.weak_entity();
        kit::grip(
            t,
            "issue-grip",
            GripEdge::Right,
            width,
            (LIST_MIN, LIST_MAX),
            window,
            cx,
            move |width, phase, _, cx| {
                let _ = page.update(cx, |w, cx| match phase {
                    Phase::Moving => w.drag_list(width, cx),
                    Phase::Kept => {
                        w.drag_list(width, cx);
                        w.keep_list(cx);
                    }
                    Phase::Reset => w.reset_list(cx),
                });
            },
        )
    }

    fn list_head(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let field = div().flex_1().min_w_0().child(self.search.clone());
        if self.picking {
            let act: AnyElement = if self.refreshing.is_empty() {
                pane::act(t, "issue-read-again", "Read again")
                    .tooltip(kit::tip("Read every project again now"))
                    .on_click(cx.listener(|w, _, _, cx| w.refresh(cx)))
                    .into_any_element()
            } else {
                pane::meta(t, "Reading…", None)
                    .debug_selector(|| "w-refreshing".into())
                    .into_any_element()
            };
            return pane::head(t).child(field).child(act);
        }
        let mut head = pane::head(t).child(field);
        if self.layout == Layout::Tree {
            let label = self
                .project(cx)
                .map(|p| self.board.projects[p].label.clone())
                .unwrap_or_else(|| "Project".into());
            head = head.child(
                pane::act(t, "issue-project", label)
                    .debug_selector(|| "issue-project".into())
                    .max_w(px(PROJECT_ACT_W))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .tooltip(kit::tip("Choose the project (P)"))
                    .on_click(cx.listener(|w, _, window, cx| w.start_picking(window, cx))),
            );
        }
        head.child(self.layout_act(t, cx))
    }

    /// The three layouts as a choice, the one in use filled; V moves to
    /// the next.
    fn layout_act(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let mut row = div()
            .debug_selector(|| "issue-layout".into())
            .flex_none()
            .flex()
            .items_center()
            .gap(px(size::LABEL_BOTTOM));
        for layout in Layout::ALL {
            let selector = format!("issue-layout-{}", layout.key());
            let chosen = layout == self.layout;
            row = row.child(
                kit::option(
                    t,
                    SharedString::from(selector.clone()),
                    layout.name(),
                    chosen,
                )
                .debug_selector(move || selector)
                .tooltip(kit::tip(layout.what()))
                .on_click(cx.listener(move |w, _, _, cx| w.set_layout(layout, true, cx))),
            );
        }
        row
    }

    /// A row of the list: its state, its title (a group's in bold, with
    /// how far it has come), its project when another's, and in columns
    /// of their own what kind of work it is, a priority that puts it
    /// ahead, and how long since it last moved. Under a group, a line
    /// down from its mark ties its items to it.
    pub(crate) fn item_row(
        &self,
        t: &Theme,
        at: usize,
        entry: &Entry,
        chosen: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Entry::Item { row, depth, away } = *entry else {
            return div().into_any_element();
        };
        let r = &self.board.rows[row];
        let selector = format!("issue-row-{}", r.id);
        let project = away.then(|| self.board.projects[r.project].label.clone());
        let quiet = matches!(
            r.status,
            Status::Closed | Status::Deferred | Status::Blocked
        );
        let progress = self.progress(row);
        let group = r.kind == "epic" || progress.is_some();
        let kind = words::kind_in_list(&r.kind).then(|| words::kind(&r.kind));
        let priority = r
            .priority
            .filter(|p| words::priority_in_list(*p))
            .map(|p| (p, words::priority(p)));
        let age = r
            .closed
            .filter(|_| r.status == Status::Closed)
            .or(r.updated)
            .map(|u| when::short_age(u, now_ms()));
        let depth = depth.min(MAX_DEPTH);
        pane::row(t, ("issue-row", at), chosen)
            .debug_selector(move || selector)
            .h(px(size::ROW))
            .children((depth > 0).then(|| guides(t, depth)))
            .child(glyph(t, r))
            .child(
                pane::cell(r.title.clone())
                    .when(group, |d| d.font_weight(FontWeight::SEMIBOLD))
                    .when(quiet && !chosen, |d| d.text_color(t.dim))
                    .when(!quiet && !chosen, |d| d.text_color(t.text)),
            )
            .children(progress.map(|(done, all)| meter(t, done, all, true)))
            .children(project.map(|p| {
                pane::meta(t, p, None)
                    .max_w(px(AWAY_W))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
            }))
            .child(
                div()
                    .id(("issue-kind", at))
                    .flex_none()
                    .w(px(KIND_W))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_size(px(size::SMALL))
                    .text_color(if group || r.kind == "bug" {
                        t.text2
                    } else {
                        t.dim
                    })
                    .when_some(kind, |d, (word, means)| {
                        d.child(word)
                            .when_some(means, |d, means| d.tooltip(kit::tip(means)))
                    }),
            )
            .child(
                div()
                    .id(("issue-priority", at))
                    .flex_none()
                    .w(px(PRIORITY_W))
                    .text_size(px(size::SMALL))
                    .when_some(priority, |d, (p, (word, means))| {
                        d.child(word)
                            .tooltip(kit::tip(means))
                            .when(p == 0, |d| {
                                d.text_color(t.text).font_weight(FontWeight::SEMIBOLD)
                            })
                            .when(p > 0, |d| d.text_color(t.text2))
                    }),
            )
            .child(
                pane::meta(t, age.unwrap_or_default(), None)
                    .w(px(AGE_W))
                    .flex()
                    .justify_end(),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |w, _, window, cx| {
                    window.focus(&w.focus, cx);
                    w.select(row, cx);
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |w, e: &MouseDownEvent, window, cx| {
                    window.focus(&w.focus, cx);
                    w.select(row, cx);
                    w.host.menu(w.item_menu(row), e.position, cx);
                }),
            )
            .into_any_element()
    }

    /// What the item is, in words anyone reads: its state, its kind and
    /// its priority, each saying what it means when hovered, and for a
    /// group how far its items have come.
    fn what_it_is(&self, t: &Theme, row: usize) -> Div {
        let r = &self.board.rows[row];
        let ink = match r.status {
            Status::InProgress => t.running,
            Status::Closed => t.done,
            Status::Deferred => t.dim,
            Status::Open | Status::Blocked => t.text2,
        };
        let (kind, kind_means) = words::kind(&r.kind);
        let dot = || div().text_color(t.dim).child("·");
        let mut line = div()
            .debug_selector(|| "issue-what".into())
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(size::HEAD_GAP))
            .child(glyph(t, r))
            .child(
                div()
                    .id("issue-what-state")
                    .text_color(ink)
                    .child(words::row_state(r))
                    .tooltip(kit::tip(words::state_means(r.status))),
            )
            .child(dot())
            .child(
                div()
                    .id("issue-what-kind")
                    .text_color(t.text2)
                    .child(words::capital(&kind))
                    .when_some(kind_means, |d, m| d.tooltip(kit::tip(m))),
            );
        if let Some(p) = r.priority {
            let (word, means) = words::priority(p);
            line = line.child(dot()).child(
                div()
                    .id("issue-what-priority")
                    .text_color(if p <= 1 { t.text } else { t.text2 })
                    .when(p == 0, |d| d.font_weight(FontWeight::SEMIBOLD))
                    .child(format!("{} priority", words::capital(word)))
                    .tooltip(kit::tip(means)),
            );
        }
        if let Some((done, all)) = self.progress(row) {
            line = line.child(dot()).child(meter(t, done, all, false));
        }
        line
    }

    /// How far a group of work has come: of the items under it, how many
    /// are done. None for an item with nothing under it.
    pub(crate) fn progress(&self, row: usize) -> Option<(usize, usize)> {
        let children = &self.board.links.children[row];
        (!children.is_empty()).then(|| {
            let done = children
                .iter()
                .filter(|&&c| self.board.rows[c].status == Status::Closed)
                .count();
            (done, children.len())
        })
    }

    /// The projects to choose from: most open work first, each with how
    /// much is open, or why it was not read.
    fn project_rows(&self, t: &Theme, cx: &mut Context<Self>) -> (Vec<AnyElement>, Option<usize>) {
        let open = crate::store::issue::outline::open_counts(&self.board);
        let current = self.project(cx);
        let mut out: Vec<AnyElement> = Vec::new();
        for (at, p) in self.projects_listed(cx).into_iter().enumerate() {
            let info = &self.board.projects[p];
            let (meta, colour) = if info.error.is_some() && !info.read {
                ("not read".to_string(), Some(t.error))
            } else if !info.read {
                ("reading…".to_string(), None)
            } else {
                (open[p].to_string(), None)
            };
            let selector = format!("issue-pick-{}", info.label);
            let why = info.error.clone();
            out.push(
                pane::row(t, ("issue-pick", at), at == self.picked)
                    .debug_selector(move || selector)
                    .child(
                        pane::cell(info.label.clone())
                            .when(Some(p) == current, |d| d.font_weight(FontWeight::SEMIBOLD)),
                    )
                    .child(pane::meta(t, meta, colour))
                    .when_some(why, |d, why| d.tooltip(kit::tip(why)))
                    .on_click(cx.listener(move |w, _, window, cx| w.choose_project(p, window, cx)))
                    .into_any_element(),
            );
        }
        let picked = Some(self.picked).filter(|&p| p < out.len());
        (out, picked)
    }

    /// The item's head: Back on the page along its trail, its title, Send
    /// and its menu. Beside the page it is the panel's header, whose Back
    /// and Close are the panel's.
    pub(crate) fn detail_head(
        &self,
        t: &Theme,
        row: usize,
        place: Place,
        cx: &mut Context<Self>,
    ) -> Div {
        let r = &self.board.rows[row];
        let mut head = match place {
            Place::Page => pane::head(t),
            Place::Beside | Place::Panel => div()
                .flex_1()
                .min_w_0()
                .h_full()
                .flex()
                .items_center()
                .gap(px(size::HEAD_GAP)),
        };
        if place == Place::Page && !self.trail.is_empty() {
            head = head.child(
                pane::act(t, "w-back", "Back")
                    .debug_selector(|| "w-back".into())
                    .tooltip(kit::tip("Alt+Left"))
                    .on_click(cx.listener(|w, _, _, cx| w.back(cx))),
            );
        }
        let title = r.title.clone();
        head = head
            .child(
                pane::title(t, r.title.clone())
                    .debug_selector(move || format!("issue-card-title-{title}")),
            )
            .child(
                pane::send(t, "issue-send", &self.host, self.line_of(row))
                    .debug_selector(|| "issue-send".into()),
            );
        head = head.child(
            pane::act(t, "issue-more", "…")
                .debug_selector(|| "issue-more".into())
                .tooltip(kit::tip("Start a session on it, copy its id"))
                .on_click(cx.listener(move |w, e: &ClickEvent, _, cx| {
                    w.host.menu(w.item_menu(row), e.position(), cx)
                })),
        );
        head
    }

    /// The item view: its title and actions on the head line, then what it
    /// is, what it is part of and waits on, its text, and its children.
    pub(crate) fn detail(
        &self,
        t: &Theme,
        row: usize,
        place: Place,
        cx: &mut Context<Self>,
    ) -> Div {
        let (body, aside) = self.item_body(t, row, place, cx);
        match place {
            // On the page the item's relations and children stand in a
            // column of their own beside its text, where the window has
            // the room.
            Place::Page => pane::detail_beside(
                t,
                "issue-detail",
                self.detail_head(t, row, place, cx),
                body,
                aside.map(|a| a.into_any_element()),
                pane::Narrow::Above,
            ),
            // Beside the page its head stands in the panel's header.
            Place::Beside => pane::panel_detail(t, "issue-beside-detail", None, body),
            Place::Panel => pane::panel_detail(t, "issue-panel-detail", None, body),
        }
    }

    /// The item's facts and text, and what it stands among (what it is
    /// part of, waits on and holds). On the page the second is a column
    /// of its own; beside the terminals, one column, relations before the
    /// text and children after it.
    fn item_body(
        &self,
        t: &Theme,
        row: usize,
        place: Place,
        cx: &mut Context<Self>,
    ) -> (Div, Option<Div>) {
        let r = &self.board.rows[row];
        let p = &self.board.projects[r.project];
        let id = r.written_id(&self.board);
        let names_project = id
            .strip_prefix(p.label.as_str())
            .is_some_and(|rest| rest.starts_with(['-', '#']));
        let mut facts = vec![id];
        // An id that starts with the project's name already names it.
        if !names_project {
            facts.push(p.label.clone());
        }
        if let Some(u) = r.updated {
            facts.push(format!("updated {} ago", when::age(u, now_ms())));
        }
        if !r.assignees.is_empty() {
            facts.push(format!("for {}", r.assignees.join(", ")));
        }
        if !r.labels.is_empty() {
            facts.push(r.labels.join(" "));
        }
        let mut body = div().flex().flex_col().gap(px(size::SECTION_GAP)).child(
            div()
                .flex()
                .flex_col()
                .gap(px(SPACE_XS))
                .child(self.what_it_is(t, row))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(size::HEAD_GAP))
                        .text_size(px(size::SMALL))
                        .text_color(t.dim)
                        // Each its own piece, so a narrow detail wraps
                        // the line between facts instead of cutting the
                        // last one off.
                        .children(
                            facts
                                .into_iter()
                                .enumerate()
                                .map(|(i, f)| if i == 0 { f } else { format!("· {f}") }),
                        ),
                ),
        );
        if let Some(pf) = &r.pull {
            let mut line = div()
                .flex()
                .flex_wrap()
                .gap(px(size::HEAD_GAP))
                .text_color(t.text2)
                .child(format!("{} → {}", pf.head, pf.base))
                .child(div().text_color(t.done).child(format!("+{}", pf.additions)))
                .child(
                    div()
                        .text_color(t.error)
                        .child(format!("−{}", pf.deletions)),
                );
            if let Some(c) = pf.checks {
                let all = c.passed + c.failed + c.pending;
                line = line.child(
                    div()
                        .text_color(if c.failed > 0 { t.error } else { t.text2 })
                        .child(if c.failed > 0 {
                            format!("{} of {all} checks failed", c.failed)
                        } else if c.pending > 0 {
                            format!("{} of {all} checks still run", c.pending)
                        } else {
                            format!("all {all} checks passed")
                        }),
                );
            }
            body = body.child(pane::section(t, "Branch").child(line));
        }
        let fam = &r.family;
        let mut relations = self.relations(t, row, place, cx);
        let on_page = place == Place::Page;
        if !on_page {
            body = body.children(relations.take());
        }
        let (words, missing) = match self.text_of(row) {
            Text::Read(text) | Text::Reading(Some(text)) => (text, "No description.".to_string()),
            Text::Reading(None) => (String::new(), "Reading the text…".to_string()),
            Text::Failed(error) => (String::new(), format!("Could not read the text: {error}")),
        };
        let text = prose::link_ids(words.trim(), &|id| {
            self.board.find_from(id, r.project).is_some()
        });
        if !text.is_empty() {
            let blocks = markdown::parse(&text);
            let me = cx.entity().downgrade();
            let project = r.project;
            let on_link: markdown::OnLink = Rc::new(move |link: &str, _, cx| {
                let link = link.to_string();
                let _ = me.update(cx, |w, cx| w.follow_link_in(&link, project, place, cx));
            });
            let look = markdown::Look {
                t,
                refs: p.tracker == Tracker::Github,
                on_link: Some(on_link),
                select: Some(self.prose.clone()),
            };
            let prose_area = div()
                .id("issue-card")
                .debug_selector(|| "issue-card".into())
                // Ctrl+C reaches the page, which copies the selection.
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|w, _, window, cx| {
                        if !w.focus.is_focused(window) {
                            window.focus(&w.focus, cx);
                        }
                    }),
                )
                .child(
                    div()
                        .debug_selector(|| "issue-text".into())
                        .child(markdown::render(&look, "issue-body", &blocks)),
                );
            body = body.child(select::track(
                prose_area,
                &self.prose,
                cx.entity_id(),
                select::copy,
            ));
        } else {
            body = body.child(
                div()
                    .id("issue-card")
                    .debug_selector(|| "issue-card".into())
                    .text_color(t.dim)
                    .child(missing),
            );
        }
        let pulls_label = if r.pull.is_some() {
            "Closes"
        } else {
            "Pull requests"
        };
        let pulls = if r.pull.is_some() {
            &fam.blocking
        } else {
            &fam.pulls
        };
        // A group's items, and how far they have come.
        let held = match self.progress(row) {
            Some((done, all)) => {
                let what = if r.kind == "epic" {
                    "In this epic"
                } else {
                    "Under it"
                };
                format!("{what}, {}", words::progress(done, all))
            }
            None => "Under it".into(),
        };
        let children = self.family(t, CHILDREN, held, &fam.children, place, cx);
        let pulls = self.family(t, pulls_label, pulls_label.into(), pulls, place, cx);
        if !on_page {
            return (body.children(children).children(pulls), None);
        }
        let aside: Vec<Div> = relations.into_iter().chain(children).chain(pulls).collect();
        let aside = (!aside.is_empty()).then(|| {
            div()
                .debug_selector(|| "issue-aside".into())
                .flex()
                .flex_col()
                .gap(px(size::SECTION_GAP))
                .children(aside)
        });
        (body, aside)
    }

    /// A section of the item's family: each a row with its title and its
    /// state, which shows that item; one no store read stays as written.
    #[allow(clippy::too_many_arguments)]
    fn family(
        &self,
        t: &Theme,
        label: &'static str,
        words: String,
        refs: &[String],
        place: Place,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        if refs.is_empty() {
            return None;
        }
        let closed = |found: &Option<usize>| {
            found.is_some_and(|i| self.board.rows[i].status == Status::Closed)
        };
        let mut rows: Vec<(usize, &String, Option<usize>)> = refs
            .iter()
            .enumerate()
            .map(|(k, reference)| (k, reference, self.board.find(reference)))
            .collect();
        // Open issues first, as the tree lists it; finished work after, and
        // folded to one line until asked for (the parent always shows).
        rows.sort_by_key(|(k, _, found)| (closed(found), *k));
        let finished = rows.iter().filter(|(_, _, f)| closed(f)).count();
        let fold = finished > 0 && label != PART_OF && !self.show_finished;
        let section = pane::section(t, words);
        let mut list = div().flex().flex_col().mx(px(-(size::ROW_PAD)));
        for (k, reference, found) in rows {
            if fold && closed(&found) {
                continue;
            }
            let row = pane::row(t, SharedString::from(format!("fam-{label}-{k}")), false);
            let row = match found {
                Some(i) => {
                    let r = &self.board.rows[i];
                    let selector = format!("issue-family-{label}-{}", r.id);
                    let status = status_meta(t, r);
                    // In the page's narrower second column a title takes
                    // a second line rather than losing its end.
                    let title = if place == Place::Page {
                        div()
                            .flex_1()
                            .min_w_0()
                            .py(px(SPACE_XS))
                            .line_clamp(2)
                            .text_ellipsis()
                            .child(r.title.clone())
                    } else {
                        pane::cell(r.title.clone())
                    };
                    row.debug_selector(move || selector)
                        .child(glyph(t, r))
                        .child(title.when(r.status == Status::Closed, |d| d.text_color(t.dim)))
                        .children(status.map(|(s, c)| pane::meta(t, s, Some(c))))
                        .child(pane::meta(t, r.written_id(&self.board), None))
                        .on_click(cx.listener(move |w, _, _, cx| w.go(i, place, cx)))
                }
                None => {
                    let selector = format!("issue-family-{label}-{reference}");
                    row.debug_selector(move || selector)
                        .cursor_default()
                        .child(pane::cell(reference.clone()).text_color(t.dim))
                }
            };
            list = list.child(row);
        }
        if fold {
            let selector = format!("issue-family-{label}-finished");
            list = list.child(
                pane::row(
                    t,
                    SharedString::from(format!("fam-{label}-finished")),
                    false,
                )
                .debug_selector(move || selector)
                .text_color(t.dim)
                .child(pane::cell(format!("{finished} finished")))
                .on_click(cx.listener(|w, _, _, cx| {
                    w.show_finished = true;
                    cx.notify();
                })),
            );
        }
        Some(section.child(list))
    }

    /// Shows a row reached from the item in `place`: in the panel it
    /// replaces the panel's item, elsewhere it joins the trail.
    pub(crate) fn go(&mut self, row: usize, place: Place, cx: &mut Context<Self>) {
        if place == Place::Panel {
            self.panel_item = self
                .board
                .rows
                .get(row)
                .map(crate::store::issue::RowKey::of);
            cx.notify();
        } else {
            self.follow_row(row, cx);
        }
    }

    fn follow_link_in(&mut self, link: &str, project: usize, place: Place, cx: &mut Context<Self>) {
        if place != Place::Panel {
            return self.follow_link(link, project, cx);
        }
        if let Some(id) = link.strip_prefix(prose::BEAD_LINK) {
            if let Some(row) = self.board.find_from(id, project) {
                self.go(row, place, cx);
            }
        } else {
            self.follow_link(link, project, cx);
        }
    }
}

const PART_OF: &str = "Part of";
/// The key of a group's items in selectors; the section says it in words.
const CHILDREN: &str = "Children";

impl Issue {
    /// What the item stands in: what it is part of, what it waits on and
    /// what waits on it, each a card that shows it. Finished blockers fold
    /// into one card until asked for; a reference no store read stays as
    /// written.
    fn relations(
        &self,
        t: &Theme,
        row: usize,
        place: Place,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let r = &self.board.rows[row];
        let fam = &r.family;
        let blocking: &[String] = if r.pull.is_some() { &[] } else { &fam.blocking };
        let groups: [(&'static str, &[String]); 3] = [
            (PART_OF, fam.parent.as_slice()),
            ("Blocked by", &fam.blockers),
            ("Blocking", blocking),
        ];
        let closed =
            |i: Option<usize>| i.is_some_and(|i| self.board.rows[i].status == Status::Closed);
        let mut cards: Vec<AnyElement> = Vec::new();
        for (label, refs) in groups {
            let mut found: Vec<(usize, &String, Option<usize>)> = refs
                .iter()
                .enumerate()
                .map(|(k, reference)| (k, reference, self.board.find(reference)))
                .collect();
            found.sort_by_key(|(k, _, f)| (closed(*f), *k));
            let finished = found.iter().filter(|(_, _, f)| closed(*f)).count();
            let fold = finished > 0 && label != PART_OF && !self.show_finished;
            for (k, reference, f) in found {
                if fold && closed(f) {
                    continue;
                }
                cards.push(self.card(t, label, k, reference, f, place, cx));
            }
            if fold {
                let selector = format!("issue-family-{label}-finished");
                cards.push(
                    card_frame(t, SharedString::from(format!("card-{label}-finished")))
                        .debug_selector(move || selector)
                        .justify_center()
                        .child(card_label(t, label))
                        .child(
                            div()
                                .text_color(t.dim)
                                .child(format!("{finished} finished")),
                        )
                        .on_click(cx.listener(|w, _, _, cx| {
                            w.show_finished = true;
                            cx.notify();
                        }))
                        .into_any_element(),
                );
            }
        }
        if cards.is_empty() {
            return None;
        }
        Some(
            div()
                .flex()
                .flex_wrap()
                .gap(px(size::HEAD_GAP))
                .children(cards),
        )
    }

    /// One related item as a card: how it relates, its state, its title
    /// over two lines at most, its id and how long since it moved.
    #[allow(clippy::too_many_arguments)]
    fn card(
        &self,
        t: &Theme,
        label: &'static str,
        k: usize,
        reference: &str,
        found: Option<usize>,
        place: Place,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let frame = card_frame(t, SharedString::from(format!("card-{label}-{k}")));
        let Some(i) = found else {
            let selector = format!("issue-family-{label}-{reference}");
            return frame
                .debug_selector(move || selector)
                .cursor_default()
                .child(card_label(t, label))
                .child(div().text_color(t.dim).child(reference.to_string()))
                .into_any_element();
        };
        let r = &self.board.rows[i];
        let selector = format!("issue-family-{label}-{}", r.id);
        let done = r.status == Status::Closed;
        let state = status_meta(t, r).map(|(s, c)| div().text_color(c).child(s));
        let age = r.updated.map(|u| when::short_age(u, now_ms()));
        frame
            .debug_selector(move || selector)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(SPACE_XS))
                    .child(card_label(t, label))
                    .child(div().flex_1())
                    .child(glyph(t, r).w_auto()),
            )
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .overflow_hidden()
                    .line_clamp(2)
                    .text_ellipsis()
                    .text_color(if done { t.dim } else { t.text })
                    .child(r.title.clone()),
            )
            .child(
                div()
                    .flex()
                    .gap(px(size::HEAD_GAP))
                    .text_size(px(size::SMALL))
                    .text_color(t.dim)
                    .child(r.written_id(&self.board))
                    .children(state)
                    .child(div().flex_1())
                    .children(age),
            )
            .on_click(cx.listener(move |w, _, _, cx| w.go(i, place, cx)))
            .into_any_element()
    }
}

/// The list's width until its edge is dragged, in `room` (from where it
/// starts to the window's right): a share of it, never under its usual
/// width unless the room is too small for that, never over its most.
pub(crate) fn open_width(room: f32) -> f32 {
    (room * LIST_SHARE_OPEN).clamp((room * LIST_SHARE).min(LIST_W), LIST_OPEN_MAX)
}

/// A card's frame: a raised tile, as wide as the room allows within its
/// bounds.
fn card_frame(t: &Theme, id: SharedString) -> Stateful<Div> {
    div()
        .id(id)
        .flex_1()
        .min_w(px(CARD_MIN))
        .max_w(px(CARD_MAX))
        .p(px(CARD_PAD))
        .rounded(px(size::RADIUS))
        .bg(t.surface)
        .border_1()
        .border_color(t.divider)
        .cursor_pointer()
        .hover(|d| d.bg(t.raised))
        .flex()
        .flex_col()
        .gap(px(SPACE_XS))
}

fn card_label(t: &Theme, label: &'static str) -> Div {
    div()
        .text_size(px(size::SMALL))
        .text_color(t.dim)
        .child(label)
}

/// A row's state as a mark ([`mark`]).
pub(crate) fn glyph(t: &Theme, r: &Row) -> Div {
    mark(t, r.status)
}

/// A state as a mark: to do a ring, under way half full in the running
/// colour, blocked full, on hold a dash, done a tick in the done colour.
/// The facets show each beside its word, as the list's key.
pub(crate) fn mark(t: &Theme, s: Status) -> Div {
    let (mark, ink) = match s {
        Status::Open => ("○", t.dim),
        Status::InProgress => ("◐", t.running),
        Status::Blocked => ("●", t.text2),
        Status::Deferred => ("–", t.dim),
        Status::Closed => ("✓", t.done),
    };
    div()
        .flex_none()
        .w(px(GLYPH_W))
        .flex()
        .justify_center()
        .text_color(ink)
        .child(mark)
}

/// The lines down the indent of a row `depth` deep: one under the mark of
/// each group it stands in, so a group's items read as its own.
fn guides(t: &Theme, depth: usize) -> Div {
    let mut d = div()
        .flex_none()
        .relative()
        .w(px(depth as f32 * INDENT))
        .h_full()
        // The row's gap after it would push the mark past its depth.
        .mr(px(-size::HEAD_GAP));
    for level in 0..depth {
        d = d.child(
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(px(level as f32 * INDENT + (GLYPH_W - GUIDE_W) / 2.))
                .w(px(GUIDE_W))
                .bg(t.divider),
        );
    }
    d
}

/// How far a group has come: a short bar filled in the done colour and,
/// in the list, `3/7` (`3 of 7 done` elsewhere).
pub(crate) fn meter(t: &Theme, done: usize, all: usize, short: bool) -> Div {
    let share = if all == 0 {
        0.
    } else {
        done as f32 / all as f32
    };
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap(px(SPACE_XS))
        .text_size(px(size::SMALL))
        .text_color(t.dim)
        .child(
            div()
                .w(px(METER_W))
                .h(px(METER_H))
                .rounded(px(METER_H / 2.))
                .bg(t.divider)
                .child(
                    div()
                        .h_full()
                        .w(px(METER_W * share))
                        .rounded(px(METER_H / 2.))
                        .bg(t.done),
                ),
        )
        .child(if short {
            format!("{done}/{all}")
        } else {
            words::progress(done, all)
        })
}

/// An error's first line with words in it: a tool's hints and blank
/// lines stay out of a one-line label.
pub(crate) fn first_line(e: &str) -> &str {
    e.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
}

/// A label between a list's rows, as high as a row so the list scrolls
/// evenly.
fn list_label(t: &Theme, text: impl Into<SharedString>) -> Div {
    div()
        .h(px(size::ROW))
        .flex()
        .items_end()
        .child(pane::label(t, text).pt_0())
}

/// `on Nov 3`, or `from Nov 3 to Nov 9`.
pub(crate) fn on_days(r: Range) -> String {
    if r.from == r.to {
        format!("on {}", range_label(r))
    } else {
        format!("from {}", range_label(r).replace(" – ", " to "))
    }
}
