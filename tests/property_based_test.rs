use blockdb::{BlockDBHandle, BlockDBConfig, AuthManager, Permission};
use proptest::prelude::*;
use tempfile::TempDir;
use std::collections::HashMap;

/// Property-based tests using proptest
/// Tests database properties that should hold for any valid input

proptest! {
    #[test]
    fn test_append_only_property(
        keys in prop::collection::vec(prop::collection::vec(0u8..=255, 1..100), 1..50),
        values in prop::collection::vec(prop::collection::vec(0u8..=255, 1..1000), 1..50)
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let temp_dir = TempDir::new().unwrap();
            let config = BlockDBConfig {
                data_dir: temp_dir.path().to_string_lossy().to_string(),
                ..Default::default()
            };

            let db = BlockDBHandle::new(config).unwrap();
            let mut written_data = HashMap::new();

            // Property: Once a key is written, it cannot be overwritten
            for (key, value) in keys.iter().zip(values.iter()) {
                if !written_data.contains_key(key) {
                    // First write should succeed
                    let result = db.put(key, value).await;
                    prop_assert!(result.is_ok());
                    written_data.insert(key.clone(), value.clone());
                } else {
                    // Subsequent write should fail
                    let result = db.put(key, value).await;
                    prop_assert!(result.is_err());
                }
            }

            // Property: All written data should be retrievable with original values
            for (key, expected_value) in &written_data {
                let retrieved = db.get(key).await.unwrap();
                prop_assert_eq!(retrieved, Some(expected_value.clone()));
            }

            // Property: Blockchain integrity should be maintained
            let integrity = db.verify_integrity().await.unwrap();
            prop_assert!(integrity);
        });
    }

    #[test]
    fn test_data_consistency_property(
        operations in prop::collection::vec(
            (prop::collection::vec(0u8..=255, 1..50), prop::collection::vec(0u8..=255, 1..500)),
            1..100
        )
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let temp_dir = TempDir::new().unwrap();
            let config = BlockDBConfig {
                data_dir: temp_dir.path().to_string_lossy().to_string(),
                ..Default::default()
            };

            let db = BlockDBHandle::new(config).unwrap();
            let mut expected_state = HashMap::new();

            // Apply operations and track expected state
            for (key, value) in operations {
                if !expected_state.contains_key(&key) {
                    let result = db.put(&key, &value).await;
                    if result.is_ok() {
                        expected_state.insert(key, value);
                    }
                }
            }

            // Property: Database state should match expected state
            for (key, expected_value) in &expected_state {
                let actual = db.get(key).await.unwrap();
                prop_assert_eq!(actual, Some(expected_value.clone()));
            }

            // Property: Non-existent keys should return None
            let non_existent_key = vec![255u8; 100]; // Unlikely to exist
            if !expected_state.contains_key(&non_existent_key) {
                let result = db.get(&non_existent_key).await.unwrap();
                prop_assert_eq!(result, None);
            }
        });
    }

    #[test]
    fn test_persistence_property(
        data in prop::collection::vec(
            (prop::collection::vec(1u8..=254, 1..30), prop::collection::vec(1u8..=254, 1..200)),
            1..20
        )
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let temp_dir = TempDir::new().unwrap();
            let config = BlockDBConfig {
                data_dir: temp_dir.path().to_string_lossy().to_string(),
                ..Default::default()
            };

            let mut written_data = HashMap::new();

            // First session: write data
            {
                let db = BlockDBHandle::new(config.clone()).unwrap();
                
                for (key, value) in &data {
                    if !written_data.contains_key(key) {
                        let result = db.put(key, value).await;
                        if result.is_ok() {
                            written_data.insert(key.clone(), value.clone());
                        }
                    }
                }
                
                db.force_flush().await.unwrap();
            }

            // Second session: verify persistence
            {
                let db = BlockDBHandle::new(config).unwrap();
                
                // Property: All data written in first session should persist
                for (key, expected_value) in &written_data {
                    let retrieved = db.get(key).await.unwrap();
                    prop_assert_eq!(retrieved, Some(expected_value.clone()));
                }

                // Property: Blockchain integrity should persist
                let integrity = db.verify_integrity().await.unwrap();
                prop_assert!(integrity);
            }
        });
    }
}

proptest! {
    #[test]
    fn test_auth_permission_property(
        usernames in prop::collection::vec("[a-zA-Z][a-zA-Z0-9_]{2,19}", 1..10),
        passwords in prop::collection::vec("[a-zA-Z0-9!@#$%^&*]{6,30}", 1..10),
        permission_combinations in prop::collection::vec(
            prop::collection::vec(0usize..6, 1..4), // Indices into permission array
            1..10
        )
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let temp_dir = TempDir::new().unwrap();
            let config = BlockDBConfig {
                data_dir: temp_dir.path().to_string_lossy().to_string(),
                auth_enabled: true,
                ..Default::default()
            };

            let mut auth_manager = AuthManager::new(config).unwrap();
            let all_permissions = vec![
                Permission::Read,
                Permission::Write,
                Permission::Delete,
                Permission::CreateUser,
                Permission::Admin,
                Permission::ViewStats,
            ];

            // Property: User creation and authentication should be consistent
            for ((username, password), perm_indices) in 
                usernames.iter().zip(passwords.iter()).zip(permission_combinations.iter()) {
                
                let permissions: Vec<Permission> = perm_indices.iter()
                    .map(|&i| all_permissions[i].clone())
                    .collect();

                // Create user
                let result = auth_manager.create_user(username, password, permissions.clone());
                
                if result.is_ok() {
                    // Property: Created user should be able to authenticate
                    let auth_result = auth_manager.authenticate_user(username, password);
                    prop_assert!(auth_result.is_ok());
                    
                    let context = auth_result.unwrap();
                    
                    // Property: User should have exactly the permissions they were granted
                    for permission in &permissions {
                        prop_assert!(context.has_permission(permission));
                    }
                    
                    // Property: User should not have permissions they weren't granted
                    for permission in &all_permissions {
                        if !permissions.contains(permission) && permission != &Permission::Admin {
                            // Admin permission implies all others, so skip this check for admin
                            if !permissions.contains(&Permission::Admin) {
                                prop_assert!(!context.has_permission(permission) || 
                                           permissions.contains(permission));
                            }
                        }
                    }
                    
                    // Property: Authentication with wrong password should fail
                    let wrong_auth = auth_manager.authenticate_user(username, "wrong_password");
                    prop_assert!(wrong_auth.is_err());
                }
            }
        });
    }

    #[test]
    fn test_session_management_property(
        usernames in prop::collection::vec("[a-zA-Z][a-zA-Z0-9_]{3,15}", 1..5),
        passwords in prop::collection::vec("[a-zA-Z0-9]{8,20}", 1..5)
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let temp_dir = TempDir::new().unwrap();
            let config = BlockDBConfig {
                data_dir: temp_dir.path().to_string_lossy().to_string(),
                auth_enabled: true,
                session_duration_hours: 1,
                ..Default::default()
            };

            let mut auth_manager = AuthManager::new(config).unwrap();
            let mut valid_sessions = Vec::new();

            // Create users and sessions
            for (username, password) in usernames.iter().zip(passwords.iter()) {
                let create_result = auth_manager.create_user(
                    username, 
                    password, 
                    vec![Permission::Read, Permission::Write]
                );
                
                if create_result.is_ok() {
                    let auth_result = auth_manager.authenticate_user(username, password);
                    if let Ok(context) = auth_result {
                        valid_sessions.push(context.session_id.clone());
                        
                        // Property: Valid session should be validatable
                        let validation = auth_manager.validate_session(&context.session_id);
                        prop_assert!(validation.is_ok());
                        
                        let validated_context = validation.unwrap();
                        prop_assert_eq!(validated_context.user_id, context.user_id);
                        prop_assert_eq!(validated_context.session_id, context.session_id);
                    }
                }
            }

            // Test session logout
            for session_id in &valid_sessions {
                // Property: Session should be valid before logout
                let pre_logout = auth_manager.validate_session(session_id);
                prop_assert!(pre_logout.is_ok());
                
                // Logout
                let logout_result = auth_manager.logout(session_id);
                prop_assert!(logout_result.is_ok());
                
                // Property: Session should be invalid after logout
                let post_logout = auth_manager.validate_session(session_id);
                prop_assert!(post_logout.is_err());
            }
        });
    }
}

proptest! {
    #[test]
    fn test_concurrent_operations_property(
        key_value_pairs in prop::collection::vec(
            (prop::collection::vec(1u8..=254, 5..20), prop::collection::vec(1u8..=254, 10..50)),
            10..50
        )
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let temp_dir = TempDir::new().unwrap();
            let config = BlockDBConfig {
                data_dir: temp_dir.path().to_string_lossy().to_string(),
                ..Default::default()
            };

            let db = std::sync::Arc::new(BlockDBHandle::new(config).unwrap());
            let mut expected_data = HashMap::new();
            let mut handles = Vec::new();

            // Property: Concurrent writes to different keys should all succeed
            for (i, (key, value)) in key_value_pairs.iter().enumerate() {
                let db_clone = db.clone();
                let key_clone = key.clone();
                let value_clone = value.clone();
                
                let handle = tokio::spawn(async move {
                    let result = db_clone.put(&key_clone, &value_clone).await;
                    (i, key_clone, value_clone, result)
                });
                
                handles.push(handle);
                expected_data.insert(key.clone(), value.clone());
            }

            // Collect results
            let mut successful_writes = HashMap::new();
            for handle in handles {
                let (_, key, value, result) = handle.await.unwrap();
                if result.is_ok() {
                    successful_writes.insert(key, value);
                }
            }

            // Property: All successful writes should be readable
            for (key, expected_value) in &successful_writes {
                let retrieved = db.get(key).await.unwrap();
                prop_assert_eq!(retrieved, Some(expected_value.clone()));
            }

            // Property: Blockchain integrity should be maintained after concurrent operations
            let integrity = db.verify_integrity().await.unwrap();
            prop_assert!(integrity);
        });
    }

    #[test]
    fn test_flush_operation_property(
        initial_data in prop::collection::vec(
            (prop::collection::vec(1u8..=254, 1..30), prop::collection::vec(1u8..=254, 1..100)),
            1..20
        ),
        post_flush_data in prop::collection::vec(
            (prop::collection::vec(1u8..=254, 1..30), prop::collection::vec(1u8..=254, 1..100)),
            1..20
        )
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let temp_dir = TempDir::new().unwrap();
            let config = BlockDBConfig {
                data_dir: temp_dir.path().to_string_lossy().to_string(),
                ..Default::default()
            };

            let db = BlockDBHandle::new(config).unwrap();

            // Write initial data
            let mut written_keys = std::collections::HashSet::new();
            for (key, value) in &initial_data {
                if !written_keys.contains(key) {
                    let result = db.put(key, value).await;
                    if result.is_ok() {
                        written_keys.insert(key.clone());
                    }
                }
            }

            // Verify initial data exists
            for key in &written_keys {
                let result = db.get(key).await.unwrap();
                prop_assert!(result.is_some());
            }

            // Property: Flush should remove all data
            db.flush_all().await.unwrap();

            // Property: All data should be gone after flush
            for key in &written_keys {
                let result = db.get(key).await.unwrap();
                prop_assert_eq!(result, None);
            }

            // Property: Database should be functional after flush
            let mut post_flush_written = std::collections::HashSet::new();
            for (key, value) in &post_flush_data {
                if !post_flush_written.contains(key) {
                    let result = db.put(key, value).await;
                    if result.is_ok() {
                        post_flush_written.insert(key.clone());
                    }
                }
            }

            // Property: New data should be accessible after flush
            for key in &post_flush_written {
                let result = db.get(key).await.unwrap();
                prop_assert!(result.is_some());
            }

            // Property: Blockchain integrity should be maintained after flush
            let integrity = db.verify_integrity().await.unwrap();
            prop_assert!(integrity);
        });
    }
}

#[cfg(test)]
mod deterministic_property_tests {
    use super::*;

    #[tokio::test]
    async fn test_key_uniqueness_property() {
        let temp_dir = TempDir::new().unwrap();
        let config = BlockDBConfig {
            data_dir: temp_dir.path().to_string_lossy().to_string(),
            ..Default::default()
        };

        let db = BlockDBHandle::new(config).unwrap();

        // Property: Each unique key can only be written once
        let test_cases = vec![
            (b"key1".to_vec(), b"value1".to_vec()),
            (b"key2".to_vec(), b"value2".to_vec()),
            (b"key1".to_vec(), b"different_value".to_vec()), // Duplicate key
            (b"key3".to_vec(), b"value3".to_vec()),
            (b"key2".to_vec(), b"another_different_value".to_vec()), // Another duplicate
        ];

        let mut expected_data = HashMap::new();
        for (key, value) in test_cases {
            if expected_data.contains_key(&key) {
                // Should fail - duplicate key
                let result = db.put(&key, &value).await;
                assert!(result.is_err());
            } else {
                // Should succeed - new key
                let result = db.put(&key, &value).await;
                assert!(result.is_ok());
                expected_data.insert(key, value);
            }
        }

        // Verify final state matches expected
        for (key, expected_value) in expected_data {
            let actual = db.get(&key).await.unwrap();
            assert_eq!(actual, Some(expected_value));
        }
    }

    #[tokio::test]
    async fn test_monotonic_sequence_property() {
        let temp_dir = TempDir::new().unwrap();
        let config = BlockDBConfig {
            data_dir: temp_dir.path().to_string_lossy().to_string(),
            ..Default::default()
        };

        let db = BlockDBHandle::new(config).unwrap();

        // Property: Operations should maintain monotonic ordering
        let operations = vec![
            ("seq_key_1", "seq_val_1"),
            ("seq_key_2", "seq_val_2"),
            ("seq_key_3", "seq_val_3"),
        ];

        for (key, value) in operations {
            db.put(key.as_bytes(), value.as_bytes()).await.unwrap();
        }

        // Verify blockchain maintains ordering
        let integrity = db.verify_integrity().await.unwrap();
        assert!(integrity);

        // Property: Data written in order should be retrievable in any order
        assert_eq!(
            db.get(b"seq_key_2").await.unwrap(),
            Some(b"seq_val_2".to_vec())
        );
        assert_eq!(
            db.get(b"seq_key_1").await.unwrap(),
            Some(b"seq_val_1".to_vec())
        );
        assert_eq!(
            db.get(b"seq_key_3").await.unwrap(),
            Some(b"seq_val_3".to_vec())
        );
    }

    #[tokio::test]
    async fn test_auth_admin_permission_property() {
        let temp_dir = TempDir::new().unwrap();
        let config = BlockDBConfig {
            data_dir: temp_dir.path().to_string_lossy().to_string(),
            auth_enabled: true,
            ..Default::default()
        };

        let mut auth_manager = AuthManager::new(config).unwrap();

        // Property: Admin permission should imply all other permissions
        auth_manager
            .create_user("admin_user", "admin_pass", vec![Permission::Admin])
            .unwrap();

        let context = auth_manager
            .authenticate_user("admin_user", "admin_pass")
            .unwrap();

        let all_permissions = vec![
            Permission::Read,
            Permission::Write,
            Permission::Delete,
            Permission::CreateUser,
            Permission::ViewStats,
            Permission::Admin,
        ];

        // Admin should have all permissions
        for permission in all_permissions {
            assert!(
                context.has_permission(&permission),
                "Admin should have {:?} permission",
                permission
            );
        }
    }
}