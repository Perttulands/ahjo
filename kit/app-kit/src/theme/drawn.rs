//! The raven drawn in a theme's own colours, for the themes that have no
//! painted art: the app's mark as a disc of the accent with the raven on
//! it, a raven perched over an empty place, and two flying past behind an
//! empty terminal window. Flat shapes, so they read at a tab's size and
//! on a light ground as well as a dark one.

use super::{Art, Palette, luminance, mix, rgb};
use gpui::{Image, ImageFormat};
use std::sync::Arc;

/// A raven perched, facing right, in a 64 by 64 box: heavy beak, shaggy
/// throat, wedge tail.
const PERCHED: &str = "M57 22.5 C53 19 49 17 44.5 16.5 C42 12.5 36 11.5 32 14.5 C26.5 18.5 23 25 20.5 31.5 L6 53 L15.5 50.5 L24.5 46.5 C31 47 38 45.5 42 39.5 L43 36 L41.5 33 L44.5 31.5 L43.5 28.5 L46.5 26 C50 24.5 53.5 23.8 57 22.5 Z M30 45 h2 v9 h-2z M35 44 h2 v10 h-2z M27.5 53 h5 v1.6 h-5z M33 53 h5.5 v1.6 h-5.5z";

/// A raven flying, seen from below, its head at the right: fingered wings
/// and a wedge tail, about its middle.
const FLYING: &str = "M18 0 L14 -1.4 C12 -2.6 8 -2.6 5 -2.2 C4 -8 1 -15 -2 -21 L-3.6 -19.6 L-3.8 -22.2 L-5.6 -20.4 L-6.2 -22.8 L-7.8 -20.6 C-7.4 -14 -6.6 -7 -6.5 -2.2 L-9 -2 L-17 -5 L-18 0 L-17 5 L-9 2 L-6.5 2.2 C-6.6 7 -7.4 14 -7.8 20.6 L-6.2 22.8 L-5.6 20.4 L-3.8 22.2 L-3.6 19.6 L-2 21 C1 15 4 8 5 2.2 C8 2.6 12 2.6 14 1.4 Z";

/// The eye, where the perched raven's head is.
const EYE: (f32, f32, f32) = (40., 18.8, 1.5);

fn hex(c: u32) -> String {
    format!("#{c:06x}")
}

fn svg(text: String) -> Arc<Image> {
    Arc::new(Image::from_bytes(ImageFormat::Svg, text.into_bytes()))
}

/// The darker of the ground and the ink: a raven is black on any theme.
fn raven(p: &Palette) -> u32 {
    if luminance(rgb(p.bg)) <= luminance(rgb(p.text)) {
        p.bg
    } else {
        p.text
    }
}

/// The mark: the raven on a disc of the accent, its eye the accent.
pub(super) fn mark_svg(p: &Palette) -> String {
    let (ex, ey, er) = EYE;
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="128" height="128" viewBox="0 0 64 64"><circle cx="32" cy="32" r="31" fill="{disc}"/><path fill="{raven}" d="{PERCHED}"/><circle cx="{ex}" cy="{ey}" r="{er}" fill="{disc}"/></svg>"#,
        disc = hex(p.accent),
        raven = hex(raven(p)),
    )
}

/// Over an empty place: the raven in a quiet tone of the ground, only its
/// eye in the accent.
pub(super) fn empty_svg(p: &Palette) -> String {
    let (ex, ey, er) = EYE;
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="440" height="440" viewBox="0 0 64 64"><path fill="{tone}" d="{PERCHED}"/><circle cx="{ex}" cy="{ey}" r="{er}" fill="{eye}"/></svg>"#,
        tone = hex(mix(p.bg, p.text, 0.16)),
        eye = hex(p.accent),
    )
}

/// Behind an empty terminal window, which covers it from its middle: two
/// ravens far off to the lower right, out from under the launcher, faint
/// in the terminal's ground.
pub(super) fn window_svg(p: &Palette) -> String {
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="1600" height="1000" viewBox="0 0 160 100"><g fill="{tone}"><path transform="translate(118 66) rotate(-24) scale(0.62)" d="{FLYING}"/><path transform="translate(136 50) rotate(-14) scale(0.4)" d="{FLYING}"/></g></svg>"#,
        tone = hex(mix(p.term_bg, p.term_fg, 0.06)),
    )
}

pub(super) fn art(p: &Palette) -> Art {
    Art {
        window: svg(window_svg(p)),
        empty: svg(empty_svg(p)),
        mark: svg(mark_svg(p)),
        window_show: 1.,
        empty_show: 1.,
    }
}
