//! Issue over time (DESIGN.md, Issue: how it moves): the items opened and
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

/// Turns a time into its local day in this machine's zone, with the
/// zone's offset at that time: a day before a summer-time change and a day
/// after it each end at their own midnight.
#[derive(Clone, Debug)]
pub struct Clock {
    zone: jiff::tz::TimeZone,
}

impl Clock {
    /// This machine's zone; UTC when it cannot be told.
    pub fn local() -> Clock {
        Clock {
            zone: jiff::tz::TimeZone::try_system().unwrap_or(jiff::tz::TimeZone::UTC),
        }
    }
    pub fn utc() -> Clock {
        Clock {
            zone: jiff::tz::TimeZone::UTC,
        }
    }
    #[cfg(test)]
    pub fn zone(zone: jiff::tz::TimeZone) -> Clock {
        Clock { zone }
    }
    pub fn day(&self, ms: u64) -> Day {
        let at =
            jiff::Timestamp::from_millisecond(ms as i64).unwrap_or(jiff::Timestamp::UNIX_EPOCH);
        let offset = self.zone.to_offset(at).seconds() as i64 * 1000;
        (ms as i64 + offset).div_euclid(DAY_MS)
    }
}

/// A row's days: when it was opened, last touched and finished.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Dated {
    pub created: Option<Day>,
    pub updated: Option<Day>,
    pub finished: Option<Day>,
}

/// Every row's days, in the board's order: counted once per board, so a
/// drag across the chart only compares numbers.
pub fn dated(board: &Board, clock: &Clock) -> Vec<Dated> {
    board
        .rows
        .iter()
        .map(|r| {
            let created = r.created.map(|c| clock.day(c));
            Dated {
                created,
                updated: r.updated.map(|u| clock.day(u)),
                // Never finished before it was opened.
                finished: finished(r).map(|f| {
                    let d = clock.day(f);
                    created.map_or(d, |c| d.max(c))
                }),
            }
        })
        .collect()
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
    pub fn of(dated: &[Dated], rows: &[usize], today: Day, days: usize) -> Timeline {
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
            let r = &dated[i];
            let Some(created) = r.created else {
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
            if let Some(done) = r.finished {
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
/// sat open and idle.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Movement {
    /// Opened in the range, newest first.
    pub opened: Vec<usize>,
    /// Finished in the range, newest first.
    pub closed: Vec<usize>,
    /// Open at the range's end and not touched for the idle days before
    /// it, as far as the last touch tells: under way first (claimed and
    /// left), then the longest idle. Deferred work is meant to wait.
    pub idle: Vec<usize>,
}

/// What moved among `rows` in `range`, and what sat `idle` days or more.
pub fn movement(
    board: &Board,
    dated: &[Dated],
    rows: &[usize],
    range: Range,
    idle: i64,
) -> Movement {
    let mut m = Movement::default();
    for &i in rows {
        let d = &dated[i];
        if d.created.is_some_and(|c| range.holds(c)) {
            m.opened.push(i);
        }
        if d.finished.is_some_and(|f| range.holds(f)) {
            m.closed.push(i);
        }
        let open_at_end =
            d.created.is_some_and(|c| c <= range.to) && d.finished.is_none_or(|f| f > range.to);
        let left = d.updated.is_some_and(|u| u <= range.to - idle);
        if open_at_end && left && board.rows[i].status != Status::Deferred {
            m.idle.push(i);
        }
    }
    let rows = &board.rows;
    m.opened
        .sort_by(|&a, &b| rows[b].created.cmp(&rows[a].created));
    m.closed
        .sort_by(|&a, &b| finished(&rows[b]).cmp(&finished(&rows[a])));
    m.idle.sort_by(|&a, &b| {
        (rows[b].status == Status::InProgress)
            .cmp(&(rows[a].status == Status::InProgress))
            .then(rows[a].updated.cmp(&rows[b].updated))
    });
    m
}

/// Whether a row was opened, touched or finished in `range`.
pub fn touched(d: &Dated, range: Range) -> bool {
    [d.created, d.updated, d.finished]
        .into_iter()
        .flatten()
        .any(|day| range.holds(day))
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
    use ahjo_issue_contract::{Item, ProjectView, Snapshot, StoreView};

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

    fn board(items: Vec<Item>) -> (Board, Vec<Dated>, Vec<usize>) {
        let b = Board::from_snapshot(&Snapshot {
            refreshed: 0,
            projects: vec![ProjectView::Beads(StoreView {
                folder: "/x".into(),
                items: Some(items),
                ..StoreView::default()
            })],
        });
        let all = (0..b.rows.len()).collect();
        let d = dated(&b, &Clock::utc());
        (b, d, all)
    }

    #[test]
    fn a_day_is_local_and_keeps_its_midnight_across_summer_time() {
        // Helsinki: two hours east in winter, three in summer; summer time
        // ended at 04:00 local on 2026-10-25 (day 20_386).
        let helsinki =
            Clock::zone(jiff::tz::TimeZone::posix("EET-2EEST,M3.5.0/3,M10.5.0/4").unwrap());
        let ms = |day: Day, utc_hour: i64| ((day * 86_400 + utc_hour * 3_600) * 1000) as u64;
        // 23:00 UTC on day 20402 is already the next day two hours east.
        assert_eq!(Clock::utc().day(ms(TODAY, 23)), TODAY);
        assert_eq!(helsinki.day(ms(TODAY, 23)), TODAY + 1);
        // In summer, 21:30 UTC is 00:30 the next day; in winter, 23:30.
        let summer = 20_380;
        assert_eq!(helsinki.day(ms(summer, 21) + 1_800_000), summer + 1);
        assert_eq!(helsinki.day(ms(TODAY, 21) + 1_800_000), TODAY);
        assert_eq!(day_label(TODAY), "Nov 10");
        assert_eq!(
            range_label(Range::between(TODAY, TODAY - 6)),
            "Nov 4 – Nov 10"
        );
    }

    #[test]
    fn the_open_line_starts_at_its_true_height() {
        let (_, d, all) = board(vec![
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
        let t = Timeline::of(&d, &all, TODAY, 5);
        assert_eq!(t.first, TODAY - 4);
        assert_eq!(t.opened, vec![0, 1, 0, 1, 0]);
        assert_eq!(t.closed, vec![0, 0, 1, 0, 1]);
        assert_eq!(t.open, vec![2, 3, 2, 3, 2]);
        assert_eq!(t.totals(Range::between(TODAY - 3, TODAY)), (2, 2));
        // A range reaching before the strip counts only the days it holds.
        assert_eq!(t.totals(Range::between(TODAY - 40, TODAY - 4)), (0, 0));
    }

    #[test]
    fn a_range_says_what_opened_closed_and_sat_idle() {
        let week = Range::between(TODAY - 6, TODAY);
        let (b, d, all) = board(vec![
            bead("new", "open", TODAY - 1, TODAY - 1, None),
            bead("done", "closed", TODAY - 30, TODAY - 2, Some(TODAY - 2)),
            bead("quick", "closed", TODAY - 3, TODAY - 3, Some(TODAY - 3)),
            bead("idle", "open", TODAY - 40, TODAY - 20, None),
            bead("claimed", "in_progress", TODAY - 40, TODAY - 15, None),
            bead("recent", "open", TODAY - 40, TODAY - 10, None),
            bead("busy", "open", TODAY - 40, TODAY - 1, None),
            bead("parked", "deferred", TODAY - 40, TODAY - 30, None),
            bead("old", "closed", TODAY - 40, TODAY - 30, Some(TODAY - 30)),
        ]);
        let ids =
            |rows: &[usize]| -> Vec<&str> { rows.iter().map(|&i| b.rows[i].id.as_str()).collect() };
        let m = movement(&b, &d, &all, week, 14);
        assert_eq!(ids(&m.opened), ["new", "quick"]);
        assert_eq!(ids(&m.closed), ["done", "quick"]);
        // Idle 14 days by the range's end: claimed and left first, then
        // the longest idle; recently touched, deferred and finished work
        // is not idle.
        assert_eq!(ids(&m.idle), ["claimed", "idle"]);
        // Fewer days take in what was touched ten days ago.
        let m = movement(&b, &d, &all, week, 7);
        assert_eq!(ids(&m.idle), ["claimed", "idle", "recent"]);
        assert!(touched(&d[6], week));
        assert!(!touched(&d[3], week));
    }
}
