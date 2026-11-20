//! Issue over time, drawn: each day's opened and closed bars with the work
//! open at each day's end, and the range of days chosen. A drag across it
//! chooses a range and the page answers as the mouse moves; a click takes
//! one day, and a click on the one day taken lets it go.
//!
//! Two shapes of one picture. The strip, over a list: the day's two bars
//! side by side, the open level faint behind them. The chart, as the page:
//! the open level in a band of its own over the daily flow, opened rising
//! from the flow's middle and closed falling from it, so a week that
//! finishes more than it opens reads as weight below the line.

use crate::Issue;
use crate::store::issue::timeline::{Day, Range, Timeline, day_label};
use crate::tokens::{self, *};
use ahjo_app_kit::theme::{FAMILY, Theme, alpha, size as kit};
use ahjo_app_kit::widgets::boundary;
use gpui::{
    App, Bounds, DispatchPhase, Hsla, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    PathBuilder, Pixels, Point, TextAlign, WeakEntity, Window, canvas, fill, point, prelude::*, px,
    size,
};
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    Strip,
    Chart,
}

/// What one frame of the chart shows.
#[derive(Clone)]
pub struct Picture {
    pub timeline: Rc<Timeline>,
    pub shape: Shape,
    /// The range chosen, or being dragged.
    pub range: Option<Range>,
    pub hover: Option<Day>,
}

/// The chart, filling its parent; the mouse over it moves `page`'s range
/// and hover.
pub fn chart(t: &Theme, picture: Picture, page: WeakEntity<Issue>) -> impl IntoElement {
    let t = t.clone();
    canvas(
        |_, _, _| (),
        boundary::paint(move |b: Bounds<Pixels>, _: (), window, cx| {
            let plot = plot_of(b);
            draw(&t, &picture, b, plot, window, cx);
            follow(&picture.timeline, plot, page.clone(), window);
        }),
    )
    .size_full()
}

/// Where the bars go: the bounds less the axes' room.
fn plot_of(b: Bounds<Pixels>) -> Bounds<Pixels> {
    Bounds::new(
        point(b.origin.x + px(AXIS_LEFT), b.origin.y + px(AXIS_TOP)),
        size(
            (b.size.width - px(AXIS_LEFT + AXIS_RIGHT)).max(px(1.)),
            (b.size.height - px(AXIS_TOP + AXIS_BOTTOM)).max(px(1.)),
        ),
    )
}

/// The day under `x`, held to the timeline's days.
fn day_at(t: &Timeline, plot: Bounds<Pixels>, x: Pixels) -> Day {
    let n = t.opened.len().max(1) as f32;
    let share = ((x - plot.origin.x) / plot.size.width).clamp(0., 0.9999);
    t.first + (share * n) as i64
}

/// A round top for an axis and the step between its gridlines, so the
/// lines fall on whole numbers (CHROTE's `niceScale`).
pub fn nice(max: u32) -> (u32, u32) {
    if max <= 4 {
        return (max.max(1), 1);
    }
    let rough = max as f64 / 3.;
    let magnitude = 10f64.powf(rough.log10().floor());
    let step = [1., 2., 5., 10.]
        .iter()
        .map(|f| f * magnitude)
        .find(|s| *s >= rough)
        .unwrap_or(rough)
        .max(1.);
    let top = (max as f64 / step).ceil() * step;
    (top as u32, step as u32)
}

fn draw(
    t: &Theme,
    p: &Picture,
    b: Bounds<Pixels>,
    plot: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let tl = &p.timeline;
    // Nothing read yet: nothing to draw.
    if tl.opened.is_empty() {
        return;
    }
    let n = tl.opened.len();
    let day_w = plot.size.width / n as f32;
    let x_of = |day: Day| plot.origin.x + day_w * (day - tl.first) as f32;
    let grid = alpha(t.divider, t.divider.a * GRID);
    // With days chosen, the days outside them step back.
    let held = |d: usize| p.range.is_none_or(|r| r.holds(tl.first + d as i64));
    let ink = |c: Hsla, d: usize| {
        if held(d) { c } else { alpha(c, c.a * OUTSIDE) }
    };
    let opened_ink = tokens::opened(t);

    // The range: a wash under everything, its edges ruled in the accent.
    if let Some(r) = p.range {
        let from = r.from.max(tl.first);
        let to = r.to.min(tl.last());
        if from <= to {
            let (x0, x1) = (x_of(from), x_of(to + 1));
            let top = plot.origin.y - px(BRUSH_RISE);
            let high = plot.size.height + px(BRUSH_RISE);
            window.paint_quad(fill(
                Bounds::new(point(x0, top), size(x1 - x0, high)),
                alpha(t.accent, BRUSH_WASH),
            ));
            for x in [x0, x1 - px(EDGE)] {
                window.paint_quad(fill(
                    Bounds::new(point(x, top), size(px(EDGE), high)),
                    alpha(t.accent, EDGE_INK),
                ));
            }
        }
    }

    let (level, flow) = match p.shape {
        Shape::Strip => (plot, plot),
        Shape::Chart => {
            let top = plot.size.height * LEVEL_SHARE;
            (
                Bounds::new(plot.origin, size(plot.size.width, top)),
                Bounds::new(
                    point(plot.origin.x, plot.origin.y + top + px(BAND_GAP)),
                    size(plot.size.width, plot.size.height - top - px(BAND_GAP)),
                ),
            )
        }
    };

    // The open level: a line through each day's middle, washed beneath.
    let most_open = tl.open.iter().copied().max().unwrap_or(0);
    let (open_top, open_step) = nice(most_open);
    let level_y = |v: u32| level.origin.y + level.size.height * (1. - v as f32 / open_top as f32);
    let base = level.origin.y + level.size.height;
    let mid = |d: usize| level.origin.x + day_w * (d as f32 + 0.5);
    if p.shape == Shape::Chart {
        gridlines(t, level, open_top, open_step, grid, window, cx);
    }
    let line_ink = match p.shape {
        Shape::Strip => alpha(t.chart, STRIP_LINE),
        Shape::Chart => t.chart,
    };
    if most_open > 0 {
        let mut under = PathBuilder::fill();
        let mut line = PathBuilder::stroke(px(STROKE));
        under.move_to(point(level.origin.x, base));
        for (d, v) in tl.open.iter().enumerate() {
            let at = point(mid(d), level_y(*v));
            if d == 0 {
                under.line_to(point(level.origin.x, at.y));
                line.move_to(point(level.origin.x, at.y));
            }
            under.line_to(at);
            line.line_to(at);
        }
        let end = level.origin.x + level.size.width;
        let last = level_y(*tl.open.last().unwrap_or(&0));
        under.line_to(point(end, last));
        line.line_to(point(end, last));
        under.line_to(point(end, base));
        under.close();
        if let Ok(path) = under.build() {
            window.paint_path(path, alpha(t.chart, LEVEL_WASH));
        }
        if let Ok(path) = line.build() {
            window.paint_path(path, line_ink);
        }
        // The open count's scale: at the right of the strip, whose left
        // scale counts the bars. The chart's level band has its own.
        if p.shape == Shape::Strip {
            text(
                &open_top.to_string(),
                point(
                    level.origin.x + level.size.width + px(6.),
                    level.origin.y - px(6.),
                ),
                TextAlign::Left,
                t.dim,
                window,
                cx,
            );
        }
    }

    // The daily flow.
    let most = tl
        .opened
        .iter()
        .chain(tl.closed.iter())
        .copied()
        .max()
        .unwrap_or(0);
    let (top, step) = nice(most);
    let bar = |w: Pixels| w.min(px(BAR_MAX)).max(px(1.));
    let rounded = |q: gpui::PaintQuad, up: bool| {
        let r = px(BAR_RADIUS);
        let z = px(0.);
        q.corner_radii(if up {
            gpui::Corners {
                top_left: r,
                top_right: r,
                bottom_left: z,
                bottom_right: z,
            }
        } else {
            gpui::Corners {
                top_left: z,
                top_right: z,
                bottom_left: r,
                bottom_right: r,
            }
        })
    };
    match p.shape {
        Shape::Strip => {
            gridlines(t, flow, top, step, grid, window, cx);
            let bw = bar(day_w / 2. - px(BAR_GAP));
            let y = |v: u32| flow.origin.y + flow.size.height * (1. - v as f32 / top as f32);
            let floor = flow.origin.y + flow.size.height;
            for d in 0..n {
                let (o, c) = (tl.opened[d], tl.closed[d]);
                if o > 0 {
                    window.paint_quad(rounded(
                        fill(
                            Bounds::new(point(mid(d) - bw, y(o)), size(bw, floor - y(o))),
                            ink(opened_ink, d),
                        ),
                        true,
                    ));
                }
                if c > 0 {
                    window.paint_quad(rounded(
                        fill(
                            Bounds::new(point(mid(d), y(c)), size(bw, floor - y(c))),
                            ink(t.done, d),
                        ),
                        true,
                    ));
                }
            }
        }
        Shape::Chart => {
            let half = flow.size.height / 2.;
            let mid_y = flow.origin.y + half;
            let bw = bar(day_w - px(BAR_GAP) * 2.);
            let h = |v: u32| half * (v as f32 / top as f32);
            // Gridlines on both sides of the middle, the scale on the left.
            let mut v = step;
            while v <= top {
                for y in [mid_y - h(v), mid_y + h(v)] {
                    rule(flow, y, grid, window);
                    text(
                        &v.to_string(),
                        point(flow.origin.x - px(6.), y - px(7.)),
                        TextAlign::Right,
                        t.dim,
                        window,
                        cx,
                    );
                }
                v += step;
            }
            for d in 0..n {
                let x = mid(d) - bw / 2.;
                let (o, c) = (tl.opened[d], tl.closed[d]);
                if o > 0 {
                    window.paint_quad(rounded(
                        fill(
                            Bounds::new(point(x, mid_y - h(o)), size(bw, h(o))),
                            ink(opened_ink, d),
                        ),
                        true,
                    ));
                }
                if c > 0 {
                    window.paint_quad(rounded(
                        fill(Bounds::new(point(x, mid_y), size(bw, h(c))), ink(t.done, d)),
                        false,
                    ));
                }
            }
            window.paint_quad(fill(
                Bounds::new(
                    point(flow.origin.x, mid_y),
                    size(flow.size.width, px(HAIRLINE)),
                ),
                t.divider,
            ));
            // Each band says what it shows, at its right end.
            let end = flow.origin.x + flow.size.width + px(6.);
            for (words, y) in [
                ("open", level.origin.y - px(6.)),
                ("opened", mid_y - px(16.)),
                ("finished", mid_y + px(2.)),
            ] {
                text(words, point(end, y), TextAlign::Left, t.dim, window, cx);
            }
        }
    }

    // The day axis: a rule, and a day's name as often as there is room,
    // counted back from today so today is always named.
    let floor = plot.origin.y + plot.size.height;
    if p.shape == Shape::Strip {
        window.paint_quad(fill(
            Bounds::new(
                point(plot.origin.x, floor),
                size(plot.size.width, px(HAIRLINE)),
            ),
            t.divider,
        ));
    }
    let every = ((px(LABEL_EVERY) / day_w).ceil() as usize).max(1);
    let label_y = b.origin.y + b.size.height - px(AXIS_BOTTOM - 4.);
    let mut d = n as i64 - 1;
    while d >= 0 {
        text(
            &day_label(tl.first + d),
            point(mid(d as usize), label_y),
            TextAlign::Center,
            t.dim,
            window,
            cx,
        );
        d -= every as i64;
    }

    // The day under the mouse: a guide through it, a mark on the open
    // line, and a plate saying the day's numbers beside it.
    if let Some(d) = p.hover.and_then(|day| tl.index(day)) {
        let x = mid(d);
        window.paint_quad(fill(
            Bounds::new(
                point(x - px(HAIRLINE / 2.), plot.origin.y - px(BRUSH_RISE)),
                size(px(HAIRLINE), plot.size.height + px(BRUSH_RISE)),
            ),
            alpha(t.text2, GUIDE),
        ));
        if most_open > 0 {
            let y = level_y(tl.open[d]);
            let r = px(DOT / 2.);
            window.paint_quad(
                fill(
                    Bounds::new(point(x - r, y - r), size(px(DOT), px(DOT))),
                    t.surface,
                )
                .corner_radii(r)
                .border_widths(px(STROKE))
                .border_color(t.chart),
            );
        }
        plate(t, tl, d, x, plot, window, cx);
    }
}

/// The hovered day's numbers on a raised plate at the plot's top, beside
/// the guide, on whichever side has the room.
#[allow(clippy::too_many_arguments)]
fn plate(
    t: &Theme,
    tl: &Timeline,
    d: usize,
    x: Pixels,
    plot: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let opened = tl.opened[d].to_string();
    let closed = tl.closed[d].to_string();
    let open = tl.open[d].to_string();
    let day = day_label(tl.first + d as i64);
    let parts: [(&str, Hsla); 8] = [
        (&day, t.text),
        ("   ", t.dim),
        (&opened, tokens::opened(t)),
        (" opened  ", t.dim),
        (&closed, t.done),
        (" finished  ", t.dim),
        (&open, t.text),
        (" open", t.dim),
    ];
    let lines: Vec<gpui::ShapedLine> = parts
        .iter()
        .map(|(w, ink)| shaped(w, *ink, window))
        .collect();
    let words: Pixels = lines.iter().map(|l| l.width).sum();
    let w = words + px(PLATE_PAD * 2.);
    let h = px(PLATE_H);
    let left = if x + px(PLATE_GAP) + w <= plot.origin.x + plot.size.width {
        x + px(PLATE_GAP)
    } else {
        x - px(PLATE_GAP) - w
    };
    // In the lane over the plot, clear of the bars.
    let top = plot.origin.y - px(AXIS_TOP) + px((AXIS_TOP - PLATE_H) / 2. - 2.);
    window.paint_quad(
        fill(Bounds::new(point(left, top), size(w, h)), t.raised)
            .corner_radii(px(kit::RADIUS))
            .border_widths(px(HAIRLINE))
            .border_color(t.divider),
    );
    let mut at = left + px(PLATE_PAD);
    let y = top + (px(PLATE_H) - px(kit::SMALL + 3.)) / 2.;
    for line in lines {
        let width = line.width;
        let _ = line.paint(
            point(at, y),
            px(kit::SMALL + 3.),
            TextAlign::Left,
            None,
            window,
            cx,
        );
        at += width;
    }
}

/// Gridlines at each step to `top` across `band`, labelled on the left.
fn gridlines(
    t: &Theme,
    band: Bounds<Pixels>,
    top: u32,
    step: u32,
    grid: Hsla,
    window: &mut Window,
    cx: &mut App,
) {
    let mut v = step;
    while v <= top {
        let y = band.origin.y + band.size.height * (1. - v as f32 / top as f32);
        rule(band, y, grid, window);
        text(
            &v.to_string(),
            point(band.origin.x - px(6.), y - px(7.)),
            TextAlign::Right,
            t.dim,
            window,
            cx,
        );
        v += step;
    }
}

fn rule(band: Bounds<Pixels>, y: Pixels, ink: Hsla, window: &mut Window) {
    window.paint_quad(fill(
        Bounds::new(point(band.origin.x, y), size(band.size.width, px(HAIRLINE))),
        ink,
    ));
}

/// Words shaped in the chart's small type.
fn shaped(words: &str, ink: Hsla, window: &mut Window) -> gpui::ShapedLine {
    let run = gpui::TextRun {
        len: words.len(),
        font: gpui::font(FAMILY),
        color: ink,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    window
        .text_system()
        .shape_line(words.to_string().into(), px(kit::SMALL), &[run], None)
}

/// One small label. Right- and centre-aligned text ends at, or centres
/// on, `at.x`.
fn text(
    words: &str,
    at: Point<Pixels>,
    align: TextAlign,
    ink: Hsla,
    window: &mut Window,
    cx: &mut App,
) {
    let line = shaped(words, ink, window);
    let x = match align {
        TextAlign::Left => at.x,
        TextAlign::Right => at.x - line.width,
        TextAlign::Center => at.x - line.width / 2.,
    };
    let _ = line.paint(
        point(x, at.y),
        px(kit::SMALL + 3.),
        TextAlign::Left,
        None,
        window,
        cx,
    );
}

/// The mouse over the plot: a press starts a range, a drag stretches it
/// (beyond the plot too, held to its days), a release ends it; moving
/// names the day under it.
fn follow(tl: &Rc<Timeline>, plot: Bounds<Pixels>, page: WeakEntity<Issue>, window: &mut Window) {
    let hit = Bounds::new(
        point(plot.origin.x, plot.origin.y - px(AXIS_TOP)),
        size(
            plot.size.width,
            plot.size.height + px(AXIS_TOP + AXIS_BOTTOM),
        ),
    );
    let (down, moving, up) = (tl.clone(), tl.clone(), page.clone());
    let pressed = page.clone();
    window.on_mouse_event(move |e: &MouseDownEvent, phase, _, cx| {
        if phase != DispatchPhase::Bubble || e.button != MouseButton::Left {
            return;
        }
        if hit.contains(&e.position) && !down.opened.is_empty() {
            let day = day_at(&down, plot, e.position.x);
            let _ = pressed.update(cx, |w, cx| w.range_press(day, cx));
        }
    });
    window.on_mouse_event(move |e: &MouseMoveEvent, phase, _, cx| {
        if phase != DispatchPhase::Bubble {
            return;
        }
        let day = day_at(&moving, plot, e.position.x);
        let over = hit.contains(&e.position).then_some(day);
        let held = e.pressed_button == Some(MouseButton::Left);
        let _ = page.update(cx, |w, cx| w.range_move(over, held.then_some(day), cx));
    });
    window.on_mouse_event(move |e: &MouseUpEvent, phase, _, cx| {
        if phase != DispatchPhase::Bubble || e.button != MouseButton::Left {
            return;
        }
        let _ = up.update(cx, |w, cx| w.range_release(cx));
    });
}

#[cfg(test)]
mod tests {
    use super::nice;

    #[test]
    fn an_axis_tops_out_on_a_round_number() {
        assert_eq!(nice(0), (1, 1));
        assert_eq!(nice(3), (3, 1));
        assert_eq!(nice(7), (10, 5));
        assert_eq!(nice(47), (60, 20));
        assert_eq!(nice(512), (600, 200));
    }
}
