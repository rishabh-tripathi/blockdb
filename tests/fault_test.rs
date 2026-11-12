//! Fault-injection tests for the Raft cluster.
//!
//! These exercise real multi-node clusters over loopback HTTP and inject faults by
//! crashing nodes (stopping both the consensus loop and the HTTP server) and then
//! restarting them from the same data directory. They cover the three failure modes
//! that matter for a consistent, durable store:
//!
//!   * restart recovery — a crashed node reloads its committed data and persisted
//!     Raft state from disk (validates the hard-state/log persistence);
//!   * leader failover — when the leader crashes, the remaining majority elects a
//!     new leader and keeps accepting writes;
//!   * follower catch-up — a follower that was down (partitioned) rejoins and
//!     replays the writes it missed.

use blockdb::cluster::{build_cluster_config, ClusterNode};
use blockdb::storage::BlockDBConfig;
use std::path::Path;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

static NEXT_PORT: AtomicU16 = AtomicU16::new(20200);

struct RunningNode {
    node: Arc<ClusterNode>,
    shutdown: Option<oneshot::Sender<()>>,
    handle: JoinHandle<()>,
}

impl RunningNode {
    /// Fully crash the node: stop the consensus loop, stop the HTTP server, and wait
    /// for the server task to exit (freeing the port). The data directory is left
    /// intact so the node can be restarted.
    async fn crash(mut self) {
        self.node.stop().await;
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        let _ = self.handle.await;
    }
}

fn peer_list(size: usize, base: u16) -> Vec<(String, String, u16)> {
    (0..size)
        .map(|i| (format!("node{}", i), "127.0.0.1".to_string(), base + i as u16))
        .collect()
}

fn start_one(
    id: &str,
    port: u16,
    peers: &[(String, String, u16)],
    data_dir: &Path,
) -> RunningNode {
    start_one_timed(id, port, peers, data_dir, 50, 150)
}

fn start_one_timed(
    id: &str,
    port: u16,
    peers: &[(String, String, u16)],
    data_dir: &Path,
    heartbeat_ms: u64,
    election_ms: u64,
) -> RunningNode {
    let storage_config = BlockDBConfig {
        data_dir: data_dir.to_string_lossy().to_string(),
        ..Default::default()
    };
    let cluster_config = build_cluster_config(
        id,
        "127.0.0.1",
        port,
        peers,
        Duration::from_millis(heartbeat_ms),
        Duration::from_millis(election_ms),
    );
    let node = ClusterNode::new(cluster_config, storage_config).unwrap();
    let (tx, rx) = oneshot::channel::<()>();
    let n = node.clone();
    let handle = tokio::spawn(async move {
        let _ = n
            .start(async move {
                let _ = rx.await;
            })
            .await;
    });
    RunningNode {
        node,
        shutdown: Some(tx),
        handle,
    }
}

async fn wait_until_leader(node: &Arc<ClusterNode>) -> bool {
    for _ in 0..120 {
        if node.is_leader().await {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

/// Return the index of the first node in `nodes` (skipping `None`) that is leader,
/// waiting up to a timeout.
async fn wait_any_leader(nodes: &[Option<RunningNode>]) -> Option<usize> {
    for _ in 0..120 {
        for (i, slot) in nodes.iter().enumerate() {
            if let Some(rn) = slot {
                if rn.node.is_leader().await {
                    return Some(i);
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    None
}

async fn wait_for_value(node: &Arc<ClusterNode>, key: &[u8], expected: &[u8]) -> bool {
    for _ in 0..120 {
        if let Ok(Some(v)) = node.get(key).await {
            if v == expected {
                return true;
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_restart_recovers_committed_data_and_raft_state() {
    let base = NEXT_PORT.fetch_add(1, Ordering::SeqCst);
    let peers = peer_list(1, base);
    let dir = TempDir::new().unwrap();

    // Boot, elect, commit a write.
    let rn = start_one("node0", base, &peers, dir.path());
    assert!(wait_until_leader(&rn.node).await, "node did not elect");
    rn.node.put(b"durable", b"value").await.unwrap();
    assert_eq!(rn.node.get(b"durable").await.unwrap(), Some(b"value".to_vec()));

    // The Raft log must have been persisted to disk.
    let log_path = dir.path().join("raft").join("log.bin");
    assert!(log_path.exists(), "raft log file was not created");
    assert!(
        std::fs::metadata(&log_path).unwrap().len() > 0,
        "raft log file is empty after a committed write"
    );

    // Crash the node.
    rn.crash().await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Restart on the SAME data directory.
    let rn2 = start_one("node0", base, &peers, dir.path());
    assert!(
        wait_until_leader(&rn2.node).await,
        "node did not re-elect after restart"
    );

    // Committed data survived the crash.
    assert_eq!(
        rn2.node.get(b"durable").await.unwrap(),
        Some(b"value".to_vec()),
        "committed value was lost across restart"
    );
    rn2.crash().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_leader_failover_on_crash() {
    let size = 3;
    let base = NEXT_PORT.fetch_add(size as u16, Ordering::SeqCst);
    let peers = peer_list(size, base);
    let dirs: Vec<TempDir> = (0..size).map(|_| TempDir::new().unwrap()).collect();

    let mut nodes: Vec<Option<RunningNode>> = (0..size)
        .map(|i| Some(start_one(&format!("node{}", i), base + i as u16, &peers, dirs[i].path())))
        .collect();

    tokio::time::sleep(Duration::from_millis(300)).await;
    let leader = wait_any_leader(&nodes).await.expect("no initial leader");

    // Crash the leader.
    let crashed = nodes[leader].take().unwrap();
    crashed.crash().await;

    // The surviving majority (2 of 3) must elect a new leader.
    let new_leader = wait_any_leader(&nodes).await.expect("no leader after crash");
    assert_ne!(new_leader, leader, "crashed leader cannot still lead");

    // The new leader accepts a write that commits on both survivors.
    let new_leader_node = nodes[new_leader].as_ref().unwrap().node.clone();
    new_leader_node.put(b"after", b"failover").await.unwrap();
    for slot in nodes.iter() {
        if let Some(rn) = slot {
            assert!(
                wait_for_value(&rn.node, b"after", b"failover").await,
                "survivor never got the post-failover write"
            );
        }
    }

    for slot in nodes.into_iter().flatten() {
        slot.crash().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_follower_rejoins_and_catches_up() {
    let size = 3;
    let base = NEXT_PORT.fetch_add(size as u16, Ordering::SeqCst);
    let peers = peer_list(size, base);
    let dirs: Vec<TempDir> = (0..size).map(|_| TempDir::new().unwrap()).collect();

    let mut nodes: Vec<Option<RunningNode>> = (0..size)
        .map(|i| Some(start_one(&format!("node{}", i), base + i as u16, &peers, dirs[i].path())))
        .collect();

    tokio::time::sleep(Duration::from_millis(300)).await;
    let leader = wait_any_leader(&nodes).await.expect("no initial leader");

    // Partition out a follower by crashing it (its data dir is preserved).
    let follower = (leader + 1) % size;
    let crashed = nodes[follower].take().unwrap();
    crashed.crash().await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Write while the follower is down — still commits on the 2-node majority.
    let leader_node = nodes[leader].as_ref().unwrap().node.clone();
    leader_node.put(b"missed", b"while-down").await.unwrap();
    assert!(
        wait_for_value(&leader_node, b"missed", b"while-down").await,
        "leader never committed the write taken during the partition"
    );

    // The follower rejoins from the same data directory and must catch up.
    let rejoined = start_one(
        &format!("node{}", follower),
        base + follower as u16,
        &peers,
        dirs[follower].path(),
    );
    assert!(
        wait_for_value(&rejoined.node, b"missed", b"while-down").await,
        "rejoined follower never caught up on the missed write"
    );
    nodes[follower] = Some(rejoined);

    for slot in nodes.into_iter().flatten() {
        slot.crash().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_add_node_to_live_cluster() {
    // Start a 3-node cluster {0,1,2}.
    let size = 3;
    let base = NEXT_PORT.fetch_add(4, Ordering::SeqCst);
    let joiner_port = base + 3;
    let core_peers = peer_list(size, base);
    let dirs: Vec<TempDir> = (0..4).map(|_| TempDir::new().unwrap()).collect();

    let mut nodes: Vec<Option<RunningNode>> = (0..size)
        .map(|i| Some(start_one(&format!("node{}", i), base + i as u16, &core_peers, dirs[i].path())))
        .collect();
    tokio::time::sleep(Duration::from_millis(300)).await;
    let leader = wait_any_leader(&nodes).await.expect("no initial leader");
    let leader_node = nodes[leader].as_ref().unwrap().node.clone();

    // Commit a write while the cluster is still 3 nodes (fast majority of 2).
    leader_node.put(b"joined", b"welcome").await.unwrap();

    // Start the joiner (node3). It knows the others; give it a long election timeout
    // so it stays a quiet follower and doesn't disrupt before being admitted.
    let joiner_peers: Vec<(String, String, u16)> = (0..4)
        .map(|i| (format!("node{}", i), "127.0.0.1".to_string(), base + i as u16))
        .collect();
    let joiner = start_one_timed("node3", joiner_port, &joiner_peers, dirs[3].path(), 50, 3000);
    tokio::time::sleep(Duration::from_millis(200)).await;

    // The leader admits node3 into the cluster; it must catch up on the committed log.
    leader_node.add_node("node3", "127.0.0.1", joiner_port).await.unwrap();
    assert!(
        wait_for_value(&joiner.node, b"joined", b"welcome").await,
        "newly added node never caught up on the replicated log"
    );

    joiner.crash().await;
    for slot in nodes.drain(..).flatten() {
        slot.crash().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_remove_node_from_live_cluster() {
    // 3-node cluster; the node we will remove gets a long election timeout so it
    // can't disrupt the cluster in the window before it is decommissioned.
    let size = 3;
    let base = NEXT_PORT.fetch_add(size as u16, Ordering::SeqCst);
    let peers = peer_list(size, base);
    let dirs: Vec<TempDir> = (0..size).map(|_| TempDir::new().unwrap()).collect();

    // Make node2 the removable one with a long election timeout.
    let mut nodes: Vec<Option<RunningNode>> = Vec::new();
    for i in 0..size {
        let election = if i == 2 { 3000 } else { 150 };
        nodes.push(Some(start_one_timed(
            &format!("node{}", i),
            base + i as u16,
            &peers,
            dirs[i].path(),
            50,
            election,
        )));
    }
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Ensure node0 or node1 leads (node2 can't, its timeout is long). Find the leader.
    let leader = wait_any_leader(&nodes).await.expect("no initial leader");
    assert_ne!(leader, 2, "the long-timeout node should not be leader");

    // Remove node2 from the cluster.
    let leader_node = nodes[leader].as_ref().unwrap().node.clone();
    leader_node.remove_node("node2").await.unwrap();

    // Decommission the removed node, then confirm the remaining two still commit
    // writes (majority is now 2 of 2).
    nodes[2].take().unwrap().crash().await;

    leader_node.put(b"after", b"removal").await.unwrap();
    for slot in nodes.iter() {
        if let Some(rn) = slot {
            assert!(
                wait_for_value(&rn.node, b"after", b"removal").await,
                "remaining node did not get the post-removal write"
            );
        }
    }

    for slot in nodes.into_iter().flatten() {
        slot.crash().await;
    }
}
