use blockdb::{BlockDBHandle, BlockDBConfig, AuthManager, Permission};
use blockdb::api::{
    BlockDBServer, ApiConfig, LoginRequest, LoginResponse, CreateUserRequest, 
    CreateUserResponse, WriteRequest, ReadRequest, PermissionRequest
};
use tempfile::TempDir;

/// Comprehensive API authentication tests
/// Tests all authentication-related API endpoints and middleware
#[tokio::test]
async fn test_login_api_endpoint() {
    let temp_dir = TempDir::new().unwrap();
    let config = BlockDBConfig {
        data_dir: temp_dir.path().to_string_lossy().to_string(),
        auth_enabled: true,
        session_duration_hours: 24,
        ..Default::default()
    };

    let db = BlockDBHandle::new(config.clone()).unwrap();
    let mut auth_manager = AuthManager::new(config.clone()).unwrap();

    // Pre-create a user for testing
    auth_manager
        .create_user("testuser", "testpass123", vec![Permission::Read, Permission::Write])
        .unwrap();

    let api_config = ApiConfig {
        auth_enabled: true,
        ..Default::default()
    };
    let server = BlockDBServer::with_auth(db, api_config, auth_manager);

    // Test 1: Successful login
    let login_request = LoginRequest {
        username: "testuser".to_string(),
        password: "testpass123".to_string(),
    };

    let response = server.login(login_request).await.unwrap();
    assert!(response.success);
    assert!(response.auth_token.is_some());
    assert!(response.expires_at.is_some());
    assert_eq!(response.message, "Login successful");

    // Test 2: Failed login - wrong password
    let bad_login = LoginRequest {
        username: "testuser".to_string(),
        password: "wrongpass".to_string(),
    };

    let response = server.login(bad_login).await.unwrap();
    assert!(!response.success);
    assert!(response.auth_token.is_none());
    assert!(response.expires_at.is_none());
    assert!(response.message.contains("Login failed"));

    // Test 3: Failed login - nonexistent user
    let nonexistent_login = LoginRequest {
        username: "nonexistent".to_string(),
        password: "anypass".to_string(),
    };

    let response = server.login(nonexistent_login).await.unwrap();
    assert!(!response.success);
    assert!(response.auth_token.is_none());
}

#[tokio::test]
async fn test_create_user_api_endpoint() {
    let temp_dir = TempDir::new().unwrap();
    let config = BlockDBConfig {
        data_dir: temp_dir.path().to_string_lossy().to_string(),
        auth_enabled: true,
        ..Default::default()
    };

    let db = BlockDBHandle::new(config.clone()).unwrap();
    let mut auth_manager = AuthManager::new(config.clone()).unwrap();

    // Create admin user
    let admin_id = auth_manager
        .create_user("admin", "admin123", vec![Permission::Admin])
        .unwrap();

    let admin_context = auth_manager
        .authenticate_user("admin", "admin123")
        .unwrap();

    let api_config = ApiConfig {
        auth_enabled: true,
        ..Default::default()
    };
    let server = BlockDBServer::with_auth(db, api_config, auth_manager);

    // Test 1: Successful user creation by admin
    let create_request = CreateUserRequest {
        username: "newuser".to_string(),
        password: "newpass123".to_string(),
        permissions: vec!["Read".to_string(), "Write".to_string()],
        auth_token: admin_context.session_id.clone(),
    };

    let response = server.create_user(create_request).await.unwrap();
    assert!(response.success);
    assert!(response.user_id.is_some());
    assert_eq!(response.message, "User created successfully");

    // Test 2: Duplicate user creation should fail
    let duplicate_request = CreateUserRequest {
        username: "newuser".to_string(), // Same username
        password: "anotherpass".to_string(),
        permissions: vec!["Read".to_string()],
        auth_token: admin_context.session_id.clone(),
    };

    let response = server.create_user(duplicate_request).await.unwrap();
    assert!(!response.success);
    assert!(response.user_id.is_none());
    assert!(response.message.contains("Failed to create user"));

    // Test 3: User creation with invalid permissions
    let invalid_perms_request = CreateUserRequest {
        username: "invaliduser".to_string(),
        password: "pass123".to_string(),
        permissions: vec!["InvalidPermission".to_string()],
        auth_token: admin_context.session_id.clone(),
    };

    let result = server.create_user(invalid_perms_request).await;
    assert!(result.is_err()); // Should fail due to invalid permission format

    // Test 4: User creation without admin token should fail
    let unauthorized_request = CreateUserRequest {
        username: "unauthorized".to_string(),
        password: "pass123".to_string(),
        permissions: vec!["Read".to_string()],
        auth_token: "invalid_token".to_string(),
    };

    let result = server.create_user(unauthorized_request).await;
    assert!(result.is_err()); // Should fail due to authentication error
}

#[tokio::test]
async fn test_authenticated_data_operations() {
    let temp_dir = TempDir::new().unwrap();
    let config = BlockDBConfig {
        data_dir: temp_dir.path().to_string_lossy().to_string(),
        auth_enabled: true,
        ..Default::default()
    };

    let db = BlockDBHandle::new(config.clone()).unwrap();
    let mut auth_manager = AuthManager::new(config.clone()).unwrap();

    // Create users with different permissions
    auth_manager
        .create_user("writer", "pass123", vec![Permission::Write])
        .unwrap();
    
    auth_manager
        .create_user("reader", "pass123", vec![Permission::Read])
        .unwrap();

    let writer_context = auth_manager.authenticate_user("writer", "pass123").unwrap();
    let reader_context = auth_manager.authenticate_user("reader", "pass123").unwrap();

    let api_config = ApiConfig {
        auth_enabled: true,
        require_auth_for_reads: true,
        ..Default::default()
    };
    let server = BlockDBServer::with_auth(db, api_config, auth_manager);

    // Test 1: Writer can write data
    let write_request = WriteRequest {
        key: "test_key".to_string(),
        value: "test_value".to_string(),
        encoding: None,
        auth_token: Some(writer_context.session_id.clone()),
    };

    let response = server.write(write_request).await.unwrap();
    assert!(response.success);
    assert_eq!(response.message, "Data written successfully");

    // Test 2: Reader can read data
    let read_request = ReadRequest {
        key: "test_key".to_string(),
        encoding: None,
        auth_token: Some(reader_context.session_id.clone()),
    };

    let response = server.read(read_request).await.unwrap();
    assert!(response.success);
    assert_eq!(response.data.unwrap(), "test_value");
    assert_eq!(response.message, "Data found");

    // Test 3: Reader cannot write data (insufficient permissions)
    let unauthorized_write = WriteRequest {
        key: "reader_key".to_string(),
        value: "reader_value".to_string(),
        encoding: None,
        auth_token: Some(reader_context.session_id.clone()),
    };

    let result = server.write(unauthorized_write).await;
    assert!(result.is_err()); // Should fail due to insufficient permissions

    // Test 4: No token provided should fail
    let no_token_write = WriteRequest {
        key: "no_token_key".to_string(),
        value: "no_token_value".to_string(),
        encoding: None,
        auth_token: None,
    };

    let result = server.write(no_token_write).await;
    assert!(result.is_err()); // Should fail due to missing authentication

    // Test 5: No token for read should also fail (since require_auth_for_reads is true)
    let no_token_read = ReadRequest {
        key: "test_key".to_string(),
        encoding: None,
        auth_token: None,
    };

    let result = server.read(no_token_read).await;
    assert!(result.is_err()); // Should fail due to missing authentication
}

#[tokio::test]
async fn test_base64_encoding_with_auth() {
    let temp_dir = TempDir::new().unwrap();
    let config = BlockDBConfig {
        data_dir: temp_dir.path().to_string_lossy().to_string(),
        auth_enabled: true,
        ..Default::default()
    };

    let db = BlockDBHandle::new(config.clone()).unwrap();
    let mut auth_manager = AuthManager::new(config.clone()).unwrap();

    auth_manager
        .create_user("encoder", "pass123", vec![Permission::Read, Permission::Write])
        .unwrap();

    let context = auth_manager.authenticate_user("encoder", "pass123").unwrap();

    let api_config = ApiConfig {
        auth_enabled: true,
        ..Default::default()
    };
    let server = BlockDBServer::with_auth(db, api_config, auth_manager);

    // Test 1: Write with base64 encoding
    let binary_data = vec![0, 1, 2, 255, 128, 64];
    let encoded_key = base64::engine::general_purpose::STANDARD.encode("binary_key");
    let encoded_value = base64::engine::general_purpose::STANDARD.encode(&binary_data);

    let write_request = WriteRequest {
        key: encoded_key.clone(),
        value: encoded_value.clone(),
        encoding: Some("base64".to_string()),
        auth_token: Some(context.session_id.clone()),
    };

    let response = server.write(write_request).await.unwrap();
    assert!(response.success);

    // Test 2: Read with base64 encoding
    let read_request = ReadRequest {
        key: encoded_key,
        encoding: Some("base64".to_string()),
        auth_token: Some(context.session_id.clone()),
    };

    let response = server.read(read_request).await.unwrap();
    assert!(response.success);
    
    let decoded_data = base64::engine::general_purpose::STANDARD
        .decode(response.data.unwrap())
        .unwrap();
    assert_eq!(decoded_data, binary_data);
}

#[tokio::test]
async fn test_batch_operations_with_auth() {
    let temp_dir = TempDir::new().unwrap();
    let config = BlockDBConfig {
        data_dir: temp_dir.path().to_string_lossy().to_string(),
        auth_enabled: true,
        ..Default::default()
    };

    let db = BlockDBHandle::new(config.clone()).unwrap();
    let mut auth_manager = AuthManager::new(config.clone()).unwrap();

    auth_manager
        .create_user("batcher", "pass123", vec![Permission::Write])
        .unwrap();

    let context = auth_manager.authenticate_user("batcher", "pass123").unwrap();

    let api_config = ApiConfig {
        auth_enabled: true,
        ..Default::default()
    };
    let server = BlockDBServer::with_auth(db, api_config, auth_manager);

    // Test 1: Batch write with authentication
    use blockdb::api::BatchWriteRequest;
    
    let batch_request = BatchWriteRequest {
        operations: vec![
            WriteRequest {
                key: "batch_key1".to_string(),
                value: "batch_value1".to_string(),
                encoding: None,
                auth_token: Some(context.session_id.clone()),
            },
            WriteRequest {
                key: "batch_key2".to_string(),
                value: "batch_value2".to_string(),
                encoding: None,
                auth_token: Some(context.session_id.clone()),
            },
            WriteRequest {
                key: "batch_key3".to_string(),
                value: "batch_value3".to_string(),
                encoding: None,
                auth_token: Some(context.session_id.clone()),
            },
        ],
    };

    let response = server.batch_write(batch_request).await.unwrap();
    assert!(response.success);
    assert_eq!(response.total_processed, 3);
    assert_eq!(response.results.len(), 3);
    
    // All operations should succeed
    for result in response.results {
        assert!(result.success);
    }
}

#[tokio::test]
async fn test_health_and_stats_endpoints() {
    let temp_dir = TempDir::new().unwrap();
    let config = BlockDBConfig {
        data_dir: temp_dir.path().to_string_lossy().to_string(),
        auth_enabled: true,
        ..Default::default()
    };

    let db = BlockDBHandle::new(config.clone()).unwrap();
    let auth_manager = AuthManager::new(config.clone()).unwrap();

    let api_config = ApiConfig {
        auth_enabled: true,
        ..Default::default()
    };
    let server = BlockDBServer::with_auth(db, api_config, auth_manager);

    // Test 1: Health endpoint should work without authentication
    let health_response = server.health().await.unwrap();
    assert_eq!(health_response.status, "healthy");
    assert!(health_response.integrity_verified);

    // Test 2: Stats endpoint should work without authentication
    let stats_response = server.stats().await.unwrap();
    assert_eq!(stats_response.total_writes, 0); // No writes yet
    assert_eq!(stats_response.total_reads, 0); // No reads yet
}

#[tokio::test]
async fn test_session_expiry_handling() {
    let temp_dir = TempDir::new().unwrap();
    let config = BlockDBConfig {
        data_dir: temp_dir.path().to_string_lossy().to_string(),
        auth_enabled: true,
        session_duration_hours: 0, // Sessions expire immediately for testing
        ..Default::default()
    };

    let db = BlockDBHandle::new(config.clone()).unwrap();
    let mut auth_manager = AuthManager::new(config.clone()).unwrap();

    auth_manager
        .create_user("expiry_test", "pass123", vec![Permission::Write])
        .unwrap();

    let context = auth_manager.authenticate_user("expiry_test", "pass123").unwrap();

    let api_config = ApiConfig {
        auth_enabled: true,
        ..Default::default()
    };
    let server = BlockDBServer::with_auth(db, api_config, auth_manager);

    // Wait a moment to ensure session expires
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Test: Operation with expired session should fail
    let write_request = WriteRequest {
        key: "expired_key".to_string(),
        value: "expired_value".to_string(),
        encoding: None,
        auth_token: Some(context.session_id),
    };

    let result = server.write(write_request).await;
    assert!(result.is_err()); // Should fail due to expired session
}

#[tokio::test]
async fn test_concurrent_authenticated_operations() {
    let temp_dir = TempDir::new().unwrap();
    let config = BlockDBConfig {
        data_dir: temp_dir.path().to_string_lossy().to_string(),
        auth_enabled: true,
        ..Default::default()
    };

    let db = BlockDBHandle::new(config.clone()).unwrap();
    let mut auth_manager = AuthManager::new(config.clone()).unwrap();

    // Create multiple users
    for i in 0..5 {
        auth_manager
            .create_user(&format!("user{}", i), "pass123", vec![Permission::Read, Permission::Write])
            .unwrap();
    }

    let api_config = ApiConfig {
        auth_enabled: true,
        ..Default::default()
    };
    let server = std::sync::Arc::new(BlockDBServer::with_auth(db, api_config, auth_manager.clone()));

    // Test: Concurrent operations from multiple authenticated users
    let mut handles = vec![];

    for i in 0..5 {
        let server_clone = server.clone();
        let mut auth_clone = auth_manager.clone();
        
        let handle = tokio::spawn(async move {
            let context = auth_clone
                .authenticate_user(&format!("user{}", i), "pass123")
                .unwrap();

            // Each user writes their own data
            let write_request = WriteRequest {
                key: format!("concurrent_key_{}", i),
                value: format!("concurrent_value_{}", i),
                encoding: None,
                auth_token: Some(context.session_id.clone()),
            };

            let write_result = server_clone.write(write_request).await.unwrap();
            assert!(write_result.success);

            // Each user reads their own data
            let read_request = ReadRequest {
                key: format!("concurrent_key_{}", i),
                encoding: None,
                auth_token: Some(context.session_id),
            };

            let read_result = server_clone.read(read_request).await.unwrap();
            assert!(read_result.success);
            assert_eq!(read_result.data.unwrap(), format!("concurrent_value_{}", i));
        });

        handles.push(handle);
    }

    // Wait for all operations to complete
    for handle in handles {
        handle.await.unwrap();
    }
}