//! End-to-end HTTP API tests. These start the real hyper server on a loopback port
//! and exercise it over TCP, verifying health, auth, key/value and consent endpoints.

use blockdb::api::{ApiConfig, BlockDBServer};
use blockdb::auth::{AuthConfig, PermissionSet};
use blockdb::{AuthManager, BlockDBConfig, BlockDBHandle, Permission};
use hyper::{Body, Client, Method, Request, StatusCode};
use std::sync::atomic::{AtomicU16, Ordering};
use tempfile::TempDir;

static NEXT_PORT: AtomicU16 = AtomicU16::new(19080);

async fn start_server(auth_enabled: bool, require_auth_for_reads: bool) -> (u16, TempDir) {
    let temp_dir = TempDir::new().unwrap();
    let port = NEXT_PORT.fetch_add(1, Ordering::SeqCst);

    let db_config = BlockDBConfig {
        data_dir: temp_dir.path().to_string_lossy().to_string(),
        auth_enabled,
        ..Default::default()
    };
    let db = BlockDBHandle::new(db_config).unwrap();

    let api_config = ApiConfig {
        host: "127.0.0.1".to_string(),
        port,
        auth_enabled,
        require_auth_for_reads,
        ..Default::default()
    };

    let server = if auth_enabled {
        let am = AuthManager::new(AuthConfig::default());
        am.create_user(
            "writer".to_string(),
            "Str0ng!Pass",
            PermissionSet::read_write(),
            Some("admin".to_string()),
            0,
        )
        .unwrap();
        BlockDBServer::with_auth(db, api_config, am)
    } else {
        BlockDBServer::new(db, api_config)
    };

    tokio::spawn(async move {
        let _ = server.start().await;
    });

    // Give the listener a moment to bind.
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    (port, temp_dir)
}

async fn get(port: u16, path: &str) -> (StatusCode, serde_json::Value) {
    let client = Client::new();
    let uri = format!("http://127.0.0.1:{}{}", port, path);
    let resp = client.get(uri.parse().unwrap()).await.unwrap();
    let status = resp.status();
    let bytes = hyper::body::to_bytes(resp.into_body()).await.unwrap();
    let value = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, value)
}

async fn post(port: u16, path: &str, body: serde_json::Value, token: Option<&str>) -> (StatusCode, serde_json::Value) {
    let client = Client::new();
    let uri = format!("http://127.0.0.1:{}{}", port, path);
    let mut builder = Request::builder().method(Method::POST).uri(uri).header("Content-Type", "application/json");
    if let Some(t) = token {
        builder = builder.header("Authorization", format!("Bearer {}", t));
    }
    let req = builder.body(Body::from(body.to_string())).unwrap();
    let resp = client.request(req).await.unwrap();
    let status = resp.status();
    let bytes = hyper::body::to_bytes(resp.into_body()).await.unwrap();
    let value = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, value)
}

#[tokio::test]
async fn test_health_endpoint() {
    let (port, _dir) = start_server(false, false).await;
    let (status, body) = get(port, "/health").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "healthy");
    assert_eq!(body["integrity_verified"], true);
}

#[tokio::test]
async fn test_metrics_endpoint() {
    let (port, _dir) = start_server(false, false).await;
    let client = Client::new();
    let uri = format!("http://127.0.0.1:{}/metrics", port);
    let resp = client.get(uri.parse().unwrap()).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = hyper::body::to_bytes(resp.into_body()).await.unwrap();
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("blockdb_requests_total"));
}

#[tokio::test]
async fn test_kv_write_read_no_auth() {
    let (port, _dir) = start_server(false, false).await;

    let (status, body) = post(port, "/kv", serde_json::json!({"key": "k1", "value": "v1"}), None).await;
    assert_eq!(status, StatusCode::OK, "write failed: {:?}", body);
    assert_eq!(body["success"], true);

    let (status, body) = get(port, "/kv?key=k1").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"], "v1");
}

#[tokio::test]
async fn test_append_only_conflict_returns_409() {
    let (port, _dir) = start_server(false, false).await;
    let (s1, _) = post(port, "/kv", serde_json::json!({"key": "dup", "value": "a"}), None).await;
    assert_eq!(s1, StatusCode::OK);
    let (s2, body) = post(port, "/kv", serde_json::json!({"key": "dup", "value": "b"}), None).await;
    assert_eq!(s2, StatusCode::CONFLICT, "expected append-only conflict: {:?}", body);
}

#[tokio::test]
async fn test_login_and_authenticated_write() {
    let (port, _dir) = start_server(true, false).await;

    // Unauthenticated write must be rejected.
    let (status, _) = post(port, "/kv", serde_json::json!({"key": "x", "value": "y"}), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Login.
    let (status, body) = post(port, "/login", serde_json::json!({"username": "writer", "password": "Str0ng!Pass"}), None).await;
    assert_eq!(status, StatusCode::OK, "login response: {:?}", body);
    let token = body["auth_token"].as_str().unwrap().to_string();

    // Authenticated write succeeds.
    let (status, body) = post(port, "/kv", serde_json::json!({"key": "x", "value": "y"}), Some(&token)).await;
    assert_eq!(status, StatusCode::OK, "auth write failed: {:?}", body);

    // Bad credentials fail.
    let (status, _) = post(port, "/login", serde_json::json!({"username": "writer", "password": "wrong"}), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_consent_lifecycle_over_http() {
    let (port, _dir) = start_server(false, false).await;

    // Grant.
    let (status, body) = post(
        port,
        "/consent/grant",
        serde_json::json!({"subject_id": "alice", "purpose": "marketing", "metadata": {"policy": "v2"}}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "grant failed: {:?}", body);
    assert_eq!(body["success"], true);

    // Status is granted.
    let (status, body) = get(port, "/consent/status?subject=alice&purpose=marketing").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["state"]["status"], "Granted");

    // Withdraw.
    tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    let (status, _) = post(
        port,
        "/consent/withdraw",
        serde_json::json!({"subject_id": "alice", "purpose": "marketing"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Status is now withdrawn.
    let (_, body) = get(port, "/consent/status?subject=alice&purpose=marketing").await;
    assert_eq!(body["state"]["status"], "Withdrawn");

    // History retains both events.
    let (_, body) = get(port, "/consent/history?subject=alice&purpose=marketing").await;
    assert_eq!(body["events"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn test_permission_enforced_for_user_creation() {
    // The pre-seeded "writer" user has read_write but NOT CreateUser permission.
    let (port, _dir) = start_server(true, false).await;
    let (_, body) = post(port, "/login", serde_json::json!({"username": "writer", "password": "Str0ng!Pass"}), None).await;
    let token = body["auth_token"].as_str().unwrap().to_string();

    let (status, _) = post(
        port,
        "/users",
        serde_json::json!({"username": "newbie", "password": "An0ther!Pass", "permissions": ["read"]}),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "non-admin must not create users");
}

// Keep a direct AuthManager sanity check alongside the HTTP tests.
#[tokio::test]
async fn test_auth_manager_direct() {
    let am = AuthManager::new(AuthConfig::default());
    am.create_user(
        "carol".to_string(),
        "Str0ng!Pass",
        PermissionSet::from_permissions(vec![Permission::Read, Permission::Write]),
        Some("admin".to_string()),
        0,
    )
    .unwrap();

    let ctx = am.authenticate("carol", "Str0ng!Pass").unwrap();
    assert!(ctx.has_permission(&Permission::Write));

    let validated = am.authenticate_with_token(&ctx.session_id).unwrap();
    assert_eq!(validated.user_id, "carol");

    assert!(am.authenticate("carol", "nope").is_err());
}
