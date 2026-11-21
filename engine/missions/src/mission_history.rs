//! Every revision of a mission as it was saved, so a view can say what
//! changed, when and by whom.
//!
//! <state-dir>/history/<slug>.<rev>.toml, beside `layout/` and `notes/`, and
//! archived with the mission when it is deleted.
use crate::{Error, Store};
use ahjo_mission_model::Mission;
use std::path::{Path, PathBuf};

fn dir(root: &Path) -> PathBuf {
    root.join("history")
}

/// The revision a file in the history folder keeps for `slug`, if it is one.
fn rev_of(slug: &str, file: &str) -> Option<i64> {
    let digits = file
        .strip_prefix(slug)?
        .strip_prefix('.')?
        .strip_suffix(".toml")?;
    (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
        .then(|| digits.parse().ok())
        .flatten()
}

/// The files of `slug`'s history, oldest first.
pub(crate) fn files(root: &Path, slug: &str) -> Result<Vec<PathBuf>, Error> {
    let mut files: Vec<(i64, PathBuf)> = match std::fs::read_dir(dir(root)) {
        Ok(entries) => entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter_map(|p| {
                let rev = rev_of(slug, p.file_name()?.to_str()?)?;
                Some((rev, p))
            })
            .collect(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => vec![],
        Err(e) => return Err(crate::error::io(e)),
    };
    files.sort();
    Ok(files.into_iter().map(|(_, p)| p).collect())
}

/// Keep `raw`, a mission document about to be saved, under its revision.
/// Called before the mission itself is written, so a failure stops the save.
pub(crate) fn record(root: &Path, slug: &str, raw: &str) -> Result<(), Error> {
    let rev = Mission::parse(raw)?.rev;
    crate::write::atomic(
        &dir(root).join(format!("{slug}.{rev:06}.toml")),
        raw.as_bytes(),
    )
    .map_err(crate::error::io)
}

impl Store {
    /// The mission's saved revisions, oldest first. A revision kept under
    /// an older schema is read as it upgrades, and left as it was kept.
    pub fn mission_history(&self, selector: &str) -> Result<Vec<Mission>, Error> {
        let slug = self.resolve(selector)?;
        files(self.root(), &slug)?
            .iter()
            .map(|p| {
                let raw = std::fs::read_to_string(p).map_err(crate::error::io)?;
                let upgraded = crate::document_migrate::upgrade_text(
                    &raw,
                    crate::document_migrate::DocumentKind::Mission,
                    |_, _, id| self.agents().read(id).ok(),
                )?;
                Mission::parse(upgraded.as_deref().unwrap_or(&raw))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::rev_of;

    #[test]
    fn a_history_file_belongs_to_its_own_slug_only() {
        assert_eq!(rev_of("work", "work.000003.toml"), Some(3));
        assert_eq!(rev_of("work", "work-two.000003.toml"), None);
        assert_eq!(rev_of("work", "work.000003.toml.deleted-x"), None);
        assert_eq!(rev_of("work", "work.toml"), None);
    }
}
