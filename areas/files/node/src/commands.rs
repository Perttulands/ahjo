//! File commands, used by both the CLI and MCP.

use crate::{Files, FilesError};
use ahjo_core::{Error, TypedCommand};
use ahjo_files_contract::{
    Diff, IO_ERROR, Matches, NOT_FOUND, RepoStatus,
    commands::{
        FileCopy, FileCreateFolder, FileDelete, FileDeleted, FileDiff, FileList, FileListChanges,
        FileListRecent, FileListed, FileMove, FilePath, FileRead, FileReadResult, FileResume,
        FileSearch, FileSearchPaths, FileSearched, FileShowTransfer, FileStat, FileStated,
        FileTransferResult, FileWrite, FileWritten, ListContent, ReadContent,
    },
};
use ahjo_node_kit::{Command, Effect};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use schemars::JsonSchema;
use std::{path::Path, sync::Arc, time::Duration};

fn bad(message: impl Into<String>) -> Error {
    Error::new("bad_request", message)
}

fn path(files: &Files, raw: &str) -> Result<String, Error> {
    let path = Path::new(raw);
    match path.strip_prefix("~") {
        Ok(rest) => files
            .home
            .as_ref()
            .ok_or_else(|| bad("home is unavailable"))?
            .join(rest)
            .into_os_string()
            .into_string()
            .map_err(|_| bad("path is not UTF-8")),
        Err(_) => Ok(raw.to_owned()),
    }
}

fn blocking<Input, F>(files: &Arc<Files>, effect: Effect, work: F) -> Command
where
    Input: TypedCommand + JsonSchema + Send,
    Input::Output: JsonSchema,
    F: Fn(&Files, Input) -> Result<Input::Output, Error> + Copy + Send + Sync + 'static,
{
    let files = files.clone();
    Command::for_request(effect, move |args| {
        let files = files.clone();
        async move {
            tokio::task::spawn_blocking(move || work(&files, args))
                .await
                .map_err(|error| Error::new(IO_ERROR, error.to_string()))?
        }
    })
}

pub fn commands(files: Arc<Files>) -> Vec<Command> {
    let copying = files.clone();
    let resuming = files.clone();
    let tracking = files.clone();
    vec![
        blocking(&files, Effect::Reads, list),
        blocking(&files, Effect::Reads, read),
        blocking(&files, Effect::Reads, |files, args: FileDiff| {
            on_path(files, &args.path, |at| {
                Ok::<Diff, Error>(files.diff(at, args.whole.unwrap_or_default())?)
            })
        }),
        blocking(&files, Effect::Reads, |files, args: FileListChanges| {
            on_path(files, &args.path, |at| {
                Ok::<RepoStatus, Error>(files.status(at)?)
            })
        }),
        blocking(&files, Effect::Reads, |files, args: FileListRecent| {
            let under = args
                .path
                .as_deref()
                .map(|raw| path(files, raw))
                .transpose()?;
            Ok(files.recent(under.as_deref())?)
        }),
        blocking(&files, Effect::Destroys, write),
        blocking(&files, Effect::Sets, |files, args: FileCreateFolder| {
            on_path(files, &args.path, |at| {
                files.create_folder(at)?;
                Ok(FilePath {
                    node: files.node.clone(),
                    path: at.into(),
                })
            })
        }),
        blocking(&files, Effect::Writes, |files, args: FileMove| {
            on_path(files, &args.path, |at| {
                let to = path(files, &args.to)?;
                files.rename(at, &to)?;
                Ok(FilePath {
                    node: files.node.clone(),
                    path: to,
                })
            })
        }),
        blocking(&files, Effect::Destroys, |files, args: FileDelete| {
            on_path(files, &args.path, |at| {
                files.delete(at)?;
                Ok(FileDeleted {
                    node: files.node.clone(),
                    path: at.into(),
                    deleted: true,
                })
            })
        }),
        blocking(&files, Effect::Reads, locate),
        blocking(&files, Effect::Reads, |files, args: FileSearch| {
            on_path(files, &args.path, |at| {
                Ok(FileSearched {
                    node: files.node.clone(),
                    results: files.find(at, &args.query, true)?,
                })
            })
        }),
        blocking(&files, Effect::Reads, |files, args: FileStat| {
            Ok(FileStated {
                times: files.stat(&args.paths),
            })
        }),
        Command::for_request(Effect::Writes, move |args: FileCopy| {
            let files = copying.clone();
            async move {
                let from = path(&files, &args.path)?;
                let to = path(&files, &args.to)?;
                let target = args.to_node.as_deref().unwrap_or(&files.node);
                let id = files.copy(&from, target, &to)?;
                wait(&files, id).await
            }
        }),
        Command::for_request(Effect::Writes, move |args: FileResume| {
            let files = resuming.clone();
            async move {
                files.resume(&args.id).map_err(|error| match error {
                    FilesError::NotFound => {
                        Error::new(NOT_FOUND, format!("no transfer {} on this node", args.id))
                    }
                    other => Error::from(other),
                })?;
                wait(&files, args.id).await
            }
        }),
        Command::for_request(Effect::Reads, move |args: FileShowTransfer| {
            let files = tracking.clone();
            async move { transfer(&files, args.id) }
        }),
    ]
    .into_iter()
    .map(|command| command.with_error_status(crate::error::failure_status))
    .collect()
}

fn on_path<T>(
    files: &Files,
    raw: &str,
    operation: impl FnOnce(&str) -> Result<T, Error>,
) -> Result<T, Error> {
    let at = path(files, raw)?;
    operation(&at).map_err(|error| {
        let at = Path::new(&at);
        if error.code == NOT_FOUND && !at.exists() {
            Error::new(
                error.code,
                format!(
                    "nothing at {}; ahjo file list {} lists what is there",
                    at.display(),
                    at.parent().unwrap_or(at).display()
                ),
            )
        } else {
            error
        }
    })
}

fn list(files: &Files, args: FileList) -> Result<FileListed, Error> {
    let content = match args.path {
        Some(raw) => on_path(files, &raw, |at| Ok(ListContent::Folder(files.list(at)?)))?,
        None => ListContent::Roots {
            roots: files.roots(),
        },
    };
    Ok(FileListed {
        node: files.node.clone(),
        content,
    })
}

fn read(files: &Files, args: FileRead) -> Result<FileReadResult, Error> {
    on_path(files, &args.path, |at| {
        let (bytes, version) = files.read(at, crate::routes::MAX_BYTES)?;
        let content = if args.base64 {
            ReadContent::Base64 {
                data: STANDARD.encode(&bytes),
            }
        } else {
            let shown = &bytes[..bytes.len().min(256 * 1024)];
            let text = match std::str::from_utf8(shown) {
                Ok(text) => text,
                Err(error) if error.error_len().is_none() => {
                    std::str::from_utf8(&shown[..error.valid_up_to()]).unwrap()
                }
                Err(_) => return Err(bad("file is not text; add --base64 to read its bytes")),
            };
            ReadContent::Text {
                text: text.into(),
                truncated: shown.len() < bytes.len(),
            }
        };
        Ok(FileReadResult {
            node: files.node.clone(),
            path: at.into(),
            size: bytes.len(),
            version,
            content,
        })
    })
}

fn write(files: &Files, args: FileWrite) -> Result<FileWritten, Error> {
    let new = args.new.unwrap_or_default();
    if args.replace && !args.base64 {
        return Err(bad("--replace requires --base64"));
    }
    if args.base64 && (new || args.base.is_some()) {
        return Err(bad("--new and --base are for text writes"));
    }
    on_path(files, &args.path, |at| {
        let (bytes, create) = if args.base64 {
            let bytes = STANDARD
                .decode(&args.text)
                .map_err(|error| bad(error.to_string()))?;
            if bytes.len() > crate::routes::MAX_BYTES as usize {
                return Err(FilesError::TooLarge.into());
            }
            (bytes, !args.replace)
        } else {
            (args.text.into_bytes(), new)
        };
        if create {
            files.create(at, &bytes)?;
        } else {
            files.save(at, &bytes, args.base.as_deref())?;
        }
        Ok(FileWritten {
            node: files.node.clone(),
            path: at.into(),
            version: crate::version(&bytes),
            size: args.base64.then_some(bytes.len()),
        })
    })
}

fn locate(files: &Files, args: FileSearchPaths) -> Result<Matches, Error> {
    let beside = args.beside.unwrap_or_default();
    if let Some(under) = args.under {
        if args.near.is_some() || beside {
            return Err(bad("--under cannot be combined with --near or --beside"));
        }
        return on_path(files, &under, |at| {
            Ok(files.find(at, &args.query, false)?)
        });
    }
    let near = args
        .near
        .as_deref()
        .map(|raw| path(files, raw))
        .transpose()?;
    Ok(files.locate(&args.query, near.as_deref(), beside))
}

/// Copy and resume wait for progress to settle and report a failure with its id.
async fn wait(files: &Files, id: String) -> Result<FileTransferResult, Error> {
    let mut watch = crate::bridge::watch(files, &id).ok_or(FilesError::NotFound)?;
    let _ = tokio::time::timeout(Duration::from_secs(60), async {
        while watch.borrow_and_update().state == "running" {
            if watch.changed().await.is_err() {
                break;
            }
        }
    })
    .await;
    let result = transfer(files, id)?;
    if let Some(error) = &result.transfer.error {
        let id = &result.id;
        return Err(Error::new(
            "copy_failed",
            format!("{error} (transfer {id}: file resume {id} goes on from here)"),
        ));
    }
    Ok(result)
}

fn transfer(files: &Files, id: String) -> Result<FileTransferResult, Error> {
    let progress = files.transfer(&id).ok_or(FilesError::NotFound)?;
    Ok(FileTransferResult {
        node: files.node.clone(),
        id,
        to_node: files.node.clone(),
        transfer: progress,
    })
}
