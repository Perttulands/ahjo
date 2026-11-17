//! Where everything sits in the night sky of the brain.
//!
//! A subject is a star system. Its topic page is the bright star at its
//! centre; the research and notes that link somewhere orbit it, the better
//! connected the closer in; its evidence is the dust around them, so how much
//! a subject rests on shows as how much nebula it has. A page that links
//! nowhere has drifted out past its system's edge, which says someone should
//! tidy it.
//!
//! Systems are placed by their links: two subjects that cite each other are
//! pulled together and every pair keeps its distance, so related work forms
//! constellations. The arithmetic is a fixed number of steps from a fixed
//! start, with no randomness, so the same brain is always the same sky.
//!
//! A growing brain would still reshuffle a sky placed from scratch, so the
//! places of the last sky are passed back in: its systems stay where they
//! were, a newcomer joins the systems it links to, and only overlaps settle.
//! (The same model as CHROTE's web sky, `skyLayout.ts`, which earned it.)

use crate::corpus::Corpus;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Core,
    Star,
    Dust,
    Stray,
}

#[derive(Clone, Debug)]
pub struct System {
    pub subject: usize,
    pub x: f32,
    pub y: f32,
    pub radius: f32,
    pub core: Option<usize>,
    pub pages: usize,
    pub evidence: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct Star {
    pub system: usize,
    pub role: Role,
    pub x: f32,
    pub y: f32,
}

/// How many links run between two systems.
#[derive(Clone, Copy, Debug)]
pub struct Bridge {
    pub a: usize,
    pub b: usize,
    pub weight: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn centre(&self) -> (f32, f32) {
        (self.x + self.w / 2., self.y + self.h / 2.)
    }
    pub fn around(points: impl Iterator<Item = (f32, f32)>, pad: f32) -> Option<Rect> {
        let mut r: Option<(f32, f32, f32, f32)> = None;
        for (x, y) in points {
            r = Some(match r {
                None => (x, y, x, y),
                Some((a, b, c, d)) => (a.min(x), b.min(y), c.max(x), d.max(y)),
            });
        }
        r.map(|(a, b, c, d)| Rect {
            x: a - pad,
            y: b - pad,
            w: c - a + pad * 2.,
            h: d - b + pad * 2.,
        })
    }
}

pub struct Sky {
    /// One per subject, in the corpus's subject order.
    pub systems: Vec<System>,
    /// One per page, in the corpus's page order.
    pub stars: Vec<Star>,
    pub bridges: Vec<Bridge>,
    pub bounds: Rect,
    /// Each page's links in and out, for its size and its orbit.
    pub degree: Vec<u32>,
}

const GOLDEN: f32 = 2.399_963_3;
/// The space kept between two systems: room for strays and a name.
pub const GAP: f32 = 54.;
const STEPS: usize = 420;

/// A stable number in 0..1 from a string, so placement never depends on order.
pub fn hash(text: &str) -> f32 {
    let mut v: u32 = 2_166_136_261;
    for b in text.bytes() {
        v ^= b as u32;
        v = v.wrapping_mul(16_777_619);
    }
    v as f32 / 4_294_967_296.
}

fn radius_of(maintained: usize, evidence: usize) -> f32 {
    34. + (maintained as f32).sqrt() * 10. + (evidence as f32).sqrt() * 7.
}

pub type Placement = HashMap<String, (f32, f32)>;

pub fn layout(corpus: &Corpus, previous: &Placement) -> Sky {
    let n = corpus.pages.len();
    let mut degree = vec![0u32; n];
    let mut between: HashMap<(usize, usize), u32> = HashMap::new();
    for &(a, b) in &corpus.links {
        degree[a] += 1;
        degree[b] += 1;
        let (sa, sb) = (corpus.pages[a].subject, corpus.pages[b].subject);
        if sa != sb {
            *between.entry((sa.min(sb), sa.max(sb))).or_default() += 1;
        }
    }
    let mut bridges: Vec<Bridge> = between
        .into_iter()
        .map(|((a, b), weight)| Bridge { a, b, weight })
        .collect();
    bridges.sort_by_key(|b| (b.a, b.b));

    let linked = |p: usize| degree[p] > 0;
    let mut systems: Vec<System> = corpus
        .subjects
        .iter()
        .enumerate()
        .map(|(s, subject)| {
            let name = subject.id.rsplit('/').next().unwrap_or(&subject.id);
            let pages = &subject.pages;
            let ev = |p: usize| corpus.pages[p].is_evidence();
            let topic = |p: usize| corpus.pages[p].kind == "topic";
            let named = |p: usize| corpus.pages[p].slug == name;
            let candidates: Vec<usize> = pages
                .iter()
                .copied()
                .filter(|&p| !ev(p) && (topic(p) || named(p)))
                .collect();
            let core = candidates
                .iter()
                .copied()
                .find(|&p| named(p) && topic(p))
                .or_else(|| candidates.iter().copied().find(|&p| topic(p)))
                .or_else(|| candidates.iter().copied().find(|&p| linked(p)));
            let evidence = pages.iter().filter(|&&p| ev(p)).count();
            let maintained = pages
                .iter()
                .filter(|&&p| !ev(p) && Some(p) != core && linked(p))
                .count();
            System {
                subject: s,
                x: 0.,
                y: 0.,
                radius: radius_of(maintained + core.is_some() as usize, evidence),
                core,
                pages: pages.len(),
                evidence,
            }
        })
        .collect();
    let ids: Vec<&str> = corpus.subjects.iter().map(|s| s.id.as_str()).collect();
    place(&mut systems, &bridges, &ids, previous);

    let mut stars = vec![
        Star {
            system: 0,
            role: Role::Stray,
            x: 0.,
            y: 0.
        };
        n
    ];
    for (s, sys) in systems.iter().enumerate() {
        let pages = &corpus.subjects[sys.subject].pages;
        let turn = hash(ids[s]) * std::f32::consts::TAU;
        let ev = |p: usize| corpus.pages[p].is_evidence();
        let mut placed = 0;
        if let Some(core) = sys.core {
            stars[core] = Star {
                system: s,
                role: Role::Core,
                x: sys.x,
                y: sys.y,
            };
            placed += 1;
        }
        let mut orbit: Vec<usize> = pages
            .iter()
            .copied()
            .filter(|&p| Some(p) != sys.core && !ev(p) && linked(p))
            .collect();
        orbit.sort_by(|&a, &b| {
            degree[b]
                .cmp(&degree[a])
                .then(corpus.pages[a].path.cmp(&corpus.pages[b].path))
        });
        let dust: Vec<usize> = pages.iter().copied().filter(|&p| ev(p)).collect();
        let inner = if sys.core.is_some() { 15. } else { 0. };
        let outer = sys.radius * if dust.is_empty() { 0.86 } else { 0.5 };
        for (i, &p) in orbit.iter().enumerate() {
            let d = inner + (outer - inner) * ((i as f32 + 0.5) / orbit.len() as f32).sqrt();
            let a = turn + i as f32 * GOLDEN;
            stars[p] = Star {
                system: s,
                role: Role::Star,
                x: sys.x + a.cos() * d,
                y: sys.y + a.sin() * d,
            };
            placed += 1;
        }
        // Dust fills the ring outside the orbits evenly by area, with a
        // little scatter so it reads as a cloud rather than a pattern.
        let from = if !orbit.is_empty() || sys.core.is_some() {
            sys.radius * 0.5
        } else {
            sys.radius * 0.08
        };
        let to = sys.radius * 0.94;
        for (i, &p) in dust.iter().enumerate() {
            let path = &corpus.pages[p].path;
            let share = (i as f32 + 0.5) / dust.len() as f32;
            let jitter = 1. + (hash(path) - 0.5) * 0.08;
            let d = (from * from + (to * to - from * from) * share).sqrt() * jitter;
            let a = turn + i as f32 * GOLDEN + (hash(&format!("{path}#")) - 0.5) * 0.35;
            stars[p] = Star {
                system: s,
                role: Role::Dust,
                x: sys.x + a.cos() * d,
                y: sys.y + a.sin() * d,
            };
            placed += 1;
        }
        // A page with no link in or out has drifted past its system's edge;
        // in a system with nothing else in it, it is all there is, and stays.
        let lone = placed == 0;
        for &p in pages
            .iter()
            .filter(|&&p| Some(p) != sys.core && !ev(p) && !linked(p))
        {
            let path = &corpus.pages[p].path;
            let a = hash(path) * std::f32::consts::TAU;
            let h = hash(&format!("{path}~"));
            let d = if lone {
                sys.radius * 0.5 * h
            } else {
                sys.radius + 12. + h * (GAP - 26.)
            };
            stars[p] = Star {
                system: s,
                role: Role::Stray,
                x: sys.x + a.cos() * d,
                y: sys.y + a.sin() * d,
            };
        }
    }
    let bounds = if systems.is_empty() {
        Rect {
            x: -100.,
            y: -100.,
            w: 200.,
            h: 200.,
        }
    } else {
        let reach = |s: &System| s.radius + GAP / 2.;
        let l = systems
            .iter()
            .map(|s| s.x - reach(s))
            .fold(f32::MAX, f32::min);
        let t = systems
            .iter()
            .map(|s| s.y - reach(s))
            .fold(f32::MAX, f32::min);
        let r = systems
            .iter()
            .map(|s| s.x + reach(s))
            .fold(f32::MIN, f32::max);
        let b = systems
            .iter()
            .map(|s| s.y + reach(s))
            .fold(f32::MIN, f32::max);
        Rect {
            x: l,
            y: t,
            w: r - l,
            h: b - t,
        }
    };
    Sky {
        systems,
        stars,
        bridges,
        bounds,
        degree,
    }
}

impl Sky {
    /// Where each system sits, for the next launch's sky.
    pub fn placement(&self, corpus: &Corpus) -> Placement {
        self.systems
            .iter()
            .map(|s| (corpus.subjects[s.subject].id.clone(), (s.x, s.y)))
            .collect()
    }
}

/// Spring the systems that link together, keep every pair apart, and settle.
fn place(systems: &mut [System], bridges: &[Bridge], ids: &[&str], previous: &Placement) {
    let mut order: Vec<usize> = (0..systems.len()).collect();
    order.sort_by(|&a, &b| {
        systems[b]
            .radius
            .total_cmp(&systems[a].radius)
            .then(ids[a].cmp(ids[b]))
    });
    let springs: Vec<(usize, usize, f32)> = bridges
        .iter()
        .map(|b| (b.a, b.b, (b.weight as f32).ln_1p()))
        .collect();
    let separate = |systems: &mut [System], strength: f32| {
        for i in 0..order.len() {
            for j in i + 1..order.len() {
                let (a, b) = (order[i], order[j]);
                let dx = systems[b].x - systems[a].x;
                let dy = systems[b].y - systems[a].y;
                let d = dx.hypot(dy).max(0.01);
                let overlap = systems[a].radius + systems[b].radius + GAP - d;
                if overlap <= 0. {
                    continue;
                }
                let push = overlap * strength / d;
                let share = systems[b].radius / (systems[a].radius + systems[b].radius);
                systems[a].x -= dx * push * share;
                systems[a].y -= dy * push * share;
                systems[b].x += dx * push * (1. - share);
                systems[b].y += dy * push * (1. - share);
            }
        }
    };
    if order.iter().any(|&s| previous.contains_key(ids[s])) {
        settle_into(systems, &springs, ids, previous);
        for _ in 0..60 {
            separate(systems, 1.);
        }
        return;
    }
    for (k, &s) in order.iter().enumerate() {
        let d = 90. * (k as f32).sqrt();
        systems[s].x = (k as f32 * GOLDEN).cos() * d;
        systems[s].y = (k as f32 * GOLDEN).sin() * d;
    }
    for step in 0..STEPS {
        let heat = 1. - step as f32 / STEPS as f32;
        for &(a, b, pull) in &springs {
            let dx = systems[b].x - systems[a].x;
            let dy = systems[b].y - systems[a].y;
            let d = dx.hypot(dy).max(0.01);
            let stretch = d - (systems[a].radius + systems[b].radius + GAP);
            if stretch <= 0. {
                continue;
            }
            let m = (stretch / 2.).min(stretch * 0.018 * pull) * heat / d;
            systems[a].x += dx * m;
            systems[a].y += dy * m;
            systems[b].x -= dx * m;
            systems[b].y -= dy * m;
        }
        for &s in &order {
            systems[s].x -= systems[s].x * 0.012 * heat;
            systems[s].y -= systems[s].y * 0.012 * heat;
        }
        separate(systems, 0.5);
    }
    for _ in 0..60 {
        separate(systems, 1.);
    }
}

/// Puts every known system where it was. A new one starts among the systems
/// it links to, weighted by how much, or past the sky's edge if it links
/// nowhere.
fn settle_into(
    systems: &mut [System],
    springs: &[(usize, usize, f32)],
    ids: &[&str],
    previous: &Placement,
) {
    let known = |s: usize| previous.contains_key(ids[s]);
    for (s, sys) in systems.iter_mut().enumerate() {
        if let Some(&(x, y)) = previous.get(ids[s]) {
            sys.x = x;
            sys.y = y;
        }
    }
    let edge = (0..systems.len())
        .filter(|&s| known(s))
        .map(|s| systems[s].x.hypot(systems[s].y) + systems[s].radius)
        .fold(0., f32::max);
    for s in (0..systems.len()).filter(|&s| !known(s)) {
        let (mut x, mut y, mut total) = (0., 0., 0.);
        for &(a, b, pull) in springs {
            let other = if a == s {
                b
            } else if b == s {
                a
            } else {
                continue;
            };
            if !known(other) {
                continue;
            }
            x += systems[other].x * pull;
            y += systems[other].y * pull;
            total += pull;
        }
        let a = hash(ids[s]) * std::f32::consts::TAU;
        let r = systems[s].radius;
        let (cx, cy, d) = if total > 0. {
            (x / total, y / total, r)
        } else {
            (0., 0., edge + GAP + r)
        };
        systems[s].x = cx + a.cos() * d;
        systems[s].y = cy + a.sin() * d;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus::{Corpus, parse};

    /// A made-up brain: three subjects that cite each other, evidence, and a
    /// stray. Nothing from the real brain.
    pub fn sample() -> Corpus {
        let mut pages = vec![];
        let mut page = |path: &str, text: &str| pages.push(parse(path, text));
        page(
            "knowledge/sky/sky.md",
            "---\nkind: topic\n---\n# Sky\n[[moon]] [[sun]] [[tide]]",
        );
        page("knowledge/sky/moon.md", "# Moon\n[[sky]]");
        page("knowledge/sky/sun.md", "# Sun\n[[sky]] [[tide]]");
        page("knowledge/sky/lost.md", "# Lost\nno links");
        page(
            "knowledge/sea/sea.md",
            "---\nkind: topic\n---\n# Sea\n[[tide]] [[sky]]",
        );
        page("knowledge/sea/tide.md", "# Tide\n[[sea]]");
        for i in 0..30 {
            page(
                &format!("evidence/sources/sea/s{i}.md"),
                "---\nkind: source\n---\n# S\n[[sea]]",
            );
        }
        page(
            "projects/boat/project-boat.md",
            "---\nkind: topic\n---\n# Boat\n[[sea]]",
        );
        Corpus::new(pages)
    }

    #[test]
    fn every_page_is_one_star_and_systems_never_overlap() {
        let corpus = sample();
        let sky = layout(&corpus, &Placement::new());
        assert_eq!(sky.stars.len(), corpus.pages.len());
        for (i, a) in sky.systems.iter().enumerate() {
            for b in &sky.systems[i + 1..] {
                let d = (a.x - b.x).hypot(a.y - b.y);
                assert!(d >= a.radius + b.radius + GAP - 1., "{d}");
            }
        }
        let role = |p: &str| sky.stars[corpus.by_path(p).unwrap()].role;
        assert_eq!(role("knowledge/sky/sky.md"), Role::Core);
        assert_eq!(role("knowledge/sky/moon.md"), Role::Star);
        assert_eq!(role("knowledge/sky/lost.md"), Role::Stray);
        assert_eq!(role("evidence/sources/sea/s3.md"), Role::Dust);
        // Records filed for a subject are drawn in its system.
        let sea = corpus.pages[corpus.by_path("knowledge/sea/sea.md").unwrap()].subject;
        assert_eq!(sky.systems[sea].evidence, 30);
    }

    #[test]
    fn the_same_brain_is_the_same_sky_and_a_kept_sky_stays_put() {
        let corpus = sample();
        let a = layout(&corpus, &Placement::new());
        let b = layout(&corpus, &Placement::new());
        for (x, y) in a.stars.iter().zip(&b.stars) {
            assert_eq!((x.x, x.y), (y.x, y.y));
        }
        let mut kept = a.placement(&corpus);
        kept.insert("knowledge/sky".into(), (5000., 5000.));
        let c = layout(&corpus, &kept);
        let sky = corpus.pages[corpus.by_path("knowledge/sky/sky.md").unwrap()].subject;
        assert_eq!((c.systems[sky].x, c.systems[sky].y), (5000., 5000.));
    }
}
