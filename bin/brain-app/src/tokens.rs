//! Every visual value the Brain draws with that the kit's theme does not
//! already name. Colours come from the theme (`parvi_app_kit::theme`) and
//! are only mixed here, so a new theme reaches the sky and the reader.

use gpui::Hsla;
use parvi_app_kit::theme::{Theme, alpha};

/// The sky's categorical hues: as many as the eye tells apart at a glance,
/// turned around the colour wheel from the theme's accent, soft enough to
/// read as light on the dark.
pub mod hue {
    pub const COUNT: usize = 9;
    pub const SATURATION: f32 = 0.52;
    pub const LIGHTNESS: f32 = 0.7;
}

/// A subject's hue, stable while others come and go.
pub fn subject_hue(t: &Theme, id: &str) -> Hsla {
    let h = id
        .bytes()
        .fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
    let i = (h % hue::COUNT as u32) as f32;
    Hsla {
        h: (t.accent.h + i / hue::COUNT as f32).fract(),
        s: hue::SATURATION,
        l: hue::LIGHTNESS,
        a: 1.,
    }
}

/// A colour moved towards white, for star cores that should read as light.
pub fn whiten(c: Hsla, share: f32) -> Hsla {
    Hsla {
        l: c.l + (1. - c.l) * share,
        s: c.s * (1. - share * 0.5),
        ..c
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
    /// A bridge shows from afar once this many links run along it.
    pub const STRONG: u32 = 4;
    /// How much a lit search dims everything it did not find.
    pub const UNMATCHED: f32 = 0.12;
    /// How much a page pointed at or read dims what it does not link.
    pub const UNRELATED: f32 = 0.28;
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
    pub const THREAD_ALPHA: f32 = 0.3;
    /// Names.
    pub const SYSTEM_NAME: f32 = 14.;
    pub const PAGE_NAME: f32 = 12.;
    pub const PAGE_NAME_MAX: usize = 42;
    /// A system's name sits this share of its radius below its centre when
    /// the sky is far.
    pub const NAME_BELOW: f32 = 0.8;
    /// The dark a name is lifted off the sky by, and how far it spreads.
    pub const HALO: f32 = 0.7;
    pub const HALO_BLUR: f32 = 7.;
    /// The scale a flight to one page lands at, and the closest a fit goes.
    pub const PAGE_SCALE: f32 = 1.6;
    pub const FIT_MOST: f32 = 1.5;
    pub const FIT_PAD: f32 = 48.;
    /// The arrows at the edge for links that leave the screen.
    pub const SIGNPOSTS: usize = 8;
    pub const SIGNPOST_INSET: f32 = 26.;
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
    pub const ROWS: usize = 8;
    pub const ROW_PAD_X: f32 = 14.;
    pub const ROW_PAD_Y: f32 = 8.;
    pub const TITLE: f32 = 14.;
    pub const SNIPPET: f32 = 12.;
    /// The best results a search flies the camera to.
    pub const FLY_TO: usize = 8;
}

/// The column a Gather search fills.
pub mod gather {
    pub const LEFT: f32 = 28.;
    pub const WIDTH: f32 = 420.;
    pub const TOP: f32 = 92.;
    pub const SLOT: f32 = 70.;
    /// A gathered star's least size.
    pub const STAR: f32 = 4.;
    /// Where in a slot its star sits, from the slot's left.
    pub const STAR_X: f32 = 8.;
    pub const TEXT_X: f32 = 28.;
    pub const MOST: usize = 14;
    pub const SCRIM: f32 = 0.86;
}

/// The orbit a page read in Orbit stands at the centre of.
pub mod orbit {
    /// Room left beside and above the outermost ring, for names.
    pub const ROOM_X: f32 = 196.;
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
    pub const NAME_MAX: usize = 24;
    /// The gap between two subjects' arcs, in radians.
    pub const ARC_GAP: f32 = 0.16;
}

/// The reader.
pub mod reader {
    /// The sheet's share of the window, its least and its most.
    pub const SHARE: f32 = 0.46;
    pub const MIN: f32 = 520.;
    pub const MAX: f32 = 820.;
    pub const PAD_X: f32 = 44.;
    pub const PAD_TOP: f32 = 36.;
    /// The widest a line of prose runs.
    pub const MEASURE: f32 = 700.;
    pub const TITLE: f32 = 27.;
    pub const TITLE_LINE: f32 = 36.;
    pub const LEDE: f32 = 16.;
    pub const LEDE_LINE: f32 = 26.;
    pub const BODY: f32 = 15.5;
    pub const BODY_LINE: f32 = 27.;
    pub const H2: f32 = 19.;
    pub const H3: f32 = 16.5;
    pub const META: f32 = 12.;
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

/// Chrome over the sky: the counts, the switch between takes.
pub mod chrome {
    pub const INSET: f32 = 16.;
    /// The line of counts along the bottom.
    pub const FOOT: f32 = 20.;
    pub const TEXT: f32 = 12.;
    pub const SWITCH_PAD_X: f32 = 12.;
    pub const SWITCH_HEIGHT: f32 = 30.;
    pub const PANEL_ALPHA: f32 = 0.88;
    pub const RADIUS: f32 = 8.;
}

/// The panel colour over the sky, letting a little of it through.
pub fn veil(t: &Theme, a: f32) -> Hsla {
    alpha(t.surface, a)
}
