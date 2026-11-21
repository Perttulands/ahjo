//! Gates drawn as gates: each look is a small scene in the theme's own
//! tones, with the question on a sign that belongs to it (a board hung from
//! a garden arch, a plate on a crossing's post, a stone over a castle gate,
//! the carved lintel of a stave gate, a toll's board), the line running
//! through it, and what it does in a run: shut while it waits, open once
//! passed, barred when it fails. Only the meanings colour it: the way on,
//! the way back, what waits on the operator, what is being judged.
//!
//! The art is SVG made for the card's size, as the raven is drawn
//! (`theme/drawn.rs`); the words over it are the app's own text.
//!
//! A prototype for AH13: `AHJO_BOARD_GATE` picks the look.

use std::fmt::Write;

/// The tones a gate is drawn in, from the theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Ink {
    pub bg: u32,
    pub text: u32,
    pub wire: u32,
    pub quiet: u32,
    pub pass: u32,
    pub fail: u32,
    pub needs: u32,
    pub running: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Look {
    Garden,
    Crossing,
    Portcullis,
    Stave,
    Toll,
}

impl Look {
    pub const ALL: [Look; 5] = [
        Look::Garden,
        Look::Crossing,
        Look::Portcullis,
        Look::Stave,
        Look::Toll,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Look::Garden => "garden",
            Look::Crossing => "crossing",
            Look::Portcullis => "portcullis",
            Look::Stave => "stave",
            Look::Toll => "toll",
        }
    }
    pub fn parse(s: &str) -> Option<Look> {
        Look::ALL.into_iter().find(|l| l.name() == s)
    }
}

/// Where a gate is in a run, or `Plan` on the board as authored.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Moment {
    Plan,
    /// Reached and being decided: lit in the operator's colour when it
    /// waits on him, in the running colour while a judge or a check works.
    Deciding,
    Passed,
    Failed,
}

/// What the gate's lines lead to and who decides it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Wiring {
    pub pass: bool,
    pub fail: bool,
    pub human: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// A gate's card: its size, the sign its words go on, where the line runs
/// through it and where its fail drops out at its foot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub w: f32,
    pub h: f32,
    pub sign: Rect,
    /// The words' room on the sign.
    pub words: Rect,
    pub art_y: f32,
    pub line_y: f32,
    pub fail_x: f32,
}

pub const W: f32 = 208.;
/// The words on a sign: the title's line, each line of the condition, and
/// the row naming who decides.
pub const TITLE_H: f32 = 19.;
pub const LINE_H: f32 = 17.;
pub const WHO_H: f32 = 22.;
const PAD_X: f32 = 14.;
const PAD_Y: f32 = 9.;

/// The words' height for a condition of `lines` lines.
fn words_h(lines: usize) -> f32 {
    TITLE_H + lines.max(1) as f32 * LINE_H + 4. + WHO_H
}

/// The art under the sign: how tall it is and where the line runs in it.
fn art(look: Look) -> (f32, f32) {
    match look {
        Look::Garden => (100., 74.),
        Look::Crossing => (104., 94.),
        Look::Portcullis => (100., 90.),
        Look::Stave => (96., 78.),
        Look::Toll => (92., 74.),
    }
}

/// How a look holds its sign: the sign's place across the card, the room
/// its frame takes round the words (top, sides), what stands above it and
/// what hangs between it and the gate.
struct Holds {
    x: f32,
    w: f32,
    top: f32,
    frame_top: f32,
    frame_side: f32,
    gap: f32,
}

fn holds(look: Look) -> Holds {
    match look {
        Look::Garden => Holds {
            x: 0.,
            w: W,
            top: 0.,
            frame_top: 0.,
            frame_side: 0.,
            gap: 12.,
        },
        Look::Crossing => Holds {
            x: 0.,
            w: W,
            top: 0.,
            frame_top: 0.,
            frame_side: 0.,
            gap: 0.,
        },
        // The castle's stone is set in its wall, under the battlements.
        Look::Portcullis => Holds {
            x: 10.,
            w: W - 20.,
            top: 0.,
            frame_top: 18.,
            frame_side: 8.,
            gap: 0.,
        },
        // The stave's lintel is under its roof, between its posts.
        Look::Stave => Holds {
            x: 20.,
            w: W - 40.,
            top: 44.,
            frame_top: 0.,
            frame_side: 0.,
            gap: 0.,
        },
        // The toll's board leaves room for its boom to stand up beside it.
        Look::Toll => Holds {
            x: 0.,
            w: W - 22.,
            top: 0.,
            frame_top: 0.,
            frame_side: 0.,
            gap: 8.,
        },
    }
}

/// How wide a condition's lines may run on this look's sign.
pub fn words_width(look: Look) -> f32 {
    let h = holds(look);
    h.w - 2. * (PAD_X + h.frame_side)
}

/// The width a condition is measured at to count its lines: a little
/// under the sign's, so the guess never comes up a line short of how the
/// app's type wraps (a sign a line too tall reads; a cut line does not).
pub fn measure_width(look: Look) -> f32 {
    words_width(look) * 0.9
}

pub fn frame(look: Look, lines: usize) -> Frame {
    let words = words_h(lines);
    let (art_h, line) = art(look);
    let h = holds(look);
    let sign = Rect {
        x: h.x,
        y: h.top,
        w: h.w,
        h: words + 2. * PAD_Y + h.frame_top,
    };
    let art_y = sign.y + sign.h + h.gap;
    Frame {
        w: W,
        h: art_y + art_h,
        sign,
        words: Rect {
            x: sign.x + h.frame_side + PAD_X,
            y: sign.y + h.frame_top + PAD_Y,
            w: words_width(look),
            h: words,
        },
        art_y,
        line_y: art_y + line,
        fail_x: 14.,
    }
}

fn hex(c: u32) -> String {
    format!("#{c:06x}")
}

fn mix(a: u32, b: u32, share: f32) -> u32 {
    let ch = |c: u32, s: u32| ((c >> s) & 0xff) as f32;
    [16, 8, 0].iter().fold(0, |out, &s| {
        let v = ch(a, s) + (ch(b, s) - ch(a, s)) * share;
        out | ((v.round().clamp(0., 255.) as u32) << s)
    })
}

fn lum(c: u32) -> f32 {
    let ch = |s: u32| ((c >> s) & 0xff) as f32 / 255.;
    0.2126 * ch(16) + 0.7152 * ch(8) + 0.0722 * ch(0)
}

/// The theme's tones from its ground (0) to its ink (1).
struct Tones {
    ink: Ink,
    dark: bool,
}

impl Tones {
    fn t(&self, k: f32) -> String {
        hex(mix(self.ink.bg, self.ink.text, k))
    }
    /// A raven is black on any theme.
    fn raven(&self) -> String {
        hex(if self.dark {
            mix(self.ink.bg, 0, 0.6)
        } else {
            self.ink.text
        })
    }
}

/// A raven perched, facing right, in a 64 by 64 box (`theme/drawn.rs`).
const PERCHED: &str = "M57 22.5 C53 19 49 17 44.5 16.5 C42 12.5 36 11.5 32 14.5 C26.5 18.5 23 25 20.5 31.5 L6 53 L15.5 50.5 L24.5 46.5 C31 47 38 45.5 42 39.5 L43 36 L41.5 33 L44.5 31.5 L43.5 28.5 L46.5 26 C50 24.5 53.5 23.8 57 22.5 Z M30 45 h2 v9 h-2z M35 44 h2 v10 h-2z";
/// A raven flying to the right, wings up, about its middle: the far wing
/// raised, the near one down, a wedge of a tail.
const FLYING: &str = "M19 -1 L16 -2.6 C13 -4.4 9 -4.2 5 -3 C3 -9 -1 -16 -8 -22 L-7.4 -18.2 L-11.2 -19.4 L-9 -15.6 L-13 -15.8 L-9.6 -12.4 L-13.2 -11.6 C-9 -8 -6 -5 -4 -2.6 L-10 -2.8 L-17 -6 L-15 -0.6 L-17 4 L-10 2 L-4 2.4 C-6 6 -8 10 -12 13 L-8.4 13.2 L-10 16.6 L-6 14.4 L-6.6 18 C-2 13 2 7 5 3 C9 4 13 3.6 16 1.2 Z";

/// A perched raven with its feet at (x, y), `size` tall, facing right or
/// (`flip`) left, its eye in `eye`.
fn raven(s: &mut String, tones: &Tones, x: f32, y: f32, size: f32, flip: bool, eye: &str) {
    let k = size / 40.;
    let sx = if flip { -k } else { k };
    // The feet stand at (33, 54) in the box.
    let rim = if tones.dark {
        format!(
            r#" stroke="{}" stroke-width="{:.2}""#,
            tones.t(0.42),
            1.1 / k
        )
    } else {
        String::new()
    };
    let _ = write!(
        s,
        r#"<g transform="translate({x} {y}) scale({sx} {k}) translate(-33 -54)"><path fill="{}"{rim} d="{PERCHED}"/><circle cx="40" cy="18.8" r="1.7" fill="{eye}"/></g>"#,
        tones.raven()
    );
}

fn flying(s: &mut String, tones: &Tones, x: f32, y: f32, size: f32, turn: f32) {
    let k = size / 44.;
    let rim = if tones.dark {
        format!(
            r#" stroke="{}" stroke-width="{:.2}""#,
            tones.t(0.42),
            1.1 / k
        )
    } else {
        String::new()
    };
    let _ = write!(
        s,
        r#"<g transform="translate({x} {y}) rotate({turn}) scale({k})"><path fill="{}"{rim} d="{FLYING}"/></g>"#,
        tones.raven()
    );
}

/// The line through the gate: the way in, through when it opens, on along
/// the pass and down the fail.
fn lines(
    s: &mut String,
    tones: &Tones,
    f: &Frame,
    wiring: Wiring,
    moment: Moment,
    gate: (f32, f32),
) {
    let ink = tones.ink;
    let y = f.line_y;
    let (into, out_of) = gate;
    let lit = |c: u32| hex(c);
    let way_in = match moment {
        Moment::Passed => lit(ink.pass),
        Moment::Failed => lit(ink.fail),
        _ => lit(ink.wire),
    };
    let pass = match (moment, wiring.pass) {
        (Moment::Failed, _) | (_, false) => lit(ink.quiet),
        _ => lit(ink.pass),
    };
    let w = |m: Moment| -> f32 { if moment == m { 2.6 } else { 1.5 } };
    let _ = write!(
        s,
        r#"<path d="M0 {y} H{into}" stroke="{way_in}" stroke-width="{}" fill="none"/>"#,
        w(Moment::Passed).max(w(Moment::Failed))
    );
    if moment == Moment::Passed {
        let _ = write!(
            s,
            r#"<path d="M{into} {y} H{out_of}" stroke="{pass}" stroke-width="2.6" fill="none"/>"#
        );
    }
    let _ = write!(
        s,
        r#"<path d="M{out_of} {y} H{}" stroke="{pass}" stroke-width="{}" fill="none"/>"#,
        f.w,
        w(Moment::Passed)
    );
    if wiring.fail {
        let fail = if moment == Moment::Passed {
            lit(ink.quiet)
        } else {
            lit(ink.fail)
        };
        let x = f.fail_x;
        let r = 8.;
        let _ = write!(
            s,
            r#"<path d="M{} {y} Q{x} {y} {x} {} V{}" stroke="{fail}" stroke-width="{}" fill="none"/>"#,
            x - r,
            y + r,
            f.h,
            w(Moment::Failed)
        );
    }
}

/// What lights a gate while it is decided: the operator's colour when it
/// waits on him, the running colour while a judge or a check works.
fn deciding(tones: &Tones, wiring: Wiring, moment: Moment) -> Option<String> {
    (moment == Moment::Deciding).then(|| {
        hex(if wiring.human {
            tones.ink.needs
        } else {
            tones.ink.running
        })
    })
}

fn glow(s: &mut String, x: f32, y: f32, r: f32, colour: &str) {
    let _ = write!(
        s,
        r#"<circle cx="{x}" cy="{y}" r="{}" fill="{colour}" opacity="0.18"/><circle cx="{x}" cy="{y}" r="{}" fill="{colour}" opacity="0.28"/>"#,
        r * 2.2,
        r * 1.4
    );
}

/// The SVG for a gate card of frame `f`, drawn `scale` times its size.
pub fn svg(look: Look, ink: Ink, f: &Frame, wiring: Wiring, moment: Moment, scale: f32) -> String {
    let tones = Tones {
        ink,
        dark: lum(ink.bg) < lum(ink.text),
    };
    let mut s = String::new();
    let _ = write!(
        s,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="0 0 {} {}" stroke-linejoin="round" stroke-linecap="round">"#,
        (f.w * scale).round(),
        (f.h * scale).round(),
        f.w,
        f.h
    );
    match look {
        Look::Garden => garden(&mut s, &tones, f, wiring, moment),
        Look::Crossing => crossing(&mut s, &tones, f, wiring, moment),
        Look::Portcullis => portcullis(&mut s, &tones, f, wiring, moment),
        Look::Stave => stave(&mut s, &tones, f, wiring, moment),
        Look::Toll => toll(&mut s, &tones, f, wiring, moment),
    }
    s.push_str("</svg>");
    s
}

/// A garden gate: two posts with round tops under an arch of leaves, two
/// picket leaves that swing open, and a board hung from the arch on chains.
fn garden(s: &mut String, tones: &Tones, f: &Frame, wiring: Wiring, moment: Moment) {
    let (wood, dark, light, edge) = (tones.t(0.40), tones.t(0.24), tones.t(0.58), tones.t(0.10));
    let leaf = tones.t(0.30);
    let a = f.art_y;
    let (l, r) = (44., 164.);
    let post = 10.;
    let ground = f.h;
    let mid = (l + r) / 2.;
    // The board, its grain and its chains.
    let b = f.sign;
    let _ = write!(
        s,
        r#"<rect x="{}" y="{}" width="{}" height="{}" rx="5" fill="{}" stroke="{wood}" stroke-width="1.5"/>"#,
        b.x + 1.,
        b.y + 1.,
        b.w - 2.,
        b.h - 2.,
        tones.t(0.09)
    );
    for i in 1..3 {
        let y = b.y + b.h * i as f32 / 3.;
        let _ = write!(
            s,
            r#"<path d="M{} {y} h{}" stroke="{}" stroke-width="0.8" opacity="0.5"/>"#,
            b.x + 8.,
            b.w - 16.,
            tones.t(0.14)
        );
    }
    let arch_top = a + 4.;
    for x in [mid - 34., mid + 34.] {
        let y0 = b.y + b.h - 1.;
        let mut y = y0;
        // Links, alternately face-on and edge-on.
        let mut i = 0;
        let end = arch_top + 6.;
        while y < end {
            if i % 2 == 0 {
                let _ = write!(
                    s,
                    r#"<ellipse cx="{x}" cy="{}" rx="2" ry="3" fill="none" stroke="{dark}" stroke-width="1.2"/>"#,
                    y + 3.
                );
            } else {
                let _ = write!(
                    s,
                    r#"<path d="M{x} {} v5" stroke="{dark}" stroke-width="1.4"/>"#,
                    y + 0.5
                );
            }
            y += 4.;
            i += 1;
        }
    }
    let gate = (l + post, r);
    lines(s, tones, f, wiring, moment, gate);
    // The arch: a beam bent over the posts, leaves along it.
    let _ = write!(
        s,
        r#"<path d="M{} {} C{} {}, {} {}, {} {}" stroke="{wood}" stroke-width="5" fill="none"/>"#,
        l + post / 2.,
        a + 24.,
        l + 4.,
        arch_top - 14.,
        r + post - 4.,
        arch_top - 14.,
        r + post / 2.,
        a + 24.
    );
    for i in 0..13 {
        let t = i as f32 / 12.;
        // On the arch's curve, by its own Bezier.
        let (x0, y0, x1, y1, x2, y2, x3, y3) = (
            l + post / 2.,
            a + 24.,
            l + 4.,
            arch_top - 14.,
            r + post - 4.,
            arch_top - 14.,
            r + post / 2.,
            a + 24.,
        );
        let u = 1. - t;
        let x = u * u * u * x0 + 3. * u * u * t * x1 + 3. * u * t * t * x2 + t * t * t * x3;
        let y = u * u * u * y0 + 3. * u * u * t * y1 + 3. * u * t * t * y2 + t * t * t * y3;
        let turn = if i % 2 == 0 { -38. } else { 38. } + (t - 0.5) * 70.;
        let up = if i % 2 == 0 { -3.5 } else { 3. };
        let _ = write!(
            s,
            r#"<ellipse cx="0" cy="0" rx="5" ry="2.4" fill="{leaf}" transform="translate({x} {}) rotate({turn})"/>"#,
            y + up
        );
    }
    // The posts and their round tops.
    for x in [l, r] {
        let _ = write!(
            s,
            r#"<rect x="{x}" y="{}" width="{post}" height="{}" fill="{wood}" stroke="{edge}" stroke-width="1"/><path d="M{} {} V{}" stroke="{light}" stroke-width="1.2" opacity="0.6"/><circle cx="{}" cy="{}" r="5.5" fill="{wood}" stroke="{edge}" stroke-width="1"/>"#,
            a + 22.,
            ground - a - 22.,
            x + 2.5,
            a + 26.,
            ground - 3.,
            x + post / 2.,
            a + 18.
        );
    }
    // A lantern from the arch's crown, lit while the gate is decided.
    let lamp = deciding(tones, wiring, moment);
    let (lx, ly) = (mid, arch_top + 1.);
    let _ = write!(
        s,
        r#"<path d="M{lx} {ly} v6" stroke="{dark}" stroke-width="1.2"/>"#
    );
    if let Some(c) = &lamp {
        glow(s, lx, ly + 12., 5., c);
    }
    let _ = write!(
        s,
        r#"<path d="M{} {} h8 l-1.5 10 h-5 z" fill="{}" stroke="{dark}" stroke-width="1.2"/><path d="M{} {} h10" stroke="{dark}" stroke-width="1.6"/>"#,
        lx - 4.,
        ly + 7.,
        lamp.clone().unwrap_or_else(|| tones.t(0.16)),
        lx - 5.,
        ly + 7.
    );
    // The leaves: pickets with pointed tops, low in the middle, on two
    // rails with a brace. Open, each is swung back against its post.
    let leaf_w = (r - l - post) / 2.;
    let open = moment == Moment::Passed;
    for side in [0, 1] {
        let hinge = if side == 0 { l + post } else { r };
        let dir = if side == 0 { 1. } else { -1. };
        let reach = if open { leaf_w * 0.3 } else { leaf_w };
        // Open toward the viewer: the far edge comes down and grows.
        let drop = if open { 6. } else { 0. };
        let n = 6;
        for i in 0..n {
            let t = (i as f32 + 0.5) / n as f32;
            let x = hinge + dir * reach * t;
            let pw = if open { 3.2 } else { 6. };
            let top = a + 34. + 12. * t * t + drop * t;
            let bottom = ground - 4. + drop * t;
            let _ = write!(
                s,
                r#"<path d="M{} {bottom} V{} L{x} {} L{} {} V{bottom} Z" fill="{wood}" stroke="{edge}" stroke-width="0.9"/>"#,
                x - pw / 2.,
                top + 4.,
                top - 1.,
                x + pw / 2.,
                top + 4.
            );
        }
        for (y0, k) in [(a + 52., 0.), (ground - 14., 1.)] {
            let _ = write!(
                s,
                r#"<path d="M{hinge} {y0} L{} {}" stroke="{dark}" stroke-width="4.2"/>"#,
                hinge + dir * reach,
                y0 + drop * (0.6 + 0.4 * k)
            );
        }
        let _ = write!(
            s,
            r#"<path d="M{hinge} {} L{} {}" stroke="{dark}" stroke-width="3"/>"#,
            ground - 14.,
            hinge + dir * reach * 0.92,
            a + 53. + drop * 0.6
        );
    }
    if moment == Moment::Failed {
        chain_lock(s, tones, mid, a + 64.);
    }
}

/// A chain across a shut gate and its padlock, in the fail colour.
fn chain_lock(s: &mut String, tones: &Tones, x: f32, y: f32) {
    let c = hex(tones.ink.fail);
    for i in -5..=5 {
        let cx = x + i as f32 * 5.;
        if i % 2 == 0 {
            let _ = write!(
                s,
                r#"<ellipse cx="{cx}" cy="{y}" rx="3.2" ry="2" fill="none" stroke="{c}" stroke-width="1.5"/>"#
            );
        } else {
            let _ = write!(
                s,
                r#"<path d="M{} {y} h5" stroke="{c}" stroke-width="1.7"/>"#,
                cx - 2.5
            );
        }
    }
    let _ = write!(
        s,
        r#"<path d="M{} {} v-5 a4 4 0 0 1 8 0 v5" fill="none" stroke="{c}" stroke-width="2"/><rect x="{}" y="{}" width="12" height="10" rx="2" fill="{c}"/>"#,
        x - 4.,
        y + 4.,
        x - 6.,
        y + 3.
    );
}

/// A level crossing: a gantry of two posts carrying the plate the question
/// is on, a pair of lamps on top that wink while it is decided, and two
/// striped half-booms that meet over the line and lift to let it through.
fn crossing(s: &mut String, tones: &Tones, f: &Frame, wiring: Wiring, moment: Moment) {
    let (metal, dark, light, edge) = (tones.t(0.46), tones.t(0.22), tones.t(0.88), tones.t(0.08));
    let b = f.sign;
    let pw = 7.;
    let posts = [10., f.w - 10. - pw];
    for x in posts {
        let _ = write!(
            s,
            r#"<rect x="{x}" y="{}" width="{pw}" height="{}" fill="{metal}" stroke="{edge}" stroke-width="1"/><rect x="{}" y="{}" width="{}" height="5" rx="1" fill="{dark}"/>"#,
            b.y + 10.,
            f.h - b.y - 10.,
            x - 3.,
            f.h - 5.,
            pw + 6.
        );
    }
    // The plate: enamel with a rim and a ring inside it.
    let _ = write!(
        s,
        r#"<rect x="{}" y="{}" width="{}" height="{}" rx="7" fill="{}" stroke="{metal}" stroke-width="1.6"/><rect x="{}" y="{}" width="{}" height="{}" rx="5" fill="none" stroke="{}" stroke-width="0.8"/>"#,
        b.x + 1.,
        b.y + 1.,
        b.w - 2.,
        b.h - 2.,
        tones.t(0.09),
        b.x + 4.,
        b.y + 4.,
        b.w - 8.,
        b.h - 8.,
        tones.t(0.26)
    );
    // Between the posts hangs the crossbuck, two boards crossed, and
    // under it the pair of lamps that wink while it is decided.
    let mid = f.w / 2.;
    let a = f.art_y;
    let (cy, arm) = (a + 26., 24.);
    let _ = write!(
        s,
        r#"<path d="M{mid} {} V{}" stroke="{dark}" stroke-width="3"/>"#,
        b.y + b.h - 1.,
        a + 58.
    );
    for turn in [35., -35.] {
        let _ = write!(
            s,
            r#"<g transform="translate({mid} {cy}) rotate({turn})"><rect x="{}" y="-5" width="{}" height="10" rx="1.5" fill="{light}" stroke="{edge}" stroke-width="1"/><rect x="{}" y="-2.6" width="{}" height="5.2" fill="none" stroke="{}" stroke-width="1"/></g>"#,
            -arm,
            2. * arm,
            -arm + 3.,
            2. * arm - 6.,
            tones.t(0.30)
        );
    }
    let ly = a + 52.;
    let lamps = match moment {
        Moment::Failed => [Some(hex(tones.ink.fail)), Some(hex(tones.ink.fail))],
        Moment::Deciding => [deciding(tones, wiring, moment), None],
        _ => [None, None],
    };
    let _ = write!(
        s,
        r#"<rect x="{}" y="{}" width="44" height="5" rx="2" fill="{dark}"/>"#,
        mid - 22.,
        ly - 2.5
    );
    for (i, lit) in lamps.iter().enumerate() {
        let x = mid + if i == 0 { -16. } else { 16. };
        if let Some(c) = lit {
            glow(s, x, ly, 6., c);
        }
        let _ = write!(
            s,
            r#"<circle cx="{x}" cy="{ly}" r="9" fill="{edge}"/><circle cx="{x}" cy="{ly}" r="5.5" fill="{}"/><path d="M{} {} a9 9 0 0 1 18 0" fill="none" stroke="{dark}" stroke-width="2.4"/>"#,
            lit.clone().unwrap_or_else(|| tones.t(0.2)),
            x - 9.,
            ly - 1.
        );
    }
    lines(s, tones, f, wiring, moment, (posts[0], posts[1] + pw));
    // The half-booms, striped; red where they hold a failed run back.
    let y = f.line_y - 1.;
    let pivots = [posts[0] + pw / 2., posts[1] + pw / 2.];
    let len = (pivots[1] - pivots[0]) / 2. - 2.;
    let stripe = if moment == Moment::Failed {
        hex(tones.ink.fail)
    } else {
        tones.t(0.30)
    };
    let mut boom = format!(
        r#"<rect x="0" y="-3.5" width="{len}" height="7" rx="3" fill="{light}" stroke="{edge}" stroke-width="1"/>"#
    );
    let mut x = 10.;
    while x < len - 8. {
        let _ = write!(
            boom,
            r#"<path d="M{x} -3 h8 l-3 6 h-8 z" fill="{stripe}"/>"#
        );
        x += 16.;
    }
    for (i, px_) in pivots.iter().enumerate() {
        let (flip, turn) = match (i, moment == Moment::Passed) {
            (0, false) => (1., 0.),
            (0, true) => (1., -86.),
            (_, false) => (-1., 0.),
            (_, true) => (-1., 86.),
        };
        let _ = write!(
            s,
            r#"<g transform="translate({px_} {y}) rotate({turn}) scale({flip} 1)">{boom}</g><rect x="{}" y="{}" width="14" height="14" rx="3" fill="{dark}" stroke="{edge}" stroke-width="1"/><circle cx="{px_}" cy="{y}" r="2.2" fill="{metal}"/>"#,
            px_ - 7.,
            y - 7.
        );
    }
}

/// A castle gate: a wall with battlements over a round arch, the question
/// on a stone set in the wall, and an iron portcullis that rises.
fn portcullis(s: &mut String, tones: &Tones, f: &Frame, wiring: Wiring, moment: Moment) {
    let (stone, joint, deep, iron, edge) = (
        tones.t(0.30),
        tones.t(0.18),
        tones.t(0.03),
        tones.t(0.56),
        tones.t(0.10),
    );
    let b = f.sign;
    let a = f.art_y;
    let (wl, wr) = (b.x, b.x + b.w);
    let top = b.y + 12.;
    // Battlements, then the wall from them to the ground.
    let mut x = wl;
    let mut merlons = String::new();
    while x < wr - 1. {
        let _ = write!(merlons, "M{x} {top} V{} h14 V{top} ", top - 12.);
        x += 24.;
    }
    let _ = write!(
        s,
        r#"<path d="{merlons}" fill="{stone}" stroke="{edge}" stroke-width="1"/><rect x="{wl}" y="{top}" width="{}" height="{}" fill="{stone}" stroke="{edge}" stroke-width="1"/>"#,
        wr - wl,
        f.h - top
    );
    // Courses of stone, each joint offset from the one under it.
    let mut y = top + 14.;
    let mut row = 0;
    let mut courses = String::new();
    while y < f.h - 2. {
        let _ = write!(courses, "M{wl} {y} H{wr} ");
        let mut x = wl + if row % 2 == 0 { 18. } else { 6. };
        while x < wr {
            let _ = write!(courses, "M{x} {} v14 ", y - 14.);
            x += 26.;
        }
        y += 14.;
        row += 1;
    }
    let _ = write!(
        s,
        r#"<path d="{courses}" stroke="{joint}" stroke-width="1" fill="none"/>"#
    );
    // The tablet: a smooth stone with a chiselled edge.
    let t = Rect {
        x: b.x + 8.,
        y: b.y + 18.,
        w: b.w - 16.,
        h: b.h - 18.,
    };
    let _ = write!(
        s,
        r#"<rect x="{}" y="{}" width="{}" height="{}" rx="3" fill="{}" stroke="{edge}" stroke-width="1.2"/><rect x="{}" y="{}" width="{}" height="{}" rx="2" fill="none" stroke="{}" stroke-width="0.8"/>"#,
        t.x,
        t.y,
        t.w,
        t.h,
        tones.t(0.10),
        t.x + 3.,
        t.y + 3.,
        t.w - 6.,
        t.h - 6.,
        tones.t(0.34)
    );
    // The arch: dark within, voussoirs round it.
    let (cx, r) = ((wl + wr) / 2., 30.);
    let spring = a + 40.;
    let opening = format!(
        "M{} {} V{spring} A{r} {r} 0 0 1 {} {spring} V{} Z",
        cx - r,
        f.h,
        cx + r,
        f.h
    );
    let _ = write!(
        s,
        r#"<path d="M{} {} V{spring} A{} {} 0 0 1 {} {spring} V{} Z" fill="{}" stroke="{edge}" stroke-width="1"/>"#,
        cx - r - 7.,
        f.h,
        r + 7.,
        r + 7.,
        cx + r + 7.,
        f.h,
        tones.t(0.38)
    );
    for i in 0..=8 {
        let ang = std::f32::consts::PI * i as f32 / 8.;
        let (c, sn) = (ang.cos(), ang.sin());
        let _ = write!(
            s,
            r#"<path d="M{} {} L{} {}" stroke="{edge}" stroke-width="1"/>"#,
            cx - c * r,
            spring - sn * r,
            cx - c * (r + 7.),
            spring - sn * (r + 7.)
        );
    }
    let _ = write!(s, r#"<path d="{opening}" fill="{deep}"/>"#);
    // Torches either side, lit while the gate is decided.
    let flame = deciding(tones, wiring, moment);
    for x in [cx - r - 20., cx + r + 20.] {
        let y = a + 36.;
        if let Some(c) = &flame {
            glow(s, x, y - 6., 5., c);
            let _ = write!(
                s,
                r#"<path d="M{x} {} c-4 -4 -3 -8 0 -12 c3 4 4 8 0 12 z" fill="{c}"/>"#,
                y - 1.
            );
        }
        let _ = write!(
            s,
            r#"<path d="M{} {y} h7 l-2 9 h-3 z" fill="{iron}" stroke="{edge}" stroke-width="0.8"/>"#,
            x - 3.5
        );
    }
    lines(s, tones, f, wiring, moment, (cx - r, cx + r));
    // The portcullis: iron bars on a grid, points at its foot, inside the
    // arch; raised, only its points show under the arch's crown.
    let lift = if moment == Moment::Passed {
        f.h - spring + 18.
    } else {
        0.
    };
    let iron_c = if moment == Moment::Failed {
        hex(tones.ink.fail)
    } else {
        iron.clone()
    };
    let mut grid = String::new();
    let foot = f.h - 2. - lift;
    let mut x = cx - r + 5.;
    while x < cx + r - 2. {
        let _ = write!(
            grid,
            "M{x} {} V{foot} l-2 -1 l2 5 l2 -5 l-2 1 ",
            spring - r - 2. - lift
        );
        x += 8.;
    }
    let mut y = foot - 6.;
    while y > spring - r - lift {
        let _ = write!(grid, "M{} {y} H{} ", cx - r, cx + r);
        y -= 11.;
    }
    let _ = write!(
        s,
        r#"<clipPath id="arch"><path d="{opening}"/></clipPath><g clip-path="url(#arch)"><path d="{grid}" stroke="{iron_c}" stroke-width="2.2" fill="none"/></g>"#
    );
}

/// A stave gate, as the old Norse built them: a shingled roof whose crossed
/// gable ends become dragons, Huginn and Muninn on its eaves, the question
/// carved on the lintel, and two plank doors on iron straps.
fn stave(s: &mut String, tones: &Tones, f: &Frame, wiring: Wiring, moment: Moment) {
    let (wood, dark, carve, iron, edge) = (
        tones.t(0.36),
        tones.t(0.22),
        tones.t(0.50),
        tones.t(0.62),
        tones.t(0.08),
    );
    let b = f.sign;
    let a = f.art_y;
    let (l, r) = (b.x + 6., b.x + b.w - 6.);
    let mid = (l + r) / 2.;
    // The roof over the lintel: shingles in scalloped rows under a ridge,
    // and the gable boards crossing over the ridge into two dragon heads
    // looking out, as on the old stave churches.
    let eave = b.y + 2.;
    let ridge = 12.;
    let (el, er) = (b.x - 16., b.x + b.w + 16.);
    let _ = write!(
        s,
        r#"<path d="M{el} {eave} L{mid} {ridge} L{er} {eave} Z" fill="{dark}" stroke="{edge}" stroke-width="1"/>"#
    );
    let mut row = 0;
    let mut y = ridge + 6.;
    while y < eave - 1. {
        let k = (y - ridge) / (eave - ridge);
        let (x0, x1) = (mid - (mid - el) * k, mid + (er - mid) * k);
        let mut x = x0 + if row % 2 == 0 { 2. } else { 6. };
        let mut scallops = String::new();
        while x < x1 - 8. {
            let _ = write!(scallops, "M{x} {y} a4 4 0 0 0 8 0 ");
            x += 8.;
        }
        let _ = write!(
            s,
            r#"<path d="{scallops}" fill="none" stroke="{}" stroke-width="0.9"/>"#,
            tones.t(0.36)
        );
        y += 5.;
        row += 1;
    }
    for side in [-1f32, 1.] {
        // The board runs up the roof's edge from the eave on this side,
        // crosses the ridge and ends in a head out past the other side.
        let (x0, y0) = (mid + side * (er - mid), eave);
        let run = (mid - x0, ridge - y0);
        let len = (run.0 * run.0 + run.1 * run.1).sqrt();
        let (ux, uy) = (run.0 / len, run.1 / len);
        let tip = (mid + ux * 14., ridge + uy * 14.);
        let _ = write!(
            s,
            r#"<path d="M{x0} {y0} L{} {}" stroke="{wood}" stroke-width="4.5"/><path d="M{x0} {} L{} {}" stroke="{carve}" stroke-width="0.8"/>"#,
            tip.0,
            tip.1,
            y0 - 1.,
            tip.0,
            tip.1 - 1.
        );
        // The head faces away from its board's foot: out over the ridge.
        let d = -side;
        let (hx, hy) = tip;
        let _ = write!(
            s,
            r#"<path d="M{hx} {} C{} {} {} {} {} {} L{} {} L{} {} L{} {} C{} {} {} {} {hx} {} Z" fill="{wood}" stroke="{edge}" stroke-width="0.9"/>"#,
            hy - 2.5,
            hx + d * 4.,
            hy - 7.,
            hx + d * 9.,
            hy - 8.,
            hx + d * 13.,
            hy - 6.,
            hx + d * 16.,
            hy - 3.,
            hx + d * 10.,
            hy - 2.,
            hx + d * 14.,
            hy + 1.,
            hx + d * 8.,
            hy + 3.,
            hx + d * 4.,
            hy + 3.,
            hy + 2.5
        );
        // Its crest curling back, and an eye.
        let _ = write!(
            s,
            r#"<path d="M{} {} c{} -6 {} -9 {} -6" fill="none" stroke="{wood}" stroke-width="2"/><circle cx="{}" cy="{}" r="1.1" fill="{edge}"/>"#,
            hx + d * 4.,
            hy - 6.,
            -d * 3.,
            -d * 7.,
            -d * 5.,
            hx + d * 9.5,
            hy - 5.
        );
    }
    // The lintel the question is carved on: a beam with a band of
    // knotwork at each end.
    let _ = write!(
        s,
        r#"<rect x="{}" y="{}" width="{}" height="{}" rx="2" fill="{}" stroke="{edge}" stroke-width="1.2"/><rect x="{}" y="{}" width="{}" height="{}" fill="none" stroke="{dark}" stroke-width="1"/>"#,
        b.x,
        b.y,
        b.w,
        b.h,
        tones.t(0.11),
        b.x + 3.,
        b.y + 3.,
        b.w - 6.,
        b.h - 6.
    );
    // The posts: past the lintel at each side, grooved.
    for x in [b.x - 12., b.x + b.w] {
        let _ = write!(
            s,
            r#"<rect x="{x}" y="{}" width="12" height="{}" fill="{wood}" stroke="{edge}" stroke-width="1"/><path d="M{} {} V{} M{} {} V{}" stroke="{dark}" stroke-width="1"/>"#,
            b.y - 2.,
            f.h - b.y + 2.,
            x + 4.,
            b.y + 2.,
            f.h - 2.,
            x + 8.,
            b.y + 2.,
            f.h - 2.
        );
        // Knotwork on the post's face where it meets the lintel.
        let _ = write!(
            s,
            r#"<path d="M{} {} c4 4 4 8 0 12 c-4 4 -4 8 0 12 M{} {} c-4 4 -4 8 0 12 c4 4 4 8 0 12" fill="none" stroke="{carve}" stroke-width="1"/>"#,
            x + 6.,
            b.y + 6.,
            x + 6.,
            b.y + 6.
        );
    }
    let (dl, dr) = (b.x, b.x + b.w);
    lines(s, tones, f, wiring, moment, (dl, dr));
    // The doors: planks, two iron straps with curled ends on each. Open,
    // each swings back into the dark, narrow and leaning.
    let open = moment == Moment::Passed;
    let door_w = (dr - dl) / 2.;
    if open {
        let _ = write!(
            s,
            r#"<rect x="{dl}" y="{a}" width="{}" height="{}" fill="{}"/>"#,
            dr - dl,
            f.h - a,
            tones.t(0.04)
        );
        lines(s, tones, f, wiring, moment, (dl, dr));
    }
    for side in [0, 1] {
        let hinge = if side == 0 { dl } else { dr };
        let dir = if side == 0 { 1. } else { -1. };
        let reach = if open { door_w * 0.26 } else { door_w };
        let rise = if open { 5. } else { 0. };
        let far = hinge + dir * reach;
        let _ = write!(
            s,
            r#"<path d="M{hinge} {a} L{far} {} L{far} {} L{hinge} {} Z" fill="{wood}" stroke="{edge}" stroke-width="1"/>"#,
            a + rise,
            f.h - rise,
            f.h
        );
        // Planks of slightly different wood, with a pale edge on each.
        let planks = 5;
        for i in 0..planks {
            let (t0, t1) = (i as f32 / planks as f32, (i + 1) as f32 / planks as f32);
            let (x0, x1) = (hinge + dir * reach * t0, hinge + dir * reach * t1);
            let tone = tones.t([0.36, 0.31, 0.39, 0.33, 0.37][i]);
            let _ = write!(
                s,
                r#"<path d="M{x0} {} L{x1} {} L{x1} {} L{x0} {} Z" fill="{tone}" stroke="{dark}" stroke-width="0.9"/><path d="M{} {} V{}" stroke="{carve}" stroke-width="0.7" opacity="0.5"/>"#,
                a + rise * t0 + 0.5,
                a + rise * t1 + 0.5,
                f.h - rise * t1,
                f.h - rise * t0,
                x0 + dir * 1.5,
                a + rise * t0 + 3.,
                f.h - rise * t0 - 3.
            );
        }
        for y in [a + 16., f.h - 16.] {
            let y2 = y + if y < (a + f.h) / 2. { rise } else { -rise } * 0.9;
            let _ = write!(
                s,
                r#"<path d="M{hinge} {y} L{} {}" stroke="{iron}" stroke-width="2.6"/>"#,
                hinge + dir * reach * 0.84,
                y2
            );
            if !open {
                let x = hinge + dir * reach * 0.84;
                let _ = write!(
                    s,
                    r#"<path d="M{x} {y2} c{} 0 {} -5 {} -4 c{} 1 {} 4 {} 3" fill="none" stroke="{iron}" stroke-width="1.6"/>"#,
                    dir * 4.,
                    dir * 6.,
                    dir * 2.,
                    -dir * 2.,
                    -dir * 1.,
                    dir * 1.
                );
            }
        }
    }
    if moment == Moment::Failed {
        // Barred: a beam across both doors in brackets.
        let y = (a + f.h) / 2. - 3.;
        let c = hex(tones.ink.fail);
        let _ = write!(
            s,
            r#"<rect x="{}" y="{y}" width="{}" height="7" rx="1.5" fill="{c}" stroke="{edge}" stroke-width="1"/>"#,
            dl - 6.,
            dr - dl + 12.
        );
    }
    // Huginn and Muninn on the eaves; once it is passed, one is off to
    // report it. Their eyes take the colour of what is being decided.
    let eye = deciding(tones, wiring, moment).unwrap_or_else(|| tones.t(0.5));
    raven(s, tones, el + 18., eave - 4., 24., false, &eye);
    if open {
        flying(s, tones, er - 30., eave - 22., 36., -14.);
    } else {
        raven(s, tones, er - 18., eave - 4., 24., true, &eye);
    }
}

/// A toll: a booth with a lit window where the gatekeeper sits, its board
/// on a pole above, and a striped boom out across the way.
fn toll(s: &mut String, tones: &Tones, f: &Frame, wiring: Wiring, moment: Moment) {
    let (wall, dark, light, edge) = (tones.t(0.34), tones.t(0.20), tones.t(0.88), tones.t(0.08));
    let b = f.sign;
    let a = f.art_y;
    // The board, on two short poles from the booth's roof.
    let (bx, bw) = (16., 58.);
    for x in [bx + 10., bx + bw - 10.] {
        let _ = write!(
            s,
            r#"<rect x="{}" y="{}" width="3" height="{}" fill="{dark}"/>"#,
            x - 1.5,
            b.y + b.h - 2.,
            a + 14. - (b.y + b.h - 2.)
        );
    }
    let _ = write!(
        s,
        r#"<rect x="{}" y="{}" width="{}" height="{}" rx="4" fill="{}" stroke="{wall}" stroke-width="1.6"/><path d="M{} {} h{}" stroke="{}" stroke-width="2"/>"#,
        b.x + 1.,
        b.y + 1.,
        b.w - 2.,
        b.h - 2.,
        tones.t(0.09),
        b.x + 6.,
        b.y + 4.,
        b.w - 12.,
        tones.t(0.22)
    );
    lines(s, tones, f, wiring, moment, (bx + bw + 2., f.w - 9.));
    // The booth: a roof with eaves, a window, a door's line.
    let roof = a + 2.;
    let _ = write!(
        s,
        r#"<rect x="{bx}" y="{}" width="{bw}" height="{}" fill="{wall}" stroke="{edge}" stroke-width="1"/><path d="M{} {} L{} {roof} L{} {roof} L{} {} Z" fill="{dark}" stroke="{edge}" stroke-width="1"/>"#,
        roof + 12.,
        f.h - roof - 12.,
        bx - 6.,
        roof + 14.,
        bx + 6.,
        bx + bw - 6.,
        bx + bw + 6.,
        roof + 14.
    );
    let (wx, wy, ww, wh) = (bx + 8., roof + 22., bw - 16., 26.);
    let lit = deciding(tones, wiring, moment);
    let pane = match (&lit, moment) {
        (Some(c), _) => c.clone(),
        (None, Moment::Failed) => hex(tones.ink.fail),
        _ => tones.t(0.14),
    };
    if let Some(c) = &lit {
        glow(s, wx + ww / 2., wy + wh / 2., 10., c);
    }
    let _ = write!(
        s,
        r#"<rect x="{wx}" y="{wy}" width="{ww}" height="{wh}" rx="2" fill="{pane}" stroke="{edge}" stroke-width="1.2"/>"#
    );
    // The keeper in the window: a raven at the sill.
    let eye = if lit.is_some() {
        pane.clone()
    } else {
        tones.t(0.5)
    };
    raven(s, tones, wx + ww / 2. - 2., wy + wh + 1., 22., false, &eye);
    let _ = write!(
        s,
        r#"<path d="M{wx} {} h{ww}" stroke="{edge}" stroke-width="2"/><path d="M{} {} V{}" stroke="{dark}" stroke-width="1"/>"#,
        wy + wh,
        bx + bw - 10.,
        wy + wh + 6.,
        f.h - 2.
    );
    // The boom, from a post at the far side back to the booth; it stands
    // up beside the board to let the line through.
    let pivot = (f.w - 9., f.line_y - 1.);
    let len = pivot.0 - (bx + bw) - 4.;
    let turn = if moment == Moment::Passed { 90. } else { 0. };
    let stripe = if moment == Moment::Failed {
        hex(tones.ink.fail)
    } else {
        tones.t(0.30)
    };
    let mut boom = format!(
        r#"<rect x="{}" y="-3.5" width="{len}" height="7" rx="3" fill="{light}" stroke="{edge}" stroke-width="1"/>"#,
        -len
    );
    let mut x = -len + 8.;
    while x < -12. {
        let _ = write!(
            boom,
            r#"<rect x="{x}" y="-3" width="8" height="6" fill="{stripe}"/>"#
        );
        x += 16.;
    }
    let _ = write!(
        s,
        r#"<rect x="{}" y="{}" width="7" height="{}" fill="{wall}" stroke="{edge}" stroke-width="1"/><g transform="translate({} {}) rotate({turn})">{boom}</g><circle cx="{}" cy="{}" r="5" fill="{dark}" stroke="{edge}" stroke-width="1"/>"#,
        pivot.0 - 3.5,
        pivot.1,
        f.h - pivot.1,
        pivot.0,
        pivot.1,
        pivot.0,
        pivot.1
    );
}

/// The look this prototype draws gates in, from `AHJO_BOARD_GATE`
/// (garden, crossing, portcullis, stave, toll); unset, the fork.
pub fn look() -> Option<Look> {
    static LOOK: std::sync::OnceLock<Option<Look>> = std::sync::OnceLock::new();
    *LOOK.get_or_init(|| {
        let name = std::env::var("AHJO_BOARD_GATE").ok()?;
        let look = Look::parse(&name);
        if look.is_none() {
            eprintln!("AHJO_BOARD_GATE={name}: no such gate look; drawing the fork");
        }
        look
    })
}

/// For the prototype's screenshots: a gate drawn in a moment of a run,
/// whether or not a run is shown, from `AHJO_BOARD_GATE_STATE`: one
/// moment for every gate (`deciding`, `passed`, `failed`), or each
/// gate's own (`gate_tests=passed,gate_ship=deciding`).
pub fn staged(gate: &str) -> Option<Moment> {
    let moment = |name: &str| match name {
        "deciding" => Some(Moment::Deciding),
        "passed" => Some(Moment::Passed),
        "failed" => Some(Moment::Failed),
        _ => None,
    };
    let stage = std::env::var("AHJO_BOARD_GATE_STATE").ok()?;
    if !stage.contains('=') {
        return moment(&stage);
    }
    stage
        .split(',')
        .filter_map(|pair| pair.split_once('='))
        .find(|(id, _)| *id == gate)
        .and_then(|(_, name)| moment(name))
}
