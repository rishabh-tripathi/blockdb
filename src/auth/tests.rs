#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{
        auth_manager::{AuthConfig, AuthManager},
        crypto::{CryptoUtils, KeyPair},
        identity::CryptoIdentity,
        permissions::{Permission, PermissionSet},
        AuthError,
    };
    use crate::transaction::Operation;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn get_test_config() -> AuthConfig {
        AuthConfig {
            enabled: true,
            session_duration_hours: 1,
            max_failed_attempts: 3,
            password_min_length: 6,
            require_strong_passwords: false,
            admin_users: vec!["admin".to_string()],
            allow_anonymous_reads: false,
            token_refresh_threshold_hours: 1,
        }
    }

    #[test]
    fn test_keypair_generation() {
        let keypair = KeyPair::generate().unwrap();
        assert_eq!(keypair.public_key.len(), 32);
        assert_eq!(keypair.private_key.len(), 32);
    }

    #[test]
    fn test_keypair_from_private_key() {
        let original_keypair = KeyPair::generate().unwrap();
        let reconstructed_keypair = KeyPair::from_private_key(&original_keypair.private_key).unwrap();
        
        assert_eq!(original_keypair.public_key, reconstructed_keypair.public_key);
        assert_eq!(original_keypair.private_key, reconstructed_keypair.private_key);
    }

    #[test]
    fn test_sign_and_verify() {
        let keypair = KeyPair::generate().unwrap();
        let data = b"test message";
        
        let signature = CryptoUtils::sign_data(data, &keypair.private_key).unwrap();
        let is_valid = CryptoUtils::verify_signature(data, &signature, &keypair.public_key).unwrap();
        
        assert!(is_valid);
    }

    #[test]
    fn test_invalid_signature() {
        let keypair1 = KeyPair::generate().unwrap();
        let keypair2 = KeyPair::generate().unwrap();
        let data = b"test message";
        
        let signature = CryptoUtils::sign_data(data, &keypair1.private_key).unwrap();
        let is_valid = CryptoUtils::verify_signature(data, &signature, &keypair2.public_key).unwrap();
        
        assert!(!is_valid);
    }

    #[test]
    fn test_permission_implications() {
        assert!(Permission::Admin.implies(&Permission::Read));
        assert!(Permission::Admin.implies(&Permission::Write));
        assert!(Permission::Admin.implies(&Permission::CreateUser));
        assert!(Permission::Write.implies(&Permission::Read));
        assert!(!Permission::Read.implies(&Permission::Write));
    }

    #[test]
    fn test_permission_set() {
        let mut permissions = PermissionSet::new();
        assert!(!permissions.has_permission(&Permission::Read));
        
        permissions.add_permission(Permission::Write);
        assert!(permissions.has_permission(&Permission::Write));
        assert!(permissions.has_permission(&Permission::Read)); // Write implies Read
        
        permissions.remove_permission(&Permission::Write);
        assert!(!permissions.has_permission(&Permission::Write));
        assert!(!permissions.has_permission(&Permission::Read));
    }

    #[test]
    fn test_admin_permission_set() {
        let admin_permissions = PermissionSet::admin();
        assert!(admin_permissions.has_permission(&Permission::Admin));
        assert!(admin_permissions.has_permission(&Permission::Read));
        assert!(admin_permissions.has_permission(&Permission::Write));
        assert!(admin_permissions.has_permission(&Permission::CreateUser));
    }

    #[test]
    fn test_crypto_identity_creation() {
        let keypair = KeyPair::generate().unwrap();
        let identity = CryptoIdentity::new(
            "test_user".to_string(),
            keypair.public_key,
            0,
            Some("admin".to_string()),
        );
        
        assert_eq!(identity.user_id, "test_user");
        assert!(identity.is_active());
        assert!(!identity.is_revoked());
        assert!(!identity.is_suspended());
    }

    #[test]
    fn test_crypto_identity_with_password() {
        let (identity, _keypair) = CryptoIdentity::create_with_password(
            "test_user".to_string(),
            "testpass123",
            0,
            Some("admin".to_string()),
        ).unwrap();
        
        assert!(identity.verify_password("testpass123").unwrap());
        assert!(!identity.verify_password("wrongpass").unwrap());
    }

    #[test]
    fn test_identity_revocation() {
        let keypair = KeyPair::generate().unwrap();
        let mut identity = CryptoIdentity::new(
            "test_user".to_string(),
            keypair.public_key,
            0,
            Some("admin".to_string()),
        );
        
        assert!(identity.is_active());
        
        identity.revoke("admin".to_string(), "User violated policy".to_string(), 1);
        assert!(identity.is_revoked());
        assert!(!identity.is_active());
    }

    #[test]
    fn test_identity_suspension() {
        let keypair = KeyPair::generate().unwrap();
        let mut identity = CryptoIdentity::new(
            "test_user".to_string(),
            keypair.public_key,
            0,
            Some("admin".to_string()),
        );
        
        let future_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() + 3600; // 1 hour from now
        
        identity.suspend("admin".to_string(), "Temporary suspension".to_string(), Some(future_time), 1);
        assert!(identity.is_suspended());
        assert!(!identity.is_active());
    }

    #[test]
    fn test_failed_login_tracking() {
        let (mut identity, _keypair) = CryptoIdentity::create_with_password(
            "test_user".to_string(),
            "testpass123",
            0,
            Some("admin".to_string()),
        ).unwrap();
        
        assert_eq!(identity.metadata.failed_login_attempts, 0);
        assert!(!identity.should_lock_account(3));
        
        identity.record_login_failure();
        identity.record_login_failure();
        identity.record_login_failure();
        
        assert_eq!(identity.metadata.failed_login_attempts, 3);
        assert!(identity.should_lock_account(3));
        
        identity.record_login_success();
        assert_eq!(identity.metadata.failed_login_attempts, 0);
        assert!(!identity.should_lock_account(3));
    }

    #[test]
    fn test_operation_signing_and_verification() {
        let keypair = KeyPair::generate().unwrap();
        let operation = Operation::Put {
            key: b"test_key".to_vec(),
            value: b"test_value".to_vec(),
        };
        let user_id = "test_user".to_string();
        let nonce = 1;
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        let signature = CryptoIdentity::sign_operation(
            &operation,
            &user_id,
            nonce,
            timestamp,
            &keypair.private_key,
        ).unwrap();
        
        let is_valid = CryptoIdentity::verify_operation_signature(
            &operation,
            &user_id,
            nonce,
            timestamp,
            &signature,
            &keypair.public_key,
        ).unwrap();
        
        assert!(is_valid);
    }

    #[test]
    fn test_identity_chain() {
        let mut chain = crate::auth::identity::IdentityChain::new();
        
        let keypair1 = KeyPair::generate().unwrap();
        let identity1 = CryptoIdentity::new(
            "user1".to_string(),
            keypair1.public_key,
            0,
            None,
        );
        
        let keypair2 = KeyPair::generate().unwrap();
        let identity2 = CryptoIdentity::new(
            "user2".to_string(),
            keypair2.public_key,
            1,
            Some("user1".to_string()),
        );
        
        chain.add_identity(identity1).unwrap();
        chain.add_identity(identity2).unwrap();
        
        assert!(chain.verify_chain_integrity());
        assert_eq!(chain.identities.len(), 2);
        assert!(chain.find_identity("user1").is_some());
        assert!(chain.find_identity("user2").is_some());
        assert!(chain.find_identity("nonexistent").is_none());
    }

    #[test]
    fn test_auth_manager_creation() {
        let config = get_test_config();
        let auth_manager = AuthManager::new(config);
        assert!(auth_manager.is_enabled());
    }

    #[test]
    fn test_user_creation() {
        let config = get_test_config();
        let auth_manager = AuthManager::new(config);
        
        let permissions = PermissionSet::read_write();
        let keypair = auth_manager.create_user(
            "test_user".to_string(),
            "password123",
            permissions,
            Some("admin".to_string()),
            0,
        ).unwrap();
        
        assert_eq!(keypair.public_key.len(), 32);
        assert_eq!(keypair.private_key.len(), 32);
    }

    #[test]
    fn test_duplicate_user_creation() {
        let config = get_test_config();
        let auth_manager = AuthManager::new(config);
        
        let permissions = PermissionSet::read_write();
        
        // First creation should succeed
        auth_manager.create_user(
            "test_user".to_string(),
            "password123",
            permissions.clone(),
            Some("admin".to_string()),
            0,
        ).unwrap();
        
        // Second creation should fail
        let result = auth_manager.create_user(
            "test_user".to_string(),
            "password456",
            permissions,
            Some("admin".to_string()),
            1,
        );
        
        assert!(matches!(result, Err(AuthError::UserAlreadyExists(_))));
    }

    #[test]
    fn test_user_authentication() {
        let config = get_test_config();
        let auth_manager = AuthManager::new(config);
        
        let permissions = PermissionSet::read_write();
        auth_manager.create_user(
            "test_user".to_string(),
            "password123",
            permissions,
            Some("admin".to_string()),
            0,
        ).unwrap();
        
        // Valid authentication
        let auth_context = auth_manager.authenticate("test_user", "password123").unwrap();
        assert_eq!(auth_context.user_id, "test_user");
        assert!(!auth_context.is_expired());
        
        // Invalid password
        let result = auth_manager.authenticate("test_user", "wrongpassword");
        assert!(matches!(result, Err(AuthError::InvalidCredentials)));
        
        // Non-existent user
        let result = auth_manager.authenticate("nonexistent", "password123");
        assert!(matches!(result, Err(AuthError::UserNotFound(_))));
    }

    #[test]
    fn test_session_management() {
        let config = get_test_config();
        let auth_manager = AuthManager::new(config);
        
        let permissions = PermissionSet::read_write();
        auth_manager.create_user(
            "test_user".to_string(),
            "password123",
            permissions,
            Some("admin".to_string()),
            0,
        ).unwrap();
        
        let auth_context = auth_manager.authenticate("test_user", "password123").unwrap();
        let session_id = auth_context.session_id.clone();
        
        // Token authentication should work
        let token_auth = auth_manager.authenticate_with_token(&session_id).unwrap();
        assert_eq!(token_auth.user_id, "test_user");
        
        // Logout should invalidate session
        auth_manager.logout(&session_id).unwrap();
        let result = auth_manager.authenticate_with_token(&session_id);
        assert!(matches!(result, Err(AuthError::SessionNotFound(_))));
    }

    #[test]
    fn test_permission_checking() {
        let config = get_test_config();
        let auth_manager = AuthManager::new(config);
        
        let read_only_permissions = PermissionSet::read_only();
        auth_manager.create_user(
            "read_user".to_string(),
            "password123",
            read_only_permissions,
            Some("admin".to_string()),
            0,
        ).unwrap();
        
        // Should have read permission
        assert!(auth_manager.check_permission("read_user", &Permission::Read).is_ok());
        
        // Should not have write permission
        let result = auth_manager.check_permission("read_user", &Permission::Write);
        assert!(matches!(result, Err(AuthError::InsufficientPermissions { .. })));
    }

    #[test]
    fn test_permission_granting_and_revoking() {
        let config = get_test_config();
        let auth_manager = AuthManager::new(config);
        
        let basic_permissions = PermissionSet::read_only();
        auth_manager.create_user(
            "test_user".to_string(),
            "password123",
            basic_permissions,
            Some("admin".to_string()),
            0,
        ).unwrap();
        
        // Initially should not have write permission
        let result = auth_manager.check_permission("test_user", &Permission::Write);
        assert!(result.is_err());
        
        // Admin grants write permission
        auth_manager.grant_permission(
            "test_user",
            Permission::Write,
            "admin",
            1,
        ).unwrap();
        
        // Now should have write permission
        assert!(auth_manager.check_permission("test_user", &Permission::Write).is_ok());
        
        // Admin revokes write permission
        auth_manager.revoke_permission(
            "test_user",
            &Permission::Write,
            "admin",
            2,
        ).unwrap();
        
        // Should no longer have write permission
        let result = auth_manager.check_permission("test_user", &Permission::Write);
        assert!(result.is_err());
    }

    #[test]
    fn test_admin_permissions() {
        let config = get_test_config();
        let auth_manager = AuthManager::new(config);
        
        // Admin should have all permissions
        assert!(auth_manager.check_permission("admin", &Permission::Read).is_ok());
        assert!(auth_manager.check_permission("admin", &Permission::Write).is_ok());
        assert!(auth_manager.check_permission("admin", &Permission::CreateUser).is_ok());
        assert!(auth_manager.check_permission("admin", &Permission::GrantPermission).is_ok());
    }

    #[test]
    fn test_nonce_tracking() {
        let config = get_test_config();
        let auth_manager = AuthManager::new(config);
        
        let nonce1 = auth_manager.get_next_nonce("user1");
        let nonce2 = auth_manager.get_next_nonce("user1");
        let nonce3 = auth_manager.get_next_nonce("user2");
        
        assert_eq!(nonce1, 1);
        assert_eq!(nonce2, 2);
        assert_eq!(nonce3, 1);
    }

    #[test]
    fn test_operation_signature_verification() {
        let config = get_test_config();
        let auth_manager = AuthManager::new(config);
        
        let permissions = PermissionSet::read_write();
        let keypair = auth_manager.create_user(
            "test_user".to_string(),
            "password123",
            permissions,
            Some("admin".to_string()),
            0,
        ).unwrap();
        
        let operation = Operation::Put {
            key: b"test_key".to_vec(),
            value: b"test_value".to_vec(),
        };
        let user_id = "test_user";
        let nonce = 1;
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        let signature = CryptoIdentity::sign_operation(
            &operation,
            user_id,
            nonce,
            timestamp,
            &keypair.private_key,
        ).unwrap();
        
        let is_valid = auth_manager.verify_operation_signature(
            &operation,
            user_id,
            nonce,
            timestamp,
            &signature,
        ).unwrap();
        
        assert!(is_valid);
    }

    #[test]
    fn test_password_validation() {
        let mut config = get_test_config();
        config.require_strong_passwords = true;
        config.password_min_length = 8;
        
        let auth_manager = AuthManager::new(config);
        let permissions = PermissionSet::read_only();
        
        // Too short password should fail
        let result = auth_manager.create_user(
            "user1".to_string(),
            "short",
            permissions.clone(),
            Some("admin".to_string()),
            0,
        );
        assert!(result.is_err());
        
        // Weak password should fail with strong password requirement
        let result = auth_manager.create_user(
            "user2".to_string(),
            "simplepwd",
            permissions.clone(),
            Some("admin".to_string()),
            0,
        );
        assert!(result.is_err());
        
        // Strong password should succeed
        auth_manager.create_user(
            "user3".to_string(),
            "StrongP@ss123",
            permissions,
            Some("admin".to_string()),
            0,
        ).unwrap();
    }

    #[test]
    fn test_account_lockout() {
        let mut config = get_test_config();
        config.max_failed_attempts = 2;
        
        let auth_manager = AuthManager::new(config);
        let permissions = PermissionSet::read_only();
        
        auth_manager.create_user(
            "test_user".to_string(),
            "password123",
            permissions,
            Some("admin".to_string()),
            0,
        ).unwrap();
        
        // First failed attempt
        let result = auth_manager.authenticate("test_user", "wrongpass");
        assert!(matches!(result, Err(AuthError::InvalidCredentials)));
        
        // Second failed attempt
        let result = auth_manager.authenticate("test_user", "wrongpass");
        assert!(matches!(result, Err(AuthError::InvalidCredentials)));
        
        // Third attempt should also fail due to account lockout
        let result = auth_manager.authenticate("test_user", "wrongpass");
        assert!(matches!(result, Err(AuthError::InvalidCredentials)));
        
        // Even correct password should fail due to lockout
        let result = auth_manager.authenticate("test_user", "password123");
        assert!(matches!(result, Err(AuthError::InvalidCredentials)));
    }

    #[test]
    fn test_audit_trail() {
        let config = get_test_config();
        let auth_manager = AuthManager::new(config);
        
        let permissions = PermissionSet::read_only();
        auth_manager.create_user(
            "test_user".to_string(),
            "password123",
            permissions,
            Some("admin".to_string()),
            0,
        ).unwrap();
        
        // Grant and revoke permissions
        auth_manager.grant_permission(
            "test_user",
            Permission::Write,
            "admin",
            1,
        ).unwrap();
        
        auth_manager.revoke_permission(
            "test_user",
            &Permission::Write,
            "admin",
            2,
        ).unwrap();
        
        let audit_trail = auth_manager.get_audit_trail();
        assert_eq!(audit_trail.len(), 2);
        
        // Check that operations are recorded correctly
        assert!(matches!(
            &audit_trail[0],
            crate::auth::permissions::PermissionOperation::Grant { .. }
        ));
        assert!(matches!(
            &audit_trail[1],
            crate::auth::permissions::PermissionOperation::Revoke { .. }
        ));
    }

    #[test]
    fn test_identity_chain_integrity() {
        let config = get_test_config();
        let auth_manager = AuthManager::new(config);
        
        let permissions = PermissionSet::read_only();
        
        // Create multiple users
        auth_manager.create_user(
            "user1".to_string(),
            "password123",
            permissions.clone(),
            Some("admin".to_string()),
            0,
        ).unwrap();
        
        auth_manager.create_user(
            "user2".to_string(),
            "password123",
            permissions.clone(),
            Some("admin".to_string()),
            1,
        ).unwrap();
        
        auth_manager.create_user(
            "user3".to_string(),
            "password123",
            permissions,
            Some("admin".to_string()),
            2,
        ).unwrap();
        
        // Verify chain integrity
        assert!(auth_manager.verify_identity_chain_integrity());
    }

    #[test]
    fn test_session_cleanup() {
        let mut config = get_test_config();
        config.session_duration_hours = 0; // Immediate expiration for testing
        
        let auth_manager = AuthManager::new(config);
        let permissions = PermissionSet::read_only();
        
        auth_manager.create_user(
            "test_user".to_string(),
            "password123",
            permissions,
            Some("admin".to_string()),
            0,
        ).unwrap();
        
        let auth_context = auth_manager.authenticate("test_user", "password123").unwrap();
        let session_id = auth_context.session_id.clone();
        
        // Session should be expired immediately
        std::thread::sleep(std::time::Duration::from_millis(100));
        
        // Cleanup expired sessions
        auth_manager.cleanup_expired_sessions();
        
        // Session should no longer exist
        let result = auth_manager.authenticate_with_token(&session_id);
        assert!(matches!(result, Err(AuthError::SessionNotFound(_))));
    }
}