//! State and rules for the Files page.
pub mod diff;
pub mod drafts;
pub mod find;
pub mod format;
pub mod kinds;
pub mod listing;
pub mod marks;
pub mod paths;
pub mod symbols;
pub mod tabs;
pub mod tree;
pub mod who;
pub mod zoom;

use serde::{Deserialize, Serialize};

/// What Files keeps between app runs.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Kept {
    pub tree: tree::Kept,
    /// The file the page showed last.
    pub file: Option<String>,
    /// The panel beside the terminals is open, and what it shows: its
    /// tree, and the file open in it.
    pub beside_open: bool,
    pub beside: tree::Kept,
    pub beside_file: Option<String>,
    pub download_folder: String,
    /// Long lines run past the edge instead of wrapping.
    pub unwrapped: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zoom: zoom::Level,
    /// What the side of the page shows, or that it is hidden.
    pub side: Side,
    /// The files open in tabs, in their order, to open again.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tabs: Vec<String>,
}

/// What the side of the page shows beside the open files: the folders, or
/// what changed by who works there; or nothing, the files across the page.
/// Kept by its word; a word not known is the folders.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum Side {
    /// Recent changes and the folders.
    #[default]
    Folders,
    /// What changed, grouped by the sessions working where it changed.
    Changes,
    /// No side: the files across the page, Ctrl+P to find another.
    Hidden,
}

impl From<String> for Side {
    fn from(word: String) -> Self {
        Side::ALL
            .into_iter()
            .find(|l| l.word().eq_ignore_ascii_case(&word))
            .unwrap_or_default()
    }
}

impl From<Side> for String {
    fn from(side: Side) -> Self {
        side.word().to_lowercase()
    }
}

impl Side {
    pub const ALL: [Side; 3] = [Side::Folders, Side::Changes, Side::Hidden];
    /// The views the side switches between.
    pub const SHOWN: [Side; 2] = [Side::Folders, Side::Changes];
    /// What it is for, in a line.
    pub fn what(self) -> &'static str {
        match self {
            Side::Folders => "Recent changes and the folders",
            Side::Changes => "What changed, by who works there",
            Side::Hidden => "The files alone; Ctrl+B shows the side",
        }
    }
    pub fn word(self) -> &'static str {
        match self {
            Side::Folders => "Folders",
            Side::Changes => "Changes",
            Side::Hidden => "Hidden",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_side_is_kept_by_its_word_and_an_unknown_one_is_the_folders() {
        let kept: Kept = serde_json::from_str(r#"{"side":"changes"}"#).unwrap();
        assert_eq!(kept.side, Side::Changes);
        let kept: Kept = serde_json::from_str(r#"{"side":"columns","unwrapped":true}"#).unwrap();
        assert_eq!(kept.side, Side::Folders);
        assert!(kept.unwrapped, "the rest still reads");
    }
}
