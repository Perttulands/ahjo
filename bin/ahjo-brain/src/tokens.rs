//! Every visual value the Brain draws with that the kit's theme does not
//! already name. Colours come from the theme (`ahjo_app_kit::theme`) and
//! are only mixed here, so a new theme reaches the sky and the reader.

use ahjo_app_kit::theme::{Direction, Theme, alpha, size, ty};
use gpui::Hsla;

/// The sky's character in each of the theme's directions.
///
/// - Night flight: a cold sky over the fjords; subjects in the aurora's
///   run from green through blue to violet, soft glows, deep nebulae.
/// - Saga: a sky seen through hearth smoke; subjects in ember, gold and
///   moss, warm light, the far field as drifting sparks.
/// - Signal: the sky as a sign; flat colour, square stars, no glow, names
///   on solid plates.
/// - A well-known theme: the whole wheel at even steps, the night's soft
///   glow on a dark ground; on a light one, deeper colours and less glow,
///   the sky as ink on paper.
pub struct Style {
    /// Where on the colour wheel the subjects' hues start, and how far
    /// round they run.
    pub hue_from: f32,
    pub hue_span: f32,
    pub saturation: f32,
    pub lightness: f32,
    /// Neighbouring hues alternate their lightness by this much, so a
    /// narrow run of the wheel still tells subjects apart.
    pub lightness_step: f32,
    /// How strongly stars glow and nebulae shine, as shares of the sky's
    /// measures; how soft a nebula's edge is, as a share of its radius.
    pub glow: f32,
    pub nebula: f32,
    pub nebula_soft: f32,
    pub nebula_size: f32,
    /// How much lines bow, as a share of the sky's bends.
    pub bend: f32,
    /// The far field's light, as a share of [`sky::FIELD_ALPHA`].
    pub field: f32,
    /// Stars and the far field drawn square.
    pub square: bool,
    /// The dark under a name: how far it spreads and how soft it is.
    pub halo_blur: f32,
    pub halo_spread: f32,
    /// How far a star's core is lit towards white.
    pub core_white: f32,
}

pub fn style(t: &Theme) -> Style {
    match t.direction {
        Direction::Night => Style {
            hue_from: 0.4,
            hue_span: 0.42,
            saturation: 0.6,
            lightness: 0.7,
            lightness_step: 0.07,
            glow: 1.,
            nebula: 1.2,
            nebula_soft: 0.45,
            nebula_size: 0.5,
            bend: 1.0,
            field: 1.,
            square: false,
            halo_blur: sky::HALO_BLUR,
            halo_spread: 2.,
            core_white: 0.75,
        },
        Direction::Saga => Style {
            hue_from: 0.98,
            hue_span: 0.3,
            saturation: 0.6,
            lightness: 0.64,
            lightness_step: 0.1,
            glow: 1.1,
            nebula: 1.,
            nebula_soft: 0.55,
            nebula_size: 0.5,
            bend: 1.3,
            field: 0.8,
            square: false,
            halo_blur: sky::HALO_BLUR + 3.,
            halo_spread: 3.,
            core_white: 0.6,
        },
        Direction::Signal => Style {
            hue_from: 0.22,
            hue_span: 0.86,
            saturation: 0.8,
            lightness: 0.62,
            lightness_step: 0.06,
            glow: 0.,
            nebula: 0.55,
            nebula_soft: 0.,
            nebula_size: 0.95,
            bend: 0.0,
            field: 0.6,
            square: true,
            halo_blur: 0.,
            halo_spread: 3.,
            core_white: 0.3,
        },
        _ if t.light => Style {
            hue_from: 0.55,
            hue_span: 0.86,
            saturation: 0.7,
            lightness: 0.4,
            lightness_step: 0.08,
            glow: 0.35,
            nebula: 0.7,
            nebula_soft: 0.45,
            nebula_size: 0.5,
            bend: 1.0,
            field: 1.4,
            square: false,
            halo_blur: sky::HALO_BLUR,
            halo_spread: 2.,
            core_white: 0.35,
        },
        _ => Style {
            hue_from: 0.55,
            hue_span: 0.86,
            saturation: 0.7,
            lightness: 0.68,
            lightness_step: 0.07,
            glow: 0.9,
            nebula: 1.,
            nebula_soft: 0.45,
            nebula_size: 0.5,
            bend: 1.0,
            field: 1.,
            square: false,
            halo_blur: sky::HALO_BLUR,
            halo_spread: 2.,
            core_white: 0.6,
        },
    }
}

/// How many hues the subjects share: as many as the eye tells apart at a
/// glance.
pub const HUES: usize = 9;

/// A subject's hue, stable while others come and go.
pub fn subject_hue(t: &Theme, id: &str) -> Hsla {
    let s = style(t);
    let h = id
        .bytes()
        .fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
    let i = h % HUES as u32;
    let lift = if i.is_multiple_of(2) { 1. } else { -1. };
    Hsla {
        h: (s.hue_from + s.hue_span * i as f32 / (HUES - 1) as f32).fract(),
        s: s.saturation,
        l: s.lightness + lift * s.lightness_step / 2.,
        a: 1.,
    }
}

/// A colour lifted off the sky, for star cores that should stand out:
/// towards white on a dark ground, towards the ink on a light one.
pub fn lift(t: &Theme, c: Hsla, share: f32) -> Hsla {
    let to = if t.light { 0. } else { 1. };
    Hsla {
        // On paper a name has to be ink: yellows and greens at the stars'
        // own lightness do not read there.
        l: c.l + (to - c.l) * share * if t.light { LIFT_ON_LIGHT } else { 1. },
        s: c.s * (1. - share * 0.5),
        ..c
    }
}

/// A subject's hue as words to read (a link, a subject's name): as it is on
/// a dark ground; on paper taken towards the ink until it reads.
pub fn words(t: &Theme, hue: Hsla) -> Hsla {
    if t.light { lift(t, hue, 0.3) } else { hue }
}

/// How much further a lifted colour goes towards the ink on a light ground.
pub const LIFT_ON_LIGHT: f32 = 1.6;

/// A quiet line in a subject's hue (a system's count): fainter on a dark
/// sky than on paper, where a faint hue is lost.
pub fn quiet(t: &Theme, hue: Hsla, lit: bool) -> Hsla {
    match (t.light, lit) {
        (true, _) => lift(t, hue, 0.25),
        (false, true) => Hsla { a: 0.75, ..hue },
        (false, false) => Hsla { a: 0.5, ..hue },
    }
}

/// The sky.
pub mod sky {
    /// Below this zoom only systems are named; above it, pages too.
    pub const NEAR: f32 = 0.65;
    /// Up close even evidence is named.
    pub const CLOSE: f32 = 3.2;
    /// The lantern's reach around the pointer, and how many it names.
    pub const LANTERN: f32 = 110.;
    pub const LANTERN_NAMES: usize = 7;
    /// How near a star the pointer must be to point at it.
    pub const HIT: f32 = 12.;
    /// The most page names drawn at once.
    pub const NAMES: usize = 70;
    /// The room a system's count keeps clear of every other name.
    pub const COUNT_PAD: f32 = 10.;
    /// A bridge shows from afar once this many links run along it.
    pub const STRONG: u32 = 4;
    /// How much a lit search dims everything it did not find.
    pub const UNMATCHED: f32 = 0.12;
    /// How much a page read, or a link in it pointed at, dims what it does
    /// not link; a star only pointed at in the sky dims less, so passing
    /// the pointer over the sky does not make it flicker.
    pub const UNRELATED: f32 = 0.3;
    pub const UNRELATED_HOVER: f32 = 0.62;
    /// How much the systems dim under a search or a page read, and under a
    /// star pointed at.
    pub const SYSTEMS_DIMMED: f32 = 0.55;
    pub const SYSTEMS_HOVER: f32 = 0.8;
    /// The light of a nebula, from none to the most evidenced subject's.
    pub const NEBULA_MIN: f32 = 0.05;
    pub const NEBULA_MAX: f32 = 0.24;
    /// The faint far field behind the systems, moving slower than they do.
    pub const FIELD_PER_TILE: usize = 70;
    pub const FIELD_TILE: f32 = 600.;
    pub const FIELD_SIZE: f32 = 0.7;
    pub const FIELD_PARALLAX: f32 = 0.18;
    pub const FIELD_ALPHA: f32 = 0.35;
    /// Star sizes in pixels at the scale they settle at.
    pub const CORE_MIN: f32 = 2.6;
    pub const CORE_MAX: f32 = 6.;
    pub const STAR_MIN: f32 = 1.1;
    pub const STAR_MAX: f32 = 4.;
    pub const DUST_MIN: f32 = 0.9;
    pub const DUST_MAX: f32 = 2.4;
    /// A glow's reach as a multiple of its star.
    pub const GLOW: f32 = 4.5;
    /// The rings: a found star, the star pointed at or read.
    pub const FOUND_RING: f32 = 4.;
    pub const HOT_RING: f32 = 6.;
    /// Lines: bridges between systems, links from a page.
    pub const BRIDGE_BEND: f32 = 0.12;
    pub const LINK_BEND: f32 = 0.06;
    pub const LINK_ALPHA: f32 = 0.6;
    /// Up to this many links draw at full strength; more share it.
    pub const LINKS_FULL: f32 = 10.;
    /// A link leaving the view draws at this share.
    pub const LINK_AWAY: f32 = 0.4;
    /// Names. The Brain is read full screen from a sitting distance: no
    /// name in the sky is smaller than the chrome's text, a page's name
    /// stands at the theme's title size.
    pub const SYSTEM_NAME: f32 = super::ty::TITLE;
    /// Far out, a system's name grows with how much it holds.
    pub const SYSTEM_NAME_MIN: f32 = super::size::TEXT;
    pub const SYSTEM_NAME_MAX: f32 = 17.;
    /// What counts as moved lately, in days.
    pub const MOVED_DAYS: i32 = 7;
    pub const PAGE_NAME: f32 = super::ty::TITLE;
    pub const PAGE_NAME_MAX: usize = 42;
    /// The least clear sky between two names, so each reads as its own.
    pub const NAME_GAP: f32 = 8.;
    /// A system's name sits this share of its radius below its centre when
    /// the sky is far.
    pub const NAME_BELOW: f32 = 0.8;
    /// The dark a name is lifted off the sky by, and how far it spreads.
    pub const HALO: f32 = 0.7;
    pub const HALO_BLUR: f32 = 7.;
    /// The scale a flight to one page lands at, and the closest a fit goes.
    pub const PAGE_SCALE: f32 = 1.6;
    /// A small sky is spread to the window's room rather than kept small
    /// in its middle, up to this.
    pub const FIT_MOST: f32 = 3.;
    pub const FIT_PAD: f32 = 48.;
    /// In the replay, a page not touched lately keeps this much of its light,
    /// and the most pages it names on a day.
    pub const REPLAY_BASE: f32 = 0.16;
    pub const REPLAY_NAMES: usize = 12;
}

/// Motion, in milliseconds.
pub mod motion {
    /// A star's light follows a change at this pace (to two thirds).
    pub const FADE: f32 = 140.;
    /// A flight: its least, its pace per unit of path, its most.
    pub const FLIGHT_BASE: f32 = 420.;
    pub const FLIGHT_PER_UNIT: f32 = 240.;
    pub const FLIGHT_MOST: f32 = 1500.;
    /// Stars travelling between the sky and a place of their own.
    pub const PULL: f32 = 620.;
    /// A star's departure is delayed by its rank, so a column fills in order.
    pub const PULL_STAGGER: f32 = 22.;
    /// The sky gathering out of its systems on first light.
    pub const GATHER: f32 = 1100.;
    pub const GATHER_SPREAD: f32 = 420.;
    /// A page's links drawing outward from it.
    pub const TRACE: f32 = 520.;
    /// A fresh page's flare when the sky first lights.
    pub const FLARE: f32 = 1600.;
    /// How long a page counts as fresh, in days.
    pub const FRESH_DAYS: i32 = 2;
    /// How long typing rests before it is searched.
    pub const SEARCH_WAIT: u64 = 110;
    /// The replay: how long a day worked on lasts, and how much of its
    /// warmth a page keeps a day later.
    pub const REPLAY_DAY: f32 = 700.;
    pub const REPLAY_COOL: f32 = 0.72;
    /// Wheel lines to zoom: each line zooms by this factor.
    pub const WHEEL_ZOOM: f32 = 1.12;
    /// Pixels a key press pans.
    pub const KEY_PAN: f32 = 80.;
}

/// The search over the sky.
pub mod search {
    pub const WIDTH: f32 = 460.;
    pub const FIELD_HEIGHT: f32 = 40.;
    pub const FIELD_TEXT: f32 = 15.;
    /// The gap between the field and the list under it; the room the
    /// notice that Citadel is away takes over the list.
    pub const LIST_GAP: f32 = 6.;
    pub const AWAY: f32 = 52.;
    /// What changed lately, before anything is asked.
    pub const RECENT: usize = 8;
    /// The inbox's list at its widest.
    pub const INBOX_W: f32 = 480.;
    pub const ROW_PAD_X: f32 = 14.;
    pub const ROW_PAD_Y: f32 = 8.;
    /// A result's title and the words under it: the snippet at the
    /// theme's title size, the title a step over it.
    pub const TITLE: f32 = super::ty::EMPTY;
    pub const SNIPPET: f32 = super::ty::TITLE;
    /// The most a search matched here, without Citadel, shows.
    pub const LOCAL_MOST: usize = 50;
    /// The best results a search flies the camera to.
    pub const FLY_TO: usize = 8;
    /// While a page is read, the ranked results stay beside its orbit:
    /// at least this wide (wider as the orbit leaves room, up to the
    /// search's width), titles only, and only if the orbit keeps at least
    /// this much room.
    pub const ASIDE: f32 = 270.;
    pub const ASIDE_ROW: f32 = 32.;
    pub const ASIDE_ROW_PAD: f32 = 4.;
    pub const ORBIT_LEAST: f32 = 680.;
    /// The room the orbit is left before the results beside it widen:
    /// the orbit grows with the window first.
    pub const ORBIT_WANTS: f32 = 1100.;
    /// What changed lately widens to its longest title, up to this and
    /// never past this share of the window.
    pub const RECENT_MOST: f32 = 760.;
    pub const RECENT_SHARE: f32 = 0.42;
    /// A letter of the list's mono type, as a share of its size.
    pub const MONO_ADVANCE: f32 = 0.6;
}

/// The orbit a page read stands at the centre of.
pub mod orbit {
    /// Room left beside and above the outermost ring, for names: beside
    /// it at least this, more as the longest name needs (and the pad past
    /// its end), while the ring keeps its least radius.
    pub const ROOM_X: f32 = 196.;
    pub const NAME_PAD: f32 = 16.;
    pub const RING_LEAST: f32 = 170.;
    pub const ROOM_Y: f32 = 56.;
    /// Inside the ring, each subject's bow, as a share of its radius.
    pub const BOW: f32 = 0.9;
    /// The key naming the subjects: how many, and its line.
    pub const KEY: usize = 6;
    pub const KEY_LINE: f32 = 20.;
    /// A second ring for a crowded page, as a share of the first.
    pub const OUTER: f32 = 1.28;
    /// Planets per ring before a second one is used.
    pub const CROWD: usize = 28;
    pub const SUN: f32 = 9.;
    pub const PLANET_MIN: f32 = 2.6;
    pub const PLANET_MAX: f32 = 6.;
    /// A planet's name: the most letters it shows and the least it is
    /// cut to (it wraps onto a second line before it is cut), a letter
    /// being about this share of its size wide, and how far it stands off
    /// its planet.
    pub const NAME_MOST: usize = 44;
    pub const NAME_LEAST: usize = 8;
    /// The ring leaves room for a name of this many letters on one line;
    /// a longer one is reckoned as two lines, the wider a few letters over
    /// half.
    pub const NAME_LINE: usize = 16;
    pub const NAME_LINE_SLACK: usize = 4;
    pub const NAME_CHAR: f32 = 0.62;
    pub const NAME_OFF: f32 = 14.;
    /// The gap between two subjects' arcs, in radians.
    pub const ARC_GAP: f32 = 0.16;
}

/// The replay's strip of days.
pub mod replay {
    pub const STRIP: f32 = 34.;
    pub const GAP: f32 = 2.;
    /// The room its bar takes from the sky's foot.
    pub const ROOM: f32 = 150.;
}

/// The reader.
pub mod reader {
    /// The sheet's share of the window, its least and its most.
    pub const SHARE: f32 = 0.42;
    pub const MIN: f32 = 520.;
    pub const MAX: f32 = 728.;
    /// Less sky than this beside the sheet is no use: the sheet takes the
    /// whole window instead.
    pub const SKY_LEAST: f32 = 380.;
    pub const PAD_X: f32 = 44.;
    pub const PAD_TOP: f32 = 36.;
    /// The widest a line of prose runs: about eighty letters.
    pub const MEASURE: f32 = 640.;
    pub const TITLE: f32 = 27.;
    pub const TITLE_LINE: f32 = 36.;
    pub const LEDE: f32 = 16.;
    pub const LEDE_LINE: f32 = 26.;
    pub const BODY: f32 = 15.5;
    pub const BODY_LINE: f32 = 27.;
    /// The body's least and most as zoomed; the rest of the page follows.
    pub const TEXT_LEAST: f32 = 10.5;
    pub const TEXT_MOST: f32 = 32.5;
    pub const H2: f32 = 19.;
    pub const H3: f32 = 16.5;
    /// The line over the title, and the pages listed under the prose, at
    /// the theme's title size before the reader is zoomed.
    pub const META: f32 = super::ty::TITLE;
    pub const LIST: f32 = super::ty::TITLE;
    pub const CODE: f32 = 13.;
    pub const PARA_GAP: f32 = 14.;
    pub const HEAD_GAP: f32 = 26.;
    pub const LIST_GAP: f32 = 6.;
    pub const INDENT: f32 = 20.;
    pub const DOT: f32 = 8.;
    pub const RULE: f32 = 1.;
    pub const QUOTE_BAR: f32 = 3.;
    pub const RADIUS: f32 = 6.;
    pub const SHEET_ALPHA: f32 = 0.97;
    pub const SHADOW: f32 = 40.;
    /// Neighbours listed under the page, before "and N more".
    pub const NEIGHBOURS: usize = 40;
}

/// Chrome over the sky: the counts, the theme button.
pub mod chrome {
    /// The back arrow and the close cross, drawn: their side and stroke.
    pub const GLYPH: f32 = 12.;
    pub const GLYPH_LINE: f32 = 1.6;

    pub const INSET: f32 = 16.;
    /// The line of counts along the bottom.
    pub const FOOT: f32 = 22.;
    /// Between the things along the bottom.
    pub const FOOT_GAP: f32 = 8.;
    /// The chrome's words (counts, the theme, the replay, notices): at
    /// the theme's title size, as the reader is read from a distance.
    pub const TEXT: f32 = super::ty::TITLE;
    pub const SWITCH_PAD_X: f32 = 12.;
    pub const SWITCH_HEIGHT: f32 = 30.;
    /// Between the chips at the top right, and within one.
    pub const CHIP_GAP: f32 = 6.;
    /// The search is never narrower than this.
    pub const SEARCH_LEAST: f32 = 220.;
    /// How much of the theme's window picture shows behind a launch.
    pub const BACKDROP: f32 = 0.9;
    /// The theme's mark in the switch.
    pub const MARK: f32 = 18.;
    pub const PANEL_ALPHA: f32 = 0.88;
    pub const RADIUS: f32 = 8.;
    /// The reader's back and close stand in from its corner by this.
    pub const SHEET_BAR: f32 = 10.;
}

/// The panel colour over the sky, letting a little of it through.
pub fn veil(t: &Theme, a: f32) -> Hsla {
    alpha(t.surface, a)
}

/// Editing a page in place.
pub mod editor {
    /// The Markdown's size: a little over the chrome's, for prose.
    pub const TEXT: f32 = 14.;
    pub const PAD: f32 = 16.;
    pub const BAR: f32 = 48.;
    /// The pages `[[` offers, and their list's width.
    pub const PICKS: usize = 8;
    pub const PICK_W: f32 = 380.;
    /// A pause in typing this long and the page beside follows, and the
    /// draft is kept.
    pub const SETTLE_MS: u64 = 150;
    /// The least the source takes beside the page; less, and it takes the
    /// window.
    pub const LEAST: f32 = 480.;
    /// The source's widest: about ninety letters a line, wrapped.
    pub const MEASURE: f32 = 860.;
    /// A note over the sky: at most this wide and tall.
    pub const NOTE_W: f32 = 880.;
    pub const NOTE_H: f32 = 760.;
    /// The header shown above the words: at most this many lines before it
    /// scrolls.
    pub const HEAD_LINES: usize = 8;
}
