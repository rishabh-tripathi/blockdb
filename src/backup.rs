//! Backup and restore of a node's data directory, with an integrity manifest.
//!
//! A backup is a full copy of a data directory plus a `MANIFEST.json` recording every
//! file's size and SHA-256. Restore copies the files back and verifies them against the
//! manifest, so a corrupted or truncated backup is caught before it is trusted.
//!
//! Consistency: for a crash-consistent backup of a *running* node, call it a hot
//! backup only after a flush, or (simplest and strongest) take a cold backup with the
//! node stopped. BlockDB's files are crash-safe (fsync'd WAL, atomic renames), so even
//! a hot copy restores to a consistent point, losing at most un-flushed recent writes —
//! the same guarantee as a crash.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::BlockDBError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupManifest {
    pub created_at_ms: u64,
    pub source: String,
    pub files: BTreeMap<String, FileEntry>,
}

const MANIFEST_NAME: &str = "MANIFEST.json";

fn sha256_file(path: &Path) -> Result<(u64, String), BlockDBError> {
    let mut f = std::fs::File::open(path)
        .map_err(|e| BlockDBError::StorageError(format!("open {}: {}", path.display(), e)))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    let mut size = 0u64;
    loop {
        let n = f
            .read(&mut buf)
            .map_err(|e| BlockDBError::StorageError(format!("read {}: {}", path.display(), e)))?;
        if n == 0 {
            break;
        }
        size += n as u64;
        hasher.update(&buf[..n]);
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for b in digest {
        hex.push_str(&format!("{:02x}", b));
    }
    Ok((size, hex))
}

/// Recursively list files under `root`, returning paths relative to it.
fn list_files(root: &Path) -> Result<Vec<PathBuf>, BlockDBError> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let rd = std::fs::read_dir(&dir)
            .map_err(|e| BlockDBError::StorageError(format!("read dir {}: {}", dir.display(), e)))?;
        for entry in rd {
            let entry = entry.map_err(|e| BlockDBError::StorageError(format!("dir entry: {}", e)))?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let rel = path
                    .strip_prefix(root)
                    .map_err(|_| BlockDBError::StorageError("path prefix".into()))?
                    .to_path_buf();
                // Never copy a previous manifest into a new backup.
                if rel.to_string_lossy() != MANIFEST_NAME {
                    out.push(rel);
                }
            }
        }
    }
    out.sort();
    Ok(out)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Back up `data_dir` into `dest_dir` (created if needed), writing a manifest.
pub fn backup(data_dir: &Path, dest_dir: &Path) -> Result<BackupManifest, BlockDBError> {
    if !data_dir.is_dir() {
        return Err(BlockDBError::StorageError(format!(
            "data dir '{}' does not exist",
            data_dir.display()
        )));
    }
    std::fs::create_dir_all(dest_dir)
        .map_err(|e| BlockDBError::StorageError(format!("create dest: {}", e)))?;

    let mut files = BTreeMap::new();
    for rel in list_files(data_dir)? {
        let src = data_dir.join(&rel);
        let dst = dest_dir.join(&rel);
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| BlockDBError::StorageError(format!("mkdir: {}", e)))?;
        }
        std::fs::copy(&src, &dst)
            .map_err(|e| BlockDBError::StorageError(format!("copy {}: {}", src.display(), e)))?;
        let (size, sha256) = sha256_file(&dst)?;
        files.insert(rel.to_string_lossy().to_string(), FileEntry { size, sha256 });
    }

    let manifest = BackupManifest {
        created_at_ms: now_ms(),
        source: data_dir.to_string_lossy().to_string(),
        files,
    };
    let body = serde_json::to_vec_pretty(&manifest)
        .map_err(|e| BlockDBError::InvalidData(format!("serialize manifest: {}", e)))?;
    std::fs::write(dest_dir.join(MANIFEST_NAME), body)
        .map_err(|e| BlockDBError::StorageError(format!("write manifest: {}", e)))?;
    Ok(manifest)
}

/// Verify a backup directory against its manifest. Returns the number of files checked.
pub fn verify(backup_dir: &Path) -> Result<usize, BlockDBError> {
    let manifest = read_manifest(backup_dir)?;
    for (rel, entry) in &manifest.files {
        let path = backup_dir.join(rel);
        let (size, sha256) = sha256_file(&path)?;
        if size != entry.size || sha256 != entry.sha256 {
            return Err(BlockDBError::BlockchainError(format!(
                "backup integrity check failed for '{}'",
                rel
            )));
        }
    }
    Ok(manifest.files.len())
}

/// Restore a verified backup into `data_dir`. Refuses to overwrite a non-empty
/// directory unless `force` is set.
pub fn restore(backup_dir: &Path, data_dir: &Path, force: bool) -> Result<usize, BlockDBError> {
    let manifest = read_manifest(backup_dir)?;
    // Verify the backup before trusting it.
    verify(backup_dir)?;

    if data_dir.exists() {
        let non_empty = std::fs::read_dir(data_dir)
            .map(|mut r| r.next().is_some())
            .unwrap_or(false);
        if non_empty && !force {
            return Err(BlockDBError::StorageError(format!(
                "target '{}' is not empty; pass force to overwrite",
                data_dir.display()
            )));
        }
    }
    std::fs::create_dir_all(data_dir)
        .map_err(|e| BlockDBError::StorageError(format!("create target: {}", e)))?;

    for rel in manifest.files.keys() {
        let src = backup_dir.join(rel);
        let dst = data_dir.join(rel);
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| BlockDBError::StorageError(format!("mkdir: {}", e)))?;
        }
        std::fs::copy(&src, &dst)
            .map_err(|e| BlockDBError::StorageError(format!("restore {}: {}", rel, e)))?;
    }
    Ok(manifest.files.len())
}

fn read_manifest(backup_dir: &Path) -> Result<BackupManifest, BlockDBError> {
    let body = std::fs::read(backup_dir.join(MANIFEST_NAME)).map_err(|e| {
        BlockDBError::StorageError(format!("read manifest in {}: {}", backup_dir.display(), e))
    })?;
    serde_json::from_slice(&body)
        .map_err(|e| BlockDBError::InvalidData(format!("parse manifest: {}", e)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write(dir: &Path, rel: &str, content: &[u8]) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }

    #[test]
    fn backup_verify_restore_roundtrip() {
        let src = TempDir::new().unwrap();
        write(src.path(), "wal.log", b"hello wal");
        write(src.path(), "raft/log.bin", b"raft entries");
        write(src.path(), "FORMAT", b"blockdb-format\nversion=1\n");

        let bdir = TempDir::new().unwrap();
        let m = backup(src.path(), bdir.path()).unwrap();
        assert_eq!(m.files.len(), 3);
        assert_eq!(verify(bdir.path()).unwrap(), 3);

        // Restore into a fresh dir and confirm contents match.
        let dst = TempDir::new().unwrap();
        let restore_dst = dst.path().join("data");
        let n = restore(bdir.path(), &restore_dst, false).unwrap();
        assert_eq!(n, 3);
        assert_eq!(std::fs::read(restore_dst.join("wal.log")).unwrap(), b"hello wal");
        assert_eq!(std::fs::read(restore_dst.join("raft/log.bin")).unwrap(), b"raft entries");
    }

    #[test]
    fn verify_detects_corruption() {
        let src = TempDir::new().unwrap();
        write(src.path(), "data.bin", b"original");
        let bdir = TempDir::new().unwrap();
        backup(src.path(), bdir.path()).unwrap();

        // Corrupt a backed-up file.
        std::fs::write(bdir.path().join("data.bin"), b"tampered!").unwrap();
        assert!(verify(bdir.path()).is_err());
        // Restore refuses a corrupt backup.
        let dst = TempDir::new().unwrap();
        assert!(restore(bdir.path(), &dst.path().join("x"), false).is_err());
    }

    #[test]
    fn restore_refuses_nonempty_without_force() {
        let src = TempDir::new().unwrap();
        write(src.path(), "a", b"1");
        let bdir = TempDir::new().unwrap();
        backup(src.path(), bdir.path()).unwrap();

        let dst = TempDir::new().unwrap();
        write(dst.path(), "existing", b"keep");
        assert!(restore(bdir.path(), dst.path(), false).is_err());
        assert!(restore(bdir.path(), dst.path(), true).is_ok());
    }
}
