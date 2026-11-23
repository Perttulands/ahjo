//! The reader: one page, set for reading.
//!
//! The kit's Markdown parser reads the blocks; this sets them in reading
//! type (a wider leading, a measure that stops long lines, headings that
//! step down clearly) rather than the chrome's sizes. A link is coloured by
//! the subject it leads into, the same hue as that subject's stars, so the
//! prose says where in the sky each link goes, and pointing at one lights
//! its star.

use crate::app::{BrainApp, Data};
use crate::corpus;
use crate::tokens::reader as r;
use ahjo_app_kit::Error;
use ahjo_app_kit::theme::{FAMILY, Theme, alpha, theme};
use ahjo_app_kit::widgets::markdown::{Block, Span, inline, merge, parse};
use ahjo_app_kit::widgets::{kit, pane};
use gpui::{
    AnyElement, Context, Div, ElementId, FontStyle, FontWeight, HighlightStyle, Hsla,
    InteractiveText, IntoElement, MouseButton, ParentElement, Render, ScrollHandle, SharedString,
    StyledText, WeakEntity, Window, div, prelude::*, px,
};
use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

pub struct Reader {
    app: WeakEntity<BrainApp>,
    data: Option<Rc<Data>>,
    page: Option<usize>,
    body: Option<Result<Vec<Block>, Error>>,
    scroll: ScrollHandle,
    asked: u64,
    /// The body's size as zoomed (`ahjo_app_kit::zoom`); the rest of the
    /// page and its measure follow it.
    text: f32,
    /// The link under the pointer (its text's id and its number there),
    /// and the one a press went down on. A link is followed when the press
    /// comes up on the link it went down on.
    link_over: Rc<RefCell<Option<(ElementId, usize)>>>,
    link_down: Rc<RefCell<Option<(ElementId, usize)>>>,
    /// The page as it is being written (its title and summary), shown
    /// live instead of as the brain last read it.
    live: Option<(String, String)>,
}

impl Reader {
    pub fn new(app: WeakEntity<BrainApp>) -> Reader {
        Reader {
            app,
            data: None,
            page: None,
            body: None,
            scroll: ScrollHandle::new(),
            asked: 0,
            text: r::BODY,
            link_over: Rc::default(),
            link_down: Rc::default(),
            live: None,
        }
    }

    /// Reads the page at `text` pixels for the body.
    pub fn set_text(&mut self, text: f32, cx: &mut Context<Self>) {
        if self.text != text {
            self.text = text;
            cx.notify();
        }
    }

    /// How much the page is zoomed from its own sizes.
    fn k(&self) -> f32 {
        self.text / r::BODY
    }

    /// Moves the page by `by` pixels (down above zero), as far as it goes.
    pub fn scroll_by(&mut self, by: f32, cx: &mut Context<Self>) {
        let most = f32::from(self.scroll.max_offset().y);
        let at = f32::from(self.scroll.offset().y);
        let to = (at - by).clamp(-most, 0.);
        if to != at {
            self.scroll.set_offset(gpui::point(px(0.), px(to)));
            cx.notify();
        }
    }

    /// Moves the page by `pages` of what the sheet shows, keeping a line
    /// of the last in view.
    pub fn scroll_pages(&mut self, pages: f32, cx: &mut Context<Self>) {
        let seen = f32::from(self.scroll.bounds().size.height);
        let step = (seen - r::BODY_LINE * self.k() * 2.).max(r::BODY_LINE);
        self.scroll_by(step * pages, cx);
    }

    /// What an arrow key moves the page: two lines of the body.
    pub fn arrow_step(&self) -> f32 {
        r::BODY_LINE * self.k() * 2.
    }

    /// To the page's top, or its end.
    pub fn scroll_end(&mut self, end: bool, cx: &mut Context<Self>) {
        let most = f32::from(self.scroll.max_offset().y);
        self.scroll
            .set_offset(gpui::point(px(0.), px(if end { -most } else { 0. })));
        cx.notify();
    }

    /// Shows `page`, reading its file off the UI thread.
    pub fn show(&mut self, data: Rc<Data>, page: usize, cx: &mut Context<Self>) {
        if self.page == Some(page) && self.data.as_ref().is_some_and(|d| Rc::ptr_eq(d, &data)) {
            return;
        }
        self.asked += 1;
        let asked = self.asked;
        let file = data.root.join(&data.corpus.pages[page].path);
        let title = data.corpus.pages[page].title.clone();
        self.data = Some(data.clone());
        self.page = Some(page);
        self.body = None;
        self.live = None;
        self.scroll.set_offset(gpui::point(px(0.), px(0.)));
        cx.notify();
        let read = cx.background_spawn(async move {
            std::fs::read(&file)
                .map(|bytes| {
                    let text = String::from_utf8_lossy(&bytes);
                    without_title(corpus::body(&text), &title)
                })
                .map_err(|e| Error::new("page_unreadable", format!("{}: {e}", file.display())))
        });
        cx.spawn(async move |me, cx| {
            // The links take their pages' titles here, where the brain is;
            // the reading and the parsing are off the UI thread.
            let body = match read.await {
                Ok(body) => {
                    let linked = wiki_links_named(&body, |target| {
                        data.corpus
                            .resolve(target)
                            .map(|p| data.corpus.pages[p].title.clone())
                    });
                    Ok(cx.background_spawn(async move { parse(&linked) }).await)
                }
                Err(e) => Err(e),
            };
            let _ = me.update(cx, |me, cx| {
                if me.asked == asked {
                    me.body = Some(body);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Reads the page shown from its file again, as the brain holds it.
    pub fn reread(&mut self, cx: &mut Context<Self>) {
        if let (Some(data), Some(page)) = (self.data.clone(), self.page) {
            let at = self.scroll.offset();
            self.page = None;
            self.show(data, page, cx);
            self.scroll.set_offset(at);
        }
    }

    /// Shows the page read as `text` says it, as it is being written: its
    /// title, summary and body, where the page was scrolled to.
    pub fn preview(&mut self, text: String, cx: &mut Context<Self>) {
        let (Some(data), Some(page)) = (self.data.clone(), self.page) else {
            return;
        };
        self.asked += 1;
        let asked = self.asked;
        let path = data.corpus.pages[page].path.clone();
        cx.spawn(async move |me, cx| {
            let (entry, body) = cx
                .background_spawn(async move {
                    let entry = corpus::parse(&path, &text);
                    let body = without_title(corpus::body(&text), &entry.title);
                    (entry, body)
                })
                .await;
            let linked = wiki_links_named(&body, |target| {
                data.corpus
                    .resolve(target)
                    .map(|p| data.corpus.pages[p].title.clone())
            });
            let blocks = cx.background_spawn(async move { parse(&linked) }).await;
            let _ = me.update(cx, |me, cx| {
                if me.asked == asked {
                    me.live = Some((entry.title, entry.summary));
                    me.body = Some(Ok(blocks));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// The page this link leads to, or the web address it names.
    fn target(&self, written: &str) -> Target {
        let Some(data) = &self.data else {
            return Target::Nowhere;
        };
        if written.contains("://") {
            return Target::Web(written.to_string());
        }
        let path = written.split('#').next().unwrap_or(written);
        if path.ends_with(".md") {
            let here = self
                .page
                .map(|p| data.corpus.pages[p].path.as_str())
                .unwrap_or("");
            let mut parts: Vec<&str> = here.split('/').collect();
            parts.pop();
            for seg in path.split('/') {
                match seg {
                    "" | "." => {}
                    ".." => {
                        parts.pop();
                    }
                    s => parts.push(s),
                }
            }
            let joined = parts.join("/");
            if let Some(p) = data.corpus.by_path(&joined) {
                return Target::Page(p);
            }
            return data
                .corpus
                .resolve(&corpus::slug_of_path(&joined))
                .map_or(Target::Nowhere, Target::Page);
        }
        data.corpus
            .resolve(path)
            .map_or(Target::Nowhere, Target::Page)
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Target {
    Page(usize),
    Web(String),
    Nowhere,
}

/// Turns `[[target#heading|label]]` into a Markdown link, outside code.
/// A link written without a label reads as what `name` calls
/// its target (the page's title), not as the slug it was written with.
pub fn wiki_links_named(source: &str, name: impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(source.len());
    let mut fence: Option<&str> = None;
    for line in source.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if let Some(open) = fence {
            if trimmed.starts_with(open) {
                fence = None;
            }
            out.push_str(line);
            continue;
        }
        if let Some(open) = ["```", "~~~"].into_iter().find(|o| trimmed.starts_with(o)) {
            fence = Some(open);
            out.push_str(line);
            continue;
        }
        let mut rest = line;
        while !rest.is_empty() {
            if rest.starts_with('`')
                && let Some(end) = rest[1..].find('`')
            {
                out.push_str(&rest[..end + 2]);
                rest = &rest[end + 2..];
                continue;
            }
            if rest.starts_with("[[")
                && let Some(end) = rest.find("]]")
            {
                let inner = &rest[2..end];
                let (target, label) = inner.split_once('|').unwrap_or((inner, inner));
                if !target.trim().is_empty() && !inner.contains(['[', ']']) {
                    let label = label.trim();
                    let label = label.split('#').next().unwrap_or(label);
                    let named = (!inner.contains('|'))
                        .then(|| name(target.trim()))
                        .flatten()
                        .filter(|n| !n.contains(['[', ']']));
                    out.push('[');
                    match &named {
                        Some(n) => out.push_str(n),
                        None => out.push_str(label.rsplit('/').next().unwrap_or(label)),
                    }
                    out.push_str("](");
                    out.push_str(&target.trim().replace(' ', "%20"));
                    out.push(')');
                    rest = &rest[end + 2..];
                    continue;
                }
            }
            let c = rest.chars().next().unwrap_or(' ');
            out.push(c);
            rest = &rest[c.len_utf8()..];
        }
    }
    out
}

/// The body without its first heading when that repeats the title.
pub fn without_title(body: &str, title: &str) -> String {
    let lines: Vec<&str> = body.split('\n').collect();
    if let Some(at) = lines.iter().position(|l| !l.trim().is_empty())
        && let Some(h) = lines[at].strip_prefix("# ")
        && h.trim() == title
    {
        return lines[at + 1..].join("\n");
    }
    body.to_string()
}

/// `5 min ago`, `3 h ago`, `yesterday`: how long since a file changed, in
/// nanoseconds since 1970.
pub fn since(then: u64, now: u64) -> String {
    let minutes = now.saturating_sub(then) / 60_000_000_000;
    match minutes {
        0 => "just now".into(),
        m if m < 60 => format!("{m} min ago"),
        m if m < 24 * 60 => format!("{} h ago", m / 60),
        m if m < 48 * 60 => "yesterday".into(),
        m => format!("{} days ago", m / (24 * 60)),
    }
}

/// `3 days ago`, from Julian days.
pub fn age(updated: i32, today: i32) -> String {
    match today - updated {
        d if d <= 0 => "today".into(),
        1 => "yesterday".into(),
        d if d < 30 => format!("{d} days ago"),
        d if d < 365 => match d / 30 {
            1 => "a month ago".into(),
            m => format!("{m} months ago"),
        },
        d => match d / 365 {
            1 => "a year ago".into(),
            y => format!("{y} years ago"),
        },
    }
}

impl Reader {
    fn hue_of(&self, page: usize, t: &Theme) -> Hsla {
        match &self.data {
            Some(d) => crate::tokens::words(t, d.hues[d.corpus.pages[page].subject]),
            None => t.text2,
        }
    }

    /// Inline text with its marks, links coloured by where they lead.
    fn styled(&self, id: ElementId, src: &str, base: HighlightStyle, t: &Theme) -> AnyElement {
        let runs = inline(src, false);
        let mut highlights: Vec<(Range<usize>, HighlightStyle)> = vec![];
        let mut ranges = vec![];
        let mut targets = vec![];
        let mut mono = vec![];
        for (range, span) in &runs.spans {
            let style = match span {
                Span::Code => {
                    mono.push((range.clone(), SharedString::from(FAMILY)));
                    HighlightStyle {
                        background_color: Some(alpha(t.text, 0.07)),
                        color: Some(t.text),
                        ..Default::default()
                    }
                }
                Span::Bold => HighlightStyle {
                    font_weight: Some(FontWeight::BOLD),
                    color: Some(t.text),
                    ..Default::default()
                },
                Span::Em => HighlightStyle {
                    font_style: Some(FontStyle::Italic),
                    ..Default::default()
                },
                Span::Link(i) => {
                    let target = self.target(&runs.links[*i]);
                    let color = match &target {
                        Target::Page(p) => self.hue_of(*p, t),
                        Target::Web(_) => t.text2,
                        Target::Nowhere => t.dim,
                    };
                    ranges.push(range.clone());
                    targets.push(target);
                    HighlightStyle {
                        color: Some(color),
                        underline: Some(gpui::UnderlineStyle {
                            color: Some(alpha(color, 0.45)),
                            thickness: px(1.),
                            wavy: false,
                        }),
                        ..Default::default()
                    }
                }
            };
            highlights.push((range.clone(), style));
        }
        let highlights = merge(base, runs.text.len(), highlights);
        let text = StyledText::new(SharedString::from(runs.text))
            .with_highlights(highlights)
            .with_font_family_overrides(mono);
        if ranges.is_empty() {
            return text.into_any_element();
        }
        // A link opens on its own press and release, not through
        // `InteractiveText::on_click`: that one listens for the release
        // only from the frame after the press, so a quick click whose
        // release came before the next frame was lost; with a page's links
        // lighting the sky as the pointer reaches them, that was most
        // clicks on a link's first line.
        let targets = Rc::new(targets);
        let (over, down) = (self.link_over.clone(), self.link_down.clone());
        let app = self.app.clone();
        let hover_id = id.clone();
        let on_hover = targets.clone();
        let text = InteractiveText::new(id.clone(), text).on_hover(move |index, _, _, cx| {
            let link = index.and_then(|ix| ranges.iter().position(|r| r.contains(&ix)));
            {
                let mut over = over.borrow_mut();
                match link {
                    Some(i) => *over = Some((hover_id.clone(), i)),
                    None if over.as_ref().is_some_and(|(at, _)| *at == hover_id) => *over = None,
                    None => {}
                }
            }
            let page = link.and_then(|i| match on_hover[i] {
                Target::Page(p) => Some(p),
                _ => None,
            });
            let _ = app.update(cx, |app, cx| app.point_at_link(page, cx));
        });
        let (over_down, down_set) = (self.link_over.clone(), down.clone());
        let over_up = self.link_over.clone();
        let app = self.app.clone();
        div()
            .on_mouse_down(MouseButton::Left, move |_, _, _| {
                *down_set.borrow_mut() = over_down.borrow().clone();
            })
            .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                let pressed = down.borrow_mut().take();
                let Some((at, i)) = over_up.borrow().clone() else {
                    return;
                };
                if at != id || pressed != Some((at, i)) {
                    return;
                }
                match &targets[i] {
                    Target::Page(p) => {
                        let p = *p;
                        let _ = app.update(cx, |app, cx| app.open(p, cx));
                    }
                    Target::Web(url) => cx.open_url(url),
                    Target::Nowhere => {}
                }
            })
            .child(text)
            .into_any_element()
    }

    fn block(&self, n: usize, b: &Block, t: &Theme) -> AnyElement {
        let id = |k: usize| ElementId::Name(format!("b{n}-{k}").into());
        let plain = HighlightStyle::default();
        let k = self.k();
        match b {
            // The page's own front matter is cut before parsing and shown as
            // the line over its title; a block of it later is not prose.
            Block::FrontMatter(_) => div().into_any_element(),
            Block::Heading(level, h) => {
                let (size, color) = match level {
                    1 | 2 => (r::H2 * k, t.text),
                    _ => (r::H3 * k, t.text),
                };
                div()
                    .pt(px((r::HEAD_GAP - r::PARA_GAP) * k))
                    .text_size(px(size))
                    .line_height(px(size * 1.4))
                    .font_family(t.ty.display)
                    .font_weight(t.ty.display_weight)
                    .text_color(color)
                    .child(self.styled(id(0), h, plain, t))
                    .into_any_element()
            }
            Block::Paragraph(p) => div()
                .child(self.styled(id(0), p, plain, t))
                .into_any_element(),
            Block::Quote(q) => div()
                .pl(px(r::INDENT * k - r::QUOTE_BAR))
                .border_l(px(r::QUOTE_BAR))
                .border_color(alpha(t.text2, 0.35))
                .text_color(t.text2)
                .child(self.styled(id(0), q, plain, t))
                .into_any_element(),
            Block::Code(c, _) => div()
                .p(px(14.))
                .bg(alpha(t.bg, 0.6))
                .rounded(px(r::RADIUS))
                .font_family(FAMILY)
                .text_size(px(r::CODE * k))
                .line_height(px(r::CODE * k * 1.55))
                .text_color(t.text2)
                .overflow_x_hidden()
                .child(c.replace('\t', "    "))
                .into_any_element(),
            Block::Rule => div()
                .my(px(r::PARA_GAP * k))
                .h(px(r::RULE))
                .bg(t.divider)
                .into_any_element(),
            Block::List(items) => {
                let mut list = div().flex().flex_col().gap(px(r::LIST_GAP * k));
                for (j, it) in items.iter().enumerate() {
                    let marker = match (it.checked, it.number) {
                        (Some(true), _) => "☑".to_string(),
                        (Some(false), _) => "☐".to_string(),
                        (None, Some(num)) => format!("{num}."),
                        (None, None) => "–".to_string(),
                    };
                    let base = if it.checked == Some(true) {
                        HighlightStyle {
                            color: Some(t.dim),
                            ..Default::default()
                        }
                    } else {
                        plain
                    };
                    list = list.child(
                        div()
                            .flex()
                            .pl(px(it.depth as f32 * r::INDENT * k))
                            .child(
                                div()
                                    .flex_none()
                                    .w(px((r::INDENT + 4.) * k))
                                    .text_color(t.dim)
                                    .child(marker),
                            )
                            .child(div().flex_1().min_w_0().child(self.styled(
                                id(j + 1),
                                &it.text,
                                base,
                                t,
                            ))),
                    );
                }
                list.into_any_element()
            }
            Block::Table(rows) => {
                let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
                let mut table = div()
                    .flex()
                    .flex_col()
                    .text_size(px(r::CODE * k))
                    .line_height(px(r::CODE * k * 1.5))
                    .border_1()
                    .border_color(t.divider)
                    .rounded(px(r::RADIUS));
                for (ri, row) in rows.iter().enumerate() {
                    let mut line = div()
                        .flex()
                        .when(ri > 0, |d| {
                            d.border_t_1().border_color(alpha(t.divider, 0.6))
                        })
                        .when(ri == 0, |d| {
                            d.font_weight(FontWeight::BOLD).text_color(t.text)
                        });
                    for c in 0..cols {
                        let cell = row.get(c).cloned().unwrap_or_default();
                        line = line.child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .px(px(10.))
                                .py(px(6.))
                                .child(self.styled(id(1 + ri * cols + c), &cell, plain, t)),
                        );
                    }
                    table = table.child(line);
                }
                table.into_any_element()
            }
        }
    }

    /// A list of pages under the prose: where this page leads, or what
    /// leads here, each in its subject's colour.
    fn pages(&self, label: &str, list: &[usize], t: &Theme, cx: &mut Context<Self>) -> Div {
        let Some(data) = &self.data else {
            return div();
        };
        let k = self.k();
        let mut col = div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .font_family(FAMILY)
            .text_size(px(r::LIST * k))
            .child(kit::label(t, format!("{label} · {}", list.len())).pb(px(6.)));
        for &p in list.iter().take(r::NEIGHBOURS) {
            let page = &data.corpus.pages[p];
            let hue = data.hues[page.subject];
            let app = self.app.clone();
            let app_hover = self.app.clone();
            col = col.child(
                div()
                    .id(ElementId::Name(format!("{label}-{p}").into()))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .py(px(4.))
                    .px(px(8.))
                    .mx(px(-8.))
                    .rounded(px(r::RADIUS))
                    .cursor_pointer()
                    .hover(|s| s.bg(alpha(hue, 0.08)))
                    .on_click(move |_, _, cx| {
                        let _ = app.update(cx, |app, cx| app.open(p, cx));
                    })
                    .on_hover(move |on, _, cx| {
                        let page = on.then_some(p);
                        let _ = app_hover.update(cx, |app, cx| app.point_at_link(page, cx));
                    })
                    .child(
                        div()
                            .flex_none()
                            .size(px(r::DOT))
                            .rounded(px(r::DOT))
                            .bg(hue),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(t.text)
                            .child(page.title.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(r::META * k))
                            .text_color(alpha(crate::tokens::words(t, hue), 0.8))
                            .child(data.corpus.subjects[page.subject].name.clone()),
                    ),
            );
        }
        if list.len() > r::NEIGHBOURS {
            col = col.child(
                div()
                    .pt(px(4.))
                    .text_size(px(r::META * k))
                    .text_color(t.dim)
                    .child(format!(
                        "and {} more in the sky",
                        list.len() - r::NEIGHBOURS
                    )),
            );
        }
        let _ = cx;
        col
    }
}

impl Render for Reader {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx).clone();
        let sheet = pane::scroll("reader")
            .size_full()
            .track_scroll(&self.scroll);
        let (Some(data), Some(p)) = (self.data.clone(), self.page) else {
            return sheet;
        };
        let page = &data.corpus.pages[p];
        let hue = data.hues[page.subject];
        let k = self.k();
        let mut meta = vec![data.corpus.subjects[page.subject].name.clone()];
        meta.push(page.kind.clone());
        if let Some(u) = page.updated {
            meta.push(format!("updated {}", age(u, data.today)));
        }
        let minutes = (page.words as f32 / 220.).ceil().max(1.) as u32;
        meta.push(format!("{minutes} min read"));
        let (title, summary) = self
            .live
            .clone()
            .unwrap_or_else(|| (page.title.clone(), page.summary.clone()));
        let head = div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.))
                    .font_family(FAMILY)
                    .text_size(px(r::META * k))
                    .text_color(t.dim)
                    .child(div().size(px(r::DOT)).rounded(px(r::DOT)).bg(hue))
                    .child(
                        div()
                            .text_color(crate::tokens::words(&t, hue))
                            .child(meta[0].clone()),
                    )
                    .children(meta[1..].iter().map(|m| div().child(format!("· {m}")))),
            )
            .child(
                div()
                    .text_size(px(r::TITLE * k))
                    .line_height(px(r::TITLE_LINE * k))
                    .font_family(t.ty.display)
                    .font_weight(t.ty.display_weight)
                    .text_color(t.text)
                    .child(title),
            )
            .when(!summary.is_empty(), |d| {
                d.child(
                    div()
                        .text_size(px(r::LEDE * k))
                        .line_height(px(r::LEDE_LINE * k))
                        .text_color(t.text2)
                        .child(summary),
                )
            });
        let body: AnyElement = match &self.body {
            None => div().text_color(t.dim).child("…").into_any_element(),
            Some(Err(e)) => div()
                .text_color(t.error)
                .child(format!("This page did not read: {}", e.message))
                .into_any_element(),
            Some(Ok(blocks)) if blocks.is_empty() => div()
                .text_color(t.dim)
                .child("This page has no words yet.")
                .into_any_element(),
            Some(Ok(blocks)) => div()
                .flex()
                .flex_col()
                .gap(px(r::PARA_GAP * k))
                .text_size(px(r::BODY * k))
                .line_height(px(r::BODY_LINE * k))
                .text_color(alpha(t.text, 0.92))
                .children(blocks.iter().enumerate().map(|(n, b)| self.block(n, b, &t)))
                .into_any_element(),
        };
        let out = data.corpus.out[p].clone();
        let inc: Vec<usize> = data.corpus.inc[p]
            .iter()
            .copied()
            .filter(|i| !out.contains(i))
            .collect();
        let column = div()
            .w_full()
            .max_w(px(r::MEASURE * k))
            .flex()
            .flex_col()
            .gap(px(28.))
            .child(head)
            .child(div().h(px(1.)).bg(alpha(hue, 0.25)))
            .child(body)
            .when(!out.is_empty(), |d| {
                d.child(self.pages("Leads to", &out, &t, cx))
            })
            .when(!inc.is_empty(), |d| {
                d.child(self.pages("Linked from", &inc, &t, cx))
            })
            .child(div().h(px(64.)));
        sheet
            .px(px(r::PAD_X))
            .pt(px(r::PAD_TOP))
            .font_family(t.ty.reading)
            .child(column)
    }
}

/// How wide the reader's sheet is in a window this wide, its text zoomed
/// `k` times: a page read bigger takes more of the window, so its measure
/// stays.
pub fn sheet_width(window_width: f32, k: f32) -> f32 {
    let most = (r::MAX * k).max(r::MIN);
    let sheet = (window_width * r::SHARE * k).clamp(r::MIN, most);
    // A sliver of sky beside it is no use: then the sheet is the window.
    if window_width - sheet < r::SKY_LEAST {
        window_width
    } else {
        sheet
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wiki_links_become_links_with_their_labels_outside_code() {
        assert_eq!(
            wiki_links_named(
                "[[first#part|read]] [[folder/b]] `[[literal]]`\n```\n[[code]]\n```\n",
                |_| None
            ),
            "[read](first#part) [b](folder/b) `[[literal]]`\n```\n[[code]]\n```\n"
        );
        // Written bare, a link reads as its page's title; a label stays.
        let title = |t: &str| (t == "the-nectar-gap").then(|| "The nectar gap".to_string());
        assert_eq!(
            wiki_links_named("[[the-nectar-gap]] [[the-nectar-gap|here]] [[gone]]", title),
            "[The nectar gap](the-nectar-gap) [here](the-nectar-gap) [gone](gone)"
        );
    }

    #[test]
    fn the_heading_that_repeats_the_title_goes() {
        assert_eq!(without_title("\n# Pace\nbody", "Pace"), "body");
        assert_eq!(without_title("# Other\nbody", "Pace"), "# Other\nbody");
    }

    #[test]
    fn a_change_is_dated_as_a_person_says_it() {
        let m = 60_000_000_000;
        assert_eq!(since(0, 30_000_000_000), "just now");
        assert_eq!(since(0, 5 * m), "5 min ago");
        assert_eq!(since(0, 185 * m), "3 h ago");
        assert_eq!(since(0, 30 * 60 * m), "yesterday");
        assert_eq!(since(10 * m, 3 * 24 * 60 * m), "2 days ago");
    }

    #[test]
    fn the_sheet_fits_any_window() {
        for w in [0., 300., 600., 1456., 4000.] {
            for k in [0.6, 1., 2.] {
                let s = sheet_width(w, k);
                assert!(s <= w, "{w} gives {s}");
                assert!(s == w || w - s >= r::SKY_LEAST, "{w} leaves a sliver");
            }
        }
        assert_eq!(sheet_width(640., 1.), 640.);
        assert_eq!(sheet_width(1000., 1.), r::MIN);
        assert_eq!(sheet_width(4000., 1.), r::MAX);
        // Read bigger, the sheet widens with its measure.
        assert!(sheet_width(1920., 1.3) > sheet_width(1920., 1.));
    }

    #[test]
    fn ages_read_as_a_person_says_them() {
        assert_eq!(age(100, 100), "today");
        assert_eq!(age(99, 100), "yesterday");
        assert_eq!(age(90, 100), "10 days ago");
        assert_eq!(age(0, 75), "2 months ago");
        assert_eq!(age(0, 800), "2 years ago");
    }
}
