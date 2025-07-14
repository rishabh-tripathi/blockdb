use base64::Engine;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};
use crate::{BlockDBHandle, BlockDBError, AuthManager, AuthContext, Permission};

// pub mod http;
// pub mod websocket;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiConfig {
    pub host: String,
    pub port: u16,
    pub max_connections: usize,
    pub request_timeout: u64,
    pub enable_cors: bool,
    pub enable_compression: bool,
    pub auth_enabled: bool,
    pub require_auth_for_reads: bool,
    pub session_duration_hours: u64,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 8080,
            max_connections: 1000,
            request_timeout: 30,
            enable_cors: true,
            enable_compression: true,
            auth_enabled: true,
            require_auth_for_reads: false,
            session_duration_hours: 24,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteRequest {
    pub key: String,
    pub value: String,
    pub encoding: Option<String>,
    pub auth_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadRequest {
    pub key: String,
    pub encoding: Option<String>,
    pub auth_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteResponse {
    pub success: bool,
    pub message: String,
    pub timestamp: u64,
    pub sequence_number: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadResponse {
    pub success: bool,
    pub data: Option<String>,
    pub message: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchWriteRequest {
    pub operations: Vec<WriteRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchWriteResponse {
    pub success: bool,
    pub results: Vec<WriteResponse>,
    pub total_processed: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub uptime: u64,
    pub total_records: u64,
    pub blockchain_height: u64,
    pub integrity_verified: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatsResponse {
    pub total_writes: u64,
    pub total_reads: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub blockchain_blocks: u64,
    pub storage_size: u64,
}

// Authentication Request/Response Types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginResponse {
    pub success: bool,
    pub auth_token: Option<String>,
    pub message: String,
    pub expires_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
    pub password: String,
    pub permissions: Vec<String>,
    pub auth_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateUserResponse {
    pub success: bool,
    pub message: String,
    pub user_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRequest {
    pub user_id: String,
    pub permissions: Vec<String>,
    pub grant: bool, // true to grant, false to revoke
    pub auth_token: String,
}

pub struct BlockDBServer {
    db: BlockDBHandle,
    config: ApiConfig,
    stats: Arc<RwLock<Stats>>,
    auth_manager: Option<Arc<RwLock<AuthManager>>>,
}

#[derive(Debug, Default)]
pub struct Stats {
    pub total_writes: u64,
    pub total_reads: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub start_time: Option<std::time::SystemTime>,
}

impl BlockDBServer {
    pub fn new(db: BlockDBHandle, config: ApiConfig) -> Self {
        let mut stats = Stats::default();
        stats.start_time = Some(std::time::SystemTime::now());
        
        BlockDBServer {
            db,
            config,
            stats: Arc::new(RwLock::new(stats)),
            auth_manager: None,
        }
    }

    pub fn with_auth(db: BlockDBHandle, config: ApiConfig, auth_manager: AuthManager) -> Self {
        let mut stats = Stats::default();
        stats.start_time = Some(std::time::SystemTime::now());
        
        BlockDBServer {
            db,
            config,
            stats: Arc::new(RwLock::new(stats)),
            auth_manager: Some(Arc::new(RwLock::new(auth_manager))),
        }
    }

    pub async fn start(&self) -> Result<(), BlockDBError> {
        let addr = format!("{}:{}", self.config.host, self.config.port);
        println!("Starting BlockDB server on {}", addr);
        
        let app = self.create_routes().await;
        
        let listener = tokio::net::TcpListener::bind(&addr).await
            .map_err(|e| BlockDBError::ApiError(format!("Failed to bind to {}: {}", addr, e)))?;
        
        println!("BlockDB server listening on {}", addr);
        
        loop {
            let (stream, _) = listener.accept().await
                .map_err(|e| BlockDBError::ApiError(format!("Failed to accept connection: {}", e)))?;
            
            let _app_clone = app.clone();
            tokio::spawn(async move {
                // HTTP server implementation would go here
                // For now just handle the connection
                drop(stream);
            });
        }
    }

    async fn create_routes(&self) -> Router {
        let db = self.db.clone();
        let stats = self.stats.clone();
        let auth_manager = self.auth_manager.clone();
        
        Router::new()
            .with_state(AppState { db, stats, auth_manager })
    }

    // Authentication methods
    async fn authenticate_request(&self, token: Option<String>, required_permission: Permission) -> Result<AuthContext, BlockDBError> {
        if !self.config.auth_enabled {
            // If auth is disabled, return a dummy context with all permissions
            return Ok(AuthContext::new(
                "anonymous".to_string(),
                vec![Permission::Admin], // Grant all permissions when auth is disabled
                self.config.session_duration_hours * 3600,
            ));
        }

        let auth_manager = self.auth_manager.as_ref()
            .ok_or_else(|| BlockDBError::AuthError(crate::auth::AuthError::InvalidCredentials))?;

        let token = token.ok_or_else(|| BlockDBError::AuthError(crate::auth::AuthError::InvalidCredentials))?;
        
        let auth_manager = auth_manager.read().await;
        let context = auth_manager.validate_session(&token)?;
        
        if !context.has_permission(&required_permission) {
            return Err(BlockDBError::AuthError(crate::auth::AuthError::InsufficientPermissions {
                required: required_permission,
                user: context.user_id.clone(),
            }));
        }

        Ok(context)
    }

    pub async fn login(&self, request: LoginRequest) -> Result<LoginResponse, BlockDBError> {
        let auth_manager = self.auth_manager.as_ref()
            .ok_or_else(|| BlockDBError::AuthError(crate::auth::AuthError::InvalidCredentials))?;

        let auth_manager = auth_manager.read().await;
        
        match auth_manager.authenticate_user(&request.username, &request.password) {
            Ok(context) => {
                Ok(LoginResponse {
                    success: true,
                    auth_token: Some(context.session_id.clone()),
                    message: "Login successful".to_string(),
                    expires_at: Some(context.expires_at),
                })
            }
            Err(e) => {
                Ok(LoginResponse {
                    success: false,
                    auth_token: None,
                    message: format!("Login failed: {}", e),
                    expires_at: None,
                })
            }
        }
    }

    pub async fn create_user(&self, request: CreateUserRequest) -> Result<CreateUserResponse, BlockDBError> {
        let _context = self.authenticate_request(Some(request.auth_token), Permission::CreateUser).await?;
        
        let auth_manager = self.auth_manager.as_ref()
            .ok_or_else(|| BlockDBError::AuthError(crate::auth::AuthError::InvalidCredentials))?;

        let permissions: Result<Vec<Permission>, _> = request.permissions.iter()
            .map(|p| p.parse())
            .collect();
        
        let permissions = permissions.map_err(|_| BlockDBError::InvalidData("Invalid permission format".to_string()))?;

        let mut auth_manager = auth_manager.write().await;
        
        match auth_manager.create_user(&request.username, &request.password, permissions) {
            Ok(user_id) => {
                Ok(CreateUserResponse {
                    success: true,
                    message: "User created successfully".to_string(),
                    user_id: Some(user_id),
                })
            }
            Err(e) => {
                Ok(CreateUserResponse {
                    success: false,
                    message: format!("Failed to create user: {}", e),
                    user_id: None,
                })
            }
        }
    }

    pub async fn write(&self, request: WriteRequest) -> Result<WriteResponse, BlockDBError> {
        // Authenticate if auth is enabled
        if self.config.auth_enabled {
            self.authenticate_request(request.auth_token, Permission::Write).await?;
        }
        let key = if request.encoding.as_deref() == Some("base64") {
            base64::engine::general_purpose::STANDARD.decode(&request.key).map_err(|e| BlockDBError::InvalidData(format!("Invalid base64 key: {}", e)))?
        } else {
            request.key.into_bytes()
        };

        let value = if request.encoding.as_deref() == Some("base64") {
            base64::engine::general_purpose::STANDARD.decode(&request.value).map_err(|e| BlockDBError::InvalidData(format!("Invalid base64 value: {}", e)))?
        } else {
            request.value.into_bytes()
        };

        self.db.put(&key, &value).await?;
        
        {
            let mut stats = self.stats.write().await;
            stats.total_writes += 1;
        }

        Ok(WriteResponse {
            success: true,
            message: "Data written successfully".to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            sequence_number: None,
        })
    }

    pub async fn read(&self, request: ReadRequest) -> Result<ReadResponse, BlockDBError> {
        // Authenticate if auth is enabled and required for reads
        if self.config.auth_enabled && self.config.require_auth_for_reads {
            self.authenticate_request(request.auth_token, Permission::Read).await?;
        }
        let key = if request.encoding.as_deref() == Some("base64") {
            base64::engine::general_purpose::STANDARD.decode(&request.key).map_err(|e| BlockDBError::InvalidData(format!("Invalid base64 key: {}", e)))?
        } else {
            request.key.into_bytes()
        };

        let result = self.db.get(&key).await?;
        
        {
            let mut stats = self.stats.write().await;
            stats.total_reads += 1;
            if result.is_some() {
                stats.cache_hits += 1;
            } else {
                stats.cache_misses += 1;
            }
        }

        let data = if let Some(value) = result {
            if request.encoding.as_deref() == Some("base64") {
                Some(base64::engine::general_purpose::STANDARD.encode(value))
            } else {
                Some(String::from_utf8_lossy(&value).to_string())
            }
        } else {
            None
        };

        Ok(ReadResponse {
            success: true,
            data: data.clone(),
            message: if data.is_some() { "Data found" } else { "Key not found" }.to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        })
    }

    pub async fn batch_write(&self, request: BatchWriteRequest) -> Result<BatchWriteResponse, BlockDBError> {
        let mut results = Vec::new();
        let mut processed = 0;

        for op in request.operations {
            match self.write(op).await {
                Ok(response) => {
                    results.push(response);
                    processed += 1;
                }
                Err(e) => {
                    results.push(WriteResponse {
                        success: false,
                        message: e.to_string(),
                        timestamp: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs(),
                        sequence_number: None,
                    });
                }
            }
        }

        Ok(BatchWriteResponse {
            success: processed > 0,
            results,
            total_processed: processed,
        })
    }

    pub async fn health(&self) -> Result<HealthResponse, BlockDBError> {
        let integrity_verified = self.db.verify_integrity().await?;
        
        let stats = self.stats.read().await;
        let uptime = if let Some(start_time) = stats.start_time {
            std::time::SystemTime::now()
                .duration_since(start_time)
                .unwrap_or_default()
                .as_secs()
        } else {
            0
        };

        Ok(HealthResponse {
            status: "healthy".to_string(),
            uptime,
            total_records: stats.total_writes,
            blockchain_height: 0, // Would need to implement blockchain height tracking
            integrity_verified,
        })
    }

    pub async fn stats(&self) -> Result<StatsResponse, BlockDBError> {
        let stats = self.stats.read().await;
        
        Ok(StatsResponse {
            total_writes: stats.total_writes,
            total_reads: stats.total_reads,
            cache_hits: stats.cache_hits,
            cache_misses: stats.cache_misses,
            blockchain_blocks: 0, // Would need to implement
            storage_size: 0, // Would need to implement
        })
    }
}

#[derive(Clone)]
struct AppState {
    db: BlockDBHandle,
    stats: Arc<RwLock<Stats>>,
    auth_manager: Option<Arc<RwLock<AuthManager>>>,
}

struct Router {
    state: AppState,
}

impl Router {
    fn new() -> RouterBuilder {
        RouterBuilder
    }

    fn with_state(state: AppState) -> Self {
        Router { state }
    }
}

struct RouterBuilder;

impl RouterBuilder {
    fn with_state(self, state: AppState) -> Router {
        Router { state }
    }
}

impl Clone for Router {
    fn clone(&self) -> Self {
        Router {
            state: self.state.clone(),
        }
    }
}