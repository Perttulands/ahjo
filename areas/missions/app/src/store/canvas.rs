//! A mission on its canvas (Archon's board): where each card sits, how big
//! it is, where its ports are, and the wires between them, in canvas units
//! before zoom. A mission with no saved layout reads left to right in run
//! order until it is arranged.

use ahjo_missions_contract::flow::Flow;
use ahjo_missions_contract::{LayoutNode, Mission};
use std::collections::HashMap;

/// Archon's card sizes (steps-d7.css). A step card is its IN rows,
/// its head, its seats and its OUT rows; ports sit on the rows.
pub const STEP_W: f32 = 320.;
/// A peer or orchestrated step is a little wider.
pub const WIDE_W: f32 = 336.;
pub const INPUT_W: f32 = 260.;
pub const GATE_W: f32 = 300.;
pub const GATE_H: f32 = 96.;
pub const END_W: f32 = 200.;
pub const END_H: f32 = 60.;
/// A Limit card: a sticky note under the card it caps.
pub const LIMIT_W: f32 = 220.;
pub const LIMIT_H: f32 = 72.;
/// How far under its target a Limit card sits until it is placed.
const LIMIT_GAP: f32 = 32.;
/// An IN or OUT row.
pub const IO_ROW: f32 = 36.;
/// A step's head: number, title, two lines of brief; a run adds its chips.
pub const STEP_HEAD: f32 = 106.;
/// Without a brief the head only needs room for the title and passport.
pub const STEP_HEAD_EMPTY: f32 = 86.;
pub const RUN_CHIPS: f32 = 30.;
/// The seats' body: its padding, each seat and the gap between them.
pub const BODY_PAD: f32 = 18.;
pub const SEAT_H: f32 = 68.;
pub const SEAT_GAP: f32 = 8.;
/// The dashed box holding an orchestrated step's workers or a peer group.
pub const POOL_PAD: f32 = 24.;
/// An Input card: eyebrow and title, goal, then one line for each fact.
pub const INPUT_HEAD: f32 = 80.;
pub const INPUT_GOAL: f32 = 46.;
pub const INPUT_LINE: f32 = 24.;
pub const INPUT_FOOT: f32 = 14.;
/// A port's dot, and how near a drop must land to take it.
pub const PORT: f32 = 18.;
pub const PORT_REACH: f32 = 16.;
const GAP_X: f32 = 80.;
const GAP_Y: f32 = 40.;
const MARGIN: f32 = 40.;
/// How far above the board a wire back runs, and how far out of its ports.
const LOOP: f32 = 24.;
pub const LOOP_OUT: f32 = 24.;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Input,
    Step,
    Gate,
    End,
    Limit,
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

/// A wire from one port to another, and what it routes.
#[derive(Clone, Debug, PartialEq)]
pub struct Wire {
    pub id: String,
    pub from: (f32, f32),
    pub to: (f32, f32),
    /// `pass`, `fail`, `judge`, `limit` (a Limit card to what it caps),
    /// or blank for work.
    pub route: String,
    pub from_node: String,
    pub to_node: String,
    /// A wire that cannot curve straight into its target runs along this
    /// height instead of behind the cards between: above the board for a
    /// loop back, through the gap between rows for a forward wire whose
    /// target stands on a row below (a wrapped chain).
    pub lane: Option<f32>,
    /// It returns to a step the run met already (Flow's loop), whichever
    /// way the cards stand: drawn dashed, labelled with where it returns.
    pub back: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Board {
    pub cards: Vec<Card>,
    pub wires: Vec<Wire>,
}

/// Whether a step's workers or peers sit in a pool under its controller.
pub fn pooled(kind: ahjo_missions_contract::Formation) -> bool {
    matches!(
        kind,
        ahjo_missions_contract::Formation::Orchestrated | ahjo_missions_contract::Formation::Peer
    )
}

/// The height of the seats' body for `seats` seats.
pub fn body_h(kind: ahjo_missions_contract::Formation, seats: usize) -> f32 {
    let n = seats.max(1) as f32;
    BODY_PAD * 2. + n * SEAT_H + (n - 1.) * SEAT_GAP + if pooled(kind) { POOL_PAD } else { 0. }
}

/// The face and the board must agree on the head height so ports meet rows.
pub fn step_head(f: &ahjo_missions_contract::Step, running: bool) -> f32 {
    let has_brief = f.brief.as_ref().is_some_and(|b| !b.goal.trim().is_empty());
    (if has_brief {
        STEP_HEAD
    } else {
        STEP_HEAD_EMPTY
    }) + if running { RUN_CHIPS } else { 0. }
}

fn input_lines(c: &ahjo_missions_contract::InputCard) -> f32 {
    // Inputs, and the human-gate channel.
    1. + f32::from(u8::from(!c.inputs.is_empty()))
}

fn size(m: &Mission, id: &str, running: bool) -> Option<(Kind, f32, f32)> {
    if let Some(c) = m.input_cards.iter().find(|c| c.id == id) {
        let h = INPUT_HEAD + INPUT_GOAL + INPUT_LINE * input_lines(c) + INPUT_FOOT;
        return Some((Kind::Input, INPUT_W, h));
    }
    if let Some(f) = m.steps.iter().find(|f| f.id == id) {
        let w = if pooled(f.formation) { WIDE_W } else { STEP_W };
        let rows = (f.inputs.len() + f.outputs.len()) as f32;
        let head = step_head(f, running);
        return Some((
            Kind::Step,
            w,
            rows * IO_ROW + head + body_h(f.formation, f.seats.len()),
        ));
    }
    if m.gates.iter().any(|g| g.id == id) {
        return Some((Kind::Gate, GATE_W, GATE_H));
    }
    if m.ends.iter().any(|e| e.id == id) {
        return Some((Kind::End, END_W, END_H));
    }
    if m.limits.iter().any(|l| l.id == id) {
        return Some((Kind::Limit, LIMIT_W, LIMIT_H));
    }
    None
}

/// Run order, left to right: Input cards first, each numbered step in its
/// column (siblings stacked), a gate's judges under it, Ends last.
pub fn arranged(m: &Mission, flow: &Flow) -> Vec<LayoutNode> {
    let columns = columns(m, flow);
    let mut out = vec![];
    let mut x = MARGIN;
    for col in &columns {
        let mut y = MARGIN;
        let mut w: f32 = 0.;
        for id in col {
            let Some((_, cw, ch)) = size(m, id, false) else {
                continue;
            };
            out.push(LayoutNode {
                id: id.clone(),
                x: x as i64,
                y: y as i64,
            });
            y += ch + GAP_Y;
            w = w.max(cw);
        }
        x += w + GAP_X;
    }
    out
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
        // A gate's judges sit under it, in a stable order.
        let mut judges: Vec<&String> = flow
            .judge_of
            .iter()
            .filter(|(_, gate)| **gate == id)
            .map(|(judge, _)| judge)
            .collect();
        judges.sort();
        columns[n].extend(judges.into_iter().cloned());
    }
    columns.push(m.ends.iter().map(|e| e.id.clone()).collect());
    columns.retain(|c| !c.is_empty());
    columns
}

/// The room between two rows of a wrapped board, for the wires that run
/// from the end of one row to the start of the next.
const ROW_GAP: f32 = 2. * GAP_Y;

/// A column's cards with where each sits in it, then its width and height.
type Column = (Vec<(String, f32)>, f32, f32);

/// The mission laid out for a pane of `room` (width, height, in pixels)
/// that only shows it: the run-order columns wrapped into the rows that
/// draw its cards largest there, so a tall pane gets a column of rows and
/// a wide one a single row. When even the best wrap draws them smaller
/// than `floor`, the rows are as wide as still fit across at `floor`, and
/// the board runs down past the pane. For display only: never saved.
pub fn shaped(m: &Mission, flow: &Flow, room: (f32, f32), floor: f32) -> Vec<LayoutNode> {
    let room = (room.0.max(1.), room.1.max(1.));
    // Each column's cards, where each sits in it, its width and height;
    // a Limit card keeps its room under the card it caps.
    let columns: Vec<Column> = columns(m, flow)
        .into_iter()
        .map(|ids| {
            let (mut y, mut w) = (0f32, 0f32);
            let mut placed = vec![];
            for id in ids {
                let Some((_, cw, ch)) = size(m, &id, false) else {
                    continue;
                };
                let capped = m.limits.iter().filter(|l| l.target == id).count() as f32;
                placed.push((id, y));
                y += ch + GAP_Y;
                if capped > 0. {
                    y += LIMIT_GAP + capped * LIMIT_H + (capped - 1.) * GAP_Y / 2.;
                }
                w = w.max(cw);
            }
            (placed, w, (y - GAP_Y).max(0.))
        })
        .filter(|(placed, ..)| !placed.is_empty())
        .collect();
    if columns.is_empty() {
        return vec![];
    }
    let extent = |n: usize| {
        let rows: Vec<_> = columns.chunks(n).collect();
        let w = rows
            .iter()
            .map(|r| r.iter().map(|c| c.1).sum::<f32>() + GAP_X * (r.len() - 1) as f32)
            .fold(0., f32::max);
        let h = rows
            .iter()
            .map(|r| r.iter().map(|c| c.2).fold(0., f32::max))
            .sum::<f32>()
            + ROW_GAP * (rows.len() - 1) as f32;
        (w, h)
    };
    let zoom = |n: usize| {
        let (w, h) = extent(n);
        (room.0 / w).min(room.1 / h).min(1.)
    };
    let all = columns.len();
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
    let mut out = vec![];
    let mut y = MARGIN;
    for row in columns.chunks(per_row) {
        let mut x = MARGIN;
        for (placed, w, _) in row {
            for (id, dy) in placed {
                out.push(LayoutNode {
                    id: id.clone(),
                    x: x as i64,
                    y: (y + dy) as i64,
                });
            }
            x += w + GAP_X;
        }
        y += row.iter().map(|c| c.2).fold(0., f32::max) + ROW_GAP;
    }
    out
}

fn port_of(endpoint: &str) -> (&str, &str) {
    endpoint.split_once(':').unwrap_or((endpoint, ""))
}

/// Where a port is: inputs on the left on their IN rows, outputs on the
/// right on their OUT rows, a gate's pass and fail on its right and its
/// judge port under it.
fn port_at(m: &Mission, c: &Card, port: &str, out: bool) -> (f32, f32) {
    let side = if out { c.x + c.w } else { c.x };
    match c.kind {
        Kind::Gate => match port {
            "pass" => (side, c.y + c.h / 3.),
            "fail" => (side, c.y + c.h * 2. / 3.),
            "judge" => (c.x + c.w / 2., c.y + c.h),
            _ => (side, c.y + c.h / 2.),
        },
        Kind::Step => {
            let f = m.steps.iter().find(|f| f.id == c.id);
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
    pub at: (f32, f32),
    pub out: bool,
    pub wired: bool,
}

/// Every port on the board. A gate's judge port is both: its question
/// leaves it and the verdict returns to it.
pub fn ports(m: &Mission, b: &Board) -> Vec<Port> {
    let mut out = vec![];
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
            Kind::Step => {
                if let Some(f) = m.steps.iter().find(|f| f.id == c.id) {
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
                add(c, "judge", true);
            }
            Kind::End => add(c, "in", false),
            Kind::Limit => {}
        }
    }
    out
}

/// The port a drop at `at` lands on: the nearest within reach that takes
/// a wire from `from` (an input for an output; the judge port takes
/// either way), never on the card the wire leaves.
pub fn drop_target<'a>(ports: &'a [Port], from: &str, at: (f32, f32)) -> Option<&'a Port> {
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
pub fn board(m: &Mission, flow: &Flow, layout: &[LayoutNode], running: bool) -> Board {
    let fallback = arranged(m, flow);
    let mut at: HashMap<&str, (f32, f32)> = fallback
        .iter()
        .map(|p| (p.id.as_str(), (p.x as f32, p.y as f32)))
        .collect();
    for p in layout {
        at.insert(p.id.as_str(), (p.x as f32, p.y as f32));
    }
    let ids = m
        .input_cards
        .iter()
        .map(|c| &c.id)
        .chain(m.steps.iter().map(|f| &f.id))
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
    for l in &m.limits {
        let placed = layout.iter().any(|p| p.id == l.id);
        let under = cards.iter().find(|c| c.id == l.target).map(|c| {
            let below = c.y + c.h + LIMIT_GAP;
            let taken = cards
                .iter()
                .filter(|o| o.kind == Kind::Limit && o.x == c.x && o.y >= below)
                .count() as f32;
            (c.x, below + taken * (LIMIT_H + GAP_Y / 2.))
        });
        let (x, y) = if placed {
            at[l.id.as_str()]
        } else {
            under.unwrap_or((MARGIN, MARGIN))
        };
        cards.push(Card {
            id: l.id.clone(),
            kind: Kind::Limit,
            x,
            y,
            w: LIMIT_W,
            h: LIMIT_H,
        });
    }
    let card = |id: &str| cards.iter().find(|c| c.id == id);
    let top = cards.iter().map(|c| c.y).fold(f32::MAX, f32::min);
    let number = |id: &str| flow.numbers.get(id).copied();
    let mut loops = 0.;
    let mut wires: Vec<Wire> = m
        .connections
        .iter()
        .filter_map(|c| {
            let (from_node, from_port) = port_of(&c.from);
            let (to_node, to_port) = port_of(&c.to);
            let (a, b) = (card(from_node)?, card(to_node)?);
            // A judge's verdict returns to the gate's judge port from below.
            let returning = to_port == "judge";
            let route = match (from_port, returning) {
                ("pass" | "fail" | "judge", _) => from_port.to_string(),
                (_, true) => "judge".into(),
                _ => String::new(),
            };
            let from = port_at(m, a, from_port, true);
            let to = if returning {
                (b.x + b.w / 2., b.y + b.h)
            } else {
                port_at(m, b, to_port, false)
            };
            // Back is told by the run's order, not by where the cards
            // stand: a chain wrapped onto a second row still reads forward.
            let back = route != "judge"
                && matches!((number(from_node), number(to_node)), (Some(a), Some(b)) if b <= a);
            // A loop back to a row above runs through the gap over its own
            // row; one within a row has a lane of its own above the board,
            // the later ones higher.
            let lane = if back && b.y + b.h <= a.y {
                Some(gap_above(&cards, a, b))
            } else if back {
                loops += 1.;
                Some(top - LOOP * loops)
            } else if route != "judge" && to.0 < from.0 + 2. * LOOP_OUT {
                Some(gutter(&cards, a, b))
            } else {
                None
            };
            Some(Wire {
                id: c.id.clone(),
                from,
                to,
                lane,
                back,
                route,
                from_node: from_node.to_string(),
                to_node: to_node.to_string(),
            })
        })
        .collect();
    for l in &m.limits {
        let (Some(a), Some(b)) = (card(&l.id), card(&l.target)) else {
            continue;
        };
        // From the note's top edge to the nearest edge of what it caps.
        let to = if b.y + b.h <= a.y {
            (b.x + b.w / 2., b.y + b.h)
        } else {
            (b.x + b.w / 2., b.y)
        };
        wires.push(Wire {
            id: l.id.clone(),
            from: (a.x + a.w / 2., a.y),
            to,
            route: "limit".into(),
            from_node: l.id.clone(),
            to_node: l.target.clone(),
            lane: None,
            back: false,
        });
    }
    Board { cards, wires }
}

/// The height a forward wire runs along to reach a card it cannot curve
/// into: the middle of the gap above the target when the target stands on
/// a row below, else just under both cards.
fn gutter(cards: &[Card], a: &Card, b: &Card) -> f32 {
    if b.y >= a.y + a.h {
        let (x0, x1) = (b.x.min(a.x), (a.x + a.w).max(b.x + b.w));
        let above = cards
            .iter()
            .filter(|c| c.y + c.h <= b.y && c.x < x1 && c.x + c.w > x0)
            .map(|c| c.y + c.h)
            .fold(a.y + a.h, f32::max);
        (above + b.y) / 2.
    } else {
        (a.y + a.h).max(b.y + b.h) + LOOP
    }
}

/// A third down the gap over `a`'s row, for a wire back from `a` to `b`
/// on a row above: clear of a forward wire, which crosses at its middle.
fn gap_above(cards: &[Card], a: &Card, b: &Card) -> f32 {
    let (x0, x1) = (b.x.min(a.x), (a.x + a.w).max(b.x + b.w));
    let above = cards
        .iter()
        .filter(|c| c.y + c.h <= a.y && c.x < x1 && c.x + c.w > x0)
        .map(|c| c.y + c.h)
        .fold(b.y + b.h, f32::max);
    above + (a.y - above) / 3.
}

/// How big a card of this kind is when it is new: one port each way, one
/// seat, no brief.
pub fn new_size(kind: Kind) -> (f32, f32) {
    match kind {
        Kind::Input => (INPUT_W, INPUT_HEAD + INPUT_GOAL + INPUT_LINE + INPUT_FOOT),
        Kind::Step => (
            STEP_W,
            2. * IO_ROW + STEP_HEAD_EMPTY + body_h(ahjo_missions_contract::Formation::Solo, 1),
        ),
        Kind::Gate => (GATE_W, GATE_H),
        Kind::End => (END_W, END_H),
        Kind::Limit => (LIMIT_W, LIMIT_H),
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

/// The smallest rectangle holding every card: x, y, width, height.
pub fn extent(b: &Board) -> (f32, f32, f32, f32) {
    if b.cards.is_empty() {
        return (0., 0., 1., 1.);
    }
    let x0 = b.cards.iter().map(|c| c.x).fold(f32::MAX, f32::min);
    let y0 = b.cards.iter().map(|c| c.y).fold(f32::MAX, f32::min);
    let x1 = b.cards.iter().map(|c| c.x + c.w).fold(f32::MIN, f32::max);
    let y1 = b.cards.iter().map(|c| c.y + c.h).fold(f32::MIN, f32::max);
    (x0, y0, x1 - x0, y1 - y0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::flow_tests::delivery;
    use ahjo_missions_contract::flow::build;

    #[test]
    fn an_unarranged_mission_reads_left_to_right_in_run_order() {
        let m = delivery();
        let b = board(&m, &build(&m), &[], false);
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
        // Sent back, it loops above every card rather than behind them.
        let top = b.cards.iter().map(|c| c.y).fold(f32::MAX, f32::min);
        assert!(fail.lane.is_some_and(|y| y < top));
        let pass = b.wires.iter().find(|w| w.route == "pass").unwrap();
        assert_eq!(pass.lane, None);
    }

    #[test]
    fn a_board_shown_in_a_pane_is_laid_out_to_the_panes_shape() {
        let m = delivery();
        let flow = build(&m);
        let rows = |room: (f32, f32), floor: f32| {
            let layout = shaped(&m, &flow, room, floor);
            // A row starts where a column's first card stands.
            let mut tops: Vec<i64> = layout
                .iter()
                .filter(|p| !layout.iter().any(|o| o.x == p.x && o.y < p.y))
                .map(|p| p.y)
                .collect();
            tops.sort();
            tops.dedup();
            (tops.len(), board(&m, &flow, &layout, false))
        };
        // A wide pane shows the mission as one row; a tall one wraps it,
        // so its cards are drawn larger than one long row would allow.
        let (wide, _) = rows((4000., 900.), 0.);
        assert_eq!(wide, 1);
        let (tall, b) = rows((700., 1400.), 0.);
        assert!(tall > 1, "a tall pane wraps the chain");
        let (_, _, w, h) = extent(&b);
        let (_, _, w1, h1) = extent(&board(&m, &flow, &arranged(&m, &flow), false));
        assert!((700. / w).min(1400. / h) > (700. / w1).min(1400. / h1));
        // Too small for the floor: the rows still fit across at the floor
        // and the board runs down instead.
        let (_, b) = rows((500., 300.), 0.6);
        assert!(extent(&b).2 * 0.6 <= 500.);
    }

    #[test]
    fn a_wire_wrapped_onto_the_next_row_reads_forward() {
        let m = delivery();
        let flow = build(&m);
        let b = board(&m, &flow, &shaped(&m, &flow, (700., 1400.), 0.), false);
        let card = |id: &str| b.cards.iter().find(|c| c.id == id).unwrap();
        let wrapped: Vec<&Wire> = b
            .wires
            .iter()
            .filter(|w| w.route != "judge" && w.route != "limit" && w.to.0 < w.from.0)
            .filter(|w| !matches!((flow.numbers.get(&w.from_node), flow.numbers.get(&w.to_node)), (Some(a), Some(b)) if b <= a))
            .collect();
        assert!(!wrapped.is_empty(), "the pane wraps a forward wire");
        for w in wrapped {
            assert!(!w.back, "{} reads forward", w.id);
            // It runs through the gap above its target, not over the board.
            let (a, t) = (card(&w.from_node), card(&w.to_node));
            let lane = w.lane.unwrap();
            assert!(
                lane > a.y + a.h && lane < t.y,
                "{} runs between the rows",
                w.id
            );
        }
        // A gate sending work back still loops back, wherever it stands,
        // over the gate's row and clear of the card it returns to.
        let fail = b.wires.iter().find(|w| w.route == "fail").unwrap();
        assert!(fail.back);
        let (gate, to) = (card(&fail.from_node), card(&fail.to_node));
        let lane = fail.lane.unwrap();
        assert!(lane < gate.y && (lane < to.y || lane > to.y + to.h));
    }

    #[test]
    fn every_wire_ends_at_the_named_visible_ports() {
        let mission = delivery();
        assert!(mission.steps.len() >= 5);
        let board = board(&mission, &build(&mission), &[], false);
        let ports = ports(&mission, &board);
        assert_eq!(board.wires.len(), mission.connections.len());
        for connection in &mission.connections {
            let wire = board
                .wires
                .iter()
                .find(|wire| wire.id == connection.id)
                .unwrap();
            let source = ports
                .iter()
                .find(|port| port.endpoint == connection.from)
                .unwrap();
            let target = ports
                .iter()
                .find(|port| port.endpoint == connection.to)
                .unwrap();
            assert_eq!(wire.from, source.at, "source {}", connection.id);
            assert_eq!(wire.to, target.at, "target {}", connection.id);
        }
    }

    #[test]
    fn a_new_card_lands_in_free_space_in_view() {
        let m = delivery();
        let b = board(&m, &build(&m), &[], false);
        let (x0, y0, w, h) = extent(&b);
        let size = new_size(Kind::Step);
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
        let b = board(&m, &build(&m), &saved, false);
        let plan = b.cards.iter().find(|c| c.id == "fmn_plan").unwrap();
        assert_eq!((plan.x, plan.y), (900., 700.));
        let into = b.wires.iter().find(|w| w.to_node == "fmn_plan").unwrap();
        assert_eq!(into.to.0, 900.);
    }

    #[test]
    fn empty_briefs_leave_no_band_and_outputs_follow_the_card() {
        let mut m = delivery();
        let id = m.steps[0].id.clone();
        m.steps[0].brief = Some(ahjo_missions_contract::Brief {
            goal: "A brief".into(),
            ..Default::default()
        });
        let full = board(&m, &build(&m), &[], false);
        m.steps[0].brief.as_mut().unwrap().goal = " \n ".into();
        let compact = board(&m, &build(&m), &[], false);
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
        assert_eq!(step_head(&m.steps[0], true), STEP_HEAD_EMPTY + RUN_CHIPS);
    }

    #[test]
    fn a_wire_drops_on_the_input_port_nearest_the_pointer() {
        let m = delivery();
        let b = board(&m, &build(&m), &[], false);
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
