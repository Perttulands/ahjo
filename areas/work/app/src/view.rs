//! Drawing the Work page on the kit's skeleton (DESIGN.md, Work): the
//! project's open work as a tree in the list, or a search's matches, or the
//! projects to choose from; the item beside it, read as a page reads. The
//! same item view stands beside a terminal and in the panel.

use crate::store::work::timeline::{Range, range_label};
use crate::store::work::{Row, Status, Tracker, prose, when};
use crate::text::Text;
use crate::tokens::*;
use crate::{Entry, Layout, Moved, Work};
use gpui::{
    AnyElement, ClickEvent, Context, Div, FontWeight, Hsla, MouseButton, MouseDownEvent,
    SharedString, Stateful, Window, div, prelude::*, px, uniform_list,
};
use parvi_app_kit::hub::now_ms;
use parvi_app_kit::theme::{FAMILY, Theme, size, theme};
use parvi_app_kit::widgets::kit;
use parvi_app_kit::widgets::markdown;
use parvi_app_kit::widgets::pane;
use parvi_app_kit::widgets::select;
use std::rc::Rc;

/// Where an item view stands: on the page, beside the page that named it,
/// or in the Work panel over the terminals.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Place {
    Page,
    Beside,
    Panel,
}

/// A status as the operator needs to see it: under way and blocked are
/// said, ready is not.
pub(crate) fn status_meta(t: &Theme, r: &Row) -> Option<(String, Hsla)> {
    match r.status {
        Status::InProgress => Some(("in progress".into(), t.running)),
        Status::Blocked => Some(("blocked".into(), t.text2)),
        Status::Deferred => Some(("deferred".into(), t.dim)),
        Status::Closed => Some((raw_label(r), t.dim)),
        Status::Open => None,
    }
}

impl Render for Work {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx).clone();
        let list = self.list_column(&t, cx);
        let detail = match self.shown_row() {
            Some(row) => {
                self.want_text(row, cx);
                self.detail(&t, row, Place::Page, cx)
            }
            // The list says why it holds nothing; the detail stays clear.
            None => pane::detail(&t, "work-detail", pane::head(&t), div()),
        };
        let page = match self.layout {
            Layout::Tree => pane::page(&t, list, detail),
            Layout::Strip => self.strip_page(&t, list, detail, cx),
            Layout::Time => self.time_page(&t, list, detail, cx),
        };
        page.id("work")
            .key_context("Work")
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

impl Work {
    /// Why the list holds nothing, in one line.
    pub(crate) fn why_empty(&self, cx: &gpui::App) -> String {
        if self.snapshots.is_empty() {
            return "Reading work…".into();
        }
        if self.board.projects.is_empty() {
            return "No projects are configured on this node: add a `beads` or `github` line to its work settings.".into();
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
            return "Reading work…".into();
        };
        let info = &self.board.projects[p];
        if let Some(e) = &info.error {
            format!("{} could not be read: {}", info.label, e.trim())
        } else if !info.read {
            format!("Reading {}…", info.label)
        } else {
            format!("Nothing open in {}.", info.label)
        }
    }

    /// The list column: its head, a line saying the project could not be
    /// read when so, and the rows.
    fn list_column(&mut self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let head = self.list_head(t, cx);
        let mut body = div().flex_1().min_h_0().flex().flex_col();
        if !self.picking
            && let Some(p) = self
                .project(cx)
                .filter(|_| !self.every || self.layout == Layout::Tree)
        {
            let info = &self.board.projects[p];
            let trouble = info
                .error
                .as_ref()
                .map(|e| format!("Not read: {}", e.trim()))
                .or_else(|| {
                    info.unwatched
                        .as_ref()
                        .map(|w| format!("Not watched: {}", w.trim()))
                });
            if let Some(trouble) = trouble.filter(|_| info.read) {
                body = body.child(pane::label(t, trouble).text_color(t.error));
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
                div()
                    .id("work-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.list_scroll)
                    .flex()
                    .flex_col()
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
                uniform_list(
                    "work-list",
                    n,
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
                .track_scroll(&self.rows_scroll)
                .flex_1(),
            );
        }
        // As wide as the operator dragged it, never more than most of
        // the room beside the facets.
        pane::list_wide_frame(t, head, body)
            .w(px(self.list_w))
            .max_w(gpui::relative(LIST_SHARE))
            .relative()
            .child(self.grip(t, cx))
    }

    /// The list's edge, dragged to widen or narrow it: a press on it holds
    /// the width it had, the drag moves it, the release keeps it.
    fn grip(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let page = cx.weak_entity();
        div()
            .id("work-grip")
            .debug_selector(|| "work-grip".into())
            .absolute()
            .top_0()
            .bottom_0()
            .right(px(-GRIP / 2.))
            .w(px(GRIP))
            .cursor(gpui::CursorStyle::ResizeLeftRight)
            .hover(|d| {
                d.bg(gpui::Hsla {
                    a: GRIP_INK,
                    ..t.accent
                })
            })
            .child(
                gpui::canvas(
                    |_, _, _| (),
                    move |b, _, window, _| {
                        let (down, moving, up) = (page.clone(), page.clone(), page);
                        window.on_mouse_event(move |e: &MouseDownEvent, phase, _, cx| {
                            if phase == gpui::DispatchPhase::Bubble
                                && e.button == MouseButton::Left
                                && b.contains(&e.position)
                            {
                                let x = f32::from(e.position.x);
                                let _ = down.update(cx, |w, _| w.grip = Some((x, w.list_w)));
                            }
                        });
                        window.on_mouse_event(move |e: &gpui::MouseMoveEvent, phase, _, cx| {
                            if phase == gpui::DispatchPhase::Bubble
                                && e.pressed_button == Some(MouseButton::Left)
                            {
                                let x = f32::from(e.position.x);
                                let _ = moving.update(cx, |w, cx| {
                                    if let Some((from, width)) = w.grip {
                                        w.drag_list(width + x - from, cx);
                                    }
                                });
                            }
                        });
                        window.on_mouse_event(move |e: &gpui::MouseUpEvent, phase, _, cx| {
                            if phase == gpui::DispatchPhase::Bubble && e.button == MouseButton::Left
                            {
                                let _ = up.update(cx, |w, cx| {
                                    if w.grip.take().is_some() {
                                        w.keep_list(cx);
                                    }
                                });
                            }
                        });
                    },
                )
                .size_full(),
            )
    }

    fn list_head(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let field = div().flex_1().min_w_0().child(self.search.clone());
        if self.picking {
            let act: AnyElement = if self.refreshing.is_empty() {
                pane::act(t, "work-read-again", "Read again")
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
                pane::act(t, "work-project", label)
                    .debug_selector(|| "work-project".into())
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

    /// The layout in use, by name; a press moves to the next.
    fn layout_act(&self, t: &Theme, cx: &mut Context<Self>) -> Stateful<Div> {
        pane::act(t, "work-layout", self.layout.name())
            .debug_selector(|| "work-layout".into())
            .tooltip(kit::tip("Layout: strip, time or tree (V)"))
            .on_click(cx.listener(|w, _, _, cx| w.set_layout(w.layout.next(), true, cx)))
    }

    /// A row of the list: its state, its title, what kind of work it is
    /// when not a plain task, a high priority, its project when another's,
    /// and how long since it last moved.
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
        let selector = format!("work-row-{}", r.id);
        let project = away.then(|| self.board.projects[r.project].label.clone());
        let quiet = matches!(
            r.status,
            Status::Closed | Status::Deferred | Status::Blocked
        );
        // Only what changes what the operator does next earns a word: a
        // bug, and work marked most urgent.
        let kind = (r.kind == "bug").then(|| r.kind.clone());
        let priority = r.priority.filter(|p| *p == 0);
        let age = r
            .closed
            .filter(|_| r.status == Status::Closed)
            .or(r.updated)
            .map(|u| when::short_age(u, now_ms()));
        pane::row(t, ("work-row", at), chosen)
            .debug_selector(move || selector)
            .h(px(size::ROW))
            .pl(px(
                size::ROW_PAD - size::ROW_INSET + depth.min(MAX_DEPTH) as f32 * INDENT
            ))
            .child(glyph(t, r))
            .child(
                pane::cell(r.title.clone())
                    .when(quiet && !chosen, |d| d.text_color(t.dim))
                    .when(!quiet && !chosen, |d| d.text_color(t.text)),
            )
            .children(kind.map(|k| pane::meta(t, k, None)))
            .children(priority.map(|p| {
                pane::meta(t, format!("P{p}"), Some(t.text)).font_weight(FontWeight::SEMIBOLD)
            }))
            .children(project.map(|p| {
                pane::meta(t, p, None)
                    .max_w(px(AWAY_W))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
            }))
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

    /// The projects to choose from: most open work first, each with how
    /// much is open, or why it was not read.
    fn project_rows(&self, t: &Theme, cx: &mut Context<Self>) -> (Vec<AnyElement>, Option<usize>) {
        let open = crate::store::work::outline::open_counts(&self.board);
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
            let selector = format!("work-pick-{}", info.label);
            let why = info.error.clone();
            out.push(
                pane::row(t, ("work-pick", at), at == self.picked)
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

    /// The item view: its title and actions on the head line, then what it
    /// is, what it is part of and waits on, its text, and its children.
    pub(crate) fn detail(
        &self,
        t: &Theme,
        row: usize,
        place: Place,
        cx: &mut Context<Self>,
    ) -> Div {
        let r = &self.board.rows[row];
        let mut head = pane::head(t);
        let back = match place {
            Place::Panel => true,
            _ => !self.trail.is_empty(),
        };
        if back {
            head = head.child(
                pane::act(t, "w-back", "Back")
                    .debug_selector(|| "w-back".into())
                    .tooltip(kit::tip("Alt+Left"))
                    .on_click(cx.listener(move |w, _, _, cx| {
                        if place == Place::Panel {
                            w.panel_item = None;
                            cx.notify();
                        } else {
                            w.back(cx);
                        }
                    })),
            );
        }
        let title = r.title.clone();
        head = head
            .child(
                pane::title(t, r.title.clone())
                    .debug_selector(move || format!("work-card-title-{title}")),
            )
            .child(
                pane::send(t, "work-send", &self.host, self.line_of(row))
                    .debug_selector(|| "work-send".into()),
            );
        head = head.child(
            pane::act(t, "work-more", "…")
                .debug_selector(|| "work-more".into())
                .tooltip(kit::tip("Start a session on it, copy its id"))
                .on_click(cx.listener(move |w, e: &ClickEvent, _, cx| {
                    w.host.menu(w.item_menu(row), e.position(), cx)
                })),
        );
        if place != Place::Page {
            head = head.child(
                pane::act(t, "work-close", "Close")
                    .tooltip(kit::tip(if place == Place::Panel {
                        "Alt+B"
                    } else {
                        "Alt+I"
                    }))
                    .on_click(cx.listener(move |w, _, _, cx| {
                        if place == Place::Panel {
                            w.close_panel(cx);
                        } else if let Some(from) = w.beside_from {
                            w.host.close_beside(from, cx);
                        }
                    })),
            );
        }
        let body = self.item_body(t, row, place, cx);
        let id = match place {
            Place::Page => "work-detail",
            Place::Beside => "work-beside-detail",
            Place::Panel => "work-panel-detail",
        };
        pane::detail(t, id, head, body)
    }

    fn item_body(&self, t: &Theme, row: usize, place: Place, cx: &mut Context<Self>) -> Div {
        let r = &self.board.rows[row];
        let p = &self.board.projects[r.project];
        let mut facts = vec![r.written_id(&self.board), r.kind.clone()];
        if let Some(pr) = r.priority {
            facts.push(format!("P{pr}"));
        }
        facts.push(p.label.clone());
        if let Some(u) = r.updated {
            facts.push(format!("updated {} ago", when::age(u, now_ms())));
        }
        if !r.assignees.is_empty() {
            facts.push(format!("for {}", r.assignees.join(", ")));
        }
        if !r.labels.is_empty() {
            facts.push(r.labels.join(" "));
        }
        let status = status_meta(t, r);
        let mut body = div().flex().flex_col().gap(px(size::SECTION_GAP)).child(
            div()
                .flex()
                .flex_wrap()
                .gap(px(size::HEAD_GAP))
                .text_size(px(size::SMALL))
                .text_color(t.dim)
                .children(status.map(|(s, c)| div().text_color(c).child(s)))
                .child(facts.join(" · ")),
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
                        .child(format!("checks {}/{all}", c.passed)),
                );
            }
            body = body.child(pane::section(t, "Branch").child(line));
        }
        let fam = &r.family;
        body = body.children(self.relations(t, row, place, cx));
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
                .id("work-card")
                .debug_selector(|| "work-card".into())
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
                        .debug_selector(|| "work-text".into())
                        .child(markdown::render(&look, "work-body", &blocks)),
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
                    .id("work-card")
                    .debug_selector(|| "work-card".into())
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
        body.children(self.family(t, "Children", &fam.children, place, cx))
            .children(self.family(t, pulls_label, pulls, place, cx))
    }

    /// A section of the item's family: each a row with its title and its
    /// state, which shows that item; one no store read stays as written.
    fn family(
        &self,
        t: &Theme,
        label: &'static str,
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
        // Open work first, as the tree lists it; finished work after, and
        // folded to one line until asked for (the parent always shows).
        rows.sort_by_key(|(k, _, found)| (closed(found), *k));
        let finished = rows.iter().filter(|(_, _, f)| closed(f)).count();
        let fold = finished > 0 && label != PART_OF && !self.show_finished;
        let section = pane::section(t, label);
        let mut list = div().flex().flex_col().mx(px(-(size::ROW_PAD)));
        for (k, reference, found) in rows {
            if fold && closed(&found) {
                continue;
            }
            let row = pane::row(t, SharedString::from(format!("fam-{label}-{k}")), false);
            let row = match found {
                Some(i) => {
                    let r = &self.board.rows[i];
                    let selector = format!("work-family-{label}-{}", r.id);
                    let status = status_meta(t, r);
                    row.debug_selector(move || selector)
                        .child(glyph(t, r))
                        .child(
                            pane::cell(r.title.clone())
                                .when(r.status == Status::Closed, |d| d.text_color(t.dim)),
                        )
                        .children(status.map(|(s, c)| pane::meta(t, s, Some(c))))
                        .child(pane::meta(t, r.written_id(&self.board), None))
                        .on_click(cx.listener(move |w, _, _, cx| w.go(i, place, cx)))
                }
                None => {
                    let selector = format!("work-family-{label}-{reference}");
                    row.debug_selector(move || selector)
                        .cursor_default()
                        .child(pane::cell(reference.clone()).text_color(t.dim))
                }
            };
            list = list.child(row);
        }
        if fold {
            let selector = format!("work-family-{label}-finished");
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
            self.panel_item = self.board.rows.get(row).map(crate::store::work::RowKey::of);
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

impl Work {
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
                let selector = format!("work-family-{label}-finished");
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
            let selector = format!("work-family-{label}-{reference}");
            return frame
                .debug_selector(move || selector)
                .cursor_default()
                .child(card_label(t, label))
                .child(div().text_color(t.dim).child(reference.to_string()))
                .into_any_element();
        };
        let r = &self.board.rows[i];
        let selector = format!("work-family-{label}-{}", r.id);
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

/// The tracker's own status word, as the operator knows it.
pub(crate) fn raw_label(r: &Row) -> String {
    match (r.status, r.raw_status.as_str()) {
        (Status::Blocked, "open") => "blocked".into(),
        (_, raw) => raw.replace('_', " "),
    }
}

/// A row's state as a mark: open a ring, under way half full in the
/// running colour, blocked full, deferred a dash, finished a tick in the
/// done colour.
pub(crate) fn glyph(t: &Theme, r: &Row) -> Div {
    let (mark, ink) = match r.status {
        Status::Open => ("○", t.dim),
        Status::InProgress => ("◐", t.running),
        Status::Blocked => ("●", t.text2),
        Status::Deferred => ("–", t.dim),
        Status::Closed => ("✓", t.done),
    };
    div().flex_none().w(px(GLYPH_W)).text_color(ink).child(mark)
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
