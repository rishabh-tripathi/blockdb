use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::{RwLock, Mutex, mpsc};
use tokio::time::{interval, sleep, timeout};
use async_trait::async_trait;
use rand::Rng;
use serde::{Serialize, Deserialize};
use uuid::Uuid;

use crate::consensus::{
    NodeId, NodeAddress, ClusterConfig, RaftState, Term, ConsensusMessage, 
    ConsensusEngine, ConsensusState
};
use crate::consensus::log_entry::{LogEntry, LogOperation, ReplicatedLog};
use crate::error::BlockDBError;
use crate::transaction::Operation;

pub struct RaftNode {
    config: ClusterConfig,
    state: Arc<RwLock<ConsensusState>>,
    log: Arc<RwLock<ReplicatedLog>>,
    next_index: Arc<RwLock<HashMap<NodeId, u64>>>,
    match_index: Arc<RwLock<HashMap<NodeId, u64>>>,
    message_sender: mpsc::UnboundedSender<(NodeId, ConsensusMessage)>,
    message_receiver: Arc<Mutex<mpsc::UnboundedReceiver<(NodeId, ConsensusMessage)>>>,
    client_requests: Arc<Mutex<HashMap<Uuid, tokio::sync::oneshot::Sender<Result<(), BlockDBError>>>>>,
    is_running: Arc<RwLock<bool>>,
}

impl RaftNode {
    pub fn new(config: ClusterConfig) -> Self {
        let (message_sender, message_receiver) = mpsc::unbounded_channel();
        
        RaftNode {
            config,
            state: Arc::new(RwLock::new(ConsensusState::new())),
            log: Arc::new(RwLock::new(ReplicatedLog::new())),
            next_index: Arc::new(RwLock::new(HashMap::new())),
            match_index: Arc::new(RwLock::new(HashMap::new())),
            message_sender,
            message_receiver: Arc::new(Mutex::new(message_receiver)),
            client_requests: Arc::new(Mutex::new(HashMap::new())),
            is_running: Arc::new(RwLock::new(false)),
        }
    }
    
    pub async fn start(&self) -> Result<(), BlockDBError> {
        *self.is_running.write().await = true;
        
        // Initialize peer indices
        {
            let mut next_index = self.next_index.write().await;
            let mut match_index = self.match_index.write().await;
            let log = self.log.read().await;
            let last_log_index = log.last_log_index();
            
            for peer_id in self.config.peers.keys() {
                next_index.insert(peer_id.clone(), last_log_index + 1);
                match_index.insert(peer_id.clone(), 0);
            }
        }
        
        // Start main Raft loop
        let self_clone = self.clone();
        tokio::spawn(async move {
            self_clone.run().await;
        });
        
        Ok(())
    }
    
    pub async fn stop(&self) {
        *self.is_running.write().await = false;
    }
    
    async fn run(&self) {
        let mut election_timer = interval(self.random_election_timeout());
        let mut heartbeat_timer = interval(self.config.heartbeat_interval);
        
        loop {
            if !*self.is_running.read().await {
                break;
            }
            
            tokio::select! {
                // Handle incoming messages
                message = self.receive_message() => {
                    if let Some((sender, msg)) = message {
                        self.handle_message(sender, msg).await;
                    }
                }
                
                // Election timeout
                _ = election_timer.tick() => {
                    let state = self.state.read().await;
                    if matches!(state.state, RaftState::Follower | RaftState::Candidate) {
                        if state.is_election_timeout(self.random_election_timeout()) {
                            drop(state);
                            self.start_election().await;
                        }
                    }
                }
                
                // Heartbeat (if leader)
                _ = heartbeat_timer.tick() => {
                    let state = self.state.read().await;
                    if matches!(state.state, RaftState::Leader) {
                        drop(state);
                        self.send_heartbeats().await;
                    }
                }
            }
        }
    }
    
    async fn handle_message(&self, sender: NodeId, message: ConsensusMessage) {
        match message {
            ConsensusMessage::RequestVote { term, candidate_id, last_log_index, last_log_term } => {
                self.handle_request_vote(sender, term, candidate_id, last_log_index, last_log_term).await;
            }
            ConsensusMessage::RequestVoteResponse { term, vote_granted } => {
                self.handle_vote_response(sender, term, vote_granted).await;
            }
            ConsensusMessage::AppendEntries { term, leader_id, prev_log_index, prev_log_term, entries, leader_commit } => {
                self.handle_append_entries(sender, term, leader_id, prev_log_index, prev_log_term, entries, leader_commit).await;
            }
            ConsensusMessage::AppendEntriesResponse { term, success, match_index } => {
                self.handle_append_entries_response(sender, term, success, match_index).await;
            }
            ConsensusMessage::ClientRequest { operation, request_id } => {
                self.handle_client_request(operation, request_id).await;
            }
            ConsensusMessage::ClientResponse { request_id, success, error } => {
                self.handle_client_response(request_id, success, error).await;
            }
        }
    }
    
    async fn handle_request_vote(&self, sender: NodeId, term: Term, candidate_id: NodeId, last_log_index: u64, last_log_term: Term) {
        let mut state = self.state.write().await;
        let log = self.log.read().await;
        
        let mut vote_granted = false;
        
        // Update term if newer
        if term.0 > state.current_term.0 {
            state.current_term = term.clone();
            state.voted_for = None;
            state.state = RaftState::Follower;
        }
        
        // Grant vote if conditions are met
        if term.0 == state.current_term.0 &&
           (state.voted_for.is_none() || state.voted_for.as_ref() == Some(&candidate_id)) &&
           log.is_up_to_date(last_log_index, &last_log_term) {
            vote_granted = true;
            state.voted_for = Some(candidate_id);
            state.last_heartbeat = SystemTime::now();
        }
        
        let response = ConsensusMessage::RequestVoteResponse {
            term: state.current_term.clone(),
            vote_granted,
        };
        
        drop(state);
        drop(log);
        
        self.send_message(sender, response).await;
    }
    
    async fn handle_vote_response(&self, sender: NodeId, term: Term, vote_granted: bool) {
        let mut state = self.state.write().await;
        
        // Update term if newer
        if term.0 > state.current_term.0 {
            state.current_term = term;
            state.state = RaftState::Follower;
            state.voted_for = None;
            return;
        }
        
        // Only process if we're a candidate in the same term
        if matches!(state.state, RaftState::Candidate) && term.0 == state.current_term.0 && vote_granted {
            // Count votes (including our own)
            let votes_received = 1; // TODO: Track actual votes received
            
            if votes_received >= self.config.majority() {
                // Become leader
                state.become_leader(self.config.node_id.clone());
                drop(state);
                
                // Initialize leader state
                self.initialize_leader_state().await;
                
                // Send initial heartbeats
                self.send_heartbeats().await;
            }
        }
    }
    
    async fn handle_append_entries(&self, sender: NodeId, term: Term, leader_id: NodeId, prev_log_index: u64, prev_log_term: Term, entries: Vec<LogEntry>, leader_commit: u64) {
        let mut state = self.state.write().await;
        let mut log = self.log.write().await;
        
        let mut success = false;
        
        // Update term if newer
        if term.0 > state.current_term.0 {
            state.current_term = term.clone();
            state.state = RaftState::Follower;
            state.voted_for = None;
        }
        
        // Accept leader if term is current
        if term.0 == state.current_term.0 {
            state.state = RaftState::Follower;
            state.leader_id = Some(leader_id);
            state.last_heartbeat = SystemTime::now();
            
            // Check log consistency
            if prev_log_index == 0 || 
               (prev_log_index <= log.last_log_index() && 
                log.get_term(prev_log_index).map_or(false, |t| t.0 == prev_log_term.0)) {
                
                // Append entries
                success = log.append_entries(prev_log_index, entries);
                
                if success {
                    // Update commit index
                    let new_commit_index = std::cmp::min(leader_commit, log.last_log_index());
                    log.update_commit_index(new_commit_index);
                }
            }
        }
        
        let response = ConsensusMessage::AppendEntriesResponse {
            term: state.current_term.clone(),
            success,
            match_index: if success { log.last_log_index() } else { 0 },
        };
        
        drop(state);
        drop(log);
        
        self.send_message(sender, response).await;
    }
    
    async fn handle_append_entries_response(&self, sender: NodeId, term: Term, success: bool, match_index: u64) {
        let mut state = self.state.write().await;
        
        // Update term if newer
        if term.0 > state.current_term.0 {
            state.current_term = term;
            state.state = RaftState::Follower;
            state.voted_for = None;
            return;
        }
        
        // Only process if we're the leader
        if !matches!(state.state, RaftState::Leader) || term.0 != state.current_term.0 {
            return;
        }
        
        drop(state);
        
        if success {
            // Update match and next indices
            let mut next_index = self.next_index.write().await;
            let mut match_index_map = self.match_index.write().await;
            
            match_index_map.insert(sender.clone(), match_index);
            next_index.insert(sender, match_index + 1);
            
            drop(next_index);
            drop(match_index_map);
            
            // Update commit index
            self.update_commit_index().await;
        } else {
            // Decrement next index and retry
            let mut next_index = self.next_index.write().await;
            if let Some(index) = next_index.get_mut(&sender) {
                *index = (*index).saturating_sub(1).max(1);
            }
        }
    }
    
    async fn handle_client_request(&self, operation: Operation, request_id: Uuid) {
        let state = self.state.read().await;
        
        if !matches!(state.state, RaftState::Leader) {
            // Not the leader, reject request
            let response = ConsensusMessage::ClientResponse {
                request_id,
                success: false,
                error: Some("Not the leader".to_string()),
            };
            
            // TODO: Send to client
            return;
        }
        
        drop(state);
        
        // Convert operation to log operation
        let log_operation = match operation {
            Operation::Put { key, value } => LogOperation::Put { key, value },
            _ => {
                // For now, only support Put operations in consensus
                let response = ConsensusMessage::ClientResponse {
                    request_id,
                    success: false,
                    error: Some("Operation not supported".to_string()),
                };
                return;
            }
        };
        
        // Add to log
        let mut log = self.log.write().await;
        let state = self.state.read().await;
        
        let entry = LogEntry::new(
            log.last_log_index() + 1,
            state.current_term.clone(),
            log_operation,
            request_id,
        );
        
        log.append(entry);
        
        drop(log);
        drop(state);
        
        // Store client request for response
        let (tx, rx) = tokio::sync::oneshot::channel();
        {
            let mut client_requests = self.client_requests.lock().await;
            client_requests.insert(request_id, tx);
        }
        
        // Replicate to followers
        self.replicate_to_followers().await;
        
        // Wait for commitment or timeout
        let timeout_duration = Duration::from_secs(5);
        match timeout(timeout_duration, rx).await {
            Ok(Ok(())) => {
                let response = ConsensusMessage::ClientResponse {
                    request_id,
                    success: true,
                    error: None,
                };
                // TODO: Send to client
            }
            _ => {
                let response = ConsensusMessage::ClientResponse {
                    request_id,
                    success: false,
                    error: Some("Request timeout".to_string()),
                };
                // TODO: Send to client
            }
        }
    }
    
    async fn handle_client_response(&self, request_id: Uuid, success: bool, error: Option<String>) {
        let mut client_requests = self.client_requests.lock().await;
        
        if let Some(sender) = client_requests.remove(&request_id) {
            let result = if success {
                Ok(())
            } else {
                Err(BlockDBError::StorageError(error.unwrap_or("Unknown error".to_string())))
            };
            
            let _ = sender.send(result);
        }
    }
    
    async fn start_election(&self) {
        let mut state = self.state.write().await;
        let log = self.log.read().await;
        
        state.become_candidate();
        
        // Vote for self
        state.voted_for = Some(self.config.node_id.clone());
        
        let request = ConsensusMessage::RequestVote {
            term: state.current_term.clone(),
            candidate_id: self.config.node_id.clone(),
            last_log_index: log.last_log_index(),
            last_log_term: log.last_log_term(),
        };
        
        drop(state);
        drop(log);
        
        // Send vote requests to all peers
        for peer_id in self.config.peers.keys() {
            self.send_message(peer_id.clone(), request.clone()).await;
        }
    }
    
    async fn send_heartbeats(&self) {
        self.replicate_to_followers().await;
    }
    
    async fn replicate_to_followers(&self) {
        let state = self.state.read().await;
        
        if !matches!(state.state, RaftState::Leader) {
            return;
        }
        
        let current_term = state.current_term.clone();
        let leader_id = self.config.node_id.clone();
        
        drop(state);
        
        let log = self.log.read().await;
        let next_index = self.next_index.read().await;
        let leader_commit = log.get_commit_index();
        
        for (peer_id, &next_idx) in next_index.iter() {
            let prev_log_index = next_idx.saturating_sub(1);
            let prev_log_term = if prev_log_index == 0 {
                Term::new()
            } else {
                log.get_term(prev_log_index).unwrap_or(Term::new())
            };
            
            let entries = log.get_entries_from(next_idx);
            
            let message = ConsensusMessage::AppendEntries {
                term: current_term.clone(),
                leader_id: leader_id.clone(),
                prev_log_index,
                prev_log_term,
                entries,
                leader_commit,
            };
            
            self.send_message(peer_id.clone(), message).await;
        }
    }
    
    async fn initialize_leader_state(&self) {
        let mut next_index = self.next_index.write().await;
        let mut match_index = self.match_index.write().await;
        let log = self.log.read().await;
        
        let last_log_index = log.last_log_index();
        
        for peer_id in self.config.peers.keys() {
            next_index.insert(peer_id.clone(), last_log_index + 1);
            match_index.insert(peer_id.clone(), 0);
        }
    }
    
    async fn update_commit_index(&self) {
        let log = self.log.read().await;
        let match_index = self.match_index.read().await;
        let state = self.state.read().await;
        
        if !matches!(state.state, RaftState::Leader) {
            return;
        }
        
        let current_commit = log.get_commit_index();
        let last_log_index = log.last_log_index();
        
        // Find highest index that majority of servers have replicated
        for n in (current_commit + 1)..=last_log_index {
            let mut count = 1; // Count self
            
            for match_idx in match_index.values() {
                if *match_idx >= n {
                    count += 1;
                }
            }
            
            if count >= self.config.majority() {
                // Check that the entry is from current term
                if let Some(term) = log.get_term(n) {
                    if term.0 == state.current_term.0 {
                        drop(log);
                        drop(match_index);
                        drop(state);
                        
                        // Update commit index
                        let mut log = self.log.write().await;
                        log.update_commit_index(n);
                        
                        // Notify committed requests
                        self.notify_committed_requests(&*log).await;
                        return;
                    }
                }
            }
        }
    }
    
    async fn notify_committed_requests(&self, log: &ReplicatedLog) {
        let last_applied = log.get_last_applied();
        let commit_index = log.get_commit_index();
        
        for index in (last_applied + 1)..=commit_index {
            if let Some(entry) = log.get_entry(index) {
                let mut client_requests = self.client_requests.lock().await;
                if let Some(sender) = client_requests.remove(&entry.request_id) {
                    let _ = sender.send(Ok(()));
                }
            }
        }
    }
    
    fn random_election_timeout(&self) -> Duration {
        let base = self.config.election_timeout;
        let jitter = rand::thread_rng().gen_range(0..base.as_millis() as u64);
        base + Duration::from_millis(jitter)
    }
    
    async fn receive_message(&self) -> Option<(NodeId, ConsensusMessage)> {
        let mut receiver = self.message_receiver.lock().await;
        receiver.recv().await
    }
    
    async fn send_message(&self, target: NodeId, message: ConsensusMessage) {
        // In a real implementation, this would send over the network
        // For now, just send to local channel for testing
        let _ = self.message_sender.send((target, message));
    }
}

impl Clone for RaftNode {
    fn clone(&self) -> Self {
        // Create new channels for the clone
        let (message_sender, message_receiver) = mpsc::unbounded_channel();
        
        RaftNode {
            config: self.config.clone(),
            state: self.state.clone(),
            log: self.log.clone(),
            next_index: self.next_index.clone(),
            match_index: self.match_index.clone(),
            message_sender,
            message_receiver: Arc::new(Mutex::new(message_receiver)),
            client_requests: self.client_requests.clone(),
            is_running: self.is_running.clone(),
        }
    }
}

#[async_trait]
impl ConsensusEngine for RaftNode {
    async fn propose(&self, operation: Operation) -> Result<(), BlockDBError> {
        let request_id = Uuid::new_v4();
        
        let message = ConsensusMessage::ClientRequest {
            operation,
            request_id,
        };
        
        self.handle_client_request(
            match message {
                ConsensusMessage::ClientRequest { operation, .. } => operation,
                _ => unreachable!(),
            },
            request_id,
        ).await;
        
        Ok(())
    }
    
    async fn is_leader(&self) -> bool {
        let state = self.state.read().await;
        matches!(state.state, RaftState::Leader)
    }
    
    async fn get_leader(&self) -> Option<NodeId> {
        let state = self.state.read().await;
        state.leader_id.clone()
    }
    
    async fn add_node(&self, node_id: NodeId, address: NodeAddress) -> Result<(), BlockDBError> {
        // TODO: Implement cluster membership changes
        Ok(())
    }
    
    async fn remove_node(&self, node_id: &NodeId) -> Result<(), BlockDBError> {
        // TODO: Implement cluster membership changes
        Ok(())
    }
}