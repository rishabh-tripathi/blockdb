use serde::{Serialize, Deserialize};
use crate::consensus::{Term, NodeId};
use crate::consensus::persistence::LogStore;
use crate::error::BlockDBError;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub index: u64,
    pub term: Term,
    pub operation: LogOperation,
    pub timestamp: u64,
    pub request_id: Uuid,
}

impl LogEntry {
    pub fn new(index: u64, term: Term, operation: LogOperation, request_id: Uuid) -> Self {
        LogEntry {
            index,
            term,
            operation,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            request_id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogOperation {
    /// Database operations
    Put { key: Vec<u8>, value: Vec<u8> },
    /// Transaction operations
    BeginTransaction { transaction_id: Uuid },
    CommitTransaction { transaction_id: Uuid },
    AbortTransaction { transaction_id: Uuid },
    /// Cluster management operations
    AddNode { node_id: NodeId, address: crate::consensus::NodeAddress },
    RemoveNode { node_id: NodeId },
    /// No-op for heartbeats
    NoOp,
}

impl LogOperation {
    pub fn is_read_only(&self) -> bool {
        matches!(self, LogOperation::NoOp)
    }
    
    pub fn requires_consensus(&self) -> bool {
        !self.is_read_only()
    }
}

#[derive(Debug)]
pub struct ReplicatedLog {
    entries: Vec<LogEntry>,
    commit_index: u64,
    last_applied: u64,
    /// Index/term of the last entry discarded by compaction. `entries[0]` holds index
    /// `base_index + 1`. Entries at or below `base_index` are no longer in memory or
    /// on disk; their effects are already applied and (for BlockDB) durably stored.
    base_index: u64,
    base_term: Term,
    /// How many committed+applied entries to retain behind the commit point before
    /// compacting older ones. A larger tail lets more lagging followers catch up from
    /// the log alone (there is no InstallSnapshot RPC yet).
    compaction_retention: u64,
    /// Durable backing store. When present, every log mutation is fsync'd before it
    /// is acknowledged, so committed entries survive a crash (Raft durability).
    store: Option<LogStore>,
}

const DEFAULT_COMPACTION_RETENTION: u64 = 10_000;

impl ReplicatedLog {
    /// In-memory-only log (tests, single-node demos without persistence).
    pub fn new() -> Self {
        ReplicatedLog {
            entries: Vec::new(),
            commit_index: 0,
            last_applied: 0,
            base_index: 0,
            base_term: Term::new(),
            compaction_retention: DEFAULT_COMPACTION_RETENTION,
            store: None,
        }
    }

    /// Durable log: `store` holds the already-loaded entries recovered from disk,
    /// starting just after `base_index` (the persisted compaction baseline).
    pub fn with_store(store: LogStore, entries: Vec<LogEntry>, base_index: u64, base_term: Term) -> Self {
        let mut log = ReplicatedLog {
            entries,
            commit_index: base_index,
            last_applied: base_index,
            base_index,
            base_term,
            compaction_retention: DEFAULT_COMPACTION_RETENTION,
            store: Some(store),
        };
        // Everything up to base_index is already applied (its effects are durable).
        log.commit_index = base_index;
        log.last_applied = base_index;
        log
    }

    /// Override the retention tail (mainly for tests that exercise compaction).
    pub fn set_compaction_retention(&mut self, retention: u64) {
        self.compaction_retention = retention;
    }

    /// The compaction baseline (index of the last discarded entry).
    pub fn base_index(&self) -> u64 {
        self.base_index
    }

    pub fn base_term(&self) -> Term {
        self.base_term.clone()
    }

    /// Vec offset for an absolute log index, or None if it is compacted/out of range.
    fn offset_of(&self, index: u64) -> Option<usize> {
        if index <= self.base_index || index > self.last_log_index() {
            None
        } else {
            Some((index - self.base_index - 1) as usize)
        }
    }

    /// Append one entry, persisting it durably first (leader path).
    pub fn append(&mut self, entry: LogEntry) -> Result<(), BlockDBError> {
        if let Some(store) = self.store.as_mut() {
            store
                .append(&entry)
                .map_err(|e| BlockDBError::ConsensusError(format!("log persist failed: {}", e)))?;
        }
        self.entries.push(entry);
        Ok(())
    }

    /// Follower AppendEntries: truncate conflicts and append new entries, each change
    /// made durable before the follower acknowledges success.
    pub fn append_entries(
        &mut self,
        prev_log_index: u64,
        entries: Vec<LogEntry>,
    ) -> Result<bool, BlockDBError> {
        // Can't match against an entry we've already compacted away.
        if prev_log_index < self.base_index {
            return Ok(false);
        }
        // Validate prev_log_index is within our log.
        if prev_log_index > self.last_log_index() {
            return Ok(false);
        }

        // Truncate conflicting suffix (durably rewrite the prefix that survives).
        // `keep` is the number of in-memory entries to retain (those up to prev_log_index).
        if prev_log_index < self.last_log_index() {
            let keep = (prev_log_index - self.base_index) as usize;
            self.entries.truncate(keep);
            if let Some(store) = self.store.as_mut() {
                store.rewrite(&self.entries).map_err(|e| {
                    BlockDBError::ConsensusError(format!("log truncate persist failed: {}", e))
                })?;
            }
        }

        // Append new entries, persisting each before it is visible.
        for entry in entries {
            if let Some(store) = self.store.as_mut() {
                store.append(&entry).map_err(|e| {
                    BlockDBError::ConsensusError(format!("log append persist failed: {}", e))
                })?;
            }
            self.entries.push(entry);
        }

        Ok(true)
    }

    pub fn get_entry(&self, index: u64) -> Option<&LogEntry> {
        self.offset_of(index).and_then(|off| self.entries.get(off))
    }

    pub fn last_log_index(&self) -> u64 {
        self.base_index + self.entries.len() as u64
    }

    pub fn last_log_term(&self) -> Term {
        self.entries
            .last()
            .map(|entry| entry.term.clone())
            .unwrap_or_else(|| self.base_term.clone())
    }

    pub fn get_term(&self, index: u64) -> Option<Term> {
        if index == self.base_index {
            return Some(self.base_term.clone());
        }
        self.get_entry(index).map(|entry| entry.term.clone())
    }
    
    pub fn update_commit_index(&mut self, new_commit_index: u64) {
        if new_commit_index > self.commit_index && new_commit_index <= self.last_log_index() {
            self.commit_index = new_commit_index;
        }
    }
    
    pub fn get_commit_index(&self) -> u64 {
        self.commit_index
    }
    
    pub fn get_last_applied(&self) -> u64 {
        self.last_applied
    }
    
    pub fn get_entries_from(&self, start_index: u64) -> Vec<LogEntry> {
        // Clamp a request that reaches into compacted territory up to the first
        // in-memory entry (the leader should never ask below base_index under the
        // safe-compaction rule, but be defensive).
        let start = start_index.max(self.base_index + 1);
        match self.offset_of(start) {
            Some(off) => self.entries[off..].to_vec(),
            None => Vec::new(),
        }
    }

    /// Discard applied+committed entries at or below `up_to_index`, keeping it as the
    /// new compaction baseline. Safe: it refuses to compact unapplied entries or to
    /// cross into entries still needed (the caller passes a point that preserves the
    /// retention tail and every peer's position). Returns the new base index.
    pub fn compact(&mut self, up_to_index: u64) -> Result<u64, BlockDBError> {
        // Only compact strictly within the applied prefix, and never the whole log.
        let safe_point = up_to_index
            .min(self.last_applied)
            .min(self.last_log_index().saturating_sub(1));
        if safe_point <= self.base_index {
            return Ok(self.base_index); // nothing to do
        }

        let new_base_term = match self.get_term(safe_point) {
            Some(t) => t,
            None => return Ok(self.base_index), // can't find the boundary term; skip
        };
        let drop_count = (safe_point - self.base_index) as usize;
        self.entries.drain(0..drop_count);
        self.base_index = safe_point;
        self.base_term = new_base_term.clone();

        // Rewrite the log file with just the retained entries. The caller must have
        // already durably recorded the new (base_index, base_term) in the snapshot
        // sidecar BEFORE this, so a crash here is reconciled on load by dropping any
        // entries whose index <= base_index.
        if let Some(store) = self.store.as_mut() {
            store.rewrite(&self.entries).map_err(|e| {
                BlockDBError::ConsensusError(format!("log compaction persist failed: {}", e))
            })?;
        }
        Ok(self.base_index)
    }

    /// The term at the current base index (for recording the snapshot before compacting).
    pub fn term_at(&self, index: u64) -> Option<Term> {
        self.get_term(index)
    }

    /// Install a received snapshot: discard the entire in-memory/on-disk log and set the
    /// baseline to `(base_index, base_term)`. Used by a follower that got an
    /// InstallSnapshot because it had fallen below the leader's log. Commit/applied are
    /// set to the baseline (everything up to it is now durable in the state machine).
    pub fn install_snapshot(&mut self, base_index: u64, base_term: Term) -> Result<(), BlockDBError> {
        self.entries.clear();
        self.base_index = base_index;
        self.base_term = base_term;
        self.commit_index = base_index;
        self.last_applied = base_index;
        if let Some(store) = self.store.as_mut() {
            store.rewrite(&[]).map_err(|e| {
                BlockDBError::ConsensusError(format!("install snapshot persist failed: {}", e))
            })?;
        }
        Ok(())
    }

    /// Whether the log has grown enough past the retention tail to be worth compacting,
    /// and the index to compact up to if so.
    pub fn compaction_target(&self) -> Option<u64> {
        let applied = self.last_applied;
        if applied <= self.base_index {
            return None;
        }
        // Keep `compaction_retention` applied entries behind the applied point.
        let target = applied.saturating_sub(self.compaction_retention);
        if target > self.base_index && (target - self.base_index) >= self.compaction_retention {
            Some(target)
        } else {
            None
        }
    }
    
    pub fn apply_up_to(&mut self, index: u64) -> Vec<LogEntry> {
        let mut applied = Vec::new();

        while self.last_applied < index && self.last_applied < self.commit_index {
            self.last_applied += 1;
            if let Some(entry) = self.get_entry(self.last_applied) {
                applied.push(entry.clone());
            }
        }

        applied
    }
    
    pub fn get_unapplied_entries(&self) -> Vec<&LogEntry> {
        let mut entries = Vec::new();
        
        for index in (self.last_applied + 1)..=self.commit_index {
            if let Some(entry) = self.get_entry(index) {
                entries.push(entry);
            }
        }
        
        entries
    }
    
    pub fn mark_applied(&mut self, index: u64) {
        if index > self.last_applied && index <= self.commit_index {
            self.last_applied = index;
        }
    }
    
    pub fn is_up_to_date(&self, last_log_index: u64, last_log_term: &Term) -> bool {
        let our_last_term = self.last_log_term();
        let our_last_index = self.last_log_index();
        
        // Compare terms first, then indices
        last_log_term.0 > our_last_term.0 || 
        (last_log_term.0 == our_last_term.0 && last_log_index >= our_last_index)
    }
}
#[cfg(test)]
mod compaction_tests {
    use super::*;
    use crate::consensus::Term;

    fn e(i: u64, t: u64) -> LogEntry {
        LogEntry::new(i, Term(t), LogOperation::NoOp, Uuid::new_v4())
    }

    #[test]
    fn base_index_math_after_compaction() {
        let mut log = ReplicatedLog::new();
        for i in 1..=10 {
            log.append(e(i, 1)).unwrap();
        }
        log.update_commit_index(10);
        log.apply_up_to(10);

        let base = log.compact(5).unwrap();
        assert_eq!(base, 5);
        assert_eq!(log.base_index(), 5);
        assert_eq!(log.last_log_index(), 10, "visible length is unchanged by compaction");

        // Entries at/below the base are gone; above it are intact.
        assert!(log.get_entry(5).is_none(), "compacted entry must be unavailable");
        assert!(log.get_entry(4).is_none());
        assert_eq!(log.get_entry(6).unwrap().index, 6);
        assert_eq!(log.get_entry(10).unwrap().index, 10);

        // Term at the base is remembered (needed as prev_log_term).
        assert_eq!(log.get_term(5).map(|t| t.0), Some(1));
        assert_eq!(log.last_log_term().0, 1);

        // get_entries_from across the base clamps to the first retained entry.
        assert_eq!(log.get_entries_from(6).len(), 5);
        assert_eq!(log.get_entries_from(1).len(), 5);
    }

    #[test]
    fn append_after_compaction_keeps_indices() {
        let mut log = ReplicatedLog::new();
        for i in 1..=6 {
            log.append(e(i, 2)).unwrap();
        }
        log.update_commit_index(6);
        log.apply_up_to(6);
        log.compact(4).unwrap();

        log.append(e(7, 2)).unwrap();
        assert_eq!(log.last_log_index(), 7);
        assert_eq!(log.get_entry(7).unwrap().index, 7);
    }

    #[test]
    fn follower_append_entries_rejects_below_base() {
        let mut log = ReplicatedLog::new();
        for i in 1..=8 {
            log.append(e(i, 1)).unwrap();
        }
        log.update_commit_index(8);
        log.apply_up_to(8);
        log.compact(5).unwrap();

        // prev_log_index below the base can't be matched (entry is gone).
        assert_eq!(log.append_entries(3, vec![]).unwrap(), false);
        // prev_log_index at the current tail appends fine.
        assert_eq!(log.append_entries(8, vec![e(9, 1)]).unwrap(), true);
        assert_eq!(log.get_entry(9).unwrap().index, 9);
    }

    #[test]
    fn install_snapshot_resets_log_to_baseline() {
        let mut log = ReplicatedLog::new();
        for i in 1..=5 {
            log.append(e(i, 1)).unwrap();
        }
        log.update_commit_index(5);
        log.apply_up_to(5);

        // A follower installs a snapshot taken at index 100, term 7.
        log.install_snapshot(100, Term(7)).unwrap();
        assert_eq!(log.base_index(), 100);
        assert_eq!(log.last_log_index(), 100, "log is empty above the new baseline");
        assert_eq!(log.get_commit_index(), 100);
        assert_eq!(log.get_term(100).map(|t| t.0), Some(7));
        assert!(log.get_entry(50).is_none());

        // New entries append above the installed baseline.
        log.append_entries(100, vec![e(101, 7)]).unwrap();
        assert_eq!(log.last_log_index(), 101);
        assert_eq!(log.get_entry(101).unwrap().index, 101);
    }

    #[test]
    fn compaction_target_respects_retention() {
        let mut log = ReplicatedLog::new();
        log.set_compaction_retention(3);
        for i in 1..=10 {
            log.append(e(i, 1)).unwrap();
        }
        log.update_commit_index(10);
        log.apply_up_to(10);
        // applied=10, retention=3 -> target 7, and 7 >= retention past base.
        assert_eq!(log.compaction_target(), Some(7));

        // With the default large retention, nothing to compact.
        let mut log2 = ReplicatedLog::new();
        for i in 1..=10 {
            log2.append(e(i, 1)).unwrap();
        }
        log2.update_commit_index(10);
        log2.apply_up_to(10);
        assert_eq!(log2.compaction_target(), None);
    }
}
