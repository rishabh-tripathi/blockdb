use blockdb::{BlockDBHandle, BlockDBConfig};
use tempfile::TempDir;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::task;

/// Comprehensive storage engine tests
/// Tests LSM-tree operations, WAL, blockchain, and data integrity
#[tokio::test]
async fn test_lsm_tree_operations() {
    let temp_dir = TempDir::new().unwrap();
    let config = BlockDBConfig {
        data_dir: temp_dir.path().to_string_lossy().to_string(),
        memtable_size_limit: 1024, // Small limit to force flushes
        compaction_threshold: 2,
        ..Default::default()
    };

    let db = BlockDBHandle::new(config).unwrap();

    // Test 1: Basic put/get operations
    db.put(b"key1", b"value1").await.unwrap();
    db.put(b"key2", b"value2").await.unwrap();
    db.put(b"key3", b"value3").await.unwrap();

    let result1 = db.get(b"key1").await.unwrap();
    let result2 = db.get(b"key2").await.unwrap();
    let result3 = db.get(b"key3").await.unwrap();

    assert_eq!(result1, Some(b"value1".to_vec()));
    assert_eq!(result2, Some(b"value2".to_vec()));
    assert_eq!(result3, Some(b"value3".to_vec()));

    // Test 2: Fill up memtable to trigger flush
    for i in 4..100 {
        let key = format!("key{}", i);
        let value = format!("value{}", i);
        db.put(key.as_bytes(), value.as_bytes()).await.unwrap();
    }

    // Force a flush
    db.force_flush().await.unwrap();

    // Test 3: Verify data persists after flush
    for i in 1..100 {
        let key = format!("key{}", i);
        let expected_value = format!("value{}", i);
        let result = db.get(key.as_bytes()).await.unwrap();
        assert_eq!(result, Some(expected_value.as_bytes().to_vec()));
    }

    // Test 4: Test large values
    let large_value = vec![b'A'; 10_000]; // 10KB value
    db.put(b"large_key", &large_value).await.unwrap();
    
    let retrieved = db.get(b"large_key").await.unwrap();
    assert_eq!(retrieved, Some(large_value));

    // Test 5: Test binary data
    let binary_data = vec![0, 1, 2, 3, 255, 128, 64, 192];
    db.put(b"binary_key", &binary_data).await.unwrap();
    
    let retrieved_binary = db.get(b"binary_key").await.unwrap();
    assert_eq!(retrieved_binary, Some(binary_data));
}

#[tokio::test]
async fn test_append_only_enforcement() {
    let temp_dir = TempDir::new().unwrap();
    let config = BlockDBConfig {
        data_dir: temp_dir.path().to_string_lossy().to_string(),
        ..Default::default()
    };

    let db = BlockDBHandle::new(config).unwrap();

    // Test 1: First write should succeed
    db.put(b"append_key", b"original_value").await.unwrap();
    
    let result = db.get(b"append_key").await.unwrap();
    assert_eq!(result, Some(b"original_value".to_vec()));

    // Test 2: Second write to same key should fail
    let duplicate_result = db.put(b"append_key", b"updated_value").await;
    assert!(duplicate_result.is_err());

    // Test 3: Original value should be preserved
    let preserved_result = db.get(b"append_key").await.unwrap();
    assert_eq!(preserved_result, Some(b"original_value".to_vec()));

    // Test 4: Different key should work fine
    db.put(b"different_key", b"different_value").await.unwrap();
    let different_result = db.get(b"different_key").await.unwrap();
    assert_eq!(different_result, Some(b"different_value".to_vec()));
}

#[tokio::test]
async fn test_wal_recovery() {
    let temp_dir = TempDir::new().unwrap();
    let config = BlockDBConfig {
        data_dir: temp_dir.path().to_string_lossy().to_string(),
        wal_sync_interval_ms: 100, // Frequent syncs for testing
        ..Default::default()
    };

    // Test 1: Write data to first instance
    {
        let db = BlockDBHandle::new(config.clone()).unwrap();
        
        db.put(b"wal_key1", b"wal_value1").await.unwrap();
        db.put(b"wal_key2", b"wal_value2").await.unwrap();
        db.put(b"wal_key3", b"wal_value3").await.unwrap();
        
        // Force flush to ensure WAL is written
        db.force_flush().await.unwrap();
    } // Database instance goes out of scope

    // Test 2: Create new instance and verify recovery
    {
        let db = BlockDBHandle::new(config.clone()).unwrap();
        
        let result1 = db.get(b"wal_key1").await.unwrap();
        let result2 = db.get(b"wal_key2").await.unwrap();
        let result3 = db.get(b"wal_key3").await.unwrap();

        assert_eq!(result1, Some(b"wal_value1".to_vec()));
        assert_eq!(result2, Some(b"wal_value2".to_vec()));
        assert_eq!(result3, Some(b"wal_value3".to_vec()));
    }

    // Test 3: Verify append-only behavior persists across restarts
    {
        let db = BlockDBHandle::new(config).unwrap();
        
        // Try to update existing key - should fail
        let update_result = db.put(b"wal_key1", b"updated_value").await;
        assert!(update_result.is_err());

        // Original value should be preserved
        let preserved = db.get(b"wal_key1").await.unwrap();
        assert_eq!(preserved, Some(b"wal_value1".to_vec()));
    }
}

#[tokio::test]
async fn test_blockchain_integrity() {
    let temp_dir = TempDir::new().unwrap();
    let config = BlockDBConfig {
        data_dir: temp_dir.path().to_string_lossy().to_string(),
        blockchain_batch_size: 5, // Small batches for testing
        ..Default::default()
    };

    let db = BlockDBHandle::new(config).unwrap();

    // Test 1: Initial state should have valid blockchain
    let initial_integrity = db.verify_integrity().await.unwrap();
    assert!(initial_integrity);

    // Test 2: Add data and verify integrity
    for i in 0..10 {
        let key = format!("blockchain_key_{}", i);
        let value = format!("blockchain_value_{}", i);
        db.put(key.as_bytes(), value.as_bytes()).await.unwrap();
    }

    let post_write_integrity = db.verify_integrity().await.unwrap();
    assert!(post_write_integrity);

    // Test 3: Force flush and verify integrity
    db.force_flush().await.unwrap();
    let post_flush_integrity = db.verify_integrity().await.unwrap();
    assert!(post_flush_integrity);

    // Test 4: Restart and verify integrity persists
    let config = BlockDBConfig {
        data_dir: temp_dir.path().to_string_lossy().to_string(),
        blockchain_batch_size: 5,
        ..Default::default()
    };
    
    let db2 = BlockDBHandle::new(config).unwrap();
    let restart_integrity = db2.verify_integrity().await.unwrap();
    assert!(restart_integrity);

    // Test 5: Verify all data is still accessible
    for i in 0..10 {
        let key = format!("blockchain_key_{}", i);
        let expected_value = format!("blockchain_value_{}", i);
        let result = db2.get(key.as_bytes()).await.unwrap();
        assert_eq!(result, Some(expected_value.as_bytes().to_vec()));
    }
}

#[tokio::test]
async fn test_concurrent_operations() {
    let temp_dir = TempDir::new().unwrap();
    let config = BlockDBConfig {
        data_dir: temp_dir.path().to_string_lossy().to_string(),
        ..Default::default()
    };

    let db = Arc::new(BlockDBHandle::new(config).unwrap());

    // Test 1: Concurrent writes to different keys
    let mut handles = vec![];
    
    for i in 0..10 {
        let db_clone = db.clone();
        let handle = task::spawn(async move {
            let key = format!("concurrent_key_{}", i);
            let value = format!("concurrent_value_{}", i);
            
            db_clone.put(key.as_bytes(), value.as_bytes()).await.unwrap();
            
            // Verify the write
            let result = db_clone.get(key.as_bytes()).await.unwrap();
            assert_eq!(result, Some(value.as_bytes().to_vec()));
        });
        handles.push(handle);
    }

    // Wait for all writes to complete
    for handle in handles {
        handle.await.unwrap();
    }

    // Test 2: Verify all data was written correctly
    for i in 0..10 {
        let key = format!("concurrent_key_{}", i);
        let expected_value = format!("concurrent_value_{}", i);
        let result = db.get(key.as_bytes()).await.unwrap();
        assert_eq!(result, Some(expected_value.as_bytes().to_vec()));
    }

    // Test 3: Concurrent reads
    let mut read_handles = vec![];
    
    for i in 0..10 {
        let db_clone = db.clone();
        let handle = task::spawn(async move {
            let key = format!("concurrent_key_{}", i);
            let expected_value = format!("concurrent_value_{}", i);
            
            for _ in 0..5 { // Multiple reads of the same key
                let result = db_clone.get(key.as_bytes()).await.unwrap();
                assert_eq!(result, Some(expected_value.as_bytes().to_vec()));
            }
        });
        read_handles.push(handle);
    }

    // Wait for all reads to complete
    for handle in read_handles {
        handle.await.unwrap();
    }
}

#[tokio::test]
async fn test_memory_pressure() {
    let temp_dir = TempDir::new().unwrap();
    let config = BlockDBConfig {
        data_dir: temp_dir.path().to_string_lossy().to_string(),
        memtable_size_limit: 1024 * 1024, // 1MB limit
        compaction_threshold: 3,
        ..Default::default()
    };

    let db = BlockDBHandle::new(config).unwrap();

    // Test 1: Write enough data to trigger multiple flushes
    let mut expected_data = HashMap::new();
    
    for i in 0..1000 {
        let key = format!("memory_key_{:06}", i);
        let value = format!("memory_value_{:06}_{}", i, "x".repeat(100)); // ~120 bytes per value
        
        db.put(key.as_bytes(), value.as_bytes()).await.unwrap();
        expected_data.insert(key, value);
    }

    // Test 2: Force compaction
    db.force_flush().await.unwrap();

    // Test 3: Verify all data is still accessible
    for (key, expected_value) in expected_data {
        let result = db.get(key.as_bytes()).await.unwrap();
        assert_eq!(result, Some(expected_value.as_bytes().to_vec()));
    }

    // Test 4: Verify blockchain integrity after heavy operations
    let integrity = db.verify_integrity().await.unwrap();
    assert!(integrity);
}

#[tokio::test]
async fn test_edge_cases() {
    let temp_dir = TempDir::new().unwrap();
    let config = BlockDBConfig {
        data_dir: temp_dir.path().to_string_lossy().to_string(),
        ..Default::default()
    };

    let db = BlockDBHandle::new(config).unwrap();

    // Test 1: Empty key and value
    db.put(b"", b"").await.unwrap();
    let result = db.get(b"").await.unwrap();
    assert_eq!(result, Some(vec![]));

    // Test 2: Very long key
    let long_key = vec![b'k'; 1000];
    db.put(&long_key, b"long_key_value").await.unwrap();
    let result = db.get(&long_key).await.unwrap();
    assert_eq!(result, Some(b"long_key_value".to_vec()));

    // Test 3: Nonexistent key
    let nonexistent = db.get(b"does_not_exist").await.unwrap();
    assert_eq!(nonexistent, None);

    // Test 4: Unicode keys and values
    let unicode_key = "🔑".as_bytes();
    let unicode_value = "🎯 Unicode value with emojis 🚀".as_bytes();
    db.put(unicode_key, unicode_value).await.unwrap();
    let result = db.get(unicode_key).await.unwrap();
    assert_eq!(result, Some(unicode_value.to_vec()));

    // Test 5: Maximum size value (within reasonable limits)
    let max_value = vec![b'M'; 1024 * 1024]; // 1MB value
    db.put(b"max_size_key", &max_value).await.unwrap();
    let result = db.get(b"max_size_key").await.unwrap();
    assert_eq!(result, Some(max_value));
}

#[tokio::test]
async fn test_flush_operations() {
    let temp_dir = TempDir::new().unwrap();
    let config = BlockDBConfig {
        data_dir: temp_dir.path().to_string_lossy().to_string(),
        ..Default::default()
    };

    let db = BlockDBHandle::new(config.clone()).unwrap();

    // Test 1: Add some data
    db.put(b"flush_key1", b"flush_value1").await.unwrap();
    db.put(b"flush_key2", b"flush_value2").await.unwrap();
    db.put(b"flush_key3", b"flush_value3").await.unwrap();

    // Verify data exists
    assert!(db.get(b"flush_key1").await.unwrap().is_some());
    assert!(db.get(b"flush_key2").await.unwrap().is_some());
    assert!(db.get(b"flush_key3").await.unwrap().is_some());

    // Test 2: Flush all data
    db.flush_all().await.unwrap();

    // Test 3: Verify data is gone
    assert!(db.get(b"flush_key1").await.unwrap().is_none());
    assert!(db.get(b"flush_key2").await.unwrap().is_none());
    assert!(db.get(b"flush_key3").await.unwrap().is_none());

    // Test 4: Database should still be functional after flush
    db.put(b"post_flush_key", b"post_flush_value").await.unwrap();
    let result = db.get(b"post_flush_key").await.unwrap();
    assert_eq!(result, Some(b"post_flush_value".to_vec()));

    // Test 5: Blockchain integrity should be maintained
    let integrity = db.verify_integrity().await.unwrap();
    assert!(integrity);
}

#[tokio::test]
async fn test_persistence_across_restarts() {
    let temp_dir = TempDir::new().unwrap();
    let config = BlockDBConfig {
        data_dir: temp_dir.path().to_string_lossy().to_string(),
        ..Default::default()
    };

    let test_data = vec![
        ("persistent_key_1", "persistent_value_1"),
        ("persistent_key_2", "persistent_value_2"),
        ("persistent_key_3", "persistent_value_3"),
    ];

    // Test 1: First session - write data
    {
        let db = BlockDBHandle::new(config.clone()).unwrap();
        
        for (key, value) in &test_data {
            db.put(key.as_bytes(), value.as_bytes()).await.unwrap();
        }
        
        db.force_flush().await.unwrap();
        
        // Verify data is written
        for (key, value) in &test_data {
            let result = db.get(key.as_bytes()).await.unwrap();
            assert_eq!(result, Some(value.as_bytes().to_vec()));
        }
    }

    // Test 2: Second session - verify persistence
    {
        let db = BlockDBHandle::new(config.clone()).unwrap();
        
        for (key, value) in &test_data {
            let result = db.get(key.as_bytes()).await.unwrap();
            assert_eq!(result, Some(value.as_bytes().to_vec()));
        }
        
        // Verify blockchain integrity
        let integrity = db.verify_integrity().await.unwrap();
        assert!(integrity);
    }

    // Test 3: Third session - verify append-only persistence
    {
        let db = BlockDBHandle::new(config).unwrap();
        
        // Try to update existing keys - should fail
        for (key, _) in &test_data {
            let update_result = db.put(key.as_bytes(), b"updated_value").await;
            assert!(update_result.is_err());
        }
        
        // Original values should be preserved
        for (key, value) in &test_data {
            let result = db.get(key.as_bytes()).await.unwrap();
            assert_eq!(result, Some(value.as_bytes().to_vec()));
        }
    }
}

#[tokio::test]
async fn test_error_conditions() {
    let temp_dir = TempDir::new().unwrap();
    let config = BlockDBConfig {
        data_dir: temp_dir.path().to_string_lossy().to_string(),
        ..Default::default()
    };

    let db = BlockDBHandle::new(config).unwrap();

    // Test 1: Duplicate key error
    db.put(b"error_key", b"original_value").await.unwrap();
    let duplicate_result = db.put(b"error_key", b"duplicate_value").await;
    
    assert!(duplicate_result.is_err());
    // Verify the error is specifically about duplicate keys
    let error_string = format!("{:?}", duplicate_result.unwrap_err());
    assert!(error_string.contains("DuplicateKey") || error_string.contains("already exists"));

    // Test 2: Original value should be preserved after error
    let preserved = db.get(b"error_key").await.unwrap();
    assert_eq!(preserved, Some(b"original_value".to_vec()));

    // Test 3: Database should remain functional after errors
    db.put(b"post_error_key", b"post_error_value").await.unwrap();
    let result = db.get(b"post_error_key").await.unwrap();
    assert_eq!(result, Some(b"post_error_value".to_vec()));
}