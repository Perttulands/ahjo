//! The surface: what the agent published, as big as the room allows, and
//! where the operator marks it. Pictures and clips sit on the dark ground
//! they read true on, one or two side by side (C) at the same zoom and
//! place; the wheel zooms about the pointer, a drag moves, a double click
//! goes between fit and 1:1, and a clip plays, scrubs and steps by the
//! frame. With a drawing tool a press marks instead: a pin, a box, an arrow
//! or a line, on a paused clip's frame. A page renders here (Windows) and a
//! click in Comment mode marks its element; the document reads here and
//! selecting words marks the passage.

use crate::app::{DeskApp, Drawing, Item, Pic, fit_scale};
use crate::marks::{self, Tool};
use crate::model::{Ask, Mark, MediaKind};
use crate::tokens::{self, CAP_TEXT, GAP, GAP_SMALL, META};
use crate::views::button;
use ahjo_app_kit::theme::{FAMILY, Theme, alpha, rgb, theme};
use ahjo_app_kit::widgets::kit::{self, Kind};
use ahjo_app_kit::widgets::{markdown, select};
use gpui::{
    AnyElement, App, BorderStyle, Bounds, ClickEvent, ContentMask, Context, Corners, Font,
    FontWeight, Hsla, MouseButton, MouseDownEvent, MouseMoveEvent, ObjectFit, PathBuilder, Pixels,
    Point, RenderImage, ScrollDelta, ScrollEvent, SharedString, StyledImage, TextAlign, TextRun,
    Window, canvas, div, fill, font, img, point, prelude::*, px, quad, relative, transparent_black,
};
use std::sync::Arc;

/// What one panel of the surface draws.
enum Shown {
    Picture(Arc<RenderImage>, (f32, f32)),
    Waiting(String),
    Failed(String),
    /// What the app does not show itself: the browser does.
    Elsewhere(String),
}

/// The marks one panel draws: each with its number, whether it is the
/// selected one, and the one being drawn.
#[derive(Clone, Default)]
struct Ink {
    marks: Vec<(usize, Mark, bool)>,
    drawing: Option<Drawing>,
}

impl DeskApp {
    fn shown(
        &mut self,
        ask: &Ask,
        desk: usize,
        item: &Item,
        playing: bool,
        cx: &mut Context<Self>,
    ) -> Shown {
        match item.media.kind() {
            MediaKind::Image => match self.picture(desk, ask, &item.media, cx) {
                Some(Pic::Ready { image, w, h, .. }) => Shown::Picture(image.clone(), (*w, *h)),
                Some(Pic::Failed(e)) => Shown::Failed(e.clone()),
                _ => Shown::Waiting("Fetching the picture".into()),
            },
            MediaKind::Video if playing => {
                let key = self.media_key(desk, ask, &item.media);
                if let Some(why) = self.clip_failed.get(&key) {
                    return Shown::Failed(format!("The clip does not play here: {why}"));
                }
                match self.player.as_mut() {
                    Some((k, p)) if *k == key => match (p.image(), &p.failed) {
                        (_, Some(e)) => Shown::Failed(e.clone()),
                        (Some(image), _) => {
                            Shown::Picture(image, (p.clip.width as f32, p.clip.height as f32))
                        }
                        (None, _) => Shown::Waiting("Decoding".into()),
                    },
                    _ => Shown::Waiting("Fetching the clip".into()),
                }
            }
            MediaKind::Video => Shown::Elsewhere("F shows this clip here".into()),
            MediaKind::OtherVideo => Shown::Elsewhere(format!(
                "{}: this kind of clip plays in the browser (O on the shown item)",
                item.media.file_name()
            )),
            MediaKind::Page | MediaKind::Other => {
                Shown::Elsewhere(format!("{}: opens in the browser", item.media.file_name()))
            }
        }
    }

    /// The surface for the ask in front, filling what it is given.
    pub fn stage(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = theme(cx).clone();
        let items = self.items();
        let Some(key) = self.current.clone() else {
            return div().size_full().bg(t.term_bg).into_any_element();
        };
        let Some(ask) = self.ask(&key).cloned() else {
            return div().size_full().bg(t.term_bg).into_any_element();
        };
        if items.is_empty() {
            return div().size_full().bg(t.term_bg).into_any_element();
        }
        let at = self.view.item.min(items.len() - 1);
        let shown_item = items[at].clone();
        let rec = if ask.questions.is_empty() {
            ask.recommended()
        } else {
            ask.questions
                .get(self.draft().at.unwrap_or(0))
                .and_then(|q| crate::model::recommended(&q.options, &q.recommend))
        };
        let (body, zoom, slots) = if shown_item.doc {
            (self.doc_surface(&t, cx), None, vec![at])
        } else if shown_item.kind() == Some(MediaKind::Page) {
            (self.page_surface(&t, &shown_item, cx), None, vec![at])
        } else {
            self.pictures_surface(&t, &ask, key.0, &items, at, rec, window, cx)
        };
        let video = shown_item.kind() == Some(MediaKind::Video);
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(t.term_bg)
            .child(self.tools(&t, &shown_item, cx))
            .child(div().flex_1().min_h_0().w_full().child(body))
            .when(video, |d| d.child(self.transport(&t, &shown_item, cx)))
            .child(self.film(&t, &ask, key.0, &items, &slots, rec, zoom, cx))
            .into_any_element()
    }

    /// The tools over the surface: how a press marks, for what can be
    /// marked here.
    fn tools(&mut self, t: &Theme, item: &Item, cx: &mut Context<Self>) -> AnyElement {
        let bar = div()
            .h(px(tokens::TOOLS))
            .flex_none()
            .px(px(GAP))
            .flex()
            .items_center()
            .gap(px(GAP_SMALL))
            .bg(t.surface)
            .border_b_1()
            .border_color(t.divider)
            .text_size(px(META))
            .text_color(t.text2);
        if item.doc {
            return bar
                .child(format!(
                    "{} · select words to comment on them",
                    item.media.path
                ))
                .into_any_element();
        }
        if item.kind() == Some(MediaKind::Page) {
            let comment = self.page_comment;
            let inside = crate::page::INSIDE;
            return bar
                .when(inside, |d| {
                    d.child(segment(t, "browse", "Browse", "V", !comment).on_click(
                        cx.listener(|app, _: &ClickEvent, _, cx| app.set_tool(Tool::Look, cx)),
                    ))
                    .child(segment(t, "comment", "Comment", "P", comment).on_click(
                        cx.listener(|app, _: &ClickEvent, _, cx| app.set_tool(Tool::Pin, cx)),
                    ))
                    .child(div().pl(px(GAP_SMALL)).child(if comment {
                        "Click an element to comment on it"
                    } else {
                        "The page works as it would in a browser"
                    }))
                })
                .child(div().flex_1())
                .child(
                    button(t, "open-page", "Open in browser", Kind::Quiet)
                        .on_click(cx.listener(|app, _: &ClickEvent, _, cx| app.open_media(cx))),
                )
                .into_any_element();
        }
        let markable = matches!(item.kind(), Some(MediaKind::Image | MediaKind::Video));
        if self.read_only() {
            let n = self.marks().len();
            return bar
                .child(match n {
                    0 => "Answered · sent without marks".to_string(),
                    1 => "Answered · your mark as you sent it".to_string(),
                    n => format!("Answered · your {n} marks as you sent them"),
                })
                .into_any_element();
        }
        if !markable {
            return bar
                .child(item.media.file_name().to_string())
                .into_any_element();
        }
        let mut bar = bar.child(
            segment(
                t,
                "tool-look",
                Tool::Look.name(),
                Tool::Look.key(),
                self.tool == Tool::Look,
            )
            .on_click(cx.listener(|app, _: &ClickEvent, _, cx| app.set_tool(Tool::Look, cx))),
        );
        for tool in Tool::DRAW {
            bar = bar.child(
                segment(
                    t,
                    SharedString::from(format!("tool-{}", tool.name())),
                    tool.name(),
                    tool.key(),
                    self.tool == tool,
                )
                .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| app.set_tool(tool, cx))),
            );
        }
        let hint = match (self.tool, item.kind()) {
            (Tool::Look, Some(MediaKind::Video)) => "Pause anywhere and mark that frame",
            (Tool::Look, _) => "Pick a tool to mark what you see",
            (Tool::Pin, _) => "Click to pin a comment",
            (Tool::Box, _) => "Drag a box around it",
            (Tool::Arrow, _) => "Drag from the comment to the thing",
            (Tool::Pen, _) => "Draw",
        };
        bar.child(div().pl(px(GAP_SMALL)).child(hint))
            .into_any_element()
    }

    /// The room the pictures had last frame (all their panels together),
    /// or, before any, the window's middle.
    fn stage_room(&self, window: &Window) -> (f32, f32) {
        let rooms = self.rooms.borrow();
        match rooms.iter().copied().reduce(|a, b| a.union(&b)) {
            Some(b) if b.size.width > px(0.) => (f32::from(b.size.width), f32::from(b.size.height)),
            _ => {
                let v = window.viewport_size();
                let (w, h) = (f32::from(v.width), f32::from(v.height));
                let (rail, column) = tokens::columns(w);
                (
                    w - rail - column,
                    h - tokens::BAR - tokens::TOOLS - tokens::STRIP_H,
                )
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn pictures_surface(
        &mut self,
        t: &Theme,
        ask: &Ask,
        desk: usize,
        items: &[Item],
        at: usize,
        rec: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (AnyElement, Option<String>, Vec<usize>) {
        let mut slots = vec![at];
        let mut cols = 1;
        if self.view.side {
            // Every picture of the ask at once, in the rows and columns
            // that draw them largest, while each still reads; past that,
            // the one in front and its partner, the strip for the rest.
            let pictures: Vec<usize> = (0..items.len())
                .filter(|&j| j != at && !items[j].doc && items[j].kind() != Some(MediaKind::Page))
                .collect();
            let room = self.stage_room(window);
            let shape = self.last_shape.get();
            let (c, least) = tokens::grid(pictures.len() + 1, room, shape, 2.);
            if pictures.len() > 1 && least >= tokens::SIDE_LEAST {
                slots.extend(pictures);
                cols = c;
            } else if let Some(p) = self.partner(items)
                && !items[p].doc
                && items[p].kind() != Some(MediaKind::Page)
            {
                slots.push(p);
                cols = tokens::grid(2, room, shape, 2.).0;
            }
        }
        self.rooms
            .borrow_mut()
            .resize(slots.len(), Bounds::default());
        self.rooms.borrow_mut().truncate(slots.len());
        self.drawn_at.borrow_mut().resize(slots.len(), None);
        self.drawn_at.borrow_mut().truncate(slots.len());
        let numbered = self.numbered();
        let frame = self.player.as_ref().map(|(_, p)| p.at);
        let mut panels = Vec::new();
        let mut natural = None;
        for (slot, &i) in slots.iter().enumerate() {
            let item = &items[i];
            let shown = self.shown(ask, desk, item, slot == 0, cx);
            if slot == 0
                && let Shown::Picture(_, n) = &shown
            {
                natural = Some(*n);
                self.last_shape.set(n.0 / n.1.max(1.));
            }
            let video = item.kind() == Some(MediaKind::Video);
            let ink = Ink {
                marks: numbered
                    .iter()
                    .filter(|(_, m)| {
                        marks::shows_on(m, &item.media.id, if video { frame } else { None })
                    })
                    .map(|(n, m)| (*n, m.clone(), self.selected.as_deref() == Some(&m.id)))
                    .collect(),
                drawing: self
                    .drawing
                    .clone()
                    .filter(|d| slot == 0 && d.media == item.media.id),
            };
            panels.push((
                i,
                self.panel(t, slot, item, shown, rec, slots.len() > 1, ink),
            ));
        }
        // Laid out in the ask's order, so a picture keeps its place as the
        // one in front changes.
        panels.sort_by_key(|(i, _)| *i);
        let mut rows = Vec::new();
        let mut panels = panels.into_iter().map(|(_, p)| p).peekable();
        while panels.peek().is_some() {
            let row: Vec<AnyElement> = panels.by_ref().take(cols).collect();
            // A short last row keeps its cells the others' size.
            let empty = cols - row.len();
            rows.push(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .gap(px(2.))
                    .children(row)
                    .children((0..empty).map(|_| div().flex_1().min_w_0())),
            );
        }
        let shown_natural = natural;
        let natural = natural.unwrap_or((1., 1.));
        let zoom = shown_natural.map(|n| {
            let pct = (self.scale_now(n, window) * 100.).round();
            if self.view.fit {
                format!("Fit · {pct}%")
            } else {
                format!("{pct}%")
            }
        });
        let drawing = self.tool != Tool::Look && shown_natural.is_some();
        let stage = div()
            .id("stage")
            .size_full()
            .flex()
            .flex_col()
            .gap(px(2.))
            .bg(t.term_bg)
            .map(|d| {
                if drawing {
                    d.cursor_crosshair()
                } else if self.held.is_some() {
                    d.cursor_grabbing()
                } else {
                    d.cursor_grab()
                }
            })
            .children(rows)
            .on_scroll(cx.listener(move |app, e: &ScrollEvent, window, cx| {
                // A wheel's notch is one step however many lines it says;
                // a touchpad's pixels zoom smoothly.
                let dy = match e.delta {
                    ScrollDelta::Lines(p) => p.y.signum() * p.y.abs().min(1.),
                    ScrollDelta::Pixels(p) => f32::from(p.y) / 40.,
                };
                if dy.abs() < f32::EPSILON {
                    return;
                }
                app.zoom_at(
                    tokens::ZOOM_STEP.powf(dy.clamp(-1., 1.)),
                    Some(e.position),
                    natural,
                    window,
                );
                cx.notify();
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |app, e: &MouseDownEvent, window, cx| {
                    window.focus(&app.focus, cx);
                    if drawing {
                        if let Some(n) = app.on_picture(e.position, false) {
                            app.draw_start(n, cx);
                        }
                        return;
                    }
                    if e.click_count == 2 {
                        if app.view.fit {
                            let s = app.scale_now(natural, window);
                            app.zoom_at(1. / s, Some(e.position), natural, window);
                        } else {
                            app.view.fit = true;
                            app.view.pan = (0., 0.);
                        }
                        cx.notify();
                        return;
                    }
                    if app.view.fit {
                        // A drag from fit moves the picture at the size it is.
                        app.view.scale = app.scale_now(natural, window);
                        app.view.fit = false;
                        app.view.pan = (0., 0.);
                    }
                    app.held = Some((e.position, app.view.pan));
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|app, e: &MouseMoveEvent, _, cx| {
                if app.drawing.is_some() {
                    if e.pressed_button != Some(MouseButton::Left) {
                        return app.draw_end(cx);
                    }
                    if let Some(n) = app.on_picture(e.position, true) {
                        app.draw_move(n, cx);
                    }
                    return;
                }
                let Some((from, pan)) = app.held else { return };
                if e.pressed_button != Some(MouseButton::Left) {
                    app.held = None;
                    cx.notify();
                    return;
                }
                app.view.pan = (
                    pan.0 + f32::from(e.position.x - from.x),
                    pan.1 + f32::from(e.position.y - from.y),
                );
                cx.notify();
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|app, _, _, cx| {
                    if app.drawing.is_some() {
                        return app.draw_end(cx);
                    }
                    app.held = None;
                    cx.notify();
                }),
            );
        (stage.into_any_element(), zoom, slots)
    }

    /// Where `p` falls on the first panel's picture, 0 to 1 across it;
    /// off it, nothing (or, `clamp`ed, its nearest edge).
    fn on_picture(&self, p: Point<Pixels>, clamp: bool) -> Option<[f64; 2]> {
        let b = (*self.drawn_at.borrow().first()?)?;
        let x = f64::from(f32::from(p.x - b.origin.x) / f32::from(b.size.width).max(1.));
        let y = f64::from(f32::from(p.y - b.origin.y) / f32::from(b.size.height).max(1.));
        if clamp {
            return Some([x.clamp(0., 1.), y.clamp(0., 1.)]);
        }
        ((0. ..=1.).contains(&x) && (0. ..=1.).contains(&y)).then_some([x, y])
    }

    #[allow(clippy::too_many_arguments)]
    fn panel(
        &self,
        t: &Theme,
        slot: usize,
        item: &Item,
        shown: Shown,
        rec: Option<usize>,
        two: bool,
        ink: Ink,
    ) -> AnyElement {
        let rooms = self.rooms.clone();
        let drawn_at = self.drawn_at.clone();
        let view = self.view;
        let edge = alpha(t.term_bg, 0.85);
        let badge_text = t.term_bg;
        let body = match shown {
            Shown::Picture(image, natural) => canvas(
                move |b, _, _| b,
                move |_, b: Bounds<Pixels>, window, cx| {
                    if let Some(r) = rooms.borrow_mut().get_mut(slot) {
                        *r = b;
                    }
                    let sf = window.scale_factor();
                    let s = if view.fit {
                        fit_scale(b, natural, sf)
                    } else {
                        view.scale
                    };
                    let pan = if view.fit { (0., 0.) } else { view.pan };
                    let (w, h) = (natural.0 * s / sf, natural.1 * s / sf);
                    let c = b.center();
                    let to = Bounds::new(
                        point(c.x + px(pan.0 - w / 2.), c.y + px(pan.1 - h / 2.)),
                        gpui::size(px(w), px(h)),
                    );
                    if let Some(d) = drawn_at.borrow_mut().get_mut(slot) {
                        *d = Some(to);
                    }
                    window.with_content_mask(Some(ContentMask { bounds: b }), |window| {
                        let _ =
                            window.paint_image(to, to, Corners::default(), image.clone(), 0, false);
                        paint_ink(&ink, to, edge, badge_text, window, cx);
                    });
                },
            )
            .size_full()
            .into_any_element(),
            Shown::Waiting(w) => quiet(w, t.dim),
            Shown::Failed(w) => quiet(w, t.error),
            Shown::Elsewhere(w) => quiet(w, t.dim),
        };
        let caption = [item.tag.as_str(), item.media.caption.as_str()]
            .iter()
            .filter(|s| !s.is_empty())
            .copied()
            .collect::<Vec<_>>()
            .join(" · ");
        let is_rec = item.option.is_some() && item.option == rec;
        div()
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(body)
            .when(!caption.is_empty() || is_rec, |d| {
                d.child(
                    div()
                        .absolute()
                        .left(px(GAP))
                        .top(px(GAP))
                        .max_w(relative(0.8))
                        .flex()
                        .items_center()
                        .gap(px(GAP_SMALL))
                        .px(px(8.))
                        .py(px(4.))
                        .rounded(px(tokens::RADIUS))
                        .bg(alpha(t.bg, 0.78))
                        .text_size(px(META))
                        .text_color(t.text)
                        .when(is_rec, |d| {
                            d.child(
                                div()
                                    .text_color(t.accent)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Recommended"),
                            )
                        })
                        .child(div().truncate().child(caption)),
                )
            })
            .when(two && slot == 0, |d| d.border_r_1().border_color(t.divider))
            .into_any_element()
    }

    /// The document, to read and to mark by selecting its words.
    fn doc_surface(&mut self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let Some(blocks) = self.doc_blocks() else {
            return quiet("The document is empty".into(), t.dim);
        };
        let look = markdown::Look {
            t,
            refs: false,
            on_link: None,
            select: Some(self.doc_select.clone()),
        };
        let column = markdown::render(&look, "doc", &blocks);
        let this = cx.entity().downgrade();
        let body = div()
            .id("doc")
            .size_full()
            .overflow_y_scroll()
            .bg(t.bg)
            .cursor_text()
            .flex()
            .justify_center()
            .child(
                div()
                    .w_full()
                    .max_w(px(tokens::READING))
                    .px(px(tokens::PAD))
                    .py(px(tokens::PAD))
                    .child(column),
            );
        select::track(body, &self.doc_select, cx.entity_id(), move |text, cx| {
            let _ = this.update(cx, |app, cx| app.mark_passage(text, cx));
        })
        .into_any_element()
    }

    /// The page the agent made: inside the window where the system has a
    /// web view the app can lay there, else a way to the browser.
    fn page_surface(&mut self, t: &Theme, item: &Item, cx: &mut Context<Self>) -> AnyElement {
        let Some(url) = self.page_url(item) else {
            return quiet("The page has no address".into(), t.error);
        };
        #[cfg(windows)]
        {
            if let Some(e) = self.page_failed.borrow().clone() {
                return self.page_outside(t, format!("The page did not open here: {e}"), cx);
            }
            let page = self.page.clone();
            let failed = self.page_failed.clone();
            let said = self.page_said.clone();
            canvas(
                move |b, _, _| b,
                move |_, b: Bounds<Pixels>, window, _| {
                    let mut slot = page.borrow_mut();
                    if slot.is_none() {
                        let said = said.clone();
                        match crate::page::WebPage::new(window, &url, move |m| {
                            let _ = said.try_send(m);
                        }) {
                            Ok(p) => *slot = Some(p),
                            Err(e) => {
                                log::error!("{e}");
                                *failed.borrow_mut() = Some(e);
                                return;
                            }
                        }
                    }
                    if let Some(p) = slot.as_mut() {
                        p.show(&url, b);
                    }
                },
            )
            .size_full()
            .into_any_element()
        }
        #[cfg(not(windows))]
        {
            let _ = url;
            self.page_outside(
                t,
                "This page renders inside the Desk on Windows. Here it opens in your browser: say what you think of it in your words.".into(),
                cx,
            )
        }
    }

    fn page_outside(&self, t: &Theme, why: String, cx: &mut Context<Self>) -> AnyElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(GAP))
            .p(px(tokens::PAD))
            .child(
                div()
                    .max_w(px(tokens::READING / 1.6))
                    .text_size(px(META))
                    .text_color(t.text2)
                    .child(why),
            )
            .child(
                button(t, "open-page-big", "Open the page", Kind::Primary)
                    .on_click(cx.listener(|app, _: &ClickEvent, _, cx| app.open_media(cx))),
            )
            .into_any_element()
    }

    /// Play, step and the track of a clip, under the surface; the clip's
    /// marks are ticks on its track.
    fn transport(&mut self, t: &Theme, item: &Item, cx: &mut Context<Self>) -> AnyElement {
        let track = self.track.clone();
        let ticks: Vec<(u64, Hsla, bool)> = self
            .marks()
            .iter()
            .filter(|m| m.media.as_deref() == Some(item.media.id.as_str()))
            .filter_map(|m| {
                Some((
                    m.frame?,
                    rgb(tokens::ink(m.color.as_deref())),
                    self.selected.as_deref() == Some(&m.id),
                ))
            })
            .collect();
        let Some((_, p)) = self.player.as_ref() else {
            return div().h(px(tokens::TRANSPORT)).into_any_element();
        };
        let n = p.frames().max(1);
        let at = p.at;
        let played = (at as f32 + 1.) / n as f32;
        let words = format!(
            "frame {} of {}  ·  {} of {}  ·  {:.0} fps",
            at + 1,
            n,
            marks::clock(p.seconds(at)),
            marks::clock(p.seconds(n)),
            p.clip.fps
        );
        let playing = p.playing;
        let (accent, divider) = (t.accent, t.divider);
        let ticks = ticks.into_iter().map(|(f, c, on)| {
            div()
                .absolute()
                .top(px(-4.))
                .h(px(tokens::TRACK_H + 8.))
                .w(px(if on {
                    tokens::TICK_W * 2.
                } else {
                    tokens::TICK_W
                }))
                .left(relative(f as f32 / n as f32))
                .rounded(px(1.))
                .bg(c)
        });
        div()
            .h(px(tokens::TRANSPORT))
            .flex_none()
            .px(px(GAP))
            .flex()
            .items_center()
            .gap(px(GAP))
            .bg(t.surface)
            .border_t_1()
            .border_color(t.divider)
            .child(
                button(
                    t,
                    "play",
                    if playing { "Pause" } else { "Play" },
                    Kind::Plain,
                )
                .tooltip(kit::tip("Space · ← → a frame · Shift a second"))
                .on_click(cx.listener(|app, _: &ClickEvent, _, cx| app.toggle_play(cx))),
            )
            .child(
                button(t, "back", "‹", Kind::Quiet)
                    .tooltip(kit::tip("A frame back (←)"))
                    .on_click(cx.listener(|app, _: &ClickEvent, _, cx| app.step_frames(-1, cx))),
            )
            .child(
                button(t, "on", "›", Kind::Quiet)
                    .tooltip(kit::tip("A frame on (→)"))
                    .on_click(cx.listener(|app, _: &ClickEvent, _, cx| app.step_frames(1, cx))),
            )
            .child(
                div()
                    .id("track")
                    .flex_1()
                    .h(px(tokens::TRANSPORT))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .child(
                        div()
                            .relative()
                            .w_full()
                            .h(px(tokens::TRACK_H))
                            .rounded_full()
                            .bg(divider)
                            .child(
                                canvas(
                                    move |b, _, _| b,
                                    move |_, b, _, _| *track.borrow_mut() = Some(b),
                                )
                                .absolute()
                                .size_full(),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .left_0()
                                    .top_0()
                                    .h_full()
                                    .w(relative(played))
                                    .rounded_full()
                                    .bg(accent),
                            )
                            .children(ticks),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|app, e: &MouseDownEvent, _, cx| {
                            let b = *app.track.borrow();
                            if let Some(b) = b {
                                let f =
                                    f32::from(e.position.x - b.left()) / f32::from(b.size.width);
                                app.seek_fraction(f, cx);
                            }
                        }),
                    )
                    .on_mouse_move(cx.listener(|app, e: &MouseMoveEvent, _, cx| {
                        if e.pressed_button != Some(MouseButton::Left) {
                            return;
                        }
                        let b = *app.track.borrow();
                        if let Some(b) = b
                            && b.contains(&point(e.position.x, b.center().y))
                        {
                            let f = f32::from(e.position.x - b.left()) / f32::from(b.size.width);
                            app.seek_fraction(f, cx);
                        }
                    })),
            )
            .child(
                div()
                    .flex_none()
                    .text_size(px(META))
                    .text_color(t.text2)
                    .child(words),
            )
            .into_any_element()
    }

    /// Everything the ask shows, small, under the surface: the one shown
    /// is outlined, the one beside it fainter.
    #[allow(clippy::too_many_arguments)]
    fn film(
        &mut self,
        t: &Theme,
        ask: &Ask,
        desk: usize,
        items: &[Item],
        slots: &[usize],
        rec: Option<usize>,
        zoom: Option<String>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut thumbs = Vec::new();
        let many = items.len() > 1;
        let marked: Vec<String> = self.marks().into_iter().filter_map(|m| m.media).collect();
        for (i, item) in items.iter().enumerate().filter(|_| many) {
            let pic = match item.kind() {
                Some(MediaKind::Image) => match self.picture(desk, ask, &item.media, cx) {
                    Some(Pic::Ready { image, .. }) => Some(image.clone()),
                    _ => None,
                },
                Some(MediaKind::Video) => self
                    .player
                    .as_mut()
                    .filter(|(k, _)| {
                        *k == format!("{desk}/{}/{}/{}", ask.id, ask.round(), item.media.id)
                    })
                    .and_then(|(_, p)| p.image()),
                _ => None,
            };
            let on = slots.first() == Some(&i);
            let beside = slots.get(1) == Some(&i);
            let is_rec = item.option.is_some() && item.option == rec;
            let word = match item.kind() {
                None => "Doc",
                Some(MediaKind::Video | MediaKind::OtherVideo) => "Clip",
                Some(MediaKind::Page) => "Page",
                _ => "",
            };
            let label: SharedString = match (item.tag.is_empty(), word.is_empty()) {
                (true, _) => word.into(),
                (false, true) => item.tag.clone().into(),
                (false, false) => format!("{} {word}", item.tag).into(),
            };
            let has_marks = marked.contains(&item.media.id);
            thumbs.push(
                div()
                    .id(("film", i))
                    .relative()
                    .flex_none()
                    .w(px(tokens::STRIP_THUMB_W))
                    .h_full()
                    .rounded(px(tokens::RADIUS))
                    .overflow_hidden()
                    .border_2()
                    .border_color(if on {
                        t.accent
                    } else if beside {
                        alpha(t.accent, 0.4)
                    } else {
                        t.divider
                    })
                    .bg(t.raised)
                    .cursor_pointer()
                    .children(pic.map(|image| img(image).size_full().object_fit(ObjectFit::Cover)))
                    .when(!label.is_empty(), |d| {
                        d.child(
                            div()
                                .absolute()
                                .left(px(4.))
                                .bottom(px(4.))
                                .px(px(4.))
                                .rounded(px(2.))
                                .bg(if is_rec { t.accent } else { alpha(t.bg, 0.8) })
                                .text_color(if is_rec { t.on_accent() } else { t.text })
                                .text_size(px(CAP_TEXT))
                                .font_weight(FontWeight::BOLD)
                                .child(label),
                        )
                    })
                    .when(has_marks, |d| {
                        d.child(
                            div()
                                .absolute()
                                .right(px(4.))
                                .top(px(4.))
                                .size(px(tokens::PIN))
                                .rounded_full()
                                .bg(rgb(tokens::MARK_INK)),
                        )
                    })
                    .tooltip(kit::tip(if item.media.caption.is_empty() {
                        item.media.file_name().to_string()
                    } else {
                        item.media.caption.clone()
                    }))
                    .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| app.show_item(i, cx))),
            );
        }
        let side = self.view.side;
        let comparable = items
            .iter()
            .filter(|i| matches!(i.kind(), Some(MediaKind::Image | MediaKind::Video)))
            .count()
            > 1;
        div()
            .h(px(if many {
                tokens::STRIP_H + 2. * GAP_SMALL
            } else {
                tokens::TRANSPORT
            }))
            .flex_none()
            .px(px(GAP))
            .py(px(GAP_SMALL))
            .flex()
            .gap(px(GAP_SMALL))
            .bg(t.surface)
            .border_t_1()
            .border_color(t.divider)
            .children(thumbs)
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(GAP))
                    .children(zoom.map(|z| {
                        div()
                            .id("zoom")
                            .text_size(px(META))
                            .text_color(t.text2)
                            .tooltip(kit::tip(
                                "Wheel zooms at the pointer · drag moves · Z fits · 0 is 1:1 · double click goes between",
                            ))
                            .child(z)
                    }))
                    .when(comparable, |d| {
                        d.child(
                            button(
                                t,
                                "compare",
                                if side { "One" } else { "Compare" },
                                Kind::Quiet,
                            )
                            .tooltip(kit::tip(
                                "C · every picture at once while each still reads · F steps the one in front",
                            ))
                            .on_click(cx.listener(|app, _: &ClickEvent, _, cx| {
                                app.view.side = !app.view.side;
                                cx.notify();
                            })),
                        )
                    }),
            )
            .into_any_element()
    }
}

/// A tool's button: on, or not; its key beside its name.
fn segment(
    t: &Theme,
    id: impl Into<gpui::ElementId>,
    name: &str,
    key: &str,
    on: bool,
) -> gpui::Stateful<gpui::Div> {
    button(
        t,
        id,
        name.to_string(),
        if on { Kind::Primary } else { Kind::Quiet },
    )
    .gap(px(GAP_SMALL))
    .child(
        div()
            .text_size(px(CAP_TEXT))
            .text_color(if on { alpha(t.on_accent(), 0.7) } else { t.dim })
            .child(key.to_string()),
    )
}

/// A mark as drawn: its kind, its points on the window, its ink, whether
/// it is selected, its number (none while being drawn).
type Shape = (String, Vec<Point<Pixels>>, Hsla, bool, Option<usize>);

/// The marks over a picture drawn at `to`: each in its ink over a dark
/// edge so it reads on any picture, the selected one heavier, each with
/// its number.
fn paint_ink(
    ink: &Ink,
    to: Bounds<Pixels>,
    edge: Hsla,
    badge_text: Hsla,
    window: &mut Window,
    cx: &mut App,
) {
    let at = |p: [f64; 2]| {
        point(
            to.origin.x + to.size.width * p[0] as f32,
            to.origin.y + to.size.height * p[1] as f32,
        )
    };
    let mut shapes: Vec<Shape> = ink
        .marks
        .iter()
        .map(|(n, m, on)| {
            (
                m.kind.clone(),
                m.points.iter().map(|p| at(*p)).collect(),
                rgb(tokens::ink(m.color.as_deref())),
                *on,
                Some(*n),
            )
        })
        .collect();
    if let Some(d) = &ink.drawing {
        let kind = match d.tool {
            Tool::Box => "box",
            Tool::Arrow => "arrow",
            Tool::Pen => "pen",
            _ => "pin",
        };
        shapes.push((
            kind.into(),
            d.points.iter().map(|p| at(*p)).collect(),
            rgb(tokens::MARK_INK),
            true,
            None,
        ));
    }
    for (kind, points, color, on, n) in shapes {
        let w = if on {
            tokens::MARK_LINE_ON
        } else {
            tokens::MARK_LINE
        };
        for (width, c) in [(w + tokens::MARK_EDGE, edge), (w, color)] {
            match (kind.as_str(), points.as_slice()) {
                ("box" | "region", [a, b, ..]) => {
                    let r = Bounds::from_corners(
                        point(a.x.min(b.x), a.y.min(b.y)),
                        point(a.x.max(b.x), a.y.max(b.y)),
                    );
                    let grow = px((width - w) / 2.);
                    let r = Bounds::from_corners(
                        point(r.origin.x - grow, r.origin.y - grow),
                        point(
                            r.origin.x + r.size.width + grow,
                            r.origin.y + r.size.height + grow,
                        ),
                    );
                    window.paint_quad(quad(
                        r,
                        px(2.),
                        transparent_black(),
                        px(width),
                        c,
                        BorderStyle::Solid,
                    ));
                }
                ("arrow", [a, b, ..]) => {
                    stroke(&[*a, *b], width, c, window);
                    let (dx, dy) = (f32::from(b.x - a.x), f32::from(b.y - a.y));
                    let len = dx.hypot(dy).max(1.);
                    let (ux, uy) = (dx / len, dy / len);
                    let head = 6. * w;
                    for side in [-1f32, 1.] {
                        let tip = point(
                            b.x - px(head * ux - side * head * 0.5 * uy),
                            b.y - px(head * uy + side * head * 0.5 * ux),
                        );
                        stroke(&[tip, *b], width, c, window);
                    }
                }
                (_, [_, _, ..]) => stroke(&points, width, c, window),
                (_, [p]) => {
                    let r = tokens::PIN + (width - w) / 2.;
                    window.paint_quad(
                        fill(
                            Bounds::new(
                                point(p.x - px(r), p.y - px(r)),
                                gpui::size(px(2. * r), px(2. * r)),
                            ),
                            c,
                        )
                        .corner_radii(px(r)),
                    );
                }
                _ => {}
            }
        }
        if let (Some(n), Some(p)) = (n, points.first()) {
            badge(n, *p, color, badge_text, edge, window, cx);
        }
    }
}

fn stroke(points: &[Point<Pixels>], width: f32, color: Hsla, window: &mut Window) {
    let mut path = PathBuilder::stroke(px(width));
    path.move_to(points[0]);
    for p in &points[1..] {
        path.line_to(*p);
    }
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

/// A mark's number on a round plate beside where it starts.
fn badge(
    n: usize,
    at: Point<Pixels>,
    color: Hsla,
    text: Hsla,
    edge: Hsla,
    window: &mut Window,
    cx: &mut App,
) {
    let size = tokens::BADGE;
    let origin = point(at.x + px(tokens::PIN), at.y - px(size + tokens::PIN / 2.));
    let plate = Bounds::new(origin, gpui::size(px(size), px(size)));
    window.paint_quad(quad(
        plate,
        px(size / 2.),
        color,
        px(1.),
        edge,
        BorderStyle::Solid,
    ));
    let words: SharedString = n.to_string().into();
    let run = TextRun {
        len: words.len(),
        font: Font {
            weight: FontWeight::BOLD,
            ..font(FAMILY)
        },
        color: text,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let line = window
        .text_system()
        .shape_line(words, px(tokens::BADGE_TEXT), &[run], None);
    let _ = line.paint(
        point(
            origin.x,
            origin.y + px((size - tokens::BADGE_TEXT * 1.3) / 2.),
        ),
        px(tokens::BADGE_TEXT * 1.3),
        TextAlign::Center,
        Some(px(size)),
        window,
        cx,
    );
}

fn quiet(words: String, colour: Hsla) -> AnyElement {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .p(px(tokens::PAD))
        .text_size(px(META))
        .text_color(colour)
        .child(words)
        .into_any_element()
}
