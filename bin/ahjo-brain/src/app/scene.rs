//! The sky's scene: where every star, ring and name is this frame.

use super::*;

pub(super) struct Boxes(Vec<(f32, f32, f32, f32)>);

impl Boxes {
    /// Takes the box if it overlaps none taken, with `pad` between.
    pub(super) fn take(&mut self, x: f32, y: f32, w: f32, h: f32, pad: f32) -> bool {
        let clear = self.0.iter().all(|&(a, b, c, d)| {
            x + w + pad <= a || a + c + pad <= x || y + h + pad <= b || b + d + pad <= y
        });
        if clear {
            self.0.push((x, y, w, h));
        }
        clear
    }
}

// ---- The scene: what every star is doing this frame.

impl BrainApp {
    /// This frame's sky, and whether anything is still moving.
    pub(super) fn scene(&mut self, window: &Window, t: &Theme) -> (Picture, bool) {
        let st = tokens::style(t);
        let mut pic = Picture {
            halo: alpha(t.bg, k::HALO),
            halo_blur: st.halo_blur,
            halo_spread: st.halo_spread,
            ..Picture::default()
        };
        let Some(data) = self.data.clone() else {
            return (pic, false);
        };
        let now = Instant::now();
        let dt = self
            .last
            .map(|l| now.duration_since(l).as_secs_f32() * 1000.)
            .unwrap_or(16.)
            .min(100.);
        self.last = Some(now);
        let mut busy = false;
        let (w, h) = self.size;

        let mut touring = false;
        if let Some(bench) = &mut self.bench {
            let start = *bench.start.get_or_insert(now);
            if bench.words.is_empty() {
                // The words of a title from the middle of the brain.
                let title = &data.corpus.pages[data.corpus.pages.len() / 2].title;
                bench.words = title.to_lowercase().chars().take(18).collect();
            }
            busy = true;
            if bench.due(now).is_none() {
                let s = now.duration_since(start).as_secs_f32();
                self.camera = bench_camera(&data.sky, self.free(), s);
                touring = true;
            }
        }
        if touring {
            // The bench holds the camera.
        } else if let Some((flight, start, ms)) = self.flight {
            let k = now.duration_since(start).as_secs_f32() * 1000. / ms;
            self.camera = flight.at(ease_in_out(k));
            if k >= 1. {
                self.flight = None;
            } else {
                busy = true;
            }
        }
        // The replay: how long since each page was touched, as it stands.
        let replay: Option<Vec<Option<f32>>> = match &mut self.replay {
            Some(Replaying::Ready(r)) => {
                if r.advance() {
                    busy = true;
                }
                Some((0..data.corpus.pages.len()).map(|p| r.since(p)).collect())
            }
            _ => None,
        };
        let cam = self.camera;
        let fade = 1. - (-dt / motion::FADE).exp();
        let follow = |v: &mut f32, target: f32, busy: &mut bool| {
            if (target - *v).abs() < 0.01 {
                *v = target;
            } else {
                *v += (target - *v) * fade;
                *busy = true;
            }
            *v
        };

        let corpus = &data.corpus;
        let sky = &data.sky;
        let free = self.free();
        let orbit: Option<Orbit> = if self.orbiting() {
            self.reading
                .map(|p| arrange::orbit(corpus, sky, p, free, !self.fold_evidence))
        } else {
            None
        };
        let picked = self.pick.and_then(|i| self.results.get(i)).map(|f| f.page);
        let hot = self.link.or(self.hover).or(self.reading);
        let related: HashSet<usize> = hot
            .map(|p| corpus.neighbours(p).into_iter().collect())
            .unwrap_or_default();
        let matches: HashSet<usize> = self.results.iter().map(|f| f.page).collect();
        let lit_search = !self.query.is_empty();

        // Where pulled stars go.
        let mut anchors: HashMap<usize, ((f32, f32), usize)> = HashMap::new();
        if let Some(o) = &orbit {
            anchors.insert(o.sun, (o.centre, 0));
            for (i, p) in o.planets.iter().enumerate() {
                anchors.insert(p.page, ((p.x, p.y), i / 3 + 1));
            }
        }
        let replanned = std::mem::take(&mut self.replanned);

        // The sky gathering out of its systems on first light.
        let first = self
            .first_light
            .map(|f| now.duration_since(f).as_secs_f32() * 1000.);
        let (bx, by) = sky.bounds.centre();
        let reach = sky
            .systems
            .iter()
            .map(|s| (s.x - bx).hypot(s.y - by))
            .fold(1., f32::max);
        let gathering = |s: &sky::System| -> f32 {
            match first {
                None => 1.,
                Some(ms) => {
                    let delay = (s.x - bx).hypot(s.y - by) / reach * motion::GATHER_SPREAD;
                    ease_out((ms - delay) / motion::GATHER)
                }
            }
        };
        if first
            .is_some_and(|ms| ms > motion::GATHER + motion::GATHER_SPREAD + motion::FLARE + 800.)
        {
            self.first_light = None;
        }

        // ---- The far field: faint stars that are no page, drifting slower
        // than the sky so it reads as deep.
        let field_a = if orbit.is_some() { 0.5 } else { 1. };
        let tile = k::FIELD_TILE;
        let (ox, oy) = (cam.x * k::FIELD_PARALLAX, cam.y * k::FIELD_PARALLAX);
        let (tx0, ty0) = ((-ox / tile).floor() as i32, (-oy / tile).floor() as i32);
        let (tx1, ty1) = (
            ((w - ox) / tile).ceil() as i32,
            ((h - oy) / tile).ceil() as i32,
        );
        for ty in ty0..ty1 {
            for tx in tx0..tx1 {
                let mut seed =
                    (tx as u32).wrapping_mul(73_856_093) ^ (ty as u32).wrapping_mul(19_349_663);
                let mut next = || {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    (seed % 10_000) as f32 / 10_000.
                };
                for _ in 0..k::FIELD_PER_TILE {
                    let (fx, fy, b) = (next(), next(), next());
                    pic.dots.push(Dot {
                        x: ox + (tx as f32 + fx) * tile,
                        y: oy + (ty as f32 + fy) * tile,
                        r: k::FIELD_SIZE * (0.6 + b),
                        color: alpha(t.text2, k::FIELD_ALPHA * st.field * b * b * field_a),
                        shape: if st.square {
                            Shape::Square
                        } else {
                            Shape::Round
                        },
                    });
                }
            }
        }

        // ---- Systems: nebulae, rims and bridges.
        let focus = self
            .hover_system
            .or(self.reading.map(|p| sky.stars[p].system));
        let near: HashSet<usize> = focus
            .map(|f| {
                sky.bridges
                    .iter()
                    .filter_map(|b| {
                        if b.a == f {
                            Some(b.b)
                        } else if b.b == f {
                            Some(b.a)
                        } else {
                            None
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        // Only a star pointed at in the sky dims gently; a search, a page
        // read or a link in it dims firmly.
        let glance = hot.is_some() && hot == self.hover && self.link.is_none();
        for (s, sys) in sky.systems.iter().enumerate() {
            let target = if orbit.is_some() {
                0.
            } else if lit_search || replay.is_some() || (hot.is_some() && !glance) {
                k::SYSTEMS_DIMMED
            } else if glance {
                k::SYSTEMS_HOVER
            } else {
                1.
            } * gathering(sys);
            let a = follow(&mut self.system_light[s], target, &mut busy);
            if a < 0.01 {
                continue;
            }
            let (x, y) = cam.screen(sys.x, sys.y);
            let r = sys.radius * cam.scale;
            if x + r * 1.6 < 0. || x - r * 1.6 > w || y + r * 1.6 < 0. || y - r * 1.6 > h {
                continue;
            }
            let hue = data.hues[sys.subject];
            let share = (sys.evidence as f32).ln_1p() / (data.max_evidence as f32).ln_1p();
            let strength =
                (k::NEBULA_MIN + (k::NEBULA_MAX - k::NEBULA_MIN) * share) * a * st.nebula;
            pic.lights.push(Light {
                x,
                y,
                r: r * st.nebula_size,
                blur: r * st.nebula_soft,
                color: alpha(hue, strength),
            });
            let edge = if Some(s) == focus {
                0.5
            } else if near.contains(&s) {
                0.22
            } else {
                0.
            };
            if edge > 0. && r > 12. {
                pic.rims.push(Ring {
                    x,
                    y,
                    r,
                    color: alpha(hue, edge * a),
                    width: 1.,
                    dashed: true,
                });
            }
        }
        for (i, b) in sky.bridges.iter().enumerate() {
            let involved = Some(b.a) == focus || Some(b.b) == focus;
            let strength = (b.weight as f32).ln_1p() / (data.strongest as f32).ln_1p();
            let target = if orbit.is_some() || first.is_some() || replay.is_some() {
                0.
            } else if hot.is_some() || lit_search {
                if involved { 0.22 } else { 0. }
            } else if involved {
                0.3 + 0.4 * strength
            } else if b.weight >= k::STRONG {
                0.05 + 0.16 * strength
            } else {
                0.
            };
            let a = follow(&mut self.bridge_light[i], target, &mut busy);
            if a < 0.01 {
                continue;
            }
            let (sa, sb) = (&sky.systems[b.a], &sky.systems[b.b]);
            pic.lines.push(Line {
                a: cam.screen(sa.x, sa.y),
                b: cam.screen(sb.x, sb.y),
                bend: k::BRIDGE_BEND * st.bend,
                color: alpha(data.hues[sa.subject], a),
                width: if involved { 1.2 } else { 1. },
                reach: 1.,
            });
        }

        // ---- Stars.
        let near_size = cam.scale.sqrt();
        let mut on_screen: Vec<(usize, f32, f32, f32)> = vec![];
        let flare_at = first.map(|ms| (ms - motion::GATHER * 0.6) / motion::FLARE);
        for (i, star) in sky.stars.iter().enumerate() {
            let page = &corpus.pages[i];
            let anchored = anchors.get(&i).copied();
            let mut target = match star.role {
                Role::Dust => 0.8,
                _ => 1.,
            };
            if orbit.is_some() {
                target = if anchored.is_some() { 1. } else { 0. };
            } else {
                if let Some(r) = &replay {
                    let warm = r[i].map_or(0., |d| motion::REPLAY_COOL.powf(d));
                    target *= k::REPLAY_BASE + (1. - k::REPLAY_BASE) * warm;
                }
                if lit_search && !matches.is_empty() && !matches.contains(&i) {
                    target *= k::UNMATCHED;
                }
                if let Some(hp) = hot
                    && hp != i
                    && !related.contains(&i)
                    && anchored.is_none()
                {
                    target *= if glance {
                        k::UNRELATED_HOVER
                    } else {
                        k::UNRELATED
                    };
                }
            }
            let sys = &sky.systems[star.system];
            let g = gathering(sys);
            let a = follow(&mut self.light[i], target * g.min(1.), &mut busy);
            if g < 1. {
                busy = true;
            }
            let home = cam.screen(star.x, star.y);
            let sky_at = if g < 1. {
                lerp(cam.screen(sys.x, sys.y), home, g)
            } else {
                home
            };
            let goal = anchored.map(|(p, _)| p).unwrap_or(sky_at);
            let was = self.anchor[i];
            let now_anchor = anchored.map(|(p, _)| p);
            let moved_anchor = match (was, now_anchor) {
                (None, None) => false,
                (Some(_), None) | (None, Some(_)) => true,
                (Some(a), Some(b)) => replanned && (a.0 - b.0).hypot(a.1 - b.1) > 1.,
            };
            if moved_anchor && self.data.is_some() && self.first_light.is_none() {
                let rank = anchored.map(|(_, r)| r).unwrap_or(0) as f32;
                let delay = std::time::Duration::from_secs_f32(rank * motion::PULL_STAGGER / 1000.);
                let (fx, fy) = self.shown[i];
                self.moving[i] = Some((fx, fy, now + delay));
            }
            self.anchor[i] = now_anchor;
            let at = match self.moving[i] {
                Some((fx, fy, start)) => {
                    let since = now.saturating_duration_since(start).as_secs_f32() * 1000.;
                    let k = if now < start {
                        0.
                    } else {
                        since / motion::PULL
                    };
                    busy = true;
                    if k >= 1. {
                        self.moving[i] = None;
                        goal
                    } else {
                        lerp((fx, fy), goal, ease_out(k))
                    }
                }
                None => goal,
            };
            self.shown[i] = at;
            if a < 0.01 {
                continue;
            }
            let (x, y) = at;
            if x < -30. || x > w + 30. || y < -30. || y > h + 30. {
                continue;
            }
            on_screen.push((i, x, y, a));
            let hue = data.hues[page.subject];
            let words = ((page.words as f32) + 10.).log10();
            let mut size = match star.role {
                Role::Core => (2. + 2.6 * near_size).clamp(k::CORE_MIN, k::CORE_MAX),
                Role::Dust => (1.15 * near_size).clamp(k::DUST_MIN, k::DUST_MAX),
                _ => ((0.6 + words * 0.5) * near_size).clamp(k::STAR_MIN, k::STAR_MAX),
            };
            if let Some(o) = &orbit {
                size = if i == o.sun {
                    tokens::orbit::SUN
                } else {
                    let d = (sky.degree[i] as f32).ln_1p();
                    (tokens::orbit::PLANET_MIN + d).min(tokens::orbit::PLANET_MAX)
                };
            }
            let is_hot = hot == Some(i);
            let found = lit_search && matches.contains(&i);
            let layer = &mut pic;
            if star.role == Role::Stray && orbit.is_none() {
                layer.rings.push(Ring {
                    x,
                    y,
                    r: size + 0.6,
                    color: alpha(hue, 0.8 * a),
                    width: 1.,
                    dashed: false,
                });
            } else {
                let white = match star.role {
                    Role::Core => st.core_white,
                    Role::Dust => 0.1,
                    _ => 0.35,
                };
                if st.glow > 0.
                    && (star.role == Role::Core || is_hot || found || anchored.is_some())
                {
                    layer.glows.push(Light {
                        x,
                        y,
                        r: size,
                        blur: size * k::GLOW * 0.5,
                        color: alpha(hue, (0.55 * a * st.glow).min(1.)),
                    });
                }
                layer.dots.push(Dot {
                    x,
                    y,
                    r: size,
                    color: alpha(
                        lift(t, hue, white),
                        a * if star.role == Role::Dust { 0.7 } else { 1. },
                    ),
                    shape: if st.square
                        || (star.role == Role::Dust && cam.scale < k::NEAR && orbit.is_none())
                    {
                        Shape::Square
                    } else {
                        Shape::Round
                    },
                });
            }
            if found {
                layer.rings.push(Ring {
                    x,
                    y,
                    r: size + k::FOUND_RING,
                    color: alpha(t.accent, 0.9 * a),
                    width: 1.2,
                    dashed: false,
                });
            }
            if is_hot || picked == Some(i) {
                layer.rings.push(Ring {
                    x,
                    y,
                    r: size + k::HOT_RING,
                    color: alpha(lift(t, hue, 0.5), a),
                    width: 1.,
                    dashed: false,
                });
            }
            // A page touched on the replay's day flares.
            if let Some(d) = replay.as_ref().and_then(|r| r[i])
                && d < 1.
                && orbit.is_none()
            {
                layer.rings.push(Ring {
                    x,
                    y,
                    r: size + 2. + d * 16.,
                    color: alpha(lift(t, hue, 0.3), (1. - d) * 0.9),
                    width: 1.5 * (1. - d) + 0.3,
                    dashed: false,
                });
            }
            if let Some(fl) = flare_at
                && data.fresh.contains(&i)
                && (0. ..1.).contains(&fl)
            {
                busy = true;
                layer.rings.push(Ring {
                    x,
                    y,
                    r: size + 2. + fl * 18.,
                    color: alpha(hue, (1. - fl) * 0.9),
                    width: 1.5 * (1. - fl) + 0.3,
                    dashed: false,
                });
            }
        }

        // ---- Lines from a page: its links, its orbit's spokes, the threads home.
        let growth = match self.trace {
            Some((p, start)) if Some(p) == hot => {
                let g = now.duration_since(start).as_secs_f32() * 1000. / motion::TRACE;
                if g < 1. {
                    busy = true;
                }
                ease_out(g)
            }
            _ => 1.,
        };
        if let Some(o) = &orbit {
            for p in &o.planets {
                let hue = data.hues[corpus.pages[p.page].subject];
                let lit = hot == Some(p.page);
                let a = self.light[p.page];
                pic.lines.push(Line {
                    a: self.shown[o.sun],
                    b: self.shown[p.page],
                    bend: 0.,
                    color: alpha(hue, if lit { 0.7 } else { 0.16 } * a),
                    width: if lit { 1.4 } else { 1. },
                    reach: 1.,
                });
            }
        } else if let Some(hp) = hot {
            let hue = data.hues[corpus.pages[hp].subject];
            let at = |p: usize| self.shown[p];
            // Many links each draw fainter, so a hub reads as a hub and not
            // as a sheet of light; links leaving the view fainter still,
            // their way told by the signposts.
            let share = (k::LINKS_FULL / related.len().max(1) as f32).sqrt().min(1.);
            let inside = |(x, y): (f32, f32)| x >= 0. && x <= w && y >= 0. && y <= h;
            for &r in &related {
                let b = at(r);
                let away = if inside(b) { 1. } else { k::LINK_AWAY };
                pic.lines.push(Line {
                    a: at(hp),
                    b,
                    bend: k::LINK_BEND * st.bend,
                    color: alpha(hue, k::LINK_ALPHA * share * away * self.light[r].max(0.4)),
                    width: 1.,
                    reach: growth,
                });
            }
        }
        // ---- Names.
        let mut taken = Boxes(vec![]);
        // Room the bar along the top (the search, the theme) already uses.
        taken
            .0
            .push((0., 0., w, search::FIELD_HEIGHT + chrome::INSET * 2.));
        // And the results down the left, which a name would show through.
        let beside = self.free().x;
        if beside > 0. {
            taken.0.push((0., 0., beside, h));
        }
        if let Some(o) = &orbit {
            self.orbit_names(&data, o, hot, &mut pic, &mut taken, window, t);
        } else {
            self.system_names(&data, focus, &mut pic, &mut taken, window, t);
            // Without a page pointed at, the replay names the day's work.
            let today: HashSet<usize> = match (&replay, hot) {
                (Some(r), None) => (0..r.len())
                    .filter(|&p| r[p].is_some_and(|d| d < 1.))
                    .collect(),
                _ => HashSet::new(),
            };
            let named = if hot.is_some() { &related } else { &today };
            self.page_names(
                &data, &on_screen, hot, named, &matches, &mut pic, &mut taken, window, t,
            );
        }
        (pic, busy)
    }

    /// Each system's name, in the theme's display face. Far out, the sky
    /// is an overview: names sized by how much a subject holds, each with
    /// its count and how much of it moved this week.
    pub(super) fn system_names(
        &self,
        data: &Data,
        focus: Option<usize>,
        pic: &mut Picture,
        taken: &mut Boxes,
        window: &Window,
        t: &Theme,
    ) {
        let cam = self.camera;
        let mut order: Vec<usize> = (0..data.sky.systems.len()).collect();
        order.sort_by(|&a, &b| {
            (Some(b) == focus).cmp(&(Some(a) == focus)).then(
                data.sky.systems[b]
                    .radius
                    .total_cmp(&data.sky.systems[a].radius),
            )
        });
        let up = cam.scale >= k::NEAR;
        let display = Face::display(t);
        let biggest = data
            .sky
            .systems
            .iter()
            .map(|s| s.pages)
            .max()
            .unwrap_or(1)
            .max(1) as f32;
        for s in order {
            let a = self.system_light[s];
            if a < 0.3 {
                continue;
            }
            let sys = &data.sky.systems[s];
            let name = t.label_case(&data.corpus.subjects[sys.subject].name);
            let share = (sys.pages as f32).ln_1p() / biggest.ln_1p();
            let size = if up {
                k::SYSTEM_NAME
            } else {
                k::SYSTEM_NAME_MIN + (k::SYSTEM_NAME_MAX - k::SYSTEM_NAME_MIN) * share
            };
            let tw = paint::measure(&name, size, &display, window);
            let (x, cy) = cam.screen(sys.x, sys.y);
            let y = if up {
                cy - sys.radius * cam.scale - size
            } else {
                cy + (sys.radius * cam.scale * k::NAME_BELOW).max(6.) + size * 0.8
            };
            let (bx, by) = (x - tw / 2., y - size * 0.7);
            if bx < 4.
                || bx + tw > self.size.0 - 4.
                || by < 4.
                || by + size * 1.4 > self.size.1 - 4.
            {
                continue;
            }
            if !taken.take(bx, by, tw, size * 1.4, k::NAME_GAP * 1.5) {
                continue;
            }
            let hue = data.hues[sys.subject];
            let lit = Some(s) == focus;
            pic.labels.push(Label {
                text: name.into(),
                x,
                y,
                size,
                color: alpha(lift(t, hue, 0.25), if lit { 1. } else { 0.82 } * a),
                face: display.clone(),
                align: Align::Centre,
            });
            if !(lit || !up) {
                continue;
            }
            let kept = sys.pages - sys.evidence;
            let mut count = match (kept, sys.evidence) {
                (0, e) => format!("{e} evidence"),
                (n, e) if lit && e > 0 => format!("{} · {e} evidence", pages(n)),
                (n, _) => pages(n),
            };
            let moved = data.moved[s];
            if moved > 0 {
                count.push_str(&format!(" · {moved} this week"));
            }
            let small = k::PAGE_NAME - 1.;
            let cy2 = y + size * 0.7 + small * 0.85;
            let cw = paint::measure(&count, small, &Face::mono(false), window);
            if taken.take(
                x - cw / 2.,
                cy2 - small * 0.6,
                cw,
                small * 1.2,
                k::COUNT_PAD,
            ) {
                pic.labels.push(Label {
                    text: count.into(),
                    x,
                    y: cy2,
                    size: small,
                    color: {
                        let c = tokens::quiet(t, hue, lit);
                        alpha(c, c.a * a)
                    },
                    face: Face::mono(false),
                    align: Align::Centre,
                });
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn page_names(
        &self,
        data: &Data,
        on_screen: &[(usize, f32, f32, f32)],
        hot: Option<usize>,
        related: &HashSet<usize>,
        matches: &HashSet<usize>,
        pic: &mut Picture,
        taken: &mut Boxes,
        window: &Window,
        t: &Theme,
    ) {
        let cam = self.camera;
        let lantern: Vec<usize> = match self.pointer {
            Some((px_, py_)) if self.drag.is_none() => {
                let mut near: Vec<(usize, f32, bool)> = on_screen
                    .iter()
                    .filter(|e| e.3 > 0.3)
                    .map(|&(i, x, y, _)| {
                        (
                            i,
                            (x - px_).hypot(y - py_),
                            data.sky.stars[i].role == Role::Dust,
                        )
                    })
                    .filter(|e| e.1 < k::LANTERN)
                    .collect();
                near.sort_by(|a, b| a.2.cmp(&b.2).then(a.1.total_cmp(&b.1)));
                near.into_iter()
                    .take(k::LANTERN_NAMES)
                    .map(|e| e.0)
                    .collect()
            }
            _ => vec![],
        };
        // Named near the page pointed at, nearest first; or, with none, the
        // pages the replay names.
        let named_related: HashSet<usize> = match hot {
            None => related.iter().copied().take(k::REPLAY_NAMES).collect(),
            Some(h) => {
                let o = self.shown[h];
                let mut list: Vec<usize> = related.iter().copied().collect();
                list.sort_by(|&a, &b| {
                    let da = (self.shown[a].0 - o.0).hypot(self.shown[a].1 - o.1);
                    let db = (self.shown[b].0 - o.0).hypot(self.shown[b].1 - o.1);
                    da.total_cmp(&db)
                });
                list.into_iter().take(14).collect()
            }
        };
        let rank = |i: usize| -> u8 {
            let role = data.sky.stars[i].role;
            if Some(i) == hot {
                0
            } else if matches.contains(&i) {
                1
            } else if lantern.contains(&i) {
                2
            } else if named_related.contains(&i) {
                3
            } else if role == Role::Core {
                5
            } else if role == Role::Dust {
                7
            } else {
                6
            }
        };
        let mut order: Vec<&(usize, f32, f32, f32)> = on_screen.iter().collect();
        order.sort_by_key(|e| rank(e.0));
        let mut count = 0;
        for &&(i, x, y, a) in &order {
            let level = rank(i);
            let wanted = level <= 3
                || (hot.is_none() && cam.scale >= k::NEAR && level <= 6)
                || (hot.is_some() && cam.scale >= k::NEAR && level == 5)
                || (hot.is_none() && cam.scale >= k::CLOSE && level == 7);
            if !wanted || a < 0.2 {
                continue;
            }
            if count >= k::NAMES {
                break;
            }
            let text = shorten(&data.corpus.pages[i].title, k::PAGE_NAME_MAX);
            let tw = paint::measure(&text, k::PAGE_NAME, &Face::mono(level == 0), window);
            // A name runs right of its star, or left where the window ends.
            let lx = if x + 9. + tw > self.size.0 - 8. {
                x - 9. - tw
            } else {
                x + 9.
            };
            let size = k::PAGE_NAME;
            if !taken.take(lx, y - size * 0.75, tw, size * 1.5, k::NAME_GAP) {
                continue;
            }
            count += 1;
            let color = match level {
                0 => t.text,
                1 => alpha(t.text, 0.92),
                2 => t.text2,
                _ => alpha(t.text2, 0.78),
            };
            pic.labels.push(Label {
                text: text.into(),
                x: lx,
                y,
                size,
                color: alpha(color, color.a * a.max(0.5)),
                face: Face::mono(level == 0),
                align: Align::Left,
            });
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn orbit_names(
        &self,
        data: &Data,
        o: &Orbit,
        hot: Option<usize>,
        pic: &mut Picture,
        taken: &mut Boxes,
        window: &Window,
        t: &Theme,
    ) {
        let (sx, sy) = self.shown[o.sun];
        let title = shorten(&data.corpus.pages[o.sun].title, 48);
        let size = k::SYSTEM_NAME;
        let tw = paint::measure(&title, size, &Face::mono(true), window);
        taken.take(sx - tw / 2., sy + 14., tw, size * 1.4, 4.);
        pic.labels.push(Label {
            text: title.into(),
            x: sx,
            y: sy + 14. + size * 0.7,
            size,
            color: t.text,
            face: Face::mono(true),
            align: Align::Centre,
        });
        // Names keep off the planets.
        for p in &o.planets {
            let (x, y) = self.shown[p.page];
            taken.0.push((x - 5., y - 5., 10., 10.));
        }
        // Inside the ring, a bow in each subject's hue along its stretch of
        // it; the subjects are named in a key in the corner, most first.
        for arc in &o.arcs {
            let hue = data.hues[data.sky.systems[arc.system].subject];
            // One subject all the way round closes its ring.
            let pad = if o.arcs.len() == 1 {
                0.
            } else {
                0.03_f32.min((arc.to - arc.from) / 4.)
            };
            pic.bows.push(paint::Bow {
                x: o.centre.0,
                y: o.centre.1,
                r: o.radius * tokens::orbit::BOW,
                from: arc.from + pad,
                to: arc.to - pad,
                color: alpha(hue, 0.6),
                width: 2.,
            });
        }
        let mut key: Vec<&arrange::Arc> = o.arcs.iter().collect();
        key.sort_by(|a, b| b.count.cmp(&a.count).then(a.system.cmp(&b.system)));
        let more = key.len().saturating_sub(tokens::orbit::KEY);
        let rows = key.len().min(tokens::orbit::KEY) + usize::from(more > 0);
        let free = self.free();
        let line = tokens::orbit::KEY_LINE;
        let (kx, mut ky) = (
            free.x + chrome::INSET + 4.,
            free.y + free.h - rows as f32 * line - chrome::INSET - 14.,
        );
        for arc in key.into_iter().take(tokens::orbit::KEY) {
            let subject = data.sky.systems[arc.system].subject;
            let hue = data.hues[subject];
            pic.dots.push(Dot {
                x: kx,
                y: ky,
                r: 3.,
                color: hue,
                shape: Shape::Round,
            });
            pic.labels.push(Label {
                text: format!("{}  {}", data.corpus.subjects[subject].name, arc.count).into(),
                x: kx + 12.,
                y: ky,
                size: k::PAGE_NAME,
                color: alpha(lift(t, hue, 0.25), 0.95),
                face: Face::mono(false),
                align: Align::Left,
            });
            taken.0.push((kx - 4., ky - 9., 260., 18.));
            ky += line;
        }
        if more > 0 {
            pic.labels.push(Label {
                text: format!("and {more} more subjects").into(),
                x: kx + 12.,
                y: ky,
                size: k::PAGE_NAME,
                color: t.dim,
                face: Face::mono(false),
                align: Align::Left,
            });
        }
        let mut order: Vec<&arrange::Planet> = o.planets.iter().collect();
        order.sort_by_key(|p| {
            (
                Some(p.page) != hot,
                std::cmp::Reverse(data.sky.degree[p.page]),
            )
        });
        for p in order {
            let (x, y) = self.shown[p.page];
            let a = self.light[p.page];
            if a < 0.3 {
                continue;
            }
            let lit = Some(p.page) == hot;
            let right = p.angle.cos() >= 0.;
            // A name runs as far as the sky beside its planet allows, on a
            // second line before it is cut; never past that room.
            let off = tokens::orbit::NAME_OFF;
            let room = if right {
                free.x + free.w - x - off
            } else {
                x - off - free.x
            } - chrome::INSET / 2.;
            let face = Face::mono(lit);
            let size = k::PAGE_NAME;
            let title = &data.corpus.pages[p.page].title;
            let lines = name_lines(title, room, |s| paint::measure(s, size, &face, window));
            let widths: Vec<f32> = lines
                .iter()
                .map(|l| paint::measure(l, size, &face, window))
                .collect();
            let tw = widths.iter().copied().fold(0., f32::max);
            let line = size * 1.25;
            let high = line * lines.len() as f32;
            let top = y - high / 2.;
            let lx = if right { x + off } else { x - off - tw };
            if !lit && !taken.take(lx, top, tw, high, 2.) {
                continue;
            }
            for (n, (text, w)) in lines.into_iter().zip(widths).enumerate() {
                pic.labels.push(Label {
                    text: text.into(),
                    x: if right { x + off } else { x - off - w },
                    y: top + line * (n as f32 + 0.5),
                    size,
                    color: if lit { t.text } else { alpha(t.text2, 0.9 * a) },
                    face: face.clone(),
                    align: Align::Left,
                });
            }
        }
    }
}

/// A name in `room` pixels, as `measure` sizes text: whole on one line if
/// it fits, else split between its words onto two, else cut to the room.
pub(super) fn name_lines(name: &str, room: f32, measure: impl Fn(&str) -> f32) -> Vec<String> {
    if measure(name) <= room {
        return vec![name.to_string()];
    }
    // The split that leaves the longer line shortest.
    let best = name
        .match_indices(' ')
        .map(|(i, _)| (name[..i].trim_end(), name[i..].trim_start()))
        .map(|(a, b)| (a, b, measure(a).max(measure(b))))
        .min_by(|x, y| x.2.total_cmp(&y.2));
    if let Some((a, b, widest)) = best
        && widest <= room
    {
        return vec![a.to_string(), b.to_string()];
    }
    // Cut where it still fits, letter by letter from the whole.
    let chars: Vec<char> = name.chars().collect();
    let mut n = chars.len().min(tokens::orbit::NAME_MOST);
    while n > tokens::orbit::NAME_LEAST {
        let cut = shorten(name, n);
        if measure(&cut) <= room {
            return vec![cut];
        }
        n -= 1;
    }
    vec![shorten(name, tokens::orbit::NAME_LEAST)]
}

/// The bench's camera: a fixed tour over the whole sky, then close in on
/// the largest systems and out again, so every frame draws the most there is.
pub(super) fn bench_camera(sky: &Sky, free: Rect, s: f32) -> Camera {
    let all = Camera::fit(sky.bounds, free, k::FIT_MOST, k::FIT_PAD);
    let mut big: Vec<&sky::System> = sky.systems.iter().collect();
    big.sort_by(|a, b| b.radius.total_cmp(&a.radius));
    let stops: Vec<&sky::System> = big.into_iter().take(4).collect();
    if stops.is_empty() {
        return all;
    }
    let leg = 2.5;
    let n = (s / leg) as usize;
    let k = (s % leg) / leg;
    let at = |i: usize| -> Camera {
        if i.is_multiple_of(2) {
            all.zoom_about(
                free.centre().0,
                free.centre().1,
                1. + 0.15 * (i as f32 * 0.7).sin(),
            )
        } else {
            let sys = stops[(i / 2) % stops.len()];
            Camera::centre(sys.x, sys.y, 2.4, free)
        }
    };
    let f = Flight::new(at(n), at(n + 1), free);
    f.at(ease_in_out(k))
}
