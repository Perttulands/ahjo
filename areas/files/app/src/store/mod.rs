//! State and rules for the Files page.
pub mod diff;
pub mod drafts;
pub mod find;
pub mod format;
pub mod kinds;
pub mod listing;
pub mod marks;
pub mod paths;
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
    /// How the page is laid out: being chosen by using each.
    pub layout: Layout,
}

/// The ways the page can be laid out, to compare by using them.
/// Kept by its word; a word not known is the tree.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum Layout {
    /// The list of recent changes and folders beside the file.
    #[default]
    Tree,
    /// The file alone across the page; a find over it, the path above it.
    Reader,
    /// What changed, by the sessions working where it changed, beside the
    /// file with its changes marked.
    Review,
}

impl From<String> for Layout {
    fn from(word: String) -> Self {
        Layout::ALL
            .into_iter()
            .find(|l| l.word().eq_ignore_ascii_case(&word))
            .unwrap_or_default()
    }
}

impl From<Layout> for String {
    fn from(layout: Layout) -> Self {
        layout.word().to_lowercase()
    }
}

impl Layout {
    pub const ALL: [Layout; 3] = [Layout::Tree, Layout::Reader, Layout::Review];
    /// What it is for, in a line.
    pub fn what(self) -> &'static str {
        match self {
            Layout::Tree => "The list of folders beside the file",
            Layout::Reader => "The file alone; Ctrl+P finds another",
            Layout::Review => "What changed, by who works there",
        }
    }
    pub fn word(self) -> &'static str {
        match self {
            Layout::Tree => "Tree",
            Layout::Reader => "Reader",
            Layout::Review => "Review",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_layout_is_kept_by_its_word_and_an_unknown_one_is_the_tree() {
        let kept: Kept = serde_json::from_str(r#"{"layout":"review"}"#).unwrap();
        assert_eq!(kept.layout, Layout::Review);
        let kept: Kept = serde_json::from_str(r#"{"layout":"columns","unwrapped":true}"#).unwrap();
        assert_eq!(kept.layout, Layout::Tree);
        assert!(kept.unwrapped, "the rest still reads");
    }
}
