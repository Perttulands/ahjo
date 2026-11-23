//! A gate drawn as a gate: a small gatehouse (two towers, a wall with
//! battlements, a round arch with an iron grate) standing where the work
//! arrives, its words beside it, so the line visibly goes into the arch and
//! out the other side. Its fail drops from a postern at the foot of the near
//! tower, and beside that drop stands the loop it starts ("up to 3 tries").
//!
//! In a run the grate rises on a pass and the line runs through lit; on a
//! fail the grate stays down in the fail colour and the work leaves by the
//! postern; while it is decided a pennant flies, in the operator's colour
//! when it waits on him. Only the meanings colour it.
//!
//! The art is SVG made for the card's size, as the raven is drawn
//! (`theme/drawn.rs`); the words over it are the app's own text.

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

/// Where a gate is in a run, or `Plan` on the board as authored.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Moment {
    Plan,
    /// Reached and being decided: a pennant in the operator's colour when
    /// it waits on him, in the running colour while a judge or check works.
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

/// A gate's card: its size, where its words and who decides stand, where
/// the line runs through it, where its fail drops out at its foot, the row
/// beside that drop for the loop it starts, and where its note mark sits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub w: f32,
    pub h: f32,
    /// The title and the condition.
    pub words: Rect,
    /// The row naming who decides.
    pub who: Rect,
    /// The row beside the fail exit saying how often it may go round.
    pub tries: Rect,
    pub line_y: f32,
    pub fail_x: f32,
    /// The gate's centre across the card.
    pub gate_x: f32,
    /// Where the note mark sits, on the gate's own corner.
    pub pin: (f32, f32),
}

pub const TITLE_H: f32 = 19.;
pub const LINE_H: f32 = 17.;
pub const WHO_H: f32 = 22.;
pub const TRIES_H: f32 = 18.;
/// How wide a condition's lines run.
pub const WORDS_W: f32 = 184.;
/// The width a condition is measured at to count its lines: a little
/// under the room it has, so the guess never comes up a line short of how
/// the app's type wraps.
pub const MEASURE_W: f32 = WORDS_W * 0.9;

/// The gatehouse at size 1: half its width, its height over the line (to
/// the pennant's top) and under it (to the ground).
const HALF: f32 = 30.;
const ABOVE: f32 = 46.;
const BELOW: f32 = 15.;
/// Between the words and the line.
const AIR: f32 = 10.;

fn words_h(lines: usize) -> f32 {
    TITLE_H + lines.max(1) as f32 * LINE_H
}

/// The card for a condition of `lines` lines: the gate where the work
/// comes in, its words beside it, who decides under the way on and the
/// loop's tries beside the fail's drop.
pub fn frame(lines: usize) -> Frame {
    let words = words_h(lines);
    let gate_x = HALF + 8.;
    let x = gate_x + HALF + 16.;
    let line_y = (words + AIR).max(ABOVE + 2.);
    let who = Rect {
        x,
        y: line_y + 8.,
        w: WORDS_W,
        h: WHO_H,
    };
    let fail_x = gate_x - 24.;
    // Its own words on the fail line's way down: the loop it starts
    // belongs to the gate.
    let tries = Rect {
        x: fail_x + 8.,
        y: who.y + WHO_H,
        w: x + WORDS_W - fail_x - 8.,
        h: TRIES_H,
    };
    Frame {
        w: x + WORDS_W,
        h: tries.y + TRIES_H + 4.,
        words: Rect {
            x,
            y: line_y - AIR - words,
            w: WORDS_W,
            h: words,
        },
        who,
        tries,
        line_y,
        fail_x,
        gate_x,
        pin: (gate_x - HALF - 4., line_y - 39.),
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
}

/// The colours of the gate's three ways and how heavy each is drawn: the
/// way taken in a run is drawn heavier.
struct Ways {
    into: String,
    pass: String,
    fail: String,
    into_w: f32,
    pass_w: f32,
    fail_w: f32,
}

fn ways(ink: Ink, wiring: Wiring, moment: Moment) -> Ways {
    let (base, heavy) = (1.5, 2.6);
    Ways {
        into: hex(match moment {
            Moment::Passed => ink.pass,
            Moment::Failed => ink.fail,
            _ => ink.wire,
        }),
        pass: hex(match (moment, wiring.pass) {
            (Moment::Failed, _) | (_, false) => ink.quiet,
            _ => ink.pass,
        }),
        fail: hex(if moment == Moment::Passed {
            ink.quiet
        } else {
            ink.fail
        }),
        into_w: if matches!(moment, Moment::Passed | Moment::Failed) {
            heavy
        } else {
            base
        },
        pass_w: if moment == Moment::Passed {
            heavy
        } else {
            base
        },
        fail_w: if moment == Moment::Failed {
            heavy
        } else {
            base
        },
    }
}

/// The SVG for a gate card of frame `f`, drawn `scale` times its size.
pub fn svg(ink: Ink, f: &Frame, wiring: Wiring, moment: Moment, scale: f32) -> String {
    let tones = Tones {
        ink,
        dark: lum(ink.bg) < lum(ink.text),
    };
    let w = ways(ink, wiring, moment);
    let mut s = String::new();
    let _ = write!(
        s,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="0 0 {} {}" stroke-linejoin="round" stroke-linecap="round">"#,
        (f.w * scale).round(),
        (f.h * scale).round(),
        f.w,
        f.h
    );
    let (cx, y) = (f.gate_x, f.line_y);
    let (left, right) = (cx - HALF, cx + HALF);
    // The way in, up to the near tower; the way on, from the far tower.
    let _ = write!(
        s,
        r#"<path d="M0 {y} H{left}" stroke="{}" stroke-width="{}" fill="none"/>"#,
        w.into, w.into_w
    );
    let _ = write!(
        s,
        r#"<path d="M{right} {y} H{}" stroke="{}" stroke-width="{}" fill="none"/>"#,
        f.w, w.pass, w.pass_w
    );
    if wiring.fail {
        let _ = write!(
            s,
            r#"<path d="M{} {} V{}" stroke="{}" stroke-width="{}" fill="none"/>"#,
            f.fail_x,
            y + BELOW - 2.,
            f.h,
            w.fail,
            w.fail_w
        );
    }
    gatehouse(&mut s, &tones, (cx, y, 1.), wiring, moment, &w);
    s.push_str("</svg>");
    s
}

/// The gatehouse standing on the line at (`cx`, `y`), `k` times its size:
/// two towers, a wall with battlements between them over a round arch, the
/// grate in the arch, the postern at the foot of the near tower and, while
/// it is decided, a pennant on the far tower.
fn gatehouse(
    s: &mut String,
    tones: &Tones,
    (cx, y, k): (f32, f32, f32),
    wiring: Wiring,
    moment: Moment,
    w: &Ways,
) {
    let ink = tones.ink;
    let stone = tones.t(if tones.dark { 0.17 } else { 0.10 });
    let edge = tones.t(if tones.dark { 0.58 } else { 0.62 });
    let joint = tones.t(if tones.dark { 0.32 } else { 0.30 });
    let deep = if tones.dark {
        hex(mix(ink.bg, 0, 0.55))
    } else {
        tones.t(0.30)
    };
    let iron = tones.t(if tones.dark { 0.80 } else { 0.78 });
    let stroke = 1.2 * k.max(0.75);
    let ground = y + BELOW * k;
    // The wall between the towers, battlements along its top.
    let (wl, wr, wt) = (cx - 19. * k, cx + 19. * k, y - 23. * k);
    let m = 5. * k;
    let mut wall = format!("M{wl} {ground} V{wt} ");
    // Three merlons over the arch.
    for i in 0..3 {
        let x0 = wl + 4. * k + i as f32 * 11.5 * k;
        let _ = write!(wall, "H{x0} v{} h{m} v{m} ", -m);
    }
    let _ = write!(wall, "H{wr} V{ground} Z");
    let _ = write!(
        s,
        r#"<path d="{wall}" fill="{stone}" stroke="{edge}" stroke-width="{stroke}"/>"#
    );
    // The towers, each with two merlons, standing proud of the wall.
    let tt = y - 31. * k;
    let tm = 4. * k;
    for (tl, tr) in [(cx - 30. * k, cx - 18. * k), (cx + 18. * k, cx + 30. * k)] {
        let _ = write!(
            s,
            r#"<path d="M{tl} {ground} V{} h{tm} v{tm} H{} v{} h{tm} V{ground} Z" fill="{stone}" stroke="{edge}" stroke-width="{stroke}"/>"#,
            tt - tm,
            tr - tm,
            -tm,
        );
        // A course joint or two, and an arrow slit.
        let mid = (tl + tr) / 2.;
        let _ = write!(
            s,
            r#"<path d="M{tl} {} H{tr} M{tl} {} H{tr}" stroke="{joint}" stroke-width="{}" fill="none"/><path d="M{mid} {} V{}" stroke="{deep}" stroke-width="{}" fill="none"/>"#,
            tt + 9. * k,
            ground - 17. * k,
            0.8 * k.max(0.75),
            tt + 2.5 * k,
            tt + 7. * k,
            1.6 * k
        );
    }
    // The arch: dark within, its ring of stones round it.
    let (ow, spring) = (11. * k, y - 3. * k);
    let crown = spring - ow;
    let opening = format!(
        "M{} {ground} V{spring} A{ow} {ow} 0 0 1 {} {spring} V{ground} Z",
        cx - ow,
        cx + ow
    );
    let ring = ow + 3.5 * k;
    let _ = write!(
        s,
        r#"<path d="M{} {ground} V{spring} A{ring} {ring} 0 0 1 {} {spring} V{ground}" fill="none" stroke="{joint}" stroke-width="{}"/><path d="{opening}" fill="{deep}" stroke="{edge}" stroke-width="{stroke}"/>"#,
        cx - ring,
        cx + ring,
        0.9 * k.max(0.75)
    );
    // On a pass the line runs on through the arch, under the raised grate.
    if moment == Moment::Passed {
        let _ = write!(
            s,
            r#"<path d="M{} {y} H{}" stroke="{}" stroke-width="{}" fill="none"/>"#,
            cx - ow,
            cx + ow,
            w.pass,
            w.pass_w
        );
    }
    // The grate: bars on a grid with points at its foot; raised on a pass
    // so only its points show under the crown.
    let foot = if moment == Moment::Passed {
        crown + 6. * k
    } else {
        ground - 1.
    };
    let mut grate = String::new();
    let bars = 4;
    for i in 0..bars {
        let x = cx - ow + (i as f32 + 0.5) * (2. * ow / bars as f32);
        let _ = write!(
            grate,
            "M{x} {} V{foot} M{} {foot} l{} {} l{} {} ",
            crown - 2.,
            x - 1.4 * k,
            1.4 * k,
            2.6 * k,
            1.4 * k,
            -2.6 * k
        );
    }
    let mut row = foot - 4. * k;
    while row > crown {
        let _ = write!(grate, "M{} {row} H{} ", cx - ow, cx + ow);
        row -= 7. * k;
    }
    // A grate that held the work back is the fail's colour.
    let bars = if moment == Moment::Failed {
        hex(ink.fail)
    } else {
        iron.clone()
    };
    let _ = write!(
        s,
        r#"<clipPath id="arch{}"><path d="{opening}"/></clipPath><g clip-path="url(#arch{})"><path d="{grate}" stroke="{bars}" stroke-width="{}" fill="none"/></g>"#,
        (cx * 10.) as i32,
        (cx * 10.) as i32,
        1.5 * k.max(0.75)
    );
    // The postern: a small door at the foot of the near tower, where a
    // fail leaves; open on a fail.
    if wiring.fail {
        let (px, pw, ph) = (cx - 24. * k, 3. * k, 9. * k);
        let door = if moment == Moment::Failed {
            deep.clone()
        } else {
            tones.t(if tones.dark { 0.36 } else { 0.40 })
        };
        let _ = write!(
            s,
            r#"<path d="M{} {ground} V{} A{pw} {pw} 0 0 1 {} {} V{ground} Z" fill="{door}" stroke="{edge}" stroke-width="{}"/>"#,
            px - pw,
            ground - ph + pw,
            px + pw,
            ground - ph + pw,
            stroke * 0.8
        );
    }
    // The pennant, flown while the gate is decided.
    if moment == Moment::Deciding {
        let colour = hex(if wiring.human { ink.needs } else { ink.running });
        let pole = cx + 24. * k;
        let top = y - ABOVE * k + 1.;
        let _ = write!(
            s,
            r#"<path d="M{pole} {} V{top}" stroke="{edge}" stroke-width="{}" fill="none"/><path d="M{pole} {top} q{} {} {} {} q{} {} {} {} z" fill="{colour}" stroke="{colour}" stroke-width="{}"/>"#,
            tt - 4. * k,
            stroke,
            6. * k,
            1. * k,
            12. * k,
            4. * k,
            -6. * k,
            1. * k,
            -12. * k,
            4. * k,
            0.6 * k
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_runs_level_and_the_fail_drops_inside_the_card() {
        for lines in [1, 4, 8] {
            let f = frame(lines);
            assert!(f.line_y > 0. && f.line_y < f.h);
            assert!(f.fail_x > 0. && f.fail_x < f.w);
            assert!(f.words.y >= 0., "words above the card");
            assert!(f.who.y + f.who.h <= f.h + 0.5, "who below it");
            assert!(f.tries.y + f.tries.h <= f.h + 0.5, "tries below it");
            assert!(f.tries.x + f.tries.w <= f.w + 0.5, "tries beside it");
        }
    }
}
