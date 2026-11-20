//! Which files of a folder's repository differ from its HEAD, as `git
//! status` says: what the tree marks.
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::{error::FilesError, roots::Roots};
pub use ahjo_files_contract::{FileState, RepoStatus};

/// Files reported at most: a repository with more changed is shown in part.
const MOST: usize = 5000;
const LIMIT: u64 = 4 << 20;

pub fn read(roots: &Roots, requested: &str) -> Result<RepoStatus, FilesError> {
    let folder = match roots.existing(requested) {
        Ok(path) => path,
        Err(FilesError::NotFound) => roots.gone(requested)?,
        Err(e) => return Err(e),
    };
    let folder = if folder.is_dir() {
        folder
    } else {
        folder.parent().map(Path::to_path_buf).unwrap_or(folder)
    };
    let mut result = RepoStatus::default();
    let Some(repository) = folder.ancestors().find(|p| p.join(".git").exists()) else {
        return Ok(result);
    };
    result.repository = repository.to_string_lossy().into_owned();
    let child = Command::new("git")
        .current_dir(repository)
        .args([
            "--literal-pathspecs",
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--renames",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(e) => {
            result.error = Some(e.to_string());
            return Ok(result);
        }
    };
    let stderr = child.stderr.take().expect("piped stderr");
    let errors = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stderr.take(LIMIT).read_to_end(&mut bytes);
        bytes
    });
    let mut bytes = Vec::new();
    let read = child
        .stdout
        .take()
        .expect("piped stdout")
        .take(LIMIT + 1)
        .read_to_end(&mut bytes);
    result.truncated = bytes.len() as u64 > LIMIT;
    if result.truncated || read.is_err() {
        let _ = child.kill();
    }
    let status = child.wait();
    let errors = errors.join().unwrap_or_default();
    if let Err(e) = read {
        result.error = Some(e.to_string());
        return Ok(result);
    }
    match status {
        Ok(status) if !status.success() && !result.truncated => {
            let message = String::from_utf8_lossy(&errors).trim().to_owned();
            result.error = Some(if message.is_empty() {
                format!("git exited with {status}")
            } else {
                message
            });
            return Ok(result);
        }
        Err(e) => {
            result.error = Some(e.to_string());
            return Ok(result);
        }
        _ => {}
    }
    bytes.truncate(LIMIT as usize);
    let out = String::from_utf8_lossy(&bytes);
    let complete = out.rsplit_once('\0').map_or("", |(complete, _)| complete);
    let absolute = |path: &str| {
        // Git separates components with `/` on every platform.
        repository
            .join(path.split('/').collect::<PathBuf>())
            .to_string_lossy()
            .into_owned()
    };
    for (state, relative, renamed_from) in entries(complete) {
        if result.files.len() == MOST {
            result.truncated = true;
            break;
        }
        result.files.push(FileState {
            path: absolute(relative),
            state,
            renamed_from: renamed_from.map(absolute),
        });
    }
    Ok(result)
}

/// `XY path\0` entries. With `-z`, a rename or copy writes the destination
/// first and adds a separate NUL-delimited source with no status prefix.
fn entries(out: &str) -> impl Iterator<Item = (char, &str, Option<&str>)> {
    let mut fields = out.split('\0');
    std::iter::from_fn(move || {
        loop {
            let entry = fields.next()?;
            let [x, y, b' ', ..] = entry.as_bytes() else {
                continue;
            };
            let path = &entry[3..];
            let has_source = matches!(x, b'R' | b'C') || matches!(y, b'R' | b'C');
            let source = if has_source {
                Some(fields.next()?)
            } else {
                None
            };
            let state = match (*x, *y) {
                (b'?', b'?') => '?',
                // Unmerged: both sides changed it, and it waits to be settled.
                (b'U', _) | (_, b'U') | (b'A', b'A') | (b'D', b'D') => 'C',
                (_, b'D') | (b'D', _) => 'D',
                (b'R', _) | (_, b'R') => 'R',
                (b'A' | b'C', _) | (_, b'C') => 'A',
                _ => 'M',
            };
            if !path.is_empty() && !path.ends_with('/') {
                return Some((state, path, source.filter(|_| state == 'R')));
            }
        }
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn porcelain_entries_read_as_what_a_reader_sees() {
        let out =
            " M a.rs\0?? new/b.md\0A  c.txt\0 D gone.rs\0MM both.rs\0UU fight.rs\0DU theirs.rs\0";
        let got: Vec<_> = super::entries(out).collect();
        assert_eq!(
            got,
            [
                ('M', "a.rs", None),
                ('?', "new/b.md", None),
                ('A', "c.txt", None),
                ('D', "gone.rs", None),
                ('M', "both.rs", None),
                ('C', "fight.rs", None),
                ('C', "theirs.rs", None),
            ]
        );
    }

    #[test]
    fn rename_sources_are_consumed_even_when_they_look_like_another_entry() {
        let out =
            "RM new name.rs\0?? old\nname.rs\0RD gone.rs\0M  before.rs\0?? real.rs\0R  partial.rs";
        assert_eq!(
            super::entries(out).collect::<Vec<_>>(),
            [
                ('R', "new name.rs", Some("?? old\nname.rs")),
                ('D', "gone.rs", None),
                ('?', "real.rs", None),
            ]
        );
    }
}
