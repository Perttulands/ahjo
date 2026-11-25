//! Files' HTTP routes. Every request names a node and a path on it; this node
//! answers for itself. Paths are the node's own absolute paths.

use std::sync::Arc;

use ahjo_files_contract::{
    At, DropAt, Dropped, FILE_BYTES_PATH, FILE_WHOLE_PATH, FOLDER_FILE_PATH, InFolder, Job, SaveAt,
};
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Query, State};
use axum::http::StatusCode;
use axum::http::header::{
    ACCEPT_RANGES, CACHE_CONTROL, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_SECURITY_POLICY,
    CONTENT_TYPE, HeaderMap, HeaderValue, RANGE, X_CONTENT_TYPE_OPTIONS,
};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};

use super::Files;
use super::disk;
use super::drops::Drops;
use super::error::FilesError;

/// The most one read or save carries.
pub const MAX_BYTES: u64 = 64 << 20;

pub fn routes(files: Arc<Files>, drops: Arc<Drops>) -> Router {
    Router::new()
        .route("/api/files/bridge/start", post(bridge_start))
        .route("/api/files/bridge/events", get(bridge_events))
        .route(FILE_BYTES_PATH, get(read).put(save).post(create_file))
        .route(FILE_WHOLE_PATH, get(read_whole))
        .route(FOLDER_FILE_PATH, get(read_in_folder))
        .layer(DefaultBodyLimit::max(MAX_BYTES as usize))
        .with_state(files)
        .merge(drop_routes(drops))
        .layer(axum::middleware::map_response(coded_rejection))
}

type Shared = State<Arc<Files>>;

async fn bridge_start(
    State(files): Shared,
    Json(start): Json<super::bridge::Start>,
) -> Result<StatusCode, FilesError> {
    super::bridge::start(files, start)?;
    Ok(StatusCode::ACCEPTED)
}

async fn bridge_events(
    State(files): Shared,
    Query(job): Query<Job>,
) -> Result<Response, FilesError> {
    if !job.node.is_empty() && job.node != files.node {
        return Err(FilesError::OtherNode(job.node));
    }
    let receiver = super::bridge::watch(&files, &job.id).ok_or(FilesError::NotFound)?;
    let stream = futures_util::stream::unfold(
        (receiver, true, false),
        |(mut receiver, first, done)| async move {
            if done {
                return None;
            }
            if !first && receiver.changed().await.is_err() {
                return None;
            }
            let value = receiver.borrow_and_update().clone();
            let finished = value.state != "running";
            let text = format!("data: {}\n\n", serde_json::to_string(&value).ok()?);
            Some((
                Ok::<_, std::convert::Infallible>(text),
                (receiver, false, finished),
            ))
        },
    );
    Ok((
        [
            (CONTENT_TYPE, "text/event-stream"),
            (CACHE_CONTROL, "no-store"),
        ],
        axum::body::Body::from_stream(stream),
    )
        .into_response())
}

/// A file's bytes. Served so a page can show a picture, and never run one:
/// the type is only ever an image's or plain bytes, and the policy sandboxes
/// anything opened on its own.
async fn read(State(files): Shared, Query(at): Query<At>) -> Result<Response, FilesError> {
    let kind = content_type(&at.path);
    let bytes = blocking(&files, &at.node, move |f| {
        disk::read(&f.roots, &at.path, MAX_BYTES)
    })
    .await?;
    let headers = [
        (CONTENT_TYPE, HeaderValue::from_static(kind)),
        (CACHE_CONTROL, HeaderValue::from_static("no-store")),
        (CONTENT_SECURITY_POLICY, HeaderValue::from_static("sandbox")),
        (X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff")),
    ];
    Ok((headers, bytes).into_response())
}

/// A file under a root, whole, streamed from the disk as it is sent, so any
/// size serves: for a copy of it to be made elsewhere. Plain bytes, never
/// run.
async fn read_whole(State(files): Shared, Query(at): Query<At>) -> Result<Response, FilesError> {
    let (file, length) = blocking(&files, &at.node, move |f| {
        let path = f.roots.existing(&at.path)?;
        let file = std::fs::File::open(&path)?;
        let meta = file.metadata()?;
        if !meta.is_file() {
            return Err(FilesError::NotFile);
        }
        Ok((file, meta.len()))
    })
    .await?;
    let headers = [
        (
            CONTENT_TYPE,
            HeaderValue::from_static("application/octet-stream"),
        ),
        (CACHE_CONTROL, HeaderValue::from_static("no-store")),
        (CONTENT_SECURITY_POLICY, HeaderValue::from_static("sandbox")),
        (X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff")),
        (CONTENT_LENGTH, header(&length.to_string())),
    ];
    let body = axum::body::Body::from_stream(stream_from(file, 0, length));
    Ok((headers, body).into_response())
}

/// A file of an opened folder, whole or the one byte range asked for,
/// streamed from the disk as it is sent: never read whole, so any size
/// serves. Typed by its name, as a browser needs it to run a script or play
/// a video. Opened on the node's own address it still runs nothing (the
/// policy sandboxes it); the app serves it to its web view under its own
/// scheme.
async fn read_in_folder(
    State(files): Shared,
    Query(at): Query<InFolder>,
    headers: HeaderMap,
) -> Result<Response, FilesError> {
    let kind = served_type(&at.path);
    let (file, length) = blocking(&files, &at.node, move |f| {
        let path = f.roots.in_folder(&at.folder, &at.path)?;
        let file = std::fs::File::open(&path)?;
        let meta = file.metadata()?;
        if !meta.is_file() {
            return Err(FilesError::NotFile);
        }
        Ok((file, meta.len()))
    })
    .await?;
    let asked = headers.get(RANGE).and_then(|v| v.to_str().ok());
    let mut out = HeaderMap::new();
    out.insert(CONTENT_TYPE, HeaderValue::from_static(kind));
    out.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    out.insert(CONTENT_SECURITY_POLICY, HeaderValue::from_static("sandbox"));
    out.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    out.insert(ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    let (status, start, end) = match asked.map(|a| byte_range(a, length)) {
        None | Some(Range::Whole) => (StatusCode::OK, 0, length),
        Some(Range::Part(start, end)) => {
            let shown = format!("bytes {start}-{}/{length}", end - 1);
            out.insert(CONTENT_RANGE, header(&shown));
            (StatusCode::PARTIAL_CONTENT, start, end)
        }
        Some(Range::Unsatisfiable) => {
            let mut refused = ahjo_node_kit::serve::Failure(
                StatusCode::RANGE_NOT_SATISFIABLE,
                ahjo_core::Error::new(
                    ahjo_files_contract::RANGE_NOT_SATISFIABLE,
                    format!("No bytes there: the file has {length}"),
                ),
            )
            .into_response();
            refused
                .headers_mut()
                .insert(CONTENT_RANGE, header(&format!("bytes */{length}")));
            return Ok(refused);
        }
    };
    out.insert(CONTENT_LENGTH, header(&(end - start).to_string()));
    let body = axum::body::Body::from_stream(stream_from(file, start, end - start));
    Ok((status, out, body).into_response())
}

fn header(text: &str) -> HeaderValue {
    HeaderValue::from_str(text).expect("digits and words are a header")
}

/// What a `Range` header asks of a file `length` bytes long.
#[derive(Debug, PartialEq)]
enum Range {
    /// No range this route serves (several, or not bytes): the whole file.
    Whole,
    /// Bytes `start..end`.
    Part(u64, u64),
    /// A range wholly past the end, or of nothing.
    Unsatisfiable,
}

/// One `bytes=` range: `a-b`, `a-` or the last `n` bytes, `-n`. An end
/// past the file is the file's end.
fn byte_range(asked: &str, length: u64) -> Range {
    let Some(spec) = asked.trim().strip_prefix("bytes=") else {
        return Range::Whole;
    };
    if spec.contains(',') {
        return Range::Whole;
    }
    let Some((first, last)) = spec.trim().split_once('-') else {
        return Range::Whole;
    };
    let (first, last) = (first.trim(), last.trim());
    let parsed = |n: &str| n.parse::<u64>().ok();
    let (start, end) = match (first.is_empty(), last.is_empty()) {
        (true, true) => return Range::Whole,
        (true, false) => match parsed(last) {
            Some(0) => return Range::Unsatisfiable,
            Some(n) => (length.saturating_sub(n), length),
            None => return Range::Whole,
        },
        (false, _) => {
            let Some(start) = parsed(first) else {
                return Range::Whole;
            };
            let end = if last.is_empty() {
                length
            } else {
                match parsed(last) {
                    Some(last) if last >= start => last.saturating_add(1).min(length),
                    _ => return Range::Whole,
                }
            };
            (start, end)
        }
    };
    if start >= length {
        Range::Unsatisfiable
    } else {
        Range::Part(start, end)
    }
}

/// `count` bytes of `file` from `start`, read a piece at a time as the
/// body is sent.
fn stream_from(
    file: std::fs::File,
    start: u64,
    count: u64,
) -> impl futures_util::Stream<Item = std::io::Result<Vec<u8>>> + Send {
    use tokio::io::{AsyncReadExt, AsyncSeekExt};
    const PIECE: u64 = 256 << 10;
    let file = tokio::fs::File::from_std(file);
    futures_util::stream::unfold(
        (file, count, false),
        move |(mut file, left, sought)| async move {
            if left == 0 {
                return None;
            }
            if !sought && let Err(e) = file.seek(std::io::SeekFrom::Start(start)).await {
                return Some((Err(e), (file, 0, true)));
            }
            let mut piece = vec![0; PIECE.min(left) as usize];
            match file.read(&mut piece).await {
                // The file shrank since it was measured: what was promised
                // cannot be sent, so the body ends short and the client
                // sees it fail.
                Ok(0) => Some((
                    Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "the file shrank while it was read",
                    )),
                    (file, 0, true),
                )),
                Ok(n) => {
                    piece.truncate(n);
                    Some((Ok(piece), (file, left - n as u64, true)))
                }
                Err(e) => Some((Err(e), (file, 0, true))),
            }
        },
    )
}

async fn save(
    State(files): Shared,
    Query(at): Query<SaveAt>,
    body: Bytes,
) -> Result<StatusCode, FilesError> {
    blocking(&files, &at.node, move |f| {
        disk::save(&f.roots, &at.path, &body, at.base.as_deref())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// A new file, empty or with the body's bytes; never over an existing one.
async fn create_file(
    State(files): Shared,
    Query(at): Query<At>,
    body: Bytes,
) -> Result<StatusCode, FilesError> {
    blocking(&files, &at.node, move |f| {
        disk::upload(&f.roots, &at.path, &body)
    })
    .await?;
    Ok(StatusCode::CREATED)
}

/// Runs a disk operation off the async threads, once the request is known to
/// be for this node.
async fn blocking<T: Send + 'static>(
    files: &Arc<Files>,
    node: &str,
    work: impl FnOnce(&Files) -> Result<T, FilesError> + Send + 'static,
) -> Result<T, FilesError> {
    if !node.is_empty() && node != files.node {
        return Err(FilesError::OtherNode(node.to_owned()));
    }
    let files = files.clone();
    tokio::task::spawn_blocking(move || work(&files))
        .await
        .map_err(|error| FilesError::Io(std::io::Error::other(error)))?
}

/// What a browser needs to know a file of a page as: a page, a script
/// (modules too), a style, media, a document or a font; else bytes.
fn served_type(path: &str) -> &'static str {
    let extension = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    match extension.as_deref() {
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("js" | "mjs" | "cjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json" | "map") => "application/json",
        Some("wasm") => "application/wasm",
        Some("txt" | "md" | "csv") => "text/plain; charset=utf-8",
        Some("xml") => "application/xml",
        Some("mp4" | "m4v") => "video/mp4",
        Some("webm") => "video/webm",
        Some("ogv") => "video/ogg",
        Some("mov") => "video/quicktime",
        Some("mp3") => "audio/mpeg",
        Some("m4a" | "aac") => "audio/mp4",
        Some("wav") => "audio/wav",
        Some("ogg" | "oga" | "opus") => "audio/ogg",
        Some("flac") => "audio/flac",
        Some("pdf") => "application/pdf",
        Some("glb") => "model/gltf-binary",
        Some("gltf") => "model/gltf+json",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("otf") => "font/otf",
        _ => content_type(path),
    }
}

/// The type a picture needs to show in an `img`; everything else is bytes.
fn content_type(path: &str) -> &'static str {
    let extension = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    match extension.as_deref() {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("avif") => "image/avif",
        Some("bmp") => "image/bmp",
        Some("ico") => "image/x-icon",
        Some("svg") => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

impl IntoResponse for FilesError {
    fn into_response(self) -> Response {
        let error = ahjo_core::Error::from(self);
        let status =
            super::error::failure_status(&error).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        ahjo_node_kit::serve::Failure(status, error).into_response()
    }
}

fn drop_routes(drops: Arc<Drops>) -> Router {
    Router::new()
        .route(ahjo_files_contract::DROPS_PATH, post(drop_file))
        .layer(DefaultBodyLimit::max(super::drops::MAX_BYTES))
        .with_state(drops)
}

async fn drop_file(
    State(drops): State<Arc<Drops>>,
    Query(at): Query<DropAt>,
    body: Bytes,
) -> Response {
    let result = tokio::task::spawn_blocking(move || drops.write(&at.name, &at.user, &body)).await;
    match result {
        Ok(Ok(path)) => (StatusCode::CREATED, Json(Dropped { path })).into_response(),
        Ok(Err(error)) => ahjo_node_kit::serve::Failure(
            StatusCode::BAD_REQUEST,
            ahjo_core::Error::new("drop_failed", format!("{error:#}")),
        )
        .into_response(),
        Err(error) => ahjo_node_kit::serve::Failure(
            StatusCode::INTERNAL_SERVER_ERROR,
            ahjo_core::Error::new("io_error", error.to_string()),
        )
        .into_response(),
    }
}

/// Axum's extractor failures also use the node's coded error envelope.
async fn coded_rejection(response: Response) -> Response {
    let status = response.status();
    if !status.is_client_error()
        || response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("application/json"))
    {
        return response;
    }
    let message = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_else(|_| "Request could not be read".into());
    let code = if status == StatusCode::PAYLOAD_TOO_LARGE {
        "too_large"
    } else {
        "bad_request"
    };
    ahjo_node_kit::serve::Failure(status, ahjo_core::Error::new(code, message)).into_response()
}

#[cfg(test)]
mod tests {
    use super::{Range, byte_range};

    #[test]
    fn a_range_header_reads_as_one_range_or_the_whole() {
        assert_eq!(byte_range("bytes=0-9", 100), Range::Part(0, 10));
        assert_eq!(byte_range("bytes=90-", 100), Range::Part(90, 100));
        assert_eq!(byte_range("bytes=90-500", 100), Range::Part(90, 100));
        assert_eq!(byte_range("bytes=-10", 100), Range::Part(90, 100));
        assert_eq!(byte_range("bytes=-500", 100), Range::Part(0, 100));
        assert_eq!(byte_range("bytes=100-", 100), Range::Unsatisfiable);
        assert_eq!(byte_range("bytes=-0", 100), Range::Unsatisfiable);
        assert_eq!(byte_range("bytes=0-", 0), Range::Unsatisfiable);
        assert_eq!(byte_range("bytes=0-1,5-6", 100), Range::Whole);
        assert_eq!(byte_range("bytes=9-3", 100), Range::Whole);
        assert_eq!(byte_range("lines=1-2", 100), Range::Whole);
    }
}
