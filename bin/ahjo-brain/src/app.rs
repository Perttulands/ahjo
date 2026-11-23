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
use ahjo_app_kit::Error;
use ahjo_app_kit::frame::{self, AppFrame, Cover, FrameEvent, Layer, Status};
use ahjo_app_kit::menu::MenuRow;
use ahjo_app_kit::pointer;
use ahjo_app_kit::popup::{self, Popup, PopupId, Spot};
use ahjo_app_kit::settings_file::{self, KeepsSettings, SettingsFile};
use ahjo_app_kit::theme::{Direction, FAMILY, Theme, alpha, art, size, theme, ty};
use ahjo_app_kit::widgets::field::{self, FieldEvent, TextField};
use ahjo_app_kit::widgets::theme_picker::{self, Picker};
use ahjo_app_kit::widgets::{kit, pane};
use ahjo_app_kit::zoom::{self, Zoom};
use gpui::{
    AnyElement, AnyView, App, AppContext, Bounds, Context, Entity, FocusHandle, Focusable,
    FontWeight, Hsla, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    PathBuilder, Pixels, ScrollDelta, ScrollEvent, ScrollHandle, SharedString, StyleRefinement,
    Subscription, WeakEntity, Window, canvas, div, img, point, prelude::*, px,
};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Instant, SystemTime};

mod scene;
mod views;

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
    Failed(Error),
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

/// The frame's owner name for what the app opens on it.
const OWNER: &str = "brain";
/// The note over the sky, on the frame's stack.
const NOTE: &str = "brain/note";
/// The folded actions' chip, beside the theme's mark.
const MORE: &str = "▾";

/// What stands at the top right, beside the search.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Action {
    Inbox,
    Note,
    Today,
    Theme,
}

impl Action {
    fn words(self) -> &'static str {
        match self {
            Action::Inbox => "Inbox",
            Action::Note => "New note",
            Action::Today => "Today",
            Action::Theme => "Theme",
        }
    }
}

/// What the app draws over the whole window on the frame's stack (a
/// note), drawn again whenever the app changes.
struct Over {
    app: WeakEntity<BrainApp>,
    _app: Subscription,
}

impl Over {
    fn new(app: &Entity<BrainApp>, cx: &mut Context<Self>) -> Self {
        Self {
            app: app.downgrade(),
            _app: cx.observe(app, |_, _, cx| cx.notify()),
        }
    }
}

impl Render for Over {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let note = self
            .app
            .update(cx, |me, cx| {
                let t = theme(cx).clone();
                let ed = me.note.clone()?;
                Some(me.note_pane(&t, &ed, cx).into_any_element())
            })
            .ok()
            .flatten();
        div().size_full().children(note)
    }
}

/// What the app is told at launch.
pub struct Start {
    pub place: Where,
    /// Another machine's brain to read instead of this one's.
    pub from: Option<Remote>,
    /// The app's settings, and why their file did not read.
    pub settings: (SettingsFile<source::Settings>, Option<Error>),
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
    /// The other machines a brain can be read from, as Swarm last saw them.
    others: Vec<Remote>,
    /// Another machine's copy is being brought up to date.
    syncing: bool,
    /// Why the copy read is as last copied: the other machine did not
    /// answer.
    away_from: Option<Error>,
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
    away: Option<Error>,
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
    /// The app's own settings, kept in their file and read again when it
    /// is edited.
    settings: SettingsFile<source::Settings>,
    /// The window's popups and layers, its status line and its toast.
    frame: Entity<AppFrame>,
    _keys: Subscription,
    /// The theme the sky's colours were mixed for: another drawn (one
    /// chosen, or tried in the picker) mixes them again.
    styled: Direction,
    /// The reader's body size, zoomed by the one zoom rule.
    reader_text: f32,
    reader_wheel: Rc<std::cell::Cell<zoom::Wheel>>,
    /// The results' list, scrolled to the row stood on.
    results_scroll: ScrollHandle,
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
    /// The inbox's list is open, and the row stood on; its popup, and what
    /// has the keyboard in it.
    inbox: Option<usize>,
    inbox_popup: Option<PopupId>,
    inbox_focus: FocusHandle,
    /// The notes caught here that agents filed, as the brain's history last
    /// told it, or why it did not.
    filed: Option<Result<Vec<inbox::Filed>, Error>>,
}

impl KeepsSettings for BrainApp {
    type Value = source::Settings;

    fn settings_file(&mut self) -> &mut SettingsFile<source::Settings> {
        &mut self.settings
    }

    fn settings_failed(&mut self, error: Error, cx: &mut Context<Self>) {
        self.fail(format!("Settings: {}", error.message), cx);
    }

    /// An edit to the file outside the app: its theme and the reader's
    /// size apply at once.
    fn settings_reloaded(&mut self, old: source::Settings, cx: &mut Context<Self>) {
        if old.theme != self.settings.theme {
            self.restyle(self.theme_kept(), cx);
        }
        if old.reader_text != self.settings.reader_text {
            self.reader_text = reader_size(&self.settings);
            let text = self.reader_text;
            self.reader.update(cx, |r, cx| r.set_text(text, cx));
            self.replanned = true;
        }
        if let Some(path) = self.settings.path() {
            let words = format!("Settings read again from {}", path.display());
            self.say(words, cx);
        }
        cx.notify();
    }
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
        let (settings, notice) = start.settings;
        let frame = cx.new(|_| AppFrame::default());
        if let Some(e) = notice {
            frame.update(cx, |f, cx| f.report(Status::failure(e.message), cx));
        }
        cx.observe(&frame, |_, _, cx| cx.notify()).detach();
        // The frame's keys come first, before a field's or the page's own:
        // Escape closes the top popup or layer, and an open menu takes its
        // arrows and Enter.
        let keys = cx.intercept_keystrokes({
            let frame = frame.clone();
            move |e, window, cx| {
                let key = KeyDownEvent {
                    keystroke: e.keystroke.clone(),
                    is_held: false,
                    prefer_character_input: false,
                };
                if frame::key(&frame, &key, window, cx) {
                    cx.stop_propagation();
                }
            }
        });
        cx.subscribe_in(&frame, window, |me, frame, event, window, cx| {
            let FrameEvent::Layers { closed } = event;
            // Settled once this update is over: what closed may run its
            // dismiss on the app.
            let (frame, closed, page) = (frame.clone(), closed.clone(), me.focus.clone());
            window.defer(cx, move |window, cx| {
                frame::settle(&frame, &closed, &page.clone(), |_| page, window, cx);
            });
        })
        .detach();
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
            fold_evidence: true,
            replay: None,
            settings,
            frame,
            _keys: keys,
            styled: theme(cx).direction,
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
            inbox_popup: None,
            inbox_focus: cx.focus_handle(),
            filed: None,
        };
        settings_file::watch(&mut app, cx);
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

    /// The window came forward, or a note was filed: if the brain's Git
    /// index moved, read it again.
    fn look_again(&mut self, cx: &mut Context<Self>) {
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

    /// Opens the list of machines whose brain can be read, beside the
    /// switch: this one, and the others Ahjo Swarm knows.
    fn choose_from(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.others = crate::swarm::places();
        let me = cx.entity().downgrade();
        let pick = |remote: Option<Remote>| {
            let me = me.clone();
            move |_: &mut Window, cx: &mut App| {
                let _ = me.update(cx, |me, cx| me.read_from(remote.clone(), cx));
            }
        };
        let mut rows =
            vec![MenuRow::new("This machine", pick(None)).checked(self.place.from.is_none())];
        for r in &self.others {
            let here = self.place.from.as_ref() == Some(r);
            rows.push(MenuRow::new(r.name.clone(), pick(Some(r.clone()))).checked(here));
        }
        if self.others.is_empty() {
            rows.push(MenuRow::note("Ahjo Swarm knows no other machine."));
        }
        let at = window.mouse_position();
        self.frame
            .update(cx, |f, cx| f.menu(OWNER, rows, at, false, cx));
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

    /// The theme kept here: one of the directions, or none, following
    /// Ahjo Swarm's.
    fn theme_kept(&self) -> theme_picker::Choice {
        Direction::from_key(&self.settings.theme)
    }

    /// The theme drawn now, as the picker starts at it: the one kept,
    /// else one `--theme` asked for, else Swarm's.
    fn theme_drawn(&self, cx: &App) -> theme_picker::Choice {
        let drawn = theme(cx).direction;
        self.theme_kept()
            .or_else(|| (drawn != theme_picker::drawn(None).direction).then_some(drawn))
    }

    /// Opens the theme picker beside `spot`: Up and Down try a theme on
    /// the whole app, the sky with it; Enter or a click keeps it.
    fn pick_theme(&mut self, spot: Spot, window: &mut Window, cx: &mut Context<Self>) {
        let me = cx.entity().downgrade();
        theme_picker::open(
            &self.frame,
            OWNER,
            spot,
            Picker::new(self.theme_drawn(cx), true),
            move |choice, _, cx| {
                let _ = me.update(cx, |me, cx| {
                    me.settings.theme = choice.map_or("", |d| d.key()).into();
                    settings_file::save(me, cx);
                    me.restyle(choice, cx);
                });
            },
            window,
            cx,
        );
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
        settings_file::save(self, cx);
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
        settings_file::save(self, cx);
        cx.notify();
    }

    /// The pointer is over the page read, where zooming sizes its text.
    fn over_sheet(&self, window: &Window) -> bool {
        self.reading.is_some()
            && f32::from(window.mouse_position().x)
                >= self.size.0 - reader::sheet_width(self.size.0, self.reader_k())
    }

    /// Says `words` for a moment.
    fn say(&mut self, words: String, cx: &mut Context<Self>) {
        self.frame.update(cx, |f, cx| f.toast(&words, cx));
    }

    /// Says what went wrong, until it is closed or something else is said.
    fn fail(&mut self, words: String, cx: &mut Context<Self>) {
        self.frame
            .update(cx, |f, cx| f.report(Status::failure(words), cx));
    }

    /// The app drawn in `choice`, a theme or Ahjo Swarm's: its colours, its
    /// letters and, as it is drawn next, the sky's character.
    fn restyle(&mut self, choice: theme_picker::Choice, cx: &mut Context<Self>) {
        let th = theme_picker::drawn(choice);
        if theme(cx).direction != th.direction {
            cx.set_global(th);
            cx.refresh_windows();
        }
    }

    /// The sky's colours mixed again for the theme drawn now.
    fn recolour(&mut self, t: &Theme, cx: &mut Context<Self>) {
        self.styled = t.direction;
        if let Some(old) = &self.data {
            let mut data = (**old).clone();
            data.hues = Data::hues(&data.corpus, t);
            let data = Rc::new(data);
            self.data = Some(data.clone());
            if let Some(p) = self.reading {
                self.reader.update(cx, |r, cx| r.show(data, p, cx));
            }
        }
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

    fn found(&mut self, hits: Result<Vec<Hit>, Error>, cx: &mut Context<Self>) {
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
                Err(e) => {
                    let words = format!("Not sent: {}", e.message);
                    let _ = me.update(cx, |me, cx| me.fail(words, cx));
                    return;
                }
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
                            me.fail(format!("This page did not read: {e}"), cx);
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
        self.close_inbox(cx);
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
        self.close_inbox(cx);
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
                self.fail(format!("This note did not read: {e}"), cx);
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
                    me.fail(
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
        // Over the whole window, on the stack: a popup opened from it
        // stands over it. Escape is the note's own (it closes a list of
        // links first, then the note).
        let me = cx.entity();
        let over = cx.new(|cx| Over::new(&me, cx));
        let weak = me.downgrade();
        let layer = Layer::new(NOTE, over)
            .cover(Cover::Window)
            .keeps_escape()
            .on_dismiss(move |window, cx| {
                let _ = weak.update(cx, |me, cx| me.leave_note(window, cx));
            });
        self.frame
            .update(cx, |f, cx| f.open(frame::Surface::Layer(layer), cx));
        cx.notify();
    }

    /// Closes the note, filing what was written in it: the pane goes at
    /// once and the save goes on, told when it ends. A note begun and left
    /// empty leaves nothing behind.
    fn leave_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ed) = self.note.take() else {
            return;
        };
        self.frame.update(cx, |f, cx| f.close(NOTE, cx));
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
    fn toggle_inbox(&mut self, spot: Spot, cx: &mut Context<Self>) {
        if self.inbox_popup.is_some() {
            self.close_inbox(cx);
            return;
        }
        let Some(data) = self.data.clone() else {
            return;
        };
        self.inbox = Some(0);
        let me = cx.entity();
        let list = popup::drawn(&me, |me, _, cx| Some(me.inbox_list(cx)), cx);
        let weak = me.downgrade();
        let popup = Popup::beside(spot, list)
            .focus(self.inbox_focus.clone())
            .on_dismiss(move |_, cx| {
                let _ = weak.update(cx, |me, cx| {
                    me.inbox = None;
                    me.inbox_popup = None;
                    cx.notify();
                });
            });
        self.inbox_popup = Some(
            self.frame
                .update(cx, |f, cx| f.open_popup(OWNER, popup, cx)),
        );
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

    /// Closes the inbox's list, if it is open.
    fn close_inbox(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.inbox_popup.take() {
            self.frame.update(cx, |f, cx| f.close_popup(id, cx));
        }
        self.inbox = None;
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
        self.close_inbox(cx);
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

    /// The sky's width beside the page read, or the window's.
    fn sky_width(&self) -> f32 {
        if self.reading.is_some() {
            self.size.0 - reader::sheet_width(self.size.0, self.reader_k())
        } else {
            self.size.0
        }
    }

    /// The actions at the top right, as chips: what each says, and whether
    /// it shows its count.
    fn actions(&self) -> Vec<Action> {
        let mut shown = vec![];
        if self.data.is_some() {
            shown.push(Action::Inbox);
        }
        if self.place.from.is_none() && self.data.is_some() {
            shown.push(Action::Note);
            shown.push(Action::Today);
        }
        shown.push(Action::Theme);
        shown
    }

    /// How wide the actions stand: their chips side by side, or folded
    /// into the one that opens them as a menu.
    fn actions_width(&self, folded: bool) -> f32 {
        let chip = |words: f32| words + chrome::SWITCH_PAD_X * 2.;
        if folded {
            return chip(chrome::MARK + chrome::CHIP_GAP + words_width(MORE));
        }
        let waiting = self.data.as_ref().map_or(0, |d| d.waiting.len());
        let widths: Vec<f32> = self
            .actions()
            .into_iter()
            .map(|a| match a {
                Action::Inbox if waiting > 0 => chip(
                    words_width(a.words()) + chrome::CHIP_GAP + words_width(&waiting.to_string()),
                ),
                Action::Theme => {
                    chip(chrome::MARK + chrome::CHIP_GAP + words_width(self.styled.label()))
                }
                a => chip(words_width(a.words())),
            })
            .collect();
        widths.iter().sum::<f32>() + chrome::CHIP_GAP * (widths.len().saturating_sub(1)) as f32
    }

    /// The actions fold into one chip when side by side they leave the
    /// search less than its least.
    fn actions_folded(&self) -> bool {
        self.sky_width() - chrome::INSET * 3. - self.actions_width(false) < chrome::SEARCH_LEAST
    }

    /// Where the actions stand, for what opens from them by a key.
    fn actions_spot(&self) -> Spot {
        let w = self.actions_width(self.actions_folded());
        let right = self.size.0 - self.sky_width() + chrome::INSET;
        Spot(Bounds::new(
            point(
                px(self.size.0 - right - w),
                px(chrome::INSET + (search::FIELD_HEIGHT - chrome::SWITCH_HEIGHT) / 2.),
            ),
            gpui::size(px(w), px(chrome::SWITCH_HEIGHT)),
        ))
    }

    /// The search: as wide as it likes, never under the actions beside it,
    /// and narrow beside an orbit.
    fn search_width(&self) -> f32 {
        let beside =
            self.sky_width() - chrome::INSET * 3. - self.actions_width(self.actions_folded());
        let rail = if self.aside() {
            // Beside an orbit, as wide as the orbit's room leaves.
            rail_beside_orbit(self.size.0, self.reader_k())
        } else {
            None
        };
        rail.unwrap_or(search::WIDTH)
            .min(beside)
            .max(chrome::SEARCH_LEAST)
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
        // Ctrl+I opens the inbox's list, or closes it.
        if (m.control || m.platform)
            && !m.alt
            && !m.shift
            && ks.key == "i"
            && self.editing.is_none()
        {
            self.toggle_inbox(self.actions_spot(), cx);
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
                self.pick_theme(self.actions_spot(), window, cx);
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

impl Render for BrainApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let began = Instant::now();
        let t = theme(cx).clone();
        if t.direction != self.styled {
            self.recolour(&t, cx);
        }
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
        root = root.child(self.actions_row(&t, cx));
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
                let count = |n: usize, one: &str, many: &str| {
                    format!("{n} {}", if n == 1 { one } else { many })
                };
                format!(
                    "{} · {} · {}",
                    count(d.corpus.pages.len(), "page", "pages"),
                    count(d.corpus.subjects.len(), "subject", "subjects"),
                    count(d.corpus.links.len(), "link", "links")
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
        if self.reading.is_some() {
            root = root.child(self.sheet(&t, cx));
            if let Some(ed) = &self.editing {
                root = root.child(self.source_pane(ed));
            }
        }
        root = root
            .children(frame::stack(&self.frame, Cover::Window, window, cx))
            .children(frame::notices(&self.frame, cx));
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
mod journey;

#[cfg(test)]
mod tests {
    use super::scene::name_lines;
    use super::{Foot, REPLAY, foot, rail_beside_orbit, words_width};
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
