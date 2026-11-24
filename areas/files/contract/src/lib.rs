//! Wire types for the Files area.

use schemars::JsonSchema;

pub mod commands;

/// HTTP route serving the bytes of a file.
pub const FILE_BYTES_PATH: &str = "/api/files/file";

/// HTTP route reading a file inside an opened folder ([`InFolder`]), whole
/// or one byte range, streamed: what a web view showing a file of that
/// folder loads, through the app. Read-only.
pub const FOLDER_FILE_PATH: &str = "/api/files/folder/file";

/// HTTP route writing an attachment ([`DropAt`]) for an agent on the node
/// to read by the path it answers ([`Dropped`]).
pub const DROPS_PATH: &str = "/api/drops";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq, JsonSchema)]
pub struct Entry {
    pub name: String,
    pub folder: bool,
    pub size: u64,
    /// Milliseconds since the Unix epoch.
    pub modified: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq, JsonSchema)]
pub struct Listing {
    /// The folder's own path, links resolved.
    pub path: String,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq, JsonSchema)]
pub struct Root {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq, JsonSchema)]
pub struct Match {
    pub path: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq, JsonSchema)]
pub struct Matches {
    pub matches: Vec<Match>,
    pub truncated: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq, JsonSchema)]
pub struct Diff {
    pub path: String,
    pub repository: String,
    pub diff: String,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// How a file stands against its repository's HEAD: changed (`M`), new to
/// it (`A`, or `?` when not even staged), taken out (`D`), renamed in
/// (`R`) or in a merge conflict (`C`).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq, JsonSchema)]
pub struct FileState {
    pub path: String,
    pub state: char,
    /// The previous absolute path of a rename.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub renamed_from: Option<String>,
}

/// Every file of a folder's repository that differs from its HEAD. A
/// folder outside a repository has none, and `repository` is empty.
#[derive(
    Clone, Debug, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq, JsonSchema,
)]
pub struct RepoStatus {
    pub repository: String,
    pub files: Vec<FileState>,
    #[serde(default)]
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// A file and when it last changed, in milliseconds since the Unix epoch;
/// or, with `files`, a folder in which that many files changed together
/// (a checkout, a batch of renders), `modified` the newest of them.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq, JsonSchema)]
pub struct Changed {
    pub path: String,
    pub modified: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<usize>,
}

/// The files changed most recently under the roots, newest first.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq, JsonSchema)]
pub struct Recent {
    pub files: Vec<Changed>,
}

/// A copy started on this node.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct TransferRequest {
    #[serde(default)]
    pub node: String,
    pub path: String,
    pub target_node: String,
    pub folder: String,
    pub id: String,
}

/// Progress of a copy started on this node.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq, JsonSchema)]
pub struct Progress {
    pub state: String,
    pub bytes: u64,
    pub total: u64,
    pub error: Option<String>,
}

/// One file or folder on the addressed node.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct At {
    #[serde(default)]
    pub node: String,
    pub path: String,
}

/// A file named from the folder it was opened in: `path` is relative to
/// `folder`, with `/` between its parts, and must stay inside `folder`
/// once every link is followed.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct InFolder {
    #[serde(default)]
    pub node: String,
    pub folder: String,
    pub path: String,
}

/// A save may name the version it was edited from.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct SaveAt {
    #[serde(default)]
    pub node: String,
    pub path: String,
    pub base: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct Job {
    #[serde(default)]
    pub node: String,
    pub id: String,
}

/// An attachment written for a session.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct DropAt {
    pub node: Option<String>,
    pub name: String,
    #[serde(default)]
    pub user: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct Dropped {
    pub path: std::path::PathBuf,
}

pub const OUTSIDE_ROOTS: &str = "outside_roots";
pub const ROOT_IMMUTABLE: &str = "root_immutable";
pub const NOT_FOUND: &str = "not_found";
pub const ALREADY_EXISTS: &str = "already_exists";
pub const FILE_CHANGED: &str = "file_changed";
pub const PERMISSION_DENIED: &str = "permission_denied";
pub const NOT_FOLDER: &str = "not_folder";
pub const NOT_FILE: &str = "not_file";
pub const TOO_LARGE: &str = "too_large";
pub const RANGE_NOT_SATISFIABLE: &str = "range_not_satisfiable";
pub const UNKNOWN_NODE: &str = "unknown_node";
pub const IO_ERROR: &str = "io_error";
