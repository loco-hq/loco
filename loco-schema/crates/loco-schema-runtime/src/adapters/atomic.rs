//! Temp-then-rename helpers shared by the filesystem adapters.
//!
//! Every temp artefact is a sibling of its target whose name starts with
//! [`TEMP_PREFIX`]. A crash can leave one behind; loaders skip that prefix, so
//! a leftover never reaches a parser and never blocks boot.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::Error;

/// Leading `.` plus a name no pathTemplate produces.
pub(crate) const TEMP_PREFIX: &str = ".loco-";

pub(crate) fn is_temp_name(name: &str) -> bool {
    name.starts_with(TEMP_PREFIX)
}

/// A unique sibling name in `parent`, used to stage a write before it is
/// renamed into place.
pub(crate) fn temp_sibling(parent: &Path, tag: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    parent.join(format!(
        "{TEMP_PREFIX}{tag}-{}-{nanos}-{n}",
        std::process::id()
    ))
}

/// Replace the file at `path` with `bytes` so that a reader, or a boot after a
/// crash, sees the old contents or the new ones and never a truncated mix.
///
/// The bytes are written and fsynced to a temp sibling, renamed over `path`,
/// and then the parent directory is fsynced so the rename itself is durable.
/// On failure the temp file is removed and `path` is untouched — except when
/// only the directory fsync fails: the rename has happened, so that is
/// [`Error::NotDurable`], not a failed write.
pub(crate) fn write_file_atomic(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::InvalidPath(path.display().to_string()))?;
    let tmp = temp_sibling(parent, "write");
    let result = write_synced(&tmp, bytes).and_then(|()| std::fs::rename(&tmp, path));
    if let Err(e) = result {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    sync_dir(parent).map_err(Error::NotDurable)
}

fn write_synced(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// Flush a directory's entries. POSIX only: std cannot open a directory as a
/// file on Windows, so there it is a no-op.
#[cfg(unix)]
fn sync_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::File::open(dir)?.sync_all()
}

#[cfg(not(unix))]
fn sync_dir(_dir: &Path) -> std::io::Result<()> {
    Ok(())
}
