use blockdb::{BlockDBConfig, BlockDBHandle, AuthManager, BlockDBError};
use tempfile::TempDir;
use std::fs;
use rand::Rng;

/// Configuration and error handling tests
/// Tests config loading, validation, and comprehensive error scenarios

#[tokio::test]
async fn test_config_defaults() {
    let config = BlockDBConfig::default();
    
    assert_eq!(config.data_dir, "./blockdb_data");
    assert_eq!(config.memtable_size_limit, 64 * 1024 * 1024); // 64MB
    assert_eq!(config.wal_sync_interval_ms, 1000);
    assert_eq!(config.compaction_threshold, 4);
    assert_eq!(config.blockchain_batch_size, 1000);
    assert_eq!(config.auth_enabled, true);
    assert_eq!(config.session_duration_hours, 24);
    assert_eq!(config.password_min_length, 8);
    assert_eq!(config.max_failed_attempts, 5);
    assert_eq!(config.account_lockout_duration_minutes, 30);
}

#[tokio::test]
async fn test_config_validation() {
    let temp_dir = TempDir::new().unwrap();
    
    // Test valid config
    let valid_config = BlockDBConfig {
        data_dir: temp_dir.path().to_string_lossy().to_string(),
        memtable_size_limit: 1024 * 1024, // 1MB
        wal_sync_interval_ms: 500,
        compaction_threshold: 2,
        blockchain_batch_size: 100,
        auth_enabled: true,
        session_duration_hours: 12,
        password_min_length: 6,
        max_failed_attempts: 3,
        account_lockout_duration_minutes: 15,
    };
    
    let db = BlockDBHandle::new(valid_config);
    assert!(db.is_ok());

    // Test config with very small memtable
    let small_memtable_config = BlockDBConfig {
        data_dir: temp_dir.path().join("small").to_string_lossy().to_string(),
        memtable_size_limit: 1024, // 1KB - very small
        ..Default::default()
    };
    
    let db = BlockDBHandle::new(small_memtable_config);
    assert!(db.is_ok()); // Should still work, just trigger frequent flushes

    // Test config with zero values (edge cases)
    let zero_config = BlockDBConfig {
        data_dir: temp_dir.path().join("zero").to_string_lossy().to_string(),
        wal_sync_interval_ms: 0, // Immediate sync
        compaction_threshold: 1, // Immediate compaction
        blockchain_batch_size: 1, // Single operation batches
        session_duration_hours: 0, // Immediate expiry
        password_min_length: 0, // No minimum
        max_failed_attempts: 1, // Lock after first failure
        account_lockout_duration_minutes: 0, // No lockout
        ..Default::default()
    };
    
    let db = BlockDBHandle::new(zero_config);
    assert!(db.is_ok());
}

#[tokio::test]
async fn test_invalid_data_directory() {
    // Test with non-existent parent directory
    let invalid_config = BlockDBConfig {
        data_dir: "/nonexistent/path/to/data".to_string(),
        ..Default::default()
    };
    
    let result = BlockDBHandle::new(invalid_config);
    assert!(result.is_err());
    
    // Test with read-only directory (if possible to create)
    let temp_dir = TempDir::new().unwrap();
    let readonly_path = temp_dir.path().join("readonly");
    fs::create_dir(&readonly_path).unwrap();
    
    // Note: Setting read-only permissions might not work on all systems
    // This is more of a documentation of the expected behavior
    let readonly_config = BlockDBConfig {
        data_dir: readonly_path.to_string_lossy().to_string(),
        ..Default::default()
    };
    
    let db = BlockDBHandle::new(readonly_config);
    // Should work unless the directory is truly read-only
    // In practice, this test depends on system permissions
}

#[tokio::test]
async fn test_comprehensive_error_scenarios() {
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
    
    match duplicate_result.unwrap_err() {
        BlockDBError::DuplicateKey(_) => {}, // Expected
        other => panic!("Expected DuplicateKey error, got: {:?}", other),
    }

    // Test 2: Large key handling
    let very_large_key = vec![b'k'; 10000]; // 10KB key
    let result = db.put(&very_large_key, b"large_key_value").await;
    // Should work - BlockDB should handle large keys
    assert!(result.is_ok());

    // Test 3: Large value handling
    let very_large_value = vec![b'v'; 10 * 1024 * 1024]; // 10MB value
    let result = db.put(b"large_value_key", &very_large_value).await;
    // Should work - BlockDB should handle large values
    assert!(result.is_ok());

    // Test 4: Empty key and value
    let result = db.put(b"", b"").await;
    assert!(result.is_ok());
    
    let retrieved = db.get(b"").await.unwrap();
    assert_eq!(retrieved, Some(vec![]));

    // Test 5: Null bytes in keys and values
    let null_key = b"key\x00with\x00nulls";
    let null_value = b"value\x00with\x00nulls";
    let result = db.put(null_key, null_value).await;
    assert!(result.is_ok());
    
    let retrieved = db.get(null_key).await.unwrap();
    assert_eq!(retrieved, Some(null_value.to_vec()));

    // Test 6: Unicode handling
    let unicode_key = "🔑 Unicode Key 🌟".as_bytes();
    let unicode_value = "🎯 Unicode Value with emojis 🚀".as_bytes();
    let result = db.put(unicode_key, unicode_value).await;
    assert!(result.is_ok());
    
    let retrieved = db.get(unicode_key).await.unwrap();
    assert_eq!(retrieved, Some(unicode_value.to_vec()));
}

#[tokio::test]
async fn test_auth_configuration_errors() {
    let temp_dir = TempDir::new().unwrap();
    
    // Test various auth configurations
    let auth_configs = vec![
        // Minimal password length
        BlockDBConfig {
            data_dir: temp_dir.path().join("auth1").to_string_lossy().to_string(),
            auth_enabled: true,
            password_min_length: 1,
            ..Default::default()
        },
        // Very long session duration
        BlockDBConfig {
            data_dir: temp_dir.path().join("auth2").to_string_lossy().to_string(),
            auth_enabled: true,
            session_duration_hours: 8760, // 1 year
            ..Default::default()
        },
        // Immediate lockout
        BlockDBConfig {
            data_dir: temp_dir.path().join("auth3").to_string_lossy().to_string(),
            auth_enabled: true,
            max_failed_attempts: 1,
            account_lockout_duration_minutes: 1,
            ..Default::default()
        },
    ];

    for config in auth_configs {
        let auth_manager = AuthManager::new(config);
        assert!(auth_manager.is_ok());
    }
}

#[tokio::test]
async fn test_auth_error_scenarios() {
    let temp_dir = TempDir::new().unwrap();
    let config = BlockDBConfig {
        data_dir: temp_dir.path().to_string_lossy().to_string(),
        auth_enabled: true,
        password_min_length: 8,
        max_failed_attempts: 3,
        ..Default::default()
    };

    let mut auth_manager = AuthManager::new(config).unwrap();

    // Test 1: User creation with short password
    let result = auth_manager.create_user("shortpass", "123", vec![]);
    assert!(result.is_err());

    // Test 2: User creation with empty username
    let result = auth_manager.create_user("", "validpass123", vec![]);
    assert!(result.is_err());

    // Test 3: User creation with invalid permissions
    // Note: This depends on how Permission parsing is implemented
    
    // Test 4: Duplicate user creation
    auth_manager.create_user("unique_user", "validpass123", vec![]).unwrap();
    let duplicate_result = auth_manager.create_user("unique_user", "anotherpass123", vec![]);
    assert!(duplicate_result.is_err());

    // Test 5: Authentication with non-existent user
    let auth_result = auth_manager.authenticate_user("nonexistent", "anypass");
    assert!(auth_result.is_err());

    // Test 6: Authentication with wrong password
    auth_manager.create_user("wrongpass_user", "correctpass123", vec![]).unwrap();
    let auth_result = auth_manager.authenticate_user("wrongpass_user", "wrongpass");
    assert!(auth_result.is_err());

    // Test 7: Session validation with invalid token
    let validation_result = auth_manager.validate_session("invalid_session_token");
    assert!(validation_result.is_err());

    // Test 8: Logout with invalid session
    let logout_result = auth_manager.logout("invalid_session_token");
    assert!(logout_result.is_err());

    // Test 9: Permission management with non-existent user
    use blockdb::Permission;
    let grant_result = auth_manager.grant_permission("nonexistent_user", Permission::Read);
    assert!(grant_result.is_err());

    let revoke_result = auth_manager.revoke_permission("nonexistent_user", Permission::Read);
    assert!(revoke_result.is_err());
}

#[tokio::test]
async fn test_concurrent_error_scenarios() {
    let temp_dir = TempDir::new().unwrap();
    let config = BlockDBConfig {
        data_dir: temp_dir.path().to_string_lossy().to_string(),
        ..Default::default()
    };

    let db = std::sync::Arc::new(BlockDBHandle::new(config).unwrap());

    // Test concurrent attempts to write the same key
    let mut handles = Vec::new();
    
    for i in 0..10 {
        let db_clone = db.clone();
        let handle = tokio::spawn(async move {
            let result = db_clone.put(b"concurrent_key", format!("value_{}", i).as_bytes()).await;
            (i, result)
        });
        handles.push(handle);
    }

    let mut success_count = 0;
    let mut error_count = 0;

    for handle in handles {
        let (i, result) = handle.await.unwrap();
        match result {
            Ok(_) => {
                success_count += 1;
                println!("Task {} succeeded", i);
            }
            Err(_) => {
                error_count += 1;
                println!("Task {} failed (expected due to duplicate key)", i);
            }
        }
    }

    // Exactly one should succeed (first to write), others should fail
    assert_eq!(success_count, 1);
    assert_eq!(error_count, 9);

    // Verify the key exists with some value
    let final_value = db.get(b"concurrent_key").await.unwrap();
    assert!(final_value.is_some());
}

#[tokio::test]
async fn test_resource_exhaustion_scenarios() {
    let temp_dir = TempDir::new().unwrap();
    let config = BlockDBConfig {
        data_dir: temp_dir.path().to_string_lossy().to_string(),
        memtable_size_limit: 1024, // Very small memtable
        compaction_threshold: 2,
        ..Default::default()
    };

    let db = BlockDBHandle::new(config).unwrap();

    // Test 1: Rapid writes that should trigger frequent flushes
    for i in 0..1000 {
        let key = format!("exhaustion_key_{}", i);
        let value = vec![b'x'; 100]; // 100 bytes each
        
        let result = db.put(key.as_bytes(), &value).await;
        assert!(result.is_ok(), "Write {} should succeed", i);
    }

    // Test 2: Verify all data is still accessible
    for i in 0..1000 {
        let key = format!("exhaustion_key_{}", i);
        let result = db.get(key.as_bytes()).await.unwrap();
        assert!(result.is_some(), "Key {} should exist", i);
    }

    // Test 3: Force flush and verify integrity
    db.force_flush().await.unwrap();
    let integrity = db.verify_integrity().await.unwrap();
    assert!(integrity);
}

#[tokio::test]
async fn test_corruption_detection() {
    let temp_dir = TempDir::new().unwrap();
    let config = BlockDBConfig {
        data_dir: temp_dir.path().to_string_lossy().to_string(),
        ..Default::default()
    };

    let db = BlockDBHandle::new(config.clone()).unwrap();

    // Add some data
    for i in 0..10 {
        let key = format!("corruption_key_{}", i);
        let value = format!("corruption_value_{}", i);
        db.put(key.as_bytes(), value.as_bytes()).await.unwrap();
    }

    db.force_flush().await.unwrap();

    // Verify integrity before any potential corruption
    let integrity = db.verify_integrity().await.unwrap();
    assert!(integrity);

    // Note: In a real test, we might manually corrupt files and test detection
    // For now, we test that integrity verification works with clean data
    
    // Test integrity after restart
    drop(db);
    let db2 = BlockDBHandle::new(config).unwrap();
    let integrity_after_restart = db2.verify_integrity().await.unwrap();
    assert!(integrity_after_restart);
}

#[tokio::test]
async fn test_error_propagation() {
    let temp_dir = TempDir::new().unwrap();
    let config = BlockDBConfig {
        data_dir: temp_dir.path().to_string_lossy().to_string(),
        ..Default::default()
    };

    let db = BlockDBHandle::new(config).unwrap();

    // Test that errors are properly propagated through the async interface
    db.put(b"prop_key", b"prop_value").await.unwrap();
    
    let error = db.put(b"prop_key", b"prop_new_value").await.unwrap_err();
    
    // Test error formatting
    let error_string = format!("{}", error);
    assert!(error_string.contains("Duplicate") || error_string.contains("already exists"));
    
    // Test error debug formatting
    let debug_string = format!("{:?}", error);
    assert!(!debug_string.is_empty());
    
    // Test error source chain (if implemented)
    let source = std::error::Error::source(&error);
    // May or may not have a source depending on implementation
}

#[tokio::test]
async fn test_boundary_conditions() {
    let temp_dir = TempDir::new().unwrap();
    let config = BlockDBConfig {
        data_dir: temp_dir.path().to_string_lossy().to_string(),
        ..Default::default()
    };

    let db = BlockDBHandle::new(config).unwrap();

    // Test 1: Maximum realistic key size (avoid OOM)
    let large_key = vec![b'k'; 65536]; // 64KB key
    let result = db.put(&large_key, b"large_key_test").await;
    assert!(result.is_ok());

    // Test 2: Maximum realistic value size (avoid OOM)
    let large_value = vec![b'v'; 1024 * 1024]; // 1MB value
    let result = db.put(b"large_value_test", &large_value).await;
    assert!(result.is_ok());

    // Test 3: Many small operations
    for i in 0..10000 {
        let key = format!("boundary_{:05}", i);
        let value = format!("val_{}", i);
        
        if i % 1000 == 0 {
            println!("Progress: {}/10000", i);
        }
        
        let result = db.put(key.as_bytes(), value.as_bytes()).await;
        assert!(result.is_ok());
    }

    // Verify random sampling of the data
    for _ in 0..100 {
        let i = fastrand::usize(0..10000);
        let key = format!("boundary_{:05}", i);
        let expected_value = format!("val_{}", i);
        
        let result = db.get(key.as_bytes()).await.unwrap();
        assert_eq!(result, Some(expected_value.as_bytes().to_vec()));
    }

    // Test 4: Verify blockchain integrity with many operations
    let integrity = db.verify_integrity().await.unwrap();
    assert!(integrity);
}