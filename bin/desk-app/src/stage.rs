//! The stage: what the ask shows, as big as the room allows, on the dark
//! ground pictures read true on. One picture or two side by side (C), the
//! two always at the same zoom and place so a difference is where the eye
//! already is; F swaps to the other in place. The wheel zooms about the
//! pointer, a drag moves, a double click goes between fit and 1:1. A clip
//! plays, scrubs and steps by the frame.

use crate::app::{DeskApp, Item, Pic, fit_scale};
use crate::model::{Ask, MediaKind};
use crate::tokens::{self, GAP, GAP_SMALL, SMALL, TEXT};
use gpui::{
    AnyElement, Bounds, ClickEvent, ContentMask, Context, Corners, FontWeight, MouseButton,
    MouseDownEvent, MouseMoveEvent, ObjectFit, Pixels, RenderImage, ScrollDelta, ScrollEvent,
    SharedString, StyledImage, Window, canvas, div, img, point, prelude::*, px, relative,
};
use parvi_app_kit::theme::{Theme, alpha, theme};
use std::sync::Arc;

/// What one panel of the stage draws.
enum Shown {
    Picture(Arc<RenderImage>, (f32, f32)),
    Waiting(String),
    Failed(String),
    /// A clip the app does not decode: the browser plays it.
    Elsewhere(String),
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
                "{}: this kind of clip plays in the browser (P)",
                item.media.file_name()
            )),
            MediaKind::Other => Shown::Elsewhere(format!(
                "{}: opens in the browser (P)",
                item.media.file_name()
            )),
        }
    }

    /// The stage for the ask in front, filling what it is given.
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
            return div()
                .size_full()
                .bg(t.term_bg)
                .flex()
                .items_center()
                .justify_center()
                .text_color(t.dim)
                .text_size(px(TEXT))
                .child("Nothing to look at: the words are the ask")
                .into_any_element();
        }
        let at = self.view.item.min(items.len() - 1);
        let mut slots = vec![at];
        if self.view.side
            && let Some(p) = self.partner(&items)
        {
            slots.push(p);
        }
        let rec = if ask.questions.is_empty() {
            ask.recommended()
        } else {
            ask.questions
                .get(self.draft().at)
                .and_then(|q| crate::model::recommended(&q.options, &q.recommend))
        };
        self.rooms
            .borrow_mut()
            .resize(slots.len(), Bounds::default());
        self.rooms.borrow_mut().truncate(slots.len());
        let mut panels = Vec::new();
        let mut natural = None;
        for (slot, &i) in slots.iter().enumerate() {
            let item = &items[i];
            let shown = self.shown(&ask, key.0, item, slot == 0, cx);
            if slot == 0
                && let Shown::Picture(_, n) = &shown
            {
                natural = Some(*n);
            }
            panels.push(self.panel(&t, slot, item, shown, rec, slots.len() > 1));
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
        let stage = div()
            .id("stage")
            .flex_1()
            .min_h_0()
            .w_full()
            .flex()
            .gap(px(2.))
            .bg(t.term_bg)
            .cursor_grab()
            .when(self.held.is_some(), |d| d.cursor_grabbing())
            .children(panels)
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
                    app.held = None;
                    cx.notify();
                }),
            );
        let video = items[at].media.kind() == MediaKind::Video;
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(t.term_bg)
            .child(stage)
            .when(video, |d| d.child(self.transport(&t, cx)))
            .child(self.film(&t, &ask, key.0, &items, &slots, rec, zoom, cx))
            .into_any_element()
    }

    fn panel(
        &self,
        t: &Theme,
        slot: usize,
        item: &Item,
        shown: Shown,
        rec: Option<usize>,
        two: bool,
    ) -> AnyElement {
        let rooms = self.rooms.clone();
        let view = self.view;
        let body = match shown {
            Shown::Picture(image, natural) => canvas(
                move |b, _, _| b,
                move |_, b: Bounds<Pixels>, window, _| {
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
                    window.with_content_mask(Some(ContentMask { bounds: b }), |w| {
                        let _ = w.paint_image(to, to, Corners::default(), image.clone(), 0, false);
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
                        .text_size(px(SMALL))
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

    /// Play, step and the track of a clip, under the stage.
    fn transport(&mut self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let track = self.track.clone();
        let Some((_, p)) = self.player.as_ref() else {
            return div().h(px(tokens::TRANSPORT)).into_any_element();
        };
        let n = p.frames().max(1);
        let at = p.at;
        let played = (at as f32 + 1.) / n as f32;
        let words = format!(
            "frame {} of {}  ·  {:.2} s of {:.2} s  ·  {:.0} fps",
            at + 1,
            n,
            p.seconds(at),
            p.seconds(n),
            p.clip.fps
        );
        let playing = p.playing;
        let (accent, divider) = (t.accent, t.divider);
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
                parvi_app_kit::widgets::kit::button(
                    t,
                    "play",
                    if playing { "Pause" } else { "Play" },
                    parvi_app_kit::widgets::kit::Kind::Plain,
                )
                .tooltip(parvi_app_kit::widgets::kit::tip(
                    "Space · ← → a frame · Shift a second",
                ))
                .on_click(cx.listener(|app, _: &ClickEvent, _, cx| app.toggle_play(cx))),
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
                            ),
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
                    .text_size(px(SMALL))
                    .text_color(t.text2)
                    .child(words),
            )
            .into_any_element()
    }

    /// Every picture and clip of the ask, small, under the stage: the one
    /// shown is outlined, the one beside it fainter.
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
        for (i, item) in items.iter().enumerate().filter(|_| many) {
            let pic = match item.media.kind() {
                MediaKind::Image => match self.picture(desk, ask, &item.media, cx) {
                    Some(Pic::Ready { image, .. }) => Some(image.clone()),
                    _ => None,
                },
                MediaKind::Video => self
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
            let label: SharedString = if item.tag.is_empty() {
                match item.media.kind() {
                    MediaKind::Video | MediaKind::OtherVideo => "clip".into(),
                    _ => "".into(),
                }
            } else {
                item.tag.clone().into()
            };
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
                                .text_size(px(SMALL))
                                .font_weight(FontWeight::BOLD)
                                .child(label),
                        )
                    })
                    .tooltip(parvi_app_kit::widgets::kit::tip(
                        if item.media.caption.is_empty() {
                            item.media.file_name().to_string()
                        } else {
                            item.media.caption.clone()
                        },
                    ))
                    .on_click(cx.listener(move |app, _: &ClickEvent, _, cx| app.show_item(i, cx))),
            );
        }
        let side = self.view.side;
        div()
            .h(px(if many { tokens::STRIP_H + 2. * GAP_SMALL } else { tokens::TRANSPORT }))
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
                            .text_size(px(SMALL))
                            .text_color(t.text2)
                            .tooltip(parvi_app_kit::widgets::kit::tip(
                                "Wheel zooms at the pointer · drag moves · Z fits · 0 is 1:1 · double click goes between",
                            ))
                            .child(z)
                    }))
                    .when(many, |d| d.child(
                        parvi_app_kit::widgets::kit::button(
                            t,
                            "compare",
                            if side { "One" } else { "Side by side" },
                            parvi_app_kit::widgets::kit::Kind::Quiet,
                        )
                        .tooltip(parvi_app_kit::widgets::kit::tip("C · F swaps the two in place"))
                        .on_click(cx.listener(|app, _: &ClickEvent, _, cx| {
                            app.view.side = !app.view.side;
                            cx.notify();
                        })),
                    )),
            )
            .into_any_element()
    }
}

fn quiet(words: String, colour: gpui::Hsla) -> AnyElement {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .p(px(tokens::PAD))
        .text_size(px(TEXT))
        .text_color(colour)
        .child(words)
        .into_any_element()
}
