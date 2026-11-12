//! Multi-node cluster integration tests.
//!
//! Spins up real 3-node BlockDB clusters that talk to each other over HTTP on
//! loopback ports, and verifies leader election, replicated writes converging on
//! every node, and follower-forwarded writes — i.e. the consistency guarantees.

use blockdb::cluster::{build_cluster_config, ClusterNode, ClusterSecurity};
use blockdb::storage::BlockDBConfig;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;

static NEXT_PORT: AtomicU16 = AtomicU16::new(19300);

struct Cluster {
    nodes: Vec<Arc<ClusterNode>>,
    ports: Vec<u16>,
    _dirs: Vec<TempDir>,
}

async fn spawn_cluster(size: usize) -> Cluster {
    spawn_cluster_secure(size, ClusterSecurity::default()).await
}

async fn spawn_cluster_secure(size: usize, security: ClusterSecurity) -> Cluster {
    let base = NEXT_PORT.fetch_add(size as u16, Ordering::SeqCst);
    let ids: Vec<String> = (0..size).map(|i| format!("node{}", i)).collect();
    let peers: Vec<(String, String, u16)> = (0..size)
        .map(|i| (ids[i].clone(), "127.0.0.1".to_string(), base + i as u16))
        .collect();

    let mut nodes = Vec::new();
    let mut dirs = Vec::new();
    let mut ports = Vec::new();
    for i in 0..size {
        let dir = TempDir::new().unwrap();
        let storage_config = BlockDBConfig {
            data_dir: dir.path().to_string_lossy().to_string(),
            ..Default::default()
        };
        // Fast timers so elections resolve quickly under test.
        let cluster_config = build_cluster_config(
            &ids[i],
            "127.0.0.1",
            base + i as u16,
            &peers,
            Duration::from_millis(50),
            Duration::from_millis(150),
        );
        let node =
            ClusterNode::new_secure(cluster_config, storage_config, security.clone()).unwrap();
        let n = node.clone();
        tokio::spawn(async move {
            let _ = n.start(std::future::pending::<()>()).await;
        });
        nodes.push(node);
        ports.push(base + i as u16);
        dirs.push(dir);
    }

    // Let the HTTP servers bind.
    tokio::time::sleep(Duration::from_millis(300)).await;
    Cluster {
        nodes,
        ports,
        _dirs: dirs,
    }
}

async fn wait_for_leader(cluster: &Cluster) -> usize {
    for _ in 0..100 {
        for (i, node) in cluster.nodes.iter().enumerate() {
            if node.is_leader().await {
                return i;
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("no leader elected within timeout");
}

async fn wait_for_value(node: &Arc<ClusterNode>, key: &[u8], expected: &[u8]) -> bool {
    for _ in 0..100 {
        if let Ok(Some(v)) = node.get(key).await {
            if v == expected {
                return true;
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

#[tokio::test]
async fn test_single_node_cluster_elects_and_writes() {
    let cluster = spawn_cluster(1).await;
    let leader = wait_for_leader(&cluster).await;
    assert_eq!(leader, 0, "the sole node must become leader");

    cluster.nodes[0].put(b"k", b"v").await.unwrap();
    assert_eq!(cluster.nodes[0].get(b"k").await.unwrap(), Some(b"v".to_vec()));
}

#[tokio::test]
async fn test_three_node_cluster_elects_single_leader() {
    let cluster = spawn_cluster(3).await;
    let leader = wait_for_leader(&cluster).await;

    // Give the cluster a moment to settle, then assert exactly one leader.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let mut leaders = 0;
    for node in &cluster.nodes {
        if node.is_leader().await {
            leaders += 1;
        }
    }
    assert_eq!(leaders, 1, "exactly one leader expected, node {} led", leader);
}

#[tokio::test]
async fn test_write_replicates_to_all_nodes() {
    let cluster = spawn_cluster(3).await;
    let leader = wait_for_leader(&cluster).await;

    // Write directly to the leader.
    cluster.nodes[leader].put(b"user:1", b"alice").await.unwrap();

    // Every node — leader and followers — must converge on the value.
    for (i, node) in cluster.nodes.iter().enumerate() {
        assert!(
            wait_for_value(node, b"user:1", b"alice").await,
            "node {} never received the replicated write",
            i
        );
    }
}

#[tokio::test]
async fn test_write_to_follower_is_forwarded() {
    let cluster = spawn_cluster(3).await;
    let leader = wait_for_leader(&cluster).await;
    let follower = (leader + 1) % 3;

    // Write to a follower; it must transparently forward to the leader.
    cluster.nodes[follower].put(b"k2", b"v2").await.unwrap();

    for (i, node) in cluster.nodes.iter().enumerate() {
        assert!(
            wait_for_value(node, b"k2", b"v2").await,
            "node {} never received the forwarded write",
            i
        );
    }
}

#[tokio::test]
async fn test_append_only_conflict_across_cluster() {
    let cluster = spawn_cluster(3).await;
    let leader = wait_for_leader(&cluster).await;

    cluster.nodes[leader].put(b"immutable", b"first").await.unwrap();
    assert!(wait_for_value(&cluster.nodes[leader], b"immutable", b"first").await);

    // A second write to the same key is rejected (append-only), consistently.
    let second = cluster.nodes[leader].put(b"immutable", b"second").await;
    assert!(second.is_err(), "duplicate key must be rejected");

    // Value is unchanged on every node.
    for node in &cluster.nodes {
        assert!(wait_for_value(node, b"immutable", b"first").await);
    }
}

// ---- Security / auth tests ----

use hyper::{Body, Client, Request, StatusCode};

/// POST a JSON body to a node, optionally with an `Authorization: Bearer` token and
/// an `x-cluster-auth` secret header. Returns the HTTP status.
async fn post(
    port: u16,
    path: &str,
    body: &str,
    bearer: Option<&str>,
    cluster_secret: Option<&str>,
) -> StatusCode {
    let uri = format!("http://127.0.0.1:{}{}", port, path);
    let mut builder = Request::post(&uri).header("content-type", "application/json");
    if let Some(t) = bearer {
        builder = builder.header("authorization", format!("Bearer {}", t));
    }
    if let Some(s) = cluster_secret {
        builder = builder.header("x-cluster-auth", s);
    }
    let req = builder.body(Body::from(body.to_string())).unwrap();
    Client::new().request(req).await.unwrap().status()
}

async fn get_status(port: u16, path: &str, bearer: Option<&str>) -> StatusCode {
    let uri = format!("http://127.0.0.1:{}{}", port, path);
    let mut builder = Request::get(&uri);
    if let Some(t) = bearer {
        builder = builder.header("authorization", format!("Bearer {}", t));
    }
    let req = builder.body(Body::empty()).unwrap();
    Client::new().request(req).await.unwrap().status()
}

#[tokio::test]
async fn test_kv_requires_api_token() {
    let security = ClusterSecurity {
        cluster_secret: None,
        api_token: Some("s3cr3t-token".to_string()),
        tls: None,
    };
    let cluster = spawn_cluster_secure(1, security).await;
    let _ = wait_for_leader(&cluster).await;
    let port = cluster.ports[0];

    let write = r#"{"key":"k","value":"v"}"#;

    // No token → rejected.
    assert_eq!(
        post(port, "/kv", write, None, None).await,
        StatusCode::UNAUTHORIZED
    );
    // Wrong token → rejected.
    assert_eq!(
        post(port, "/kv", write, Some("wrong"), None).await,
        StatusCode::UNAUTHORIZED
    );
    // Correct token → accepted.
    assert_eq!(
        post(port, "/kv", write, Some("s3cr3t-token"), None).await,
        StatusCode::OK
    );
    // Reads are gated too.
    assert_eq!(
        get_status(port, "/kv?key=k", None).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        get_status(port, "/kv?key=k", Some("s3cr3t-token")).await,
        StatusCode::OK
    );
    // Liveness endpoint stays open.
    assert_eq!(get_status(port, "/status", None).await, StatusCode::OK);
}

#[tokio::test]
async fn test_raft_message_requires_cluster_secret() {
    let security = ClusterSecurity {
        cluster_secret: Some("cluster-shared-secret".to_string()),
        api_token: None,
        tls: None,
    };
    let cluster = spawn_cluster_secure(1, security).await;
    let _ = wait_for_leader(&cluster).await;
    let port = cluster.ports[0];

    // A forged Raft RPC with no secret is rejected before the body is even parsed.
    assert_eq!(
        post(port, "/raft/message", "{}", None, None).await,
        StatusCode::UNAUTHORIZED
    );
    // Wrong secret → rejected.
    assert_eq!(
        post(port, "/raft/message", "{}", None, Some("nope")).await,
        StatusCode::UNAUTHORIZED
    );
    // Correct secret passes auth; the empty body then fails to parse (400),
    // which proves the secret gate let it through.
    assert_eq!(
        post(port, "/raft/message", "{}", None, Some("cluster-shared-secret")).await,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn test_secured_cluster_elects_and_replicates() {
    // Both secrets set: inter-node RPCs must carry the cluster secret (or no leader
    // is ever elected) and forwarded writes must carry the API token.
    let security = ClusterSecurity {
        cluster_secret: Some("abc".to_string()),
        api_token: Some("tok".to_string()),
        tls: None,
    };
    let cluster = spawn_cluster_secure(3, security).await;
    let leader = wait_for_leader(&cluster).await;
    let follower = (leader + 1) % 3;

    // Internal write via a follower forwards to the leader with the token attached.
    cluster.nodes[follower].put(b"secured", b"yes").await.unwrap();
    for (i, node) in cluster.nodes.iter().enumerate() {
        assert!(
            wait_for_value(node, b"secured", b"yes").await,
            "node {} never received the replicated write under auth",
            i
        );
    }
}

#[tokio::test]
async fn test_health_and_metrics_endpoints() {
    let cluster = spawn_cluster(1).await;
    let _ = wait_for_leader(&cluster).await;
    let port = cluster.ports[0];
    cluster.nodes[0].put(b"m", b"1").await.unwrap();

    // /health is open and reports healthy once a leader exists.
    let uri = format!("http://127.0.0.1:{}/health", port);
    let resp = Client::new().get(uri.parse().unwrap()).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = hyper::body::to_bytes(resp.into_body()).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["status"], "healthy");
    assert_eq!(v["has_leader"], true);

    // /metrics is open and exposes Prometheus counters.
    let uri = format!("http://127.0.0.1:{}/metrics", port);
    let resp = Client::new().get(uri.parse().unwrap()).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = hyper::body::to_bytes(resp.into_body()).await.unwrap();
    let text = String::from_utf8_lossy(&body);
    assert!(text.contains("blockdb_cluster_kv_writes_total"));
    assert!(text.contains("blockdb_cluster_is_leader 1"));
}

#[tokio::test]
async fn test_linearizable_read_from_follower() {
    let cluster = spawn_cluster(3).await;
    let leader = wait_for_leader(&cluster).await;
    let follower = (leader + 1) % 3;

    // Write to the leader.
    cluster.nodes[leader].put(b"lin:key", b"committed").await.unwrap();

    // A linearizable read issued at a follower must return the committed value
    // (it forwards to the leader, which confirms leadership before answering),
    // even immediately after the write.
    let got = cluster.nodes[follower]
        .get_linearizable(b"lin:key")
        .await
        .unwrap();
    assert_eq!(got, Some(b"committed".to_vec()));

    // And on the leader itself.
    let got = cluster.nodes[leader]
        .get_linearizable(b"lin:key")
        .await
        .unwrap();
    assert_eq!(got, Some(b"committed".to_vec()));
}
