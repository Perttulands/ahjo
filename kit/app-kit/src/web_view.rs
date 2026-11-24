//! A web page laid inside the app's window.
//!
//! On Windows the page is a WebView2 view (through `wry`), a child window of
//! the app's window placed over a surface's bounds. A child window draws above
//! everything the app draws where it lies, so the kit decides once a frame,
//! after everything else is drawn ([`settle`]), whether it shows: where its
//! surface was painted this frame and no popup, menu or layer on the stack
//! overlaps it. A surface not painted (its owner shows something else, Back,
//! the panel put away) hides.
//!
//! A page reaches the app in two ways besides its own messages: it says when
//! it takes the keyboard, which the app then gives to the owner's stand-in
//! focus ([`WebViewSetup::focus`]), and an Escape it leaves unhandled, which
//! the app then handles as an Escape pressed there, once (ADR 0006).
//!
//! A page can load from a scheme of the app's own ([`Scheme`]): the app
//! answers each request itself, so the page never gets another origin's
//! authority.
//!
//! Elsewhere there is no system web view the app can lay inside its window: on
//! Linux one needs GTK's event loop, which a GPUI window does not run. There
//! a [`WebSurface`] fails with `web_view_unsupported`, and [`SHOWS_INSIDE`]
//! tells the owner beforehand so it can offer the browser instead.

use crate::Error;
use gpui::{
    App, Bounds, FocusHandle, IntoElement, Keystroke, Pixels, Styled, Window, WindowId, canvas,
};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::sync::Arc;

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
    /// The app's own scheme the view's pages load from.
    pub scheme: Option<Scheme>,
    /// What has the keyboard, as far as the app's keys go, while the page
    /// has it: an Escape the page leaves goes there.
    pub focus: Option<FocusHandle>,
}

/// A scheme of the app's own: every request a page makes to an address of
/// it ([`scheme_url`]) is answered by `serve`, never by the network.
#[derive(Clone)]
pub struct Scheme {
    /// Its name, in lower case letters and `-`.
    pub name: &'static str,
    /// Answers one request, from any thread, once.
    pub serve: Arc<dyn Fn(SchemeRequest, Respond) + Send + Sync>,
}

/// A request a page made to the app's scheme.
#[derive(Clone, Debug, PartialEq)]
pub struct SchemeRequest {
    pub method: String,
    /// The site the address names ([`scheme_url`]'s `site`).
    pub site: String,
    /// The address's path after the site's `/`, decoded, without its query.
    pub path: String,
    pub headers: Vec<(String, String)>,
}

impl SchemeRequest {
    /// The request for `url`, an address [`scheme_url`] made, or for the
    /// same address with the scheme's own name in front (`name://site/`),
    /// as the system web view hands it over; `None` for another address.
    pub fn of(name: &str, method: &str, url: &str, headers: Vec<(String, String)>) -> Option<Self> {
        let rest = url
            .strip_prefix(&format!("http://{name}."))
            .or_else(|| url.strip_prefix(&format!("{name}://")))?;
        let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
        let site = host.strip_suffix(".localhost").unwrap_or(host);
        let path = path.split(['?', '#']).next().unwrap_or_default();
        Some(Self {
            method: method.into(),
            site: site.into(),
            path: decode(path)?,
            headers,
        })
    }

    /// The first header named `name` (any case).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// The app's answer to a [`SchemeRequest`].
#[derive(Clone, Debug, PartialEq)]
pub struct SchemeResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl SchemeResponse {
    /// A refusal in words, as plain text.
    pub fn refused(status: u16, why: impl Into<String>) -> Self {
        Self {
            status,
            headers: vec![("Content-Type".into(), "text/plain; charset=utf-8".into())],
            body: why.into().into_bytes(),
        }
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// Sends the answer to one [`SchemeRequest`].
pub type Respond = Box<dyn FnOnce(SchemeResponse) + Send>;

/// The address of `path` (its parts encoded) on the app's scheme `name`
/// at `site`, a name of letters, digits and `-`. Each site is an origin
/// of its own, and one the system web view treats as secure, as it does
/// `localhost`.
pub fn scheme_url(name: &str, site: &str, path: &str) -> String {
    let path: Vec<String> = path.split('/').map(encode).collect();
    format!("http://{name}.{site}.localhost/{}", path.join("/"))
}

fn encode(part: &str) -> String {
    part.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// `%`-escapes decoded, or `None` when they do not make UTF-8.
fn decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// What runs in every page before its own scripts: the page says when it
/// takes the keyboard, and hands back an Escape it leaves unhandled. An
/// Escape is the page's when it prevented the default, or while a dialog
/// of its own is open, or when anything in the page changed while the key
/// was handled (it closed or changed something of its own).
const KIT_SCRIPT: &str = r#"(() => {
  if (window.__ahjoKit) return;
  window.__ahjoKit = true;
  const post = (what) => window.ipc && window.ipc.postMessage("ahjo-kit:" + what);
  addEventListener("focus", () => post("focus"));
  addEventListener("blur", () => post("blur"));
  addEventListener("pointerdown", () => post("focus"), true);
  addEventListener("keydown", (e) => {
    if (e.key !== "Escape" || e.repeat) return;
    const open = document.querySelector("dialog[open], [aria-modal='true']") || document.fullscreenElement;
    const seen = new MutationObserver(() => {});
    seen.observe(document, { subtree: true, childList: true, attributes: true, characterData: true });
    setTimeout(() => {
      const changed = seen.takeRecords().length > 0;
      seen.disconnect();
      if (!e.defaultPrevented && !open && !changed) post("escape");
    }, 0);
  }, true);
})();"#;

/// What a page says to the kit, apart from its owner's messages.
const KIT_MESSAGE: &str = "ahjo-kit:";

/// A web view that opens the first time its surface is painted, and lies
/// over the surface's bounds while it is painted and nothing on the stack
/// covers it. Clones share the one view.
#[derive(Clone, Default)]
pub struct WebSurface {
    inner: Rc<RefCell<Inner>>,
}

#[derive(Default)]
struct Inner {
    slot: Slot,
    /// Where it was painted since the last [`settle`]: its url, bounds and
    /// the window's zoom then.
    painted: Option<(String, Bounds<Pixels>, f32)>,
    window: Option<WindowId>,
}

#[derive(Default)]
enum Slot {
    #[default]
    Closed,
    Opening,
    Open(imp::View),
    Failed(Error),
}

thread_local! {
    /// Every surface painted once and not dropped, for [`settle`].
    static LIVE: RefCell<Vec<Weak<RefCell<Inner>>>> = const { RefCell::new(Vec::new()) };
}

impl WebSurface {
    /// The surface showing `url`, filling its parent. Its first paint starts
    /// opening the view with what `setup` gives. When the view did not
    /// open, [`Self::failed`] says why and the surface draws nothing.
    ///
    /// The view opens in a task of its own: the system web view's creation
    /// runs the window's messages until it is done, and the app's other
    /// tasks among them, so it must not run while a paint holds the app.
    pub fn element(
        &self,
        url: impl Into<String>,
        setup: impl FnOnce() -> WebViewSetup + 'static,
    ) -> impl IntoElement {
        let inner = self.inner.clone();
        let url = url.into();
        canvas(
            |_, _, _| {},
            move |bounds, _, window, cx| {
                let mut me = inner.borrow_mut();
                if me.window.is_none() {
                    me.window = Some(window.window_handle().window_id());
                    LIVE.with(|live| live.borrow_mut().push(Rc::downgrade(&inner)));
                }
                me.painted = Some((url.clone(), bounds, window.ui_scale()));
                if !matches!(me.slot, Slot::Closed) {
                    return;
                }
                me.slot = Slot::Opening;
                drop(me);
                let parent = imp::Parent::of(window);
                let (opening, focus, heard) = Opening::of(url.clone(), setup());
                let inner = Rc::downgrade(&inner);
                window
                    .spawn(cx, async move |cx| {
                        let opened = parent.and_then(|p| imp::View::open(p, opening));
                        let Some(me) = inner.upgrade() else {
                            return;
                        };
                        me.borrow_mut().slot = match opened {
                            Ok(view) => Slot::Open(view),
                            Err(e) => {
                                log::error!("{e}");
                                Slot::Failed(e)
                            }
                        };
                        drop(me);
                        // The next paint shows it, or its owner says why it
                        // did not open.
                        let _ = cx.update(|window, _| window.refresh());
                        // What the page says to the app, while the view lives.
                        while let Ok(what) = heard.recv().await {
                            let Some(me) = inner.upgrade() else {
                                return;
                            };
                            let _ =
                                cx.update(|window, cx| hear(&me, what, focus.as_ref(), window, cx));
                        }
                    })
                    .detach();
            },
        )
        .size_full()
    }

    /// Why the view did not open, once it tried and failed.
    pub fn failed(&self) -> Option<Error> {
        match &self.inner.borrow().slot {
            Slot::Failed(e) => Some(e.clone()),
            _ => None,
        }
    }

    /// Runs `script` in the page as it is now, when the view is open.
    pub fn run(&self, script: &str) {
        if let Slot::Open(view) = &self.inner.borrow().slot {
            view.run(script);
        }
    }
}

/// What a view opens with, as the system's view takes it: the kit's
/// script first, and the page's messages sorted into the kit's and its
/// owner's.
#[cfg_attr(
    not(windows),
    expect(dead_code, reason = "only Windows has a system view to open with it")
)]
struct Opening {
    url: String,
    data_dir: Option<PathBuf>,
    /// Each script, and whether it runs in the main frame only.
    scripts: Vec<(String, bool)>,
    on_message: Box<dyn Fn(String)>,
    scheme: Option<Scheme>,
}

impl Opening {
    fn of(
        url: String,
        setup: WebViewSetup,
    ) -> (Self, Option<FocusHandle>, async_channel::Receiver<Said>) {
        let (said, heard) = async_channel::unbounded::<Said>();
        let owner = setup.on_message;
        let mut scripts = vec![(KIT_SCRIPT.to_string(), true)];
        scripts.extend(setup.script.map(|s| (s, false)));
        let opening = Self {
            url,
            data_dir: setup.data_dir,
            scripts,
            on_message: Box::new(move |message| match Said::of(&message) {
                Some(what) => {
                    let _ = said.try_send(what);
                }
                None => {
                    if let Some(owner) = &owner {
                        owner(message)
                    }
                }
            }),
            scheme: setup.scheme,
        };
        (opening, setup.focus, heard)
    }
}

/// What a page said to the kit.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Said {
    /// It took the keyboard.
    Focus,
    /// It let the keyboard go.
    Blur,
    /// It left an Escape unhandled.
    Escape,
}

impl Said {
    fn of(message: &str) -> Option<Self> {
        Some(match message.strip_prefix(KIT_MESSAGE)? {
            "focus" => Said::Focus,
            "blur" => Said::Blur,
            "escape" => Said::Escape,
            _ => return None,
        })
    }
}

/// The page took the keyboard: the app's keys go to the owner's stand-in.
/// It left an Escape: the keyboard comes back to the app's window, and the
/// Escape is pressed there, where the stand-in has it, so it closes what
/// any Escape there closes, once.
fn hear(
    me: &Rc<RefCell<Inner>>,
    what: Said,
    focus: Option<&FocusHandle>,
    window: &mut Window,
    cx: &mut App,
) {
    match what {
        Said::Focus => {
            if let Slot::Open(view) = &mut me.borrow_mut().slot {
                view.set_keyboard(true);
            }
            if let Some(focus) = focus {
                window.focus(focus, cx);
            }
        }
        Said::Blur => {
            if let Slot::Open(view) = &mut me.borrow_mut().slot {
                view.set_keyboard(false);
            }
        }
        Said::Escape => {
            if let Slot::Open(view) = &mut me.borrow_mut().slot {
                view.give_back();
            }
            if let Some(focus) = focus {
                window.focus(focus, cx);
            }
            if let Ok(escape) = Keystroke::parse("escape") {
                window.dispatch_keystroke(escape, cx);
            }
        }
    }
}

/// Once a frame, after everything else is drawn: each surface of this
/// window painted this frame lies over its bounds unless one of `covers`
/// (what the stack drew) overlaps them; the others hide.
pub fn settle(covers: &[Bounds<Pixels>], window: &mut Window) {
    let here = window.window_handle().window_id();
    let live: Vec<Rc<RefCell<Inner>>> = LIVE.with(|live| {
        let mut live = live.borrow_mut();
        live.retain(|s| s.strong_count() > 0);
        live.iter().filter_map(Weak::upgrade).collect()
    });
    for surface in live {
        let mut me = surface.borrow_mut();
        if me.window != Some(here) {
            continue;
        }
        let painted = me.painted.take();
        let Slot::Open(view) = &mut me.slot else {
            continue;
        };
        match painted {
            Some((url, bounds, ui)) if !covers.iter().any(|c| c.intersects(&bounds)) => {
                view.show(&url, bounds, ui)
            }
            _ => view.hide(),
        }
    }
}

/// A press anywhere the app draws takes the keyboard back from a page that
/// has it: a child window keeps the keyboard through a click on its parent
/// otherwise.
pub fn pressed(window: &Window) {
    let here = window.window_handle().window_id();
    LIVE.with(|live| {
        for surface in live.borrow().iter().filter_map(Weak::upgrade) {
            let mut me = surface.borrow_mut();
            if me.window != Some(here) {
                continue;
            }
            if let Slot::Open(view) = &mut me.slot {
                view.give_back();
            }
        }
    });
}

#[cfg(windows)]
mod imp {
    use super::{Opening, SchemeRequest, SchemeResponse};
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
        /// The page has the keyboard, as it last said.
        has_keyboard: bool,
    }

    impl View {
        pub fn open(parent: Parent, opening: Opening) -> Result<Self, Error> {
            let mut context = WebContext::new(opening.data_dir);
            let mut builder = WebViewBuilder::new_with_web_context(&mut context)
                .with_url(&opening.url)
                .with_visible(false);
            for (script, main_only) in &opening.scripts {
                builder = builder.with_initialization_script_for_main_only(script, *main_only);
            }
            let on_message = opening.on_message;
            builder = builder.with_ipc_handler(move |req| on_message(req.into_body()));
            if let Some(scheme) = opening.scheme {
                let name = scheme.name;
                builder = builder.with_asynchronous_custom_protocol(
                    name.into(),
                    move |_, request, responder| {
                        let headers = request
                            .headers()
                            .iter()
                            .filter_map(|(k, v)| {
                                Some((k.to_string(), v.to_str().ok()?.to_string()))
                            })
                            .collect();
                        let url = request.uri().to_string();
                        let Some(asked) =
                            SchemeRequest::of(name, request.method().as_str(), &url, headers)
                        else {
                            return responder.respond(answer(SchemeResponse::refused(
                                400,
                                "not an address of this scheme",
                            )));
                        };
                        (scheme.serve)(
                            asked,
                            Box::new(move |response| responder.respond(answer(response))),
                        );
                    },
                );
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
                url: opening.url,
                at: None,
                shown: false,
                has_keyboard: false,
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
                // A hidden page cannot keep the keyboard.
                self.give_back();
                self.shown = false;
                if let Err(e) = self.view.set_visible(false) {
                    log::warn!("the web view did not hide: {e}");
                }
            }
        }

        /// What the page last said of the keyboard.
        pub fn set_keyboard(&mut self, has: bool) {
            self.has_keyboard = has;
        }

        /// The keyboard goes back to the app's window, when the page has it.
        pub fn give_back(&mut self) {
            if std::mem::take(&mut self.has_keyboard)
                && let Err(e) = self.view.focus_parent()
            {
                log::warn!("the web view did not give the keyboard back: {e}");
            }
        }

        pub fn run(&self, script: &str) {
            if let Err(e) = self.view.evaluate_script(script) {
                log::warn!("the web view did not run the app's script: {e}");
            }
        }
    }

    fn answer(response: SchemeResponse) -> wry::http::Response<Vec<u8>> {
        let mut built = wry::http::Response::builder().status(response.status);
        for (name, value) in &response.headers {
            built = built.header(name, value);
        }
        built.body(response.body).unwrap_or_else(|e| {
            let mut failed = wry::http::Response::new(e.to_string().into_bytes());
            *failed.status_mut() = wry::http::StatusCode::INTERNAL_SERVER_ERROR;
            failed
        })
    }
}

#[cfg(not(windows))]
mod imp {
    use super::Opening;
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
        pub fn open(_: Parent, _: Opening) -> Result<Self, Error> {
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

        pub fn set_keyboard(&mut self, _: bool) {
            match self.0 {}
        }

        pub fn give_back(&mut self) {
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
            .update(cx, |_, window, _| {
                let e = surface.failed().expect("the view could not open");
                let why = if cfg!(windows) {
                    "web_view_unopened"
                } else {
                    "web_view_unsupported"
                };
                assert_eq!(e.code, why);
                // Settling and running a script without a view do nothing.
                settle(&[], window);
                surface.run("1");
            })
            .unwrap();
    }

    #[test]
    fn a_scheme_address_reads_back_as_its_site_and_path() {
        let url = scheme_url("ahjo-file", "f00d", "my assets/a b+c.css");
        assert_eq!(
            url,
            "http://ahjo-file.f00d.localhost/my%20assets/a%20b%2Bc.css"
        );
        let asked = SchemeRequest::of("ahjo-file", "GET", &format!("{url}?v=1"), vec![]).unwrap();
        assert_eq!(asked.site, "f00d");
        assert_eq!(asked.path, "my assets/a b+c.css");
        // As the system web view hands it over, the scheme's name in front.
        let handed = SchemeRequest::of(
            "ahjo-file",
            "GET",
            "ahjo-file://f00d.localhost/x.js",
            vec![],
        )
        .unwrap();
        assert_eq!(
            (handed.site.as_str(), handed.path.as_str()),
            ("f00d", "x.js")
        );
        assert_eq!(
            SchemeRequest::of("ahjo-file", "GET", "http://elsewhere/x", vec![]),
            None
        );
        assert_eq!(Said::of("ahjo-kit:escape"), Some(Said::Escape));
        assert_eq!(Said::of("{\"mark\":1}"), None);
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
