//! Multi-node cluster integration tests.
//!
//! Spins up real 3-node BlockDB clusters that talk to each other over HTTP on
//! loopback ports, and verifies leader election, replicated writes converging on
//! every node, and follower-forwarded writes — i.e. the consistency guarantees.

use blockdb::cluster::{build_cluster_config, ClusterNode};
use blockdb::storage::BlockDBConfig;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;

static NEXT_PORT: AtomicU16 = AtomicU16::new(19300);

struct Cluster {
    nodes: Vec<Arc<ClusterNode>>,
    _dirs: Vec<TempDir>,
}

async fn spawn_cluster(size: usize) -> Cluster {
    let base = NEXT_PORT.fetch_add(size as u16, Ordering::SeqCst);
    let ids: Vec<String> = (0..size).map(|i| format!("node{}", i)).collect();
    let peers: Vec<(String, String, u16)> = (0..size)
        .map(|i| (ids[i].clone(), "127.0.0.1".to_string(), base + i as u16))
        .collect();

    let mut nodes = Vec::new();
    let mut dirs = Vec::new();
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
        let node = ClusterNode::new(cluster_config, storage_config).unwrap();
        let n = node.clone();
        tokio::spawn(async move {
            let _ = n.start(std::future::pending::<()>()).await;
        });
        nodes.push(node);
        dirs.push(dir);
    }

    // Let the HTTP servers bind.
    tokio::time::sleep(Duration::from_millis(300)).await;
    Cluster { nodes, _dirs: dirs }
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
