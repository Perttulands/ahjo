//! A page an agent published (an HTML prototype), rendered inside the
//! window and marked element by element.
//!
//! The page shows in the kit's web view (`ahjo_app_kit::web_view`, Windows
//! only) with [`LAYER`] in it: in Comment mode a click marks the element
//! under it and the page tells the app ([`FromPage`]); the app draws its
//! marks back over the page ([`show_script`]). Where the kit cannot show a
//! page inside the window, the page opens in the browser and is answered in
//! words.

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
