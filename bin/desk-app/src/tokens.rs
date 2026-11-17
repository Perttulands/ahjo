//! The Desk's own measures. Colours come only from the kit's theme
//! (`parvi_app_kit::theme`), with the meanings it fixes: `accent` for what
//! acts and has focus, `needs` for what waits on the operator, `done`,
//! `error`. The one colour that is not the theme's is a mark's ink, which is
//! data: the store keeps it with the mark, and the web desk draws it too.
//! Sizes the kit has are used from it; those only this app needs are named
//! here, once.

use parvi_app_kit::theme::size;

pub use parvi_app_kit::theme::size::{PAD, RADIUS, SMALL, TEXT};
pub use parvi_app_kit::theme::ty::{READ, READ_LEADING};

/// Type: the ask's title, the words under it.
pub const TITLE: f32 = 18.;
pub const BODY: f32 = 14.;
pub const BODY_LINE: f32 = 1.45;
/// A number worth reading at a glance (the waiting count).
pub const COUNT: f32 = 20.;

/// The header bar, as high as Parvi's tab bar, and the surface's tool bar.
pub const BAR: f32 = size::TAB_BAR;
pub const TOOLS: f32 = 40.;
pub const GAP: f32 = 12.;
pub const GAP_SMALL: f32 = 6.;
/// The conversation beside the surface, and the queue rail.
pub const COLUMN: f32 = 460.;
pub const RAIL: f32 = 300.;
/// How wide the conversation stands when there is nothing to look at, and
/// how wide the document reads.
pub const READING: f32 = 820.;
/// His turn in the conversation stands in from the left by this much.
pub const REPLY_INDENT: f32 = 40.;
/// The most room his words take before they scroll.
pub const WORDS_MAX_H: f32 = 320.;
/// The list of his marks before it scrolls.
pub const MARKS_MAX_H: f32 = 220.;
/// A queue row's picture.
pub const ROW_THUMB_W: f32 = 64.;
pub const ROW_THUMB_H: f32 = 44.;
/// The film strip's pictures under the surface.
pub const STRIP_H: f32 = 64.;
pub const STRIP_THUMB_W: f32 = 96.;
/// The video transport under the surface, and its track; a mark's tick
/// on the track.
pub const TRANSPORT: f32 = 40.;
pub const TRACK_H: f32 = 6.;
pub const TICK_W: f32 = 3.;
/// A key cap's height.
pub const CAP: f32 = 20.;
/// The accent rule beside a recommendation.
pub const RULE_W: f32 = 3.;
/// How far a picture fits up: never past twice its size, so a small one
/// is big enough to judge and still reads as itself.
pub const FIT_MAX: f32 = 2.;
/// Zoom limits and a wheel notch's step.
pub const ZOOM_MIN: f32 = 0.05;
pub const ZOOM_MAX: f32 = 32.;
pub const ZOOM_STEP: f32 = 1.25;
/// Room around the surface's pictures.
pub const STAGE_PAD: f32 = 16.;
/// How long a notice stays.
pub const TOAST_MS: u64 = 3200;

/// A mark's ink when it names none: the store's and the web desk's.
pub const MARK_INK: u32 = 0xd7b879;
/// A mark's stroke, and the selected one's; the dark edge under each that
/// keeps it legible on any picture.
pub const MARK_LINE: f32 = 2.5;
pub const MARK_LINE_ON: f32 = 4.;
pub const MARK_EDGE: f32 = 2.;
/// A pin's radius, and a mark's number badge.
pub const PIN: f32 = 7.;
pub const BADGE: f32 = 18.;
pub const BADGE_TEXT: f32 = 11.;

/// Decoded video frames kept for stepping back and scrubbing, in bytes.
pub const VIDEO_CACHE: usize = 640 << 20;
/// Frames decoded ahead of the one shown while playing, and kept behind a
/// wanted one while seeking.
pub const VIDEO_AHEAD: usize = 8;
pub const VIDEO_BEHIND: usize = 90;
/// Pictures kept decoded, in bytes.
pub const PICTURE_CACHE: usize = 900 << 20;

/// A mark's ink as the store keeps it (`#rrggbb`), else [`MARK_INK`].
pub fn ink(color: Option<&str>) -> u32 {
    color
        .and_then(|c| c.strip_prefix('#'))
        .filter(|c| c.len() == 6)
        .and_then(|c| u32::from_str_radix(c, 16).ok())
        .unwrap_or(MARK_INK)
}
