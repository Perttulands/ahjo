//! The Desk's window: the shared surface where an agent's ask is shown (its
//! pictures, clips, page or document) and the operator answers it in his
//! own words, marking what he sees. Every desk's open asks in one queue; the
//! one in front, its rounds as a dialogue, what he has written and marked
//! on it so far, and the surface that shows and takes marks.

use crate::answer::{self, Draft, DraftKey, Said, Verdict};
use crate::desk::{self, Place};
use crate::marks::{self, Tool};
use crate::model::{self, Ask, Form, Mark, Media, MediaKind};
use crate::page::{self, FromPage};
use crate::player::{Out, Player};
use crate::tokens;
use crate::video::Clip;
use ahjo_app_kit::Error;
use ahjo_app_kit::frame::{self, AppFrame, Cover, FrameEvent, Layer, Status};
use ahjo_app_kit::menu::MenuRow;
use ahjo_app_kit::popup::Spot;
use ahjo_app_kit::settings_file::{self, KeepsSettings, SettingsFile};
use ahjo_app_kit::widgets::field::{self, AttachmentEvent, FieldEvent, TextField};
use ahjo_app_kit::widgets::theme_picker::{self, Picker};
use gpui::{
    App, AppContext, AsyncApp, Bounds, ClipboardEntry, ClipboardItem, Context, Entity, FocusHandle,
    Focusable, ImageFormat, KeyDownEvent, Pixels, Point, RenderImage, Subscription, WeakEntity,
    Window,
};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

/// Where a desk's asks stand.
pub enum Reach {
    Loading,
    Ok,
    /// Nothing answered: the desk is not running. Not an error while
    /// another desk answers; the settings button says it quietly.
    Away(Error),
    /// The desk answered wrongly: an error he sees.
    Failed(Error),
}

/// How the desks stand together: at least one read, none read while some
/// are still being reached, or none answering at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reached {
    Some,
    Waiting,
    None,
}

/// What a set of desks comes to.
pub fn reached<'a>(each: impl Iterator<Item = &'a Reach>) -> Reached {
    let mut waiting = false;
    for r in each {
        match r {
            Reach::Ok => return Reached::Some,
            Reach::Loading => waiting = true,
            Reach::Away(_) | Reach::Failed(_) => {}
        }
    }
    if waiting {
        Reached::Waiting
    } else {
        Reached::None
    }
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

/// An answer made and not yet posted: what goes to the desk, and the ask
/// to open after.
pub struct PendingAnswer {
    pub key: Key,
    pub title: String,
    revision: u64,
    marks: Vec<model::Mark>,
    /// His marks differ from the desk's, so they go first.
    changed: bool,
    payload: serde_json::Value,
    next: Option<Key>,
}

/// Which asks the list on the left shows: those waiting on him, or those
/// he answered, to look at again.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum List {
    #[default]
    Waiting,
    Answered,
}

/// One thing the surface can show: a picture, clip or page and what it
/// belongs to, or the ask's document.
#[derive(Clone, Debug)]
pub struct Item {
    pub media: Media,
    /// The option it shows, if it is one's.
    pub option: Option<usize>,
    /// The option's letter, or empty for the ask's own.
    pub tag: String,
    /// The ask's document rather than a media file.
    pub doc: bool,
}

impl Item {
    pub fn kind(&self) -> Option<MediaKind> {
        (!self.doc).then(|| self.media.kind())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    /// The item shown, and whether the one beside it shows too.
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

/// A mark being drawn: with what, on which media, through which points.
#[derive(Clone, Debug)]
pub struct Drawing {
    pub tool: Tool,
    pub media: String,
    pub points: Vec<[f64; 2]>,
    /// The clip's time and frame it is drawn on.
    pub at: Option<(f64, u64)>,
}

/// A column edge he drags: the list's or the answer's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grip {
    List,
    Talk,
}

/// Where the keyboard goes once the window is drawn next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Aim {
    Words,
    MarkNote,
    /// The pen on the paper, writing a rewrite or a note.
    Pen,
    Surface,
}

/// What owns the Desk's popups and layers on its frame.
const SETTINGS: &str = "desk-settings";
const MENU: &str = "desk-menu";
const THEME: &str = "desk-theme";

pub struct DeskApp {
    pub focus: FocusHandle,
    /// The window's popups, menus and layers, its status line and toasts.
    pub frame: Entity<AppFrame>,
    /// Its updates from the node it was installed from, if it was.
    pub updates: gpui::Entity<ahjo_app_kit::update::Updates>,
    pub desks: Vec<DeskState>,
    pub current: Option<Key>,
    /// Where the conversation is scrolled, read to fade the edge that has
    /// more beyond it: its children are the rounds, the earliest first.
    pub turns: gpui::ScrollHandle,
    /// The ask and round the conversation was last brought to: another
    /// one in front starts at its own top, the earlier rounds above it.
    landed: Option<(Key, u32)>,
    /// He wrote since his words were last drawn: the conversation follows
    /// their caret.
    wrote: bool,
    /// Where the list is scrolled, read the same way.
    pub rail: gpui::ScrollHandle,
    /// Where the document is scrolled.
    pub doc_scroll: gpui::ScrollHandle,
    pub drafts: HashMap<DraftKey, Draft>,
    /// Where the drafts are kept between launches (none when the file
    /// there did not read: it is left as it is), whether they changed
    /// since, and whether a write is on its way.
    drafts_at: Option<std::path::PathBuf>,
    drafts_changed: bool,
    drafts_keeping: bool,
    pub view: View,
    pub pictures: HashMap<String, Pic>,
    picture_order: VecDeque<String>,
    pub player: Option<(String, Player)>,
    pub clip_failed: HashMap<String, String>,
    clip_loading: Option<String>,
    /// His words: always there, the keyboard's home.
    pub words: Entity<TextField>,
    _words: Vec<Subscription>,
    /// Enter changed what it does outside the Desk: his words' field is
    /// made anew as the window draws next.
    words_stale: bool,
    /// How he set the Desk up (`desk.json`), read again as it changes.
    pub config: SettingsFile<desk::Config>,
    /// The surface alone in the window, to look and mark at its largest.
    pub full: bool,
    /// How the list slides as it is put away and brought back.
    pub list_slide: ahjo_app_kit::panel::Slide,
    /// What has the keyboard while it is in the list.
    pub list_focus: FocusHandle,
    /// Settings' field for the node that reaches sessions.
    pub node_field: Entity<TextField>,
    /// What the node said when last checked from Settings.
    pub node_check: Option<Result<String, Error>>,
    /// Pictures he pasted still on their way to the desk.
    pub pasting: usize,
    /// Hands jobs he pressed Done on, until their check comes out.
    pub checking: std::collections::HashSet<Key>,
    /// The selected mark's words.
    pub mark_note: Entity<TextField>,
    pub selected: Option<String>,
    pub tool: Tool,
    pub drawing: Option<Drawing>,
    /// A page takes clicks as marks rather than following them.
    pub page_comment: bool,
    aim: Option<Aim>,
    /// The whole app's size in percent, by the one zoom rule: Ctrl+wheel
    /// anywhere but a picture, and the zoom keys.
    pub app_size: u16,
    app_wheel: Rc<Cell<ahjo_app_kit::zoom::Wheel>>,
    /// An answer waiting out its undo before it is posted, and when the
    /// status line began to say so.
    pub pending_answer: Option<PendingAnswer>,
    pending_said: Option<SystemTime>,
    undo_gen: u64,
    pub sending: bool,
    live: Arc<AtomicU64>,
    /// Where each surface panel was laid out, where its picture was drawn
    /// in it, and the clip's track.
    pub rooms: Rc<RefCell<Vec<Bounds<Pixels>>>>,
    pub drawn_at: Rc<RefCell<Vec<Option<Bounds<Pixels>>>>>,
    /// The shape (width over height) of the picture in front, as last
    /// shown: how the pictures compared at once are laid out.
    pub last_shape: Rc<Cell<f32>>,
    pub track: Rc<RefCell<Option<Bounds<Pixels>>>>,
    pub held: Option<(Point<Pixels>, (f32, f32))>,
    play_gen: u64,
    /// He has chosen an ask; until then the queue's first is in front.
    chosen: bool,
    /// The list on the left.
    pub list: List,
    /// The document as a paper to grade, read once per ask and round.
    pub paper: Option<(String, Rc<crate::paper::Paper>)>,
    /// What he has in hand on the paper.
    pub grading: crate::paper::Grading,
    /// How wide the paper was last drawn.
    pub paper_width: Rc<Cell<f32>>,
    /// The pen's words on the paper: a rewrite or a note.
    pub pen: Entity<TextField>,
    /// The page view and what the page says to the app.
    #[cfg(windows)]
    pub page: Rc<RefCell<Option<page::WebPage>>>,
    #[cfg_attr(not(windows), allow(dead_code))]
    pub page_said: async_channel::Sender<String>,
    #[cfg(windows)]
    pub page_failed: Rc<RefCell<Option<String>>>,
}

impl DeskApp {
    pub fn new(
        loaded: desk::Loaded,
        notice: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let desk::Loaded { config, places, .. } = loaded;
        let frame = cx.new(|_| AppFrame::default());
        cx.observe(&frame, |_, _, cx| cx.notify()).detach();
        cx.subscribe_in(&frame, window, |app, frame, e, window, cx| {
            let FrameEvent::Layers { closed } = e;
            // What closed runs its dismiss, which may reach the app: done
            // once the app is no longer being updated.
            let (frame, closed, focus) = (frame.clone(), closed.clone(), app.focus.clone());
            window.defer(cx, move |window, cx| {
                frame::settle(&frame, &closed, &focus, |_| focus.clone(), window, cx);
            });
        })
        .detach();
        let (words, _words) = Self::words_field(config.enter_sends, window, cx);
        let node_field = cx.new(|cx| {
            let mut f = TextField::new(cx, &config.node());
            f.set(&config.node, false, cx);
            f
        });
        cx.subscribe_in(&node_field, window, |app, f, e, _, cx| {
            if let FieldEvent::Changed | FieldEvent::Submit = e {
                let text = f.read(cx).text.trim().to_string();
                if app.config.node != text {
                    app.config.node = text;
                    app.node_check = None;
                    app.keep_config(cx);
                }
                if *e == FieldEvent::Submit {
                    app.check_node(cx);
                }
            }
        })
        .detach();
        let read = ahjo_app_kit::theme::theme(cx).ty.read;
        let mark_note = cx.new(|cx| {
            TextField::new(cx, "")
                .bare()
                .multiline()
                .min_lines(1)
                .reading()
                .text_size(read)
        });
        cx.subscribe_in(&mark_note, window, Self::on_mark_note)
            .detach();
        let pen = cx.new(|cx| {
            TextField::new(cx, "")
                .bare()
                .multiline()
                .min_lines(1)
                .face(
                    tokens::paper::HAND,
                    ahjo_app_kit::theme::rgb(tokens::paper::GRADE),
                )
                .text_size(tokens::paper::HAND_TEXT)
        });
        cx.subscribe_in(&pen, window, Self::on_pen).detach();
        cx.observe_window_activation(window, |this, window, cx| {
            this.follow(window.is_window_active(), cx);
            if window.is_window_active() {
                this.refresh_all(cx);
                this.updates.update(cx, |u, cx| u.check(cx));
            }
        })
        .detach();
        let updates = cx.new(|_| ahjo_app_kit::update::Updates::new(env!("AHJO_VERSION")));
        let me = cx.weak_entity();
        updates.update(cx, |u, cx| {
            // Unsent answers are kept before the restart, as at closing.
            u.set_guard(std::rc::Rc::new(move |_, cx| {
                let _ = me.update(cx, |app, _| app.keep_drafts_last());
                true
            }));
            u.check(cx);
        });
        cx.observe(&updates, |_, _, cx| cx.notify()).detach();
        let (page_said, heard) = async_channel::unbounded::<String>();
        cx.spawn(async move |this, cx| {
            while let Ok(message) = heard.recv().await {
                if this
                    .update(cx, |app, cx| app.on_page(&message, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        // What he had written and marked when the window last closed.
        let (drafts, drafts_at, notice) = match desk::drafts_path() {
            Some(path) => match desk::read_drafts(&path) {
                Ok(drafts) => (drafts, Some(path), notice),
                Err(e) => (
                    HashMap::new(),
                    None,
                    Some(format!(
                        "Your unsent answers did not read, and are left as they were: {}",
                        e.message
                    )),
                ),
            },
            None => (HashMap::new(), None, notice),
        };
        if let Some(notice) = notice {
            frame.update(cx, |f, cx| f.report(Status::failure(notice), cx));
        }
        let this = cx.weak_entity();
        window.on_window_should_close(cx, move |_, cx| {
            let _ = this.update(cx, |app, _| app.keep_drafts_last());
            true
        });
        let mut app = Self {
            focus: cx.focus_handle(),
            frame,
            updates,
            desks: places
                .into_iter()
                .map(|place| DeskState {
                    place,
                    asks: vec![],
                    reach: Reach::Loading,
                })
                .collect(),
            current: None,
            turns: gpui::ScrollHandle::new(),
            landed: None,
            wrote: false,
            rail: gpui::ScrollHandle::new(),
            doc_scroll: gpui::ScrollHandle::new(),
            drafts,
            drafts_at,
            drafts_changed: false,
            drafts_keeping: false,
            view: View::default(),
            pictures: HashMap::new(),
            picture_order: VecDeque::new(),
            player: None,
            clip_failed: HashMap::new(),
            clip_loading: None,
            words,
            _words,
            words_stale: false,
            config,
            full: false,
            list_slide: Default::default(),
            list_focus: cx.focus_handle(),
            node_field,
            node_check: None,
            pasting: 0,
            checking: Default::default(),
            mark_note,
            selected: None,
            tool: Tool::Look,
            drawing: None,
            page_comment: false,
            aim: Some(Aim::Words),
            app_size: (window.ui_scale() * 100.).round() as u16,
            app_wheel: Rc::default(),
            pending_answer: None,
            pending_said: None,
            undo_gen: 0,
            sending: false,
            live: Arc::new(AtomicU64::new(0)),
            rooms: Rc::new(RefCell::new(vec![])),
            drawn_at: Rc::new(RefCell::new(vec![])),
            last_shape: Rc::new(Cell::new(16. / 9.)),
            track: Rc::new(RefCell::new(None)),
            held: None,
            play_gen: 0,
            chosen: false,
            list: List::Waiting,
            paper: None,
            grading: Default::default(),
            paper_width: Rc::default(),
            pen,
            #[cfg(windows)]
            page: Rc::new(RefCell::new(None)),
            page_said,
            #[cfg(windows)]
            page_failed: Rc::default(),
        };
        app.discover(cx);
        app.refresh_all(cx);
        app.follow(true, cx);
        settings_file::watch(&mut app, cx);
        app
    }

    // ---- The queue ----

    pub fn reached(&self) -> Reached {
        reached(self.desks.iter().map(|d| &d.reach))
    }

    /// Every open ask, in the order he chose: by default the quickest
    /// first, then the longest waiting, since each answer frees an agent
    /// and the quick ones free the most agents soonest.
    pub fn queue(&self) -> Vec<Key> {
        let mut open: Vec<(&Ask, usize)> = self
            .desks
            .iter()
            .enumerate()
            .flat_map(|(d, s)| s.asks.iter().filter(|a| a.is_open()).map(move |a| (a, d)))
            .collect();
        match self.config.order {
            desk::Order::Quickest => open.sort_by(|(a, _), (b, _)| {
                model::effort(a)
                    .cmp(&model::effort(b))
                    .then_with(|| a.created_at.cmp(&b.created_at))
            }),
            desk::Order::Oldest => open.sort_by(|(a, _), (b, _)| a.created_at.cmp(&b.created_at)),
            desk::Order::Newest => open.sort_by(|(a, _), (b, _)| b.created_at.cmp(&a.created_at)),
        }
        open.into_iter().map(|(a, d)| (d, a.id.clone())).collect()
    }

    /// Every answered ask, the last answered first: what he said stays
    /// where he can find it again.
    pub fn answered(&self) -> Vec<Key> {
        let mut done: Vec<(&Ask, usize)> = self
            .desks
            .iter()
            .enumerate()
            .flat_map(|(d, s)| {
                s.asks
                    .iter()
                    .filter(|a| a.status == "answered" && a.answer.is_some())
                    .map(move |a| (a, d))
            })
            .collect();
        done.sort_by(|(a, _), (b, _)| answered_at(b).cmp(answered_at(a)));
        done.into_iter().map(|(a, d)| (d, a.id.clone())).collect()
    }

    /// The asks the list on the left shows now.
    pub fn listed(&self) -> Vec<Key> {
        match self.list {
            List::Waiting => self.queue(),
            List::Answered => self.answered(),
        }
    }

    /// Shows the asks waiting, or those answered, with the first in front.
    pub fn set_list(&mut self, list: List, cx: &mut Context<Self>) {
        if self.list == list {
            return;
        }
        self.list = list;
        self.chosen = false;
        let first = self.listed().first().cloned();
        self.show(first, cx);
        self.aim(Aim::Words, cx);
    }

    /// The ask in front was answered: it is shown as it was sent, and
    /// nothing on it changes.
    pub fn read_only(&self) -> bool {
        self.current_ask().is_some_and(|a| !a.is_open())
    }

    pub fn ask(&self, key: &Key) -> Option<&Ask> {
        self.desks.get(key.0)?.asks.iter().find(|a| a.id == key.1)
    }

    pub fn current_ask(&self) -> Option<&Ask> {
        self.current.as_ref().and_then(|k| self.ask(k))
    }

    fn draft_key(&self) -> Option<DraftKey> {
        let k = self.current.as_ref()?;
        let desk = self.desks.get(k.0)?.place.url.clone();
        Some((desk, k.1.clone(), self.ask(k)?.round()))
    }

    pub fn draft(&self) -> Draft {
        self.draft_key()
            .and_then(|k| self.drafts.get(&k).cloned())
            .unwrap_or_default()
    }

    pub fn draft_mut(&mut self) -> Option<&mut Draft> {
        if self.read_only() {
            return None;
        }
        let k = self.draft_key()?;
        self.drafts_changed = true;
        Some(self.drafts.entry(k).or_default())
    }

    /// Keeps the drafts on disk a moment after they change, so a window
    /// closed or a machine gone down loses at most that moment.
    pub fn keep_drafts(&mut self, cx: &mut Context<Self>) {
        if !self.drafts_changed || self.drafts_keeping || self.drafts_at.is_none() {
            return;
        }
        self.drafts_keeping = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(tokens::KEEP_DRAFTS_MS))
                .await;
            let _ = this.update(cx, |app, cx| {
                if let Err(e) = app.keep_drafts_now() {
                    app.fail(e.message, cx);
                }
                app.drafts_keeping = false;
                cx.notify();
            });
        })
        .detach();
    }

    /// Writes the drafts as they are now.
    fn keep_drafts_now(&mut self) -> Result<(), Error> {
        let Some(path) = &self.drafts_at else {
            return Ok(());
        };
        if !self.drafts_changed {
            return Ok(());
        }
        self.drafts_changed = false;
        desk::write_drafts(path, &answer::keep(&self.drafts)).map_err(|e| {
            Error::new(
                "drafts_unwritable",
                format!("Your unsent answers could not be kept: {}", e.message),
            )
        })
    }

    /// The drafts kept as the window closes or the app restarts: what
    /// fails then has no window left to say it in, so it goes to the log.
    fn keep_drafts_last(&mut self) {
        if let Err(e) = self.keep_drafts_now() {
            log::error!("{e}");
        }
    }

    /// Opens the ask he chose, with the keyboard in his words.
    pub fn open(&mut self, key: Option<Key>, cx: &mut Context<Self>) {
        self.chosen = true;
        self.show(key, cx);
        self.aim(Aim::Words, cx);
    }

    /// Where the keyboard goes when the window is next drawn.
    pub fn aim(&mut self, aim: Aim, cx: &mut Context<Self>) {
        self.aim = Some(aim);
        cx.notify();
    }

    /// Moves the keyboard where it was aimed; the window calls it as it
    /// draws.
    pub fn take_aim(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.words_stale {
            self.remake_words(window, cx);
        }
        match self.aim.take() {
            // Alone in the window, the surface has no words beside it.
            Some(Aim::Words) if self.full => window.focus(&self.focus, cx),
            Some(Aim::Words) => field::focus(&self.words, window, cx),
            Some(Aim::MarkNote) => field::focus(&self.mark_note, window, cx),
            Some(Aim::Pen) => field::focus(&self.pen, window, cx),
            Some(Aim::Surface) => window.focus(&self.focus, cx),
            None => {}
        }
    }

    /// Points his words at what they answer: a grill's question, or the
    /// ask (a grill's whole round).
    fn point_words(&mut self, cx: &mut Context<Self>) {
        let draft = self.draft();
        let ask = self.current_ask().cloned();
        let (text, hint) = match (ask, draft.at) {
            (Some(a), Some(i)) if a.form() == Form::Grill && i < a.questions.len() => {
                let q = &a.questions[i];
                (draft.said(q).words, String::new())
            }
            (Some(a), _) if a.form() == Form::Grill => (draft.words, "On the whole round".into()),
            (Some(a), _) if a.form() == Form::Hands => (
                draft.words,
                "What stops the job, or a note on how it went".into(),
            ),
            // The card and its Send say what the field is for.
            _ => (draft.words, String::new()),
        };
        self.words.update(cx, |f, cx| {
            f.set_placeholder(&hint);
            f.set(&text, false, cx);
        });
    }

    /// Puts an ask in front: its draft, its first picture, and the next
    /// ask's pictures fetched behind it so moving on is instant.
    fn show(&mut self, key: Option<Key>, cx: &mut Context<Self>) {
        if self.current != key {
            self.doc_scroll
                .set_offset(gpui::point(gpui::px(0.), gpui::px(0.)));
            self.grading = Default::default();
        }
        if let Some(answer) = &self.pending_answer
            && key.as_ref() == Some(&answer.key)
        {
            self.pending_answer = None;
        }
        self.current = key;
        self.view = View::default();
        self.held = None;
        self.drawing = None;
        self.selected = None;
        self.page_comment = false;
        self.point_words(cx);
        let queue = self.listed();
        // The ask in front stands in sight in the list, however it came
        // to the front (J, K, Ctrl+PgDn, the next after Send).
        if let Some(i) = self
            .current
            .as_ref()
            .and_then(|k| queue.iter().position(|q| q == k))
        {
            self.rail.scroll_to_item(i);
        }
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

    pub fn step_ask(&mut self, by: i64, cx: &mut Context<Self>) {
        let queue = self.listed();
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
        self.open(Some(queue[to].clone()), cx);
    }

    // ---- What the surface shows ----

    /// What is in front of the operator: the ask's document, its own
    /// media, then each option's; in a grill, the chosen question's.
    pub fn items(&self) -> Vec<Item> {
        let Some(ask) = self.current_ask() else {
            return vec![];
        };
        let (own, options) = if ask.form() == Form::Grill {
            let at = self.draft().at.unwrap_or(0);
            match ask
                .questions
                .get(at.min(ask.questions.len().saturating_sub(1)))
            {
                Some(q) => (&q.media, &q.options),
                None => return vec![],
            }
        } else {
            (&ask.media, &ask.options)
        };
        let mut items = vec![];
        if let Some(doc) = &ask.doc {
            items.push(Item {
                media: Media {
                    id: "doc".into(),
                    path: doc.name.clone(),
                    caption: String::new(),
                },
                option: None,
                tag: String::new(),
                doc: true,
            });
        }
        items.extend(own.iter().map(|m| Item {
            media: m.clone(),
            option: None,
            tag: String::new(),
            doc: false,
        }));
        for (i, o) in options.iter().enumerate() {
            for m in &o.media {
                items.push(Item {
                    media: m.clone(),
                    option: Some(i),
                    tag: model::letter(options, i),
                    doc: false,
                });
            }
        }
        items
    }

    pub fn shown_item(&self) -> Option<Item> {
        let items = self.items();
        items
            .get(self.view.item.min(items.len().saturating_sub(1)))
            .cloned()
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
            .find(|&j| items[j].option != items[at].option && !items[j].doc)
            .or(Some((at + 1) % n))
    }

    /// The document's blocks, parsed once per ask and round.
    /// The ask's document as a paper, with the round he marked before.
    pub fn paper(&mut self) -> Option<Rc<crate::paper::Paper>> {
        let ask = self.current_ask()?;
        let doc = ask.doc.as_ref()?;
        let key = format!("{}/{}", ask.id, ask.round());
        if self.paper.as_ref().map(|(k, _)| k) != Some(&key) {
            let before = ask
                .history
                .iter()
                .rev()
                .find(|r| r.doc.is_some() && r.marks.iter().any(|m| m.on_text()))
                .and_then(|r| Some((r.round(), r.doc.as_ref()?.text.as_str(), r.marks.as_slice())));
            let paper = Rc::new(crate::paper::Paper::new(&doc.text, before));
            self.paper = Some((key, paper));
        }
        self.paper.as_ref().map(|(_, p)| p.clone())
    }

    // ---- Marks ----

    /// His marks on this round as they stand.
    pub fn marks(&self) -> Vec<Mark> {
        match self.current_ask() {
            Some(a) => self.draft().marks(a).to_vec(),
            None => vec![],
        }
    }

    pub fn edit_marks(&mut self, f: impl FnOnce(&mut Vec<Mark>)) {
        let Some(ask) = self.current_ask().cloned() else {
            return;
        };
        if let Some(d) = self.draft_mut() {
            let marks = d.marks.get_or_insert_with(|| ask.marks.clone());
            f(marks);
        }
    }

    /// A new mark: selected, with the keyboard in its words.
    pub fn add_mark(&mut self, mut mark: Mark, cx: &mut Context<Self>) {
        let id = marks::next_id(&self.marks());
        mark.id = id.clone();
        self.edit_marks(|m| m.push(mark));
        self.select_mark(Some(id), false, cx);
        self.aim(Aim::MarkNote, cx);
    }

    /// Selects a mark (none: none), and with `jump` shows where it is: its
    /// picture or page, its clip at its frame.
    pub fn select_mark(&mut self, id: Option<String>, jump: bool, cx: &mut Context<Self>) {
        self.selected = id.clone();
        let mark = id.and_then(|id| self.marks().into_iter().find(|m| m.id == id));
        let text = mark.as_ref().map(|m| m.text.clone()).unwrap_or_default();
        self.mark_note.update(cx, |f, cx| f.set(&text, false, cx));
        if jump
            && let Some(m) = &mark
            && let Some(media) = &m.media
            && let Some(i) = self.items().iter().position(|it| &it.media.id == media)
        {
            if i != self.view.item {
                self.show_item(i, cx);
            }
            if let (Some(f), Some((_, p))) = (m.frame, self.player.as_mut()) {
                p.seek(f as usize);
            }
        }
        if jump
            && mark.as_ref().is_some_and(Mark::on_text)
            && let Some(i) = self.items().iter().position(|it| it.doc)
            && i != self.view.item
        {
            self.show_item(i, cx);
        }
        self.tell_page();
        cx.notify();
    }

    pub fn delete_mark(&mut self, id: &str, cx: &mut Context<Self>) {
        self.edit_marks(|m| m.retain(|m| m.id != id));
        if self.selected.as_deref() == Some(id) {
            self.select_mark(None, false, cx);
        }
        self.tell_page();
        cx.notify();
    }

    /// The marks numbered as the list shows them.
    pub fn numbered(&self) -> Vec<(usize, Mark)> {
        self.marks()
            .into_iter()
            .enumerate()
            .map(|(i, m)| (i + 1, m))
            .collect()
    }

    /// Picks the tool; a page takes Comment for any drawing tool.
    pub fn set_tool(&mut self, tool: Tool, cx: &mut Context<Self>) {
        self.tool = tool;
        self.drawing = None;
        let page = self.shown_item().and_then(|i| i.kind()) == Some(MediaKind::Page);
        if page {
            self.page_comment = tool != Tool::Look;
            self.tell_page();
        }
        cx.notify();
    }

    /// Where the clip is, when the shown item is a clip: seconds and frame.
    pub fn clip_at(&self) -> Option<(f64, u64)> {
        let (_, p) = self.player.as_ref()?;
        Some((p.seconds(p.at), p.at as u64))
    }

    /// Starts a mark at `n` (0 to 1 across the shown media).
    pub fn draw_start(&mut self, n: [f64; 2], cx: &mut Context<Self>) {
        let Some(item) = self.shown_item().filter(|_| !self.read_only()) else {
            return;
        };
        if let Some((_, p)) = self.player.as_mut()
            && p.playing
        {
            p.pause();
        }
        self.drawing = Some(Drawing {
            tool: self.tool,
            media: item.media.id.clone(),
            points: vec![n],
            at: self.clip_at(),
        });
        cx.notify();
    }

    pub fn draw_move(&mut self, n: [f64; 2], cx: &mut Context<Self>) {
        let Some(d) = self.drawing.as_mut() else {
            return;
        };
        match d.tool {
            Tool::Pen => {
                let last = d.points.last().copied().unwrap_or(n);
                if (last[0] - n[0]).hypot(last[1] - n[1]) > 0.002 {
                    d.points.push(n);
                }
            }
            _ => {
                d.points.truncate(1);
                d.points.push(n);
            }
        }
        cx.notify();
    }

    pub fn draw_end(&mut self, cx: &mut Context<Self>) {
        let Some(d) = self.drawing.take() else {
            return;
        };
        if let Some(mark) = marks::drawn(String::new(), d.tool, &d.media, &d.points, d.at) {
            self.add_mark(mark, cx);
        }
    }

    /// A note on the whole of what is shown, its words taking the
    /// keyboard: one to a picture or page, one to a clip's frame; asked
    /// again, the one there opens.
    pub fn note(&mut self, cx: &mut Context<Self>) {
        if self.read_only() {
            return;
        }
        let Some(item) = self.shown_item() else {
            return;
        };
        if item.doc {
            // The document is marked by its words; a word on all of it is
            // his answer.
            return self.aim(Aim::Words, cx);
        }
        if let Some((_, p)) = self.player.as_mut()
            && p.playing
        {
            p.pause();
        }
        let at = (item.kind() == Some(MediaKind::Video))
            .then(|| self.clip_at())
            .flatten();
        let there = self.marks().into_iter().find(|m| {
            marks::is_whole(m)
                && m.media.as_deref() == Some(item.media.id.as_str())
                && m.frame == at.map(|a| a.1)
        });
        match there {
            Some(m) => {
                self.select_mark(Some(m.id), false, cx);
                self.aim(Aim::MarkNote, cx);
            }
            None => self.add_mark(marks::whole(String::new(), &item.media.id, at), cx),
        }
    }

    // ---- The page ----

    /// The page's URL on its desk.
    pub fn page_url(&self, item: &Item) -> Option<String> {
        let key = self.current.as_ref()?;
        let ask = self.ask(key)?;
        Some(self.media_url(key.0, ask, &item.media))
    }

    fn on_page(&mut self, message: &str, cx: &mut Context<Self>) {
        match page::parse(message) {
            Some(FromPage::Ready) => self.tell_page(),
            Some(FromPage::Mark {
                selector,
                quote,
                x,
                y,
            }) => {
                let Some(item) = self
                    .shown_item()
                    .filter(|i| i.kind() == Some(MediaKind::Page))
                else {
                    return;
                };
                let mark = marks::on_page(String::new(), &item.media.id, &selector, &quote, x, y);
                self.add_mark(mark, cx);
            }
            None => log::warn!("the page said something the app does not read: {message}"),
        }
    }

    /// Draws the page's marks on it and sets its mode.
    pub fn tell_page(&self) {
        #[cfg(windows)]
        {
            let Some(item) = self
                .shown_item()
                .filter(|i| i.kind() == Some(MediaKind::Page))
            else {
                return;
            };
            let numbered = self.numbered();
            let on: Vec<(usize, &Mark)> = numbered
                .iter()
                .filter(|(_, m)| m.media.as_deref() == Some(item.media.id.as_str()))
                .map(|(n, m)| (*n, m))
                .collect();
            let script = page::show_script(&on, self.selected.as_deref(), self.page_comment);
            if let Some(p) = self.page.borrow().as_ref() {
                p.run(&script);
            }
        }
    }

    /// Hides the page view unless the surface shows a page: it lies over
    /// whatever the window draws there.
    pub fn keep_page(&self) {
        #[cfg(windows)]
        if self.shown_item().and_then(|i| i.kind()) != Some(MediaKind::Page)
            && let Some(p) = self.page.borrow_mut().as_mut()
        {
            p.hide();
        }
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
                if let Ok(bytes) = desk::off(desk::media(base.clone(), thumb)).await
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
                let got = match desk::off(desk::media(base, full)).await {
                    Ok(bytes) => match decode(cx, format, bytes, svg).await {
                        Some((image, w, h)) => Pic::Ready {
                            image,
                            w,
                            h,
                            full: true,
                        },
                        None => Pic::Failed("The picture does not decode".into()),
                    },
                    Err(e) => Pic::Failed(e.message),
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
            let bytes = match desk::off(desk::media(base, path)).await {
                Ok(b) => b,
                Err(e) => return Self::clip_failed(&this, cx, &mkey, e),
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
                        Out::Failed(e) => p.failed = Some(e.message),
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

    fn clip_failed(this: &WeakEntity<Self>, cx: &mut AsyncApp, key: &str, why: Error) {
        let _ = this.update(cx, |app, cx| {
            app.clip_loading = None;
            app.clip_failed.insert(key.to_string(), why.message);
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

    fn on_words(
        &mut self,
        _: &Entity<TextField>,
        e: &FieldEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match e {
            FieldEvent::Changed => self.words_changed(cx),
            FieldEvent::Moved => self.follow_caret(cx),
            // Enter sends when he chose it (the field is then not prose).
            FieldEvent::SubmitAlt | FieldEvent::Submit => self.send(Verdict::Send, cx),
            _ => {}
        }
    }

    /// His words' field: one line until he writes more, it grows with what
    /// he writes up to a dozen lines,
    /// then scrolls inside itself with the caret always in sight. Enter
    /// starts a new line, or sends when he chose that in Settings.
    fn words_field(
        enter_sends: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Entity<TextField>, Vec<Subscription>) {
        let read = ahjo_app_kit::theme::theme(cx).ty.read;
        let words = cx.new(|cx| {
            let f = TextField::new(cx, "Your thoughts, in your words")
                .bare()
                .min_lines(1)
                .reading()
                .text_size(read)
                .max_lines(tokens::WORDS_LINES)
                .attachments();
            if enter_sends {
                f.multiline()
            } else {
                f.prose()
            }
        });
        let subs = vec![
            cx.subscribe_in(&words, window, Self::on_words),
            cx.subscribe_in(&words, window, |app, _, e: &AttachmentEvent, _, cx| {
                app.paste_pictures(e.0.clone(), cx)
            }),
        ];
        (words, subs)
    }

    /// His words, kept with what they answer.
    fn words_changed(&mut self, cx: &mut Context<Self>) {
        let text = self.words.read(cx).text.clone();
        let at = self.draft().at;
        let question = self
            .current_ask()
            .filter(|a| a.form() == Form::Grill)
            .and_then(|a| at.and_then(|i| a.questions.get(i)))
            .map(|q| q.id.clone());
        if let Some(d) = self.draft_mut() {
            match question {
                Some(q) => d.grill.entry(q).or_default().words = text,
                None => d.words = text,
            }
        }
        self.wrote = true;
        cx.notify();
    }

    /// Starts the conversation at the round in front: the rounds before it
    /// stand above, a scroll away. The views call it as they draw.
    pub fn land(&mut self, ask: &Ask, cx: &mut Context<Self>) {
        let at = self.current.clone().map(|key| (key, ask.round()));
        if self.landed == at {
            return;
        }
        self.turns.scroll_to_top_of_item(ask.history.len());
        if self.turns.bounds().size.height > gpui::px(0.) {
            self.landed = at;
        } else {
            // Never drawn, the column has no place to scroll against yet:
            // it lands once more when it has been (a notify while drawing
            // is lost).
            cx.spawn(async move |this, cx| {
                let _ = this.update(cx, |_, cx| cx.notify());
            })
            .detach();
        }
    }

    /// Keeps the line he is writing in sight while his words stand in the
    /// conversation, on a grill's question: the field grows down as he
    /// writes, out of the scrolled column unless the column follows.
    fn follow_caret(&mut self, cx: &mut Context<Self>) {
        if !std::mem::take(&mut self.wrote) {
            return;
        }
        let on_question = self.draft().at.is_some()
            && self
                .current_ask()
                .is_some_and(|a| a.is_open() && a.form() == Form::Grill);
        let Some(caret) = self.words.read(cx).caret().filter(|_| on_question) else {
            return;
        };
        let column = self.turns.bounds();
        let clear = gpui::px(tokens::FADE);
        let mut offset = self.turns.offset();
        if caret.bottom() + clear > column.bottom() {
            offset.y -= caret.bottom() + clear - column.bottom();
        } else if caret.top() - clear < column.top() {
            offset.y += column.top() - (caret.top() - clear);
        } else {
            return;
        }
        self.turns.set_offset(offset);
        cx.notify();
    }

    fn on_mark_note(
        &mut self,
        _: &Entity<TextField>,
        e: &FieldEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match e {
            FieldEvent::Changed => {
                let text = self.mark_note.read(cx).text.clone();
                if let Some(id) = self.selected.clone() {
                    self.edit_marks(|m| {
                        if let Some(m) = m.iter_mut().find(|m| m.id == id) {
                            m.text = text;
                        }
                    });
                }
                cx.notify();
            }
            // Enter: the mark is said; back to his words, or, the surface
            // alone, back to it.
            FieldEvent::Submit if self.full => {
                self.select_mark(None, false, cx);
                self.aim(Aim::Surface, cx);
            }
            FieldEvent::Submit => self.aim(Aim::Words, cx),
            FieldEvent::SubmitAlt => self.send(Verdict::Send, cx),
            _ => {}
        }
    }

    fn on_pen(
        &mut self,
        _: &Entity<TextField>,
        e: &FieldEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match e {
            FieldEvent::Submit | FieldEvent::SubmitAlt => self.put_pen_down(cx),
            _ => {}
        }
    }

    /// Leans to option `i` (again: no longer), of the ask or of the grill's
    /// question his words are on.
    pub fn pick(&mut self, i: usize, cx: &mut Context<Self>) {
        let Some(ask) = self.current_ask().cloned() else {
            return;
        };
        match ask.form() {
            Form::Open if i < ask.options.len() => {
                if let Some(d) = self.draft_mut() {
                    d.pick = if d.pick == Some(i) { None } else { Some(i) };
                }
                if let Some(at) = self.items().iter().position(|it| it.option == Some(i)) {
                    self.show_item(at, cx);
                }
            }
            Form::Grill => {
                let at = self.draft().at.unwrap_or(0);
                let Some(q) = ask.questions.get(at) else {
                    return;
                };
                if i >= q.options.len() {
                    return;
                }
                if let Some(d) = self.draft_mut() {
                    d.at = Some(at);
                    let s = d.grill.entry(q.id.clone()).or_default();
                    s.pick = if s.pick == Some(i) { None } else { Some(i) };
                    s.rec = false;
                    s.unclear = false;
                }
                if let Some(at) = self.items().iter().position(|it| it.option == Some(i)) {
                    self.show_item(at, cx);
                }
            }
            _ => {}
        }
        // A pick is part of answering: what he types next is his words,
        // not a tool key on the picture.
        self.aim(Aim::Words, cx);
        cx.notify();
    }

    /// Puts his words on a grill's question (none: on the whole round).
    pub fn goto_question(&mut self, at: Option<usize>, cx: &mut Context<Self>) {
        let Some(ask) = self.current_ask().cloned() else {
            return;
        };
        if ask.questions.is_empty() {
            return;
        }
        let at = at.map(|i| i.min(ask.questions.len() - 1));
        if self.draft().at != at {
            if let Some(d) = self.draft_mut() {
                d.at = at;
            }
            self.view = View::default();
            self.sync_player(cx);
        }
        self.point_words(cx);
        self.aim(Aim::Words, cx);
    }

    pub fn toggle_unclear(&mut self, cx: &mut Context<Self>) {
        let Some(ask) = self.current_ask().cloned() else {
            return;
        };
        let Some(q) = self.draft().at.and_then(|i| ask.questions.get(i)).cloned() else {
            return;
        };
        if let Some(d) = self.draft_mut() {
            let s = d.grill.entry(q.id.clone()).or_default();
            s.unclear = !s.unclear;
            if s.unclear {
                s.pick = None;
            }
        }
        self.aim(Aim::Words, cx);
        cx.notify();
    }

    /// The question's state as he has answered it.
    pub fn said(&self, q: &model::Question) -> Said {
        self.draft().said(q)
    }

    /// Sends his answer (his marks first when he changed them), and opens
    /// the next ask while the desk keeps it. Answered elsewhere waits out
    /// a short undo before anything is posted: it says nothing of its own,
    /// so a slip would close a round on nothing.
    pub fn send(&mut self, verdict: Verdict, cx: &mut Context<Self>) {
        if self.sending || self.read_only() {
            return;
        }
        // Answering on before the undo ran out lets the waiting one go.
        if let Some(waiting) = self.pending_answer.take() {
            self.unsay_pending(cx);
            self.post(waiting, cx);
        }
        let Some(key) = self.current.clone() else {
            return;
        };
        let Some(ask) = self.ask(&key).cloned() else {
            return;
        };
        let draft = self.draft();
        let marks = draft.marks(&ask).to_vec();
        let payload = match answer::payload(&ask, &draft, verdict, marks.len()) {
            Ok(p) => p,
            Err(why) => return self.fail(why.message, cx),
        };
        let queue = self.queue();
        let next = queue
            .iter()
            .position(|k| *k == key)
            .and_then(|i| queue.get(i + 1).or_else(|| queue.get(i.wrapping_sub(1))))
            .cloned();
        let answer = PendingAnswer {
            changed: draft.marks.as_ref().is_some_and(|m| *m != ask.marks),
            key,
            title: ask.title.clone(),
            revision: ask.revision,
            marks,
            payload,
            next,
        };
        if verdict != Verdict::Elsewhere {
            return self.post(answer, cx);
        }
        self.undo_gen += 1;
        let generation = self.undo_gen;
        let next = answer.next.clone();
        let me = cx.weak_entity();
        let waiting = Status::new(format!("Answered elsewhere · {}", answer.title)).action(
            "Undo",
            move |_, cx| {
                let _ = me.update(cx, |app, cx| app.undo_answer(cx));
            },
        );
        self.pending_said = Some(waiting.at);
        self.frame.update(cx, |f, cx| f.report(waiting, cx));
        self.pending_answer = Some(answer);
        self.open(next, cx);
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(tokens::UNDO_MS))
                .await;
            let _ = this.update(cx, |app, cx| {
                if app.undo_gen == generation
                    && let Some(answer) = app.pending_answer.take()
                {
                    app.unsay_pending(cx);
                    app.post(answer, cx);
                }
            });
        })
        .detach();
    }

    /// Takes back an answer still waiting out its undo, and opens its ask
    /// again as he left it.
    pub fn undo_answer(&mut self, cx: &mut Context<Self>) {
        let Some(answer) = self.pending_answer.take() else {
            return;
        };
        self.unsay_pending(cx);
        self.open(Some(answer.key), cx);
        self.toast(format!("Still open · {}", answer.title), cx);
    }

    /// The status line no longer says an answer is waiting.
    fn unsay_pending(&mut self, cx: &mut Context<Self>) {
        if let Some(at) = self.pending_said.take() {
            self.frame.update(cx, |f, cx| f.withdraw(at, cx));
        }
    }

    fn post(&mut self, answer: PendingAnswer, cx: &mut Context<Self>) {
        let PendingAnswer {
            key,
            title,
            revision,
            marks,
            changed,
            payload,
            next,
        } = answer;
        self.sending = true;
        let base = self.desks[key.0].place.url.clone();
        cx.spawn(async move |this, cx| {
            let id = key.1.clone();
            let sent = desk::off(async move {
                if changed {
                    desk::save_marks(base.clone(), id.clone(), &marks, revision).await?;
                }
                desk::answer(base, id, payload).await
            })
            .await;
            let _ = this.update(cx, |app, cx| {
                app.sending = false;
                match sent {
                    Ok(now) => {
                        if let Some(a) = app.desks[key.0].asks.iter_mut().find(|a| a.id == now.id) {
                            *a = now;
                        }
                        let url = &app.desks[key.0].place.url;
                        app.drafts.retain(|k, _| !(&k.0 == url && k.1 == key.1));
                        app.drafts_changed = true;
                        app.toast(format!("Sent · {title}"), cx);
                        if let Some(a) = app.ask(&key).cloned() {
                            app.tell(&a, cx);
                        }
                        if app.current.as_ref() == Some(&key) {
                            app.open(next.filter(|n| app.ask(n).is_some_and(|a| a.is_open())), cx);
                        }
                    }
                    Err(e) => app.fail(format!("Not sent: {}", e.message), cx),
                }
            });
        })
        .detach();
        cx.notify();
    }

    /// The whole app `n` steps bigger or smaller, or back to 100%; kept,
    /// and said.
    pub fn zoom_app(
        &mut self,
        z: ahjo_app_kit::zoom::Zoom,
        n: i32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use ahjo_app_kit::zoom::app;
        let Some(to) = app::zoomed(self.app_size, z, n) else {
            return;
        };
        self.app_size = to;
        window.set_ui_scale(app::scale(to), cx);
        match desk::size_path().map(|p| desk::write_size(&p, to)) {
            Some(Err(e)) => self.fail(format!("{} · not kept: {}", app::words(to), e.message), cx),
            _ => self.toast(app::words(to), cx),
        }
    }

    pub fn app_wheel_handle(&self) -> Rc<Cell<ahjo_app_kit::zoom::Wheel>> {
        self.app_wheel.clone()
    }

    /// A moment's word, fading on its own.
    pub fn toast(&mut self, words: impl Into<String>, cx: &mut Context<Self>) {
        let words = words.into();
        self.frame.update(cx, |f, cx| f.toast(&words, cx));
    }

    /// Something that went wrong: it stays until he closes it or another
    /// replaces it.
    pub fn fail(&mut self, words: impl Into<String>, cx: &mut Context<Self>) {
        let words = words.into();
        self.frame
            .update(cx, |f, cx| f.report(Status::failure(words), cx));
    }

    // ---- The desks ----

    pub fn refresh_all(&mut self, cx: &mut Context<Self>) {
        for d in 0..self.desks.len() {
            self.refresh(d, cx);
        }
    }

    fn refresh(&mut self, d: usize, cx: &mut Context<Self>) {
        let base = self.desks[d].place.url.clone();
        // A desk that did not answer is being asked again: say so, or
        // trying again looks like nothing happened until it times out. A
        // desk away while others answer stays quiet.
        let away = matches!(self.desks[d].reach, Reach::Away(_));
        if matches!(self.desks[d].reach, Reach::Failed(_))
            || (away && self.reached() != Reached::Some)
        {
            self.desks[d].reach = Reach::Loading;
            cx.notify();
        }
        cx.spawn(async move |this, cx| {
            let got = desk::runtime()
                .spawn(desk::asks(base))
                .await
                .unwrap_or_else(|e| {
                    let failed = Error::new("task_failed", e.to_string());
                    Err(desk::Unread::Wrong(failed))
                });
            let _ = this.update(cx, |app, cx| {
                match got {
                    Ok(asks) => {
                        app.desks[d].asks = asks;
                        app.desks[d].reach = Reach::Ok;
                        // A draft for a round closed or moved on is
                        // answered: it is not kept.
                        let (url, open) = (&app.desks[d].place.url, &app.desks[d].asks);
                        let before = app.drafts.len();
                        app.drafts.retain(|(desk, id, round), _| {
                            desk != url
                                || open
                                    .iter()
                                    .any(|a| &a.id == id && a.round() == *round && a.is_open())
                        });
                        app.drafts_changed |= app.drafts.len() != before;
                        app.checked(d, cx);
                    }
                    Err(desk::Unread::Away(e)) => app.desks[d].reach = Reach::Away(e),
                    Err(desk::Unread::Wrong(e)) => app.desks[d].reach = Reach::Failed(e),
                }
                // The first answerable ask opens by itself; an ask answered
                // elsewhere (the phone) lets the next one in.
                let listed = app.listed();
                let gone = app.current.as_ref().is_some_and(|k| !listed.contains(k));
                let first = listed.first().cloned();
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

    // ---- Pictures he pastes ----

    /// What he pasted that is a picture (one copied, or picture files)
    /// goes to the desk for the ask in front, and with his answer to it.
    fn paste_pictures(&mut self, item: ClipboardItem, cx: &mut Context<Self>) {
        let Some(key) = self.current.clone() else {
            return;
        };
        let Some(ask) = self.ask(&key) else { return };
        if !ask.is_open() {
            return self.fail("This ask is answered: nothing more goes with it", cx);
        }
        let mut pasted = vec![];
        for entry in item.entries() {
            match entry {
                ClipboardEntry::Image(i) => pasted.push(Pasted::Bytes(i.format, i.bytes.clone())),
                ClipboardEntry::ExternalPaths(p) => {
                    pasted.extend(p.paths().iter().cloned().map(Pasted::File))
                }
                _ => {}
            }
        }
        if pasted.is_empty() {
            return;
        }
        let base = self.desks[key.0].place.url.clone();
        let id = ask.id.clone();
        let draft: DraftKey = (base.clone(), id.clone(), ask.round());
        self.pasting += pasted.len();
        cx.notify();
        cx.spawn(async move |this, cx| {
            for p in pasted {
                let ready = cx
                    .background_executor()
                    .spawn(async move { p.ready() })
                    .await;
                let got = match ready {
                    Ok(bytes) => desk::off(desk::attach(base.clone(), id.clone(), bytes)).await,
                    Err(e) => Err(e),
                };
                let _ = this.update(cx, |app, cx| {
                    app.pasting = app.pasting.saturating_sub(1);
                    match got {
                        Ok(a) => {
                            app.drafts
                                .entry(draft.clone())
                                .or_default()
                                .pictures
                                .push(a.id);
                            app.drafts_changed = true;
                        }
                        Err(e) => app.fail(format!("The picture did not go: {}", e.message), cx),
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Takes a pasted picture out of his answer. The desk keeps the file;
    /// it goes nowhere unless sent.
    pub fn unpaste(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(d) = self.draft_mut() {
            d.pictures.retain(|p| p != id);
        }
        cx.notify();
    }

    /// A picture he pasted into an answer to ask `ask` on desk `desk`, as
    /// it stands, fetched when not yet asked for.
    pub fn pasted_picture(
        &mut self,
        desk: usize,
        ask: &str,
        id: &str,
        cx: &mut Context<Self>,
    ) -> Option<&Pic> {
        let base = self.desks.get(desk)?.place.url.clone();
        let key = format!("{base}{}", desk::attachment_path(ask, id));
        if !self.pictures.contains_key(&key) {
            self.pictures.insert(key.clone(), Pic::Loading);
            self.picture_order.push_back(key.clone());
            let (slot, path, format) = (
                key.clone(),
                desk::attachment_path(ask, id),
                image_format(id),
            );
            cx.spawn(async move |this, cx| {
                let svg = cx.update(|cx| cx.svg_renderer());
                let got = match desk::off(desk::media(base, path)).await {
                    Ok(bytes) => match decode(cx, format, bytes, svg).await {
                        Some((image, w, h)) => Pic::Ready {
                            image,
                            w,
                            h,
                            full: true,
                        },
                        None => Pic::Failed("The picture does not decode".into()),
                    },
                    Err(e) => Pic::Failed(e.message),
                };
                Self::put(&this, cx, &slot, got);
            })
            .detach();
        }
        self.pictures.get(&key)
    }

    // ---- Hands jobs ----

    /// Done on the hands job in front: the desk checks `out/`; his words
    /// go with it.
    pub fn hands_done(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self.current.clone() else {
            return;
        };
        let Some(ask) = self.ask(&key).cloned() else {
            return;
        };
        if !ask.is_open() || ask.check.as_ref().is_some_and(|c| c.running) {
            return;
        }
        let base = self.desks[key.0].place.url.clone();
        let words = self.draft().words.trim().to_string();
        self.checking.insert(key.clone());
        cx.spawn(async move |this, cx| {
            let got = desk::off(desk::done(base, ask.id.clone(), ask.round(), words)).await;
            let _ = this.update(cx, |app, cx| {
                if let Err(e) = got {
                    app.checking.remove(&key);
                    app.fail(format!("The check did not start: {}", e.message), cx);
                }
                app.refresh(key.0, cx);
            });
        })
        .detach();
        cx.notify();
    }

    /// The checks he started on desk `d` that came out: a pass is sent,
    /// and its agent told; a fail says why.
    fn checked(&mut self, d: usize, cx: &mut Context<Self>) {
        let mine: Vec<Key> = self.checking.iter().filter(|k| k.0 == d).cloned().collect();
        for key in mine {
            let Some(ask) = self.ask(&key).cloned() else {
                self.checking.remove(&key);
                continue;
            };
            match &ask.check {
                Some(c) if c.running => {}
                Some(c) if c.ok || !ask.is_open() => {
                    self.checking.remove(&key);
                    self.toast(format!("Done · {} · {}", ask.title, c.summary), cx);
                    self.tell(&ask, cx);
                }
                Some(c) => {
                    self.checking.remove(&key);
                    self.fail(format!("The check did not pass: {}", c.summary), cx);
                }
                None => {}
            }
        }
    }

    // ---- Telling the agent ----

    /// He answered `ask`: its session hears so through Ahjo Swarm, when
    /// he wants that and the ask says where it came from. A note that does
    /// not arrive is said, not hidden.
    pub fn tell(&mut self, ask: &Ask, cx: &mut Context<Self>) {
        if !self.config.tell_session || ask.session.trim().is_empty() {
            return;
        }
        let (node, session, note) = (self.config.node(), ask.session.clone(), desk::note(ask));
        cx.spawn(async move |this, cx| {
            let got = desk::off(desk::tell(node, session.clone(), note)).await;
            let _ = this.update(cx, |app, cx| match got {
                Ok(()) => {
                    let said = app.frame.read(cx).toast.as_ref().map(|t| t.text.clone());
                    if let Some(words) = said {
                        app.toast(format!("{words} · {session} told"), cx);
                    }
                }
                Err(e) => {
                    let words = format!("Sent, but {session} was not told: {}", e.message);
                    app.fail(words, cx)
                }
            });
        })
        .detach();
    }

    // ---- Settings ----

    /// Settings stand in front of the asks.
    pub fn settings_open(&self, cx: &App) -> bool {
        self.frame.read(cx).has_open(SETTINGS)
    }

    /// Opens Settings over the window, or closes them: a layer on the
    /// frame, which Escape closes.
    pub fn toggle_settings(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if self.settings_open(cx) {
            self.frame.update(cx, |f, cx| f.close(SETTINGS, cx));
            return;
        }
        let app = cx.entity();
        let view = cx.new(|cx| crate::settings::SettingsLayer::new(app, cx));
        let focus = view.read(cx).focus.clone();
        let me = cx.weak_entity();
        let layer = Layer::new(SETTINGS, view)
            .cover(Cover::Window)
            .focus(focus)
            .on_dismiss(move |_, cx| {
                let _ = me.update(cx, |app, cx| app.aim(Aim::Words, cx));
            });
        self.frame
            .update(cx, |f, cx| f.open(frame::Surface::Layer(layer), cx));
        cx.notify();
    }

    /// Writes the settings a moment after a change.
    fn keep_config(&mut self, cx: &mut Context<Self>) {
        settings_file::save(self, cx);
    }

    /// Opens the theme picker beside the theme's control.
    pub fn pick_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = ahjo_app_kit::theme::Direction::from_key(&self.config.theme);
        let me = cx.weak_entity();
        theme_picker::open(
            &self.frame,
            THEME,
            Spot::here(window, cx),
            Picker::new(now, true),
            move |d, _, cx| {
                let _ = me.update(cx, |app, cx| app.set_theme(d, cx));
            },
            window,
            cx,
        );
    }

    /// Draws the whole app in theme `d` and keeps it; none, as in Ahjo
    /// Swarm.
    pub fn set_theme(&mut self, d: Option<ahjo_app_kit::theme::Direction>, cx: &mut Context<Self>) {
        self.config.theme = d.map(|d| d.key().to_string()).unwrap_or_default();
        self.draw_theme(cx);
        self.keep_config(cx);
    }

    /// Draws the whole app in the theme the settings name.
    fn draw_theme(&mut self, cx: &mut Context<Self>) {
        let d = ahjo_app_kit::theme::Direction::from_key(&self.config.theme);
        cx.set_global(theme_picker::drawn(d));
        cx.refresh_windows();
    }

    pub fn set_order(&mut self, order: desk::Order, cx: &mut Context<Self>) {
        self.config.order = order;
        self.keep_config(cx);
        cx.notify();
    }

    /// Puts the list of asks away, or brings it back; kept.
    pub fn toggle_fold(&mut self, cx: &mut Context<Self>) {
        self.config.fold_list = !self.config.fold_list;
        self.keep_config(cx);
        cx.notify();
    }

    /// The surface alone in the window, or back beside the list and the
    /// answer.
    pub fn toggle_full(&mut self, cx: &mut Context<Self>) {
        self.full = !self.full && self.current.is_some();
        self.frame.update(cx, |f, cx| f.close_menu(cx));
        cx.notify();
    }

    /// The list's and the answer's widths in a window `wide`.
    pub fn columns(&self, wide: f32) -> (f32, f32) {
        tokens::columns(
            wide,
            !self.config.fold_list,
            self.config.list_w,
            self.config.talk_w,
        )
    }

    /// The least and most a column is dragged to in a window `wide`,
    /// leaving the surface its least beside the other column.
    pub fn column_range(&self, grip: Grip, wide: f32) -> (f32, f32) {
        let (list, talk) = self.columns(wide);
        match grip {
            Grip::List => (
                tokens::LIST_MIN,
                tokens::dragged(
                    tokens::LIST_MAX,
                    tokens::LIST_MIN,
                    tokens::LIST_MAX,
                    wide,
                    talk,
                ),
            ),
            Grip::Talk => (
                tokens::TALK_MIN,
                tokens::dragged(
                    tokens::TALK_MAX,
                    tokens::TALK_MIN,
                    tokens::TALK_MAX,
                    wide,
                    list,
                ),
            ),
        }
    }

    /// A column dragged to `width` (none: back at the share of the window
    /// it takes before he drags it); kept once let go.
    pub fn size_column(
        &mut self,
        grip: Grip,
        width: Option<f32>,
        keep: bool,
        cx: &mut Context<Self>,
    ) {
        match grip {
            Grip::List => self.config.list_w = width,
            Grip::Talk => self.config.talk_w = width,
        }
        if keep {
            self.keep_config(cx);
        }
        cx.notify();
    }

    pub fn toggle_tell(&mut self, cx: &mut Context<Self>) {
        self.config.tell_session = !self.config.tell_session;
        self.keep_config(cx);
        cx.notify();
    }

    /// Enter sends, or starts a new line: his words' field is made anew,
    /// with what he had written in it.
    pub fn toggle_enter_sends(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.config.enter_sends = !self.config.enter_sends;
        self.remake_words(window, cx);
        self.keep_config(cx);
        cx.notify();
    }

    /// His words' field made anew for what Enter does now, with what he
    /// had written in it.
    pub fn remake_words(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.words_stale = false;
        let (words, subs) = Self::words_field(self.config.enter_sends, window, cx);
        self.words = words;
        self._words = subs;
        self.point_words(cx);
    }

    /// Opens the settings menu beside `at`: the desks and how they stand,
    /// the list's order, and the few things done to the whole Desk.
    pub fn open_menu(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let me = cx.weak_entity();
        let act = |run: fn(&mut DeskApp, &mut Window, &mut Context<DeskApp>)| {
            let me = me.clone();
            move |window: &mut Window, cx: &mut App| {
                let _ = me.update(cx, |app, cx| run(app, window, cx));
            }
        };
        let mut rows = vec![MenuRow::note("Desks")];
        for d in &self.desks {
            let n = d.asks.iter().filter(|a| a.is_open()).count();
            let state = match &d.reach {
                Reach::Ok if n == 0 => "nothing waiting".to_string(),
                Reach::Ok => format!("{n} waiting"),
                Reach::Loading => "reaching…".to_string(),
                Reach::Away(_) => "not running".to_string(),
                Reach::Failed(_) => "answers wrongly".to_string(),
            };
            let row = MenuRow::note(d.name()).chord(&state);
            rows.push(match &d.reach {
                Reach::Failed(e) => row.hint(e.message.clone()),
                _ => row.hint(d.place.url.clone()),
            });
        }
        rows.push(MenuRow::rule());
        for o in desk::Order::ALL {
            let me = me.clone();
            rows.push(
                MenuRow::new(o.label(), move |_, cx| {
                    let _ = me.update(cx, |app, cx| app.set_order(o, cx));
                })
                .checked(self.config.order == o),
            );
        }
        rows.push(MenuRow::rule());
        let fold = if self.config.fold_list {
            "Show the list"
        } else {
            "Put the list away"
        };
        rows.push(MenuRow::new(fold, act(|app, _, cx| app.toggle_fold(cx))).chord("Ctrl B"));
        rows.push(
            MenuRow::new(
                "Read the desks again",
                act(|app, _, cx| app.refresh_all(cx)),
            )
            .chord("R"),
        );
        rows.push(
            MenuRow::new(
                "Settings",
                act(|app, window, cx| app.toggle_settings(window, cx)),
            )
            .chord("Ctrl ,"),
        );
        self.frame
            .update(cx, |f, cx| f.menu(MENU, rows, at, false, cx));
    }

    /// The settings menu is open.
    pub fn menu_open(&self, cx: &App) -> bool {
        self.frame
            .read(cx)
            .stack
            .iter()
            .any(|s| matches!(s, frame::Surface::Popup(p) if p.name().starts_with(MENU)))
    }

    /// Asks the node whether it is there, for Settings to say.
    pub fn check_node(&mut self, cx: &mut Context<Self>) {
        let node = self.config.node();
        self.node_check = None;
        cx.spawn(async move |this, cx| {
            let got = desk::off(desk::node_health(node)).await;
            let _ = this.update(cx, |app, cx| {
                app.node_check = Some(got.map(|device| match device.as_str() {
                    "" => "Reached".to_string(),
                    d => format!("Reached · {d}"),
                }));
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Adds the desks the configured ones name (`/api/desks`) on the same
    /// host, so naming one desk finds the rest.
    fn discover(&mut self, cx: &mut Context<Self>) {
        let given: Vec<String> = self.desks.iter().map(|d| d.place.url.clone()).collect();
        for base in given {
            cx.spawn(async move |this, cx| {
                let got = desk::off(desk::desks(base.clone())).await;
                let Ok(found) = got else {
                    // An older desk has no list: the configured ones stand.
                    return;
                };
                let _ = this.update(cx, |app, cx| {
                    for place in desk::siblings(&base, &found) {
                        if app
                            .desks
                            .iter()
                            .any(|d| desk::same_place(&d.place.url, &place.url))
                        {
                            continue;
                        }
                        app.desks.push(DeskState {
                            place,
                            asks: vec![],
                            reach: Reach::Loading,
                        });
                        let d = app.desks.len() - 1;
                        app.refresh(d, cx);
                    }
                    app.follow(true, cx);
                });
            })
            .detach();
        }
    }

    pub fn open_on_web(&mut self, cx: &mut Context<Self>) {
        if let Some(k) = self.current.clone() {
            let url = format!("{}/#{}", self.desks[k.0].place.url, k.1);
            cx.open_url(&url);
        }
    }

    /// Opens the shown media (a page, a clip the app does not play) in the
    /// browser.
    pub fn open_media(&mut self, cx: &mut Context<Self>) {
        if let Some(url) = self.shown_item().and_then(|i| self.page_url(&i)) {
            cx.open_url(&url);
        }
    }

    // ---- Keys ----

    /// The window's keys. In his words or a mark's, keys type; Escape gives
    /// the keyboard to the surface, where letters pick tools and arrows
    /// step. Ctrl+Enter sends from anywhere.
    pub fn on_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        // The zoom keys size the whole app, wherever the keyboard is: the
        // picture zooms by its own keys and the wheel.
        if let Some(z) = ahjo_app_kit::zoom::from_key(&e.keystroke) {
            cx.stop_propagation();
            return self.zoom_app(z, 1, window, cx);
        }
        let key = e.keystroke.key.as_str();
        let m = &e.keystroke.modifiers;
        if m.control && !m.alt && key == "," {
            return self.toggle_settings(window, cx);
        }
        // Settings, or a popup, stand in front of the asks: their keys
        // wait.
        if !self.frame.read(cx).stack.is_empty() {
            return;
        }
        if m.control && !m.alt {
            match key {
                "pagedown" => return self.step_ask(1, cx),
                "pageup" => return self.step_ask(-1, cx),
                "b" => return self.toggle_fold(cx),
                "enter" if !self.typing(window, cx) => return self.send(Verdict::Send, cx),
                // Pasted anywhere, a picture goes with the answer and words
                // go into it.
                "v" if !self.typing(window, cx) && !self.read_only() => {
                    if let Some(item) = cx.read_from_clipboard() {
                        let picture = item.entries().iter().any(|e| {
                            matches!(
                                e,
                                ClipboardEntry::Image(_) | ClipboardEntry::ExternalPaths(_)
                            )
                        });
                        if picture {
                            self.paste_pictures(item, cx);
                        } else if let Some(text) = item.text() {
                            self.words.update(cx, |f, cx| {
                                let text = f.text.clone() + &text.replace("\r\n", "\n");
                                f.set(&text, false, cx);
                            });
                            self.words_changed(cx);
                            self.aim(Aim::Words, cx);
                        }
                    }
                    return;
                }
                _ => {}
            }
        }
        if key == "f11" {
            return self.toggle_full(cx);
        }
        if self.typing(window, cx) {
            if key == "escape" {
                if self.grading.writing.is_some() {
                    self.grading.writing = None;
                }
                self.aim(Aim::Surface, cx);
            }
            return;
        }
        if m.control || m.alt || m.platform {
            return;
        }
        let video = self.player.is_some();
        // An answered ask is looked at, not answered again: its keys move
        // and look, nothing marks or writes.
        if self.read_only()
            && matches!(
                key,
                "p" | "b" | "a" | "d" | "n" | "delete" | "backspace" | "enter" | "tab"
            )
        {
            return;
        }
        // A passage chosen on the paper: the grader's pens.
        if self.grading.chosen.is_some() && self.paper_key(key, cx) {
            return;
        }
        let writes = self.read_only() && e.keystroke.key_char.is_some();
        match key {
            "j" | "pagedown" => self.step_ask(1, cx),
            "k" | "pageup" => self.step_ask(-1, cx),
            "v" => self.set_tool(Tool::Look, cx),
            "p" => self.set_tool(Tool::Pin, cx),
            "b" => self.set_tool(Tool::Box, cx),
            "a" => self.set_tool(Tool::Arrow, cx),
            "d" => self.set_tool(Tool::Pen, cx),
            "n" => self.note(cx),
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
            "delete" | "backspace" => {
                if let Some(id) = self.selected.clone() {
                    self.delete_mark(&id, cx);
                }
            }
            "escape" => {
                if self.drawing.is_some() {
                    self.drawing = None;
                } else if self.tool != Tool::Look {
                    self.set_tool(Tool::Look, cx);
                } else if self.view.side {
                    self.view.side = false;
                } else if self.selected.is_some() {
                    self.select_mark(None, false, cx);
                } else if self.full {
                    self.full = false;
                }
                cx.notify();
            }
            // Anything else is him starting to write.
            "enter" | "tab" => self.aim(Aim::Words, cx),
            _ if writes || self.full => {}
            _ => {
                if let Some(ch) = e.keystroke.key_char.clone() {
                    self.words.update(cx, |f, cx| {
                        let text = f.text.clone() + &ch;
                        f.set(&text, false, cx);
                    });
                    self.words_changed(cx);
                    self.aim(Aim::Words, cx);
                }
            }
        }
    }

    fn typing(&self, window: &Window, cx: &App) -> bool {
        self.words.read(cx).is_focused(window)
            || self.mark_note.read(cx).is_focused(window)
            || self.pen.read(cx).is_focused(window)
    }

    fn fps_frames(&self) -> i64 {
        self.player
            .as_ref()
            .map(|(_, p)| p.clip.fps.round() as i64)
            .unwrap_or(1)
            .max(1)
    }
}

impl KeepsSettings for DeskApp {
    type Value = desk::Config;

    fn settings_file(&mut self) -> &mut SettingsFile<desk::Config> {
        &mut self.config
    }

    fn settings_failed(&mut self, error: Error, cx: &mut Context<Self>) {
        self.fail(error.message, cx);
    }

    /// `desk.json` was edited outside the Desk: what it changed applies
    /// now. The desks it names are read at the next start, as `--desk`
    /// and `AHJO_DESK_DESKS` may stand in for them.
    fn settings_reloaded(&mut self, old: desk::Config, cx: &mut Context<Self>) {
        if old.theme != self.config.theme {
            self.draw_theme(cx);
        }
        if old.enter_sends != self.config.enter_sends {
            self.words_stale = true;
        }
        if old.node != self.config.node {
            let node = self.config.node.clone();
            self.node_field.update(cx, |f, cx| f.set(&node, false, cx));
            self.node_check = None;
        }
        let path = self.config.path().map(|p| p.display().to_string());
        self.toast(
            format!("Settings read again from {}", path.unwrap_or_default()),
            cx,
        );
        cx.notify();
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

/// When an ask was answered, as the desk dates it (RFC 3339, which sorts
/// as text).
fn answered_at(a: &Ask) -> &str {
    a.answer.as_ref().map_or("", |x| x.at.as_str())
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
        Some("tif" | "tiff") => ImageFormat::Tiff,
        _ => ImageFormat::Png,
    }
}

/// One thing he pasted that may be a picture.
enum Pasted {
    Bytes(ImageFormat, Vec<u8>),
    File(std::path::PathBuf),
}

impl Pasted {
    /// The picture as the desk takes it: PNG, JPEG, GIF or WebP as they
    /// are, any other picture (a bitmap from a Windows screenshot) as PNG.
    fn ready(self) -> Result<Vec<u8>, Error> {
        let refused = |words: String| Error::new("picture_unready", words);
        let (format, bytes) = match self {
            Pasted::Bytes(f, b) => (f, b),
            Pasted::File(path) => {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
                if !matches!(
                    ext.as_str(),
                    "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tif" | "tiff"
                ) {
                    return Err(refused(format!("{name} is not a picture")));
                }
                let bytes = std::fs::read(&path).map_err(|e| refused(format!("{name}: {e}")))?;
                (image_format(&name), bytes)
            }
        };
        match format {
            ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Gif | ImageFormat::Webp => {
                Ok(bytes)
            }
            ImageFormat::Svg => Err(refused("a drawing (SVG) does not go as a picture".into())),
            _ => {
                let picture = image::load_from_memory(&bytes)
                    .map_err(|e| refused(format!("the picture does not read: {e}")))?;
                let mut png = std::io::Cursor::new(vec![]);
                picture
                    .write_to(&mut png, image::ImageFormat::Png)
                    .map_err(|e| refused(format!("the picture does not turn to PNG: {e}")))?;
                Ok(png.into_inner())
            }
        }
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
