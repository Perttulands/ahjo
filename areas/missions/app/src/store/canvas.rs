//! A mission on its canvas (Archon's board): where each card sits, how big
//! it is, where its ports are, and the wires between them, in canvas units
//! before zoom. A mission with no saved layout reads in rows, in run order,
//! shaped for the room it is shown in, until it is arranged.

use crate::look;
use ahjo_missions_contract::flow::{Flow, judge_chain};
use ahjo_missions_contract::{LayoutFrame, LayoutNode, Limit, Mission};
use std::collections::{HashMap, HashSet};

/// Archon's card shapes, at its density: a step card is its IN rows, its
/// head, its slots and its OUT rows; ports sit on the rows.
pub const STEP_W: f32 = 300.;
/// A peer or orchestrated step is a little wider.
pub const WIDE_W: f32 = 316.;
pub const INPUT_W: f32 = 240.;
/// A gate as a verdict card: its head (who decides, its title and rule),
/// the judge folded into it, and a row for each outcome with its port.
pub const GATE_CARD_W: f32 = 284.;
pub const GATE_CARD_HEAD: f32 = 74.;
pub const GATE_CARD_JUDGE: f32 = 40.;
pub const GATE_CARD_ROW: f32 = 28.;
/// A gate as the flowchart's diamond, its words beside it.
pub const DIAMOND: f32 = 60.;
pub const DIAMOND_W: f32 = 300.;
pub const DIAMOND_H: f32 = 76.;
/// A gate as a checkpoint badge on the wire, its words under it.
pub const BADGE: f32 = 56.;
pub const BADGE_W: f32 = 168.;
pub const BADGE_H: f32 = 132.;
pub const BADGE_MID: f32 = 34.;
/// What a note explaining the shape takes over its group: its title's
/// line and the padding round the cards.
pub const FRAME_PAD: f32 = 16.;
pub const FRAME_TITLE: f32 = 22.;
const FRAME_TOP: f32 = FRAME_PAD + FRAME_TITLE + 6.;
/// A frame title's letters, to size the room it keeps clear of wires.
const FRAME_LETTER: f32 = 7.4;
pub const END_W: f32 = 176.;
pub const END_H: f32 = 52.;
/// A Limit card: a small card under the card it caps.
pub const LIMIT_W: f32 = 200.;
pub const LIMIT_H: f32 = 58.;
/// How far under its target a Limit card sits until it is placed.
const LIMIT_GAP: f32 = 28.;
/// An IN or OUT row.
pub const IO_ROW: f32 = 30.;
/// A step's head: number, title, two lines of brief; a run adds its chips.
pub const STEP_HEAD: f32 = 80.;
/// Without a brief the head only needs room for the title.
pub const STEP_HEAD_EMPTY: f32 = 46.;
pub const RUN_CHIPS: f32 = 26.;
/// The slots' body: its padding, each slot and the gap between them.
pub const BODY_PAD: f32 = 8.;
pub const SLOT_H: f32 = 40.;
pub const SLOT_GAP: f32 = 4.;
/// The dashed box holding an orchestrated step's workers or a peer group.
pub const POOL_PAD: f32 = 22.;
/// An Input card: eyebrow and title, goal, then one line for each fact.
pub const INPUT_HEAD: f32 = 52.;
pub const INPUT_GOAL: f32 = 38.;
pub const INPUT_LINE: f32 = 18.;
pub const INPUT_FOOT: f32 = 10.;
/// A port's dot, and how near a drop must land to take it.
pub const PORT: f32 = 14.;
pub const PORT_REACH: f32 = 16.;
/// How far apart a gate's pass and fail ports stand, each from its middle.
pub const GATE_PORTS: f32 = 13.;
/// The room a card's notes take under it: a closed sticky of a few lines.
pub const NOTE_ROOM: f32 = 92.;
const GAP_X: f32 = 72.;
const GAP_Y: f32 = 32.;
const MARGIN: f32 = 40.;
/// The room between two rows of a wrapped board, for the wires that run
/// from the end of one row to the start of the next and the loops back.
const ROW_GAP: f32 = 76.;

/// Archon's router: a wire leaves its port straight for a stub, turns at
/// right angles with rounded corners, and keeps a lane's clearance from
/// the cards it passes.
const STUB: f32 = 24.;
const LANE_CLEARANCE: f32 = 30.;
const SPAN_PAD: f32 = 14.;
const OBSTACLE_PAD: f32 = 6.;
const LEVEL: f32 = 6.;
/// Loops over the same span stand this far apart, the wider outside.
const CHANNEL_GAP: f32 = 12.;
/// A judge's wires leave the gate's foot this far down, and pass its card
/// this far out.
const JUDGE_DROP: f32 = 18.;
/// The radius a wire's corner is drawn with.
pub const CORNER: f32 = 10.;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Input,
    Formation,
    Gate,
    End,
    Limit,
    /// A frame's title line: drawn by the frame, here only so wires keep
    /// off its words.
    Frame,
}

/// One card on the canvas.
#[derive(Clone, Debug, PartialEq)]
pub struct Card {
    pub id: String,
    pub kind: Kind,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

type P = (f32, f32);

/// A wire from one port to another, the way it runs, and what it routes.
#[derive(Clone, Debug, PartialEq)]
pub struct Wire {
    pub id: String,
    /// Its course, port to port: straight runs meeting at right angles,
    /// clear of the cards between.
    pub points: Vec<P>,
    /// `pass`, `fail`, `judge`, `limit` (a Limit card to what it caps),
    /// or blank for work.
    pub route: String,
    pub from_node: String,
    pub to_node: String,
    /// It returns to a step the run met already (Flow's loop), whichever
    /// way the cards stand: drawn dashed, labelled with where it returns.
    pub back: bool,
    /// Where a loop says where it returns to: the start of its lane.
    pub label: Option<P>,
}

impl Wire {
    pub fn from(&self) -> P {
        self.points[0]
    }
    pub fn to(&self) -> P {
        self.points[self.points.len() - 1]
    }
    /// The middle of its longest run: where a note pin or a mark sits on it.
    pub fn middle(&self) -> P {
        self.points
            .windows(2)
            .max_by(|a, b| span(a[0], a[1]).total_cmp(&span(b[0], b[1])))
            .map(|w| ((w[0].0 + w[1].0) / 2., (w[0].1 + w[1].1) / 2.))
            .unwrap_or(self.from())
    }
}

fn span(a: P, b: P) -> f32 {
    (a.0 - b.0).abs() + (a.1 - b.1).abs()
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Board {
    pub cards: Vec<Card>,
    pub wires: Vec<Wire>,
    pub frames: Vec<FrameBox>,
}

/// A note that explains the shape, where it is drawn: the area round its
/// cards and its title's line at the area's top.
#[derive(Clone, Debug, PartialEq)]
pub struct FrameBox {
    pub id: String,
    pub title: String,
    pub text: String,
    pub region: (f32, f32, f32, f32),
    pub title_at: (f32, f32, f32, f32),
}

/// Where a gate's ports and foot are on it, by the look chosen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GateGeom {
    pub w: f32,
    pub h: f32,
    pub in_y: f32,
    pub pass_y: f32,
    pub fail_y: f32,
    /// Where its judge hangs from.
    pub foot: (f32, f32),
}

pub fn gate_geom(m: &Mission, id: &str) -> GateGeom {
    match look::gate() {
        look::Gate::Card => {
            // Room under the outcomes for the judge's row, or a little so
            // the fail row clears the gate's border.
            let judge = if folded_judge(m, id).is_some() {
                GATE_CARD_JUDGE + 12.
            } else {
                8.
            };
            let h = GATE_CARD_HEAD + judge + 2. * GATE_CARD_ROW;
            GateGeom {
                w: GATE_CARD_W,
                h,
                in_y: GATE_CARD_HEAD / 2.,
                pass_y: GATE_CARD_HEAD + 0.5 * GATE_CARD_ROW,
                fail_y: GATE_CARD_HEAD + 1.5 * GATE_CARD_ROW,
                foot: (GATE_CARD_W / 2., h),
            }
        }
        look::Gate::Diamond => GateGeom {
            w: DIAMOND_W,
            h: DIAMOND_H,
            in_y: DIAMOND_H / 2.,
            pass_y: DIAMOND_H / 2. - GATE_PORTS,
            fail_y: DIAMOND_H / 2. + GATE_PORTS,
            // Under the diamond's tip, on the box's edge, so the tether
            // leaves the gate rather than starting inside it.
            foot: (DIAMOND / 2., DIAMOND_H),
        },
        look::Gate::Badge => GateGeom {
            w: BADGE_W,
            h: BADGE_H,
            in_y: BADGE_MID,
            pass_y: BADGE_MID - GATE_PORTS,
            fail_y: BADGE_MID + GATE_PORTS,
            foot: (BADGE_W / 2., BADGE_H),
        },
    }
}

/// The judge drawn inside its gate's card (the card look, a chain of one
/// step): its own card is not on the board.
pub fn folded_judge(m: &Mission, gate: &str) -> Option<String> {
    if look::gate() != look::Gate::Card {
        return None;
    }
    let chain = judge_chain(m, gate);
    (chain.len() == 1).then(|| chain[0].clone())
}

fn folded(m: &Mission, id: &str) -> bool {
    m.gates
        .iter()
        .any(|g| folded_judge(m, &g.id).as_deref() == Some(id))
}

/// Every step that judges a gate.
pub fn judges(m: &Mission) -> HashSet<String> {
    m.gates.iter().flat_map(|g| judge_chain(m, &g.id)).collect()
}

/// What a Limit caps, as the board shows it: a loop's rounds on the gate
/// that sends the work back, the whole mission on its Input card, a step's
/// time or tokens on the step; only one capping nothing is a card.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LimitHome {
    Gate(String),
    Input(String),
    Step(String),
    Card,
}

pub fn limit_home(m: &Mission, l: &Limit) -> LimitHome {
    if m.input_cards.iter().any(|c| c.id == l.target) {
        return LimitHome::Input(l.target.clone());
    }
    if m.formations.iter().any(|f| f.id == l.target) {
        let sends_back = m.gates.iter().find(|g| {
            let fail = format!("{}:{}", g.id, ahjo_missions_contract::GATE_FAIL);
            m.connections
                .iter()
                .any(|c| c.from == fail && port_of(&c.to).0 == l.target)
        });
        return match sends_back {
            Some(g) if l.rounds.is_some() => LimitHome::Gate(g.id.clone()),
            _ => LimitHome::Step(l.target.clone()),
        };
    }
    LimitHome::Card
}

/// The limits shown on `id` (a gate, the Input card or a step).
pub fn limits_on<'a>(m: &'a Mission, id: &str) -> Vec<&'a Limit> {
    m.limits
        .iter()
        .filter(|l| {
            matches!(limit_home(m, l), LimitHome::Gate(g) | LimitHome::Input(g) | LimitHome::Step(g) if g == id)
        })
        .collect()
}

fn carded(m: &Mission, l: &Limit) -> bool {
    limit_home(m, l) == LimitHome::Card
}

/// Whether a step's workers or peers sit in a pool under its controller.
pub fn pooled(kind: &str) -> bool {
    matches!(kind, "orchestrated" | "peer")
}

/// The height of the slots' body for `slots` slots.
pub fn body_h(kind: &str, slots: usize) -> f32 {
    let n = slots.max(1) as f32;
    BODY_PAD * 2. + n * SLOT_H + (n - 1.) * SLOT_GAP + if pooled(kind) { POOL_PAD } else { 0. }
}

/// The face and the board must agree on the head height so ports meet rows.
pub fn step_head(f: &ahjo_missions_contract::Formation, running: bool) -> f32 {
    let has_brief = f.brief.as_ref().is_some_and(|b| !b.goal.trim().is_empty());
    (if has_brief {
        STEP_HEAD
    } else {
        STEP_HEAD_EMPTY
    }) + if running { RUN_CHIPS } else { 0. }
}

fn input_lines(m: &Mission, c: &ahjo_missions_contract::InputCard) -> f32 {
    // Inputs, and the human-gate channel, and a line for each limit on the
    // whole mission.
    1. + f32::from(u8::from(!c.inputs.is_empty())) + limits_on(m, &c.id).len() as f32
}

fn size(m: &Mission, id: &str, running: bool) -> Option<(Kind, f32, f32)> {
    if let Some(c) = m.input_cards.iter().find(|c| c.id == id) {
        let h = INPUT_HEAD + INPUT_GOAL + INPUT_LINE * input_lines(m, c) + INPUT_FOOT;
        return Some((Kind::Input, INPUT_W, h));
    }
    if let Some(f) = m.formations.iter().find(|f| f.id == id) {
        let w = if pooled(&f.kind) { WIDE_W } else { STEP_W };
        let rows = (f.inputs.len() + f.outputs.len()) as f32;
        let head = step_head(f, running);
        return Some((
            Kind::Formation,
            w,
            rows * IO_ROW + head + body_h(&f.kind, f.slots.len()),
        ));
    }
    if m.gates.iter().any(|g| g.id == id) {
        let g = gate_geom(m, id);
        return Some((Kind::Gate, g.w, g.h));
    }
    if m.ends.iter().any(|e| e.id == id) {
        return Some((Kind::End, END_W, END_H));
    }
    if m.limits.iter().any(|l| l.id == id) {
        return Some((Kind::Limit, LIMIT_W, LIMIT_H));
    }
    None
}

/// Run order, left to right in one row: what a card the layout misses
/// falls back to.
pub fn arranged(m: &Mission, flow: &Flow) -> Vec<LayoutNode> {
    shaped(m, flow, (f32::MAX, f32::MAX), 0., &|_| 0., &[])
}

/// The run-order columns, each top to bottom: Input cards, each numbered
/// step (siblings stacked, a gate's judges under it), Ends.
fn columns(m: &Mission, flow: &Flow) -> Vec<Vec<String>> {
    let mut columns: Vec<Vec<String>> = vec![m.input_cards.iter().map(|c| c.id.clone()).collect()];
    let mut by_number: Vec<(usize, String)> = flow
        .numbers
        .iter()
        .map(|(id, n)| (*n, id.clone()))
        .collect();
    by_number.sort();
    for (n, id) in by_number {
        while columns.len() <= n {
            columns.push(vec![]);
        }
        columns[n].push(id.clone());
        // A gate's judges sit under it, in a stable order; one drawn in
        // its gate's card takes no place of its own.
        let mut judges: Vec<&String> = flow
            .judge_of
            .iter()
            .filter(|(judge, gate)| **gate == id && !folded(m, judge))
            .map(|(judge, _)| judge)
            .collect();
        judges.sort();
        columns[n].extend(judges.into_iter().cloned());
    }
    columns.push(m.ends.iter().map(|e| e.id.clone()).collect());
    columns.retain(|c| !c.is_empty());
    columns
}

/// A column's cards with where each sits in it, then its width and height.
type Column = (Vec<(String, f32)>, f32, f32);

/// The mission laid out in reading rows for a view of `room` (width,
/// height, in pixels): its run-order columns wrapped into the rows that
/// draw its cards largest there, so a tall view gets a column of rows and
/// a wide one a single row. When even the best wrap draws them smaller
/// than `floor`, the rows are as wide as still fit across at `floor`, and
/// the board runs down past the view. Each card keeps the room `under`
/// gives it beneath (for its notes). A row never breaks inside a loop, so
/// work sent back runs back along its own row, nor inside a frame, whose
/// title takes a line over its row.
pub fn shaped(
    m: &Mission,
    flow: &Flow,
    room: (f32, f32),
    floor: f32,
    under: &dyn Fn(&str) -> f32,
    frames: &[LayoutFrame],
) -> Vec<LayoutNode> {
    let room = (room.0.max(1.), room.1.max(1.));
    let columns: Vec<Column> = columns(m, flow)
        .into_iter()
        .map(|ids| {
            let (mut y, mut w) = (0f32, 0f32);
            let mut placed = vec![];
            for id in ids {
                let Some((_, cw, ch)) = size(m, &id, false) else {
                    continue;
                };
                placed.push((id.clone(), y));
                y += ch + under(&id);
                for limit in m.limits.iter().filter(|l| l.target == id && carded(m, l)) {
                    y += LIMIT_GAP;
                    placed.push((limit.id.clone(), y));
                    y += LIMIT_H + under(&limit.id);
                }
                y += GAP_Y;
                w = w.max(cw);
            }
            (placed, w, (y - GAP_Y).max(0.))
        })
        .filter(|(placed, ..)| !placed.is_empty())
        .collect();
    if columns.is_empty() {
        return vec![];
    }
    let all = columns.len();
    let column_of: HashMap<&str, usize> = columns
        .iter()
        .enumerate()
        .flat_map(|(i, c)| c.0.iter().map(move |(id, _)| (id.as_str(), i)))
        .collect();
    // No break after a column inside a loop's or a frame's span.
    let mut held = vec![false; all];
    let mut hold = |a: usize, b: usize| {
        for h in held.iter_mut().take(a.max(b)).skip(a.min(b)) {
            *h = true;
        }
    };
    for c in &m.connections {
        let (from, to) = (port_of(&c.from).0, port_of(&c.to).0);
        let back = matches!(
            (flow.numbers.get(from), flow.numbers.get(to)),
            (Some(a), Some(b)) if b <= a
        );
        if let (true, Some(a), Some(b)) = (back, column_of.get(from), column_of.get(to)) {
            hold(*a, *b);
        }
    }
    let framed: HashSet<usize> = frames
        .iter()
        .flat_map(|f| {
            let cols: Vec<usize> = f
                .nodes
                .iter()
                .filter_map(|id| column_of.get(id.as_str()).copied())
                .collect();
            if let (Some(a), Some(b)) = (cols.iter().min(), cols.iter().max()) {
                hold(*a, *b);
            }
            cols
        })
        .collect();
    // The rows for about `n` columns each, breaking only where allowed.
    let rows_for = |n: usize| -> Vec<std::ops::Range<usize>> {
        let mut rows = vec![];
        let mut start = 0;
        for (i, &kept) in held.iter().enumerate().take(all) {
            if (i + 1 - start >= n && !kept) || i + 1 == all {
                rows.push(start..i + 1);
                start = i + 1;
            }
        }
        rows
    };
    let top = |r: &std::ops::Range<usize>| {
        if r.clone().any(|i| framed.contains(&i)) {
            FRAME_TOP
        } else {
            0.
        }
    };
    let extent = |n: usize| {
        let rows = rows_for(n);
        let w = rows
            .iter()
            .map(|r| {
                columns[r.clone()].iter().map(|c| c.1).sum::<f32>() + GAP_X * (r.len() - 1) as f32
            })
            .fold(0., f32::max);
        let h = rows
            .iter()
            .map(|r| top(r) + columns[r.clone()].iter().map(|c| c.2).fold(0., f32::max))
            .sum::<f32>()
            + ROW_GAP * (rows.len() - 1) as f32;
        (w + 2. * MARGIN, h + 2. * MARGIN)
    };
    let zoom = |n: usize| {
        let (w, h) = extent(n);
        (room.0 / w).min(room.1 / h).min(1.)
    };
    // The fewest rows among those that draw it largest (the last of
    // equals wins).
    let best = (1..=all)
        .max_by(|a, b| zoom(*a).total_cmp(&zoom(*b)))
        .unwrap_or(all);
    let per_row = if zoom(best) >= floor {
        best
    } else {
        (1..=all)
            .rev()
            .find(|n| room.0 / extent(*n).0 >= floor)
            .unwrap_or(1)
    };
    // A gate's pass port stands level with the way in of the card it
    // passes to, so the wire between them runs straight.
    let level = |gate: &str, next: &str| -> f32 {
        let passes = m.connections.iter().any(|c| {
            c.from == format!("{gate}:{}", ahjo_missions_contract::GATE_PASS)
                && port_of(&c.to).0 == next
        });
        let (Some((Kind::Gate, ..)), Some((kind, _, nh))) =
            (size(m, gate, false), size(m, next, false))
        else {
            return 0.;
        };
        let way_in = if kind == Kind::Formation {
            IO_ROW / 2.
        } else {
            nh / 2.
        };
        if passes {
            (gate_geom(m, gate).pass_y - way_in).max(0.)
        } else {
            0.
        }
    };
    let mut out = vec![];
    let mut y = MARGIN;
    for row in rows_for(per_row) {
        let mut x = MARGIN;
        let mut tallest = 0f32;
        let mut before: Option<&str> = None;
        y += top(&row);
        for (placed, w, h) in &columns[row] {
            let first = placed.first().map(|(id, _)| id.as_str());
            let drop = match (before, first) {
                (Some(gate), Some(next)) => level(gate, next),
                _ => 0.,
            };
            for (id, dy) in placed {
                out.push(LayoutNode {
                    id: id.clone(),
                    x: x as i64,
                    y: (y + drop + dy) as i64,
                });
            }
            x += w + GAP_X;
            tallest = tallest.max(h + drop);
            before = first;
        }
        y += tallest + ROW_GAP;
    }
    out
}

fn port_of(endpoint: &str) -> (&str, &str) {
    endpoint.split_once(':').unwrap_or((endpoint, ""))
}

/// Where a port is: inputs on the left on their IN rows, outputs on the
/// right on their OUT rows, a gate's pass over its fail on its right and
/// its judge port at its foot.
fn port_at(m: &Mission, c: &Card, port: &str, out: bool) -> P {
    let side = if out { c.x + c.w } else { c.x };
    match c.kind {
        Kind::Gate => {
            let g = gate_geom(m, &c.id);
            match port {
                "pass" => (side, c.y + g.pass_y),
                "fail" => (side, c.y + g.fail_y),
                "judge" => (c.x + g.foot.0, c.y + g.foot.1),
                _ => (side, c.y + g.in_y),
            }
        }
        Kind::Formation => {
            let f = m.formations.iter().find(|f| f.id == c.id);
            let ports = f.map(|f| if out { &f.outputs } else { &f.inputs });
            let n = ports.map(|p| p.len()).unwrap_or(0);
            let i = ports
                .and_then(|p| p.iter().position(|p| p.id == port))
                .unwrap_or(0);
            let row = if out {
                c.h - IO_ROW * (n - i) as f32
            } else {
                IO_ROW * i as f32
            };
            (side, c.y + row + IO_ROW / 2.)
        }
        _ => (side, c.y + c.h / 2.),
    }
}

/// A port on the board: its endpoint (`node:port`), where it is, whether
/// wires leave it, and whether a wire is on it.
#[derive(Clone, Debug, PartialEq)]
pub struct Port {
    pub endpoint: String,
    pub at: P,
    pub out: bool,
    pub wired: bool,
}

/// Every port on the board. A gate's judge port is both: its question
/// leaves it and the verdict returns to it.
pub fn ports(m: &Mission, b: &Board) -> Vec<Port> {
    let mut out = vec![];
    // A judge's ports are its gate's business: the tether between them
    // says it, so the judge shows none of its own.
    let judges = judges(m);
    let wired = |e: &str| m.connections.iter().any(|c| c.from == e || c.to == e);
    let mut add = |c: &Card, port: &str, leaves: bool| {
        let endpoint = format!("{}:{port}", c.id);
        out.push(Port {
            at: port_at(m, c, port, leaves),
            out: leaves,
            wired: wired(&endpoint),
            endpoint,
        });
    };
    for c in &b.cards {
        match c.kind {
            Kind::Input => add(c, "out", true),
            Kind::Formation => {
                if judges.contains(&c.id) {
                    continue;
                }
                if let Some(f) = m.formations.iter().find(|f| f.id == c.id) {
                    for p in &f.inputs {
                        add(c, &p.id, false);
                    }
                    for p in &f.outputs {
                        add(c, &p.id, true);
                    }
                }
            }
            Kind::Gate => {
                add(c, "in", false);
                add(c, "pass", true);
                add(c, "fail", true);
                // A judge's port only where a judge decides, and not when
                // the judge is drawn in the gate.
                let judged = m
                    .gates
                    .iter()
                    .any(|g| g.id == c.id && g.kinds.iter().any(|k| k == "formation"));
                if judged && folded_judge(m, &c.id).is_none() {
                    add(c, "judge", true);
                }
            }
            Kind::End => add(c, "in", false),
            Kind::Limit | Kind::Frame => {}
        }
    }
    out
}

/// The port a drop at `at` lands on: the nearest within reach that takes
/// a wire from `from` (an input for an output; the judge port takes
/// either way), never on the card the wire leaves.
pub fn drop_target<'a>(ports: &'a [Port], from: &str, at: P) -> Option<&'a Port> {
    let node = from.split(':').next().unwrap_or("");
    ports
        .iter()
        .filter(|p| {
            (!p.out || p.endpoint.ends_with(":judge")) && p.endpoint.split(':').next() != Some(node)
        })
        .map(|p| (p, (p.at.0 - at.0).hypot(p.at.1 - at.1)))
        .filter(|(_, d)| *d <= PORT_REACH)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(p, _)| p)
}

/// The board for a mission at these places; a card the layout misses is
/// placed where the run-order arrangement puts it.
/// `running` gives step cards room for their run chips.
pub fn board(
    m: &Mission,
    flow: &Flow,
    layout: &[LayoutNode],
    running: bool,
    frames: &[LayoutFrame],
) -> Board {
    let fallback = arranged(m, flow);
    let mut at: HashMap<&str, P> = fallback
        .iter()
        .filter(|p| !m.limits.iter().any(|l| l.id == p.id))
        .map(|p| (p.id.as_str(), (p.x as f32, p.y as f32)))
        .collect();
    for p in layout {
        at.insert(p.id.as_str(), (p.x as f32, p.y as f32));
    }
    let ids = m
        .input_cards
        .iter()
        .map(|c| &c.id)
        .chain(
            m.formations
                .iter()
                .map(|f| &f.id)
                .filter(|id| !folded(m, id)),
        )
        .chain(m.gates.iter().map(|g| &g.id))
        .chain(m.ends.iter().map(|e| &e.id));
    let mut cards: Vec<Card> = ids
        .filter_map(|id| {
            let (kind, w, h) = size(m, id, running)?;
            let (x, y) = at.get(id.as_str()).copied().unwrap_or((MARGIN, MARGIN));
            Some(Card {
                id: id.clone(),
                kind,
                x,
                y,
                w,
                h,
            })
        })
        .collect();
    // A Limit card not yet placed sits under the card it caps, after any
    // other limit there.
    for l in m.limits.iter().filter(|l| carded(m, l)) {
        let under = cards.iter().find(|c| c.id == l.target).map(|c| {
            let below = c.y + c.h + LIMIT_GAP;
            let taken = cards
                .iter()
                .filter(|o| o.kind == Kind::Limit && o.x == c.x && o.y >= below)
                .count() as f32;
            (c.x, below + taken * (LIMIT_H + GAP_Y / 2.))
        });
        let (x, y) = at
            .get(l.id.as_str())
            .copied()
            .or(under)
            .unwrap_or((MARGIN, MARGIN));
        cards.push(Card {
            id: l.id.clone(),
            kind: Kind::Limit,
            x,
            y,
            w: LIMIT_W,
            h: LIMIT_H,
        });
    }
    let boxes = frame_boxes(&cards, frames);
    cards.extend(boxes.iter().map(|f| Card {
        id: format!("frame:{}", f.id),
        kind: Kind::Frame,
        x: f.title_at.0,
        y: f.title_at.1,
        w: f.title_at.2,
        h: f.title_at.3,
    }));
    let card = |id: &str| cards.iter().find(|c| c.id == id);
    let number = |id: &str| flow.numbers.get(id).copied();
    let mut wires = vec![];
    let mut loops = vec![];
    for c in &m.connections {
        let (from_node, from_port) = port_of(&c.from);
        let (to_node, to_port) = port_of(&c.to);
        let (Some(a), Some(b)) = (card(from_node), card(to_node)) else {
            continue;
        };
        // A judge's verdict returns to the gate's judge port.
        let returning = to_port == "judge";
        let route = match (from_port, returning) {
            ("pass" | "fail" | "judge", _) => from_port.to_string(),
            (_, true) => "judge".into(),
            _ => String::new(),
        };
        let from = port_at(m, a, from_port, true);
        // The verdict returns to the gate's foot, where its question left.
        let to = port_at(m, b, to_port, false);
        // Back is told by the run's order, not by where the cards stand:
        // a chain wrapped onto a second row still reads forward.
        let back = route != "judge"
            && matches!((number(from_node), number(to_node)), (Some(a), Some(b)) if b <= a);
        let others: Vec<&Card> = cards
            .iter()
            .filter(|o| o.id != a.id && o.id != b.id)
            .collect();
        let points = if from_port == "judge" {
            // The gate and its judge are one relation, the question and
            // the verdict alike: a tether from the gate's foot to the
            // judge's top.
            tether(from, b)
        } else if returning {
            tether(to, a)
        } else if back {
            loops.push(wires.len());
            vec![from, to]
        } else {
            let points = route_wire(from, to, b.y - LANE_CLEARANCE, &others);
            let all: Vec<&Card> = cards.iter().collect();
            if clear(&points[1..points.len() - 1], &all) {
                points
            } else {
                detour(from, to, &all, &[]).unwrap_or(points)
            }
        };
        wires.push(Wire {
            id: c.id.clone(),
            points,
            back,
            label: None,
            route,
            from_node: from_node.to_string(),
            to_node: to_node.to_string(),
        });
    }
    route_loops(&mut wires, &loops, &cards);
    for l in m.limits.iter().filter(|l| carded(m, l)) {
        let (Some(a), Some(b)) = (card(&l.id), card(&l.target)) else {
            continue;
        };
        // From the card's top edge to the nearest edge of what it caps,
        // straight up where the card stands under it.
        let below = b.y + b.h <= a.y;
        let middle = a.x + a.w / 2.;
        let x = middle.clamp(b.x + CORNER, b.x + b.w - CORNER);
        let straight = (a.x..=a.x + a.w).contains(&x);
        let from = (
            if straight { x } else { middle },
            if below { a.y } else { a.y + a.h },
        );
        let to = (x, if below { b.y + b.h } else { b.y });
        wires.push(Wire {
            id: l.id.clone(),
            points: vec![from, to],
            route: "limit".into(),
            from_node: l.id.clone(),
            to_node: l.target.clone(),
            back: false,
            label: None,
        });
    }
    Board {
        cards,
        wires,
        frames: boxes,
    }
}

/// From a gate's foot to its judge: straight down where the judge hangs
/// under it, else down, across and down onto its top.
fn tether(foot: P, judge: &Card) -> Vec<P> {
    if judge.y >= foot.1 + LEVEL {
        let x = foot
            .0
            .clamp(judge.x + CORNER * 2., judge.x + judge.w - CORNER * 2.);
        if (x - foot.0).abs() < 0.5 {
            return vec![foot, (x, judge.y)];
        }
        let mid = ((foot.1 + judge.y) / 2.).round();
        return vec![foot, (foot.0, mid), (x, mid), (x, judge.y)];
    }
    let low = foot.1.max(judge.y + judge.h) + JUDGE_DROP;
    let x = judge.x + judge.w / 2.;
    vec![foot, (foot.0, low), (x, low), (x, judge.y + judge.h)]
}

/// Where each frame is drawn: round its cards on the board, its title's
/// line at the top. A frame none of whose cards are shown is not drawn.
fn frame_boxes(cards: &[Card], frames: &[LayoutFrame]) -> Vec<FrameBox> {
    frames
        .iter()
        .filter_map(|f| {
            let held: Vec<&Card> = cards.iter().filter(|c| f.nodes.contains(&c.id)).collect();
            let x0 = held.iter().map(|c| c.x).fold(f32::MAX, f32::min);
            let y0 = held.iter().map(|c| c.y).fold(f32::MAX, f32::min);
            let x1 = held.iter().map(|c| c.x + c.w).fold(f32::MIN, f32::max);
            let y1 = held.iter().map(|c| c.y + c.h).fold(f32::MIN, f32::max);
            if held.is_empty() {
                return None;
            }
            let region = (
                x0 - FRAME_PAD,
                y0 - FRAME_PAD - FRAME_TITLE,
                x1 - x0 + 2. * FRAME_PAD,
                y1 - y0 + 2. * FRAME_PAD + FRAME_TITLE,
            );
            let letters = (f.title.chars().count()
                + if f.text.is_empty() {
                    0
                } else {
                    f.text.chars().count() + 3
                }) as f32;
            let title_w = (letters * FRAME_LETTER + 2. * FRAME_PAD).min(region.2);
            Some(FrameBox {
                id: f.id.clone(),
                title: f.title.clone(),
                text: f.text.clone(),
                region,
                title_at: (region.0, region.1, title_w, FRAME_TITLE + FRAME_PAD / 2.),
            })
        })
        .collect()
}

/// Whether the segment a to b crosses `c` grown by `margin` (Liang-Barsky).
fn hits(a: P, b: P, c: &Card, margin: f32) -> bool {
    let (x1, y1) = (c.x - margin, c.y - margin);
    let (x2, y2) = (c.x + c.w + margin, c.y + c.h + margin);
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let (mut t0, mut t1) = (0f32, 1f32);
    for (p, q) in [
        (-dx, a.0 - x1),
        (dx, x2 - a.0),
        (-dy, a.1 - y1),
        (dy, y2 - a.1),
    ] {
        if p == 0. {
            if q < 0. {
                return false;
            }
        } else {
            let t = q / p;
            if p < 0. {
                if t > t1 {
                    return false;
                }
                t0 = t0.max(t);
            } else {
                if t < t0 {
                    return false;
                }
                t1 = t1.min(t);
            }
        }
    }
    true
}

fn clear(points: &[P], cards: &[&Card]) -> bool {
    points
        .windows(2)
        .all(|w| cards.iter().all(|c| !hits(w[0], w[1], c, OBSTACLE_PAD)))
}

/// A height clear of the cards over `xl..xr`, above them or below them,
/// whichever is nearer `prefer`.
fn clear_lane(cards: &[&Card], xl: f32, xr: f32, prefer: f32) -> f32 {
    let blocking: Vec<_> = cards
        .iter()
        .filter(|c| c.x < xr + SPAN_PAD && c.x + c.w > xl - SPAN_PAD)
        .collect();
    if blocking.is_empty() {
        return prefer;
    }
    let above = blocking.iter().map(|c| c.y).fold(f32::MAX, f32::min) - LANE_CLEARANCE;
    let below = blocking.iter().map(|c| c.y + c.h).fold(f32::MIN, f32::max) + LANE_CLEARANCE;
    if (above - prefer).abs() <= (below - prefer).abs() {
        above
    } else {
        below
    }
}

/// A forward wire: out of its port's right, into its target's left, level
/// where it can be, else turning once halfway, else along a lane round the
/// cards in its way. One wrapped onto the next row runs through the gap
/// between the rows, just over the row it goes to (`over`), clear of the
/// notes hanging under the row it leaves.
fn route_wire(from: P, to: P, over: f32, cards: &[&Card]) -> Vec<P> {
    let s = (from.0 + STUB, from.1);
    let t = (to.0 - STUB, to.1);
    let lane = |y: f32| vec![from, s, (s.0, y), (t.0, y), t, to];
    if to.0 >= from.0 - STUB {
        if (from.1 - to.1).abs() < LEVEL && clear(&[from, to], cards) {
            return vec![from, to];
        }
        let mid = ((s.0 + t.0) / 2.).round();
        let direct = vec![from, s, (mid, s.1), (mid, t.1), t, to];
        if clear(&direct, cards) {
            return direct;
        }
        return lane(clear_lane(cards, s.0, t.0, (from.1 + to.1) / 2.));
    }
    if to.1 > from.1 + STUB * 2. {
        let middle = over;
        let mut heights: Vec<f32> = std::iter::once(middle)
            .chain(
                cards
                    .iter()
                    .flat_map(|c| [c.y - LANE_CLEARANCE, c.y + c.h + LANE_CLEARANCE]),
            )
            .filter(|y| *y > from.1 + STUB && *y < to.1 - STUB)
            .collect();
        heights.sort_by(|a, b| {
            (a - middle)
                .abs()
                .total_cmp(&(b - middle).abs())
                .then(a.total_cmp(b))
        });
        // Down past the cards under its port where they stand wider.
        let mut outs: Vec<f32> = std::iter::once(s.0)
            .chain(
                cards
                    .iter()
                    .filter(|c| c.x + c.w >= s.0 && c.x < s.0 && c.y > from.1)
                    .map(|c| c.x + c.w + STUB),
            )
            .collect();
        outs.sort_by(f32::total_cmp);
        let at = |x: f32, y: f32| vec![from, (x, from.1), (x, y), (t.0, y), t, to];
        for x in outs {
            if let Some(y) = heights.iter().find(|y| clear(&at(x, **y), cards)) {
                return at(x, *y);
            }
        }
    }
    lane(clear_lane(cards, t.0, s.0, (from.1 + to.1) / 2.))
}

/// The loops back, routed together, the narrowest first. Each leaves its
/// port to the right, rises to a lane just over the cards it spans (over
/// its own row when it returns to a row above), runs back and drops into
/// its target from the left. A loop that would share a lane or a leg with
/// a wire placed already moves out a channel.
fn route_loops(wires: &mut [Wire], loops: &[usize], cards: &[Card]) {
    let mut order = loops.to_vec();
    order.sort_by(|a, b| {
        let w = |i: &usize| (wires[*i].from().0 - wires[*i].to().0).abs();
        w(a).total_cmp(&w(b)).then(wires[*a].id.cmp(&wires[*b].id))
    });
    // Clear of the forward wires' runs too: a loop never shares a lane.
    let mut placed: Vec<(P, P)> = wires
        .iter()
        .enumerate()
        .filter(|(i, _)| !loops.contains(i))
        .flat_map(|(_, w)| {
            w.points
                .windows(2)
                .map(|s| (s[0], s[1]))
                .collect::<Vec<_>>()
        })
        .collect();
    for i in order {
        let (from, to) = (wires[i].from(), wires[i].to());
        let card = |id: &str| cards.iter().find(|c| c.id == id);
        let (Some(a), Some(b)) = (card(&wires[i].from_node), card(&wires[i].to_node)) else {
            continue;
        };
        let (out, back) = (from.0 + STUB, to.0 - STUB);
        let (left, right) = (out.min(back), out.max(back));
        let spans = |c: &&Card| c.x < right + SPAN_PAD && c.x + c.w > left - SPAN_PAD;
        // Over the cards it spans in its own row; a loop to a row above
        // runs over its source's row (over its source card at least), under
        // the row it returns to.
        let upward = b.y + b.h <= a.y;
        let base = cards
            .iter()
            .filter(spans)
            .filter(|c| {
                if upward {
                    c.y >= b.y + b.h
                } else {
                    c.y + c.h > a.y.min(b.y)
                }
            })
            .map(|c| c.y)
            .fold(if upward { a.y } else { from.1.min(to.1) }, f32::min)
            - LANE_CLEARANCE;
        let path = |channel: f32| {
            let (out, back, y) = (
                out + channel * CHANNEL_GAP,
                back - channel * CHANNEL_GAP,
                base - channel * CHANNEL_GAP,
            );
            vec![from, (out, from.1), (out, y), (back, y), (back, to.1), to]
        };
        let legs = |points: &[P]| -> Vec<(P, P)> {
            points[1..points.len() - 1]
                .windows(2)
                .map(|w| (w[0], w[1]))
                .collect()
        };
        let mut channel = 0.;
        while channel < 24.
            && legs(&path(channel))
                .iter()
                .any(|s| placed.iter().any(|o| shares(*s, *o)))
        {
            channel += 1.;
        }
        let mut points = path(channel);
        // Where the lane would cross a card, the way round it.
        let all: Vec<&Card> = cards.iter().collect();
        if !clear(&points[1..points.len() - 1], &all)
            && let Some(round) = detour(from, to, &all, &placed)
        {
            points = round;
        }
        placed.extend(legs(&points));
        wires[i].label = Some(loop_label(&points));
        wires[i].points = points;
    }
}

/// Where a loop names what it returns to: under the start of its longest
/// level run, so the words lie along the way back and inside the board.
fn loop_label(points: &[P]) -> P {
    points
        .windows(2)
        .filter(|w| (w[0].1 - w[1].1).abs() < 0.5)
        .max_by(|a, b| (a[0].0 - a[1].0).abs().total_cmp(&(b[0].0 - b[1].0).abs()))
        .map(|w| (w[0].0.min(w[1].0) + CORNER, w[0].1))
        .unwrap_or_else(|| points.get(1).copied().unwrap_or(points[0]))
}

/// Bends cost as much as this much length, so a way round prefers few.
const BEND: f32 = 60.;
/// A run along a wire already placed costs as much as this much length.
const SHARED: f32 = 400.;

/// The way from an out port to an in port round every card: the shortest
/// path with few bends over the lines just clear of the cards' edges (a
/// visibility grid), leaving to the right and coming in from the left. It
/// keeps off the runs `taken` where it can. None when no way exists.
fn detour(from: P, to: P, cards: &[&Card], taken: &[(P, P)]) -> Option<Vec<P>> {
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;
    let (s, t) = ((from.0 + STUB, from.1), (to.0 - STUB, to.1));
    let line = |mut v: Vec<f32>| {
        v.sort_by(f32::total_cmp);
        v.dedup_by(|a, b| (*a - *b).abs() < 1.);
        v
    };
    let xs = line(
        [s.0, t.0]
            .into_iter()
            .chain(cards.iter().flat_map(|c| [c.x - STUB, c.x + c.w + STUB]))
            .collect(),
    );
    let ys = line(
        [s.1, t.1]
            .into_iter()
            .chain(
                cards
                    .iter()
                    .flat_map(|c| [c.y - LANE_CLEARANCE, c.y + c.h + LANE_CLEARANCE]),
            )
            .collect(),
    );
    let find = |v: &[f32], x: f32| v.iter().position(|p| (p - x).abs() < 1.);
    let (Some(sx), Some(sy), Some(tx), Some(ty)) = (
        find(&xs, s.0),
        find(&ys, s.1),
        find(&xs, t.0),
        find(&ys, t.1),
    ) else {
        return None;
    };
    let at = |(i, j): (usize, usize)| (xs[i], ys[j]);
    let open = |a: P, b: P| cards.iter().all(|c| !hits(a, b, c, OBSTACLE_PAD));
    let cost = |a: P, b: P| {
        span(a, b)
            + if taken.iter().any(|o| shares((a, b), *o)) {
                SHARED
            } else {
                0.
            }
    };
    // A state is a grid point and whether it was reached across (0) or
    // down (1); costs are kept in tenths of a pixel to order them exactly.
    let index = |i: usize, j: usize, d: usize| (i * ys.len() + j) * 2 + d;
    let mut best = vec![u64::MAX; xs.len() * ys.len() * 2];
    let mut came: Vec<Option<usize>> = vec![None; best.len()];
    let mut queue = BinaryHeap::new();
    let start = index(sx, sy, 0);
    best[start] = 0;
    queue.push(Reverse((0u64, sx, sy, 0usize)));
    let tenths = |f: f32| (f * 10.).round() as u64;
    let mut end = None;
    while let Some(Reverse((d, i, j, dir))) = queue.pop() {
        if d > best[index(i, j, dir)] {
            continue;
        }
        if (i, j) == (tx, ty) && dir == 0 {
            end = Some(index(i, j, dir));
            break;
        }
        let steps = [
            (i.wrapping_sub(1), j, 0),
            (i + 1, j, 0),
            (i, j.wrapping_sub(1), 1),
            (i, j + 1, 1),
        ];
        for (ni, nj, nd) in steps {
            if ni >= xs.len() || nj >= ys.len() || !open(at((i, j)), at((ni, nj))) {
                continue;
            }
            let turn = if nd != dir { BEND } else { 0. };
            let next = d + tenths(cost(at((i, j)), at((ni, nj))) + turn);
            let k = index(ni, nj, nd);
            if next < best[k] {
                best[k] = next;
                came[k] = Some(index(i, j, dir));
                queue.push(Reverse((next, ni, nj, nd)));
            }
        }
    }
    let mut k = end?;
    let mut path = vec![to];
    loop {
        let cell = k / 2;
        path.push(at((cell / ys.len(), cell % ys.len())));
        match came[k] {
            Some(p) => k = p,
            None => break,
        }
    }
    path.push(from);
    path.reverse();
    path.dedup_by(|a, b| span(*a, *b) < 0.5);
    // Only the corners stay.
    let mut corners: Vec<P> = vec![path[0]];
    for w in path.windows(3) {
        let straight = ((w[0].0 - w[1].0).abs() < 0.5 && (w[1].0 - w[2].0).abs() < 0.5)
            || ((w[0].1 - w[1].1).abs() < 0.5 && (w[1].1 - w[2].1).abs() < 0.5);
        if !straight {
            corners.push(w[1]);
        }
    }
    corners.push(*path.last()?);
    Some(corners)
}

/// Two straight runs on one line that overlap for more than a point.
fn shares((a1, a2): (P, P), (b1, b2): (P, P)) -> bool {
    let overlap = |p1: f32, p2: f32, q1: f32, q2: f32| {
        p1.max(p2).min(q1.max(q2)) - p1.min(p2).max(q1.min(q2)) > 1.
    };
    let vertical = |p: P, q: P| (p.0 - q.0).abs() < 1.;
    let horizontal = |p: P, q: P| (p.1 - q.1).abs() < 1.;
    if vertical(a1, a2) && vertical(b1, b2) && (a1.0 - b1.0).abs() < 1. {
        return overlap(a1.1, a2.1, b1.1, b2.1);
    }
    if horizontal(a1, a2) && horizontal(b1, b2) && (a1.1 - b1.1).abs() < 1. {
        return overlap(a1.0, a2.0, b1.0, b2.0);
    }
    false
}

/// How big a card of this kind is when it is new: one port each way, one
/// slot, no brief.
pub fn new_size(kind: Kind) -> (f32, f32) {
    match kind {
        Kind::Input => (INPUT_W, INPUT_HEAD + INPUT_GOAL + INPUT_LINE + INPUT_FOOT),
        Kind::Formation => (STEP_W, 2. * IO_ROW + STEP_HEAD_EMPTY + body_h("solo", 1)),
        Kind::Gate => (GATE_CARD_W, GATE_CARD_HEAD + 2. * GATE_CARD_ROW),
        Kind::End => (END_W, END_H),
        Kind::Limit => (LIMIT_W, LIMIT_H),
        Kind::Frame => (0., 0.),
    }
}

/// Where a new card of `size` goes: the first place inside `view` (x, y,
/// width, height on the board) that keeps clear of every card and of the
/// room `below` each Input card keeps for the mission's notes. It is tried
/// first to the right of the rightmost cards, so a mission grows in reading
/// order; None when the view has no such place.
pub fn free_spot(
    b: &Board,
    view: (f32, f32, f32, f32),
    size: (f32, f32),
    below: f32,
) -> Option<(f32, f32)> {
    let (vx, vy, vw, vh) = view;
    let taken = |x: f32, y: f32| {
        b.cards.iter().any(|c| {
            let h = c.h + if c.kind == Kind::Input { below } else { 0. };
            x < c.x + c.w + GAP_Y
                && c.x < x + size.0 + GAP_Y
                && y < c.y + h + GAP_Y
                && c.y < y + size.1 + GAP_Y
        })
    };
    let inside =
        |x: f32, y: f32| x >= vx && y >= vy && x + size.0 <= vx + vw && y + size.1 <= vy + vh;
    let mut beside: Vec<&Card> = b.cards.iter().collect();
    beside.sort_by(|a, b| (b.x + b.w).total_cmp(&(a.x + a.w)));
    let first = beside.iter().map(|c| (c.x + c.w + GAP_X, c.y));
    let step = PORT;
    let rows = ((vh - size.1) / step).max(0.) as usize;
    let cols = ((vw - size.0) / step).max(0.) as usize;
    // Failing those, the free place nearest the middle of the view.
    let middle = (vx + (vw - size.0) / 2., vy + (vh - size.1) / 2.);
    let mut grid: Vec<(f32, f32)> = (0..=rows)
        .flat_map(|r| (0..=cols).map(move |c| (vx + c as f32 * step, vy + r as f32 * step)))
        .collect();
    grid.sort_by(|a, b| {
        let d = |p: &(f32, f32)| (p.0 - middle.0).powi(2) + (p.1 - middle.1).powi(2);
        d(a).total_cmp(&d(b))
    });
    first
        .chain(grid)
        .find(|&(x, y)| inside(x, y) && !taken(x, y))
        .map(|(x, y)| (x.round(), y.round()))
}

/// The smallest rectangle holding every card and every wire's course
/// (a loop runs over its cards): x, y, width, height.
pub fn extent(b: &Board) -> (f32, f32, f32, f32) {
    if b.cards.is_empty() {
        return (0., 0., 1., 1.);
    }
    let corners = b
        .cards
        .iter()
        .flat_map(|c| [(c.x, c.y), (c.x + c.w, c.y + c.h)])
        .chain(b.wires.iter().flat_map(|w| w.points.iter().copied()));
    let (x0, y0, x1, y1) = corners.fold(
        (f32::MAX, f32::MAX, f32::MIN, f32::MIN),
        |(x0, y0, x1, y1), (x, y)| (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
    );
    (x0, y0, x1 - x0, y1 - y0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::flow_tests::delivery;
    use ahjo_missions_contract::flow::build;

    #[test]
    fn an_unarranged_mission_reads_left_to_right_in_run_order() {
        // The judge as its own card, under its gate.
        let _look = crate::look::gate_for_test(crate::look::Gate::Diamond);
        let m = delivery();
        let b = board(&m, &build(&m), &[], false, &[]);
        let x = |id: &str| b.cards.iter().find(|c| c.id == id).unwrap().x;
        assert!(x("mis_delivery") < x("fmn_plan"));
        assert!(x("fmn_plan") < x("fmn_beads"));
        assert!(x("fmn_execution") < x("fmn_final_review"));
        assert!(x("fmn_final_review") < x("end_delivered"));
        // The judge sits under its gate.
        let gate = b
            .cards
            .iter()
            .find(|c| c.id == "gate_beads_review")
            .unwrap();
        let judge = b
            .cards
            .iter()
            .find(|c| c.id == "fmn_beads_reviewer")
            .unwrap();
        assert_eq!(gate.x, judge.x);
        assert!(judge.y > gate.y + gate.h);
        assert_eq!(b.wires.len(), m.connections.len());
        let fail = b.wires.iter().find(|w| w.route == "fail").unwrap();
        assert_eq!(fail.to_node, "fmn_beads");
        // Sent back, it loops just over the cards it spans, not over the
        // whole board, and dashed and named where it returns.
        assert!(fail.back && fail.label.is_some());
        let lane = fail.points.iter().map(|p| p.1).fold(f32::MAX, f32::min);
        let (left, right) = (fail.to().0, fail.from().0);
        let spanned: Vec<&Card> = b
            .cards
            .iter()
            .filter(|c| c.x < right && c.x + c.w > left)
            .collect();
        assert!(spanned.iter().all(|c| lane < c.y), "{lane}");
        assert!(lane > spanned.iter().map(|c| c.y).fold(f32::MAX, f32::min) - 60.);
        let pass = b.wires.iter().find(|w| w.route == "pass").unwrap();
        assert!(!pass.back && pass.label.is_none());
        assert_eq!(crossing(&b), Vec::<String>::new());
    }

    /// The wires that run through a card they neither leave nor enter.
    fn crossing(b: &Board) -> Vec<String> {
        b.wires
            .iter()
            .filter(|w| w.route != "limit")
            .filter(|w| {
                w.points.windows(2).any(|s| {
                    b.cards.iter().any(|c| {
                        // A port's own stub touches its card's edge only.
                        hits(s[0], s[1], c, -1.)
                    })
                })
            })
            .map(|w| w.id.clone())
            .collect()
    }

    #[test]
    fn wires_turn_at_right_angles() {
        let m = delivery();
        let flow = build(&m);
        for layout in [vec![], shaped(&m, &flow, (700., 1400.), 0., &|_| 0., &[])] {
            let b = board(&m, &flow, &layout, false, &[]);
            for w in b.wires.iter().filter(|w| w.route != "limit") {
                for s in w.points.windows(2) {
                    assert!(
                        (s[0].0 - s[1].0).abs() < 0.5 || (s[0].1 - s[1].1).abs() < 0.5,
                        "{} {s:?}",
                        w.id
                    );
                }
            }
        }
    }

    #[test]
    fn wires_keep_off_the_cards_they_pass_and_a_pass_runs_level() {
        let m = delivery();
        let flow = build(&m);
        for room in [(4000., 900.), (1400., 900.), (700., 1400.), (500., 300.)] {
            let b = board(
                &m,
                &flow,
                &shaped(&m, &flow, room, 0., &|_| 0., &[]),
                false,
                &[],
            );
            let cards: Vec<&Card> = b.cards.iter().collect();
            // Between its own two ports a wire crosses no card. A judge's
            // brackets and a limit's tether hug their own cards.
            for w in b
                .wires
                .iter()
                .filter(|w| w.route != "limit" && w.route != "judge")
            {
                let n = w.points.len();
                assert!(
                    clear(&w.points[1..n - 1], &cards),
                    "{room:?} {} {:?}",
                    w.id,
                    w.points
                );
            }
            // On one row, a gate's pass to the card after it runs straight
            // across.
            if room.0 > 2000. {
                let pass = b.wires.iter().find(|w| w.route == "pass").unwrap();
                let (a, z) = (pass.points[0], pass.points[pass.points.len() - 1]);
                assert!((a.1 - z.1).abs() < 1., "{:?}", pass.points);
            }
        }
    }

    #[test]
    fn a_detour_goes_round_a_card_in_the_way_with_few_bends() {
        let card = |id: &str, x: f32, y: f32| Card {
            id: id.into(),
            kind: Kind::Formation,
            x,
            y,
            w: 200.,
            h: 120.,
        };
        let (a, wall, b) = (
            card("a", 0., 0.),
            card("wall", 300., 0.),
            card("b", 600., 0.),
        );
        let all = [&a, &wall, &b];
        let way = detour((200., 60.), (600., 60.), &all, &[]).unwrap();
        assert_eq!((way[0], way[way.len() - 1]), ((200., 60.), (600., 60.)));
        assert!(clear(&way[1..way.len() - 1], &all), "{way:?}");
        assert!(
            way.len() <= 6,
            "over or under the wall, not a maze: {way:?}"
        );
        for s in way.windows(2) {
            assert!(s[0].0 == s[1].0 || s[0].1 == s[1].1, "{way:?}");
        }
    }

    #[test]
    fn a_board_shown_in_a_pane_is_laid_out_to_the_panes_shape() {
        // The judge as its own card, under its gate.
        let _look = crate::look::gate_for_test(crate::look::Gate::Diamond);
        let m = delivery();
        let flow = build(&m);
        let rows = |room: (f32, f32), floor: f32| {
            let layout = shaped(&m, &flow, room, floor, &|_| 0., &[]);
            // A row starts at the left margin (a column after a gate may
            // stand lower, level with the gate's pass). Cards stacked in
            // the first column (a judge, a limit) follow closer than a row
            // gap; the next row starts a full row gap further down.
            let b = board(&m, &flow, &layout, false, &[]);
            let left = b.cards.iter().map(|c| c.x).fold(f32::MAX, f32::min);
            let mut first: Vec<&Card> = b.cards.iter().filter(|c| c.x == left).collect();
            first.sort_by(|p, q| p.y.total_cmp(&q.y));
            let rows = 1 + first
                .windows(2)
                .filter(|w| w[1].y - (w[0].y + w[0].h) >= ROW_GAP)
                .count();
            (rows, b)
        };
        // A wide pane shows the mission as one row; a tall one wraps it,
        // so its cards are drawn larger than one long row would allow.
        let (wide, _) = rows((4000., 900.), 0.);
        assert_eq!(wide, 1);
        let (tall, b) = rows((700., 1400.), 0.);
        assert!(tall > 1, "a tall pane wraps the chain");
        let (_, _, w, h) = extent(&b);
        let (_, _, w1, h1) = extent(&board(&m, &flow, &arranged(&m, &flow), false, &[]));
        assert!((700. / w).min(1400. / h) > (700. / w1).min(1400. / h1));
        // Too small for the floor: the rows still fit across at the floor
        // and the board runs down instead.
        let (_, b) = rows((500., 300.), 0.6);
        assert!(extent(&b).2 * 0.6 <= 500.);
        // A card with notes keeps room for them under it: its judge stands
        // that much lower.
        let at = |layout: &[LayoutNode], id: &str| layout.iter().find(|p| p.id == id).unwrap().y;
        let plain = shaped(&m, &flow, (4000., 900.), 0., &|_| 0., &[]);
        let noted = shaped(
            &m,
            &flow,
            (4000., 900.),
            0.,
            &|id| {
                if id == "gate_beads_review" {
                    NOTE_ROOM
                } else {
                    0.
                }
            },
            &[],
        );
        assert_eq!(
            at(&noted, "fmn_beads_reviewer") - at(&plain, "fmn_beads_reviewer"),
            NOTE_ROOM as i64
        );
    }

    #[test]
    fn a_wire_wrapped_onto_the_next_row_reads_forward() {
        let m = delivery();
        let flow = build(&m);
        let b = board(
            &m,
            &flow,
            &shaped(&m, &flow, (700., 1400.), 0., &|_| 0., &[]),
            false,
            &[],
        );
        let card = |id: &str| b.cards.iter().find(|c| c.id == id).unwrap();
        let wrapped: Vec<&Wire> = b
            .wires
            .iter()
            .filter(|w| w.route != "judge" && w.route != "limit" && w.to().0 < w.from().0)
            .filter(|w| !matches!((flow.numbers.get(&w.from_node), flow.numbers.get(&w.to_node)), (Some(a), Some(b)) if b <= a))
            .collect();
        assert!(!wrapped.is_empty(), "the pane wraps a forward wire");
        for w in wrapped {
            assert!(!w.back, "{} reads forward", w.id);
            // It runs through the gap above its target, not over the board.
            let (a, t) = (card(&w.from_node), card(&w.to_node));
            assert!(
                w.points
                    .windows(2)
                    .any(|s| s[0].1 == s[1].1 && s[0].1 > a.y + a.h && s[0].1 < t.y),
                "{} runs between the rows: {:?}",
                w.id,
                w.points
            );
        }
        // A gate sending work back still loops back, wherever it stands,
        // over the gate's row and clear of the card it returns to.
        let fail = b.wires.iter().find(|w| w.route == "fail").unwrap();
        assert!(fail.back);
        let (gate, to) = (card(&fail.from_node), card(&fail.to_node));
        let lane = fail.points[2].1;
        assert!(lane < gate.y && (lane < to.y || lane > to.y + to.h));
        assert_eq!(crossing(&b), Vec::<String>::new());
    }

    #[test]
    fn every_wire_ends_at_the_named_visible_ports() {
        // The judge as its own card, under its gate.
        let _look = crate::look::gate_for_test(crate::look::Gate::Diamond);
        let mission = delivery();
        assert!(mission.formations.len() >= 5);
        let board = board(&mission, &build(&mission), &[], false, &[]);
        let ports = ports(&mission, &board);
        assert_eq!(board.wires.len(), mission.connections.len());
        for connection in &mission.connections {
            let wire = board
                .wires
                .iter()
                .find(|wire| wire.id == connection.id)
                .unwrap();
            // A gate and its judge are tied by one tether, from the gate's
            // foot to the judge's top, whichever way the connection runs;
            // the judge shows no ports of its own.
            if wire.route == "judge" {
                let gate = board.cards.iter().find(|c| c.kind == Kind::Gate).unwrap();
                let judge = board
                    .cards
                    .iter()
                    .find(|c| c.id == "fmn_beads_reviewer")
                    .unwrap();
                let g = gate_geom(&mission, &gate.id);
                assert_eq!(wire.from(), (gate.x + g.foot.0, gate.y + g.foot.1));
                assert_eq!(wire.to().1, judge.y, "{}", connection.id);
                continue;
            }
            let source = ports
                .iter()
                .find(|port| port.endpoint == connection.from)
                .unwrap();
            let target = ports
                .iter()
                .find(|port| port.endpoint == connection.to)
                .unwrap();
            assert_eq!(wire.from(), source.at, "source {}", connection.id);
            assert_eq!(wire.to(), target.at, "target {}", connection.id);
        }
    }

    #[test]
    fn a_new_card_lands_in_free_space_in_view() {
        let m = delivery();
        let b = board(&m, &build(&m), &[], false, &[]);
        let (x0, y0, w, h) = extent(&b);
        let size = new_size(Kind::Formation);
        let clear = |(x, y): (f32, f32)| {
            b.cards
                .iter()
                .all(|c| x + size.0 <= c.x || c.x + c.w <= x || y + size.1 <= c.y || c.y + c.h <= y)
        };
        // A view over the whole board and room beside it: the card goes to
        // the right of the rightmost card, in view, touching nothing.
        let view = (x0, y0, w + size.0 * 2., h);
        let at = free_spot(&b, view, size, 0.).unwrap();
        assert!(clear(at));
        assert!(at.0 >= x0 + w);
        assert!(at.0 + size.0 <= view.0 + view.2 && at.1 + size.1 <= view.1 + view.3);
        // A view the cards fill leaves no place; one beside them has one.
        assert_eq!(free_spot(&b, (x0, y0, w, h), (w, h), 0.), None);
        // On an empty board the first card lands in the middle of the view.
        let empty = Board::default();
        let (x, y) = free_spot(&empty, (500., 300., 800., 600.), size, 0.).unwrap();
        assert!((x + size.0 / 2. - 900.).abs() <= PORT && (y + size.1 / 2. - 600.).abs() <= PORT);
    }

    #[test]
    fn a_saved_place_wins_over_the_arrangement() {
        let m = delivery();
        let saved = [LayoutNode {
            id: "fmn_plan".into(),
            x: 900,
            y: 700,
        }];
        let b = board(&m, &build(&m), &saved, false, &[]);
        let plan = b.cards.iter().find(|c| c.id == "fmn_plan").unwrap();
        assert_eq!((plan.x, plan.y), (900., 700.));
        let into = b.wires.iter().find(|w| w.to_node == "fmn_plan").unwrap();
        assert_eq!(into.to().0, 900.);
    }

    #[test]
    fn empty_briefs_leave_no_band_and_outputs_follow_the_card() {
        let mut m = delivery();
        let id = m.formations[0].id.clone();
        m.formations[0].brief = Some(ahjo_missions_contract::Brief {
            goal: "A brief".into(),
            ..Default::default()
        });
        let full = board(&m, &build(&m), &[], false, &[]);
        m.formations[0].brief.as_mut().unwrap().goal = " \n ".into();
        let compact = board(&m, &build(&m), &[], false, &[]);
        let card = |b: &Board| b.cards.iter().find(|c| c.id == id).unwrap().clone();
        assert_eq!(
            card(&full).h - card(&compact).h,
            STEP_HEAD - STEP_HEAD_EMPTY
        );
        let full_ports = ports(&m, &full);
        let compact_ports = ports(&m, &compact);
        let output = compact_ports
            .iter()
            .find(|p| p.out && p.endpoint.starts_with(&format!("{id}:")))
            .unwrap();
        let old = full_ports
            .iter()
            .find(|p| p.endpoint == output.endpoint)
            .unwrap();
        assert_eq!(old.at.1 - output.at.1, STEP_HEAD - STEP_HEAD_EMPTY);
        assert_eq!(
            step_head(&m.formations[0], true),
            STEP_HEAD_EMPTY + RUN_CHIPS
        );
    }

    #[test]
    fn a_gate_card_holds_its_judge_and_the_limit_on_its_loop() {
        let _look = crate::look::gate_for_test(crate::look::Gate::Card);
        let mut m = delivery();
        let limit = |id: &str, target: &str, rounds: Option<i64>| Limit {
            id: id.into(),
            title: id.into(),
            target: target.into(),
            rounds,
            when_spent: "best".into(),
            ..Limit::default()
        };
        m.limits = vec![
            // Rounds on the step the gate sends back to: the gate's rule.
            limit("lim_loop", "fmn_beads", Some(3)),
            // A cap on the Input: the whole mission's.
            limit("lim_all", "mis_delivery", None),
            // Time on a step nothing sends back to: the step's own.
            limit("lim_plan", "fmn_plan", None),
        ];
        assert_eq!(
            limit_home(&m, &m.limits[0]),
            LimitHome::Gate("gate_beads_review".into())
        );
        assert_eq!(
            limit_home(&m, &m.limits[1]),
            LimitHome::Input("mis_delivery".into())
        );
        assert_eq!(
            limit_home(&m, &m.limits[2]),
            LimitHome::Step("fmn_plan".into())
        );
        let b = board(&m, &build(&m), &[], false, &[]);
        // Every limit lives on what it caps, so none is a card of its own.
        assert!(b.cards.iter().all(|c| c.kind != Kind::Limit));
        // The single judge is drawn in the gate: no card, no wires, and no
        // judge port to drop one on.
        assert_eq!(
            folded_judge(&m, "gate_beads_review").as_deref(),
            Some("fmn_beads_reviewer")
        );
        assert!(b.cards.iter().all(|c| c.id != "fmn_beads_reviewer"));
        assert!(
            b.wires
                .iter()
                .all(|w| w.from_node != "fmn_beads_reviewer" && w.to_node != "fmn_beads_reviewer")
        );
        let ports = ports(&m, &b);
        assert!(
            ports
                .iter()
                .all(|p| p.endpoint != "gate_beads_review:judge")
        );
        // The gate is tall enough for its judge row under its outcomes.
        let gate = b
            .cards
            .iter()
            .find(|c| c.id == "gate_beads_review")
            .unwrap();
        let g = gate_geom(&m, "gate_beads_review");
        assert_eq!(gate.h, g.h);
        assert!(g.fail_y > g.pass_y && g.h >= g.fail_y + GATE_CARD_JUDGE);
        // The loop back still leaves the fail row, dashed and named.
        let fail = b.wires.iter().find(|w| w.route == "fail").unwrap();
        assert!(fail.back && fail.label.is_some());
        assert_eq!(fail.from().1, gate.y + g.fail_y);
    }

    #[test]
    fn a_wire_drops_on_the_input_port_nearest_the_pointer() {
        // The judge as its own card, under its gate.
        let _look = crate::look::gate_for_test(crate::look::Gate::Diamond);
        let m = delivery();
        let b = board(&m, &build(&m), &[], false, &[]);
        let ports = ports(&m, &b);
        let into_beads = ports
            .iter()
            .find(|p| p.endpoint == "fmn_beads:port_beads_in")
            .unwrap();
        assert!(!into_beads.out && into_beads.wired);
        let beads = b.cards.iter().find(|c| c.id == "fmn_beads").unwrap();
        // On the card's left edge, on its IN row.
        assert_eq!(into_beads.at, (beads.x, beads.y + IO_ROW / 2.));
        let near = (into_beads.at.0 + 5., into_beads.at.1 - 5.);
        let hit = drop_target(&ports, "fmn_plan:port_plan_out", near).unwrap();
        assert_eq!(hit.endpoint, "fmn_beads:port_beads_in");
        // Not onto the card the wire leaves, nor onto an output.
        assert!(drop_target(&ports, "fmn_beads:port_beads_out", near).is_none());
        let out = ports
            .iter()
            .find(|p| p.endpoint == "fmn_plan:port_plan_out")
            .unwrap();
        assert!(drop_target(&ports, "mis_delivery:out", out.at).is_none());
        // A gate's judge port takes the judge's verdict.
        let judge = ports
            .iter()
            .find(|p| p.endpoint == "gate_beads_review:judge")
            .unwrap();
        let hit = drop_target(
            &ports,
            "fmn_beads_reviewer:port_beads_reviewer_out",
            judge.at,
        );
        assert_eq!(
            hit.map(|p| p.endpoint.as_str()),
            Some("gate_beads_review:judge")
        );
    }
}
