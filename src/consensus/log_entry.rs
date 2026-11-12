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
                .unwrap()
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
    /// Durable backing store. When present, every log mutation is fsync'd before it
    /// is acknowledged, so committed entries survive a crash (Raft durability).
    store: Option<LogStore>,
}

impl ReplicatedLog {
    /// In-memory-only log (tests, single-node demos without persistence).
    pub fn new() -> Self {
        ReplicatedLog {
            entries: Vec::new(),
            commit_index: 0,
            last_applied: 0,
            store: None,
        }
    }

    /// Durable log: `store` holds the already-loaded entries recovered from disk.
    pub fn with_store(store: LogStore, entries: Vec<LogEntry>) -> Self {
        ReplicatedLog {
            entries,
            commit_index: 0,
            last_applied: 0,
            store: Some(store),
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
        // Validate prev_log_index
        if prev_log_index > 0 && prev_log_index as usize > self.entries.len() {
            return Ok(false);
        }

        // Truncate conflicting suffix (durably rewrite the prefix that survives).
        if prev_log_index < self.entries.len() as u64 {
            self.entries.truncate(prev_log_index as usize);
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
        if index == 0 || index > self.entries.len() as u64 {
            None
        } else {
            self.entries.get((index - 1) as usize)
        }
    }
    
    pub fn last_log_index(&self) -> u64 {
        self.entries.len() as u64
    }
    
    pub fn last_log_term(&self) -> Term {
        self.entries
            .last()
            .map(|entry| entry.term.clone())
            .unwrap_or(Term::new())
    }
    
    pub fn get_term(&self, index: u64) -> Option<Term> {
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
        if start_index == 0 || start_index > self.entries.len() as u64 {
            Vec::new()
        } else {
            self.entries[(start_index - 1) as usize..].to_vec()
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