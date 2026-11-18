//! One rule for zooming what the operator reads or works in, on every
//! surface that has a size of its own (a terminal, a code view, a picture,
//! a reader): Ctrl+wheel over the surface zooms it, Ctrl+= (or Ctrl++) and
//! Ctrl+- step it from the keyboard, Ctrl+0 puts it back. Each surface keeps
//! its own level where it keeps its other settings; this reads the gesture.

use gpui::{
    App, DispatchPhase, HitboxBehavior, IntoElement, Keystroke, ScrollDelta, ScrollEvent, Styled,
    Window, canvas,
};
use std::cell::Cell;
use std::rc::Rc;

/// What a zoom gesture asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zoom {
    In,
    Out,
    Reset,
}

/// How zooming reads in the Keys panel.
pub const HINT: &str = "Zoom: Ctrl+wheel over a terminal, code, a picture or a page you read; Ctrl+= and Ctrl+- from the keyboard; Ctrl+0 resets it.";

/// The zoom a key asks for: Ctrl, without Alt or the platform key, and
/// `=`, `+`, `-` or `0`. Shift is allowed, since `+` takes it on many layouts.
pub fn from_key(k: &Keystroke) -> Option<Zoom> {
    let m = &k.modifiers;
    if !m.control || m.alt || m.platform {
        return None;
    }
    match k.key.as_str() {
        "=" | "+" => Some(Zoom::In),
        "-" => Some(Zoom::Out),
        "0" => Some(Zoom::Reset),
        _ => None,
    }
}

/// A touchpad's pixels that make one step.
const PIXELS_PER_STEP: f32 = 50.;
/// The lines a mouse wheel's notch scrolls as the platforms report it
/// (three on Linux, and Windows' default): one step.
const LINES_PER_STEP: f32 = 3.;

/// The wheel with Ctrl held, as steps: a mouse's notch is one, a touchpad's
/// pixels add up to one. It keeps the part of a step not yet taken.
#[derive(Clone, Copy, Debug, Default)]
pub struct Wheel {
    rest: f32,
}

impl Wheel {
    /// `None` when the scroll is not a zoom (Ctrl is not held, or Alt or
    /// Shift is): it stays the surface's own scroll. Otherwise the steps it
    /// makes, up being in, and zero while a touchpad's step builds up.
    pub fn steps(&mut self, e: &ScrollEvent) -> Option<i32> {
        let m = &e.modifiers;
        if !m.control || m.alt || m.shift || m.platform {
            return None;
        }
        let amount = match e.delta {
            ScrollDelta::Lines(l) => l.y / LINES_PER_STEP,
            ScrollDelta::Pixels(p) => f32::from(p.y) / PIXELS_PER_STEP,
        };
        let total = self.rest + amount;
        let whole = total.trunc();
        self.rest = total - whole;
        Some(whole as i32)
    }
}

/// Put over a surface that scrolls (a list, a reader, a code view), it
/// takes Ctrl+wheel before the surface's own scroll does and gives `on` the
/// steps; any other wheel scrolls as before. Something drawn over it (a
/// menu, a dialog) keeps its wheel.
pub fn catch(
    wheel: Rc<Cell<Wheel>>,
    on: impl Fn(i32, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    canvas(
        |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
        move |_, hitbox, window, _| {
            window.on_mouse_event(move |e: &ScrollEvent, phase, window, cx| {
                if phase != DispatchPhase::Capture || !hitbox.should_handle_scroll(window) {
                    return;
                }
                let mut w = wheel.get();
                let steps = w.steps(e);
                wheel.set(w);
                if let Some(n) = steps {
                    cx.stop_propagation();
                    if n != 0 {
                        on(n, window, cx);
                    }
                }
            });
        },
    )
    // Over the whole surface, wherever among its children it is put.
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// A size `at` stepped `n` times by `by`, held between `min` and `max`;
/// `None` when it would not change.
pub fn step(at: f32, n: i32, by: f32, min: f32, max: f32) -> Option<f32> {
    let to = (at + n as f32 * by).clamp(min, max);
    (to != at).then_some(to)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Modifiers, point, px};

    fn key(k: &str, control: bool, alt: bool) -> Keystroke {
        Keystroke {
            modifiers: Modifiers {
                control,
                alt,
                ..Default::default()
            },
            key: k.into(),
            key_char: None,
        }
    }

    fn wheel(delta: ScrollDelta, control: bool) -> ScrollEvent {
        ScrollEvent {
            position: point(px(0.), px(0.)),
            delta,
            modifiers: Modifiers {
                control,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn ctrl_and_the_browser_keys_zoom_and_nothing_else_does() {
        assert_eq!(from_key(&key("=", true, false)), Some(Zoom::In));
        assert_eq!(from_key(&key("+", true, false)), Some(Zoom::In));
        assert_eq!(from_key(&key("-", true, false)), Some(Zoom::Out));
        assert_eq!(from_key(&key("0", true, false)), Some(Zoom::Reset));
        assert_eq!(from_key(&key("=", false, false)), None);
        assert_eq!(
            from_key(&key("=", true, true)),
            None,
            "Alt+Ctrl is not a zoom"
        );
        assert_eq!(from_key(&key("9", true, false)), None);
    }

    #[test]
    fn a_notch_is_a_step_and_a_touchpad_adds_up_to_one() {
        let mut w = Wheel::default();
        let notch = wheel(ScrollDelta::Lines(point(0., 3.)), true);
        assert_eq!(w.steps(&notch), Some(1));
        let down = wheel(ScrollDelta::Lines(point(0., -6.)), true);
        assert_eq!(w.steps(&down), Some(-2));
        // A wheel with finer notches adds up to a step, as a touchpad does.
        let fine = wheel(ScrollDelta::Lines(point(0., 1.5)), true);
        assert_eq!(w.steps(&fine), Some(0));
        assert_eq!(w.steps(&fine), Some(1));
        let plain = wheel(ScrollDelta::Lines(point(0., 1.)), false);
        assert_eq!(w.steps(&plain), None, "without Ctrl the wheel scrolls");
        let bit = wheel(ScrollDelta::Pixels(point(px(0.), px(30.))), true);
        assert_eq!(w.steps(&bit), Some(0));
        assert_eq!(w.steps(&bit), Some(1), "60 pixels made one step");
    }

    #[test]
    fn a_step_stays_in_range_and_says_when_it_did_nothing() {
        assert_eq!(step(14., 1, 1., 9., 32.), Some(15.));
        assert_eq!(step(14., -3, 1., 9., 32.), Some(11.));
        assert_eq!(step(32., 1, 1., 9., 32.), None);
        assert_eq!(step(31., 5, 1., 9., 32.), Some(32.));
    }
}
