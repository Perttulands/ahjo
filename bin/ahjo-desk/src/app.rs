//! The Desk's window: the shared surface where an agent's ask is shown (its
//! pictures, clips, page or document) and the operator answers it in his
//! own words, marking what he sees. Every desk's open asks in one queue; the
//! one in front, its rounds as a dialogue, what he has written and marked
//! on it so far, and the surface that shows and takes marks.

use crate::answer::{self, Draft, Said, Verdict};
use crate::desk::{self, Place};
use crate::marks::{self, Tool};
use crate::model::{self, Ask, Form, Mark, Media, MediaKind};
use crate::page::{self, FromPage};
use crate::player::{Out, Player};
use crate::tokens;
use crate::video::Clip;
use ahjo_app_kit::widgets::field::{self, FieldEvent, TextField};
use ahjo_app_kit::widgets::markdown::{self, Block};
use ahjo_app_kit::widgets::select::Selectable;
use gpui::{
    App, AppContext, AsyncApp, Bounds, Context, Entity, FocusHandle, Focusable, ImageFormat,
    KeyDownEvent, Pixels, Point, RenderImage, WeakEntity, Window,
};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Where a desk's asks stand.
pub enum Reach {
    Loading,
    Ok,
    Failed(String),
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
            Reach::Failed(_) => {}
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

/// Where the keyboard goes once the window is drawn next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Aim {
    Words,
    MarkNote,
    Surface,
}

type DraftKey = (usize, String, u32);

pub struct DeskApp {
    pub focus: FocusHandle,
    pub desks: Vec<DeskState>,
    pub current: Option<Key>,
    pub drafts: HashMap<DraftKey, Draft>,
    pub view: View,
    pub pictures: HashMap<String, Pic>,
    picture_order: VecDeque<String>,
    pub player: Option<(String, Player)>,
    pub clip_failed: HashMap<String, String>,
    clip_loading: Option<String>,
    /// His words: always there, the keyboard's home.
    pub words: Entity<TextField>,
    /// The selected mark's words.
    pub mark_note: Entity<TextField>,
    pub selected: Option<String>,
    pub tool: Tool,
    pub drawing: Option<Drawing>,
    /// A page takes clicks as marks rather than following them.
    pub page_comment: bool,
    aim: Option<Aim>,
    pub toast: Option<(String, bool)>,
    toast_gen: u64,
    pub sending: bool,
    pub notice: Option<String>,
    live: Arc<AtomicU64>,
    /// Where each surface panel was laid out, where its picture was drawn
    /// in it, and the clip's track.
    pub rooms: Rc<RefCell<Vec<Bounds<Pixels>>>>,
    pub drawn_at: Rc<RefCell<Vec<Option<Bounds<Pixels>>>>>,
    pub track: Rc<RefCell<Option<Bounds<Pixels>>>>,
    pub held: Option<(Point<Pixels>, (f32, f32))>,
    play_gen: u64,
    /// He has chosen an ask; until then the queue's first is in front.
    chosen: bool,
    /// The document's text as selectable blocks, kept per ask and round.
    pub doc_select: Rc<RefCell<Selectable>>,
    pub doc_blocks: Option<(String, Rc<Vec<Block>>)>,
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
        places: Vec<Place>,
        notice: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let words = cx.new(|cx| {
            TextField::new(cx, "Your thoughts, in your words")
                .prose()
                .reading()
        });
        cx.subscribe_in(&words, window, Self::on_words).detach();
        let mark_note = cx.new(|cx| TextField::new(cx, "What about it?").multiline().reading());
        cx.subscribe_in(&mark_note, window, Self::on_mark_note)
            .detach();
        cx.observe_window_activation(window, |this, window, cx| {
            this.follow(window.is_window_active(), cx);
            if window.is_window_active() {
                this.refresh_all(cx);
            }
        })
        .detach();
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
            view: View::default(),
            pictures: HashMap::new(),
            picture_order: VecDeque::new(),
            player: None,
            clip_failed: HashMap::new(),
            clip_loading: None,
            words,
            mark_note,
            selected: None,
            tool: Tool::Look,
            drawing: None,
            page_comment: false,
            aim: Some(Aim::Words),
            toast: None,
            toast_gen: 0,
            sending: false,
            notice,
            live: Arc::new(AtomicU64::new(0)),
            rooms: Rc::new(RefCell::new(vec![])),
            drawn_at: Rc::new(RefCell::new(vec![])),
            track: Rc::new(RefCell::new(None)),
            held: None,
            play_gen: 0,
            chosen: false,
            doc_select: Rc::default(),
            doc_blocks: None,
            #[cfg(windows)]
            page: Rc::new(RefCell::new(None)),
            page_said,
            #[cfg(windows)]
            page_failed: Rc::default(),
        };
        app.discover(cx);
        app.refresh_all(cx);
        app.follow(true, cx);
        app
    }

    // ---- The queue ----

    pub fn reached(&self) -> Reached {
        reached(self.desks.iter().map(|d| &d.reach))
    }

    /// Every open ask, quickest first, then the longest waiting: each
    /// answer frees an agent, and the quick ones first free the most
    /// agents soonest.
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

    fn draft_key(&self) -> Option<DraftKey> {
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
        match self.aim.take() {
            Some(Aim::Words) => field::focus(&self.words, window, cx),
            Some(Aim::MarkNote) => field::focus(&self.mark_note, window, cx),
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
                (
                    draft.said(q).words,
                    format!("Your thoughts on {}", q.title.trim()),
                )
            }
            (Some(a), _) if a.form() == Form::Grill => {
                (draft.words, "Your thoughts on the whole round".into())
            }
            (Some(a), _) if a.form() == Form::Hands => (
                draft.words,
                "What stops the job, or a note on how it went".into(),
            ),
            _ => (draft.words, "Your thoughts, in your words".into()),
        };
        self.words.update(cx, |f, cx| {
            f.set_placeholder(&hint);
            f.set(&text, false, cx);
        });
    }

    /// Puts an ask in front: its draft, its first picture, and the next
    /// ask's pictures fetched behind it so moving on is instant.
    fn show(&mut self, key: Option<Key>, cx: &mut Context<Self>) {
        self.current = key;
        self.view = View::default();
        self.held = None;
        self.drawing = None;
        self.selected = None;
        self.page_comment = false;
        self.point_words(cx);
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

    pub fn step_ask(&mut self, by: i64, cx: &mut Context<Self>) {
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
    pub fn doc_blocks(&mut self) -> Option<Rc<Vec<Block>>> {
        let ask = self.current_ask()?;
        let doc = ask.doc.as_ref()?;
        let key = format!("{}/{}", ask.id, ask.round());
        if self.doc_blocks.as_ref().map(|(k, _)| k) != Some(&key) {
            let blocks = Rc::new(markdown::parse(&doc.text));
            self.doc_blocks = Some((key, blocks));
        }
        self.doc_blocks.as_ref().map(|(_, b)| b.clone())
    }

    // ---- Marks ----

    /// His marks on this round as they stand.
    pub fn marks(&self) -> Vec<Mark> {
        match self.current_ask() {
            Some(a) => self.draft().marks(a).to_vec(),
            None => vec![],
        }
    }

    fn edit_marks(&mut self, f: impl FnOnce(&mut Vec<Mark>)) {
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
        let Some(item) = self.shown_item() else {
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

    /// The document's words he selected: a mark on that passage.
    pub fn mark_passage(&mut self, selected: String, cx: &mut Context<Self>) {
        let Some(doc) = self.current_ask().and_then(|a| a.doc.clone()) else {
            return;
        };
        if selected.trim().is_empty() {
            return;
        }
        match marks::locate(&doc.text, &selected) {
            Some((start, end)) => self.add_mark(marks::on_text(String::new(), start, end), cx),
            None => self.say(
                "That passage could not be found in the document's text: say it in your words",
                true,
                cx,
            ),
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

    fn on_words(
        &mut self,
        _: &Entity<TextField>,
        e: &FieldEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match e {
            FieldEvent::Changed => self.words_changed(cx),
            FieldEvent::SubmitAlt => self.send(Verdict::Send, window, cx),
            _ => {}
        }
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
        cx.notify();
    }

    fn on_mark_note(
        &mut self,
        _: &Entity<TextField>,
        e: &FieldEvent,
        window: &mut Window,
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
            // Enter: the mark is said; back to his words.
            FieldEvent::Submit => self.aim(Aim::Words, cx),
            FieldEvent::SubmitAlt => self.send(Verdict::Send, window, cx),
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
    /// the next ask while the desk keeps it.
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
        let marks = draft.marks(&ask).to_vec();
        let payload = match answer::payload(&ask, &draft, verdict, marks.len()) {
            Ok(p) => p,
            Err(why) => return self.say(why, true, cx),
        };
        let changed = draft.marks.as_ref().is_some_and(|m| *m != ask.marks);
        self.sending = true;
        let queue = self.queue();
        let next = queue
            .iter()
            .position(|k| *k == key)
            .and_then(|i| queue.get(i + 1).or_else(|| queue.get(i.wrapping_sub(1))))
            .cloned();
        let base = self.desks[key.0].place.url.clone();
        let title = ask.title.clone();
        let revision = ask.revision;
        cx.spawn_in(window, async move |this, cx| {
            let id = key.1.clone();
            let sent = desk::runtime()
                .spawn(async move {
                    if changed {
                        desk::save_marks(base.clone(), id.clone(), &marks, revision).await?;
                    }
                    desk::answer(base, id, payload).await
                })
                .await
                .unwrap_or_else(|e| Err(e.to_string()));
            let _ = this.update_in(cx, |app, _, cx| {
                app.sending = false;
                match sent {
                    Ok(now) => {
                        if let Some(a) = app.desks[key.0].asks.iter_mut().find(|a| a.id == now.id) {
                            *a = now;
                        }
                        app.drafts.retain(|k, _| !(k.0 == key.0 && k.1 == key.1));
                        app.say(format!("Sent · {title}"), false, cx);
                        if app.current.as_ref() == Some(&key) {
                            app.open(next.filter(|n| app.ask(n).is_some_and(|a| a.is_open())), cx);
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
        // A desk that did not answer is being asked again: say so, or
        // trying again looks like nothing happened until it times out.
        if matches!(self.desks[d].reach, Reach::Failed(_)) {
            self.desks[d].reach = Reach::Loading;
            cx.notify();
        }
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

    /// Adds the desks the configured ones name (`/api/desks`) on the same
    /// host, so naming one desk finds the rest.
    fn discover(&mut self, cx: &mut Context<Self>) {
        let given: Vec<String> = self.desks.iter().map(|d| d.place.url.clone()).collect();
        for base in given {
            cx.spawn(async move |this, cx| {
                let got = desk::runtime()
                    .spawn(desk::desks(base.clone()))
                    .await
                    .unwrap_or_else(|e| Err(e.to_string()));
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
        let key = e.keystroke.key.as_str();
        let m = &e.keystroke.modifiers;
        if m.control && !m.alt {
            match key {
                "pagedown" => return self.step_ask(1, cx),
                "pageup" => return self.step_ask(-1, cx),
                "enter" if !self.typing(window, cx) => return self.send(Verdict::Send, window, cx),
                _ => {}
            }
        }
        if self.typing(window, cx) {
            if key == "escape" {
                self.aim(Aim::Surface, cx);
            }
            return;
        }
        if m.control || m.alt || m.platform {
            return;
        }
        let video = self.player.is_some();
        match key {
            "j" | "pagedown" => self.step_ask(1, cx),
            "k" | "pageup" => self.step_ask(-1, cx),
            "v" => self.set_tool(Tool::Look, cx),
            "p" => self.set_tool(Tool::Pin, cx),
            "b" => self.set_tool(Tool::Box, cx),
            "a" => self.set_tool(Tool::Arrow, cx),
            "d" => self.set_tool(Tool::Pen, cx),
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
                } else {
                    self.select_mark(None, false, cx);
                }
                cx.notify();
            }
            // Anything else is him starting to write.
            "enter" | "tab" => self.aim(Aim::Words, cx),
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
        self.words.read(cx).is_focused(window) || self.mark_note.read(cx).is_focused(window)
    }

    fn fps_frames(&self) -> i64 {
        self.player
            .as_ref()
            .map(|(_, p)| p.clip.fps.round() as i64)
            .unwrap_or(1)
            .max(1)
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
