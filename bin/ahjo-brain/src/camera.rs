//! The camera over the sky, and the flights between two views of it.
//!
//! A screen point is `world * scale + (x, y)`. A view is described for a
//! flight by the world point at the centre of the part of the window the sky
//! shows in (`View`, which leaves out a reader or a column over it) and how
//! much world fits across it, so a flight lands its target in the middle of
//! what is visible, not under the reader.
//!
//! A long flight rises: it zooms out on the way and back in on arrival (van
//! Wijk and Nuij, "Smooth and efficient zooming and panning", 2003), so the
//! operator sees where he is being taken and from where.

use crate::sky::Rect;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub x: f32,
    pub y: f32,
    pub scale: f32,
}

pub const MIN_SCALE: f32 = 0.05;
pub const MAX_SCALE: f32 = 8.;

impl Camera {
    pub fn screen(&self, wx: f32, wy: f32) -> (f32, f32) {
        (wx * self.scale + self.x, wy * self.scale + self.y)
    }

    pub fn world(&self, sx: f32, sy: f32) -> (f32, f32) {
        ((sx - self.x) / self.scale, (sy - self.y) / self.scale)
    }

    /// Zoomed by `factor` about a screen point, which stays where it is.
    pub fn zoom_about(&self, sx: f32, sy: f32, factor: f32) -> Camera {
        let scale = (self.scale * factor).clamp(MIN_SCALE, MAX_SCALE);
        let (wx, wy) = self.world(sx, sy);
        Camera {
            x: sx - wx * scale,
            y: sy - wy * scale,
            scale,
        }
    }

    pub fn pan(&self, dx: f32, dy: f32) -> Camera {
        Camera {
            x: self.x + dx,
            y: self.y + dy,
            ..*self
        }
    }

    /// `world` at the centre of `view`, at `scale`.
    pub fn centre(wx: f32, wy: f32, scale: f32, view: Rect) -> Camera {
        let scale = scale.clamp(MIN_SCALE, MAX_SCALE);
        let (cx, cy) = view.centre();
        Camera {
            x: cx - wx * scale,
            y: cy - wy * scale,
            scale,
        }
    }

    /// `world` fitted inside `view` with `pad` to spare, no closer than `most`.
    pub fn fit(world: Rect, view: Rect, most: f32, pad: f32) -> Camera {
        let scale = ((view.w - pad * 2.) / world.w.max(1.))
            .min((view.h - pad * 2.) / world.h.max(1.))
            .min(most);
        let (wx, wy) = world.centre();
        Camera::centre(wx, wy, scale, view)
    }

    /// The world point at the centre of `view`, and the world width across it.
    fn seen(&self, view: Rect) -> (f32, f32, f32) {
        let (cx, cy) = view.centre();
        let (wx, wy) = self.world(cx, cy);
        (wx, wy, view.w / self.scale)
    }
}

/// A flight from one camera to another over `view`.
#[derive(Clone, Copy, Debug)]
pub struct Flight {
    from: (f32, f32, f32),
    to: (f32, f32, f32),
    view: Rect,
    /// The path's length in van Wijk and Nuij's units, and its shape.
    length: f32,
    r0: f32,
    distance: f32,
}

/// How far the flight rises: √2 is the paper's recommended balance between
/// panning and zooming.
const RHO: f32 = std::f32::consts::SQRT_2;

impl Flight {
    pub fn new(from: Camera, to: Camera, view: Rect) -> Flight {
        let from = from.seen(view);
        let to = to.seen(view);
        let (w0, w1) = (from.2.max(1e-3), to.2.max(1e-3));
        let distance = (to.0 - from.0).hypot(to.1 - from.1);
        let (length, r0) = if distance < 1e-3 {
            ((w1 / w0).ln().abs() / RHO, 0.)
        } else {
            let b = |w: f32, sign: f32| {
                (w1 * w1 - w0 * w0 + sign * RHO.powi(4) * distance * distance)
                    / (2. * w * RHO * RHO * distance)
            };
            let r = |b: f32| (-b + (b * b + 1.).sqrt()).ln();
            let r0 = r(b(w0, 1.));
            let r1 = r(b(w1, -1.));
            ((r1 - r0) / RHO, r0)
        };
        Flight {
            from,
            to,
            view,
            length,
            r0,
            distance,
        }
    }

    /// How long the flight takes: longer for a longer path, within bounds.
    pub fn millis(&self, base: f32, per_unit: f32, most: f32) -> f32 {
        (base + self.length.abs() * per_unit).min(most)
    }

    /// The camera `t` of the way (0..1) along the flight.
    pub fn at(&self, t: f32) -> Camera {
        let t = t.clamp(0., 1.);
        let (w0, w1) = (self.from.2.max(1e-3), self.to.2.max(1e-3));
        let s = t * self.length;
        let (u, w) = if self.distance < 1e-3 {
            (0., w0 * (w1 / w0).powf(t))
        } else {
            let r0 = self.r0;
            let u = w0 / (RHO * RHO) * (r0.cosh() * (RHO * s + r0).tanh() - r0.sinh());
            let w = w0 * r0.cosh() / (RHO * s + r0).cosh();
            (u / self.distance, w)
        };
        let (wx, wy) = (
            self.from.0 + (self.to.0 - self.from.0) * u,
            self.from.1 + (self.to.1 - self.from.1) * u,
        );
        let w = if t >= 1. { w1 } else { w };
        Camera::centre(wx, wy, self.view.w / w, self.view)
    }
}

/// Slow in and out, for flights.
pub fn ease_in_out(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    if t < 0.5 {
        4. * t * t * t
    } else {
        1. - (-2. * t + 2.).powi(3) / 2.
    }
}

/// Fast out, slow in, for things settling into place.
pub fn ease_out(t: f32) -> f32 {
    1. - (1. - t.clamp(0., 1.)).powi(3)
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEW: Rect = Rect {
        x: 0.,
        y: 0.,
        w: 800.,
        h: 600.,
    };

    fn close(a: Camera, b: Camera) -> bool {
        (a.x - b.x).abs() < 0.5 && (a.y - b.y).abs() < 0.5 && (a.scale - b.scale).abs() < 1e-3
    }

    #[test]
    fn zooming_keeps_the_point_under_the_pointer() {
        let c = Camera {
            x: 10.,
            y: 20.,
            scale: 1.,
        };
        let z = c.zoom_about(300., 200., 2.);
        let (a, b) = c.world(300., 200.);
        let (x, y) = z.screen(a, b);
        assert!((x - 300.).abs() < 1e-3 && (y - 200.).abs() < 1e-3);
    }

    #[test]
    fn a_flight_starts_and_ends_where_asked_and_rises_between() {
        let a = Camera::centre(0., 0., 2., VIEW);
        let b = Camera::centre(3000., 0., 2., VIEW);
        let f = Flight::new(a, b, VIEW);
        assert!(close(f.at(0.), a));
        assert!(close(f.at(1.), b));
        assert!(f.at(0.5).scale < 0.5, "{}", f.at(0.5).scale);
        // A flight that only zooms stays over the same point.
        let c = Camera::centre(0., 0., 0.5, VIEW);
        let z = Flight::new(a, c, VIEW);
        let mid = z.at(0.5);
        let (wx, wy) = mid.world(400., 300.);
        assert!(wx.abs() < 1e-2 && wy.abs() < 1e-2);
        assert!(mid.scale < 2. && mid.scale > 0.5);
    }

    #[test]
    fn a_fit_takes_the_whole_rect_in() {
        let world = Rect {
            x: -500.,
            y: -100.,
            w: 1000.,
            h: 200.,
        };
        let c = Camera::fit(world, VIEW, 10., 20.);
        let (l, _) = c.screen(-500., 0.);
        let (r, _) = c.screen(500., 0.);
        assert!((l - 20.).abs() < 1e-2 && (r - 780.).abs() < 1e-2);
    }
}
