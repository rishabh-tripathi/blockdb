use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use dashmap::DashMap;
use parking_lot::{RwLock, Mutex};
use serde::{Serialize, Deserialize};
use uuid::Uuid;
use crate::error::BlockDBError;

pub mod lock_manager;
pub mod transaction_log;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TransactionId(pub Uuid);

impl TransactionId {
    pub fn new() -> Self {
        TransactionId(Uuid::new_v4())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Operation {
    Put { key: Vec<u8>, value: Vec<u8> },
    Get { key: Vec<u8> },
    Delete { key: Vec<u8> }, // For future use, currently not allowed
}

impl Operation {
    pub fn get_key(&self) -> &[u8] {
        match self {
            Operation::Put { key, .. } => key,
            Operation::Get { key } => key,
            Operation::Delete { key } => key,
        }
    }
    
    pub fn is_write_operation(&self) -> bool {
        matches!(self, Operation::Put { .. } | Operation::Delete { .. })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransactionState {
    Active,
    Preparing,
    Committed,
    Aborted,
}

#[derive(Debug)]
pub struct Transaction {
    pub id: TransactionId,
    pub state: TransactionState,
    pub operations: Vec<Operation>,
    pub read_set: HashSet<Vec<u8>>,
    pub write_set: HashMap<Vec<u8>, Vec<u8>>,
    pub start_time: SystemTime,
    pub timeout: std::time::Duration,
}

impl Transaction {
    pub fn new(timeout: std::time::Duration) -> Self {
        Transaction {
            id: TransactionId::new(),
            state: TransactionState::Active,
            operations: Vec::new(),
            read_set: HashSet::new(),
            write_set: HashMap::new(),
            start_time: SystemTime::now(),
            timeout,
        }
    }
    
    pub fn add_operation(&mut self, operation: Operation) {
        match &operation {
            Operation::Get { key } => {
                self.read_set.insert(key.clone());
            }
            Operation::Put { key, value } => {
                self.write_set.insert(key.clone(), value.clone());
            }
            Operation::Delete { key } => {
                self.write_set.insert(key.clone(), Vec::new()); // Empty value for deletion
            }
        }
        self.operations.push(operation);
    }
    
    pub fn is_expired(&self) -> bool {
        SystemTime::now()
            .duration_since(self.start_time)
            .unwrap_or(std::time::Duration::ZERO) > self.timeout
    }
    
    pub fn can_commit(&self) -> bool {
        matches!(self.state, TransactionState::Active) && !self.is_expired()
    }
    
    pub fn prepare(&mut self) -> Result<(), BlockDBError> {
        if !self.can_commit() {
            return Err(BlockDBError::StorageError("Cannot prepare transaction".to_string()));
        }
        self.state = TransactionState::Preparing;
        Ok(())
    }
    
    pub fn commit(&mut self) -> Result<(), BlockDBError> {
        if !matches!(self.state, TransactionState::Preparing) {
            return Err(BlockDBError::StorageError("Transaction not in preparing state".to_string()));
        }
        self.state = TransactionState::Committed;
        Ok(())
    }
    
    pub fn abort(&mut self) {
        self.state = TransactionState::Aborted;
    }
}

#[derive(Debug)]
pub struct TransactionManager {
    active_transactions: Arc<DashMap<TransactionId, Arc<Mutex<Transaction>>>>,
    lock_manager: Arc<lock_manager::LockManager>,
    transaction_log: Arc<Mutex<transaction_log::TransactionLog>>,
    default_timeout: std::time::Duration,
}

impl TransactionManager {
    pub fn new(data_dir: &str) -> Result<Self, BlockDBError> {
        Ok(TransactionManager {
            active_transactions: Arc::new(DashMap::new()),
            lock_manager: Arc::new(lock_manager::LockManager::new()),
            transaction_log: Arc::new(Mutex::new(transaction_log::TransactionLog::new(data_dir)?)),
            default_timeout: std::time::Duration::from_secs(30),
        })
    }
    
    pub async fn begin_transaction(&self) -> Result<TransactionId, BlockDBError> {
        self.begin_transaction_with_timeout(self.default_timeout).await
    }
    
    pub async fn begin_transaction_with_timeout(&self, timeout: std::time::Duration) -> Result<TransactionId, BlockDBError> {
        let transaction = Transaction::new(timeout);
        let tx_id = transaction.id.clone();
        
        // Log transaction begin
        {
            let mut log = self.transaction_log.lock();
            log.log_begin(&tx_id)?;
        }
        
        self.active_transactions.insert(tx_id.clone(), Arc::new(Mutex::new(transaction)));
        Ok(tx_id)
    }
    
    pub async fn execute_operation(&self, tx_id: &TransactionId, operation: Operation) -> Result<Option<Vec<u8>>, BlockDBError> {
        let tx_arc = self.active_transactions.get(tx_id)
            .ok_or_else(|| BlockDBError::StorageError("Transaction not found".to_string()))?
            .clone();
        
        let mut tx = tx_arc.lock();
        
        if !tx.can_commit() {
            return Err(BlockDBError::StorageError("Transaction expired or invalid".to_string()));
        }
        
        // Acquire appropriate locks
        match &operation {
            Operation::Get { key } => {
                self.lock_manager.acquire_read_lock(key, tx_id).await?;
            }
            Operation::Put { key, .. } | Operation::Delete { key } => {
                self.lock_manager.acquire_write_lock(key, tx_id).await?;
            }
        }
        
        tx.add_operation(operation.clone());
        
        // For read operations, return the value
        match operation {
            Operation::Get { key } => {
                // Check write set first (read your own writes)
                if let Some(value) = tx.write_set.get(&key) {
                    Ok(Some(value.clone()))
                } else {
                    // TODO: Integration with storage layer would happen here
                    // For now, return None to indicate not found in transaction
                    // In a complete implementation, this would read from the storage layer
                    Ok(None)
                }
            }
            _ => Ok(None),
        }
    }
    
    pub async fn prepare_transaction(&self, tx_id: &TransactionId) -> Result<(), BlockDBError> {
        let tx_arc = self.active_transactions.get(tx_id)
            .ok_or_else(|| BlockDBError::StorageError("Transaction not found".to_string()))?
            .clone();
        
        let mut tx = tx_arc.lock();
        tx.prepare()?;
        
        // Log prepare
        let mut log = self.transaction_log.lock();
        log.log_prepare(tx_id)?;
        
        Ok(())
    }
    
    pub async fn commit_transaction(&self, tx_id: &TransactionId) -> Result<(), BlockDBError> {
        let tx_arc = self.active_transactions.get(tx_id)
            .ok_or_else(|| BlockDBError::StorageError("Transaction not found".to_string()))?
            .clone();
        
        let mut tx = tx_arc.lock();
        tx.commit()?;
        
        // Log commit
        {
            let mut log = self.transaction_log.lock();
            log.log_commit(tx_id)?;
        }
        
        // Release all locks
        self.lock_manager.release_all_locks(tx_id).await;
        
        // Remove from active transactions
        self.active_transactions.remove(tx_id);
        
        Ok(())
    }
    
    pub async fn abort_transaction(&self, tx_id: &TransactionId) -> Result<(), BlockDBError> {
        if let Some((_, tx_arc)) = self.active_transactions.remove(tx_id) {
            let mut tx = tx_arc.lock();
            tx.abort();
            
            // Log abort
            {
                let mut log = self.transaction_log.lock();
                log.log_abort(tx_id)?;
            }
            
            // Release all locks
            self.lock_manager.release_all_locks(tx_id).await;
        }
        
        Ok(())
    }
    
    pub async fn cleanup_expired_transactions(&self) -> Result<(), BlockDBError> {
        let expired_transactions: Vec<TransactionId> = self.active_transactions
            .iter()
            .filter_map(|entry| {
                let tx = entry.value().lock();
                if tx.is_expired() {
                    Some(tx.id.clone())
                } else {
                    None
                }
            })
            .collect();
        
        for tx_id in expired_transactions {
            self.abort_transaction(&tx_id).await?;
        }
        
        Ok(())
    }
    
    pub fn get_transaction(&self, tx_id: &TransactionId) -> Option<Arc<Mutex<Transaction>>> {
        self.active_transactions.get(tx_id).map(|entry| entry.value().clone())
    }
    
    pub fn active_transaction_count(&self) -> usize {
        self.active_transactions.len()
    }
}

// 2PC (Two-Phase Commit) implementation for distributed transactions
#[derive(Debug)]
pub struct TwoPhaseCommitCoordinator {
    transaction_manager: Arc<TransactionManager>,
    participants: Vec<String>, // Node addresses
}

impl TwoPhaseCommitCoordinator {
    pub fn new(transaction_manager: Arc<TransactionManager>, participants: Vec<String>) -> Self {
        TwoPhaseCommitCoordinator {
            transaction_manager,
            participants,
        }
    }
    
    pub async fn execute_distributed_transaction(&self, tx_id: &TransactionId, operations: Vec<Operation>) -> Result<(), BlockDBError> {
        // Phase 1: Prepare
        let mut prepare_success = true;
        
        for operation in operations {
            if let Err(_) = self.transaction_manager.execute_operation(tx_id, operation).await {
                prepare_success = false;
                break;
            }
        }
        
        if prepare_success {
            if let Err(_) = self.transaction_manager.prepare_transaction(tx_id).await {
                prepare_success = false;
            }
        }
        
        // TODO: Send prepare requests to all participants
        // For now, assume all participants are ready
        
        // Phase 2: Commit or Abort
        if prepare_success {
            self.transaction_manager.commit_transaction(tx_id).await?;
            // TODO: Send commit requests to all participants
        } else {
            self.transaction_manager.abort_transaction(tx_id).await?;
            // TODO: Send abort requests to all participants
        }
        
        Ok(())
    }
}