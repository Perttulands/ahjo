//! Mission canvas interaction, adapted from the original Missions page.
use super::*;
use ahjo_app_kit::widgets::boundary;
use ahjo_missions_contract::{
    Effort, EndOutcome, Formation, GateKind, HumanChannel, LayoutCard, Mission, SeatSettings,
    WhenSpent,
};
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
const READABLE: f32 = 0.8;
const DRAG_SLOP: f32 = 3.;
const CURVE_REACH: f32 = 40.;
const CURVE_DOWN: f32 = 60.;
const DASH_LONG: f32 = 5.;
const TITLE_SIZE: f32 = 14.;
/// A card's words at world size: at the zoom a board opens at, never below
/// 0.8, its small words still read and its text reads at the page's size.
pub(super) const CARD_SMALL: f32 = 11.5;
pub(super) const CARD_TEXT: f32 = 13.;
/// Archon's dot grid, in the board's space: a dot every `DOT`, a faint line
/// every `GRID`, drawn in the text colour at these strengths.
const DOT: f32 = 28.;
const GRID: f32 = 112.;
const DOT_ALPHA: f32 = 0.09;
const GRID_ALPHA: f32 = 0.025;
const DOT_SIZE: f32 = 1.5;
/// The zoom controls' stack in the board's corner, and what Fit keeps clear.
const ZOOM_BUTTON: (f32, f32) = (76., 32.);
const WIRE: f32 = 1.5;
/// A card's inner padding across and down, at Archon's density.
const CARD_PAD: f32 = board::CARD_PAD;
const HEAD_PAD: f32 = 8.;
/// A step's number in its ring.
const GAP: f32 = 8.;
/// A finding's mark beside a control's size.
const BADGE_SCALE: f32 = 0.75;
const SEAT_PAD: f32 = 10.;
const END_MARK: f32 = 14.;
/// The dot a fork's line splits at.
const FORK_DOT_SIZE: f32 = 9.;
/// A seat's mark: a harness's, or an empty seat's ring.
const SEAT: f32 = 16.;
/// The room a loop's words are centred in, along its lane.
const LOOP_WORDS: f32 = 320.;
/// A line of a loop's words, for keeping one loop's off another's.
const LOOP_LINE: f32 = 17.;
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
    /// The window's size when the board was last drawn.
    window: Rc<Cell<(f32, f32)>>,
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
            window: Rc::new(Cell::new((0., 0.))),
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
    /// The board's room changed by the operator's own choice (Flow shown or
    /// hidden): framed again for it, unless the view was moved by hand.
    pub(super) fn room_changed(&mut self) {
        if !self.touched {
            self.reopen();
        }
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
        let (x, y, bw, bh) = shown_extent(board);
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
        let (x, y, bw, bh) = shown_extent(board);
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

/// What a fit frames: the board, and under its lowest cards the room a
/// loop's words or the start's take.
fn shown_extent(board: &board::Board) -> (f32, f32, f32, f32) {
    let (x, y, w, h) = board::extent(board);
    (x, y, w, h + board::START_WORDS)
}

impl MissionsPage {
    /// The board as drawn: at its saved places, or until it has any, in
    /// reading rows shaped for the room it has, each card keeping room
    /// under it for its notes.
    pub(super) fn shown_board(&self, mission: &Mission, running: bool) -> board::Board {
        let flow = flow::build(mission);
        // Places nobody chose by hand (Arrange's own, or none) give way to
        // the reading rows for this room.
        let mut layout = match &self.state.layout {
            Some(l) if !self.state.layout_arranged => l.cards.clone(),
            _ => vec![],
        };
        // Cards the saved places miss stand where the reading rows put
        // them.
        let missing = |id: &str| !layout.iter().any(|p| p.id == id);
        let ids = mission
            .input_cards
            .iter()
            .map(|c| &c.id)
            .chain(mission.steps.iter().map(|f| &f.id))
            .chain(mission.gates.iter().map(|g| &g.id))
            .chain(mission.ends.iter().map(|e| &e.id));
        if ids.into_iter().any(|id| missing(id)) {
            // A Limit card stands under what it caps; on a board laid out
            // whole, where the rows leave it room.
            let whole = layout.is_empty();
            let shaped: Vec<LayoutCard> = self
                .shaped_layout(mission)
                .into_iter()
                .filter(|p| {
                    missing(&p.id) && (whole || !mission.limits.iter().any(|l| l.id == p.id))
                })
                .collect();
            layout.extend(shaped);
        }
        if let Some(Drag::Card { id, now, .. }) = &self.canvas.drag {
            layout.retain(|p| p.id != *id);
            layout.push(LayoutCard {
                id: id.clone(),
                x: now.0.round() as i64,
                y: now.1.round() as i64,
            });
        }
        board::board(mission, &flow, &layout, running)
    }
    /// The mission in reading rows for the board's room: how it opens
    /// until arranged, and Arrange.
    pub(super) fn shaped_layout(&self, mission: &Mission) -> Vec<LayoutCard> {
        // The start keeps room under it for what a run takes.
        board::shaped(
            mission,
            &flow::build(mission),
            self.canvas.room(),
            READABLE,
            &|id| {
                if mission.input_cards.iter().any(|c| c.id == id) {
                    board::START_WORDS
                } else {
                    0.
                }
            },
        )
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
            painted(mission, &board),
            view.clone(),
            cx.weak_entity(),
            self.generation,
            &findings,
            Fitting {
                ready: running || self.state.layout.is_some(),
                placed: running
                    || (!self.state.layout_arranged
                        && self
                            .state
                            .layout
                            .as_ref()
                            .is_some_and(|l| !l.cards.is_empty())),
                running,
            },
        ));
        // A loop back says, along its lane, how often it goes round and
        // what then.
        let said = loop_places(
            board
                .wires
                .iter()
                .filter(|w| w.back)
                .filter_map(|w| Some((w.id.clone(), loop_words(mission, w)?, w.label?)))
                .collect(),
        );
        for (id, words, label) in said {
            let z = view.zoom;
            let (x, y) = view.position(label.0, label.1);
            let selector = format!("canvas-loop-{id}");
            layer = layer.child(
                div()
                    .absolute()
                    .left(px(x - LOOP_WORDS / 2. * z))
                    .w(px(LOOP_WORDS * z))
                    .top(px(y + TAB_GAP * z))
                    .flex()
                    .justify_center()
                    .debug_selector(move || selector.clone())
                    .text_size(px(CARD_SMALL * z))
                    .text_color(t.text2)
                    .whitespace_nowrap()
                    .child(words),
            );
        }
        // Under the start: what a run takes, and the mission's cap.
        for card in board.cards.iter().filter(|c| c.kind == board::Kind::Input) {
            let Some(c) = mission.input_cards.iter().find(|c| c.id == card.id) else {
                continue;
            };
            let z = view.zoom;
            let (x, y) = view.position(card.x, card.y + card.h);
            let names: Vec<_> = c.inputs.iter().map(|i| i.name.clone()).collect();
            let id = card.id.clone();
            layer = layer.child(
                div()
                    .id(SharedString::from(format!("canvas-start-{id}")))
                    .absolute()
                    .left(px(x))
                    .top(px(y + TAB_GAP * z))
                    .flex()
                    .flex_col()
                    .text_size(px(CARD_SMALL * z))
                    .text_color(t.text2)
                    .whitespace_nowrap()
                    .cursor_pointer()
                    .children(names)
                    .children(cap_of(mission, &card.id))
                    .when(!running, |d| {
                        d.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(move |p, _, _, cx| p.edit_step(&id, cx)))
                    }),
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
            if self.todo_open {
                layer = layer.children(self.card_badge(t, mission, card, &view, &findings));
            }
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
        if !running && mission.input_cards.is_empty() && mission.steps.is_empty() {
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
        for note in self.canvas_notes(t, mission, &board, &view, cx) {
            layer = layer.child(note);
        }
        if !running {
            for port in &ports {
                layer = layer.child(self.canvas_port(t, mission, port, &view, cx));
            }
        }
        for edge in mission.connections.iter().filter(|_| self.todo_open) {
            let matching: Vec<_> = findings
                .iter()
                .filter(|(finding, _)| finding.card_id == edge.id)
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
                .map(board::Wire::middle)
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
        // The board's own controls, in one quiet row in its corner: out,
        // the zoom, in, and Fit.
        let (_, bh) = ZOOM_BUTTON;
        let button =
            |id: &'static str, selector: &'static str, text: &'static str, tip: &'static str| {
                div()
                    .id(id)
                    .debug_selector(move || selector.into())
                    .h(px(bh))
                    .px(px(GAP + EDGE))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(size::SMALL))
                    .text_color(t.text2)
                    .cursor_pointer()
                    .hover(|d| d.bg(t.raised).text_color(t.text))
                    .tooltip(kit::tip(tip))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(text)
            };
        let controls = div()
            .absolute()
            .right(px(PAD))
            .bottom(px(PAD))
            .flex()
            .items_center()
            .border_1()
            .border_color(t.divider)
            .rounded(px(size::RADIUS))
            .overflow_hidden()
            .bg(t.surface)
            .child(
                button("canvas-out", "canvas-out", "−", "Zoom out (Ctrl+-)").on_click(cx.listener(
                    |p, _, _, cx| {
                        p.canvas.step(false);
                        cx.notify();
                    },
                )),
            )
            .child(
                div()
                    .debug_selector(|| "canvas-zoom".into())
                    .w(px(ZOOM_BUTTON.0 / 1.5))
                    .flex()
                    .justify_center()
                    .text_size(px(size::SMALL))
                    .text_color(t.dim)
                    .child(format!("{:.0}%", view.zoom * 100.)),
            )
            .child(
                button("canvas-in", "canvas-in", "+", "Zoom in (Ctrl+=)").on_click(cx.listener(
                    |p, _, _, cx| {
                        p.canvas.step(true);
                        cx.notify();
                    },
                )),
            )
            .child(
                button(
                    "canvas-fit",
                    "canvas-fit",
                    "Fit",
                    "Show the whole mission (Ctrl+0: back to how it opened)",
                )
                .border_l_1()
                .border_color(t.divider)
                .on_click(cx.listener(move |p, _, _, cx| {
                    // The view is the operator's again: it stops following an
                    // edited card and frames each change.
                    p.canvas.follow = None;
                    p.canvas.touched = false;
                    p.canvas.fit(&fit);
                    cx.notify();
                })),
            );
        if let Some(id) = &self.reveal_card {
            let selector = format!("mission-reveal-{id}");
            layer = layer.child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .debug_selector(move || selector.clone())
                    .child(format!("Card: {}", mission.card_name(id))),
            );
        }
        div().size_full().relative().child(layer).child(controls)
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
            .filter(|(finding, _)| finding.card_id == card.id)
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
                // Above the card's near corner: clear of its ports, its
                // header and the note marker on its far corner.
                .left(px(x))
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
        state: Option<crate::store::run_state::CardState>,
        attempts: Option<i64>,
        findings: &[(ahjo_missions_contract::Finding, bool)],
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let z = view.zoom;
        let id = card.id.clone();
        let from = (card.x, card.y);
        let selector = format!("mission-card-{id}");
        // A gate's judge has no card of its own: its seat on the gate opens
        // it.
        let judge = (!running && card.kind == board::Kind::Gate)
            .then(|| {
                flow::build(mission)
                    .judge_of
                    .into_iter()
                    .find(|(_, of)| *of == id)
                    .map(|(judge, _)| judge)
            })
            .flatten()
            .map(|judge| -> OnJudge {
                Rc::new(cx.listener(move |p, _: &gpui::ClickEvent, _, cx| {
                    cx.stop_propagation();
                    p.edit_step(&judge, cx);
                }))
            });
        let face = card_face(
            t,
            mission,
            card,
            z,
            running.then_some(state),
            judge,
            self.agents.as_ref(),
        );
        let issues: Vec<_> = findings
            .iter()
            .filter(|(finding, _)| finding.card_id == id)
            .collect();
        let issue_color = self.finding_color(t, issues.iter().any(|(_, error)| *error));
        let color = match state {
            Some(crate::store::run_state::CardState::Done) => t.done,
            Some(
                crate::store::run_state::CardState::Failed
                | crate::store::run_state::CardState::Blocked,
            ) => t.error,
            Some(crate::store::run_state::CardState::Running) => t.running,
            Some(crate::store::run_state::CardState::Waiting) => t.needs,
            _ if card.kind == board::Kind::End => {
                if mission
                    .ends
                    .iter()
                    .any(|e| e.id == card.id && e.outcome == EndOutcome::Rejected)
                {
                    t.error
                } else {
                    t.done
                }
            }
            _ => t.divider,
        };
        // Only a step is drawn as a card; a fork is painted with the wires,
        // the start is its play control and an end is its mark.
        let boxed = card.kind == board::Kind::Step;
        let marked = self.reveal_card.as_deref() == Some(&id)
            || self.flow_focus.as_deref() == Some(&id)
            || self.activity_marks.contains(&id);
        let edge = if !issues.is_empty() && self.todo_open {
            issue_color
        } else if marked {
            t.accent
        } else {
            color
        };
        let r = px(size::RADIUS * z);
        let rendered = div()
            .id(SharedString::from(format!("card-{id}")))
            .absolute()
            .left(px(card.x * z + view.pan.0))
            .top(px(card.y * z + view.pan.1))
            .w(px(card.w * z))
            .h(px(card.h * z))
            .overflow_hidden()
            .when(boxed, |d| {
                d.border_1()
                    .border_color(edge)
                    .bg(t.surface)
                    .shadow_sm()
                    .rounded(r)
            })
            .when(marked && boxed, |d| d.border_2())
            .when(card.kind == board::Kind::Input, |d| d.rounded_full())
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
                    .size_full()
                    .rounded_full()
                    .border_1()
                    .border_color(if runnable { t.accent } else { t.divider })
                    .bg(if runnable { t.accent } else { t.surface })
                    .text_color(if runnable { t.on_accent() } else { t.text2 })
                    .text_size(px(CARD_TEXT * z))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .tooltip(kit::tip(if runnable {
                        "Start a run"
                    } else {
                        "Start a run (it needs work first)"
                    }))
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
                .steps
                .iter()
                .find(|f| f.id == step)
                .map(|_| board::STEP_H - size::CONTROL)
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
        let before = LayoutCard {
            id: id.into(),
            x: from.0.round() as i64,
            y: from.1.round() as i64,
        };
        let after = LayoutCard {
            id: id.into(),
            x: to.0.round() as i64,
            y: to.1.round() as i64,
        };
        // A board laid out for the room it is shown in (never placed by
        // hand, or not saved in full): the first move keeps every card
        // where it now stands.
        let saved = self.state.layout.as_ref().map(|l| &l.cards);
        let unsaved = |c: &board::Card| !saved.is_some_and(|n| n.iter().any(|p| p.id == c.id));
        let rest: Vec<LayoutCard> = match self.state.mission.as_ref() {
            Some(m) => {
                let board = self.shown_board(m, false);
                if self.state.layout_arranged || board.cards.iter().any(unsaved) {
                    board
                        .cards
                        .iter()
                        .filter(|c| c.id != id)
                        .map(|c| LayoutCard {
                            id: c.id.clone(),
                            x: c.x.round() as i64,
                            y: c.y.round() as i64,
                        })
                        .collect()
                } else {
                    vec![]
                }
            }
            None => vec![],
        };
        let mut nodes = rest.clone();
        nodes.push(after);
        let mut reverse = rest;
        reverse.push(before);
        self.canvas_layout(
            EditLayout {
                cards: Some(nodes),
                ..Default::default()
            },
            reverse,
            false,
            cx,
        );
    }
    /// Save a change to the board's places (`{"nodes":…}`, or
    /// `{"arrange":true}` for Arrange's own), undone by `reverse`.
    pub(super) fn canvas_layout(
        &mut self,
        change: EditLayout,
        reverse: Vec<LayoutCard>,
        reframe: bool,
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
        self.writing = true;
        let asked = self.host.call(
            NodeId(node),
            LayoutEdit {
                mission: slug,
                etag: Some(etag),
                change,
            },
            cx,
        );
        cx.spawn(async move |this, cx| {
            let result = asked.await;
            let _ = this.update(cx, |p, cx| {
                if p.generation != generation {
                    return;
                }
                p.writing = false;
                match result {
                    Ok(v) => {
                        let etag = v.etag.clone();
                        // A move by hand keeps its places.
                        p.state.set_layout(v.layout, v.etag, reframe);
                        if reframe {
                            p.canvas.reopen();
                        }
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

/// When the board is fitted to its room: once its layout is in (`ready`),
/// reshaped on a resize unless someone `placed` its cards, and shown as a
/// run shows it while `running`.
#[derive(Clone, Copy)]
struct Fitting {
    ready: bool,
    placed: bool,
    running: bool,
}

/// A gate the wires' layer paints: a fork's line and dot. Its card,
/// whether it waits on the operator, and whether its pass and fail are
/// wired.
#[derive(Clone)]
struct Painted {
    card: board::Card,
    human: bool,
    pass: bool,
    fail: bool,
}

fn painted(mission: &Mission, board: &board::Board) -> Vec<Painted> {
    let wired = |e: String| mission.connections.iter().any(|c| c.from == e);
    board
        .cards
        .iter()
        .filter(|c| c.kind == board::Kind::Gate)
        .map(|c| Painted {
            card: c.clone(),
            human: mission
                .gates
                .iter()
                .any(|g| g.id == c.id && g.kinds.contains(&GateKind::Human)),
            pass: wired(format!("{}:pass", c.id)),
            fail: wired(format!("{}:fail", c.id)),
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn wires(
    t: &ahjo_app_kit::theme::Theme,
    board: &board::Board,
    gates: Vec<Painted>,
    view: View,
    entity: gpui::WeakEntity<MissionsPage>,
    generation: u64,
    findings: &[(ahjo_missions_contract::Finding, bool)],
    fitting: Fitting,
) -> impl IntoElement {
    let Fitting {
        ready: ready_to_fit,
        placed,
        running,
    } = fitting;
    let list = board.wires.clone();
    let errors: std::collections::HashSet<_> = findings
        .iter()
        .filter(|(_, error)| *error)
        .map(|(finding, _)| finding.card_id.clone())
        .collect();
    let accent = t.accent;
    let (work, pass, fail, quiet, error) = (t.wire, t.pass, t.fail, t.dim, t.error);
    let (needs, mark) = (t.needs, t.text2);
    let grid = (
        ahjo_app_kit::theme::alpha(t.text, DOT_ALPHA),
        ahjo_app_kit::theme::alpha(t.text, GRID_ALPHA),
    );
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
            // A board nobody placed is laid out for the room it has, so a
            // new window size lays it out and frames it again. Placed, it
            // stays put; and what comes and goes around the board in the
            // same window (a panel, the to-do under it) moves no card.
            let size = window.viewport_size();
            let now = (f32::from(size.width), f32::from(size.height));
            let was = view.window.replace(now);
            if was != now && was.0 > 0. && !placed {
                let entity = entity.clone();
                cx.defer(move |cx| {
                    let _ = entity.update(cx, |page, cx| {
                        page.canvas.reopen();
                        cx.notify();
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
                cx.defer(move |cx| {
                    let _ = entity.update(cx, |page, cx| {
                        if page.generation == generation && !page.canvas.fitted {
                            // Laid out for the board as it now is, and
                            // framed so.
                            if let Some(m) = page.state.mission.clone() {
                                let board = page.shown_board(&m, running);
                                page.canvas.open(&board, READABLE, false);
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
            paint_grid(window, b, &view, grid);
            for g in &gates {
                paint_gate(window, g, &at, z, (work, pass, fail, quiet), (needs, mark));
            }
            for w in &list {
                let colour = if errors.contains(&w.id) {
                    error
                } else {
                    // A fail is red all the way, back or out; a pass is
                    // green where the gate splits, and the way on after.
                    match w.route.as_str() {
                        "fail" => fail,
                        _ => work,
                    }
                };
                let mut path = PathBuilder::stroke(px(WIRE));
                // A loop back is dashed.
                if w.back {
                    path = path.dash_array(&[px(DASH_LONG * 1.2), px(DASH_LONG)]);
                }
                let points: Vec<_> = w.points.iter().map(|p| at(*p)).collect();
                rounded(&mut path, &points, px(board::CORNER * z));
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

/// A fork: the line through it, splitting at its dot into the pass going on
/// and the fail dropping; the dot amber when the operator answers.
fn paint_gate(
    window: &mut Window,
    g: &Painted,
    at: &dyn Fn((f32, f32)) -> Point<Pixels>,
    z: f32,
    (work, pass, fail, quiet): (Hsla, Hsla, Hsla, Hsla),
    (needs, mark): (Hsla, Hsla),
) {
    let c = &g.card;
    let line = |points: &[(f32, f32)], colour: Hsla, window: &mut Window| {
        let mut path = PathBuilder::stroke(px(WIRE));
        let points: Vec<_> = points.iter().map(|p| at(*p)).collect();
        rounded(&mut path, &points, px(0.));
        if let Ok(path) = path.build() {
            window.paint_path(path, colour);
        }
    };
    let y = c.y + board::fork_line(c.h);
    let dot = c.x + board::FORK_DOT;
    line(&[(c.x, y), (dot, y)], work, window);
    line(
        &[(dot, y), (c.x + c.w, y)],
        if g.pass { pass } else { quiet },
        window,
    );
    if g.fail {
        line(&[(dot, y), (dot, c.y + c.h)], fail, window);
    }
    let r = FORK_DOT_SIZE / 2. * z;
    let centre = at((dot, y));
    window.paint_quad(
        gpui::fill(
            Bounds::new(
                point(centre.x - px(r), centre.y - px(r)),
                gpui::size(px(2. * r), px(2. * r)),
            ),
            if g.human { needs } else { mark },
        )
        .corner_radii(px(r)),
    );
}

/// A course of straight runs, its corners rounded to `radius` (less where a
/// run is too short for it).
fn rounded(path: &mut PathBuilder, points: &[Point<Pixels>], radius: Pixels) {
    let Some(first) = points.first() else { return };
    path.move_to(*first);
    for i in 1..points.len().saturating_sub(1) {
        let (a, p, b) = (points[i - 1], points[i], points[i + 1]);
        let len =
            |u: Point<Pixels>, v: Point<Pixels>| f32::from(u.x - v.x).hypot(f32::from(u.y - v.y));
        let (into, out) = (len(a, p), len(p, b));
        if into < 0.5 || out < 0.5 {
            continue;
        }
        let r = f32::from(radius).min(into / 2.).min(out / 2.);
        let toward = |from: Point<Pixels>, to: Point<Pixels>, d: f32, len: f32| {
            point(
                from.x + (to.x - from.x) * (d / len),
                from.y + (to.y - from.y) * (d / len),
            )
        };
        path.line_to(toward(p, a, r, into));
        path.curve_to(toward(p, b, r, out), p);
    }
    if let Some(last) = points.last().filter(|_| points.len() > 1) {
        path.line_to(*last);
    }
}

/// Archon's grid under the board: faint lines every `GRID` and a dot every
/// `DOT`, in the board's space so they pan and zoom with it.
fn paint_grid(window: &mut Window, b: Bounds<Pixels>, view: &View, (dot, line): (Hsla, Hsla)) {
    let z = view.zoom;
    let (w, h) = (f32::from(b.size.width), f32::from(b.size.height));
    let first = |pan: f32, step: f32| (pan.rem_euclid(step * z)) - step * z;
    let mut x = first(view.pan.0, GRID);
    while x < w {
        window.paint_quad(gpui::fill(
            Bounds::new(
                point(b.origin.x + px(x), b.origin.y),
                gpui::size(px(1.), b.size.height),
            ),
            line,
        ));
        x += GRID * z;
    }
    let mut y = first(view.pan.1, GRID);
    while y < h {
        window.paint_quad(gpui::fill(
            Bounds::new(
                point(b.origin.x, b.origin.y + px(y)),
                gpui::size(b.size.width, px(1.)),
            ),
            line,
        ));
        y += GRID * z;
    }
    // Too far out, the dots would be a haze: the lines alone then.
    if DOT * z < 10. {
        return;
    }
    let d = DOT_SIZE.max(1.);
    let mut y = first(view.pan.1, DOT);
    while y < h {
        let mut x = first(view.pan.0, DOT);
        while x < w {
            window.paint_quad(
                gpui::fill(
                    Bounds::new(
                        point(b.origin.x + px(x - d / 2.), b.origin.y + px(y - d / 2.)),
                        gpui::size(px(d), px(d)),
                    ),
                    dot,
                )
                .corner_radii(px(d / 2.)),
            );
            x += DOT * z;
        }
        y += DOT * z;
    }
}

/// What choosing a gate's judge does.
type OnJudge = Rc<dyn Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App)>;
/// The node's agents, which give a seat the settings it leaves to them.
type Catalog<'a> = Option<&'a ahjo_harness_contract::agents::ListedAgents>;

/// What a seat runs on once its agent's settings fill what it leaves out.
fn settings(seat: &ahjo_missions_contract::Seat, catalog: Catalog) -> SeatSettings {
    seat.settings(catalog.and_then(|c| c.agents.iter().find(|a| a.id == seat.agent)))
}

/// What a card says. A step: its number, its name, and who works it. The
/// start: a play control. A gate: its question, what it asks, and who
/// answers. An end: its name.
fn card_face(
    t: &ahjo_app_kit::theme::Theme,
    mission: &Mission,
    card: &board::Card,
    z: f32,
    run: Option<Option<crate::store::run_state::CardState>>,
    on_judge: Option<OnJudge>,
    catalog: Catalog,
) -> Div {
    match card.kind {
        board::Kind::Step => {
            let Some(f) = mission.steps.iter().find(|f| f.id == card.id) else {
                return div();
            };
            step_face(t, mission, f, z, run, catalog)
        }
        // The start is its play control, drawn by the card itself.
        board::Kind::Input | board::Kind::Limit => div(),
        board::Kind::Gate => {
            let Some(g) = mission.gates.iter().find(|g| g.id == card.id) else {
                return div();
            };
            gate_face(t, mission, g, card, z, on_judge, catalog)
        }
        board::Kind::End => {
            let Some(e) = mission.ends.iter().find(|e| e.id == card.id) else {
                return div();
            };
            let colour = if e.outcome == EndOutcome::Rejected {
                t.fail
            } else {
                t.pass
            };
            div()
                .size_full()
                .flex()
                .items_center()
                .gap(px(GAP * z))
                .pl(px(board::PORT * z))
                .child(
                    div()
                        .size(px(END_MARK * z))
                        .flex_none()
                        .rounded_full()
                        .border_2()
                        .border_color(colour),
                )
                .child(
                    div()
                        .text_size(px(CARD_TEXT * z))
                        .text_color(t.text)
                        .truncate()
                        .child(e.title.clone()),
                )
        }
    }
}

fn step_face(
    t: &ahjo_app_kit::theme::Theme,
    mission: &Mission,
    f: &ahjo_missions_contract::Step,
    z: f32,
    run: Option<Option<crate::store::run_state::CardState>>,
    catalog: Catalog,
) -> Div {
    let number = flow::build(mission).numbers.get(&f.id).copied();
    let top = div()
        .flex()
        .items_baseline()
        .gap(px(GAP * z))
        .when_some(number, |d, n| {
            d.child(
                div()
                    .flex_none()
                    .text_size(px(CARD_SMALL * z))
                    .text_color(t.dim)
                    .child(n.to_string()),
            )
        })
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(TITLE_SIZE * z))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.text)
                .child(f.title.clone()),
        )
        .children(cap_of(mission, &f.id).map(|cap| {
            div()
                .flex_none()
                .text_size(px(CARD_SMALL * z))
                .text_color(t.text2)
                .child(cap)
        }));
    // An orchestrated step's lead over its workers; peers all on one row.
    let lead: Vec<_> = match f.formation {
        Formation::Orchestrated => f.seats.iter().filter(|s| s.controller).collect(),
        Formation::Peer => vec![],
        _ => f.seats.iter().collect(),
    };
    let mut who = div().flex().items_center().gap(px(SEAT_PAD * z)).min_w_0();
    for s in &lead {
        who = who.child(seat(t, s, z, catalog));
    }
    if lead.is_empty() && f.formation != Formation::Peer {
        who = who.child(empty_seat(t, z));
    }
    let mut face = div()
        .size_full()
        .flex()
        .flex_col()
        .justify_center()
        .gap(px(HEAD_PAD * z))
        .px(px(CARD_PAD * z))
        .child(top)
        .child(who);
    if board::pooled(f.formation) {
        let crew: Vec<_> = f.seats.iter().filter(|s| !s.controller).collect();
        face = face.child(crew_row(t, &crew, f.formation, z, catalog));
    }
    if let Some(state) = run {
        let (word, colour) = state_words(t, state);
        face = face.child(
            div()
                .flex()
                .items_center()
                .gap(px(TAB_GAP * z))
                .text_size(px(CARD_SMALL * z))
                .text_color(colour)
                .child(div().size(px(TAB_GAP * z)).rounded_full().bg(colour))
                .child(word),
        );
    }
    face
}

/// One seat: its harness's mark, the agent's name, and its effort where it
/// is not the usual.
fn seat(
    t: &ahjo_app_kit::theme::Theme,
    s: &ahjo_missions_contract::Seat,
    z: f32,
    catalog: Catalog,
) -> Div {
    let settings = settings(s, catalog);
    if settings.harness.is_unset() && s.agent.is_empty() {
        return empty_seat(t, z);
    }
    let selector = format!(
        "canvas-seat-{}-{}-{}",
        crate::store::words::seat_label(s),
        crate::store::words::agent_name(&s.agent),
        crate::store::words::harness_name(settings.harness.as_str())
    );
    div()
        .debug_selector(move || selector.clone())
        .flex()
        .items_center()
        .gap(px(TAB_GAP * z))
        .min_w_0()
        .text_size(px(CARD_SMALL * z))
        .text_color(t.text2)
        .children(harness_mark(settings.harness.as_str(), t.text, t.text2, z))
        .child(div().truncate().child(seat_name(s)))
        .when(
            !matches!(settings.effort, Effort::Unset | Effort::Medium),
            |d| {
                d.child(
                    div()
                        .flex_none()
                        .text_color(t.dim)
                        .child(settings.effort.as_str()),
                )
            },
        )
}

fn seat_name(s: &ahjo_missions_contract::Seat) -> String {
    if s.agent.trim().is_empty() {
        crate::store::words::seat_label(s).to_owned()
    } else {
        crate::store::words::agent_name(&s.agent)
    }
}

/// A seat nobody works yet: Archon's dashed ring with a plus.
fn empty_seat(t: &ahjo_app_kit::theme::Theme, z: f32) -> Div {
    div()
        .size(px(SEAT * z))
        .flex_none()
        .rounded_full()
        .border_1()
        .border_dashed()
        .border_color(t.dim)
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(CARD_SMALL * z))
        .text_color(t.dim)
        .child("+")
}

/// An orchestrated step's workers, under its lead: their marks stacked, and
/// who they are.
fn crew_row(
    t: &ahjo_app_kit::theme::Theme,
    crew: &[&ahjo_missions_contract::Seat],
    kind: Formation,
    z: f32,
    catalog: Catalog,
) -> Div {
    let mut marks = div().flex().items_center();
    for (i, s) in crew.iter().enumerate() {
        let harness = settings(s, catalog).harness;
        let mark = if harness.is_unset() {
            empty_seat(t, z)
        } else {
            div()
                .size(px(SEAT * z))
                .flex_none()
                .rounded_full()
                .bg(t.raised)
                .border_1()
                .border_color(t.surface)
                .flex()
                .items_center()
                .justify_center()
                .children(harness_mark(
                    harness.as_str(),
                    t.text,
                    t.text2,
                    z * BADGE_SCALE,
                ))
        };
        marks = marks.child(mark.when(i > 0, |d| d.ml(px(-SEAT / 3. * z))));
    }
    let names: Vec<String> = crew.iter().map(|s| seat_name(s)).collect();
    let same = names.windows(2).all(|w| w[0] == w[1]);
    let words = match (names.first(), same) {
        (Some(name), true) if names.len() > 1 => format!("{} × {name}", names.len()),
        _ => names.join(", "),
    };
    div()
        .flex()
        .items_center()
        .gap(px(TAB_GAP * z))
        .pl(px(if kind == Formation::Peer {
            0.
        } else {
            CARD_PAD * z
        }))
        .text_size(px(CARD_SMALL * z))
        .text_color(t.text2)
        .child(marks)
        .child(div().truncate().child(words))
}

/// A step's or a gate's cap in time or tokens, said where it applies.
fn cap_of(mission: &Mission, id: &str) -> Option<String> {
    let caps: Vec<String> = mission
        .limits
        .iter()
        .filter(|l| l.scope == id)
        .flat_map(|l| {
            l.seconds
                .map(|s| format!("⏱ {}", crate::store::words::span(s)))
                .into_iter()
                .chain(l.tokens.map(|n| format!("{}k tokens", n / 1000)))
        })
        .collect();
    (!caps.is_empty()).then(|| caps.join(" · "))
}

/// Who answers a gate: its judge's seat, or You.
fn answerer(
    t: &ahjo_app_kit::theme::Theme,
    mission: &Mission,
    gate: &str,
    z: f32,
    on_judge: Option<OnJudge>,
    catalog: Catalog,
) -> Div {
    let flow = flow::build(mission);
    let g = mission.gates.iter().find(|g| g.id == gate);
    let judge = flow
        .judge_of
        .iter()
        .find(|(_, of)| of.as_str() == gate)
        .and_then(|(judge, _)| mission.steps.iter().find(|f| f.id == *judge));
    let mut who = div().flex().items_center().gap(px(SEAT_PAD * z)).min_w_0();
    let judged = match judge.and_then(|f| f.seats.first()) {
        Some(s) => Some(seat(t, s, z, catalog)),
        None if g.is_some_and(|g| g.kinds.contains(&GateKind::Step)) => Some(empty_seat(t, z)),
        None => None,
    };
    if let Some(judged) = judged {
        who = match on_judge {
            Some(open) => {
                let selector = format!("canvas-judge-{gate}");
                who.child(
                    div()
                        .id(SharedString::from(selector.clone()))
                        .debug_selector(move || selector.clone())
                        .min_w_0()
                        .cursor_pointer()
                        .rounded(px(size::RADIUS * z))
                        .hover(|d| d.bg(t.raised))
                        .tooltip(kit::tip("Edit the judge"))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(move |e, window, cx| open(e, window, cx))
                        .child(judged),
                )
            }
            None => who.child(judged),
        };
    }
    if g.is_some_and(|g| g.kinds.contains(&GateKind::Code)) {
        who = who.child(
            div()
                .text_size(px(CARD_SMALL * z))
                .text_color(t.text2)
                .child(">_ check"),
        );
    }
    if g.is_some_and(|g| g.kinds.contains(&GateKind::Human)) {
        who = who.child(
            div()
                .flex()
                .items_center()
                .gap(px(TAB_GAP * z))
                .text_size(px(CARD_SMALL * z))
                .text_color(t.needs)
                .child(
                    div()
                        .size(px(SEAT * z * BADGE_SCALE))
                        .rounded_full()
                        .border_2()
                        .border_color(t.needs),
                )
                .child("You"),
        );
    }
    if let Some(cap) = cap_of(mission, gate) {
        who = who.child(
            div()
                .text_size(px(CARD_SMALL * z))
                .text_color(t.text2)
                .child(cap),
        );
    }
    who
}

fn gate_face(
    t: &ahjo_app_kit::theme::Theme,
    mission: &Mission,
    g: &ahjo_missions_contract::Gate,
    card: &board::Card,
    z: f32,
    on_judge: Option<OnJudge>,
    catalog: Catalog,
) -> Div {
    // Numbered like the steps, so the count has no gap where a gate is.
    let number = flow::build(mission).numbers.get(&g.id).copied();
    let title = div()
        .min_w_0()
        .flex()
        .items_baseline()
        .gap(px(GAP * z))
        .when_some(number, |d, n| {
            d.child(
                div()
                    .flex_none()
                    .text_size(px(CARD_SMALL * z))
                    .text_color(t.dim)
                    .child(n.to_string()),
            )
        })
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_size(px(CARD_TEXT * z))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.text)
                .child(g.title.clone()),
        );
    // The condition is the decision: it shows whole, the gate sized to
    // it, and only one past all reason is cut, whole on hover.
    let cut = board::wrapped_lines(&g.criterion, board::CRITERION_W) > board::CRITERION_MOST;
    let whole = g.criterion.clone();
    let criterion = || {
        div()
            .id(SharedString::from(format!("canvas-criterion-{}", g.id)))
            .min_w_0()
            .line_clamp(board::CRITERION_MOST)
            .text_ellipsis()
            .text_size(px(board::CRITERION_SIZE * z))
            .line_height(px(board::CRITERION_LINE * z))
            .text_color(t.text2)
            .when(cut, |d| d.tooltip(kit::tip(whole.clone())))
            .child(g.criterion.clone())
    };
    div()
        .size_full()
        .relative()
        .child(
            div()
                .absolute()
                .left(px(board::FORK_DOT * z))
                .right_0()
                .bottom(px((card.h - board::fork_line(card.h) + GAP) * z))
                .flex()
                .flex_col()
                .gap(px(EDGE * z))
                .child(title)
                .child(criterion()),
        )
        .child(
            answerer(t, mission, &g.id, z, on_judge, catalog)
                .absolute()
                .left(px((board::FORK_DOT + CARD_PAD) * z))
                .right_0()
                .top(px((board::fork_line(card.h) + GAP) * z)),
        )
}

fn state_words(
    t: &ahjo_app_kit::theme::Theme,
    state: Option<crate::store::run_state::CardState>,
) -> (&'static str, Hsla) {
    use crate::store::run_state::CardState;
    match state {
        Some(CardState::Done) => ("done", t.done),
        Some(CardState::Running) => ("running", t.running),
        Some(CardState::Waiting) => ("needs you", t.needs),
        Some(CardState::Blocked) => ("blocked", t.error),
        Some(CardState::Failed) => ("failed", t.error),
        Some(CardState::Canceled) => ("canceled", t.dim),
        None => ("not reached", t.dim),
    }
}

/// Where each loop's words go, given where each would say them: two
/// loops held by one limit say the same thing near each other, and it is
/// said once; words that would still overprint others step down a line.
fn loop_places(wanted: Vec<(String, String, (f32, f32))>) -> Vec<(String, String, (f32, f32))> {
    let mut said: Vec<(String, String, (f32, f32))> = Vec::new();
    let span = |words: &str, x: f32| {
        let half = words.chars().count() as f32 * CARD_SMALL * board::GLYPH / 2.;
        (x - half, x + half)
    };
    for (id, words, (x, mut y)) in wanted {
        let (a, b) = span(&words, x);
        let clash = |y: f32, said: &[(String, String, (f32, f32))]| {
            said.iter()
                .find(|(_, w, (sx, sy))| {
                    let (sa, sb) = span(w, *sx);
                    a < sb && sa < b && (y - sy).abs() < LOOP_LINE
                })
                .map(|(_, w, _)| w.clone())
        };
        if clash(y, &said).is_some_and(|w| w == words) {
            continue;
        }
        while clash(y, &said).is_some() {
            y += LOOP_LINE;
        }
        said.push((id, words, (x, y)));
    }
    said
}

/// What a loop back says along its lane: how often it goes round, and
/// what happens when that is spent.
fn loop_words(mission: &Mission, wire: &board::Wire) -> Option<String> {
    if wire.route != "fail" {
        return None;
    }
    let limit = mission
        .limits
        .iter()
        .find(|l| (l.scope == wire.to_card || l.scope == wire.from_card) && l.rounds.is_some());
    Some(match limit.and_then(|l| l.rounds.map(|r| (r, l))) {
        Some((r, l)) => {
            let tries = format!("{r} tries");
            let then = match l.when_spent.as_str() {
                "best" => "then the best goes on",
                "fail" => "then it fails",
                _ => "then it waits for you",
            };
            format!("up to {tries} · {then}")
        }
        None => "again until it passes".into(),
    })
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
            .size(px(CARD_TEXT * z))
            .flex_none()
            .into_any_element(),
    )
}

/// Whether any gate in the mission waits on the operator.
pub(super) fn asks_you(mission: &Mission) -> bool {
    mission
        .gates
        .iter()
        .any(|gate| gate.kinds.contains(&GateKind::Human))
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
            (board::Kind::Step, Some(m)) => step_after(m, editing),
            // A Limit caps the step being edited, else the last step.
            (board::Kind::Limit, Some(m)) => editing
                .filter(|id| m.steps.iter().any(|f| f.id == *id))
                .map(str::to_owned)
                .or_else(|| m.steps.last().map(|f| f.id.clone())),
            _ => None,
        };
        self.place_card(kind, after.as_deref(), cx);
    }

    /// Add the step that comes after `card`, to its right, wired from it.
    fn next_step(&mut self, card: &str, cx: &mut Context<Self>) {
        self.place_card(board::Kind::Step, Some(card), cx);
    }

    fn place_card(&mut self, kind: board::Kind, after: Option<&str>, cx: &mut Context<Self>) {
        let Some(mission) = self.state.mission.clone() else {
            return;
        };
        let layout = self
            .state
            .layout
            .as_ref()
            .map(|l| l.cards.clone())
            .unwrap_or_default();
        let b = board::board(&mission, &flow::build(&mission), &layout, false);
        let size = board::new_size(kind);
        let beside = after.and_then(|id| b.cards.iter().find(|c| c.id == id));
        let room = 0.;
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
        if matches!(kind, board::Kind::Step | board::Kind::End) {
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
            m.steps
                .iter()
                .find(|f| !before.steps.iter().any(|b| b.id == f.id))
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
            m.steps
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
    if let Some(f) = m.steps.iter().find(|f| f.id == card) {
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
            "Solo step",
            new_step(Formation::Solo, "New step", x, y),
        ),
        (
            "canvas-new-orch",
            "Orchestrated step",
            new_step(Formation::Orchestrated, "New step", x, y),
        ),
        ("canvas-new-gate", "Gate", new_gate("Review gate", x, y)),
        ("canvas-new-limit", "Limit", new_limit("", x, y)),
        (
            "canvas-new-done",
            "End · done",
            new_end("Done", EndOutcome::Done, x, y),
        ),
        (
            "canvas-new-rejected",
            "End · rejected",
            new_end("Rejected", EndOutcome::Rejected, x, y),
        ),
    ]
    .into_iter()
    .skip(usize::from(!show_input))
}

fn place_patch(kind: board::Kind, after: Option<&str>, x: i64, y: i64) -> MissionChange {
    match kind {
        board::Kind::Input => new_input(x, y),
        board::Kind::Step => new_step(Formation::Solo, "New step", x, y),
        board::Kind::Gate => new_gate("New gate", x, y),
        board::Kind::End => new_end("End", EndOutcome::Done, x, y),
        board::Kind::Limit => new_limit(after.unwrap_or_default(), x, y),
    }
}

fn new_input(x: i64, y: i64) -> MissionChange {
    MissionChange::CreateInputCard {
        title: "Input".into(),
        goal: String::new(),
        files: vec![],
        human_channel: HumanChannel::Unset,
        x,
        y,
    }
}

fn new_step(kind: Formation, title: &str, x: i64, y: i64) -> MissionChange {
    MissionChange::CreateStep {
        formation: kind,
        title: title.into(),
        x,
        y,
    }
}

fn new_gate(title: &str, x: i64, y: i64) -> MissionChange {
    MissionChange::CreateGate {
        title: title.into(),
        kinds: vec![GateKind::Human],
        criterion: String::new(),
        check: String::new(),
        check_version: String::new(),
        check_value: String::new(),
        files: vec![],
        x,
        y,
    }
}

fn new_end(title: &str, outcome: EndOutcome, x: i64, y: i64) -> MissionChange {
    MissionChange::CreateEnd {
        title: title.into(),
        outcome,
        x,
        y,
    }
}

// New limits start at three rounds and, when placed after a card, cover it.
fn new_limit(target: &str, x: i64, y: i64) -> MissionChange {
    MissionChange::CreateLimit {
        title: "Limit".into(),
        scope: target.into(),
        rounds: Some(3),
        seconds: None,
        warn_seconds: None,
        tokens: None,
        when_spent: WhenSpent::Unset,
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
            board::Kind::Step,
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
        let mission: Mission = serde_json::from_value(serde_json::json!({
            "input_card": [{"id": "inp"}],
            "step": [
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
        let fresh: Mission =
            serde_json::from_value(serde_json::json!({"input_card": [{"id": "inp"}]})).unwrap();
        assert_eq!(step_after(&fresh, None).as_deref(), Some("inp"));
    }

    #[test]
    fn loops_held_by_one_limit_say_it_once_and_others_keep_clear() {
        let words = "up to 3 tries · then it fails".to_string();
        let near = |id: &str, w: &str, at| (id.to_string(), w.to_string(), at);
        let placed = loop_places(vec![
            near("a", &words, (720., 490.)),
            near("b", &words, (850., 502.)),
            near("c", "again until it passes", (800., 490.)),
            near("d", &words, (2000., 490.)),
        ]);
        let ids: Vec<_> = placed.iter().map(|(id, ..)| id.as_str()).collect();
        assert_eq!(ids, ["a", "c", "d"]);
        let (_, _, c) = &placed[1];
        assert!(c.1 >= 490. + LOOP_LINE, "{c:?}");
    }
}
