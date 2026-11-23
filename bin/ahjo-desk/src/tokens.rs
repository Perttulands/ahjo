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

/// Type: the ask's title. Words to read are the theme's reading size
/// (`Theme::ty.read`).
pub const TITLE: f32 = 18.;
pub const BODY: f32 = 14.;

/// The row atop each column: the list's tabs, the ask's name, the
/// surface's tools. One height, so the three read as one line.
pub const TOOLS: f32 = 44.;
pub const GAP: f32 = 12.;
pub const GAP_SMALL: f32 = 6.;
/// An icon, and the square button around it.
pub const ICON: f32 = 18.;
pub const ICON_BUTTON: f32 = 32.;

/// The three columns: the list of asks, the answer, and the surface,
/// in that order. The list and the answer stand at these until he drags
/// their edges; then at what he dragged them to. At its narrowest the
/// list's head still reads whole: Waiting, Answered, the menu and Close.
pub const LIST: f32 = 340.;
pub const LIST_SHARE: f32 = 0.18;
pub const LIST_MIN: f32 = 296.;
pub const LIST_MAX: f32 = 560.;
pub const TALK: f32 = 680.;
pub const TALK_SHARE: f32 = 0.34;
pub const TALK_MIN: f32 = 400.;
pub const TALK_MAX: f32 = 1100.;
/// The least the surface keeps: the answer gives way first, then the list.
pub const STAGE_MIN: f32 = 360.;
/// A column's edge is caught this wide, either side of its line.
pub const GRIP_HIT: f32 = 10.;
pub const WINDOW_MIN: (f32, f32) = (880., 560.);
/// Under this width a list row drops its picture: its words need the room.
pub const LIST_THUMBS: f32 = 280.;
/// The room around the list's rows, which stand as cards (half of it
/// between them).
pub const LIST_PAD: f32 = 6.;
/// The settings menu.
pub const MENU_W: f32 = 300.;
/// A mark's words written over the surface, with it alone in the window.
pub const NOTE_CARD: f32 = 640.;
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
/// A list row's picture.
pub const ROW_THUMB_W: f32 = 72.;
pub const ROW_THUMB_H: f32 = 54.;
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
/// The answer panel's measures. Its column stands in by `COLUMN_PAD`;
/// the question breathes before its options (`BEFORE_OPTIONS`) and his
/// answer after them (`BEFORE_ANSWER`).
pub const COLUMN_PAD: f32 = 24.;
pub const COLUMN_TOP: f32 = 20.;
pub const BETWEEN_TURNS: f32 = 28.;
pub const META_TO_TITLE: f32 = 6.;
pub const TITLE_LEADING: f32 = 1.3;
pub const TITLE_TO_QUESTION: f32 = 6.;
pub const BEFORE_OPTIONS: f32 = 20.;
pub const BETWEEN_QUESTIONS: f32 = 36.;
pub const BEFORE_ANSWER: f32 = 20.;
/// An option's card: its corner, its padding, the space between cards
/// and between its lines, and the gutter its letter hangs in.
pub const CARD_RADIUS: f32 = 8.;
pub const CARD_PAD_X: f32 = 16.;
pub const CARD_PAD_Y: f32 = 12.;
pub const CARD_GAP: f32 = 8.;
pub const CARD_LINES: f32 = 2.;
pub const HANG: f32 = 24.;
/// The agent's reason stands a little apart from the option's own why.
pub const BEFORE_REASON: f32 = 6.;
/// His answer's card: the room around his words, its foot, and Send.
pub const FIELD_PAD_Y: f32 = 8.;
pub const FOOT_PAD: f32 = 8.;
pub const SEND_H: f32 = 34.;
/// How far a scrolled column fades at an edge with more beyond it.
pub const FADE: f32 = 40.;
/// A mark's row at the head of his answer.
pub const MARK_GAP: f32 = 10.;
pub const MARK_PAD_Y: f32 = 4.;
/// How far a picture fits up: never past twice its size, so a small one
/// is big enough to judge and still reads as itself.
pub const FIT_MAX: f32 = 2.;
/// Zoom limits and a wheel notch's step.
pub const ZOOM_MIN: f32 = 0.05;
pub const ZOOM_MAX: f32 = 32.;
pub const ZOOM_STEP: f32 = 1.25;
/// How much of a picture stays in sight however far it is moved.
pub const KEEP_SEEN: f32 = 96.;

/// A picture `size` long, moved `pan` from the middle of a room `room`
/// long, kept so at least [`KEEP_SEEN`] of it (or all of it, when it is
/// smaller) stays in the room.
pub fn kept(pan: f32, size: f32, room: f32) -> f32 {
    let most = ((size + room) / 2. - KEEP_SEEN.min(size)).max(0.);
    pan.clamp(-most, most)
}

/// Room around the surface's pictures.
pub const STAGE_PAD: f32 = 16.;
/// How long a notice stays.
pub const TOAST_MS: u64 = 3200;
/// How long an answer that says nothing of its own (answered elsewhere)
/// waits for an undo before it is posted.
pub const UNDO_MS: u64 = 6000;
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

/// The least a clip's track narrows to, so it can still be sought.
pub const TRACK_MIN: f32 = 96.;

/// The share of the window's height his words may take before they
/// scroll: a short window keeps the question in sight above them.
pub const WORDS_SHARE: f32 = 0.3;

/// How many lines his words show before they scroll, in a window `tall`
/// high with reading text of `read`.
pub fn words_lines(tall: f32, read: f32) -> usize {
    ((tall * WORDS_SHARE / (read * 1.5)).floor() as usize).clamp(3, WORDS_LINES)
}

/// The list's and the answer's widths in a window `wide`: as he dragged
/// them (`list`, `talk`), else a share of the window; the list none when
/// it is put away. However they were dragged, the surface keeps
/// [`STAGE_MIN`]: the answer gives way first, then the list.
pub fn columns(wide: f32, open: bool, list: Option<f32>, talk: Option<f32>) -> (f32, f32) {
    let list = if open {
        list.unwrap_or((wide * LIST_SHARE).clamp(LIST_MIN, LIST))
            .clamp(LIST_MIN, LIST_MAX)
    } else {
        0.
    };
    let talk = talk
        .unwrap_or((wide * TALK_SHARE).clamp(TALK_MIN, TALK))
        .clamp(TALK_MIN, TALK_MAX);
    let over = (list + talk + STAGE_MIN - wide).max(0.);
    let talk = (talk - over).max(TALK_MIN);
    let over = (list + talk + STAGE_MIN - wide).max(0.);
    let list = if open {
        (list - over).max(LIST_MIN)
    } else {
        0.
    };
    (list, talk)
}

/// A column's width as its edge is dragged to `want`: within its own
/// bounds, and leaving the surface [`STAGE_MIN`] beside `others`.
pub fn dragged(want: f32, least: f32, most: f32, wide: f32, others: f32) -> f32 {
    want.min(most).min(wide - others - STAGE_MIN).max(least)
}

/// A mark's ink as the store keeps it (`#rrggbb`), else [`MARK_INK`].
/// The paper a document is graded on: light paper on every theme, as on
/// the web desk (`/srv/desk/web/style.css`, "Paper"), so the grader's red
/// reads as a grader's red and nothing else. These colours live only on
/// the sheet; the theme's meanings hold everywhere around it.
pub mod paper {
    /// The sheet, its words, its quieter words and its rules.
    pub const PAPER: u32 = 0xfbf7ec;
    pub const INK: u32 = 0x221d17;
    pub const SOFT: u32 = 0x5d5447;
    pub const RULE: u32 = 0xd8ccb2;
    pub const EDGE: u32 = 0xd6cbb3;
    /// The grader's hand, and its wash behind a chosen passage.
    pub const GRADE: u32 = 0xc22a1d;
    pub const GRADE_WASH: f32 = 0.12;
    /// The last round's marks, fainter.
    pub const GRADE_OLD: f32 = 0.38;
    /// Words new since the round he marked, and words taken out.
    pub const NEW: u32 = 0xe8be40;
    pub const NEW_WASH: f32 = 0.34;
    pub const GONE: u32 = 0x8d8374;
    /// The grader's handwriting (Caveat, at the web desk's weight).
    pub const HAND: &str = "Caveat";
    pub const HAND_FILE: &[u8] = include_bytes!("../assets/fonts/Caveat.ttf");
    /// The sheet's words, a heading's, and the hand's (which runs small).
    pub const TEXT: f32 = 17.;
    pub const LEADING: f32 = 1.62;
    pub const H1: f32 = 28.;
    pub const H2: f32 = 22.;
    pub const H3: f32 = 19.;
    pub const HAND_TEXT: f32 = 22.;
    pub const STAMP: f32 = 40.;
    /// The page's margins, the column the notes stand in, and the room
    /// between blocks.
    pub const PAD_X: f32 = 44.;
    pub const PAD_Y: f32 = 30.;
    pub const MARGIN: f32 = 200.;
    pub const MARGIN_GAP: f32 = 26.;
    pub const BLOCK_GAP: f32 = 12.;
    /// The sheet: text and margin.
    pub const WIDTH: f32 = 960.;
    /// A sheet at least this wide writes notes in its margin.
    pub const WIDE: f32 = 820.;
    /// Room kept under the sheet's end while the pens stand over it.
    pub const TRAY_ROOM: f32 = 120.;
    /// How thick the hand's strokes are: a line, a tick, the old round's.
    pub const STROKE: f32 = 2.1;
    pub const TICK: f32 = 2.8;
    pub const STROKE_OLD: f32 = 1.8;
}

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
    fn the_surface_keeps_room_however_the_columns_were_dragged() {
        // Full HD, never dragged: the list at its measure, the answer a
        // third, the picture the rest and the most.
        let (list, talk) = columns(1920., true, None, None);
        assert_eq!(list, LIST);
        assert!(1920. - list - talk > talk);
        // A laptop: the picture still has more than a third.
        let (list, talk) = columns(1366., true, None, None);
        assert!(1366. - list - talk > 1366. / 3.);
        // Dragged wide on a big screen, then the window made small: the
        // answer gives way first, and the picture keeps its least.
        let (list, talk) = columns(1366., true, Some(500.), Some(1000.));
        assert_eq!(list, 500.);
        assert_eq!(1366. - list - talk, STAGE_MIN);
        // The list put away gives its room to the others.
        assert_eq!(columns(1366., false, Some(500.), None).0, 0.);
        // The narrowest window still lays out, each at its least.
        let (list, talk) = columns(WINDOW_MIN.0, true, Some(LIST_MAX), Some(TALK_MAX));
        assert_eq!((list, talk), (LIST_MIN, TALK_MIN));
    }

    #[test]
    fn a_moved_picture_never_leaves_the_room() {
        // A big picture in a small room: up to all but a strip of it out.
        assert_eq!(kept(10_000., 4000., 1000.), 2500. - KEEP_SEEN);
        assert_eq!(kept(-10_000., 4000., 1000.), -(2500. - KEEP_SEEN));
        // Within that, it goes where it is put.
        assert_eq!(kept(300., 4000., 1000.), 300.);
        // A small one stays wholly seen at the room's edge.
        assert_eq!(kept(10_000., 50., 1000.), 475.);
    }

    #[test]
    fn a_dragged_edge_stays_in_bounds() {
        assert_eq!(dragged(100., LIST_MIN, LIST_MAX, 1920., 600.), LIST_MIN);
        assert_eq!(dragged(900., LIST_MIN, LIST_MAX, 1920., 600.), LIST_MAX);
        // Never over the surface's least.
        assert_eq!(dragged(1500., TALK_MIN, TALK_MAX, 1920., 340.), TALK_MAX);
        assert_eq!(
            dragged(1500., TALK_MIN, TALK_MAX, 1366., 340.),
            1366. - 340. - STAGE_MIN
        );
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
        // Full HD, the stage beside the list and the answer.
        let (list, talk) = columns(1920., true, None, None);
        let room = (1920. - list - talk, 1080. - TOOLS - STRIP_H);
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

    #[test]
    fn a_short_window_keeps_the_question_above_the_words() {
        // Full HD: the full twelve lines.
        assert_eq!(words_lines(1080., 15.), WORDS_LINES);
        // A laptop window at 150%: the words take under a third.
        let n = words_lines(626., 15.);
        assert!(n < WORDS_LINES && n as f32 * 15. * 1.5 <= 626. * WORDS_SHARE);
        // Never fewer than three.
        assert_eq!(words_lines(200., 15.), 3);
    }
}
