//! A mission on its canvas: where each card sits, how big it is, where its
//! ports are, and the wires between them, in canvas units before zoom. A
//! mission with no saved layout reads in rows, in run order, shaped for the
//! room it is shown in, until it is arranged.
//!
//! The board draws only what a person reads the flow by: the start, the
//! steps, the gates and the ends. A gate's judge is drawn in its gate, and
//! a limit on what it caps (a loop's rounds along the loop), so neither is
//! a card of its own.

use ahjo_missions_contract::flow::Flow;
use ahjo_missions_contract::{LayoutCard, Mission};
use std::collections::HashMap;

/// A step: its name over who works it.
pub const STEP_W: f32 = 216.;
pub const STEP_H: f32 = 62.;
/// The row of an orchestrated step's workers, under its lead.
pub const CREW_ROW: f32 = 22.;
/// The row a run's state takes on a step.
pub const RUN_ROW: f32 = 22.;
/// The start: a round play control.
pub const START: f32 = 36.;
/// The room under the start for what a run takes and the mission's cap.
pub const START_WORDS: f32 = 60.;
/// An end: its mark and its name.
pub const END_W: f32 = 132.;
pub const END_H: f32 = 28.;
/// A card's inner padding across.
pub const CARD_PAD: f32 = 12.;
/// A gate's condition: its type size and line, the most lines it takes
/// before it is cut (the whole of it then on hover), and a glyph's advance
/// per unit of type size, at the widest of the faces the themes use, so a
/// condition is never given fewer lines than it wraps to.
pub const CRITERION_SIZE: f32 = 11.5;
pub const CRITERION_LINE: f32 = 17.;
pub const CRITERION_MOST: usize = 10;
/// A glyph's width for its size, roughly, for guessing how text wraps.
pub const GLYPH: f32 = 0.62;
/// A port's dot, and how near a drop must land to take it.
pub const PORT: f32 = 9.;
pub const PORT_REACH: f32 = 16.;
const GAP_X: f32 = 64.;
const GAP_Y: f32 = 28.;
const MARGIN: f32 = 40.;
/// The room between two rows of a wrapped board, for the wires that run
/// from the end of one row to the start of the next and the loops under.
const ROW_GAP: f32 = 92.;

/// A wire leaves its port straight for a stub, turns at right angles with
/// rounded corners, and keeps a lane's clearance from the cards it passes.
const STUB: f32 = 24.;
const LANE_CLEARANCE: f32 = 30.;
const SPAN_PAD: f32 = 14.;
const OBSTACLE_PAD: f32 = 6.;
const LEVEL: f32 = 6.;
/// Loops over the same span stand this far apart, the wider outside.
const CHANNEL_GAP: f32 = 12.;
/// The radius a wire's corner is drawn with.
pub const CORNER: f32 = 10.;

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

type P = (f32, f32);

/// A wire from one port to another, the way it runs, and what it routes.
#[derive(Clone, Debug, PartialEq)]
pub struct Wire {
    pub id: String,
    /// Its course, port to port: straight runs meeting at right angles,
    /// clear of the cards between.
    pub points: Vec<P>,
    /// `pass`, `fail`, or blank for work.
    pub route: String,
    pub from_card: String,
    pub to_card: String,
    /// It returns to a step the run met already (Flow's loop), whichever
    /// way the cards stand: drawn dashed.
    pub back: bool,
}

impl Wire {
    pub fn from(&self) -> P {
        self.points[0]
    }
    pub fn to(&self) -> P {
        self.points[self.points.len() - 1]
    }
    /// The middle of its longest run: where a mark sits on it.
    pub fn middle(&self) -> P {
        self.points
            .windows(2)
            .max_by(|a, b| span(a[0], a[1]).total_cmp(&span(b[0], b[1])))
            .map(|w| ((w[0].0 + w[1].0) / 2., (w[0].1 + w[1].1) / 2.))
            .unwrap_or(self.from())
    }
}

/// The wire nearest `p` within `reach` of it: what a click there picks.
pub fn wire_at(wires: &[Wire], p: P, reach: f32) -> Option<&Wire> {
    wires
        .iter()
        .map(|w| {
            let near = w.points.windows(2).map(|s| to_run(p, s[0], s[1]));
            (w, near.fold(f32::INFINITY, f32::min))
        })
        .filter(|(_, d)| *d <= reach)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(w, _)| w)
}

/// How far `p` is from the run `a` to `b`.
fn to_run(p: P, a: P, b: P) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length = dx * dx + dy * dy;
    let t = if length == 0. {
        0.
    } else {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / length).clamp(0., 1.)
    };
    (a.0 + t * dx - p.0).hypot(a.1 + t * dy - p.1)
}

fn span(a: P, b: P) -> f32 {
    (a.0 - b.0).abs() + (a.1 - b.1).abs()
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Board {
    pub cards: Vec<Card>,
    pub wires: Vec<Wire>,
}

/// Whether a step's workers or peers work under its controller.
pub fn pooled(kind: ahjo_missions_contract::Formation) -> bool {
    matches!(
        kind,
        ahjo_missions_contract::Formation::Orchestrated | ahjo_missions_contract::Formation::Peer
    )
}

/// A judge step: drawn in the gate it judges, not as a card.
pub fn folded(flow: &Flow, id: &str) -> bool {
    flow.judge_of.contains_key(id)
}

fn size(m: &Mission, flow: &Flow, id: &str, running: bool) -> Option<(Kind, f32, f32)> {
    if m.input_cards.iter().any(|c| c.id == id) {
        return Some((Kind::Input, START, START));
    }
    if let Some(f) = m.steps.iter().find(|f| f.id == id) {
        if folded(flow, id) {
            return None;
        }
        let h = STEP_H
            + if pooled(f.formation) { CREW_ROW } else { 0. }
            + if running { RUN_ROW } else { 0. };
        return Some((Kind::Step, STEP_W, h));
    }
    if let Some(g) = m.gates.iter().find(|g| g.id == id) {
        let f = gate_frame(&g.criterion);
        return Some((Kind::Gate, f.w, f.h));
    }
    if m.ends.iter().any(|e| e.id == id) {
        return Some((Kind::End, END_W, END_H));
    }
    None
}

/// A gate's card for its condition (`gate_art`), which grows down with
/// the condition's lines.
pub fn gate_frame(criterion: &str) -> super::gate_art::Frame {
    super::gate_art::frame(criterion_lines(criterion, super::gate_art::MEASURE_W))
}

/// The lines `text` wraps to at `width`, word by word, at most
/// `CRITERION_MOST`.
pub fn criterion_lines(text: &str, width: f32) -> usize {
    wrapped_lines(text, width).clamp(1, CRITERION_MOST)
}

/// The lines `text` wraps to at `width`, word by word, a word longer
/// than a line broken across lines.
pub fn wrapped_lines(text: &str, width: f32) -> usize {
    let per_line = ((width / (CRITERION_SIZE * GLYPH)) as usize).max(1);
    let (mut lines, mut used) = (0usize, 0usize);
    for word in text.split_whitespace() {
        let n = word.chars().count();
        if used > 0 && used + 1 + n <= per_line {
            used += 1 + n;
        } else {
            lines += if used > 0 { 1 } else { 0 } + n.saturating_sub(1) / per_line;
            used = (n - 1) % per_line + 1;
        }
    }
    lines + usize::from(used > 0)
}

/// Run order, left to right in one row: what a card the layout misses
/// falls back to.
pub fn arranged(m: &Mission, flow: &Flow) -> Vec<LayoutCard> {
    shaped(m, flow, (f32::MAX, f32::MAX), 0., &|_| 0.)
}

/// The run-order columns, each top to bottom: Input cards, each numbered
/// step (siblings stacked), Ends, then the cards wired to nothing yet, so one just
/// added moves none of the rest.
fn columns(m: &Mission, flow: &Flow) -> Vec<Vec<String>> {
    let wired = |id: &str| {
        m.connections
            .iter()
            .any(|c| port_of(&c.from).0 == id || port_of(&c.to).0 == id)
    };
    let mut loose = vec![];
    let mut columns: Vec<Vec<String>> = vec![m.input_cards.iter().map(|c| c.id.clone()).collect()];
    let mut by_number: Vec<(usize, String)> = flow
        .numbers
        .iter()
        .map(|(id, n)| (*n, id.clone()))
        .collect();
    by_number.sort();
    let dropped = dropped_ends(m);
    for (n, id) in by_number {
        if !wired(&id) && !m.ends.iter().any(|e| e.id == id) {
            loose.push(id);
            continue;
        }
        while columns.len() <= n {
            columns.push(vec![]);
        }
        columns[n].push(id.clone());
        for (end, gate) in &dropped {
            if *gate == id {
                columns[n].push(end.clone());
            }
        }
    }
    columns.push(
        m.ends
            .iter()
            .filter(|e| !dropped.contains_key(&e.id))
            .map(|e| e.id.clone())
            .collect(),
    );
    columns.push(loose);
    columns.retain(|c| !c.is_empty());
    columns
}

/// The ends only a gate's fail reaches, each with its gate: they stand
/// under the gate, so the fail drops straight into them.
fn dropped_ends(m: &Mission) -> HashMap<String, String> {
    m.ends
        .iter()
        .filter_map(|e| {
            let into: Vec<_> = m
                .connections
                .iter()
                .filter(|c| port_of(&c.to).0 == e.id)
                .collect();
            match into.as_slice() {
                [c] if port_of(&c.from).1 == "fail" => {
                    Some((e.id.clone(), port_of(&c.from).0.to_owned()))
                }
                _ => None,
            }
        })
        .collect()
}

/// A column's cards with where each sits in it (across, down), then its
/// width and height.
type Column = (Vec<(String, P)>, f32, f32);

/// Where a port is on a card of this kind standing at the origin.
fn port_offset(m: &Mission, flow: &Flow, id: &str, port: &str, out: bool) -> Option<P> {
    let (kind, w, h) = size(m, flow, id, false)?;
    let card = Card {
        id: id.into(),
        kind,
        x: 0.,
        y: 0.,
        w,
        h,
    };
    Some(port_at(m, &card, port, out))
}

/// The mission laid out in reading rows for a view of `room` (width,
/// height, in pixels): its run-order columns wrapped into the rows that
/// break the fewest loops and draw its cards largest there, so a tall view
/// gets a column of rows and a wide one a single row. When even the best wrap draws them smaller
/// than `floor`, the rows are as wide as still fit across at `floor`, and
/// the board runs down past the view. Each card keeps the room `under`
/// gives it beneath. Within a row the ways in stand on one line, so the
/// wires along it run straight.
pub fn shaped(
    m: &Mission,
    flow: &Flow,
    room: (f32, f32),
    floor: f32,
    under: &dyn Fn(&str) -> f32,
) -> Vec<LayoutCard> {
    let room = (room.0.max(1.), room.1.max(1.));
    // The way into a column: its first card's first input port.
    let way_in = |id: &str| -> f32 {
        let port = if m.steps.iter().any(|f| f.id == id) {
            m.steps
                .iter()
                .find(|f| f.id == id)
                .and_then(|f| f.inputs.first())
                .map(|p| p.id.clone())
                .unwrap_or_default()
        } else {
            "in".into()
        };
        let out = m.input_cards.iter().any(|c| c.id == id);
        port_offset(m, flow, id, if out { "out" } else { &port }, out).map_or(0., |p| p.1)
    };
    let dropped = dropped_ends(m);
    let columns: Vec<Column> = columns(m, flow)
        .into_iter()
        .map(|ids| {
            let (mut y, mut w) = (0f32, 0f32);
            let mut placed = vec![];
            for id in ids {
                let Some((_, cw, ch)) = size(m, flow, &id, false) else {
                    continue;
                };
                // An end under its gate stands right of the gate's fail,
                // which turns into it.
                let dx = dropped
                    .get(&id)
                    .and_then(|gate| port_offset(m, flow, gate, "fail", true))
                    .map_or(0., |p| p.0 + STUB);
                placed.push((id.clone(), (dx, y)));
                y += ch + under(&id) + GAP_Y;
                w = w.max(cw + dx);
            }
            (placed, w, (y - GAP_Y).max(0.))
        })
        .filter(|(placed, ..)| !placed.is_empty())
        .collect();
    if columns.is_empty() {
        return vec![];
    }
    // How far each column drops, wrapped `n` to a row, so its way in is
    // on its row's line, or level with the pass of a gate in the column
    // before it on that row. A column that starts a row follows nothing.
    // Only wired cards set the line: a card just added, wired to nothing,
    // must not move the rest.
    let wired = |id: &str| {
        m.connections
            .iter()
            .any(|c| port_of(&c.from).0 == id || port_of(&c.to).0 == id)
    };
    let drops_for = |n: usize| {
        let mut drops = vec![0f32; columns.len()];
        for (k, row) in columns.chunks(n).enumerate() {
            let baseline = row
                .iter()
                .map(|c| &c.0[0].0)
                .filter(|id| wired(id))
                .map(|id| way_in(id))
                .fold(0., f32::max);
            for (j, column) in row.iter().enumerate() {
                let i = k * n + j;
                let first = &column.0[0].0;
                let mut drop = baseline - way_in(first);
                if j > 0 {
                    let prev = &columns[i - 1];
                    for (id, (_, dy)) in &prev.0 {
                        let passes = m.connections.iter().any(|c| {
                            c.from == format!("{id}:{}", ahjo_missions_contract::GATE_PASS)
                                && port_of(&c.to).0 == first
                        });
                        if passes && let Some(pass) = port_offset(m, flow, id, "pass", true) {
                            drop = drop.max(drops[i - 1] + dy + pass.1 - way_in(first));
                        }
                    }
                }
                drops[i] = drop.max(0.);
            }
        }
        drops
    };
    let extent = |n: usize| {
        let drops = drops_for(n);
        let rows: Vec<_> = columns.chunks(n).collect();
        let w = rows
            .iter()
            .map(|r| r.iter().map(|c| c.1).sum::<f32>() + GAP_X * (r.len() - 1) as f32)
            .fold(0., f32::max);
        let h = rows
            .iter()
            .enumerate()
            .map(|(k, r)| {
                r.iter()
                    .enumerate()
                    .map(|(j, c)| c.2 + drops[k * n + j])
                    .fold(0., f32::max)
            })
            .sum::<f32>()
            + ROW_GAP * (rows.len() - 1) as f32;
        (w + 2. * MARGIN, h + 2. * MARGIN)
    };
    let zoom = |n: usize| {
        let (w, h) = extent(n);
        (room.0 / w).min(room.1 / h).min(1.)
    };
    let all = columns.len();
    // A wire back to an earlier column, a loop, reads best when its two
    // ends share a row.
    let column_of = |id: &str| {
        columns
            .iter()
            .position(|c| c.0.iter().any(|(p, _)| p == id))
    };
    let back: Vec<(usize, usize)> = m
        .connections
        .iter()
        .filter_map(|c| {
            let from = column_of(port_of(&c.from).0)?;
            let to = column_of(port_of(&c.to).0)?;
            (to <= from).then_some((from, to))
        })
        .collect();
    let broken = |n: usize| back.iter().filter(|(a, b)| a / n != b / n).count();
    // Among the wraps that read, the one that breaks the fewest loops,
    // then draws it largest, then has the fewest rows. A loop between rows
    // has to climb back past the row it leaves, so a smaller board that
    // keeps it whole reads better than a larger one that breaks it.
    let per_row = (1..=all)
        .filter(|n| zoom(*n) >= floor)
        .min_by(|a, b| {
            broken(*a)
                .cmp(&broken(*b))
                .then(zoom(*b).total_cmp(&zoom(*a)))
                .then(b.cmp(a))
        })
        .unwrap_or_else(|| {
            (1..=all)
                .rev()
                .find(|n| room.0 / extent(*n).0 >= floor)
                .unwrap_or(1)
        });
    let drops = drops_for(per_row);
    let mut out = vec![];
    let mut y = MARGIN;
    for (k, row) in columns.chunks(per_row).enumerate() {
        let mut x = MARGIN;
        let mut tallest = 0f32;
        for (j, (placed, w, h)) in row.iter().enumerate() {
            let drop = drops[k * per_row + j];
            for (id, (dx, dy)) in placed {
                out.push(LayoutCard {
                    id: id.clone(),
                    x: (x + dx) as i64,
                    y: (y + drop + dy) as i64,
                });
            }
            x += w + GAP_X;
            tallest = tallest.max(h + drop);
        }
        y += tallest + ROW_GAP;
    }
    out
}

fn port_of(endpoint: &str) -> (&str, &str) {
    endpoint.split_once(':').unwrap_or((endpoint, ""))
}

/// Where a port is: inputs on the left, outputs on the right, spread over
/// a step's name and who works it; a gate's pass on the way on and its
/// fail at its foot.
fn port_at(m: &Mission, c: &Card, port: &str, out: bool) -> P {
    let side = if out { c.x + c.w } else { c.x };
    match c.kind {
        Kind::Gate => {
            let criterion = m
                .gates
                .iter()
                .find(|g| g.id == c.id)
                .map(|g| g.criterion.as_str());
            let f = gate_frame(criterion.unwrap_or_default());
            match port {
                "pass" => (c.x + c.w, c.y + f.line_y),
                "fail" => (c.x + f.fail_x, c.y + c.h),
                _ => (c.x, c.y + f.line_y),
            }
        }
        Kind::Step => {
            let f = m.steps.iter().find(|f| f.id == c.id);
            let ports = f.map(|f| if out { &f.outputs } else { &f.inputs });
            let n = ports.map(|p| p.len()).unwrap_or(0).max(1);
            let i = ports
                .and_then(|p| p.iter().position(|p| p.id == port))
                .unwrap_or(0);
            (side, c.y + STEP_H * (i + 1) as f32 / (n + 1) as f32)
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

/// Every port on the board.
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
            }
            Kind::End => add(c, "in", false),
            Kind::Limit => {}
        }
    }
    out
}

/// The port a drop at `at` lands on: the nearest within reach that takes
/// a wire from `from`, never on the card the wire leaves.
pub fn drop_target<'a>(ports: &'a [Port], from: &str, at: P) -> Option<&'a Port> {
    let node = from.split(':').next().unwrap_or("");
    ports
        .iter()
        .filter(|p| !p.out && p.endpoint.split(':').next() != Some(node))
        .map(|p| (p, (p.at.0 - at.0).hypot(p.at.1 - at.1)))
        .filter(|(_, d)| *d <= PORT_REACH)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(p, _)| p)
}

/// The board for a mission at these places; a card the layout misses is
/// placed where the run-order arrangement puts it.
/// `running` gives step cards room for their run state.
pub fn board(m: &Mission, flow: &Flow, layout: &[LayoutCard], running: bool) -> Board {
    let fallback = arranged(m, flow);
    let mut at: HashMap<&str, P> = fallback
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
    let cards: Vec<Card> = ids
        .filter_map(|id| {
            let (kind, w, h) = size(m, flow, id, running)?;
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
    let card = |id: &str| cards.iter().find(|c| c.id == id);
    let number = |id: &str| flow.numbers.get(id).copied();
    let mut wires = vec![];
    let mut loops = vec![];
    let mut under = vec![];
    for c in &m.connections {
        let (from_card, from_port) = port_of(&c.from);
        let (to_card, to_port) = port_of(&c.to);
        let (Some(a), Some(b)) = (card(from_card), card(to_card)) else {
            continue;
        };
        let route = match from_port {
            "pass" | "fail" => from_port.to_string(),
            _ => String::new(),
        };
        let from = port_at(m, a, from_port, true);
        let to = port_at(m, b, to_port, false);
        // Back is told by the run's order, not by where the cards stand:
        // a chain wrapped onto a second row still reads forward.
        let back = matches!((number(from_card), number(to_card)), (Some(a), Some(b)) if b <= a)
            || (from_card == to_card);
        let others: Vec<&Card> = cards
            .iter()
            .filter(|o| o.id != a.id && o.id != b.id)
            .collect();
        let all: Vec<&Card> = cards.iter().collect();
        let points = if from_port == "fail" && back {
            under.push(wires.len());
            vec![from, to]
        } else if from_port == "fail" {
            // Down from the foot, then on into what it fails to.
            let corner = (from.0, to.1);
            if to.1 > from.1 + STUB && to.0 > from.0 && clear(&[from, corner, to], &others) {
                vec![from, corner, to]
            } else {
                let drop = (from.0, from.1 + STUB);
                let mut rest = route_wire(drop, to, b.y - LANE_CLEARANCE, &others);
                rest[0] = from;
                rest
            }
        } else if back {
            loops.push(wires.len());
            vec![from, to]
        } else {
            let points = route_wire(from, to, b.y - LANE_CLEARANCE, &others);
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
            route,
            from_card: from_card.to_string(),
            to_card: to_card.to_string(),
        });
    }
    // A loop that finds no clear way under goes over, as other loops do.
    loops.extend(route_under(&mut wires, &under, &cards));
    route_loops(&mut wires, &loops, &cards);
    Board { cards, wires }
}

/// A gate's fail back to a step it met already: down from the gate's
/// foot, back along a lane under the cards between, and up into the step
/// from its left. A step in a row above is reached round the rows' left
/// edge (`climb`). Loops going back to one place nest, the nearest inside,
/// so none crosses another. The loops it finds no clear way for are
/// returned.
fn route_under(wires: &mut [Wire], loops: &[usize], cards: &[Card]) -> Vec<usize> {
    let mut order = loops.to_vec();
    order.sort_by(|a, b| {
        let (p, q) = (wires[*a].from(), wires[*b].from());
        p.1.total_cmp(&q.1).then(p.0.total_cmp(&q.0))
    });
    let all: Vec<&Card> = cards.iter().collect();
    let mut stuck = vec![];
    let mut taken = 0.;
    for i in order {
        let (from, to) = (wires[i].from(), wires[i].to());
        let back = to.0 - STUB - taken;
        let target = cards.iter().find(|c| c.id == wires[i].to_card);
        let points = match target {
            Some(t) if t.y + t.h < from.1 - STUB => climb(from, to, back, t, cards, taken),
            // Under the row, the step must stand back from the gate.
            _ if back > from.0 => {
                stuck.push(i);
                continue;
            }
            _ => {
                let (left, right) = (back.min(from.0), back.max(from.0));
                let lane = cards
                    .iter()
                    .filter(|c| c.x < right + SPAN_PAD && c.x + c.w > left - SPAN_PAD)
                    .filter(|c| c.y < from.1 && c.y + c.h > to.1.min(from.1) - 1.)
                    .map(|c| c.y + c.h)
                    .fold(from.1, f32::max)
                    + LANE_CLEARANCE
                    + taken;
                vec![from, (from.0, lane), (back, lane), (back, to.1), to]
            }
        };
        if !clear(&points[1..points.len() - 1], &all) {
            stuck.push(i);
            continue;
        }
        wires[i].points = points;
        taken += CHANNEL_GAP;
    }
    stuck
}

/// A loop back to `target` in a row above the gate it leaves: down from
/// the gate's foot to a lane under its own row, back along it past the
/// rows' left edge, outside the wires that drop into the rows there, up
/// that edge to the gap under the target's row, along it, and up into the
/// target from its left. `taken` moves it out a channel on every leg, so a
/// loop placed later runs outside the earlier.
fn climb(from: P, to: P, back: f32, target: &Card, cards: &[Card], taken: f32) -> Vec<P> {
    let below = target.y + target.h;
    // The rows it passes: everything under the target's row, down to the
    // gate's own.
    let passed: Vec<&Card> = cards
        .iter()
        .filter(|c| c.y > below && c.y < from.1)
        .collect();
    let edge = cards
        .iter()
        .filter(|c| c.y + c.h > target.y && c.y < from.1)
        .map(|c| c.x)
        .fold(back, f32::min)
        - STUB
        - CHANNEL_GAP
        - taken;
    // Under its own row: clear of every card it runs back under.
    let mut lane = from.1 + LANE_CLEARANCE + taken;
    while let Some(c) = passed
        .iter()
        .find(|c| hits((from.0, lane), (edge, lane), c, OBSTACLE_PAD))
    {
        lane = c.y + c.h + LANE_CLEARANCE + taken;
    }
    // Over the row under the target's, outside the wire that wraps into
    // that row.
    let gap = passed
        .iter()
        .filter(|c| c.x < back + SPAN_PAD)
        .map(|c| c.y)
        .fold(below + ROW_GAP, f32::min)
        - LANE_CLEARANCE
        - CHANNEL_GAP
        - taken;
    vec![
        from,
        (from.0, lane),
        (edge, lane),
        (edge, gap),
        (back, gap),
        (back, to.1),
        to,
    ]
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
        let (Some(a), Some(b)) = (card(&wires[i].from_card), card(&wires[i].to_card)) else {
            continue;
        };
        // A fail leaves its gate's foot: down a stub, then out past the
        // gate's right before it rises.
        let drops = wires[i].route == "fail";
        let out = if drops {
            (a.x + a.w).max(from.0) + STUB
        } else {
            from.0 + STUB
        };
        let back = to.0 - STUB;
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
            if drops {
                let foot = from.1 + STUB + channel * CHANNEL_GAP;
                vec![
                    from,
                    (from.0, foot),
                    (out, foot),
                    (out, y),
                    (back, y),
                    (back, to.1),
                    to,
                ]
            } else {
                vec![from, (out, from.1), (out, y), (back, y), (back, to.1), to]
            }
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
        wires[i].points = points;
    }
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

/// How big a card of this kind is when it is new.
pub fn new_size(kind: Kind) -> (f32, f32) {
    match kind {
        Kind::Input => (START, START),
        Kind::Step => (STEP_W, STEP_H),
        Kind::Gate => {
            let f = gate_frame("");
            (f.w, f.h)
        }
        Kind::End => (END_W, END_H),
        Kind::Limit => (0., 0.),
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
        let m = delivery();
        let b = board(&m, &build(&m), &[], false);
        let x = |id: &str| b.cards.iter().find(|c| c.id == id).unwrap().x;
        assert!(x("mis_delivery") < x("fmn_plan"));
        assert!(x("fmn_plan") < x("fmn_beads"));
        assert!(x("fmn_execution") < x("fmn_final_review"));
        assert!(x("fmn_final_review") < x("end_delivered"));
        // The judge is drawn in its gate, not as a card, and its wires
        // with it.
        assert!(!b.cards.iter().any(|c| c.id == "fmn_beads_reviewer"));
        let drawn = m
            .connections
            .iter()
            .filter(|c| !c.from.contains("reviewer") && !c.to.contains("reviewer"))
            .filter(|c| !c.from.ends_with(":judge") && !c.to.ends_with(":judge"))
            .count();
        assert_eq!(b.wires.len(), drawn);
        let fail = b.wires.iter().find(|w| w.route == "fail").unwrap();
        assert_eq!(fail.to_card, "fmn_beads");
        // Sent back, it drops from the gate and runs back under the cards
        // it spans, just under them, dashed.
        assert!(fail.back);
        let lane = fail.points.iter().map(|p| p.1).fold(f32::MIN, f32::max);
        let (left, right) = (fail.to().0, fail.from().0);
        let spanned: Vec<&Card> = b
            .cards
            .iter()
            .filter(|c| c.x < right && c.x + c.w > left)
            .collect();
        let foot = spanned.iter().map(|c| c.y + c.h).fold(f32::MIN, f32::max);
        assert!(lane > foot && lane < foot + 2. * LANE_CLEARANCE, "{lane}");
        let pass = b.wires.iter().find(|w| w.route == "pass").unwrap();
        assert!(!pass.back);
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
        for layout in [vec![], shaped(&m, &flow, (700., 1400.), 0., &|_| 0.)] {
            let b = board(&m, &flow, &layout, false);
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
            let b = board(&m, &flow, &shaped(&m, &flow, room, 0., &|_| 0.), false);
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
            kind: Kind::Step,
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
        let m = delivery();
        let flow = build(&m);
        let rows = |room: (f32, f32), floor: f32| {
            let layout = shaped(&m, &flow, room, floor, &|_| 0.);
            // A row starts at the left margin (a column after a gate may
            // stand lower, level with the gate's pass). Cards stacked in
            // the first column (a judge, a limit) follow closer than a row
            // gap; the next row starts a full row gap further down.
            let b = board(&m, &flow, &layout, false);
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
        let (_, _, w1, h1) = extent(&board(&m, &flow, &arranged(&m, &flow), false));
        assert!((700. / w).min(1400. / h) > (700. / w1).min(1400. / h1));
        // Too small for the floor: the rows still fit across at the floor
        // and the board runs down instead.
        let (_, b) = rows((500., 300.), 0.6);
        assert!(extent(&b).2 * 0.6 <= 500.);
        // A card keeps the room it asks for under it: a step beside it in
        // run order stands that much lower.
        let mut m = delivery();
        let mut twin = m.steps[0].clone();
        twin.id = "fmn_twin".into();
        let into = format!("{}:{}", twin.id, twin.inputs[0].id);
        let from = m
            .connections
            .iter()
            .find(|c| c.to.starts_with(&format!("{}:", m.steps[0].id)))
            .unwrap()
            .from
            .clone();
        m.steps.push(twin);
        m.connections.push(ahjo_missions_contract::Connection {
            id: "edge_twin".into(),
            from,
            to: into,
        });
        let flow = build(&m);
        let at = |layout: &[LayoutCard], id: &str| layout.iter().find(|p| p.id == id).unwrap().y;
        let upper = m.steps[0].id.clone();
        let plain = shaped(&m, &flow, (4000., 900.), 0., &|_| 0.);
        let roomy = shaped(&m, &flow, (4000., 900.), 0., &|id| {
            if id == upper { 50. } else { 0. }
        });
        assert!(at(&plain, "fmn_twin") > at(&plain, &upper));
        assert_eq!(at(&roomy, "fmn_twin") - at(&plain, "fmn_twin"), 50);
    }

    #[test]
    fn a_gate_grows_to_show_its_whole_condition() {
        let width = 7. * CRITERION_SIZE * GLYPH * 1.01; // seven glyphs a line
        assert_eq!(wrapped_lines("", width), 0);
        assert_eq!(wrapped_lines("ship it", width), 1);
        assert_eq!(wrapped_lines("ship it now", width), 2);
        assert_eq!(wrapped_lines("abcdefghij", width), 2);
        assert_eq!(criterion_lines(&"word ".repeat(200), width), CRITERION_MOST);
        let mut m = delivery();
        let gate = m.gates[0].id.clone();
        let flow = build(&m);
        m.gates[0].criterion = "it builds".into();
        let short = size(&m, &flow, &gate, false).unwrap().2;
        m.gates[0].criterion = "the whole suite passes, slow tests included, and nothing \
            new is flaky across three runs on a clean checkout"
            .into();
        let tall = size(&m, &flow, &gate, false).unwrap().2;
        assert!(tall > short, "{tall} should exceed {short}");
    }

    #[test]
    fn a_loop_is_never_wrapped_between_rows_when_a_whole_one_reads() {
        let m = delivery();
        let flow = build(&m);
        for width in [700., 900., 1100., 1300., 1600.] {
            let layout = shaped(&m, &flow, (width, 2000.), 0.8, &|_| 0.);
            // On one row the two stand side by side, so their heights meet.
            let span = |id: &str| {
                let y = layout.iter().find(|p| p.id == id).unwrap().y as f32;
                (y, y + size(&m, &flow, id, false).unwrap().2)
            };
            let (step, gate) = (span("fmn_beads"), span("gate_beads_review"));
            assert!(
                step.0 < gate.1 && gate.0 < step.1,
                "at {width} the loop's ends are on two rows: {step:?} and {gate:?}"
            );
        }
    }

    /// A fix that two gates send back to, the way a mission is usually
    /// written: tests, then review, then the operator's word.
    fn ship_a_fix() -> Mission {
        serde_json::from_value(serde_json::json!({
            "input_card": [{"id": "inp", "inputs": [{"name": "report"}]}],
            "step": [{"id": "fix", "title": "Fix", "input": [{"id": "in"}], "output": [{"id": "out"}],
                      "seat": [{"id": "s", "harness": "claude-code"}]}],
            "gate": [
                {"id": "tests", "kinds": ["code"], "criterion": "Every test in the workspace passes, including the new one that reproduces the bug."},
                {"id": "review", "kinds": ["human"], "criterion": "The change fixes the cause rather than the symptom, touches nothing it does not need, and its test would catch the bug coming back."},
                {"id": "ship", "kinds": ["human"], "criterion": "You have read the diff and the review, and you are happy for this to go out today."}
            ],
            "end": [{"id": "shipped", "outcome": "done"}, {"id": "held", "outcome": "rejected"}],
            "limit": [{"id": "three", "scope": "fix", "rounds": 3, "when_spent": "fail"}],
            "connection": [
                {"id": "bug", "from": "inp:out", "to": "fix:in"},
                {"id": "fixed", "from": "fix:out", "to": "tests:in"},
                {"id": "red", "from": "tests:fail", "to": "fix:in"},
                {"id": "green", "from": "tests:pass", "to": "review:in"},
                {"id": "again", "from": "review:fail", "to": "fix:in"},
                {"id": "reviewed", "from": "review:pass", "to": "ship:in"},
                {"id": "ship", "from": "ship:pass", "to": "shipped:in"},
                {"id": "hold", "from": "ship:fail", "to": "held:in"}
            ]
        }))
        .unwrap()
    }

    /// Where a horizontal and a vertical run cross inside both.
    fn cross((a1, a2): (P, P), (b1, b2): (P, P)) -> bool {
        let (h, v) = if (a1.1 - a2.1).abs() < 0.5 {
            ((a1, a2), (b1, b2))
        } else {
            ((b1, b2), (a1, a2))
        };
        let inside = |x: f32, p: f32, q: f32| x > p.min(q) + 0.5 && x < p.max(q) - 0.5;
        (h.0.1 - h.1.1).abs() < 0.5
            && (v.0.0 - v.1.0).abs() < 0.5
            && inside(v.0.0, h.0.0, h.1.0)
            && inside(h.0.1, v.0.1, v.1.1)
    }

    #[test]
    fn loops_back_to_a_row_above_go_round_its_edge_and_never_tangle() {
        // A narrow pane, as on a laptop with a panel open, wraps the
        // mission; whatever the wrap, no wire crosses a card, no loop
        // crosses another and none runs along another.
        let m = ship_a_fix();
        let flow = build(&m);
        let mut climbed = 0;
        for width in (500..=1300).step_by(25) {
            for height in [600., 900.] {
                let layout = shaped(&m, &flow, (width as f32, height), 0.8, &|_| 0.);
                let b = board(&m, &flow, &layout, false);
                assert_eq!(crossing(&b), Vec::<String>::new(), "at {width}x{height}");
                let legs = |w: &Wire| -> Vec<(P, P)> {
                    w.points[1..w.points.len() - 1]
                        .windows(2)
                        .map(|s| (s[0], s[1]))
                        .collect()
                };
                let fix = b.cards.iter().find(|c| c.id == "fix").unwrap();
                for a in b.wires.iter().filter(|w| w.back) {
                    let gate = b.cards.iter().find(|c| c.id == a.from_card).unwrap();
                    if gate.y > fix.y + fix.h {
                        climbed += 1;
                    }
                    for other in b.wires.iter().filter(|w| w.id != a.id) {
                        for p in legs(a) {
                            for q in legs(other) {
                                assert!(
                                    !shares(p, q) && !(other.back && cross(p, q)),
                                    "at {width}x{height} {} and {} meet: {:?} {:?}",
                                    a.id,
                                    other.id,
                                    a.points,
                                    other.points
                                );
                            }
                        }
                    }
                }
            }
        }
        assert!(climbed > 0, "some width puts a loop's gate under its step");
    }

    #[test]
    fn a_card_wired_to_nothing_leaves_the_others_where_they_were() {
        let mut m = delivery();
        let before = shaped(&m, &build(&m), (4000., 900.), 0., &|_| 0.);
        // A gate just added: its way in sits lower than a step's.
        let mut gate = m.gates[0].clone();
        gate.id = "gate_new".into();
        m.gates.push(gate);
        let after = shaped(&m, &build(&m), (4000., 900.), 0., &|_| 0.);
        for p in &before {
            let now = after.iter().find(|q| q.id == p.id).unwrap();
            assert_eq!((now.x, now.y), (p.x, p.y), "{} moved", p.id);
        }
    }

    #[test]
    fn every_wrapped_row_starts_a_row_gap_under_the_one_above() {
        // A column that starts a row stands on that row's line, not lower
        // to follow a gate's pass in the row above.
        let m = delivery();
        let flow = build(&m);
        let mut wrapped = 0;
        for width in (300..=2400).step_by(50) {
            let b = board(
                &m,
                &flow,
                &shaped(&m, &flow, (width as f32, 4000.), 0.8, &|_| 0.),
                false,
            );
            let mut cards: Vec<&Card> = b.cards.iter().collect();
            cards.sort_by(|p, q| p.y.total_cmp(&q.y));
            let mut bottom = cards[0].y + cards[0].h;
            for c in &cards[1..] {
                let gap = c.y - bottom;
                if gap >= ROW_GAP - 1. {
                    wrapped += 1;
                    assert!(
                        (gap - ROW_GAP).abs() < 1.,
                        "{width}: {} starts its row {gap} under the row above",
                        c.id
                    );
                }
                bottom = bottom.max(c.y + c.h);
            }
        }
        assert!(wrapped > 0, "some width wraps the mission");
    }

    #[test]
    fn a_wire_wrapped_onto_the_next_row_reads_forward() {
        let m = delivery();
        let flow = build(&m);
        let b = board(
            &m,
            &flow,
            &shaped(&m, &flow, (700., 1400.), 0., &|_| 0.),
            false,
        );
        let card = |id: &str| b.cards.iter().find(|c| c.id == id).unwrap();
        let wrapped: Vec<&Wire> = b
            .wires
            .iter()
            .filter(|w| w.route != "judge" && w.route != "limit" && w.to().0 < w.from().0)
            .filter(|w| !matches!((flow.numbers.get(&w.from_card), flow.numbers.get(&w.to_card)), (Some(a), Some(b)) if b <= a))
            .collect();
        assert!(!wrapped.is_empty(), "the pane wraps a forward wire");
        for w in wrapped {
            assert!(!w.back, "{} reads forward", w.id);
            // It runs through the gap above its target, not over the board.
            let (a, t) = (card(&w.from_card), card(&w.to_card));
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
        // under the gate and clear of the card it returns to.
        let fail = b.wires.iter().find(|w| w.route == "fail").unwrap();
        assert!(fail.back);
        let (gate, to) = (card(&fail.from_card), card(&fail.to_card));
        let lane = fail.points[1].1;
        assert!(lane > gate.y + gate.h && (lane < to.y || lane > to.y + to.h));
        assert_eq!(crossing(&b), Vec::<String>::new());
    }

    #[test]
    fn every_wire_ends_at_the_named_visible_ports() {
        let mission = delivery();
        assert!(mission.steps.len() >= 5);
        let board = board(&mission, &build(&mission), &[], false);
        let ports = ports(&mission, &board);
        // All but a judge's, which is drawn in its gate.
        let drawn: Vec<_> = mission
            .connections
            .iter()
            .filter(|c| !c.from.ends_with(":judge") && !c.to.ends_with(":judge"))
            .collect();
        assert_eq!(board.wires.len(), drawn.len());
        for connection in drawn {
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
            assert_eq!(wire.from(), source.at, "source {}", connection.id);
            assert_eq!(wire.to(), target.at, "target {}", connection.id);
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
        let saved = [LayoutCard {
            id: "fmn_plan".into(),
            x: 900,
            y: 700,
        }];
        let b = board(&m, &build(&m), &saved, false);
        let plan = b.cards.iter().find(|c| c.id == "fmn_plan").unwrap();
        assert_eq!((plan.x, plan.y), (900., 700.));
        let into = b.wires.iter().find(|w| w.to_card == "fmn_plan").unwrap();
        assert_eq!(into.to().0, 900.);
    }

    #[test]
    fn a_step_is_its_name_and_who_works_it_whatever_its_brief() {
        let mut m = delivery();
        let id = m.steps[0].id.clone();
        m.steps[0].brief = Some(ahjo_missions_contract::Brief {
            goal: "A brief".into(),
            ..Default::default()
        });
        let full = board(&m, &build(&m), &[], false);
        m.steps[0].brief.as_mut().unwrap().goal = " \n ".into();
        let compact = board(&m, &build(&m), &[], false);
        let running = board(&m, &build(&m), &[], true);
        let card = |b: &Board| b.cards.iter().find(|c| c.id == id).unwrap().clone();
        // A step is its name and who works it: its brief is read where it
        // is edited, and only a run adds a row, for its state.
        assert_eq!(card(&full).h, card(&compact).h);
        assert_eq!(card(&running).h - card(&compact).h, RUN_ROW);
        assert_eq!(ports(&m, &full), ports(&m, &compact));
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
        // On the card's left edge, level with its name and who works it.
        assert_eq!(into_beads.at, (beads.x, beads.y + STEP_H / 2.));
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
        // A gate's judge is drawn in it: no port to wire one by hand.
        assert!(
            !ports
                .iter()
                .any(|p| p.endpoint == "gate_beads_review:judge")
        );
    }

    #[test]
    fn a_click_picks_the_nearest_wire_within_reach() {
        let wire = |id: &str, points: Vec<P>| Wire {
            id: id.into(),
            points,
            route: String::new(),
            from_card: String::new(),
            to_card: String::new(),
            back: false,
        };
        let wires = [
            wire("low", vec![(0., 100.), (200., 100.)]),
            wire("bent", vec![(0., 0.), (100., 0.), (100., 90.)]),
        ];
        let at = |p| wire_at(&wires, p, 6.).map(|w| w.id.as_str());
        assert_eq!(at((50., 3.)), Some("bent"));
        assert_eq!(at((104., 50.)), Some("bent"), "its second run");
        assert_eq!(at((100., 97.)), Some("low"), "the nearer of the two");
        assert_eq!(at((150., 50.)), None, "out of reach of both");
        assert_eq!(at((210., 100.)), None, "past a wire's end");
    }
}
