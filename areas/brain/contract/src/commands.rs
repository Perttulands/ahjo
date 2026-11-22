//! Brain command arguments and their answers.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Collection, Commit, File, Found, Listed, Page, PageRef, Text};
use ahjo_core::TypedCommand;

/// Search the brain for pages.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BrainSearch {
    /// Words to search for.
    pub query: String,
}

/// Read a brain page.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BrainRead {
    /// Page path relative to the corpus.
    pub path: String,
}

/// Save a page as a commit. Pass the page's newest commit hash (the first in brain read's history) as base to guard the edit.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BrainWrite {
    /// Page path relative to the corpus.
    pub path: String,
    /// The page's full text, including its front matter.
    pub text: String,
    /// Commit message. Omit it to use the page's path in the message.
    pub summary: Option<String>,
    /// The page's newest commit hash: the first in brain read's history. An empty string for an uncommitted page.
    pub base: Option<String>,
}

/// List the brain's collections and page counts.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BrainListCollections {}

/// List recent commits, newest first, with their pages.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BrainListChanges {
    /// Maximum commits to read. Defaults to 30 and is capped at 200.
    #[schemars(range(min = 1))]
    pub limit: Option<usize>,
}

/// Send a page reference to a session.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BrainSend {
    /// Page path relative to the corpus.
    pub path: String,
    /// Session id or name. Use user/name for another Unix user's session.
    pub session: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct BrainSearchResults {
    pub node: String,
    pub results: Vec<Found>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct BrainPage {
    pub node: String,
    #[serde(flatten)]
    pub page: Page,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct BrainSaved {
    pub node: String,
    pub commit: Commit,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct BrainCollectionsResult {
    pub node: String,
    pub collections: Vec<Collection>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct BrainChangesResult {
    pub node: String,
    pub changes: Vec<BrainChange>,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct BrainChange {
    #[serde(flatten)]
    pub commit: Commit,
    /// Paths of the pages in this commit, joined by commas.
    pub paths: String,
    pub pages: Vec<PageRef>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct BrainSent {
    pub node: String,
    pub session: String,
    pub reference: String,
    pub sent: bool,
}

/// List the page files of this node's brain.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BrainListFiles {}

/// Read the full text of selected page files.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BrainReadTexts {
    /// Corpus-relative page paths.
    pub paths: Vec<String>,
}

/// List the pages in one collection and their latest changes.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BrainListCollection {
    /// Collection name.
    pub name: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct BrainFilesResult {
    pub node: String,
    pub files: Vec<File>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct BrainTextsResult {
    pub node: String,
    pub texts: Vec<Text>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct BrainCollectionResult {
    pub node: String,
    pub pages: Vec<Listed>,
    pub error: Option<String>,
}

macro_rules! typed_commands {
    ($($input:ty => ($name:literal, $output:ty)),* $(,)?) => {
        $(impl TypedCommand for $input {
            const NAME: &'static str = $name;
            type Output = $output;
        })*
    };
}

typed_commands! {
    BrainSearch => ("brain_search", BrainSearchResults),
    BrainRead => ("brain_read", BrainPage),
    BrainWrite => ("brain_write", BrainSaved),
    BrainListCollections => ("brain_list_collections", BrainCollectionsResult),
    BrainListChanges => ("brain_list_changes", BrainChangesResult),
    BrainSend => ("brain_send", BrainSent),
    BrainListFiles => ("brain_list_files", BrainFilesResult),
    BrainReadTexts => ("brain_read_texts", BrainTextsResult),
    BrainListCollection => ("brain_list_collection", BrainCollectionResult),
}
