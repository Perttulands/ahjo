//! The page's measures. Colours, sizes and fades come from the app kit's
//! theme; these are the few this page adds.

/// A button that waits for the node's answer.
pub const BUSY: f32 = 0.5;
/// The field widths in the form.
pub const SHORT_FIELD: f32 = 90.;
pub const CRON_FIELD: f32 = 260.;
pub const DAY_W: f32 = 42.;
/// Between the choices in a row of them: sessions, modes, days, zones.
pub const TOGGLE_GAP: f32 = 6.;
/// Recent runs' columns: when it started and what started it.
pub const RUN_WHEN_W: f32 = 130.;
pub const RUN_BY_W: f32 = 90.;
/// A task's row: its name and when it runs next, then its cadence and how
/// its last run went; the second line starts under the name.
pub const TASK_ROW: f32 = 44.;
pub const SECOND_LINE_INDENT: f32 = 14.;
/// The week (`week.rs`): a lane's height, its name's column, a run's mark
/// and a steady band's height, now's line, and the axis' words: how far
/// down and in from their line, and how much of the week now's own name
/// takes.
pub const WEEK_LANE: f32 = 24.;
pub const WEEK_LABEL_W: f32 = 200.;
pub const WEEK_MARK_W: f32 = 3.;
pub const WEEK_MARK_H: f32 = 12.;
pub const WEEK_MARK_RADIUS: f32 = 1.;
pub const WEEK_STEADY: f32 = 4.;
pub const WEEK_NOW_W: f32 = 2.;
pub const WEEK_AXIS_TOP: f32 = 4.;
pub const WEEK_AXIS_PAD: f32 = 4.;
pub const WEEK_NOW_ROOM: f32 = 0.05;
