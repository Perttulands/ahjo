//! The Brain's one window: the sky, the search over it and the reader
//! beside it, in one of three takes on how they relate.
//!
//! - **Sky**: the sky is the place. A search lights what it found and the
//!   camera flies there; a page opens in a sheet over the sky's right side
//!   and the camera brings its star into the part still visible, its links
//!   traced and the ones that leave the screen signposted at its edge.
//! - **Gather**: the sky comes to you. The camera holds the whole brain;
//!   a search pulls the pages it found out of their systems into a column
//!   you read down, each with a thread back to where it lives. A page read
//!   draws its links across the whole sky.
//! - **Orbit**: you stand on a page. Reading turns the sky into the page's
//!   own orbit: what it links and what links to it, one arc per subject,
//!   each arc pointing the way its subject lies in the sky. A planet clicked
//!   becomes the sun. Closing the page lets the sky back.
//!
//! The sky is drawn from the scene each frame while anything moves, and not
//! at all while nothing does.

use crate::arrange::{self, Orbit};
use crate::camera::{Camera, Flight, ease_in_out, ease_out};
use crate::citadel::{self, Hit};
use crate::corpus::{self, Corpus, Entry};
use crate::paint::{self, Align, Dot, Label, Light, Line, Picture, Ring, Shape};
use crate::reader::{self, Reader};
use crate::sky::{self, Rect, Role, Sky};
use crate::source::{self, Where};
use crate::tokens::{self, chrome, gather, motion, search, sky as k, veil, whiten};
use gpui::{
    AnyView, App, AppContext, Context, Entity, FocusHandle, Focusable, FontWeight, Hsla,
    KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ScrollDelta,
    ScrollEvent, SharedString, StyleRefinement, Window, canvas, div, linear_color_stop,
    linear_gradient, prelude::*, px,
};
use parvi_app_kit::theme::{FAMILY, Theme, alpha, theme};
use parvi_app_kit::widgets::field::{self, FieldEvent, TextField};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Instant, SystemTime};

/// The brain as one launch holds it.
pub struct Data {
    pub root: PathBuf,
    pub corpus: Corpus,
    pub sky: Sky,
    /// One per subject.
    pub hues: Vec<Hsla>,
    /// Today as a Julian day, for a page's age.
    pub today: i32,
    /// Pages changed in the last days, newest first, which flare as the sky
    /// first lights.
    pub fresh: HashSet<usize>,
    pub max_evidence: usize,
    pub strongest: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Take {
    Sky,
    Gather,
    Orbit,
}

impl Take {
    pub const ALL: [Take; 3] = [Take::Sky, Take::Gather, Take::Orbit];

    pub fn name(self) -> &'static str {
        match self {
            Take::Sky => "Sky",
            Take::Gather => "Gather",
            Take::Orbit => "Orbit",
        }
    }

    pub fn parse(name: &str) -> Option<Take> {
        Take::ALL
            .into_iter()
            .find(|t| t.name().eq_ignore_ascii_case(name))
    }
}

struct Found {
    page: usize,
    snippet: String,
}

struct Drag {
    at: (f32, f32),
    camera: Camera,
    moved: bool,
}

struct Signpost {
    target: usize,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

/// A scripted flight over the sky that measures its frames (`--bench`).
pub struct Bench {
    pub seconds: f32,
    start: Option<Instant>,
    last: Option<Instant>,
    frames: Vec<f32>,
    work: Vec<f32>,
    painted: Rc<std::cell::Cell<Option<Instant>>>,
    began: Option<Instant>,
}

impl Bench {
    pub fn new(seconds: f32) -> Bench {
        Bench {
            seconds,
            start: None,
            last: None,
            frames: vec![],
            work: vec![],
            painted: Rc::new(std::cell::Cell::new(None)),
            began: None,
        }
    }
}

/// What the app is told at launch.
pub struct Start {
    pub place: Where,
    pub take: Take,
    pub query: Option<String>,
    pub open: Option<String>,
    pub bench: Option<Bench>,
}

pub struct BrainApp {
    place: Where,
    data: Option<Rc<Data>>,
    entries: Vec<Entry>,
    marker: Option<SystemTime>,
    status: Option<String>,
    take: Take,
    field: Entity<TextField>,
    query: String,
    asked: u64,
    answered: u64,
    results: Vec<Found>,
    failed: Option<String>,
    pick: Option<usize>,
    reading: Option<usize>,
    back: Vec<usize>,
    reader: Entity<Reader>,
    focus: FocusHandle,
    open_on_arrival: Option<String>,

    camera: Camera,
    flight: Option<(Flight, Instant, f32)>,
    framed: bool,
    drag: Option<Drag>,
    pointer: Option<(f32, f32)>,
    hover: Option<usize>,
    hover_system: Option<usize>,
    link: Option<usize>,
    light: Vec<f32>,
    system_light: Vec<f32>,
    bridge_light: Vec<f32>,
    moving: Vec<Option<(f32, f32, Instant)>>,
    anchor: Vec<Option<(f32, f32)>>,
    shown: Vec<(f32, f32)>,
    replanned: bool,
    first_light: Option<Instant>,
    trace: Option<(usize, Instant)>,
    last: Option<Instant>,
    signposts: Vec<Signpost>,
    size: (f32, f32),
    bench: Option<Bench>,
    /// A page opened: the keyboard leaves the search for the page.
    leave_field: bool,
}

impl Focusable for BrainApp {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl BrainApp {
    pub fn new(start: Start, window: &mut Window, cx: &mut Context<Self>) -> BrainApp {
        let field =
            cx.new(|cx| TextField::new(cx, "Search the brain").text_size(search::FIELD_TEXT));
        cx.subscribe_in(&field, window, |me, _, event, _, cx| match event {
            FieldEvent::Changed => me.typed(cx),
            FieldEvent::Up => me.step(-1, cx),
            FieldEvent::Down => me.step(1, cx),
            FieldEvent::Submit => {
                if let Some(p) = me.pick.and_then(|i| me.results.get(i)).map(|f| f.page) {
                    me.open(p, cx);
                }
            }
            _ => {}
        })
        .detach();
        let me = cx.weak_entity();
        let reader = cx.new(|_| Reader::new(me));
        cx.observe_window_activation(window, |me, window, cx| {
            if window.is_window_active() {
                me.look_again(cx);
            }
        })
        .detach();
        field::focus(&field, window, cx);
        if let Some(q) = &start.query {
            field.update(cx, |f, cx| f.set(q, false, cx));
        }
        let mut app = BrainApp {
            place: start.place,
            data: None,
            entries: vec![],
            marker: None,
            status: Some("Looking for the brain…".into()),
            take: start.take,
            field,
            query: String::new(),
            asked: 0,
            answered: 0,
            results: vec![],
            failed: None,
            pick: None,
            reading: None,
            back: vec![],
            reader,
            focus: cx.focus_handle(),
            open_on_arrival: start.open,
            camera: Camera {
                x: 0.,
                y: 0.,
                scale: 1.,
            },
            flight: None,
            framed: false,
            drag: None,
            pointer: None,
            hover: None,
            hover_system: None,
            link: None,
            light: vec![],
            system_light: vec![],
            bridge_light: vec![],
            moving: vec![],
            anchor: vec![],
            shown: vec![],
            replanned: false,
            first_light: None,
            trace: None,
            last: None,
            signposts: vec![],
            size: (1440., 900.),
            bench: start.bench,
            leave_field: false,
        };
        app.load(cx);
        app
    }

    // ---- The brain arriving.

    fn load(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.place.brain.clone() else {
            self.status =
                Some("No brain found here. Start with --brain <folder> or set BRAIN_ROOT.".into());
            return;
        };
        let cache = self.place.cache.clone();
        self.marker = source::marker(&root);
        cx.spawn(async move |me, cx| {
            let kept = cx
                .background_spawn({
                    let (root, cache) = (root.clone(), cache.clone());
                    async move { cache.map(|c| source::load(&c, &root)).unwrap_or_default() }
                })
                .await;
            let mut placement = kept.placement.clone();
            if !kept.entries.is_empty() {
                let entries = kept.entries.clone();
                let built = cx
                    .background_spawn(async move { build(entries, &placement) })
                    .await;
                placement = built.1.placement(&built.0);
                let entries = kept.entries.clone();
                let _ = me.update(cx, |me, cx| me.arrive(built, entries, cx));
            } else {
                let _ = me.update(cx, |me, cx| {
                    me.status = Some("Reading the brain for the first time…".into());
                    cx.notify();
                });
            }
            let scanned = cx
                .background_spawn({
                    let root = root.clone();
                    let known = kept.entries.clone();
                    async move { corpus::scan(&root, &known) }
                })
                .await;
            let entries = match scanned {
                Ok(e) => e,
                Err(e) => {
                    let _ = me.update(cx, |me, cx| {
                        me.status = Some(format!("The brain did not read: {e}"));
                        cx.notify();
                    });
                    return;
                }
            };
            if same(&entries, &kept.entries) {
                return;
            }
            let built = cx
                .background_spawn({
                    let entries = entries.clone();
                    async move { build(entries, &placement) }
                })
                .await;
            let placement = built.1.placement(&built.0);
            let saved = entries.clone();
            let _ = me.update(cx, |me, cx| me.arrive(built, entries, cx));
            if let Some(cache) = cache {
                let root_name = root.display().to_string();
                cx.background_spawn(async move {
                    if let Err(e) = source::save(
                        &cache,
                        source::Kept {
                            root: root_name,
                            entries: saved,
                            placement,
                        },
                    ) {
                        eprintln!("the brain's cache was not kept: {e}");
                    }
                })
                .await;
            }
        })
        .detach();
    }

    /// The window came forward: if the brain's Git index moved, read again.
    fn look_again(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.place.brain.clone() else {
            return;
        };
        if self.data.is_none() {
            return;
        }
        let now = source::marker(&root);
        if now == self.marker {
            return;
        }
        self.marker = now;
        let known = self.entries.clone();
        let placement = self
            .data
            .as_ref()
            .map(|d| d.sky.placement(&d.corpus))
            .unwrap_or_default();
        let cache = self.place.cache.clone();
        cx.spawn(async move |me, cx| {
            let scanned = cx
                .background_spawn({
                    let (root, known) = (root.clone(), known.clone());
                    async move { corpus::scan(&root, &known) }
                })
                .await;
            let Ok(entries) = scanned else { return };
            if same(&entries, &known) {
                return;
            }
            let built = cx
                .background_spawn({
                    let entries = entries.clone();
                    async move { build(entries, &placement) }
                })
                .await;
            let placement = built.1.placement(&built.0);
            let saved = entries.clone();
            let _ = me.update(cx, |me, cx| me.arrive(built, entries, cx));
            if let Some(cache) = cache {
                let root = root.display().to_string();
                cx.background_spawn(async move {
                    let kept = source::Kept {
                        root,
                        entries: saved,
                        placement,
                    };
                    if let Err(e) = source::save(&cache, kept) {
                        eprintln!("the brain's cache was not kept: {e}");
                    }
                })
                .await;
            }
        })
        .detach();
    }

    fn arrive(
        &mut self,
        (corpus, sky): (Corpus, Sky),
        entries: Vec<Entry>,
        cx: &mut Context<Self>,
    ) {
        let t = theme(cx).clone();
        let today = time::OffsetDateTime::now_utc().date().to_julian_day();
        let mut fresh: Vec<usize> = (0..corpus.pages.len())
            .filter(|&p| {
                !corpus.pages[p].is_evidence()
                    && corpus.pages[p]
                        .updated
                        .is_some_and(|u| today - u < motion::FRESH_DAYS)
            })
            .collect();
        fresh.sort_by_key(|&p| std::cmp::Reverse(corpus.pages[p].updated));
        let data = Rc::new(Data {
            root: self.place.brain.clone().unwrap_or_default(),
            hues: corpus
                .subjects
                .iter()
                .map(|s| tokens::subject_hue(&t, &s.id))
                .collect(),
            today,
            fresh: fresh.into_iter().take(40).collect(),
            max_evidence: sky
                .systems
                .iter()
                .map(|s| s.evidence)
                .max()
                .unwrap_or(1)
                .max(1),
            strongest: sky
                .bridges
                .iter()
                .map(|b| b.weight)
                .max()
                .unwrap_or(1)
                .max(1),
            corpus,
            sky,
        });
        let n = data.corpus.pages.len();
        let now = Instant::now();
        // Pages that were already drawn travel from where they were.
        let old: HashMap<String, (f32, f32)> = match &self.data {
            Some(d) => d
                .corpus
                .pages
                .iter()
                .enumerate()
                .filter_map(|(i, p)| self.shown.get(i).map(|s| (p.path.clone(), *s)))
                .collect(),
            None => HashMap::new(),
        };
        let remap = |i: usize, from: &Data| data.corpus.by_path(&from.corpus.pages[i].path);
        if let Some(prev) = self.data.clone() {
            self.reading = self.reading.and_then(|p| remap(p, &prev));
            self.back = self.back.iter().filter_map(|&p| remap(p, &prev)).collect();
            self.results = std::mem::take(&mut self.results)
                .into_iter()
                .filter_map(|f| {
                    remap(f.page, &prev).map(|page| Found {
                        page,
                        snippet: f.snippet,
                    })
                })
                .collect();
            self.hover = None;
            self.link = None;
            self.light = (0..n)
                .map(|i| {
                    prev.corpus
                        .by_path(&data.corpus.pages[i].path)
                        .and_then(|j| self.light.get(j).copied())
                        .unwrap_or(0.)
                })
                .collect();
            self.moving = (0..n)
                .map(|i| {
                    old.get(&data.corpus.pages[i].path)
                        .map(|&(x, y)| (x, y, now))
                })
                .collect();
        } else {
            self.light = vec![0.; n];
            self.moving = vec![None; n];
            self.first_light = Some(now);
        }
        self.system_light = vec![0.; data.sky.systems.len()];
        self.bridge_light = vec![0.; data.sky.bridges.len()];
        self.anchor = vec![None; n];
        self.shown = vec![(0., 0.); n];
        self.entries = entries;
        self.status = None;
        self.data = Some(data.clone());
        if !self.framed {
            self.framed = true;
            self.camera = Camera::fit(data.sky.bounds, self.free(), k::FIT_MOST, k::FIT_PAD);
        }
        if let Some(p) = self.reading {
            self.reader.update(cx, |r, cx| r.show(data.clone(), p, cx));
        }
        if let Some(path) = self.open_on_arrival.take()
            && let Some(p) = data
                .corpus
                .by_path(&path)
                .or_else(|| data.corpus.resolve(&path))
        {
            self.open(p, cx);
        }
        if !self.query.is_empty() || !self.field.read(cx).text.is_empty() {
            self.query.clear();
            self.typed(cx);
        }
        cx.notify();
    }

    // ---- Search.

    fn typed(&mut self, cx: &mut Context<Self>) {
        let q = self.field.read(cx).text.trim().to_string();
        if q == self.query {
            return;
        }
        self.query = q.clone();
        self.asked += 1;
        let asked = self.asked;
        if q.is_empty() {
            self.answered = asked;
            self.results.clear();
            self.failed = None;
            self.pick = None;
            self.replanned = true;
            cx.notify();
            return;
        }
        let url = self.place.citadel.clone();
        cx.spawn(async move |me, cx| {
            // Typing on: only the words it rests on are asked.
            cx.background_executor()
                .timer(std::time::Duration::from_millis(motion::SEARCH_WAIT))
                .await;
            if me.read_with(cx, |me, _| me.asked != asked).unwrap_or(true) {
                return;
            }
            let hits = cx
                .background_spawn(async move { citadel::search(&url, &q) })
                .await;
            let _ = me.update(cx, |me, cx| {
                if me.asked == asked {
                    me.answered = asked;
                    me.found(hits, cx);
                }
            });
        })
        .detach();
    }

    fn found(&mut self, hits: Result<Vec<Hit>, String>, cx: &mut Context<Self>) {
        let Some(data) = self.data.clone() else {
            return;
        };
        match hits {
            Ok(hits) => {
                let mut seen = HashSet::new();
                self.results = hits
                    .into_iter()
                    .filter_map(|h| {
                        let page = data.corpus.by_path(&h.path)?;
                        seen.insert(page).then_some(Found {
                            page,
                            snippet: h.snippet,
                        })
                    })
                    .collect();
                self.failed = None;
            }
            Err(e) => {
                self.results.clear();
                self.failed = Some(e);
            }
        }
        self.pick = (!self.results.is_empty()).then_some(0);
        self.replanned = true;
        match self.take {
            Take::Gather => self.frame_everything(),
            _ if self.orbiting() => {}
            _ => {
                let stars = self
                    .results
                    .iter()
                    .take(search::FLY_TO)
                    .map(|f| data.sky.stars[f.page])
                    .map(|s| (s.x, s.y));
                if let Some(world) = Rect::around(stars, 60.) {
                    let to = Camera::fit(world, self.free(), k::FIT_MOST, k::FIT_PAD);
                    self.fly(to);
                }
            }
        }
        cx.notify();
    }

    fn step(&mut self, by: isize, cx: &mut Context<Self>) {
        if self.results.is_empty() {
            return;
        }
        let shown = match self.take {
            Take::Gather => arrange::slots(self.size.1).min(self.results.len()),
            _ => search::ROWS.min(self.results.len()),
        };
        let at = self.pick.unwrap_or(0) as isize + by;
        self.pick = Some(at.rem_euclid(shown.max(1) as isize) as usize);
        if matches!(self.take, Take::Sky) || (self.take == Take::Orbit && !self.orbiting()) {
            let page = self.results[self.pick.unwrap_or(0)].page;
            self.fly_to_page(page);
        }
        cx.notify();
    }

    // ---- Reading.

    pub fn open(&mut self, page: usize, cx: &mut Context<Self>) {
        if self.reading == Some(page) {
            return;
        }
        if let Some(now) = self.reading {
            self.back.push(now);
        }
        self.show(page, cx);
    }

    fn show(&mut self, page: usize, cx: &mut Context<Self>) {
        let Some(data) = self.data.clone() else {
            return;
        };
        self.reading = Some(page);
        self.link = None;
        self.hover = None;
        self.trace = Some((page, Instant::now()));
        self.replanned = true;
        self.reader.update(cx, |r, cx| r.show(data, page, cx));
        self.leave_field = true;
        match self.take {
            Take::Sky => self.fly_to_page(page),
            Take::Gather => self.frame_everything(),
            Take::Orbit => {}
        }
        cx.notify();
    }

    pub fn go_back(&mut self, cx: &mut Context<Self>) {
        if let Some(p) = self.back.pop() {
            self.show(p, cx);
        }
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.reading = None;
        self.back.clear();
        self.trace = None;
        self.link = None;
        self.replanned = true;
        cx.notify();
    }

    /// A link in the reader pointed at, or let go.
    pub fn point_at_link(&mut self, page: Option<usize>, cx: &mut Context<Self>) {
        if self.link != page {
            self.link = page;
            cx.notify();
        }
    }

    fn switch(&mut self, take: Take, cx: &mut Context<Self>) {
        if self.take == take {
            return;
        }
        self.take = take;
        self.replanned = true;
        match take {
            Take::Gather => self.frame_everything(),
            Take::Sky => {
                if let Some(p) = self.reading {
                    self.fly_to_page(p);
                }
            }
            Take::Orbit => {}
        }
        cx.notify();
    }

    // ---- The camera.

    fn orbiting(&self) -> bool {
        self.take == Take::Orbit && self.reading.is_some()
    }

    /// The part of the window the sky shows through: not under the reader,
    /// the Gather column or the search.
    fn free(&self) -> Rect {
        let (w, h) = self.size;
        let right = if self.reading.is_some() {
            w - reader::sheet_width(w)
        } else {
            w
        };
        let left = if self.results.is_empty() {
            0.
        } else if self.take == Take::Gather {
            gather::LEFT + gather::WIDTH
        } else if self.reading.is_none() {
            // The list under the search covers the sky's left.
            search::WIDTH + chrome::INSET * 2.
        } else {
            0.
        };
        let top = search::FIELD_HEIGHT + chrome::INSET * 2.;
        Rect {
            x: left,
            y: top,
            w: (right - left).max(80.),
            // Above the counts along the bottom.
            h: (h - top - chrome::INSET - chrome::FOOT).max(80.),
        }
    }

    fn fly(&mut self, to: Camera) {
        let flight = Flight::new(self.camera, to, self.free());
        let ms = flight.millis(
            motion::FLIGHT_BASE,
            motion::FLIGHT_PER_UNIT,
            motion::FLIGHT_MOST,
        );
        self.flight = Some((flight, Instant::now(), ms));
    }

    fn fly_to_page(&mut self, page: usize) {
        let Some(data) = &self.data else { return };
        let s = data.sky.stars[page];
        let scale = if self.camera.scale < k::NEAR {
            k::PAGE_SCALE
        } else {
            self.camera.scale
        };
        let to = Camera::centre(s.x, s.y, scale, self.free());
        self.fly(to);
    }

    fn frame_everything(&mut self) {
        let Some(data) = &self.data else { return };
        let to = Camera::fit(data.sky.bounds, self.free(), k::FIT_MOST, k::FIT_PAD);
        self.fly(to);
    }

    fn fit_system(&mut self, s: usize) {
        let Some(data) = &self.data else { return };
        let sys = &data.sky.systems[s];
        let free = self.free();
        let scale = ((free.w - 60.) / (sys.radius * 2.3))
            .min((free.h - 60.) / (sys.radius * 2.3))
            .min(2.5);
        let to = Camera::centre(sys.x, sys.y, scale, free);
        self.fly(to);
    }

    // ---- The pointer.

    fn star_at(&self, x: f32, y: f32) -> Option<usize> {
        let data = self.data.as_ref()?;
        let mut best: Option<(usize, f32)> = None;
        for (i, &(sx, sy)) in self.shown.iter().enumerate() {
            if self.light.get(i).copied().unwrap_or(0.) < 0.2 {
                continue;
            }
            let dust = data.sky.stars[i].role == Role::Dust;
            let d = (sx - x).hypot(sy - y) + if dust { 5. } else { 0. };
            if d < k::HIT && best.is_none_or(|(_, b)| d < b) {
                best = Some((i, d));
            }
        }
        best.map(|(i, _)| i)
    }

    fn system_at(&self, x: f32, y: f32) -> Option<usize> {
        let data = self.data.as_ref()?;
        if self.orbiting() {
            return None;
        }
        let (wx, wy) = self.camera.world(x, y);
        data.sky
            .systems
            .iter()
            .position(|s| (s.x - wx).hypot(s.y - wy) <= s.radius)
    }

    fn mouse_down(&mut self, e: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.flight = None;
        self.drag = Some(Drag {
            at: (f32::from(e.position.x), f32::from(e.position.y)),
            camera: self.camera,
            moved: false,
        });
        cx.notify();
    }

    fn mouse_move(&mut self, e: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let (x, y) = (f32::from(e.position.x), f32::from(e.position.y));
        let orbiting = self.orbiting();
        if let Some(d) = &mut self.drag
            && e.pressed_button == Some(MouseButton::Left)
        {
            let (dx, dy) = (x - d.at.0, y - d.at.1);
            if dx.hypot(dy) > 3. {
                d.moved = true;
            }
            if d.moved && !orbiting {
                self.camera = d.camera.pan(dx, dy);
            }
            cx.notify();
            return;
        }
        self.pointer = Some((x, y));
        let hover = self.star_at(x, y);
        let system = match hover {
            Some(p) => self.data.as_ref().map(|d| d.sky.stars[p].system),
            None => self.system_at(x, y),
        };
        if hover != self.hover || system != self.hover_system || self.hover.is_none() {
            self.hover = hover;
            self.hover_system = system;
            cx.notify();
        }
    }

    fn mouse_up(&mut self, e: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = self.drag.take() else { return };
        if drag.moved {
            cx.notify();
            return;
        }
        let (x, y) = (f32::from(e.position.x), f32::from(e.position.y));
        if let Some(post) = self
            .signposts
            .iter()
            .find(|s| x >= s.x && x <= s.x + s.w && y >= s.y && y <= s.y + s.h)
        {
            let target = post.target;
            self.travel_to(target);
        } else if let Some(p) = self.star_at(x, y) {
            self.open(p, cx);
        } else if let Some(s) = self.system_at(x, y) {
            self.fit_system(s);
        }
        cx.notify();
    }

    /// Follows a signpost: the camera takes in the page read and where the
    /// link leads.
    fn travel_to(&mut self, target: usize) {
        let Some(data) = &self.data else { return };
        let Some(from) = self.reading else { return };
        let (a, b) = (data.sky.stars[from], data.sky.stars[target]);
        if let Some(world) = Rect::around([(a.x, a.y), (b.x, b.y)].into_iter(), 60.) {
            let to = Camera::fit(world, self.free(), self.camera.scale.max(0.9), 80.);
            self.fly(to);
        }
    }

    fn wheel(&mut self, e: &ScrollEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.orbiting() {
            return;
        }
        self.flight = None;
        let lines = match e.delta {
            ScrollDelta::Lines(p) => p.y,
            ScrollDelta::Pixels(p) => f32::from(p.y) / 40.,
        };
        let factor = motion::WHEEL_ZOOM.powf(lines);
        self.camera =
            self.camera
                .zoom_about(f32::from(e.position.x), f32::from(e.position.y), factor);
        cx.notify();
    }

    fn key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let ks = &e.keystroke;
        let m = &ks.modifiers;
        let typing = self.field.read(cx).is_focused(window);
        if m.control || m.platform {
            if let Some(take) = match ks.key.as_str() {
                "1" => Some(Take::Sky),
                "2" => Some(Take::Gather),
                "3" => Some(Take::Orbit),
                _ => None,
            } {
                self.switch(take, cx);
                cx.stop_propagation();
            }
            if ks.key == "k" || ks.key == "f" {
                field::focus(&self.field, window, cx);
                self.field.update(cx, |f, cx| {
                    let text = f.text.clone();
                    f.set(&text, true, cx)
                });
                cx.stop_propagation();
            }
            return;
        }
        if m.alt && ks.key == "left" {
            self.go_back(cx);
            cx.stop_propagation();
            return;
        }
        match ks.key.as_str() {
            "escape" => {
                if self.reading.is_some() {
                    self.close(cx);
                } else if !self.field.read(cx).text.is_empty() {
                    self.field.update(cx, |f, cx| f.set("", false, cx));
                    self.typed(cx);
                } else {
                    self.frame_everything();
                }
                cx.stop_propagation();
                cx.notify();
            }
            "home" if !typing => {
                self.frame_everything();
                cx.notify();
            }
            "+" | "=" | "-" if !typing => {
                let free = self.free();
                let (cx_, cy_) = free.centre();
                let factor = if ks.key == "-" { 0.8 } else { 1.25 };
                let to = self.camera.zoom_about(cx_, cy_, factor);
                self.fly(to);
                cx.notify();
            }
            "left" | "right" | "up" | "down" if !typing => {
                let p = motion::KEY_PAN;
                let (dx, dy) = match ks.key.as_str() {
                    "left" => (p, 0.),
                    "right" => (-p, 0.),
                    "up" => (0., p),
                    _ => (0., -p),
                };
                self.flight = None;
                self.camera = self.camera.pan(dx, dy);
                cx.notify();
            }
            _ if !typing => {
                // Typing anywhere searches.
                if let Some(ch) = ks.key_char.as_ref().filter(|c| !c.trim().is_empty()) {
                    field::focus(&self.field, window, cx);
                    self.field.update(cx, |f, cx| {
                        let text = format!("{}{ch}", f.text);
                        f.set(&text, false, cx)
                    });
                    self.typed(cx);
                    cx.stop_propagation();
                }
            }
            _ => {}
        }
    }
}

/// A corpus and its sky, off the UI thread.
fn build(entries: Vec<Entry>, placement: &sky::Placement) -> (Corpus, Sky) {
    let corpus = Corpus::new(entries);
    let sky = sky::layout(&corpus, placement);
    (corpus, sky)
}

fn same(a: &[Entry], b: &[Entry]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let index: HashMap<&str, &Entry> = b.iter().map(|e| (e.path.as_str(), e)).collect();
    a.iter()
        .all(|e| index.get(e.path.as_str()).is_some_and(|o| *o == e))
}

fn shorten(text: &str, most: usize) -> String {
    if text.chars().count() <= most {
        return text.to_string();
    }
    let mut out: String = text.chars().take(most.saturating_sub(1)).collect();
    out = out.trim_end().to_string();
    out.push('…');
    out
}

fn lerp(a: (f32, f32), b: (f32, f32), t: f32) -> (f32, f32) {
    (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
}

struct Boxes(Vec<(f32, f32, f32, f32)>);

impl Boxes {
    /// Takes the box if it overlaps none taken, with `pad` between.
    fn take(&mut self, x: f32, y: f32, w: f32, h: f32, pad: f32) -> bool {
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
    /// This frame's sky, what is drawn over the Gather column, and whether
    /// anything is still moving.
    fn scene(&mut self, window: &Window, t: &Theme) -> (Picture, Picture, bool) {
        let mut pic = Picture {
            halo: alpha(t.bg, k::HALO),
            ..Picture::default()
        };
        let Some(data) = self.data.clone() else {
            return (pic, Picture::default(), false);
        };
        let mut over = Picture::default();
        let now = Instant::now();
        let dt = self
            .last
            .map(|l| now.duration_since(l).as_secs_f32() * 1000.)
            .unwrap_or(16.)
            .min(100.);
        self.last = Some(now);
        let mut busy = false;
        let (w, h) = self.size;

        if let Some(bench) = &mut self.bench {
            let start = *bench.start.get_or_insert(now);
            let s = now.duration_since(start).as_secs_f32();
            self.camera = bench_camera(&data.sky, self.free(), s);
            busy = true;
        } else if let Some((flight, start, ms)) = self.flight {
            let k = now.duration_since(start).as_secs_f32() * 1000. / ms;
            self.camera = flight.at(ease_in_out(k));
            if k >= 1. {
                self.flight = None;
            } else {
                busy = true;
            }
        }
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
            self.reading.map(|p| arrange::orbit(corpus, sky, p, free))
        } else {
            None
        };
        let picked = self.pick.and_then(|i| self.results.get(i)).map(|f| f.page);
        let hot = self.link.or(self.hover).or(self.reading);
        let related: HashSet<usize> = hot
            .map(|p| corpus.neighbours(p).into_iter().collect())
            .unwrap_or_default();
        let matches: HashSet<usize> = self.results.iter().map(|f| f.page).collect();
        let lit_search = !self.query.is_empty() && self.failed.is_none();

        // Where pulled stars go.
        let mut anchors: HashMap<usize, ((f32, f32), usize)> = HashMap::new();
        if self.take == Take::Gather {
            for (i, f) in self.results.iter().take(arrange::slots(h)).enumerate() {
                anchors.insert(f.page, (arrange::slot_star(i), i));
            }
        }
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
                        color: alpha(t.text2, k::FIELD_ALPHA * b * b * field_a),
                        shape: Shape::Round,
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
        let dimmed = lit_search || hot.is_some();
        for (s, sys) in sky.systems.iter().enumerate() {
            let target = if orbit.is_some() {
                0.
            } else if dimmed {
                0.55
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
            let strength = (k::NEBULA_MIN + (k::NEBULA_MAX - k::NEBULA_MIN) * share) * a;
            pic.lights.push(Light {
                x,
                y,
                r: r * 0.5,
                blur: r * 0.42,
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
            let target = if orbit.is_some() || first.is_some() {
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
                bend: k::BRIDGE_BEND,
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
                if lit_search && !matches.is_empty() && !matches.contains(&i) {
                    target *= k::UNMATCHED;
                }
                if let Some(hp) = hot
                    && hp != i
                    && !related.contains(&i)
                    && anchored.is_none()
                {
                    target *= k::UNRELATED;
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
            } else if anchored.is_some() {
                size = size.max(tokens::gather::STAR);
            }
            let is_hot = hot == Some(i);
            let found = lit_search && matches.contains(&i);
            // Gathered stars shine over the column's scrim.
            let layer = if self.take == Take::Gather && anchored.is_some() {
                &mut over
            } else {
                &mut pic
            };
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
                    Role::Core => 0.75,
                    Role::Dust => 0.1,
                    _ => 0.35,
                };
                if star.role == Role::Core || is_hot || found || anchored.is_some() {
                    layer.glows.push(Light {
                        x,
                        y,
                        r: size,
                        blur: size * k::GLOW * 0.5,
                        color: alpha(hue, 0.55 * a),
                    });
                }
                layer.dots.push(Dot {
                    x,
                    y,
                    r: size,
                    color: alpha(
                        whiten(hue, white),
                        a * if star.role == Role::Dust { 0.7 } else { 1. },
                    ),
                    shape: if star.role == Role::Dust && cam.scale < k::NEAR && orbit.is_none() {
                        Shape::Square
                    } else {
                        Shape::Round
                    },
                });
            }
            if found && self.take != Take::Gather {
                layer.rings.push(Ring {
                    x,
                    y,
                    r: size + k::FOUND_RING,
                    color: alpha(t.accent, 0.9 * a),
                    width: 1.2,
                    dashed: false,
                });
            }
            if is_hot || (self.take != Take::Gather && picked == Some(i)) {
                layer.rings.push(Ring {
                    x,
                    y,
                    r: size + k::HOT_RING,
                    color: alpha(whiten(hue, 0.5), a),
                    width: 1.,
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
            // In Gather the links run between homes in the sky, which holds
            // still, not out of the column.
            let at = |p: usize| {
                if self.take == Take::Gather {
                    cam.screen(sky.stars[p].x, sky.stars[p].y)
                } else {
                    self.shown[p]
                }
            };
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
                    bend: k::LINK_BEND,
                    color: alpha(hue, k::LINK_ALPHA * share * away * self.light[r].max(0.4)),
                    width: 1.,
                    reach: growth,
                });
            }
        }
        if self.take == Take::Gather && orbit.is_none() {
            for f in self.results.iter().take(arrange::slots(h)) {
                let star = sky.stars[f.page];
                let home = cam.screen(star.x, star.y);
                let hue = data.hues[corpus.pages[f.page].subject];
                let lit = hot == Some(f.page);
                let a = if lit { 0.85 } else { k::THREAD_ALPHA };
                pic.lines.push(Line {
                    a: self.shown[f.page],
                    b: home,
                    bend: 0.14,
                    color: alpha(hue, a),
                    width: if lit { 1.4 } else { 1. },
                    reach: 1.,
                });
                pic.rings.push(Ring {
                    x: home.0,
                    y: home.1,
                    r: if lit { 6. } else { 3.5 },
                    color: alpha(hue, a),
                    width: 1.,
                    dashed: false,
                });
            }
        }

        // ---- Names.
        let mut taken = Boxes(vec![]);
        // Room the bar along the top (the search, the switch) and the
        // column already use.
        taken
            .0
            .push((0., 0., w, search::FIELD_HEIGHT + chrome::INSET * 2.));
        if self.take == Take::Gather && !self.results.is_empty() {
            taken.0.push((0., 0., gather::LEFT + gather::WIDTH, h));
        }
        // Signposts first: they say where the page read leads.
        self.signposts.clear();
        if self.take == Take::Sky
            && let Some(p) = self.reading
        {
            self.signposts(&data, p, free, &mut pic, &mut taken, window);
        }
        if let Some(o) = &orbit {
            self.orbit_names(&data, o, hot, &mut pic, &mut taken, window, t);
        } else {
            self.system_names(&data, focus, &mut pic, &mut taken, window);
            self.page_names(
                &data, &on_screen, hot, &related, &matches, &anchors, &mut pic, &mut taken, window,
                t,
            );
        }
        (pic, over, busy)
    }

    fn system_names(
        &self,
        data: &Data,
        focus: Option<usize>,
        pic: &mut Picture,
        taken: &mut Boxes,
        window: &Window,
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
        for s in order {
            let a = self.system_light[s];
            if a < 0.3 {
                continue;
            }
            let sys = &data.sky.systems[s];
            let name = &data.corpus.subjects[sys.subject].name;
            let size = k::SYSTEM_NAME;
            let tw = paint::measure(name, size, false, window);
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
            if !taken.take(bx, by, tw, size * 1.4, 6.) {
                continue;
            }
            let hue = self
                .data
                .as_ref()
                .map(|d| d.hues[sys.subject])
                .unwrap_or_default();
            pic.labels.push(Label {
                text: name.clone().into(),
                x,
                y,
                size,
                color: alpha(
                    whiten(hue, 0.25),
                    if Some(s) == focus { 1. } else { 0.82 } * a,
                ),
                bold: Some(s) == focus,
                align: Align::Centre,
            });
            if Some(s) == focus {
                let kept = sys.pages - sys.evidence;
                let count = if sys.evidence > 0 {
                    format!("{kept} pages · {} evidence", sys.evidence)
                } else {
                    format!("{kept} pages")
                };
                let cy2 = y + size * 1.25;
                let cw = paint::measure(&count, k::PAGE_NAME - 1., false, window);
                if taken.take(x - cw / 2., cy2 - 8., cw, 14., 2.) {
                    pic.labels.push(Label {
                        text: count.into(),
                        x,
                        y: cy2,
                        size: k::PAGE_NAME - 1.,
                        color: alpha(hue, 0.6 * a),
                        bold: false,
                        align: Align::Centre,
                    });
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn page_names(
        &self,
        data: &Data,
        on_screen: &[(usize, f32, f32, f32)],
        hot: Option<usize>,
        related: &HashSet<usize>,
        matches: &HashSet<usize>,
        anchors: &HashMap<usize, ((f32, f32), usize)>,
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
        let named_related: HashSet<usize> = match hot {
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
            None => HashSet::new(),
        };
        let rank = |i: usize| -> u8 {
            let role = data.sky.stars[i].role;
            if Some(i) == hot {
                0
            } else if matches.contains(&i) && self.take != Take::Gather {
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
        let mut order: Vec<&(usize, f32, f32, f32)> = on_screen
            .iter()
            .filter(|e| !(self.take == Take::Gather && anchors.contains_key(&e.0)))
            .collect();
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
            let tw = paint::measure(&text, k::PAGE_NAME, level == 0, window);
            // A name runs right of its star, or left where the window ends.
            let lx = if x + 9. + tw > self.size.0 - 8. {
                x - 9. - tw
            } else {
                x + 9.
            };
            let size = k::PAGE_NAME;
            if !taken.take(lx, y - size * 0.75, tw, size * 1.5, 3.) {
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
                bold: level == 0,
                align: Align::Left,
            });
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn orbit_names(
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
        let tw = paint::measure(&title, size, true, window);
        taken.take(sx - tw / 2., sy + 14., tw, size * 1.4, 4.);
        pic.labels.push(Label {
            text: title.into(),
            x: sx,
            y: sy + 14. + size * 0.7,
            size,
            color: t.text,
            bold: true,
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
            let pad = 0.03_f32.min((arc.to - arc.from) / 4.);
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
                color: alpha(whiten(hue, 0.25), 0.95),
                bold: false,
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
                bold: false,
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
            let text = shorten(&data.corpus.pages[p.page].title, tokens::orbit::NAME_MAX);
            let right = p.angle.cos() >= 0.;
            let tw = paint::measure(&text, k::PAGE_NAME, lit, window);
            let lx = if right { x + 10. } else { x - 10. - tw };
            if !lit && !taken.take(lx, y - 9., tw, 18., 2.) {
                continue;
            }
            pic.labels.push(Label {
                text: text.into(),
                x: lx,
                y,
                size: k::PAGE_NAME,
                color: if lit { t.text } else { alpha(t.text2, 0.9 * a) },
                bold: lit,
                align: Align::Left,
            });
        }
    }

    /// Links from the page read that leave the visible sky, as signs at its
    /// edge, one per subject they lead into.
    fn signposts(
        &mut self,
        data: &Data,
        page: usize,
        free: Rect,
        pic: &mut Picture,
        taken: &mut Boxes,
        window: &Window,
    ) {
        let inside = |(x, y): (f32, f32)| {
            x >= free.x && x <= free.x + free.w && y >= free.y && y <= free.y + free.h
        };
        let origin = self.shown[page];
        let (cx0, cy0) = if inside(origin) {
            origin
        } else {
            free.centre()
        };
        let mut by_system: HashMap<usize, (usize, usize, f32)> = HashMap::new();
        for p in data.corpus.neighbours(page) {
            let at = self.shown[p];
            if inside(at) {
                continue;
            }
            let d = (at.0 - cx0).hypot(at.1 - cy0);
            let s = data.sky.stars[p].system;
            let e = by_system.entry(s).or_insert((0, p, f32::MAX));
            e.0 += 1;
            if d < e.2 {
                e.1 = p;
                e.2 = d;
            }
        }
        let mut posts: Vec<(usize, (usize, usize, f32))> = by_system.into_iter().collect();
        posts.sort_by(|a, b| b.1.0.cmp(&a.1.0).then(a.0.cmp(&b.0)));
        let inset = k::SIGNPOST_INSET;
        for (s, (count, target, _)) in posts.into_iter().take(k::SIGNPOSTS) {
            let (tx, ty) = self.shown[target];
            let ang = (ty - cy0).atan2(tx - cx0);
            let (c, sn) = (ang.cos(), ang.sin());
            let rx = if c.abs() > 1e-4 {
                ((if c > 0. {
                    free.x + free.w - inset
                } else {
                    free.x + inset
                }) - cx0)
                    / c
            } else {
                f32::MAX
            };
            let ry = if sn.abs() > 1e-4 {
                ((if sn > 0. {
                    free.y + free.h - inset
                } else {
                    free.y + inset
                }) - cy0)
                    / sn
            } else {
                f32::MAX
            };
            let reach = rx.min(ry);
            let (x, y) = (cx0 + c * reach, cy0 + sn * reach);
            let subject = data.sky.systems[s].subject;
            let hue = data.hues[subject];
            let arrow = match ((ang + std::f32::consts::PI) / (std::f32::consts::PI / 4.)).round()
                as i32
                % 8
            {
                0 => "←",
                1 => "↖",
                2 => "↑",
                3 => "↗",
                4 => "→",
                5 => "↘",
                6 => "↓",
                _ => "↙",
            };
            let text = format!("{arrow} {} {count}", data.corpus.subjects[subject].name);
            let tw = paint::measure(&text, k::PAGE_NAME, false, window);
            let lx = (x - tw / 2.).clamp(free.x + 6., free.x + free.w - tw - 6.);
            if !taken.take(lx - 4., y - 11., tw + 8., 22., 4.) {
                continue;
            }
            pic.labels.push(Label {
                text: text.into(),
                x: lx,
                y,
                size: k::PAGE_NAME,
                color: alpha(whiten(hue, 0.3), 0.95),
                bold: false,
                align: Align::Left,
            });
            self.signposts.push(Signpost {
                target,
                x: lx - 4.,
                y: y - 11.,
                w: tw + 8.,
                h: 22.,
            });
        }
    }
}

/// The bench's camera: a fixed tour over the whole sky, then close in on
/// the largest systems and out again, so every frame draws the most there is.
fn bench_camera(sky: &Sky, free: Rect, s: f32) -> Camera {
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

impl Render for BrainApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let began = Instant::now();
        let t = theme(cx).clone();
        let vs = window.viewport_size();
        let size = (f32::from(vs.width), f32::from(vs.height));
        if size != self.size {
            // The camera keeps the middle of the view where it was.
            let free_before = self.free();
            let (cx0, cy0) = free_before.centre();
            let (wx, wy) = self.camera.world(cx0, cy0);
            self.size = size;
            if self.framed {
                let (nx, ny) = self.free().centre();
                self.camera = Camera::centre(
                    wx,
                    wy,
                    self.camera.scale,
                    Rect {
                        x: nx - 1.,
                        y: ny - 1.,
                        w: 2.,
                        h: 2.,
                    },
                );
            }
        }
        if std::mem::take(&mut self.leave_field) {
            window.focus(&self.focus, cx);
        }
        let typing = self.field.read(cx).is_focused(window);
        let (picture, over, busy) = self.scene(window, &t);
        if busy {
            window.request_animation_frame();
        }
        let painted = self.bench.as_ref().map(|b| b.painted.clone());
        if let Some(b) = &mut self.bench {
            if let Some(last) = b.last {
                b.frames
                    .push(began.duration_since(last).as_secs_f32() * 1000.);
            }
            if let (Some(prev), Some(done)) = (b.began, b.painted.get()) {
                b.work.push(done.duration_since(prev).as_secs_f32() * 1000.);
            }
            b.last = Some(began);
            b.began = Some(began);
            if b.start
                .is_some_and(|s| began.duration_since(s).as_secs_f32() > b.seconds)
            {
                report(
                    b,
                    self.data
                        .as_ref()
                        .map(|d| d.corpus.pages.len())
                        .unwrap_or(0),
                );
                cx.quit();
            }
        }
        let sky = canvas(
            |_, _, _| {},
            move |_, _, window, cx| {
                paint::paint(&picture, window, cx);
                if let Some(p) = &painted {
                    p.set(Some(Instant::now()));
                }
            },
        )
        .absolute()
        .size_full();

        let mut root = div()
            .id("brain")
            .key_context("Brain")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key))
            .size_full()
            .relative()
            .overflow_hidden()
            .bg(t.bg)
            .font_family(FAMILY)
            .text_color(t.text)
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::mouse_up))
            .on_scroll(cx.listener(Self::wheel))
            .when(self.hover.is_some(), |d| d.cursor_pointer())
            .child(sky);

        if self.take == Take::Gather && !self.results.is_empty() {
            root = root.child(self.gather_column(&t, cx)).child(
                canvas(
                    |_, _, _| {},
                    move |_, _, window, cx| paint::paint(&over, window, cx),
                )
                .absolute()
                .size_full(),
            );
        }
        root = root.child(self.search_panel(&t, typing, cx));
        root = root.child(self.switcher(&t, cx));
        if let Some(status) = &self.status {
            root = root.child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(search::TITLE))
                    .text_color(t.text2)
                    .child(status.clone()),
            );
        } else if let Some(d) = &self.data {
            let counts = format!(
                "{} pages · {} subjects · {} links",
                d.corpus.pages.len(),
                d.corpus.subjects.len(),
                d.corpus.links.len()
            );
            root = root.child(
                div()
                    .absolute()
                    .left(px(chrome::INSET))
                    .bottom(px(chrome::INSET))
                    .text_size(px(chrome::TEXT))
                    .text_color(t.dim)
                    .child(counts),
            );
        }
        if self.reading.is_some() {
            root = root.child(self.sheet(&t, cx));
        }
        root
    }
}

fn report(b: &Bench, pages: usize) {
    let stats = |v: &[f32]| -> String {
        if v.is_empty() {
            return "none".into();
        }
        let mut s = v.to_vec();
        s.sort_by(f32::total_cmp);
        let mean = s.iter().sum::<f32>() / s.len() as f32;
        let at = |q: f32| s[((s.len() - 1) as f32 * q) as usize];
        format!(
            "mean {mean:.2} ms, p50 {:.2}, p95 {:.2}, p99 {:.2}, max {:.2}",
            at(0.5),
            at(0.95),
            at(0.99),
            s[s.len() - 1]
        )
    };
    let fps = if b.frames.is_empty() {
        0.
    } else {
        1000. * b.frames.len() as f32 / b.frames.iter().sum::<f32>()
    };
    eprintln!(
        "bench: {pages} pages, {} frames, {fps:.1} fps\n  frame interval: {}\n  build+paint on the UI thread: {}",
        b.frames.len(),
        stats(&b.frames),
        stats(&b.work)
    );
}

// ---- Overlays over the sky.

impl BrainApp {
    /// The search, and under it what it found, while a page is not being
    /// read over it.
    fn search_panel(&self, t: &Theme, typing: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let mut panel = div()
            .absolute()
            .left(px(chrome::INSET))
            .top(px(chrome::INSET))
            .w(px(search::WIDTH))
            .flex()
            .flex_col()
            .gap(px(6.))
            .occlude()
            .child(
                div()
                    .h(px(search::FIELD_HEIGHT))
                    .flex()
                    .items_center()
                    .px(px(6.))
                    .rounded(px(chrome::RADIUS))
                    .bg(veil(t, chrome::PANEL_ALPHA))
                    .child(self.field.clone()),
            );
        let Some(data) = &self.data else { return panel };
        if self.query.is_empty() {
            return panel;
        }
        if let Some(e) = &self.failed {
            return panel.child(
                div()
                    .px(px(search::ROW_PAD_X))
                    .py(px(search::ROW_PAD_Y))
                    .rounded(px(chrome::RADIUS))
                    .bg(veil(t, chrome::PANEL_ALPHA))
                    .text_size(px(search::SNIPPET))
                    .text_color(t.error)
                    .child(format!("Search did not answer: {e}")),
            );
        }
        if self.results.is_empty() && self.answered == self.asked {
            return panel.child(
                div()
                    .px(px(search::ROW_PAD_X))
                    .py(px(search::ROW_PAD_Y))
                    .rounded(px(chrome::RADIUS))
                    .bg(veil(t, chrome::PANEL_ALPHA))
                    .text_size(px(search::SNIPPET))
                    .text_color(t.dim)
                    .child("Nothing in the brain matches."),
            );
        }
        if self.results.is_empty()
            || self.take == Take::Gather
            || (self.reading.is_some() && !typing)
        {
            return panel;
        }
        let mut list = div()
            .flex()
            .flex_col()
            .py(px(4.))
            .rounded(px(chrome::RADIUS))
            .bg(veil(t, chrome::PANEL_ALPHA));
        for (i, f) in self.results.iter().take(search::ROWS).enumerate() {
            list = list.child(self.result_row(i, f, data, t, false, cx));
        }
        if self.results.len() > search::ROWS {
            list = list.child(
                div()
                    .px(px(search::ROW_PAD_X))
                    .py(px(4.))
                    .text_size(px(search::SNIPPET))
                    .text_color(t.dim)
                    .child(format!(
                        "{} more lit in the sky",
                        self.results.len() - search::ROWS
                    )),
            );
        }
        panel = panel.child(list);
        panel
    }

    fn result_row(
        &self,
        i: usize,
        f: &Found,
        data: &Data,
        t: &Theme,
        tall: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let page = &data.corpus.pages[f.page];
        let hue = data.hues[page.subject];
        let picked = self.pick == Some(i);
        let p = f.page;
        let snippet = snippet_text(&f.snippet, &page.summary, hue, t);
        div()
            .id(("result", i))
            .flex()
            .gap(px(10.))
            .px(px(search::ROW_PAD_X))
            .py(px(search::ROW_PAD_Y))
            .when(tall, |d| d.h(px(gather::SLOT)).py(px(6.)).px(px(0.)))
            .when(picked && !tall, |d| d.bg(alpha(hue, 0.1)))
            .cursor_pointer()
            .hover(|s| s.bg(alpha(hue, 0.08)))
            .on_click(cx.listener(move |me, _, _, cx| {
                me.pick = Some(i);
                me.open(p, cx);
            }))
            .on_hover(cx.listener(move |me, on: &bool, _, cx| {
                let want = on.then_some(p);
                if me.hover != want && (*on || me.hover == Some(p)) {
                    me.hover = want;
                    cx.notify();
                }
            }))
            .when(!tall, |d| {
                d.child(
                    div()
                        .flex_none()
                        .mt(px(5.))
                        .size(px(7.))
                        .rounded(px(7.))
                        .bg(hue),
                )
            })
            .when(tall, |d| {
                d.child(div().flex_none().w(px(gather::TEXT_X - 10.)))
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(search::TITLE))
                                    .text_color(if picked { t.text } else { alpha(t.text, 0.9) })
                                    .when(picked, |d| d.font_weight(FontWeight::BOLD))
                                    .child(page.title.clone()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(px(search::SNIPPET))
                                    .text_color(alpha(hue, 0.85))
                                    .child(page.kind.clone()),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(search::SNIPPET))
                            .line_height(px(search::SNIPPET * 1.45))
                            .text_color(t.dim)
                            .when(!tall, |d| d.truncate())
                            .when(tall, |d| d.line_clamp(2))
                            .child(snippet),
                    ),
            )
    }

    fn gather_column(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let (_, h) = self.size;
        let mut col = div()
            .absolute()
            .left(px(0.))
            .top(px(0.))
            .w(px(gather::LEFT + gather::WIDTH + 80.))
            .h_full()
            .bg(linear_gradient(
                90.,
                linear_color_stop(alpha(t.bg, gather::SCRIM), 0.78),
                linear_color_stop(alpha(t.bg, 0.), 1.),
            ));
        let Some(data) = &self.data else { return col };
        let shown = arrange::slots(h);
        for (i, f) in self.results.iter().take(shown).enumerate() {
            col = col.child(
                div()
                    .absolute()
                    .left(px(gather::LEFT))
                    .top(px(gather::TOP + i as f32 * gather::SLOT))
                    .w(px(gather::WIDTH))
                    .occlude()
                    .child(self.result_row(i, f, data, t, true, cx)),
            );
        }
        if self.results.len() > shown {
            col = col.child(
                div()
                    .absolute()
                    .left(px(gather::LEFT + gather::TEXT_X))
                    .top(px(gather::TOP + shown as f32 * gather::SLOT))
                    .text_size(px(search::SNIPPET))
                    .text_color(t.dim)
                    .child(format!("{} more found", self.results.len() - shown)),
            );
        }
        col
    }

    fn switcher(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let right = if self.reading.is_some() {
            reader::sheet_width(self.size.0) + chrome::INSET
        } else {
            chrome::INSET
        };
        let mut row = div()
            .absolute()
            .top(px(
                chrome::INSET + (search::FIELD_HEIGHT - chrome::SWITCH_HEIGHT) / 2.
            ))
            .right(px(right))
            .h(px(chrome::SWITCH_HEIGHT))
            .flex()
            .items_center()
            .p(px(3.))
            .gap(px(2.))
            .rounded(px(chrome::RADIUS))
            .bg(veil(t, chrome::PANEL_ALPHA))
            .occlude()
            .text_size(px(chrome::TEXT));
        for (n, take) in Take::ALL.into_iter().enumerate() {
            let on = self.take == take;
            row = row.child(
                div()
                    .id(("take", n))
                    .h_full()
                    .flex()
                    .items_center()
                    .px(px(chrome::SWITCH_PAD_X))
                    .rounded(px(chrome::RADIUS - 2.))
                    .cursor_pointer()
                    .when(on, |d| d.bg(alpha(t.accent, 0.28)).text_color(t.text))
                    .when(!on, |d| {
                        d.text_color(t.text2).hover(|s| s.text_color(t.text))
                    })
                    .on_click(cx.listener(move |me, _, _, cx| me.switch(take, cx)))
                    .child(take.name()),
            );
        }
        row
    }

    fn sheet(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let w = reader::sheet_width(self.size.0);
        let back = !self.back.is_empty();
        let bar = div()
            .absolute()
            .top(px(10.))
            .right(px(14.))
            .flex()
            .gap(px(4.))
            .text_size(px(tokens::reader::LEDE))
            .when(back, |d| {
                d.child(
                    div()
                        .id("back")
                        .px(px(8.))
                        .py(px(2.))
                        .rounded(px(4.))
                        .cursor_pointer()
                        .text_color(t.text2)
                        .hover(|s| s.text_color(t.text).bg(t.raised))
                        .on_click(cx.listener(|me, _, _, cx| me.go_back(cx)))
                        .child("←"),
                )
            })
            .child(
                div()
                    .id("close")
                    .px(px(8.))
                    .py(px(2.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .text_color(t.text2)
                    .hover(|s| s.text_color(t.text).bg(t.raised))
                    .on_click(cx.listener(|me, _, _, cx| me.close(cx)))
                    .child("×"),
            );
        div()
            .absolute()
            .top(px(0.))
            .right(px(0.))
            .w(px(w))
            .h_full()
            .occlude()
            .bg(veil(t, tokens::reader::SHEET_ALPHA))
            .border_l_1()
            .border_color(alpha(t.divider, 0.7))
            .shadow(vec![gpui::BoxShadow {
                color: alpha(t.bg, 0.6),
                offset: gpui::point(px(-8.), px(0.)),
                blur_radius: px(tokens::reader::SHADOW),
                spread_radius: px(0.),
                inset: false,
            }])
            .on_mouse_move(|_, _, cx| cx.stop_propagation())
            .on_scroll(|_, _, cx| cx.stop_propagation())
            .child(
                AnyView::from(self.reader.clone()).cached(StyleRefinement::default().size_full()),
            )
            .child(bar)
    }
}

/// A search's snippet with the words it matched lit; the page's summary
/// where the search gave none.
fn snippet_text(snippet: &str, summary: &str, hue: Hsla, t: &Theme) -> gpui::StyledText {
    let src = if snippet.trim().is_empty() {
        summary
    } else {
        snippet
    };
    let mut text = String::new();
    let mut lit = vec![];
    let mut start = None;
    for c in src.chars() {
        match c {
            '»' => start = Some(text.len()),
            '«' => {
                if let Some(s) = start.take() {
                    lit.push(s..text.len());
                }
            }
            '[' | ']' => {}
            '#' if text.trim_end_matches(['…', ' ']).is_empty() => {}
            c => text.push(c),
        }
    }
    let highlights = lit
        .into_iter()
        .map(|r| {
            (
                r,
                gpui::HighlightStyle {
                    color: Some(whiten(hue, 0.3)),
                    ..Default::default()
                },
            )
        })
        .collect::<Vec<_>>();
    let _ = t;
    gpui::StyledText::new(SharedString::from(text)).with_highlights(highlights)
}

pub fn open_window(start: Start, cx: &mut App) {
    let bounds = window_bounds(cx);
    let opened = cx.open_window(
        gpui::WindowOptions {
            window_bounds: Some(gpui::WindowBounds::Windowed(bounds)),
            titlebar: Some(gpui::TitlebarOptions {
                title: Some("Brain".into()),
                ..Default::default()
            }),
            app_id: Some("brain".into()),
            ..Default::default()
        },
        |window, cx| cx.new(|cx| BrainApp::new(start, window, cx)),
    );
    if let Err(e) = opened {
        panic!("the window did not open: {e}");
    }
}

/// `BRAIN_APP_BOUNDS=x,y,w,h` places the window exactly (tests on an
/// invisible display); otherwise it opens centred.
fn window_bounds(cx: &App) -> gpui::Bounds<gpui::Pixels> {
    let given: Option<Vec<f32>> = std::env::var("BRAIN_APP_BOUNDS")
        .ok()
        .map(|v| v.split(',').filter_map(|n| n.trim().parse().ok()).collect());
    match given.as_deref() {
        Some([x, y, w, h]) => {
            gpui::Bounds::new(gpui::point(px(*x), px(*y)), gpui::size(px(*w), px(*h)))
        }
        _ => gpui::Bounds::centered(None, gpui::size(px(1456.), px(939.)), cx),
    }
}
