use blockdb::{BlockDBHandle, BlockDBConfig, AuthManager, Permission, AuthError};
use blockdb::api::{BlockDBServer, ApiConfig, LoginRequest, CreateUserRequest, WriteRequest, ReadRequest};
use tempfile::TempDir;
use std::time::Duration;
use tokio::time::sleep;

/// Comprehensive authentication integration tests
/// Tests the full authentication flow from user creation to API access
#[tokio::test]
async fn test_full_authentication_integration() {
    let temp_dir = TempDir::new().unwrap();
    let config = BlockDBConfig {
        data_dir: temp_dir.path().to_string_lossy().to_string(),
        auth_enabled: true,
        session_duration_hours: 1,
        password_min_length: 6,
        max_failed_attempts: 3,
        account_lockout_duration_minutes: 5,
        ..Default::default()
    };

    // Create database and auth manager
    let db = BlockDBHandle::new(config.clone()).unwrap();
    let mut auth_manager = AuthManager::new(config.clone()).unwrap();

    // Create API server with authentication
    let api_config = ApiConfig {
        auth_enabled: true,
        require_auth_for_reads: true,
        session_duration_hours: 1,
        ..Default::default()
    };
    let server = BlockDBServer::with_auth(db.clone(), api_config, auth_manager.clone());

    // Test 1: Create admin user
    let admin_permissions = vec![Permission::Admin];
    let admin_user_id = auth_manager
        .create_user("admin", "admin123", admin_permissions)
        .expect("Failed to create admin user");
    
    assert!(!admin_user_id.is_empty());

    // Test 2: Admin login
    let admin_context = auth_manager
        .authenticate_user("admin", "admin123")
        .expect("Failed to authenticate admin");
    
    assert_eq!(admin_context.user_id, admin_user_id);
    assert!(admin_context.has_permission(&Permission::Admin));

    // Test 3: Create regular user via API (authenticated as admin)
    let create_user_request = CreateUserRequest {
        username: "testuser".to_string(),
        password: "testpass123".to_string(),
        permissions: vec!["Read".to_string(), "Write".to_string()],
        auth_token: admin_context.session_id.clone(),
    };

    let user_response = server.create_user(create_user_request).await
        .expect("Failed to create user via API");
    
    assert!(user_response.success);
    assert!(user_response.user_id.is_some());

    // Test 4: Regular user login via API
    let login_request = LoginRequest {
        username: "testuser".to_string(),
        password: "testpass123".to_string(),
    };

    let login_response = server.login(login_request).await
        .expect("Failed to login via API");
    
    assert!(login_response.success);
    assert!(login_response.auth_token.is_some());
    
    let user_token = login_response.auth_token.unwrap();

    // Test 5: Authenticated write operation
    let write_request = WriteRequest {
        key: "test_key".to_string(),
        value: "test_value".to_string(),
        encoding: None,
        auth_token: Some(user_token.clone()),
    };

    let write_response = server.write(write_request).await
        .expect("Failed to perform authenticated write");
    
    assert!(write_response.success);

    // Test 6: Authenticated read operation
    let read_request = ReadRequest {
        key: "test_key".to_string(),
        encoding: None,
        auth_token: Some(user_token.clone()),
    };

    let read_response = server.read(read_request).await
        .expect("Failed to perform authenticated read");
    
    assert!(read_response.success);
    assert_eq!(read_response.data.unwrap(), "test_value");

    // Test 7: Unauthorized access (no token)
    let unauthorized_write = WriteRequest {
        key: "unauthorized_key".to_string(),
        value: "unauthorized_value".to_string(),
        encoding: None,
        auth_token: None,
    };

    let result = server.write(unauthorized_write).await;
    assert!(result.is_err());

    // Test 8: Invalid token access
    let invalid_token_write = WriteRequest {
        key: "invalid_key".to_string(),
        value: "invalid_value".to_string(),
        encoding: None,
        auth_token: Some("invalid_token".to_string()),
    };

    let result = server.write(invalid_token_write).await;
    assert!(result.is_err());

    // Test 9: Session expiry simulation
    // Note: In a real test, we'd wait for actual expiry or manipulate time
    let expired_context = auth_manager.validate_session(&user_token);
    // Should still be valid since we just created it
    assert!(expired_context.is_ok());
}

#[tokio::test]
async fn test_permission_management_integration() {
    let temp_dir = TempDir::new().unwrap();
    let config = BlockDBConfig {
        data_dir: temp_dir.path().to_string_lossy().to_string(),
        auth_enabled: true,
        ..Default::default()
    };

    let mut auth_manager = AuthManager::new(config.clone()).unwrap();

    // Create admin user
    let admin_id = auth_manager
        .create_user("admin", "admin123", vec![Permission::Admin])
        .unwrap();

    // Create regular user with only read permission
    let user_id = auth_manager
        .create_user("readonly_user", "pass123", vec![Permission::Read])
        .unwrap();

    // Test 1: User can authenticate
    let user_context = auth_manager
        .authenticate_user("readonly_user", "pass123")
        .unwrap();

    // Test 2: User has read permission
    assert!(user_context.has_permission(&Permission::Read));

    // Test 3: User doesn't have write permission
    assert!(!user_context.has_permission(&Permission::Write));

    // Test 4: Grant write permission
    auth_manager
        .grant_permission("readonly_user", Permission::Write)
        .unwrap();

    // Re-authenticate to get updated permissions
    let updated_context = auth_manager
        .authenticate_user("readonly_user", "pass123")
        .unwrap();

    // Test 5: User now has write permission
    assert!(updated_context.has_permission(&Permission::Read));
    assert!(updated_context.has_permission(&Permission::Write));

    // Test 6: Revoke read permission
    auth_manager
        .revoke_permission("readonly_user", Permission::Read)
        .unwrap();

    // Re-authenticate again
    let final_context = auth_manager
        .authenticate_user("readonly_user", "pass123")
        .unwrap();

    // Test 7: User no longer has read permission but still has write
    assert!(!final_context.has_permission(&Permission::Read));
    assert!(final_context.has_permission(&Permission::Write));
}

#[tokio::test]
async fn test_account_lockout_integration() {
    let temp_dir = TempDir::new().unwrap();
    let config = BlockDBConfig {
        data_dir: temp_dir.path().to_string_lossy().to_string(),
        auth_enabled: true,
        max_failed_attempts: 3,
        account_lockout_duration_minutes: 1, // Short for testing
        ..Default::default()
    };

    let mut auth_manager = AuthManager::new(config).unwrap();

    // Create test user
    auth_manager
        .create_user("lockout_test", "correct_pass", vec![Permission::Read])
        .unwrap();

    // Test 1: Successful login works
    let result = auth_manager.authenticate_user("lockout_test", "correct_pass");
    assert!(result.is_ok());

    // Test 2: Failed login attempts
    for _ in 0..3 {
        let result = auth_manager.authenticate_user("lockout_test", "wrong_pass");
        assert!(result.is_err());
    }

    // Test 3: Account should now be locked
    let result = auth_manager.authenticate_user("lockout_test", "correct_pass");
    assert!(result.is_err());
    
    // Check that it's specifically a lockout error (if implemented)
    match result.unwrap_err() {
        AuthError::InvalidCredentials => {
            // This is expected - the account is locked
        }
        _ => panic!("Expected InvalidCredentials error for locked account"),
    }

    // Test 4: Account is still locked even with correct password
    let result = auth_manager.authenticate_user("lockout_test", "correct_pass");
    assert!(result.is_err());
}

#[tokio::test]
async fn test_session_management_integration() {
    let temp_dir = TempDir::new().unwrap();
    let config = BlockDBConfig {
        data_dir: temp_dir.path().to_string_lossy().to_string(),
        auth_enabled: true,
        session_duration_hours: 1,
        ..Default::default()
    };

    let mut auth_manager = AuthManager::new(config).unwrap();

    // Create test user
    auth_manager
        .create_user("session_test", "pass123", vec![Permission::Read, Permission::Write])
        .unwrap();

    // Test 1: Login creates valid session
    let context = auth_manager
        .authenticate_user("session_test", "pass123")
        .unwrap();

    let session_id = context.session_id.clone();

    // Test 2: Session validation works
    let validated_context = auth_manager.validate_session(&session_id);
    assert!(validated_context.is_ok());

    // Test 3: Session has correct permissions
    let validated = validated_context.unwrap();
    assert!(validated.has_permission(&Permission::Read));
    assert!(validated.has_permission(&Permission::Write));

    // Test 4: Logout invalidates session
    auth_manager.logout(&session_id).unwrap();

    // Test 5: Session is no longer valid after logout
    let result = auth_manager.validate_session(&session_id);
    assert!(result.is_err());
}

#[tokio::test]
async fn test_auth_disabled_mode() {
    let temp_dir = TempDir::new().unwrap();
    let config = BlockDBConfig {
        data_dir: temp_dir.path().to_string_lossy().to_string(),
        auth_enabled: false, // Disable authentication
        ..Default::default()
    };

    let db = BlockDBHandle::new(config.clone()).unwrap();
    
    // Create API server without authentication
    let api_config = ApiConfig {
        auth_enabled: false,
        require_auth_for_reads: false,
        ..Default::default()
    };
    let server = BlockDBServer::new(db, api_config);

    // Test 1: Write operation works without authentication
    let write_request = WriteRequest {
        key: "no_auth_key".to_string(),
        value: "no_auth_value".to_string(),
        encoding: None,
        auth_token: None, // No token provided
    };

    let write_response = server.write(write_request).await
        .expect("Write should work without auth when disabled");
    
    assert!(write_response.success);

    // Test 2: Read operation works without authentication
    let read_request = ReadRequest {
        key: "no_auth_key".to_string(),
        encoding: None,
        auth_token: None, // No token provided
    };

    let read_response = server.read(read_request).await
        .expect("Read should work without auth when disabled");
    
    assert!(read_response.success);
    assert_eq!(read_response.data.unwrap(), "no_auth_value");
}

#[tokio::test]
async fn test_mixed_auth_mode() {
    let temp_dir = TempDir::new().unwrap();
    let config = BlockDBConfig {
        data_dir: temp_dir.path().to_string_lossy().to_string(),
        auth_enabled: true,
        ..Default::default()
    };

    let db = BlockDBHandle::new(config.clone()).unwrap();
    let mut auth_manager = AuthManager::new(config.clone()).unwrap();
    
    // Create user
    auth_manager
        .create_user("mixed_test", "pass123", vec![Permission::Read, Permission::Write])
        .unwrap();
    
    let context = auth_manager
        .authenticate_user("mixed_test", "pass123")
        .unwrap();

    // Create API server with auth enabled but reads don't require auth
    let api_config = ApiConfig {
        auth_enabled: true,
        require_auth_for_reads: false, // Reads don't require auth
        ..Default::default()
    };
    let server = BlockDBServer::with_auth(db, api_config, auth_manager);

    // Test 1: Write requires authentication
    let write_request = WriteRequest {
        key: "mixed_key".to_string(),
        value: "mixed_value".to_string(),
        encoding: None,
        auth_token: Some(context.session_id.clone()),
    };

    let write_response = server.write(write_request).await
        .expect("Authenticated write should work");
    
    assert!(write_response.success);

    // Test 2: Read works without authentication (due to config)
    let read_request = ReadRequest {
        key: "mixed_key".to_string(),
        encoding: None,
        auth_token: None, // No token needed for reads
    };

    let read_response = server.read(read_request).await
        .expect("Unauthenticated read should work when configured");
    
    assert!(read_response.success);
    assert_eq!(read_response.data.unwrap(), "mixed_value");

    // Test 3: Write without authentication fails
    let unauth_write = WriteRequest {
        key: "unauth_key".to_string(),
        value: "unauth_value".to_string(),
        encoding: None,
        auth_token: None,
    };

    let result = server.write(unauth_write).await;
    assert!(result.is_err());
}