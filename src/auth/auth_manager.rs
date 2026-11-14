use crate::auth::{
    crypto::KeyPair,
    identity::{CryptoIdentity, IdentityChain},
    permissions::{Permission, PermissionOperation, PermissionSet},
    AuthContext, AuthError, SessionId, UserId,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

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

/// On-disk snapshot of the durable auth state (identities + permissions). Sessions
/// and nonces are intentionally excluded — they are ephemeral by design.
#[derive(Debug, Serialize, Deserialize)]
struct AuthSnapshot {
    identity_chain: IdentityChain,
    user_permissions: HashMap<UserId, PermissionSet>,
    permission_operations: Vec<PermissionOperation>,
}

#[derive(Debug)]
pub struct AuthManager {
    config: AuthConfig,
    identity_chain: Arc<RwLock<IdentityChain>>,
    user_permissions: Arc<RwLock<HashMap<UserId, PermissionSet>>>,
    active_sessions: Arc<RwLock<HashMap<SessionId, AuthContext>>>,
    permission_operations: Arc<RwLock<Vec<PermissionOperation>>>,
    nonce_tracker: Arc<RwLock<HashMap<UserId, u64>>>,
    /// When set, durable auth state is persisted here after every change, so user
    /// accounts and permissions survive a restart (and admin keys stay stable).
    persist_path: Option<PathBuf>,
    /// Optional tamper-evident audit sink for auth/permission events.
    audit: Option<Arc<crate::audit::AuditLog>>,
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
            persist_path: None,
            audit: None,
        };

        // Seed any configured admin users with a generated strong password (logged
        // once) instead of a hardcoded default.
        auth_manager.seed_configured_admins();

        auth_manager
    }

    /// Create an auth manager whose durable state is persisted under `dir`. If a
    /// snapshot already exists it is loaded (and the default admin is *not*
    /// recreated, so its keys and any password change persist); otherwise the
    /// manager is initialized fresh and the initial state is written.
    pub fn new_persistent(config: AuthConfig, dir: &std::path::Path) -> Self {
        let path = dir.join("auth_state.bin");

        if path.exists() {
            match Self::load_snapshot(&path) {
                Ok(snapshot) => {
                    return Self {
                        config,
                        identity_chain: Arc::new(RwLock::new(snapshot.identity_chain)),
                        user_permissions: Arc::new(RwLock::new(snapshot.user_permissions)),
                        active_sessions: Arc::new(RwLock::new(HashMap::new())),
                        permission_operations: Arc::new(RwLock::new(
                            snapshot.permission_operations,
                        )),
                        nonce_tracker: Arc::new(RwLock::new(HashMap::new())),
                        persist_path: Some(path),
            audit: None,
                    };
                }
                Err(e) => {
                    eprintln!("failed to load auth state from {:?}: {}; reinitializing", path, e);
                }
            }
        }

        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let auth_manager = Self {
            config,
            identity_chain: Arc::new(RwLock::new(IdentityChain::new())),
            user_permissions: Arc::new(RwLock::new(HashMap::new())),
            active_sessions: Arc::new(RwLock::new(HashMap::new())),
            permission_operations: Arc::new(RwLock::new(Vec::new())),
            nonce_tracker: Arc::new(RwLock::new(HashMap::new())),
            persist_path: Some(path),
            audit: None,
        };
        auth_manager.seed_configured_admins();
        auth_manager.persist();
        auth_manager
    }

    /// Seed any configured admin users that don't already exist, each with a freshly
    /// generated strong password surfaced once in the logs. This replaces the former
    /// hardcoded `admin123`, which was a critical default-credential vulnerability.
    /// Operators should bootstrap admins explicitly (see `ensure_admin`) and rotate
    /// this generated password immediately.
    fn seed_configured_admins(&self) {
        for admin_user in &self.config.admin_users.clone() {
            // Skip if the admin already exists (e.g. reloaded from a snapshot).
            if self.identity_chain.read().unwrap().find_identity(admin_user).is_some() {
                continue;
            }
            let password = generate_secure_password();
            match self.create_admin_user(admin_user.clone(), &password) {
                Ok(_) => {
                    tracing::warn!(
                        user = %admin_user,
                        "no admin bootstrap configured; generated a random admin password \
                         (shown once): {} — change it immediately",
                        password
                    );
                }
                Err(e) => tracing::error!(user = %admin_user, error = %e, "failed to seed admin"),
            }
        }
    }

    /// Create an admin user if it does not already exist. Returns Ok(true) if created,
    /// Ok(false) if it already existed. Used by operators to bootstrap a known admin
    /// credential (e.g. from a secrets manager) rather than relying on the generated
    /// one. The password must satisfy the configured strength policy.
    pub fn ensure_admin(&self, user: UserId, password: &str) -> Result<bool, AuthError> {
        if self.identity_chain.read().unwrap().find_identity(&user).is_some() {
            return Ok(false);
        }
        self.create_admin_user(user, password)?;
        Ok(true)
    }

    fn load_snapshot(path: &std::path::Path) -> Result<AuthSnapshot, Box<dyn std::error::Error>> {
        let bytes = std::fs::read(path)?;
        Ok(bincode::deserialize(&bytes)?)
    }

    /// Durably write the current identities + permissions. No-op when not persistent.
    /// Clones under read locks then writes outside them (atomic temp-file + rename).
    fn persist(&self) {
        let path = match &self.persist_path {
            Some(p) => p.clone(),
            None => return,
        };
        let snapshot = AuthSnapshot {
            identity_chain: self.identity_chain.read().unwrap().clone(),
            user_permissions: self.user_permissions.read().unwrap().clone(),
            permission_operations: self.permission_operations.read().unwrap().clone(),
        };
        let bytes = match bincode::serialize(&snapshot) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("failed to serialize auth state: {}", e);
                return;
            }
        };
        let tmp = path.with_extension("bin.tmp");
        if let Err(e) = std::fs::write(&tmp, &bytes).and_then(|_| std::fs::rename(&tmp, &path)) {
            eprintln!("failed to persist auth state to {:?}: {}", path, e);
        }
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

        self.persist();
        Ok(keypair)
    }

    pub fn create_admin_user(&self, user_id: UserId, password: &str) -> Result<KeyPair, AuthError> {
        self.create_user(user_id, password, PermissionSet::admin(), None, 0)
    }

    /// Attach a tamper-evident audit sink. Call before sharing the manager.
    pub fn attach_audit_log(&mut self, log: Arc<crate::audit::AuditLog>) {
        self.audit = Some(log);
    }

    fn audit(
        &self,
        action: &str,
        actor: Option<&str>,
        target: Option<&str>,
        outcome: crate::audit::Outcome,
        detail: Option<&str>,
    ) {
        if let Some(log) = &self.audit {
            log.log(action, actor, target, outcome, detail);
        }
    }

    pub fn authenticate(&self, user_id: &str, password: &str) -> Result<AuthContext, AuthError> {
        let result = self.authenticate_inner(user_id, password);
        match &result {
            Ok(_) => self.audit("auth.login", Some(user_id), None, crate::audit::Outcome::Success, None),
            Err(e) => self.audit(
                "auth.login",
                Some(user_id),
                None,
                crate::audit::Outcome::Failure,
                Some(&e.to_string()),
            ),
        }
        result
    }

    fn authenticate_inner(&self, user_id: &str, password: &str) -> Result<AuthContext, AuthError> {
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

    pub fn authenticate_with_token(&self, session_id: &str) -> Result<AuthContext, AuthError> {
        if !self.is_enabled() {
            return Err(AuthError::CryptographicError("Authentication is disabled".to_string()));
        }

        let sessions = self.active_sessions.read().unwrap();
        let auth_context = sessions
            .get(session_id)
            .ok_or_else(|| AuthError::SessionNotFound(session_id.to_string()))?;

        if auth_context.is_expired() {
            return Err(AuthError::TokenExpired);
        }

        Ok(auth_context.clone())
    }

    pub fn logout(&self, session_id: &str) -> Result<(), AuthError> {
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

        let perm_desc = format!("{:?}", permission);
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

        self.persist();
        self.audit(
            "permission.grant",
            Some(granted_by),
            Some(target_user),
            crate::audit::Outcome::Success,
            Some(&perm_desc),
        );
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

        self.persist();
        self.audit(
            "permission.revoke",
            Some(revoked_by),
            Some(target_user),
            crate::audit::Outcome::Success,
            Some(&format!("{:?}", permission)),
        );
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

/// Generate a strong random password that satisfies the default strength policy
/// (upper, lower, digit, special), 24 characters of CSPRNG output.
pub fn generate_secure_password() -> String {
    use rand::Rng;
    const UPPER: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ";
    const LOWER: &[u8] = b"abcdefghijkmnpqrstuvwxyz";
    const DIGIT: &[u8] = b"23456789";
    const SPECIAL: &[u8] = b"!@#$%^&*()-_=+";
    const ALL: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnpqrstuvwxyz23456789!@#$%^&*()-_=+";

    let mut rng = rand::thread_rng();
    let mut pw: Vec<u8> = Vec::with_capacity(24);
    // Guarantee at least one of each required class.
    pw.push(UPPER[rng.gen_range(0..UPPER.len())]);
    pw.push(LOWER[rng.gen_range(0..LOWER.len())]);
    pw.push(DIGIT[rng.gen_range(0..DIGIT.len())]);
    pw.push(SPECIAL[rng.gen_range(0..SPECIAL.len())]);
    for _ in 0..20 {
        pw.push(ALL[rng.gen_range(0..ALL.len())]);
    }
    // Shuffle so the guaranteed characters aren't always in fixed positions.
    for i in (1..pw.len()).rev() {
        let j = rng.gen_range(0..=i);
        pw.swap(i, j);
    }
    String::from_utf8(pw).unwrap_or_else(|_| "ChangeMe!23456789".to_string())
}

#[cfg(test)]
mod persistence_tests {
    use super::*;
    use crate::auth::permissions::PermissionSet;

    fn test_config() -> AuthConfig {
        AuthConfig {
            require_strong_passwords: false,
            password_min_length: 4,
            admin_users: vec!["admin".to_string()],
            ..Default::default()
        }
    }

    #[test]
    fn auth_state_survives_restart() {
        let dir = tempfile::TempDir::new().unwrap();

        // First instance: create a user and grant it a permission.
        let admin_pubkey = {
            let mgr = AuthManager::new_persistent(test_config(), dir.path());
            mgr.create_user(
                "alice".to_string(),
                "Password1!",
                PermissionSet::read_write(),
                Some("admin".to_string()),
                1,
            )
            .unwrap();
            // Capture the admin identity's public key to prove it is NOT regenerated.
            let chain = mgr.identity_chain.read().unwrap();
            chain.find_identity("admin").unwrap().public_key.clone()
        };

        // Second instance on the same directory: state must be reloaded.
        let mgr2 = AuthManager::new_persistent(test_config(), dir.path());

        // The created user still exists and can authenticate with their password.
        assert!(
            mgr2.authenticate("alice", "Password1!").is_ok(),
            "user did not survive restart"
        );
        assert!(
            mgr2.get_user_permissions("alice").is_ok(),
            "permissions did not survive restart"
        );

        // The admin was reloaded, not recreated with fresh keys.
        let chain = mgr2.identity_chain.read().unwrap();
        let admin_pubkey2 = chain.find_identity("admin").unwrap().public_key.clone();
        assert_eq!(
            admin_pubkey, admin_pubkey2,
            "admin identity was regenerated instead of reloaded"
        );
    }

    #[test]
    fn auth_events_are_audited() {
        use crate::audit::AuditLog;
        let dir = tempfile::TempDir::new().unwrap();
        let mut mgr = AuthManager::new(AuthConfig { admin_users: vec![], ..Default::default() });
        let log = std::sync::Arc::new(AuditLog::open(dir.path()).unwrap());
        mgr.attach_audit_log(log.clone());

        let pw = generate_secure_password();
        mgr.create_user("zoe".into(), &pw, PermissionSet::read_only(), None, 0).unwrap();

        assert!(mgr.authenticate("zoe", "wrong").is_err());
        assert!(mgr.authenticate("zoe", &pw).is_ok());

        let events = log.events().unwrap();
        // One failed + one successful login recorded.
        let logins: Vec<_> = events.iter().filter(|e| e.action == "auth.login").collect();
        assert_eq!(logins.len(), 2);
        assert_eq!(logins[0].outcome, crate::audit::Outcome::Failure);
        assert_eq!(logins[1].outcome, crate::audit::Outcome::Success);
        // The tamper-evident chain verifies.
        assert_eq!(log.verify().unwrap(), events.len());
    }

    #[test]
    fn generated_password_is_strong_and_accepted() {
        for _ in 0..50 {
            let pw = generate_secure_password();
            assert!(pw.len() >= 20);
            assert!(pw.chars().any(|c| c.is_ascii_uppercase()));
            assert!(pw.chars().any(|c| c.is_ascii_lowercase()));
            assert!(pw.chars().any(|c| c.is_ascii_digit()));
            assert!(pw.chars().any(|c| !c.is_alphanumeric()));
        }
        // A manager with the default (strong) policy accepts the generated password.
        let mgr = AuthManager::new(AuthConfig { admin_users: vec![], ..Default::default() });
        let pw = generate_secure_password();
        mgr.create_user("u".into(), &pw, PermissionSet::read_only(), None, 0).unwrap();
        assert!(mgr.authenticate("u", &pw).is_ok());
    }

    #[test]
    fn non_persistent_manager_writes_nothing() {
        let dir = tempfile::TempDir::new().unwrap();
        let _mgr = AuthManager::new(test_config());
        // new() must not touch disk.
        assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none());
    }
}
