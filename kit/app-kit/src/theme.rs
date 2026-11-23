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
//!
//! Three directions are the app's own, Odin's ravens with their own art and
//! type; ten are the colour themes people know from their editors and
//! terminals (`theme/known.rs`), in the app's default faces, the raven drawn in
//! their colours. Two of those are light: nothing here assumes a dark
//! background ([`Theme::light`], [`Theme::shadow`], [`Theme::on_accent`]).

use gpui::{App, FontWeight, Global, Hsla, Image, ImageFormat, Rgba};
use std::sync::{Arc, OnceLock};

mod drawn;
mod known;

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
    /// A popup's gap from the thing it stands beside, and the least room
    /// it keeps from the window's edges.
    pub const POPUP_GAP: f32 = 4.;
    pub const POPUP_EDGE: f32 = 8.;
    /// The side panel at the window's left (sessions, files), and the
    /// edge it leaves when put away.
    pub const SIDE: f32 = 260.;
    pub const SIDE_SHUT: f32 = 36.;
    /// A page's panel (Files, which reads a file in it as well as its
    /// folders): as it first opens, and the least and most it is dragged to.
    pub const PANEL: f32 = 300.;
    pub const PANEL_MIN: f32 = 200.;
    pub const PANEL_MAX: f32 = 960.;
    /// The least a page keeps beside a panel dragged wide.
    pub const PANEL_PAGE_MIN: f32 = 400.;
    /// A grip on an edge dragged to size what it edges
    /// ([`crate::widgets::kit::grip`]): the line it lights, and how wide it
    /// is caught, across the edge.
    pub const GRIP: f32 = 6.;
    pub const GRIP_HIT: f32 = 16.;
    /// The widest a finder row's other words (a folder that found it) are
    /// drawn at its end.
    pub const FINDER_FOUND_BY: f32 = 160.;
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
    /// A detail in two columns (`widgets::pane::detail_beside`): the body
    /// at a reading measure, wider than its least before the aside is put
    /// above it, never wider than its most; the aside's least and most,
    /// and the gap between them.
    pub const MEASURE_MIN: f32 = 400.;
    pub const MEASURE: f32 = 720.;
    pub const ASIDE: f32 = 360.;
    pub const ASIDE_MAX: f32 = 520.;
    pub const ASIDE_GAP: f32 = 32.;
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
    /// A page's panel (Files) sliding open or away.
    pub const PANEL_SLIDE: u64 = 160;
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

/// A theme. The first three are the app's own ideas of Odin's ravens,
/// Huginn and Muninn, thought and memory, who fly out over the world and
/// come back to report; the rest are well-known colour themes, each from
/// its published specification (`theme/known.rs`).
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
    Dracula,
    Nord,
    GruvboxDark,
    SolarizedDark,
    SolarizedLight,
    CatppuccinMocha,
    TokyoNight,
    OneDark,
    Monokai,
    GithubLight,
}

impl Direction {
    /// The app's own first, then the well-known, as the picker lists them.
    pub const ALL: [Direction; 13] = [
        Direction::Night,
        Direction::Saga,
        Direction::Signal,
        Direction::Dracula,
        Direction::Nord,
        Direction::GruvboxDark,
        Direction::SolarizedDark,
        Direction::SolarizedLight,
        Direction::CatppuccinMocha,
        Direction::TokyoNight,
        Direction::OneDark,
        Direction::Monokai,
        Direction::GithubLight,
    ];

    /// Each direction's key and name, as a setting offers them.
    pub const CHOICES: &[(&str, &str)] = &[
        ("night", "Night flight"),
        ("saga", "Saga"),
        ("signal", "Signal"),
        ("dracula", "Dracula"),
        ("nord", "Nord"),
        ("gruvbox-dark", "Gruvbox Dark"),
        ("solarized-dark", "Solarized Dark"),
        ("solarized-light", "Solarized Light"),
        ("catppuccin-mocha", "Catppuccin Mocha"),
        ("tokyo-night", "Tokyo Night"),
        ("one-dark", "One Dark"),
        ("monokai", "Monokai"),
        ("github-light", "GitHub Light"),
    ];

    fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|d| *d == self)
            .expect("every direction is in ALL")
    }

    /// Its value in the `theme` setting.
    pub fn key(self) -> &'static str {
        Self::CHOICES[self.index()].0
    }

    pub fn label(self) -> &'static str {
        Self::CHOICES[self.index()].1
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|d| d.key() == key)
    }

    /// One of the app's own ravens, with art and type of its own.
    pub fn own(self) -> bool {
        matches!(self, Direction::Night | Direction::Saga | Direction::Signal)
    }

    /// The direction `steps` on in [`Direction::ALL`], round at the ends.
    pub fn step(self, steps: isize) -> Self {
        let n = Self::ALL.len() as isize;
        Self::ALL[(self.index() as isize + steps).rem_euclid(n) as usize]
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
/// place, and the app's mark. The ravens' own are painted; a well-known
/// theme's are drawn in its colours ([`drawn`]).
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
    /// Dark ink on a light ground. What lifts a thing off the ground
    /// (a shadow, a glow, text on the accent) asks this, never assumes.
    pub light: bool,
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
    /// Code's colours, on [`Theme::bg`].
    pub syntax: Syntax,
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

/// `a` moved `share` of the way to `b`, as 0xRRGGBB.
pub fn mix(a: u32, b: u32, share: f32) -> u32 {
    let ch = |c: u32, s: u32| ((c >> s) & 0xff) as f32;
    [16, 8, 0].iter().fold(0, |out, &s| {
        let v = ch(a, s) + (ch(b, s) - ch(a, s)) * share;
        out | ((v.round().clamp(0., 255.) as u32) << s)
    })
}

/// How bright a colour is to the eye, from 0 (black) to 1 (white), as WCAG
/// measures it; its alpha is not counted.
pub fn luminance(c: Hsla) -> f32 {
    let c = Rgba::from(c);
    let lin = |v: f32| {
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b)
}

/// The WCAG contrast ratio of two opaque colours, from 1 to 21. Reading
/// text wants 4.5 against what it stands on; a large label or a mark, 3.
pub fn contrast(a: Hsla, b: Hsla) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Code's colours: the four kinds `widgets::syntax` tells apart (a link in
/// prose, its fifth, is drawn in the accent).
#[derive(Clone, Copy, Debug)]
pub struct Syntax {
    pub comment: Hsla,
    pub string: Hsla,
    pub number: Hsla,
    pub keyword: Hsla,
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
    /// The terminal's cursor, and its selection with how much shows.
    cursor: u32,
    selection: (u32, f32),
    ansi: [u32; 16],
    /// Comment, string, number, keyword.
    syntax: [u32; 4],
    identity: [u32; 4],
}

const NIGHT: Palette = Palette {
    bg: 0x0a0f18,
    surface: 0x0f1622,
    raised: 0x172133,
    divider: 0x243149,
    text: 0xe4ebf5,
    text2: 0xa3b1c6,
    dim: 0x7b889f,
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
    cursor: 0x6cb6ff,
    selection: (0x6cb6ff, 0.28),
    ansi: [
        0x0a0f18, 0xff6b7a, 0x5fd3a0, 0xffbf5a, 0x6cb6ff, 0xc49bff, 0x4fd6d6, 0xa3b1c6, 0x55627a,
        0xff8c97, 0x86e3b8, 0xffd28a, 0x9acdff, 0xd8bfff, 0x7fe6e6, 0xf2f6fb,
    ],
    syntax: [0x7b889f, 0x5fd3a0, 0xffbf5a, 0xc49bff],
    identity: [0x3f5f8a, 0x2f6b6b, 0x5b4f8a, 0x6a5a3a],
};

const SAGA: Palette = Palette {
    bg: 0x120e0a,
    surface: 0x1a140f,
    raised: 0x261d15,
    divider: 0x3b2d20,
    text: 0xefe3cc,
    text2: 0xc2ad8c,
    dim: 0x998363,
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
    cursor: 0xe0a94a,
    selection: (0xe0a94a, 0.28),
    ansi: [
        0x120e0a, 0xe5574b, 0x9cbf6a, 0xe0a94a, 0x6fa8dc, 0xc7849e, 0x4fc3b5, 0xc2ad8c, 0x6e5d46,
        0xf07a6e, 0xb6d68a, 0xf0c46e, 0x94c0ea, 0xdba3b9, 0x7fd8cd, 0xf5ecd9,
    ],
    syntax: [0x998363, 0x9cbf6a, 0xe0a94a, 0xc7849e],
    identity: [0x7a5230, 0x4f6a3a, 0x7a3f36, 0x3f5a6a],
};

const SIGNAL: Palette = Palette {
    bg: 0x0b0b0b,
    surface: 0x121212,
    raised: 0x1d1d1d,
    divider: 0x2e2e2e,
    text: 0xf0ece3,
    text2: 0xb8b3a8,
    dim: 0x88847b,
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
    cursor: 0xffd000,
    selection: (0xffd000, 0.28),
    ansi: [
        0x0b0b0b, 0xff5a36, 0x9ee34a, 0xffd000, 0x3a8dff, 0xff4fa0, 0x3ad0ff, 0xcfcac0, 0x5a5751,
        0xff7f61, 0xbdf27a, 0xffe14d, 0x6eadff, 0xff80bd, 0x7fe2ff, 0xffffff,
    ],
    syntax: [0x88847b, 0x9ee34a, 0xffd000, 0xff4fa0],
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

/// The theme chosen in Ahjo Swarm's settings, for an app of the kit that runs
/// on its own (Brain, Desk) and draws as the operator chose. No settings,
/// or settings that do not read, give the default; so does a theme no
/// direction is called, with a notice that says so. The file is only read.
pub fn chosen() -> (Theme, Option<String>) {
    let Some(settings) = crate::settings::swarm() else {
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

/// The app's size chosen in Ahjo Swarm's settings (`zoom::app`), as
/// [`chosen`] reads the theme, for a window of the kit that runs on its own:
/// what its window's measures are multiplied by.
pub fn chosen_scale() -> f32 {
    let size = crate::settings::swarm().map_or(crate::zoom::app::DEFAULT, |s| s.size);
    crate::zoom::app::scale(size)
}

impl Default for Theme {
    fn default() -> Self {
        Self::new(Direction::default())
    }
}

/// Night flight's type, which the well-known themes share.
fn default_type() -> Type {
    Type {
        display: "Space Grotesk",
        display_weight: FontWeight::BOLD,
        reading: "Inter",
        group: 11.,
        read: 15.,
        labels_upper: true,
        tracking: 1.5,
    }
}

impl Theme {
    pub fn new(direction: Direction) -> Self {
        let (p, ty, art) = match direction {
            Direction::Night => (
                &NIGHT,
                default_type(),
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
            known => {
                let p = known::palette(known);
                (p, default_type(), drawn::art(p))
            }
        };
        let bg = rgb(p.bg);
        let [comment, string, number, keyword] = p.syntax.map(rgb);
        Self {
            direction,
            light: luminance(bg) > 0.4,
            bg,
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
            cursor: rgb(p.cursor),
            selection: rgba(p.selection.0, p.selection.1),
            ansi: p.ansi,
            term_bg_rgb: p.term_bg,
            term_fg_rgb: p.term_fg,
            syntax: Syntax {
                comment,
                string,
                number,
                keyword,
            },
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

    /// Text on an accent fill: the ground or the ink, whichever reads
    /// better on it (a light accent on a dark ground takes the ground; a
    /// mid blue on white takes white).
    pub fn on_accent(&self) -> Hsla {
        if contrast(self.bg, self.accent) >= contrast(self.text, self.accent) {
            self.bg
        } else {
            self.text
        }
    }

    /// What a raised thing's shadow falls in: the deep ground of a dark
    /// theme, a soft grey on a light one, where a shadow of the ground
    /// would be a glow.
    pub fn shadow(&self) -> Hsla {
        if self.light {
            Hsla {
                h: 0.,
                s: 0.,
                l: 0.,
                a: 0.16,
            }
        } else {
            alpha(self.bg, 0.6)
        }
    }

    /// The terminal's faint text (SGR 2): its text two thirds of the way
    /// from its ground, so it stays readable on a light ground too.
    pub fn term_dim_rgb(&self) -> u32 {
        mix(self.term_bg_rgb, self.term_fg_rgb, 0.66)
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

    /// WCAG AA: reading text 4.5:1 on every ground it stands on (the
    /// page, a panel, a chosen row); a mark 3:1. The accent and the states
    /// are read as words too ("failed", a waiting question), so they count
    /// as text.
    #[test]
    fn every_direction_reads_at_wcag_aa() {
        let mut low = vec![];
        for d in Direction::ALL {
            let t = Theme::new(d);
            let mut need = |what: &str, ink: Hsla, ground: (&str, Hsla), least: f32| {
                let c = contrast(ink, ground.1);
                if c < least {
                    low.push(format!("{}: {what} on {} is {c:.2}", d.label(), ground.0));
                }
            };
            let grounds = [("bg", t.bg), ("surface", t.surface), ("raised", t.raised)];
            let words = [
                ("text", t.text),
                ("text2", t.text2),
                ("dim", t.dim),
                ("accent", t.accent),
                ("error", t.error),
                ("running", t.running),
                ("done", t.done),
                ("needs", t.needs),
            ];
            for (what, ink) in words {
                for g in grounds {
                    need(what, ink, g, 4.5);
                }
            }
            let code = [
                ("comment", t.syntax.comment),
                ("string", t.syntax.string),
                ("number", t.syntax.number),
                ("keyword", t.syntax.keyword),
            ];
            let marks = [
                ("chart", t.chart),
                ("wire", t.wire),
                ("pass", t.pass),
                ("fail", t.fail),
            ];
            for g in &grounds[..2] {
                for (what, ink) in code {
                    need(what, ink, *g, 4.5);
                }
                for (what, ink) in marks {
                    need(what, ink, *g, 3.);
                }
            }
            need("terminal text", t.term_fg, ("its ground", t.term_bg), 4.5);
            need(
                "text on the accent",
                t.on_accent(),
                ("accent", t.accent),
                4.5,
            );
            for (i, square) in t.identity.iter().enumerate() {
                need("text", t.text, (&format!("identity {i}"), *square), 4.5);
            }
            // A divider is seen, if only just, on the page and on a panel.
            for g in &grounds[..2] {
                need("divider", t.divider, *g, 1.3);
            }
        }
        assert!(low.is_empty(), "under WCAG AA:\n{}", low.join("\n"));
    }

    #[test]
    fn every_direction_fills_every_token() {
        for d in Direction::ALL {
            let t = Theme::new(d);
            let colours = [
                t.bg, t.surface, t.raised, t.divider, t.text, t.text2, t.dim, t.accent, t.error,
                t.running, t.done, t.needs, t.chart, t.wire, t.pass, t.fail, t.term_bg, t.term_fg,
                t.cursor,
            ];
            assert!(colours.iter().all(|c| c.a == 1.), "{}", d.label());
            assert!(t.selection.a > 0., "{}", d.label());
            assert_eq!(t.identity.len(), 4, "{}", d.label());
            assert_eq!(rgb(t.term_bg_rgb), t.term_bg, "{}", d.label());
            // The ground and the ink stand apart, whichever way round.
            assert_eq!(
                t.light,
                luminance(t.bg) > luminance(t.text),
                "{}",
                d.label()
            );
            assert_eq!(
                d.own(),
                d.key() == "night" || d.key() == "saga" || d.key() == "signal"
            );
        }
        let light: Vec<_> = Direction::ALL
            .into_iter()
            .filter(|d| Theme::new(*d).light)
            .map(Direction::key)
            .collect();
        assert_eq!(light, ["solarized-light", "github-light"]);
    }

    #[test]
    fn a_theme_drawn_without_art_draws_the_raven_in_its_colours() {
        let p = known::palette(Direction::GithubLight);
        let mark = drawn::mark_svg(p);
        // A black raven on the blue disc: on a light theme the raven is
        // the ink, not the white ground.
        assert!(mark.contains(r##"fill="#0968d8""##), "{mark}");
        assert!(mark.contains(r##"fill="#1f2328""##), "{mark}");
        let p = known::palette(Direction::Dracula);
        assert!(drawn::mark_svg(p).contains(r##"fill="#282a36""##));
    }

    #[test]
    fn the_picker_steps_round_every_direction() {
        assert_eq!(Direction::Night.step(-1), Direction::GithubLight);
        assert_eq!(Direction::GithubLight.step(1), Direction::Night);
        assert_eq!(Direction::Signal.step(1), Direction::Dracula);
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
