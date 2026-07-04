use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::{RwLock, Mutex, mpsc};
use tokio::time::{interval, timeout};
use async_trait::async_trait;
use rand::Rng;
use uuid::Uuid;

use crate::consensus::{
    NodeId, NodeAddress, ClusterConfig, RaftState, Term, ConsensusMessage,
    ConsensusEngine, ConsensusState, StateMachine,
};
use crate::consensus::log_entry::{LogEntry, LogOperation, ReplicatedLog};
use crate::consensus::transport::{NoopTransport, RaftTransport};
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
    /// Delivers outbound messages to peers (HTTP in production, no-op for local tests).
    transport: Arc<dyn RaftTransport>,
    /// Applies committed log entries to the replicated store.
    state_machine: Arc<dyn StateMachine>,
    /// Peers that granted their vote in the current election (leader elected at majority).
    votes_received: Arc<Mutex<HashSet<NodeId>>>,
}

impl RaftNode {
    /// Construct a node with an explicit transport and state machine (the real
    /// multi-node configuration).
    pub fn new(
        config: ClusterConfig,
        transport: Arc<dyn RaftTransport>,
        state_machine: Arc<dyn StateMachine>,
    ) -> Self {
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
            transport,
            state_machine,
            votes_received: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Construct a node with a no-op transport and no-op state machine. Useful for
    /// smoke tests of the local state machine without networking or storage.
    pub fn new_local(config: ClusterConfig) -> Self {
        Self::new(config, Arc::new(NoopTransport), Arc::new(NoopStateMachine))
    }

    /// A handle for injecting messages received over the network into this node's
    /// processing queue (used by the cluster server's `/raft/message` endpoint).
    pub fn message_injector(&self) -> mpsc::UnboundedSender<(NodeId, ConsensusMessage)> {
        self.message_sender.clone()
    }

    /// Inject a message received from `from` into the processing queue.
    pub fn inject(&self, from: NodeId, message: ConsensusMessage) {
        let _ = self.message_sender.send((from, message));
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
                let _ = self.handle_client_request(operation, request_id).await;
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
            // Record this peer's vote and count the distinct votes (including our own).
            let votes_received = {
                let mut votes = self.votes_received.lock().await;
                votes.insert(sender);
                votes.len()
            };

            if votes_received >= self.config.majority() {
                // Won the election: become leader for this term.
                state.become_leader(self.config.node_id.clone());
                drop(state);

                self.initialize_leader_state().await;
                // Immediately assert leadership with heartbeats so followers stop
                // their election timers.
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

        // If the leader advanced our commit index, apply the newly-committed entries
        // to our local state machine so this follower stays consistent with the leader.
        if success {
            self.apply_committed().await;
        }

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
    
    async fn handle_client_request(&self, operation: Operation, request_id: Uuid) -> Result<(), BlockDBError> {
        {
            let state = self.state.read().await;
            if !matches!(state.state, RaftState::Leader) {
                return Err(BlockDBError::ConsensusError("not the leader".to_string()));
            }
        }

        // Only writes go through the replicated log.
        let log_operation = match operation {
            Operation::Put { key, value } => LogOperation::Put { key, value },
            other => {
                return Err(BlockDBError::ConsensusError(format!(
                    "operation not supported by consensus: {:?}",
                    other
                )))
            }
        };

        // Append the entry to the leader's log.
        {
            let mut log = self.log.write().await;
            let state = self.state.read().await;
            let entry = LogEntry::new(
                log.last_log_index() + 1,
                state.current_term.clone(),
                log_operation,
                request_id,
            );
            log.append(entry);
        }

        // Register a waiter that apply_committed() will signal once this entry commits.
        let (tx, rx) = tokio::sync::oneshot::channel();
        {
            let mut client_requests = self.client_requests.lock().await;
            client_requests.insert(request_id, tx);
        }

        // Replicate to followers, then try to advance the commit index. For a
        // single-node cluster (majority == 1) this commits and applies immediately;
        // for a multi-node cluster the commit happens once a majority acknowledges.
        self.replicate_to_followers().await;
        self.update_commit_index().await;

        // Wait for the entry to be committed and applied (or time out).
        let timeout_duration = self.config.election_timeout * 10;
        match timeout(timeout_duration, rx).await {
            Ok(Ok(Ok(()))) => Ok(()),
            Ok(Ok(Err(e))) => Err(e),
            _ => {
                // Clean up the waiter so a late commit doesn't try to signal a dropped rx.
                self.client_requests.lock().await.remove(&request_id);
                Err(BlockDBError::ConsensusError(
                    "timed out waiting for entry to commit".to_string(),
                ))
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

        // Vote for self and reset the vote tally for this new election term.
        state.voted_for = Some(self.config.node_id.clone());
        {
            let mut votes = self.votes_received.lock().await;
            votes.clear();
            votes.insert(self.config.node_id.clone());
        }

        // A single-node cluster wins its own election immediately.
        if self.config.majority() <= 1 {
            state.become_leader(self.config.node_id.clone());
            drop(state);
            self.initialize_leader_state().await;
            return;
        }

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
        let mut highest_committable: Option<u64> = None;

        // Find the highest index that a majority of servers have replicated.
        for n in (current_commit + 1)..=last_log_index {
            let mut count = 1; // Count self
            
            for match_idx in match_index.values() {
                if *match_idx >= n {
                    count += 1;
                }
            }
            
            if count >= self.config.majority() {
                // Raft safety: a leader only commits entries from its *current* term
                // (older entries commit indirectly once a current-term entry does).
                if let Some(term) = log.get_term(n) {
                    if term.0 == state.current_term.0 {
                        highest_committable = Some(n);
                    }
                }
            }
        }

        drop(log);
        drop(match_index);
        drop(state);

        if let Some(n) = highest_committable {
            {
                let mut log = self.log.write().await;
                log.update_commit_index(n);
            }
            // Apply everything now committed to the state machine and wake waiters.
            self.apply_committed().await;
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
        // Messages addressed to ourselves loop back through the local queue; messages
        // to peers go over the transport (HTTP) to their `/raft/message` endpoint.
        if target == self.config.node_id {
            let _ = self.message_sender.send((self.config.node_id.clone(), message));
            return;
        }

        if let Some(address) = self.config.peers.get(&target) {
            let transport = self.transport.clone();
            let from = self.config.node_id.clone();
            let address = address.clone();
            // Fire-and-forget: don't block the state machine on network I/O.
            tokio::spawn(async move {
                transport.send(&address, from, message).await;
            });
        }
    }

    /// Apply all newly-committed log entries to the state machine, in order, and wake
    /// any client requests waiting on them. Idempotent: `apply_up_to` advances
    /// `last_applied` so each entry is applied exactly once across all callers.
    async fn apply_committed(&self) {
        let entries = {
            let mut log = self.log.write().await;
            let commit_index = log.get_commit_index();
            log.apply_up_to(commit_index)
        };

        for entry in entries {
            // Apply deterministically on every node. For an append-only store, a
            // duplicate key is rejected identically on all nodes, so the outcome stays
            // consistent cluster-wide; the proposing client receives that same result.
            let apply_result = self.state_machine.apply(&entry).await;
            if let Err(ref e) = apply_result {
                tracing::debug!(index = entry.index, error = %e, "state machine apply returned error");
            }

            // Wake the client that proposed this entry, if any (leader side).
            let mut client_requests = self.client_requests.lock().await;
            if let Some(sender) = client_requests.remove(&entry.request_id) {
                let _ = sender.send(apply_result);
            }
        }
    }
}

/// A state machine that ignores everything. Paired with `RaftNode::new_local`.
pub struct NoopStateMachine;

#[async_trait]
impl StateMachine for NoopStateMachine {
    async fn apply(&self, _entry: &LogEntry) -> Result<(), BlockDBError> {
        Ok(())
    }
}

impl Clone for RaftNode {
    fn clone(&self) -> Self {
        // Share the SAME message channel and all state so a clone (e.g. the one the
        // run loop is spawned on) sees messages injected via the original handle.
        // Cloning an mpsc sender points at the same channel; the receiver Arc is shared.
        RaftNode {
            config: self.config.clone(),
            state: self.state.clone(),
            log: self.log.clone(),
            next_index: self.next_index.clone(),
            match_index: self.match_index.clone(),
            message_sender: self.message_sender.clone(),
            message_receiver: self.message_receiver.clone(),
            client_requests: self.client_requests.clone(),
            is_running: self.is_running.clone(),
            transport: self.transport.clone(),
            state_machine: self.state_machine.clone(),
            votes_received: self.votes_received.clone(),
        }
    }
}

#[async_trait]
impl ConsensusEngine for RaftNode {
    async fn propose(&self, operation: Operation) -> Result<(), BlockDBError> {
        // Drive the write through the replicated log and wait for it to commit + apply.
        let request_id = Uuid::new_v4();
        self.handle_client_request(operation, request_id).await
    }
    
    async fn is_leader(&self) -> bool {
        let state = self.state.read().await;
        matches!(state.state, RaftState::Leader)
    }
    
    async fn get_leader(&self) -> Option<NodeId> {
        let state = self.state.read().await;
        state.leader_id.clone()
    }
    
    async fn add_node(&self, _node_id: NodeId, _address: NodeAddress) -> Result<(), BlockDBError> {
        // TODO: Implement cluster membership changes
        Ok(())
    }
    
    async fn remove_node(&self, _node_id: &NodeId) -> Result<(), BlockDBError> {
        // TODO: Implement cluster membership changes
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}