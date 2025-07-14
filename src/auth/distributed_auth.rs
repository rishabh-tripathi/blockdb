use crate::auth::{AuthConfig, AuthContext, AuthManager, Permission, PermissionSet, AuthError};
use crate::distributed::{DistributedBlockDB, DistributedBlockDBConfig};
use crate::transaction::Operation;
use crate::error::BlockDBError;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

// Extended configuration that includes authentication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthenticatedDistributedBlockDBConfig {
    pub node_id: String,
    pub data_dir: String,
    pub cluster: ClusterConfigAuth,
    pub memtable_size_limit: usize,
    pub wal_sync_interval_ms: u64,
    pub compaction_threshold: usize,
    pub blockchain_batch_size: usize,
    pub auth_config: Option<AuthConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterConfigAuth {
    pub nodes: std::collections::HashMap<String, crate::consensus::NodeAddress>,
    pub heartbeat_interval_ms: u64,
    pub election_timeout_ms: u64,
    pub enable_transactions: bool,
    pub transaction_timeout_secs: u64,
}

impl Default for ClusterConfigAuth {
    fn default() -> Self {
        let mut nodes = std::collections::HashMap::new();
        nodes.insert(
            "node1".to_string(),
            crate::consensus::NodeAddress {
                host: "127.0.0.1".to_string(),
                port: 8080,
            },
        );
        
        Self {
            nodes,
            heartbeat_interval_ms: 150,
            election_timeout_ms: 300,
            enable_transactions: true,
            transaction_timeout_secs: 30,
        }
    }
}

impl Default for AuthenticatedDistributedBlockDBConfig {
    fn default() -> Self {
        Self {
            node_id: "node1".to_string(),
            data_dir: "./blockdb_data".to_string(),
            cluster: ClusterConfigAuth::default(),
            memtable_size_limit: 64 * 1024 * 1024, // 64MB
            wal_sync_interval_ms: 1000,
            compaction_threshold: 4,
            blockchain_batch_size: 1000,
            auth_config: Some(AuthConfig::default()),
        }
    }
}

// Wrapper around DistributedBlockDB that adds authentication
pub struct AuthenticatedDistributedBlockDB {
    inner: DistributedBlockDB,
    auth_manager: Option<Arc<AuthManager>>,
}

impl AuthenticatedDistributedBlockDB {
    pub async fn new(config: AuthenticatedDistributedBlockDBConfig) -> Result<Self, BlockDBError> {
        // Convert to the standard DistributedBlockDBConfig
        let node_id = crate::consensus::NodeId::from_string(config.node_id.clone());
        let address = config.cluster.nodes.get(&config.node_id)
            .cloned()
            .unwrap_or(crate::consensus::NodeAddress {
                host: "127.0.0.1".to_string(),
                port: 8080,
            });

        let mut peers = std::collections::HashMap::new();
        for (id, addr) in config.cluster.nodes.iter() {
            if id != &config.node_id {
                peers.insert(crate::consensus::NodeId::from_string(id.clone()), addr.clone());
            }
        }

        let distributed_config = DistributedBlockDBConfig {
            storage_config: crate::storage::BlockDBConfig {
                data_dir: config.data_dir.clone(),
                memtable_size_limit: config.memtable_size_limit,
                wal_sync_interval_ms: config.wal_sync_interval_ms,
                compaction_threshold: config.compaction_threshold,
                blockchain_batch_size: config.blockchain_batch_size,
            },
            cluster_config: crate::consensus::ClusterConfig {
                node_id,
                address,
                peers,
                heartbeat_interval: std::time::Duration::from_millis(config.cluster.heartbeat_interval_ms),
                election_timeout: std::time::Duration::from_millis(config.cluster.election_timeout_ms),
            },
            enable_transactions: config.cluster.enable_transactions,
            transaction_timeout: std::time::Duration::from_secs(config.cluster.transaction_timeout_secs),
            consensus_timeout: std::time::Duration::from_secs(5),
        };

        // Create the underlying distributed database
        let inner = DistributedBlockDB::new(distributed_config).await?;

        // Initialize auth manager if configured
        let auth_manager = if let Some(auth_config) = config.auth_config {
            Some(Arc::new(AuthManager::new(auth_config)))
        } else {
            None
        };

        Ok(Self {
            inner,
            auth_manager,
        })
    }

    pub async fn start(&self) -> Result<(), BlockDBError> {
        self.inner.start().await
    }

    pub async fn stop(&self) -> Result<(), BlockDBError> {
        self.inner.stop().await
    }

    // Authentication methods
    pub async fn authenticate(&self, user_id: &str, password: &str) -> Result<AuthContext, BlockDBError> {
        if let Some(auth_manager) = &self.auth_manager {
            auth_manager.authenticate(user_id, password).map_err(BlockDBError::AuthError)
        } else {
            Err(BlockDBError::AuthError(AuthError::CryptographicError(
                "Authentication is not enabled".to_string(),
            )))
        }
    }

    pub async fn authenticate_with_token(&self, session_id: &str) -> Result<AuthContext, BlockDBError> {
        if let Some(auth_manager) = &self.auth_manager {
            auth_manager.authenticate_with_token(session_id).map_err(BlockDBError::AuthError)
        } else {
            Err(BlockDBError::AuthError(AuthError::CryptographicError(
                "Authentication is not enabled".to_string(),
            )))
        }
    }

    pub async fn logout(&self, session_id: &str) -> Result<(), BlockDBError> {
        if let Some(auth_manager) = &self.auth_manager {
            auth_manager.logout(session_id).map_err(BlockDBError::AuthError)
        } else {
            Ok(())
        }
    }

    pub async fn create_user(
        &self,
        user_id: &str,
        password: &str,
        permissions: PermissionSet,
        created_by_context: &AuthContext,
    ) -> Result<crate::auth::crypto::KeyPair, BlockDBError> {
        if let Some(auth_manager) = &self.auth_manager {
            // Check if the creator has permission to create users
            auth_manager.check_permission(&created_by_context.user_id, &Permission::CreateUser)
                .map_err(BlockDBError::AuthError)?;

            let block_index = self.get_current_block_index().await?;
            auth_manager.create_user(
                user_id.to_string(),
                password,
                permissions,
                Some(created_by_context.user_id.clone()),
                block_index,
            ).map_err(BlockDBError::AuthError)
        } else {
            Err(BlockDBError::AuthError(AuthError::CryptographicError(
                "Authentication is not enabled".to_string(),
            )))
        }
    }

    pub async fn create_user_with_keypair(
        &self,
        user_id: &str,
        password: &str,
        permissions: PermissionSet,
        created_by_context: &AuthContext,
    ) -> Result<crate::auth::crypto::KeyPair, BlockDBError> {
        self.create_user(user_id, password, permissions, created_by_context).await
    }

    pub async fn grant_permission(
        &self,
        target_user: &str,
        permission: Permission,
        granted_by_context: &AuthContext,
    ) -> Result<(), BlockDBError> {
        if let Some(auth_manager) = &self.auth_manager {
            let block_index = self.get_current_block_index().await?;
            auth_manager.grant_permission(
                target_user,
                permission,
                &granted_by_context.user_id,
                block_index,
            ).map_err(BlockDBError::AuthError)
        } else {
            Err(BlockDBError::AuthError(AuthError::CryptographicError(
                "Authentication is not enabled".to_string(),
            )))
        }
    }

    pub async fn revoke_permission(
        &self,
        target_user: &str,
        permission: &Permission,
        revoked_by_context: &AuthContext,
    ) -> Result<(), BlockDBError> {
        if let Some(auth_manager) = &self.auth_manager {
            let block_index = self.get_current_block_index().await?;
            auth_manager.revoke_permission(
                target_user,
                permission,
                &revoked_by_context.user_id,
                block_index,
            ).map_err(BlockDBError::AuthError)
        } else {
            Err(BlockDBError::AuthError(AuthError::CryptographicError(
                "Authentication is not enabled".to_string(),
            )))
        }
    }

    // Authenticated database operations
    pub async fn authenticated_put(
        &self,
        key: &[u8],
        value: &[u8],
        auth_context: &AuthContext,
    ) -> Result<(), BlockDBError> {
        if let Some(auth_manager) = &self.auth_manager {
            // Check write permission
            auth_manager.check_permission(&auth_context.user_id, &Permission::Write)
                .map_err(BlockDBError::AuthError)?;

            // Check session validity
            if auth_context.is_expired() {
                return Err(BlockDBError::AuthError(AuthError::TokenExpired));
            }

            // Perform the operation
            self.inner.put(key, value).await
        } else {
            // If auth is disabled, allow operation
            self.inner.put(key, value).await
        }
    }

    pub async fn authenticated_get(
        &self,
        key: &[u8],
        auth_context: &AuthContext,
    ) -> Result<Option<Vec<u8>>, BlockDBError> {
        if let Some(auth_manager) = &self.auth_manager {
            // Check read permission
            auth_manager.check_permission(&auth_context.user_id, &Permission::Read)
                .map_err(BlockDBError::AuthError)?;

            // Check session validity
            if auth_context.is_expired() {
                return Err(BlockDBError::AuthError(AuthError::TokenExpired));
            }

            // Perform the operation
            self.inner.get(key).await
        } else {
            // If auth is disabled, allow operation
            self.inner.get(key).await
        }
    }

    pub async fn authenticated_delete(
        &self,
        key: &[u8],
        auth_context: &AuthContext,
    ) -> Result<(), BlockDBError> {
        if let Some(auth_manager) = &self.auth_manager {
            // Check delete permission
            auth_manager.check_permission(&auth_context.user_id, &Permission::Delete)
                .map_err(BlockDBError::AuthError)?;

            // Check session validity
            if auth_context.is_expired() {
                return Err(BlockDBError::AuthError(AuthError::TokenExpired));
            }

            // For now, return an error since we're append-only
            // In the future, this could mark as deleted in the blockchain
            Err(BlockDBError::ApiError(
                "Delete operations not supported in append-only database".to_string(),
            ))
        } else {
            Err(BlockDBError::ApiError(
                "Delete operations not supported in append-only database".to_string(),
            ))
        }
    }

    // Transaction support with authentication
    pub async fn execute_authenticated_transaction<F, Fut, R>(
        &self,
        auth_context: &AuthContext,
        f: F,
    ) -> Result<R, BlockDBError>
    where
        F: FnOnce(AuthenticatedTransactionContext) -> Fut,
        Fut: std::future::Future<Output = Result<R, BlockDBError>>,
    {
        if let Some(auth_manager) = &self.auth_manager {
            // Check transaction permissions
            auth_manager.check_permission(&auth_context.user_id, &Permission::BeginTransaction)
                .map_err(BlockDBError::AuthError)?;

            // Check session validity
            if auth_context.is_expired() {
                return Err(BlockDBError::AuthError(AuthError::TokenExpired));
            }

            // Create authenticated transaction context
            let tx_context = AuthenticatedTransactionContext {
                inner: &self.inner,
                auth_context: auth_context.clone(),
                auth_manager: auth_manager.clone(),
            };

            // Execute the transaction
            f(tx_context).await
        } else {
            Err(BlockDBError::AuthError(AuthError::CryptographicError(
                "Authentication is not enabled".to_string(),
            )))
        }
    }

    // Utility methods
    pub async fn get_next_nonce(&self, user_id: &str) -> Result<u64, BlockDBError> {
        if let Some(auth_manager) = &self.auth_manager {
            Ok(auth_manager.get_next_nonce(user_id))
        } else {
            Ok(1) // Default nonce when auth is disabled
        }
    }

    pub async fn verify_operation_signature(
        &self,
        operation: &Operation,
        user_id: &str,
        nonce: u64,
        timestamp: u64,
        signature: &[u8],
    ) -> Result<bool, BlockDBError> {
        if let Some(auth_manager) = &self.auth_manager {
            auth_manager.verify_operation_signature(operation, user_id, nonce, timestamp, signature)
                .map_err(BlockDBError::AuthError)
        } else {
            Ok(true) // Always valid when auth is disabled
        }
    }

    pub async fn get_auth_audit_trail(&self) -> Result<Vec<crate::auth::permissions::PermissionOperation>, BlockDBError> {
        if let Some(auth_manager) = &self.auth_manager {
            Ok(auth_manager.get_audit_trail())
        } else {
            Ok(Vec::new())
        }
    }

    pub async fn verify_authenticated_integrity(&self) -> Result<bool, BlockDBError> {
        // Verify both blockchain integrity and identity chain integrity
        let blockchain_valid = self.inner.verify_integrity().await?;
        
        let identity_chain_valid = if let Some(auth_manager) = &self.auth_manager {
            auth_manager.verify_identity_chain_integrity()
        } else {
            true
        };

        Ok(blockchain_valid && identity_chain_valid)
    }

    pub async fn verify_identity_chain_integrity(&self) -> Result<bool, BlockDBError> {
        if let Some(auth_manager) = &self.auth_manager {
            Ok(auth_manager.verify_identity_chain_integrity())
        } else {
            Ok(true)
        }
    }

    pub async fn get_user_operations(&self, _user_id: &str) -> Result<Vec<Operation>, BlockDBError> {
        // This would require extending the blockchain to track user operations
        // For now, return empty list
        Ok(Vec::new())
    }

    pub async fn cleanup_expired_sessions(&self) {
        if let Some(auth_manager) = &self.auth_manager {
            auth_manager.cleanup_expired_sessions();
        }
    }

    async fn get_current_block_index(&self) -> Result<u64, BlockDBError> {
        // This would need to be implemented by extending the blockchain to track block index
        // For now, use timestamp as a proxy
        Ok(SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs())
    }

    // Delegate non-authenticated methods to inner
    pub async fn put(&self, key: &[u8], value: &[u8]) -> Result<(), BlockDBError> {
        self.inner.put(key, value).await
    }

    pub async fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, BlockDBError> {
        self.inner.get(key).await
    }

    pub async fn verify_integrity(&self) -> Result<bool, BlockDBError> {
        self.inner.verify_integrity().await
    }
}

// Authenticated transaction context
pub struct AuthenticatedTransactionContext<'a> {
    inner: &'a DistributedBlockDB,
    auth_context: AuthContext,
    auth_manager: Arc<AuthManager>,
}

impl<'a> AuthenticatedTransactionContext<'a> {
    pub async fn put(&self, key: &[u8], value: &[u8]) -> Result<(), BlockDBError> {
        // Check write permission
        self.auth_manager.check_permission(&self.auth_context.user_id, &Permission::Write)
            .map_err(BlockDBError::AuthError)?;

        // Perform the operation within transaction
        self.inner.put(key, value).await
    }

    pub async fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, BlockDBError> {
        // Check read permission
        self.auth_manager.check_permission(&self.auth_context.user_id, &Permission::Read)
            .map_err(BlockDBError::AuthError)?;

        // Perform the operation within transaction
        self.inner.get(key).await
    }

    pub fn get_user_id(&self) -> &str {
        &self.auth_context.user_id
    }

    pub fn get_session_id(&self) -> &str {
        &self.auth_context.session_id
    }
}