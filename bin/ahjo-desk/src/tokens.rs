//! The Desk's own measures. Colours come only from the kit's theme
//! (`ahjo_app_kit::theme`), with the meanings it fixes: `accent` for what
//! acts and has focus, `needs` for what waits on the operator, `done`,
//! `error`. The one colour that is not the theme's is a mark's ink, which is
//! data: the store keeps it with the mark, and the web desk draws it too.
//! Sizes the kit has are used from it; those only this app needs are named
//! here, once.

use ahjo_app_kit::theme::{size, ty};

pub use ahjo_app_kit::theme::size::{DOT, PAD, RADIUS, TEXT};

/// The words around the ask (meta lines, the tools, hints, chips and
/// buttons). The Desk is used full screen from a sitting distance, so they
/// stand a step above the chrome's [`TEXT`], which is for a dense workspace:
/// at the theme's title size.
pub const META: f32 = ty::TITLE;
/// The least the Desk writes: a key on its cap, a mark's number, an
/// item's letter. The chrome's text size, never under it.
pub const CAP_TEXT: f32 = size::TEXT;

/// Type: the ask's title, and the Desk's own name in its bar. Words to
/// read are the theme's reading size (`Theme::ty.read`).
pub const TITLE: f32 = 18.;
pub const BODY: f32 = 14.;
/// A number worth reading at a glance (the waiting count).
pub const COUNT: f32 = 20.;

/// The header bar, as high as Ahjo Swarm's tab bar, and the surface's tool bar.
pub const BAR: f32 = size::TAB_BAR;
pub const TOOLS: f32 = 40.;
pub const GAP: f32 = 12.;
pub const GAP_SMALL: f32 = 6.;
/// The conversation beside the surface, and the queue rail.
pub const COLUMN: f32 = 460.;
pub const RAIL: f32 = 360.;
/// In a narrow window both give way so the surface keeps the most room:
/// never narrower than these, and the window never narrower than
/// [`WINDOW_MIN`].
pub const COLUMN_MIN: f32 = 320.;
pub const RAIL_MIN: f32 = 200.;
pub const RAIL_SHARE: f32 = 0.21;
pub const COLUMN_SHARE: f32 = 0.32;
pub const WINDOW_MIN: (f32, f32) = (880., 560.);
/// Under this width a queue row drops its picture: its words need the room.
pub const RAIL_THUMBS: f32 = 260.;
/// How wide the conversation stands when there is nothing to look at, and
/// how wide the document reads.
pub const READING: f32 = 820.;
/// His turn in the conversation stands in from the left by this much.
pub const REPLY_INDENT: f32 = 40.;
/// The most lines his words grow to before they scroll inside their box,
/// the caret always in sight.
pub const WORDS_LINES: usize = 12;
/// A pasted picture beside his words.
pub const PASTED_H: f32 = 56.;
/// Settings: the page's width, the space between its sections, the node
/// field, and a theme's card.
pub const SETTINGS_W: f32 = 860.;
pub const SECTION_GAP: f32 = 28.;
pub const SETTINGS_FIELD: f32 = 320.;
pub const THEME_CARD: f32 = 160.;
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
/// A key cap's height; a chip stands a little taller, so a pointer finds it.
pub const CAP: f32 = 22.;
/// The most of an option's name a "Lean to" chip shows: the chips wrap,
/// so a name reads whole unless it is a sentence.
pub const LEAN_WORDS: usize = 40;
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
/// How soon after a change his unsent answers are written to disk.
pub const KEEP_DRAFTS_MS: u64 = 400;

/// A mark's ink when it names none: the store's and the web desk's.
pub const MARK_INK: u32 = 0xd7b879;
/// A mark's stroke, and the selected one's; the dark edge under each that
/// keeps it legible on any picture.
pub const MARK_LINE: f32 = 2.5;
pub const MARK_LINE_ON: f32 = 4.;
pub const MARK_EDGE: f32 = 2.;
/// A pin's radius, and a mark's number badge.
pub const PIN: f32 = 7.;
pub const BADGE: f32 = 22.;
pub const BADGE_TEXT: f32 = CAP_TEXT;

/// Decoded video frames kept for stepping back and scrubbing, in bytes.
pub const VIDEO_CACHE: usize = 640 << 20;
/// Frames decoded ahead of the one shown while playing, and kept behind a
/// wanted one while seeking.
pub const VIDEO_AHEAD: usize = 8;
pub const VIDEO_BEHIND: usize = 90;
/// Pictures kept decoded, in bytes.
pub const PICTURE_CACHE: usize = 900 << 20;

/// The queue rail's and the conversation's widths in a window `wide`.
pub fn columns(wide: f32) -> (f32, f32) {
    (
        (wide * RAIL_SHARE).clamp(RAIL_MIN, RAIL),
        (wide * COLUMN_SHARE).clamp(COLUMN_MIN, COLUMN),
    )
}

/// A mark's ink as the store keeps it (`#rrggbb`), else [`MARK_INK`].
pub fn ink(color: Option<&str>) -> u32 {
    color
        .and_then(|c| c.strip_prefix('#'))
        .filter(|c| c.len() == 6)
        .and_then(|c| u32::from_str_radix(c, 16).ok())
        .unwrap_or(MARK_INK)
}

/// A picture `w` by `h` drawn whole in a queue row's thumbnail box.
pub fn thumb_fit(w: f32, h: f32) -> (f32, f32) {
    let k = (ROW_THUMB_W / w.max(1.)).min(ROW_THUMB_H / h.max(1.));
    (w * k, h * k)
}

/// Pictures compared at once read while the shorter side of each, as
/// drawn, is at least this.
pub const SIDE_LEAST: f32 = 240.;

/// The share of its size a layout gives up for each cell it leaves empty:
/// a full grid reads as one comparison, a hole in it draws the eye.
pub const GRID_EMPTY: f32 = 0.1;

/// The columns that draw `n` pictures of `shape` (width over height)
/// largest in `room` with `gap` between, every cell the same size, and the
/// shorter side each is then drawn at.
pub fn grid(n: usize, room: (f32, f32), shape: f32, gap: f32) -> (usize, f32) {
    let n = n.max(1);
    let mut best = (1, f32::MIN, f32::MIN);
    for cols in 1..=n {
        let rows = n.div_ceil(cols);
        let w = (room.0 - gap * (cols - 1) as f32) / cols as f32;
        let h = (room.1 - gap * (rows - 1) as f32) / rows as f32;
        // Drawn whole in its cell: as high as the cell, or as wide.
        let (dw, dh) = if w / h > shape {
            (h * shape, h)
        } else {
            (w, w / shape)
        };
        let side = dw.min(dh);
        let score = side * (1. - GRID_EMPTY * (rows * cols - n) as f32);
        if score > best.2 {
            best = (cols, side, score);
        }
    }
    (best.0, best.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_surface_keeps_room_in_a_narrow_window() {
        // Full screen: both stand at their full measure.
        assert_eq!(columns(1920.), (RAIL, COLUMN));
        // At the narrowest window the surface still has a third of it.
        let (rail, column) = columns(WINDOW_MIN.0);
        assert!(rail >= RAIL_MIN && column >= COLUMN_MIN);
        assert!(WINDOW_MIN.0 - rail - column >= WINDOW_MIN.0 / 3.);
    }

    #[test]
    fn a_thumbnail_shows_the_whole_picture() {
        // A portrait stands whole in the box, its height the box's.
        let (w, h) = thumb_fit(1080., 1920.);
        assert_eq!(h, ROW_THUMB_H);
        assert!(w < ROW_THUMB_W);
        // A wide one spans the box.
        let (w, h) = thumb_fit(3840., 2160.);
        assert_eq!(w, ROW_THUMB_W);
        assert!(h <= ROW_THUMB_H);
    }

    #[test]
    fn compared_pictures_use_the_height() {
        // Full screen, the stage between the list and the conversation.
        let (rail, column) = columns(1920.);
        let room = (1920. - rail - column, 1080. - BAR - TOOLS - STRIP_H);
        // Three wide pictures stack, each still readable, rather than leave
        // a hole in a square.
        let (cols, least) = grid(3, room, 16. / 9., 2.);
        assert_eq!(cols, 1);
        assert!(least >= SIDE_LEAST, "{least}");
        // Three tall ones stand in a row.
        assert_eq!(grid(3, room, 9. / 16., 2.).0, 3);
        // Twelve no longer read: the strip takes them.
        assert!(grid(12, room, 16. / 9., 2.).1 < SIDE_LEAST);
    }
}

/// PROTOTYPE: the answer's column in the middle, wide when the queue is
/// folded, and the folded queue's strip.
pub const PROTO_TALK: f32 = 720.;
pub const PROTO_TALK_WIDE: f32 = 860.;
pub const PROTO_STRIP: f32 = 72.;
