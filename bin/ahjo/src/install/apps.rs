//! The Windows apps an upgrade puts beside the node, which it hands to other
//! machines (`ahjo_node_kit::serve::apps`): the three builds of the node's
//! own commit in `<root>/apps/windows/<version>/` with their manifest, and
//! no other version's.

use std::path::{Path, PathBuf};

use ahjo_core::Error;
use ahjo_node_kit::serve::apps::{File, MANIFEST, Manifest, folder};
use sha2::{Digest, Sha256};

use super::io;

/// What a Windows install holds, as `cargo build` names the files.
pub const WINDOWS: &[&str] = &["ahjo-swarm.exe", "ahjo-brain.exe", "ahjo-desk.exe"];

const PLATFORM: &str = "windows";

/// Puts the apps in `from` in place as `version`'s under `apps`, after
/// checking each was built from that version's commit. The folder appears
/// whole: it is filled beside and renamed into place.
pub fn put(from: &Path, apps: &Path, version: &str) -> Result<PathBuf, Error> {
    let mut files = Vec::new();
    let mut bytes = Vec::new();
    for name in WINDOWS {
        let path = from.join(name);
        let read = std::fs::read(&path).map_err(|e| {
            Error::new(
                "apps_missing",
                format!(
                    "{}: {e}; --windows-apps names the folder cargo built the three Windows apps in",
                    path.display()
                ),
            )
        })?;
        // Each app carries its build's version (its `AHJO_VERSION`).
        if memchr::memmem::find(&read, version.as_bytes()).is_none() {
            return Err(Error::new(
                "apps_mismatch",
                format!(
                    "{} was not built from Ahjo {version}: build the Windows apps from the same \
                     commit as the node",
                    path.display()
                ),
            ));
        }
        files.push(File {
            name: (*name).to_owned(),
            size: read.len() as u64,
            sha256: hex(&Sha256::digest(&read)),
        });
        bytes.push(read);
    }
    let to = folder(apps, PLATFORM, version);
    let parent = to.parent().unwrap_or(apps);
    std::fs::create_dir_all(parent).map_err(|e| io("creating", parent, e))?;
    let next = parent.join(format!(".{version}.{}.new", std::process::id()));
    let _ = std::fs::remove_dir_all(&next);
    std::fs::create_dir(&next).map_err(|e| io("creating", &next, e))?;
    for (file, read) in files.iter().zip(&bytes) {
        let path = next.join(&file.name);
        std::fs::write(&path, read).map_err(|e| io("writing", &path, e))?;
    }
    let manifest = Manifest {
        version: version.to_owned(),
        files,
    };
    let path = next.join(MANIFEST);
    std::fs::write(&path, serde_json::to_vec_pretty(&manifest).unwrap())
        .map_err(|e| io("writing", &path, e))?;
    if to.exists() {
        std::fs::remove_dir_all(&to).map_err(|e| io("replacing", &to, e))?;
    }
    std::fs::rename(&next, &to).map_err(|e| io("putting in place", &to, e))?;
    Ok(to)
}

/// Removes every version's apps under `apps` but `keep`'s.
pub fn prune(apps: &Path, keep: &str) -> Result<(), Error> {
    let dir = apps.join(PLATFORM);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        if entry.file_name() != keep {
            let path = entry.path();
            std::fs::remove_dir_all(&path).map_err(|e| io("removing", &path, e))?;
        }
    }
    Ok(())
}

/// Removes `version`'s apps, as a failed upgrade does.
pub fn remove(apps: &Path, version: &str) {
    let _ = std::fs::remove_dir_all(folder(apps, PLATFORM, version));
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
