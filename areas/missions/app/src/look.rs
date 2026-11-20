//! The board's look, prototyped: each piece has alternatives, chosen by an
//! environment variable when the app starts, so one build shows them all.
//!
//! - `AHJO_BOARD_GATE`: `card` (a verdict card: who decides, the rule, and
//!   each outcome on its own row, the judge folded in), `diamond` (the
//!   flowchart's decision, no box), `badge` (a small checkpoint on the
//!   wire).
//! - `AHJO_BOARD_COLOURS`: `quiet` (neutral cards, colour only for
//!   meaning), `bands` (a band across each card's top by its kind), `wash`
//!   (each kind's card tinted).
//! - `AHJO_BOARD_SHAPES`: how notes that explain the shape are drawn:
//!   `regions` (a tinted area behind a group, its title at its top) or
//!   `brackets` (a rule over the group with its title, nothing behind).
//! - `AHJO_BOARD_COMMENTS`: comments that ask for a change: `pins` (a
//!   bubble on the card's corner) or `tags` (a one-line tag under it).
//! - `AHJO_BOARD_PANEL`: the panel beside the board: `chat` (a
//!   conversation with the mission's agents) or `notepad` (the mission's
//!   page: what it is for, then the open asks as a list).
use ahjo_app_kit::theme::{Theme, alpha};
use gpui::Hsla;
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    Card,
    Diamond,
    Badge,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scheme {
    Quiet,
    Bands,
    Wash,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shapes {
    Regions,
    Brackets,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Comments {
    Pins,
    Tags,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Panel {
    Chat,
    Notepad,
}

fn var(name: &str) -> String {
    std::env::var(name).unwrap_or_default().to_lowercase()
}

#[cfg(test)]
thread_local! {
    static GATE_FOR_TEST: std::cell::Cell<Option<Gate>> = const { std::cell::Cell::new(None) };
}

/// Draws the gate as `look` on this thread until the guard drops, so a
/// test can check the board in each look whatever the environment says.
#[cfg(test)]
pub fn gate_for_test(look: Gate) -> impl Drop {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            GATE_FOR_TEST.with(|g| g.set(None));
        }
    }
    GATE_FOR_TEST.with(|g| g.set(Some(look)));
    Reset
}

pub fn gate() -> Gate {
    #[cfg(test)]
    if let Some(look) = GATE_FOR_TEST.with(|g| g.get()) {
        return look;
    }
    static V: OnceLock<Gate> = OnceLock::new();
    *V.get_or_init(|| match var("AHJO_BOARD_GATE").as_str() {
        "diamond" => Gate::Diamond,
        "badge" => Gate::Badge,
        _ => Gate::Card,
    })
}
pub fn scheme() -> Scheme {
    static V: OnceLock<Scheme> = OnceLock::new();
    *V.get_or_init(|| match var("AHJO_BOARD_COLOURS").as_str() {
        "bands" => Scheme::Bands,
        "wash" => Scheme::Wash,
        _ => Scheme::Quiet,
    })
}
pub fn shapes() -> Shapes {
    static V: OnceLock<Shapes> = OnceLock::new();
    *V.get_or_init(|| match var("AHJO_BOARD_SHAPES").as_str() {
        "brackets" => Shapes::Brackets,
        _ => Shapes::Regions,
    })
}
pub fn comments() -> Comments {
    static V: OnceLock<Comments> = OnceLock::new();
    *V.get_or_init(|| match var("AHJO_BOARD_COMMENTS").as_str() {
        "tags" => Comments::Tags,
        _ => Comments::Pins,
    })
}
pub fn panel() -> Panel {
    static V: OnceLock<Panel> = OnceLock::new();
    *V.get_or_init(|| match var("AHJO_BOARD_PANEL").as_str() {
        "notepad" => Panel::Notepad,
        _ => Panel::Chat,
    })
}

/// The board's colours, one meaning each, made from the theme's so every
/// scheme reads in every theme, light or dark. Pass is the theme's done
/// and fail its failed: a green and a red that mean the same everywhere.
#[derive(Clone, Copy, Debug)]
pub struct Ink {
    /// A card's fill and edge.
    pub card: Hsla,
    pub edge: Hsla,
    /// The band across a card's top, by kind (`bands`); none in the others.
    pub band_input: Option<Hsla>,
    pub band_step: Option<Hsla>,
    pub band_gate: Option<Hsla>,
    /// A gate: its fill, its edge and its mark, drawn heavier than a
    /// step's so it never reads as one.
    pub gate_fill: Hsla,
    pub gate_edge: Hsla,
    pub gate_mark: Hsla,
    /// A gate a person decides: the theme's "needs you".
    pub human: Hsla,
    /// The Input card's fill and edge.
    pub input_fill: Hsla,
    pub input_edge: Hsla,
    pub pass: Hsla,
    pub fail: Hsla,
    /// What carries work, and a judge's or a limit's quiet tether.
    pub wire: Hsla,
    pub tether: Hsla,
    /// How many times a work wire's width an outcome's wire is.
    pub outcome_width: f32,
    /// Live states, as everywhere in the app.
    pub running: Hsla,
    pub done: Hsla,
    pub failed: Hsla,
}

pub fn ink(t: &Theme) -> Ink {
    let base = Ink {
        card: t.surface,
        edge: t.divider,
        band_input: None,
        band_step: None,
        band_gate: None,
        gate_fill: t.raised,
        gate_edge: t.text2,
        gate_mark: t.text,
        human: t.needs,
        input_fill: t.surface,
        input_edge: t.accent,
        pass: t.done,
        fail: t.error,
        wire: t.wire,
        tether: t.dim,
        outcome_width: 1.,
        running: t.running,
        done: t.done,
        failed: t.error,
    };
    match scheme() {
        Scheme::Quiet => base,
        Scheme::Bands => Ink {
            band_input: Some(t.accent),
            band_step: Some(t.dim),
            band_gate: Some(t.text),
            gate_fill: t.surface,
            gate_edge: t.divider,
            ..base
        },
        Scheme::Wash => Ink {
            // Kinds told apart by fill, not by line: steps are quiet
            // paper with almost no edge, the gate a solid darker (or, in
            // light, deeper) block, the input a tinted one.
            card: t.surface,
            edge: alpha(t.divider, 0.35),
            gate_fill: alpha(t.text, if t.light { 0.13 } else { 0.17 }),
            gate_edge: alpha(t.text, 0.8),
            input_fill: alpha(t.accent, if t.light { 0.16 } else { 0.22 }),
            input_edge: alpha(t.accent, 0.9),
            wire: t.text2,
            outcome_width: 2.0,
            ..base
        },
    }
}

impl Ink {
    /// The gate's fill as an opaque colour, over the board's background.
    pub fn gate_fill_solid(&self, t: &Theme) -> Hsla {
        let (a, b) = (gpui::Rgba::from(t.bg), gpui::Rgba::from(self.gate_fill));
        let k = b.a;
        gpui::Hsla::from(gpui::Rgba {
            r: a.r + (b.r - a.r) * k,
            g: a.g + (b.g - a.g) * k,
            b: a.b + (b.b - a.b) * k,
            a: 1.,
        })
    }
}
