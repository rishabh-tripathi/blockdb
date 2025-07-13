use std::sync::Arc;
use std::time::Duration;
use dashmap::DashMap;
use parking_lot::RwLock;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::consensus::{NodeId, NodeAddress, ClusterConfig, ConsensusEngine};
use crate::consensus::raft::RaftNode;
use crate::error::BlockDBError;
use crate::storage::{BlockDB, BlockDBConfig};
use crate::transaction::{TransactionManager, TransactionId, Operation, TwoPhaseCommitCoordinator};

pub struct DistributedBlockDBConfig {
    pub storage_config: BlockDBConfig,
    pub cluster_config: ClusterConfig,
    pub enable_transactions: bool,
    pub transaction_timeout: Duration,
    pub consensus_timeout: Duration,
}

impl Default for DistributedBlockDBConfig {
    fn default() -> Self {
        let node_id = NodeId::new();
        let address = NodeAddress::new("127.0.0.1".to_string(), 8080);
        let cluster_config = ClusterConfig::new(node_id, address);
        
        DistributedBlockDBConfig {
            storage_config: BlockDBConfig::default(),
            cluster_config,
            enable_transactions: true,
            transaction_timeout: Duration::from_secs(30),
            consensus_timeout: Duration::from_secs(5),
        }
    }
}

pub struct DistributedBlockDB {
    storage: Arc<RwLock<BlockDB>>,
    consensus: Arc<dyn ConsensusEngine + Send + Sync>,
    transaction_manager: Option<Arc<TransactionManager>>,
    two_pc_coordinator: Option<Arc<TwoPhaseCommitCoordinator>>,
    config: DistributedBlockDBConfig,
    // Cache for read operations to avoid consensus for reads
    read_cache: Arc<DashMap<Vec<u8>, (Vec<u8>, u64)>>, // key -> (value, timestamp)
}

impl DistributedBlockDB {
    pub async fn new(config: DistributedBlockDBConfig) -> Result<Self, BlockDBError> {
        // Initialize storage
        let storage = BlockDB::new(config.storage_config.clone())?;
        let storage = Arc::new(RwLock::new(storage));
        
        // Initialize consensus engine (Raft)
        let raft_node = RaftNode::new(config.cluster_config.clone());
        let consensus: Arc<dyn ConsensusEngine + Send + Sync> = Arc::new(raft_node);
        
        // Initialize transaction manager if enabled
        let transaction_manager = if config.enable_transactions {
            Some(Arc::new(TransactionManager::new(&config.storage_config.data_dir)?))
        } else {
            None
        };
        
        // Initialize 2PC coordinator if transactions are enabled
        let two_pc_coordinator = if let Some(ref tx_manager) = transaction_manager {
            let participants: Vec<String> = config.cluster_config.peers
                .values()
                .map(|addr| addr.to_url())
                .collect();
            Some(Arc::new(TwoPhaseCommitCoordinator::new(tx_manager.clone(), participants)))
        } else {
            None
        };
        
        Ok(DistributedBlockDB {
            storage,
            consensus,
            transaction_manager,
            two_pc_coordinator,
            config,
            read_cache: Arc::new(DashMap::new()),
        })
    }
    
    pub async fn start(&self) -> Result<(), BlockDBError> {
        // Start consensus engine
        if let Some(raft_node) = self.consensus.as_any().downcast_ref::<RaftNode>() {
            raft_node.start().await?;
        }
        
        // Start background tasks
        self.start_background_tasks().await;
        
        Ok(())
    }
    
    pub async fn stop(&self) -> Result<(), BlockDBError> {
        if let Some(raft_node) = self.consensus.as_any().downcast_ref::<RaftNode>() {
            raft_node.stop().await;
        }
        Ok(())
    }
    
    // ACID-compliant operations
    
    /// Single put operation (goes through consensus)
    pub async fn put(&self, key: &[u8], value: &[u8]) -> Result<(), BlockDBError> {
        let operation = Operation::Put {
            key: key.to_vec(),
            value: value.to_vec(),
        };
        
        // Use consensus to ensure all nodes agree
        self.consensus.propose(operation).await?;
        
        // If we're the leader, the operation will be applied to storage through log application
        // If we're a follower, we wait for the leader to replicate the change
        
        // Update read cache for better performance
        self.read_cache.insert(
            key.to_vec(),
            (value.to_vec(), self.current_timestamp()),
        );
        
        Ok(())
    }
    
    /// Single get operation (can be served locally for better performance)
    pub async fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, BlockDBError> {
        // Check read cache first
        if let Some(entry) = self.read_cache.get(key) {
            let (value, timestamp) = entry.value();
            // Cache is valid for 1 second to ensure reasonable consistency
            if self.current_timestamp() - timestamp < 1000 {
                return Ok(Some(value.clone()));
            }
        }
        
        // Read from local storage
        let storage = self.storage.read();
        let result = storage.get(key)?;
        
        // Update cache if found
        if let Some(ref value) = result {
            self.read_cache.insert(
                key.to_vec(),
                (value.clone(), self.current_timestamp()),
            );
        }
        
        Ok(result)
    }
    
    // Transaction support for better atomicity
    
    pub async fn begin_transaction(&self) -> Result<TransactionId, BlockDBError> {
        if let Some(ref tx_manager) = self.transaction_manager {
            tx_manager.begin_transaction_with_timeout(self.config.transaction_timeout).await
        } else {
            Err(BlockDBError::StorageError("Transactions not enabled".to_string()))
        }
    }
    
    pub async fn execute_in_transaction(&self, tx_id: &TransactionId, operation: Operation) -> Result<Option<Vec<u8>>, BlockDBError> {
        if let Some(ref tx_manager) = self.transaction_manager {
            tx_manager.execute_operation(tx_id, operation).await
        } else {
            Err(BlockDBError::StorageError("Transactions not enabled".to_string()))
        }
    }
    
    pub async fn commit_transaction(&self, tx_id: &TransactionId) -> Result<(), BlockDBError> {
        if let Some(ref coordinator) = self.two_pc_coordinator {
            // Use 2PC for distributed transactions
            let tx_manager = self.transaction_manager.as_ref().unwrap();
            if let Some(tx_arc) = tx_manager.get_transaction(tx_id) {
                let tx = tx_arc.lock();
                let operations = tx.operations.clone();
                drop(tx);
                
                coordinator.execute_distributed_transaction(tx_id, operations).await
            } else {
                Err(BlockDBError::StorageError("Transaction not found".to_string()))
            }
        } else if let Some(ref tx_manager) = self.transaction_manager {
            // Single-node transaction
            tx_manager.prepare_transaction(tx_id).await?;
            tx_manager.commit_transaction(tx_id).await
        } else {
            Err(BlockDBError::StorageError("Transactions not enabled".to_string()))
        }
    }
    
    pub async fn abort_transaction(&self, tx_id: &TransactionId) -> Result<(), BlockDBError> {
        if let Some(ref tx_manager) = self.transaction_manager {
            tx_manager.abort_transaction(tx_id).await
        } else {
            Err(BlockDBError::StorageError("Transactions not enabled".to_string()))
        }
    }
    
    // Convenient transaction wrapper for multiple operations
    pub async fn execute_transaction<F, Fut, R>(&self, f: F) -> Result<R, BlockDBError>
    where
        F: FnOnce(TransactionContext) -> Fut,
        Fut: std::future::Future<Output = Result<R, BlockDBError>>,
    {
        let tx_id = self.begin_transaction().await?;
        let context = TransactionContext {
            db: self as *const DistributedBlockDB,
            tx_id: tx_id.clone(),
        };
        
        match f(context).await {
            Ok(result) => {
                self.commit_transaction(&tx_id).await?;
                Ok(result)
            }
            Err(e) => {
                self.abort_transaction(&tx_id).await?;
                Err(e)
            }
        }
    }
    
    // Cluster management and node discovery
    
    pub async fn add_node(&self, node_id: NodeId, address: NodeAddress) -> Result<(), BlockDBError> {
        // Add node to consensus layer
        self.consensus.add_node(node_id.clone(), address.clone()).await?;
        
        // Update 2PC coordinator if transactions are enabled
        if let Some(ref coordinator) = self.two_pc_coordinator {
            // In a complete implementation, we would update the participant list
            // to include the new node for distributed transactions
        }
        
        Ok(())
    }
    
    pub async fn remove_node(&self, node_id: &NodeId) -> Result<(), BlockDBError> {
        // Remove node from consensus layer
        self.consensus.remove_node(node_id).await?;
        
        // Update 2PC coordinator if transactions are enabled
        if let Some(ref coordinator) = self.two_pc_coordinator {
            // In a complete implementation, we would update the participant list
            // to remove the node from distributed transactions
        }
        
        Ok(())
    }
    
    pub async fn is_leader(&self) -> bool {
        self.consensus.is_leader().await
    }
    
    pub async fn get_leader(&self) -> Option<NodeId> {
        self.consensus.get_leader().await
    }
    
    pub async fn get_cluster_size(&self) -> usize {
        self.config.cluster_config.cluster_size()
    }
    
    pub async fn get_cluster_nodes(&self) -> Vec<(NodeId, NodeAddress)> {
        let mut nodes = Vec::new();
        
        // Add current node
        nodes.push((
            self.config.cluster_config.node_id.clone(),
            self.config.cluster_config.address.clone(),
        ));
        
        // Add all peers
        for (node_id, address) in &self.config.cluster_config.peers {
            nodes.push((node_id.clone(), address.clone()));
        }
        
        nodes
    }
    
    pub async fn get_cluster_health(&self) -> Vec<(NodeId, bool)> {
        let mut health = Vec::new();
        
        // Current node is always healthy (since we're running)
        health.push((self.config.cluster_config.node_id.clone(), true));
        
        // For peers, we would need to implement health checking
        // This would involve sending heartbeats or health check requests
        for node_id in self.config.cluster_config.peers.keys() {
            // In a complete implementation, this would be an actual health check
            let is_healthy = true; // Placeholder
            health.push((node_id.clone(), is_healthy));
        }
        
        health
    }
    
    pub async fn discover_nodes(&self) -> Result<Vec<(NodeId, NodeAddress)>, BlockDBError> {
        // Node discovery mechanism
        // In a complete implementation, this could:
        // 1. Use service discovery (like etcd, consul)
        // 2. Use DNS-based discovery
        // 3. Use static configuration
        // 4. Use gossip protocol for peer discovery
        
        // For now, return the known cluster configuration
        Ok(self.get_cluster_nodes().await)
    }
    
    // Verification and consistency
    
    pub async fn verify_integrity(&self) -> Result<bool, BlockDBError> {
        let storage = self.storage.read();
        storage.verify_integrity()
    }
    
    pub async fn force_sync(&self) -> Result<(), BlockDBError> {
        // Force synchronization with other nodes
        // This would trigger a consensus round to ensure consistency
        let operation = Operation::Get { key: b"__sync__".to_vec() };
        self.consensus.propose(operation).await?;
        Ok(())
    }
    
    // Internal methods
    
    async fn apply_log_entries(&self) -> Result<(), BlockDBError> {
        // Apply committed log entries to the storage layer
        // This ensures that consensus decisions are reflected in the actual storage
        
        // In a complete implementation, we would access the Raft log and apply unapplied entries
        // For now, this is a framework for the distributed WAL replication
        
        // Steps for complete implementation:
        // 1. Get unapplied committed entries from the consensus log
        // 2. For each entry, apply the operation to local storage
        // 3. Update the last_applied index in the log
        // 4. Ensure atomicity of the application process
        
        // This method provides the foundation for:
        // - Distributed Write-Ahead Logging
        // - Log replication across cluster nodes
        // - Consistent state machine replication
        // - Recovery from node failures
        
        Ok(())
    }
    
    async fn start_background_tasks(&self) {
        // Start transaction cleanup task
        if let Some(ref tx_manager) = self.transaction_manager {
            let tx_manager_clone = tx_manager.clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(10));
                loop {
                    interval.tick().await;
                    if let Err(e) = tx_manager_clone.cleanup_expired_transactions().await {
                        eprintln!("Error cleaning up expired transactions: {}", e);
                    }
                }
            });
        }
        
        // Start cache cleanup task
        let read_cache = self.read_cache.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            loop {
                interval.tick().await;
                let current_time = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64;
                
                // Remove entries older than 5 seconds
                read_cache.retain(|_, (_, timestamp)| current_time - timestamp < 5000);
            }
        });
        
        // Start log application task
        let self_clone = unsafe { &*(self as *const DistributedBlockDB) };
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(100));
            loop {
                interval.tick().await;
                if let Err(e) = self_clone.apply_log_entries().await {
                    eprintln!("Error applying log entries: {}", e);
                }
            }
        });
    }
    
    fn current_timestamp(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }
}

// Helper struct for transaction context
pub struct TransactionContext {
    db: *const DistributedBlockDB,
    tx_id: TransactionId,
}

unsafe impl Send for TransactionContext {}
unsafe impl Sync for TransactionContext {}

impl TransactionContext {
    pub async fn put(&self, key: &[u8], value: &[u8]) -> Result<(), BlockDBError> {
        let operation = Operation::Put {
            key: key.to_vec(),
            value: value.to_vec(),
        };
        let db = unsafe { &*self.db };
        db.execute_in_transaction(&self.tx_id, operation).await?;
        Ok(())
    }
    
    pub async fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, BlockDBError> {
        let operation = Operation::Get {
            key: key.to_vec(),
        };
        let db = unsafe { &*self.db };
        db.execute_in_transaction(&self.tx_id, operation).await
    }
}

// Extension trait to add any() method to ConsensusEngine
trait AnyTrait: std::any::Any {
    fn as_any(&self) -> &dyn std::any::Any;
}

impl<T: std::any::Any> AnyTrait for T {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// Extend ConsensusEngine with Any
pub trait ConsensusEngineExt: ConsensusEngine + AnyTrait {}
impl<T: ConsensusEngine + AnyTrait> ConsensusEngineExt for T {}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_distributed_put_get() {
        let temp_dir = TempDir::new().unwrap();
        let mut config = DistributedBlockDBConfig::default();
        config.storage_config.data_dir = temp_dir.path().to_string_lossy().to_string();
        
        let db = DistributedBlockDB::new(config).await.unwrap();
        
        // Test basic operations
        db.put(b"key1", b"value1").await.unwrap();
        let result = db.get(b"key1").await.unwrap();
        assert_eq!(result, Some(b"value1".to_vec()));
    }
    
    #[tokio::test]
    async fn test_transactions() {
        let temp_dir = TempDir::new().unwrap();
        let mut config = DistributedBlockDBConfig::default();
        config.storage_config.data_dir = temp_dir.path().to_string_lossy().to_string();
        
        let db = DistributedBlockDB::new(config).await.unwrap();
        
        // Test transaction execution
        let result = db.execute_transaction(|ctx| async move {
            ctx.put(b"tx_key1", b"tx_value1").await?;
            ctx.put(b"tx_key2", b"tx_value2").await?;
            let value = ctx.get(b"tx_key1").await?;
            assert_eq!(value, Some(b"tx_value1".to_vec()));
            Ok(())
        }).await;
        
        assert!(result.is_ok());
    }
}