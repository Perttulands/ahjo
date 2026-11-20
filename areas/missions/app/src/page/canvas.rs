//! Mission canvas interaction, adapted from the original Missions page.
use super::*;
use ahjo_app_kit::widgets::boundary;
use ahjo_missions_contract::{LayoutNode, Mission};
use gpui::{
    AnyElement, Bounds, Hsla, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    PathBuilder, Pixels, Point, ScrollEvent, SharedString, Stateful, canvas, point,
};
use std::{cell::Cell, rc::Rc};

/// Far enough out to see a long mission whole.
const ZOOM_MIN: f32 = 0.15;
const ZOOM_MAX: f32 = 1.6;
const ZOOM_STEP: f32 = 1.2;
const FIT_MARGIN: f32 = 80.;
/// The least zoom a card's words read at. A board that fits whole only
/// smaller opens at this zoom on its start; Fit still shows it whole.
const READABLE: f32 = 0.85;
/// The least zoom the board beside Flow is drawn at: its titles still
/// read, its small words only just. A mission it cannot show whole at this
/// size runs down past the pane and scrolls; Fit still shows it whole.
pub(super) const BESIDE_FLOOR: f32 = 0.6;
/// The strip on the right the zoom controls stand in; new cards keep out.
const DRAG_SLOP: f32 = 3.;
const CURVE_REACH: f32 = 40.;
const CURVE_DOWN: f32 = 60.;
const DASH_SHORT: f32 = 3.;
const DASH_LONG: f32 = 5.;
const TITLE_SIZE: f32 = 18.;
/// A card's words at world size: at the zoom a board opens at, never below
/// 0.85, its small words read at the page's small size and its text at the
/// page's text size.
pub(super) const CARD_SMALL: f32 = 13.;
pub(super) const CARD_TEXT: f32 = 15.;
const RING_SIZE: f32 = 30.;
const GAP: f32 = 8.;
/// A finding's mark beside a control's size.
const BADGE_SCALE: f32 = 0.75;
const SLOT_PAD: f32 = 10.;
const POOL_LABEL: f32 = 14.;
const CHIP_PAD: f32 = 6.;
const GATE_MARK: f32 = 46.;
const END_MARK: f32 = 22.;
const CHIP_ALPHA: f32 = 0.14;
#[derive(Clone, Debug)]
enum Drag {
    Pan(Point<Pixels>, (f32, f32)),
    Card {
        id: String,
        at: Point<Pixels>,
        from: (f32, f32),
        now: (f32, f32),
        moved: bool,
    },
    Wire {
        from: String,
        start: (f32, f32),
        lifted: Option<String>,
        now: (f32, f32),
    },
}
#[derive(Clone, Debug)]
pub(super) struct View {
    zoom: f32,
    pan: (f32, f32),
    drag: Option<Drag>,
    menu: Option<(f32, f32)>,
    bounds: Rc<Cell<(f32, f32, f32, f32)>>,
    fitted: bool,
    /// Panned or zoomed by hand: the board is framed again only by Fit.
    touched: bool,
    /// The card being edited, kept in view until the view is moved by hand.
    follow: Option<String>,
    /// Ctrl+wheel as zoom steps, a touchpad's pixels adding up to one.
    wheel: ahjo_app_kit::zoom::Wheel,
}
impl Default for View {
    fn default() -> Self {
        Self {
            zoom: 1.,
            pan: (PAD, PAD),
            drag: None,
            menu: None,
            bounds: Rc::new(Cell::new((0., 0., 0., 0.))),
            fitted: false,
            touched: false,
            follow: None,
            wheel: Default::default(),
        }
    }
}
impl View {
    pub(super) fn position(&self, x: f32, y: f32) -> (f32, f32) {
        (x * self.zoom + self.pan.0, y * self.zoom + self.pan.1)
    }
    pub(super) fn zoom(&self) -> f32 {
        self.zoom
    }
    pub(super) fn follow(&mut self, id: Option<String>) {
        self.follow = id;
    }
    pub(super) fn invalidate_fit(&mut self) {
        self.fitted = false;
    }
    /// Back to the view the board opened at (Ctrl+0): framed again on the
    /// next draw, as a board just opened is.
    pub(super) fn reopen(&mut self) {
        self.follow = None;
        self.touched = false;
        self.fitted = false;
    }
    /// Zoom a step in or out about the middle of the view (Ctrl+= and
    /// Ctrl+-, and the + and − buttons).
    pub(super) fn step(&mut self, inward: bool) {
        let center = self.center();
        self.zoom_at(if inward { ZOOM_STEP } else { 1. / ZOOM_STEP }, center);
    }
    pub(super) fn close_menu(&mut self) -> bool {
        self.menu.take().is_some()
    }
    fn zoom_at(&mut self, factor: f32, at: (f32, f32)) {
        self.follow = None;
        let z = (self.zoom * factor).clamp(ZOOM_MIN, ZOOM_MAX);
        let k = z / self.zoom;
        self.pan = (
            at.0 - (at.0 - self.pan.0) * k,
            at.1 - (at.1 - self.pan.1) * k,
        );
        self.zoom = z;
        self.fitted = true;
        self.touched = true;
    }
    fn world(&self, p: Point<Pixels>) -> (f32, f32) {
        let (x, y, _, _) = self.bounds.get();
        (
            (f32::from(p.x) - x - self.pan.0) / self.zoom,
            (f32::from(p.y) - y - self.pan.1) / self.zoom,
        )
    }
    /// The part of the board in view as x, y, width, height on the board;
    /// None before the canvas is first laid out. The zoom controls float
    /// over its corner and take no width from it.
    pub(super) fn visible(&self) -> Option<(f32, f32, f32, f32)> {
        let (_, _, w, h) = self.bounds.get();
        (w > 0. && h > 0.).then(|| {
            (
                -self.pan.0 / self.zoom,
                -self.pan.1 / self.zoom,
                w / self.zoom,
                h / self.zoom,
            )
        })
    }
    /// Bring a place on the board (x, y, width, height) into view, moving
    /// the board no further than needed so the rest stays where it was.
    pub(super) fn show(&mut self, (x, y, w, h): (f32, f32, f32, f32)) {
        let Some((vx, vy, vw, vh)) = self.visible() else {
            return;
        };
        if x >= vx && y >= vy && x + w <= vx + vw && y + h <= vy + vh {
            return;
        }
        let margin = FIT_MARGIN / 2. / self.zoom;
        let shift = |at: f32, len: f32, view: f32, span: f32| {
            if at < view || len + 2. * margin > span {
                view + margin - at
            } else if at + len > view + span {
                view + span - margin - (at + len)
            } else {
                0.
            }
        };
        self.pan.0 += shift(x, w, vx, vw) * self.zoom;
        self.pan.1 += shift(y, h, vy, vh) * self.zoom;
        // A framed board is framed again; this only keeps the card in view
        // meanwhile.
        if self.touched {
            self.fitted = true;
        }
    }
    fn center(&self) -> (f32, f32) {
        let (_, _, w, h) = self.bounds.get();
        (w / 2., h / 2.)
    }
    /// Fit the whole board in view. The view is the operator's again, so it
    /// stops following an edited card.
    fn fit(&mut self, board: &board::Board) {
        let (_, _, w, h) = self.bounds.get();
        if w <= 0. || h <= 0. {
            return;
        }
        let (x, y, bw, bh) = board::extent(board);
        let z = ((w - FIT_MARGIN) / bw)
            .min((h - FIT_MARGIN) / bh)
            .clamp(ZOOM_MIN, 1.);
        self.zoom = z;
        // Centred when it fits; a board too big even at the least zoom shows
        // its start, where the Input is, rather than cutting it off.
        let place = |room: f32, size: f32, at: f32| {
            if size * z <= room {
                (room - size * z) / 2. - at * z
            } else {
                FIT_MARGIN / 2. - at * z
            }
        };
        self.pan = (place(w, bw, x), place(h, bh, y));
        // An empty board fits again once its first card lands.
        self.fitted = !board.cards.is_empty();
    }
    /// The view a board opens at: whole when its words read at that size,
    /// otherwise at `floor` on its start, where the Input is, so a long
    /// mission or a narrow window opens readable rather than tiny. With
    /// `across`, a board too long to show whole is drawn as large as its
    /// width allows, at `floor` or more, and runs down past the view.
    fn open(&mut self, board: &board::Board, floor: f32, across: bool) {
        self.fit(board);
        if self.zoom >= floor || board.cards.is_empty() {
            return;
        }
        let (_, _, w, h) = self.bounds.get();
        let (x, y, bw, bh) = board::extent(board);
        let floor = if across {
            ((w - FIT_MARGIN) / bw).clamp(floor, 1.)
        } else {
            floor
        };
        self.zoom = floor;
        // Centred each way it fits.
        let place = |room: f32, size: f32, at: f32| {
            if size * floor <= room - FIT_MARGIN {
                (room - size * floor) / 2. - at * floor
            } else {
                FIT_MARGIN / 2. - at * floor
            }
        };
        self.pan = (place(w, bw, x), place(h, bh, y));
    }
    /// The room the board has in view, less the margin a fit keeps.
    pub(super) fn room(&self) -> (f32, f32) {
        let (_, _, w, h) = self.bounds.get();
        (w - FIT_MARGIN, h - FIT_MARGIN)
    }
}

impl MissionsPage {
    /// The board as drawn. Beside Flow it is read only and shows no run (no
    /// run chips, no room for them), and is laid out for the pane it has
    /// rather than at its saved places, which are for the whole window.
    pub(super) fn shown_board(&self, mission: &Mission, running: bool) -> board::Board {
        let flow = flow::build(mission);
        if self.beside {
            let layout = board::shaped(mission, &flow, self.canvas.room(), BESIDE_FLOOR);
            return board::board(mission, &flow, &layout, false);
        }
        let mut layout = self
            .state
            .layout
            .as_ref()
            .map(|l| l.nodes.clone())
            .unwrap_or_default();
        if let Some(Drag::Card { id, now, .. }) = &self.canvas.drag {
            layout.retain(|p| p.id != *id);
            layout.push(LayoutNode {
                id: id.clone(),
                x: now.0.round() as i64,
                y: now.1.round() as i64,
            });
        }
        board::board(mission, &flow, &layout, running)
    }
    pub(super) fn render_canvas(
        &mut self,
        t: &ahjo_app_kit::theme::Theme,
        mission: &Mission,
        running: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let board = self.shown_board(mission, running);
        if let Some(id) = self.canvas.follow.clone() {
            match board.cards.iter().find(|c| c.id == id) {
                Some(c) if self.editor.as_ref().is_some_and(|e| e.id == id) => {
                    self.canvas.show((c.x, c.y, c.w, c.h))
                }
                _ => self.canvas.follow = None,
            }
        }
        let states = self
            .selected_run()
            .map(|run| crate::store::run_state::project(&run.events));
        let ports = board::ports(mission, &board);
        let findings = if running {
            Vec::new()
        } else {
            self.canvas_findings()
        };
        let view = self.canvas.clone();
        let drag_ports = ports.clone();
        let mut layer = div()
            .id("mission-canvas")
            .debug_selector(|| "mission-canvas".into())
            .size_full()
            .relative()
            .overflow_hidden()
            .bg(t.bg)
            .cursor_grab()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|p, e: &MouseDownEvent, _, cx| {
                    p.canvas.menu = None;
                    p.canvas.drag = Some(Drag::Pan(e.position, p.canvas.pan));
                    p.canvas.follow = None;
                    cx.notify();
                }),
            )
            .when(!running, |d| {
                d.on_mouse_down(
                    MouseButton::Right,
                    cx.listener(|p, e: &MouseDownEvent, _, cx| {
                        p.canvas.menu = Some(p.canvas.world(e.position));
                        cx.notify();
                    }),
                )
            })
            .on_mouse_move(cx.listener(|p, e: &MouseMoveEvent, _, cx| {
                if e.pressed_button != Some(MouseButton::Left) {
                    p.canvas.drag = None;
                    return;
                }
                let world = p.canvas.world(e.position);
                let z = p.canvas.zoom;
                match &mut p.canvas.drag {
                    None => return,
                    Some(Drag::Pan(from, pan)) => {
                        let d = e.position - *from;
                        p.canvas.pan = (pan.0 + f32::from(d.x), pan.1 + f32::from(d.y));
                        p.canvas.fitted = true;
                        p.canvas.touched |= f32::from(d.x).hypot(f32::from(d.y)) > DRAG_SLOP;
                    }
                    Some(Drag::Card {
                        at,
                        from,
                        now,
                        moved,
                        ..
                    }) => {
                        let d = e.position - *at;
                        let (dx, dy) = (f32::from(d.x), f32::from(d.y));
                        *moved |= dx.hypot(dy) > DRAG_SLOP;
                        *now = (from.0 + dx / z, from.1 + dy / z);
                    }
                    Some(Drag::Wire { now, .. }) => *now = world,
                }
                cx.notify();
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |p, e: &MouseUpEvent, _, cx| {
                    let at = p.canvas.world(e.position);
                    match p.canvas.drag.take() {
                        Some(Drag::Card {
                            id,
                            from,
                            now,
                            moved,
                            ..
                        }) => {
                            if moved {
                                p.canvas_move(&id, from, now, cx);
                            } else {
                                p.edit_step(&id, cx);
                            }
                        }
                        Some(Drag::Wire { from, lifted, .. }) => {
                            let target = board::drop_target(&drag_ports, &from, at)
                                .map(|port| port.endpoint.clone());
                            p.canvas_wire(from, lifted, target, cx);
                        }
                        _ => (),
                    }
                    cx.notify();
                }),
            )
            .on_scroll(cx.listener(|p, e: &ScrollEvent, _, cx| {
                let delta = e.delta.pixel_delta(px(size::ROW));
                // Ctrl+wheel zooms, as on every surface (app-kit's zoom).
                if let Some(steps) = p.canvas.wheel.steps(e) {
                    if steps != 0 {
                        // The point under the pointer stays under it.
                        let (x, y, _, _) = p.canvas.bounds.get();
                        let at = (f32::from(e.position.x) - x, f32::from(e.position.y) - y);
                        p.canvas.zoom_at(ZOOM_STEP.powi(steps), at);
                    }
                } else {
                    p.canvas.follow = None;
                    p.canvas.pan.0 += f32::from(delta.x);
                    p.canvas.pan.1 += f32::from(delta.y);
                    p.canvas.fitted = true;
                    p.canvas.touched = true;
                }
                p.canvas.menu = None;
                cx.notify();
            }))
            // Its Ctrl+wheel is the board's zoom, not the whole app's.
            .child(ahjo_app_kit::zoom::claim(|_, _| true));
        layer = layer.child(wires(
            t,
            &board,
            view.clone(),
            cx.weak_entity(),
            self.generation,
            &findings,
            running || self.state.layout.is_some(),
        ));
        // A loop back says where it returns to, on its lane.
        for w in board.wires.iter().filter(|w| w.back) {
            let lane = w.lane.unwrap_or_default();
            let (x, y) = view.position((w.from.0 + w.to.0) / 2., lane);
            let colour = if w.route == "fail" { t.fail } else { t.wire };
            let selector = format!("canvas-loop-{}", w.id);
            layer = layer.child(
                div()
                    .absolute()
                    .left(px(x))
                    .top(px(y - CARD_SMALL * view.zoom))
                    .debug_selector(move || selector.clone())
                    .px(px(CHIP_PAD * view.zoom))
                    .rounded(px(size::RADIUS * view.zoom))
                    .bg(t.bg)
                    .text_size(px(CARD_SMALL * view.zoom))
                    .text_color(colour)
                    .whitespace_nowrap()
                    .child(format!("↺ {}", mission.node_name(&w.to_node))),
            );
        }
        for card in &board.cards {
            let state = states
                .as_ref()
                .and_then(|s| s.states.get(&card.id))
                .copied();
            let attempts = states
                .as_ref()
                .and_then(|s| s.attempts.get(&card.id))
                .copied();
            layer = layer.child(self.canvas_card(
                t, mission, card, &view, running, state, attempts, &findings, cx,
            ));
            layer = layer.children(self.card_badge(t, mission, card, &view, &findings));
        }
        if !running {
            // "+" offers what comes next, so only where nothing does yet; a
            // wire never runs through it.
            let open = |card: &board::Card| {
                leaving_port(mission, &card.id)
                    .is_some_and(|port| !mission.connections.iter().any(|c| c.from == port))
            };
            for card in board
                .cards
                .iter()
                .filter(|c| c.kind != board::Kind::End && open(c))
            {
                layer = layer.child(self.canvas_next(t, card, &view, cx));
            }
        }
        if !running && mission.input_cards.is_empty() && mission.formations.is_empty() {
            // Top left, clear of the notes card that fitting centres.
            layer = layer.child(
                div()
                    .absolute()
                    .top(px(PAD))
                    .left(px(PAD))
                    .w(px(size::NOTICE))
                    .child(self.empty_mission(t, "canvas-empty", cx)),
            );
        }
        // Beside Flow the notes are read in Flow; the board shows the shape.
        if !self.beside {
            for note in self.canvas_notes(t, mission, &board, &view, cx) {
                layer = layer.child(note);
            }
        }
        if !running {
            for port in &ports {
                layer = layer.child(self.canvas_port(t, mission, port, &view, cx));
            }
        }
        for edge in &mission.connections {
            let matching: Vec<_> = findings
                .iter()
                .filter(|(finding, _)| finding.node_id == edge.id)
                .collect();
            if matching.is_empty() {
                continue;
            }
            // A dangling connection can have no drawn wire. Anchor its mark
            // by the surviving port, or by a surviving endpoint's card.
            let location = board
                .wires
                .iter()
                .find(|wire| wire.id == edge.id)
                .map(|wire| {
                    (
                        (wire.from.0 + wire.to.0) / 2.,
                        wire.lane.unwrap_or((wire.from.1 + wire.to.1) / 2.),
                    )
                })
                .or_else(|| {
                    ports
                        .iter()
                        .find(|port| port.endpoint == edge.from || port.endpoint == edge.to)
                        .map(|port| (port.at.0 + board::PORT, port.at.1 + board::PORT))
                })
                .or_else(|| {
                    board
                        .cards
                        .iter()
                        .find(|card| {
                            edge.from.split(':').next() == Some(card.id.as_str())
                                || edge.to.split(':').next() == Some(card.id.as_str())
                        })
                        .map(|card| (card.x + card.w / 2., card.y + card.h / 2.))
                });
            let Some((x, y)) = location else {
                continue;
            };
            let (x, y) = view.position(x, y);
            let selector = format!("validation-edge-{}", edge.id);
            let message = matching
                .iter()
                .map(|(finding, _)| {
                    crate::store::validation::readable_message(mission, &finding.message)
                })
                .collect::<Vec<_>>()
                .join("\n");
            let color = self.finding_color(t, matching.iter().any(|(_, error)| *error));
            layer = layer.child(
                div()
                    .id(SharedString::from(selector.clone()))
                    .debug_selector(move || selector.clone())
                    .absolute()
                    .left(px(x))
                    .top(px(y))
                    .size(px(size::CONTROL * view.zoom))
                    .rounded_full()
                    .bg(color)
                    .text_color(t.bg)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child("!")
                    .tooltip(kit::tip(message)),
            );
        }
        if !running && let Some(at) = view.menu {
            layer = layer.child(self.canvas_menu(t, mission, at, &view, cx));
        }
        let fit = board.clone();
        // The board's own controls, in a strip under it: never over a card.
        let controls = div()
            .flex_none()
            .flex()
            .items_center()
            .justify_end()
            .gap(px(size::RADIUS))
            .px(px(PAD))
            .py(px(size::RADIUS))
            .border_t_1()
            .border_color(t.divider)
            .child(
                div()
                    .text_size(px(size::SMALL))
                    .text_color(t.text2)
                    .child(format!("{:.0}%", view.zoom * 100.)),
            )
            .child(
                kit::button(t, "canvas-in", "+", Kind::Plain)
                    .debug_selector(|| "canvas-in".into())
                    .on_click(cx.listener(|p, _, _, cx| {
                        p.canvas.step(true);
                        cx.notify();
                    })),
            )
            .child(
                kit::button(t, "canvas-out", "−", Kind::Plain)
                    .debug_selector(|| "canvas-out".into())
                    .on_click(cx.listener(|p, _, _, cx| {
                        p.canvas.step(false);
                        cx.notify();
                    })),
            )
            .child(
                kit::button(t, "canvas-fit", "Fit", Kind::Plain)
                    .debug_selector(|| "canvas-fit".into())
                    .tooltip(kit::tip(
                        "Show the whole mission (Ctrl+0: back to how it opened)",
                    ))
                    .on_click(cx.listener(move |p, _, _, cx| {
                        // The view is the operator's again: it stops
                        // following an edited card and frames each change.
                        p.canvas.follow = None;
                        p.canvas.touched = false;
                        p.canvas.fit(&fit);
                        cx.notify();
                    })),
            )
            .when(!running, |d| {
                d.child(
                    kit::button(t, "canvas-arrange", "Arrange", Kind::Plain)
                        .debug_selector(|| "arrange".into())
                        .on_click(cx.listener(|p, _, _, cx| p.arrange(cx))),
                )
            });
        if let Some(id) = &self.reveal_card {
            let selector = format!("mission-reveal-{id}");
            layer = layer.child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .debug_selector(move || selector.clone())
                    .child(format!("Card: {}", mission.node_name(id))),
            );
        }
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(layer.flex_1().min_w_0().min_h_0())
            .child(controls)
    }

    /// The mark of a card that has findings: astride its top-left corner,
    /// outside the card, so it covers none of the card's words.
    fn card_badge(
        &self,
        t: &ahjo_app_kit::theme::Theme,
        mission: &Mission,
        card: &board::Card,
        view: &View,
        findings: &[(ahjo_missions_contract::Finding, bool)],
    ) -> Option<AnyElement> {
        let issues: Vec<_> = findings
            .iter()
            .filter(|(finding, _)| finding.node_id == card.id)
            .collect();
        if issues.is_empty() {
            return None;
        }
        let z = view.zoom;
        let side = size::CONTROL * z * BADGE_SCALE;
        let (x, y) = view.position(card.x, card.y);
        let selector = format!("validation-card-{}", card.id);
        let message = issues
            .iter()
            .map(|(finding, _)| {
                crate::store::validation::readable_message(mission, &finding.message)
            })
            .collect::<Vec<_>>()
            .join("\n");
        Some(
            div()
                .id(SharedString::from(selector.clone()))
                .debug_selector(move || selector.clone())
                .absolute()
                // Above the card's far corner: clear of its ports and
                // header, whichever part the finding is about.
                .left(px(x + card.w * z - side))
                .top(px(y - side - EDGE * z))
                .size(px(side))
                .rounded_full()
                .bg(self.finding_color(t, issues.iter().any(|(_, error)| *error)))
                .text_color(t.bg)
                .text_size(px(CARD_SMALL * z))
                .flex()
                .items_center()
                .justify_center()
                .child("!")
                .tooltip(kit::tip(message))
                .into_any_element(),
        )
    }

    /// A finding's colour: red or amber once a run was asked for, a quiet
    /// to-do while the mission is being built.
    fn finding_color(&self, t: &ahjo_app_kit::theme::Theme, error: bool) -> gpui::Hsla {
        match (self.run_tried, error) {
            (false, _) => t.text2,
            (true, true) => t.error,
            (true, false) => t.needs,
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn canvas_card(
        &self,
        t: &ahjo_app_kit::theme::Theme,
        mission: &Mission,
        card: &board::Card,
        view: &View,
        running: bool,
        state: Option<crate::store::run_state::NodeState>,
        attempts: Option<i64>,
        findings: &[(ahjo_missions_contract::Finding, bool)],
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let z = view.zoom;
        let id = card.id.clone();
        let from = (card.x, card.y);
        let selector = format!("mission-card-{id}");
        let face = card_face(t, mission, card, z, running && !self.beside, state);
        let issues: Vec<_> = findings
            .iter()
            .filter(|(finding, _)| finding.node_id == id)
            .collect();
        let issue_color = self.finding_color(t, issues.iter().any(|(_, error)| *error));
        let color = match state {
            Some(crate::store::run_state::NodeState::Done) => t.done,
            Some(
                crate::store::run_state::NodeState::Failed
                | crate::store::run_state::NodeState::Blocked,
            ) => t.error,
            Some(crate::store::run_state::NodeState::Running) => t.running,
            Some(crate::store::run_state::NodeState::Waiting) => t.needs,
            _ if card.kind == board::Kind::End => {
                if mission
                    .ends
                    .iter()
                    .any(|e| e.id == card.id && e.outcome == "rejected")
                {
                    t.error
                } else {
                    t.done
                }
            }
            _ => t.divider,
        };
        let rendered = div()
            .id(SharedString::from(format!("card-{id}")))
            .absolute()
            .left(px(card.x * z + view.pan.0))
            .top(px(card.y * z + view.pan.1))
            .w(px(card.w * z))
            .h(px(card.h * z))
            .overflow_hidden()
            .border_1()
            // The step chosen in Flow beside the board stands out on it.
            .when(
                self.beside && self.flow_focus.as_deref() == Some(&id),
                |d| d.border_2(),
            )
            .border_color(if !issues.is_empty() && self.run_tried {
                issue_color
            } else if self.reveal_card.as_deref() == Some(&id)
                || card.kind == board::Kind::Input
                || (self.beside && self.flow_focus.as_deref() == Some(&id))
            {
                t.accent
            } else {
                color
            })
            .bg(if card.kind == board::Kind::Limit {
                t.raised
            } else {
                t.surface
            })
            .shadow_md()
            .when(card.kind == board::Kind::End, |d| {
                d.rounded(px(board::END_H * z / 2.))
            })
            .when(card.kind != board::Kind::End, |d| {
                d.rounded(px(size::RADIUS * z))
            })
            .cursor_pointer()
            .debug_selector(move || selector.clone())
            .child(face);
        let rendered = if card.kind == board::Kind::Input && !running {
            let card_id = card.id.clone();
            let runnable = self.mission_input_runnable(&card_id);
            rendered.child(
                div()
                    .id(SharedString::from(format!("canvas-play-{card_id}")))
                    .debug_selector({
                        let card_id = card_id.clone();
                        move || format!("canvas-play-{card_id}")
                    })
                    .absolute()
                    .top(px(PAD * z))
                    .right(px(PAD * z))
                    .size(px(RING_SIZE * z))
                    .rounded(px(size::RADIUS * z))
                    .bg(if runnable { t.accent } else { t.raised })
                    .text_color(if runnable { t.on_accent() } else { t.dim })
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .child("▶")
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |page, _, _, cx| {
                        cx.stop_propagation();
                        page.prepare_card_run(&card_id, cx);
                    })),
            )
        } else {
            rendered
        };
        let rendered = if running && let Some(count) = attempts.filter(|n| *n > 0) {
            let step = card.id.clone();
            let offset = mission
                .formations
                .iter()
                .find(|f| f.id == step)
                .map(|f| {
                    f.inputs.len() as f32 * board::IO_ROW + board::step_head(f, true)
                        - size::CONTROL
                })
                .unwrap_or(0.);
            rendered.child(
                div()
                    .id(SharedString::from(format!("card-attempts-{step}")))
                    .debug_selector({
                        let step = step.clone();
                        move || format!("card-attempts-{step}")
                    })
                    .absolute()
                    .right(px(PAD * z))
                    .top(px(offset * z))
                    .px(px(GAP * z))
                    .rounded(px(size::RADIUS * z))
                    .bg(t.raised)
                    .text_color(t.text2)
                    .text_size(px(CARD_SMALL * z))
                    .cursor_pointer()
                    .child(format!(
                        "{count} attempt{}",
                        if count == 1 { "" } else { "s" }
                    ))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |page, _, window, cx| {
                        cx.stop_propagation();
                        page.open_canvas_attempts(&step, window, cx);
                    })),
            )
        } else {
            rendered
        };
        if running && self.beside {
            // Beside Flow, a card is where to read about it.
            let step = card.id.clone();
            return rendered.on_click(cx.listener(move |page, _, _, cx| {
                page.choose_in_flow(&step, true, cx);
            }));
        }
        if running {
            return rendered.cursor_default();
        }
        rendered.on_mouse_down(
            MouseButton::Left,
            cx.listener(move |p, e: &MouseDownEvent, _, cx| {
                p.canvas.menu = None;
                p.canvas.drag = Some(Drag::Card {
                    id: id.clone(),
                    at: e.position,
                    from,
                    now: from,
                    moved: false,
                });
                cx.stop_propagation();
            }),
        )
    }
    fn canvas_port(
        &self,
        t: &ahjo_app_kit::theme::Theme,
        mission: &Mission,
        port: &board::Port,
        view: &View,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let z = view.zoom;
        let diameter = board::PORT * z;
        let colour = if port.endpoint.ends_with(":pass") {
            t.pass
        } else if port.endpoint.ends_with(":fail") {
            t.fail
        } else if port.endpoint.ends_with(":judge") {
            t.dim
        } else {
            t.wire
        };
        let endpoint = port.endpoint.clone();
        let out = port.out;
        let at = port.at;
        let wired = port.wired;
        let existing = mission
            .connections
            .iter()
            .rev()
            .find(|c| c.to == endpoint)
            .map(|c| c.from.clone());
        let selector = format!("mission-port-{endpoint}");
        let accent = t.accent;
        div()
            .id(SharedString::from(selector.clone()))
            .debug_selector(move || selector.clone())
            .absolute()
            .left(px(at.0 * z + view.pan.0 - diameter / 2.))
            .top(px(at.1 * z + view.pan.1 - diameter / 2.))
            .size(px(diameter))
            .rounded_full()
            .border_2()
            .border_color(colour)
            .bg(if wired { colour } else { t.surface })
            .cursor_crosshair()
            .hover(move |d| d.border_color(accent))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |p, _, _, cx| {
                    cx.stop_propagation();
                    p.canvas.menu = None;
                    p.canvas.drag = if out {
                        Some(Drag::Wire {
                            from: endpoint.clone(),
                            start: at,
                            lifted: None,
                            now: at,
                        })
                    } else {
                        existing.clone().map(|from| Drag::Wire {
                            from,
                            start: (at.0 - CURVE_DOWN, at.1),
                            lifted: Some(endpoint.clone()),
                            now: at,
                        })
                    };
                    cx.notify();
                }),
            )
    }
    fn canvas_wire(
        &mut self,
        from: String,
        lifted: Option<String>,
        to: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if let Some(patch) = wire_patch(&from, lifted.as_deref(), to.as_deref()) {
            self.write_board(patch, WriteMode::Edit, cx);
        }
    }
    fn canvas_move(&mut self, id: &str, from: (f32, f32), to: (f32, f32), cx: &mut Context<Self>) {
        let before = LayoutNode {
            id: id.into(),
            x: from.0.round() as i64,
            y: from.1.round() as i64,
        };
        let after = LayoutNode {
            id: id.into(),
            x: to.0.round() as i64,
            y: to.1.round() as i64,
        };
        self.canvas_layout(vec![after], vec![before], cx);
    }
    fn canvas_layout(
        &mut self,
        nodes: Vec<LayoutNode>,
        reverse: Vec<LayoutNode>,
        cx: &mut Context<Self>,
    ) {
        if self.writing || self.editor.as_ref().is_some_and(|e| e.busy) {
            return;
        }
        let (Some((node, slug)), Some(etag), Some(mission)) = (
            self.state.selected.clone(),
            self.state.layout_etag.clone(),
            self.state.mission.as_ref(),
        ) else {
            return;
        };
        let rev = mission.rev;
        let generation = self.generation;
        let request = Request::new(
            "PATCH",
            NodeId(node),
            format!("{}/layout", mission_path(&slug)),
        )
        .header("If-Match", etag);
        let body = EditLayout {
            nodes: Some(nodes),
            ..Default::default()
        };
        let request = match edits::with_json_body(request, &body) {
            Ok(request) => request,
            Err(error) => {
                self.state.error = Some(error.to_string());
                cx.notify();
                return;
            }
        };
        self.writing = true;
        let asked = self.host.request(request, cx);
        cx.spawn(async move |this, cx| {
            let result = asked.await;
            let _ = this.update(cx, |p, cx| {
                if p.generation != generation {
                    return;
                }
                p.writing = false;
                match result {
                    Ok(answer) if answer.ok() => {
                        match serde_json::from_slice::<LayoutResponse>(&answer.body) {
                            Ok(v) => {
                                let etag = v.layout.etag;
                                p.state.layout = Some(v.layout.layout);
                                p.state.layout_etag = Some(etag.clone());
                                let entry = undo::Entry {
                                    label: "move".into(),
                                    steps: vec![undo::Step::Layout(reverse)],
                                };
                                p.history.wrote(undo::Version {
                                    rev,
                                    layout: Some(etag),
                                });
                                p.history.record(entry);
                            }
                            Err(e) => {
                                p.state.error = Some(e.to_string());
                            }
                        }
                    }
                    Ok(answer) => {
                        p.state.error = Some(answer.error_text());
                    }
                    Err(error) => {
                        p.state.error = Some(error.message);
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    fn canvas_menu(
        &self,
        t: &ahjo_app_kit::theme::Theme,
        mission: &Mission,
        at: (f32, f32),
        view: &View,
        cx: &mut Context<Self>,
    ) -> Div {
        let item = |id: &'static str, label: &'static str, patch: MissionChange| {
            div()
                .id(id)
                .debug_selector(move || id.into())
                .px(px(PAD))
                .py(px(size::RADIUS))
                .cursor_pointer()
                .child(label)
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |p, _, _, cx| {
                    p.canvas.menu = None;
                    p.write_board(patch.clone(), WriteMode::Edit, cx);
                }))
        };
        let x = at.0.round() as i64;
        let y = at.1.round() as i64;
        div()
            .absolute()
            .occlude()
            .left(px(at.0 * view.zoom + view.pan.0))
            .top(px(at.1 * view.zoom + view.pan.1))
            .w(px(size::MENU))
            .bg(t.raised)
            .border_1()
            .border_color(t.divider)
            .rounded(px(size::RADIUS))
            .shadow_lg()
            .child(kit::label(t, "New"))
            .children(
                new_card_items(x, y, mission.input_cards.is_empty())
                    .map(|(id, label, patch)| item(id, label, patch)),
            )
    }
}

fn wires(
    t: &ahjo_app_kit::theme::Theme,
    board: &board::Board,
    view: View,
    entity: gpui::WeakEntity<MissionsPage>,
    generation: u64,
    findings: &[(ahjo_missions_contract::Finding, bool)],
    ready_to_fit: bool,
) -> impl IntoElement {
    let fit_board = board.clone();
    let list = board.wires.clone();
    let errors: std::collections::HashSet<_> = findings
        .iter()
        .filter(|(_, error)| *error)
        .map(|(finding, _)| finding.node_id.clone())
        .collect();
    let accent = t.accent;
    let (work, pass, fail, quiet, error) = (t.wire, t.pass, t.fail, t.dim, t.error);
    let bounds = view.bounds.clone();
    let loose = match &view.drag {
        Some(Drag::Wire { start, now, .. }) => Some((*start, *now)),
        _ => None,
    };
    canvas(
        |_, _, _| (),
        boundary::paint(move |b: Bounds<Pixels>, _, window, cx| {
            let before = bounds.get();
            bounds.set((
                f32::from(b.origin.x),
                f32::from(b.origin.y),
                f32::from(b.size.width),
                f32::from(b.size.height),
            ));
            // A panel opening beside the canvas narrows it: draw again so
            // the card being edited is brought back into view. The board
            // itself stays where it is, so nothing moves under the pointer
            // when the panel closes.
            let resized = (before.2, before.3) != (bounds.get().2, bounds.get().3);
            // Beside Flow the board is laid out for its pane, so a new
            // size lays it out and frames it again. On its own it stays put.
            if resized && before.2 > 0. {
                let entity = entity.clone();
                cx.defer(move |cx| {
                    let _ = entity.update(cx, |page, cx| {
                        if page.beside {
                            page.canvas.reopen();
                            cx.notify();
                        }
                    });
                });
            }
            if resized && view.follow.is_some() {
                let entity = entity.clone();
                cx.defer(move |cx| {
                    let _ = entity.update(cx, |_, cx| cx.notify());
                });
            }
            if ready_to_fit && !view.fitted {
                let entity = entity.clone();
                let fit_board = fit_board.clone();
                cx.defer(move |cx| {
                    let _ = entity.update(cx, |page, cx| {
                        if page.generation == generation && !page.canvas.fitted {
                            // Beside Flow the board is laid out for the pane
                            // as it now is, and opens as large as fits it.
                            let mission = page.state.mission.clone();
                            match mission {
                                Some(m) if page.beside => {
                                    let board = page.shown_board(&m, false);
                                    page.canvas.open(&board, BESIDE_FLOOR, true);
                                }
                                _ => page.canvas.open(&fit_board, READABLE, false),
                            }
                            if page.canvas.fitted {
                                cx.notify();
                            }
                        }
                    });
                });
            }
            let z = view.zoom;
            let at = |p: (f32, f32)| {
                point(
                    b.origin.x + px(p.0 * z + view.pan.0),
                    b.origin.y + px(p.1 * z + view.pan.1),
                )
            };
            for w in &list {
                let colour = if errors.contains(&w.id) {
                    error
                } else {
                    match w.route.as_str() {
                        "pass" => pass,
                        "fail" => fail,
                        "judge" | "limit" => quiet,
                        _ => work,
                    }
                };
                let (a, end) = (at(w.from), at(w.to));
                let mut path = PathBuilder::stroke(px(EDGE));
                // A judge's question and answer, and a Limit card's link,
                // are dotted; a loop back is dashed.
                if matches!(w.route.as_str(), "judge" | "limit") {
                    path = path.dash_array(&[px(DASH_SHORT), px(DASH_LONG)]);
                } else if w.back {
                    path = path.dash_array(&[px(DASH_LONG * 2.), px(DASH_LONG)]);
                }
                path.move_to(a);
                if let Some(lane) = w.lane {
                    let out = px(board::LOOP_OUT * z);
                    let y = at((0., lane)).y;
                    for p in [
                        point(a.x + out, a.y),
                        point(a.x + out, y),
                        point(end.x - out, y),
                        point(end.x - out, end.y),
                        end,
                    ] {
                        path.line_to(p);
                    }
                } else if w.route == "limit" {
                    path.line_to(end);
                } else {
                    let reach = px(((w.to.0 - w.from.0).abs() / 2.).max(CURVE_REACH) * z);
                    let down = px(CURVE_DOWN * z);
                    let (ca, cb) = if w.route == "judge" {
                        (
                            if w.to.1 > w.from.1 {
                                point(a.x, a.y + down)
                            } else {
                                point(a.x + reach, a.y)
                            },
                            if w.to.1 < w.from.1 {
                                point(end.x, end.y + down)
                            } else {
                                point(end.x - reach, end.y)
                            },
                        )
                    } else {
                        (point(a.x + reach, a.y), point(end.x - reach, end.y))
                    };
                    path.cubic_bezier_to(end, ca, cb);
                }
                if let Ok(path) = path.build() {
                    window.paint_path(path, colour);
                }
            }
            if let Some((from, to)) = loose {
                let (a, end) = (at(from), at(to));
                let reach = px(((to.0 - from.0).abs() / 2.).max(CURVE_REACH) * z);
                let mut path =
                    PathBuilder::stroke(px(EDGE)).dash_array(&[px(DASH_LONG), px(DASH_LONG)]);
                path.move_to(a);
                path.cubic_bezier_to(end, point(a.x + reach, a.y), point(end.x - reach, end.y));
                if let Ok(path) = path.build() {
                    window.paint_path(path, accent);
                }
            }
        }),
    )
    .absolute()
    .inset_0()
    .size_full()
}

fn card_face(
    t: &ahjo_app_kit::theme::Theme,
    mission: &Mission,
    card: &board::Card,
    z: f32,
    running: bool,
    state: Option<crate::store::run_state::NodeState>,
) -> Div {
    let label = |text: String, colour: Hsla| {
        div()
            .text_size(px(CARD_SMALL * z))
            .text_color(colour)
            .truncate()
            .child(text)
    };
    match card.kind {
        board::Kind::Formation => {
            let Some(f) = mission.formations.iter().find(|f| f.id == card.id) else {
                return div();
            };
            let mut face = div().size_full().flex().flex_col();
            for p in &f.inputs {
                let to = format!("{}:{}", f.id, p.id);
                let from: Vec<String> = mission
                    .connections
                    .iter()
                    .filter(|c| c.to == to)
                    .map(|c| {
                        let (id, port) = c.from.split_once(':').unwrap_or((&c.from, ""));
                        if matches!(port, "pass" | "fail" | "judge") {
                            format!("{} ({port})", mission.node_name(id))
                        } else {
                            mission.node_name(id).into()
                        }
                    })
                    .collect();
                let source = if from.is_empty() {
                    "nothing wired".into()
                } else {
                    format!("from {}", from.join(", "))
                };
                face = face.child(
                    div()
                        .h(px(board::IO_ROW * z))
                        .flex_none()
                        .px(px(PAD * z))
                        .flex()
                        .items_center()
                        .gap(px(PAD * z))
                        .border_b_1()
                        .border_color(t.divider)
                        .child(label("IN".into(), t.dim))
                        .child(label(source, t.text2)),
                );
            }
            let number = flow::build(mission).numbers.get(&f.id).copied();
            let brief = f
                .brief
                .as_ref()
                .map(|b| b.goal.trim())
                .filter(|s| !s.is_empty());
            let mut titles = div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(size::RADIUS * z))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(GAP * z))
                        .when_some(number, |d, n| {
                            d.child(
                                div()
                                    .flex_none()
                                    .size(px(END_MARK * z))
                                    .rounded_full()
                                    .border_1()
                                    .border_color(t.dim)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(CARD_SMALL * z))
                                    .text_color(t.text2)
                                    .child(n.to_string()),
                            )
                        })
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_size(px(TITLE_SIZE * z))
                                .font_weight(FontWeight::BOLD)
                                .text_color(t.text)
                                .child(f.title.clone()),
                        ),
                )
                .when_some(brief, |d, brief| {
                    d.child(
                        div()
                            .text_size(px(CARD_SMALL * z))
                            .text_color(t.text2)
                            .min_w_0()
                            .line_clamp(2)
                            .text_ellipsis()
                            .child(brief.to_owned()),
                    )
                });
            if running {
                let (word, colour) = state_words(t, state);
                // Its own width, with the attempts count at the row's other end.
                titles = titles.child(div().flex().child(chip(z, word, colour)));
            }
            let head = div()
                .h(px(board::step_head(f, running) * z))
                .flex_none()
                .px(px(PAD * z))
                .py(px(PAD * z))
                .border_b_1()
                .border_color(t.divider)
                .flex()
                .gap(px(SLOT_PAD * z))
                .child(titles)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_end()
                        .gap(px(CHIP_PAD * z))
                        // A step is worked by one agent unless it says otherwise.
                        .when(f.kind != "solo", |d| d.child(chip(z, &f.kind, t.text2)))
                        // Each attempt in sessions of its own.
                        .when(f.fresh, |d| d.child(chip(z, "fresh", t.text2))),
                );
            face = face.child(head);
            let mut body = div()
                .h(px(board::body_h(&f.kind, f.slots.len()) * z))
                .flex_none()
                .px(px(SLOT_PAD * z))
                .py(px(board::BODY_PAD * z))
                .flex()
                .flex_col()
                .gap(px(board::SLOT_GAP * z));
            if board::pooled(&f.kind) {
                for slot in f.slots.iter().filter(|slot| slot.controller) {
                    body = body.child(slot_row(t, mission, f, slot, z));
                }
                let mut pool = div()
                    .relative()
                    .flex()
                    .flex_col()
                    .gap(px(board::SLOT_GAP * z))
                    .pt(px(POOL_LABEL * z))
                    .pb(px((board::POOL_PAD - POOL_LABEL) * z))
                    .px(px(size::RADIUS * z))
                    .border_1()
                    .border_dashed()
                    .border_color(t.divider)
                    .rounded(px(GAP * z))
                    .child(
                        label(
                            if f.kind == "peer" { "PEERS" } else { "WORKERS" }.into(),
                            t.dim,
                        )
                        .absolute()
                        .top(px(-GAP * z))
                        .left(px(POOL_LABEL * z))
                        .px(px(CHIP_PAD * z))
                        .bg(t.surface),
                    );
                for slot in f.slots.iter().filter(|slot| !slot.controller) {
                    pool = pool.child(slot_row(t, mission, f, slot, z));
                }
                body = body.child(pool);
            } else {
                for slot in &f.slots {
                    body = body.child(slot_row(t, mission, f, slot, z));
                }
            }
            if f.slots.is_empty() {
                body = body.child(label("No slots".into(), t.dim));
            }
            face = face.child(body);
            for p in &f.outputs {
                let output = match state {
                    Some(crate::store::run_state::NodeState::Done) => "output ready",
                    Some(crate::store::run_state::NodeState::Running) => "running",
                    Some(crate::store::run_state::NodeState::Waiting) => "needs you",
                    Some(
                        crate::store::run_state::NodeState::Blocked
                        | crate::store::run_state::NodeState::Failed,
                    ) => "blocked",
                    _ => "no output yet",
                };
                face = face.child(
                    div()
                        .h(px(board::IO_ROW * z))
                        .flex_none()
                        .px(px(PAD * z))
                        .flex()
                        .items_center()
                        .gap(px(PAD * z))
                        .border_t_1()
                        .border_color(t.divider)
                        .child(label("OUT".into(), t.dim))
                        .child(
                            label(
                                if p.label.is_empty() {
                                    p.id.clone()
                                } else {
                                    p.label.clone()
                                },
                                t.text2,
                            )
                            .flex_1(),
                        )
                        // A step's output has a state only while a run is shown.
                        .when(state.is_some(), |d| {
                            d.child(chip(z, output, state_words(t, state).1))
                        }),
                );
            }
            face
        }
        board::Kind::Input => {
            let Some(c) = mission.input_cards.iter().find(|c| c.id == card.id) else {
                return div();
            };
            let names: Vec<_> = c.inputs.iter().map(|i| i.name.as_str()).collect();
            let line = |text: String, colour: Hsla| {
                div()
                    .h(px(board::INPUT_LINE * z))
                    .px(px(POOL_LABEL * z))
                    .flex()
                    .items_center()
                    .text_size(px(CARD_SMALL * z))
                    .text_color(colour)
                    .child(div().truncate().child(text))
            };
            div()
                .size_full()
                .flex()
                .flex_col()
                .child(
                    div()
                        .h(px(board::INPUT_HEAD * z))
                        .px(px(POOL_LABEL * z))
                        .pt(px(PAD * z))
                        .flex()
                        .flex_col()
                        .gap(px(CHIP_PAD * z))
                        .child(label("◆ INPUT".into(), t.accent))
                        .child(
                            div()
                                .font_weight(FontWeight::BOLD)
                                .text_size(px(TITLE_SIZE * z))
                                .truncate()
                                .child(c.title.clone()),
                        ),
                )
                .child(
                    div()
                        .h(px(board::INPUT_GOAL * z))
                        .px(px(POOL_LABEL * z))
                        .overflow_hidden()
                        .line_clamp(2)
                        .text_size(px(CARD_SMALL * z))
                        .text_color(t.text2)
                        .child(c.goal.clone()),
                )
                .when(!names.is_empty(), |d| {
                    d.child(line(format!("Inputs · {}", names.join(" · ")), t.text2))
                })
                .when(asks_you(mission), |d| {
                    d.child(line("Human gates · notify me".into(), t.dim))
                })
        }
        board::Kind::Gate => {
            let Some(g) = mission.gates.iter().find(|g| g.id == card.id) else {
                return div();
            };
            div()
                .size_full()
                .flex()
                .items_center()
                .gap(px(PAD * z))
                .p(px(PAD * z))
                .child(
                    div()
                        .size(px(GATE_MARK * z))
                        .flex_none()
                        .rounded_full()
                        .border_2()
                        .border_color(state_words(t, state).1)
                        .bg(t.raised)
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(TITLE_SIZE * z))
                        .child("⛩"),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .font_weight(FontWeight::BOLD)
                                .text_size(px(CARD_TEXT * z))
                                .truncate()
                                .child(format!(
                                    "{}{}",
                                    flow::build(mission)
                                        .numbers
                                        .get(&g.id)
                                        .map(|n| format!("{n}  "))
                                        .unwrap_or_default(),
                                    g.title
                                )),
                        )
                        .child(
                            div()
                                .flex()
                                .gap(px(size::RADIUS * z))
                                .children(g.kinds.iter().map(|kind| {
                                    let word = match kind.as_str() {
                                        "formation" => "judge",
                                        "human" => "you",
                                        other => other,
                                    };
                                    label(word.to_owned(), t.text2)
                                        .px(px(size::RADIUS * z))
                                        .border_1()
                                        .border_color(t.divider)
                                })),
                        )
                        .child(
                            div()
                                .line_clamp(2)
                                .text_size(px(CARD_SMALL * z))
                                .text_color(t.text2)
                                .child(g.criterion.clone()),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .pt(px(size::CONTROL * z))
                        .child(
                            label("✓ PASS".into(), t.pass)
                                .debug_selector(|| "canvas-gate-pass-label".into()),
                        )
                        .child(
                            label("× FAIL".into(), t.fail)
                                .debug_selector(|| "canvas-gate-fail-label".into()),
                        ),
                )
        }
        board::Kind::Limit => {
            let Some(l) = mission.limits.iter().find(|l| l.id == card.id) else {
                return div();
            };
            div()
                .size_full()
                .flex()
                .flex_col()
                .justify_center()
                .gap(px(EDGE * z))
                .px(px(PAD * z))
                .child(label("⏱ LIMIT".into(), t.dim))
                .child(
                    div()
                        .font_weight(FontWeight::BOLD)
                        .text_size(px(CARD_TEXT * z))
                        .truncate()
                        .child(l.title.clone()),
                )
                .child(label(crate::store::words::limit_line(l), t.text2))
        }
        board::Kind::End => {
            let Some(e) = mission.ends.iter().find(|e| e.id == card.id) else {
                return div();
            };
            let colour = if e.outcome == "rejected" {
                t.error
            } else {
                t.done
            };
            div()
                .size_full()
                .flex()
                .items_center()
                .gap(px(PAD * z))
                .px(px(PAD * z))
                .child(
                    div()
                        .size(px(END_MARK * z))
                        .flex_none()
                        .rounded_full()
                        .border_2()
                        .border_color(colour)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(div().size(px(GAP * z)).rounded_full().bg(colour)),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(label(format!("END · {}", e.outcome), colour))
                        .child(
                            div()
                                .font_weight(FontWeight::BOLD)
                                .text_size(px(CARD_TEXT * z))
                                .child(e.title.clone()),
                        ),
                )
        }
    }
}

fn state_words(
    t: &ahjo_app_kit::theme::Theme,
    state: Option<crate::store::run_state::NodeState>,
) -> (&'static str, Hsla) {
    use crate::store::run_state::NodeState;
    match state {
        Some(NodeState::Done) => ("done", t.done),
        Some(NodeState::Running) => ("running", t.running),
        Some(NodeState::Waiting) => ("needs you", t.needs),
        Some(NodeState::Blocked) => ("blocked", t.error),
        Some(NodeState::Failed) => ("failed", t.error),
        Some(NodeState::Canceled) => ("canceled", t.dim),
        None => ("not reached", t.dim),
    }
}
fn chip(z: f32, text: &str, colour: Hsla) -> Div {
    div()
        .flex_none()
        .px(px(GAP * z))
        .py(px(EDGE * z))
        .rounded(px(size::RADIUS * z))
        .bg(ahjo_app_kit::theme::alpha(colour, CHIP_ALPHA))
        .text_size(px(CARD_SMALL * z))
        .text_color(colour)
        .child(text.to_uppercase())
}

fn slot_row(
    t: &ahjo_app_kit::theme::Theme,
    mission: &Mission,
    f: &ahjo_missions_contract::Formation,
    slot: &ahjo_missions_contract::Slot,
    z: f32,
) -> Div {
    let role = (!slot.agent_id.is_empty()).then(|| crate::store::words::role_name(&slot.agent_id));
    let session = crate::store::words::session_of(mission, f, slot);
    let mark = harness_mark(&slot.harness, t.text, t.text2, z);
    let selector = format!(
        "canvas-seat-{}-{}-{}",
        crate::store::words::slot_label(slot),
        crate::store::words::role_name(&slot.agent_id),
        crate::store::words::harness_name(&slot.harness)
    );
    let ring = div()
        .size(px(RING_SIZE * z))
        .flex_none()
        .rounded_full()
        .border_2()
        .border_color(if slot.controller {
            ahjo_app_kit::theme::rgb(t.ansi[3])
        } else {
            t.text2
        })
        .bg(t.raised)
        .flex()
        .items_center()
        .justify_center()
        .children(mark);
    div()
        .h(px(board::SLOT_H * z))
        .debug_selector(move || selector.clone())
        .flex_none()
        .flex()
        .items_center()
        .gap(px(GAP * z))
        .px(px(size::RADIUS * z))
        .child(ring)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_size(px(CARD_SMALL * z))
                        .text_color(t.dim)
                        .truncate()
                        .child(match role {
                            Some(role) => format!(
                                "{} · {}",
                                crate::store::words::slot_label(slot).to_uppercase(),
                                role.to_uppercase()
                            ),
                            None => crate::store::words::slot_label(slot).to_uppercase(),
                        }),
                )
                // The agent by the session it works in, as the session list
                // shows it.
                .child(
                    div()
                        .text_size(px(CARD_TEXT * z))
                        .font_weight(FontWeight::BOLD)
                        .truncate()
                        .debug_selector({
                            let session = session.clone();
                            move || format!("canvas-session-{session}")
                        })
                        .child(session),
                )
                .child(
                    div()
                        .text_size(px(CARD_SMALL * z))
                        .text_color(t.text2)
                        .truncate()
                        .child(crate::store::words::slot_line(slot)),
                ),
        )
}

const CLAUDE: &str = "M20.998 10.949H24v3.102h-3v3.028h-1.487V20H18v-2.921h-1.487V20H15v-2.921H9V20H7.488v-2.921H6V20H4.487v-2.921H3V14.05H0V10.95h3V5h17.998v5.949zM6 10.949h1.488V8.102H6v2.847zm10.51 0H18V8.102h-1.49v2.847z";
const CODEX: &str = "M8.086.457a6.105 6.105 0 013.046-.415c1.333.153 2.521.72 3.564 1.7a.117.117 0 00.107.029c1.408-.346 2.762-.224 4.061.366l.063.03.154.076c1.357.703 2.33 1.77 2.918 3.198.278.679.418 1.388.421 2.126a5.655 5.655 0 01-.18 1.631.167.167 0 00.04.155 5.982 5.982 0 011.578 2.891c.385 1.901-.01 3.615-1.183 5.14l-.182.22a6.063 6.063 0 01-2.934 1.851.162.162 0 00-.108.102c-.255.736-.511 1.364-.987 1.992-1.199 1.582-2.962 2.462-4.948 2.451-1.583-.008-2.986-.587-4.21-1.736a.145.145 0 00-.14-.032c-.518.167-1.04.191-1.604.185a5.924 5.924 0 01-2.595-.622 6.058 6.058 0 01-2.146-1.781c-.203-.269-.404-.522-.551-.821a7.74 7.74 0 01-.495-1.283 6.11 6.11 0 01-.017-3.064.166.166 0 00.008-.074.115.115 0 00-.037-.064 5.958 5.958 0 01-1.38-2.202 5.196 5.196 0 01-.333-1.589 6.915 6.915 0 01.188-2.132c.45-1.484 1.309-2.648 2.577-3.493.282-.188.55-.334.802-.438.286-.12.573-.22.861-.304a.129.129 0 00.087-.087A6.016 6.016 0 015.635 2.31C6.315 1.464 7.132.846 8.086.457zm-.804 7.85a.848.848 0 00-1.473.842l1.694 2.965-1.688 2.848a.849.849 0 001.46.864l1.94-3.272a.849.849 0 00.007-.854l-1.94-3.393zm5.446 6.24a.849.849 0 000 1.695h4.848a.849.849 0 000-1.696h-4.848z";
thread_local! { static MARKS: std::cell::RefCell<std::collections::HashMap<String, std::sync::Arc<gpui::Image>>> = std::cell::RefCell::new(std::collections::HashMap::new()); }
fn harness_mark(harness: &str, text: Hsla, secondary: Hsla, z: f32) -> Option<gpui::AnyElement> {
    let (path, fill) = match harness {
        "claude-code" | "claude" => (CLAUDE, "#D97757".to_owned()),
        "openai-codex" | "codex" => {
            let c = gpui::Rgba::from(text);
            (
                CODEX,
                format!(
                    "#{:02x}{:02x}{:02x}",
                    (c.r * 255.).round() as u8,
                    (c.g * 255.).round() as u8,
                    (c.b * 255.).round() as u8
                ),
            )
        }
        "shell" => {
            return Some(
                div()
                    .text_size(px(PAD * z))
                    .text_color(secondary)
                    .child(">_")
                    .into_any_element(),
            );
        }
        _ => return None,
    };
    let key = format!("{harness}:{fill}");
    let image=MARKS.with(|marks|marks.borrow_mut().entry(key).or_insert_with(|| {
        let svg=format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="96" height="96" viewBox="0 0 24 24"><path fill="{fill}" fill-rule="evenodd" clip-rule="evenodd" d="{path}"/></svg>"#);
        std::sync::Arc::new(gpui::Image::from_bytes(gpui::ImageFormat::Svg,svg.into_bytes()))
    }).clone());
    Some(
        gpui::img(image)
            .size(px(TITLE_SIZE * z))
            .flex_none()
            .into_any_element(),
    )
}

/// Whether any gate in the mission waits on the operator.
pub(super) fn asks_you(mission: &Mission) -> bool {
    mission
        .gates
        .iter()
        .any(|gate| gate.kinds.iter().any(|kind| kind == "human"))
}

impl MissionsPage {
    /// Add a card where it can be seen: in free space in view, or beside
    /// the board and brought into view when the view is full.
    /// A step comes after the one being edited, else after the last step
    /// nothing follows yet, else after the Input: wired, so the common
    /// case needs no wiring by hand.
    pub(super) fn add_card(&mut self, kind: board::Kind, cx: &mut Context<Self>) {
        let editing = self.editor.as_ref().map(|e| e.id.as_str());
        let after = match (kind, self.state.mission.as_ref()) {
            (board::Kind::Formation, Some(m)) => step_after(m, editing),
            // A Limit caps the step being edited, else the last step.
            (board::Kind::Limit, Some(m)) => editing
                .filter(|id| m.formations.iter().any(|f| f.id == *id))
                .map(str::to_owned)
                .or_else(|| m.formations.last().map(|f| f.id.clone())),
            _ => None,
        };
        self.place_card(kind, after.as_deref(), cx);
    }

    /// Add the step that comes after `card`, to its right, wired from it.
    fn next_step(&mut self, card: &str, cx: &mut Context<Self>) {
        self.place_card(board::Kind::Formation, Some(card), cx);
    }

    fn place_card(&mut self, kind: board::Kind, after: Option<&str>, cx: &mut Context<Self>) {
        let Some(mission) = self.state.mission.clone() else {
            return;
        };
        let layout = self
            .state
            .layout
            .as_ref()
            .map(|l| l.nodes.clone())
            .unwrap_or_default();
        let b = board::board(&mission, &flow::build(&mission), &layout, false);
        let size = board::new_size(kind);
        let beside = after.and_then(|id| b.cards.iter().find(|c| c.id == id));
        let room = super::notes::STICKY_ROOM;
        let spot = beside
            .and_then(|c| {
                let region = (c.x + c.w, c.y - size.1, size.0 * 3., size.1 * 3. + c.h);
                board::free_spot(&b, region, size, room)
            })
            .or_else(|| {
                self.canvas
                    .visible()
                    .and_then(|v| board::free_spot(&b, v, size, room))
            })
            .unwrap_or_else(|| {
                if b.cards.is_empty() {
                    (FIT_MARGIN, FIT_MARGIN)
                } else {
                    let (x, y, w, _) = board::extent(&b);
                    (x + w + FIT_MARGIN, y)
                }
            });
        self.canvas.show((spot.0, spot.1, size.0, size.1));
        let (x, y) = (spot.0 as i64, spot.1 as i64);
        let patch = place_patch(kind, after, x, y);
        if matches!(kind, board::Kind::Formation | board::Kind::End) {
            self.new_step = Some((kind, beside.and_then(|c| leaving_port(&mission, &c.id))));
        }
        self.write_board(patch, WriteMode::Edit, cx);
        if !self.writing {
            self.new_step = None;
        }
    }

    /// After a write: a step or End that was being added now exists, so
    /// wire it from the card it follows; a step opens for editing.
    pub(super) fn created_step(&mut self, before: &Mission, cx: &mut Context<Self>) {
        let Some((kind, from)) = self.new_step.take() else {
            return;
        };
        if kind == board::Kind::End {
            let end = self.state.mission.as_ref().and_then(|m| {
                m.ends
                    .iter()
                    .find(|e| !before.ends.iter().any(|b| b.id == e.id))
                    .map(|e| e.id.clone())
            });
            if let (Some(from), Some(end)) = (from, end) {
                self.canvas_wire(from, None, Some(format!("{end}:in")), cx);
            }
            return;
        }
        let Some(step) = self.state.mission.as_ref().and_then(|m| {
            m.formations
                .iter()
                .find(|f| !before.formations.iter().any(|b| b.id == f.id))
                .cloned()
        }) else {
            return;
        };
        // Open it before wiring: the editor waits for no write.
        self.edit_step(&step.id, cx);
        if let (Some(from), Some(port)) = (from, step.inputs.first()) {
            let to = format!("{}:{}", step.id, port.id);
            self.canvas_wire(from, None, Some(to), cx);
        }
    }

    /// The "+" beside a card: add the step that comes next.
    fn canvas_next(
        &self,
        t: &ahjo_app_kit::theme::Theme,
        card: &board::Card,
        view: &View,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let z = view.zoom();
        let side = size::CONTROL * z.max(ZOOM_MIN * 2.);
        let (x, y) = view.position(card.x + card.w, card.y);
        let id = card.id.clone();
        let end_id = card.id.clone();
        let selector = format!("canvas-next-{id}");
        let end_selector = format!("canvas-end-{id}");
        let next = div()
            .id(SharedString::from(selector.clone()))
            .debug_selector(move || selector.clone())
            .size(px(side))
            .rounded_full()
            .border_1()
            .border_color(t.accent)
            .bg(t.raised)
            .text_color(t.accent)
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(|d| d.bg(t.accent).text_color(t.bg))
            .tooltip(kit::tip("Add the next step"))
            .child("+")
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |page, _, _, cx| page.next_step(&id, cx)));
        // Ending the mission here is as near as adding a step.
        let end = div()
            .id(SharedString::from(end_selector.clone()))
            .debug_selector(move || end_selector.clone())
            .h(px(side))
            .px(px(TAB_GAP * z.max(ZOOM_MIN * 2.)))
            .rounded_full()
            .border_1()
            .border_color(t.divider)
            .bg(t.raised)
            .text_color(t.text2)
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(|d| d.border_color(t.accent).text_color(t.accent))
            .tooltip(kit::tip("End the mission here"))
            .child("End")
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |page, _, _, cx| {
                page.place_card(board::Kind::End, Some(&end_id), cx)
            }));
        div()
            .absolute()
            .left(px(x + GAP * z + board::PORT * z / 2.))
            .top(px(y))
            .flex()
            .flex_col()
            .items_start()
            .gap(px(TAB_GAP * z.max(ZOOM_MIN * 2.)))
            .child(next)
            .child(end)
            .into_any_element()
    }
}

/// The port a next step is wired from: an Input card's, a step's first
/// output, or a gate's pass.
/// The card a new step follows: the one being edited when a step can follow
/// it, else the last step nothing follows yet, else an Input nothing follows.
fn step_after(m: &Mission, editing: Option<&str>) -> Option<String> {
    let open = |id: &str| {
        leaving_port(m, id).is_some_and(|port| !m.connections.iter().any(|c| c.from == port))
    };
    editing
        .filter(|id| leaving_port(m, id).is_some())
        .map(str::to_owned)
        .or_else(|| {
            m.formations
                .iter()
                .rev()
                .find(|f| open(&f.id))
                .map(|f| f.id.clone())
        })
        .or_else(|| {
            m.input_cards
                .iter()
                .find(|c| open(&c.id))
                .map(|c| c.id.clone())
        })
}

fn leaving_port(m: &Mission, card: &str) -> Option<String> {
    if m.input_cards.iter().any(|c| c.id == card) {
        return Some(format!("{card}:out"));
    }
    if let Some(f) = m.formations.iter().find(|f| f.id == card) {
        return f.outputs.first().map(|p| format!("{card}:{}", p.id));
    }
    m.gates
        .iter()
        .any(|g| g.id == card)
        .then(|| format!("{card}:pass"))
}

fn wire_patch(from: &str, lifted: Option<&str>, to: Option<&str>) -> Option<MissionChange> {
    let from = from.to_owned();
    Some(match (lifted, to) {
        (None, None) => return None,
        (Some(old), Some(to)) if old == to => return None,
        (None, Some(to)) => MissionChange::WireConnection {
            id: String::new(),
            from,
            to: to.into(),
            join_if_occupied: true,
        },
        (Some(old), Some(to)) => MissionChange::RewireConnection {
            from,
            previous_to: old.into(),
            to: to.into(),
            join_if_occupied: true,
            remove_previous_input: false,
        },
        (Some(old), None) => MissionChange::UnwireConnection {
            from,
            to: old.into(),
        },
    })
}

fn new_card_items(
    x: i64,
    y: i64,
    show_input: bool,
) -> impl Iterator<Item = (&'static str, &'static str, MissionChange)> {
    [
        ("canvas-new-input", "Input card", new_input(x, y)),
        (
            "canvas-new-solo",
            "Solo formation",
            new_formation("solo", "New formation", x, y),
        ),
        (
            "canvas-new-orch",
            "Orchestrated formation",
            new_formation("orchestrated", "New formation", x, y),
        ),
        ("canvas-new-gate", "Gate", new_gate("Review gate", x, y)),
        ("canvas-new-limit", "Limit", new_limit("", x, y)),
        (
            "canvas-new-done",
            "End · done",
            new_end("Done", "done", x, y),
        ),
        (
            "canvas-new-rejected",
            "End · rejected",
            new_end("Rejected", "rejected", x, y),
        ),
    ]
    .into_iter()
    .skip(usize::from(!show_input))
}

fn place_patch(kind: board::Kind, after: Option<&str>, x: i64, y: i64) -> MissionChange {
    match kind {
        board::Kind::Input => new_input(x, y),
        board::Kind::Formation => new_formation("solo", "New step", x, y),
        board::Kind::Gate => new_gate("New gate", x, y),
        board::Kind::End => new_end("End", "done", x, y),
        board::Kind::Limit => new_limit(after.unwrap_or_default(), x, y),
    }
}

fn new_input(x: i64, y: i64) -> MissionChange {
    MissionChange::CreateInputCard {
        title: "Input".into(),
        goal: String::new(),
        files: vec![],
        human_channel: String::new(),
        x,
        y,
    }
}

fn new_formation(kind: &str, title: &str, x: i64, y: i64) -> MissionChange {
    MissionChange::CreateFormation {
        kind: kind.into(),
        title: title.into(),
        x,
        y,
    }
}

fn new_gate(title: &str, x: i64, y: i64) -> MissionChange {
    MissionChange::CreateGate {
        title: title.into(),
        kinds: vec!["human".into()],
        criterion: String::new(),
        check: String::new(),
        check_version: String::new(),
        check_value: String::new(),
        files: vec![],
        x,
        y,
    }
}

fn new_end(title: &str, outcome: &str, x: i64, y: i64) -> MissionChange {
    MissionChange::CreateEnd {
        title: title.into(),
        outcome: outcome.into(),
        x,
        y,
    }
}

// New limits start at three rounds and, when placed after a card, cover it.
fn new_limit(target: &str, x: i64, y: i64) -> MissionChange {
    MissionChange::CreateLimit {
        title: "Limit".into(),
        target: target.into(),
        rounds: Some(3),
        seconds: None,
        warn_seconds: None,
        tokens: None,
        when_spent: String::new(),
        x,
        y,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canvas_card_and_wire_patches_decode() {
        let menu: Vec<_> = new_card_items(10, 20, true).collect();
        assert_eq!(menu.len(), 7);
        for (_, _, patch) in menu {
            super::super::assert_patch_decodes(&patch);
        }
        for kind in [
            board::Kind::Input,
            board::Kind::Formation,
            board::Kind::Gate,
            board::Kind::End,
            board::Kind::Limit,
        ] {
            super::super::assert_patch_decodes(&place_patch(kind, Some("step"), 10, 20));
        }
        for (lifted, to) in [
            (None, Some("step:in")),
            (Some("step:in"), Some("end:in")),
            (Some("step:in"), None),
        ] {
            super::super::assert_patch_decodes(&wire_patch("input:out", lifted, to).unwrap());
        }
    }

    #[test]
    fn a_new_step_follows_the_edited_card_else_the_open_end_of_the_flow() {
        let mission: Mission = serde_json::from_value(json!({
            "inputCard": [{"id": "inp"}],
            "formation": [
                {"id": "a", "input": [{"id": "in"}], "output": [{"id": "out"}]},
                {"id": "b", "input": [{"id": "in"}], "output": [{"id": "out"}]}
            ],
            "end": [{"id": "end", "outcome": "done"}],
            "connection": [
                {"id": "e1", "from": "inp:out", "to": "a:in"},
                {"id": "e2", "from": "a:out", "to": "b:in"}
            ]
        }))
        .unwrap();
        assert_eq!(step_after(&mission, Some("a")).as_deref(), Some("a"));
        assert_eq!(step_after(&mission, Some("end")).as_deref(), Some("b"));
        assert_eq!(step_after(&mission, None).as_deref(), Some("b"));
        let fresh: Mission = serde_json::from_value(json!({"inputCard": [{"id": "inp"}]})).unwrap();
        assert_eq!(step_after(&fresh, None).as_deref(), Some("inp"));
    }
}
