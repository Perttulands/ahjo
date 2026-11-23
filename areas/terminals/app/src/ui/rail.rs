//! The Sessions rail and Peek. The rail lists every session the node
//! follows, by device and environment, then in CHROTE's name groups; it is
//! this page's section of the side panel, so it shows on every page. On
//! this page a click peeks, Ctrl+ and Shift+click pick rows to kill
//! together, a drag puts the session in a window, the menu does the rest;
//! on another page a click goes to the session here. Peek is the session's
//! terminal over the page, used as in a window, without sizing it.

use super::page::{Act, MenuItem, PEEK_SLOT, Terminals};
use super::place;
use super::session_acts::Gesture;
use super::surface::NameAt;
use super::tiles::{DragPreview, Dragged};
use crate::store::devices;
use crate::store::sessions::{self, Key};
use crate::store::workspace::LinkState;
use crate::theme::{Theme, size, theme};
use ahjo_app_kit::widgets::finder::marked_letters;
use ahjo_app_kit::widgets::kit::{self, Kind};
use ahjo_app_kit::widgets::{field, pane};
use ahjo_terminals_contract::{AgentEvent, EventKind, Session};
use gpui::{
    AnyElement, Context, Entity, FontWeight, MouseButton, MouseDownEvent, Subscription, WeakEntity,
    Window, div, prelude::*, px,
};

/// The rail as the side panel holds it: the page draws it, and it draws
/// again whenever the page does.
pub struct Rail {
    page: WeakEntity<Terminals>,
    _page_changed: Subscription,
}

impl Rail {
    pub fn new(page: &Entity<Terminals>, cx: &mut Context<Self>) -> Self {
        Self {
            page: page.downgrade(),
            _page_changed: cx.observe(page, |_, _, cx| cx.notify()),
        }
    }
}

impl Render for Rail {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.page
            .update(cx, |p, cx| {
                // Acting here is looking at the page, as inside it.
                div()
                    .size_full()
                    .capture_any_mouse_down(cx.listener(|s, _, _, _| s.acted = true))
                    .capture_key_down(cx.listener(|s, _, _, _| s.acted = true))
                    .child(p.render_rail(window, cx))
                    .into_any_element()
            })
            .unwrap_or_else(|_| div().into_any_element())
    }
}

impl Terminals {
    pub fn render_rail(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = theme(cx).clone();
        let tidy = self.render_tidy(cx);
        // The list's head as every page has it: the filter, then the acts.
        // Searching what sessions printed starts from the same words, as
        // the list's first row, so the head holds one search.
        let query = self.rail_search.read(cx).text.clone();
        // A filter that holds words shows how to empty it (Escape does too).
        let filter =
            div()
                .relative()
                .child(self.rail_search.clone())
                .when(!query.is_empty(), |d| {
                    d.child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .right(px(2.))
                            .flex()
                            .items_center()
                            .child(
                                kit::button(&t, "rail-search-clear", "×", Kind::Quiet)
                                    .debug_selector(|| "rail-search-clear".into())
                                    .tooltip(kit::tip("Clear  Esc"))
                                    .on_click(cx.listener(|s, _, window, cx| {
                                        s.clear_rail_search(cx);
                                        field::focus(&s.rail_search, window, cx);
                                    })),
                            ),
                    )
                });
        let header = pane::list_head(&t, Some(filter.into_any_element()))
            .child(
                kit::button(&t, "rail-new", "New", Kind::Quiet)
                    .tooltip(kit::tip("Start a session"))
                    .on_click(cx.listener(|s, _, window, cx| {
                        s.run(super::chords::Action::Launcher, window, cx);
                    })),
            )
            .child(
                kit::button(&t, "rail-close", "‹", Kind::Quiet)
                    .debug_selector(|| "rail-close".into())
                    .tooltip(kit::tip("Hide sessions  Alt+A"))
                    .on_click(cx.listener(|s, _, window, cx| {
                        s.run(super::chords::Action::Sessions, window, cx);
                    })),
            );
        // The filter matches names; the same words search what every
        // session printed, one click away.
        let search = (!query.trim().is_empty()).then(|| {
            let words = query.clone();
            pane::row(&t, "rail-search-inside", false)
                .debug_selector(|| "rail-search-inside".into())
                .child(pane::cell(format!("Search output for “{}”", query.trim())))
                // The key on the row itself: a tooltip lay over the words.
                .child(pane::meta(&t, "Alt+H", None))
                .on_click(cx.listener(move |s, _, window, cx| s.search_inside(&words, window, cx)))
        });
        let fleet = &self.fleet;
        let devices =
            devices::devices(
                &self.places,
                &self.settings.kept,
                fleet.here_id(),
                |n| match fleet.get(n) {
                    Some(c) => (
                        c.state == LinkState::Open,
                        c.known.then_some(&c.sessions[..]),
                    ),
                    None => (false, None),
                },
            );
        let mut list = pane::scroll("rail-list")
            .flex_1()
            .track_scroll(&self.rail_scroll)
            .py(px(8.));
        // The app's link says why the serving node does not answer.
        let here = self.fleet.here_id().and_then(|n| self.fleet.get(n));
        let note = match here.map(|c| c.state) {
            Some(LinkState::Open) => None,
            Some(LinkState::Lost | LinkState::Refused) => {
                Some("The node does not answer. Trying again by itself.".to_string())
            }
            Some(LinkState::Connecting) | None => Some("Connecting…".to_string()),
        };
        let quiet = |text: String| {
            div()
                .px(px(16.))
                .pb(px(8.))
                .text_size(px(12.))
                .text_color(t.dim)
                .child(text)
        };
        // Once devices are listed, an away one says so in its own place and
        // the status line names it: nothing above them moves.
        if let Some(n) = note.filter(|_| devices.is_empty()) {
            list = list.child(quiet(n));
        }
        let mut shown = 0;
        // What a pick counts from: the rows as drawn, collapsed groups out.
        self.rail_shown.clear();
        let mut existing = std::collections::HashSet::new();
        for d in &devices {
            for n in &d.nodes {
                existing.extend(n.sessions.iter().map(|s| Key::of(s).text()));
            }
        }
        self.pick.retain_existing(|k| existing.contains(k));
        for d in &devices {
            if !devices::visible_in_search(d, &query) {
                continue;
            }
            let mut block = div()
                .flex()
                .flex_col()
                .pb(px(6.))
                .child(self.device_heading(&t, &d.device, d.live));
            for n in &d.nodes {
                let found = sessions::search(&n.sessions, &query);
                if found.is_empty() && !query.trim().is_empty() {
                    continue;
                }
                shown += found.len();
                block =
                    block.child(self.environment_heading(&t, &n.environment, n.live || !d.live));
                for (group, members) in sessions::groups(&n.sessions, &found) {
                    if !self.settings.collapsed_groups.contains(&group) {
                        self.rail_shown.extend(members.iter().map(|s| Key::of(s)));
                    }
                    block = block.child(self.group(&t, &group, &members, n.live, cx));
                }
            }
            list = list.child(block.when(!d.live, |b| b.opacity(crate::theme::fade::DIMMED)));
        }
        if shown == 0 && here.is_some_and(|c| c.known) {
            list = list.child(quiet(if query.trim().is_empty() {
                "No sessions. New starts one.".to_string()
            } else {
                "No session matches.".to_string()
            }));
        }
        div()
            .track_focus(&self.rail_focus)
            .on_key_down(cx.listener(Self::rail_key))
            .size_full()
            .flex()
            .flex_col()
            .child(header)
            .children(search)
            .children(tidy)
            .child(list)
            .into_any_element()
    }

    /// A device: its plate, and "waiting" while none of it answers.
    fn device_heading(&self, t: &Theme, device: &str, live: bool) -> AnyElement {
        div()
            .px(px(12.))
            .pb(px(4.))
            .flex()
            .items_center()
            .gap(px(8.))
            .child(place::plate(t, device))
            .child(div().flex_1())
            .child(waiting_word(t, !live))
            .into_any_element()
    }

    /// An environment of a device; "waiting" when it alone is away.
    fn environment_heading(&self, t: &Theme, environment: &str, quiet: bool) -> AnyElement {
        div()
            .pl(px(22.))
            .pr(px(12.))
            .pb(px(2.))
            .flex()
            .items_center()
            .gap(px(8.))
            .child(place::strip(t, environment, 60.))
            .child(div().flex_1())
            .child(waiting_word(t, !quiet))
            .into_any_element()
    }

    fn group(
        &self,
        t: &Theme,
        name: &str,
        members: &[&Session],
        live: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let collapsed = self.settings.collapsed_groups.iter().any(|g| g == name);
        let g = name.to_string();
        let header = div()
            .id(gpui::SharedString::from(format!("group-{name}")))
            .mx(px(size::ROW_INSET))
            .border_t_1()
            .border_color(t.divider)
            .px(px(12. - size::ROW_INSET))
            .hover(|d| d.bg(t.accent_soft()))
            .pt(px(size::LABEL_TOP))
            .pb(px(size::LABEL_BOTTOM))
            .flex()
            .items_center()
            .gap(px(6.))
            .cursor_pointer()
            .child(
                div()
                    .w(px(10.))
                    .text_size(px(8.))
                    .text_color(t.dim)
                    .child(if collapsed { "▶" } else { "▼" }),
            )
            // A group's name reads before its rows: the label in full
            // colour, under a rule that starts the group.
            .child(
                kit::group_label(t, name.to_string())
                    .flex_1()
                    .text_color(t.text),
            )
            .child(kit::count(t, members.len()))
            .on_click(cx.listener(move |s, _, _, cx| {
                let list = &mut s.settings.collapsed_groups;
                if let Some(i) = list.iter().position(|x| *x == g) {
                    list.remove(i);
                } else {
                    list.push(g.clone());
                }
                s.save(cx);
                cx.notify();
            }));
        let mut items = div().flex().flex_col();
        if !collapsed {
            for s in members {
                items = items.child(self.row(t, s, live, cx));
            }
        }
        div()
            .mb(px(4.))
            .child(header)
            .child(items)
            .into_any_element()
    }

    fn row(&self, t: &Theme, s: &Session, live: bool, cx: &mut Context<Self>) -> AnyElement {
        let key = Key::of(s);
        let shown = self.settings.workspace.bound(&key);
        let in_focused = shown.is_some_and(|(tab, w)| {
            tab == self.settings.workspace.active && w == self.settings.workspace.focus
        });
        let picked = self.pick.has(&key.text());
        let mark = self.marked(s).cloned();
        // What it asks outranks what it last reported: a question on its
        // screen, else the needs-input report's.
        let asks = sessions::waiting(s).map(str::to_owned).or_else(|| {
            mark.as_ref()
                .filter(|e| e.kind == EventKind::NeedsInput)
                .map(|e| e.summary.clone())
        });
        let mark = mark.filter(|_| asks.is_none());
        let activity = if live {
            sessions::activity(s, super::page::now_ms())
        } else {
            "waiting".to_string()
        };
        let (k1, k2, k3) = (key.clone(), key.clone(), key.clone());
        let name = s.name.clone();
        let found = sessions::found_in_name(s, &self.rail_search.read(cx).text);
        let id = gpui::SharedString::from(format!("row-{}", key.text()));
        let selector = id.to_string();
        let group = id.clone();
        let inset = size::ROW_INSET;
        div()
            .id(id.clone())
            .group(group.clone())
            .debug_selector(move || selector)
            .min_h(px(size::ROW))
            .mx(px(inset))
            // The names stand under their group's name.
            .pl(px(size::RAIL_ROW_LEAD - inset))
            .pr(px(4.))
            .flex()
            .items_center()
            .gap(px(6.))
            .cursor_pointer()
            .text_color(if shown.is_some() { t.text } else { t.text2 })
            .relative()
            .rounded(px(size::RADIUS))
            // The focused window's session on a quiet fill, its name in
            // weight; picked rows on the accent wash. No bars.
            .when(in_focused, |d| {
                d.bg(t.raised).font_weight(FontWeight::SEMIBOLD)
            })
            .when(picked, |d| d.bg(t.accent_light()))
            .hover(|d| d.bg(t.accent_soft()))
            // The news dot keeps its room on every row, so the names stand
            // in one column; what a session runs is said on the right.
            .child(
                div()
                    .flex_none()
                    .w(px(size::DOT_SLOT))
                    .flex()
                    .justify_center()
                    .map(|d| match (&asks, &mark) {
                        (Some(_), _) => d.child(ask_mark(t, &key)),
                        (None, Some(e)) => d.child(event_dot(t, &key, e)),
                        (None, None) => d,
                    }),
            )
            .when_some(s.user.clone(), |d, u| d.child(kit::user_square(t, &u)))
            // What runs in it, before its name: the harness's mark, in a
            // slot every row keeps so the names stand in one column.
            .child(
                div()
                    .flex_none()
                    .w(px(size::HARNESS_SLOT))
                    .flex()
                    .justify_center()
                    .children(match sessions::running(s) {
                        sessions::Running::Harness(h) => crate::harness::harness_mark(h, 14., cx),
                        _ => None,
                    }),
            )
            // The name takes the room it needs, up to all of it; the
            // program's name beside it has what is left. The name is what a
            // person looks for.
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .flex_none()
                            .max_w_full()
                            .min_w_0()
                            .py(px(3.))
                            .flex()
                            .flex_col()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(if self.renaming_at(&key, NameAt::Rail) {
                                self.name_field().into_any_element()
                            } else {
                                name_label(&s.name, &found, t)
                                    .id(gpui::SharedString::from(format!("name-{}", key.text())))
                                    .tooltip(kit::tip(name.clone()))
                                    .into_any_element()
                            })
                            .children(asks.as_ref().map(|question| {
                                let question = question.lines().find(|l| !l.trim().is_empty());
                                div()
                                    .debug_selector(|| "row-asks".into())
                                    .text_size(px(11.))
                                    .text_color(t.needs)
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .child(match question {
                                        Some(q) => format!("Asks: {}", q.trim()),
                                        None => "Asks for your answer".to_string(),
                                    })
                            }))
                            .children(mark.as_ref().filter(|e| !e.summary.is_empty()).map(|e| {
                                div()
                                    .text_size(px(11.))
                                    .text_color(t.dim)
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .child(e.summary.clone())
                            })),
                    )
                    .children(command_word(t, s)),
            )
            .children(
                glyphs(s)
                    .into_iter()
                    .enumerate()
                    .map(|(n, (mark, why))| glyph(t, format!("{}-{n}", key.text()), mark, why)),
            )
            .child(
                div()
                    .flex_none()
                    .text_size(px(10.))
                    .text_color(if activity == "active" {
                        t.running
                    } else {
                        t.dim
                    })
                    .child(activity),
            )
            // The menu is always there at the row's end; a right click
            // opens it too.
            .child(
                div()
                    .id(gpui::SharedString::from(format!("rowmenu-{}", key.text())))
                    .debug_selector({
                        let selector = format!("rowmenu-{}", key.text());
                        move || selector
                    })
                    .flex_none()
                    .w(px(size::ROW_MENU))
                    .h(px(size::ROW_MENU))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(3.))
                    .text_color(t.dim)
                    .group_hover(group.clone(), |d| d.text_color(t.text2))
                    .hover(|d| d.bg(t.raised).text_color(t.text))
                    .child("…")
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |sh, e: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            let items = sh.menu_for(&k1);
                            sh.menu(items, e.position, window, cx);
                        }),
                    ),
            )
            .on_click(cx.listener(move |sh, e: &gpui::ClickEvent, window, cx| {
                let down = match e {
                    gpui::ClickEvent::Mouse(m) => Some(m.down.position),
                    _ => None,
                };
                sh.rail_click(&k2, Gesture::of(&e.modifiers()), down, window, cx)
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |sh, e: &MouseDownEvent, window, cx| {
                    let items = sh.menu_for(&k3);
                    sh.menu(items, e.position, window, cx);
                }),
            )
            .on_drag(
                Dragged {
                    key: key.clone(),
                    from: None,
                },
                move |_, _, _, cx| cx.new(|_| DragPreview::named(name.clone())),
            )
            .into_any_element()
    }

    /// A picked row's menu is the pick's, as in CHROTE; any other row's
    /// is its own.
    fn menu_for(&self, key: &Key) -> Vec<MenuItem> {
        if self.pick.has(&key.text()) && !self.pick_targets().is_empty() {
            self.pick_menu()
        } else {
            self.row_menu(key)
        }
    }

    pub fn row_menu(&self, key: &Key) -> Vec<MenuItem> {
        // What shapes an agent in its folder: this one's, or, for a shell
        // or a program, one started there.
        let sees = if self.fleet.session(key).is_some_and(sessions::is_agent) {
            "What this agent sees"
        } else {
            "What an agent here sees"
        };
        let mut items = vec![
            MenuItem::new("Peek", Act::Peek(key.clone())),
            MenuItem::new(sees, Act::Sees(key.clone())),
            MenuItem::new("Send…", Act::Send(key.clone())).chord("Alt+S"),
            MenuItem::new("Rename…", Act::Rename(key.clone(), NameAt::Rail)),
            self.copy_row(key),
            MenuItem::new("Copy folder path", Act::CopyPath(key.clone())),
        ];
        // Up to eight windows: one row, or a row per window behind one.
        let n = self.settings.workspace.windows().len();
        if n == 1 {
            items.push(MenuItem::new(
                "Attach to window",
                Act::Attach(key.clone(), 0),
            ));
        } else {
            let to = (0..n)
                .map(|w| MenuItem::new(format!("Window {}", w + 1), Act::Attach(key.clone(), w)))
                .collect();
            items.push(MenuItem::sub("Attach to window", None, to));
        }
        if self.settings.workspace.bound(key).is_some() {
            items.push(MenuItem::new("Unassign", Act::Unassign(key.clone())));
        }
        items.push(MenuItem::rule());
        items.push(MenuItem::new("Kill session", Act::Kill(key.clone())).danger());
        items
    }

    /// Peek: the session in a terminal over the page, used as in a window
    /// (typed into, pasted, scrolled back, selected, clicked) without
    /// sizing it. Escape is its program's; Shift+Escape, Close or a click
    /// anywhere outside its card closes it (the rail, the top bar, the
    /// page under it).
    pub fn render_peek(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let t = theme(cx).clone();
        let p = self.peek.as_ref()?;
        let key = p.key.clone();
        let pane = p.pane;
        let session = self.fleet.session(&key).cloned();
        // A device that went keeps its last screen here, saying since when.
        let away = (!self
            .fleet
            .get(&key.node)
            .is_some_and(|c| c.state == LinkState::Open))
        .then(|| self.away_since(&key.node));
        let word = match &away {
            Some(Some(at)) => Some(format!("offline since {at}, last screen")),
            Some(None) => Some("offline, last screen".to_string()),
            None => self
                .histories
                .contains_key(&PEEK_SLOT)
                .then(|| "reading back".to_string()),
        };
        let device = self
            .where_is(&key.node)
            .map(|p| p.device.to_uppercase())
            .unwrap_or_else(|| "the node".into());
        let (k1, k2) = (key.clone(), key.clone());
        let header = div()
            .flex_none()
            .h(px(size::PEEK_HEAD))
            .px(px(12.))
            .flex()
            .items_center()
            .gap(px(12.))
            .border_b_1()
            .border_color(t.divider)
            .children(
                self.where_is(&key.node)
                    .map(|p| place::passport(&t, &p.device, &p.environment)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(key.name.clone()),
            )
            .children(word.map(|w| {
                div()
                    .flex_none()
                    .text_size(px(12.))
                    .text_color(t.dim)
                    .child(w)
            }))
            .child(
                kit::button(&t, "peek-attach", "Attach", Kind::Plain)
                    .debug_selector(|| "peek-attach".into())
                    .tooltip(kit::tip("Put it in a window, sized to it"))
                    .on_click(cx.listener(move |s, _, window, cx| {
                        s.close(super::surface::PEEK, cx);
                        s.place(k1.clone(), window, cx);
                    })),
            )
            .child(
                kit::button(&t, "peek-send", "Send", Kind::Primary).on_click(cx.listener(
                    move |s, _, window, cx| {
                        s.close(super::surface::PEEK, cx);
                        s.open_send(k2.clone(), window, cx);
                    },
                )),
            )
            // Escape is the program's here, unlike every other layer, so
            // the way out says its key where it is seen.
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        kit::button(&t, "peek-close", "Close", Kind::Quiet)
                            .debug_selector(|| "peek-close".into())
                            .tooltip(kit::tip(
                                "Shift+Esc, or a click outside. Esc goes to the session.",
                            ))
                            .on_click(cx.listener(|s, _, _, cx| s.close(super::surface::PEEK, cx))),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(11.))
                            .text_color(t.dim)
                            .child("Shift+Esc"),
                    ),
            );
        let live = match (&session, pane) {
            (Some(s), Some(pane)) if away.is_none() => self.source(PEEK_SLOT, pane, s),
            _ => None,
        };
        let body: AnyElement = match (live, &session) {
            (Some(src), _) => self.term_body(PEEK_SLOT, src, true, &t, cx),
            (None, None) => kit::note(&t, format!("{} is not running.", key.name), None)
                .p(px(16.))
                .into_any_element(),
            (None, Some(_)) => {
                let last = pane.and_then(|pane| self.fleet.screen(&key.node, pane));
                let note = kit::note(
                    &t,
                    match away {
                        Some(_) => format!("Waiting for {device}; it comes back by itself."),
                        None => "Connecting…".to_string(),
                    },
                    None,
                )
                .into_any_element();
                self.detached_body(last, note, &t)
            }
        };
        Some(
            div()
                .absolute()
                .inset_0()
                .occlude()
                .track_focus(&self.peek_focus)
                .on_key_down(cx.listener(Self::term_key))
                .flex()
                .items_center()
                .justify_center()
                .bg(crate::theme::alpha(t.bg, 0.5))
                .child(
                    div()
                        .id("peek")
                        .debug_selector(|| "peek".into())
                        .w(gpui::relative(size::PEEK_SHARE))
                        .h(gpui::relative(size::PEEK_SHARE))
                        .flex()
                        .flex_col()
                        .bg(t.surface)
                        .border_1()
                        .border_color(t.accent)
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        // The scrim covers only the page; a down anywhere
                        // else in the window closes Peek as well.
                        .on_mouse_down_out(
                            cx.listener(|s, e: &MouseDownEvent, _, cx| s.peek_down_outside(e, cx)),
                        )
                        .on_mouse_move(cx.listener(Self::term_mouse_move))
                        .on_mouse_up(MouseButton::Left, cx.listener(Self::term_mouse_up))
                        .on_mouse_up_out(MouseButton::Left, cx.listener(Self::term_mouse_up))
                        .on_mouse_up(MouseButton::Middle, cx.listener(Self::term_mouse_up))
                        .on_mouse_up(MouseButton::Right, cx.listener(Self::term_mouse_up))
                        .on_scroll(cx.listener(Self::term_scroll))
                        .child(Self::zoom_claim(cx))
                        .child(header)
                        .child(
                            div()
                                .id("peek-body")
                                .debug_selector(|| "peek-body".into())
                                .flex_1()
                                .min_h_0()
                                .flex()
                                .flex_col()
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|s, e: &MouseDownEvent, window, cx| {
                                        cx.stop_propagation();
                                        s.tile_mouse_down(PEEK_SLOT, e, window, cx);
                                    }),
                                )
                                .on_mouse_down(
                                    MouseButton::Middle,
                                    cx.listener(|s, e: &MouseDownEvent, window, cx| {
                                        cx.stop_propagation();
                                        s.tile_mouse_down(PEEK_SLOT, e, window, cx);
                                    }),
                                )
                                .on_mouse_down(
                                    MouseButton::Right,
                                    cx.listener(|s, e: &MouseDownEvent, window, cx| {
                                        cx.stop_propagation();
                                        s.tile_mouse_down(PEEK_SLOT, e, window, cx);
                                    }),
                                )
                                .child(body),
                        ),
                )
                .into_any_element(),
        )
    }
}

impl DragPreview {
    pub fn named(name: String) -> Self {
        DragPreview { name }
    }
}

/// The quiet word an away device or environment carries. Its room is kept
/// while it answers, so the plate does not move when it goes or comes back.
fn waiting_word(t: &Theme, away: bool) -> gpui::Div {
    div()
        .flex_none()
        .when(!away, |d| d.opacity(0.))
        .text_size(px(10.))
        .text_color(t.dim)
        .child("waiting")
}

/// A program the session runs that is no harness, by its name at the row's
/// end; nothing for a shell or a harness, whose mark leads the row, or a
/// program named as the session is.
fn command_word(t: &Theme, s: &Session) -> Option<AnyElement> {
    match sessions::running(s) {
        sessions::Running::Command(c) if c != s.name => Some(
            div()
                // In the room the session's name leaves, at the row's end.
                .flex_1()
                .min_w_0()
                .flex()
                .justify_end()
                .child(
                    div()
                        .min_w_0()
                        .max_w(px(64.))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_size(px(10.))
                        .text_color(t.dim)
                        .child(c.to_string()),
                )
                .into_any_element(),
        ),
        _ => None,
    }
}

/// What the session runs, as CHROTE's row shows it: the harness mark, the
/// program's name, or nothing for a shell.
pub(super) fn running_mark(t: &Theme, s: &Session, cx: &gpui::App) -> Option<AnyElement> {
    match sessions::running(s) {
        sessions::Running::Harness(h) => crate::harness::harness_mark(h, 14., cx),
        sessions::Running::Command(c) => Some(
            div()
                .flex_none()
                .max_w(px(64.))
                .overflow_hidden()
                .whitespace_nowrap()
                .text_size(px(10.))
                .text_color(t.dim)
                .child(c.to_string())
                .into_any_element(),
        ),
        sessions::Running::Nothing => None,
    }
}

/// A session's name as CHROTE's `SessionLabel` draws it: the head clipped
/// with an ellipsis when the row is narrow, the tail always whole, and the
/// letters at `at` that the search found it by marked.
fn name_label(name: &str, at: &[usize], t: &Theme) -> gpui::Div {
    let (label, at) = sessions::rail_name_marked(name, size::RAIL_NAME_COLUMNS, at);
    let (head, tail) = sessions::split_name(&label);
    let cut = head.chars().count();
    let (in_head, in_tail): (Vec<usize>, Vec<usize>) = at.iter().partition(|&&i| i < cut);
    let in_tail: Vec<usize> = in_tail.into_iter().map(|i| i - cut).collect();
    clipped(
        (!head.is_empty()).then(|| marked_letters(head, &in_head, t)),
        marked_letters(tail, &in_tail, t),
    )
}

/// A name that gives way from its head when its room is short, its tail
/// (what tells sessions apart) always whole.
pub(super) fn clipped_name(label: &str) -> gpui::Div {
    let (head, tail) = sessions::split_name(label);
    clipped(
        (!head.is_empty()).then(|| head.to_string()),
        tail.to_string(),
    )
}

fn clipped(head: Option<impl IntoElement>, tail: impl IntoElement) -> gpui::Div {
    div()
        .min_w_0()
        .flex()
        .when_some(head, |d, head| {
            d.child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .child(head),
            )
        })
        .child(div().flex_none().child(tail))
}

/// The badges as CHROTE's row has them: one glyph each, windows and panes
/// sharing one.
fn glyphs(s: &Session) -> Vec<(&'static str, String)> {
    let mut out: Vec<(&'static str, String)> = vec![];
    for b in sessions::badges(s) {
        match out.iter_mut().find(|(m, _)| *m == b.mark) {
            Some((_, why)) => {
                why.push(' ');
                why.push_str(&b.why);
            }
            None => out.push((b.mark, b.why)),
        }
    }
    out
}

/// A badge as one glyph, its explanation on hover.
/// An agent's report, while it is news: a dot in the colour of what it
/// said, done or waiting for the operator.
fn event_dot(t: &Theme, key: &Key, e: &AgentEvent) -> AnyElement {
    let (why, colour) = match e.kind {
        EventKind::Finished => ("Its agent finished", t.done),
        EventKind::NeedsInput => ("Its agent needs input", t.needs),
    };
    div()
        .id(gpui::SharedString::from(format!("event-{}", key.text())))
        .flex_none()
        .child(kit::state_dot(colour))
        .tooltip(move |_, cx| cx.new(|_| kit::Tip(why.into())).into())
        .into_any_element()
}

/// A session waiting for the operator's answer: a question mark on the
/// colour of what waits for them, unlike a finished report's dot.
fn ask_mark(t: &Theme, key: &Key) -> AnyElement {
    div()
        .id(gpui::SharedString::from(format!("asks-{}", key.text())))
        .debug_selector({
            let selector = format!("asks-{}", key.text());
            move || selector
        })
        .flex_none()
        .size(px(size::ASK_MARK))
        .rounded_full()
        .bg(t.needs)
        .text_color(t.bg)
        .text_size(px(10.))
        .font_weight(gpui::FontWeight::BOLD)
        .flex()
        .items_center()
        .justify_center()
        .child("?")
        .tooltip(|_, cx| {
            cx.new(|_| kit::Tip("It waits for your answer".into()))
                .into()
        })
        .into_any_element()
}

fn glyph(t: &Theme, id: String, mark: &'static str, why: String) -> AnyElement {
    div()
        .id(gpui::SharedString::from(format!("glyph-{id}")))
        .flex_none()
        .text_size(px(11.))
        .text_color(t.text2)
        .tooltip(move |_, cx| cx.new(|_| kit::Tip(why.clone().into())).into())
        .child(mark)
        .into_any_element()
}
