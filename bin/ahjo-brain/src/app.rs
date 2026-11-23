//! Ahjo Brain's one window: the sky, the search over it and the reader
//! beside it.
//!
//! The sky is the place. A search lights what it found and the camera flies
//! there. Opening a page turns the sky into the page's own orbit: what it
//! links and what links to it, one arc per subject, each arc pointing the
//! way its subject lies in the sky, with the page in a sheet beside it and
//! the ranked results still at hand. A planet clicked becomes the sun. Esc,
//! or going back past the first page, lets the sky back where it was.
//!
//! The sky is drawn from the scene each frame while anything moves, and not
//! at all while nothing does.

use crate::arrange::{self, Orbit};
use crate::camera::{Camera, Flight, ease_in_out, ease_out};
use crate::citadel::{self, Hit};
use crate::corpus::{self, Corpus, Entry};
use crate::editor::{self, Editor, EditorEvent, Mode};
use crate::front;
use crate::history::{self, History};
use crate::inbox;
use crate::mirror::{self, Remote};
use crate::paint::{self, Align, Dot, Face, Label, Light, Line, Picture, Ring, Shape};
use crate::reader::{self, Reader};
use crate::sky::{self, Rect, Role, Sky};
use crate::source::{self, Where};
use crate::tokens::{self, chrome, lift, motion, search, sky as k, veil};
use ahjo_app_kit::theme::{Direction, FAMILY, Theme, alpha, art, size, theme, ty};
use ahjo_app_kit::widgets::field::{self, FieldEvent, TextField};
use ahjo_app_kit::widgets::{kit, pane};
use ahjo_app_kit::zoom::{self, Zoom};
use gpui::{
    AnyElement, AnyView, App, AppContext, Bounds, Context, Entity, FocusHandle, Focusable,
    FontWeight, Hsla, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    PathBuilder, Pixels, ScrollDelta, ScrollEvent, ScrollHandle, SharedString, StyleRefinement,
    Window, canvas, div, img, point, prelude::*, px,
};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Instant, SystemTime};

/// The brain as one launch holds it.
#[derive(Clone)]
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
    /// The pages whose files changed last, newest first, evidence aside.
    pub recent: Vec<usize>,
    /// Per system, how many of its pages moved this week.
    pub moved: Vec<usize>,
    pub max_evidence: usize,
    pub strongest: u32,
    /// The notes caught here waiting in the inbox, newest first.
    pub waiting: Vec<usize>,
}

impl Data {
    /// The subjects' hues in this theme.
    fn hues(corpus: &Corpus, t: &Theme) -> Vec<Hsla> {
        corpus
            .subjects
            .iter()
            .map(|s| tokens::subject_hue(t, &s.id))
            .collect()
    }
}

struct Found {
    page: usize,
    snippet: String,
}

/// The brain's history played over the sky: each day it was worked on, the
/// pages touched flare and stay warm for a while, so the sky shows where the
/// attention went and how it moved.
struct Replay {
    /// Oldest first: a day worked on (Julian) and the pages touched.
    days: Vec<(i32, Vec<usize>)>,
    /// Per page, the days (indices into `days`) it was touched, in order.
    touched: Vec<Vec<u32>>,
    reshuffles: usize,
    /// Where it stands, in days worked on from the first.
    at: f32,
    /// Playing since then, from there.
    playing: Option<(Instant, f32)>,
}

impl Replay {
    fn new(history: History, corpus: &Corpus) -> Replay {
        let mut touched = vec![vec![]; corpus.pages.len()];
        let mut days = vec![];
        for d in history.days {
            let pages: Vec<usize> = d.paths.iter().filter_map(|p| corpus.by_path(p)).collect();
            if pages.is_empty() {
                continue;
            }
            for &p in &pages {
                touched[p].push(days.len() as u32);
            }
            days.push((d.day, pages));
        }
        Replay {
            days,
            touched,
            reshuffles: history.reshuffles,
            at: 0.,
            playing: Some((Instant::now(), 0.)),
        }
    }

    fn last(&self) -> f32 {
        self.days.len().saturating_sub(1) as f32
    }

    /// Days worked on since a page was last touched, as the replay stands;
    /// none if it has not been yet.
    fn since(&self, page: usize) -> Option<f32> {
        let list = &self.touched[page];
        let upto = list.partition_point(|&d| d as f32 <= self.at);
        upto.checked_sub(1).map(|i| self.at - list[i] as f32)
    }

    /// Played on to now; whether it still plays.
    fn advance(&mut self) -> bool {
        let Some((since, from)) = self.playing else {
            return false;
        };
        self.at =
            (from + since.elapsed().as_secs_f32() * 1000. / motion::REPLAY_DAY).min(self.last());
        if self.at >= self.last() {
            self.playing = None;
        }
        self.playing.is_some()
    }

    fn play_pause(&mut self) {
        self.playing = match self.playing {
            Some(_) => None,
            None => {
                let from = if self.at >= self.last() { 0. } else { self.at };
                self.at = from;
                Some((Instant::now(), from))
            }
        };
    }

    fn seek(&mut self, day: usize) {
        self.at = day as f32;
        if self.playing.is_some() {
            self.playing = Some((Instant::now(), self.at));
        }
    }
}

enum Replaying {
    Reading,
    Failed(String),
    Ready(Replay),
}

struct Drag {
    at: (f32, f32),
    camera: Camera,
    moved: bool,
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
    /// When the process started, and how long until the sky first lit.
    launched: Instant,
    lit: Option<f32>,
    /// The second part types a search, a letter at a time: the words, how
    /// many are in, how long each key took to answer from the titles, and
    /// the frames' work while it ran.
    words: Vec<char>,
    typed: usize,
    keys: Vec<f32>,
    work_typing: Vec<f32>,
}

impl Bench {
    pub fn new(seconds: f32, launched: Instant) -> Bench {
        Bench {
            seconds,
            start: None,
            last: None,
            frames: vec![],
            work: vec![],
            painted: Rc::new(std::cell::Cell::new(None)),
            began: None,
            launched,
            lit: None,
            words: vec![],
            typed: 0,
            keys: vec![],
            work_typing: vec![],
        }
    }

    /// The flight takes this share of the run; the typing the rest.
    const FLIGHT: f32 = 0.6;
    /// A key every so many seconds, as a person types.
    const KEY_EVERY: f32 = 0.18;

    /// How many letters should be in by now, once the typing has begun.
    fn due(&self, now: Instant) -> Option<usize> {
        let s = now.duration_since(self.start?).as_secs_f32() - self.seconds * Self::FLIGHT;
        (s >= 0.).then(|| ((s / Self::KEY_EVERY) as usize + 1).min(self.words.len()))
    }
}

/// What the app is told at launch.
pub struct Start {
    pub place: Where,
    /// Another machine's brain to read instead of this one's.
    pub from: Option<Remote>,
    /// The app's settings file, what it held, and why it did not read.
    pub settings: (Option<PathBuf>, source::Settings, Option<String>),
    pub query: Option<String>,
    pub open: Option<String>,
    pub bench: Option<Bench>,
}

pub struct BrainApp {
    /// Its updates from the node it was installed from, if it was.
    updates: Entity<ahjo_app_kit::update::Updates>,
    /// Where the brain read is: this machine's, or another's copy.
    place: Where,
    /// This machine's brain, as the launch found it.
    here: Where,
    /// The other machines a brain can be read from, as Swarm last saw them,
    /// and whether their list is open.
    others: Vec<Remote>,
    choosing: bool,
    /// Another machine's copy is being brought up to date.
    syncing: bool,
    /// Why the copy read is as last copied: the other machine did not
    /// answer.
    away_from: Option<String>,
    data: Option<Rc<Data>>,
    entries: Vec<Entry>,
    marker: Option<SystemTime>,
    status: Option<String>,
    field: Entity<TextField>,
    query: String,
    asked: u64,
    answered: u64,
    results: Vec<Found>,
    /// Why Citadel did not answer the last search, which was then matched
    /// here against titles and aliases.
    away: Option<String>,
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
    size: (f32, f32),
    bench: Option<Bench>,
    /// A page opened: the keyboard leaves the search for the page.
    leave_field: bool,
    /// The search field has the keys, as of the last frame: what changed
    /// lately then shows under it.
    typing: bool,
    /// Evidence left out of a page's orbit, counted at its edge.
    fold_evidence: bool,
    replay: Option<Replaying>,
    settings_path: Option<PathBuf>,
    settings: source::Settings,
    settings_seen: Option<SystemTime>,
    /// Why the settings file did not read; while it does not, it is not
    /// written over.
    settings_broken: Option<String>,
    /// The reader's body size, zoomed by the one zoom rule.
    reader_text: f32,
    reader_wheel: Rc<std::cell::Cell<zoom::Wheel>>,
    /// The results' list, scrolled to the row stood on.
    results_scroll: ScrollHandle,
    /// A word the app says for a moment (the zoom), and which saying it is.
    said: Option<String>,
    said_gen: u64,
    /// The whole app's size in percent, by the one zoom rule: Ctrl+wheel
    /// and the zoom keys anywhere but the page read.
    app_size: u16,
    app_wheel: Rc<std::cell::Cell<zoom::Wheel>>,
    /// The page read, being written in place beside itself.
    editing: Option<Entity<Editor>>,
    /// A note over the sky: one caught, the day's, or one from the inbox.
    note: Option<Entity<Editor>>,
    /// Notes closed while their save runs, to tell how it went.
    filing: Vec<Entity<Editor>>,
    /// A note closed that its save did not file: Ctrl+N brings it back.
    held: Option<Entity<Editor>>,
    /// The inbox's list is open, and the row stood on.
    inbox: Option<usize>,
    /// The notes caught here that agents filed, as the brain's history last
    /// told it, or why it did not.
    filed: Option<Result<Vec<inbox::Filed>, String>>,
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
        // A draft is kept in the cache as it is typed, so a restart for an
        // update loses nothing and needs no guard.
        let updates = cx.new(|cx| {
            let mut u = ahjo_app_kit::update::Updates::new(env!("AHJO_VERSION"));
            u.check(cx);
            u
        });
        cx.observe(&updates, |_, _, cx| cx.notify()).detach();
        cx.observe_window_activation(window, |me, window, cx| {
            if window.is_window_active() {
                me.updates.update(cx, |u, cx| u.check(cx));
                me.look_again(cx);
            }
        })
        .detach();
        field::focus(&field, window, cx);
        if let Some(q) = &start.query {
            field.update(cx, |f, cx| f.set(q, false, cx));
        }
        let (settings_path, settings, settings_broken) = start.settings;
        let reader_text = reader_size(&settings);
        reader.update(cx, |r, cx| r.set_text(reader_text, cx));
        // Its own size once zoomed here, else the one chosen in Swarm.
        let app_size = settings
            .app_size
            .unwrap_or_else(|| (ahjo_app_kit::theme::chosen_scale() * 100.).round() as u16);
        window.set_ui_scale(zoom::app::scale(app_size), cx);
        let others = crate::swarm::places();
        let mut app = BrainApp {
            updates,
            app_size,
            app_wheel: Rc::default(),
            reader_text,
            reader_wheel: Rc::default(),
            results_scroll: ScrollHandle::new(),
            said: None,
            said_gen: 0,
            settings_seen: settings_path.as_deref().and_then(source::changed),
            fold_evidence: true,
            replay: None,
            settings_path,
            settings,
            settings_broken,
            here: start.place.clone(),
            place: match start.from {
                // Named as Swarm names it, when Swarm knows it.
                Some(remote) => start.place.reading(
                    others
                        .iter()
                        .find(|r| r.url == remote.url)
                        .cloned()
                        .unwrap_or(remote),
                ),
                None => start.place,
            },
            others,
            choosing: false,
            syncing: false,
            away_from: None,
            data: None,
            entries: vec![],
            marker: None,
            status: Some("Looking for the brain…".into()),
            field,
            query: String::new(),
            asked: 0,
            answered: 0,
            results: vec![],
            away: None,
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
            size: (1440., 900.),
            bench: start.bench,
            leave_field: false,
            // The field takes the keys from the start (above).
            typing: true,
            editing: None,
            note: None,
            filing: vec![],
            held: None,
            inbox: None,
            filed: None,
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
        let from = self.place.from.clone();
        self.syncing = from.is_some();
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
                let _ = me.update(cx, |me, cx| {
                    if me.place.brain.as_ref() == Some(&root) {
                        me.arrive(built, entries, cx)
                    }
                });
            } else {
                let _ = me.update(cx, |me, cx| {
                    me.status = Some(match &from {
                        Some(r) => format!("Copying {}'s brain to this machine…", r.name),
                        None => "Reading the brain for the first time…".into(),
                    });
                    cx.notify();
                });
            }
            if let (Some(remote), Some(home)) = (from.clone(), cache.clone()) {
                let synced = cx
                    .background_spawn(async move { mirror::sync(&remote.url, &home) })
                    .await;
                let stop = me
                    .update(cx, |me, cx| {
                        if me.place.brain.as_ref() != Some(&root) {
                            return true;
                        }
                        me.syncing = false;
                        me.away_from = synced.as_ref().err().cloned();
                        let Err(e) = synced else { return false };
                        if kept.entries.is_empty() {
                            me.status = Some(format!("{}'s brain did not come: {e}", remote.name));
                            cx.notify();
                            return true;
                        }
                        cx.notify();
                        false
                    })
                    .unwrap_or(true);
                if stop {
                    return;
                }
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
                        if me.place.brain.as_ref() != Some(&root) {
                            return;
                        }
                        me.status = Some(format!("The brain did not read: {e}"));
                        cx.notify();
                    });
                    return;
                }
            };
            if entries.is_empty() && kept.entries.is_empty() {
                // Read, and nothing in it: say so instead of reading on.
                let _ = me.update(cx, |me, cx| {
                    if me.place.brain.as_ref() != Some(&root) {
                        return;
                    }
                    me.status = Some(no_pages(&root));
                    cx.notify();
                });
                return;
            }
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
            let _ = me.update(cx, |me, cx| {
                if me.place.brain.as_ref() == Some(&root) {
                    me.arrive(built, entries, cx)
                }
            });
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

    /// The window came forward: if the settings file or the brain's Git
    /// index moved, read them again.
    fn look_again(&mut self, cx: &mut Context<Self>) {
        self.settings_again(cx);
        let Some(root) = self.place.brain.clone() else {
            return;
        };
        if self.data.is_none() {
            return;
        }
        if let Some(remote) = self.place.from.clone() {
            self.sync_again(remote, cx);
            return;
        }
        let now = source::marker(&root);
        if now == self.marker {
            return;
        }
        self.marker = now;
        self.rescan(root, cx);
    }

    /// Reads the brain of `remote`, or this machine's: the sky, the lists and
    /// the search start again from it.
    fn read_from(&mut self, remote: Option<Remote>, cx: &mut Context<Self>) {
        self.choosing = false;
        if self.place.from == remote {
            cx.notify();
            return;
        }
        self.close(cx);
        self.back.clear();
        self.field.update(cx, |f, cx| f.set("", false, cx));
        self.query.clear();
        self.results.clear();
        self.pick = None;
        self.replay = None;
        self.hover = None;
        self.hover_system = None;
        self.flight = None;
        self.open_on_arrival = None;
        self.data = None;
        self.entries.clear();
        self.framed = false;
        self.syncing = false;
        self.away_from = None;
        self.place = match remote {
            Some(r) => self.here.reading(r),
            None => self.here.clone(),
        };
        self.status = Some("Looking for the brain…".into());
        self.load(cx);
        cx.notify();
    }

    /// Opens or closes the list of machines whose brain can be read.
    fn choose_from(&mut self, cx: &mut Context<Self>) {
        self.choosing = !self.choosing;
        if self.choosing {
            self.others = crate::swarm::places();
        }
        cx.notify();
    }

    /// Brings another machine's copy up to date, and reads it again when
    /// anything came.
    fn sync_again(&mut self, remote: Remote, cx: &mut Context<Self>) {
        let (Some(home), Some(root)) = (self.place.cache.clone(), self.place.brain.clone()) else {
            return;
        };
        if self.syncing {
            return;
        }
        self.syncing = true;
        cx.spawn(async move |me, cx| {
            let url = remote.url.clone();
            let synced = cx
                .background_spawn(async move { mirror::sync(&url, &home) })
                .await;
            let _ = me.update(cx, |me, cx| {
                me.syncing = false;
                // Another brain was chosen meanwhile.
                if me.place.from.as_ref() != Some(&remote) {
                    return;
                }
                me.away_from = synced.as_ref().err().cloned();
                match synced {
                    Ok(s) if s.changed() => me.rescan(root, cx),
                    _ => cx.notify(),
                }
            });
        })
        .detach();
    }

    /// Reads the brain at `root` again and shows what changed.
    fn rescan(&mut self, root: PathBuf, cx: &mut Context<Self>) {
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
            let _ = me.update(cx, |me, cx| {
                if me.place.brain.as_ref() == Some(&root) {
                    me.arrive(built, entries, cx)
                }
            });
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
            hues: Data::hues(&corpus, &t),
            recent: {
                let mut r: Vec<usize> = (0..corpus.pages.len())
                    .filter(|&p| !corpus.pages[p].is_evidence())
                    .collect();
                r.sort_by_key(|&p| std::cmp::Reverse(corpus.pages[p].modified));
                r.truncate(search::RECENT);
                r
            },
            moved: sky
                .systems
                .iter()
                .map(|s| {
                    corpus.subjects[s.subject]
                        .pages
                        .iter()
                        .filter(|&&p| {
                            corpus.pages[p]
                                .updated
                                .is_some_and(|u| today - u < k::MOVED_DAYS)
                        })
                        .count()
                })
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
            waiting: {
                let mut w: Vec<usize> = (0..corpus.pages.len())
                    .filter(|&p| front::waiting(&corpus.pages[p].path))
                    .collect();
                // Newest first, by the day the note gives, then its file:
                // a brain fetched anew has every file new.
                w.sort_by_key(|&p| {
                    let page = &corpus.pages[p];
                    std::cmp::Reverse((page.updated, page.modified))
                });
                w
            },
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
        if matches!(self.replay, Some(Replaying::Ready(_))) {
            self.replay = None;
        }
        self.system_light = vec![0.; data.sky.systems.len()];
        self.bridge_light = vec![0.; data.sky.bridges.len()];
        self.anchor = vec![None; n];
        self.shown = vec![(0., 0.); n];
        self.entries = entries;
        self.status = None;
        self.data = Some(data.clone());
        if let Some(b) = &mut self.bench
            && b.lit.is_none()
        {
            b.lit = Some(b.launched.elapsed().as_secs_f32() * 1000.);
        }
        if !self.framed {
            self.framed = true;
            self.camera = Camera::fit(data.sky.bounds, self.free(), k::FIT_MOST, k::FIT_PAD);
        }
        if let Some(p) = self.reading {
            self.reader.update(cx, |r, cx| r.show(data.clone(), p, cx));
            // Written meanwhile: the page beside the source stays as written.
            if let Some(ed) = &self.editing {
                let text = ed.read(cx).text(cx).to_string();
                self.reader.update(cx, |r, cx| r.preview(text, cx));
            }
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

    // ---- The theme.

    fn settings_again(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.settings_path.clone() else {
            return;
        };
        let now = source::changed(&path);
        if now == self.settings_seen {
            return;
        }
        self.settings_seen = now;
        match source::read_settings(&path) {
            Ok(s) => {
                self.settings_broken = None;
                let want = Direction::from_key(&s.theme);
                self.reader_text = reader_size(&s);
                let text = self.reader_text;
                self.reader.update(cx, |r, cx| r.set_text(text, cx));
                self.settings = s;
                if let Some(d) = want {
                    self.restyle(d, cx);
                }
            }
            Err(e) => self.settings_broken = Some(e),
        }
        cx.notify();
    }

    /// The next of the theme's directions, kept in the settings.
    fn next_theme(&mut self, cx: &mut Context<Self>) {
        let next = theme(cx).direction.step(1);
        self.restyle(next, cx);
        if self.settings_broken.is_some() {
            return;
        }
        let Some(path) = self.settings_path.clone() else {
            return;
        };
        self.settings.theme = next.key().into();
        match source::write_settings(&path, &self.settings) {
            Ok(()) => self.settings_seen = source::changed(&path),
            Err(e) => {
                self.settings_broken = Some(format!("{} was not written: {e}", path.display()))
            }
        }
    }

    /// How much the reader is zoomed from its own size.
    fn reader_k(&self) -> f32 {
        self.reader_text / tokens::reader::BODY
    }

    /// The one zoom rule on the page read: its text `n` steps bigger or
    /// smaller, or back; kept, and said.
    fn zoom_reader(&mut self, z: Zoom, n: i32, cx: &mut Context<Self>) {
        let at = self.reader_text;
        let (least, most) = (tokens::reader::TEXT_LEAST, tokens::reader::TEXT_MOST);
        let to = match z {
            Zoom::Reset => (at != tokens::reader::BODY).then_some(tokens::reader::BODY),
            Zoom::In => zoom::step(at, n, 1., least, most),
            Zoom::Out => zoom::step(at, -n, 1., least, most),
        };
        let Some(to) = to else { return };
        self.reader_text = to;
        self.reader.update(cx, |r, cx| r.set_text(to, cx));
        let reset = if to == tokens::reader::BODY {
            ""
        } else {
            " · Ctrl+0 resets"
        };
        self.say(format!("Text {to} px{reset}"), cx);
        // The orbit gives the wider sheet its room.
        self.replanned = true;
        self.settings.reader_text = (to != tokens::reader::BODY).then_some(to);
        self.keep_settings();
        cx.notify();
    }

    /// The one zoom rule anywhere but the page read: the whole app `n`
    /// steps bigger or smaller, or back to 100%; kept, and said.
    fn zoom_app(&mut self, z: Zoom, n: i32, window: &mut Window, cx: &mut Context<Self>) {
        let Some(to) = zoom::app::zoomed(self.app_size, z, n) else {
            return;
        };
        self.app_size = to;
        window.set_ui_scale(zoom::app::scale(to), cx);
        self.say(zoom::app::words(to), cx);
        self.replanned = true;
        self.settings.app_size = Some(to);
        self.keep_settings();
        cx.notify();
    }

    /// What the app says for a moment, low in the middle of what it is
    /// about: the page read, else the window.
    fn said_view(&self, t: &Theme) -> Option<gpui::Div> {
        self.said.clone().map(|words| {
            div()
                .absolute()
                .bottom(px(chrome::INSET * 2.))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(
                    div()
                        .px(px(chrome::SWITCH_PAD_X))
                        .py(px(6.))
                        .rounded(px(chrome::RADIUS))
                        .bg(t.raised)
                        .border_1()
                        .border_color(t.divider)
                        .text_size(px(size::TEXT))
                        .text_color(t.text)
                        .child(words),
                )
        })
    }

    /// Writes the settings, unless the file did not read.
    fn keep_settings(&mut self) {
        if self.settings_broken.is_none()
            && let Some(path) = self.settings_path.clone()
        {
            match source::write_settings(&path, &self.settings) {
                Ok(()) => self.settings_seen = source::changed(&path),
                Err(e) => {
                    self.settings_broken = Some(format!("{} was not written: {e}", path.display()))
                }
            }
        }
    }

    /// The pointer is over the page read, where zooming sizes its text.
    fn over_sheet(&self, window: &Window) -> bool {
        self.reading.is_some()
            && f32::from(window.mouse_position().x)
                >= self.size.0 - reader::sheet_width(self.size.0, self.reader_k())
    }

    /// Says `words` for a moment.
    fn say(&mut self, words: String, cx: &mut Context<Self>) {
        self.said = Some(words);
        self.said_gen += 1;
        cx.notify();
        let said = self.said_gen;
        cx.spawn(async move |me, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(motion::SAID))
                .await;
            let _ = me.update(cx, |me, cx| {
                if me.said_gen == said {
                    me.said = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// The app in another of the theme's directions: its colours, its
    /// letters and the sky's character.
    fn restyle(&mut self, d: Direction, cx: &mut Context<Self>) {
        if theme(cx).direction == d {
            return;
        }
        cx.set_global(Theme::new(d));
        let t = theme(cx).clone();
        if let Some(old) = &self.data {
            let mut data = (**old).clone();
            data.hues = Data::hues(&data.corpus, &t);
            let data = Rc::new(data);
            self.data = Some(data.clone());
            if let Some(p) = self.reading {
                self.reader.update(cx, |r, cx| r.show(data, p, cx));
            }
        }
        cx.refresh_windows();
    }

    // ---- Search.

    /// The bench's second part: the next letter of its search, when due.
    fn bench_types(&mut self, cx: &mut Context<Self>) {
        let Some(b) = &self.bench else { return };
        let Some(due) = b.due(Instant::now()).filter(|&d| d > b.typed) else {
            return;
        };
        let text: String = b.words[..due].iter().collect();
        self.field.update(cx, |f, cx| f.set(&text, false, cx));
        let began = Instant::now();
        self.typed(cx);
        if let Some(b) = &mut self.bench {
            b.typed = due;
            b.keys.push(began.elapsed().as_secs_f32() * 1000.);
        }
    }

    fn typed(&mut self, cx: &mut Context<Self>) {
        let q = self.field.read(cx).text.trim().to_string();
        if q == self.query {
            return;
        }
        self.query = q.clone();
        self.asked += 1;
        self.results_scroll.set_offset(gpui::point(px(0.), px(0.)));
        let asked = self.asked;
        if q.is_empty() {
            self.answered = asked;
            self.results.clear();
            self.away = None;
            self.pick = None;
            self.replanned = true;
            cx.notify();
            return;
        }
        // The titles and aliases answer at once, on every key; Citadel's
        // ranking and its passages take their place when they come.
        if let Some(data) = self.data.clone() {
            self.results = self.local(&data);
            self.pick = (!self.results.is_empty()).then_some(0);
            self.replanned = true;
            cx.notify();
        }
        let url = self.place.citadel.clone();
        let from = self.place.from.as_ref().map(|r| r.url.clone());
        cx.spawn(async move |me, cx| {
            // Typing on: only the words it rests on are asked.
            cx.background_executor()
                .timer(std::time::Duration::from_millis(motion::SEARCH_WAIT))
                .await;
            if me.read_with(cx, |me, _| me.asked != asked).unwrap_or(true) {
                return;
            }
            let hits = cx
                .background_spawn(async move {
                    match from {
                        Some(node) => mirror::search(&node, &q),
                        None => citadel::search(&url, &q),
                    }
                })
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

    /// What the titles and aliases answer to the words asked.
    fn local(&self, data: &Data) -> Vec<Found> {
        data.corpus
            .local_search(&self.query, search::LOCAL_MOST)
            .into_iter()
            .map(|page| Found {
                page,
                snippet: String::new(),
            })
            .collect()
    }

    fn found(&mut self, hits: Result<Vec<Hit>, String>, cx: &mut Context<Self>) {
        let Some(data) = self.data.clone() else {
            return;
        };
        // The row he stands on stays his when the ranking arrives.
        let stood = self
            .pick
            .filter(|&i| i > 0)
            .and_then(|i| self.results.get(i))
            .map(|f| f.page);
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
                self.away = None;
            }
            Err(e) => {
                // Citadel away: the titles and aliases still answer.
                self.results = self.local(&data);
                self.away = Some(e);
            }
        }
        self.pick = stood
            .and_then(|p| self.results.iter().position(|f| f.page == p))
            .or((!self.results.is_empty()).then_some(0));
        self.replanned = true;
        if !self.orbiting() {
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
        cx.notify();
    }

    fn step(&mut self, by: isize, cx: &mut Context<Self>) {
        if self.results.is_empty() {
            return;
        }
        let at = self.pick.unwrap_or(0) as isize + by;
        let pick = at.rem_euclid(self.results.len() as isize) as usize;
        self.pick = Some(pick);
        self.results_scroll.scroll_to_item(pick);
        if !self.orbiting() {
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
        if self.reading != Some(page) {
            self.stop_editing(cx);
        }
        self.reading = Some(page);
        self.link = None;
        self.hover = None;
        self.trace = Some((page, Instant::now()));
        self.replanned = true;
        self.reader.update(cx, |r, cx| r.show(data, page, cx));
        self.leave_field = true;
        cx.notify();
    }

    /// Hands the page read to Ahjo Swarm's Send, where its session is
    /// picked; says how it went.
    fn send_page(&mut self, cx: &mut Context<Self>) {
        let (Some(data), Some(p)) = (self.data.clone(), self.reading) else {
            return;
        };
        let path = &data.corpus.pages[p].path;
        // Another machine's page is named where it lives; its file there
        // is not known here.
        let text = match &self.place.from {
            Some(r) => format!("brain page {path} on {}", r.name),
            None => crate::swarm::reference(&crate::swarm::file_on_node(
                &data.root.to_string_lossy(),
                path,
            )),
        };
        let sent = cx.background_spawn(async move {
            let node = crate::swarm::node();
            crate::swarm::send(&node, &text)
        });
        cx.spawn(async move |me, cx| {
            let words = match sent.await {
                Ok(()) => "In Ahjo Swarm's Send: pick the session there.".to_string(),
                Err(e) => format!("Not sent: {e}"),
            };
            let _ = me.update(cx, |me, cx| me.say(words, cx));
        })
        .detach();
    }

    // ---- Editing the page read in place.

    /// The file of the page at `path` in the brain.
    pub fn file_of(&self, path: &str) -> Option<PathBuf> {
        self.data.as_ref().map(|d| d.root.join(path))
    }

    /// Writes the page read in place: its file (and a draft kept from
    /// before) read off the UI thread, then the source beside the page.
    fn edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(data), Some(p)) = (self.data.clone(), self.reading) else {
            return;
        };
        if self.editing.is_some() {
            return;
        }
        if let Some(remote) = &self.place.from {
            let words = format!(
                "This is {}'s brain, read only: write it there.",
                remote.name
            );
            self.say(words, cx);
            return;
        }
        let page = data.corpus.pages[p].clone();
        let file = data.root.join(&page.path);
        let cache = self.place.cache.clone();
        let names = link_names(&data, Some(p));
        let path = page.path.clone();
        let read = cx.background_spawn(async move {
            let base = std::fs::read(&file)
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .map_err(|e| format!("{}: {e}", file.display()))?;
            let draft = cache.as_ref().and_then(|c| editor::read_draft(c, &path));
            Ok::<_, String>((base, draft))
        });
        let citadel = self.place.citadel.clone();
        let cache = self.place.cache.clone();
        let window_handle = window.window_handle();
        cx.spawn(async move |me, cx| {
            let read = read.await;
            let _ = cx.update_window(window_handle, |_, window, cx| {
                let _ = me.update(cx, |me, cx| {
                    let (base, draft) = match read {
                        Ok(r) => r,
                        Err(e) => {
                            me.say(format!("This page did not read: {e}"), cx);
                            return;
                        }
                    };
                    if me.reading != Some(p) || me.editing.is_some() {
                        return;
                    }
                    let opened = editor::Opened {
                        path: page.path.clone(),
                        title: page.title.clone(),
                        base,
                        draft,
                        mode: Mode::Page,
                        fresh: None,
                    };
                    let root = data.root.clone();
                    let ed =
                        cx.new(|cx| Editor::new(root, opened, names, citadel, cache, window, cx));
                    cx.subscribe_in(&ed, window, |me, ed, ev, window, cx| match ev {
                        EditorEvent::Changed => {
                            let text = ed.read(cx).text(cx).to_string();
                            me.reader.update(cx, |r, cx| r.preview(text, cx));
                        }
                        EditorEvent::Saved => me.look_again(cx),
                        EditorEvent::NotSaved => {}
                        EditorEvent::Leave => {
                            me.stop_editing(cx);
                            window.focus(&me.focus, cx);
                        }
                    })
                    .detach();
                    let text = ed.read(cx).text(cx).to_string();
                    if text != ed.read(cx).saved_text() {
                        me.reader.update(cx, |r, cx| r.preview(text, cx));
                    }
                    let code = ed.read(cx).code.clone();
                    ahjo_app_kit::widgets::code::Code::focus(&code, window, cx);
                    me.editing = Some(ed);
                    cx.notify();
                });
            });
        })
        .detach();
    }

    // ---- Notes over the sky.

    /// Whether notes can be written here: another machine's brain is read
    /// only, and says so.
    fn may_write(&mut self, cx: &mut Context<Self>) -> bool {
        if self.data.is_none() {
            return false;
        }
        if let Some(remote) = &self.place.from {
            let words = format!(
                "Notes are caught in a brain on its own machine; {}'s is read only from here.",
                remote.name
            );
            self.say(words, cx);
            return false;
        }
        true
    }

    /// Catches a note: an empty page over the sky, its first line its
    /// title, filed in the inbox when it is closed, where agents file it
    /// into the brain. A note open is filed first. One left unfiled, or
    /// not filed before the app closed, comes back.
    fn catch_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.may_write(cx) {
            return;
        }
        self.inbox = None;
        if self.note.is_some() {
            self.leave_note(window, cx);
        }
        if let Some(ed) = self.held.take() {
            self.show_note(ed, window, cx);
            return;
        }
        let day = today().to_string();
        let start = front::join(&front::note_head(&day), front::NOTE_START);
        let filing: Vec<String> = self
            .filing
            .iter()
            .map(|ed| ed.read(cx).begun_as().to_string())
            .collect();
        let kept = self
            .place
            .cache
            .as_ref()
            .and_then(|c| editor::caught_draft(c, &filing));
        let path = kept
            .as_ref()
            .map_or_else(editor::caught_key, |d| d.path.clone());
        let draft = kept.unwrap_or(editor::Draft {
            path: path.clone(),
            base: String::new(),
            text: start.clone(),
        });
        self.open_note(
            editor::Opened {
                path,
                title: String::new(),
                base: String::new(),
                draft: Some(draft),
                mode: Mode::Note,
                fresh: Some(editor::Fresh { named: true, start }),
            },
            window,
            cx,
        );
    }

    /// The day's note: one page a day in the inbox, its heading the day,
    /// begun when it is not there yet.
    fn day_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.may_write(cx) {
            return;
        }
        let now = today();
        let path = front::daily_path(&now.to_string());
        if let Some(ed) = &self.note
            && ed.read(cx).path() == path
        {
            let code = ed.read(cx).code.clone();
            ahjo_app_kit::widgets::code::Code::focus(&code, window, cx);
            return;
        }
        let heading = format!(
            "{} {} {} {}",
            now.weekday(),
            now.day(),
            now.month(),
            now.year()
        );
        let start = front::join(
            &front::note_head(&now.to_string()),
            &front::daily_start(&heading),
        );
        self.open_note_at(path, Some(start), window, cx);
    }

    /// Opens the note at `path` over the sky, read from the brain; one not
    /// there yet begins as `start`.
    fn open_note_at(
        &mut self,
        path: String,
        start: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(file) = self.file_of(&path) else {
            return;
        };
        self.inbox = None;
        if self.note.is_some() {
            self.leave_note(window, cx);
        }
        // The note still being filed, or kept when filing failed, is the
        // one to go on writing: its file may not say all of it yet.
        let same = |ed: &Entity<Editor>, cx: &App| {
            let ed = ed.read(cx);
            ed.path() == path || ed.begun_as() == path
        };
        if let Some(i) = self.filing.iter().position(|ed| same(ed, cx)) {
            let ed = self.filing.remove(i);
            self.show_note(ed, window, cx);
            return;
        }
        if let Some(ed) = self.held.take_if(|ed| same(ed, cx)) {
            self.show_note(ed, window, cx);
            return;
        }
        let base = match std::fs::read(&file) {
            Ok(b) => String::from_utf8_lossy(&b).into_owned(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && start.is_some() => String::new(),
            Err(e) => {
                self.say(format!("This note did not read: {e}"), cx);
                return;
            }
        };
        let kept = self
            .place
            .cache
            .as_ref()
            .and_then(|c| editor::read_draft(c, &path));
        let fresh = match (base.is_empty(), start) {
            (true, Some(start)) => Some(editor::Fresh {
                named: false,
                start,
            }),
            _ => None,
        };
        let draft = match &fresh {
            Some(f) => Some(kept.unwrap_or(editor::Draft {
                path: path.clone(),
                base: String::new(),
                text: f.start.clone(),
            })),
            None => kept,
        };
        let title = front::title(front::split(&base).1);
        self.open_note(
            editor::Opened {
                path,
                title,
                base,
                draft,
                mode: Mode::Note,
                fresh,
            },
            window,
            cx,
        );
    }

    /// A note's editor over the sky, the keys its.
    fn open_note(&mut self, opened: editor::Opened, window: &mut Window, cx: &mut Context<Self>) {
        let Some(data) = self.data.clone() else {
            return;
        };
        let names = link_names(&data, data.corpus.by_path(&opened.path));
        let (citadel, cache) = (self.place.citadel.clone(), self.place.cache.clone());
        let root = data.root.clone();
        let ed = cx.new(|cx| Editor::new(root, opened, names, citadel, cache, window, cx));
        cx.subscribe_in(&ed, window, |me, ed, ev, window, cx| match ev {
            EditorEvent::Changed => {}
            EditorEvent::Saved => {
                // Closed while it saved: said here, as the pane is gone.
                if let Some(i) = me.filing.iter().position(|f| f == ed) {
                    me.filing.remove(i);
                    let title = ed.read(cx).title(cx);
                    me.say(format!("Filed in the inbox: {title}"), cx);
                }
                me.look_again(cx);
            }
            EditorEvent::NotSaved => {
                if let Some(i) = me.filing.iter().position(|f| f == ed) {
                    me.filing.remove(i);
                    me.held = Some(ed.clone());
                    me.say(
                        "Your note was not filed, and is kept: Ctrl+N opens it again.".into(),
                        cx,
                    );
                }
            }
            EditorEvent::Leave => me.leave_note(window, cx),
        })
        .detach();
        self.show_note(ed, window, cx);
    }

    fn show_note(&mut self, ed: Entity<Editor>, window: &mut Window, cx: &mut Context<Self>) {
        let code = ed.read(cx).code.clone();
        ahjo_app_kit::widgets::code::Code::focus(&code, window, cx);
        self.note = Some(ed);
        cx.notify();
    }

    /// Closes the note, filing what was written in it: the pane goes at
    /// once and the save goes on, told when it ends. A note begun and left
    /// empty leaves nothing behind.
    fn leave_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ed) = self.note.take() else {
            return;
        };
        ed.update(cx, |ed, cx| {
            ed.keep(cx);
            if ed.written(cx) {
                ed.save(cx);
            }
        });
        if ed.read(cx).is_saving() {
            self.filing.push(ed);
        }
        match &self.editing {
            Some(page) => {
                let code = page.read(cx).code.clone();
                ahjo_app_kit::widgets::code::Code::focus(&code, window, cx);
            }
            None => window.focus(&self.focus, cx),
        }
        cx.notify();
    }

    // ---- The inbox.

    /// Opens the inbox's list, reading again which notes agents filed, or
    /// closes it.
    fn toggle_inbox(&mut self, cx: &mut Context<Self>) {
        if self.inbox.take().is_some() {
            cx.notify();
            return;
        }
        let Some(data) = self.data.clone() else {
            return;
        };
        self.inbox = Some(0);
        // The list takes the keys, not the search.
        self.leave_field = true;
        if self.place.from.is_none() {
            let root = data.root.clone();
            let read = cx.background_spawn(async move { inbox::read(&root) });
            cx.spawn(async move |me, cx| {
                let filed = read.await;
                let _ = me.update(cx, |me, cx| {
                    me.filed = Some(filed);
                    cx.notify();
                });
            })
            .detach();
        }
        cx.notify();
    }

    /// The inbox's rows: the notes waiting, then those filed, each with
    /// what picking it does.
    fn inbox_rows(&self) -> Vec<InboxRow> {
        let Some(data) = &self.data else {
            return vec![];
        };
        let mut rows: Vec<InboxRow> = data.waiting.iter().map(|&p| InboxRow::Waiting(p)).collect();
        if let Some(Ok(filed)) = &self.filed {
            rows.extend(
                filed
                    .iter()
                    .take(search::RECENT)
                    .cloned()
                    .map(InboxRow::Filed),
            );
        }
        rows
    }

    fn pick_inbox(&mut self, row: InboxRow, window: &mut Window, cx: &mut Context<Self>) {
        self.inbox = None;
        let Some(data) = self.data.clone() else {
            return;
        };
        match row {
            InboxRow::Waiting(p) if self.place.from.is_none() => {
                let path = data.corpus.pages[p].path.clone();
                self.open_note_at(path, None, window, cx);
            }
            InboxRow::Waiting(p) => self.open(p, cx),
            InboxRow::Filed(f) => match f.to.first().and_then(|to| data.corpus.by_path(to)) {
                Some(p) => self.open(p, cx),
                None => self.say(format!("{} is no longer in the brain.", f.title), cx),
            },
        }
        cx.notify();
    }

    /// Back to reading: the page as the brain holds it. A page written and
    /// not saved stays a draft, to come back to.
    fn stop_editing(&mut self, cx: &mut Context<Self>) {
        let Some(ed) = self.editing.take() else {
            return;
        };
        ed.update(cx, |ed, cx| ed.keep(cx));
        if ed.read(cx).unsaved(cx) {
            self.say("Your draft is kept. E writes on.".into(), cx);
        }
        self.reader.update(cx, |r, cx| r.reread(cx));
        cx.notify();
    }

    /// The page read before this one; back past the first lets the sky
    /// back.
    pub fn go_back(&mut self, cx: &mut Context<Self>) {
        match self.back.pop() {
            Some(p) => self.show(p, cx),
            None if self.reading.is_some() => self.close(cx),
            None => {}
        }
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.stop_editing(cx);
        self.reading = None;
        self.back.clear();
        self.trace = None;
        self.link = None;
        self.replanned = true;
        cx.notify();
    }

    /// The next neighbour round the ring of the page read (clockwise for
    /// `by` above zero), lit as if pointed at.
    fn round_the_ring(&mut self, by: isize, cx: &mut Context<Self>) {
        let (Some(data), Some(sun)) = (self.data.clone(), self.reading) else {
            return;
        };
        let o = arrange::orbit(
            &data.corpus,
            &data.sky,
            sun,
            self.free(),
            !self.fold_evidence,
        );
        let ring: Vec<usize> = arrange::clockwise(&o);
        if ring.is_empty() {
            return;
        }
        let n = ring.len() as isize;
        let at = match self.hover.and_then(|h| ring.iter().position(|&p| p == h)) {
            Some(i) => (i as isize + by).rem_euclid(n),
            None if by > 0 => 0,
            None => n - 1,
        };
        self.hover = Some(ring[at as usize]);
        self.link = None;
        cx.notify();
    }

    /// A link in the reader pointed at, or let go.
    pub fn point_at_link(&mut self, page: Option<usize>, cx: &mut Context<Self>) {
        if self.link != page {
            self.link = page;
            cx.notify();
        }
    }

    /// Starts the replay, reading the brain's history first; or ends it.
    fn toggle_replay(&mut self, cx: &mut Context<Self>) {
        if self.replay.take().is_some() {
            cx.notify();
            return;
        }
        let Some(data) = self.data.clone() else {
            return;
        };
        self.replay = Some(Replaying::Reading);
        self.frame_everything();
        let root = data.root.clone();
        cx.spawn(async move |me, cx| {
            let read = cx
                .background_spawn(async move { history::read(&root) })
                .await;
            let _ = me.update(cx, |me, cx| {
                // Ended while it was read, or the brain changed under it.
                if !matches!(me.replay, Some(Replaying::Reading)) {
                    return;
                }
                let Some(data) = me.data.clone() else { return };
                me.replay = Some(match read {
                    Ok(h) => Replaying::Ready(Replay::new(h, &data.corpus)),
                    Err(e) => Replaying::Failed(e),
                });
                // The whole sky again, above the bar now there.
                me.frame_everything();
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn replay_mut(&mut self) -> Option<&mut Replay> {
        match &mut self.replay {
            Some(Replaying::Ready(r)) => Some(r),
            _ => None,
        }
    }

    fn toggle_evidence(&mut self, cx: &mut Context<Self>) {
        self.fold_evidence = !self.fold_evidence;
        self.replanned = true;
        cx.notify();
    }

    // ---- The camera.

    /// The search, narrower in a narrow window so the switch still fits,
    /// and narrow beside an orbit.
    fn search_width(&self) -> f32 {
        if self.aside() {
            // Beside an orbit, as wide as the orbit's room leaves.
            if let Some(rail) = rail_beside_orbit(self.size.0, self.reader_k()) {
                return rail;
            }
        }
        (self.size.0 - chrome::INSET * 3. - chrome::SWITCH_ROOM)
            .clamp(chrome::SEARCH_LEAST, search::WIDTH)
    }

    /// What changed lately, as wide as its longest row needs, while the sky
    /// keeps most of the window; never narrower than the search.
    fn recent_width(&self) -> f32 {
        let least = self.search_width();
        let Some(data) = &self.data else { return least };
        let letter = |size: f32| size * search::MONO_ADVANCE;
        let row = data
            .recent
            .iter()
            .map(|&p| data.corpus.pages[p].title.chars().count())
            .max()
            .unwrap_or(0) as f32
            * letter(search::TITLE)
            // The dot, the gaps, "13 days ago" and the padding.
            + 7. + 20. + 12. * letter(search::SNIPPET) + search::ROW_PAD_X * 2.;
        let most = search::RECENT_MOST.min(self.size.0 * search::RECENT_SHARE);
        row.min(most).max(least)
    }

    /// Whether what changed lately shows under the search: before anything
    /// is asked, with the field taking the keys over the sky.
    fn recent_shown(&self) -> bool {
        self.typing
            && self.query.is_empty()
            && self.reading.is_none()
            && self.replay.is_none()
            && self.data.as_ref().is_some_and(|d| !d.recent.is_empty())
    }

    /// Whether the ranked results stay beside the orbit of the page read:
    /// only when there are some and the orbit keeps its room.
    fn aside(&self) -> bool {
        self.reading.is_some()
            && !self.results.is_empty()
            && rail_beside_orbit(self.size.0, self.reader_k()).is_some()
    }

    /// A page open is the sky turned into its orbit.
    fn orbiting(&self) -> bool {
        self.reading.is_some()
    }

    /// The part of the window the sky shows through: not under the reader,
    /// the results beside it or the search.
    fn free(&self) -> Rect {
        let (w, h) = self.size;
        let right = if self.reading.is_some() {
            w - reader::sheet_width(w, self.reader_k())
        } else {
            w
        };
        // The list under the search covers the sky's left; beside an
        // orbit only when it stays there. So does what changed lately.
        let listed = !self.results.is_empty() && (self.reading.is_none() || self.aside());
        let left = if self.recent_shown() {
            self.recent_width() + chrome::INSET * 2.
        } else if listed {
            self.search_width() + chrome::INSET * 2.
        } else {
            0.
        };
        let top = search::FIELD_HEIGHT + chrome::INSET * 2.;
        // The replay's bar lies over the sky's foot.
        let foot = match &self.replay {
            Some(Replaying::Ready(_)) => tokens::replay::ROOM,
            _ => 0.,
        };
        Rect {
            x: left,
            y: top,
            w: (right - left).max(80.),
            // Above the counts along the bottom.
            h: (h - top - chrome::INSET - chrome::FOOT - foot).max(80.),
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
        if let Some(p) = self.star_at(x, y) {
            self.open(p, cx);
        } else if let Some(s) = self.system_at(x, y) {
            self.fit_system(s);
        }
        cx.notify();
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
        // The one zoom rule: the keys zoom what the wheel would under the
        // pointer, the page read's text over it, else the whole app.
        if let Some(z) = zoom::from_key(ks) {
            if self.over_sheet(window) {
                self.zoom_reader(z, 1, cx);
            } else {
                self.zoom_app(z, 1, window, cx);
            }
            cx.stop_propagation();
            return;
        }
        // From anywhere: Ctrl+N catches a note, Ctrl+D opens the day's.
        if (m.control || m.platform) && !m.alt && !m.shift {
            match ks.key.as_str() {
                "n" => {
                    self.catch_note(window, cx);
                    cx.stop_propagation();
                    return;
                }
                "d" => {
                    self.day_note(window, cx);
                    cx.stop_propagation();
                    return;
                }
                _ => {}
            }
        }
        // A note has the keys.
        if self.note.is_some() {
            return;
        }
        // The inbox's list takes the arrows, Enter and Esc while it is open;
        // Ctrl+I opens it.
        if self.inbox.is_some() && !typing && self.inbox_key(ks.key.as_str(), m, window, cx) {
            cx.stop_propagation();
            return;
        }
        if (m.control || m.platform)
            && !m.alt
            && !m.shift
            && ks.key == "i"
            && self.editing.is_none()
        {
            self.toggle_inbox(cx);
            cx.stop_propagation();
            return;
        }
        // Writing in place, the keys are the source's; Ctrl+E goes back to
        // reading as it came.
        if self.editing.is_some() {
            if (m.control || m.platform) && !m.alt && ks.key == "e" {
                self.stop_editing(cx);
                window.focus(&self.focus, cx);
                cx.stop_propagation();
            }
            return;
        }
        // E (or Ctrl+E) writes the page read in place.
        if self.reading.is_some() && !typing && ks.key == "e" && !m.alt && !m.shift {
            self.edit(window, cx);
            cx.stop_propagation();
            return;
        }
        // The page read scrolls from the keyboard as any page does; Left
        // and Right go round its ring.
        if self.reading.is_some() && !typing && !m.control && !m.alt && !m.platform {
            let reader = self.reader.clone();
            let step = reader.read(cx).arrow_step();
            let moved = match ks.key.as_str() {
                "pagedown" => Some(Scroll::Pages(1.)),
                "pageup" => Some(Scroll::Pages(-1.)),
                "space" if self.replay.is_some() => None,
                "space" if m.shift => Some(Scroll::Pages(-1.)),
                "space" => Some(Scroll::Pages(1.)),
                "down" => Some(Scroll::By(step)),
                "up" => Some(Scroll::By(-step)),
                "home" => Some(Scroll::End(false)),
                "end" => Some(Scroll::End(true)),
                _ => None,
            };
            if let Some(how) = moved {
                reader.update(cx, |r, cx| match how {
                    Scroll::Pages(n) => r.scroll_pages(n, cx),
                    Scroll::By(by) => r.scroll_by(by, cx),
                    Scroll::End(end) => r.scroll_end(end, cx),
                });
                cx.stop_propagation();
                return;
            }
        }
        if m.control || m.platform {
            if ks.key == "t" {
                self.next_theme(cx);
                cx.stop_propagation();
            }
            if ks.key == "r" {
                self.toggle_replay(cx);
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
                if self.replay.is_some() {
                    self.replay = None;
                } else if self.reading.is_some() {
                    // Back where he came from: the search, its keys his.
                    self.close(cx);
                    field::focus(&self.field, window, cx);
                } else if !self.field.read(cx).text.is_empty() {
                    self.field.update(cx, |f, cx| f.set("", false, cx));
                    self.typed(cx);
                } else {
                    self.frame_everything();
                }
                cx.stop_propagation();
                cx.notify();
            }
            "space" if !typing && self.replay_mut().is_some() => {
                if let Some(r) = self.replay_mut() {
                    r.play_pause();
                }
                cx.stop_propagation();
                cx.notify();
            }
            // In an orbit the sky holds still, where it was left.
            "home" if !typing && !self.orbiting() => {
                self.frame_everything();
                cx.notify();
            }
            "+" | "=" | "-" if !typing && !self.orbiting() => {
                let free = self.free();
                let (cx_, cy_) = free.centre();
                let factor = if ks.key == "-" { 0.8 } else { 1.25 };
                let to = self.camera.zoom_about(cx_, cy_, factor);
                self.fly(to);
                cx.notify();
            }
            // Round the ring of the page read, one neighbour at a time;
            // Enter reads the one stood on.
            "left" | "right" if !typing && self.orbiting() => {
                let by = if ks.key == "right" { 1 } else { -1 };
                self.round_the_ring(by, cx);
                cx.stop_propagation();
            }
            "enter" if !typing && self.orbiting() => {
                if let Some(p) = self.hover.filter(|&p| Some(p) != self.reading) {
                    self.open(p, cx);
                    cx.stop_propagation();
                }
            }
            "left" | "right" | "up" | "down" if !typing && !self.orbiting() => {
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

/// When a note was caught, and where when it was not here: `from
/// ChatGPT · 2 h ago`.
fn caught(origin: &str, when: String) -> String {
    match origin {
        "" | front::ORIGIN => when,
        "chatgpt" => format!("from ChatGPT · {when}"),
        o => format!("from {o} · {when}"),
    }
}

/// A row of the inbox's list.
#[derive(Clone, Debug)]
enum InboxRow {
    /// A note waiting, by its page.
    Waiting(usize),
    /// A note an agent filed.
    Filed(inbox::Filed),
}

/// How a key moves the page read.
#[derive(Clone, Copy)]
enum Scroll {
    Pages(f32),
    By(f32),
    End(bool),
}

/// The reader's body size the settings keep, held in range.
fn reader_size(s: &source::Settings) -> f32 {
    s.reader_text
        .filter(|v| v.is_finite())
        .map_or(tokens::reader::BODY, |v| {
            v.clamp(tokens::reader::TEXT_LEAST, tokens::reader::TEXT_MOST)
        })
}

/// A corpus and its sky, off the UI thread.
/// How wide the ranked results stand beside the orbit of a page read in a
/// window `w` wide, or none when the orbit would not keep its room. The
/// orbit grows with the window first; the list widens only past that.
/// Replay's words, and the fewer it says when the foot is narrow.
const REPLAY: [&str; 2] = ["Replay the brain's history", "Replay"];

/// How wide the chrome's words are: its type is monospaced.
fn words_width(words: &str) -> f32 {
    words.chars().count() as f32 * search::MONO_ADVANCE * chrome::TEXT
}

/// What the sky's foot shows.
#[derive(Debug, PartialEq)]
struct Foot {
    counts: bool,
    replay: Option<&'static str>,
}

/// The foot, `room` wide from the window's left inset to where Replay ends:
/// the counts, then the pills that act (whose brain, Update) at the left,
/// Replay at the right when it is `offered`. When they do not all fit the
/// counts go first, then Replay says less, then Replay goes; the pills stay.
fn foot(room: f32, counts: Option<&str>, pills: &[f32], offered: bool) -> Foot {
    let pill = |words: &str| words_width(words) + chrome::SWITCH_PAD_X * 2.;
    let fits = |show_counts: bool, replay: Option<&str>| {
        let widths: Vec<f32> = counts
            .filter(|_| show_counts)
            .map(words_width)
            .into_iter()
            .chain(pills.iter().copied())
            .collect();
        let left =
            widths.iter().sum::<f32>() + chrome::FOOT_GAP * widths.len().saturating_sub(1) as f32;
        left + replay.map_or(0., |r| chrome::INSET + pill(r)) <= room
    };
    let replay = offered.then_some(REPLAY[0]);
    let choices = [
        (true, replay),
        (false, replay),
        (false, offered.then_some(REPLAY[1])),
    ];
    let (counts, replay) = choices
        .into_iter()
        .find(|&(c, r)| fits(c, r))
        .unwrap_or((false, None));
    Foot { counts, replay }
}

fn rail_beside_orbit(w: f32, k: f32) -> Option<f32> {
    let room = w - reader::sheet_width(w, k) - chrome::INSET * 2.;
    (room - search::ASIDE >= search::ORBIT_LEAST)
        .then(|| (room - search::ORBIT_WANTS).clamp(search::ASIDE, search::WIDTH))
}

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

/// What an empty brain says: where it looked and where pages go.
fn no_pages(root: &std::path::Path) -> String {
    format!(
        "No pages in {} yet. A page is a Markdown file under knowledge/, evidence/, \
         projects/ or self/; the sky lights as soon as one is there.",
        root.display()
    )
}

/// A Julian day as a person writes it: `21 October 2026`.
fn date(day: i32) -> String {
    match time::Date::from_julian_day(day) {
        Ok(d) => format!("{} {} {}", d.day(), d.month(), d.year()),
        Err(_) => String::new(),
    }
}

/// `1 page`, `3 pages`.
fn pages(n: usize) -> String {
    if n == 1 {
        "1 page".into()
    } else {
        format!("{n} pages")
    }
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
    /// This frame's sky, and whether anything is still moving.
    fn scene(&mut self, window: &Window, t: &Theme) -> (Picture, bool) {
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
    fn system_names(
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
    fn page_names(
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
fn name_lines(name: &str, room: f32, measure: impl Fn(&str) -> f32) -> Vec<String> {
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
        self.typing = typing;
        self.bench_types(cx);
        let (picture, busy) = self.scene(window, &t);
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
                let ms = done.duration_since(prev).as_secs_f32() * 1000.;
                if b.typed > 0 {
                    b.work_typing.push(ms);
                } else {
                    b.work.push(ms);
                }
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

        // Launch: the theme's picture behind the window while the brain is
        // read, fading as the sky gathers out of it.
        let backdrop = match (&self.status, self.first_light) {
            (Some(_), _) => 1.,
            (None, Some(f)) => 1. - ease_out(f.elapsed().as_secs_f32() * 1000. / motion::GATHER),
            _ => 0.,
        };
        let backdrop = (backdrop > 0.).then(|| {
            img(t.art.window.clone())
                .absolute()
                .inset_0()
                .size_full()
                .object_fit(gpui::ObjectFit::Cover)
                .opacity(t.art.window_show * backdrop * chrome::BACKDROP)
        });
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
            .children(backdrop)
            .child(sky);

        root = root.child(self.search_panel(&t, typing, cx));
        root = root.child(self.theme_button(&t, cx));
        if let Some(status) = &self.status {
            root = root.child(div().absolute().inset_0().child(pane::vacant(
                &t,
                status.clone(),
                None,
            )));
        }
        // The counts, and whose brain they count: the switch stays while
        // another brain comes, so a slow one can be left.
        let counts = self
            .data
            .as_ref()
            .filter(|_| self.status.is_none())
            .map(|d| {
                format!(
                    "{} pages · {} subjects · {} links",
                    d.corpus.pages.len(),
                    d.corpus.subjects.len(),
                    d.corpus.links.len()
                )
            });
        let switch = self.place.from.is_some() || !self.others.is_empty();
        let update = self.update_pill(&t, cx);
        // Another machine's copy has no history of its own to replay.
        let replay = self.status.is_none() && self.data.is_some() && self.place.from.is_none();
        let pill = |words: &str| words_width(words) + chrome::SWITCH_PAD_X * 2.;
        let mut pills = vec![];
        if switch {
            // The warning dot beside the words, when that machine is away.
            let dot = if self.away_from.is_some() { 15. } else { 0. };
            pills.push(pill(&self.source_label()) + dot);
        }
        if let Some(b) = self.updates.read(cx).badge() {
            pills.push(pill(b.words));
        }
        let fitted = foot(
            self.size.0 - chrome::INSET - self.replay_right(),
            counts.as_deref(),
            &pills,
            replay && self.replay.is_none(),
        );
        let counts = counts.filter(|_| fitted.counts);
        if counts.is_some() || switch || update.is_some() {
            root = root.child(
                div()
                    .absolute()
                    .left(px(chrome::INSET))
                    .bottom(px(
                        chrome::INSET - (chrome::SWITCH_HEIGHT - chrome::FOOT) / 2.
                    ))
                    .h(px(chrome::SWITCH_HEIGHT))
                    .flex()
                    .items_center()
                    .gap(px(chrome::FOOT_GAP))
                    .text_size(px(chrome::TEXT))
                    .text_color(t.dim)
                    .children(counts)
                    .when(switch, |d| d.child(self.source_switch(&t, cx)))
                    .children(update),
            );
        }
        if replay && let Some(bar) = self.replay_bar(&t, fitted.replay, cx) {
            root = root.child(bar);
        }

        if let Some(chip) = self.evidence_chip(&t, cx) {
            root = root.child(chip);
        }
        if let Some(e) = &self.settings_broken {
            root = root.child(
                div()
                    .absolute()
                    .left(px(chrome::INSET))
                    .bottom(px(chrome::INSET + chrome::FOOT))
                    .max_w(px(self.search_width()))
                    .text_size(px(chrome::TEXT))
                    .text_color(t.error)
                    .child(format!("Settings: {e}")),
            );
        }
        if self.reading.is_some() {
            root = root.child(self.sheet(&t, cx));
            if let Some(ed) = &self.editing {
                root = root.child(self.source_pane(ed));
            }
        } else {
            root = root.children(self.said_view(&t));
        }
        if self.inbox.is_some() {
            root = root.child(self.inbox_panel(&t, cx));
        }
        if let Some(ed) = &self.note {
            root = root.child(self.note_pane(&t, &ed.clone(), cx));
        }
        // Ctrl+wheel the page read did not take sizes the whole app.
        let me = cx.entity().downgrade();
        root.child(zoom::app_catch(
            self.app_wheel.clone(),
            move |n, window, cx| {
                let _ = me.update(cx, |me, cx| {
                    let z = if n > 0 { Zoom::In } else { Zoom::Out };
                    me.zoom_app(z, n.abs(), window, cx)
                });
            },
        ))
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
        "bench: {pages} pages, sky lit {:.0} ms after launch, {} frames, {fps:.1} fps\n  frame interval: {}\n  build+paint on the UI thread, flying: {}\n  build+paint on the UI thread, typing a search: {}\n  a key answered from the titles ({} keys): {}",
        b.lit.unwrap_or(f32::NAN),
        b.frames.len(),
        stats(&b.frames),
        stats(&b.work),
        stats(&b.work_typing),
        b.keys.len(),
        stats(&b.keys)
    );
}

// ---- Overlays over the sky.

impl BrainApp {
    /// The search, and under it what it found, while a page is not being
    /// read over it.
    fn search_panel(&self, t: &Theme, typing: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let recent = typing && self.query.is_empty() && self.recent_shown();
        let width = if recent {
            self.recent_width()
        } else {
            self.search_width()
        };
        let mut panel = div()
            .absolute()
            .left(px(chrome::INSET))
            .top(px(chrome::INSET))
            .w(px(width))
            .flex()
            .flex_col()
            .gap(px(search::LIST_GAP))
            .occlude()
            .child(
                div()
                    .w(px(self.search_width()))
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
            if recent {
                panel = panel.child(self.recent(data, t, cx));
            }
            return panel;
        }
        if let Some(e) = &self.away {
            panel = panel.child(
                div()
                    .id("away")
                    .flex()
                    .gap(px(8.))
                    .px(px(search::ROW_PAD_X))
                    .py(px(search::ROW_PAD_Y))
                    .rounded(px(chrome::RADIUS))
                    .bg(veil(t, chrome::PANEL_ALPHA))
                    .text_size(px(search::SNIPPET))
                    .text_color(t.text2)
                    .tooltip(kit::tip(e.clone()))
                    .child(
                        div()
                            .flex_none()
                            .mt(px(4.))
                            .size(px(7.))
                            .rounded(px(7.))
                            .bg(t.error),
                    )
                    .child(match &self.place.from {
                        Some(r) => format!(
                            "{}'s search is not answering, so only titles and aliases \
                             are matched here.",
                            r.name
                        ),
                        None => "Citadel is not answering, so only titles and aliases are \
                                 matched here."
                            .to_string(),
                    }),
            );
        }
        if self.results.is_empty() && self.answered == self.asked {
            return panel.child(
                div()
                    .h(px(art::EMPTY_ROOM))
                    .rounded(px(chrome::RADIUS))
                    .bg(veil(t, chrome::PANEL_ALPHA))
                    .child(pane::vacant(t, "Nothing in the brain matches.", None)),
            );
        }
        let aside = self.aside();
        if self.results.is_empty() || (self.reading.is_some() && !typing && !aside) {
            return panel;
        }
        // Every result is reachable: the list scrolls in the room down to
        // the counts, and the wheel over it is its own, not the sky's.
        let mut room = self.size.1
            - search::FIELD_HEIGHT
            - chrome::INSET * 3.
            - chrome::FOOT
            - search::LIST_GAP;
        if self.away.is_some() {
            room -= search::AWAY;
        }
        let rows = self
            .results
            .iter()
            .enumerate()
            .map(|(i, f)| self.result_row(i, f, data, t, aside, cx).into_any_element())
            .collect::<Vec<_>>();
        let list = pane::scroll("results")
            .py(px(4.))
            .max_h(px(room.max(search::ASIDE_ROW * 3.)))
            .track_scroll(&self.results_scroll)
            .on_scroll(|_, _, cx| cx.stop_propagation())
            .rounded(px(chrome::RADIUS))
            .bg(veil(t, chrome::PANEL_ALPHA))
            .children(rows);
        panel = panel.child(list);
        panel
    }

    /// What changed last, as a way in before anything is asked.
    fn recent(&self, data: &Data, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let now = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        let mut list = div()
            .flex()
            .flex_col()
            .py(px(4.))
            .rounded(px(chrome::RADIUS))
            .bg(veil(t, chrome::PANEL_ALPHA))
            .child(
                kit::label(t, "Changed lately")
                    .px(px(search::ROW_PAD_X))
                    .pt(px(search::ROW_PAD_Y))
                    .pb(px(2.)),
            );
        for &p in &data.recent {
            let page = &data.corpus.pages[p];
            let hue = data.hues[page.subject];
            list = list.child(
                div()
                    .id(("recent", p))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .px(px(search::ROW_PAD_X))
                    .py(px(search::ROW_PAD_Y - 2.))
                    .cursor_pointer()
                    .hover(|s| s.bg(alpha(hue, 0.08)))
                    .on_click(cx.listener(move |me, _, _, cx| me.open(p, cx)))
                    .on_hover(cx.listener(move |me, on: &bool, _, cx| {
                        let want = on.then_some(p);
                        if me.hover != want && (*on || me.hover == Some(p)) {
                            me.hover = want;
                            cx.notify();
                        }
                    }))
                    .child(div().flex_none().size(px(7.)).rounded(px(7.)).bg(hue))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(search::TITLE))
                            .text_color(alpha(t.text, 0.9))
                            .child(page.title.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(search::SNIPPET))
                            .text_color(t.dim)
                            .child(reader::since(page.modified, now)),
                    ),
            );
        }
        list
    }

    fn result_row(
        &self,
        i: usize,
        f: &Found,
        data: &Data,
        t: &Theme,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let page = &data.corpus.pages[f.page];
        let hue = data.hues[page.subject];
        let p = f.page;
        // Beside an orbit the row marked is the page read.
        let picked = match self.reading {
            Some(r) => r == p,
            None => self.pick == Some(i),
        };
        let snippet = snippet_text(&f.snippet, &page.summary, hue, t);
        div()
            .id(("result", i))
            // Its own height in the scrolling list, a wrapped title too.
            .flex_none()
            .flex()
            .gap(px(10.))
            .px(px(search::ROW_PAD_X))
            .py(px(search::ROW_PAD_Y))
            // Beside an orbit a title wraps onto a second line before it
            // is cut.
            .when(compact, |d| {
                d.min_h(px(search::ASIDE_ROW))
                    .py(px(search::ASIDE_ROW_PAD))
                    .items_center()
            })
            .when(picked, |d| d.bg(alpha(hue, 0.1)))
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
            .child(
                div()
                    .flex_none()
                    .when(!compact, |d| d.mt(px(5.)))
                    .size(px(7.))
                    .rounded(px(7.))
                    .bg(hue),
            )
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
                                    .when(compact, |d| d.line_clamp(2).text_ellipsis())
                                    .when(!compact, |d| d.truncate())
                                    .text_size(px(search::TITLE))
                                    .text_color(if picked { t.text } else { alpha(t.text, 0.9) })
                                    .when(picked, |d| d.font_weight(FontWeight::BOLD))
                                    .child(page.title.clone()),
                            )
                            .when(!compact, |d| {
                                d.child(
                                    div()
                                        .flex_none()
                                        .text_size(px(search::SNIPPET))
                                        .text_color(alpha(hue, 0.85))
                                        .child(page.kind.clone()),
                                )
                            }),
                    )
                    .when(!compact, |d| {
                        d.child(
                            div()
                                .text_size(px(search::SNIPPET))
                                .line_height(px(search::SNIPPET * 1.45))
                                .text_color(t.dim)
                                .truncate()
                                .child(snippet),
                        )
                    }),
            )
    }

    /// The replay's button, or the replay itself: the day it stands on, the
    /// days worked on as a strip to pick from, and play and pause.
    /// How far from the window's right Replay ends: beside the sheet while
    /// a page is read.
    fn replay_right(&self) -> f32 {
        if self.reading.is_some() {
            reader::sheet_width(self.size.0, self.reader_k()) + chrome::INSET
        } else {
            chrome::INSET
        }
    }

    /// Replay's button says `words`, or is left out when there is no room.
    fn replay_bar(
        &self,
        t: &Theme,
        words: Option<&'static str>,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let right = self.replay_right();
        let button = |id: &'static str, words: String| {
            div()
                .id(id)
                .flex_none()
                .px(px(chrome::SWITCH_PAD_X))
                .h(px(chrome::SWITCH_HEIGHT))
                .flex()
                .items_center()
                .rounded(px(chrome::RADIUS))
                .cursor_pointer()
                .text_size(px(chrome::TEXT))
                .text_color(t.text2)
                .hover(|s| s.text_color(t.text).bg(t.raised))
                .child(words)
        };
        let Some(state) = &self.replay else {
            let words = words?;
            return Some(
                div()
                    .absolute()
                    .right(px(right))
                    .bottom(px(chrome::INSET - 6.))
                    .rounded(px(chrome::RADIUS))
                    .bg(veil(t, chrome::PANEL_ALPHA))
                    .occlude()
                    .child(
                        button("replay", words.into())
                            .when(words != REPLAY[0], |b| b.tooltip(kit::tip(REPLAY[0])))
                            .on_click(cx.listener(|me, _, _, cx| me.toggle_replay(cx))),
                    )
                    .into_any_element(),
            );
        };
        let close = button("end-replay", "×".into())
            .on_click(cx.listener(|me, _, _, cx| me.toggle_replay(cx)));
        let bar = div()
            .absolute()
            .left(px(chrome::INSET))
            .right(px(right))
            .bottom(px(chrome::INSET + chrome::FOOT))
            .p(px(chrome::INSET))
            .flex()
            .flex_col()
            .gap(px(10.))
            .rounded(px(chrome::RADIUS))
            .bg(veil(t, chrome::PANEL_ALPHA))
            .occlude();
        let r = match state {
            Replaying::Reading => {
                return Some(
                    bar.child(
                        div()
                            .flex()
                            .justify_between()
                            .items_center()
                            .text_color(t.text2)
                            .child("Reading the brain's history…")
                            .child(close),
                    )
                    .into_any_element(),
                );
            }
            Replaying::Failed(e) => {
                return Some(
                    bar.child(
                        div()
                            .flex()
                            .justify_between()
                            .items_center()
                            .gap(px(12.))
                            .child(div().text_color(t.error).child(e.clone()))
                            .child(close),
                    )
                    .into_any_element(),
                );
            }
            Replaying::Ready(r) => r,
        };
        let Some(&(today, ref pages)) = r.days.get(r.at.floor() as usize) else {
            return Some(
                bar.child(
                    div()
                        .flex()
                        .justify_between()
                        .items_center()
                        .text_color(t.text2)
                        .child("The brain's history names none of its pages.")
                        .child(close),
                )
                .into_any_element(),
            );
        };
        let most = r.days.iter().map(|d| d.1.len()).max().unwrap_or(1).max(1) as f32;
        let now = r.at.floor() as usize;
        let mut strip = div()
            .h(px(tokens::replay::STRIP))
            .flex()
            .items_end()
            .gap(px(tokens::replay::GAP));
        for (i, (_, touched)) in r.days.iter().enumerate() {
            let share = (touched.len() as f32).ln_1p() / most.ln_1p();
            let color = match i.cmp(&now) {
                std::cmp::Ordering::Less => alpha(t.text2, 0.7),
                std::cmp::Ordering::Equal => t.accent,
                std::cmp::Ordering::Greater => alpha(t.dim, 0.5),
            };
            strip = strip.child(
                div()
                    .id(("day", i))
                    .flex_1()
                    .h_full()
                    .flex()
                    .items_end()
                    .cursor_pointer()
                    .on_click(cx.listener(move |me, _, _, cx| {
                        if let Some(r) = me.replay_mut() {
                            r.seek(i);
                        }
                        cx.notify();
                    }))
                    .child(
                        div()
                            .w_full()
                            .h(px((tokens::replay::STRIP * share).max(2.)))
                            .bg(color),
                    ),
            );
        }
        let first = r.days.first().map(|d| d.0).unwrap_or(today);
        let last = r.days.last().map(|d| d.0).unwrap_or(today);
        let play = button(
            "play",
            if r.playing.is_some() { "Pause" } else { "Play" }.into(),
        )
        .on_click(cx.listener(|me, _, _, cx| {
            if let Some(r) = me.replay_mut() {
                r.play_pause();
            }
            cx.notify();
        }));
        let mut note = format!("{} worked on", self::pages(pages.len()));
        if r.reshuffles > 0 {
            note.push_str(&format!(
                " · {} reorganisation{} left out",
                r.reshuffles,
                if r.reshuffles == 1 { "" } else { "s" }
            ));
        }
        Some(
            bar.child(
                div()
                    .flex()
                    .items_end()
                    .gap(px(16.))
                    .child(
                        div()
                            .font_family(t.ty.display)
                            .font_weight(t.ty.display_weight)
                            .text_size(px(ty::GLANCE))
                            .line_height(px(ty::GLANCE * 1.1))
                            .text_color(t.text)
                            .child(date(today)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .pb(px(4.))
                            .text_size(px(chrome::TEXT))
                            .text_color(t.text2)
                            .child(note),
                    )
                    .child(play)
                    .child(close),
            )
            .child(strip)
            .child(
                div()
                    .flex()
                    .justify_between()
                    .text_size(px(chrome::TEXT))
                    .text_color(t.dim)
                    .child(date(first))
                    .child(format!("{} days worked on", r.days.len()))
                    .child(date(last)),
            )
            .into_any_element(),
        )
    }

    /// In a page's orbit, its evidence: how much there is, folded out of
    /// the ring or shown in it, and the switch between.
    fn evidence_chip(&self, t: &Theme, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let data = self.data.as_ref()?;
        let page = self.reading.filter(|_| self.orbiting())?;
        let n = data
            .corpus
            .neighbours(page)
            .into_iter()
            .filter(|&p| data.corpus.pages[p].is_evidence())
            .count();
        if n == 0 {
            return None;
        }
        let free = self.free();
        let (words, act) = if self.fold_evidence {
            (format!("{n} evidence folded away"), "Show")
        } else {
            (format!("{n} evidence in the ring"), "Fold")
        };
        Some(
            div()
                .id("evidence")
                .absolute()
                .left(px(free.x + chrome::INSET))
                .top(px(free.y))
                .h(px(chrome::SWITCH_HEIGHT))
                .flex()
                .items_center()
                .gap(px(10.))
                .px(px(chrome::SWITCH_PAD_X))
                .rounded(px(chrome::RADIUS))
                .bg(veil(t, chrome::PANEL_ALPHA))
                .occlude()
                .cursor_pointer()
                .text_size(px(chrome::TEXT))
                .text_color(t.text2)
                .on_click(cx.listener(|me, _, _, cx| me.toggle_evidence(cx)))
                .child(words)
                .child(div().text_color(t.accent).child(act)),
        )
    }

    /// Whose brain is read, as the switch says it.
    fn source_label(&self) -> String {
        match (&self.place.from, &self.away_from) {
            (Some(r), Some(_)) => format!("{}'s brain, as last copied", r.name),
            (Some(r), None) => format!("{}'s brain", r.name),
            (None, _) => "This machine's brain".to_string(),
        }
    }

    /// Whose brain is read, at the foot of the search column: this
    /// machine's, or one of the others Ahjo Swarm knows, in a list it opens.
    fn source_switch(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let label = self.source_label();
        let tip = match &self.away_from {
            Some(e) => format!("{e}. Read another machine's brain"),
            None => "Read another machine's brain".to_string(),
        };
        let row = |id: SharedString, words: String, here: bool| {
            div()
                .id(id)
                .h(px(chrome::SWITCH_HEIGHT))
                .px(px(chrome::SWITCH_PAD_X))
                .flex()
                .items_center()
                .gap(px(8.))
                .rounded(px(chrome::RADIUS))
                .cursor_pointer()
                .text_color(if here { t.text } else { t.text2 })
                .hover(|s| s.text_color(t.text).bg(t.raised))
                .child(
                    div()
                        .size(px(6.))
                        .rounded(px(6.))
                        .when(here, |d| d.bg(t.accent)),
                )
                .child(words)
        };
        let mut list = div()
            .absolute()
            .left_0()
            .bottom(px(chrome::SWITCH_HEIGHT + 4.))
            .min_w(px(220.))
            .flex()
            .flex_col()
            .p(px(4.))
            .rounded(px(chrome::RADIUS))
            .bg(t.raised)
            .border_1()
            .border_color(t.divider);
        if self.choosing {
            list = list.child(
                row(
                    "from-here".into(),
                    "This machine".into(),
                    self.place.from.is_none(),
                )
                .on_click(cx.listener(|me, _, _, cx| me.read_from(None, cx))),
            );
            for (i, r) in self.others.iter().enumerate() {
                let here = self.place.from.as_ref() == Some(r);
                let remote = r.clone();
                list = list.child(
                    row(format!("from-{i}").into(), r.name.clone(), here).on_click(
                        cx.listener(move |me, _, _, cx| me.read_from(Some(remote.clone()), cx)),
                    ),
                );
            }
            if self.others.is_empty() {
                list = list.child(
                    div()
                        .px(px(chrome::SWITCH_PAD_X))
                        .py(px(4.))
                        .text_color(t.dim)
                        .child("Ahjo Swarm knows no other machine."),
                );
            }
        }
        div()
            .relative()
            .occlude()
            .when(self.choosing, |d| d.child(list))
            .child(
                div()
                    .id("from")
                    .h(px(chrome::SWITCH_HEIGHT))
                    .px(px(chrome::SWITCH_PAD_X))
                    .flex()
                    .items_center()
                    .rounded(px(chrome::RADIUS))
                    .bg(veil(t, chrome::PANEL_ALPHA))
                    .cursor_pointer()
                    .text_color(t.text2)
                    .hover(|s| s.text_color(t.text).bg(t.raised))
                    .tooltip(kit::tip(tip))
                    .when(self.away_from.is_some(), |d| {
                        d.child(div().mr(px(8.)).size(px(7.)).rounded(px(7.)).bg(t.error))
                    })
                    .on_click(cx.listener(|me, _, _, cx| me.choose_from(cx)))
                    .child(label),
            )
    }

    /// Update, beside whose brain is read, when the node this app was
    /// installed from hands out another build: a pill as the switch is,
    /// its words in the action colour.
    fn update_pill(&self, t: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        use ahjo_app_kit::update::Tone;
        let badge = self.updates.read(cx).badge()?;
        let updates = self.updates.clone();
        Some(
            div()
                .id("update")
                .h(px(chrome::SWITCH_HEIGHT))
                .px(px(chrome::SWITCH_PAD_X))
                .flex()
                .items_center()
                .rounded(px(chrome::RADIUS))
                .bg(veil(t, chrome::PANEL_ALPHA))
                .occlude()
                .text_color(match badge.tone {
                    Tone::Action => t.accent,
                    Tone::Busy => t.dim,
                    Tone::Failed => t.error,
                })
                .tooltip(kit::tip(badge.tip))
                .when(badge.acts, |d| {
                    d.cursor_pointer()
                        .hover(|s| s.bg(t.raised))
                        .on_click(move |_, window, cx| {
                            updates.update(cx, |u, cx| u.act(window, cx))
                        })
                })
                .child(badge.words)
                .into_any_element(),
        )
    }

    /// The inbox's keys while its list is open; answers whether it took
    /// the key.
    fn inbox_key(
        &mut self,
        key: &str,
        m: &gpui::Modifiers,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if m.control || m.alt || m.platform {
            return false;
        }
        let rows = self.inbox_rows();
        let n = rows.len();
        match key {
            "escape" => self.inbox = None,
            "down" if n > 0 => self.inbox = self.inbox.map(|i| (i + 1) % n),
            "up" if n > 0 => self.inbox = self.inbox.map(|i| (i + n - 1) % n),
            "enter" => {
                if let Some(row) = self.inbox.and_then(|i| rows.get(i).cloned()) {
                    self.pick_inbox(row, window, cx);
                }
            }
            _ => return false,
        }
        cx.notify();
        true
    }

    /// The notes caught here: those waiting in the inbox for an agent, and
    /// those agents filed, with where each went.
    fn inbox_panel(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let right = if self.reading.is_some() {
            reader::sheet_width(self.size.0, self.reader_k()) + chrome::INSET
        } else {
            chrome::INSET
        };
        let now = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        let today = time::OffsetDateTime::now_utc().date().to_julian_day();
        let at = self.inbox.unwrap_or(0);
        let rows = self.inbox_rows();
        let waiting = rows
            .iter()
            .filter(|r| matches!(r, InboxRow::Waiting(_)))
            .count();
        let filed = rows.len() - waiting;
        let mut list = pane::scroll("inbox")
            .absolute()
            .top(px(chrome::INSET + search::FIELD_HEIGHT + search::LIST_GAP))
            .right(px(right))
            .w(px(self.recent_width().min(search::INBOX_W)))
            .max_h(px(self.size.1 - chrome::INSET * 3. - search::FIELD_HEIGHT))
            .py(px(4.))
            .rounded(px(chrome::RADIUS))
            .bg(veil(t, chrome::PANEL_ALPHA))
            .border_1()
            .border_color(t.divider)
            .occlude()
            .on_scroll(|_, _, cx| cx.stop_propagation());
        let label = |words: String| {
            kit::label(t, words)
                .px(px(search::ROW_PAD_X))
                .pt(px(search::ROW_PAD_Y))
                .pb(px(2.))
        };
        let Some(data) = self.data.clone() else {
            return list;
        };
        let pending = self.place.from.is_none() && self.filed.is_none();
        if rows.is_empty() && !pending {
            return list.child(
                div()
                    .px(px(search::ROW_PAD_X))
                    .py(px(search::ROW_PAD_Y * 2.))
                    .text_size(px(search::SNIPPET))
                    .text_color(t.text2)
                    .child("No notes yet. Ctrl+N catches one in the inbox, and an agent files it into the brain."),
            );
        }
        let row = |i: usize, id: &'static str| {
            div()
                .id((id, i))
                .flex()
                .items_center()
                .gap(px(10.))
                .px(px(search::ROW_PAD_X))
                .py(px(search::ROW_PAD_Y - 2.))
                .cursor_pointer()
                .when(i == at, |d| d.bg(alpha(t.accent, 0.14)))
                .hover(|s| s.bg(alpha(t.text, 0.06)))
        };
        let age = |words: String| {
            div()
                .flex_none()
                .text_size(px(search::SNIPPET))
                .text_color(t.dim)
                .child(words)
        };
        if waiting > 0 {
            list = list.child(label(format!("Waiting for an agent · {waiting}")));
        }
        for (i, r) in rows.iter().enumerate() {
            let pick = r.clone();
            let el = match r {
                InboxRow::Waiting(p) => {
                    let page = &data.corpus.pages[*p];
                    row(i, "waiting")
                        .child(div().flex_none().size(px(7.)).rounded(px(7.)).bg(t.dim))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(search::TITLE))
                                .text_color(t.text)
                                .child(page.title.clone()),
                        )
                        .child(age(caught(&page.origin, {
                            // A file's time says when it was fetched as
                            // much as written: the note's day says older.
                            match page.updated {
                                Some(day) if day < today => reader::age(day, today),
                                _ => reader::since(page.modified, now),
                            }
                        })))
                }
                InboxRow::Filed(f) => {
                    let named: Vec<(String, Hsla)> =
                        f.to.iter()
                            .filter_map(|to| data.corpus.by_path(to))
                            .map(|p| {
                                let page = &data.corpus.pages[p];
                                (page.title.clone(), data.hues[page.subject])
                            })
                            .collect();
                    let hue = named.first().map_or(t.dim, |n| n.1);
                    let went = match (f.moved, named.as_slice()) {
                        (_, []) if f.to.is_empty() => "Taken out of the inbox".to_string(),
                        (_, []) => "Filed in a page since gone".to_string(),
                        (true, [(title, _)]) if *title == f.title => {
                            let p = f.to.first().and_then(|to| data.corpus.by_path(to));
                            let subject = p
                                .map(|p| {
                                    data.corpus.subjects[data.corpus.pages[p].subject]
                                        .name
                                        .clone()
                                })
                                .unwrap_or_default();
                            format!("Filed in {subject}")
                        }
                        (true, [(title, _)]) => {
                            let p = f.to.first().and_then(|to| data.corpus.by_path(to));
                            let subject = p
                                .map(|p| {
                                    data.corpus.subjects[data.corpus.pages[p].subject]
                                        .name
                                        .clone()
                                })
                                .unwrap_or_default();
                            format!("Filed in {subject} as {title}")
                        }
                        (_, [(title, _)]) => format!("Folded into {title}"),
                        (_, [(title, _), rest @ ..]) => {
                            format!("Folded into {title} and {} more", rest.len())
                        }
                    };
                    row(i, "filed")
                        .child(div().flex_none().size(px(7.)).rounded(px(7.)).bg(hue))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(px(search::TITLE))
                                        .text_color(alpha(t.text, 0.9))
                                        .child(f.title.clone()),
                                )
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(px(search::SNIPPET))
                                        .text_color(t.text2)
                                        .child(went),
                                ),
                        )
                        .child(age(caught(
                            &f.origin,
                            reader::since(f.when.max(0) as u64 * 1_000_000_000, now),
                        )))
                }
            };
            if i == waiting && filed > 0 {
                list = list.child(label("Filed by agents".into()).mt(px(search::ROW_PAD_Y)));
            }
            list = list.child(el.on_click(
                cx.listener(move |me, _, window, cx| me.pick_inbox(pick.clone(), window, cx)),
            ));
        }
        match (&self.place.from, &self.filed) {
            (Some(r), _) => {
                list = list.child(
                    div()
                        .px(px(search::ROW_PAD_X))
                        .py(px(search::ROW_PAD_Y))
                        .text_size(px(search::SNIPPET))
                        .text_color(t.dim)
                        .child(format!(
                            "Which notes agents filed shows on {}'s own machine.",
                            r.name
                        )),
                )
            }
            (None, Some(Err(e))) => {
                list = list.child(
                    div()
                        .px(px(search::ROW_PAD_X))
                        .py(px(search::ROW_PAD_Y))
                        .text_size(px(search::SNIPPET))
                        .text_color(t.dim)
                        .child(format!("Which notes agents filed did not read: {e}")),
                )
            }
            _ => {}
        }
        list
    }

    fn theme_button(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let right = if self.reading.is_some() {
            reader::sheet_width(self.size.0, self.reader_k()) + chrome::INSET
        } else {
            chrome::INSET
        };
        let chip = |id: &'static str| {
            div()
                .id(id)
                .h(px(chrome::SWITCH_HEIGHT))
                .flex()
                .items_center()
                .gap(px(6.))
                .px(px(chrome::SWITCH_PAD_X))
                .rounded(px(chrome::RADIUS))
                .bg(veil(t, chrome::PANEL_ALPHA))
                .cursor_pointer()
                .text_size(px(chrome::TEXT))
                .text_color(t.text2)
                .hover(|s| s.text_color(t.text).bg(t.raised))
        };
        let writable = self.place.from.is_none() && self.data.is_some();
        div()
            .absolute()
            .top(px(
                chrome::INSET + (search::FIELD_HEIGHT - chrome::SWITCH_HEIGHT) / 2.
            ))
            .right(px(right))
            .flex()
            .gap(px(6.))
            .occlude()
            .when(self.data.is_some(), |d| {
                let waiting = self.data.as_ref().map_or(0, |d| d.waiting.len());
                d.child(
                    chip("inbox")
                        .when(self.inbox.is_some(), |c| c.text_color(t.text).bg(t.raised))
                        .tooltip(kit::tip(
                            "The notes waiting in the inbox, and where agents filed them (Ctrl+I)",
                        ))
                        .on_click(cx.listener(|me, _, _, cx| me.toggle_inbox(cx)))
                        .child("Inbox")
                        .when(waiting > 0, |c| {
                            c.child(div().text_color(t.dim).child(waiting.to_string()))
                        }),
                )
            })
            .when(writable, |d| {
                d.child(
                    chip("note")
                        .tooltip(kit::tip(
                            "A new note, filed in the inbox when you close it (Ctrl+N)",
                        ))
                        .on_click(cx.listener(|me, _, window, cx| me.catch_note(window, cx)))
                        .child("New note"),
                )
                .child(
                    chip("today")
                        .tooltip(kit::tip("Today's note (Ctrl+D)"))
                        .on_click(cx.listener(|me, _, window, cx| me.day_note(window, cx)))
                        .child("Today"),
                )
            })
            .child(
                chip("theme")
                    .tooltip(kit::tip("Next theme (Ctrl+T)"))
                    .on_click(cx.listener(|me, _, _, cx| me.next_theme(cx)))
                    .child(img(t.art.mark.clone()).size(px(chrome::MARK)))
                    .child(t.direction.label()),
            )
    }

    fn sheet(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let w = reader::sheet_width(self.size.0, self.reader_k());
        let back = !self.back.is_empty();
        // Back and close: drawn marks a pointer finds, on targets it can
        // hit. The font's arrow and cross draw at a third of its size.
        let (ink, raised) = (t.text2, t.raised);
        let glyph = move |id: &'static str, back: bool, tip: &'static str| {
            div()
                .id(id)
                .size(px(size::CONTROL))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(size::RADIUS))
                .cursor_pointer()
                .hover(move |s| s.bg(raised))
                .tooltip(kit::tip(tip))
                .child(
                    canvas(
                        |_, _, _| {},
                        move |b: Bounds<Pixels>, _, window, _| {
                            let o = b.origin;
                            let at = |x: f32, y: f32| point(o.x + px(x), o.y + px(y));
                            let g = chrome::GLYPH;
                            let mut path = PathBuilder::stroke(px(chrome::GLYPH_LINE));
                            if back {
                                path.move_to(at(g, g / 2.));
                                path.line_to(at(0., g / 2.));
                                path.move_to(at(g / 2.2, g * 0.08));
                                path.line_to(at(0., g / 2.));
                                path.line_to(at(g / 2.2, g * 0.92));
                            } else {
                                path.move_to(at(0., 0.));
                                path.line_to(at(g, g));
                                path.move_to(at(g, 0.));
                                path.line_to(at(0., g));
                            }
                            if let Ok(path) = path.build() {
                                window.paint_path(path, ink);
                            }
                        },
                    )
                    .size(px(chrome::GLYPH)),
                )
        };
        let bar = div()
            .absolute()
            .top(px(chrome::SHEET_BAR))
            .right(px(chrome::SHEET_BAR))
            .flex()
            .gap(px(4.))
            .child(
                kit::button(t, "send", "Send", kit::Kind::Quiet)
                    .tooltip(kit::tip("Send it to a session, in Ahjo Swarm"))
                    .on_click(cx.listener(|me, _, _, cx| me.send_page(cx))),
            )
            .when(self.editing.is_none() && self.place.from.is_none(), |d| {
                d.child(
                    kit::button(t, "edit", "Edit", kit::Kind::Quiet)
                        .tooltip(kit::tip("Write this page (E)"))
                        .on_click(cx.listener(|me, _, window, cx| me.edit(window, cx))),
                )
            })
            .when(back, |d| {
                d.child(
                    glyph("back", true, "Back (Alt+Left)")
                        .on_click(cx.listener(|me, _, _, cx| me.go_back(cx))),
                )
            })
            .child(
                glyph("close", false, "Close the page (Esc)")
                    .on_click(cx.listener(|me, _, _, cx| me.close(cx))),
            );
        let said = self.said_view(t);
        let me = cx.entity().downgrade();
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
                color: t.shadow(),
                offset: gpui::point(px(-8.), px(0.)),
                blur_radius: px(tokens::reader::SHADOW),
                spread_radius: px(0.),
                inset: false,
            }])
            .on_mouse_move(|_, _, cx| cx.stop_propagation())
            .on_scroll(|_, _, cx| cx.stop_propagation())
            // A click on the page is the page's: the sky under it does not
            // take it as a click on a star.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                AnyView::from(self.reader.clone()).cached(StyleRefinement::default().size_full()),
            )
            .child(zoom::catch(self.reader_wheel.clone(), move |n, _, cx| {
                let _ = me.update(cx, |me, cx| {
                    let z = if n > 0 { Zoom::In } else { Zoom::Out };
                    me.zoom_reader(z, n.abs(), cx)
                });
            }))
            .child(bar)
            .children(said)
    }
}

impl BrainApp {
    /// A note over the sky at a reading measure; a click beside it closes
    /// it, filing what was written.
    fn note_pane(
        &self,
        t: &Theme,
        ed: &Entity<Editor>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let w = (self.size.0 - 2. * chrome::INSET).min(tokens::editor::NOTE_W);
        let h = (self.size.1 - 2. * chrome::INSET).min(tokens::editor::NOTE_H);
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(veil(t, chrome::PANEL_ALPHA))
            .occlude()
            .on_mouse_move(|_, _, cx| cx.stop_propagation())
            .on_scroll(|_, _, cx| cx.stop_propagation())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|me, _, window, cx| {
                    cx.stop_propagation();
                    me.leave_note(window, cx);
                }),
            )
            .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .w(px(w))
                    .h(px(h))
                    .rounded(px(chrome::RADIUS))
                    .border_1()
                    .border_color(t.divider)
                    .overflow_hidden()
                    .shadow(vec![gpui::BoxShadow {
                        color: t.shadow(),
                        offset: gpui::point(px(0.), px(12.)),
                        blur_radius: px(40.),
                        spread_radius: px(0.),
                        inset: false,
                    }])
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(ed.clone()),
            )
    }

    /// The page's source, left of the page it makes; the whole window when
    /// the page leaves too little beside it.
    fn source_pane(&self, ed: &Entity<Editor>) -> impl IntoElement {
        let sheet = reader::sheet_width(self.size.0, self.reader_k());
        let room = self.size.0 - sheet;
        let w = if room >= tokens::editor::LEAST {
            room
        } else {
            self.size.0
        };
        div()
            .absolute()
            .top(px(0.))
            .left(px(0.))
            .w(px(w))
            .h_full()
            .occlude()
            .on_mouse_move(|_, _, cx| cx.stop_propagation())
            .on_scroll(|_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(ed.clone())
    }
}

/// The pages a link may name, but `except`: lately changed first, as `[[`
/// offers them before anything is typed.
fn link_names(data: &Data, except: Option<usize>) -> Rc<Vec<editor::Name>> {
    let mut order: Vec<usize> = data.recent.clone();
    let seen: HashSet<usize> = order.iter().copied().collect();
    order.extend((0..data.corpus.pages.len()).filter(|i| !seen.contains(i)));
    Rc::new(
        order
            .into_iter()
            .filter(|&i| Some(i) != except)
            // A note in the inbox takes a new name when an agent files it,
            // so a link to it would break.
            .filter(|&i| !front::waiting(&data.corpus.pages[i].path))
            .map(|i| {
                let q = &data.corpus.pages[i];
                editor::Name {
                    slug: q.slug.clone(),
                    title: q.title.clone(),
                    aliases: q.aliases.clone(),
                    hue: data.hues[q.subject],
                    subject: data.corpus.subjects[q.subject].name.clone(),
                }
            })
            .collect(),
    )
}

fn today() -> time::Date {
    time::OffsetDateTime::now_local()
        .unwrap_or_else(|_| time::OffsetDateTime::now_utc())
        .date()
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
                    color: Some(lift(t, hue, 0.3)),
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
                title: Some("Ahjo Brain".into()),
                ..Default::default()
            }),
            app_id: Some("ahjo-brain".into()),
            ..Default::default()
        },
        |window, cx| {
            // The app's zoom chosen in Ahjo Swarm.
            window.set_ui_scale(ahjo_app_kit::theme::chosen_scale(), cx);
            cx.new(|cx| BrainApp::new(start, window, cx))
        },
    );
    if let Err(e) = opened {
        panic!("the window did not open: {e}");
    }
}

/// `AHJO_BRAIN_BOUNDS=x,y,w,h` places the window exactly (tests on an
/// invisible display); otherwise it opens centred.
fn window_bounds(cx: &App) -> gpui::Bounds<gpui::Pixels> {
    let given: Option<Vec<f32>> = std::env::var("AHJO_BRAIN_BOUNDS")
        .ok()
        .map(|v| v.split(',').filter_map(|n| n.trim().parse().ok()).collect());
    match given.as_deref() {
        Some([x, y, w, h]) => {
            gpui::Bounds::new(gpui::point(px(*x), px(*y)), gpui::size(px(*w), px(*h)))
        }
        _ => gpui::Bounds::centered(None, gpui::size(px(1456.), px(939.)), cx),
    }
}

#[cfg(test)]
mod tests {
    use super::{Foot, REPLAY, foot, name_lines, rail_beside_orbit, words_width};
    use crate::arrange;
    use crate::corpus::parse;
    use crate::sky::{Placement, Rect, layout};
    use crate::tokens::{chrome, search};

    /// The orbit of a page read, with its results beside it, in a window
    /// `w` by `h`: its ring's radius.
    fn ring_in(w: f32, h: f32) -> f32 {
        let mut pages = vec![parse(
            "knowledge/hub/hub.md",
            "# Packing a tide clock for freight\n[[a1]] [[a2]] [[b1]] [[b2]]",
        )];
        for (path, title) in [
            ("knowledge/a/a1.md", "Supplier list for small parts"),
            ("knowledge/a/a2.md", "Calibrating against a port table"),
            ("knowledge/b/b1.md", "Mixed tides in the inner fjords"),
            ("knowledge/b/b2.md", "Assembly order"),
        ] {
            pages.push(parse(path, &format!("# {title}")));
        }
        let corpus = crate::corpus::Corpus::new(pages);
        let sky = layout(&corpus, &Placement::new());
        let hub = corpus.by_path("knowledge/hub/hub.md").unwrap();
        let sheet = crate::reader::sheet_width(w, 1.);
        let left = rail_beside_orbit(w, 1.).map_or(0., |r| r + chrome::INSET * 2.);
        let top = search::FIELD_HEIGHT + chrome::INSET * 2.;
        let view = Rect {
            x: left,
            y: top,
            w: w - sheet - left,
            h: h - top - chrome::INSET - chrome::FOOT,
        };
        arrange::orbit(&corpus, &sky, hub, view, true).radius
    }

    #[test]
    fn the_orbit_grows_with_the_window() {
        // Full HD keeps the results beside the orbit, and the ring takes
        // a good part of the height it has (it once stopped at 155).
        assert_eq!(rail_beside_orbit(1920., 1.), Some(search::ASIDE));
        let full_hd = ring_in(1920., 1080.);
        assert!(full_hd >= 225., "a ring of {full_hd} at 1920x1080");
        // A larger window, a larger ring; a smaller one, smaller.
        assert!(ring_in(2560., 1440.) > full_hd * 1.4);
        assert!(ring_in(1472., 978.) < full_hd);
        assert!(ring_in(1472., 978.) >= 210.);
    }

    #[test]
    fn an_orbit_name_wraps_before_it_is_cut() {
        // Ten pixels a letter.
        let measure = |s: &str| s.chars().count() as f32 * 10.;
        assert_eq!(
            name_lines("Bronze bushings", 200., measure),
            vec!["Bronze bushings"]
        );
        assert_eq!(
            name_lines("Time sync without a network", 200., measure),
            vec!["Time sync", "without a network"]
        );
        // No split fits: cut to the room, never past it.
        let cut = name_lines("Synchronisation-without-networks", 120., measure);
        assert_eq!(cut.len(), 1);
        assert!(measure(&cut[0]) <= 120.);
    }

    /// The foot's room at window width `w`: between the window's inset and
    /// Replay's right, which stands beside the sheet while a page is read.
    fn foot_room(w: f32, reading: bool) -> f32 {
        let right = if reading {
            crate::reader::sheet_width(w, 1.) + chrome::INSET
        } else {
            chrome::INSET
        };
        w - chrome::INSET - right
    }

    #[test]
    fn the_foot_never_overlaps_replay() {
        let counts = "1234 pages · 345 subjects · 6789 links";
        let switch = words_width("This machine's brain") + chrome::SWITCH_PAD_X * 2.;
        let update = words_width("Update") + chrome::SWITCH_PAD_X * 2.;
        let all = Foot {
            counts: true,
            replay: Some(REPLAY[0]),
        };
        // A wide window has room for everything, reading or not.
        assert_eq!(
            foot(foot_room(1920., true), Some(counts), &[switch], true),
            all
        );
        assert_eq!(
            foot(foot_room(1024., false), Some(counts), &[switch], true),
            all
        );
        // A page read in a narrower window: the counts go first, then
        // Replay says less, and the pills that act stay.
        let no_counts = foot(foot_room(1280., true), Some(counts), &[switch], true);
        assert_eq!(
            no_counts,
            Foot {
                counts: false,
                replay: Some(REPLAY[0])
            }
        );
        let short = foot(
            foot_room(1024., true),
            Some(counts),
            &[switch, update],
            true,
        );
        assert_eq!(
            short,
            Foot {
                counts: false,
                replay: Some(REPLAY[1])
            }
        );
        // No room even for that (the sheet takes the window): no Replay.
        assert_eq!(
            foot(foot_room(800., true), Some(counts), &[switch], true),
            Foot {
                counts: false,
                replay: None
            }
        );
        // Whatever is chosen fits, at every width from 1024 up.
        for w in (1024..=2560).step_by(8) {
            for reading in [false, true] {
                let room = foot_room(w as f32, reading);
                let f = foot(room, Some(counts), &[switch, update], true);
                let need = f.counts as u8 as f32 * (words_width(counts) + 8.)
                    + switch
                    + 8.
                    + update
                    + f.replay.map_or(0., |r| {
                        chrome::INSET + words_width(r) + chrome::SWITCH_PAD_X * 2.
                    });
                assert!(need <= room, "{need} in {room} at {w}, reading {reading}");
            }
        }
    }
}
