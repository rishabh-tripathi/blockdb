use blockdb::consensus::{RaftNode, LogEntry, Command, NodeState, ClusterConfig, NodeId, NodeAddress};
use blockdb::{BlockDBConfig};
use tempfile::TempDir;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::time::{sleep, Duration};

/// Comprehensive consensus layer tests
/// Tests Raft implementation, leader election, log replication, and safety properties
#[tokio::test]
async fn test_raft_single_node_operations() {
    let temp_dir = TempDir::new().unwrap();
    let node_id = NodeId("node1".to_string());
    
    let cluster_config = ClusterConfig {
        nodes: vec![
            (node_id.clone(), NodeAddress("127.0.0.1:9001".to_string())),
        ].into_iter().collect(),
        heartbeat_interval: Duration::from_millis(100),
        election_timeout: Duration::from_millis(300),
    };

    let db_config = BlockDBConfig {
        data_dir: temp_dir.path().to_string_lossy().to_string(),
        ..Default::default()
    };

    // Test 1: Single node should become leader immediately
    let node = RaftNode::new(
        node_id.clone(),
        cluster_config.clone(),
        db_config,
    ).await.unwrap();

    // Wait for election
    sleep(Duration::from_millis(500)).await;

    let state = node.get_state().await;
    assert_eq!(state.current_state, NodeState::Leader);
    assert_eq!(state.current_term, 1);
    assert_eq!(state.voted_for, Some(node_id.clone()));

    // Test 2: Leader should be able to append entries
    let command = Command::Put {
        key: b"test_key".to_vec(),
        value: b"test_value".to_vec(),
    };

    let entry_index = node.append_entry(command.clone()).await.unwrap();
    assert_eq!(entry_index, 1); // First log entry

    // Test 3: Entry should be committed immediately in single-node cluster
    sleep(Duration::from_millis(200)).await;
    
    let log_state = node.get_log_state().await;
    assert!(log_state.commit_index >= entry_index);
    assert!(log_state.last_applied >= entry_index);

    // Test 4: Multiple entries
    for i in 0..5 {
        let cmd = Command::Put {
            key: format!("key_{}", i).into_bytes(),
            value: format!("value_{}", i).into_bytes(),
        };
        let idx = node.append_entry(cmd).await.unwrap();
        assert_eq!(idx, 2 + i as u64);
    }

    sleep(Duration::from_millis(300)).await;
    
    let final_state = node.get_log_state().await;
    assert_eq!(final_state.last_log_index, 6); // 1 + 5 entries
    assert_eq!(final_state.commit_index, 6);
}

#[tokio::test]
async fn test_raft_leader_election() {
    let temp_dirs: Vec<_> = (0..3).map(|_| TempDir::new().unwrap()).collect();
    
    let nodes_config: HashMap<NodeId, NodeAddress> = (0..3)
        .map(|i| {
            (
                NodeId(format!("node{}", i)),
                NodeAddress(format!("127.0.0.1:900{}", i + 1)),
            )
        })
        .collect();

    let cluster_config = ClusterConfig {
        nodes: nodes_config.clone(),
        heartbeat_interval: Duration::from_millis(50),
        election_timeout: Duration::from_millis(150),
    };

    // Test 1: Start three nodes
    let mut nodes = Vec::new();
    for (i, (node_id, _)) in nodes_config.iter().enumerate() {
        let db_config = BlockDBConfig {
            data_dir: temp_dirs[i].path().to_string_lossy().to_string(),
            ..Default::default()
        };

        let node = RaftNode::new(
            node_id.clone(),
            cluster_config.clone(),
            db_config,
        ).await.unwrap();
        
        nodes.push(node);
    }

    // Test 2: Wait for leader election
    sleep(Duration::from_millis(500)).await;

    let mut leader_count = 0;
    let mut follower_count = 0;
    let mut current_term = 0;

    for node in &nodes {
        let state = node.get_state().await;
        match state.current_state {
            NodeState::Leader => {
                leader_count += 1;
                current_term = state.current_term;
            }
            NodeState::Follower => follower_count += 1,
            NodeState::Candidate => {
                // Should not remain in candidate state
                panic!("Node stuck in candidate state");
            }
        }
    }

    // Test 3: Exactly one leader should be elected
    assert_eq!(leader_count, 1);
    assert_eq!(follower_count, 2);
    assert!(current_term >= 1);

    // Test 4: All nodes should be in the same term
    for node in &nodes {
        let state = node.get_state().await;
        assert_eq!(state.current_term, current_term);
    }

    // Test 5: Leader should be able to commit entries
    let leader_node = nodes
        .iter()
        .find(|node| {
            let state = futures::executor::block_on(node.get_state());
            state.current_state == NodeState::Leader
        })
        .unwrap();

    let command = Command::Put {
        key: b"election_test_key".to_vec(),
        value: b"election_test_value".to_vec(),
    };

    let entry_index = leader_node.append_entry(command).await.unwrap();
    
    // Wait for replication
    sleep(Duration::from_millis(300)).await;

    // Test 6: All nodes should have the entry committed
    for node in &nodes {
        let log_state = node.get_log_state().await;
        assert!(log_state.commit_index >= entry_index);
    }
}

#[tokio::test]
async fn test_raft_log_replication() {
    let temp_dirs: Vec<_> = (0..3).map(|_| TempDir::new().unwrap()).collect();
    
    let nodes_config: HashMap<NodeId, NodeAddress> = (0..3)
        .map(|i| {
            (
                NodeId(format!("node{}", i)),
                NodeAddress(format!("127.0.0.1:900{}", i + 5)), // Different ports
            )
        })
        .collect();

    let cluster_config = ClusterConfig {
        nodes: nodes_config.clone(),
        heartbeat_interval: Duration::from_millis(50),
        election_timeout: Duration::from_millis(150),
    };

    // Start nodes
    let mut nodes = Vec::new();
    for (i, (node_id, _)) in nodes_config.iter().enumerate() {
        let db_config = BlockDBConfig {
            data_dir: temp_dirs[i].path().to_string_lossy().to_string(),
            ..Default::default()
        };

        let node = RaftNode::new(
            node_id.clone(),
            cluster_config.clone(),
            db_config,
        ).await.unwrap();
        
        nodes.push(node);
    }

    // Wait for leader election
    sleep(Duration::from_millis(500)).await;

    let leader_node = nodes
        .iter()
        .find(|node| {
            let state = futures::executor::block_on(node.get_state());
            state.current_state == NodeState::Leader
        })
        .unwrap();

    // Test 1: Replicate multiple entries
    let test_entries = vec![
        ("key1", "value1"),
        ("key2", "value2"),
        ("key3", "value3"),
        ("key4", "value4"),
        ("key5", "value5"),
    ];

    let mut entry_indices = Vec::new();
    for (key, value) in &test_entries {
        let command = Command::Put {
            key: key.as_bytes().to_vec(),
            value: value.as_bytes().to_vec(),
        };
        
        let index = leader_node.append_entry(command).await.unwrap();
        entry_indices.push(index);
    }

    // Test 2: Wait for replication to complete
    sleep(Duration::from_millis(500)).await;

    // Test 3: All nodes should have all entries
    for (i, node) in nodes.iter().enumerate() {
        let log_state = node.get_log_state().await;
        assert_eq!(
            log_state.last_log_index,
            test_entries.len() as u64,
            "Node {} has incorrect last_log_index",
            i
        );
        assert!(
            log_state.commit_index >= test_entries.len() as u64,
            "Node {} has entries not committed",
            i
        );
    }

    // Test 4: Test log consistency across nodes
    let leader_log = leader_node.get_log_entries(1, test_entries.len() as u64).await.unwrap();
    
    for (i, node) in nodes.iter().enumerate() {
        if node as *const _ == leader_node as *const _ {
            continue; // Skip leader comparison with itself
        }
        
        let follower_log = node.get_log_entries(1, test_entries.len() as u64).await.unwrap();
        assert_eq!(
            leader_log.len(),
            follower_log.len(),
            "Node {} has different log length",
            i
        );
        
        for (j, (leader_entry, follower_entry)) in leader_log.iter().zip(follower_log.iter()).enumerate() {
            assert_eq!(
                leader_entry.term,
                follower_entry.term,
                "Node {} entry {} has different term",
                i,
                j
            );
            assert_eq!(
                leader_entry.command,
                follower_entry.command,
                "Node {} entry {} has different command",
                i,
                j
            );
        }
    }
}

#[tokio::test]
async fn test_raft_network_partition_simulation() {
    let temp_dirs: Vec<_> = (0..5).map(|_| TempDir::new().unwrap()).collect();
    
    let nodes_config: HashMap<NodeId, NodeAddress> = (0..5)
        .map(|i| {
            (
                NodeId(format!("node{}", i)),
                NodeAddress(format!("127.0.0.1:901{}", i)),
            )
        })
        .collect();

    let cluster_config = ClusterConfig {
        nodes: nodes_config.clone(),
        heartbeat_interval: Duration::from_millis(50),
        election_timeout: Duration::from_millis(200),
    };

    // Start 5 nodes
    let mut nodes = Vec::new();
    for (i, (node_id, _)) in nodes_config.iter().enumerate() {
        let db_config = BlockDBConfig {
            data_dir: temp_dirs[i].path().to_string_lossy().to_string(),
            ..Default::default()
        };

        let node = RaftNode::new(
            node_id.clone(),
            cluster_config.clone(),
            db_config,
        ).await.unwrap();
        
        nodes.push(node);
    }

    // Wait for initial leader election
    sleep(Duration::from_millis(500)).await;

    // Test 1: Verify initial state
    let initial_leader = nodes
        .iter()
        .find(|node| {
            let state = futures::executor::block_on(node.get_state());
            state.current_state == NodeState::Leader
        })
        .unwrap();

    let initial_term = initial_leader.get_state().await.current_term;

    // Test 2: Append some entries before partition
    for i in 0..3 {
        let command = Command::Put {
            key: format!("pre_partition_key_{}", i).into_bytes(),
            value: format!("pre_partition_value_{}", i).into_bytes(),
        };
        initial_leader.append_entry(command).await.unwrap();
    }

    sleep(Duration::from_millis(200)).await;

    // Test 3: Simulate network partition (isolate leader)
    // In a real test, we would disconnect the leader from other nodes
    // For this test, we simulate by checking that followers will elect a new leader
    // when they don't receive heartbeats
    
    // Stop the initial leader (simulate network partition)
    let leader_id = initial_leader.get_node_id().clone();
    initial_leader.stop().await;

    // Test 4: Wait for new leader election among remaining nodes
    sleep(Duration::from_millis(800)).await; // Allow time for election timeout and new election

    // Test 5: Check that a new leader was elected
    let remaining_nodes: Vec<_> = nodes
        .iter()
        .filter(|node| node.get_node_id() != &leader_id)
        .collect();

    let new_leader = remaining_nodes
        .iter()
        .find(|node| {
            let state = futures::executor::block_on(node.get_state());
            state.current_state == NodeState::Leader
        });

    assert!(new_leader.is_some(), "No new leader elected after partition");

    let new_leader = new_leader.unwrap();
    let new_term = new_leader.get_state().await.current_term;
    
    // Test 6: New term should be higher than initial term
    assert!(new_term > initial_term, "New leader should have higher term");

    // Test 7: New leader should be able to commit new entries
    let command = Command::Put {
        key: b"post_partition_key".to_vec(),
        value: b"post_partition_value".to_vec(),
    };

    let new_entry_index = new_leader.append_entry(command).await.unwrap();
    
    sleep(Duration::from_millis(300)).await;

    // Test 8: Majority of remaining nodes should commit the new entry
    let mut commit_count = 0;
    for node in &remaining_nodes {
        let log_state = node.get_log_state().await;
        if log_state.commit_index >= new_entry_index {
            commit_count += 1;
        }
    }
    
    assert!(commit_count >= (remaining_nodes.len() + 1) / 2, "Majority should commit new entry");
}

#[tokio::test]
async fn test_raft_safety_properties() {
    let temp_dirs: Vec<_> = (0..3).map(|_| TempDir::new().unwrap()).collect();
    
    let nodes_config: HashMap<NodeId, NodeAddress> = (0..3)
        .map(|i| {
            (
                NodeId(format!("safety_node{}", i)),
                NodeAddress(format!("127.0.0.1:902{}", i)),
            )
        })
        .collect();

    let cluster_config = ClusterConfig {
        nodes: nodes_config.clone(),
        heartbeat_interval: Duration::from_millis(50),
        election_timeout: Duration::from_millis(150),
    };

    let mut nodes = Vec::new();
    for (i, (node_id, _)) in nodes_config.iter().enumerate() {
        let db_config = BlockDBConfig {
            data_dir: temp_dirs[i].path().to_string_lossy().to_string(),
            ..Default::default()
        };

        let node = RaftNode::new(
            node_id.clone(),
            cluster_config.clone(),
            db_config,
        ).await.unwrap();
        
        nodes.push(node);
    }

    sleep(Duration::from_millis(500)).await;

    // Test 1: Election Safety - at most one leader per term
    let mut term_leaders: HashMap<u64, Vec<NodeId>> = HashMap::new();
    
    for node in &nodes {
        let state = node.get_state().await;
        if state.current_state == NodeState::Leader {
            term_leaders
                .entry(state.current_term)
                .or_insert_with(Vec::new)
                .push(node.get_node_id().clone());
        }
    }

    for (term, leaders) in term_leaders {
        assert_eq!(
            leaders.len(),
            1,
            "Term {} has {} leaders, should have exactly 1",
            term,
            leaders.len()
        );
    }

    // Test 2: Log Matching Property - logs are consistent across nodes
    let leader = nodes
        .iter()
        .find(|node| {
            let state = futures::executor::block_on(node.get_state());
            state.current_state == NodeState::Leader
        })
        .unwrap();

    // Add several entries
    for i in 0..5 {
        let command = Command::Put {
            key: format!("safety_key_{}", i).into_bytes(),
            value: format!("safety_value_{}", i).into_bytes(),
        };
        leader.append_entry(command).await.unwrap();
    }

    sleep(Duration::from_millis(300)).await;

    // Verify log consistency
    let leader_log = leader.get_log_entries(1, 5).await.unwrap();
    
    for (i, node) in nodes.iter().enumerate() {
        let node_log = node.get_log_entries(1, 5).await.unwrap();
        let log_state = node.get_log_state().await;
        
        // Check that committed entries are identical
        for j in 0..std::cmp::min(log_state.commit_index as usize, leader_log.len()) {
            assert_eq!(
                leader_log[j].term,
                node_log[j].term,
                "Node {} entry {} has different term",
                i,
                j
            );
            assert_eq!(
                leader_log[j].command,
                node_log[j].command,
                "Node {} entry {} has different command",
                i,
                j
            );
        }
    }

    // Test 3: Leader Completeness - leader has all committed entries
    let leader_state = leader.get_log_state().await;
    
    for node in &nodes {
        let node_state = node.get_log_state().await;
        
        // Leader should have at least as many entries as any follower has committed
        assert!(
            leader_state.last_log_index >= node_state.commit_index,
            "Leader missing entries that are committed on followers"
        );
    }
}

#[tokio::test]
async fn test_raft_persistence() {
    let temp_dir = TempDir::new().unwrap();
    let node_id = NodeId("persistent_node".to_string());
    
    let cluster_config = ClusterConfig {
        nodes: vec![
            (node_id.clone(), NodeAddress("127.0.0.1:9025".to_string())),
        ].into_iter().collect(),
        heartbeat_interval: Duration::from_millis(100),
        election_timeout: Duration::from_millis(300),
    };

    let db_config = BlockDBConfig {
        data_dir: temp_dir.path().to_string_lossy().to_string(),
        ..Default::default()
    };

    // Test 1: Create node and add entries
    let initial_entries = vec![
        ("persistent_key_1", "persistent_value_1"),
        ("persistent_key_2", "persistent_value_2"),
        ("persistent_key_3", "persistent_value_3"),
    ];

    {
        let node = RaftNode::new(
            node_id.clone(),
            cluster_config.clone(),
            db_config.clone(),
        ).await.unwrap();

        sleep(Duration::from_millis(200)).await; // Become leader

        for (key, value) in &initial_entries {
            let command = Command::Put {
                key: key.as_bytes().to_vec(),
                value: value.as_bytes().to_vec(),
            };
            node.append_entry(command).await.unwrap();
        }

        sleep(Duration::from_millis(200)).await; // Ensure entries are committed
        
        let final_state = node.get_state().await;
        assert_eq!(final_state.current_state, NodeState::Leader);
        assert!(final_state.current_term >= 1);
    } // Node goes out of scope

    // Test 2: Restart node and verify persistence
    {
        let restarted_node = RaftNode::new(
            node_id.clone(),
            cluster_config.clone(),
            db_config.clone(),
        ).await.unwrap();

        sleep(Duration::from_millis(200)).await; // Allow initialization

        let state = restarted_node.get_state().await;
        let log_state = restarted_node.get_log_state().await;

        // Test 3: State should be restored
        assert_eq!(state.current_state, NodeState::Leader); // Single node becomes leader
        assert!(state.current_term >= 1);

        // Test 4: Log should be restored
        assert_eq!(log_state.last_log_index, initial_entries.len() as u64);
        
        let restored_log = restarted_node.get_log_entries(1, initial_entries.len() as u64).await.unwrap();
        assert_eq!(restored_log.len(), initial_entries.len());

        // Test 5: Verify log contents
        for (i, (expected_key, expected_value)) in initial_entries.iter().enumerate() {
            match &restored_log[i].command {
                Command::Put { key, value } => {
                    assert_eq!(key, &expected_key.as_bytes().to_vec());
                    assert_eq!(value, &expected_value.as_bytes().to_vec());
                }
                _ => panic!("Unexpected command type in restored log"),
            }
        }

        // Test 6: Node should still be functional after restart
        let new_command = Command::Put {
            key: b"post_restart_key".to_vec(),
            value: b"post_restart_value".to_vec(),
        };
        
        let new_index = restarted_node.append_entry(new_command).await.unwrap();
        assert_eq!(new_index, initial_entries.len() as u64 + 1);
    }
}

// Helper trait for testing (these methods would need to be implemented on RaftNode)
trait RaftNodeTestInterface {
    async fn get_state(&self) -> TestNodeState;
    async fn get_log_state(&self) -> TestLogState;
    async fn append_entry(&self, command: Command) -> Result<u64, String>;
    async fn get_log_entries(&self, start: u64, count: u64) -> Result<Vec<LogEntry>, String>;
    async fn stop(&self);
    fn get_node_id(&self) -> &NodeId;
}

#[derive(Debug, PartialEq)]
struct TestNodeState {
    current_state: NodeState,
    current_term: u64,
    voted_for: Option<NodeId>,
}

#[derive(Debug)]
struct TestLogState {
    last_log_index: u64,
    commit_index: u64,
    last_applied: u64,
}

// Note: These tests assume that the RaftNode implementation provides the necessary
// test interfaces. In a real implementation, these would be actual methods on RaftNode
// or exposed through a testing interface.