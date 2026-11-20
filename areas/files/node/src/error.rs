//! What can go wrong with a file, in words the operator reads.

use ahjo_core::Error;
use ahjo_files_contract::{
    ALREADY_EXISTS, FILE_CHANGED, IO_ERROR, NOT_FILE, NOT_FOLDER, NOT_FOUND, OUTSIDE_ROOTS,
    PERMISSION_DENIED, ROOT_IMMUTABLE, TOO_LARGE, UNKNOWN_NODE,
};
use axum::http::StatusCode;
use std::fmt;
use std::io;

#[derive(Debug)]
pub enum FilesError {
    /// The path is not under any of this node's roots, or is not absolute.
    Outside,
    /// A root itself cannot be created, renamed or deleted.
    Root,
    NotFound,
    /// Creating or renaming onto something already there.
    Exists,
    /// A save from a version the file no longer is: it changed since it was
    /// read.
    Changed,
    Permission,
    /// A folder was wanted.
    NotFolder,
    /// A file was wanted.
    NotFile,
    /// Over the most this node reads or writes in one request.
    TooLarge,
    /// The request names another node; reaching it is the serving node's pipe.
    OtherNode(String),
    Io(io::Error),
}

impl From<io::Error> for FilesError {
    fn from(error: io::Error) -> FilesError {
        match error.kind() {
            io::ErrorKind::NotFound => FilesError::NotFound,
            io::ErrorKind::PermissionDenied => FilesError::Permission,
            io::ErrorKind::AlreadyExists => FilesError::Exists,
            io::ErrorKind::NotADirectory => FilesError::NotFolder,
            io::ErrorKind::IsADirectory => FilesError::NotFile,
            _ => FilesError::Io(error),
        }
    }
}

impl std::error::Error for FilesError {}

impl fmt::Display for FilesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FilesError::Outside => write!(f, "Outside this node's roots"),
            FilesError::Root => write!(f, "A root cannot be changed"),
            FilesError::NotFound => write!(f, "Not found"),
            FilesError::Exists => write!(f, "Something already has that name"),
            FilesError::Changed => write!(f, "Changed on the device since it was read"),
            FilesError::Permission => write!(f, "Permission denied"),
            FilesError::NotFolder => write!(f, "Not a folder"),
            FilesError::NotFile => write!(f, "Not a file"),
            FilesError::TooLarge => write!(f, "Too large"),
            FilesError::OtherNode(node) => write!(f, "Node {node} is not reachable here"),
            FilesError::Io(error) => write!(f, "{error}"),
        }
    }
}

impl From<FilesError> for ahjo_core::Error {
    fn from(error: FilesError) -> Self {
        let code = match &error {
            FilesError::Outside => OUTSIDE_ROOTS,
            FilesError::Root => ROOT_IMMUTABLE,
            FilesError::NotFound => NOT_FOUND,
            FilesError::Exists => ALREADY_EXISTS,
            FilesError::Changed => FILE_CHANGED,
            FilesError::Permission => PERMISSION_DENIED,
            FilesError::NotFolder => NOT_FOLDER,
            FilesError::NotFile => NOT_FILE,
            FilesError::TooLarge => TOO_LARGE,
            FilesError::OtherNode(_) => UNKNOWN_NODE,
            FilesError::Io(_) => IO_ERROR,
        };
        Self::new(code, error.to_string())
    }
}

/// The same statuses for file commands and byte routes.
pub(crate) fn failure_status(error: &Error) -> Option<StatusCode> {
    Some(match error.code.as_str() {
        OUTSIDE_ROOTS | ROOT_IMMUTABLE | PERMISSION_DENIED => StatusCode::FORBIDDEN,
        NOT_FOUND => StatusCode::NOT_FOUND,
        UNKNOWN_NODE => StatusCode::MISDIRECTED_REQUEST,
        ALREADY_EXISTS => StatusCode::CONFLICT,
        FILE_CHANGED => StatusCode::PRECONDITION_FAILED,
        NOT_FOLDER | NOT_FILE => StatusCode::BAD_REQUEST,
        TOO_LARGE => StatusCode::PAYLOAD_TOO_LARGE,
        IO_ERROR => StatusCode::INTERNAL_SERVER_ERROR,
        _ => return None,
    })
}
