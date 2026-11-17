//! Which files of a folder's repository differ from its HEAD, as `git
//! status` says: what the tree marks.
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

use super::{error::FilesError, roots::Roots};
pub use ahjo_files_contract::{FileState, RepoStatus};

/// Files reported at most: a repository with more changed is shown in part.
const MOST: usize = 5000;
const LIMIT: u64 = 4 << 20;

pub fn read(roots: &Roots, requested: &str) -> Result<RepoStatus, FilesError> {
    let folder = roots.existing(requested)?;
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
            "--no-renames",
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
    let mut bytes = Vec::new();
    let _ = child
        .stdout
        .take()
        .expect("piped stdout")
        .take(LIMIT)
        .read_to_end(&mut bytes);
    let mut errors = Vec::new();
    let _ = child
        .stderr
        .take()
        .expect("piped stderr")
        .take(LIMIT)
        .read_to_end(&mut errors);
    let _ = child.kill();
    match child.wait() {
        Ok(status) if !status.success() => {
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
    for (state, relative) in entries(&String::from_utf8_lossy(&bytes)) {
        if result.files.len() == MOST {
            result.truncated = true;
            break;
        }
        result.files.push(FileState {
            path: repository.join(relative).to_string_lossy().into_owned(),
            state,
        });
    }
    Ok(result)
}

/// `XY path\0` entries: the state that matters to a reader, worktree first.
fn entries(out: &str) -> impl Iterator<Item = (char, &str)> {
    out.split('\0').filter_map(|entry| {
        let (xy, path) = (entry.get(..2)?, entry.get(3..)?);
        let mut c = xy.chars();
        let (x, y) = (c.next()?, c.next()?);
        let state = match (x, y) {
            ('?', '?') => '?',
            (_, 'D') | ('D', _) => 'D',
            ('A', _) => 'A',
            ('R', _) => 'R',
            _ => 'M',
        };
        (!path.is_empty() && !path.ends_with('/')).then_some((state, path))
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn porcelain_entries_read_as_what_a_reader_sees() {
        let out = " M a.rs\0?? new/b.md\0A  c.txt\0 D gone.rs\0MM both.rs\0";
        let got: Vec<_> = super::entries(out).collect();
        assert_eq!(
            got,
            [
                ('M', "a.rs"),
                ('?', "new/b.md"),
                ('A', "c.txt"),
                ('D', "gone.rs"),
                ('M', "both.rs")
            ]
        );
    }
}
