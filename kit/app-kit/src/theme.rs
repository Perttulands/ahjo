//! Every visual value the app draws with: the theme's colours, type and
//! art, and the fixed measures of the chrome. Views read these and never
//! write a colour or size of their own.
//!
//! A colour carries one meaning, the same on every page: `accent` is what
//! acts and what has focus; `running`, `done`, `needs` (waits for the
//! operator) and `error` (failed) are states; `chart` draws data. A
//! mission's wires have their own: `wire` carries work, `pass` and `fail`
//! leave a gate. No colour stands for two of them. The operator picks the theme
//! ([`Direction`], the `theme` setting); every direction fills every token.

use gpui::{App, FontWeight, Global, Hsla, Image, ImageFormat, Rgba};
use std::sync::{Arc, OnceLock};

/// Measures in pixels, from CHROTE's layout (DESIGN-SYSTEM.md and the
/// dashboard as it renders). A page's own measures live with the page.
pub mod size {
    pub const TAB_BAR: f32 = 44.;
    pub const ROW: f32 = 28.;
    pub const CONTROL: f32 = 28.;
    pub const RADIUS: f32 = 4.;
    /// Chrome text and its small companion (labels, counts, hints).
    pub const TEXT: f32 = 13.;
    pub const SMALL: f32 = 11.;
    /// The widest a notice over an empty page grows (a failed page's).
    pub const NOTICE: f32 = 420.;
    /// The name column of a page of settings or help.
    pub const LABEL_COL: f32 = 240.;
    /// The keys panel and a menu: their widths.
    pub const KEYS: f32 = 520.;
    pub const MENU: f32 = 220.;
    /// The side panel at the window's left (sessions, files), and the
    /// edge it leaves when put away.
    pub const SIDE: f32 = 260.;
    pub const SIDE_SHUT: f32 = 36.;
    /// A tab card's gap above it.
    pub const TAB_CARD_TOP: f32 = 6.;
    /// The bar under the shown tab, and its inset from the tab's sides.
    pub const TAB_MARK: f32 = 2.;
    pub const TAB_MARK_INSET: f32 = 8.;
    /// How far in from its ends the tab mark starts as it draws out.
    pub const TAB_MARK_DRAW: f32 = 14.;
    /// A state's dot.
    pub const DOT: f32 = 7.;
    /// The plate a group's count stands on.
    pub const COUNT: f32 = 18.;
    /// A list row's inset from its list's sides.
    pub const ROW_INSET: f32 = 6.;
    /// The page skeleton (`widgets::pane`): the list's width, as the
    /// terminals' session rail; the detail's padding, its widest reading
    /// width and the gap between its sections.
    pub const LIST: f32 = 260.;
    /// A list whose rows are sentences (work items, search results with
    /// their words), which a name's width would cut to a few words.
    pub const LIST_WIDE: f32 = 420.;
    pub const PAD: f32 = 24.;
    pub const READING: f32 = 960.;
    pub const SECTION_GAP: f32 = 24.;
    /// A head's padding at its ends and the gap between its parts.
    pub const HEAD_PAD: f32 = 12.;
    pub const HEAD_GAP: f32 = 8.;
    /// Where a row's words start, from its list's side.
    pub const ROW_PAD: f32 = 12.;
    /// A label's room above and below it.
    pub const LABEL_TOP: f32 = 12.;
    pub const LABEL_BOTTOM: f32 = 4.;
    /// The name column of a detail's named lines.
    pub const NAME_COL: f32 = 200.;
}

/// Measures the pages that list things share (a rail of rows beside a
/// reader, a table, a device's room), carried over from the old app.
pub mod board {
    /// A page's padding, and the gap between its parts.
    pub const PAD: f32 = 24.;
    pub const GAP: f32 = 16.;
    /// A page heading.
    pub const HEADING: f32 = 18.;
    /// The side list beside a detail.
    pub const LIST_W: f32 = 380.;
    /// A reading column's widest.
    pub const READER_MAX: f32 = 760.;
    /// A device's room has walls this thick.
    pub const WALL: f32 = 2.;
    /// A table row, and the status glyph in it.
    pub const ROW: f32 = 34.;
    pub const GLYPH: f32 = 8.;
}

/// How much of a thing shows when it steps back.
pub mod fade {
    pub const HOVER: f32 = 0.9;
    /// A control with nothing to do now.
    pub const IDLE: f32 = 0.5;
}

/// Toast timing in milliseconds: in, shown, out.
pub mod motion {
    pub const TOAST_IN: u64 = 120;
    pub const TOAST_SHOWN: u64 = 1800;
    pub const TOAST_OUT: u64 = 200;
    /// A page coming up as its tab is chosen, and how much of the
    /// background veils it at first.
    pub const PAGE_IN: u64 = 180;
    pub const PAGE_VEIL: f32 = 0.6;
    /// The whole window coming up in a theme chosen.
    pub const THEME_IN: u64 = 360;
    /// A menu or a layer opening, and how far a menu drops into place.
    pub const PANEL_IN: u64 = 140;
    pub const PANEL_DROP: f32 = 6.;
    /// The active tab's mark drawing out from its middle.
    pub const TAB_IN: u64 = 220;
}

/// The type scale: what is read first is largest. Body text is
/// [`size::TEXT`] and its small companion [`size::SMALL`].
pub mod ty {
    /// A page's own name at its head, in the theme's display face.
    pub const PAGE: f32 = 22.;
    /// The name of the thing a detail shows, in its head.
    pub const TITLE: f32 = 15.;
    /// A number worth reading at a glance (a reading, a count).
    pub const GLANCE: f32 = 30.;
    /// What an empty place says, under its art.
    pub const EMPTY: f32 = 16.;
    /// The line height of prose, as a share of its size.
    pub const READ_LEADING: f32 = 1.55;
}

/// How the art sits behind the interface.
pub mod art {
    /// The widest an empty place's picture is drawn, and the least room
    /// it needs to be drawn at all (a narrow list says its words alone).
    pub const EMPTY: f32 = 220.;
    pub const EMPTY_ROOM: f32 = 360.;
    /// The app's mark at the tab bar's start, and its room.
    pub const MARK: f32 = 26.;
    pub const MARK_PAD: f32 = 12.;
}

pub const FAMILY: &str = "JetBrains Mono";

/// The family's files, regular and bold, as anything that draws text off
/// the UI (a diagram) loads them: the same letters on every system.
pub const FAMILY_FILES: [&[u8]; 2] = [
    include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMono-Bold.ttf"),
];

/// Every face the app registers: the family, and each direction's display
/// and reading faces (all under the SIL Open Font License, beside them).
pub const FONT_FILES: [&[u8]; 16] = [
    FAMILY_FILES[0],
    FAMILY_FILES[1],
    include_bytes!("../assets/fonts/JetBrainsMono-Italic.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMono-BoldItalic.ttf"),
    include_bytes!("../assets/fonts/SpaceGrotesk-Medium.ttf"),
    include_bytes!("../assets/fonts/SpaceGrotesk-Bold.ttf"),
    include_bytes!("../assets/fonts/Inter-Regular.ttf"),
    include_bytes!("../assets/fonts/Inter-Bold.ttf"),
    include_bytes!("../assets/fonts/Inter-Italic.ttf"),
    include_bytes!("../assets/fonts/Cinzel-SemiBold.ttf"),
    include_bytes!("../assets/fonts/Cinzel-Bold.ttf"),
    include_bytes!("../assets/fonts/EBGaramond-Regular.ttf"),
    include_bytes!("../assets/fonts/EBGaramond-Bold.ttf"),
    include_bytes!("../assets/fonts/EBGaramond-Italic.ttf"),
    include_bytes!("../assets/fonts/Archivo-Regular.ttf"),
    include_bytes!("../assets/fonts/Archivo-ExtraBold.ttf"),
];

/// The theme's three ideas of Odin's ravens, Huginn and Muninn, thought
/// and memory, who fly out over the world and come back to report.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Direction {
    /// Huginn's night flight: a cold moonlit sky over the fjords, aurora
    /// in it; a clean grotesk over the mono.
    #[default]
    Night,
    /// Muninn's saga: what is remembered, carved and engraved; hearth gold
    /// on smoked wood, inscribed capitals, a book face for reading.
    Saga,
    /// The raven as a sign: flat black and bone, one signal yellow, heavy
    /// grotesk headers, nothing soft.
    Signal,
}

impl Direction {
    pub const ALL: [Direction; 3] = [Direction::Night, Direction::Saga, Direction::Signal];

    /// Each direction's key and name, as a setting offers them.
    pub const CHOICES: &[(&str, &str)] = &[
        ("night", "Night flight"),
        ("saga", "Saga"),
        ("signal", "Signal"),
    ];

    /// Its value in the `theme` setting.
    pub fn key(self) -> &'static str {
        match self {
            Direction::Night => "night",
            Direction::Saga => "saga",
            Direction::Signal => "signal",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Direction::Night => "Night flight",
            Direction::Saga => "Saga",
            Direction::Signal => "Signal",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|d| d.key() == key)
    }
}

/// A direction's type: the display face for page names, group labels,
/// numbers read at a glance and empty places; the reading face for prose.
/// Rows, fields and terminals stay in [`FAMILY`].
#[derive(Clone, Debug)]
pub struct Type {
    pub display: &'static str,
    pub display_weight: FontWeight,
    pub reading: &'static str,
    /// A group's or a section's label: its size, whether in capitals
    /// (Cinzel's own lower case is small capitals, which read better
    /// small), and the room between its letters.
    pub group: f32,
    /// Prose (a document, a work item's description, a brain page) at the
    /// size its face reads at: a book face wants more than a grotesk.
    pub read: f32,
    pub labels_upper: bool,
    pub tracking: f32,
}

/// A direction's pictures: behind an empty terminal window, over an empty
/// place, and the app's mark.
#[derive(Clone, Debug)]
pub struct Art {
    pub window: Arc<Image>,
    pub empty: Arc<Image>,
    pub mark: Arc<Image>,
    /// How much of the window's picture shows behind what stands on it.
    pub window_show: f32,
    /// How much of an empty place's picture shows.
    pub empty_show: f32,
}

#[derive(Clone, Debug)]
pub struct Theme {
    pub direction: Direction,
    pub bg: Hsla,
    pub surface: Hsla,
    pub raised: Hsla,
    pub divider: Hsla,
    pub text: Hsla,
    pub text2: Hsla,
    pub dim: Hsla,
    /// What acts and what has focus: the primary button, a focused field
    /// or window, a link, the tab shown.
    pub accent: Hsla,
    /// Failed.
    pub error: Hsla,
    /// Running now.
    pub running: Hsla,
    /// Done, passed, finished well.
    pub done: Hsla,
    /// Waits for the operator.
    pub needs: Hsla,
    /// Data drawn: a chart's line and fill.
    pub chart: Hsla,
    /// A mission's wire that carries work from one card to the next, and
    /// its ports.
    pub wire: Hsla,
    /// A gate's pass route and port.
    pub pass: Hsla,
    /// A gate's fail route and port, and the loop back it makes.
    pub fail: Hsla,
    pub term_bg: Hsla,
    pub term_fg: Hsla,
    pub cursor: Hsla,
    pub selection: Hsla,
    /// ANSI 0..16 as 0xRRGGBB, for the terminal's palette.
    pub ansi: [u32; 16],
    pub term_bg_rgb: u32,
    pub term_fg_rgb: u32,
    pub identity: Vec<Hsla>,
    pub ty: Type,
    pub art: Art,
}

impl Global for Theme {}

pub fn theme(cx: &App) -> &Theme {
    cx.global::<Theme>()
}

pub fn rgb(c: u32) -> Hsla {
    rgba(c, 1.)
}

pub fn rgba(c: u32, a: f32) -> Hsla {
    Rgba {
        r: ((c >> 16) & 0xff) as f32 / 255.,
        g: ((c >> 8) & 0xff) as f32 / 255.,
        b: (c & 0xff) as f32 / 255.,
        a,
    }
    .into()
}

pub fn alpha(c: Hsla, a: f32) -> Hsla {
    Hsla { a, ..c }
}

/// One direction's values, as written.
struct Palette {
    bg: u32,
    surface: u32,
    raised: u32,
    divider: u32,
    text: u32,
    text2: u32,
    dim: u32,
    accent: u32,
    error: u32,
    running: u32,
    done: u32,
    needs: u32,
    chart: u32,
    wire: u32,
    pass: u32,
    fail: u32,
    term_bg: u32,
    term_fg: u32,
    ansi: [u32; 16],
    identity: [u32; 4],
}

const NIGHT: Palette = Palette {
    bg: 0x0a0f18,
    surface: 0x0f1622,
    raised: 0x172133,
    divider: 0x243149,
    text: 0xe4ebf5,
    text2: 0xa3b1c6,
    dim: 0x6b7a93,
    accent: 0x6cb6ff,
    error: 0xff6b7a,
    running: 0xa98bff,
    done: 0x5fd3a0,
    needs: 0xffbf5a,
    chart: 0xaab4c2,
    wire: 0x8a9bb5,
    pass: 0xb5e550,
    fail: 0xff5fc8,
    term_bg: 0x080c14,
    term_fg: 0xd8e1ee,
    ansi: [
        0x0a0f18, 0xff6b7a, 0x5fd3a0, 0xffbf5a, 0x6cb6ff, 0xc49bff, 0x4fd6d6, 0xa3b1c6, 0x55627a,
        0xff8c97, 0x86e3b8, 0xffd28a, 0x9acdff, 0xd8bfff, 0x7fe6e6, 0xf2f6fb,
    ],
    identity: [0x3f5f8a, 0x2f6b6b, 0x5b4f8a, 0x6a5a3a],
};

const SAGA: Palette = Palette {
    bg: 0x120e0a,
    surface: 0x1a140f,
    raised: 0x261d15,
    divider: 0x3b2d20,
    text: 0xefe3cc,
    text2: 0xc2ad8c,
    dim: 0x8a7659,
    accent: 0xe0a94a,
    error: 0xe5574b,
    running: 0x6fa8dc,
    done: 0x9cbf6a,
    needs: 0x4fc3b5,
    chart: 0xc9c2b4,
    wire: 0xa8997e,
    pass: 0x6fcf7a,
    fail: 0xd06a9a,
    term_bg: 0x0e0a07,
    term_fg: 0xe8dcc4,
    ansi: [
        0x120e0a, 0xe5574b, 0x9cbf6a, 0xe0a94a, 0x6fa8dc, 0xc7849e, 0x4fc3b5, 0xc2ad8c, 0x6e5d46,
        0xf07a6e, 0xb6d68a, 0xf0c46e, 0x94c0ea, 0xdba3b9, 0x7fd8cd, 0xf5ecd9,
    ],
    identity: [0x7a5230, 0x4f6a3a, 0x7a3f36, 0x3f5a6a],
};

const SIGNAL: Palette = Palette {
    bg: 0x0b0b0b,
    surface: 0x121212,
    raised: 0x1d1d1d,
    divider: 0x2e2e2e,
    text: 0xf0ece3,
    text2: 0xb8b3a8,
    dim: 0x77736b,
    accent: 0xffd000,
    error: 0xff5a36,
    running: 0x3ad0ff,
    done: 0x9ee34a,
    needs: 0xff4fa0,
    chart: 0xcfcac0,
    wire: 0x8a867e,
    pass: 0x3ae0a0,
    fail: 0xb06bff,
    term_bg: 0x080808,
    term_fg: 0xece8df,
    ansi: [
        0x0b0b0b, 0xff5a36, 0x9ee34a, 0xffd000, 0x3a8dff, 0xff4fa0, 0x3ad0ff, 0xcfcac0, 0x5a5751,
        0xff7f61, 0xbdf27a, 0xffe14d, 0x6eadff, 0xff80bd, 0x7fe2ff, 0xffffff,
    ],
    identity: [0x4a4a4a, 0x6b5a1e, 0x1e5a6b, 0x6b1e45],
};

macro_rules! art {
    ($name:literal) => {{
        static ART: OnceLock<Art> = OnceLock::new();
        ART.get_or_init(|| Art {
            window: picture(
                ImageFormat::Jpeg,
                include_bytes!(concat!("../assets/art/", $name, "-window.jpg")),
            ),
            empty: picture(
                ImageFormat::Webp,
                include_bytes!(concat!("../assets/art/", $name, "-empty.webp")),
            ),
            mark: picture(
                ImageFormat::Png,
                include_bytes!(concat!("../assets/art/", $name, "-mark.png")),
            ),
            window_show: 1.,
            empty_show: 1.,
        })
        .clone()
    }};
}

fn picture(format: ImageFormat, bytes: &[u8]) -> Arc<Image> {
    Arc::new(Image::from_bytes(format, bytes.to_vec()))
}

/// The theme chosen in Parvi's settings, for an app of the kit that runs
/// on its own (Brain, Desk) and draws as the operator chose. No settings,
/// or settings that do not read, give the default; so does a theme no
/// direction is called, with a notice that says so. The file is only read.
pub fn chosen() -> (Theme, Option<String>) {
    let settings = crate::settings::path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|text| crate::settings::Settings::parse(&text));
    let Some(settings) = settings else {
        return (Theme::default(), None);
    };
    match Direction::from_key(&settings.theme) {
        Some(d) => (Theme::new(d), None),
        None => (
            Theme::default(),
            Some(format!(
                "No theme is called {:?}; drawn in {}",
                settings.theme,
                Direction::default().label()
            )),
        ),
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::new(Direction::default())
    }
}

impl Theme {
    pub fn new(direction: Direction) -> Self {
        let (p, ty, art) = match direction {
            Direction::Night => (
                &NIGHT,
                Type {
                    display: "Space Grotesk",
                    display_weight: FontWeight::BOLD,
                    reading: "Inter",
                    group: 11.,
                    read: 15.,
                    labels_upper: true,
                    tracking: 1.5,
                },
                Art {
                    empty_show: 0.85,
                    ..art!("night")
                },
            ),
            Direction::Saga => (
                &SAGA,
                Type {
                    display: "Cinzel",
                    display_weight: FontWeight::BOLD,
                    reading: "EB Garamond",
                    group: 14.,
                    read: 17.,
                    labels_upper: false,
                    tracking: 0.6,
                },
                Art {
                    window_show: 0.85,
                    empty_show: 0.55,
                    ..art!("saga")
                },
            ),
            Direction::Signal => (
                &SIGNAL,
                Type {
                    display: "Archivo",
                    display_weight: FontWeight::EXTRA_BOLD,
                    reading: FAMILY,
                    group: 11.,
                    read: 14.,
                    labels_upper: true,
                    tracking: 2.,
                },
                art!("signal"),
            ),
        };
        Self {
            direction,
            bg: rgb(p.bg),
            surface: rgb(p.surface),
            raised: rgb(p.raised),
            divider: rgb(p.divider),
            text: rgb(p.text),
            text2: rgb(p.text2),
            dim: rgb(p.dim),
            accent: rgb(p.accent),
            error: rgb(p.error),
            running: rgb(p.running),
            done: rgb(p.done),
            needs: rgb(p.needs),
            chart: rgb(p.chart),
            wire: rgb(p.wire),
            pass: rgb(p.pass),
            fail: rgb(p.fail),
            term_bg: rgb(p.term_bg),
            term_fg: rgb(p.term_fg),
            cursor: rgb(p.accent),
            selection: rgba(p.accent, 0.28),
            ansi: p.ansi,
            term_bg_rgb: p.term_bg,
            term_fg_rgb: p.term_fg,
            identity: p.identity.iter().map(|c| rgb(*c)).collect(),
            ty,
            art,
        }
    }

    /// The accent at the washes CHROTE mixes: 12% for a chosen thing, 7% for
    /// the current row.
    pub fn accent_light(&self) -> Hsla {
        alpha(self.accent, 0.12)
    }
    pub fn accent_soft(&self) -> Hsla {
        alpha(self.accent, 0.07)
    }

    /// The faint rule between a table's rows.
    pub fn rule(&self) -> Hsla {
        alpha(self.divider, 0.5)
    }

    /// The colour of a Unix user's square, stable per name.
    pub fn identity_of(&self, user: &str) -> Hsla {
        let h = user
            .bytes()
            .fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
        self.identity[h as usize % self.identity.len()]
    }

    /// Text on an accent fill.
    pub fn on_accent(&self) -> Hsla {
        self.bg
    }

    /// A group label's words in the theme's case.
    pub fn label_case(&self, words: &str) -> String {
        if self.ty.labels_upper {
            words.to_uppercase()
        } else {
            words.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two colours are told apart when they differ this much in hue, in
    /// lightness, or in how coloured they are (a grey ink beside a vivid
    /// accent of the same hue).
    fn apart(a: Hsla, b: Hsla) -> bool {
        let dh = (a.h - b.h).abs().min(1. - (a.h - b.h).abs());
        dh > 0.04 || (a.l - b.l).abs() > 0.12 || (a.s - b.s).abs() > 0.5
    }

    #[test]
    fn every_direction_gives_each_meaning_its_own_colour() {
        for d in Direction::ALL {
            let t = Theme::new(d);
            let meanings = [
                ("action", t.accent),
                ("failed", t.error),
                ("running", t.running),
                ("done", t.done),
                ("needs you", t.needs),
                ("chart", t.chart),
                ("pass", t.pass),
                ("fail", t.fail),
            ];
            for (i, (a, ca)) in meanings.iter().enumerate() {
                for (b, cb) in &meanings[i + 1..] {
                    assert!(apart(*ca, *cb), "{}: {a} and {b} look alike", d.label());
                }
                assert!(
                    apart(*ca, t.text) || *a == "chart",
                    "{}: {a} looks like text",
                    d.label()
                );
            }
        }
    }

    #[test]
    fn a_wire_is_told_from_what_acts_and_from_every_state() {
        for d in Direction::ALL {
            let t = Theme::new(d);
            for (meaning, colour) in [
                ("action", t.accent),
                ("failed", t.error),
                ("running", t.running),
                ("done", t.done),
                ("needs you", t.needs),
                ("pass", t.pass),
                ("fail", t.fail),
            ] {
                assert!(
                    apart(t.wire, colour),
                    "{}: wire and {meaning} look alike",
                    d.label()
                );
            }
        }
    }

    #[test]
    fn a_direction_is_named_by_its_key() {
        for d in Direction::ALL {
            assert_eq!(Direction::from_key(d.key()), Some(d));
        }
        assert_eq!(Direction::from_key("violet"), None);
        let named: Vec<_> = Direction::ALL.map(|d| (d.key(), d.label())).into();
        assert_eq!(named, Direction::CHOICES);
    }
}
