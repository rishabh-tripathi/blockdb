//! Smoke tests for the consensus foundation.
//!
//! NOTE: BlockDB's Raft layer currently uses an in-process message transport and has
//! no cross-node networking, so these tests cover the single-node building blocks
//! (cluster configuration, node creation, and the ConsensusEngine trait) rather than
//! true multi-node elections or replication.

use blockdb::consensus::raft::RaftNode;
use blockdb::consensus::{ClusterConfig, ConsensusEngine, NodeAddress, NodeId};
use blockdb::transaction::Operation;

fn single_node_config() -> ClusterConfig {
    let node_id = NodeId::from_string("node1".to_string());
    let address = NodeAddress::new("127.0.0.1".to_string(), 9001);
    ClusterConfig::new(node_id, address)
}

#[test]
fn test_cluster_config_majority_and_size() {
    let mut config = single_node_config();
    assert_eq!(config.cluster_size(), 1);
    assert_eq!(config.majority(), 1);

    config.add_peer(
        NodeId::from_string("node2".to_string()),
        NodeAddress::new("127.0.0.1".to_string(), 9002),
    );
    config.add_peer(
        NodeId::from_string("node3".to_string()),
        NodeAddress::new("127.0.0.1".to_string(), 9003),
    );
    assert_eq!(config.cluster_size(), 3);
    assert_eq!(config.majority(), 2);
}

#[test]
fn test_node_address_url() {
    let addr = NodeAddress::new("10.0.0.5".to_string(), 8080);
    assert_eq!(addr.to_url(), "http://10.0.0.5:8080");
}

#[tokio::test]
async fn test_raft_node_starts_and_stops() {
    let node = RaftNode::new_local(single_node_config());
    node.start().await.unwrap();

    // Membership changes are accepted (no-ops today) and the engine reports a leader view.
    node.add_node(
        NodeId::from_string("node2".to_string()),
        NodeAddress::new("127.0.0.1".to_string(), 9002),
    )
    .await
    .unwrap();

    // is_leader is queryable without panicking.
    let _ = node.is_leader().await;

    node.stop().await;
}

#[tokio::test]
async fn test_consensus_engine_propose_smoke() {
    let node = RaftNode::new_local(single_node_config());
    node.start().await.unwrap();

    // Proposing is accepted by the local engine (single-node, in-process transport).
    let op = Operation::Put {
        key: b"k".to_vec(),
        value: b"v".to_vec(),
    };
    // Should not panic; result may be Ok in single-node mode.
    let _ = node.propose(op).await;

    node.stop().await;
}
