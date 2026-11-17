//! Work over time (DESIGN.md, Work: how it moves): the items opened and
//! closed each day with the work open at each day's end, a range of days,
//! and what moved in it. Days are this machine's local days, counted from
//! the Unix epoch, so a range is the same days wherever it is drawn.
//!
//! Every tracker gives an item its created, updated and closed times; that
//! is all this reads, so GitHub issues and milestones join by giving their
//! rows those times.

use super::{Board, Row, Status};

/// Days since the Unix epoch, in local time.
pub type Day = i64;

const DAY_MS: i64 = 86_400_000;

/// Turns a time into its local day: one offset for the whole board, taken
/// when it was read (a summer-time change moves a day's edge by an hour).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clock {
    pub offset_ms: i64,
}

impl Clock {
    pub fn local(now: u64) -> Clock {
        Clock {
            offset_ms: super::when::local_offset(now) * 1000,
        }
    }
    pub fn day(self, ms: u64) -> Day {
        (ms as i64 + self.offset_ms).div_euclid(DAY_MS)
    }
}

/// Days `from` to `to`, both included.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Range {
    pub from: Day,
    pub to: Day,
}

impl Range {
    /// The range between two days, in either order.
    pub fn between(a: Day, b: Day) -> Range {
        Range {
            from: a.min(b),
            to: a.max(b),
        }
    }
    pub fn holds(self, day: Day) -> bool {
        (self.from..=self.to).contains(&day)
    }
    pub fn days(self) -> i64 {
        self.to - self.from + 1
    }
}

/// When a row was finished: its closed time, else (a tracker that keeps
/// none) its last update.
fn finished(r: &Row) -> Option<u64> {
    (r.status == Status::Closed).then(|| r.closed.or(r.updated))?
}

/// Each day's opened and closed counts and the work open at its end, for
/// the days `first..=last`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Timeline {
    pub first: Day,
    pub opened: Vec<u32>,
    pub closed: Vec<u32>,
    pub open: Vec<u32>,
}

impl Timeline {
    /// The `days` days ending `today`, over `rows` of `board`. The open
    /// count starts from everything opened before the first day and not
    /// finished by then, so the line starts at its true height.
    pub fn of(board: &Board, rows: &[usize], clock: Clock, today: Day, days: usize) -> Timeline {
        let n = days.max(1);
        let first = today - n as i64 + 1;
        let mut t = Timeline {
            first,
            opened: vec![0; n],
            closed: vec![0; n],
            open: vec![0; n],
        };
        // The change in open work on each day, and before the first.
        let mut delta = vec![0i64; n];
        let mut before = 0i64;
        let at = |day: Day| -> Option<usize> {
            (first..=today)
                .contains(&day)
                .then(|| (day - first) as usize)
        };
        for &i in rows {
            let r = &board.rows[i];
            let Some(created) = r.created.map(|c| clock.day(c)) else {
                continue;
            };
            match at(created) {
                Some(d) => {
                    t.opened[d] += 1;
                    delta[d] += 1;
                }
                None if created < first => before += 1,
                None => {}
            }
            if let Some(done) = finished(r).map(|c| clock.day(c).max(created)) {
                match at(done) {
                    Some(d) => {
                        t.closed[d] += 1;
                        delta[d] -= 1;
                    }
                    None if done < first => before -= 1,
                    None => {}
                }
            }
        }
        let mut level = before;
        for (d, change) in delta.into_iter().enumerate() {
            level += change;
            t.open[d] = level.max(0) as u32;
        }
        t
    }

    pub fn last(&self) -> Day {
        self.first + self.opened.len() as i64 - 1
    }

    /// The index of `day`, if the timeline holds it.
    pub fn index(&self, day: Day) -> Option<usize> {
        (self.first..=self.last())
            .contains(&day)
            .then(|| (day - self.first) as usize)
    }

    /// Opened and closed in `range`, as far as the timeline holds it.
    pub fn totals(&self, range: Range) -> (u32, u32) {
        let (mut opened, mut closed) = (0, 0);
        for day in range.from..=range.to {
            if let Some(d) = self.index(day) {
                opened += self.opened[d];
                closed += self.closed[d];
            }
        }
        (opened, closed)
    }
}

/// What moved in a range: what was opened, what was finished, and what
/// stayed open without anyone touching it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Movement {
    /// Opened in the range, newest first.
    pub opened: Vec<usize>,
    /// Finished in the range, newest first.
    pub closed: Vec<usize>,
    /// Open through the whole range and last touched before it: under way
    /// first (claimed and left), then the longest untouched.
    pub stuck: Vec<usize>,
}

/// What moved among `rows` in `range`.
pub fn movement(board: &Board, rows: &[usize], clock: Clock, range: Range) -> Movement {
    let mut m = Movement::default();
    for &i in rows {
        let r = &board.rows[i];
        let created = r.created.map(|c| clock.day(c));
        let done = finished(r).map(|c| clock.day(c));
        if created.is_some_and(|d| range.holds(d)) {
            m.opened.push(i);
        }
        if done.is_some_and(|d| range.holds(d)) {
            m.closed.push(i);
        }
        let open_through = created.is_some_and(|d| d < range.from)
            && done.is_none_or(|d| d > range.to)
            && r.status != Status::Deferred;
        let untouched = r.updated.is_some_and(|u| clock.day(u) < range.from);
        if open_through && untouched {
            m.stuck.push(i);
        }
    }
    let rows = &board.rows;
    m.opened
        .sort_by(|&a, &b| rows[b].created.cmp(&rows[a].created));
    m.closed
        .sort_by(|&a, &b| finished(&rows[b]).cmp(&finished(&rows[a])));
    m.stuck.sort_by(|&a, &b| {
        (rows[b].status == Status::InProgress)
            .cmp(&(rows[a].status == Status::InProgress))
            .then(rows[a].updated.cmp(&rows[b].updated))
    });
    m
}

/// Whether a row was opened, touched or finished in `range`.
pub fn touched(r: &Row, clock: Clock, range: Range) -> bool {
    [r.created, r.updated, finished(r)]
        .into_iter()
        .flatten()
        .any(|t| range.holds(clock.day(t)))
}

/// The day as the strip's axis says it: `Nov 3`.
pub fn day_label(day: Day) -> String {
    // A local day counted from the epoch names the same calendar date as
    // that many days after the epoch in UTC.
    let at = ::time::OffsetDateTime::from_unix_timestamp(day * 86_400 + 43_200)
        .unwrap_or(::time::OffsetDateTime::UNIX_EPOCH);
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!("{} {}", MONTHS[at.month() as usize - 1], at.day())
}

/// A range as the page names it: `Nov 3`, or `Nov 3 – Nov 9`.
pub fn range_label(range: Range) -> String {
    if range.from == range.to {
        day_label(range.from)
    } else {
        format!("{} – {}", day_label(range.from), day_label(range.to))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parvi_work_contract::{Item, ProjectView, Snapshot, StoreView};

    const UTC: Clock = Clock { offset_ms: 0 };
    /// 2026-11-10 in days since the epoch.
    const TODAY: Day = 20_402;

    fn at(day: Day) -> String {
        let t = ::time::OffsetDateTime::from_unix_timestamp(day * 86_400 + 3_600).unwrap();
        format!(
            "{:04}-{:02}-{:02}T01:00:00Z",
            t.year(),
            t.month() as u8,
            t.day()
        )
    }

    /// A Bead opened on `created`, last touched on `updated`, and closed
    /// on `closed` when given.
    fn bead(id: &str, status: &str, created: Day, updated: Day, closed: Option<Day>) -> Item {
        Item {
            reference: format!("bead:n:/x#{id}"),
            id: id.into(),
            status: status.into(),
            created: at(created),
            updated: at(updated),
            closed: closed.map(at),
            ..Item::default()
        }
    }

    fn board(items: Vec<Item>) -> (Board, Vec<usize>) {
        let b = Board::from_snapshot(&Snapshot {
            refreshed: 0,
            projects: vec![ProjectView::Beads(StoreView {
                folder: "/x".into(),
                items: Some(items),
                ..StoreView::default()
            })],
        });
        let all = (0..b.rows.len()).collect();
        (b, all)
    }

    #[test]
    fn a_day_is_local() {
        let helsinki = Clock {
            offset_ms: 2 * 3_600_000,
        };
        // 23:00 UTC on day 20402 is already the next day two hours east.
        let late = (TODAY * 86_400 + 23 * 3_600) as u64 * 1000;
        assert_eq!(UTC.day(late), TODAY);
        assert_eq!(helsinki.day(late), TODAY + 1);
        assert_eq!(day_label(TODAY), "Nov 10");
        assert_eq!(
            range_label(Range::between(TODAY, TODAY - 6)),
            "Nov 4 – Nov 10"
        );
    }

    #[test]
    fn the_open_line_starts_at_its_true_height() {
        let (b, all) = board(vec![
            // Opened long before the strip and still open: counts all along.
            bead("x-1", "open", TODAY - 100, TODAY - 100, None),
            // Opened and finished before the strip: never counts.
            bead("x-2", "closed", TODAY - 90, TODAY - 80, Some(TODAY - 80)),
            // Opened before, finished inside.
            bead("x-3", "closed", TODAY - 50, TODAY - 2, Some(TODAY - 2)),
            // Opened inside, still open.
            bead("x-4", "in_progress", TODAY - 3, TODAY, None),
            // Closed without a closed time: its last update stands for it.
            bead("x-5", "closed", TODAY - 1, TODAY, None),
        ]);
        let t = Timeline::of(&b, &all, UTC, TODAY, 5);
        assert_eq!(t.first, TODAY - 4);
        assert_eq!(t.opened, vec![0, 1, 0, 1, 0]);
        assert_eq!(t.closed, vec![0, 0, 1, 0, 1]);
        assert_eq!(t.open, vec![2, 3, 2, 3, 2]);
        assert_eq!(t.totals(Range::between(TODAY - 3, TODAY)), (2, 2));
        // A range reaching before the strip counts only the days it holds.
        assert_eq!(t.totals(Range::between(TODAY - 40, TODAY - 4)), (0, 0));
    }

    #[test]
    fn a_range_says_what_opened_closed_and_stayed_stuck() {
        let week = Range::between(TODAY - 6, TODAY);
        let (b, all) = board(vec![
            bead("new", "open", TODAY - 1, TODAY - 1, None),
            bead("done", "closed", TODAY - 30, TODAY - 2, Some(TODAY - 2)),
            bead("quick", "closed", TODAY - 3, TODAY - 3, Some(TODAY - 3)),
            bead("idle", "open", TODAY - 40, TODAY - 20, None),
            bead("claimed", "in_progress", TODAY - 40, TODAY - 10, None),
            bead("busy", "open", TODAY - 40, TODAY - 1, None),
            bead("parked", "deferred", TODAY - 40, TODAY - 30, None),
            bead("old", "closed", TODAY - 40, TODAY - 30, Some(TODAY - 30)),
        ]);
        let ids =
            |rows: &[usize]| -> Vec<&str> { rows.iter().map(|&i| b.rows[i].id.as_str()).collect() };
        let m = movement(&b, &all, UTC, week);
        assert_eq!(ids(&m.opened), ["new", "quick"]);
        assert_eq!(ids(&m.closed), ["done", "quick"]);
        // Claimed and left first, then the longest untouched; touched,
        // deferred and finished work is not stuck.
        assert_eq!(ids(&m.stuck), ["claimed", "idle"]);
        let busy = &b.rows[all[5]];
        assert!(touched(busy, UTC, week));
        assert!(!touched(&b.rows[all[3]], UTC, week));
    }
}
