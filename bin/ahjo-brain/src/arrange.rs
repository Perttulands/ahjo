//! Where stars go when they leave the sky: the orbit a page read stands at
//! the centre of. Plain geometry in screen space, worked out again each
//! frame from the window, so a resize simply moves it.

use crate::corpus::Corpus;
use crate::sky::{Rect, Sky};
use crate::tokens::orbit;
use std::f32::consts::{PI, TAU};

#[derive(Clone, Debug, PartialEq)]
pub struct Planet {
    pub page: usize,
    pub x: f32,
    pub y: f32,
    pub angle: f32,
    /// It links to the sun (else the sun links to it, or both).
    pub inbound: bool,
}

/// A subject's stretch of the ring, to name it.
#[derive(Clone, Debug, PartialEq)]
pub struct Arc {
    pub system: usize,
    pub from: f32,
    pub to: f32,
    pub count: usize,
}

pub struct Orbit {
    pub sun: usize,
    pub centre: (f32, f32),
    pub radius: f32,
    pub planets: Vec<Planet>,
    pub arcs: Vec<Arc>,
}

/// An orbit's pages in the order they stand round the ring, clockwise
/// from the top: the order the arrow keys walk them.
pub fn clockwise(o: &Orbit) -> Vec<usize> {
    let top = -std::f32::consts::FRAC_PI_2;
    let mut ring: Vec<(f32, usize)> = o
        .planets
        .iter()
        .map(|p| ((p.angle - top).rem_euclid(std::f32::consts::TAU), p.page))
        .collect();
    ring.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    ring.into_iter().map(|p| p.1).collect()
}

/// The page `sun` at the centre of `view`, the pages it links and that link
/// to it around it, gathered into one arc per subject; its evidence too
/// unless `evidence` is folded away.
///
/// Each arc points the way its subject lies in the sky, as nearly as the
/// arcs' widths allow, so north-east in the orbit is north-east in the sky.
/// An arc's width is its share of the planets.
pub fn orbit(corpus: &Corpus, sky: &Sky, sun: usize, view: Rect, evidence: bool) -> Orbit {
    let centre = view.centre();
    let here = sky.stars[sun];
    let mut groups: Vec<(usize, Vec<usize>, f32)> = vec![];
    for p in corpus.neighbours(sun) {
        if !evidence && corpus.pages[p].is_evidence() {
            continue;
        }
        let s = sky.stars[p].system;
        match groups.iter_mut().find(|g| g.0 == s) {
            Some(g) => g.1.push(p),
            None => groups.push((s, vec![p], 0.)),
        }
    }
    for g in &mut groups {
        let n = g.1.len() as f32;
        let (mx, my) = g.1.iter().fold((0., 0.), |(x, y), &p| {
            (x + sky.stars[p].x / n, y + sky.stars[p].y / n)
        });
        g.2 = (my - here.y).atan2(mx - here.x);
        g.1.sort_by(|&a, &b| {
            sky.degree[b]
                .cmp(&sky.degree[a])
                .then(corpus.pages[a].title.cmp(&corpus.pages[b].title))
        });
    }
    groups.sort_by(|a, b| a.2.total_cmp(&b.2).then(a.0.cmp(&b.0)));
    let total: usize = groups.iter().map(|g| g.1.len()).sum();
    // The outermost ring leaves room beside it for the planets' names.
    let outer = (view.w / 2. - orbit::ROOM_X)
        .min(view.h / 2. - orbit::ROOM_Y)
        .max(60.);
    let radius = if total > orbit::CROWD {
        outer / orbit::OUTER
    } else {
        outer
    };
    if total == 0 {
        return Orbit {
            sun,
            centre,
            radius,
            planets: vec![],
            arcs: vec![],
        };
    }
    let gaps = if groups.len() > 1 {
        orbit::ARC_GAP * groups.len() as f32
    } else {
        0.
    };
    let room = TAU - gaps.min(PI);
    let gap = if groups.len() > 1 {
        (TAU - room) / groups.len() as f32
    } else {
        0.
    };
    // Lay the arcs out in order from zero, then turn the whole ring so the
    // arcs sit as near their subjects' directions as they can.
    let mut starts = vec![];
    let mut at = 0.;
    for g in &groups {
        let width = room * g.1.len() as f32 / total as f32;
        starts.push((at, width));
        at += width + gap;
    }
    let (mut sx, mut sy) = (0., 0.);
    for (g, (start, width)) in groups.iter().zip(&starts) {
        let d = g.2 - (start + width / 2.);
        let w = g.1.len() as f32;
        sx += d.cos() * w;
        sy += d.sin() * w;
    }
    let turn = sy.atan2(sx);
    let crowded = total > orbit::CROWD;
    let mut planets = vec![];
    let mut arcs = vec![];
    for (g, (start, width)) in groups.iter().zip(&starts) {
        let from = start + turn;
        let n = g.1.len();
        for (k, &p) in g.1.iter().enumerate() {
            let a = from + width * (k as f32 + 0.5) / n as f32;
            let r = if crowded && k % 2 == 1 {
                radius * orbit::OUTER
            } else {
                radius
            };
            planets.push(Planet {
                page: p,
                x: centre.0 + a.cos() * r,
                y: centre.1 + a.sin() * r,
                angle: a,
                inbound: corpus.inc[sun].contains(&p),
            });
        }
        arcs.push(Arc {
            system: g.0,
            from,
            to: from + width,
            count: n,
        });
    }
    Orbit {
        sun,
        centre,
        radius,
        planets,
        arcs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus::parse;
    use crate::sky::{Placement, layout};

    #[test]
    fn evidence_folds_out_of_the_ring() {
        let corpus = crate::corpus::Corpus::new(vec![
            parse("knowledge/hub/hub.md", "# Hub\n[[a1]] [[s1]] [[s2]]"),
            parse("knowledge/hub/a1.md", "# A1"),
            parse("evidence/sources/hub/s1.md", "---\nkind: source\n---\n# S1"),
            parse("evidence/sources/hub/s2.md", "---\nkind: source\n---\n# S2"),
        ]);
        let sky = layout(&corpus, &Placement::new());
        let hub = corpus.by_path("knowledge/hub/hub.md").unwrap();
        let view = Rect {
            x: 0.,
            y: 0.,
            w: 800.,
            h: 600.,
        };
        let open = orbit(&corpus, &sky, hub, view, true);
        assert_eq!(open.planets.len(), 3);
        let folded = orbit(&corpus, &sky, hub, view, false);
        assert_eq!(folded.planets.len(), 1);
        // The arrow keys walk every planet once, from the top round.
        let ring = clockwise(&open);
        assert_eq!(ring.len(), 3);
        let angle = |page: usize| {
            let p = open.planets.iter().find(|p| p.page == page).unwrap();
            (p.angle + std::f32::consts::FRAC_PI_2).rem_euclid(std::f32::consts::TAU)
        };
        assert!(ring.windows(2).all(|w| angle(w[0]) <= angle(w[1])));
    }

    #[test]
    fn every_neighbour_is_one_planet_and_arcs_point_at_their_subjects() {
        let mut pages = vec![parse("knowledge/hub/hub.md", "# Hub\n[[a1]] [[a2]] [[b1]]")];
        pages.push(parse("knowledge/a/a1.md", "# A1\n[[a2]]"));
        pages.push(parse("knowledge/a/a2.md", "# A2\n[[a1]]"));
        pages.push(parse("knowledge/b/b1.md", "# B1\n[[hub]]"));
        let corpus = crate::corpus::Corpus::new(pages);
        let sky = layout(&corpus, &Placement::new());
        let hub = corpus.by_path("knowledge/hub/hub.md").unwrap();
        let view = Rect {
            x: 0.,
            y: 0.,
            w: 800.,
            h: 600.,
        };
        let o = orbit(&corpus, &sky, hub, view, true);
        assert_eq!(o.planets.len(), 3);
        assert_eq!(o.arcs.len(), 2);
        let b1 = corpus.by_path("knowledge/b/b1.md").unwrap();
        let planet = o.planets.iter().find(|p| p.page == b1).unwrap();
        assert!(planet.inbound);
        // The ring is turned so the arcs, weighted by their planets, point
        // the way their subjects lie from the hub: no other turn does better.
        let h = sky.stars[hub];
        let fit = |turn: f32| -> f32 {
            o.arcs
                .iter()
                .map(|arc| {
                    let members: Vec<_> = o
                        .planets
                        .iter()
                        .filter(|p| sky.stars[p.page].system == arc.system)
                        .collect();
                    let n = members.len() as f32;
                    let (mx, my) = members.iter().fold((0., 0.), |(x, y), p| {
                        (x + sky.stars[p.page].x / n, y + sky.stars[p.page].y / n)
                    });
                    let want = (my - h.y).atan2(mx - h.x);
                    n * (want - ((arc.from + arc.to) / 2. + turn)).cos()
                })
                .sum()
        };
        let best = fit(0.);
        for i in 1..64 {
            assert!(fit(i as f32 / 64. * TAU) <= best + 1e-3);
        }
        for p in &o.planets {
            let r = (p.x - o.centre.0).hypot(p.y - o.centre.1);
            assert!((r - o.radius).abs() < 0.5);
        }
    }
}
