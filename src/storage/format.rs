//! On-disk format versioning and migration.
//!
//! Every BlockDB data directory carries a `FORMAT` marker recording the on-disk
//! format version it was written with. On open we:
//!   * stamp a fresh (or legacy, unmarked) directory with the current version;
//!   * **refuse** to open a directory written by a newer version than this binary
//!     understands, with a clear error, rather than silently misreading it;
//!   * run forward migrations for an older-but-supported version, then re-stamp.
//!
//! This gives a shippable product a safe story for evolving its formats: old data
//! upgrades through a defined path, and a downgraded binary fails loudly instead of
//! corrupting data.

use std::fs;
use std::path::Path;

use crate::error::BlockDBError;

/// The on-disk format version this build writes and can read up to.
pub const STORAGE_FORMAT_VERSION: u32 = 1;

const MARKER_FILE: &str = "FORMAT";
const MARKER_MAGIC: &str = "blockdb-format";

/// Ensure `data_dir` is at a format this binary supports, migrating if needed.
/// Returns the current (post-migration) format version.
pub fn open_or_init(data_dir: &str) -> Result<u32, BlockDBError> {
    fs::create_dir_all(data_dir)
        .map_err(|e| BlockDBError::StorageError(format!("create data dir: {}", e)))?;
    let path = Path::new(data_dir).join(MARKER_FILE);

    if !path.exists() {
        write_marker(&path, STORAGE_FORMAT_VERSION)?;
        return Ok(STORAGE_FORMAT_VERSION);
    }

    let found = read_marker(&path)?;
    if found > STORAGE_FORMAT_VERSION {
        return Err(BlockDBError::StorageError(format!(
            "data directory '{}' was written with on-disk format v{} but this build \
             supports only up to v{}; upgrade BlockDB to open it",
            data_dir, found, STORAGE_FORMAT_VERSION
        )));
    }
    if found < STORAGE_FORMAT_VERSION {
        migrate(data_dir, found, STORAGE_FORMAT_VERSION)?;
        write_marker(&path, STORAGE_FORMAT_VERSION)?;
        return Ok(STORAGE_FORMAT_VERSION);
    }
    Ok(found)
}

/// Apply forward migrations from `from` to `to`, one version at a time. New format
/// changes register their migration step here. (v1 is the first versioned format, so
/// there are no steps yet — the framework is in place for the first breaking change.)
fn migrate(data_dir: &str, from: u32, to: u32) -> Result<(), BlockDBError> {
    let v = from;
    while v < to {
        match v {
            // Example for the future:
            // 1 => migrate_v1_to_v2(data_dir)?,
            _ => {
                return Err(BlockDBError::StorageError(format!(
                    "no migration path from on-disk format v{} (data dir '{}')",
                    v, data_dir
                )));
            }
        }
        #[allow(unreachable_code)]
        {
            v += 1;
        }
    }
    Ok(())
}

fn write_marker(path: &Path, version: u32) -> Result<(), BlockDBError> {
    let tmp = path.with_extension("tmp");
    let body = format!("{}\nversion={}\n", MARKER_MAGIC, version);
    fs::write(&tmp, body.as_bytes())
        .and_then(|_| fs::rename(&tmp, path))
        .map_err(|e| BlockDBError::StorageError(format!("write format marker: {}", e)))?;
    Ok(())
}

fn read_marker(path: &Path) -> Result<u32, BlockDBError> {
    let content = fs::read_to_string(path)
        .map_err(|e| BlockDBError::StorageError(format!("read format marker: {}", e)))?;
    let mut lines = content.lines();
    let magic = lines.next().unwrap_or("").trim();
    if magic != MARKER_MAGIC {
        return Err(BlockDBError::StorageError(format!(
            "'{}' is not a BlockDB format marker (got header {:?})",
            path.display(),
            magic
        )));
    }
    let version = lines
        .find_map(|l| l.trim().strip_prefix("version=").map(|v| v.to_string()))
        .and_then(|v| v.trim().parse::<u32>().ok())
        .ok_or_else(|| {
            BlockDBError::StorageError(format!("format marker '{}' has no version", path.display()))
        })?;
    Ok(version)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn stamps_a_fresh_directory() {
        let dir = TempDir::new().unwrap();
        let v = open_or_init(&dir.path().to_string_lossy()).unwrap();
        assert_eq!(v, STORAGE_FORMAT_VERSION);
        // Idempotent on reopen.
        let v2 = open_or_init(&dir.path().to_string_lossy()).unwrap();
        assert_eq!(v2, STORAGE_FORMAT_VERSION);
    }

    #[test]
    fn refuses_newer_format() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(MARKER_FILE);
        write_marker(&path, STORAGE_FORMAT_VERSION + 5).unwrap();
        let err = open_or_init(&dir.path().to_string_lossy()).unwrap_err();
        assert!(format!("{}", err).contains("upgrade BlockDB"));
    }

    #[test]
    fn rejects_foreign_marker() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join(MARKER_FILE), b"not-blockdb\nversion=1\n").unwrap();
        assert!(open_or_init(&dir.path().to_string_lossy()).is_err());
    }
}
