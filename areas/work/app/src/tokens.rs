//! Work page's visual measures, and the colours it gives meanings.

use ahjo_app_kit::theme::{Theme, alpha};
use gpui::Hsla;

pub const SPACE_XS: f32 = 4.;
pub const SPACE_XL: f32 = 12.;
pub const SPACE_4XL: f32 = 20.;
/// The report layer's width.
pub const CARD_W: f32 = 520.;

/// How far each level of the tree stands in from its parent.
pub const INDENT: f32 = 16.;
/// The deepest the tree indents; deeper items stand at this depth.
pub const MAX_DEPTH: usize = 5;
/// The most the project's name takes in the list's head.
pub const PROJECT_ACT_W: f32 = 160.;

/// A row's state glyph and its age, in fixed columns so titles line up.
pub const GLYPH_W: f32 = 14.;
pub const AGE_W: f32 = 34.;
/// A row's kind of work and its priority, in columns of their own.
pub const KIND_W: f32 = 56.;
pub const PRIORITY_W: f32 = 44.;
/// The line down a group's indent, under its mark.
pub const GUIDE_W: f32 = 1.;
/// A group's progress bar: its length and thickness.
pub const METER_W: f32 = 36.;
pub const METER_H: f32 = 4.;
/// The most a row's away project name takes.
pub const AWAY_W: f32 = 120.;

/// The facets column beside the list.
pub const FACETS_W: f32 = 184.;
/// The list's width until its edge is dragged, and the least and most a
/// drag gives it; the grip at its edge.
pub const LIST_W: f32 = 520.;
pub const LIST_MIN: f32 = 320.;
pub const LIST_MAX: f32 = 960.;
pub const GRIP: f32 = 6.;
/// The most of its row the list takes, and the grip's ink when the mouse
/// is on it.
pub const LIST_SHARE: f32 = 0.6;
/// The list's share of the room beside the facets until its edge is
/// dragged, at least [`LIST_W`] and at most this.
pub const LIST_SHARE_OPEN: f32 = 0.4;
pub const LIST_OPEN_MAX: f32 = 760.;
pub const GRIP_INK: f32 = 0.5;

/// The time strip over the list: its height, and the room its axis
/// labels take on the left (counts), right (open count) and below (days);
/// above, the hovered day's plate has a lane of its own.
pub const STRIP_H: f32 = 150.;
pub const AXIS_LEFT: f32 = 40.;
pub const AXIS_RIGHT: f32 = 64.;
pub const AXIS_BOTTOM: f32 = 22.;
pub const AXIS_TOP: f32 = 30.;
/// The time page's chart: the open level over the daily flow, each band
/// this share of the chart, with this gap between them.
pub const CHART_H: f32 = 276.;
pub const LEVEL_SHARE: f32 = 0.42;
pub const BAND_GAP: f32 = 14.;
/// The widest a day's bar grows, the narrowest gap between two days, and
/// the gap between a day's opened and closed bars.
pub const BAR_MAX: f32 = 10.;
pub const BAR_GAP: f32 = 1.;
/// The fewest pixels between two day labels on the axis.
pub const LABEL_EVERY: f32 = 64.;
/// The open line, and the wash under it.
pub const STROKE: f32 = 1.5;
pub const LEVEL_WASH: f32 = 0.10;
/// The selected range's wash, its edges' width and ink, and how far the
/// days outside it step back.
pub const BRUSH_WASH: f32 = 0.12;
pub const EDGE: f32 = 1.5;
pub const EDGE_INK: f32 = 0.85;
pub const OUTSIDE: f32 = 0.35;
/// The hovered day: its guide's ink, the mark on the open line, and the
/// plate of its numbers (padding, and gap from the guide).
pub const GUIDE: f32 = 0.55;
pub const DOT: f32 = 7.;
pub const PLATE_PAD: f32 = 6.;
pub const PLATE_GAP: f32 = 8.;
pub const PLATE_H: f32 = 22.;
/// How far the range's wash and the hover guide reach above the plot.
pub const BRUSH_RISE: f32 = 6.;
/// A bar's rounded end, and the strip's open line against the chart's.
pub const BAR_RADIUS: f32 = 1.5;
pub const STRIP_LINE: f32 = 0.55;
pub const HAIRLINE: f32 = 1.;
/// A colour's mark in the chart's key.
pub const SWATCH: f32 = 8.;
pub const SWATCH_RADIUS: f32 = 2.;
/// How faint a gridline is against the divider.
pub const GRID: f32 = 0.45;

/// Days the strip spans, and the time page's chart.
pub const STRIP_DAYS: usize = 60;
pub const CHART_DAYS: usize = 90;
/// The range the time page opens on: the last week.
pub const OPENING_RANGE: i64 = 7;
/// Days untouched that make open work idle: the choices, and the first.
pub const IDLE_DAYS: [i64; 4] = [7, 14, 30, 60];
pub const IDLE_DEFAULT: i64 = 14;

/// A number worth reading at a glance (the time page's totals).
pub const NUMBER: f32 = 24.;
/// The totals' tabs: each one's least width and its padding.
pub const TOTAL_W: f32 = 132.;
pub const TOTAL_PAD: f32 = 12.;

/// A family card in the item: its least and greatest width.
pub const CARD_MIN: f32 = 240.;
pub const CARD_MAX: f32 = 320.;
pub const CARD_PAD: f32 = 10.;

// Finished and under way are the theme's own `done` and `running`.

/// Work being opened: data with no state of its own, so the theme's chart
/// ink, a step back from the open line drawn in it.
pub fn opened(t: &Theme) -> Hsla {
    alpha(t.chart, OPENED_INK)
}
pub const OPENED_INK: f32 = 0.6;
