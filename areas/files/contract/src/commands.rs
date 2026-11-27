//! File command arguments and their answers.

use ahjo_core::TypedCommand;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Diff, Listing, Matches, Progress, Recent, RepoStatus, Root};

/// List the roots or the entries in one folder.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FileList {
    /// Folder to list. Omit it to list the node's roots.
    pub path: Option<String>,
}

/// Read a file and its version. Use --base64 to read bytes in full.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FileRead {
    /// File to read.
    pub path: String,
    /// Return the bytes as base64. The default is a text preview.
    #[serde(default)]
    pub base64: bool,
}

/// Show a file's diff against HEAD.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FileDiff {
    /// File to compare.
    pub path: String,
    /// Include every line of the file around the changes.
    pub whole: Option<bool>,
}

/// List files in the repository that differ from HEAD.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FileListChanges {
    /// Folder or file in the repository.
    pub path: String,
}

/// List what changed most recently under the roots or in one folder.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FileListRecent {
    /// Folder to scope the list to. Its files are listed individually.
    pub path: Option<String>,
}

/// Save text, or create a file from base64 bytes.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FileWrite {
    /// File to write.
    pub path: String,
    /// File contents. With --base64, give the encoded bytes here.
    pub text: String,
    /// Version read before editing text. Refuse a stale edit.
    pub base: Option<String>,
    /// Create a text file only if the name is free.
    pub new: Option<bool>,
    /// Decode the contents from base64 and create a new file.
    #[serde(default)]
    pub base64: bool,
    /// Allow a base64 write to replace an existing file.
    #[serde(default)]
    pub replace: bool,
}

/// Create a folder.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FileCreateFolder {
    /// Folder to create.
    pub path: String,
}

/// Move or rename a file or folder.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FileMove {
    /// File or folder to move.
    pub path: String,
    /// New path, including the new name.
    pub to: String,
}

/// Delete a file or folder.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FileDelete {
    /// File or folder to delete.
    pub path: String,
}

/// Find files by path or words, across the roots or under one folder.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FileSearchPaths {
    /// Path or words to find. With --under, match this text in names and paths.
    pub query: String,
    /// Search only under this folder. Can include hidden folders when named explicitly.
    pub under: Option<String>,
    /// Search across the roots, giving results from this folder priority.
    pub near: Option<String>,
    /// Search next to --near, without walking the roots.
    pub beside: Option<bool>,
}

/// Find text under a folder.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FileSearch {
    /// Folder to search.
    pub path: String,
    /// Text to look for within files.
    pub query: String,
}

/// Copy to another folder on this node.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FileCopy {
    /// File or folder to copy.
    pub path: String,
    /// Folder to copy into.
    pub to: String,
    /// Node to copy to. Omit it to copy on this node.
    pub to_node: Option<String>,
}

/// Resume a failed local copy.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FileResume {
    /// Id of the transfer to resume.
    pub id: String,
}

/// Read local copy progress.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FileShowTransfer {
    /// Id of the transfer to read.
    pub id: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct FileListed {
    pub node: String,
    #[serde(flatten)]
    pub content: ListContent,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum ListContent {
    Roots { roots: Vec<Root> },
    Folder(Listing),
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct FileReadResult {
    pub node: String,
    pub path: String,
    pub size: usize,
    pub version: String,
    #[serde(flatten)]
    pub content: ReadContent,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum ReadContent {
    Text { text: String, truncated: bool },
    Base64 { data: String },
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct FileWritten {
    pub node: String,
    pub path: String,
    pub version: String,
    /// Decoded byte count of a base64 write.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<usize>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct FilePath {
    pub node: String,
    pub path: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct FileDeleted {
    pub node: String,
    pub path: String,
    pub deleted: bool,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct FileSearched {
    pub node: String,
    #[serde(flatten)]
    pub results: Matches,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct FileTransferResult {
    pub node: String,
    pub id: String,
    pub to_node: String,
    pub transfer: Progress,
}

/// Read the modification time of each path under the roots.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FileStat {
    /// Paths to check, in the order to report their times.
    pub paths: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct FileStated {
    /// Unix milliseconds for each path, or null when it is missing or outside the roots.
    pub times: Vec<Option<u64>>,
}

/// The most bytes `file_read_image` gives: what multimodal clients take in one
/// image.
pub const MAX_IMAGE_BYTES: u64 = 5 << 20;

/// The longest side, in pixels, `file_read_image` gives.
pub const MAX_IMAGE_SIDE: u32 = 8000;

/// List the PNG, JPEG and WebP images under a folder, newest first.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FileListImages {
    /// Folder to look in, such as a project's or session's screenshot folder. Its hidden and build folders are skipped unless named.
    pub folder: String,
    /// How many images to answer, newest first. Defaults to 50, at most 200.
    #[schemars(range(min = 1, max = 200))]
    pub limit: Option<usize>,
}

/// Show a PNG, JPEG or WebP image from a folder as an image, with its size.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FileReadImage {
    /// Folder the image is in, as file_list_images was given it.
    pub folder: String,
    /// The image's path from the folder, as file_list_images answers it.
    pub path: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct FileImages {
    pub node: String,
    /// The folder, links resolved.
    pub folder: String,
    pub images: Vec<ImageEntry>,
    /// More images were there, or the walk stopped before seeing them all.
    pub truncated: bool,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ImageEntry {
    /// From the folder, parts joined by `/`.
    pub path: String,
    /// By its name; file_read_image checks the bytes themselves.
    pub mime_type: String,
    pub size: u64,
    /// Milliseconds since the Unix epoch.
    pub modified: u64,
    /// Over the most file_read_image gives.
    pub too_large: bool,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct FileImage {
    pub node: String,
    pub folder: String,
    pub path: String,
    pub mime_type: String,
    pub width: u32,
    pub height: u32,
    pub size: u64,
    /// Milliseconds since the Unix epoch.
    pub modified: u64,
    pub version: String,
    /// The image itself. MCP gives it as image content; the command line's
    /// --json as base64.
    #[schemars(extend("x-ahjo-image" = true))]
    pub image: ImageData,
}

/// An image's bytes as base64, and what they are.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ImageData {
    pub mime_type: String,
    pub data: String,
}

macro_rules! command {
    ($input:ty, $name:literal, $output:ty) => {
        impl TypedCommand for $input {
            const NAME: &'static str = $name;
            type Output = $output;
        }
    };
}

command!(FileList, "file_list", FileListed);
command!(FileRead, "file_read", FileReadResult);
command!(FileDiff, "file_diff", Diff);
command!(FileListChanges, "file_list_changes", RepoStatus);
command!(FileListRecent, "file_list_recent", Recent);
command!(FileWrite, "file_write", FileWritten);
command!(FileCreateFolder, "file_create_folder", FilePath);
command!(FileMove, "file_move", FilePath);
command!(FileDelete, "file_delete", FileDeleted);
command!(FileSearchPaths, "file_search_paths", Matches);
command!(FileSearch, "file_search", FileSearched);
command!(FileShowTransfer, "file_show_transfer", FileTransferResult);
command!(FileStat, "file_stat", FileStated);
command!(FileListImages, "file_list_images", FileImages);
command!(FileReadImage, "file_read_image", FileImage);

impl TypedCommand for FileCopy {
    const NAME: &'static str = "file_copy";
    const TIMEOUT: Option<std::time::Duration> = Some(std::time::Duration::from_secs(70));
    type Output = FileTransferResult;
}

impl TypedCommand for FileResume {
    const NAME: &'static str = "file_resume";
    const TIMEOUT: Option<std::time::Duration> = Some(std::time::Duration::from_secs(70));
    type Output = FileTransferResult;
}
