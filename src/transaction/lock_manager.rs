use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use dashmap::DashMap;
use parking_lot::Mutex;
use tokio::sync::Notify;
use crate::error::BlockDBError;
use crate::transaction::TransactionId;

#[derive(Debug, Clone, PartialEq)]
pub enum LockMode {
    Shared,    // Read lock
    Exclusive, // Write lock
}

#[derive(Debug)]
pub struct Lock {
    pub mode: LockMode,
    pub holder: TransactionId,
    pub acquired_at: SystemTime,
}

#[derive(Debug)]
pub struct LockRequest {
    pub transaction_id: TransactionId,
    pub mode: LockMode,
    pub requested_at: SystemTime,
    pub notify: Arc<Notify>,
}

impl LockRequest {
    pub fn new(transaction_id: TransactionId, mode: LockMode) -> Self {
        LockRequest {
            transaction_id,
            mode,
            requested_at: SystemTime::now(),
            notify: Arc::new(Notify::new()),
        }
    }
}

#[derive(Debug)]
pub struct ResourceLocks {
    pub locks: Vec<Lock>,
    pub wait_queue: VecDeque<LockRequest>,
}

impl ResourceLocks {
    pub fn new() -> Self {
        ResourceLocks {
            locks: Vec::new(),
            wait_queue: VecDeque::new(),
        }
    }
    
    pub fn can_grant(&self, mode: &LockMode, transaction_id: &TransactionId) -> bool {
        // Check if transaction already holds a compatible lock
        for lock in &self.locks {
            if &lock.holder == transaction_id {
                return match (&lock.mode, mode) {
                    (LockMode::Shared, LockMode::Shared) => true,
                    (LockMode::Exclusive, _) => true,
                    (LockMode::Shared, LockMode::Exclusive) => self.locks.len() == 1, // Only this shared lock
                };
            }
        }
        
        // Check compatibility with existing locks
        match mode {
            LockMode::Shared => {
                // Shared locks are compatible with other shared locks
                self.locks.iter().all(|lock| lock.mode == LockMode::Shared)
            }
            LockMode::Exclusive => {
                // Exclusive locks are not compatible with any other locks
                self.locks.is_empty()
            }
        }
    }
    
    pub fn grant_lock(&mut self, transaction_id: TransactionId, mode: LockMode) {
        let lock = Lock {
            mode,
            holder: transaction_id,
            acquired_at: SystemTime::now(),
        };
        self.locks.push(lock);
    }
    
    pub fn release_lock(&mut self, transaction_id: &TransactionId) -> bool {
        let initial_len = self.locks.len();
        self.locks.retain(|lock| &lock.holder != transaction_id);
        self.locks.len() != initial_len
    }
    
    pub fn has_lock(&self, transaction_id: &TransactionId) -> bool {
        self.locks.iter().any(|lock| &lock.holder == transaction_id)
    }
}

#[derive(Debug)]
pub struct LockManager {
    // Key -> ResourceLocks
    resource_locks: DashMap<Vec<u8>, Arc<Mutex<ResourceLocks>>>,
    // TransactionId -> Set of keys locked by this transaction
    transaction_locks: DashMap<TransactionId, HashSet<Vec<u8>>>,
    deadlock_detector: Arc<Mutex<DeadlockDetector>>,
    lock_timeout: Duration,
}

impl LockManager {
    pub fn new() -> Self {
        LockManager {
            resource_locks: DashMap::new(),
            transaction_locks: DashMap::new(),
            deadlock_detector: Arc::new(Mutex::new(DeadlockDetector::new())),
            lock_timeout: Duration::from_secs(10),
        }
    }
    
    pub async fn acquire_read_lock(&self, key: &[u8], transaction_id: &TransactionId) -> Result<(), BlockDBError> {
        self.acquire_lock(key, transaction_id, LockMode::Shared).await
    }
    
    pub async fn acquire_write_lock(&self, key: &[u8], transaction_id: &TransactionId) -> Result<(), BlockDBError> {
        self.acquire_lock(key, transaction_id, LockMode::Exclusive).await
    }
    
    async fn acquire_lock(&self, key: &[u8], transaction_id: &TransactionId, mode: LockMode) -> Result<(), BlockDBError> {
        let key_vec = key.to_vec();
        
        // Get or create resource locks for this key
        let resource_locks = self.resource_locks
            .entry(key_vec.clone())
            .or_insert_with(|| Arc::new(Mutex::new(ResourceLocks::new())))
            .clone();
        
        let request = {
            let mut locks = resource_locks.lock();
            
            // Check if we can grant the lock immediately
            if locks.can_grant(&mode, transaction_id) {
                locks.grant_lock(transaction_id.clone(), mode);
                
                // Track this lock for the transaction
                self.transaction_locks
                    .entry(transaction_id.clone())
                    .or_insert_with(HashSet::new)
                    .insert(key_vec);
                
                return Ok(());
            }
            
            // Cannot grant immediately, add to wait queue
            let request = LockRequest::new(transaction_id.clone(), mode);
            let notify = request.notify.clone();
            locks.wait_queue.push_back(request);
            notify
        };
        
        // Add to deadlock detector
        {
            let mut detector = self.deadlock_detector.lock();
            detector.add_wait_edge(transaction_id.clone(), &key_vec);
        }
        
        // Wait for lock to be available or timeout
        let wait_result = tokio::time::timeout(self.lock_timeout, request.notified()).await;
        
        match wait_result {
            Ok(_) => {
                // Check if we got the lock
                let mut locks = resource_locks.lock();
                if locks.has_lock(transaction_id) {
                    // Track this lock for the transaction
                    self.transaction_locks
                        .entry(transaction_id.clone())
                        .or_insert_with(HashSet::new)
                        .insert(key_vec);
                    
                    // Remove from deadlock detector
                    let mut detector = self.deadlock_detector.lock();
                    detector.remove_wait_edge(transaction_id, &key_vec);
                    
                    Ok(())
                } else {
                    Err(BlockDBError::StorageError("Failed to acquire lock".to_string()))
                }
            }
            Err(_) => {
                // Timeout - remove from wait queue
                let mut locks = resource_locks.lock();
                locks.wait_queue.retain(|req| &req.transaction_id != transaction_id);
                
                // Remove from deadlock detector
                let mut detector = self.deadlock_detector.lock();
                detector.remove_wait_edge(transaction_id, &key_vec);
                
                Err(BlockDBError::StorageError("Lock acquisition timeout".to_string()))
            }
        }
    }
    
    pub async fn release_lock(&self, key: &[u8], transaction_id: &TransactionId) -> Result<(), BlockDBError> {
        let key_vec = key.to_vec();
        
        if let Some(resource_locks) = self.resource_locks.get(&key_vec) {
            let mut locks = resource_locks.lock();
            
            if locks.release_lock(transaction_id) {
                // Remove from transaction's lock set
                if let Some(mut tx_locks) = self.transaction_locks.get_mut(transaction_id) {
                    tx_locks.remove(&key_vec);
                }
                
                // Process wait queue
                self.process_wait_queue(&mut locks);
            }
        }
        
        Ok(())
    }
    
    pub async fn release_all_locks(&self, transaction_id: &TransactionId) {
        // Get all keys locked by this transaction
        let keys: Vec<Vec<u8>> = if let Some(tx_locks) = self.transaction_locks.get(transaction_id) {
            tx_locks.iter().cloned().collect()
        } else {
            Vec::new()
        };
        
        // Release each lock
        for key in keys {
            let _ = self.release_lock(&key, transaction_id).await;
        }
        
        // Remove transaction from tracking
        self.transaction_locks.remove(transaction_id);
        
        // Remove from deadlock detector
        let mut detector = self.deadlock_detector.lock();
        detector.remove_transaction(transaction_id);
    }
    
    fn process_wait_queue(&self, locks: &mut ResourceLocks) {
        while let Some(request) = locks.wait_queue.front() {
            if locks.can_grant(&request.mode, &request.transaction_id) {
                let request = locks.wait_queue.pop_front().unwrap();
                locks.grant_lock(request.transaction_id, request.mode);
                request.notify.notify_one();
            } else {
                break;
            }
        }
    }
    
    pub async fn detect_deadlocks(&self) -> Vec<TransactionId> {
        let detector = self.deadlock_detector.lock();
        detector.detect_cycles()
    }
}

// Simple deadlock detector using wait-for graph
#[derive(Debug)]
pub struct DeadlockDetector {
    // TransactionId -> Set of resources it's waiting for
    wait_for: HashMap<TransactionId, HashSet<Vec<u8>>>,
    // Resource -> Set of transactions holding it
    held_by: HashMap<Vec<u8>, HashSet<TransactionId>>,
}

impl DeadlockDetector {
    pub fn new() -> Self {
        DeadlockDetector {
            wait_for: HashMap::new(),
            held_by: HashMap::new(),
        }
    }
    
    pub fn add_wait_edge(&mut self, transaction: TransactionId, resource: &[u8]) {
        self.wait_for
            .entry(transaction)
            .or_insert_with(HashSet::new)
            .insert(resource.to_vec());
    }
    
    pub fn remove_wait_edge(&mut self, transaction: &TransactionId, resource: &[u8]) {
        if let Some(resources) = self.wait_for.get_mut(transaction) {
            resources.remove(resource);
            if resources.is_empty() {
                self.wait_for.remove(transaction);
            }
        }
    }
    
    pub fn add_hold_edge(&mut self, transaction: TransactionId, resource: &[u8]) {
        self.held_by
            .entry(resource.to_vec())
            .or_insert_with(HashSet::new)
            .insert(transaction);
    }
    
    pub fn remove_hold_edge(&mut self, transaction: &TransactionId, resource: &[u8]) {
        if let Some(holders) = self.held_by.get_mut(resource) {
            holders.remove(transaction);
            if holders.is_empty() {
                self.held_by.remove(resource);
            }
        }
    }
    
    pub fn remove_transaction(&mut self, transaction: &TransactionId) {
        self.wait_for.remove(transaction);
        
        // Remove from all held_by sets
        for holders in self.held_by.values_mut() {
            holders.remove(transaction);
        }
        
        // Remove empty entries
        self.held_by.retain(|_, holders| !holders.is_empty());
    }
    
    pub fn detect_cycles(&self) -> Vec<TransactionId> {
        // Simple cycle detection using DFS
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();
        let mut cycles = Vec::new();
        
        for transaction in self.wait_for.keys() {
            if !visited.contains(transaction) {
                self.dfs_cycle_detection(transaction, &mut visited, &mut rec_stack, &mut cycles);
            }
        }
        
        cycles
    }
    
    fn dfs_cycle_detection(
        &self,
        transaction: &TransactionId,
        visited: &mut HashSet<TransactionId>,
        rec_stack: &mut HashSet<TransactionId>,
        cycles: &mut Vec<TransactionId>,
    ) {
        visited.insert(transaction.clone());
        rec_stack.insert(transaction.clone());
        
        // For each resource this transaction is waiting for
        if let Some(resources) = self.wait_for.get(transaction) {
            for resource in resources {
                // For each transaction holding this resource
                if let Some(holders) = self.held_by.get(resource) {
                    for holder in holders {
                        if !visited.contains(holder) {
                            self.dfs_cycle_detection(holder, visited, rec_stack, cycles);
                        } else if rec_stack.contains(holder) {
                            // Found cycle
                            cycles.push(holder.clone());
                        }
                    }
                }
            }
        }
        
        rec_stack.remove(transaction);
    }
}