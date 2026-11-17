//! Work page's visual measures, and the colours it gives meanings.

use gpui::Hsla;
use parvi_app_kit::theme::{Theme, alpha};

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
/// The most a row's away project name takes.
pub const AWAY_W: f32 = 120.;

/// The facets column beside the list.
pub const FACETS_W: f32 = 220.;

/// The time strip over the list: its height, and the room its axis
/// labels take on the left (counts), right (open count) and below (days).
pub const STRIP_H: f32 = 132.;
pub const AXIS_LEFT: f32 = 40.;
pub const AXIS_RIGHT: f32 = 64.;
pub const AXIS_BOTTOM: f32 = 22.;
pub const AXIS_TOP: f32 = 14.;
/// The time page's chart: the open level over the daily flow, each band
/// this share of the chart, with this gap between them.
pub const CHART_H: f32 = 260.;
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
/// The selected range's wash, and the hover day's.
pub const BRUSH_WASH: f32 = 0.16;
pub const HOVER_WASH: f32 = 0.06;
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

/// Work being opened: not a meaning of its own, so the text's own grey.
pub fn opened(t: &Theme) -> Hsla {
    alpha(t.text2, 0.75)
}
