#[cfg(test)]
mod simple_auth_tests {
    use crate::auth::{
        auth_manager::{AuthConfig, AuthManager},
        crypto::KeyPair,
        permissions::{Permission, PermissionSet},
    };

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
    fn test_permission_implications() {
        assert!(Permission::Admin.implies(&Permission::Read));
        assert!(Permission::Admin.implies(&Permission::Write));
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
        let result = auth_manager.create_user(
            "test_user".to_string(),
            "password123",
            permissions,
            Some("admin".to_string()),
            0,
        );
        
        assert!(result.is_ok());
        let keypair = result.unwrap();
        assert_eq!(keypair.public_key.len(), 32);
        assert_eq!(keypair.private_key.len(), 32);
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
        assert!(result.is_err());
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
    }
}