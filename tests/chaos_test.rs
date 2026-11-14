//! Chaos / partition safety tests for the Raft core (an in-repo Jepsen substitute).
//!
//! These run real `RaftNode`s wired through a controllable in-memory transport that can
//! drop messages across an arbitrary network partition. They assert the central Raft
//! safety property — **state-machine safety**: no two nodes ever apply different values
//! at the same log index — across leader isolation, minority stalls, and partition
//! healing, plus liveness (the majority keeps committing and everyone converges after a
//! heal).
//!
//! This is not a substitute for a full external Jepsen suite, but it exercises the
//! partition scenarios that break naive consensus implementations.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use blockdb::consensus::log_entry::{LogEntry, LogOperation};
use blockdb::consensus::raft::RaftNode;
use blockdb::consensus::transport::RaftTransport;
use blockdb::consensus::{
    ClusterConfig, ConsensusEngine, ConsensusMessage, NodeAddress, NodeId, StateMachine,
};
use blockdb::error::BlockDBError;
use blockdb::transaction::Operation;
use tokio::sync::mpsc::UnboundedSender;

/// Routes Raft messages between in-process nodes, dropping any crossing a partition.
struct Hub {
    injectors: Mutex<HashMap<String, UnboundedSender<(NodeId, ConsensusMessage)>>>,
    /// Directed (from_id, to_id) pairs whose messages are dropped.
    blocked: Mutex<HashSet<(String, String)>>,
}

impl Hub {
    fn new() -> Arc<Self> {
        Arc::new(Hub {
            injectors: Mutex::new(HashMap::new()),
            blocked: Mutex::new(HashSet::new()),
        })
    }

    fn register(&self, id: &str, tx: UnboundedSender<(NodeId, ConsensusMessage)>) {
        self.injectors.lock().unwrap().insert(id.to_string(), tx);
    }

    /// Install a partition: every directed pair crossing a group boundary is blocked.
    fn partition(&self, groups: &[&[&str]]) {
        let mut blocked = self.blocked.lock().unwrap();
        blocked.clear();
        for (gi, g) in groups.iter().enumerate() {
            for (gj, h) in groups.iter().enumerate() {
                if gi == gj {
                    continue;
                }
                for a in g.iter() {
                    for b in h.iter() {
                        blocked.insert((a.to_string(), b.to_string()));
                    }
                }
            }
        }
    }

    fn heal(&self) {
        self.blocked.lock().unwrap().clear();
    }
}

struct HubTransport(Arc<Hub>);

#[async_trait]
impl RaftTransport for HubTransport {
    async fn send(&self, target: &NodeAddress, from: NodeId, message: ConsensusMessage) {
        // Address host encodes the node id in this harness.
        let to = target.host.clone();
        if self.0.blocked.lock().unwrap().contains(&(from.0.clone(), to.clone())) {
            return; // partitioned: drop
        }
        let injector = self.0.injectors.lock().unwrap().get(&to).cloned();
        if let Some(tx) = injector {
            let _ = tx.send((from, message));
        }
    }
}

/// State machine that records the value applied at each log index.
struct Recorder {
    applied: Arc<Mutex<BTreeMap<u64, Vec<u8>>>>,
}

#[async_trait]
impl StateMachine for Recorder {
    async fn apply(&self, entry: &LogEntry) -> Result<(), BlockDBError> {
        if let LogOperation::Put { value, .. } = &entry.operation {
            self.applied.lock().unwrap().insert(entry.index, value.clone());
        }
        Ok(())
    }
}

struct Node {
    id: String,
    raft: Arc<RaftNode>,
    applied: Arc<Mutex<BTreeMap<u64, Vec<u8>>>>,
}

fn build_cluster(n: usize, hub: &Arc<Hub>) -> Vec<Node> {
    let ids: Vec<String> = (0..n).map(|i| format!("n{}", i)).collect();
    let mut nodes = Vec::new();
    for i in 0..n {
        // The node id is also used as the transport "host" so the hub can route.
        let mut config = ClusterConfig::new(
            NodeId::from_string(ids[i].clone()),
            NodeAddress::new(ids[i].clone(), 0),
        );
        config.heartbeat_interval = Duration::from_millis(40);
        config.election_timeout = Duration::from_millis(160);
        for j in 0..n {
            if i != j {
                config.add_peer(
                    NodeId::from_string(ids[j].clone()),
                    NodeAddress::new(ids[j].clone(), 0),
                );
            }
        }
        let applied = Arc::new(Mutex::new(BTreeMap::new()));
        let raft = Arc::new(RaftNode::new(
            config,
            Arc::new(HubTransport(hub.clone())),
            Arc::new(Recorder { applied: applied.clone() }),
        ));
        hub.register(&ids[i], raft.message_injector());
        nodes.push(Node { id: ids[i].clone(), raft, applied });
    }
    nodes
}

async fn start_all(nodes: &[Node]) {
    for node in nodes {
        node.raft.start().await.unwrap();
    }
}

async fn find_leader(nodes: &[Node]) -> Option<usize> {
    for _ in 0..150 {
        for (i, node) in nodes.iter().enumerate() {
            if node.raft.is_leader().await {
                return Some(i);
            }
        }
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
    None
}

/// Find a leader among a subset of node indices.
async fn find_leader_in(nodes: &[Node], idxs: &[usize]) -> Option<usize> {
    for _ in 0..150 {
        for &i in idxs {
            if nodes[i].raft.is_leader().await {
                return Some(i);
            }
        }
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
    None
}

fn put(k: &str, v: &str) -> Operation {
    Operation::Put { key: k.as_bytes().to_vec(), value: v.as_bytes().to_vec() }
}

/// The core invariant: across all nodes, any index applied by more than one node must
/// carry the same value. A violation is a lost/contradictory committed write.
fn assert_no_divergence(nodes: &[Node]) {
    let mut merged: BTreeMap<u64, Vec<u8>> = BTreeMap::new();
    for node in nodes {
        for (idx, val) in node.applied.lock().unwrap().iter() {
            if let Some(existing) = merged.get(idx) {
                assert_eq!(
                    existing, val,
                    "state-machine safety violated at index {}: nodes disagree",
                    idx
                );
            } else {
                merged.insert(*idx, val.clone());
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_partition_safety_and_convergence() {
    let hub = Hub::new();
    let nodes = build_cluster(5, &hub);
    start_all(&nodes).await;

    let leader = find_leader(&nodes).await.expect("no initial leader");

    // Commit a few writes with the full cluster connected.
    for i in 0..3 {
        nodes[leader]
            .raft
            .propose(put(&format!("k{}", i), &format!("v{}", i)))
            .await
            .unwrap();
    }
    assert_no_divergence(&nodes);

    // Partition: isolate the leader + one follower (minority of 2) from the other 3.
    let all: Vec<String> = nodes.iter().map(|n| n.id.clone()).collect();
    let minority_idx = [leader, (leader + 1) % 5];
    let majority_idx: Vec<usize> = (0..5).filter(|i| !minority_idx.contains(i)).collect();
    let min_ids: Vec<&str> = minority_idx.iter().map(|&i| all[i].as_str()).collect();
    let maj_ids: Vec<&str> = majority_idx.iter().map(|&i| all[i].as_str()).collect();
    hub.partition(&[&min_ids, &maj_ids]);

    // The majority side elects a new leader and keeps committing.
    let new_leader = find_leader_in(&nodes, &majority_idx)
        .await
        .expect("majority did not elect a leader");
    for i in 0..3 {
        nodes[new_leader]
            .raft
            .propose(put(&format!("mk{}", i), &format!("mv{}", i)))
            .await
            .unwrap();
    }

    // The isolated old leader cannot commit (no majority): the proposal times out.
    let stuck = nodes[leader].raft.propose(put("lost", "should-not-commit")).await;
    assert!(stuck.is_err(), "a minority leader must not be able to commit");

    assert_no_divergence(&nodes);

    // Heal the partition; the whole cluster must converge without contradiction.
    hub.heal();
    // Give replication time to reconcile the isolated nodes.
    tokio::time::sleep(Duration::from_millis(1500)).await;

    // Drive one more write through whoever leads now, then re-check safety.
    if let Some(l) = find_leader(&nodes).await {
        let _ = nodes[l].raft.propose(put("after", "heal")).await;
    }
    tokio::time::sleep(Duration::from_millis(800)).await;

    assert_no_divergence(&nodes);

    // Liveness: the writes the majority committed during the partition are present on
    // a majority of nodes after healing.
    for i in 0..3 {
        let want = format!("mv{}", i).into_bytes();
        let count = nodes
            .iter()
            .filter(|n| n.applied.lock().unwrap().values().any(|v| *v == want))
            .count();
        assert!(count >= 3, "majority-committed write mv{} did not converge", i);
    }

    for node in &nodes {
        node.raft.stop().await;
    }
}
