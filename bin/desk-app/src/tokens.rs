//! The Desk's own measures and tones. Colours come only from the kit's
//! theme (`parvi_app_kit::theme`), with the meanings DESIGN.md fixes: the
//! accent for action and focus, green for done, amber for "needs you", red
//! for failed. Sizes the kit has are used from it; those only this app
//! needs are named here, once.

use gpui::Hsla;
use parvi_app_kit::theme::{Theme, rgb, size};

pub use parvi_app_kit::theme::size::{PAD, RADIUS, SMALL, TEXT};

/// Type: the ask's title, the words under it, and a key cap's letter.
pub const TITLE: f32 = 22.;
pub const QUESTION_TITLE: f32 = 18.;
pub const BODY: f32 = 14.;
pub const BODY_LINE: f32 = 1.45;
/// A number worth reading at a glance (the waiting count).
pub const COUNT: f32 = 20.;

/// The header bar, as high as Parvi's tab bar.
pub const BAR: f32 = size::TAB_BAR;
pub const GAP: f32 = 12.;
pub const GAP_SMALL: f32 = 6.;
/// The answer column beside the stage, and the queue rail.
pub const COLUMN: f32 = 440.;
pub const RAIL: f32 = 340.;
/// How wide words stand when there is nothing to look at.
pub const READING: f32 = 1040.;
/// A queue row's picture.
pub const ROW_THUMB_W: f32 = 72.;
pub const ROW_THUMB_H: f32 = 48.;
/// The film strip's pictures under the stage.
pub const STRIP_H: f32 = 64.;
pub const STRIP_THUMB_W: f32 = 96.;
/// An option's picture in a list of options.
pub const OPTION_THUMB_W: f32 = 120.;
pub const OPTION_THUMB_H: f32 = 68.;
/// The video transport under the stage, and its track.
pub const TRANSPORT: f32 = 40.;
pub const TRACK_H: f32 = 6.;
/// A key cap's height.
pub const CAP: f32 = 20.;
/// The accent rule beside a recommendation.
pub const RULE_W: f32 = 3.;
/// The answer strip under the stage in Focus.
pub const STRIP_PANEL_MAX_H: f32 = 340.;
/// How far a picture fits up: never past twice its size, so a small one
/// is big enough to judge and still reads as itself.
pub const FIT_MAX: f32 = 2.;
/// Zoom limits and a wheel notch's step.
pub const ZOOM_MIN: f32 = 0.05;
pub const ZOOM_MAX: f32 = 32.;
pub const ZOOM_STEP: f32 = 1.25;
/// Room around the stage's pictures, and between compared ones.
pub const STAGE_PAD: f32 = 16.;
/// How long a notice stays.
pub const TOAST_MS: u64 = 3200;

/// Decoded video frames kept for stepping back and scrubbing, in bytes.
pub const VIDEO_CACHE: usize = 640 << 20;
/// Frames decoded ahead of the one shown while playing, and kept behind a
/// wanted one while seeking.
pub const VIDEO_AHEAD: usize = 8;
pub const VIDEO_BEHIND: usize = 90;
/// Pictures kept decoded, in bytes.
pub const PICTURE_CACHE: usize = 900 << 20;

pub fn done(t: &Theme) -> Hsla {
    rgb(t.ansi[2])
}

pub fn needs_you(t: &Theme) -> Hsla {
    rgb(t.ansi[3])
}
