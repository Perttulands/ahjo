//! One frame of the sky, painted.
//!
//! The scene (`scene.rs`) works out what every star is doing (where it is,
//! how lit, what is pointed at) and hands this a flat picture in screen
//! space; this only paints, on the GPU: a star is a round quad, light is a
//! blurred shadow around one, a line is a tessellated curve and a name is a
//! shaped line of text.

use ahjo_app_kit::theme::{FAMILY, Theme};
use gpui::{
    App, BorderStyle, Bounds, BoxShadow, Corners, Edges, FontWeight, Hsla, PathBuilder, Pixels,
    SharedString, TextAlign, TextRun, Window, font, point, px, quad, size, transparent_black,
};

#[derive(Clone, Debug)]
pub struct Light {
    pub x: f32,
    pub y: f32,
    /// The lit disc's radius, and how far its light spreads past it.
    pub r: f32,
    pub blur: f32,
    pub color: Hsla,
}

#[derive(Clone, Debug)]
pub struct Ring {
    pub x: f32,
    pub y: f32,
    pub r: f32,
    pub color: Hsla,
    pub width: f32,
    pub dashed: bool,
}

#[derive(Clone, Debug)]
pub struct Line {
    pub a: (f32, f32),
    pub b: (f32, f32),
    /// Sideways bend as a share of the length, so a line reads as an arc.
    pub bend: f32,
    pub color: Hsla,
    pub width: f32,
    /// How much of it is drawn from `a`, for a trace growing outward.
    pub reach: f32,
}

/// A stretch of a circle, from angle `from` to `to`.
#[derive(Clone, Debug)]
pub struct Bow {
    pub x: f32,
    pub y: f32,
    pub r: f32,
    pub from: f32,
    pub to: f32,
    pub color: Hsla,
    pub width: f32,
}

#[derive(Clone, Copy, Debug)]
pub enum Shape {
    Round,
    Square,
}

#[derive(Clone, Debug)]
pub struct Dot {
    pub x: f32,
    pub y: f32,
    pub r: f32,
    pub color: Hsla,
    pub shape: Shape,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Align {
    Left,
    Centre,
}

/// The letters a name is set in.
#[derive(Clone, Debug, PartialEq)]
pub struct Face {
    pub family: SharedString,
    pub weight: FontWeight,
}

impl Face {
    /// The app's own letters, for a page's name.
    pub fn mono(bold: bool) -> Face {
        Face {
            family: FAMILY.into(),
            weight: if bold {
                FontWeight::BOLD
            } else {
                FontWeight::NORMAL
            },
        }
    }

    /// The theme's display face, for a subject's name over the sky.
    pub fn display(t: &Theme) -> Face {
        Face {
            family: t.ty.display.into(),
            weight: t.ty.display_weight,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Label {
    pub text: SharedString,
    pub x: f32,
    pub y: f32,
    pub size: f32,
    pub color: Hsla,
    pub face: Face,
    pub align: Align,
}

#[derive(Default)]
pub struct Picture {
    pub lights: Vec<Light>,
    pub rims: Vec<Ring>,
    pub lines: Vec<Line>,
    pub bows: Vec<Bow>,
    pub glows: Vec<Light>,
    pub dots: Vec<Dot>,
    pub rings: Vec<Ring>,
    pub labels: Vec<Label>,
    /// The dark behind every name, so it reads over stars and lines: its
    /// colour, how soft and how far it spreads.
    pub halo: Hsla,
    pub halo_blur: f32,
    pub halo_spread: f32,
}

fn bounds(x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
    Bounds::new(point(px(x), px(y)), size(px(w), px(h)))
}

fn light(window: &mut Window, l: &Light) {
    let b = bounds(l.x - l.r, l.y - l.r, l.r * 2., l.r * 2.);
    window.paint_drop_shadows(
        b,
        Corners::all(px(l.r)),
        &[BoxShadow {
            color: l.color,
            offset: point(px(0.), px(0.)),
            blur_radius: px(l.blur),
            spread_radius: px(0.),
            inset: false,
        }],
    );
}

fn ring(window: &mut Window, r: &Ring) {
    if r.dashed {
        let mut path = PathBuilder::stroke(px(r.width)).dash_array(&[px(1.5), px(5.)]);
        let steps = ((r.r * 0.5) as usize).clamp(24, 160);
        for i in 0..=steps {
            let a = i as f32 / steps as f32 * std::f32::consts::TAU;
            let p = point(px(r.x + a.cos() * r.r), px(r.y + a.sin() * r.r));
            if i == 0 {
                path.move_to(p);
            } else {
                path.line_to(p);
            }
        }
        if let Ok(path) = path.build() {
            window.paint_path(path, r.color);
        }
        return;
    }
    window.paint_quad(quad(
        bounds(r.x - r.r, r.y - r.r, r.r * 2., r.r * 2.),
        Corners::all(px(r.r)),
        transparent_black(),
        Edges::all(px(r.width)),
        r.color,
        BorderStyle::Solid,
    ));
}

/// A quadratic curve's point at `t`.
fn curve(a: (f32, f32), c: (f32, f32), b: (f32, f32), t: f32) -> (f32, f32) {
    let u = 1. - t;
    (
        u * u * a.0 + 2. * u * t * c.0 + t * t * b.0,
        u * u * a.1 + 2. * u * t * c.1 + t * t * b.1,
    )
}

fn line(window: &mut Window, l: &Line) {
    let (dx, dy) = (l.b.0 - l.a.0, l.b.1 - l.a.1);
    if dx.hypot(dy) < 1. || l.reach <= 0. {
        return;
    }
    let c = (l.a.0 + dx / 2. - dy * l.bend, l.a.1 + dy / 2. + dx * l.bend);
    let mut path = PathBuilder::stroke(px(l.width));
    path.move_to(point(px(l.a.0), px(l.a.1)));
    if l.reach >= 1. {
        path.curve_to(point(px(l.b.0), px(l.b.1)), point(px(c.0), px(c.1)));
    } else {
        // The part drawn so far, as its own smaller curve.
        let t = l.reach;
        let q = (l.a.0 + (c.0 - l.a.0) * t, l.a.1 + (c.1 - l.a.1) * t);
        let end = curve(l.a, c, l.b, t);
        path.curve_to(point(px(end.0), px(end.1)), point(px(q.0), px(q.1)));
    }
    if let Ok(path) = path.build() {
        window.paint_path(path, l.color);
    }
}

fn bow(window: &mut Window, b: &Bow) {
    let steps = (((b.to - b.from).abs() * b.r / 4.) as usize).clamp(2, 120);
    let mut path = PathBuilder::stroke(px(b.width));
    for i in 0..=steps {
        let a = b.from + (b.to - b.from) * i as f32 / steps as f32;
        let p = point(px(b.x + a.cos() * b.r), px(b.y + a.sin() * b.r));
        if i == 0 {
            path.move_to(p);
        } else {
            path.line_to(p);
        }
    }
    if let Ok(path) = path.build() {
        window.paint_path(path, b.color);
    }
}

/// Where a name of `width` starts for its alignment at `x`.
pub fn start_of(align: Align, x: f32, width: f32) -> f32 {
    match align {
        Align::Left => x,
        Align::Centre => x - width / 2.,
    }
}

/// The width a name will take, for placing names apart before painting.
pub fn measure(text: &str, size: f32, face: &Face, window: &Window) -> f32 {
    f32::from(shape(text, size, transparent_black(), face, window).width)
}

fn shape(text: &str, size: f32, color: Hsla, face: &Face, window: &Window) -> gpui::ShapedLine {
    let mut f = font(face.family.clone());
    f.weight = face.weight;
    let text: SharedString = text.to_string().into();
    window.text_system().shape_line(
        text.clone(),
        px(size),
        &[TextRun {
            len: text.len(),
            font: f,
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        }],
        None,
    )
}

pub fn paint(picture: &Picture, window: &mut Window, cx: &mut App) {
    for l in &picture.lights {
        light(window, l);
    }
    for r in &picture.rims {
        ring(window, r);
    }
    for l in &picture.lines {
        line(window, l);
    }
    for b in &picture.bows {
        bow(window, b);
    }
    for g in &picture.glows {
        light(window, g);
    }
    for d in &picture.dots {
        let b = bounds(d.x - d.r, d.y - d.r, d.r * 2., d.r * 2.);
        let corners = match d.shape {
            Shape::Round => Corners::all(px(d.r)),
            Shape::Square => Corners::all(px(0.)),
        };
        window.paint_quad(quad(
            b,
            corners,
            d.color,
            Edges::all(px(0.)),
            transparent_black(),
            BorderStyle::Solid,
        ));
    }
    for r in &picture.rings {
        ring(window, r);
    }
    let shaped: Vec<_> = picture
        .labels
        .iter()
        .map(|l| shape(&l.text, l.size, l.color, &l.face, window))
        .collect();
    for (l, s) in picture.labels.iter().zip(&shaped) {
        let w = f32::from(s.width);
        let x = start_of(l.align, l.x, w);
        window.paint_drop_shadows(
            bounds(x, l.y - l.size * 0.45, w, l.size * 0.9),
            // A soft halo is round; a hard one is a plate, square as a sign.
            Corners::all(px(if picture.halo_blur > 0. {
                l.size * 0.45
            } else {
                0.
            })),
            &[BoxShadow {
                color: Hsla {
                    a: picture.halo.a * l.color.a,
                    ..picture.halo
                },
                offset: point(px(0.), px(0.)),
                blur_radius: px(picture.halo_blur),
                spread_radius: px(picture.halo_spread),
                inset: false,
            }],
        );
    }
    for (l, shaped) in picture.labels.iter().zip(shaped) {
        let x = start_of(l.align, l.x, f32::from(shaped.width));
        let line_height = px(l.size * 1.3);
        let _ = shaped.paint(
            point(px(x), px(l.y - l.size * 0.65)),
            line_height,
            TextAlign::Left,
            None,
            window,
            cx,
        );
    }
}
