pub mod storage;
pub mod api;
pub mod error;
pub mod consensus;
pub mod transaction;
pub mod distributed;
pub mod auth;

pub use storage::{BlockDB, BlockDBConfig, Record};
pub use api::{BlockDBServer, ApiConfig};
pub use error::BlockDBError;
pub use distributed::{DistributedBlockDB, DistributedBlockDBConfig};
pub use transaction::{TransactionManager, TransactionId, Operation};
pub use consensus::{NodeId, NodeAddress, ClusterConfig};
pub use auth::{AuthManager, AuthContext, AuthError, Permission, PermissionSet, CryptoIdentity, AuthenticatedDistributedBlockDB};

use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct BlockDBHandle {
    db: Arc<RwLock<BlockDB>>,
}

impl BlockDBHandle {
    pub fn new(config: BlockDBConfig) -> Result<Self, BlockDBError> {
        let db = BlockDB::new(config)?;
        Ok(BlockDBHandle {
            db: Arc::new(RwLock::new(db)),
        })
    }

    pub async fn put(&self, key: &[u8], value: &[u8]) -> Result<(), BlockDBError> {
        let db = self.db.read().await;
        db.put(key, value)?;
        Ok(())
    }

    pub async fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, BlockDBError> {
        let db = self.db.read().await;
        db.get(key).map_err(BlockDBError::from)
    }

    pub async fn verify_integrity(&self) -> Result<bool, BlockDBError> {
        let db = self.db.read().await;
        db.verify_integrity().map_err(BlockDBError::from)
    }

    pub async fn force_flush(&self) -> Result<(), BlockDBError> {
        let db = self.db.write().await;
        db.force_flush_memtable().map_err(BlockDBError::from)
    }

    /// Flush all data in the database (dangerous - clears everything)
    pub async fn flush_all(&self) -> Result<(), BlockDBError> {
        let db = self.db.write().await;
        db.flush_all().map_err(BlockDBError::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_basic_put_get() {
        let temp_dir = TempDir::new().unwrap();
        let config = BlockDBConfig {
            data_dir: temp_dir.path().to_string_lossy().to_string(),
            ..Default::default()
        };

        let db = BlockDBHandle::new(config).unwrap();

        // Test put and get
        db.put(b"test_key", b"test_value").await.unwrap();
        let result = db.get(b"test_key").await.unwrap();
        assert_eq!(result, Some(b"test_value".to_vec()));

        // Test non-existent key
        let result = db.get(b"nonexistent").await.unwrap();
        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn test_multiple_puts() {
        let temp_dir = TempDir::new().unwrap();
        let config = BlockDBConfig {
            data_dir: temp_dir.path().to_string_lossy().to_string(),
            ..Default::default()
        };

        let db = BlockDBHandle::new(config).unwrap();

        // Put multiple values
        for i in 0..10 {
            let key = format!("key_{}", i);
            let value = format!("value_{}", i);
            db.put(key.as_bytes(), value.as_bytes()).await.unwrap();
        }

        // Verify all values
        for i in 0..10 {
            let key = format!("key_{}", i);
            let expected_value = format!("value_{}", i);
            let result = db.get(key.as_bytes()).await.unwrap();
            assert_eq!(result, Some(expected_value.as_bytes().to_vec()));
        }
    }

    #[tokio::test]
    async fn test_blockchain_integrity() {
        let temp_dir = TempDir::new().unwrap();
        let config = BlockDBConfig {
            data_dir: temp_dir.path().to_string_lossy().to_string(),
            ..Default::default()
        };

        let db = BlockDBHandle::new(config).unwrap();

        // Add some data
        db.put(b"key1", b"value1").await.unwrap();
        db.put(b"key2", b"value2").await.unwrap();

        // Verify blockchain integrity
        let is_valid = db.verify_integrity().await.unwrap();
        assert!(is_valid);
    }

    #[tokio::test]
    async fn test_large_values() {
        let temp_dir = TempDir::new().unwrap();
        let config = BlockDBConfig {
            data_dir: temp_dir.path().to_string_lossy().to_string(),
            ..Default::default()
        };

        let db = BlockDBHandle::new(config).unwrap();

        // Test large value
        let large_value = vec![b'A'; 1024 * 1024]; // 1MB
        db.put(b"large_key", &large_value).await.unwrap();
        
        let result = db.get(b"large_key").await.unwrap();
        assert_eq!(result, Some(large_value));
    }

    #[tokio::test]
    async fn test_binary_data() {
        let temp_dir = TempDir::new().unwrap();
        let config = BlockDBConfig {
            data_dir: temp_dir.path().to_string_lossy().to_string(),
            ..Default::default()
        };

        let db = BlockDBHandle::new(config).unwrap();

        // Test binary data
        let binary_data = vec![0u8, 1, 2, 3, 255, 128, 64];
        db.put(b"binary_key", &binary_data).await.unwrap();
        
        let result = db.get(b"binary_key").await.unwrap();
        assert_eq!(result, Some(binary_data));
    }

    #[tokio::test]
    async fn test_wal_recovery() {
        let temp_dir = TempDir::new().unwrap();
        let config = BlockDBConfig {
            data_dir: temp_dir.path().to_string_lossy().to_string(),
            ..Default::default()
        };

        // Create first database instance and write data
        {
            let db = BlockDBHandle::new(config.clone()).unwrap();
            db.put(b"key1", b"value1").await.unwrap();
            db.put(b"key2", b"value2").await.unwrap();
        }

        // Create second database instance and verify data is recovered
        {
            let db = BlockDBHandle::new(config).unwrap();
            let result1 = db.get(b"key1").await.unwrap();
            let result2 = db.get(b"key2").await.unwrap();
            
            assert_eq!(result1, Some(b"value1".to_vec()));
            assert_eq!(result2, Some(b"value2".to_vec()));
        }
    }

    #[tokio::test]
    async fn test_append_only_behavior() {
        let temp_dir = TempDir::new().unwrap();
        let config = BlockDBConfig {
            data_dir: temp_dir.path().to_string_lossy().to_string(),
            ..Default::default()
        };

        let db = BlockDBHandle::new(config).unwrap();

        // First put should succeed
        db.put(b"key1", b"value1").await.unwrap();
        
        // Verify the value was stored
        let result = db.get(b"key1").await.unwrap();
        assert_eq!(result, Some(b"value1".to_vec()));

        // Second put with same key should fail
        let result = db.put(b"key1", b"value2").await;
        assert!(result.is_err());
        
        // Verify the original value is preserved
        let result = db.get(b"key1").await.unwrap();
        assert_eq!(result, Some(b"value1".to_vec()));

        // Different key should work
        db.put(b"key2", b"value2").await.unwrap();
        let result = db.get(b"key2").await.unwrap();
        assert_eq!(result, Some(b"value2".to_vec()));

        // But updating key2 should also fail
        let result = db.put(b"key2", b"updated_value").await;
        assert!(result.is_err());
        
        // Verify key2 original value is preserved
        let result = db.get(b"key2").await.unwrap();
        assert_eq!(result, Some(b"value2".to_vec()));
    }

    #[tokio::test]
    async fn test_append_only_across_sessions() {
        let temp_dir = TempDir::new().unwrap();
        let config = BlockDBConfig {
            data_dir: temp_dir.path().to_string_lossy().to_string(),
            ..Default::default()
        };

        // First session: create key
        {
            let db = BlockDBHandle::new(config.clone()).unwrap();
            db.put(b"persistent_key", b"persistent_value").await.unwrap();
        }

        // Second session: try to update the same key
        {
            let db = BlockDBHandle::new(config.clone()).unwrap();
            let result = db.put(b"persistent_key", b"new_value").await;
            assert!(result.is_err());
            
            // Verify original value is preserved
            let result = db.get(b"persistent_key").await.unwrap();
            assert_eq!(result, Some(b"persistent_value".to_vec()));
        }
    }
}