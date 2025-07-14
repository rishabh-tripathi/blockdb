use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub mod crypto;
pub mod permissions;
pub mod identity;
pub mod auth_manager;
pub mod distributed_auth;
pub mod simple_tests;

pub use crypto::*;
pub use permissions::*;
pub use identity::*;
pub use auth_manager::*;
pub use distributed_auth::*;

pub type UserId = String;
pub type SessionId = String;
pub type Signature = Vec<u8>;
pub type PublicKey = Vec<u8>;
pub type PrivateKey = Vec<u8>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AuthError {
    InvalidCredentials,
    UserNotFound(UserId),
    UserAlreadyExists(UserId),
    InsufficientPermissions {
        required: Permission,
        user: UserId,
    },
    InvalidSignature,
    TokenExpired,
    SessionNotFound(SessionId),
    CryptographicError(String),
    PermissionDenied {
        operation: String,
        resource: String,
        user: UserId,
    },
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthError::InvalidCredentials => write!(f, "Invalid credentials"),
            AuthError::UserNotFound(user) => write!(f, "User not found: {}", user),
            AuthError::UserAlreadyExists(user) => write!(f, "User already exists: {}", user),
            AuthError::InsufficientPermissions { required, user } => {
                write!(f, "User {} lacks required permission: {:?}", user, required)
            }
            AuthError::InvalidSignature => write!(f, "Invalid cryptographic signature"),
            AuthError::TokenExpired => write!(f, "Authentication token expired"),
            AuthError::SessionNotFound(session) => write!(f, "Session not found: {}", session),
            AuthError::CryptographicError(msg) => write!(f, "Cryptographic error: {}", msg),
            AuthError::PermissionDenied { operation, resource, user } => {
                write!(f, "Permission denied for user {} on operation {} for resource {}", user, operation, resource)
            }
        }
    }
}

impl std::error::Error for AuthError {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthenticatedOperation {
    pub operation: crate::transaction::Operation,
    pub user_id: UserId,
    pub signature: Signature,
    pub nonce: u64,
    pub timestamp: u64,
}

impl AuthenticatedOperation {
    pub fn new(
        operation: crate::transaction::Operation,
        user_id: UserId,
        private_key: &PrivateKey,
        nonce: u64,
    ) -> Result<Self, AuthError> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| AuthError::CryptographicError(e.to_string()))?
            .as_secs();

        let signature = CryptoIdentity::sign_operation(
            &operation,
            &user_id,
            nonce,
            timestamp,
            private_key,
        )?;

        Ok(Self {
            operation,
            user_id,
            signature,
            nonce,
            timestamp,
        })
    }

    pub fn verify(&self, public_key: &PublicKey) -> Result<bool, AuthError> {
        CryptoIdentity::verify_operation_signature(
            &self.operation,
            &self.user_id,
            self.nonce,
            self.timestamp,
            &self.signature,
            public_key,
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthContext {
    pub user_id: UserId,
    pub session_id: SessionId,
    pub permissions: Vec<Permission>,
    pub created_at: u64,
    pub expires_at: u64,
}

impl AuthContext {
    pub fn new(user_id: UserId, permissions: Vec<Permission>, session_duration_secs: u64) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self {
            user_id,
            session_id: Uuid::new_v4().to_string(),
            permissions,
            created_at: now,
            expires_at: now + session_duration_secs,
        }
    }

    pub fn is_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        now >= self.expires_at
    }

    pub fn has_permission(&self, required: &Permission) -> bool {
        self.permissions.contains(required)
    }
}