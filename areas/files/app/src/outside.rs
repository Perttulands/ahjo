//! A node's file copied whole to this computer, for Download and for its own
//! program here to open. The copy is written as it streams from the node
//! ([`FILE_WHOLE_PATH`]), a piece at a time, so a file of any size copies and
//! none sits whole in memory.

use ahjo_app_kit::host::{Reach, Request};
use ahjo_core::NodeId;
use ahjo_files_contract::FILE_WHOLE_PATH;
use std::io::Write;
use std::path::{Path, PathBuf};

/// The words of the action that opens a file in this computer's own
/// program, for it.
pub const OPEN_HERE: &str = if cfg!(windows) {
    "Open in its Windows app"
} else {
    "Open in its own app"
};

/// Copies the file at `path` on `node` to `to`, a file that must not exist
/// yet (a download): the bytes copied, or why not. A failed copy leaves
/// nothing at `to`.
pub fn download(
    reach: &Reach,
    node: &str,
    path: &str,
    to: PathBuf,
) -> impl Future<Output = Result<u64, String>> + 'static {
    run(reach, node, path, move |answer| async move {
        if let Some(folder) = to.parent() {
            std::fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
        }
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&to)
            .map_err(|e| format!("{}: {e}", to.display()))?;
        let copied = write(answer, file, &to).await;
        if copied.is_err() {
            let _ = std::fs::remove_file(&to);
        }
        copied
    })
}

/// How long a copy made for a program is kept: by then the program has
/// long read it, or holds it open and keeps it.
const KEPT: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

/// Copies the file at `path` on `node` into a folder of its own under
/// `under` (this computer's temporary files), for its program to open: the
/// copy's path, or why not. Each open gets a folder no other open shares,
/// so a copy being written or handed over is never touched by another;
/// the folders of opens more than a day old go.
pub fn for_its_app(
    reach: &Reach,
    node: &str,
    path: &str,
    under: PathBuf,
) -> impl Future<Output = Result<PathBuf, String>> + 'static {
    let name = local_name(crate::store::paths::base_name(path));
    run(reach, node, path, move |answer| async move {
        sweep(&under, KEPT);
        let dir = fresh_folder(&under)?;
        let to = dir.join(&name);
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&to)
            .map_err(|e| format!("{}: {e}", to.display()))?;
        match write(answer, file, &to).await {
            Ok(_) => Ok(to),
            Err(e) => {
                let _ = std::fs::remove_dir_all(&dir);
                Err(e)
            }
        }
    })
}

/// A folder under `under` made for this open alone, named for when it was
/// made: one that exists already is another open's, so the next name is
/// tried.
fn fresh_folder(under: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(under).map_err(|e| format!("{}: {e}", under.display()))?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    for n in 0..1000 {
        let dir = under.join(format!("{now}-{n}"));
        match std::fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("{}: {e}", dir.display())),
        }
    }
    Err(format!("{}: no free folder for the copy", under.display()))
}

/// Removes the folders of opens made more than `kept` ago, by the time in
/// their name; what is still in use (a program holding its copy on
/// Windows) stays until a later open.
fn sweep(under: &Path, kept: std::time::Duration) {
    let Ok(folders) = std::fs::read_dir(under) else {
        return;
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    for folder in folders.flatten() {
        let made = folder
            .file_name()
            .to_str()
            .and_then(|n| n.split_once('-'))
            .and_then(|(millis, _)| millis.parse::<u128>().ok());
        if made.is_some_and(|made| now.saturating_sub(made) > kept.as_millis()) {
            let _ = std::fs::remove_dir_all(folder.path());
        }
    }
}

/// A node's file name as one file name this computer (Windows or not)
/// takes as it is, inside the folder it is put in: characters that are a
/// path's or not allowed in a name become `_`, a name Windows keeps for a
/// device gets `_` before it, trailing dots and spaces go, and it is kept
/// short. Its extension, which picks its program, stays.
pub fn local_name(name: &str) -> String {
    const MOST: usize = 150;
    let mut out: String = name
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .collect();
    while out.ends_with(['.', ' ']) {
        out.pop();
    }
    if out.chars().count() > MOST {
        let ext = out
            .rsplit_once('.')
            .map(|(_, ext)| ext)
            .filter(|ext| !ext.is_empty() && ext.chars().count() <= 16)
            .map(|ext| format!(".{ext}"))
            .unwrap_or_default();
        let stem: String = out.chars().take(MOST - ext.chars().count()).collect();
        out = format!("{}{ext}", stem.trim_end_matches(['.', ' ']));
    }
    let stem = out
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end()
        .to_ascii_uppercase();
    let device = matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || (stem.len() == 4
        && (stem.starts_with("COM") || stem.starts_with("LPT"))
        && stem.as_bytes()[3].is_ascii_digit())
        || ["COM", "LPT"].iter().any(|d| {
            stem.strip_prefix(d)
                .is_some_and(|rest| matches!(rest, "\u{b9}" | "\u{b2}" | "\u{b3}"))
        });
    if out.is_empty() {
        return "_file".into();
    }
    if device {
        out.insert(0, '_');
    }
    out
}

/// Reads the file whole from the node and hands the answer to `copy`, all
/// on the link's own threads, where its pieces are read; what `copy`
/// returns comes back to the caller.
fn run<T: Send + 'static, F: Future<Output = Result<T, String>> + Send + 'static>(
    reach: &Reach,
    node: &str,
    path: &str,
    copy: impl FnOnce(ahjo_app_kit::link::http::Streamed) -> F + Send + 'static,
) -> impl Future<Output = Result<T, String>> + 'static {
    let uri = format!("{FILE_WHOLE_PATH}?path={}", crate::live::enc(path));
    let opened = reach.open(Request::get(NodeId(node.to_owned()), uri));
    let (done, result) = async_channel::bounded(1);
    reach.spawn(async move {
        let copied = match opened.await {
            Err(e) => Err(e.message),
            Ok(mut answer) if !(200..300).contains(&answer.status) => {
                let body = answer.up_to(64 << 10).await.unwrap_or_default();
                Err(ahjo_app_kit::link::http::error_text(answer.status, &body))
            }
            Ok(answer) => copy(answer).await,
        };
        let _ = done.send(copied).await;
    });
    async move {
        result
            .recv()
            .await
            .unwrap_or_else(|_| Err("The copy stopped before it finished".into()))
    }
}

/// Writes the answer's body to `file` (at `to`) as it comes: the bytes
/// written, or why the copy is not whole.
async fn write(
    mut answer: ahjo_app_kit::link::http::Streamed,
    mut file: std::fs::File,
    to: &Path,
) -> Result<u64, String> {
    let whole = answer
        .header("content-length")
        .and_then(|n| n.trim().parse::<u64>().ok());
    let mut written = 0u64;
    while let Some(piece) = answer.piece().await? {
        file.write_all(&piece)
            .map_err(|e| format!("{}: {e}", to.display()))?;
        written += piece.len() as u64;
    }
    file.sync_all()
        .map_err(|e| format!("{}: {e}", to.display()))?;
    match whole {
        Some(whole) if whole != written => {
            Err(format!("The copy stopped at {written} of {whole} bytes"))
        }
        _ => Ok(written),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_node_name_becomes_one_safe_local_name_with_its_extension() {
        for (name, local) in [
            ("clip.mp4", "clip.mp4"),
            ("a\\..\\..\\boot.ini", "a_.._.._boot.ini"),
            ("C:x.docx", "C_x.docx"),
            ("what?.pdf", "what_.pdf"),
            ("tab\there.zip", "tab_here.zip"),
            ("CON", "_CON"),
            ("nul.txt", "_nul.txt"),
            ("com1.mp4", "_com1.mp4"),
            ("LPT9", "_LPT9"),
            ("com\u{b9}.txt", "_com\u{b9}.txt"),
            ("console.txt", "console.txt"),
            ("com10.txt", "com10.txt"),
            ("trailing. . ", "trailing"),
            ("...", "_file"),
            ("..", "_file"),
            ("", "_file"),
            ("ääkköset.wav", "ääkköset.wav"),
        ] {
            assert_eq!(local_name(name), local, "{name:?}");
        }
        let long = format!("{}.docx", "x".repeat(400));
        let short = local_name(&long);
        assert_eq!(short.chars().count(), 150);
        assert!(short.ends_with(".docx"));
    }

    #[test]
    fn each_open_has_its_own_folder_and_old_ones_go() {
        let under = std::env::temp_dir().join(format!("ahjo-outside-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&under);
        let a = fresh_folder(&under).unwrap();
        let b = fresh_folder(&under).unwrap();
        assert_ne!(a, b);
        std::fs::write(a.join("x.mp4"), b"x").unwrap();
        let old = under.join("1000-0");
        std::fs::create_dir(&old).unwrap();
        std::fs::create_dir(under.join("not-an-open")).unwrap();
        sweep(&under, KEPT);
        assert!(!old.exists());
        assert!(a.join("x.mp4").exists() && b.exists());
        assert!(under.join("not-an-open").exists());
        std::fs::remove_dir_all(&under).unwrap();
    }
}
