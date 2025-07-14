use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{RwLock, Mutex, mpsc};
use serde::{Serialize, Deserialize};
use uuid::Uuid;

pub mod raft;
pub mod log_entry;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeId(pub String);

impl NodeId {
    pub fn new() -> Self {
        NodeId(Uuid::new_v4().to_string())
    }
    
    pub fn from_string(s: String) -> Self {
        NodeId(s)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeAddress {
    pub host: String,
    pub port: u16,
}

impl NodeAddress {
    pub fn new(host: String, port: u16) -> Self {
        NodeAddress { host, port }
    }
    
    pub fn to_url(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterConfig {
    pub node_id: NodeId,
    pub address: NodeAddress,
    pub peers: HashMap<NodeId, NodeAddress>,
    pub heartbeat_interval: Duration,
    pub election_timeout: Duration,
}

impl ClusterConfig {
    pub fn new(node_id: NodeId, address: NodeAddress) -> Self {
        ClusterConfig {
            node_id,
            address,
            peers: HashMap::new(),
            heartbeat_interval: Duration::from_millis(150),
            election_timeout: Duration::from_millis(300),
        }
    }
    
    pub fn add_peer(&mut self, node_id: NodeId, address: NodeAddress) {
        self.peers.insert(node_id, address);
    }
    
    pub fn cluster_size(&self) -> usize {
        self.peers.len() + 1 // +1 for this node
    }
    
    pub fn majority(&self) -> usize {
        (self.cluster_size() / 2) + 1
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RaftState {
    Follower,
    Candidate,
    Leader,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Term(pub u64);

impl Term {
    pub fn new() -> Self {
        Term(0)
    }
    
    pub fn increment(&mut self) {
        self.0 += 1;
    }
    
    pub fn update(&mut self, other: &Term) -> bool {
        if other.0 > self.0 {
            self.0 = other.0;
            true
        } else {
            false
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConsensusMessage {
    RequestVote {
        term: Term,
        candidate_id: NodeId,
        last_log_index: u64,
        last_log_term: Term,
    },
    RequestVoteResponse {
        term: Term,
        vote_granted: bool,
    },
    AppendEntries {
        term: Term,
        leader_id: NodeId,
        prev_log_index: u64,
        prev_log_term: Term,
        entries: Vec<log_entry::LogEntry>,
        leader_commit: u64,
    },
    AppendEntriesResponse {
        term: Term,
        success: bool,
        match_index: u64,
    },
    ClientRequest {
        operation: crate::transaction::Operation,
        request_id: Uuid,
    },
    ClientResponse {
        request_id: Uuid,
        success: bool,
        error: Option<String>,
    },
}

pub trait ConsensusEngine: Send + Sync {
    async fn propose(&self, operation: crate::transaction::Operation) -> Result<(), crate::error::BlockDBError>;
    async fn is_leader(&self) -> bool;
    async fn get_leader(&self) -> Option<NodeId>;
    async fn add_node(&self, node_id: NodeId, address: NodeAddress) -> Result<(), crate::error::BlockDBError>;
    async fn remove_node(&self, node_id: &NodeId) -> Result<(), crate::error::BlockDBError>;
}

#[derive(Debug)]
pub struct ConsensusState {
    pub state: RaftState,
    pub current_term: Term,
    pub voted_for: Option<NodeId>,
    pub last_heartbeat: SystemTime,
    pub leader_id: Option<NodeId>,
}

impl ConsensusState {
    pub fn new() -> Self {
        ConsensusState {
            state: RaftState::Follower,
            current_term: Term::new(),
            voted_for: None,
            last_heartbeat: SystemTime::now(),
            leader_id: None,
        }
    }
    
    pub fn become_follower(&mut self, term: Term, leader_id: Option<NodeId>) {
        self.state = RaftState::Follower;
        self.current_term = term;
        self.voted_for = None;
        self.leader_id = leader_id;
        self.last_heartbeat = SystemTime::now();
    }
    
    pub fn become_candidate(&mut self) {
        self.state = RaftState::Candidate;
        self.current_term.increment();
        self.voted_for = None;
        self.leader_id = None;
        self.last_heartbeat = SystemTime::now();
    }
    
    pub fn become_leader(&mut self, node_id: NodeId) {
        self.state = RaftState::Leader;
        self.leader_id = Some(node_id);
        self.voted_for = None;
        self.last_heartbeat = SystemTime::now();
    }
    
    pub fn is_election_timeout(&self, timeout: Duration) -> bool {
        SystemTime::now()
            .duration_since(self.last_heartbeat)
            .unwrap_or(Duration::ZERO) > timeout
    }
}