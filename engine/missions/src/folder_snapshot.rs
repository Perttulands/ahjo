//! Copies folder-run attempts and publishes a chosen attempt after checking
//! that the run folder still matches the attempt's starting point.
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{self, Read},
    path::{Path, PathBuf},
};

#[derive(Debug, PartialEq, Eq)]
struct Entry {
    kind: Kind,
    permissions: u32,
}

#[derive(Debug, PartialEq, Eq)]
enum Kind {
    Directory,
    File([u8; 32]),
    Link(PathBuf),
}

/// Make an independent baseline and a writable attempt from the run folder.
/// `excluded` names private state below the folder, if any.
pub(crate) fn prepare(
    source: &Path,
    attempt: &Path,
    baseline: &Path,
    excluded: &[PathBuf],
) -> io::Result<()> {
    let source = fs::canonicalize(source)?;
    let excluded = relative_exclusions(&source, excluded);
    if attempt.exists() || baseline.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "attempt snapshot already exists",
        ));
    }
    let result = (|| {
        copy_entry(&source, baseline, Path::new(""), &excluded)?;
        let original = scan(&source, &excluded)?;
        let saved = scan(baseline, &[])?;
        let changed = differences(&saved, &original);
        if !changed.is_empty() {
            return Err(changed_error(&changed));
        }
        copy_entry(baseline, attempt, Path::new(""), &[])
    })();
    if result.is_err() {
        let _ = crate::kept_copy::remove_tree(attempt);
        let _ = crate::kept_copy::remove_tree(baseline);
    }
    result
}

/// Copy a tree as a snapshot, refusing a source that changed during the copy.
pub(crate) fn snapshot(source: &Path, destination: &Path, excluded: &[PathBuf]) -> io::Result<()> {
    let source = fs::canonicalize(source)?;
    let excluded = relative_exclusions(&source, excluded);
    if fs::symlink_metadata(destination).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "snapshot already exists",
        ));
    }
    let result = (|| {
        copy_entry(&source, destination, Path::new(""), &excluded)?;
        let changed = differences(&scan(destination, &[])?, &scan(&source, &excluded)?);
        if !changed.is_empty() {
            return Err(changed_error(&changed));
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = crate::kept_copy::remove_tree(destination);
    }
    result
}

/// Whether two snapshots have identical entries and permissions.
pub(crate) fn same(first: &Path, second: &Path) -> io::Result<bool> {
    Ok(scan(first, &[])? == scan(second, &[])?)
}

/// Publish the selected attempt, refusing before publication when the run
/// folder has changed since this attempt started. Replacements are staged in
/// the run folder so renames stay on the same filesystem. A failed replacement
/// rolls back earlier replacements.
pub(crate) fn publish(
    source: &Path,
    attempt: &Path,
    baseline: &Path,
    excluded: &[PathBuf],
) -> io::Result<()> {
    publish_inner(source, attempt, baseline, excluded, true)
}

/// Publish only this attempt's changes, retaining concurrent disjoint edits.
pub(crate) fn publish_changes(
    source: &Path,
    attempt: &Path,
    baseline: &Path,
    excluded: &[PathBuf],
) -> io::Result<()> {
    publish_inner(source, attempt, baseline, excluded, false)
}

fn publish_inner(
    source: &Path,
    attempt: &Path,
    baseline: &Path,
    excluded: &[PathBuf],
    strict: bool,
) -> io::Result<()> {
    let source = fs::canonicalize(source)?;
    let mut excluded = relative_exclusions(&source, excluded);
    let initial = scan(baseline, &[])?;
    let chosen = scan(attempt, &[])?;
    let (scratch, journal) = transaction_paths(&source, baseline);
    let scratch_relative = scratch
        .strip_prefix(&source)
        .expect("scratch is inside source")
        .to_owned();
    if !journal.exists() && fs::symlink_metadata(&scratch).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!(
                "publication scratch path already exists: {}",
                scratch.display()
            ),
        ));
    }
    if journal.exists() {
        verify_scratch_ownership(&scratch, &journal)?;
    } else if strict {
        let changed = differences(&initial, &scan(&source, &excluded)?);
        if !changed.is_empty() {
            return Err(changed_error(&changed));
        }
    }
    excluded.push(scratch_relative);
    let operations = if strict {
        plan(&initial, &chosen, &excluded)
    } else {
        plan_changes(&initial, &chosen, &excluded)
    };

    if journal.join("done").exists() {
        crate::kept_copy::remove_tree(&scratch)?;
        return Ok(());
    }
    if journal.exists() {
        recover(&source, &scratch, &journal, &initial, &chosen, &operations)?;
    }
    let actual = scan(&source, &excluded)?;
    if strict {
        let changed = differences(&initial, &actual);
        if !changed.is_empty() {
            return Err(changed_error(&changed));
        }
    }
    fs::create_dir(&journal)?;
    if let Err(error) = fs::create_dir(&scratch) {
        crate::kept_copy::remove_tree(&journal)?;
        return Err(error);
    }
    let ownership = File::create(scratch.join("owner"))
        .and_then(|file| file.sync_all())
        .and_then(|()| File::create(journal.join("owned")))
        .and_then(|file| file.sync_all());
    if let Err(error) = ownership {
        crate::kept_copy::remove_tree(&scratch)?;
        crate::kept_copy::remove_tree(&journal)?;
        return Err(error);
    }
    let result = (|| {
        let staged = scratch.join("selected");
        copy_entry(attempt, &staged, Path::new(""), &[])?;
        let staged_map = scan(&staged, &[])?;
        let changed_attempt = differences(&chosen, &staged_map);
        if !changed_attempt.is_empty() {
            return Err(io::Error::other(format!(
                "selected attempt changed while preparing publication: {}",
                paths_text(&changed_attempt)
            )));
        }
        let actual = scan(&source, &excluded)?;
        if strict {
            let changed = differences(&initial, &actual);
            if !changed.is_empty() {
                return Err(changed_error(&changed));
            }
        }
        fs::create_dir(scratch.join("backup"))?;
        for (index, path) in operations.iter().enumerate() {
            if !subtree_has_expected(&source, path, &initial)? {
                return Err(changed_error(std::slice::from_ref(path)));
            }
            record_progress(&journal, index + 1)?;
            replace(&source, &staged, &scratch.join("backup"), path, &initial)?;
        }
        let done = File::create(journal.join("done-next"))?;
        done.sync_all()?;
        fs::rename(journal.join("done-next"), journal.join("done"))?;
        Ok(())
    })();
    if let Err(ref error) = result {
        if let Err(undo) = recover(&source, &scratch, &journal, &initial, &chosen, &operations) {
            return Err(io::Error::other(format!(
                "publication failed: {error}; recovery failed: {undo}; backup remains at {}",
                scratch.display()
            )));
        }
    } else {
        crate::kept_copy::remove_tree(&scratch)?;
    }
    result
}

fn transaction_paths(source: &Path, baseline: &Path) -> (PathBuf, PathBuf) {
    let mut digest = Sha256::new();
    digest.update(baseline.to_string_lossy().as_bytes());
    let scratch_name = format!(".ahjo-publish-{}", hex::encode(digest.finalize()));
    (
        source.join(scratch_name),
        baseline.with_extension("publication"),
    )
}

/// Roll back an interrupted publication when a run is abandoned. The
/// attempt path is recorded alongside the journal by the caller.
pub(crate) fn abandon(
    source: &Path,
    baseline: &Path,
    attempt: &Path,
    excluded: &[PathBuf],
) -> io::Result<()> {
    abandon_inner(source, baseline, attempt, excluded, true)
}

/// Roll back an interrupted publication of only changed paths.
pub(crate) fn abandon_changes(
    source: &Path,
    baseline: &Path,
    attempt: &Path,
    excluded: &[PathBuf],
) -> io::Result<()> {
    abandon_inner(source, baseline, attempt, excluded, false)
}

fn abandon_inner(
    source: &Path,
    baseline: &Path,
    attempt: &Path,
    excluded: &[PathBuf],
    strict: bool,
) -> io::Result<()> {
    if !baseline.with_extension("publication").exists() {
        return Ok(());
    }
    let source = fs::canonicalize(source)?;
    let (scratch, journal) = transaction_paths(&source, baseline);
    verify_scratch_ownership(&scratch, &journal)?;
    if journal.join("done").exists() {
        crate::kept_copy::remove_tree(&scratch)?;
        return crate::kept_copy::remove_tree(&journal);
    }
    let initial = scan(baseline, &[])?;
    let chosen = scan(attempt, &[])?;
    let mut excluded = relative_exclusions(&source, excluded);
    excluded.push(
        scratch
            .strip_prefix(&source)
            .expect("scratch is inside source")
            .to_owned(),
    );
    let operations = if strict {
        plan(&initial, &chosen, &excluded)
    } else {
        plan_changes(&initial, &chosen, &excluded)
    };
    recover(&source, &scratch, &journal, &initial, &chosen, &operations)
}

fn verify_scratch_ownership(scratch: &Path, journal: &Path) -> io::Result<()> {
    if !scratch.exists() && !journal.join("done").exists() && journal.join("progress").exists() {
        return Err(io::Error::other(format!(
            "publication backup is missing: {}",
            scratch.display()
        )));
    }
    if fs::symlink_metadata(scratch).is_ok()
        && (!journal.join("owned").exists() || !scratch.join("owner").is_file())
    {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("unclaimed publication scratch path: {}", scratch.display()),
        ));
    }
    Ok(())
}

fn record_progress(journal: &Path, count: usize) -> io::Result<()> {
    use std::io::Write;
    let next = journal.join("next");
    let mut file = File::create(&next)?;
    write!(file, "{count}")?;
    file.sync_all()?;
    fs::rename(next, journal.join("progress"))
}

/// Restore the starting tree after an interruption. Before removing an
/// installed entry, compare it with the selected snapshot so a subsequent
/// external edit is never silently removed during recovery.
fn recover(
    source: &Path,
    scratch: &Path,
    journal: &Path,
    baseline: &BTreeMap<PathBuf, Entry>,
    chosen: &BTreeMap<PathBuf, Entry>,
    operations: &[PathBuf],
) -> io::Result<()> {
    let count = match fs::read_to_string(journal.join("progress")) {
        Ok(text) => text.parse::<usize>().map_err(io::Error::other)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
        Err(error) => return Err(error),
    };
    if count > operations.len() {
        return Err(io::Error::other("invalid folder publication journal"));
    }
    for path in operations[..count].iter().rev() {
        let old = scratch.join("backup").join(path);
        let staged = scratch.join("selected").join(path);
        let target = source.join(path);
        let had_old = baseline.contains_key(path);
        let has_new = chosen.contains_key(path);
        let old_exists = fs::symlink_metadata(&old).is_ok();
        let staged_exists = fs::symlink_metadata(&staged).is_ok();
        let target_exists = fs::symlink_metadata(&target).is_ok();
        let moved_new = has_new && !staged_exists && (old_exists || !had_old);
        if moved_new && target_exists {
            if !subtree_matches(source, path, chosen)? {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("run folder changed during publication: {}", path.display()),
                ));
            }
            crate::kept_copy::remove_tree(&target)?;
        } else if old_exists && target_exists {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("run folder changed during publication: {}", path.display()),
            ));
        }
        if old_exists {
            fs::rename(old, target)?;
        }
    }
    crate::kept_copy::remove_tree(scratch)?;
    crate::kept_copy::remove_tree(journal)
}

fn subtree_matches(
    root: &Path,
    path: &Path,
    expected: &BTreeMap<PathBuf, Entry>,
) -> io::Result<bool> {
    let mut actual = BTreeMap::new();
    scan_entry(root, path, &[], &mut actual)?;
    Ok(actual.iter().eq(expected
        .range(path.to_owned()..)
        .take_while(|(entry, _)| entry.starts_with(path))))
}

fn subtree_has_expected(
    root: &Path,
    path: &Path,
    expected: &BTreeMap<PathBuf, Entry>,
) -> io::Result<bool> {
    if fs::symlink_metadata(root.join(path)).is_err() {
        return Ok(!expected.contains_key(path));
    }
    subtree_matches(root, path, expected)
}

/// Select whole subtrees whenever they contain no excluded private state.
fn plan(
    baseline: &BTreeMap<PathBuf, Entry>,
    chosen: &BTreeMap<PathBuf, Entry>,
    excluded: &[PathBuf],
) -> Vec<PathBuf> {
    differences(baseline, chosen)
        .into_iter()
        .filter(|path| !excluded.iter().any(|entry| path.starts_with(entry)))
        .filter_map(|path| {
            path.ancestors()
                .take_while(|parent| {
                    !parent.as_os_str().is_empty()
                        && !excluded.iter().any(|entry| entry.starts_with(parent))
                })
                .last()
                .map(Path::to_owned)
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn plan_changes(
    baseline: &BTreeMap<PathBuf, Entry>,
    chosen: &BTreeMap<PathBuf, Entry>,
    excluded: &[PathBuf],
) -> Vec<PathBuf> {
    let changed: BTreeSet<_> = differences(baseline, chosen)
        .into_iter()
        .filter(|path| !excluded.iter().any(|entry| path.starts_with(entry)))
        .collect();
    changed
        .iter()
        .filter(|path| {
            !path.ancestors().skip(1).any(|parent| {
                changed.contains(parent)
                    && (baseline
                        .get(parent)
                        .is_some_and(|entry| entry.kind == Kind::Directory)
                        || chosen
                            .get(parent)
                            .is_some_and(|entry| entry.kind == Kind::Directory))
            })
        })
        .cloned()
        .collect()
}

fn replace(
    source: &Path,
    staged: &Path,
    backup: &Path,
    path: &Path,
    initial: &BTreeMap<PathBuf, Entry>,
) -> io::Result<()> {
    let target = source.join(path);
    let old = backup.join(path);
    let new = staged.join(path);
    let had_old = fs::symlink_metadata(&target).is_ok();
    let has_new = fs::symlink_metadata(&new).is_ok();
    if let Some(parent) = old.parent() {
        fs::create_dir_all(parent)?;
    }
    if had_old {
        fs::rename(&target, &old)?;
        if !subtree_has_expected(backup, path, initial)? {
            return Err(changed_error(std::slice::from_ref(&path.to_owned())));
        }
    }
    if has_new {
        rename_new(&new, &target).map_err(|error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                changed_error(std::slice::from_ref(&path.to_owned()))
            } else {
                error
            }
        })?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn rename_new(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let source = std::ffi::CString::new(source.as_os_str().as_bytes()).map_err(io::Error::other)?;
    let destination =
        std::ffi::CString::new(destination.as_os_str().as_bytes()).map_err(io::Error::other)?;
    let result = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            source.as_ptr(),
            libc::AT_FDCWD,
            destination.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(target_os = "linux"))]
fn rename_new(source: &Path, destination: &Path) -> io::Result<()> {
    if fs::symlink_metadata(destination).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "publication destination changed",
        ));
    }
    fs::rename(source, destination)
}

fn relative_exclusions(source: &Path, excluded: &[PathBuf]) -> Vec<PathBuf> {
    excluded
        .iter()
        .filter_map(|path| {
            let path = fs::canonicalize(path).unwrap_or_else(|_| path.clone());
            path.strip_prefix(source)
                .ok()
                .filter(|p| !p.as_os_str().is_empty())
                .map(Path::to_owned)
        })
        .collect()
}

fn scan(root: &Path, excluded: &[PathBuf]) -> io::Result<BTreeMap<PathBuf, Entry>> {
    let mut entries = BTreeMap::new();
    scan_entry(root, Path::new(""), excluded, &mut entries)?;
    Ok(entries)
}

fn scan_entry(
    root: &Path,
    relative: &Path,
    excluded: &[PathBuf],
    entries: &mut BTreeMap<PathBuf, Entry>,
) -> io::Result<()> {
    if excluded.iter().any(|path| relative == path) {
        return Ok(());
    }
    let path = root.join(relative);
    let metadata = fs::symlink_metadata(&path)?;
    let kind = if metadata.file_type().is_symlink() {
        Kind::Link(fs::read_link(&path)?)
    } else if metadata.is_dir() {
        for child in fs::read_dir(&path)? {
            scan_entry(root, &relative.join(child?.file_name()), excluded, entries)?;
        }
        Kind::Directory
    } else if metadata.is_file() {
        let mut input = open_file(&path)?;
        let mut digest = Sha256::new();
        let mut buffer = [0; 65536];
        loop {
            let count = input.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            digest.update(&buffer[..count]);
        }
        Kind::File(digest.finalize().into())
    } else {
        return Err(io::Error::other(format!(
            "unsupported workspace entry: {}",
            path.display()
        )));
    };
    // Root permissions are not changed by publishing its entries.
    if !relative.as_os_str().is_empty() {
        entries.insert(
            relative.to_owned(),
            Entry {
                kind,
                permissions: permissions(&metadata),
            },
        );
    }
    Ok(())
}

fn differences(a: &BTreeMap<PathBuf, Entry>, b: &BTreeMap<PathBuf, Entry>) -> Vec<PathBuf> {
    let paths: BTreeSet<_> = a.keys().chain(b.keys()).collect();
    paths
        .into_iter()
        .filter(|p| a.get(*p) != b.get(*p))
        .cloned()
        .collect()
}

fn changed_error(changed: &[PathBuf]) -> io::Error {
    io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!(
            "run folder changed since the attempt started: {}",
            paths_text(changed)
        ),
    )
}

fn paths_text(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn copy_entry(
    source: &Path,
    destination: &Path,
    relative: &Path,
    excluded: &[PathBuf],
) -> io::Result<()> {
    if excluded.iter().any(|path| relative == path) {
        return Ok(());
    }
    let src = source.join(relative);
    let dst = destination.join(relative);
    let metadata = fs::symlink_metadata(&src)?;
    if metadata.file_type().is_symlink() {
        copy_link(&src, &dst, &metadata)
    } else if metadata.is_dir() {
        fs::create_dir(&dst)?;
        for child in fs::read_dir(&src)? {
            copy_entry(
                source,
                destination,
                &relative.join(child?.file_name()),
                excluded,
            )?;
        }
        fs::set_permissions(dst, metadata.permissions())
    } else if metadata.is_file() {
        let mut input = open_file(&src)?;
        let mut output = OpenOptions::new().write(true).create_new(true).open(&dst)?;
        io::copy(&mut input, &mut output)?;
        fs::set_permissions(dst, metadata.permissions())
    } else {
        Err(io::Error::other(format!(
            "unsupported workspace entry: {}",
            src.display()
        )))
    }
}

fn open_file(path: &Path) -> io::Result<File> {
    let mut opts = OpenOptions::new();
    opts.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = opts.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::other(format!(
            "workspace entry changed: {}",
            path.display()
        )));
    }
    Ok(file)
}

#[cfg(unix)]
fn permissions(metadata: &fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o7777
}
#[cfg(not(unix))]
fn permissions(metadata: &fs::Metadata) -> u32 {
    u32::from(metadata.permissions().readonly())
}

#[cfg(unix)]
fn copy_link(source: &Path, destination: &Path, _: &fs::Metadata) -> io::Result<()> {
    std::os::unix::fs::symlink(fs::read_link(source)?, destination)
}
#[cfg(windows)]
fn copy_link(source: &Path, destination: &Path, metadata: &fs::Metadata) -> io::Result<()> {
    use std::os::windows::fs::FileTypeExt;
    let target = fs::read_link(source)?;
    if metadata.file_type().is_symlink_dir() {
        std::os::windows::fs::symlink_dir(target, destination)
    } else {
        std::os::windows::fs::symlink_file(target, destination)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> PathBuf {
        let mut random = [0u8; 16];
        getrandom::fill(&mut random).unwrap();
        let path = std::env::temp_dir().join(format!("ahjo-folder-test-{}", hex::encode(random)));
        fs::create_dir(&path).unwrap();
        path
    }

    #[test]
    fn independent_attempts_publish_one_and_refuse_an_external_change() {
        let root = scratch();
        let workspace = root.join("workspace");
        let one = root.join("one");
        let two = root.join("two");
        let base1 = root.join("base1");
        let base2 = root.join("base2");
        fs::create_dir(&workspace).unwrap();
        fs::write(workspace.join("edit"), "original").unwrap();
        fs::write(workspace.join("delete"), "remove").unwrap();
        prepare(&workspace, &one, &base1, &[]).unwrap();
        prepare(&workspace, &two, &base2, &[]).unwrap();
        fs::write(one.join("edit"), "one").unwrap();
        fs::write(two.join("edit"), "two").unwrap();
        fs::remove_file(one.join("delete")).unwrap();
        assert_eq!(fs::read_to_string(one.join("edit")).unwrap(), "one");
        assert_eq!(fs::read_to_string(two.join("edit")).unwrap(), "two");
        fs::write(workspace.join("other"), "concurrent").unwrap();
        let error = publish(&workspace, &one, &base1, &[]).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert!(error.to_string().contains("other"));
        assert_eq!(
            fs::read_to_string(workspace.join("edit")).unwrap(),
            "original"
        );
        fs::remove_file(workspace.join("other")).unwrap();
        publish(&workspace, &one, &base1, &[]).unwrap();
        assert_eq!(fs::read_to_string(workspace.join("edit")).unwrap(), "one");
        assert!(!workspace.join("delete").exists());
        // Replay sees the receipt and does not overwrite changes made after publication.
        fs::write(workspace.join("edit"), "after").unwrap();
        publish(&workspace, &one, &base1, &[]).unwrap();
        assert_eq!(fs::read_to_string(workspace.join("edit")).unwrap(), "after");
        crate::kept_copy::remove_tree(&root).unwrap();
    }

    #[test]
    fn reconcile_preserves_local_work_and_applies_disjoint_upstream_changes() {
        let root = scratch();
        let workspace = root.join("workspace");
        let upstream = root.join("upstream");
        let checkpoint = root.join("checkpoint");
        fs::create_dir(&workspace).unwrap();
        fs::write(workspace.join("shared"), "old").unwrap();
        fs::write(workspace.join("local"), "old").unwrap();
        prepare(&workspace, &upstream, &checkpoint, &[]).unwrap();
        fs::write(upstream.join("shared"), "new upstream").unwrap();
        fs::write(workspace.join("local"), "new local").unwrap();

        publish_changes(&workspace, &upstream, &checkpoint, &[]).unwrap();
        assert_eq!(
            fs::read_to_string(workspace.join("shared")).unwrap(),
            "new upstream"
        );
        assert_eq!(
            fs::read_to_string(workspace.join("local")).unwrap(),
            "new local"
        );

        crate::kept_copy::remove_tree(&root).unwrap();
    }

    #[test]
    fn reconcile_refuses_overlapping_changes() {
        let root = scratch();
        let workspace = root.join("workspace");
        let upstream = root.join("upstream");
        let checkpoint = root.join("checkpoint");
        fs::create_dir(&workspace).unwrap();
        fs::write(workspace.join("shared"), "old").unwrap();
        prepare(&workspace, &upstream, &checkpoint, &[]).unwrap();
        fs::write(upstream.join("shared"), "new upstream").unwrap();
        fs::write(workspace.join("shared"), "new local").unwrap();

        assert_eq!(
            publish_changes(&workspace, &upstream, &checkpoint, &[])
                .unwrap_err()
                .kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(
            fs::read_to_string(workspace.join("shared")).unwrap(),
            "new local"
        );

        crate::kept_copy::remove_tree(&root).unwrap();
    }

    #[test]
    fn scratch_collision_is_refused_without_removing_existing_data() {
        let root = scratch();
        let workspace = root.join("workspace");
        let attempt = root.join("attempt");
        let baseline = root.join("baseline");
        fs::create_dir(&workspace).unwrap();
        fs::write(workspace.join("file"), "before").unwrap();
        prepare(&workspace, &attempt, &baseline, &[]).unwrap();
        fs::write(attempt.join("file"), "after").unwrap();
        let (collision, journal) = transaction_paths(&workspace, &baseline);
        fs::create_dir(&collision).unwrap();
        fs::write(collision.join("keep"), "operator data").unwrap();
        let error = publish(&workspace, &attempt, &baseline, &[]).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(
            fs::read_to_string(collision.join("keep")).unwrap(),
            "operator data"
        );
        assert!(!journal.exists());
        fs::remove_dir_all(&workspace).unwrap();
        abandon(&workspace, &baseline, &attempt, &[]).unwrap();
        crate::kept_copy::remove_tree(&root).unwrap();
    }

    #[test]
    fn interruption_rolls_back_before_retry_and_preserves_excluded_state() {
        let root = scratch();
        let workspace = root.join("workspace");
        let private = workspace.join("state");
        let attempt = private.join("attempt");
        let baseline = private.join("baseline");
        fs::create_dir_all(&private).unwrap();
        fs::write(workspace.join("file"), "before").unwrap();
        prepare(
            &workspace,
            &attempt,
            &baseline,
            std::slice::from_ref(&private),
        )
        .unwrap();
        fs::write(attempt.join("file"), "after").unwrap();

        let initial = scan(&baseline, &[]).unwrap();
        let chosen = scan(&attempt, &[]).unwrap();
        let (staging, journal) = transaction_paths(&workspace, &baseline);
        fs::create_dir(&journal).unwrap();
        fs::create_dir(&staging).unwrap();
        copy_entry(&attempt, &staging.join("selected"), Path::new(""), &[]).unwrap();
        fs::create_dir(staging.join("backup")).unwrap();
        record_progress(&journal, 1).unwrap();
        replace(
            &workspace,
            &staging.join("selected"),
            &staging.join("backup"),
            Path::new("file"),
            &initial,
        )
        .unwrap();
        assert_eq!(fs::read_to_string(workspace.join("file")).unwrap(), "after");

        recover(
            &workspace,
            &staging,
            &journal,
            &initial,
            &chosen,
            &[PathBuf::from("file")],
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(workspace.join("file")).unwrap(),
            "before"
        );
        publish(
            &workspace,
            &attempt,
            &baseline,
            std::slice::from_ref(&private),
        )
        .unwrap();
        assert_eq!(fs::read_to_string(workspace.join("file")).unwrap(), "after");
        assert!(private.join("attempt").exists());
        crate::kept_copy::remove_tree(&root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn snapshot_copies_links_without_modifying_their_targets() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let root = scratch();
        let workspace = root.join("workspace");
        fs::create_dir(&workspace).unwrap();
        fs::write(root.join("outside"), "outside").unwrap();
        symlink(root.join("outside"), workspace.join("link")).unwrap();
        let attempt = root.join("attempt");
        let baseline = root.join("baseline");
        prepare(&workspace, &attempt, &baseline, &[]).unwrap();
        assert_eq!(
            fs::read_link(attempt.join("link")).unwrap(),
            root.join("outside")
        );
        fs::remove_file(attempt.join("link")).unwrap();
        fs::write(attempt.join("link"), "new file").unwrap();
        fs::set_permissions(attempt.join("link"), fs::Permissions::from_mode(0o640)).unwrap();
        publish(&workspace, &attempt, &baseline, &[]).unwrap();
        assert_eq!(fs::read_to_string(root.join("outside")).unwrap(), "outside");
        assert_eq!(
            fs::read_to_string(workspace.join("link")).unwrap(),
            "new file"
        );
        assert_eq!(
            fs::metadata(workspace.join("link"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o640
        );
        crate::kept_copy::remove_tree(&root).unwrap();
    }
}
