//! How a tab's windows share it: a tree of rows (side by side) and columns
//! (stacked), nested as deep as the operator splits them, each leaf one of
//! the tab's windows by its index. Every part of a group carries its share,
//! a weight its siblings divide the group's room by, so a dragged divider
//! moves only the two windows either side of it.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Axis {
    /// Side by side.
    Row,
    /// Stacked.
    Column,
}

/// Where beside a window another one goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

impl Side {
    pub fn axis(self) -> Axis {
        match self {
            Side::Left | Side::Right => Axis::Row,
            Side::Top | Side::Bottom => Axis::Column,
        }
    }

    /// After the window it is beside: right of it or below it.
    pub fn after(self) -> bool {
        matches!(self, Side::Right | Side::Bottom)
    }

    pub fn word(self) -> &'static str {
        match self {
            Side::Left => "left",
            Side::Right => "right",
            Side::Top => "above",
            Side::Bottom => "below",
        }
    }
}

/// The share a new part of a group starts with, and a group's parts when
/// they are even.
pub const EVEN: u16 = 1000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Split {
    /// One window, by its index in the tab.
    Window(usize),
    /// Two or more parts along `axis`, each with its share.
    Group {
        axis: Axis,
        parts: Vec<(u16, Split)>,
    },
}

impl Default for Split {
    fn default() -> Self {
        Split::Window(0)
    }
}

/// Windows `from..to` along `axis`, evenly.
fn line(axis: Axis, from: usize, to: usize) -> Split {
    if to - from == 1 {
        return Split::Window(from);
    }
    Split::Group {
        axis,
        parts: (from..to).map(|w| (EVEN, Split::Window(w))).collect(),
    }
}

impl Split {
    /// `n` windows laid out evenly: a row of up to three, else two rows.
    pub fn even(n: usize) -> Self {
        let n = n.max(1);
        if n <= 3 {
            return line(Axis::Row, 0, n);
        }
        let top = n.div_ceil(2);
        Split::Group {
            axis: Axis::Column,
            parts: vec![
                (EVEN, line(Axis::Row, 0, top)),
                (EVEN, line(Axis::Row, top, n)),
            ],
        }
    }

    /// The layout an app stored before the tree: one row of up to three
    /// windows, or two rows of two sharing their columns, with the weights
    /// its dividers were dragged to for that count.
    pub fn from_columns(n: usize, dragged: Option<(&[u16], u16)>) -> Self {
        let mut tree = Split::even(n);
        let Some((columns, top)) = dragged else {
            return tree;
        };
        if columns.contains(&0) || !(1..1000).contains(&top) {
            return tree;
        }
        let weigh = |row: &mut Split| {
            if let Split::Group { parts, .. } = row
                && parts.len() == columns.len()
            {
                for (p, w) in parts.iter_mut().zip(columns) {
                    p.0 = *w;
                }
            }
        };
        match (&mut tree, n) {
            (Split::Group { parts, .. }, 4) if columns.len() == 2 => {
                parts[0].0 = top;
                parts[1].0 = 1000 - top;
                for (_, row) in parts.iter_mut() {
                    weigh(row);
                }
            }
            (row, 2 | 3) => weigh(row),
            _ => {}
        }
        tree
    }

    /// Its windows, in reading order: left to right, top to bottom.
    pub fn windows(&self) -> Vec<usize> {
        let mut out = vec![];
        self.walk(&mut out);
        out
    }

    fn walk(&self, out: &mut Vec<usize>) {
        match self {
            Split::Window(w) => out.push(*w),
            Split::Group { parts, .. } => parts.iter().for_each(|(_, p)| p.walk(out)),
        }
    }

    /// Whether its windows are exactly `0..n`, each once, in groups of two
    /// or more with shares.
    pub fn holds_exactly(&self, n: usize) -> bool {
        let mut w = self.windows();
        w.sort_unstable();
        w == (0..n).collect::<Vec<_>>() && self.well_formed()
    }

    fn well_formed(&self) -> bool {
        match self {
            Split::Window(_) => true,
            Split::Group { parts, .. } => {
                parts.len() >= 2 && parts.iter().all(|(s, p)| *s > 0 && p.well_formed())
            }
        }
    }

    /// Puts window `w` beside window `at`, on `side`: in `at`'s group when
    /// it runs the same way, sharing `at`'s room; else `at` and `w` become a
    /// group of their own in its place. False if `at` is not here.
    pub fn insert(&mut self, at: usize, side: Side, w: usize) -> bool {
        match self {
            Split::Window(x) if *x == at => {
                let pair = if side.after() {
                    [(EVEN, Split::Window(at)), (EVEN, Split::Window(w))]
                } else {
                    [(EVEN, Split::Window(w)), (EVEN, Split::Window(at))]
                };
                *self = Split::Group {
                    axis: side.axis(),
                    parts: pair.into(),
                };
                true
            }
            Split::Window(_) => false,
            Split::Group { axis, parts } => {
                let here = parts
                    .iter()
                    .position(|(_, p)| matches!(p, Split::Window(x) if *x == at));
                if let Some(k) = here
                    && *axis == side.axis()
                {
                    let share = parts[k].0;
                    let half = (share / 2).max(1);
                    parts[k].0 = share - half;
                    let at = if side.after() { k + 1 } else { k };
                    parts.insert(at, (half.max(1), Split::Window(w)));
                    return true;
                }
                parts.iter_mut().any(|(_, p)| p.insert(at, side, w))
            }
        }
    }

    /// Takes window `w` out; its room goes to the parts beside it. False
    /// when it is the only window, or not here.
    pub fn remove(&mut self, w: usize) -> bool {
        if matches!(self, Split::Window(_)) {
            return false;
        }
        let gone = self.cut(w);
        self.tidy();
        gone
    }

    fn cut(&mut self, w: usize) -> bool {
        let Split::Group { parts, .. } = self else {
            return false;
        };
        if let Some(k) = parts
            .iter()
            .position(|(_, p)| matches!(p, Split::Window(x) if *x == w))
        {
            parts.remove(k);
            return true;
        }
        parts.iter_mut().any(|(_, p)| p.cut(w))
    }

    /// A group of one is its part; a group inside one that runs the same
    /// way gives its parts to it, in its share.
    fn tidy(&mut self) {
        if let Split::Group { parts, .. } = self {
            for (_, p) in parts.iter_mut() {
                p.tidy();
            }
        }
        let Split::Group { axis, parts } = self else {
            return;
        };
        if parts.len() == 1 {
            *self = parts.remove(0).1;
            return;
        }
        let axis = *axis;
        let mut flat = Vec::with_capacity(parts.len());
        for (share, p) in parts.drain(..) {
            match p {
                Split::Group {
                    axis: a,
                    parts: inner,
                } if a == axis => {
                    let all: u32 = inner.iter().map(|(s, _)| u32::from(*s)).sum();
                    for (s, q) in inner {
                        let part = (u32::from(share) * u32::from(s) / all.max(1)).max(1);
                        flat.push((part.min(u32::from(u16::MAX)) as u16, q));
                    }
                }
                p => flat.push((share, p)),
            }
        }
        *parts = flat;
    }

    /// Moves window `w` beside window `to`, on `side`. False if either is
    /// not here, or they are one window.
    pub fn place(&mut self, w: usize, to: usize, side: Side) -> bool {
        let ws = self.windows();
        if w == to || !ws.contains(&w) || !ws.contains(&to) {
            return false;
        }
        self.remove(w) && self.insert(to, side, w)
    }

    /// Gives every window a new index.
    pub fn renumber(&mut self, f: &impl Fn(usize) -> usize) {
        match self {
            Split::Window(w) => *w = f(*w),
            Split::Group { parts, .. } => parts.iter_mut().for_each(|(_, p)| p.renumber(f)),
        }
    }

    /// The group at `path` (child indexes from the root).
    pub fn group_mut(&mut self, path: &[usize]) -> Option<(Axis, &mut Vec<(u16, Split)>)> {
        match (self, path.split_first()) {
            (Split::Group { axis, parts }, None) => Some((*axis, parts)),
            (Split::Group { parts, .. }, Some((k, rest))) => parts.get_mut(*k)?.1.group_mut(rest),
            _ => None,
        }
    }

    pub fn group(&self, path: &[usize]) -> Option<(Axis, &[(u16, Split)])> {
        match (self, path.split_first()) {
            (Split::Group { axis, parts }, None) => Some((*axis, parts)),
            (Split::Group { parts, .. }, Some((k, rest))) => parts.get(*k)?.1.group(rest),
            _ => None,
        }
    }

    /// The group window `w` is a part of, the path to it and `w`'s place
    /// in it; `None` when `w` is the whole layout.
    pub fn parent_of(&self, w: usize) -> Option<(Vec<usize>, usize)> {
        fn find(s: &Split, w: usize, path: &mut Vec<usize>) -> Option<usize> {
            let Split::Group { parts, .. } = s else {
                return None;
            };
            for (k, (_, p)) in parts.iter().enumerate() {
                if matches!(p, Split::Window(x) if *x == w) {
                    return Some(k);
                }
                path.push(k);
                if let Some(at) = find(p, w, path) {
                    return Some(at);
                }
                path.pop();
            }
            None
        }
        let mut path = vec![];
        find(self, w, &mut path).map(|k| (path, k))
    }

    /// Each window's rectangle in a room of `w` by `h`, gaps left out: what
    /// a window's size is judged by before anything is drawn.
    pub fn rects(&self, x: f32, y: f32, w: f32, h: f32, out: &mut Vec<(usize, [f32; 4])>) {
        match self {
            Split::Window(i) => out.push((*i, [x, y, w, h])),
            Split::Group { axis, parts } => {
                let all: f32 = parts.iter().map(|(s, _)| f32::from(*s)).sum();
                let mut at = 0.;
                for (s, p) in parts {
                    let f = f32::from(*s) / all;
                    match axis {
                        Axis::Row => p.rects(x + at * w, y, f * w, h, out),
                        Axis::Column => p.rects(x, y + at * h, w, f * h, out),
                    }
                    at += f;
                }
            }
        }
    }
}

impl Split {
    /// The window beside window `w` on `side`, as a desktop-shaped room
    /// lays them out: the nearest one that shares some of `w`'s edge,
    /// most of it first.
    pub fn neighbor(&self, w: usize, side: Side) -> Option<usize> {
        let mut rects = vec![];
        self.rects(0., 0., 16., 9., &mut rects);
        let [x, y, rw, rh] = rects.iter().find(|r| r.0 == w)?.1;
        let eps = 1e-3;
        rects
            .iter()
            .filter(|(o, _)| *o != w)
            .filter_map(|(o, [ox, oy, ow, oh])| {
                let (gap, overlap) = match side {
                    Side::Left => (x - (ox + ow), (y + rh).min(oy + oh) - y.max(*oy)),
                    Side::Right => (ox - (x + rw), (y + rh).min(oy + oh) - y.max(*oy)),
                    Side::Top => (y - (oy + oh), (x + rw).min(ox + ow) - x.max(*ox)),
                    Side::Bottom => (oy - (y + rh), (x + rw).min(ox + ow) - x.max(*ox)),
                };
                (gap > -eps && overlap > eps).then_some((gap, -overlap, *o))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)))
            .map(|(_, _, o)| o)
    }
}

/// Moves the divider after part `k` of a group by `by`, a fraction of the
/// room the two parts either side of it have together, leaving each at
/// least `least` of that room.
pub fn drag(shares: &mut [u16], k: usize, by: f32, least: f32) {
    let Some(pair) = shares.get(k..k + 2) else {
        return;
    };
    let both = f32::from(pair[0]) + f32::from(pair[1]);
    let least = least.clamp(0., 0.5);
    let left = (f32::from(pair[0]) + by * both).clamp(both * least, both * (1. - least));
    shares[k] = (left.round() as u16).max(1);
    shares[k + 1] = ((both - left).round() as u16).max(1);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(ws: &[usize]) -> Split {
        Split::Group {
            axis: Axis::Row,
            parts: ws.iter().map(|w| (EVEN, Split::Window(*w))).collect(),
        }
    }

    #[test]
    fn even_layouts_are_a_row_then_two_rows() {
        assert_eq!(Split::even(1), Split::Window(0));
        assert_eq!(Split::even(3), row(&[0, 1, 2]));
        let five = Split::even(5);
        assert_eq!(five.windows(), [0, 1, 2, 3, 4]);
        assert!(five.holds_exactly(5));
        assert!(!five.holds_exactly(4));
    }

    #[test]
    fn a_split_beside_shares_the_window_and_stacks_across() {
        let mut t = Split::even(2);
        // Right of 0, in the same row: 0's room is halved.
        assert!(t.insert(0, Side::Right, 2));
        assert_eq!(t.windows(), [0, 2, 1]);
        let Split::Group { parts, .. } = &t else {
            panic!()
        };
        assert_eq!(
            parts.iter().map(|p| p.0).collect::<Vec<_>>(),
            [500, 500, 1000]
        );
        // Below 1: a column in 1's place.
        assert!(t.insert(1, Side::Bottom, 3));
        assert_eq!(t.windows(), [0, 2, 1, 3]);
        assert_eq!(
            t.group(&[2]).map(|(a, p)| (a, p.len())),
            Some((Axis::Column, 2))
        );
        // Above 3, inside that column.
        assert!(t.insert(3, Side::Top, 4));
        assert_eq!(t.windows(), [0, 2, 1, 4, 3]);
        assert!(!t.insert(9, Side::Left, 5), "no window 9");
        assert!(t.holds_exactly(5));
    }

    #[test]
    fn a_window_taken_out_leaves_no_group_of_one() {
        let mut t = Split::even(2);
        t.insert(1, Side::Bottom, 2);
        assert!(t.remove(2));
        assert_eq!(t, row(&[0, 1]));
        assert!(t.remove(0));
        assert_eq!(t, Split::Window(1));
        assert!(!t.remove(1), "the last window stays");
        // A column taken down to one window inside a row joins the row.
        let mut t = Split::even(2);
        t.insert(1, Side::Bottom, 2);
        t.insert(2, Side::Right, 3);
        assert!(t.remove(1));
        assert_eq!(t.windows(), [0, 2, 3]);
        assert!(matches!(&t, Split::Group { axis: Axis::Row, parts } if parts.len() == 3));
    }

    #[test]
    fn a_window_moves_beside_another() {
        let mut t = Split::even(3);
        assert!(t.place(0, 2, Side::Bottom));
        assert_eq!(t.windows(), [1, 2, 0]);
        assert!(!t.place(1, 1, Side::Left));
        assert!(t.holds_exactly(3));
    }

    #[test]
    fn an_old_layout_keeps_its_dragged_dividers() {
        let t = Split::from_columns(4, Some((&[1500, 500], 300)));
        assert_eq!(t.windows(), [0, 1, 2, 3]);
        let (_, rows) = t.group(&[]).unwrap();
        assert_eq!((rows[0].0, rows[1].0), (300, 700));
        let (_, top) = t.group(&[0]).unwrap();
        assert_eq!((top[0].0, top[1].0), (1500, 500));
        let three = Split::from_columns(3, Some((&[200, 300, 500], 500)));
        let (_, parts) = three.group(&[]).unwrap();
        assert_eq!(
            parts.iter().map(|p| p.0).collect::<Vec<_>>(),
            [200, 300, 500]
        );
        assert_eq!(Split::from_columns(2, Some((&[0, 9], 500))), Split::even(2));
    }

    #[test]
    fn a_divider_leaves_each_side_its_least() {
        let mut s = [1000, 1000];
        drag(&mut s, 0, 0.25, 0.1);
        assert_eq!(s, [1500, 500]);
        drag(&mut s, 0, 0.9, 0.1);
        assert_eq!(s, [1800, 200]);
    }

    #[test]
    fn a_neighbor_is_the_window_across_the_nearest_edge() {
        // 0 | 1 over 2
        let mut t = Split::even(2);
        t.insert(1, Side::Bottom, 2);
        assert_eq!(t.neighbor(0, Side::Right), Some(1), "most of its edge");
        assert_eq!(t.neighbor(2, Side::Left), Some(0));
        assert_eq!(t.neighbor(1, Side::Bottom), Some(2));
        assert_eq!(t.neighbor(2, Side::Top), Some(1));
        assert_eq!(t.neighbor(0, Side::Left), None);
        assert_eq!(t.neighbor(0, Side::Top), None);
    }

    #[test]
    fn rectangles_follow_the_shares() {
        let mut t = Split::even(2);
        t.insert(1, Side::Bottom, 2);
        let mut out = vec![];
        t.rects(0., 0., 100., 50., &mut out);
        assert_eq!(out[0], (0, [0., 0., 50., 50.]));
        assert_eq!(out[2], (2, [50., 25., 50., 25.]));
    }
}
