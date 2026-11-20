//! The Desk's icons: line drawings on a 24 by 24 grid, built into the
//! binary and drawn in whatever colour the text around them has, so a
//! theme colours them as it colours words.

use gpui::{AssetSource, Hsla, SharedString, Svg, prelude::*, px, svg};
use std::borrow::Cow;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Look,
    Pin,
    Box,
    Arrow,
    Pen,
    Note,
    Settings,
    Sidebar,
    Expand,
    Shrink,
    External,
    Refresh,
    Compare,
}

impl Icon {
    pub const ALL: [Icon; 13] = [
        Icon::Look,
        Icon::Pin,
        Icon::Box,
        Icon::Arrow,
        Icon::Pen,
        Icon::Note,
        Icon::Settings,
        Icon::Sidebar,
        Icon::Expand,
        Icon::Shrink,
        Icon::External,
        Icon::Refresh,
        Icon::Compare,
    ];

    pub fn path(self) -> &'static str {
        match self {
            Icon::Look => "icons/look.svg",
            Icon::Pin => "icons/pin.svg",
            Icon::Box => "icons/box.svg",
            Icon::Arrow => "icons/arrow.svg",
            Icon::Pen => "icons/pen.svg",
            Icon::Note => "icons/note.svg",
            Icon::Settings => "icons/settings.svg",
            Icon::Sidebar => "icons/sidebar.svg",
            Icon::Expand => "icons/expand.svg",
            Icon::Shrink => "icons/shrink.svg",
            Icon::External => "icons/external.svg",
            Icon::Refresh => "icons/refresh.svg",
            Icon::Compare => "icons/compare.svg",
        }
    }

    fn bytes(self) -> &'static [u8] {
        match self {
            Icon::Look => include_bytes!("../assets/icons/look.svg"),
            Icon::Pin => include_bytes!("../assets/icons/pin.svg"),
            Icon::Box => include_bytes!("../assets/icons/box.svg"),
            Icon::Arrow => include_bytes!("../assets/icons/arrow.svg"),
            Icon::Pen => include_bytes!("../assets/icons/pen.svg"),
            Icon::Note => include_bytes!("../assets/icons/note.svg"),
            Icon::Settings => include_bytes!("../assets/icons/settings.svg"),
            Icon::Sidebar => include_bytes!("../assets/icons/sidebar.svg"),
            Icon::Expand => include_bytes!("../assets/icons/expand.svg"),
            Icon::Shrink => include_bytes!("../assets/icons/shrink.svg"),
            Icon::External => include_bytes!("../assets/icons/external.svg"),
            Icon::Refresh => include_bytes!("../assets/icons/refresh.svg"),
            Icon::Compare => include_bytes!("../assets/icons/compare.svg"),
        }
    }

    /// The icon `size` square, in `color`.
    pub fn draw(self, size: f32, color: Hsla) -> Svg {
        svg()
            .path(self.path())
            .size(px(size))
            .flex_none()
            .text_color(color)
    }
}

/// Where the window finds the icons.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        Ok(Icon::ALL
            .iter()
            .find(|i| i.path() == path)
            .map(|i| Cow::Borrowed(i.bytes())))
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(Icon::ALL
            .iter()
            .map(|i| i.path())
            .filter(|p| p.starts_with(path))
            .map(SharedString::from)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_is_found_by_its_path() {
        for icon in Icon::ALL {
            let bytes = Assets.load(icon.path()).unwrap().unwrap();
            let text = std::str::from_utf8(&bytes).unwrap();
            assert!(text.starts_with("<svg") && text.contains("viewBox=\"0 0 24 24\""));
        }
        assert_eq!(Assets.list("icons/").unwrap().len(), Icon::ALL.len());
        assert!(Assets.load("icons/none.svg").unwrap().is_none());
    }
}
