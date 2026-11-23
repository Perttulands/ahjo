//! A page an agent published (an HTML prototype), rendered inside the
//! window and marked element by element.
//!
//! On Windows the page is a WebView2 view (through `wry`) laid over the
//! surface as a child of the app's window, with [`LAYER`] in it: in Comment
//! mode a click marks the element under it and the page tells the app
//! ([`FromPage`]); the app draws its marks back over the page
//! ([`show_script`]). Elsewhere there is no system web view the app can lay
//! inside its window (on Linux one needs GTK's loop, which the window does
//! not run), so the page opens in the browser and is answered in words.

use crate::model::Mark;
use serde::Deserialize;
use serde_json::json;

/// The script every page gets before its own.
pub const LAYER: &str = include_str!("page.js");

/// What the page tells the app.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum FromPage {
    /// The page has loaded: it wants its marks and the mode.
    Ready,
    /// He clicked an element in Comment mode.
    Mark {
        selector: String,
        #[serde(default)]
        quote: String,
        x: f64,
        y: f64,
    },
}

pub fn parse(message: &str) -> Option<FromPage> {
    serde_json::from_str(message).ok()
}

/// The script that draws `marks` (with their numbers in the list) on the
/// page, `selected` outlined, and sets the mode.
pub fn show_script(marks: &[(usize, &Mark)], selected: Option<&str>, comment: bool) -> String {
    let list: Vec<_> = marks
        .iter()
        .map(|(n, m)| {
            let p = m.points.first().copied().unwrap_or([0., 0.]);
            json!({"id": m.id, "n": n, "selector": m.selector, "x": p[0], "y": p[1], "color": m.color})
        })
        .collect();
    format!(
        "window.__desk && (__desk.mode({}), __desk.show({}, {}));",
        json!(if comment { "comment" } else { "browse" }),
        serde_json::Value::Array(list),
        json!(selected)
    )
}

/// Whether this system can show a page inside the window.
pub const INSIDE: bool = cfg!(windows);

#[cfg(windows)]
pub use inside::WebPage;

#[cfg(windows)]
mod inside {
    use gpui::{Bounds, Pixels, Window};
    use wry::dpi::{LogicalPosition, LogicalSize};
    use wry::{Rect, WebContext, WebView, WebViewBuilder};

    /// The page view laid over the surface. It is a child window: it draws
    /// above everything the app draws where it lies, so the app keeps it
    /// to the surface's bounds and hides it when the surface shows
    /// something else.
    pub struct WebPage {
        view: WebView,
        _context: WebContext,
        url: String,
        at: Option<Bounds<Pixels>>,
        shown: bool,
        /// The window's zoom, set before the view opens: the app's pixels
        /// are this many of the system's.
        ui: f32,
    }

    impl WebPage {
        /// A view in `window` loading `url`; each message the page posts
        /// goes to `message`.
        pub fn new(
            window: &Window,
            url: &str,
            message: impl Fn(String) + 'static,
        ) -> Result<Self, ahjo_app_kit::Error> {
            let dir = std::env::var_os("LOCALAPPDATA")
                .map(|d| std::path::PathBuf::from(d).join("Ahjo").join("desk-web"));
            let mut context = WebContext::new(dir);
            let view = WebViewBuilder::new_with_web_context(&mut context)
                .with_url(url)
                .with_initialization_script(super::LAYER)
                .with_ipc_handler(move |req| message(req.into_body()))
                .with_visible(false)
                .build_as_child(window)
                .map_err(|e| {
                    ahjo_app_kit::Error::new(
                        "page_view_unopened",
                        format!("the page view did not open: {e}"),
                    )
                })?;
            Ok(Self {
                view,
                _context: context,
                url: url.into(),
                at: None,
                shown: false,
                ui: window.ui_scale(),
            })
        }

        /// Shows `url` (loading it when it is another page) over `bounds`.
        pub fn show(&mut self, url: &str, bounds: Bounds<Pixels>) {
            if self.url != url {
                self.url = url.into();
                if let Err(e) = self.view.load_url(url) {
                    log::warn!("the page did not load: {e}");
                }
            }
            if self.at != Some(bounds) {
                self.at = Some(bounds);
                let ui = self.ui;
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
                    log::warn!("the page did not move: {e}");
                }
            }
            if !self.shown {
                self.shown = true;
                let _ = self.view.set_visible(true);
            }
        }

        pub fn hide(&mut self) {
            if self.shown {
                self.shown = false;
                let _ = self.view.set_visible(false);
            }
        }

        pub fn run(&self, script: &str) {
            if let Err(e) = self.view.evaluate_script(script) {
                log::warn!("the page did not take the app's marks: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_says_ready_and_what_he_marked() {
        assert_eq!(parse(r#"{"kind":"ready"}"#), Some(FromPage::Ready));
        assert_eq!(
            parse(r#"{"kind":"mark","selector":"main > h2","quote":"Choose","x":0.5,"y":0.25}"#),
            Some(FromPage::Mark {
                selector: "main > h2".into(),
                quote: "Choose".into(),
                x: 0.5,
                y: 0.25
            })
        );
        assert_eq!(parse("not json"), None);
    }

    #[test]
    fn the_marks_go_to_the_page_with_their_numbers() {
        let m = crate::marks::on_page("d1".into(), "m4", "main > h2", "Choose", 0.5, 0.25);
        let s = show_script(&[(3, &m)], Some("d1"), true);
        assert!(s.contains(r#"__desk.mode("comment")"#), "{s}");
        assert!(
            s.contains(r#""n":3"#) && s.contains(r#""selector":"main > h2""#),
            "{s}"
        );
        assert!(s.ends_with(r#", "d1"));"#), "{s}");
    }
}
