//! Portable atomic writes to definition targets, including symlinked files.
use std::collections::HashMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock, Weak};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
type Registry<T> = Mutex<HashMap<PathBuf, Weak<T>>>;
static REFERENCES: OnceLock<Registry<RwLock<()>>> = OnceLock::new();
static DOCUMENTS: OnceLock<Registry<Mutex<()>>> = OnceLock::new();
static FILES: OnceLock<Registry<Mutex<()>>> = OnceLock::new();

/// A mission's three files share one lock; agent archival excludes mission writers.
pub(crate) struct Locks {
    pub references: Arc<RwLock<()>>,
    pub document: Arc<Mutex<()>>,
}
pub(crate) fn locks(root: &Path, document: &str) -> Locks {
    let root = canonical_root(root);
    Locks {
        references: get(REFERENCES.get_or_init(Default::default), root.clone()),
        document: get(DOCUMENTS.get_or_init(Default::default), root.join(document)),
    }
}
fn canonical_root(root: &Path) -> PathBuf {
    let absolute = if root.is_absolute() {
        root.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(root)
    };
    let mut ancestor = absolute.as_path();
    let mut suffix = Vec::new();
    loop {
        if let Ok(mut resolved) = std::fs::canonicalize(ancestor) {
            for component in suffix.into_iter().rev() {
                resolved.push(component);
            }
            return resolved;
        }
        let (Some(name), Some(parent)) = (ancestor.file_name(), ancestor.parent()) else {
            return absolute;
        };
        suffix.push(name.to_owned());
        ancestor = parent;
    }
}

fn get<T: Default>(registry: &Registry<T>, key: PathBuf) -> Arc<T> {
    let mut map = registry.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(value) = map.get(&key).and_then(Weak::upgrade) {
        return value;
    }
    map.retain(|_, value| value.strong_count() > 0);
    let value = Arc::new(T::default());
    map.insert(key, Arc::downgrade(&value));
    value
}

#[cfg(test)]
mod reference_lock_tests {
    #[test]
    fn agent_archival_and_mission_writes_share_the_reference_lock() {
        let root = std::env::temp_dir().join(format!("ahjo-reference-lock-{}", std::process::id()));
        let mission = super::locks(&root, "mission:example");
        let agent = super::locks(&root, "agent:writer");
        let guard = mission.references.read().unwrap();
        assert!(agent.references.try_write().is_err());
        drop(guard);
        assert!(agent.references.try_write().is_ok());
    }
}

/// Replace a TOML value while retaining its spacing and inline comment. A
/// new key goes last and takes the closing space the last value had (the
/// space before an inline table's `}`), so no space is left before its comma.
pub(crate) fn set(table: &mut dyn toml_edit::TableLike, key: &str, mut item: toml_edit::Item) {
    let decor = table
        .get(key)
        .and_then(toml_edit::Item::as_value)
        .map(|v| v.decor().clone());
    match (decor, item.as_value_mut()) {
        (Some(decor), Some(value)) => *value.decor_mut() = decor,
        (None, Some(value)) => {
            let last = table
                .iter_mut()
                .last()
                .and_then(|(_, last)| last.as_value_mut());
            if let Some(last) = last {
                let closing = last
                    .decor()
                    .suffix()
                    .and_then(|s| s.as_str())
                    .map(str::to_owned);
                if let Some(closing) = closing.filter(|s| !s.is_empty() && s.trim().is_empty()) {
                    last.decor_mut().set_suffix("");
                    value.decor_mut().set_suffix(closing);
                }
            }
        }
        _ => {}
    }
    table.insert(key, item);
}

#[cfg(test)]
mod set_tests {
    #[test]
    fn a_key_added_to_an_inline_table_leaves_no_space_before_its_comma() {
        let mut doc: toml_edit::DocumentMut = "seat = [{ id = \"s\", controller = false }]\n"
            .parse()
            .unwrap();
        let seat = doc["seat"]
            .as_array_mut()
            .unwrap()
            .get_mut(0)
            .unwrap()
            .as_inline_table_mut()
            .unwrap();
        super::set(seat, "harness", toml_edit::value("claude-code"));
        super::set(seat, "controller", toml_edit::value(true));
        assert_eq!(
            doc.to_string(),
            "seat = [{ id = \"s\", controller = true, harness = \"claude-code\" }]\n"
        );
    }
}

/// Make top-level inline table arrays editable by the graph authoring paths.
pub(crate) fn table_arrays(doc: &mut toml_edit::DocumentMut, keys: &[&str]) {
    for &key in keys {
        let Some(array) = doc.get(key).and_then(toml_edit::Item::as_array) else {
            continue;
        };
        if !array.iter().all(|value| value.is_inline_table()) {
            continue;
        }
        let mut tables = toml_edit::ArrayOfTables::new();
        for value in array.iter() {
            tables.push(value.as_inline_table().unwrap().clone().into_table());
        }
        doc[key] = toml_edit::Item::ArrayOfTables(tables);
    }
}

/// Serializes migration with publication, including through a symlink.
pub(crate) fn file_lock(path: &Path) -> Arc<Mutex<()>> {
    get(FILES.get_or_init(Default::default), canonical_root(path))
}

fn backup_path(path: &Path) -> io::Result<PathBuf> {
    let target = std::fs::canonicalize(path)?;
    let mut name = target.as_os_str().to_os_string();
    name.push(".bak");
    Ok(PathBuf::from(name))
}

fn marker_path(backup: &Path) -> PathBuf {
    let mut name = backup.as_os_str().to_os_string();
    name.push(".ahjo-migration");
    name.into()
}

fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(bytes))
}

/// Keep the exact old bytes before replacing a document with its upgraded form.
/// A stale backup is never overwritten.
pub(crate) fn publish_upgrade(path: &Path, original: &[u8], upgraded: &[u8]) -> io::Result<()> {
    let backup = backup_path(path)?;
    let marker = marker_path(&backup);
    let fingerprint = digest(original);
    let marker_hash = match std::fs::symlink_metadata(&marker) {
        Ok(metadata) if metadata.file_type().is_file() => Some(std::fs::read_to_string(&marker)?),
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{} is not a regular migration marker", marker.display()),
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    let backup_exists = match std::fs::symlink_metadata(&backup) {
        Ok(metadata) if marker_hash.is_some() && metadata.file_type().is_file() => {
            if digest(&std::fs::read(&backup)?) != marker_hash.as_deref().unwrap_or_default() {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("{} does not match its migration marker", backup.display()),
                ));
            }
            true
        }
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{} is an existing, unowned backup", backup.display()),
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => return Err(error),
    };
    if !backup_exists
        && marker_hash
            .as_deref()
            .is_some_and(|hash| hash != fingerprint)
    {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!(
                "{} does not match the source for its missing backup",
                marker.display()
            ),
        ));
    }
    if marker_hash.is_none() {
        let staged = prepare(&marker, fingerprint.as_bytes())?;
        std::fs::hard_link(&staged.temp, &marker)?;
    }
    if !backup_exists {
        let staged = prepare(&backup, original)?;
        std::fs::hard_link(&staged.temp, &backup)?;
    }
    // The backup links must be durable before the replacement can become durable.
    #[cfg(unix)]
    sync_parent(&backup)?;
    prepare(path, upgraded)?.publish()?;
    #[cfg(unix)]
    sync_parent(&backup)?;
    Ok(())
}

#[cfg(unix)]
fn sync_parent(path: &Path) -> io::Result<()> {
    std::fs::File::open(path.parent().unwrap_or(Path::new(".")))?.sync_all()
}

fn remove_backup(path: &Path) -> io::Result<()> {
    let backup = backup_path(path)?;
    let marker = marker_path(&backup);
    let marker_type = match std::fs::symlink_metadata(&marker) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if !marker_type.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "migration marker is not a regular file",
        ));
    }
    let expected = std::fs::read_to_string(&marker)?;
    match std::fs::symlink_metadata(&backup) {
        Ok(metadata) if metadata.file_type().is_file() => {
            if digest(&std::fs::read(&backup)?) != expected {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "{} no longer matches its migration marker",
                        backup.display()
                    ),
                ));
            }
            std::fs::remove_file(&backup)?;
        }
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "migration backup is not a regular file",
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => (),
        Err(error) => return Err(error),
    }
    std::fs::remove_file(marker)
}

fn cleanup_backup(path: &Path) {
    if let Err(error) = remove_backup(path) {
        // Publication succeeded. A backup cleanup error must not report
        // that the edit failed and cause a retry of the already saved edit.
        eprintln!(
            "ahjo: saved {}, but could not remove its migration backup: {error}",
            path.display()
        );
    }
}

/// Replace an existing target without replacing a symlink at the definition.
pub(crate) fn atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let lock = file_lock(path);
    let _guard = lock.lock().unwrap_or_else(|error| error.into_inner());
    prepare(path, bytes)?.publish()?;
    cleanup_backup(path);
    Ok(())
}
/// Stage both files before publishing either, under the caller's mission lock.
pub(crate) fn pair(first: (&Path, &[u8]), second: (&Path, &[u8])) -> io::Result<()> {
    let first_lock = file_lock(first.0);
    let second_lock = file_lock(second.0);
    let first_key = canonical_root(first.0);
    let second_key = canonical_root(second.0);
    if first_key == second_key {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "two documents share a path",
        ));
    }
    let (earlier, later) = if first_key < second_key {
        (&first_lock, &second_lock)
    } else {
        (&second_lock, &first_lock)
    };
    let _earlier_guard = earlier.lock().unwrap_or_else(|error| error.into_inner());
    let _later_guard = later.lock().unwrap_or_else(|error| error.into_inner());
    pair_with(first, second, Staged::publish)?;
    cleanup_backup(first.0);
    cleanup_backup(second.0);
    Ok(())
}

fn pair_with(
    first: (&Path, &[u8]),
    second: (&Path, &[u8]),
    publish_second: impl FnOnce(Staged) -> io::Result<()>,
) -> io::Result<()> {
    let old = match std::fs::read(first.0) {
        Ok(bytes) => Some(prepare(first.0, &bytes)?),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    let first = prepare(first.0, first.1)?;
    let second = prepare(second.0, second.1)?;
    let first_target = first.target.clone();
    first.publish()?;
    if let Err(error) = publish_second(second) {
        let rollback = if let Some(mut backup) = old {
            let backup_path = backup.temp.clone();
            match std::fs::rename(&backup.temp, &backup.target) {
                Ok(()) => Ok(()),
                Err(rollback_error) => {
                    backup.retain = true;
                    Err(io::Error::new(
                        rollback_error.kind(),
                        format!(
                            "{rollback_error}; original saved at {}",
                            backup_path.display()
                        ),
                    ))
                }
            }
        } else {
            std::fs::remove_file(first_target)
        };
        return match rollback {
            Ok(()) => Err(error),
            Err(rollback_error) => Err(io::Error::new(
                error.kind(),
                format!("second publish failed: {error}; rollback failed: {rollback_error}"),
            )),
        };
    }
    Ok(())
}
struct Staged {
    temp: PathBuf,
    target: PathBuf,
    retain: bool,
}
impl Staged {
    fn publish(self) -> io::Result<()> {
        std::fs::rename(&self.temp, &self.target)
    }
}
impl Drop for Staged {
    fn drop(&mut self) {
        if !self.retain {
            let _ = std::fs::remove_file(&self.temp);
        }
    }
}
fn prepare(path: &Path, bytes: &[u8]) -> io::Result<Staged> {
    let target = match std::fs::symlink_metadata(path) {
        Ok(_) => std::fs::canonicalize(path)?,
        Err(err) if err.kind() == io::ErrorKind::NotFound => path.to_path_buf(),
        Err(err) => return Err(err),
    };
    let dir = target.parent().unwrap_or_else(|| Path::new("."));
    let existed = dir.exists();
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    if !existed {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o2770))?;
    }
    #[cfg(not(unix))]
    let _ = existed;
    let permissions = match std::fs::metadata(&target) {
        Ok(metadata) if metadata.is_file() => Some(metadata.permissions()),
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "target is not a regular file",
            ));
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => None,
        Err(err) => return Err(err),
    };
    let (temp_path, mut temp) = create_temp(dir)?;
    let staged = Staged {
        temp: temp_path,
        target,
        retain: false,
    };
    temp.write_all(bytes)?;
    if let Some(permissions) = permissions {
        temp.set_permissions(permissions)?;
    } else {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            temp.set_permissions(std::fs::Permissions::from_mode(0o660))?;
        }
    }
    temp.sync_all()?;
    drop(temp);
    Ok(staged)
}

fn create_temp(dir: &Path) -> io::Result<(PathBuf, std::fs::File)> {
    loop {
        let path = dir.join(format!(
            ".ahjo-{}-{}.tmp",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => return Ok((path, file)),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_second_publish_restores_first_file_and_keeps_second() {
        let dir = std::env::temp_dir().join(format!(
            "ahjo-97-pair-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let first = dir.join("mission.toml");
        let second = dir.join("layout.toml");
        std::fs::write(&first, b"old mission").unwrap();
        std::fs::write(&second, b"old layout").unwrap();
        let err = pair_with((&first, b"new mission"), (&second, b"new layout"), |_| {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "injected second rename failure",
            ))
        })
        .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(std::fs::read(&first).unwrap(), b"old mission");
        assert_eq!(std::fs::read(&second).unwrap(), b"old layout");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rollback_failure_keeps_original_in_the_reported_backup() {
        let dir = std::env::temp_dir().join(format!(
            "ahjo-97-pair-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let first = dir.join("mission.toml");
        let second = dir.join("layout.toml");
        std::fs::write(&first, b"old mission").unwrap();
        std::fs::write(&second, b"old layout").unwrap();
        let err = pair_with((&first, b"new mission"), (&second, b"new layout"), |_| {
            std::fs::remove_file(&first)?;
            std::fs::create_dir(&first)?;
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "second rename",
            ))
        })
        .unwrap_err();
        let backup = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(".ahjo-")
            })
            .unwrap();
        assert!(
            err.to_string()
                .contains(backup.file_name().unwrap().to_str().unwrap())
        );
        assert_eq!(std::fs::read(&backup).unwrap(), b"old mission");
        assert_eq!(std::fs::read(&second).unwrap(), b"old layout");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
