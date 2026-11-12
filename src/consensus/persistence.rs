//! Durable persistence for Raft's non-volatile state.
//!
//! Raft's safety proof requires three pieces of state to survive a crash/restart,
//! persisted *before* the node acts on them:
//!   * `currentTerm` and `votedFor` — the "hard state" (so a node never double-votes
//!     in a term, and never forgets a term it has already advanced to).
//!   * the replicated `log` — so committed entries are never lost and the node can
//!     recover its view of what has been agreed.
//!
//! Without this, a cluster can elect two leaders for one term and overwrite committed
//! data. This module provides two small on-disk stores used by the Raft node.

use std::fs::{File, OpenOptions};
use std::io::{BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::consensus::log_entry::LogEntry;

/// Hard state that must be fsync'd before the node responds to votes or appends.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HardState {
    pub current_term: u64,
    pub voted_for: Option<String>,
}

/// Compaction snapshot metadata: the log baseline plus the cluster membership as of
/// that baseline. BlockDB's key/value data is NOT snapshotted here — it is already
/// durably stored by the state machine — so this stays tiny. Membership is kept so a
/// restart can reconstruct the configuration even if the AddNode/RemoveNode entries
/// that produced it have been compacted away.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SnapshotMeta {
    pub base_index: u64,
    pub base_term: u64,
    /// (node_id, host, port) for each peer in the configuration at `base_index`.
    pub membership: Vec<(String, String, u16)>,
}

/// Persists the compaction snapshot metadata with the same atomic rewrite as the hard
/// state (temp file + fsync + rename + dir fsync).
#[derive(Debug)]
pub struct SnapshotStore {
    path: PathBuf,
    tmp_path: PathBuf,
}

impl SnapshotStore {
    pub fn open(dir: &Path) -> Result<(Self, SnapshotMeta), Box<dyn std::error::Error>> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join("snapshot.bin");
        let tmp_path = dir.join("snapshot.bin.tmp");
        let meta = if path.exists() {
            let mut buf = Vec::new();
            File::open(&path)?.read_to_end(&mut buf)?;
            if buf.is_empty() {
                SnapshotMeta::default()
            } else {
                bincode::deserialize(&buf).unwrap_or_default()
            }
        } else {
            SnapshotMeta::default()
        };
        Ok((SnapshotStore { path, tmp_path }, meta))
    }

    pub fn save(&self, meta: &SnapshotMeta) -> Result<(), Box<dyn std::error::Error>> {
        let bytes = bincode::serialize(meta)?;
        {
            let mut f = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&self.tmp_path)?;
            f.write_all(&bytes)?;
            f.sync_all()?;
        }
        std::fs::rename(&self.tmp_path, &self.path)?;
        if let Some(parent) = self.path.parent() {
            if let Ok(dir) = File::open(parent) {
                let _ = dir.sync_all();
            }
        }
        Ok(())
    }
}

/// Persists `currentTerm` / `votedFor` by atomically rewriting a small file and
/// fsyncing it. Rewritten in full on every change — the state is tiny.
#[derive(Debug)]
pub struct HardStateStore {
    path: PathBuf,
    tmp_path: PathBuf,
}

impl HardStateStore {
    pub fn open(dir: &Path) -> Result<(Self, HardState), Box<dyn std::error::Error>> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join("hardstate.bin");
        let tmp_path = dir.join("hardstate.bin.tmp");

        let state = if path.exists() {
            let mut buf = Vec::new();
            File::open(&path)?.read_to_end(&mut buf)?;
            if buf.is_empty() {
                HardState::default()
            } else {
                bincode::deserialize(&buf).unwrap_or_default()
            }
        } else {
            HardState::default()
        };

        Ok((HardStateStore { path, tmp_path }, state))
    }

    /// Durably store the hard state. Writes to a temp file, fsyncs it, then renames
    /// over the real file so a crash mid-write never leaves a torn hard state.
    pub fn save(&self, state: &HardState) -> Result<(), Box<dyn std::error::Error>> {
        let bytes = bincode::serialize(state)?;
        {
            let mut f = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&self.tmp_path)?;
            f.write_all(&bytes)?;
            f.sync_all()?;
        }
        std::fs::rename(&self.tmp_path, &self.path)?;
        // fsync the directory so the rename itself is durable.
        if let Some(parent) = self.path.parent() {
            if let Ok(dir) = File::open(parent) {
                let _ = dir.sync_all();
            }
        }
        Ok(())
    }
}

/// Append-only, CRC-framed persistence for the Raft log.
///
/// Frame layout per entry: `[len: u32 BE][crc32: u32 BE][bincode(LogEntry)]`. A torn
/// tail frame (crash mid-append) is detected by length/CRC and dropped on load, so
/// recovery stops cleanly at the last intact entry. Conflicting-entry truncation
/// (Raft §5.3) rewrites the file with the surviving prefix.
#[derive(Debug)]
pub struct LogStore {
    path: PathBuf,
    file: File,
}

impl LogStore {
    /// Open (creating if needed) the log file and return it plus all intact entries.
    pub fn open(dir: &Path) -> Result<(Self, Vec<LogEntry>), Box<dyn std::error::Error>> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join("log.bin");
        let entries = Self::read_all(&path)?;
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(&path)?;
        Ok((LogStore { path, file }, entries))
    }

    fn read_all(path: &Path) -> Result<Vec<LogEntry>, Box<dyn std::error::Error>> {
        if !path.exists() {
            return Ok(Vec::new());
        }
        let f = File::open(path)?;
        let file_len = f.metadata()?.len();
        let mut reader = BufReader::new(f);
        let mut entries = Vec::new();
        let mut pos: u64 = 0;
        loop {
            if pos + 8 > file_len {
                break;
            }
            let mut header = [0u8; 8];
            if reader.read_exact(&mut header).is_err() {
                break;
            }
            let len = u32::from_be_bytes([header[0], header[1], header[2], header[3]]) as usize;
            let crc = u32::from_be_bytes([header[4], header[5], header[6], header[7]]);
            if pos + 8 + len as u64 > file_len {
                break; // torn tail
            }
            let mut payload = vec![0u8; len];
            if reader.read_exact(&mut payload).is_err() {
                break;
            }
            if crc32(&payload) != crc {
                break; // corruption at tail
            }
            match bincode::deserialize::<LogEntry>(&payload) {
                Ok(e) => entries.push(e),
                Err(_) => break,
            }
            pos += 8 + len as u64;
        }
        Ok(entries)
    }

    /// Durably append one entry to the end of the log.
    pub fn append(&mut self, entry: &LogEntry) -> Result<(), Box<dyn std::error::Error>> {
        let payload = bincode::serialize(entry)?;
        let crc = crc32(&payload);
        self.file.seek(SeekFrom::End(0))?;
        self.file.write_all(&(payload.len() as u32).to_be_bytes())?;
        self.file.write_all(&crc.to_be_bytes())?;
        self.file.write_all(&payload)?;
        self.file.sync_data()?;
        Ok(())
    }

    /// Rewrite the whole log with exactly `entries` (used after conflict truncation or
    /// snapshot compaction). Durable via truncate + append + fsync.
    pub fn rewrite(&mut self, entries: &[LogEntry]) -> Result<(), Box<dyn std::error::Error>> {
        self.file.set_len(0)?;
        self.file.seek(SeekFrom::Start(0))?;
        for entry in entries {
            let payload = bincode::serialize(entry)?;
            let crc = crc32(&payload);
            self.file.write_all(&(payload.len() as u32).to_be_bytes())?;
            self.file.write_all(&crc.to_be_bytes())?;
            self.file.write_all(&payload)?;
        }
        self.file.sync_all()?;
        Ok(())
    }
}

/// Standard CRC-32 (IEEE polynomial), no external crate.
fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consensus::log_entry::LogOperation;
    use crate::consensus::Term;
    use tempfile::TempDir;
    use uuid::Uuid;

    fn entry(i: u64, term: u64) -> LogEntry {
        LogEntry::new(i, Term(term), LogOperation::NoOp, Uuid::new_v4())
    }

    #[test]
    fn hardstate_round_trips() {
        let dir = TempDir::new().unwrap();
        let (store, state) = HardStateStore::open(dir.path()).unwrap();
        assert_eq!(state.current_term, 0);
        store
            .save(&HardState { current_term: 7, voted_for: Some("node3".into()) })
            .unwrap();

        let (_store2, reloaded) = HardStateStore::open(dir.path()).unwrap();
        assert_eq!(reloaded.current_term, 7);
        assert_eq!(reloaded.voted_for.as_deref(), Some("node3"));
    }

    #[test]
    fn log_appends_and_recovers() {
        let dir = TempDir::new().unwrap();
        {
            let (mut store, entries) = LogStore::open(dir.path()).unwrap();
            assert!(entries.is_empty());
            store.append(&entry(1, 1)).unwrap();
            store.append(&entry(2, 1)).unwrap();
            store.append(&entry(3, 2)).unwrap();
        }
        let (_store, entries) = LogStore::open(dir.path()).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[2].index, 3);
        assert_eq!(entries[2].term.0, 2);
    }

    #[test]
    fn log_rewrite_truncates() {
        let dir = TempDir::new().unwrap();
        let (mut store, _) = LogStore::open(dir.path()).unwrap();
        store.append(&entry(1, 1)).unwrap();
        store.append(&entry(2, 1)).unwrap();
        store.append(&entry(3, 1)).unwrap();
        store.rewrite(&[entry(1, 1), entry(2, 1)]).unwrap();

        let (_s, entries) = LogStore::open(dir.path()).unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn torn_tail_is_dropped() {
        let dir = TempDir::new().unwrap();
        {
            let (mut store, _) = LogStore::open(dir.path()).unwrap();
            store.append(&entry(1, 1)).unwrap();
        }
        // Append a torn frame: claims 100 bytes, writes 4.
        let mut f = OpenOptions::new().append(true).open(dir.path().join("log.bin")).unwrap();
        f.write_all(&100u32.to_be_bytes()).unwrap();
        f.write_all(&0u32.to_be_bytes()).unwrap();
        f.write_all(b"xxxx").unwrap();
        f.sync_all().unwrap();

        let (_s, entries) = LogStore::open(dir.path()).unwrap();
        assert_eq!(entries.len(), 1, "torn tail frame must be dropped");
    }

    #[test]
    fn snapshot_meta_round_trips() {
        let dir = TempDir::new().unwrap();
        let (store, meta) = SnapshotStore::open(dir.path()).unwrap();
        assert_eq!(meta.base_index, 0);
        store
            .save(&SnapshotMeta {
                base_index: 42,
                base_term: 3,
                membership: vec![("n1".into(), "10.0.0.1".into(), 8080)],
            })
            .unwrap();

        let (_s2, reloaded) = SnapshotStore::open(dir.path()).unwrap();
        assert_eq!(reloaded.base_index, 42);
        assert_eq!(reloaded.base_term, 3);
        assert_eq!(reloaded.membership.len(), 1);
        assert_eq!(reloaded.membership[0].2, 8080);
    }

    #[test]
    fn compaction_persists_and_reconciles_on_restart() {
        use crate::consensus::log_entry::ReplicatedLog;

        let dir = TempDir::new().unwrap();

        // Phase 1: durable log with a small retention; append, apply, compact.
        {
            let (log_store, loaded) = LogStore::open(dir.path()).unwrap();
            assert!(loaded.is_empty());
            let mut log = ReplicatedLog::with_store(log_store, Vec::new(), 0, Term(0));
            log.set_compaction_retention(2);
            for i in 1..=8 {
                log.append(entry(i, 1)).unwrap();
            }
            log.update_commit_index(8);
            log.apply_up_to(8);

            // Simulate the node's compaction ordering: record the snapshot baseline
            // first, then compact the log file.
            let target = log.compaction_target().expect("should want compaction");
            let base_term = log.term_at(target).unwrap().0;
            let (snap_store, _) = SnapshotStore::open(dir.path()).unwrap();
            snap_store
                .save(&SnapshotMeta {
                    base_index: target,
                    base_term,
                    membership: vec![],
                })
                .unwrap();
            log.compact(target).unwrap();
            assert_eq!(log.base_index(), target);
        }

        // Phase 2: restart — reload snapshot + log and reconcile (drop entries at or
        // below base_index), exactly as RaftNode::new_persistent does.
        let (_snap_store, meta) = SnapshotStore::open(dir.path()).unwrap();
        let (log_store, all_entries) = LogStore::open(dir.path()).unwrap();
        let base = meta.base_index;
        assert!(base >= 6, "expected compaction to advance the base index");
        let kept: Vec<LogEntry> = all_entries.into_iter().filter(|e| e.index > base).collect();
        let log = ReplicatedLog::with_store(log_store, kept, base, Term(meta.base_term));

        assert_eq!(log.base_index(), base);
        assert_eq!(log.last_log_index(), 8, "all 8 indices still accounted for");
        assert_eq!(log.get_entry(8).unwrap().index, 8);
        assert!(log.get_entry(base).is_none(), "compacted entry stays gone after restart");
    }
}
