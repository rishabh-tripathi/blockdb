//! Tamper-evident security audit log.
//!
//! Compliance regimes (GDPR, SOC 2, ISO 27001) require an auditable trail of
//! security-relevant events: authentications, permission changes, membership changes,
//! and data erasures. This log appends one JSON record per event to a file, and each
//! record is hash-chained to the previous one (`hash = SHA-256(prev_hash || canonical
//! event)`), so any insertion, deletion or modification of a past record breaks the
//! chain and is detectable by `verify`.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::BlockDBError;

/// Outcome of an audited action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Success,
    Failure,
}

/// A single audited security event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub timestamp_ms: u64,
    /// Who performed the action (user id / node id), if known.
    pub actor: Option<String>,
    /// What was done, e.g. "auth.login", "permission.grant", "cluster.add_node",
    /// "gdpr.erase".
    pub action: String,
    /// What the action was performed on (target user, node, subject), if applicable.
    pub target: Option<String>,
    pub outcome: Outcome,
    /// Free-form detail (reason, permission name, address, ...).
    pub detail: Option<String>,
}

impl AuditEvent {
    fn canonical(&self) -> Vec<u8> {
        // Stable, field-ordered encoding independent of JSON key order.
        let mut buf = Vec::new();
        buf.extend_from_slice(&self.timestamp_ms.to_be_bytes());
        buf.extend_from_slice(self.actor.as_deref().unwrap_or("").as_bytes());
        buf.push(0);
        buf.extend_from_slice(self.action.as_bytes());
        buf.push(0);
        buf.extend_from_slice(self.target.as_deref().unwrap_or("").as_bytes());
        buf.push(0);
        buf.extend_from_slice(match self.outcome {
            Outcome::Success => b"success",
            Outcome::Failure => b"failure",
        });
        buf.push(0);
        buf.extend_from_slice(self.detail.as_deref().unwrap_or("").as_bytes());
        buf
    }
}

/// One line of the audit file: the event plus the hash-chain link.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct AuditRecord {
    event: AuditEvent,
    prev_hash: String,
    hash: String,
}

fn chain_hash(prev_hash: &str, event: &AuditEvent) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prev_hash.as_bytes());
    hasher.update(event.canonical());
    let digest = hasher.finalize();
    let mut s = String::with_capacity(64);
    for b in digest {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

const GENESIS_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// Append-only, hash-chained audit log backed by a file.
#[derive(Debug)]
pub struct AuditLog {
    path: PathBuf,
    inner: Mutex<AuditState>,
}

#[derive(Debug)]
struct AuditState {
    file: File,
    last_hash: String,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

impl AuditLog {
    /// Open (or create) the audit log at `dir/audit.log`, resuming the hash chain.
    pub fn open(dir: &Path) -> Result<Self, BlockDBError> {
        std::fs::create_dir_all(dir)
            .map_err(|e| BlockDBError::StorageError(format!("create audit dir: {}", e)))?;
        let path = dir.join("audit.log");

        // Recover the last chain hash by reading the final record.
        let last_hash = Self::read_all(&path)?
            .last()
            .map(|r| r.hash.clone())
            .unwrap_or_else(|| GENESIS_HASH.to_string());

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| BlockDBError::StorageError(format!("open audit log: {}", e)))?;

        Ok(AuditLog {
            path,
            inner: Mutex::new(AuditState { file, last_hash }),
        })
    }

    fn read_all(path: &Path) -> Result<Vec<AuditRecord>, BlockDBError> {
        if !path.exists() {
            return Ok(Vec::new());
        }
        let file =
            File::open(path).map_err(|e| BlockDBError::StorageError(format!("read audit: {}", e)))?;
        let mut records = Vec::new();
        for line in BufReader::new(file).lines() {
            let line = line.map_err(|e| BlockDBError::StorageError(format!("read audit: {}", e)))?;
            if line.trim().is_empty() {
                continue;
            }
            let rec: AuditRecord = serde_json::from_str(&line)
                .map_err(|e| BlockDBError::InvalidData(format!("corrupt audit record: {}", e)))?;
            records.push(rec);
        }
        Ok(records)
    }

    /// Append an event, linking it into the hash chain, and fsync it.
    pub fn record(&self, event: AuditEvent) -> Result<(), BlockDBError> {
        let mut st = self.inner.lock().unwrap();
        let prev_hash = st.last_hash.clone();
        let hash = chain_hash(&prev_hash, &event);
        let record = AuditRecord { event, prev_hash, hash: hash.clone() };
        let line = serde_json::to_string(&record)
            .map_err(|e| BlockDBError::InvalidData(format!("serialize audit record: {}", e)))?;
        st.file
            .write_all(line.as_bytes())
            .and_then(|_| st.file.write_all(b"\n"))
            .and_then(|_| st.file.sync_data())
            .map_err(|e| BlockDBError::StorageError(format!("write audit: {}", e)))?;
        st.last_hash = hash;
        Ok(())
    }

    /// Convenience constructor + record for common events.
    pub fn log(
        &self,
        action: &str,
        actor: Option<&str>,
        target: Option<&str>,
        outcome: Outcome,
        detail: Option<&str>,
    ) {
        let event = AuditEvent {
            timestamp_ms: now_ms(),
            actor: actor.map(|s| s.to_string()),
            action: action.to_string(),
            target: target.map(|s| s.to_string()),
            outcome,
            detail: detail.map(|s| s.to_string()),
        };
        if let Err(e) = self.record(event) {
            tracing::error!(error = %e, action, "failed to write audit record");
        }
    }

    /// Verify the hash chain over the whole log: every record must link to its
    /// predecessor and recompute to its stored hash. Returns the number of records
    /// verified, or an error at the first broken link.
    pub fn verify(&self) -> Result<usize, BlockDBError> {
        let records = Self::read_all(&self.path)?;
        let mut prev = GENESIS_HASH.to_string();
        for (i, rec) in records.iter().enumerate() {
            if rec.prev_hash != prev {
                return Err(BlockDBError::BlockchainError(format!(
                    "audit chain broken at record {}: prev_hash mismatch",
                    i
                )));
            }
            let expect = chain_hash(&rec.prev_hash, &rec.event);
            if expect != rec.hash {
                return Err(BlockDBError::BlockchainError(format!(
                    "audit chain broken at record {}: hash mismatch (tampered)",
                    i
                )));
            }
            prev = rec.hash.clone();
        }
        Ok(records.len())
    }

    /// Read back all events (for export / inspection).
    pub fn events(&self) -> Result<Vec<AuditEvent>, BlockDBError> {
        Ok(Self::read_all(&self.path)?.into_iter().map(|r| r.event).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn records_chain_and_verify() {
        let dir = TempDir::new().unwrap();
        let log = AuditLog::open(dir.path()).unwrap();
        log.log("auth.login", Some("alice"), None, Outcome::Success, None);
        log.log("auth.login", Some("mallory"), None, Outcome::Failure, Some("bad password"));
        log.log("permission.grant", Some("admin"), Some("bob"), Outcome::Success, Some("Write"));
        assert_eq!(log.verify().unwrap(), 3);
        assert_eq!(log.events().unwrap().len(), 3);
    }

    #[test]
    fn chain_resumes_across_reopen() {
        let dir = TempDir::new().unwrap();
        {
            let log = AuditLog::open(dir.path()).unwrap();
            log.log("a", None, None, Outcome::Success, None);
        }
        let log2 = AuditLog::open(dir.path()).unwrap();
        log2.log("b", None, None, Outcome::Success, None);
        assert_eq!(log2.verify().unwrap(), 2);
    }

    #[test]
    fn tampering_is_detected() {
        let dir = TempDir::new().unwrap();
        {
            let log = AuditLog::open(dir.path()).unwrap();
            log.log("auth.login", Some("alice"), None, Outcome::Success, None);
            log.log("gdpr.erase", Some("admin"), Some("alice"), Outcome::Success, None);
        }
        // Tamper: rewrite the file flipping an outcome in the first record.
        let path = dir.path().join("audit.log");
        let content = std::fs::read_to_string(&path).unwrap();
        let tampered = content.replacen("\"success\"", "\"failure\"", 1);
        assert_ne!(content, tampered);
        std::fs::write(&path, tampered).unwrap();

        let log = AuditLog::open(dir.path()).unwrap();
        assert!(log.verify().is_err(), "tampering must break the hash chain");
    }
}
