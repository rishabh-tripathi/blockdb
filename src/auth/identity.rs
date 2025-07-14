use crate::auth::{
    crypto::{CryptoUtils, KeyPair},
    permissions::{Permission, PermissionSet},
    AuthError, PrivateKey, PublicKey, Signature, UserId,
};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CryptoIdentity {
    pub user_id: UserId,
    pub public_key: PublicKey,
    pub creation_block: u64,
    pub creation_timestamp: u64,
    pub permissions_merkle_root: Vec<u8>,
    pub revocation_status: RevocationStatus,
    pub metadata: IdentityMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RevocationStatus {
    Active,
    Revoked {
        revoked_at: u64,
        revoked_by: UserId,
        reason: String,
        block_index: u64,
    },
    Suspended {
        suspended_at: u64,
        suspended_by: UserId,
        reason: String,
        until: Option<u64>,
        block_index: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityMetadata {
    pub display_name: Option<String>,
    pub email: Option<String>,
    pub created_by: Option<UserId>,
    pub last_login: Option<u64>,
    pub login_count: u64,
    pub failed_login_attempts: u64,
    pub last_failed_login: Option<u64>,
    pub password_hash: Option<Vec<u8>>,
    pub password_salt: Option<Vec<u8>>,
}

impl Default for IdentityMetadata {
    fn default() -> Self {
        Self {
            display_name: None,
            email: None,
            created_by: None,
            last_login: None,
            login_count: 0,
            failed_login_attempts: 0,
            last_failed_login: None,
            password_hash: None,
            password_salt: None,
        }
    }
}

impl CryptoIdentity {
    pub fn new(
        user_id: UserId,
        public_key: PublicKey,
        creation_block: u64,
        created_by: Option<UserId>,
    ) -> Self {
        let creation_timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut metadata = IdentityMetadata::default();
        metadata.created_by = created_by;

        Self {
            user_id,
            public_key,
            creation_block,
            creation_timestamp,
            permissions_merkle_root: vec![],
            revocation_status: RevocationStatus::Active,
            metadata,
        }
    }

    pub fn create_with_password(
        user_id: UserId,
        password: &str,
        creation_block: u64,
        created_by: Option<UserId>,
    ) -> Result<(Self, KeyPair), AuthError> {
        let keypair = KeyPair::generate()?;
        let salt = CryptoUtils::generate_salt();
        let password_hash = CryptoUtils::hash_password(password, &salt);

        let mut identity = Self::new(user_id, keypair.public_key.clone(), creation_block, created_by);
        identity.metadata.password_hash = Some(password_hash);
        identity.metadata.password_salt = Some(salt.to_vec());

        Ok((identity, keypair))
    }

    pub fn verify_password(&self, password: &str) -> Result<bool, AuthError> {
        match (&self.metadata.password_hash, &self.metadata.password_salt) {
            (Some(hash), Some(salt)) => {
                let computed_hash = CryptoUtils::hash_password(password, salt);
                Ok(computed_hash == *hash)
            }
            _ => Ok(false), // No password set
        }
    }

    pub fn is_active(&self) -> bool {
        matches!(self.revocation_status, RevocationStatus::Active)
    }

    pub fn is_revoked(&self) -> bool {
        matches!(self.revocation_status, RevocationStatus::Revoked { .. })
    }

    pub fn is_suspended(&self) -> bool {
        match &self.revocation_status {
            RevocationStatus::Suspended { until, .. } => {
                if let Some(until_time) = until {
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_secs();
                    now < *until_time
                } else {
                    true // Indefinite suspension
                }
            }
            _ => false,
        }
    }

    pub fn revoke(&mut self, revoked_by: UserId, reason: String, block_index: u64) {
        let revoked_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        self.revocation_status = RevocationStatus::Revoked {
            revoked_at,
            revoked_by,
            reason,
            block_index,
        };
    }

    pub fn suspend(&mut self, suspended_by: UserId, reason: String, until: Option<u64>, block_index: u64) {
        let suspended_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        self.revocation_status = RevocationStatus::Suspended {
            suspended_at,
            suspended_by,
            reason,
            until,
            block_index,
        };
    }

    pub fn reactivate(&mut self) {
        self.revocation_status = RevocationStatus::Active;
    }

    pub fn record_login_success(&mut self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        self.metadata.last_login = Some(now);
        self.metadata.login_count += 1;
        self.metadata.failed_login_attempts = 0; // Reset failed attempts on successful login
    }

    pub fn record_login_failure(&mut self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        self.metadata.failed_login_attempts += 1;
        self.metadata.last_failed_login = Some(now);
    }

    pub fn should_lock_account(&self, max_failed_attempts: u64) -> bool {
        self.metadata.failed_login_attempts >= max_failed_attempts
    }

    pub fn sign_operation(
        operation: &crate::transaction::Operation,
        user_id: &str,
        nonce: u64,
        timestamp: u64,
        private_key: &PrivateKey,
    ) -> Result<Signature, AuthError> {
        let operation_hash = CryptoUtils::create_operation_hash(operation, user_id, nonce, timestamp);
        CryptoUtils::sign_data(&operation_hash, private_key)
    }

    pub fn verify_operation_signature(
        operation: &crate::transaction::Operation,
        user_id: &str,
        nonce: u64,
        timestamp: u64,
        signature: &[u8],
        public_key: &PublicKey,
    ) -> Result<bool, AuthError> {
        let operation_hash = CryptoUtils::create_operation_hash(operation, user_id, nonce, timestamp);
        CryptoUtils::verify_signature(&operation_hash, signature, public_key)
    }

    pub fn compute_identity_hash(&self) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(self.user_id.as_bytes());
        data.extend_from_slice(&self.public_key);
        data.extend_from_slice(&self.creation_block.to_le_bytes());
        data.extend_from_slice(&self.creation_timestamp.to_le_bytes());
        data.extend_from_slice(&self.permissions_merkle_root);

        CryptoUtils::hash_data(&data)
    }

    pub fn update_permissions_merkle_root(&mut self, permissions: &PermissionSet) {
        let mut permission_data = Vec::new();
        let mut permissions_list = permissions.get_permissions();
        permissions_list.sort_by_key(|p| format!("{:?}", p)); // Sort for consistent hashing

        for permission in permissions_list {
            permission_data.extend_from_slice(format!("{:?}", permission).as_bytes());
        }

        self.permissions_merkle_root = CryptoUtils::hash_data(&permission_data);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityChain {
    pub identities: Vec<CryptoIdentity>,
    pub identity_hash_chain: Vec<Vec<u8>>,
}

impl IdentityChain {
    pub fn new() -> Self {
        Self {
            identities: Vec::new(),
            identity_hash_chain: Vec::new(),
        }
    }

    pub fn add_identity(&mut self, identity: CryptoIdentity) -> Result<(), AuthError> {
        let identity_hash = identity.compute_identity_hash();
        
        // Verify chain integrity if not empty
        if !self.identity_hash_chain.is_empty() {
            let previous_hash = self.identity_hash_chain.last().unwrap();
            let mut combined_data = Vec::new();
            combined_data.extend_from_slice(previous_hash);
            combined_data.extend_from_slice(&identity_hash);
            
            let chain_hash = CryptoUtils::hash_data(&combined_data);
            self.identity_hash_chain.push(chain_hash);
        } else {
            self.identity_hash_chain.push(identity_hash.clone());
        }

        self.identities.push(identity);
        Ok(())
    }

    pub fn find_identity(&self, user_id: &str) -> Option<&CryptoIdentity> {
        self.identities.iter().find(|id| id.user_id == user_id)
    }

    pub fn find_identity_mut(&mut self, user_id: &str) -> Option<&mut CryptoIdentity> {
        self.identities.iter_mut().find(|id| id.user_id == user_id)
    }

    pub fn verify_chain_integrity(&self) -> bool {
        if self.identities.len() != self.identity_hash_chain.len() {
            return false;
        }

        for (i, identity) in self.identities.iter().enumerate() {
            let computed_hash = identity.compute_identity_hash();
            
            if i == 0 {
                if self.identity_hash_chain[i] != computed_hash {
                    return false;
                }
            } else {
                let previous_hash = &self.identity_hash_chain[i - 1];
                let mut combined_data = Vec::new();
                combined_data.extend_from_slice(previous_hash);
                combined_data.extend_from_slice(&computed_hash);
                
                let expected_chain_hash = CryptoUtils::hash_data(&combined_data);
                if self.identity_hash_chain[i] != expected_chain_hash {
                    return false;
                }
            }
        }

        true
    }

    pub fn get_active_identities(&self) -> Vec<&CryptoIdentity> {
        self.identities.iter().filter(|id| id.is_active()).collect()
    }
}

impl Default for IdentityChain {
    fn default() -> Self {
        Self::new()
    }
}