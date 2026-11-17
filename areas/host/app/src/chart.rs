//! The machine's figures as bands on one time axis, drawn by one canvas so
//! the crosshair under the mouse stands at the same moment in every band.
//! Each band is runs of line over a filled area, x by time (a silence shows
//! as space), y to the band's top; a lone point is a stem. The axis's ticks
//! are faint rules through every band.

use crate::page::HostPage;
use crate::tokens;
use ahjo_app_kit::widgets::boundary;
use gpui::{
    Bounds, Hsla, MouseMoveEvent, PathBuilder, Pixels, WeakEntity, canvas, fill, point, prelude::*,
    px, size,
};

/// One figure's band.
pub struct Band {
    pub runs: Vec<Vec<(u64, f32)>>,
    /// The value at the band's top.
    pub top: f32,
    pub ink: Hsla,
}

/// The bands, top to bottom, over one span of time.
pub struct Stack {
    pub bands: Vec<Band>,
    pub start: u64,
    pub end: u64,
    /// The time axis's ticks, Unix ms.
    pub ticks: Vec<u64>,
    pub hover: Option<u64>,
    pub grid: Hsla,
    /// The crosshair's colour.
    pub hair: Hsla,
}

/// The stack, drawn to fill its parent: each band [`tokens::BAND_H`] high,
/// [`tokens::BAND_GAP`] apart. The mouse over it sets the moment the page
/// reads; leaving it clears it.
pub fn stack(s: Stack, page: WeakEntity<HostPage>) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        boundary::paint(move |b: Bounds<Pixels>, _: (), window, _| {
            for (i, band) in s.bands.iter().enumerate() {
                let top = b.origin.y + px(i as f32 * (tokens::BAND_H + tokens::BAND_GAP));
                let area = Bounds::new(
                    point(b.origin.x, top),
                    size(b.size.width, px(tokens::BAND_H)),
                );
                draw(&s, band, area, window);
            }
            follow_mouse(s.start, s.end, b, page.clone(), window);
        }),
    )
    .size_full()
}

/// The height of a stack of `n` bands.
pub fn height(n: usize) -> f32 {
    n as f32 * tokens::BAND_H + n.saturating_sub(1) as f32 * tokens::BAND_GAP
}

fn x_of(at: u64, s: &Stack, b: Bounds<Pixels>) -> Pixels {
    let span = (s.end - s.start).max(1) as f32;
    b.origin.x + b.size.width * ((at.saturating_sub(s.start)) as f32 / span).clamp(0., 1.)
}

fn y_of(v: f32, top: f32, b: Bounds<Pixels>) -> Pixels {
    let share = (v / top).clamp(0., 1.);
    b.origin.y + b.size.height * (1. - share)
}

fn rule(window: &mut gpui::Window, at: gpui::Point<Pixels>, sz: gpui::Size<Pixels>, c: Hsla) {
    window.paint_quad(fill(Bounds::new(at, sz), c));
}

fn draw(s: &Stack, band: &Band, b: Bounds<Pixels>, window: &mut gpui::Window) {
    let base = b.origin.y + b.size.height - px(tokens::HAIRLINE);
    let faint = Hsla {
        a: s.grid.a * tokens::GRID_TICK,
        ..s.grid
    };
    rule(
        window,
        point(b.origin.x, base),
        size(b.size.width, px(tokens::HAIRLINE)),
        s.grid,
    );
    rule(
        window,
        b.origin,
        size(b.size.width, px(tokens::HAIRLINE)),
        faint,
    );
    for at in &s.ticks {
        rule(
            window,
            point(x_of(*at, s, b), b.origin.y),
            size(px(tokens::HAIRLINE), b.size.height),
            faint,
        );
    }
    let area = Hsla {
        a: tokens::AREA_ALPHA,
        ..band.ink
    };
    for run in &band.runs {
        if let [(at, v)] = run.as_slice() {
            let x = x_of(*at, s, b);
            let y = y_of(*v, band.top, b);
            rule(
                window,
                point(x, y),
                size(px(tokens::STROKE), base - y),
                band.ink,
            );
            continue;
        }
        let mut under = PathBuilder::fill();
        let mut stroke = PathBuilder::stroke(px(tokens::STROKE));
        for (i, (at, v)) in run.iter().enumerate() {
            let p = point(x_of(*at, s, b), y_of(*v, band.top, b));
            if i == 0 {
                under.move_to(point(p.x, base));
                under.line_to(p);
                stroke.move_to(p);
            } else {
                under.line_to(p);
                stroke.line_to(p);
            }
        }
        if let Some((at, _)) = run.last() {
            under.line_to(point(x_of(*at, s, b), base));
            under.close();
        }
        if let Ok(p) = under.build() {
            window.paint_path(p, area);
        }
        if let Ok(p) = stroke.build() {
            window.paint_path(p, band.ink);
        }
    }
    if let Some(at) = s.hover
        && at >= s.start
    {
        rule(
            window,
            point(x_of(at, s, b), b.origin.y),
            size(px(tokens::HAIRLINE), b.size.height),
            s.hair,
        );
    }
}

/// The mouse over the stack sets the moment the page reads; leaving it
/// clears it.
fn follow_mouse(
    start: u64,
    end: u64,
    b: Bounds<Pixels>,
    page: WeakEntity<HostPage>,
    window: &mut gpui::Window,
) {
    window.on_mouse_event(move |e: &MouseMoveEvent, phase, _, cx| {
        if phase != gpui::DispatchPhase::Bubble {
            return;
        }
        let at = b.contains(&e.position).then(|| {
            let share = ((e.position.x - b.origin.x) / b.size.width).clamp(0., 1.);
            start + ((end - start) as f32 * share) as u64
        });
        let _ = page.update(cx, |s, cx| {
            if s.hover != at {
                s.hover = at;
                cx.notify();
            }
        });
    });
}
