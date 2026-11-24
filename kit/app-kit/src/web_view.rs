//! A web page laid inside the app's window.
//!
//! On Windows the page is a WebView2 view (through `wry`), a child window of
//! the app's window placed over a surface's bounds. A child window draws above
//! everything the app draws where it lies, so its owner shows it where the
//! surface is and hides it when the surface shows something else.
//!
//! Elsewhere there is no system web view the app can lay inside its window: on
//! Linux one needs GTK's event loop, which a GPUI window does not run. There
//! a [`WebSurface`] fails with `web_view_unsupported`, and [`SHOWS_INSIDE`]
//! tells the owner beforehand so it can offer the browser instead.

use crate::Error;
use gpui::{IntoElement, Styled, canvas};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

/// Whether this system can show a web page inside the app's window.
pub const SHOWS_INSIDE: bool = cfg!(windows);

/// What a web view opens with.
#[derive(Default)]
pub struct WebViewSetup {
    /// Where the system web view keeps its profile (cache, storage). `None`
    /// keeps it beside the program.
    pub data_dir: Option<PathBuf>,
    /// A script that runs on every page before the page's own.
    pub script: Option<String>,
    /// Called with each message the page posts (`window.ipc.postMessage`),
    /// on the app's thread.
    pub on_message: Option<Box<dyn Fn(String)>>,
}

/// A web view that opens the first time its surface is painted and follows
/// the surface's bounds after that. Clones share the one view.
#[derive(Clone, Default)]
pub struct WebSurface {
    slot: Rc<RefCell<Slot>>,
}

#[derive(Default)]
enum Slot {
    #[default]
    Closed,
    Opening,
    Open(imp::View),
    Failed(Error),
}

impl WebSurface {
    /// The surface showing `url`, filling its parent. Its first paint starts
    /// opening the view with what `setup` gives; once open, every paint lays
    /// it over the surface's bounds. When the view did not open,
    /// [`Self::failed`] says why and the surface draws nothing.
    ///
    /// The view opens in a task of its own: the system web view's creation
    /// runs the window's messages until it is done, and the app's other
    /// tasks among them, so it must not run while a paint holds the app.
    pub fn element(
        &self,
        url: impl Into<String>,
        setup: impl FnOnce() -> WebViewSetup + 'static,
    ) -> impl IntoElement {
        let slot = self.slot.clone();
        let url = url.into();
        canvas(
            |_, _, _| {},
            move |bounds, _, window, cx| {
                let mut shown = slot.borrow_mut();
                match &mut *shown {
                    Slot::Open(view) => view.show(&url, bounds, window.ui_scale()),
                    Slot::Opening | Slot::Failed(_) => {}
                    Slot::Closed => {
                        *shown = Slot::Opening;
                        drop(shown);
                        let parent = imp::Parent::of(window);
                        let setup = setup();
                        window
                            .spawn(cx, async move |cx| {
                                let opened = parent.and_then(|p| imp::View::open(p, &url, setup));
                                *slot.borrow_mut() = match opened {
                                    Ok(view) => Slot::Open(view),
                                    Err(e) => {
                                        log::error!("{e}");
                                        Slot::Failed(e)
                                    }
                                };
                                // The next paint shows it, or its owner says
                                // why it did not open.
                                let _ = cx.update(|window, _| window.refresh());
                            })
                            .detach();
                    }
                }
            },
        )
        .size_full()
    }

    /// Why the view did not open, once it tried and failed.
    pub fn failed(&self) -> Option<Error> {
        match &*self.slot.borrow() {
            Slot::Failed(e) => Some(e.clone()),
            _ => None,
        }
    }

    /// Hides the view until the surface is painted again.
    pub fn hide(&self) {
        if let Slot::Open(view) = &mut *self.slot.borrow_mut() {
            view.hide();
        }
    }

    /// Runs `script` in the page as it is now, when the view is open.
    pub fn run(&self, script: &str) {
        if let Slot::Open(view) = &*self.slot.borrow() {
            view.run(script);
        }
    }
}

#[cfg(windows)]
mod imp {
    use super::WebViewSetup;
    use crate::Error;
    use gpui::{Bounds, Pixels, Window};
    use raw_window_handle::{HandleError, HasWindowHandle, RawWindowHandle, WindowHandle};
    use wry::dpi::{LogicalPosition, LogicalSize};
    use wry::{Rect, WebContext, WebView, WebViewBuilder};

    /// The app's window, kept to open a view in it from outside the paint.
    pub struct Parent(RawWindowHandle);

    impl Parent {
        pub fn of(window: &Window) -> Result<Self, Error> {
            HasWindowHandle::window_handle(window)
                .map(|h| Self(h.as_raw()))
                .map_err(|e| {
                    Error::new(
                        "web_view_unopened",
                        format!("the window has no handle: {e}"),
                    )
                })
        }
    }

    impl HasWindowHandle for Parent {
        fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
            // SAFETY: the handle is the app's window's, which outlives the
            // task that opens a view in it; a window closed meanwhile makes
            // the view's creation fail, not misbehave.
            Ok(unsafe { WindowHandle::borrow_raw(self.0) })
        }
    }

    pub struct View {
        view: WebView,
        _context: WebContext,
        url: String,
        /// The bounds it lies over and the window's zoom they were laid at.
        at: Option<(Bounds<Pixels>, f32)>,
        shown: bool,
    }

    impl View {
        pub fn open(parent: Parent, url: &str, setup: WebViewSetup) -> Result<Self, Error> {
            let mut context = WebContext::new(setup.data_dir);
            let mut builder = WebViewBuilder::new_with_web_context(&mut context)
                .with_url(url)
                .with_visible(false);
            if let Some(script) = &setup.script {
                builder = builder.with_initialization_script(script);
            }
            if let Some(message) = setup.on_message {
                builder = builder.with_ipc_handler(move |req| message(req.into_body()));
            }
            let view = builder.build_as_child(&parent).map_err(|e| {
                Error::new(
                    "web_view_unopened",
                    format!("the web view did not open: {e}"),
                )
            })?;
            Ok(Self {
                view,
                _context: context,
                url: url.into(),
                at: None,
                shown: false,
            })
        }

        /// `ui` is the window's zoom: the app's pixels are this many of the
        /// system's.
        pub fn show(&mut self, url: &str, bounds: Bounds<Pixels>, ui: f32) {
            if self.url != url {
                self.url = url.into();
                if let Err(e) = self.view.load_url(url) {
                    log::warn!("the web view did not load {url}: {e}");
                }
            }
            if self.at != Some((bounds, ui)) {
                self.at = Some((bounds, ui));
                let rect = Rect {
                    position: LogicalPosition::new(
                        f32::from(bounds.origin.x) * ui,
                        f32::from(bounds.origin.y) * ui,
                    )
                    .into(),
                    size: LogicalSize::new(
                        f32::from(bounds.size.width) * ui,
                        f32::from(bounds.size.height) * ui,
                    )
                    .into(),
                };
                if let Err(e) = self.view.set_bounds(rect) {
                    log::warn!("the web view did not move: {e}");
                }
            }
            if !self.shown {
                self.shown = true;
                if let Err(e) = self.view.set_visible(true) {
                    log::warn!("the web view did not show: {e}");
                }
            }
        }

        pub fn hide(&mut self) {
            if self.shown {
                self.shown = false;
                if let Err(e) = self.view.set_visible(false) {
                    log::warn!("the web view did not hide: {e}");
                }
            }
        }

        pub fn run(&self, script: &str) {
            if let Err(e) = self.view.evaluate_script(script) {
                log::warn!("the web view did not run the app's script: {e}");
            }
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::WebViewSetup;
    use crate::Error;
    use gpui::{Bounds, Pixels, Window};
    use std::convert::Infallible;

    pub struct Parent;

    impl Parent {
        pub fn of(_: &Window) -> Result<Self, Error> {
            Ok(Self)
        }
    }

    /// No view: [`View::open`] always fails, so none exists.
    pub struct View(Infallible);

    impl View {
        pub fn open(_: Parent, _: &str, _: WebViewSetup) -> Result<Self, Error> {
            Err(Error::new(
                "web_view_unsupported",
                "this system cannot show a web page inside the app's window; \
                 open it in the browser",
            ))
        }

        pub fn show(&mut self, _: &str, _: Bounds<Pixels>, _: f32) {
            match self.0 {}
        }

        pub fn hide(&mut self) {
            match self.0 {}
        }

        pub fn run(&self, _: &str) {
            match self.0 {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{ParentElement, TestAppContext, Window, div};

    /// Off Windows there is no system web view; on Windows the test
    /// platform's window has no handle to lay one in. Either way the surface
    /// says why it did not open.
    #[gpui::test]
    fn a_view_that_cannot_open_says_why_and_draws_nothing(cx: &mut TestAppContext) {
        let surface = WebSurface::default();
        let shown = surface.clone();
        let window = cx.add_window(move |_, _| Holder(shown));
        cx.run_until_parked();
        window
            .update(cx, |_, _, _| {
                let e = surface.failed().expect("the view could not open");
                let why = if cfg!(windows) {
                    "web_view_unopened"
                } else {
                    "web_view_unsupported"
                };
                assert_eq!(e.code, why);
                // Hiding and running a script without a view do nothing.
                surface.hide();
                surface.run("1");
            })
            .unwrap();
    }

    struct Holder(WebSurface);

    impl gpui::Render for Holder {
        fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .child(self.0.element("about:blank", WebViewSetup::default))
        }
    }
}
