use crate::auth::{
    crypto::{CryptoUtils, KeyPair},
    identity::{CryptoIdentity, IdentityChain},
    permissions::{Permission, PermissionOperation, PermissionSet},
    AuthContext, AuthError, SessionId, UserId,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    pub enabled: bool,
    pub session_duration_hours: u64,
    pub max_failed_attempts: u64,
    pub password_min_length: usize,
    pub require_strong_passwords: bool,
    pub admin_users: Vec<UserId>,
    pub allow_anonymous_reads: bool,
    pub token_refresh_threshold_hours: u64,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            session_duration_hours: 24,
            max_failed_attempts: 5,
            password_min_length: 8,
            require_strong_passwords: true,
            admin_users: vec!["admin".to_string()],
            allow_anonymous_reads: false,
            token_refresh_threshold_hours: 4,
        }
    }
}

#[derive(Debug)]
pub struct AuthManager {
    config: AuthConfig,
    identity_chain: Arc<RwLock<IdentityChain>>,
    user_permissions: Arc<RwLock<HashMap<UserId, PermissionSet>>>,
    active_sessions: Arc<RwLock<HashMap<SessionId, AuthContext>>>,
    permission_operations: Arc<RwLock<Vec<PermissionOperation>>>,
    nonce_tracker: Arc<RwLock<HashMap<UserId, u64>>>,
}

impl AuthManager {
    pub fn new(config: AuthConfig) -> Self {
        let auth_manager = Self {
            config,
            identity_chain: Arc::new(RwLock::new(IdentityChain::new())),
            user_permissions: Arc::new(RwLock::new(HashMap::new())),
            active_sessions: Arc::new(RwLock::new(HashMap::new())),
            permission_operations: Arc::new(RwLock::new(Vec::new())),
            nonce_tracker: Arc::new(RwLock::new(HashMap::new())),
        };

        // Initialize default admin user if configured
        if !auth_manager.config.admin_users.is_empty() {
            for admin_user in &auth_manager.config.admin_users.clone() {
                if let Err(e) = auth_manager.create_admin_user(admin_user.clone(), "admin123") {
                    eprintln!("Failed to create admin user {}: {}", admin_user, e);
                }
            }
        }

        auth_manager
    }

    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    pub fn create_user(
        &self,
        user_id: UserId,
        password: &str,
        permissions: PermissionSet,
        created_by: Option<UserId>,
        block_index: u64,
    ) -> Result<KeyPair, AuthError> {
        if !self.is_enabled() {
            return Err(AuthError::CryptographicError("Authentication is disabled".to_string()));
        }

        // Check if user already exists
        {
            let chain = self.identity_chain.read().unwrap();
            if chain.find_identity(&user_id).is_some() {
                return Err(AuthError::UserAlreadyExists(user_id));
            }
        }

        // Validate password
        self.validate_password(password)?;

        // Create cryptographic identity
        let (identity, keypair) = CryptoIdentity::create_with_password(
            user_id.clone(),
            password,
            block_index,
            created_by,
        )?;

        // Add to identity chain
        {
            let mut chain = self.identity_chain.write().unwrap();
            chain.add_identity(identity)?;
        }

        // Set permissions
        {
            let mut user_perms = self.user_permissions.write().unwrap();
            user_perms.insert(user_id, permissions);
        }

        Ok(keypair)
    }

    pub fn create_admin_user(&self, user_id: UserId, password: &str) -> Result<KeyPair, AuthError> {
        self.create_user(user_id, password, PermissionSet::admin(), None, 0)
    }

    pub fn authenticate(&self, user_id: &str, password: &str) -> Result<AuthContext, AuthError> {
        if !self.is_enabled() {
            return Err(AuthError::CryptographicError("Authentication is disabled".to_string()));
        }

        let identity = {
            let mut chain = self.identity_chain.write().unwrap();
            let identity = chain
                .find_identity_mut(user_id)
                .ok_or_else(|| AuthError::UserNotFound(user_id.to_string()))?;

            // Check if account is locked due to failed attempts
            if identity.should_lock_account(self.config.max_failed_attempts) {
                return Err(AuthError::InvalidCredentials);
            }

            // Check if user is active
            if !identity.is_active() {
                return Err(AuthError::InvalidCredentials);
            }

            identity.clone()
        };

        // Verify password
        let password_valid = identity.verify_password(password)?;
        
        {
            let mut chain = self.identity_chain.write().unwrap();
            let identity = chain.find_identity_mut(user_id).unwrap();
            
            if password_valid {
                identity.record_login_success();
            } else {
                identity.record_login_failure();
                return Err(AuthError::InvalidCredentials);
            }
        }

        // Get user permissions
        let permissions = {
            let user_perms = self.user_permissions.read().unwrap();
            user_perms
                .get(user_id)
                .cloned()
                .unwrap_or_else(PermissionSet::new)
        };

        // Create auth context
        let session_duration_secs = self.config.session_duration_hours * 3600;
        let auth_context = AuthContext::new(user_id.to_string(), permissions.get_permissions(), session_duration_secs);

        // Store active session
        {
            let mut sessions = self.active_sessions.write().unwrap();
            sessions.insert(auth_context.session_id.clone(), auth_context.clone());
        }

        Ok(auth_context)
    }

    pub fn authenticate_with_token(&self, session_id: &SessionId) -> Result<AuthContext, AuthError> {
        if !self.is_enabled() {
            return Err(AuthError::CryptographicError("Authentication is disabled".to_string()));
        }

        let sessions = self.active_sessions.read().unwrap();
        let auth_context = sessions
            .get(session_id)
            .ok_or_else(|| AuthError::SessionNotFound(session_id.clone()))?;

        if auth_context.is_expired() {
            return Err(AuthError::TokenExpired);
        }

        Ok(auth_context.clone())
    }

    pub fn logout(&self, session_id: &SessionId) -> Result<(), AuthError> {
        let mut sessions = self.active_sessions.write().unwrap();
        sessions.remove(session_id);
        Ok(())
    }

    pub fn check_permission(&self, user_id: &str, required_permission: &Permission) -> Result<bool, AuthError> {
        if !self.is_enabled() {
            return Ok(true); // Allow all operations when auth is disabled
        }

        // Check if user exists and is active
        {
            let chain = self.identity_chain.read().unwrap();
            let identity = chain
                .find_identity(user_id)
                .ok_or_else(|| AuthError::UserNotFound(user_id.to_string()))?;

            if !identity.is_active() {
                return Err(AuthError::PermissionDenied {
                    operation: format!("{:?}", required_permission),
                    resource: "system".to_string(),
                    user: user_id.to_string(),
                });
            }
        }

        // Check permissions
        let user_perms = self.user_permissions.read().unwrap();
        let permissions = user_perms
            .get(user_id)
            .ok_or_else(|| AuthError::UserNotFound(user_id.to_string()))?;

        if permissions.has_permission(required_permission) {
            Ok(true)
        } else {
            Err(AuthError::InsufficientPermissions {
                required: required_permission.clone(),
                user: user_id.to_string(),
            })
        }
    }

    pub fn grant_permission(
        &self,
        target_user: &str,
        permission: Permission,
        granted_by: &str,
        block_index: u64,
    ) -> Result<(), AuthError> {
        // Check if grantor has permission to grant
        self.check_permission(granted_by, &Permission::GrantPermission)?;

        // Check if target user exists
        {
            let chain = self.identity_chain.read().unwrap();
            chain
                .find_identity(target_user)
                .ok_or_else(|| AuthError::UserNotFound(target_user.to_string()))?;
        }

        // Grant permission
        {
            let mut user_perms = self.user_permissions.write().unwrap();
            let permissions = user_perms
                .entry(target_user.to_string())
                .or_insert_with(PermissionSet::new);
            permissions.add_permission(permission.clone());
        }

        // Record operation
        let operation = PermissionOperation::Grant {
            target_user: target_user.to_string(),
            permission,
            granted_by: granted_by.to_string(),
            block_index,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };

        {
            let mut ops = self.permission_operations.write().unwrap();
            ops.push(operation);
        }

        Ok(())
    }

    pub fn revoke_permission(
        &self,
        target_user: &str,
        permission: &Permission,
        revoked_by: &str,
        block_index: u64,
    ) -> Result<(), AuthError> {
        // Check if revoker has permission to revoke
        self.check_permission(revoked_by, &Permission::RevokePermission)?;

        // Revoke permission
        {
            let mut user_perms = self.user_permissions.write().unwrap();
            if let Some(permissions) = user_perms.get_mut(target_user) {
                permissions.remove_permission(permission);
            }
        }

        // Record operation
        let operation = PermissionOperation::Revoke {
            target_user: target_user.to_string(),
            permission: permission.clone(),
            revoked_by: revoked_by.to_string(),
            block_index,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };

        {
            let mut ops = self.permission_operations.write().unwrap();
            ops.push(operation);
        }

        Ok(())
    }

    pub fn get_user_permissions(&self, user_id: &str) -> Result<PermissionSet, AuthError> {
        let user_perms = self.user_permissions.read().unwrap();
        user_perms
            .get(user_id)
            .cloned()
            .ok_or_else(|| AuthError::UserNotFound(user_id.to_string()))
    }

    pub fn list_users(&self) -> Vec<UserId> {
        let chain = self.identity_chain.read().unwrap();
        chain.identities.iter().map(|id| id.user_id.clone()).collect()
    }

    pub fn get_next_nonce(&self, user_id: &str) -> u64 {
        let mut nonces = self.nonce_tracker.write().unwrap();
        let nonce = nonces.entry(user_id.to_string()).or_insert(0);
        *nonce += 1;
        *nonce
    }

    pub fn verify_operation_signature(
        &self,
        operation: &crate::transaction::Operation,
        user_id: &str,
        nonce: u64,
        timestamp: u64,
        signature: &[u8],
    ) -> Result<bool, AuthError> {
        let chain = self.identity_chain.read().unwrap();
        let identity = chain
            .find_identity(user_id)
            .ok_or_else(|| AuthError::UserNotFound(user_id.to_string()))?;

        CryptoIdentity::verify_operation_signature(
            operation,
            user_id,
            nonce,
            timestamp,
            signature,
            &identity.public_key,
        )
    }

    pub fn cleanup_expired_sessions(&self) {
        let mut sessions = self.active_sessions.write().unwrap();
        sessions.retain(|_, context| !context.is_expired());
    }

    pub fn get_audit_trail(&self) -> Vec<PermissionOperation> {
        let ops = self.permission_operations.read().unwrap();
        ops.clone()
    }

    pub fn verify_identity_chain_integrity(&self) -> bool {
        let chain = self.identity_chain.read().unwrap();
        chain.verify_chain_integrity()
    }

    fn validate_password(&self, password: &str) -> Result<(), AuthError> {
        if password.len() < self.config.password_min_length {
            return Err(AuthError::CryptographicError(format!(
                "Password must be at least {} characters long",
                self.config.password_min_length
            )));
        }

        if self.config.require_strong_passwords {
            let has_upper = password.chars().any(|c| c.is_ascii_uppercase());
            let has_lower = password.chars().any(|c| c.is_ascii_lowercase());
            let has_digit = password.chars().any(|c| c.is_ascii_digit());
            let has_special = password.chars().any(|c| !c.is_alphanumeric());

            if !(has_upper && has_lower && has_digit && has_special) {
                return Err(AuthError::CryptographicError(
                    "Password must contain uppercase, lowercase, digit, and special character".to_string(),
                ));
            }
        }

        Ok(())
    }
}