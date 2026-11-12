//! HTTP API for BlockDB.
//!
//! A real hyper-based server exposing key/value, authentication, consent, health,
//! stats and Prometheus metrics endpoints. Requests are size-limited, optionally
//! authenticated, and the server supports graceful shutdown.

use base64::Engine;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Method, Request, Response, Server, StatusCode};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{error, info};

use crate::consent::{ConsentStatus, ConsentStore};
use crate::{AuthManager, BlockDBError, BlockDBHandle, Permission};

/// Maximum accepted request body size (16 MiB) to bound memory per request.
const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;

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

// ---------------------------------------------------------------------------
// Request / response DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteRequest {
    pub key: String,
    pub value: String,
    pub encoding: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteResponse {
    pub success: bool,
    pub message: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadResponse {
    pub success: bool,
    pub data: Option<String>,
    pub message: String,
}

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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsentRequest {
    pub subject_id: String,
    pub purpose: String,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub uptime_secs: u64,
    pub integrity_verified: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub success: bool,
    pub error: String,
}

// ---------------------------------------------------------------------------
// Server state
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct Metrics {
    pub writes_total: AtomicU64,
    pub reads_total: AtomicU64,
    pub read_hits: AtomicU64,
    pub read_misses: AtomicU64,
    pub consent_grants_total: AtomicU64,
    pub consent_withdrawals_total: AtomicU64,
    pub auth_failures_total: AtomicU64,
    pub requests_total: AtomicU64,
}

struct AppState {
    db: BlockDBHandle,
    consent: ConsentStore,
    config: ApiConfig,
    auth_manager: Option<Arc<RwLock<AuthManager>>>,
    metrics: Metrics,
    start_time: std::time::Instant,
    /// Bounds the number of requests served concurrently (config.max_connections);
    /// excess requests are shed with 503 rather than exhausting memory/CPU.
    conn_limiter: Arc<tokio::sync::Semaphore>,
}

pub struct BlockDBServer {
    state: Arc<AppState>,
}

impl BlockDBServer {
    pub fn new(db: BlockDBHandle, config: ApiConfig) -> Self {
        let consent = ConsentStore::new(db.clone());
        let conn_limiter = Arc::new(tokio::sync::Semaphore::new(config.max_connections.max(1)));
        BlockDBServer {
            state: Arc::new(AppState {
                db,
                consent,
                config,
                auth_manager: None,
                metrics: Metrics::default(),
                start_time: std::time::Instant::now(),
                conn_limiter,
            }),
        }
    }

    pub fn with_auth(db: BlockDBHandle, config: ApiConfig, auth_manager: AuthManager) -> Self {
        let consent = ConsentStore::new(db.clone());
        let conn_limiter = Arc::new(tokio::sync::Semaphore::new(config.max_connections.max(1)));
        BlockDBServer {
            state: Arc::new(AppState {
                db,
                consent,
                config,
                auth_manager: Some(Arc::new(RwLock::new(auth_manager))),
                metrics: Metrics::default(),
                start_time: std::time::Instant::now(),
                conn_limiter,
            }),
        }
    }

    /// Run the server until `shutdown` resolves (e.g. on SIGTERM/Ctrl-C).
    pub async fn serve_with_shutdown<F>(&self, shutdown: F) -> Result<(), BlockDBError>
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        let addr: SocketAddr = format!("{}:{}", self.state.config.host, self.state.config.port)
            .parse()
            .map_err(|e| BlockDBError::ApiError(format!("invalid bind address: {}", e)))?;

        let state = self.state.clone();
        let make_svc = make_service_fn(move |_conn| {
            let state = state.clone();
            async move {
                Ok::<_, Infallible>(service_fn(move |req| {
                    let state = state.clone();
                    async move { Ok::<_, Infallible>(serve_one(state, req).await) }
                }))
            }
        });

        let server = Server::bind(&addr).serve(make_svc);
        info!("BlockDB HTTP server listening on http://{}", addr);

        server
            .with_graceful_shutdown(shutdown)
            .await
            .map_err(|e| BlockDBError::ApiError(format!("server error: {}", e)))?;

        info!("BlockDB HTTP server shut down cleanly");
        Ok(())
    }

    /// Convenience: run until Ctrl-C / SIGTERM.
    pub async fn start(&self) -> Result<(), BlockDBError> {
        self.serve_with_shutdown(shutdown_signal()).await
    }
}

/// Resolves when the process receives SIGINT or (on Unix) SIGTERM.
pub async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{signal, SignalKind};
        if let Ok(mut s) = signal(SignalKind::terminate()) {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    info!("shutdown signal received");
}

// ---------------------------------------------------------------------------
// Routing
// ---------------------------------------------------------------------------

/// Enforce the connection cap and per-request timeout around `handle_request`.
async fn serve_one(state: Arc<AppState>, req: Request<Body>) -> Response<Body> {
    // Shed load when at the concurrency cap instead of queueing unboundedly.
    let _permit = match state.conn_limiter.clone().try_acquire_owned() {
        Ok(p) => p,
        Err(_) => {
            return json(
                StatusCode::SERVICE_UNAVAILABLE,
                &serde_json::json!({
                    "success": false,
                    "error": "server at capacity, retry later",
                }),
            );
        }
    };

    let timeout = Duration::from_secs(state.config.request_timeout.max(1));
    match tokio::time::timeout(timeout, handle_request(state, req)).await {
        Ok(resp) => resp,
        Err(_) => json(
            StatusCode::SERVICE_UNAVAILABLE,
            &serde_json::json!({
                "success": false,
                "error": "request timed out",
            }),
        ),
    }
}

async fn handle_request(state: Arc<AppState>, req: Request<Body>) -> Response<Body> {
    state.metrics.requests_total.fetch_add(1, Ordering::Relaxed);
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let query = req.uri().query().unwrap_or("").to_string();

    let result = route(&state, method.clone(), &path, &query, req).await;

    let mut response = match result {
        Ok(resp) => resp,
        Err(err) => error_response(&err),
    };

    if state.config.enable_cors {
        let headers = response.headers_mut();
        // from_static is infallible for these constant values (no runtime parse/unwrap).
        headers.insert(
            "Access-Control-Allow-Origin",
            hyper::header::HeaderValue::from_static("*"),
        );
        headers.insert(
            "Access-Control-Allow-Methods",
            hyper::header::HeaderValue::from_static("GET, POST, PUT, OPTIONS"),
        );
        headers.insert(
            "Access-Control-Allow-Headers",
            hyper::header::HeaderValue::from_static("Content-Type, Authorization"),
        );
    }
    response
}

async fn route(
    state: &Arc<AppState>,
    method: Method,
    path: &str,
    query: &str,
    req: Request<Body>,
) -> Result<Response<Body>, BlockDBError> {
    match (&method, path) {
        (&Method::OPTIONS, _) => Ok(empty(StatusCode::NO_CONTENT)),

        (&Method::GET, "/health") => handle_health(state).await,
        (&Method::GET, "/metrics") => Ok(handle_metrics(state)),
        (&Method::GET, "/stats") => Ok(handle_stats(state)),

        (&Method::POST, "/login") | (&Method::POST, "/auth/login") => {
            handle_login(state, req).await
        }
        (&Method::POST, "/users") | (&Method::POST, "/auth/users") => {
            handle_create_user(state, req).await
        }

        (&Method::POST, "/kv") | (&Method::PUT, "/kv") => handle_write(state, req).await,
        (&Method::GET, "/kv") => handle_read(state, req, query).await,

        (&Method::POST, "/consent/grant") => handle_consent(state, req, ConsentStatus::Granted).await,
        (&Method::POST, "/consent/withdraw") => {
            handle_consent(state, req, ConsentStatus::Withdrawn).await
        }
        (&Method::GET, "/consent/status") => handle_consent_status(state, query).await,
        (&Method::GET, "/consent/history") => handle_consent_history(state, query).await,
        (&Method::GET, "/consent/subject") => handle_consent_subject(state, query).await,

        _ => Ok(error_status(
            StatusCode::NOT_FOUND,
            &format!("no route for {} {}", method, path),
        )),
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn handle_health(state: &Arc<AppState>) -> Result<Response<Body>, BlockDBError> {
    let integrity_verified = state.db.verify_integrity().await.unwrap_or(false);
    let body = HealthResponse {
        status: if integrity_verified { "healthy" } else { "degraded" }.to_string(),
        uptime_secs: state.start_time.elapsed().as_secs(),
        integrity_verified,
    };
    let code = if integrity_verified {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    Ok(json(code, &body))
}

fn handle_stats(state: &Arc<AppState>) -> Response<Body> {
    let m = &state.metrics;
    let body = serde_json::json!({
        "writes_total": m.writes_total.load(Ordering::Relaxed),
        "reads_total": m.reads_total.load(Ordering::Relaxed),
        "read_hits": m.read_hits.load(Ordering::Relaxed),
        "read_misses": m.read_misses.load(Ordering::Relaxed),
        "consent_grants_total": m.consent_grants_total.load(Ordering::Relaxed),
        "consent_withdrawals_total": m.consent_withdrawals_total.load(Ordering::Relaxed),
        "auth_failures_total": m.auth_failures_total.load(Ordering::Relaxed),
        "requests_total": m.requests_total.load(Ordering::Relaxed),
        "uptime_secs": state.start_time.elapsed().as_secs(),
    });
    json(StatusCode::OK, &body)
}

fn handle_metrics(state: &Arc<AppState>) -> Response<Body> {
    let m = &state.metrics;
    let mut out = String::new();
    let mut gauge = |name: &str, help: &str, value: u64| {
        out.push_str(&format!("# HELP {} {}\n# TYPE {} counter\n{} {}\n", name, help, name, name, value));
    };
    gauge("blockdb_requests_total", "Total HTTP requests", m.requests_total.load(Ordering::Relaxed));
    gauge("blockdb_writes_total", "Total key writes", m.writes_total.load(Ordering::Relaxed));
    gauge("blockdb_reads_total", "Total key reads", m.reads_total.load(Ordering::Relaxed));
    gauge("blockdb_read_hits_total", "Reads that found a value", m.read_hits.load(Ordering::Relaxed));
    gauge("blockdb_read_misses_total", "Reads with no value", m.read_misses.load(Ordering::Relaxed));
    gauge("blockdb_consent_grants_total", "Consent grants recorded", m.consent_grants_total.load(Ordering::Relaxed));
    gauge("blockdb_consent_withdrawals_total", "Consent withdrawals recorded", m.consent_withdrawals_total.load(Ordering::Relaxed));
    gauge("blockdb_auth_failures_total", "Authentication failures", m.auth_failures_total.load(Ordering::Relaxed));
    out.push_str(&format!(
        "# HELP blockdb_uptime_seconds Uptime in seconds\n# TYPE blockdb_uptime_seconds gauge\nblockdb_uptime_seconds {}\n",
        state.start_time.elapsed().as_secs()
    ));

    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/plain; version=0.0.4")
        .body(Body::from(out))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

async fn handle_login(
    state: &Arc<AppState>,
    req: Request<Body>,
) -> Result<Response<Body>, BlockDBError> {
    let body = read_body(req).await?;
    let request: LoginRequest = parse_json(&body)?;

    let auth_manager = match &state.auth_manager {
        Some(am) => am,
        None => {
            return Ok(json(
                StatusCode::OK,
                &LoginResponse {
                    success: false,
                    auth_token: None,
                    message: "authentication is not enabled".to_string(),
                    expires_at: None,
                },
            ))
        }
    };

    let am = auth_manager.read().await;
    match am.authenticate(&request.username, &request.password) {
        Ok(ctx) => Ok(json(
            StatusCode::OK,
            &LoginResponse {
                success: true,
                auth_token: Some(ctx.session_id.clone()),
                message: "login successful".to_string(),
                expires_at: Some(ctx.expires_at),
            },
        )),
        Err(e) => {
            state.metrics.auth_failures_total.fetch_add(1, Ordering::Relaxed);
            Ok(json(
                StatusCode::UNAUTHORIZED,
                &LoginResponse {
                    success: false,
                    auth_token: None,
                    message: format!("login failed: {}", e),
                    expires_at: None,
                },
            ))
        }
    }
}

async fn handle_create_user(
    state: &Arc<AppState>,
    req: Request<Body>,
) -> Result<Response<Body>, BlockDBError> {
    let token = bearer_token(&req);
    let body = read_body(req).await?;
    let request: CreateUserRequest = parse_json(&body)?;

    authenticate(state, token.as_deref(), Permission::CreateUser).await?;

    let auth_manager = state
        .auth_manager
        .as_ref()
        .ok_or_else(|| BlockDBError::ApiError("authentication is not enabled".to_string()))?;

    let permissions: Result<Vec<Permission>, _> =
        request.permissions.iter().map(|p| p.parse::<Permission>()).collect();
    let permissions =
        permissions.map_err(|_| BlockDBError::InvalidData("invalid permission name".to_string()))?;
    let permission_set = crate::auth::PermissionSet::from_permissions(permissions);

    let am = auth_manager.write().await;
    am.create_user(request.username.clone(), &request.password, permission_set, Some("api".to_string()), 0)
        .map_err(BlockDBError::AuthError)?;

    Ok(json(
        StatusCode::CREATED,
        &serde_json::json!({ "success": true, "username": request.username }),
    ))
}

async fn handle_write(
    state: &Arc<AppState>,
    req: Request<Body>,
) -> Result<Response<Body>, BlockDBError> {
    let token = bearer_token(&req);
    let body = read_body(req).await?;
    let request: WriteRequest = parse_json(&body)?;

    if state.config.auth_enabled {
        authenticate(state, token.as_deref(), Permission::Write).await?;
    }

    let key = decode(&request.key, request.encoding.as_deref())?;
    let value = decode(&request.value, request.encoding.as_deref())?;

    state.db.put(&key, &value).await?;
    state.metrics.writes_total.fetch_add(1, Ordering::Relaxed);

    Ok(json(
        StatusCode::OK,
        &WriteResponse {
            success: true,
            message: "data written".to_string(),
            timestamp: now_secs(),
        },
    ))
}

async fn handle_read(
    state: &Arc<AppState>,
    req: Request<Body>,
    query: &str,
) -> Result<Response<Body>, BlockDBError> {
    let token = bearer_token(&req);
    if state.config.auth_enabled && state.config.require_auth_for_reads {
        authenticate(state, token.as_deref(), Permission::Read).await?;
    }

    let params = parse_query(query);
    let key_str = params
        .get("key")
        .ok_or_else(|| BlockDBError::InvalidData("missing 'key' query parameter".to_string()))?;
    let encoding = params.get("encoding").map(|s| s.as_str());
    let key = decode(key_str, encoding)?;

    state.metrics.reads_total.fetch_add(1, Ordering::Relaxed);
    let result = state.db.get(&key).await?;

    let data = match &result {
        Some(value) => {
            state.metrics.read_hits.fetch_add(1, Ordering::Relaxed);
            if encoding == Some("base64") {
                Some(base64::engine::general_purpose::STANDARD.encode(value))
            } else {
                Some(String::from_utf8_lossy(value).to_string())
            }
        }
        None => {
            state.metrics.read_misses.fetch_add(1, Ordering::Relaxed);
            None
        }
    };

    Ok(json(
        StatusCode::OK,
        &ReadResponse {
            success: true,
            message: if data.is_some() { "found" } else { "not found" }.to_string(),
            data,
        },
    ))
}

async fn handle_consent(
    state: &Arc<AppState>,
    req: Request<Body>,
    status: ConsentStatus,
) -> Result<Response<Body>, BlockDBError> {
    let token = bearer_token(&req);
    let body = read_body(req).await?;
    let request: ConsentRequest = parse_json(&body)?;

    if state.config.auth_enabled {
        authenticate(state, token.as_deref(), Permission::Write).await?;
    }

    let event = match status {
        ConsentStatus::Granted => {
            let e = state
                .consent
                .grant(&request.subject_id, &request.purpose, request.metadata)
                .await?;
            state.metrics.consent_grants_total.fetch_add(1, Ordering::Relaxed);
            e
        }
        ConsentStatus::Withdrawn => {
            let e = state
                .consent
                .withdraw(&request.subject_id, &request.purpose, request.metadata)
                .await?;
            state.metrics.consent_withdrawals_total.fetch_add(1, Ordering::Relaxed);
            e
        }
    };

    Ok(json(StatusCode::OK, &serde_json::json!({ "success": true, "event": event })))
}

async fn handle_consent_status(
    state: &Arc<AppState>,
    query: &str,
) -> Result<Response<Body>, BlockDBError> {
    let params = parse_query(query);
    let subject = require_param(&params, "subject")?;
    let purpose = require_param(&params, "purpose")?;

    let status = state.consent.status(subject, purpose).await?;
    Ok(json(StatusCode::OK, &serde_json::json!({ "success": true, "state": status })))
}

async fn handle_consent_history(
    state: &Arc<AppState>,
    query: &str,
) -> Result<Response<Body>, BlockDBError> {
    let params = parse_query(query);
    let subject = require_param(&params, "subject")?;
    let purpose = require_param(&params, "purpose")?;

    let history = state.consent.history(subject, purpose).await?;
    Ok(json(StatusCode::OK, &serde_json::json!({ "success": true, "events": history })))
}

async fn handle_consent_subject(
    state: &Arc<AppState>,
    query: &str,
) -> Result<Response<Body>, BlockDBError> {
    let params = parse_query(query);
    let subject = require_param(&params, "subject")?;

    let states = state.consent.list_for_subject(subject).await?;
    Ok(json(StatusCode::OK, &serde_json::json!({ "success": true, "consents": states })))
}

// ---------------------------------------------------------------------------
// Auth helper
// ---------------------------------------------------------------------------

async fn authenticate(
    state: &Arc<AppState>,
    token: Option<&str>,
    required: Permission,
) -> Result<(), BlockDBError> {
    if !state.config.auth_enabled {
        return Ok(());
    }

    let auth_manager = state
        .auth_manager
        .as_ref()
        .ok_or_else(|| BlockDBError::AuthError(crate::auth::AuthError::InvalidCredentials))?;

    let token = token.ok_or_else(|| {
        state.metrics.auth_failures_total.fetch_add(1, Ordering::Relaxed);
        BlockDBError::AuthError(crate::auth::AuthError::InvalidCredentials)
    })?;

    let am = auth_manager.read().await;
    let ctx = match am.authenticate_with_token(token) {
        Ok(ctx) => ctx,
        Err(e) => {
            state.metrics.auth_failures_total.fetch_add(1, Ordering::Relaxed);
            return Err(BlockDBError::AuthError(e));
        }
    };

    if !ctx.has_permission(&required) {
        state.metrics.auth_failures_total.fetch_add(1, Ordering::Relaxed);
        return Err(BlockDBError::AuthError(
            crate::auth::AuthError::InsufficientPermissions {
                required,
                user: ctx.user_id.clone(),
            },
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// HTTP helpers
// ---------------------------------------------------------------------------

fn bearer_token(req: &Request<Body>) -> Option<String> {
    req.headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(|s| s.to_string())
}

async fn read_body(req: Request<Body>) -> Result<Vec<u8>, BlockDBError> {
    let body = req.into_body();
    let bytes = hyper::body::to_bytes(body)
        .await
        .map_err(|e| BlockDBError::ApiError(format!("failed to read body: {}", e)))?;
    if bytes.len() > MAX_BODY_BYTES {
        return Err(BlockDBError::InvalidData(format!(
            "request body exceeds {} bytes",
            MAX_BODY_BYTES
        )));
    }
    Ok(bytes.to_vec())
}

fn parse_json<T: for<'de> Deserialize<'de>>(body: &[u8]) -> Result<T, BlockDBError> {
    serde_json::from_slice(body)
        .map_err(|e| BlockDBError::InvalidData(format!("invalid JSON body: {}", e)))
}

fn decode(value: &str, encoding: Option<&str>) -> Result<Vec<u8>, BlockDBError> {
    if encoding == Some("base64") {
        base64::engine::general_purpose::STANDARD
            .decode(value)
            .map_err(|e| BlockDBError::InvalidData(format!("invalid base64: {}", e)))
    } else {
        Ok(value.as_bytes().to_vec())
    }
}

fn parse_query(query: &str) -> BTreeMap<String, String> {
    let mut params = BTreeMap::new();
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let mut it = pair.splitn(2, '=');
        let key = it.next().unwrap_or("");
        let value = it.next().unwrap_or("");
        params.insert(url_decode(key), url_decode(value));
    }
    params
}

fn require_param<'a>(
    params: &'a BTreeMap<String, String>,
    name: &str,
) -> Result<&'a str, BlockDBError> {
    params
        .get(name)
        .map(|s| s.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| BlockDBError::InvalidData(format!("missing '{}' query parameter", name)))
}

/// Minimal percent-decoding sufficient for query parameters (handles %XX and '+').
fn url_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                if let Ok(byte) = u8::from_str_radix(&input[i + 1..i + 3], 16) {
                    out.push(byte);
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn json<T: Serialize>(status: StatusCode, body: &T) -> Response<Body> {
    let payload = serde_json::to_vec(body).unwrap_or_else(|_| b"{}".to_vec());
    Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .body(Body::from(payload))
        // Never panic while building a response to a live request.
        .unwrap_or_else(|_| Response::new(Body::from(payload_fallback())))
}

fn payload_fallback() -> &'static [u8] {
    b"{\"success\":false,\"error\":\"response build failed\"}"
}

fn empty(status: StatusCode) -> Response<Body> {
    Response::builder()
        .status(status)
        .body(Body::empty())
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

fn error_status(status: StatusCode, message: &str) -> Response<Body> {
    json(
        status,
        &ErrorResponse {
            success: false,
            error: message.to_string(),
        },
    )
}

fn error_response(err: &BlockDBError) -> Response<Body> {
    let status = match err {
        BlockDBError::AuthError(_) => StatusCode::UNAUTHORIZED,
        BlockDBError::DuplicateKey(_) => StatusCode::CONFLICT,
        BlockDBError::InvalidData(_) => StatusCode::BAD_REQUEST,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    if status == StatusCode::INTERNAL_SERVER_ERROR {
        error!("request failed: {}", err);
    }
    error_status(status, &err.to_string())
}
