//! A picture in a pane (CHROTE `FilePanelViewer` and `ImageGlance`): drawn
//! at the one zoom level every picture shares (`store::files::zoom`), whole
//! in its room at fit, with its size in pixels and the level's words.

use crate::open::Show;
use crate::page::FilesPage;
use crate::store::zoom::{self, Px};
use ahjo_app_kit::theme::{Theme, size};
use ahjo_app_kit::widgets::kit::{self, Kind};
use gpui::{AnyElement, App, Context, Div, RenderImage, Window, canvas, div, img, prelude::*, px};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

/// The room a side's picture is drawn in, as last laid out.
pub type Room = Rc<Cell<Option<Px>>>;

const ZOOM_GAP: f32 = 2.;
const ZOOM_PAD: f32 = 6.;
const ZOOM_LABEL_WIDTH: f32 = 44.;

/// A picture a Markdown file shows in its text: on its way, read, or not
/// to be had (then its words stay a link to it).
#[derive(Clone)]
pub enum Inline {
    Reading,
    Ready(Arc<RenderImage>),
    Failed,
}

/// The widest an inline picture is drawn, as a share of its natural size
/// in device pixels: none larger than it is.
const INLINE_MOST: f32 = 1.;

impl FilesPage {
    /// The picture `src` of the Markdown open from `folder`, drawn at its
    /// size or the column's width; asked for the first time it is seen.
    pub fn inline_picture(
        &self,
        node: &str,
        markdown: &str,
        src: &str,
        cx: &mut Context<Self>,
    ) -> Option<Inline> {
        let folder = crate::store::paths::parent(markdown);
        if src.contains("://") || src.starts_with("data:") || src.starts_with('#') {
            return None;
        }
        let path = crate::page::resolve(&folder, src.split('#').next().unwrap_or(src));
        if crate::open::picture_format(&path).is_none()
            || !self.tree.roots.is_empty()
                && crate::store::paths::root_of(&self.tree.roots, &path).is_none()
        {
            return None;
        }
        let key = [node.to_string(), markdown.to_string(), path.clone()];
        if let Some(p) = self.md_pictures.borrow().get(&key) {
            return Some(p.clone());
        }
        self.md_pictures
            .borrow_mut()
            .insert(key.clone(), Inline::Reading);
        let format = crate::open::picture_format(&path)?;
        let task = self.host.request(
            ahjo_app_kit::host::Request::get(
                ahjo_core::NodeId(node.to_string()),
                crate::page::bytes_uri(&path),
            ),
            cx,
        );
        let svg = cx.svg_renderer();
        cx.spawn(async move |this, cx| {
            let got = match task.await {
                Ok(a) if a.status == 200 => Some(a.body),
                _ => None,
            };
            let decoded = match got {
                Some(bytes) => {
                    cx.background_executor()
                        .spawn(async move {
                            gpui::Image::from_bytes(format, bytes)
                                .to_image_data(svg)
                                .ok()
                        })
                        .await
                }
                None => None,
            };
            let _ = this.update(cx, |p, cx| {
                let now = decoded.map_or(Inline::Failed, Inline::Ready);
                if let Some(slot) = p.md_pictures.borrow_mut().get_mut(&key) {
                    *slot = now;
                }
                cx.notify();
            });
        })
        .detach();
        Some(Inline::Reading)
    }

    /// An inline picture drawn: whole, no wider than the column, its
    /// words under it.
    pub fn inline_drawn(image: Arc<RenderImage>, most: f32, alt: &str, t: &Theme) -> AnyElement {
        let natural = image.size(0);
        let (w, h) = (natural.width.0 as f32, natural.height.0 as f32);
        let w_at = (w * INLINE_MOST).min(most).max(1.);
        let h_at = h * w_at / w.max(1.);
        div()
            .flex()
            .flex_col()
            .gap(px(ZOOM_GAP))
            .child(img(image).w(px(w_at)).h(px(h_at)))
            .when(!alt.is_empty(), |d| {
                d.child(
                    div()
                        .text_size(px(size::SMALL))
                        .text_color(t.dim)
                        .child(alt.to_string()),
                )
            })
            .into_any_element()
    }

    /// How big pictures are drawn, kept in the settings file.
    pub fn zoom(&self, cx: &App) -> zoom::Level {
        self.host
            .setting::<crate::store::Kept>(cx)
            .unwrap_or_default()
            .zoom
    }

    pub fn set_zoom(&mut self, level: zoom::Level, cx: &mut Context<Self>) {
        let mut kept: crate::store::Kept = self.host.setting(cx).unwrap_or_default();
        kept.zoom = zoom::settle(level);
        let _ = self.host.set_setting(&kept, cx);
        cx.notify();
    }

    /// − and +: the next stop from what side `i`'s picture shows now.
    pub fn step_zoom(&mut self, up: bool, cx: &mut Context<Self>) {
        let Some(Show::Picture { natural, .. }) = self.opened.as_ref().map(|o| &o.show) else {
            return;
        };
        let Some(room) = self.room.get() else {
            return;
        };
        let next = zoom::step(self.zoom(cx), *natural, room, up);
        self.set_zoom(next, cx);
    }

    /// Frees the pictures the page no longer shows; the window keeps a
    /// decoded picture until told.
    pub fn release_pictures(&mut self, window: &mut Window) {
        let mut shown: Vec<Arc<RenderImage>> = self
            .opened
            .iter()
            .filter_map(|o| match &o.show {
                Show::Picture { image, .. } => Some(image.clone()),
                Show::Model(model) => Some(model.image.clone()),
                _ => None,
            })
            .collect();
        // The open Markdown's pictures stay; another file's go.
        let open = self.opened.as_ref().map(|o| o.key());
        let mut inline = self.md_pictures.borrow_mut();
        inline.retain(|[node, md, _], _| open.as_ref().is_some_and(|(n, p)| n == node && p == md));
        shown.extend(inline.values().filter_map(|p| match p {
            Inline::Ready(image) => Some(image.clone()),
            _ => None,
        }));
        drop(inline);
        for gone in self.drawn.drain(..) {
            if !shown.iter().any(|s| Arc::ptr_eq(s, &gone)) {
                let _ = window.drop_image(gone);
            }
        }
        self.drawn = shown;
    }

    /// The picture's words in the file's header: its pixels, Fit, 1:1,
    /// −, the percent it is drawn at, +.
    /// `narrow` (0 to 3) leaves out its pixels and 1:1, then the percent,
    /// then − and +.
    pub(super) fn zoom_bar(&self, t: &Theme, narrow: u8, cx: &mut Context<Self>) -> Option<Div> {
        let Some(Show::Picture { natural, .. }) = self.opened.as_ref().map(|o| &o.show) else {
            return None;
        };
        let level = self.zoom(cx);
        let percent = self
            .room
            .get()
            .map(|room| format!("{}%", zoom::drawn_percent(*natural, room, level)));
        let word = |id: &'static str, label: &'static str, on: bool| {
            kit::button(t, id, label, if on { Kind::Plain } else { Kind::Quiet })
                .debug_selector(move || id.into())
        };
        Some(
            div()
                .flex()
                .items_center()
                .gap(px(ZOOM_GAP))
                .when(narrow == 0, |d| {
                    d.child(
                        div()
                            .px(px(ZOOM_PAD))
                            .text_size(px(size::SMALL))
                            .text_color(t.text2)
                            .whitespace_nowrap()
                            .child(format!("{} × {}", natural.w, natural.h)),
                    )
                })
                .child(
                    word("zoom-fit", "Fit", level.is_none())
                        .on_click(cx.listener(|f, _, _, cx| f.set_zoom(None, cx))),
                )
                .when(narrow == 0, |d| {
                    d.child(
                        word("zoom-1", "1:1", level == Some(100))
                            .on_click(cx.listener(|f, _, _, cx| f.set_zoom(Some(100), cx))),
                    )
                })
                .when(narrow < 3, |d| {
                    d.child(
                        word("zoom-out", "−", false)
                            .on_click(cx.listener(move |f, _, _, cx| f.step_zoom(false, cx))),
                    )
                    .when(narrow < 2, |d| {
                        d.child(
                            div()
                                .min_w(px(ZOOM_LABEL_WIDTH))
                                .text_size(px(size::SMALL))
                                .text_color(t.text2)
                                .flex()
                                .justify_center()
                                .children(percent),
                        )
                    })
                    .child(
                        word("zoom-in", "+", false)
                            .on_click(cx.listener(move |f, _, _, cx| f.step_zoom(true, cx))),
                    )
                }),
        )
    }

    /// The picture at the level: centred while it fits, scrolling inside
    /// the room when a percentage makes it bigger.
    pub(super) fn picture(&self, image: &Arc<RenderImage>, natural: Px, cx: &App) -> AnyElement {
        let room = self.room.clone();
        let mut picture = img(image.clone())
            .flex_none()
            .debug_selector(|| "files-picture".into());
        // The stage is the room, or the picture where it is bigger, so a
        // small picture is centred and a big one scrolls from its top left.
        let mut stage = div().p(px(MARGIN)).flex().items_center().justify_center();
        if let Some(r) = room.get() {
            let d = zoom::draw(natural, r, self.zoom(cx));
            picture = picture.w(px(d.w)).h(px(d.h));
            stage = stage
                .w(px(r.w.max(d.w) + 2. * MARGIN))
                .h(px(r.h.max(d.h) + 2. * MARGIN));
        }
        div()
            .size_full()
            .relative()
            .child(measure(room))
            .child(
                div()
                    .id("picture")
                    .absolute()
                    .inset_0()
                    .overflow_scroll()
                    .child(stage.flex_none().child(picture)),
            )
            .into_any_element()
    }
}

/// Room left around a picture, so it never touches the pane's edges.
const MARGIN: f32 = 16.;

/// Records the room's size as laid out, and draws again when it changed.
fn measure(room: Room) -> impl IntoElement {
    canvas(
        move |bounds, window, cx| {
            let now = Px {
                w: (f32::from(bounds.size.width) - 2. * MARGIN).max(1.),
                h: (f32::from(bounds.size.height) - 2. * MARGIN).max(1.),
            };
            if room.get() != Some(now) {
                room.set(Some(now));
                // A refresh asked for while drawing is dropped, so ask after it.
                window.defer(cx, |window, _| window.refresh());
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_full()
}
