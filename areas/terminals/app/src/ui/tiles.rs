//! The Sessions page: the Sessions rail, the strip above the windows, and
//! the windows themselves, each a session's terminal, a state, or the
//! launcher. The terminal takes every key and the mouse.

use super::page::{Act, Geo, MenuItem, PEEK_SLOT, Terminals};
use super::place;
use super::surface::NameAt;
use crate::remote::SharedScreen;
use crate::remotes::PaneAt;
use crate::store::layout::{self, Axis, Side as Edge, Split};
use crate::store::sessions::{self, Key};
use crate::store::workspace::{Binding, LEAST_SHARE, MAX_WINDOWS, TileState, Workspace};
use crate::term::links::{self, Link};
use crate::term::paint::{self, Look};
use crate::term::screen::Screen;
use crate::term::{keys, mouse};
use crate::theme::{Theme, alpha, size, theme};
use ahjo_app_kit::Ask;
use ahjo_app_kit::widgets::kit::{self, Kind};
use ahjo_app_kit::zoom::{self, Zoom};
use ahjo_core::NodeId;
use ahjo_terminals_contract::commands::SessionListWindows;
use alacritty_terminal::index::{Column, Line, Point as GridPoint, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::TermMode;
use gpui::{
    AnyElement, Bounds, ClipboardItem, ContentMask, Context, DispatchPhase, ElementInputHandler,
    EntityInputHandler, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, ScrollDelta, ScrollEvent, UTF16Selection, Window, canvas, deferred, div, point,
    prelude::*, px, relative,
};
use std::ops::Range;
use std::sync::Arc;

mod drag;
pub(super) mod pictures;

/// How long a window's new grid must hold before tmux hears it: dragging a
/// divider or the window resizes the session once, when the drag rests,
/// not at every frame.
const RESIZE_SETTLE: std::time::Duration = std::time::Duration::from_millis(120);

/// A painted theme's ravens behind an empty window, covering it from its
/// middle. A drawn theme has none: two flat shapes stretched over a window
/// of any shape showed as scraps at its edge, and the launcher is enough.
fn window_art(t: &Theme) -> Option<gpui::Img> {
    t.direction.own().then(|| {
        gpui::img(t.art.window.clone())
            .absolute()
            .inset_0()
            .size_full()
            .object_fit(gpui::ObjectFit::Cover)
            .opacity(t.art.window_show)
    })
}

/// What a window shows while a drop on it would land: the part of it the
/// session or window would take, and what releasing does.
fn drop_note(t: &Theme, landing: &Landing) -> gpui::Div {
    let (edge, text) = match landing {
        Landing::Into(_) => (None, "Release to add here"),
        Landing::Split(_, e) => (Some(*e), "Release for a new window"),
        Landing::Move(_, _, e) => (Some(*e), "Release to move the window here"),
    };
    // The label keeps clear of the pointer, which is a quarter or more
    // in from every edge for Into and within a quarter of its edge
    // otherwise: it sits at the top, or at the half's inner end.
    let half = div()
        .absolute()
        .flex()
        .p(px(24.))
        .bg(alpha(t.accent, 0.16))
        .border_2()
        .border_color(t.accent)
        .child(
            kit::panel(t)
                .px(px(14.))
                .py(px(8.))
                .text_color(t.text)
                .child(text),
        );
    let half = match edge {
        None => half.inset_0().justify_center().items_start(),
        Some(Edge::Left) => (half.left_0().top_0().bottom_0().w(relative(0.5)))
            .items_center()
            .justify_end(),
        Some(Edge::Right) => (half.right_0().top_0().bottom_0().w(relative(0.5)))
            .items_center()
            .justify_start(),
        Some(Edge::Top) => (half.left_0().right_0().top_0().h(relative(0.5)))
            .justify_center()
            .items_end(),
        Some(Edge::Bottom) => (half.left_0().right_0().bottom_0().h(relative(0.5)))
            .justify_center()
            .items_start(),
    };
    div().absolute().inset_0().child(half)
}

/// A session dragged from the rail, or a tag from window `from`, onto a
/// window.
#[derive(Clone)]
pub struct Dragged {
    pub key: Key,
    pub from: Option<usize>,
}

/// Where in a window a drop is: near an edge, beside it; else in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone {
    Center,
    Edge(Edge),
}

/// How near an edge, as a share of the window, a drop goes beside it.
const EDGE_REACH: f32 = 0.25;

/// The zone of a window at `b` the pointer at `p` is in.
pub fn zone(b: Bounds<Pixels>, p: gpui::Point<Pixels>) -> Zone {
    let w = f32::from(b.size.width).max(1.);
    let h = f32::from(b.size.height).max(1.);
    let x = f32::from(p.x - b.origin.x) / w;
    let y = f32::from(p.y - b.origin.y) / h;
    let near = [
        (x, Edge::Left),
        (1. - x, Edge::Right),
        (y, Edge::Top),
        (1. - y, Edge::Bottom),
    ];
    match near.into_iter().min_by(|a, b| a.0.total_cmp(&b.0)) {
        Some((d, e)) if d < EDGE_REACH => Zone::Edge(e),
        _ => Zone::Center,
    }
}

/// What a drop does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Landing {
    /// The session becomes window `0`'s tag on screen.
    Into(usize),
    /// A new window beside window `0` takes the session.
    Split(usize, Edge),
    /// The window that holds only the session moves beside window `1`.
    Move(usize, usize, Edge),
}

/// What dropping `key` (a tag of window `from`, or a rail row) in `zone`
/// of window `i` does, if anything. A session alone in its window takes
/// the window with it; an empty window just takes the session; a tab with
/// every window shown adds it as a tag.
pub fn landing(
    ws: &Workspace,
    key: &Key,
    from: Option<usize>,
    i: usize,
    zone: Zone,
) -> Option<Landing> {
    let windows = ws.windows();
    let target = windows.get(i)?;
    let src = from.or_else(|| {
        ws.bound(key)
            .filter(|(tab, w)| *tab == ws.active && *w < windows.len())
            .map(|(_, w)| w)
    });
    let alone = src.filter(|j| {
        windows
            .get(*j)
            .is_some_and(|w| w.sessions.len() == 1 && &w.sessions[0].key == key)
    });
    let into = (src != Some(i)).then_some(Landing::Into(i));
    match zone {
        Zone::Edge(e) if !target.is_empty() => match alone {
            Some(j) => (j != i).then_some(Landing::Move(j, i, e)),
            None if windows.len() < MAX_WINDOWS => Some(Landing::Split(i, e)),
            None => into,
        },
        _ => into,
    }
}

pub struct DragPreview {
    pub name: String,
}

impl Render for DragPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        kit::panel(t)
            .px(px(10.))
            .py(px(4.))
            .text_color(t.text)
            .child(self.name.clone())
    }
}

/// A divider being dragged: the group it divides (its path in the
/// layout), the part it follows, where the press was, the group's shares
/// then and how many pixels the two parts either side of it had.
pub struct DividerDrag {
    pub path: Vec<usize>,
    pub after: usize,
    pub axis: Axis,
    pub from: gpui::Point<Pixels>,
    pub shares: Vec<u16>,
    pub pair: f32,
}

/// The fewest pixels a divider leaves a window.
const LEAST_PX: f32 = 90.;

/// What a window's terminal draws from.
pub(super) struct Source {
    screen: SharedScreen,
    /// This window sets the pane's grid.
    sizing: Option<PaneAt>,
    /// Reading back: the live pane, whose input stays in view below; the
    /// same screen as `screen` unless on the alternate screen.
    live: Option<SharedScreen>,
    /// Reading back: the row the view starts at (`Screen::view`).
    top: Option<usize>,
}

impl Terminals {
    pub fn render_terminals(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.tidy_histories();
        let mut row = div()
            .size_full()
            .flex()
            .flex_row()
            .capture_any_mouse_down(cx.listener(|s, _, _, _| s.acted = true))
            .capture_key_down(cx.listener(|s, _, _, _| s.acted = true));
        // What opens beside the windows (a file, an item, Send) stands in
        // the window's panel, and the windows make room for it.
        row = row.child(
            div()
                .flex_1()
                .min_w_0()
                .h_full()
                .relative()
                .flex()
                .flex_col()
                .child(self.render_grid(cx)),
        );
        row.relative().into_any_element()
    }

    fn render_grid(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx).clone();
        let n = self.settings.workspace.windows().len();
        if !cx.has_active_drag() {
            self.drop_at = None;
            self.in_air = None;
        }
        let mut tiles: Vec<Option<AnyElement>> =
            (0..n).map(|i| Some(self.render_tile(i, cx))).collect();
        let grid = div()
            .id("grid")
            .track_focus(&self.term_focus)
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .p(px(size::TILE_GAP))
            .bg(t.raised)
            .on_key_down(cx.listener(Self::term_key))
            .on_mouse_move(cx.listener(Self::term_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::term_mouse_up))
            // A drag that ends outside the windows still copies.
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::term_mouse_up))
            .on_mouse_up(MouseButton::Middle, cx.listener(Self::term_mouse_up))
            .on_mouse_up(MouseButton::Right, cx.listener(Self::term_mouse_up))
            .on_hover(cx.listener(|s, over: &bool, _, cx| {
                if !*over {
                    s.hover_link(None, cx);
                }
            }))
            .on_scroll(cx.listener(Self::term_scroll))
            .child(Self::zoom_claim(cx));
        // A press that began in a window is followed outside it, and outside
        // the app's window while the button is held.
        let entity = cx.entity();
        let grid = grid.child(
            canvas(
                |_, _, _| (),
                move |_, _, window, _| {
                    let entity = entity.clone();
                    window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, cx| {
                        if phase == DispatchPhase::Bubble {
                            entity.update(cx, |s, cx| s.term_drag(e, window, cx));
                        }
                    });
                },
            )
            .absolute()
            .size_0(),
        );
        let layout = self.settings.workspace.tab().layout.clone();
        let laid = self.render_split(&layout, &mut vec![], &mut tiles, &t, cx);
        grid.child(laid)
    }

    /// A part of the layout: a window, or a row or column of parts with a
    /// divider between each two.
    fn render_split(
        &mut self,
        split: &Split,
        path: &mut Vec<usize>,
        tiles: &mut [Option<AnyElement>],
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (axis, parts) = match split {
            Split::Window(w) => {
                return tiles
                    .get_mut(*w)
                    .and_then(Option::take)
                    .unwrap_or_else(|| div().into_any_element());
            }
            Split::Group { axis, parts } => (*axis, parts),
        };
        let groups = self.group_bounds.clone();
        let at = path.clone();
        let mut g = div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .size_full()
            .flex()
            .gap(px(size::TILE_GAP))
            .relative()
            .child(
                canvas(
                    |_, _, _| (),
                    move |b, _, _, _| {
                        groups.borrow_mut().insert(at.clone(), b);
                    },
                )
                .absolute()
                .inset_0(),
            );
        g = match axis {
            Axis::Row => g.flex_row(),
            Axis::Column => g.flex_col(),
        };
        let n = parts.len();
        for (k, (share, part)) in parts.iter().enumerate() {
            path.push(k);
            let inner = self.render_split(part, path, tiles, t, cx);
            path.pop();
            let divider = (k + 1 < n).then(|| self.divider(path, k, axis, t, cx));
            g = g.child(
                div()
                    .flex_grow(f32::from(*share))
                    .flex_shrink(1.)
                    .flex_basis(px(0.))
                    .min_w_0()
                    .min_h_0()
                    .when(axis == Axis::Row, |d| d.h_full())
                    .when(axis == Axis::Column, |d| d.w_full())
                    .flex()
                    .relative()
                    .child(inner)
                    .children(divider),
            );
        }
        g.into_any_element()
    }

    /// The divider after part `after` of the group at `path`: the gap and
    /// a pad either side, over both parts. Dragged, it moves the two parts'
    /// shares (the sessions resize once it rests); a double-click evens the
    /// group.
    fn divider(
        &self,
        path: &[usize],
        after: usize,
        axis: Axis,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let reach = size::TILE_GAP + 2. * size::SEAM_PAD;
        let out = px(-(size::TILE_GAP + size::SEAM_PAD));
        let held = self
            .divider
            .as_ref()
            .is_some_and(|d| d.path == path && d.after == after);
        let name = format!(
            "divider-{}{after}",
            path.iter().map(|k| format!("{k}.")).collect::<String>()
        );
        let line = div().when(held, |d| d.bg(t.accent));
        let (e, line) = match axis {
            Axis::Row => (
                div()
                    .top_0()
                    .bottom_0()
                    .right(out)
                    .w(px(reach))
                    .flex()
                    .justify_center()
                    .cursor(gpui::CursorStyle::ResizeLeftRight),
                line.w(px(size::TILE_GAP)).h_full(),
            ),
            Axis::Column => (
                div()
                    .left_0()
                    .right_0()
                    .bottom(out)
                    .h(px(reach))
                    .flex()
                    .flex_col()
                    .justify_center()
                    .cursor(gpui::CursorStyle::ResizeUpDown),
                line.h(px(size::TILE_GAP)).w_full(),
            ),
        };
        let path = path.to_vec();
        let selector = name.clone();
        deferred(
            e.id(gpui::ElementId::Name(name.into()))
                .debug_selector(move || selector.clone())
                .absolute()
                .group("divider")
                .child(line.group_hover("divider", |d| d.bg(t.accent_soft())))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |s, e: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        s.press_divider(&path, after, axis, e, cx);
                    }),
                ),
        )
        .with_priority(1)
        .into_any_element()
    }

    fn press_divider(
        &mut self,
        path: &[usize],
        after: usize,
        axis: Axis,
        e: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        let Some((_, parts)) = self.settings.workspace.tab().layout.group(path) else {
            return;
        };
        let shares: Vec<u16> = parts.iter().map(|p| p.0).collect();
        if e.click_count >= 2 {
            self.divider = None;
            if let Some((_, parts)) = self.settings.workspace.layout_mut().group_mut(path) {
                for p in parts.iter_mut() {
                    p.0 = layout::EVEN;
                }
            }
            self.save(cx);
            cx.notify();
            return;
        }
        let Some(b) = self.group_bounds.borrow().get(path).copied() else {
            return;
        };
        let length = match axis {
            Axis::Row => f32::from(b.size.width),
            Axis::Column => f32::from(b.size.height),
        };
        let room = length - (shares.len() as f32 - 1.) * size::TILE_GAP;
        let all: f32 = shares.iter().map(|s| f32::from(*s)).sum();
        let pair = room * (f32::from(shares[after]) + f32::from(shares[after + 1])) / all;
        self.divider = Some(DividerDrag {
            path: path.to_vec(),
            after,
            axis,
            from: e.position,
            shares,
            pair,
        });
        cx.notify();
    }

    /// A divider's drag moved to `p`: the parts either side share their
    /// room as it says.
    pub(super) fn drag_divider(&mut self, p: gpui::Point<Pixels>, cx: &mut Context<Self>) {
        let Some(d) = &self.divider else {
            return;
        };
        if d.pair <= 0. {
            return;
        }
        let moved = match d.axis {
            Axis::Row => f32::from(p.x - d.from.x),
            Axis::Column => f32::from(p.y - d.from.y),
        };
        let mut shares = d.shares.clone();
        let least = LEAST_SHARE.max(LEAST_PX / d.pair);
        layout::drag(&mut shares, d.after, moved / d.pair, least);
        let path = d.path.clone();
        match self.settings.workspace.layout_mut().group_mut(&path) {
            Some((_, parts)) if parts.len() == shares.len() => {
                for (part, s) in parts.iter_mut().zip(shares) {
                    part.0 = s;
                }
            }
            _ => self.divider = None,
        }
        cx.notify();
    }

    /// The zone of window `i` a drag is over, as its move says.
    fn drag_over(&mut self, i: usize, e: &gpui::DragMoveEvent<Dragged>, cx: &mut Context<Self>) {
        let here = e.bounds.contains(&e.event.position);
        let now = here.then(|| (i, zone(e.bounds, e.event.position)));
        if self.in_air.is_none() {
            self.in_air = Some(e.drag(cx).clone());
        }
        if here && self.drop_at != now {
            self.drop_at = now;
            cx.notify();
        } else if !here && self.drop_at.is_some_and(|(w, _)| w == i) {
            self.drop_at = None;
            cx.notify();
        }
    }

    /// Where the drag in the air would land, if released now.
    pub fn landing_now(&self, d: &Dragged) -> Option<Landing> {
        let (i, z) = self.drop_at?;
        landing(&self.settings.workspace, &d.key, d.from, i, z)
    }

    /// A drop: the session goes into a window, into a new one beside it,
    /// or its window moves.
    pub fn land(&mut self, d: &Dragged, l: Landing, window: &mut Window, cx: &mut Context<Self>) {
        let ws = &mut self.settings.workspace;
        match l {
            Landing::Into(i) => {
                // The window the session leaves empty closes.
                let emptied = d.from.filter(|j| {
                    ws.windows()
                        .get(*j)
                        .is_some_and(|w| w.sessions.len() == 1 && w.sessions[0].key == d.key)
                });
                self.attach(d.key.clone(), i, window, cx);
                if let Some(j) = emptied
                    && self.settings.workspace.close_window(j)
                {
                    self.settings.workspace.focus = if i > j { i - 1 } else { i };
                    self.histories.clear();
                    self.sync(cx);
                    self.save(cx);
                }
            }
            Landing::Split(i, e) => {
                if ws.split(i, e) {
                    let w = ws.focus;
                    self.attach(d.key.clone(), w, window, cx);
                }
            }
            Landing::Move(j, i, e) => {
                if ws.move_window(j, i, e) {
                    self.save(cx);
                    window.focus(&self.term_focus, cx);
                }
            }
        }
        cx.notify();
    }

    pub(super) fn source(
        &self,
        i: usize,
        pane: u32,
        session: &ahjo_terminals_contract::Session,
    ) -> Option<Source> {
        let at = PaneAt {
            node: session.node.clone(),
            pane,
        };
        if let Some(h) = self.histories.get(&i)
            && h.pane == at
        {
            return Some(Source {
                screen: h.screen.clone(),
                sizing: None,
                live: self.fleet.screen(&at.node, pane),
                top: Some(h.top),
            });
        }
        let screen = self.fleet.screen(&at.node, pane)?;
        // One window sizes a pane: the first in the tab that shows it,
        // unless tmux keeps the window at a fixed size.
        let first = self
            .window_panes()
            .iter()
            .position(|p| p.as_ref() == Some(&at))
            == Some(i);
        let sizing = (first && session.pinned != Some(true)).then_some(at);
        Some(Source {
            screen,
            sizing,
            live: None,
            top: None,
        })
    }

    fn render_tile(&mut self, i: usize, cx: &mut Context<Self>) -> AnyElement {
        let t = theme(cx).clone();
        let ws = &self.settings.workspace;
        let focused = ws.focus == i;
        let binding = ws.shown(i).cloned();
        let head = self.tile_head(i, focused, &t, cx);
        let state = self.fleet.tile(binding.as_ref());
        let landing = (self.in_air.as_ref())
            .and_then(|d| self.landing_now(d))
            .filter(|_| self.drop_at.is_some_and(|(w, _)| w == i));
        let drop_over = landing.is_some();
        let frame = div()
            .id(("tile", i))
            .debug_selector(move || format!("window-{i}"))
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(t.surface)
            .border_1()
            .border_color(if focused || drop_over {
                t.accent
            } else {
                t.divider
            })
            .when(drop_over, |d| d.border_dashed())
            .overflow_hidden()
            .on_drag_move(
                cx.listener(move |s, e: &gpui::DragMoveEvent<Dragged>, _, cx| {
                    s.drag_over(i, e, cx)
                }),
            )
            .on_drop(cx.listener(move |s, d: &Dragged, window, cx| {
                let l = s
                    .drop_at
                    .filter(|(w, _)| *w == i)
                    .and_then(|_| s.landing_now(d));
                s.drop_at = None;
                if let Some(l) = l {
                    s.land(d, l, window, cx);
                }
            }));
        let body: AnyElement = match state {
            // Only the focused empty window offers the launcher; others
            // say what they are for, and a click there brings it.
            TileState::Empty if !focused => div()
                .flex_1()
                .min_h_0()
                .relative()
                .flex()
                .items_center()
                .justify_center()
                .p(px(12.))
                .bg(t.term_bg)
                .text_color(t.text2)
                .debug_selector(move || format!("empty-window-{i}"))
                .children(window_art(&t))
                .child(
                    kit::panel(&t)
                        .px(px(14.))
                        .py(px(8.))
                        .child("Drag a session here, or click to start one"),
                )
                .into_any_element(),
            TileState::Empty => {
                let launcher = self.launchers[i].clone();
                // The launcher stays in its window, whatever its size: in a
                // short one its fields scroll under its pinned button, and
                // the windows beside it stay in sight.
                let slot = div()
                    .flex()
                    .flex_col()
                    .flex_shrink_1()
                    .min_h_0()
                    .max_w_full()
                    .child(launcher);
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .bg(t.term_bg)
                    .children(window_art(&t))
                    .child(
                        div()
                            .id(("launcher-scroll", i))
                            .debug_selector(move || format!("launcher-scroll-{i}"))
                            .absolute()
                            .inset_0()
                            .overflow_hidden()
                            .p(px(12.))
                            .flex()
                            .flex_col()
                            .items_center()
                            // Up to a fixed share of the window above the panel,
                            // not centring: the panel keeps its top edge when
                            // the node it targets lists more or fewer harnesses.
                            // In a short window that room goes first, then the
                            // panel's fields scroll under its pinned button.
                            .child(div().flex_1().max_h(relative(size::LAUNCHER_TOP)))
                            .child(slot)
                            .child(
                                div()
                                    .flex_none()
                                    .pt(px(14.))
                                    .font_family(t.ty.reading)
                                    .text_size(px(size::TEXT))
                                    .text_color(t.text2)
                                    .child("or drag a session here"),
                            )
                            .child(div().flex_1().min_h(px(0.))),
                    )
                    .into_any_element()
            }
            TileState::Live { session, pane } => {
                let session = session.clone();
                match self.source(i, pane, &session) {
                    Some(src) => self.term_body(i, src, focused, &t, cx),
                    None => self.detached_body(
                        None,
                        kit::note(&t, "Connecting…", None).into_any_element(),
                        &t,
                    ),
                }
            }
            TileState::Connecting(_) => self.detached_body(
                None,
                kit::note(&t, "Connecting…", None).into_any_element(),
                &t,
            ),
            TileState::Lost(_) => {
                let last = self.bound_pane_screen(i);
                let node = self.window_node(i);
                let device = node
                    .as_deref()
                    .and_then(|n| self.where_is(n))
                    .map(|p| p.device.to_uppercase())
                    .unwrap_or_else(|| "the node".into());
                let since = node
                    .as_deref()
                    .and_then(|n| self.away_since(n))
                    .map(|at| format!("Away since {at}. "))
                    .unwrap_or_default();
                let note = div()
                    .px(px(14.))
                    .py(px(8.))
                    .rounded(px(size::RADIUS))
                    .bg(alpha(t.surface, 0.85))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(2.))
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(format!("Waiting for {device}")),
                    )
                    .child(kit::note(
                        &t,
                        format!("{since}The session is kept; it comes back by itself."),
                        Some(t.dim),
                    ));
                self.detached_body(last, note.into_any_element(), &t)
            }
            TileState::Ended(name) => {
                let note = match binding
                    .as_ref()
                    .and_then(|b| self.settings.restarts.get(&b.key.text()))
                {
                    Some(saved) if saved.harness.is_some() => format!("{name} has ended."),
                    Some(saved) if !saved.folder.is_empty() => format!(
                        "{name} has ended. Restart opens a shell in {}.",
                        saved.folder
                    ),
                    _ => format!("{name} has ended. Restart opens a shell in its home folder."),
                };
                let last = binding
                    .as_ref()
                    .and_then(|b| self.fleet.ended_screen(&b.key, b.pane));
                let key = binding.clone().map(|b| b.key).unwrap_or_default();
                let restart = if self.restarting.contains(&key) {
                    "Restarting…"
                } else {
                    "Restart"
                };
                let panel = self.ended_panel(
                    i,
                    &note,
                    &[
                        (restart, Act::Restart(key.clone())),
                        ("Remove", Act::Remove(i, key)),
                    ],
                    &t,
                    cx,
                );
                self.detached_body(last, panel, &t)
            }
        };
        let grids = self.launch_grids.clone();
        let tiles = self.tile_bounds.clone();
        let font = self.font_of(i);
        // An empty window has no head yet; the session launched in it will.
        let tagged = !self.settings.workspace.windows()[i].sessions.is_empty();
        let head_to_come = if tagged { 0. } else { size::TILE_HEAD };
        let measure = canvas(
            |_, _, _| (),
            move |b, _, window, _| {
                let pad = px(2. * size::TERM_PAD);
                let inner = gpui::size(b.size.width - pad, b.size.height - pad - px(head_to_come));
                grids.borrow_mut()[i] = Some(paint::grid_for(inner, paint::metrics(window, font)));
                tiles.borrow_mut()[i] = Some(b);
            },
        )
        .absolute()
        .inset_0();
        // Only the body focuses the window and reaches the terminal; the
        // head is the tags'.
        let body = div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .relative()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |s, e: &MouseDownEvent, window, cx| {
                    s.tile_mouse_down(i, e, window, cx)
                }),
            )
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener(move |s, e: &MouseDownEvent, window, cx| {
                    s.tile_mouse_down(i, e, window, cx)
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |s, e: &MouseDownEvent, window, cx| {
                    s.tile_mouse_down(i, e, window, cx)
                }),
            )
            .child(measure)
            .child(body)
            .children(landing.map(|l| drop_note(&t, &l)));
        // An empty window has no tags, so no strip for them.
        frame
            .when(tagged, |f| f.child(head))
            .child(body)
            .into_any_element()
    }

    fn bound_pane_screen(&self, i: usize) -> Option<SharedScreen> {
        let b = self.settings.workspace.shown(i)?;
        let s = self.fleet.session(&b.key)?;
        let p = s
            .panes
            .iter()
            .find(|p| Some(p.id) == b.pane)
            .or(s.panes.first())?;
        self.fleet.screen(&b.key.node, p.id)
    }

    fn window_node(&self, i: usize) -> Option<String> {
        Some(self.settings.workspace.shown(i)?.key.node.clone())
    }

    /// The passport of the node window `i` shows a session of, when the
    /// fleet has more than one node to tell apart.
    fn passport_here(&self, i: usize, t: &Theme) -> Option<gpui::Div> {
        if self.fleet.clients().len() < 2 {
            return None;
        }
        let p = self.where_is(&self.window_node(i)?)?;
        Some(place::passport(t, &p.device, &p.environment))
    }

    /// A window's head: its device's passport, a tag for each session bound
    /// to it, and Send and the menu for the one on screen. A right click
    /// anywhere on it opens that session's menu.
    fn tile_head(
        &mut self,
        i: usize,
        focused: bool,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let w = self.settings.workspace.windows()[i].clone();
        let mut tags = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .items_end()
            .gap(px(2.))
            .overflow_hidden();
        for (n, b) in w.sessions.iter().enumerate() {
            tags = tags.child(self.tag(i, n, b, n == w.active, focused, t, cx));
        }
        let mut head = self
            .head_frame(t)
            .id(("head", i))
            .children(self.passport_here(i, t))
            .child(tags);
        let Some(shown) = w.shown().cloned() else {
            return head.into_any_element();
        };
        if let TileState::Live { .. } = self.fleet.tile(Some(&shown)) {
            let send_key = shown.key.clone();
            let menu_key = shown.key.clone();
            head =
                head.child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(px(2.))
                        .pb(px(3.))
                        .child(
                            kit::button(t, ("send", i), "Send", Kind::Quiet)
                                .debug_selector(move || format!("window-send-{i}"))
                                .on_click(cx.listener(move |s, _, window, cx| {
                                    s.open_send(send_key.clone(), window, cx);
                                })),
                        )
                        .child(kit::button(t, ("menu", i), "…", Kind::Quiet).on_click(
                            cx.listener(move |s, e: &gpui::ClickEvent, window, cx| {
                                let items = s.session_menu(i, &menu_key);
                                s.menu(items, e.position(), window, cx);
                            }),
                        )),
                );
        }
        let key = shown.key;
        head.on_mouse_down(
            MouseButton::Right,
            cx.listener(move |s, e: &MouseDownEvent, window, cx| {
                let items = s.session_menu(i, &key);
                s.menu(items, e.position, window, cx);
            }),
        )
        .into_any_element()
    }

    /// One session's tag: pressed it goes on screen, dragged it moves to
    /// another window or a seam, × takes it out of the window.
    #[allow(clippy::too_many_arguments)]
    fn tag(
        &mut self,
        i: usize,
        n: usize,
        b: &Binding,
        active: bool,
        focused: bool,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = b.key.clone();
        let state = self.fleet.tile(Some(b));
        let session = match &state {
            TileState::Live { session, .. } => Some((*session).clone()),
            _ => None,
        };
        let word = if self.restarting.contains(&key) {
            Some("restarting")
        } else {
            match state {
                TileState::Live { .. } | TileState::Empty => None,
                TileState::Connecting(_) => Some("connecting"),
                TileState::Lost(_) => Some("offline"),
                TileState::Ended(_) => Some("ended"),
            }
        };
        let seat = session.as_ref().and_then(|s| match s.seat {
            Some(ahjo_terminals_contract::SizingPane::Mine) => None,
            Some(ahjo_terminals_contract::SizingPane::Other) => Some("viewing"),
            None if s.pinned == Some(true) => Some("viewing"),
            None => None,
        });
        let history = active && self.histories.contains_key(&i);
        // The one state that changes what the pointer does here.
        let program_mouse = active && self.mouse_mode(i, false).is_some();
        let id = i * 100 + n;
        let (k_show, k_menu, k_remove) = (key.clone(), key.clone(), key.clone());
        let name = key.name.clone();
        div()
            .id(("tag", id))
            .flex_shrink(1.)
            .min_w(px(0.))
            .max_w(px(size::TAG_MAX))
            .h(px(28.))
            .pl(px(8.))
            .pr(px(2.))
            .flex()
            .items_center()
            .gap(px(6.))
            .rounded_t(px(size::RADIUS))
            .cursor_pointer()
            // The one on screen is a card of the terminal's own ground,
            // joined to it over the head's rule; the others are words.
            .border_1()
            .border_b_0()
            .border_color(if active {
                t.divider
            } else {
                gpui::transparent_black()
            })
            .when(active, |d| d.bg(t.term_bg))
            .font_weight(if active && focused {
                gpui::FontWeight::SEMIBOLD
            } else {
                gpui::FontWeight::NORMAL
            })
            .text_color(if active { t.text } else { t.text2 })
            .hover(|d| d.text_color(t.text))
            .when_some(session.as_ref().and_then(|s| s.user.clone()), |d, u| {
                d.child(kit::user_square(t, &u))
            })
            .when(
                session
                    .as_ref()
                    .is_some_and(|s| sessions::waiting(s).is_some()),
                |d| d.child(kit::waiting_dot(t)),
            )
            // What it runs, as its rail row says: nothing for a shell.
            .children(
                session
                    .as_ref()
                    .and_then(|s| super::rail::running_mark(t, s, cx)),
            )
            // The name keeps its room; badges give way before it.
            .child(if self.renaming_at(&key, NameAt::Window(i)) {
                self.name_field().into_any_element()
            } else {
                super::rail::clipped_name(&sessions::rail_name(&key.name, size::RAIL_NAME_COLUMNS))
                    .flex_none()
                    .whitespace_nowrap()
                    .into_any_element()
            })
            // What a terminal does not show is said for the one on screen;
            // in a narrow window the badges give way, never the name or ×.
            .children(session.iter().filter(|_| active).map(|s| {
                div()
                    .flex_shrink(1.)
                    .min_w_0()
                    .overflow_hidden()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .children(
                        sessions::badges(s)
                            .into_iter()
                            .enumerate()
                            .map(|(m, b)| kit::badge(t, ("badge", id * 10 + m), b.text, b.why)),
                    )
            }))
            .children(word.or(seat).map(|w| {
                div()
                    .flex_none()
                    .text_size(px(10.))
                    .text_color(t.dim)
                    .child(w)
            }))
            .when(program_mouse, |d| {
                d.child(
                    kit::badge(
                        t,
                        ("program-mouse", id),
                        "mouse goes to the program",
                        "The program here reads the mouse: clicks and the wheel go to it. \
                         Hold Shift to select text or scroll back."
                            .into(),
                    )
                    .debug_selector(move || format!("program-mouse-{i}")),
                )
            })
            .when(history, |d| {
                d.child(
                    div()
                        .flex_none()
                        .text_size(px(10.))
                        .text_color(t.accent)
                        .child("history"),
                )
            })
            .child(
                div()
                    .id(("tag-remove", id))
                    .flex_none()
                    .px(px(4.))
                    .text_color(t.dim)
                    .hover(|d| d.text_color(t.text))
                    .child("×")
                    // The remove control is not the tag: it neither shows
                    // the session nor starts a drag.
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |s, _, window, cx| {
                        s.act(Act::Remove(i, k_remove.clone()), window, cx)
                    })),
            )
            // The press shows it, so a click that drifts into a drag still
            // does; only the primary button, the other one is the menu's.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |s, _, window, cx| {
                    if !s
                        .settings
                        .workspace
                        .shown(i)
                        .is_some_and(|b| b.key == k_show)
                    {
                        s.act(Act::ShowTag(i, k_show.clone()), window, cx);
                    }
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |s, e: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    let items = s.session_menu(i, &k_menu);
                    s.menu(items, e.position, window, cx);
                }),
            )
            .on_drag(Dragged { key, from: Some(i) }, move |_, _, _, cx| {
                cx.new(|_| DragPreview::named(name.clone()))
            })
            .into_any_element()
    }

    fn head_frame(&self, t: &Theme) -> gpui::Div {
        div()
            .flex_none()
            .h(px(size::TILE_HEAD))
            .px(px(8.))
            .pt(px(4.))
            .relative()
            .flex()
            .items_end()
            .gap(px(8.))
            .bg(t.raised)
            // The rule under the tags, first so the tag on screen covers it.
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h(px(1.))
                    .bg(t.divider),
            )
    }

    /// The menu of a session bound in window `i`: the full set while it is on
    /// screen and live, else what acts on the session itself.
    pub fn session_menu(&self, i: usize, key: &Key) -> Vec<MenuItem> {
        let shown = self
            .settings
            .workspace
            .shown(i)
            .is_some_and(|b| &b.key == key);
        let pane = self.window_panes().get(i).cloned().flatten();
        if shown && let Some(at) = pane {
            return self.tile_menu(i, key, at.pane);
        }
        let mut items = vec![];
        if !shown {
            items.push(MenuItem::new(
                "Show in this window",
                Act::ShowTag(i, key.clone()),
            ));
        }
        let live = self.fleet.session(key).is_some();
        if live {
            items.push(MenuItem::new("Send…", Act::Send(key.clone())).chord("Alt+S"));
            items.push(MenuItem::new("Peek", Act::Peek(key.clone())));
        } else if matches!(
            self.fleet.tile(Some(&Binding {
                key: key.clone(),
                pane: None
            })),
            TileState::Ended(_)
        ) {
            items.push(MenuItem::new("Restart", Act::Restart(key.clone())));
        }
        items.push(MenuItem::rule());
        if live {
            items.push(MenuItem::new(
                "Rename session…",
                Act::Rename(key.clone(), NameAt::Window(i)),
            ));
            items.push(self.copy_row(key));
            items.push(MenuItem::new(
                "Copy folder path",
                Act::CopyPath(key.clone()),
            ));
            items.push(MenuItem::rule());
        }
        items.extend(super::views::layout_rows(&self.settings.workspace, i));
        items.push(MenuItem::new(
            "Remove from window",
            Act::Remove(i, key.clone()),
        ));
        if live {
            items.push(MenuItem::rule());
            items.push(MenuItem::new("Kill session", Act::Kill(key.clone())).danger());
        }
        items
    }

    pub fn tile_menu(&self, i: usize, key: &Key, pane: u32) -> Vec<MenuItem> {
        let mut items = vec![
            MenuItem::new("Send…", Act::Send(key.clone())).chord("Alt+S"),
            MenuItem::new("Peek", Act::Peek(key.clone())),
            MenuItem::new("Scroll back", Act::History(i)),
        ];
        let session = self.fleet.session(key);
        if session.is_some_and(|s| s.windows.unwrap_or(0) > 1) {
            items.push(MenuItem::new(
                "tmux windows…",
                Act::WindowList(i, key.clone()),
            ));
        }
        if let Some(s) = session
            && s.panes.len() > 1
        {
            items.push(MenuItem::rule());
            for (n, p) in s.panes.iter().enumerate() {
                items.push(
                    MenuItem::new(
                        format!("Pane {} ({}×{})", n + 1, p.cols, p.rows),
                        Act::Pane(i, p.id),
                    )
                    .checked(p.id == pane),
                );
            }
        }
        items.extend(self.zoom_row(i));
        items.push(MenuItem::rule());
        items.push(MenuItem::new(
            "Rename session…",
            Act::Rename(key.clone(), NameAt::Window(i)),
        ));
        items.push(self.copy_row(key));
        items.push(MenuItem::new(
            "Copy folder path",
            Act::CopyPath(key.clone()),
        ));
        items.push(MenuItem::rule());
        items.extend(super::views::layout_rows(&self.settings.workspace, i));
        items.push(MenuItem::new(
            "Remove from window",
            Act::Remove(i, key.clone()),
        ));
        items.push(MenuItem::rule());
        items.push(MenuItem::new("Kill session", Act::Kill(key.clone())).danger());
        items
    }

    /// Where a terminal's menu says its text size: a zoomed one offers
    /// the reset, at the size it is.
    pub(super) fn zoom_row(&self, i: usize) -> Option<MenuItem> {
        self.own_font(i)?;
        let size = self.font_of(i);
        Some(
            MenuItem::new(format!("Reset text size ({size} px)"), Act::ResetZoom(i))
                .chord("Ctrl+0"),
        )
    }

    /// Peek's menu: what the rail's row offers, without Peek itself, and
    /// reading back.
    pub(super) fn peek_menu(&self) -> Vec<MenuItem> {
        let Some(key) = self.peek.as_ref().map(|p| p.key.clone()) else {
            return vec![];
        };
        let mut items = vec![MenuItem::new("Scroll back", Act::History(PEEK_SLOT))];
        items.extend(self.zoom_row(PEEK_SLOT));
        items.extend(
            self.row_menu(&key)
                .into_iter()
                .filter(|m| m.act != Some(Act::Peek(key.clone()))),
        );
        items
    }
    /// The session's tmux windows, as a menu where the last one stood,
    /// once its node has listed them.
    pub(super) fn window_list(&mut self, i: usize, key: Key, cx: &mut Context<Self>) {
        let Some(session) = self.fleet.session(&key) else {
            return;
        };
        let session_id = session.id.clone();
        let selected = self
            .window_panes()
            .get(i)
            .cloned()
            .flatten()
            .map(|p| p.pane);
        let task = self.host.call(
            NodeId(key.node.clone()),
            SessionListWindows {
                session: session_id,
            },
            cx,
        );
        cx.spawn(async move |this, cx| {
            let items = match task.await.map(|a| a.windows) {
                Ok(windows) if !windows.is_empty() => windows
                    .into_iter()
                    .map(|w| {
                        MenuItem::new(
                            format!("{}: {}", w.index, w.name),
                            Act::WindowPane(i, key.clone(), w.pane),
                        )
                        .checked(Some(w.pane) == selected)
                    })
                    .collect(),
                Ok(_) => vec![MenuItem::note("No tmux windows found")],
                Err(e) => vec![MenuItem::note(format!(
                    "Could not load windows: {}",
                    e.message
                ))],
            };
            let _ = this.update(cx, |s, cx| {
                let rows = items.into_iter().map(|i| i.row(&s.me)).collect();
                s.host.menu(rows, s.menu_at, cx);
            });
        })
        .detach();
    }

    /// The buttons an ended window offers, on a panel.
    fn ended_panel(
        &mut self,
        i: usize,
        note: &str,
        actions: &[(&str, Act)],
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut buttons = div().flex().gap(px(8.)).justify_center();
        for (n, (label, act)) in actions.iter().enumerate() {
            let act = act.clone();
            let kind = if n == 0 { Kind::Primary } else { Kind::Plain };
            buttons = buttons.child(
                kit::button(t, ("detached", i * 10 + n), label.to_string(), kind)
                    .debug_selector(move || format!("ended-{i}-{n}"))
                    .on_click(cx.listener(move |s, _, window, cx| s.act(act.clone(), window, cx))),
            );
        }
        kit::panel(t)
            .px(px(18.))
            .py(px(14.))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(12.))
            .max_w(px(360.))
            .child(kit::note(t, note.to_string(), None).debug_selector({
                let note = note.to_string();
                move || format!("ended-note-{i}-{note}")
            }))
            .child(buttons)
            .into_any_element()
    }

    /// A window without a live terminal: the last frame dimmed behind, and
    /// what to know over it.
    pub(super) fn detached_body(
        &self,
        last: Option<SharedScreen>,
        over: AnyElement,
        t: &Theme,
    ) -> AnyElement {
        let theme_t = t.clone();
        let behind = last.map(|screen| {
            let font = self.settings.font_size;
            div()
                .absolute()
                .top(px(size::TERM_PAD))
                .left(px(size::TERM_PAD))
                .right(px(size::TERM_PAD))
                .bottom(px(size::TERM_PAD))
                .opacity(crate::theme::fade::DIMMED)
                .child(
                    canvas(
                        |_, _, _| (),
                        move |b, _, window, cx| {
                            let s = screen.lock();
                            let m = paint::fit(window, b.size, s.cols, s.rows, font);
                            let o = paint::origin(b, m, s.rows_to_show());
                            window.with_content_mask(Some(ContentMask { bounds: b }), |window| {
                                paint::paint(
                                    &s,
                                    m,
                                    o,
                                    Look {
                                        theme: &theme_t,
                                        cursor: false,
                                        focused: false,
                                        link: &[],
                                    },
                                    window,
                                    cx,
                                );
                            });
                        },
                    )
                    .size_full(),
                )
        });
        div()
            .flex_1()
            .min_h_0()
            .relative()
            .bg(t.term_bg)
            .children(behind)
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(over),
            )
            .into_any_element()
    }

    pub(super) fn term_body(
        &mut self,
        i: usize,
        src: Source,
        focused: bool,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let entity = cx.entity();
        let geo = self.geo.clone();
        // Peek takes what is typed while it is on top; a window while
        // nothing is open over the page.
        let (focus, take_input) = if i == PEEK_SLOT {
            (self.peek_focus.clone(), self.peek_on_top(cx))
        } else {
            (self.term_focus.clone(), focused && !self.host.has_open(cx))
        };
        let font = self.font_of(i);
        // Peek zoomed draws at its own size, never resizing the session:
        // a grid larger than its room is moved under it, following the
        // cursor (`peek_view`).
        let zoomed = i == PEEK_SLOT && self.settings.peek_font.is_some();
        let theme_t = t.clone();
        let composing = self.composing.clone().filter(|_| take_input);
        let hover = self
            .link_hover
            .filter(|h| h.0 == i)
            .map(|(_, col, row)| (col, row));
        let Source {
            screen,
            sizing,
            live,
            top,
        } = src;
        let history = live.is_some() || self.histories.contains_key(&i);
        let place = self.slot_pane(i).map(|at| pictures::Place {
            node: at.node,
            cwd: self.cwd_of(i),
        });
        let chips = self.chips.clone();
        // A window between grids draws at the terminal font, clipped or
        // with room to spare, until tmux has the new grid; one sized by
        // another client shrinks to show its grid whole.
        let settling = sizing.as_ref().is_some_and(|at| {
            let s = screen.lock();
            self.settling(at, (s.cols, s.rows))
        });
        let body = canvas(
            |_, _, _| (),
            move |bounds, _, window, cx| {
                if take_input {
                    window.handle_input(
                        &focus,
                        ElementInputHandler::new(bounds, entity.clone()),
                        cx,
                    );
                }
                if let Some(at) = &sizing {
                    let grid = paint::grid_for(bounds.size, paint::metrics(window, font));
                    entity.update(cx, |sh, cx| sh.want_grid(at, grid, cx));
                }
                let mut s = screen.lock();
                if let Some(top) = top {
                    s.view(top);
                }
                // Reading back on the live screen, the dock is its own rows.
                let same = live.as_ref().is_some_and(|l| Arc::ptr_eq(l, &screen));
                let m = if settling || zoomed {
                    paint::metrics(window, font)
                } else {
                    paint::fit(window, bounds.size, s.cols, s.rows, font)
                };
                // Reading back, the live screen's input rows stay docked
                // below the history, at the same cell size. The history's
                // own copy of them, from when it was read, stays hidden
                // below its view, so it runs on into the live input.
                let mut stale = 0;
                let dock = live.as_ref().map(|l| {
                    let most = (s.rows * 2 / 5).max(3);
                    let (rows, all) = if same {
                        (s.input_rows(most), s.rows)
                    } else {
                        let l = l.lock();
                        (l.input_rows(most), l.rows)
                    };
                    stale = all.saturating_sub(rows.start).min(s.rows - 1);
                    let h = (m.cell.height * rows.len() as f32).min(bounds.size.height / 2.);
                    let top = bounds.bottom() - h;
                    let b =
                        Bounds::new(point(bounds.left(), top), gpui::size(bounds.size.width, h));
                    (
                        b,
                        point(bounds.left(), top - m.cell.height * rows.start as f32),
                    )
                });
                let view = match dock {
                    Some((d, _)) => Bounds::new(
                        bounds.origin,
                        gpui::size(
                            bounds.size.width,
                            d.top() - bounds.top() - px(size::DOCK_GAP),
                        ),
                    ),
                    None => bounds,
                };
                let o = paint::origin(view, m, s.rows_to_show());
                let before = geo.borrow()[i];
                let cursor = s.term.grid().cursor.point;
                let (pan, pan_room, followed) = peek_view(
                    zoomed,
                    view.size.width / m.cell.width,
                    s.cols,
                    (cursor.column.0, cursor.line.0),
                    (before.pan, before.followed),
                );
                let o = point(
                    o.x - m.cell.width * pan as f32,
                    o.y + m.cell.height * stale as f32,
                );
                // Found again at each paint, so output that moves the text
                // under a resting pointer moves the underline with it.
                let link = hover
                    .and_then(|(col, row)| {
                        let line = Line(row as i32) - s.term.grid().display_offset();
                        links::at(&s.term, GridPoint::new(line, Column(col)))
                    })
                    .map(|l| l.cells)
                    .unwrap_or_default();
                window.with_content_mask(Some(ContentMask { bounds: view }), |window| {
                    paint::paint(
                        &s,
                        m,
                        o,
                        Look {
                            theme: &theme_t,
                            cursor: !history,
                            focused,
                            link: &link,
                        },
                        window,
                        cx,
                    );
                    if let Some(text) = composing.as_deref().filter(|_| !history) {
                        paint::composition(&s, m, o, text, &theme_t, window, cx);
                    }
                });
                s.dirty = false;
                // The way back to live sits above the dock, laid out from
                // this geometry: a dock that changed height lays it out again.
                let moved =
                    geo.borrow()[i].dock.map(|d| d.size.height) != dock.map(|d| d.0.size.height);
                geo.borrow_mut()[i] = Geo {
                    body: Some(bounds),
                    origin: Some(o),
                    cell: Some(m.cell),
                    rows: s.rows,
                    cols: s.cols,
                    font: Some(paint::design_px(m.font_size, window)),
                    display_offset: s.term.grid().display_offset(),
                    dock: dock.map(|d| d.0),
                    dock_origin: dock.filter(|_| same).map(|d| d.1),
                    pan,
                    pan_room,
                    followed,
                };
                let drawn = place.as_ref().map(|place| {
                    window.with_content_mask(Some(ContentMask { bounds }), |window| {
                        pictures::paint(
                            &entity,
                            place,
                            &s,
                            pictures::Painted {
                                origin: o,
                                m,
                                view,
                                hover: &link,
                            },
                            &theme_t,
                            window,
                            cx,
                        )
                    })
                });
                chips.borrow_mut()[i] = drawn.unwrap_or_default();
                if let (Some((d, at)), Some(l)) = (dock, live.as_ref()) {
                    let rule = Bounds::new(
                        point(bounds.left(), d.top() - px(size::DOCK_GAP / 2.)),
                        gpui::size(bounds.size.width, px(1.)),
                    );
                    window.paint_quad(gpui::fill(rule, theme_t.divider));
                    // The live rows, also when the view above is the same
                    // screen scrolled back.
                    let mut other = (!same).then(|| l.lock());
                    let l: &mut Screen = match other.as_mut() {
                        Some(l) => l,
                        None => {
                            s.live_view();
                            &mut s
                        }
                    };
                    window.with_content_mask(Some(ContentMask { bounds: d }), |window| {
                        paint::paint(
                            l,
                            m,
                            at,
                            Look {
                                theme: &theme_t,
                                cursor: true,
                                focused,
                                link: &[],
                            },
                            window,
                            cx,
                        );
                        if let Some(text) = composing.as_deref() {
                            paint::composition(l, m, at, text, &theme_t, window, cx);
                        }
                    });
                    l.dirty = false;
                }
                // Outside its paint a screen reads as live to everyone.
                if top.is_some() {
                    s.live_view();
                }
                if moved {
                    let entity = entity.clone();
                    cx.defer(move |cx| entity.update(cx, |_, cx| cx.notify()));
                }
            },
        )
        .size_full();
        // Reading back: a way to the live end, on the rule above the docked
        // input, where it covers no text.
        let hint = history.then(|| {
            let g = self.geo.borrow()[i];
            let above = g.dock.map_or(px(0.), |d| {
                d.size.height + px((size::DOCK_GAP - size::LIVE_PILL_H) / 2.)
            });
            div()
                .id(("live", i))
                .absolute()
                .bottom(above + px(size::TERM_PAD))
                .right(px(12.))
                .h(px(size::LIVE_PILL_H))
                .flex()
                .items_center()
                .px(px(8.))
                .rounded(px(3.))
                .bg(alpha(t.surface, 0.92))
                .border_1()
                .border_color(t.divider)
                .text_size(px(11.))
                .text_color(t.text2)
                .cursor_pointer()
                .hover(|d| d.text_color(t.text))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |sh, _, _, cx| {
                        sh.leave_history(i, cx);
                        cx.stop_propagation();
                    }),
                )
                .child("Back to live \u{2193}")
        });
        div()
            .flex_1()
            .min_h_0()
            .relative()
            .bg(t.term_bg)
            .p(px(size::TERM_PAD))
            .cursor(if hover.is_some() {
                gpui::CursorStyle::PointingHand
            } else {
                gpui::CursorStyle::IBeam
            })
            .child(body)
            .children(hint)
            .into_any_element()
    }

    /// Asks tmux for the grid window `at`'s paint holds: at once the first
    /// time, else once the grid has held for [`RESIZE_SETTLE`].
    fn want_grid(&mut self, at: &PaneAt, grid: (u16, u16), cx: &mut Context<Self>) {
        match self.fleet.claimed(&at.node, at.pane) {
            None => {
                self.resizing.remove(at);
                self.fleet.claim(&at.node, at.pane, grid.0, grid.1);
                self.flush(cx);
                return;
            }
            Some(c) if (c.cols, c.rows) == grid => {
                self.resizing.remove(at);
                return;
            }
            Some(_) => {}
        }
        if self.resizing.get(at).is_some_and(|r| r.0 == grid) {
            return;
        }
        self.resize_seq += 1;
        let seq = self.resize_seq;
        self.resizing.insert(at.clone(), (grid, seq));
        let at = at.clone();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(RESIZE_SETTLE).await;
            let _ = this.update(cx, |sh, cx| {
                if sh.resizing.get(&at).is_some_and(|r| r.1 == seq) {
                    sh.resizing.remove(&at);
                    sh.fleet.claim(&at.node, at.pane, grid.0, grid.1);
                    sh.flush(cx);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Whether a pane this app sizes is between grids: a new grid waits to
    /// be sent, or was sent so recently that tmux may not have it yet.
    fn settling(&self, at: &PaneAt, shown: (usize, usize)) -> bool {
        self.resizing.contains_key(at)
            || self.fleet.claimed(&at.node, at.pane).is_some_and(|c| {
                (c.cols as usize, c.rows as usize) != shown
                    && c.at.elapsed() < crate::remote::RETAKE_EVERY
            })
    }

    // ---- input to the terminal ----

    /// The screen a window shows now: its history, or its live pane.
    /// What window `i` shows now, line by line, as the operator reads it.
    pub fn window_text(&self, i: usize) -> Option<String> {
        let (screen, _) = self.shown_screen(i)?;
        let mut s = screen.lock();
        let Some(h) = self.histories.get(&i) else {
            return Some(s.text());
        };
        s.view(h.top);
        let text = s.text();
        s.live_view();
        Some(text)
    }

    fn shown_screen(&self, i: usize) -> Option<(SharedScreen, Option<PaneAt>)> {
        if let Some(h) = self.histories.get(&i) {
            return Some((h.screen.clone(), None));
        }
        let at = self.slot_pane(i)?;
        Some((self.fleet.screen(&at.node, at.pane)?, Some(at)))
    }

    fn focused_pane(&self, cx: &gpui::App) -> Option<PaneAt> {
        self.slot_pane(self.active_slot(cx))
    }

    /// Bytes for the program of the terminal that has the keyboard (Peek,
    /// or the focused window). A history being read stays open: the
    /// program's input is docked below it.
    pub fn send_input(&mut self, bytes: &[u8], cx: &mut Context<Self>) {
        let Some(pane) = self.focused_pane(cx) else {
            return;
        };
        if let Some(s) = self.fleet.screen(&pane.node, pane.pane) {
            s.lock().term.selection = None;
        }
        self.fleet.input(&pane.node, pane.pane, bytes);
        self.flush(cx);
        if let Some(k) = self.slot_key(self.active_slot(cx)) {
            self.answered(&k, cx);
        }
    }

    /// Terminal `i`'s text size: Peek's or the window's own, else the
    /// Settings size.
    /// The size terminal `i` is drawn at, or, when its grid is set
    /// elsewhere, the most it is drawn at. Peek not zoomed fills its room:
    /// the session's whole grid at the largest size that fits.
    pub(super) fn font_of(&self, i: usize) -> f32 {
        match self.own_font(i) {
            None if i == PEEK_SLOT => size::TERM_MAX,
            own => self.settings.font(own),
        }
    }

    fn own_font(&self, i: usize) -> Option<u8> {
        if i == PEEK_SLOT {
            self.settings.peek_font
        } else {
            self.settings
                .workspace
                .windows()
                .get(i)
                .and_then(|w| w.font)
        }
    }

    /// Zooms terminal `i` by `n` steps, or resets it, and says the size it
    /// is at now. A window sized by this app asks its session for the grid
    /// the new size holds, as a window resized does; Peek never does.
    pub(super) fn zoom_slot(&mut self, i: usize, z: Zoom, n: i32, cx: &mut Context<Self>) {
        if i == PEEK_SLOT {
            return self.zoom_peek(z, n, cx);
        }
        let before = self.font_of(i);
        let Some(own) = self.settings.zoom_font(self.own_font(i), z, n) else {
            return;
        };
        // A grid set elsewhere (Peek's, a window another client sizes)
        // already drawn smaller than asked grows no further.
        let drawn = self.geo.borrow()[i].font;
        let capped = z == Zoom::In && drawn.is_some_and(|d| d < before - 0.25);
        if i == PEEK_SLOT {
            self.settings.peek_font = own;
        } else {
            self.settings.workspace.set_font(i, own);
        }
        self.save(cx);
        let size = self.settings.font(own);
        let reset = if own.is_some() {
            " · Ctrl+0 resets"
        } else {
            ""
        };
        // Drawn smaller than asked (its grid is set elsewhere): the size
        // it is drawn at, which is what the operator sees.
        let shown = drawn.filter(|_| capped).unwrap_or(size);
        let text = format!("Text {shown} px{reset}");
        self.toast(&text, cx);
        cx.notify();
    }

    /// Peek's zoom: from the size it is drawn at, which unzoomed fills its
    /// room with the session's whole grid. Larger than that, the session
    /// keeps its size (Peek never resizes it) and the view follows its
    /// cursor, the wheel with Shift moving it sideways. Ctrl+0 fits the
    /// whole grid again.
    fn zoom_peek(&mut self, z: Zoom, n: i32, cx: &mut Context<Self>) {
        let own = self.settings.peek_font;
        let drawn = self.geo.borrow()[PEEK_SLOT].font;
        let at = own
            .map(f32::from)
            .or(drawn)
            .unwrap_or(self.settings.font_size);
        let to = match z {
            Zoom::Reset if own.is_none() => return,
            Zoom::Reset => None,
            Zoom::In => match zoom::step(at.round(), n, 1., size::TERM_MIN, size::TERM_MAX) {
                Some(v) => Some(v),
                None => return,
            },
            Zoom::Out => match zoom::step(at.round(), -n, 1., size::TERM_MIN, size::TERM_MAX) {
                Some(v) => Some(v),
                None => return,
            },
        };
        self.settings.peek_font = to.map(|v| v.round() as u8);
        {
            let mut geo = self.geo.borrow_mut();
            geo[PEEK_SLOT].pan = 0;
            geo[PEEK_SLOT].followed = None;
        }
        self.save(cx);
        let text = match to {
            Some(v) => format!("Text {v} px · Ctrl+0 fits the whole session"),
            None => "Text fits the whole session".to_string(),
        };
        self.toast(&text, cx);
        cx.notify();
    }

    fn paste_clipboard(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) else {
            return;
        };
        let Some(pane) = self.focused_pane(cx) else {
            return;
        };
        let bracketed = self
            .fleet
            .screen(&pane.node, pane.pane)
            .is_some_and(|s| s.lock().mode().contains(TermMode::BRACKETED_PASTE));
        self.send_input(&keys::paste(&text, bracketed), cx);
    }

    fn copy_selection(&mut self, i: usize, cx: &mut Context<Self>) -> bool {
        let Some((screen, _)) = self.shown_screen(i) else {
            return false;
        };
        let text = screen.lock().term.selection_to_string();
        match text {
            Some(t) if !t.is_empty() => {
                self.copy_text(t, cx);
                true
            }
            _ => false,
        }
    }

    /// Puts a selection on the clipboard and says whether it got there. The
    /// platform's write reports nothing (Windows fails when another program
    /// holds the clipboard open), so the clipboard is read back.
    fn copy_text(&mut self, text: String, cx: &mut Context<Self>) {
        let text = keys::copied(text);
        cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
        let landed = cx.read_from_clipboard().and_then(|c| c.text());
        if landed.as_deref() == Some(text.as_str()) {
            self.toast("Copied", cx);
        } else {
            self.report(
                "Could not copy the selection: the clipboard did not take it",
                true,
                None,
            );
            cx.notify();
        }
    }

    pub fn term_key(&mut self, e: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let k = &e.keystroke;
        let m = &k.modifiers;
        let w = self.active_slot(cx);
        // Escape is the program's in Peek; Shift+Escape, which closes it,
        // is the app's (`chords::route`) wherever focus is.
        if m.control && m.shift && k.key == "c" {
            self.copy_selection(w, cx);
            cx.stop_propagation();
            return;
        }
        if (m.control && !m.alt && k.key == "v") || (m.shift && !m.control && k.key == "insert") {
            self.paste_clipboard(cx);
            cx.stop_propagation();
            return;
        }
        // Ctrl+=, Ctrl+- and Ctrl+0 zoom this terminal, as browsers do.
        // At the end of the range the key is still the zoom's. A window
        // with no session (its launcher) has no text of its own: the key
        // goes on to the app's zoom.
        if let Some(z) = zoom::from_key(k)
            && (w == PEEK_SLOT || self.settings.workspace.shown(w).is_some())
        {
            self.zoom_slot(w, z, 1, cx);
            cx.stop_propagation();
            return;
        }
        if m.shift && !m.control && k.key == "end" && self.histories.contains_key(&w) {
            self.leave_history(w, cx);
            cx.stop_propagation();
            return;
        }
        // Shift+PageUp/PageDown read the history, as terminals do.
        if m.shift && !m.control && (k.key == "pageup" || k.key == "pagedown") {
            self.scroll_window(w, if k.key == "pageup" { 20 } else { -20 }, cx);
            cx.stop_propagation();
            return;
        }
        // A window with no session (its launcher) has no program to type
        // into: its keys go on, so Ctrl+B there reaches the app.
        if e.prefer_character_input || self.focused_pane(cx).is_none() {
            return;
        }
        let app_cursor = self
            .shown_screen(w)
            .is_some_and(|(s, _)| s.lock().mode().contains(TermMode::APP_CURSOR));
        if let Some(b) = keys::bytes(k, app_cursor) {
            self.send_input(&b, cx);
            cx.stop_propagation();
        }
    }

    /// Ctrl+wheel over a terminal's body zooms it, not the app.
    pub(super) fn zoom_claim(cx: &mut Context<Self>) -> impl IntoElement {
        let me = cx.weak_entity();
        zoom::claim(move |p, cx| {
            me.upgrade()
                .is_some_and(|me| me.read(cx).window_at(p).is_some())
        })
    }

    /// The terminal under the pointer: Peek's while it is open over the
    /// windows, else a window's.
    fn window_at(&self, p: gpui::Point<Pixels>) -> Option<usize> {
        let geo = self.geo.borrow();
        if self.peek.is_some() {
            return geo[PEEK_SLOT]
                .body
                .is_some_and(|b| b.contains(&p))
                .then_some(PEEK_SLOT);
        }
        geo.iter()
            .position(|g| g.body.is_some_and(|b| b.contains(&p)))
            .filter(|i| *i < self.settings.workspace.windows().len())
    }

    fn cell_at(&self, i: usize, p: gpui::Point<Pixels>) -> Option<(usize, usize, Side)> {
        let g = self.geo.borrow()[i];
        let (o, cell) = (g.origin?, g.cell?);
        let x = f32::from(p.x - o.x) / f32::from(cell.width);
        let y = f32::from(p.y - o.y) / f32::from(cell.height);
        let col = (x.max(0.) as usize).min(g.cols.saturating_sub(1));
        let row = (y.max(0.) as usize).min(g.rows.saturating_sub(1));
        Some((
            col,
            row,
            if x.fract() < 0.5 {
                Side::Left
            } else {
                Side::Right
            },
        ))
    }

    fn grid_point(&self, i: usize, p: gpui::Point<Pixels>) -> Option<(GridPoint, Side)> {
        let (col, row, side) = self.cell_at(i, p)?;
        let offset = self.geo.borrow()[i].display_offset;
        Some((GridPoint::new(Line(row as i32) - offset, Column(col)), side))
    }

    /// The program asked for the mouse, and Shift isn't overriding it.
    fn mouse_mode(&self, i: usize, shift: bool) -> Option<(TermMode, PaneAt)> {
        if shift || self.histories.contains_key(&i) {
            return None;
        }
        let (screen, pane) = self.shown_screen(i)?;
        let mode = screen.lock().mode();
        mode.intersects(TermMode::MOUSE_MODE)
            .then_some((mode, pane?))
    }

    pub(super) fn tile_mouse_down(
        &mut self,
        i: usize,
        e: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let peek = i == PEEK_SLOT;
        if !peek && self.settings.workspace.focus != i {
            self.settings.workspace.focus = i;
            self.publish_sessions(cx);
            self.save(cx);
            cx.notify();
        }
        let in_body = self.geo.borrow()[i]
            .body
            .is_some_and(|b| b.contains(&e.position));
        let Some(at) = self.slot_pane(i).filter(|_| in_body) else {
            // A window with no live terminal: a right click offers its
            // session's menu, or the layout's.
            if !peek && e.button == MouseButton::Right {
                let ws = &self.settings.workspace;
                let items = match ws.shown(i).map(|b| b.key.clone()) {
                    Some(key) => self.session_menu(i, &key),
                    None => super::views::layout_rows(ws, i),
                };
                self.menu(items, e.position, window, cx);
                cx.stop_propagation();
            }
            return;
        };
        if peek {
            window.focus(&self.peek_focus, cx);
        } else {
            // A click is using the session here: it takes the window back
            // from another client that sized it since. Peek sizes nothing.
            self.fleet.retake(&at.node, at.pane);
            self.flush(cx);
            window.focus(&self.term_focus, cx);
        }
        let button = match e.button {
            MouseButton::Left => mouse::Button::Left,
            MouseButton::Middle => mouse::Button::Middle,
            MouseButton::Right => mouse::Button::Right,
            _ => return,
        };
        if let Some((mode, pane)) = self.mouse_mode(i, e.modifiers.shift)
            && let Some((col, row, _)) = self.cell_at(i, e.position)
        {
            let mods = (e.modifiers.shift, e.modifiers.alt, e.modifiers.control);
            self.fleet.input(
                &pane.node,
                pane.pane,
                &mouse::report(mode, button, true, false, col, row, mods),
            );
            self.flush(cx);
            self.mouse_down_in = Some((i, button as u8));
            cx.stop_propagation();
            return;
        }
        // The docked input is the program's: a press there only focuses.
        if self.geo.borrow()[i]
            .dock
            .is_some_and(|d| d.contains(&e.position))
            && e.button == MouseButton::Left
        {
            return;
        }
        match e.button {
            MouseButton::Left => {
                let Some((p, side)) = self.grid_point(i, e.position) else {
                    return;
                };
                let ty = match e.click_count {
                    2 => SelectionType::Semantic,
                    3 => SelectionType::Lines,
                    _ => SelectionType::Simple,
                };
                if let Some((screen, _)) = self.shown_screen(i) {
                    screen.lock().term.selection = Some(Selection::new(ty, p, side));
                }
                self.selecting = Some(i);
                cx.notify();
            }
            MouseButton::Middle => self.paste_clipboard(cx),
            // A right click copies a selection, else offers the menu.
            MouseButton::Right if !self.copy_selection(i, cx) => {
                let items = if peek {
                    self.peek_menu()
                } else {
                    match (
                        self.settings.workspace.shown(i).cloned(),
                        self.window_panes()[i].clone(),
                    ) {
                        (Some(b), Some(at)) => self.tile_menu(i, &b.key, at.pane),
                        _ => vec![],
                    }
                };
                if !items.is_empty() {
                    self.menu(items, e.position, window, cx);
                }
            }
            _ => {}
        }
    }

    pub(super) fn term_mouse_move(
        &mut self,
        e: &MouseMoveEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A press from a window is followed everywhere (`term_drag`).
        if self.selecting.is_some() || self.mouse_down_in.is_some() {
            return;
        }
        let Some(i) = self.window_at(e.position) else {
            self.hover_link(None, cx);
            return;
        };
        let held = e.pressed_button.is_some();
        let program = self.mouse_mode(i, e.modifiers.shift);
        // A link shows where the mouse is the reader's: no button held, and
        // the program not listening to it (or Shift overriding it).
        let link = (program.is_none() && !held)
            .then(|| self.link_at(i, e.position))
            .flatten()
            .map(|(col, row, _)| (i, col, row));
        self.hover_link(link, cx);
        if let Some((mode, pane)) = program
            && mouse::wants(mode, true, held)
            && let Some((col, row, _)) = self.cell_at(i, e.position)
        {
            let button = match e.pressed_button {
                Some(MouseButton::Left) => mouse::Button::Left,
                Some(MouseButton::Middle) => mouse::Button::Middle,
                Some(MouseButton::Right) => mouse::Button::Right,
                _ => mouse::Button::None,
            };
            let mods = (e.modifiers.shift, e.modifiers.alt, e.modifiers.control);
            self.fleet.input(
                &pane.node,
                pane.pane,
                &mouse::report(mode, button, true, true, col, row, mods),
            );
            self.flush(cx);
        }
    }

    fn hover_link(&mut self, at: Option<(usize, usize, usize)>, cx: &mut Context<Self>) {
        if self.link_hover != at {
            self.link_hover = at;
            cx.notify();
        }
    }

    /// The link drawn where the pointer is in window `i`: its cell on
    /// screen, and the link.
    fn link_at(&self, i: usize, p: gpui::Point<Pixels>) -> Option<(usize, usize, Link)> {
        let g = self.geo.borrow()[i];
        // A small picture stands for the name it was drawn for.
        let chip = self.chips.borrow()[i]
            .iter()
            .find(|c| c.bounds.contains(&p))
            .map(|c| c.link.clone());
        if let Some(link) = chip {
            let first = *link.cells.first()?;
            let row = first.line.0 + g.display_offset as i32;
            return Some((first.column.0, row.max(0) as usize, link));
        }
        let (o, cell) = (g.origin?, g.cell?);
        let grid = Bounds::new(
            o,
            gpui::size(cell.width * g.cols as f32, cell.height * g.rows as f32),
        );
        if !g.body.is_some_and(|b| b.contains(&p))
            || !grid.contains(&p)
            || g.dock.is_some_and(|d| d.contains(&p))
        {
            return None;
        }
        let (col, row, _) = self.cell_at(i, p)?;
        let (screen, _) = self.shown_screen(i)?;
        let s = screen.lock();
        let line = Line(row as i32) - g.display_offset;
        links::at(&s.term, GridPoint::new(line, Column(col))).map(|l| (col, row, l))
    }

    /// A link was clicked: an address opens in the browser, a path in the
    /// Files panel, on the device of the window's session, at the
    /// line named after it, and a diagram drawn beside. A relative path is
    /// the folder's of the pane the window shows, which tmux is asked for,
    /// as the session's own folder is its active pane's.
    fn open_link(&mut self, i: usize, target: links::Target, cx: &mut Context<Self>) {
        let pane = match self.histories.get(&i) {
            Some(h) => Some(h.pane.clone()),
            None => self.slot_pane(i),
        };
        let ask = match target {
            links::Target::Url(url) => return cx.open_url(&url),
            links::Target::Diagram(source) => return self.open_diagram(source, cx),
            links::Target::WorkItem(id) => {
                let Some(pane) = pane else { return };
                Ask::Reveal {
                    what: "item".into(),
                    id,
                    node: Some(NodeId(pane.node)),
                    part: None,
                    beside: true,
                }
            }
            links::Target::Path(path, line) => {
                let Some(pane) = pane else { return };
                let session = self.fleet.session_of_pane(&pane.node, pane.pane);
                let cwd = session.and_then(|s| s.cwd.clone());
                let id = session.map(|s| s.id.clone()).unwrap_or_default();
                if path.starts_with(['/', '~']) || id.is_empty() {
                    let path = links::resolve(cwd.as_deref(), &path);
                    open_path(NodeId(pane.node), path, line)
                } else {
                    let node = NodeId(pane.node.clone());
                    let task = self
                        .host
                        .call(node.clone(), SessionListWindows { session: id }, cx);
                    cx.spawn(async move |this, cx| {
                        let folder = task
                            .await
                            .ok()
                            .map(|a| a.windows)
                            .and_then(|w| {
                                w.into_iter().find(|w| w.pane == pane.pane).map(|w| w.cwd)
                            })
                            .or(cwd);
                        let path = links::resolve(folder.as_deref(), &path);
                        let _ =
                            this.update(cx, |s, cx| s.ask_link(open_path(node, path, line), cx));
                    })
                    .detach();
                    return;
                }
            }
        };
        self.ask_link(ask, cx);
    }

    fn ask_link(&mut self, ask: Ask, cx: &mut Context<Self>) {
        if let Err(e) = self.host.ask(ask, cx) {
            self.report(&e.message, true, None);
        }
    }

    /// The folder of the session window `i` shows, which relative paths
    /// its output names are relative to.
    fn cwd_of(&self, i: usize) -> Option<String> {
        let session = match self.histories.get(&i) {
            Some(h) => self.fleet.session_of_pane(&h.pane.node, h.pane.pane),
            None => self.fleet.session(&self.slot_key(i)?),
        };
        session?.cwd.clone()
    }

    pub(super) fn term_mouse_up(
        &mut self,
        e: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.divider.take().is_some() {
            self.save(cx);
            cx.notify();
            return;
        }
        if let Some((i, b)) = self.mouse_down_in.take() {
            if let Some((mode, pane)) = self.mouse_mode(i, false)
                && let Some((col, row, _)) = self.cell_at(i, e.position)
            {
                let button = match b {
                    0 => mouse::Button::Left,
                    1 => mouse::Button::Middle,
                    _ => mouse::Button::Right,
                };
                let mods = (e.modifiers.shift, e.modifiers.alt, e.modifiers.control);
                self.fleet.input(
                    &pane.node,
                    pane.pane,
                    &mouse::report(mode, button, false, false, col, row, mods),
                );
                self.flush(cx);
            }
            return;
        }
        let Some(i) = self.selecting.take() else {
            return;
        };
        self.end_drag();
        let Some((screen, _)) = self.shown_screen(i) else {
            return;
        };
        let text = screen.lock().term.selection_to_string();
        match text {
            Some(t) if !t.is_empty() => {
                if self.settings.copy_on_select {
                    self.copy_text(t, cx);
                }
            }
            // A plain click selects nothing, copies nothing, and opens the
            // link it lands on.
            _ => {
                screen.lock().term.selection = None;
                if let Some((_, _, link)) = self.link_at(i, e.position) {
                    self.open_link(i, link.target, cx);
                }
            }
        }
        cx.notify();
    }

    pub(super) fn term_scroll(&mut self, e: &ScrollEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(i) = self.window_at(e.position) else {
            return;
        };
        // Ctrl+wheel zooms the terminal under the pointer, never reaching
        // its program.
        if let Some(n) = self.zoom_wheel.steps(e) {
            let z = if n > 0 { Zoom::In } else { Zoom::Out };
            if n != 0 {
                self.zoom_slot(i, z, n.abs(), cx);
            }
            cx.stop_propagation();
            return;
        }
        // Peek zoomed past its room: Shift+wheel, or a sideways wheel,
        // moves the view along the row.
        let g = self.geo.borrow()[i];
        if g.pan_room > 0 {
            let cell_w = g.cell.map(|c| c.width).unwrap_or(px(8.));
            let (dx, dy) = match e.delta {
                ScrollDelta::Lines(l) => (l.x * 3., l.y * 3.),
                ScrollDelta::Pixels(p) => (
                    f32::from(p.x) / f32::from(cell_w),
                    f32::from(p.y) / f32::from(cell_w),
                ),
            };
            // Linux turns Shift+wheel into a sideways wheel itself.
            let along = if dx != 0. && dx.abs() >= dy.abs() {
                dx
            } else if e.modifiers.shift {
                dy
            } else {
                0.
            };
            if along != 0. {
                let to = (g.pan as f32 - along * 2.)
                    .round()
                    .clamp(0., g.pan_room as f32);
                self.geo.borrow_mut()[i].pan = to as usize;
                cx.stop_propagation();
                cx.notify();
                return;
            }
        }
        let cell_h = g.cell.map(|c| c.height).unwrap_or(px(16.));
        let lines = match e.delta {
            ScrollDelta::Lines(l) => l.y * 3.,
            ScrollDelta::Pixels(p) => f32::from(p.y) / f32::from(cell_h),
        };
        let rest = match self.scroll_rest {
            (w, r) if w == i => r,
            _ => 0.,
        };
        let (n, rest) = mouse::whole_lines(rest, lines);
        self.scroll_rest = (i, rest);
        if n == 0 {
            return;
        }
        // The program's own wheel first, then the alternate screen's arrows.
        if let Some((mode, pane)) = self.mouse_mode(i, e.modifiers.shift)
            && let Some((col, row, _)) = self.cell_at(i, e.position)
        {
            let b = if n > 0 {
                mouse::Button::WheelUp
            } else {
                mouse::Button::WheelDown
            };
            let mods = (e.modifiers.shift, e.modifiers.alt, e.modifiers.control);
            let r = mouse::report(mode, b, true, false, col, row, mods);
            for _ in 0..n.unsigned_abs() {
                self.fleet.input(&pane.node, pane.pane, &r);
                self.flush(cx);
            }
            return;
        }
        if !self.histories.contains_key(&i)
            && let Some((screen, Some(pane))) = self.shown_screen(i)
        {
            let mode = screen.lock().mode();
            if mode.contains(TermMode::ALT_SCREEN) && mode.contains(TermMode::ALTERNATE_SCROLL) {
                let app = mode.contains(TermMode::APP_CURSOR);
                let key = match (n > 0, app) {
                    (true, true) => b"\x1bOA".as_slice(),
                    (true, false) => b"\x1b[A",
                    (false, true) => b"\x1bOB",
                    (false, false) => b"\x1b[B",
                };
                for _ in 0..n.unsigned_abs() {
                    self.fleet.input(&pane.node, pane.pane, key);
                    self.flush(cx);
                }
                return;
            }
        }
        self.scroll_window(i, n, cx);
    }

    /// A window reading back on a live screen that lost its grown lines
    /// shows it live again; a live screen nobody reads back drops them.
    fn tidy_histories(&mut self) {
        self.histories.retain(|_, h| {
            let grown = self.grown.iter().any(|g| Arc::ptr_eq(g, &h.screen));
            !grown || h.screen.lock().has_history()
        });
        let histories = &self.histories;
        self.grown.retain(|g| {
            let read = histories.values().any(|h| Arc::ptr_eq(g, &h.screen));
            if !read {
                g.lock().drop_history();
            }
            read
        });
    }

    /// Shows window `i`'s live pane again in place of its history.
    pub fn leave_history(&mut self, i: usize, cx: &mut Context<Self>) {
        if self.histories.remove(&i).is_some() {
            if self.selecting == Some(i) {
                self.selecting = None;
                self.end_drag();
            }
            cx.notify();
        }
    }

    /// Scrolls a window's history: opens it on the way up, leaves it at the
    /// bottom on the way down.
    fn scroll_window(&mut self, i: usize, n: i32, cx: &mut Context<Self>) {
        match self.histories.get(&i) {
            Some(h) => {
                let top = h.screen.lock().scroll_view(h.top, n);
                match top {
                    Some(top) => {
                        if let Some(h) = self.histories.get_mut(&i) {
                            h.top = top;
                        }
                    }
                    None if n < 0 => self.leave_history(i, cx),
                    None => {}
                }
                cx.notify();
            }
            None if n > 0 => self.open_history(i, cx),
            None => {}
        }
    }
}

/// The terminal's input method: committed text goes to the focused pane.
impl EntityInputHandler for Terminals {
    fn text_for_range(
        &mut self,
        _: Range<usize>,
        _: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        None
    }
    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: 0..0,
            reversed: false,
        })
    }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        let n = self.composing.as_ref()?.encode_utf16().count();
        Some(0..n)
    }
    /// The composition ends unsent: only committed text reaches the pane.
    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if self.composing.take().is_some() {
            cx.notify();
        }
    }
    /// Committed text replaces what was composed. An empty commit is a
    /// composition given up, and sends nothing.
    fn replace_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.composing.take().is_some() {
            cx.notify();
        }
        if !text.is_empty() {
            self.send_input(text.as_bytes(), cx);
        }
    }
    /// A dead key's accent or a word being composed shows at the cursor
    /// until it is committed.
    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.composing = (!text.is_empty()).then(|| text.to_string());
        cx.notify();
    }
    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        // The candidate window opens at the focused terminal's cursor.
        let w = self.active_slot(cx);
        let g = self.geo.borrow()[w];
        let (o, cell) = (g.origin?, g.cell?);
        let (screen, _) = self.shown_screen(w)?;
        let c = screen.lock().term.grid().cursor.point;
        Some(Bounds::new(
            point(
                o.x + cell.width * c.column.0 as f32,
                o.y + cell.height * c.line.0 as f32,
            ),
            cell,
        ))
    }
    fn character_index_for_point(
        &mut self,
        _: gpui::Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}

/// Peek's view along a row: the columns it is moved by, the most it can be
/// moved, and the cursor it followed. Unzoomed, or with the grid in its
/// room, it stays put. Zoomed past its room it holds where the wheel left
/// it until the cursor moves, then brings the cursor into view with a few
/// columns to spare.
fn peek_view(
    zoomed: bool,
    room: f32,
    cols: usize,
    cursor: (usize, i32),
    (pan, followed): (usize, Option<(usize, i32)>),
) -> (usize, usize, Option<(usize, i32)>) {
    let shown = room.floor().max(1.) as usize;
    let most = cols.saturating_sub(shown);
    if !zoomed || most == 0 {
        return (0, 0, None);
    }
    const SPARE: usize = 4;
    let mut pan = pan.min(most);
    if followed != Some(cursor) {
        let c = cursor.0;
        if c < pan + SPARE.min(shown / 2) {
            pan = c.saturating_sub(SPARE);
        } else if c + SPARE >= pan + shown {
            pan = (c + SPARE + 1).saturating_sub(shown);
        }
        pan = pan.min(most);
    }
    (pan, most, Some(cursor))
}

/// Opens a file in the Files panel beside the page, at `line` if one is
/// named: as Files reveals a path at a part of it.
fn open_path(node: NodeId, path: String, line: Option<u32>) -> Ask {
    match line {
        None => Ask::OpenPath {
            node,
            path,
            beside: true,
        },
        Some(line) => Ask::Reveal {
            what: "path".into(),
            id: path,
            node: Some(node),
            part: Some(line.to_string()),
            beside: true,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(name: &str) -> Key {
        Key {
            node: "n".into(),
            user: String::new(),
            name: name.into(),
        }
    }

    #[test]
    fn a_drop_lands_only_where_it_would_change_something() {
        // a alone in 0, b and c in 1, 2 empty.
        let mut ws = Workspace::default();
        ws.set_windows(0, 3);
        ws.bind(0, key("a"), None);
        ws.bind(1, key("b"), None);
        ws.bind(1, key("c"), None);
        let at =
            |k: &str, from: Option<usize>, i: usize, z: Zone| landing(&ws, &key(k), from, i, z);
        let right = Zone::Edge(Edge::Right);
        // A tag on its own window: only an edge, and only with company.
        assert_eq!(at("a", Some(0), 0, Zone::Center), None);
        assert_eq!(at("a", Some(0), 0, right), None);
        assert_eq!(
            at("b", Some(1), 1, right),
            Some(Landing::Split(1, Edge::Right))
        );
        // A session alone in its window takes the window with it.
        assert_eq!(
            at("a", Some(0), 1, right),
            Some(Landing::Move(0, 1, Edge::Right))
        );
        assert_eq!(
            at("a", None, 1, right),
            Some(Landing::Move(0, 1, Edge::Right)),
            "from the rail too"
        );
        assert_eq!(at("a", Some(0), 1, Zone::Center), Some(Landing::Into(1)));
        // An empty window is filled, whatever the zone.
        assert_eq!(at("b", Some(1), 2, right), Some(Landing::Into(2)));
        // A session on no window splits beside one.
        assert_eq!(
            at("d", None, 0, Zone::Edge(Edge::Top)),
            Some(Landing::Split(0, Edge::Top))
        );
        assert_eq!(at("d", None, 9, Zone::Center), None);
        // With every window shown, an edge adds a tag.
        ws.set_windows(0, MAX_WINDOWS);
        let full = landing(&ws, &key("d"), None, 0, right);
        assert_eq!(full, Some(Landing::Into(0)));
    }

    #[test]
    fn the_zone_is_the_nearest_edge_within_a_quarter() {
        let b = Bounds::new(point(px(100.), px(100.)), gpui::size(px(400.), px(200.)));
        assert_eq!(zone(b, point(px(300.), px(200.))), Zone::Center);
        assert_eq!(zone(b, point(px(120.), px(200.))), Zone::Edge(Edge::Left));
        assert_eq!(zone(b, point(px(480.), px(150.))), Zone::Edge(Edge::Right));
        assert_eq!(zone(b, point(px(300.), px(110.))), Zone::Edge(Edge::Top));
        assert_eq!(zone(b, point(px(300.), px(290.))), Zone::Edge(Edge::Bottom));
    }

    /// Peek zoomed past its room follows the cursor along the row, and a
    /// pan by the wheel holds until the cursor moves.
    #[test]
    fn peek_zoomed_follows_the_cursor_and_keeps_a_pan_while_it_rests() {
        // Unzoomed, or the whole row in its room: no pan.
        assert_eq!(
            peek_view(false, 50., 200, (150, 3), (0, None)),
            (0, 0, None)
        );
        assert_eq!(peek_view(true, 120., 100, (90, 3), (0, None)).0, 0);
        // The cursor past the right edge is brought into view with spare.
        let (pan, most, followed) = peek_view(true, 50., 200, (120, 3), (0, None));
        assert_eq!(most, 150);
        assert!(pan <= 120 && 120 + 4 < pan + 50, "{pan}");
        // Panned away by the wheel while the cursor rests: it holds.
        assert_eq!(peek_view(true, 50., 200, (120, 3), (10, followed)).0, 10);
        // The cursor moves: the view follows it again.
        let (pan, ..) = peek_view(true, 50., 200, (121, 3), (10, followed));
        assert!(pan <= 121 && 121 + 4 < pan + 50, "{pan}");
        // Back at the start of the line, the view goes back to the start.
        assert_eq!(peek_view(true, 50., 200, (0, 4), (pan, followed)).0, 0);
    }
}
