//! The Desk's window: every desk's open asks in one queue, the one in front
//! of the operator, what he has said on each so far, and the stage that
//! shows its pictures and clips. The three ways of answering (`Variant`)
//! share all of this and differ only in how they lay it out and what Enter
//! means first (`views`).

use crate::answer::{self, Draft, Said, Verdict};
use crate::desk::{self, Place};
use crate::model::{self, Ask, Form, Media, MediaKind};
use crate::player::{Out, Player};
use crate::tokens;
use crate::video::Clip;
use gpui::{
    App, AppContext, AsyncApp, Bounds, Context, Entity, FocusHandle, Focusable, ImageFormat,
    KeyDownEvent, Pixels, Point, RenderImage, WeakEntity, Window,
};
use parvi_app_kit::widgets::field::{self, FieldEvent, TextField};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Three takes on answering, switchable while the app runs (V).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    /// One ask at a time, the whole window for its evidence; the queue is
    /// a count and the next title.
    Focus,
    /// The queue beside the ask: choose what to answer, then answer it.
    Desk,
    /// The agent's recommendation first: agree with Enter, or compare and
    /// overrule. A grill is a list of proposals to confirm.
    Verdict,
}

impl Variant {
    pub const ALL: [Variant; 3] = [Variant::Focus, Variant::Desk, Variant::Verdict];

    pub fn name(self) -> &'static str {
        match self {
            Variant::Focus => "Focus",
            Variant::Desk => "Desk",
            Variant::Verdict => "Verdict",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Variant::Focus => "focus",
            Variant::Desk => "desk",
            Variant::Verdict => "verdict",
        }
    }

    pub fn why(self) -> &'static str {
        match self {
            Variant::Focus => "One ask at a time, the window for its evidence",
            Variant::Desk => "The queue beside the ask",
            Variant::Verdict => "The recommendation first: agree, or compare and overrule",
        }
    }

    fn from_key(k: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|v| v.key() == k)
            .unwrap_or(Variant::Focus)
    }
}

/// Where a desk's asks stand.
pub enum Reach {
    Loading,
    Ok,
    Failed(String),
}

pub struct DeskState {
    pub place: Place,
    pub asks: Vec<Ask>,
    pub reach: Reach,
}

impl DeskState {
    /// Its name as configured, else the project its asks name, else where
    /// it listens.
    pub fn name(&self) -> String {
        if !self.place.name.is_empty() {
            return self.place.name.clone();
        }
        self.asks
            .iter()
            .find(|a| !a.project.is_empty())
            .map(|a| a.project.clone())
            .unwrap_or_else(|| self.place.url.trim_start_matches("http://").to_string())
    }
}

/// An ask: its desk and its number.
pub type Key = (usize, String);

/// One thing the stage can show: a picture or clip, and what it belongs to.
#[derive(Clone, Debug)]
pub struct Item {
    pub media: Media,
    /// The option it shows, if it is one's.
    pub option: Option<usize>,
    /// The option's letter, or empty for the ask's own.
    pub tag: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    /// The item shown, and in Side the one beside it.
    pub item: usize,
    pub side: bool,
    /// Fit to the room, or `scale` device pixels to an image pixel.
    pub fit: bool,
    pub scale: f32,
    pub pan: (f32, f32),
}

impl Default for View {
    fn default() -> Self {
        Self {
            item: 0,
            side: false,
            fit: true,
            scale: 1.,
            pan: (0., 0.),
        }
    }
}

pub enum Pic {
    Loading,
    /// Decoded, its size in pixels, and whether it is the whole file or
    /// the desk's preview.
    Ready {
        image: Arc<RenderImage>,
        w: f32,
        h: f32,
        full: bool,
    },
    Failed(String),
}

/// Where the note field's words go.
#[derive(Clone, Debug, PartialEq)]
pub enum NoteFor {
    Ask,
    Question(String),
}

pub struct DeskApp {
    pub focus: FocusHandle,
    pub desks: Vec<DeskState>,
    pub current: Option<Key>,
    pub drafts: HashMap<(usize, String, u32), Draft>,
    pub variant: Variant,
    pub view: View,
    pub pictures: HashMap<String, Pic>,
    picture_order: VecDeque<String>,
    pub player: Option<(String, Player)>,
    pub clip_failed: HashMap<String, String>,
    clip_loading: Option<String>,
    pub note: Entity<TextField>,
    pub note_for: NoteFor,
    /// What Enter in the note sends.
    pub intent: Verdict,
    pub toast: Option<(String, bool)>,
    toast_gen: u64,
    pub sending: bool,
    pub notice: Option<String>,
    live: Arc<AtomicU64>,
    /// Where each stage panel was laid out, and the video track.
    pub rooms: Rc<RefCell<Vec<Bounds<Pixels>>>>,
    pub track: Rc<RefCell<Option<Bounds<Pixels>>>>,
    pub held: Option<(Point<Pixels>, (f32, f32))>,
    /// A grill's summary, before it is sent.
    pub review: bool,
    play_gen: u64,
    /// The keyboard is in the note.
    pub writing: bool,
    /// He has chosen an ask; until then the queue's first is in front.
    chosen: bool,
}

impl DeskApp {
    pub fn new(
        places: Vec<Place>,
        variant: &str,
        notice: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let note = cx.new(|cx| TextField::new(cx, "A word for the agent").multiline());
        cx.subscribe_in(&note, window, Self::on_note).detach();
        cx.observe_window_activation(window, |this, window, cx| {
            this.follow(window.is_window_active(), cx);
            if window.is_window_active() {
                this.refresh_all(cx);
            }
        })
        .detach();
        let mut app = Self {
            focus: cx.focus_handle(),
            desks: places
                .into_iter()
                .map(|place| DeskState {
                    place,
                    asks: vec![],
                    reach: Reach::Loading,
                })
                .collect(),
            current: None,
            drafts: HashMap::new(),
            variant: Variant::from_key(variant),
            view: View::default(),
            pictures: HashMap::new(),
            picture_order: VecDeque::new(),
            player: None,
            clip_failed: HashMap::new(),
            clip_loading: None,
            note,
            note_for: NoteFor::Ask,
            intent: Verdict::Yes,
            toast: None,
            toast_gen: 0,
            sending: false,
            notice,
            live: Arc::new(AtomicU64::new(0)),
            rooms: Rc::new(RefCell::new(vec![])),
            track: Rc::new(RefCell::new(None)),
            held: None,
            review: false,
            play_gen: 0,
            writing: false,
            chosen: false,
        };
        app.refresh_all(cx);
        app.follow(true, cx);
        window.focus(&app.focus, cx);
        app
    }

    // ---- The queue ----

    /// Every open ask, quickest first, then the longest waiting: each
    /// answer frees an agent, and the quick ones first free the most
    /// agents soonest. What the app cannot answer comes last.
    pub fn queue(&self) -> Vec<Key> {
        let mut open: Vec<(&Ask, usize)> = self
            .desks
            .iter()
            .enumerate()
            .flat_map(|(d, s)| s.asks.iter().filter(|a| a.is_open()).map(move |a| (a, d)))
            .collect();
        open.sort_by(|(a, _), (b, _)| {
            model::effort(a)
                .cmp(&model::effort(b))
                .then_with(|| a.created_at.cmp(&b.created_at))
        });
        open.into_iter().map(|(a, d)| (d, a.id.clone())).collect()
    }

    pub fn ask(&self, key: &Key) -> Option<&Ask> {
        self.desks.get(key.0)?.asks.iter().find(|a| a.id == key.1)
    }

    pub fn current_ask(&self) -> Option<&Ask> {
        self.current.as_ref().and_then(|k| self.ask(k))
    }

    fn draft_key(&self) -> Option<(usize, String, u32)> {
        let k = self.current.as_ref()?;
        Some((k.0, k.1.clone(), self.ask(k)?.round()))
    }

    pub fn draft(&self) -> Draft {
        self.draft_key()
            .and_then(|k| self.drafts.get(&k).cloned())
            .unwrap_or_default()
    }

    pub fn draft_mut(&mut self) -> Option<&mut Draft> {
        let k = self.draft_key()?;
        Some(self.drafts.entry(k).or_default())
    }

    /// Opens the ask he chose, with the keyboard on it.
    pub fn open(&mut self, key: Option<Key>, window: &mut Window, cx: &mut Context<Self>) {
        self.chosen = true;
        self.show(key, cx);
        self.leave_note(window, cx);
    }

    /// Points the note at what is in front, a grill's question or else the
    /// ask (a grill's whole round in its summary), showing what was written
    /// to it.
    fn point_note(&mut self, cx: &mut Context<Self>) {
        let draft = self.draft();
        let question = self
            .current_ask()
            .filter(|a| a.form() == Form::Grill && !self.review)
            .and_then(|a| {
                a.questions
                    .get(draft.at.min(a.questions.len().saturating_sub(1)))
            })
            .cloned();
        let (target, text, hint) = match question {
            Some(q) => (
                NoteFor::Question(q.id.clone()),
                draft.said(&q).words,
                if q.options.is_empty() {
                    "Your answer"
                } else {
                    "Your words on this question"
                },
            ),
            None if self.review => (NoteFor::Ask, draft.note, "A word on the whole round"),
            None => (NoteFor::Ask, draft.note, "A word for the agent"),
        };
        self.note_for = target;
        self.note.update(cx, |f, cx| {
            f.set_placeholder(hint);
            f.set(&text, false, cx);
        });
    }

    /// The keyboard back from the note to the ask.
    pub fn leave_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.writing = false;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// Puts an ask in front: its draft, its first picture, and the next
    /// ask's pictures fetched behind it so moving on is instant.
    fn show(&mut self, key: Option<Key>, cx: &mut Context<Self>) {
        self.writing = false;
        self.current = key;
        self.view = View::default();
        self.review = false;
        self.held = None;
        if self.variant == Variant::Verdict {
            self.view.item = self.recommended_item().unwrap_or(0);
        }
        self.intent = Verdict::Yes;
        self.point_note(cx);
        let queue = self.queue();
        let next = self
            .current
            .as_ref()
            .and_then(|k| queue.iter().position(|q| q == k))
            .and_then(|i| queue.get(i + 1).cloned());
        let keys: Vec<Key> = self.current.iter().cloned().chain(next).collect();
        for k in keys {
            if let Some(ask) = self.ask(&k).cloned() {
                for m in ask.all_media() {
                    if m.kind() == MediaKind::Image {
                        self.picture(k.0, &ask, m, cx);
                    }
                }
            }
        }
        self.sync_player(cx);
        cx.notify();
    }

    pub fn step_ask(&mut self, by: i64, window: &mut Window, cx: &mut Context<Self>) {
        let queue = self.queue();
        if queue.is_empty() {
            return;
        }
        let at = self
            .current
            .as_ref()
            .and_then(|k| queue.iter().position(|q| q == k))
            .map(|i| i as i64)
            .unwrap_or(-1);
        let to = (at + by).clamp(0, queue.len() as i64 - 1) as usize;
        self.open(Some(queue[to].clone()), window, cx);
    }

    // ---- What the stage shows ----

    /// The pictures and clips in front of the operator: the ask's own, then
    /// each option's; in a grill, the current question's.
    pub fn items(&self) -> Vec<Item> {
        let Some(ask) = self.current_ask() else {
            return vec![];
        };
        let (own, options) = if ask.form() == Form::Grill {
            let draft = self.draft();
            match ask
                .questions
                .get(draft.at.min(ask.questions.len().saturating_sub(1)))
            {
                Some(q) => (&q.media, &q.options),
                None => return vec![],
            }
        } else {
            (&ask.media, &ask.options)
        };
        let mut items: Vec<Item> = own
            .iter()
            .map(|m| Item {
                media: m.clone(),
                option: None,
                tag: String::new(),
            })
            .collect();
        for (i, o) in options.iter().enumerate() {
            for m in &o.media {
                items.push(Item {
                    media: m.clone(),
                    option: Some(i),
                    tag: model::letter(options, i),
                });
            }
        }
        items
    }

    /// The item showing the recommended option, for Verdict to open on.
    pub fn recommended_item(&self) -> Option<usize> {
        let ask = self.current_ask()?;
        let rec = if ask.form() == Form::Grill {
            let q = ask.questions.get(self.draft().at)?;
            model::recommended(&q.options, &q.recommend)
        } else {
            ask.recommended()
        }?;
        self.items().iter().position(|i| i.option == Some(rec))
    }

    /// The item beside the shown one when comparing: the next that shows
    /// another option, else the next.
    pub fn partner(&self, items: &[Item]) -> Option<usize> {
        if items.len() < 2 {
            return None;
        }
        let at = self.view.item.min(items.len() - 1);
        let n = items.len();
        (1..n)
            .map(|k| (at + k) % n)
            .find(|&j| items[j].option != items[at].option)
            .or(Some((at + 1) % n))
    }

    pub fn media_key(&self, desk: usize, ask: &Ask, m: &Media) -> String {
        format!("{desk}/{}/{}/{}", ask.id, ask.round(), m.id)
    }

    pub fn media_url(&self, desk: usize, ask: &Ask, m: &Media) -> String {
        format!(
            "{}{}",
            self.desks[desk].place.url,
            desk::media_path(&ask.id, ask.round(), &m.id, false)
        )
    }

    /// A picture as it stands, fetched (the desk's preview first, then the
    /// whole file) when not yet asked for.
    pub fn picture(
        &mut self,
        desk: usize,
        ask: &Ask,
        m: &Media,
        cx: &mut Context<Self>,
    ) -> Option<&Pic> {
        let key = self.media_key(desk, ask, m);
        if !self.pictures.contains_key(&key) {
            self.pictures.insert(key.clone(), Pic::Loading);
            self.picture_order.push_back(key.clone());
            let slot = key.clone();
            let base = self.desks[desk].place.url.clone();
            let thumb = desk::media_path(&ask.id, ask.round(), &m.id, true);
            let full = desk::media_path(&ask.id, ask.round(), &m.id, false);
            let format = image_format(&m.path);
            cx.spawn(async move |this, cx| {
                let svg = cx.update(|cx| cx.svg_renderer());
                if let Ok(Ok(bytes)) = desk::runtime()
                    .spawn(desk::media(base.clone(), thumb))
                    .await
                    && let Some((image, w, h)) =
                        decode(cx, ImageFormat::Jpeg, bytes, svg.clone()).await
                {
                    Self::put(
                        &this,
                        cx,
                        &slot,
                        Pic::Ready {
                            image,
                            w,
                            h,
                            full: false,
                        },
                    );
                }
                let got = match desk::runtime().spawn(desk::media(base, full)).await {
                    Ok(Ok(bytes)) => match decode(cx, format, bytes, svg).await {
                        Some((image, w, h)) => Pic::Ready {
                            image,
                            w,
                            h,
                            full: true,
                        },
                        None => Pic::Failed("The picture does not decode".into()),
                    },
                    Ok(Err(e)) => Pic::Failed(e),
                    Err(e) => Pic::Failed(e.to_string()),
                };
                Self::put(&this, cx, &slot, got);
            })
            .detach();
        }
        self.pictures.get(&key)
    }

    fn put(this: &WeakEntity<Self>, cx: &mut AsyncApp, key: &str, pic: Pic) {
        let _ = this.update(cx, |app, cx| {
            // A preview never replaces the whole picture.
            if matches!(app.pictures.get(key), Some(Pic::Ready { full: true, .. })) {
                if let Pic::Ready { image, .. } = pic {
                    cx.drop_image(image, None);
                }
                return;
            }
            if let Some(Pic::Ready { image, .. }) = app.pictures.insert(key.to_string(), pic) {
                cx.drop_image(image, None);
            }
            app.trim_pictures(cx);
            cx.notify();
        });
    }

    /// Lets the oldest pictures go once they pass the budget.
    fn trim_pictures(&mut self, cx: &mut Context<Self>) {
        let size = |p: &Pic| match p {
            Pic::Ready { w, h, .. } => (*w * *h * 4.) as usize,
            _ => 0,
        };
        let mut total: usize = self.pictures.values().map(size).sum();
        while total > tokens::PICTURE_CACHE && self.picture_order.len() > 1 {
            let Some(old) = self.picture_order.pop_front() else {
                break;
            };
            if let Some(p) = self.pictures.remove(&old) {
                total -= size(&p);
                if let Pic::Ready { image, .. } = p {
                    cx.drop_image(image, None);
                }
            }
        }
    }

    // ---- Clips ----

    /// The stage's clip playing in the player, loaded when it changed.
    pub fn sync_player(&mut self, cx: &mut Context<Self>) {
        let items = self.items();
        let Some(key) = self.current.clone() else {
            self.stop_player(cx);
            return;
        };
        let Some(ask) = self.ask(&key).cloned() else {
            return;
        };
        let Some(item) = items.get(self.view.item.min(items.len().saturating_sub(1))) else {
            self.stop_player(cx);
            return;
        };
        if item.media.kind() != MediaKind::Video {
            self.stop_player(cx);
            return;
        }
        let mkey = self.media_key(key.0, &ask, &item.media);
        if self.player.as_ref().is_some_and(|(k, _)| *k == mkey)
            || self.clip_loading.as_deref() == Some(&mkey)
            || self.clip_failed.contains_key(&mkey)
        {
            return;
        }
        self.stop_player(cx);
        self.clip_loading = Some(mkey.clone());
        let base = self.desks[key.0].place.url.clone();
        let path = desk::media_path(&ask.id, ask.round(), &item.media.id, false);
        cx.spawn(async move |this, cx| {
            let bytes = match desk::runtime().spawn(desk::media(base, path)).await {
                Ok(Ok(b)) => b,
                Ok(Err(e)) => return Self::clip_failed(&this, cx, &mkey, e),
                Err(e) => return Self::clip_failed(&this, cx, &mkey, e.to_string()),
            };
            let opened = cx
                .background_executor()
                .spawn(async move { Clip::open(&bytes) })
                .await;
            let clip = match opened {
                Ok(c) => Arc::new(c),
                Err(e) => return Self::clip_failed(&this, cx, &mkey, e),
            };
            let (player, frames) = Player::start(clip);
            let ok = this.update(cx, |app, cx| {
                if app.clip_loading.as_deref() != Some(&mkey) {
                    return false;
                }
                app.clip_loading = None;
                app.player = Some((mkey.clone(), player));
                cx.notify();
                true
            });
            if !matches!(ok, Ok(true)) {
                return;
            }
            while let Ok(out) = frames.recv().await {
                let alive = this.update(cx, |app, cx| {
                    let Some((k, p)) = app.player.as_mut() else {
                        return false;
                    };
                    if *k != mkey {
                        return false;
                    }
                    match out {
                        Out::Frame(i, image) => {
                            for gone in p.keep(i, image) {
                                cx.drop_image(gone, None);
                            }
                        }
                        Out::Failed(e) => p.failed = Some(e),
                    }
                    cx.notify();
                    true
                });
                if !matches!(alive, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    fn clip_failed(this: &WeakEntity<Self>, cx: &mut AsyncApp, key: &str, why: String) {
        let _ = this.update(cx, |app, cx| {
            app.clip_loading = None;
            app.clip_failed.insert(key.to_string(), why);
            cx.notify();
        });
    }

    fn stop_player(&mut self, cx: &mut Context<Self>) {
        self.clip_loading = None;
        if let Some((_, mut p)) = self.player.take() {
            for f in p.release() {
                cx.drop_image(f, None);
            }
        }
    }

    pub fn toggle_play(&mut self, cx: &mut Context<Self>) {
        let Some((_, p)) = self.player.as_mut() else {
            return;
        };
        if p.playing {
            p.pause();
        } else {
            p.play();
            self.play_gen += 1;
            let generation = self.play_gen;
            let tick = Duration::from_secs_f64(1. / p.clip.fps.clamp(1., 120.));
            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(tick).await;
                    let going = this.update(cx, |app, cx| {
                        if app.play_gen != generation {
                            return false;
                        }
                        let Some((_, p)) = app.player.as_mut() else {
                            return false;
                        };
                        if !p.playing {
                            return false;
                        }
                        p.tick();
                        cx.notify();
                        true
                    });
                    if !matches!(going, Ok(true)) {
                        break;
                    }
                }
            })
            .detach();
        }
        cx.notify();
    }

    pub fn step_frames(&mut self, by: i64, cx: &mut Context<Self>) {
        if let Some((_, p)) = self.player.as_mut() {
            p.step(by);
            cx.notify();
        }
    }

    pub fn seek_fraction(&mut self, f: f32, cx: &mut Context<Self>) {
        if let Some((_, p)) = self.player.as_mut() {
            let n = p.frames();
            p.seek(((f.clamp(0., 1.) * n as f32) as usize).min(n.saturating_sub(1)));
            cx.notify();
        }
    }

    // ---- The stage's view ----

    pub fn show_item(&mut self, i: usize, cx: &mut Context<Self>) {
        let n = self.items().len();
        if n == 0 {
            return;
        }
        self.view.item = i.min(n - 1);
        self.sync_player(cx);
        cx.notify();
    }

    pub fn step_item(&mut self, by: i64, cx: &mut Context<Self>) {
        let n = self.items().len() as i64;
        if n == 0 {
            return;
        }
        let i = (self.view.item as i64 + by).rem_euclid(n) as usize;
        self.show_item(i, cx);
    }

    /// Swaps the shown item with the one beside it: the quickest way to see
    /// what changed between two.
    pub fn flip(&mut self, cx: &mut Context<Self>) {
        let items = self.items();
        if let Some(p) = self.partner(&items) {
            self.show_item(p, cx);
        }
    }

    /// The scale the first panel shows at now, device pixels to an image
    /// pixel.
    pub fn scale_now(&self, natural: (f32, f32), window: &Window) -> f32 {
        if !self.view.fit {
            return self.view.scale;
        }
        let rooms = self.rooms.borrow();
        let Some(b) = rooms.first() else { return 1. };
        fit_scale(*b, natural, window.scale_factor())
    }

    /// Zooms by `factor` about a point in the window, as a wheel does.
    pub fn zoom_at(
        &mut self,
        factor: f32,
        at: Option<Point<Pixels>>,
        natural: (f32, f32),
        window: &Window,
    ) {
        let s = self.scale_now(natural, window);
        let to = (s * factor).clamp(tokens::ZOOM_MIN, tokens::ZOOM_MAX);
        let rooms = self.rooms.borrow().clone();
        let room = at
            .and_then(|p| rooms.iter().find(|b| b.contains(&p)).copied())
            .or_else(|| rooms.first().copied());
        let pan = if self.view.fit {
            (0., 0.)
        } else {
            self.view.pan
        };
        let (cx_, cy) = match (at, room) {
            (Some(p), Some(b)) => (f32::from(p.x - b.center().x), f32::from(p.y - b.center().y)),
            _ => (0., 0.),
        };
        let k = to / s;
        self.view.pan = (cx_ - (cx_ - pan.0) * k, cy - (cy - pan.1) * k);
        self.view.scale = to;
        self.view.fit = false;
    }

    pub fn natural_of_shown(&mut self, cx: &mut Context<Self>) -> Option<(f32, f32)> {
        let items = self.items();
        let item = items.get(self.view.item)?.clone();
        if item.media.kind() == MediaKind::Video {
            return self
                .player
                .as_ref()
                .map(|(_, p)| (p.clip.width as f32, p.clip.height as f32));
        }
        let key = self.current.clone()?;
        let ask = self.ask(&key)?.clone();
        match self.picture(key.0, &ask, &item.media, cx)? {
            Pic::Ready { w, h, full, .. } if *full => Some((*w, *h)),
            Pic::Ready { w, h, .. } => {
                // The preview stands in at the whole picture's size.
                Some((*w, *h))
            }
            _ => None,
        }
    }

    // ---- Answering ----

    fn on_note(
        &mut self,
        _: &Entity<TextField>,
        e: &FieldEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match e {
            FieldEvent::Changed => {
                let text = self.note.read(cx).text.clone();
                let target = self.note_for.clone();
                if let Some(d) = self.draft_mut() {
                    match target {
                        NoteFor::Ask => d.note = text,
                        NoteFor::Question(q) => d.grill.entry(q).or_default().words = text,
                    }
                }
                cx.notify();
            }
            FieldEvent::Focused => {
                self.writing = true;
                cx.notify();
            }
            FieldEvent::Submit | FieldEvent::SubmitAlt => {
                self.leave_note(window, cx);
                match self.note_for.clone() {
                    NoteFor::Question(_) => self.next_question(window, cx),
                    NoteFor::Ask => self.send(self.intent, window, cx),
                }
            }
            _ => {}
        }
    }

    /// Moves the keyboard into the note, for the ask or the question in
    /// front, so Enter there sends `intent`.
    pub fn write(&mut self, intent: Verdict, window: &mut Window, cx: &mut Context<Self>) {
        if self.current.is_none() {
            return;
        }
        self.point_note(cx);
        self.intent = intent;
        if intent == Verdict::No && self.note_for == NoteFor::Ask {
            self.note.update(cx, |f, _| {
                f.set_placeholder("What should change? Enter sends it back")
            });
        }
        self.writing = true;
        field::focus(&self.note, window, cx);
        cx.notify();
    }

    pub fn pick(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ask) = self.current_ask().cloned() else {
            return;
        };
        match ask.form() {
            Form::Pick if i < ask.options.len() => {
                if let Some(d) = self.draft_mut() {
                    d.pick = if d.pick == Some(i) { None } else { Some(i) };
                }
                // Show what was picked.
                if let Some(at) = self.items().iter().position(|it| it.option == Some(i)) {
                    self.show_item(at, cx);
                }
            }
            Form::Grill => {
                let at = self.draft().at;
                let Some(q) = ask.questions.get(at) else {
                    return;
                };
                if i >= q.options.len() {
                    return;
                }
                if let Some(d) = self.draft_mut() {
                    let s = d.grill.entry(q.id.clone()).or_default();
                    s.pick = if s.pick == Some(i) { None } else { Some(i) };
                    s.rec = false;
                }
                if let Some(at) = self.items().iter().position(|it| it.option == Some(i)) {
                    self.show_item(at, cx);
                }
                if self.variant != Variant::Verdict && self.draft().said(q).pick.is_some() {
                    // Picking answers the question: on to the next.
                    self.next_question(window, cx);
                }
            }
            _ => {}
        }
        cx.notify();
    }

    pub fn goto_question(&mut self, at: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ask) = self.current_ask().cloned() else {
            return;
        };
        if ask.questions.is_empty() {
            return;
        }
        self.review = false;
        if let Some(d) = self.draft_mut() {
            d.at = at.min(ask.questions.len() - 1);
        }
        self.view = View::default();
        if self.variant == Variant::Verdict {
            self.view.item = self.recommended_item().unwrap_or(0);
        }
        self.point_note(cx);
        self.leave_note(window, cx);
        self.sync_player(cx);
        cx.notify();
    }

    /// On from the question in front; past the last, the round's summary.
    pub fn next_question(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ask) = self.current_ask().cloned() else {
            return;
        };
        let at = self.draft().at;
        if at + 1 >= ask.questions.len() {
            self.review = true;
            self.point_note(cx);
            self.stop_player(cx);
            self.leave_note(window, cx);
            cx.notify();
        } else {
            self.goto_question(at + 1, window, cx);
        }
    }

    /// Enter on a grill question: what was said stands; with nothing said,
    /// the recommendation is taken; a question with no options asks for
    /// words.
    pub fn accept_question(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ask) = self.current_ask().cloned() else {
            return;
        };
        let at = self.draft().at;
        let Some(q) = ask.questions.get(at) else {
            return;
        };
        let said = self.draft().said(q);
        if !said.answered() {
            match model::recommended(&q.options, &q.recommend) {
                Some(r) => {
                    if let Some(d) = self.draft_mut() {
                        d.grill.insert(
                            q.id.clone(),
                            Said {
                                pick: Some(r),
                                rec: true,
                                ..Default::default()
                            },
                        );
                    }
                }
                None if q.options.is_empty() => return self.write(Verdict::Yes, window, cx),
                None => {}
            }
        }
        self.next_question(window, cx);
    }

    pub fn toggle_unclear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ask) = self.current_ask().cloned() else {
            return;
        };
        if ask.form() == Form::Grill && !self.review {
            let at = self.draft().at;
            let Some(q) = ask.questions.get(at) else {
                return;
            };
            let unclear = if let Some(d) = self.draft_mut() {
                let s = d.grill.entry(q.id.clone()).or_default();
                s.unclear = !s.unclear;
                if s.unclear {
                    s.pick = None;
                }
                s.unclear
            } else {
                false
            };
            if unclear && self.variant != Variant::Verdict {
                self.next_question(window, cx);
            }
            cx.notify();
        } else if ask.form() != Form::Grill && ask.form() != Form::Web {
            if let Some(d) = self.draft_mut()
                && !d.note.starts_with("Unclear")
            {
                d.note = format!("{} {}", answer::UNCLEAR, d.note).trim().to_string();
            }
            self.write(Verdict::No, window, cx);
        }
    }

    /// Sends the answer, and opens the next ask while the desk keeps it.
    pub fn send(&mut self, verdict: Verdict, window: &mut Window, cx: &mut Context<Self>) {
        if self.sending {
            return;
        }
        let Some(key) = self.current.clone() else {
            return;
        };
        let Some(ask) = self.ask(&key).cloned() else {
            return;
        };
        let draft = self.draft();
        let payload = match answer::payload(&ask, &draft, verdict) {
            Ok(p) => p,
            Err(why) => {
                if matches!(verdict, Verdict::No) && draft.note.trim().is_empty() {
                    self.write(Verdict::No, window, cx);
                }
                return self.say(why, true, cx);
            }
        };
        self.sending = true;
        let queue = self.queue();
        let next = queue
            .iter()
            .position(|k| *k == key)
            .and_then(|i| queue.get(i + 1).or_else(|| queue.get(i.wrapping_sub(1))))
            .cloned();
        let base = self.desks[key.0].place.url.clone();
        let title = ask.title.clone();
        cx.spawn_in(window, async move |this, cx| {
            let sent = desk::runtime()
                .spawn(desk::answer(base, key.1.clone(), payload))
                .await
                .unwrap_or_else(|e| Err(e.to_string()));
            let _ = this.update_in(cx, |app, window, cx| {
                app.sending = false;
                match sent {
                    Ok(now) => {
                        if let Some(a) = app.desks[key.0].asks.iter_mut().find(|a| a.id == now.id) {
                            *a = now;
                        }
                        app.drafts.retain(|k, _| !(k.0 == key.0 && k.1 == key.1));
                        app.say(format!("Sent · {title}"), false, cx);
                        if app.current.as_ref() == Some(&key) {
                            app.open(
                                next.filter(|n| app.ask(n).is_some_and(|a| a.is_open())),
                                window,
                                cx,
                            );
                        }
                    }
                    Err(e) => app.say(format!("Not sent: {e}"), true, cx),
                }
            });
        })
        .detach();
        cx.notify();
    }

    pub fn say(&mut self, words: impl Into<String>, error: bool, cx: &mut Context<Self>) {
        self.toast = Some((words.into(), error));
        self.toast_gen += 1;
        let generation = self.toast_gen;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(tokens::TOAST_MS))
                .await;
            let _ = this.update(cx, |app, cx| {
                if app.toast_gen == generation {
                    app.toast = None;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    // ---- The desks ----

    pub fn refresh_all(&mut self, cx: &mut Context<Self>) {
        for d in 0..self.desks.len() {
            self.refresh(d, cx);
        }
    }

    fn refresh(&mut self, d: usize, cx: &mut Context<Self>) {
        let base = self.desks[d].place.url.clone();
        cx.spawn(async move |this, cx| {
            let got = desk::runtime()
                .spawn(desk::asks(base))
                .await
                .unwrap_or_else(|e| Err(e.to_string()));
            let _ = this.update(cx, |app, cx| {
                match got {
                    Ok(asks) => {
                        app.desks[d].asks = asks;
                        app.desks[d].reach = Reach::Ok;
                    }
                    Err(e) => app.desks[d].reach = Reach::Failed(e),
                }
                // The first answerable ask opens by itself; an ask answered
                // elsewhere (the phone) lets the next one in.
                let gone = app
                    .current
                    .as_ref()
                    .is_some_and(|k| !app.ask(k).is_some_and(|a| a.is_open()));
                let first = app.queue().first().cloned();
                if (app.current.is_none() || gone || (!app.chosen && app.current != first))
                    && !app.sending
                {
                    app.show(first, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Follows every desk's changes while the window is in front; a desk
    /// in the background costs its server nothing.
    fn follow(&mut self, on: bool, cx: &mut Context<Self>) {
        let generation = self.live.fetch_add(1, Ordering::Relaxed) + 1;
        if !on {
            return;
        }
        for d in 0..self.desks.len() {
            let (tx, rx) = async_channel::unbounded::<()>();
            let base = self.desks[d].place.url.clone();
            desk::runtime().spawn(desk::follow(
                base,
                self.live.clone(),
                generation,
                move || {
                    let _ = tx.try_send(());
                },
            ));
            cx.spawn(async move |this, cx| {
                while rx.recv().await.is_ok() {
                    while rx.try_recv().is_ok() {}
                    if this.update(cx, |app, cx| app.refresh(d, cx)).is_err() {
                        break;
                    }
                }
            })
            .detach();
        }
    }

    pub fn set_variant(&mut self, v: Variant, window: &mut Window, cx: &mut Context<Self>) {
        self.variant = v;
        desk::save_variant(v.key());
        let current = self.current.clone();
        self.open(current, window, cx);
    }

    pub fn open_on_web(&mut self, cx: &mut Context<Self>) {
        if let Some(k) = self.current.clone() {
            let url = format!("{}/#{}", self.desks[k.0].place.url, k.1);
            cx.open_url(&url);
        }
    }

    // ---- Keys ----

    pub fn on_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = e.keystroke.key.as_str();
        let m = &e.keystroke.modifiers;
        if self.note.read(cx).is_focused(window) {
            if key == "escape" {
                self.leave_note(window, cx);
                cx.notify();
            }
            return;
        }
        if m.control || m.alt || m.platform {
            return;
        }
        let form = self.current_ask().map(|a| a.form());
        let video = self.player.is_some();
        match key {
            "j" | "pagedown" => self.step_ask(1, window, cx),
            "k" | "pageup" => self.step_ask(-1, window, cx),
            "v" => {
                let i = Variant::ALL
                    .iter()
                    .position(|v| *v == self.variant)
                    .unwrap_or(0);
                self.set_variant(Variant::ALL[(i + 1) % 3], window, cx);
            }
            "r" => self.refresh_all(cx),
            "o" => self.open_on_web(cx),
            "c" => {
                self.view.side = !self.view.side && self.items().len() > 1;
                cx.notify();
            }
            "f" => self.flip(cx),
            "z" => {
                self.view.fit = true;
                self.view.pan = (0., 0.);
                cx.notify();
            }
            "0" => {
                self.view.fit = false;
                self.view.scale = 1.;
                self.view.pan = (0., 0.);
                cx.notify();
            }
            "=" | "+" | "-" => {
                if let Some(n) = self.natural_of_shown(cx) {
                    self.zoom_at(
                        if key == "-" {
                            1. / tokens::ZOOM_STEP
                        } else {
                            tokens::ZOOM_STEP
                        },
                        None,
                        n,
                        window,
                    );
                    cx.notify();
                }
            }
            "[" => self.step_item(-1, cx),
            "]" => self.step_item(1, cx),
            "space" if video => self.toggle_play(cx),
            "left" | "," if video => {
                self.step_frames(if m.shift { -self.fps_frames() } else { -1 }, cx)
            }
            "right" | "." if video => {
                self.step_frames(if m.shift { self.fps_frames() } else { 1 }, cx)
            }
            "left" => self.step_item(-1, cx),
            "right" => self.step_item(1, cx),
            "escape" => {
                if self.view.side {
                    self.view.side = false;
                } else if self.review {
                    self.review = false;
                }
                cx.notify();
            }
            _ => self.on_answer_key(key, m.shift, form, window, cx),
        }
    }

    fn fps_frames(&self) -> i64 {
        self.player
            .as_ref()
            .map(|(_, p)| p.clip.fps.round() as i64)
            .unwrap_or(1)
            .max(1)
    }

    fn on_answer_key(
        &mut self,
        key: &str,
        shift: bool,
        form: Option<Form>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(form) = form else { return };
        if form == Form::Web {
            if key == "enter" {
                self.open_on_web(cx);
            }
            return;
        }
        if let Some(n) = key.parse::<usize>().ok().filter(|n| (1..=9).contains(n)) {
            return self.pick(n - 1, window, cx);
        }
        let grill = form == Form::Grill;
        match key {
            "enter" if grill && (shift || self.variant == Variant::Verdict) => {
                self.send(Verdict::GrillRest, window, cx)
            }
            "enter" if grill && self.review => self.send(Verdict::Grill, window, cx),
            "enter" if grill => self.accept_question(window, cx),
            "enter" => self.send(Verdict::Yes, window, cx),
            "s" if grill => self.send(Verdict::Grill, window, cx),
            "down" if grill => self.next_question(window, cx),
            "up" if grill => {
                let at = if self.review {
                    self.current_ask().map(|a| a.questions.len()).unwrap_or(1)
                } else {
                    self.draft().at
                };
                self.goto_question(at.saturating_sub(1), window, cx)
            }
            "x" if !grill => {
                if self.draft().note.trim().is_empty() {
                    self.write(Verdict::No, window, cx)
                } else {
                    self.send(Verdict::No, window, cx)
                }
            }
            "n" | "w" => self.write(
                if grill { Verdict::Grill } else { Verdict::Yes },
                window,
                cx,
            ),
            "u" => self.toggle_unclear(window, cx),
            _ => {}
        }
    }
}

impl Focusable for DeskApp {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// The scale that fits `natural` image pixels into a room, in device
/// pixels to an image pixel, never past [`tokens::FIT_MAX`].
pub fn fit_scale(room: Bounds<Pixels>, natural: (f32, f32), sf: f32) -> f32 {
    let pad = 2. * tokens::STAGE_PAD;
    let w = (f32::from(room.size.width) - pad).max(1.) * sf;
    let h = (f32::from(room.size.height) - pad).max(1.) * sf;
    (w / natural.0.max(1.))
        .min(h / natural.1.max(1.))
        .min(tokens::FIT_MAX)
}

fn image_format(path: &str) -> ImageFormat {
    match path
        .rsplit('.')
        .next()
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("jpg" | "jpeg") => ImageFormat::Jpeg,
        Some("webp") => ImageFormat::Webp,
        Some("gif") => ImageFormat::Gif,
        Some("bmp") => ImageFormat::Bmp,
        _ => ImageFormat::Png,
    }
}

async fn decode(
    cx: &mut AsyncApp,
    format: ImageFormat,
    bytes: Vec<u8>,
    svg: gpui::SvgRenderer,
) -> Option<(Arc<RenderImage>, f32, f32)> {
    cx.background_executor()
        .spawn(async move {
            let image = gpui::Image::from_bytes(format, bytes)
                .to_image_data(svg)
                .ok()?;
            let s = image.size(0);
            Some((image, s.width.0 as f32, s.height.0 as f32))
        })
        .await
}
