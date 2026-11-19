//! Editing a page in place.
//!
//! The page's Markdown is written in the kit's text view (its undo, its
//! selection, its keys), coloured by its marks, beside the page itself,
//! which the reader shows live as it is written. What a writer of notes
//! expects is here: `[[` offers the brain's pages by name, Enter goes on
//! with a list, Ctrl+B and Ctrl+I mark words, Ctrl+1 to 3 make a heading,
//! Ctrl+F finds. Ctrl+S writes the page through Citadel, the brain's one
//! writer, which runs `brain check` and commits it, so what is saved is a
//! page the brain accepts or a refusal that says why. A draft is kept
//! beside the app's cache while it is unsaved, so leaving loses nothing.

use crate::app::BrainApp;
use crate::citadel;
use crate::tokens::editor as e;
use ahjo_app_kit::theme::{FAMILY, alpha, size, theme};
use ahjo_app_kit::widgets::code::{Code, CodeEvent};
use ahjo_app_kit::widgets::field::{self, FieldEvent, TextField};
use ahjo_app_kit::widgets::kit::{self, Kind};
use ahjo_app_kit::widgets::syntax::MARKDOWN;
use gpui::{
    Context, Entity, EventEmitter, Hsla, KeyDownEvent, MouseButton, ParentElement, Render,
    WeakEntity, Window, anchored, deferred, div, prelude::*, px,
};
use serde::{Deserialize, Serialize};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

/// A page a link can name, as `[[` offers it.
#[derive(Clone, Debug)]
pub struct Name {
    pub slug: String,
    pub title: String,
    pub aliases: Vec<String>,
    pub hue: Hsla,
    pub subject: String,
}

/// What the editor tells the app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorEvent {
    /// The text settled after a change: the page shown beside it follows.
    Changed,
    /// Citadel wrote and committed the page.
    Saved,
    /// Done (Esc): back to reading.
    Leave,
}

/// The page as it is written, and where it came from.
pub struct Opened {
    /// The page's file, under the brain's root, with `/`.
    pub path: String,
    pub title: String,
    /// Its file as read when editing began.
    pub base: String,
    /// The draft kept from before, when there is one.
    pub draft: Option<Draft>,
}

/// An unsaved page, kept in the app's cache.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Draft {
    pub path: String,
    /// The file the draft was begun from.
    pub base: String,
    pub text: String,
}

/// Where a page's draft is kept.
pub fn draft_file(cache: &Path, path: &str) -> PathBuf {
    cache
        .join("drafts")
        .join(format!("{}.json", path.replace(['/', '\\'], "__")))
}

pub fn read_draft(cache: &Path, path: &str) -> Option<Draft> {
    let bytes = std::fs::read(draft_file(cache, path)).ok()?;
    serde_json::from_slice::<Draft>(&bytes)
        .ok()
        .filter(|d| d.path == path)
}

fn keep_draft(cache: &Path, draft: &Draft) -> std::io::Result<()> {
    let file = draft_file(cache, &draft.path);
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = file.with_extension(format!("json.{}", std::process::id()));
    std::fs::write(&tmp, serde_json::to_vec(draft)?)?;
    std::fs::rename(&tmp, &file)
}

fn drop_draft(cache: &Path, path: &str) {
    let _ = std::fs::remove_file(draft_file(cache, path));
}

/// Where `[[` was opened before the caret and not yet closed on its line:
/// the start of what is typed after it.
pub fn link_open(text: &str, caret: usize) -> Option<usize> {
    let caret = caret.min(text.len());
    let line = text[..caret].rfind('\n').map_or(0, |i| i + 1);
    let before = &text[line..caret];
    let open = before.rfind("[[")?;
    let typed = &before[open + 2..];
    (!typed.contains(['[', ']', '|', '#'])).then_some(line + open + 2)
}

/// The pages `[[query` may mean, best first: a name that begins with it,
/// then one with every typed word beginning one of its words, then one
/// that holds it. With nothing typed, the first in `names`' own order.
pub fn complete(query: &str, names: &[Name]) -> Vec<usize> {
    let q = query.trim().to_lowercase();
    let words: Vec<&str> = q.split_whitespace().collect();
    let split = |s: &str| -> Vec<String> {
        s.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(str::to_string)
            .collect()
    };
    let mut ranked: Vec<(u8, usize, usize)> = names
        .iter()
        .enumerate()
        .filter_map(|(i, n)| {
            if q.is_empty() {
                return Some((0, 0, i));
            }
            let title = n.title.to_lowercase();
            let mut all = vec![n.slug.to_lowercase(), title.clone()];
            all.extend(n.aliases.iter().map(|a| a.to_lowercase()));
            let rank = if all.iter().any(|s| s.starts_with(&q)) {
                0
            } else if words.iter().all(|w| {
                all.iter()
                    .flat_map(|s| split(s))
                    .any(|part| part.starts_with(w))
            }) {
                1
            } else if all.iter().any(|s| s.contains(&q)) {
                2
            } else {
                return None;
            };
            Some((rank, n.title.len(), i))
        })
        .collect();
    ranked.sort();
    ranked.into_iter().take(e::PICKS).map(|r| r.2).collect()
}

/// What Enter does at the end of a list's line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Next {
    /// The next item, begun with this.
    Item(String),
    /// The item was empty: the list ends, its mark taken away.
    End,
}

/// A list's, a task list's or a quote's line, as written before the caret:
/// how Enter goes on with it.
pub fn list_next(line: &str) -> Option<Next> {
    let trimmed = line.trim_start();
    let indent = &line[..line.len() - trimmed.len()];
    let digits = trimmed.chars().take_while(char::is_ascii_digit).count();
    let (mark, rest) = if let Some((m, rest)) = ["- ", "* ", "+ ", "> "]
        .iter()
        .find_map(|m| trimmed.strip_prefix(m).map(|r| (m.to_string(), r)))
    {
        (m, rest)
    } else if digits > 0 && digits < 10 {
        let after = &trimmed[digits..];
        let close = after.chars().next().filter(|c| *c == '.' || *c == ')')?;
        let rest = after[1..].strip_prefix(' ')?;
        let n: u64 = trimmed[..digits].parse().ok()?;
        (format!("{}{close} ", n + 1), rest)
    } else {
        return None;
    };
    let boxed = ["[ ] ", "[x] ", "[X] "].iter().any(|b| rest.starts_with(b));
    let words = if boxed { &rest[4..] } else { rest };
    if words.trim().is_empty() {
        return Some(Next::End);
    }
    let task = if boxed { "[ ] " } else { "" };
    Some(Next::Item(format!("{indent}{mark}{task}")))
}

/// Marks `sel` with `mark` on both sides (`**` bold, `*` emphasis), or
/// takes the marks away when they are already there: the stretch to
/// replace, what goes there, and what is selected after.
pub fn toggle_mark(
    text: &str,
    sel: Range<usize>,
    mark: &str,
) -> (Range<usize>, String, Range<usize>) {
    let m = mark.len();
    let inner = &text[sel.clone()];
    let marked = sel.start >= m
        && text.get(sel.start - m..sel.start) == Some(mark)
        && text.get(sel.end..sel.end + m) == Some(mark);
    if marked {
        (
            sel.start - m..sel.end + m,
            inner.to_string(),
            sel.start - m..sel.end - m,
        )
    } else {
        (
            sel.clone(),
            format!("{mark}{inner}{mark}"),
            sel.start + m..sel.end + m,
        )
    }
}

/// A line as a heading of `level`, or as plain text when it already is
/// one of that level.
pub fn heading(line: &str, level: usize) -> String {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    let words = if hashes > 0 && line[hashes..].starts_with(' ') {
        &line[hashes + 1..]
    } else if hashes > 0 && line.len() == hashes {
        ""
    } else {
        line
    };
    if hashes == level && words.len() < line.len() {
        words.to_string()
    } else {
        format!("{} {words}", "#".repeat(level))
    }
}

/// Citadel's refusal in the words of someone writing a page: what the
/// brain's check found, without the advice meant for an agent's commit.
pub fn plain_refusal(said: &str) -> String {
    match said.split_once("brain check refuses this commit") {
        Some((_, rest)) => {
            let found = rest.split_once(":\n").map_or(rest, |(_, f)| f).trim();
            format!("The brain's check found: {found}")
        }
        None => said.trim().to_string(),
    }
}

/// The front matter's `updated` date, when it has one other than `today`:
/// where it is written, and today's to write there.
pub fn updated_today(text: &str, today: time::Date) -> Option<(Range<usize>, String)> {
    let front = text.strip_prefix("---\n")?;
    let end = front.find("\n---")?;
    let mut at = 4;
    for line in front[..end].split_inclusive('\n') {
        if let Some(rest) = line.strip_prefix("updated:") {
            let value = rest.trim();
            let start = at + "updated:".len() + (rest.len() - rest.trim_start().len());
            let date = format!(
                "{}-{:02}-{:02}",
                today.year(),
                u8::from(today.month()),
                today.day()
            );
            return (value != date).then_some((start..start + value.len(), date));
        }
        at += line.len();
    }
    None
}

/// Where `needle` is in `text`, ignoring the case of Latin letters.
pub fn find_all(text: &str, needle: &str) -> Vec<Range<usize>> {
    let n = needle.len();
    if n == 0 {
        return vec![];
    }
    let (hay, pin) = (text.as_bytes(), needle.as_bytes());
    let mut out = vec![];
    let mut at = 0;
    while at + n <= hay.len() {
        if text.is_char_boundary(at)
            && text.is_char_boundary(at + n)
            && hay[at..at + n].eq_ignore_ascii_case(pin)
        {
            out.push(at..at + n);
            at += n;
        } else {
            at += 1;
        }
    }
    out
}

/// What the editor says of the page: how it stands, or why it was not
/// written.
#[derive(Clone, Debug, PartialEq)]
enum Said {
    Note(String),
    Refused(String),
}

/// Why a save did not write the page.
enum NotSaved {
    /// Someone else changed it meanwhile: the page as it is now.
    Changed(String),
    /// Citadel's words: a check that refused, a writer mid-change, or no
    /// Citadel.
    Refused(String),
}

/// The open `[[`: where its typed words start, and the pages offered.
struct Picking {
    start: usize,
    picks: Vec<usize>,
    at: usize,
}

pub struct Editor {
    app: WeakEntity<BrainApp>,
    pub code: Entity<Code>,
    find: Entity<TextField>,
    finding: bool,
    found: Vec<Range<usize>>,
    path: String,
    title: String,
    /// The file as it was when this began (or last saved): a save finds
    /// out by it whether someone else changed the page meanwhile.
    base: String,
    /// What the brain holds of this page, as far as this editor knows.
    saved: String,
    citadel: String,
    cache: Option<PathBuf>,
    names: Rc<Vec<Name>>,
    picking: Option<Picking>,
    said: Option<Said>,
    saving: bool,
    /// A change not yet settled, by its count.
    pending: u64,
}

impl EventEmitter<EditorEvent> for Editor {}

impl Editor {
    pub fn new(
        app: WeakEntity<BrainApp>,
        opened: Opened,
        names: Rc<Vec<Name>>,
        citadel: String,
        cache: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Editor {
        let (text, said) = match &opened.draft {
            Some(d) if d.text != opened.base && d.base == opened.base => (
                d.text.clone(),
                Some(Said::Note(
                    "Your unsaved draft is back. Ctrl+S saves it to the brain.".into(),
                )),
            ),
            Some(d) if d.text != opened.base => (
                d.text.clone(),
                Some(Said::Note(
                    "Your unsaved draft is back, but the page changed in the brain since you \
                     began it."
                        .into(),
                )),
            ),
            _ => (opened.base.clone(), None),
        };
        // A draft begun from an older page saves against that page, so the
        // save says the page changed instead of writing over it unseen.
        let base = opened
            .draft
            .as_ref()
            .filter(|d| d.text != opened.base)
            .map_or(opened.base.clone(), |d| d.base.clone());
        let code = cx.new(|cx| {
            let mut c = Code::new(cx);
            c.set_wrap(true, cx);
            c.set_text_size(e::TEXT, cx);
            c.set_lang(Some(&MARKDOWN), cx);
            c.set(&text, cx);
            c
        });
        code.update(cx, |c, cx| {
            c.edit(true, window, cx);
            // The caret after the front matter, where the words begin.
            let at = crate::corpus::body(&text).as_ptr() as usize - text.as_ptr() as usize;
            c.select(at.min(text.len())..at.min(text.len()), cx);
        });
        cx.subscribe_in(&code, window, |me, _, ev, window, cx| match ev {
            CodeEvent::Changed => me.changed(cx),
            CodeEvent::Save => me.save(cx),
            CodeEvent::Escape => {
                if me.picking.take().is_some() {
                    cx.notify();
                } else {
                    let _ = window;
                    cx.emit(EditorEvent::Leave);
                }
            }
            CodeEvent::Moved => {
                if me.picking.is_some() {
                    me.look_for_link(cx);
                }
            }
        })
        .detach();
        let find = cx.new(|cx| TextField::new(cx, "Find in the page"));
        cx.subscribe_in(&find, window, |me, _, ev, window, cx| match ev {
            FieldEvent::Changed => me.refind(cx),
            FieldEvent::Submit | FieldEvent::Down => me.step_find(true, window, cx),
            FieldEvent::Up => me.step_find(false, window, cx),
            _ => {}
        })
        .detach();
        Editor {
            app,
            code,
            find,
            finding: false,
            found: vec![],
            path: opened.path,
            title: opened.title,
            base,
            saved: opened.base,
            citadel,
            cache,
            names,
            picking: None,
            said,
            saving: false,
            pending: 0,
        }
    }

    pub fn text<'a>(&self, cx: &'a gpui::App) -> &'a str {
        self.code.read(cx).text()
    }

    /// Written and not yet in the brain.
    pub fn unsaved(&self, cx: &gpui::App) -> bool {
        self.text(cx) != self.saved
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        self.look_for_link(cx);
        if self.finding {
            self.refind(cx);
        }
        if matches!(self.said, Some(Said::Note(_))) {
            self.said = None;
        }
        self.pending += 1;
        let pending = self.pending;
        cx.spawn(async move |me, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(e::SETTLE_MS))
                .await;
            let _ = me.update(cx, |me, cx| {
                if me.pending == pending {
                    me.settled(cx);
                }
            });
        })
        .detach();
        cx.notify();
    }

    /// Typing paused: the page beside follows, and the draft is kept.
    fn settled(&mut self, cx: &mut Context<Self>) {
        cx.emit(EditorEvent::Changed);
        self.keep(cx);
    }

    /// What the brain holds of this page, as far as this editor knows.
    pub fn saved_text(&self) -> &str {
        &self.saved
    }

    /// Keeps the text as a draft while it is unsaved, and forgets the
    /// draft once it is not.
    pub fn keep(&mut self, cx: &mut Context<Self>) {
        let Some(cache) = self.cache.clone() else {
            return;
        };
        let text = self.text(cx).to_string();
        let path = self.path.clone();
        let draft = (text != self.saved).then(|| Draft {
            path: path.clone(),
            base: self.base.clone(),
            text,
        });
        cx.background_spawn(async move {
            match draft {
                Some(d) => {
                    if let Err(err) = keep_draft(&cache, &d) {
                        eprintln!("the draft of {path} was not kept: {err}");
                    }
                }
                None => drop_draft(&cache, &path),
            }
        })
        .detach();
    }

    /// Opens or closes the pages offered for the `[[` before the caret.
    fn look_for_link(&mut self, cx: &mut Context<Self>) {
        let code = self.code.read(cx);
        let caret = code.selection().end;
        let text = code.text();
        let open = code
            .selection()
            .is_empty()
            .then(|| link_open(text, caret))
            .flatten();
        self.picking = open.and_then(|start| {
            let picks = complete(&text[start..caret], &self.names);
            (!picks.is_empty()).then_some(Picking {
                start,
                picks,
                at: 0,
            })
        });
        cx.notify();
    }

    /// Writes the page picked into the open `[[`, closed.
    fn take_pick(&mut self, n: usize, cx: &mut Context<Self>) {
        let Some(p) = self.picking.take() else {
            return;
        };
        let Some(&name) = p.picks.get(n) else {
            return;
        };
        let slug = self.names[name].slug.clone();
        self.code.update(cx, |c, cx| {
            let caret = c.selection().end;
            let closed = c.text()[caret..].starts_with("]]");
            let with = if closed {
                slug.clone()
            } else {
                format!("{slug}]]")
            };
            c.replace_all(&[p.start..caret], &with, cx);
            let after = p.start + slug.len() + 2;
            c.select(after..after, cx);
        });
        cx.notify();
    }

    /// The keys a writer of notes expects, before the text view's own.
    fn key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &ev.keystroke;
        let m = &k.modifiers;
        let ctrl = (m.control || m.platform) && !m.alt;
        let plain = !m.control && !m.platform && !m.alt;
        let in_text = self.code.read(cx).is_focused(window);
        // The pages offered for a link take the arrows and Enter.
        if let Some(p) = &mut self.picking
            && in_text
            && plain
        {
            let n = p.picks.len();
            let handled = match k.key.as_str() {
                "down" => {
                    p.at = (p.at + 1) % n;
                    true
                }
                "up" => {
                    p.at = (p.at + n - 1) % n;
                    true
                }
                "enter" | "tab" => {
                    let at = p.at;
                    self.take_pick(at, cx);
                    true
                }
                "escape" => {
                    self.picking = None;
                    true
                }
                _ => false,
            };
            if handled {
                cx.stop_propagation();
                cx.notify();
                return;
            }
        }
        if self.finding && !in_text && k.key == "escape" {
            self.close_find(window, cx);
            cx.stop_propagation();
            return;
        }
        if ctrl && k.key == "f" && !m.shift {
            self.open_find(window, cx);
            cx.stop_propagation();
            return;
        }
        if !in_text {
            return;
        }
        let done = match k.key.as_str() {
            "enter" if plain && !m.shift => self.go_on_with_list(cx),
            "b" if ctrl && !m.shift => self.mark("**", cx),
            "i" if ctrl && !m.shift => self.mark("*", cx),
            "1" | "2" | "3" if ctrl && !m.shift => {
                let level = k.key.parse().unwrap_or(1);
                self.make_heading(level, cx)
            }
            _ => false,
        };
        if done {
            cx.stop_propagation();
            window.prevent_default();
        }
    }

    /// Enter on a list's line begins the next item; on an empty item it
    /// ends the list.
    fn go_on_with_list(&mut self, cx: &mut Context<Self>) -> bool {
        self.code.update(cx, |c, cx| {
            let sel = c.selection();
            if !sel.is_empty() {
                return false;
            }
            let text = c.text();
            let line = text[..sel.start].rfind('\n').map_or(0, |i| i + 1);
            let end = text[sel.start..]
                .find('\n')
                .map_or(text.len(), |i| sel.start + i);
            match list_next(&text[line..sel.start]) {
                Some(Next::Item(mark)) => {
                    c.replace_all(std::slice::from_ref(&sel), &format!("\n{mark}"), cx);
                    true
                }
                Some(Next::End) if end == sel.start => {
                    c.replace_all(&[line..sel.start], "", cx);
                    true
                }
                _ => false,
            }
        })
    }

    fn mark(&mut self, mark: &str, cx: &mut Context<Self>) -> bool {
        self.code.update(cx, |c, cx| {
            let (range, with, after) = toggle_mark(c.text(), c.selection(), mark);
            c.replace_all(&[range], &with, cx);
            c.select(after, cx);
        });
        true
    }

    fn make_heading(&mut self, level: usize, cx: &mut Context<Self>) -> bool {
        self.code.update(cx, |c, cx| {
            let text = c.text();
            let at = c.selection().start;
            let start = text[..at].rfind('\n').map_or(0, |i| i + 1);
            let end = text[at..].find('\n').map_or(text.len(), |i| at + i);
            let with = heading(&text[start..end], level);
            let after = start + with.len();
            c.replace_all(&[start..end], &with, cx);
            c.select(after..after, cx);
        });
        true
    }

    fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.finding = true;
        let chosen = self.code.read(cx).selected_text().to_string();
        if !chosen.is_empty() && !chosen.contains('\n') {
            self.find.update(cx, |f, cx| f.set(&chosen, true, cx));
        } else {
            self.find.update(cx, |f, cx| f.select_everything(cx));
        }
        field::focus(&self.find, window, cx);
        self.refind(cx);
    }

    fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.finding = false;
        self.found.clear();
        self.code.update(cx, |c, cx| c.set_found(vec![], cx));
        Code::focus(&self.code, window, cx);
        cx.notify();
    }

    fn refind(&mut self, cx: &mut Context<Self>) {
        let needle = self.find.read(cx).text.clone();
        self.found = find_all(self.text(cx), &needle);
        let found = self.found.clone();
        self.code.update(cx, |c, cx| c.set_found(found, cx));
        cx.notify();
    }

    /// The next find after the caret (or the one before), selected.
    fn step_find(&mut self, forward: bool, _: &mut Window, cx: &mut Context<Self>) {
        if self.found.is_empty() {
            return;
        }
        let sel = self.code.read(cx).selection();
        let next = if forward {
            self.found
                .iter()
                .find(|f| f.start > sel.start)
                .or(self.found.first())
        } else {
            self.found
                .iter()
                .rev()
                .find(|f| f.start < sel.start)
                .or(self.found.last())
        };
        if let Some(f) = next.cloned() {
            self.code.update(cx, |c, cx| c.select(f, cx));
        }
    }

    /// Writes the page through Citadel, unless someone else changed it
    /// since this began: then it says so, and a second Ctrl+S writes over
    /// theirs knowingly.
    pub fn save(&mut self, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        if self.text(cx) == self.saved {
            self.said = Some(Said::Note("Nothing new to save.".into()));
            cx.notify();
            return;
        }
        let Some(file) = self
            .app
            .upgrade()
            .and_then(|a| a.read(cx).file_of(&self.path))
        else {
            return;
        };
        // A page written today says so, where it keeps its date.
        let today = time::OffsetDateTime::now_local()
            .unwrap_or_else(|_| time::OffsetDateTime::now_utc())
            .date();
        if let Some((at, date)) = updated_today(self.text(cx), today) {
            self.code.update(cx, |c, cx| {
                let sel = c.selection();
                c.replace_all(&[at], &date, cx);
                c.select(sel, cx);
            });
        }
        let text = self.text(cx).to_string();
        self.saving = true;
        self.said = Some(Said::Note("Saving…".into()));
        cx.notify();
        let (url, path, base) = (self.citadel.clone(), self.path.clone(), self.base.clone());
        let message = format!("Edit {} in Ahjo Brain", self.title);
        let written = text.clone();
        let job = cx.background_spawn(async move {
            let now = std::fs::read(&file)
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .unwrap_or_default();
            if now != base && now != written {
                return Err(NotSaved::Changed(now));
            }
            citadel::write(&url, &path, &written, &message).map_err(NotSaved::Refused)
        });
        cx.spawn(async move |me, cx| {
            let result = job.await;
            let _ = me.update(cx, |me, cx| {
                me.saving = false;
                match result {
                    Ok(w) => {
                        me.saved = text.clone();
                        me.base = text;
                        me.said = Some(Said::Note(if w.committed {
                            format!(
                                "Saved to the brain as {}.",
                                &w.revision[..w.revision.len().min(7)]
                            )
                        } else {
                            "The brain already reads this way.".into()
                        }));
                        if let Some(cache) = me.cache.clone() {
                            drop_draft(&cache, &me.path);
                        }
                        cx.emit(EditorEvent::Saved);
                    }
                    Err(NotSaved::Changed(theirs)) => {
                        // Changed meanwhile: the next save writes over it.
                        me.base = theirs;
                        me.said = Some(Said::Refused(
                            "This page changed in the brain since you began. Ctrl+S again \
                             writes yours over theirs, and theirs is lost; Done keeps yours \
                             as a draft."
                                .into(),
                        ));
                    }
                    Err(NotSaved::Refused(why)) => {
                        me.said = Some(Said::Refused(plain_refusal(&why)))
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for Editor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx).clone();
        let unsaved = self.unsaved(cx);
        let status = match (&self.said, unsaved) {
            (Some(Said::Note(n)), _) => n.clone(),
            (_, true) => "Unsaved: Ctrl+S saves it to the brain".into(),
            (_, false) => "As in the brain".into(),
        };
        let bar = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(10.))
            .h(px(e::BAR))
            .px(px(e::PAD))
            .border_b_1()
            .border_color(t.divider)
            .child(kit::label(&t, "Editing"))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(size::TEXT))
                    .text_color(t.text)
                    .child(self.title.clone()),
            )
            .child(
                div()
                    .flex_none()
                    .text_size(px(size::TEXT))
                    .text_color(if unsaved { t.text2 } else { t.dim })
                    .child(status),
            )
            .child(
                kit::button(
                    &t,
                    "save",
                    "Save",
                    if unsaved { Kind::Primary } else { Kind::Plain },
                )
                .tooltip(kit::tip("Save to the brain (Ctrl+S)"))
                .on_click(cx.listener(|me, _, _, cx| me.save(cx))),
            )
            .child(
                kit::button(&t, "done", "Done", Kind::Quiet)
                    .tooltip(kit::tip("Back to reading (Esc)"))
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(EditorEvent::Leave))),
            );
        let refused = match &self.said {
            Some(Said::Refused(why)) => Some(
                div()
                    .flex_none()
                    .px(px(e::PAD))
                    .py(px(10.))
                    .bg(alpha(t.error, 0.08))
                    .border_b_1()
                    .border_color(alpha(t.error, 0.3))
                    .text_size(px(size::TEXT))
                    .text_color(t.error)
                    .child(format!("Not saved. {why}")),
            ),
            _ => None,
        };
        let find = self.finding.then(|| {
            let n = self.found.len();
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(10.))
                .px(px(e::PAD))
                .py(px(6.))
                .border_b_1()
                .border_color(t.divider)
                .child(div().flex_1().child(self.find.clone()))
                .child(
                    div()
                        .flex_none()
                        .text_size(px(size::TEXT))
                        .text_color(t.dim)
                        .child(match n {
                            0 if self.find.read(cx).text.is_empty() => String::new(),
                            0 => "None found".into(),
                            1 => "1 found".into(),
                            n => format!("{n} found"),
                        }),
                )
        });
        let hint = div()
            .flex_none()
            .px(px(e::PAD))
            .py(px(8.))
            .border_t_1()
            .border_color(t.divider)
            .text_size(px(size::TEXT - 1.))
            .text_color(t.dim)
            .truncate()
            .child(
                "[[ links a page · Ctrl+B bold · Ctrl+I emphasis · Ctrl+1–3 heading · \
                 Ctrl+F find · Ctrl+Z undo",
            );
        let caret = self
            .picking
            .is_some()
            .then(|| self.code.update(cx, |c, cx| c.caret_bounds(window, cx)))
            .flatten();
        let picks = self.picking.as_ref().zip(caret).map(|(p, caret)| {
            let mut list = div()
                .w(px(e::PICK_W))
                .py(px(4.))
                .rounded(px(size::RADIUS))
                .bg(t.raised)
                .border_1()
                .border_color(t.divider)
                .shadow(vec![gpui::BoxShadow {
                    color: t.shadow(),
                    offset: gpui::point(px(0.), px(6.)),
                    blur_radius: px(18.),
                    spread_radius: px(0.),
                    inset: false,
                }])
                .font_family(FAMILY)
                .text_size(px(size::TEXT));
            for (i, &n) in p.picks.iter().enumerate() {
                let name = &self.names[n];
                list = list.child(
                    div()
                        .id(("pick", i))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .px(px(10.))
                        .py(px(5.))
                        .cursor_pointer()
                        .when(i == p.at, |d| d.bg(alpha(t.accent, 0.16)))
                        .hover(|s| s.bg(alpha(t.text, 0.06)))
                        .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
                        .on_click(cx.listener(move |me, _, _, cx| me.take_pick(i, cx)))
                        .child(div().flex_none().size(px(7.)).rounded(px(7.)).bg(name.hue))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_color(t.text)
                                .child(name.title.clone()),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_color(t.dim)
                                .child(name.subject.clone()),
                        ),
                );
            }
            deferred(
                anchored()
                    .position(caret.bottom_left() + gpui::point(px(0.), px(4.)))
                    .snap_to_window_with_margin(px(8.))
                    .child(list),
            )
            .with_priority(1)
        });
        div()
            .id("editor")
            .size_full()
            .flex()
            .flex_col()
            .bg(t.bg)
            .font_family(FAMILY)
            .capture_key_down(cx.listener(Self::key))
            .child(bar)
            .children(refused)
            .children(find)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .max_w(px(e::MEASURE))
                    .pl(px(e::PAD / 2.))
                    .pt(px(e::PAD / 2.))
                    .child(self.code.clone()),
            )
            .child(hint)
            .children(picks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(slug: &str, title: &str) -> Name {
        Name {
            slug: slug.into(),
            title: title.into(),
            aliases: vec![],
            hue: gpui::black(),
            subject: String::new(),
        }
    }

    #[test]
    fn a_link_is_open_until_it_closes_on_its_line() {
        let text = "See [[sky ma";
        assert_eq!(link_open(text, text.len()), Some(6));
        assert_eq!(link_open("See [[sky]] and", 15), None);
        assert_eq!(link_open("[[a|label", 9), None);
        assert_eq!(link_open("[[a\nb", 5), None);
        assert_eq!(link_open("plain", 5), None);
    }

    #[test]
    fn pages_are_offered_by_how_well_their_names_begin() {
        let names = vec![
            name("garden-notes", "Notes from the garden"),
            name("sky-map", "The map of the sky"),
            name("skylark", "Skylark"),
            name("mapping", "Mapping rivers"),
        ];
        // The name begins with it, then every word begins a word.
        assert_eq!(complete("sky", &names), [2, 1]);
        assert_eq!(complete("map sk", &names), [1]);
        assert_eq!(complete("map", &names), [3, 1]);
        assert_eq!(complete("arden", &names), [0]);
        assert_eq!(complete("", &names), [0, 1, 2, 3]);
        assert!(complete("nothing", &names).is_empty());
    }

    #[test]
    fn enter_goes_on_with_a_list() {
        assert_eq!(list_next("- milk"), Some(Next::Item("- ".into())));
        assert_eq!(list_next("  * nested"), Some(Next::Item("  * ".into())));
        assert_eq!(list_next("9. ninth"), Some(Next::Item("10. ".into())));
        assert_eq!(list_next("- [x] done"), Some(Next::Item("- [ ] ".into())));
        assert_eq!(list_next("> said"), Some(Next::Item("> ".into())));
        assert_eq!(list_next("- "), Some(Next::End));
        assert_eq!(list_next("- [ ] "), Some(Next::End));
        assert_eq!(list_next("Plain words"), None);
        assert_eq!(list_next("2024 was"), None);
    }

    #[test]
    fn marks_go_on_and_come_off() {
        let text = "a word here";
        let (r, with, sel) = toggle_mark(text, 2..6, "**");
        assert_eq!((r, with.as_str(), sel), (2..6, "**word**", 4..8));
        let text = "a **word** here";
        let (r, with, sel) = toggle_mark(text, 4..8, "**");
        assert_eq!((r, with.as_str(), sel), (2..10, "word", 2..6));
        // Nothing chosen: the marks, the caret between them.
        let (_, with, sel) = toggle_mark("ab", 1..1, "*");
        assert_eq!((with.as_str(), sel), ("**", 2..2));
    }

    #[test]
    fn a_line_becomes_a_heading_and_back() {
        assert_eq!(heading("Words", 2), "## Words");
        assert_eq!(heading("# Words", 2), "## Words");
        assert_eq!(heading("## Words", 2), "Words");
        assert_eq!(heading("#tag", 1), "# #tag");
    }

    #[test]
    fn a_page_saved_is_updated_today_where_it_keeps_a_date() {
        let today = time::Date::from_calendar_date(2026, time::Month::November, 19).unwrap();
        let text = "---\nkind: note\nupdated: 2026-11-01\n---\n# A\nupdated: never\n";
        let (at, date) = updated_today(text, today).unwrap();
        assert_eq!((&text[at], date.as_str()), ("2026-11-01", "2026-11-19"));
        assert_eq!(
            updated_today("---\nupdated: 2026-11-19\n---\n", today),
            None
        );
        // No date kept, none added: the brain does not ask for one.
        assert_eq!(
            updated_today("---\nkind: note\n---\nupdated: x\n", today),
            None
        );
        assert_eq!(updated_today("# No front matter\n", today), None);
    }

    #[test]
    fn a_refusal_says_what_the_check_found() {
        let said = "brain: brain check refuses this commit; fix and commit again (nothing was \
                    committed):\nbroken link [[nope]] in a.md";
        assert_eq!(
            plain_refusal(said),
            "The brain's check found: broken link [[nope]] in a.md"
        );
        assert_eq!(
            plain_refusal(" x has an uncommitted change "),
            "x has an uncommitted change"
        );
    }

    #[test]
    fn find_ignores_case_and_keeps_to_letters() {
        assert_eq!(find_all("Sky and sky", "sky"), [0..3, 8..11]);
        assert_eq!(find_all("äsky", "sky"), [2..5]);
        assert!(find_all("x", "").is_empty());
    }

    #[test]
    fn a_draft_is_kept_and_read_back_by_its_page() {
        let dir = std::env::temp_dir().join(format!("brain-drafts-{}", std::process::id()));
        let d = Draft {
            path: "knowledge/a/b.md".into(),
            base: "old".into(),
            text: "new".into(),
        };
        keep_draft(&dir, &d).unwrap();
        assert_eq!(read_draft(&dir, "knowledge/a/b.md"), Some(d));
        assert_eq!(read_draft(&dir, "knowledge/a/c.md"), None);
        drop_draft(&dir, "knowledge/a/b.md");
        assert_eq!(read_draft(&dir, "knowledge/a/b.md"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
