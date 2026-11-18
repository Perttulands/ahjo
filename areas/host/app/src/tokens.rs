//! The Host page's own measures.

/// A figure's band on the machine's time axis, and the room between bands.
pub const BAND_H: f32 = 92.;
pub const BAND_GAP: f32 = 12.;
/// A band's legend beside it: its name, its number now, its whole and its
/// peak and average.
pub const LEGEND_W: f32 = 210.;
pub const LEGEND_GAP: f32 = 2.;
/// The least the chart takes beside the legends: in less room the legends
/// stand above it, side by side, each at least this wide.
pub const CHART_MIN: f32 = 440.;
pub const LEGEND_MIN: f32 = 150.;
/// The time axis under the bands and a tick's label.
pub const AXIS_H: f32 = 18.;
pub const TICK_W: f32 = 44.;
/// A disk's bar.
pub const DISK_BAR_H: f32 = 6.;
pub const DISK_W: f32 = 240.;
/// Table columns: a figure, a time.
pub const FIGURE_COL: f32 = 72.;
pub const TIME_COL: f32 = 64.;
/// A time with its day before it (`13 Nov 23:48`).
pub const DAY_TIME_COL: f32 = 104.;
/// A service's state mark.
pub const GLYPH: f32 = 8.;
/// A service row's two lines, and where the second starts: under the
/// name, past the state mark.
pub const SERVICE_ROW: f32 = 44.;
/// The machine row's line of figures, one line high.
pub const FIGURES_H: f32 = 16.;
pub const SECOND_LINE_INDENT: f32 = GLYPH + 8.;
/// The most of an address a service row shows before it shortens it.
pub const ADDRESS_CHARS: usize = 30;
pub const HAIRLINE: f32 = 1.;
pub const STROKE: f32 = 1.5;
pub const GRID_TICK: f32 = 0.4;
pub const AREA_ALPHA: f32 = 0.18;
pub const LINE_ALPHA: f32 = 0.6;
/// A service that is starting: its mark half there.
pub const FADED: f32 = 0.5;
/// The log lines read for a service: enough to reach back before its run.
pub const LOG_LINES: usize = 60;
