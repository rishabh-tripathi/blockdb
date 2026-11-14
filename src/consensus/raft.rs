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
use crate::consensus::persistence::{
    HardState, HardStateStore, LogStore, SnapshotMeta, SnapshotStore,
};
use crate::consensus::transport::{NoopTransport, RaftTransport};
use crate::error::BlockDBError;
use crate::transaction::Operation;

pub struct RaftNode {
    config: ClusterConfig,
    /// Active cluster membership (peer id -> address). Starts from `config.peers` and
    /// is mutated by committed AddNode/RemoveNode config entries, so the cluster can
    /// grow and shrink at runtime. All quorum math reads this, not `config.peers`.
    active_peers: Arc<RwLock<HashMap<NodeId, NodeAddress>>>,
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
    /// Durable store for `currentTerm`/`votedFor`. `None` for in-memory/test nodes.
    hard_state_store: Option<Arc<Mutex<HardStateStore>>>,
    /// Durable store for the log-compaction snapshot baseline + membership. `None` for
    /// in-memory/test nodes.
    snapshot_store: Option<Arc<Mutex<SnapshotStore>>>,
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

        let active_peers = config.peers.clone();
        RaftNode {
            config,
            active_peers: Arc::new(RwLock::new(active_peers)),
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
            hard_state_store: None,
            snapshot_store: None,
        }
    }

    /// Construct a node that durably persists its Raft log and hard state under
    /// `data_dir/raft/`, recovering both on startup. This is the production
    /// configuration — a restarted node resumes its term, vote, and committed log.
    pub fn new_persistent(
        config: ClusterConfig,
        transport: Arc<dyn RaftTransport>,
        state_machine: Arc<dyn StateMachine>,
        data_dir: &std::path::Path,
    ) -> Result<Self, BlockDBError> {
        let raft_dir = data_dir.join("raft");
        let (hs_store, hard) = HardStateStore::open(&raft_dir)
            .map_err(|e| BlockDBError::ConsensusError(format!("open raft hard state: {}", e)))?;
        let (snap_store, snap) = SnapshotStore::open(&raft_dir)
            .map_err(|e| BlockDBError::ConsensusError(format!("open raft snapshot: {}", e)))?;
        let (log_store, all_entries) = LogStore::open(&raft_dir)
            .map_err(|e| BlockDBError::ConsensusError(format!("open raft log: {}", e)))?;

        // Reconcile against the snapshot baseline: drop any persisted entries at or
        // below base_index (a crash between writing the snapshot and rewriting the log
        // can leave such entries behind).
        let base_index = snap.base_index;
        let base_term = Term(snap.base_term);
        let entries: Vec<LogEntry> = all_entries
            .into_iter()
            .filter(|e| e.index > base_index)
            .collect();

        // Restore persisted term/vote into the in-memory consensus state.
        let mut consensus_state = ConsensusState::new();
        consensus_state.current_term = Term(hard.current_term);
        consensus_state.voted_for = hard.voted_for.clone().map(NodeId::from_string);

        // Rebuild membership: start from the snapshot's membership if it recorded one
        // (it captured the config as of base_index, so compacted AddNode/RemoveNode
        // entries aren't lost), else the configured peers; then fold the surviving log.
        let base_membership: HashMap<NodeId, NodeAddress> = if snap.membership.is_empty() {
            config.peers.clone()
        } else {
            snap.membership
                .iter()
                .map(|(id, host, port)| {
                    (
                        NodeId::from_string(id.clone()),
                        NodeAddress::new(host.clone(), *port),
                    )
                })
                .collect()
        };
        let active_peers = Self::fold_membership(base_membership, &entries);

        let mut log = ReplicatedLog::with_store(log_store, entries, base_index, base_term);
        log.set_compaction_retention(config.compaction_retention);

        let (message_sender, message_receiver) = mpsc::unbounded_channel();
        Ok(RaftNode {
            config,
            active_peers: Arc::new(RwLock::new(active_peers)),
            state: Arc::new(RwLock::new(consensus_state)),
            log: Arc::new(RwLock::new(log)),
            next_index: Arc::new(RwLock::new(HashMap::new())),
            match_index: Arc::new(RwLock::new(HashMap::new())),
            message_sender,
            message_receiver: Arc::new(Mutex::new(message_receiver)),
            client_requests: Arc::new(Mutex::new(HashMap::new())),
            is_running: Arc::new(RwLock::new(false)),
            transport,
            state_machine,
            votes_received: Arc::new(Mutex::new(HashSet::new())),
            hard_state_store: Some(Arc::new(Mutex::new(hs_store))),
            snapshot_store: Some(Arc::new(Mutex::new(snap_store))),
        })
    }

    /// Construct a node with a no-op transport and no-op state machine. Useful for
    /// smoke tests of the local state machine without networking or storage.
    pub fn new_local(config: ClusterConfig) -> Self {
        Self::new(config, Arc::new(NoopTransport), Arc::new(NoopStateMachine))
    }

    /// Durably persist `currentTerm`/`votedFor`. Must be called (and awaited) after a
    /// term or vote change and before the node acts on it externally (replies to a
    /// vote, sends RequestVote), so a crash never causes a double-vote or lost term.
    async fn persist_hard_state(&self, current_term: u64, voted_for: Option<String>) {
        if let Some(store) = &self.hard_state_store {
            let hs = HardState { current_term, voted_for };
            let store = store.lock().await;
            if let Err(e) = store.save(&hs) {
                tracing::error!(error = %e, "failed to persist raft hard state");
            }
        }
    }

    /// Fold membership-change entries over an initial peer set to get the effective
    /// membership (used on restart to rebuild from the persisted log).
    fn fold_membership(
        mut peers: HashMap<NodeId, NodeAddress>,
        entries: &[LogEntry],
    ) -> HashMap<NodeId, NodeAddress> {
        for entry in entries {
            match &entry.operation {
                LogOperation::AddNode { node_id, address } => {
                    peers.insert(node_id.clone(), address.clone());
                }
                LogOperation::RemoveNode { node_id } => {
                    peers.remove(node_id);
                }
                _ => {}
            }
        }
        peers
    }

    /// Snapshot of the current peer set (excludes self).
    async fn peers_snapshot(&self) -> HashMap<NodeId, NodeAddress> {
        self.active_peers.read().await.clone()
    }

    /// Current cluster size (peers + self) and the majority needed for a quorum.
    async fn cluster_majority(&self) -> usize {
        let size = self.active_peers.read().await.len() + 1;
        (size / 2) + 1
    }

    /// Apply a membership change to the live peer set and replication bookkeeping.
    /// Called as the config entry is appended (apply-on-append) on every node, so the
    /// new configuration governs quorum decisions immediately.
    async fn apply_membership_change(&self, op: &LogOperation) {
        match op {
            LogOperation::AddNode { node_id, address } => {
                if *node_id == self.config.node_id {
                    return; // never list ourselves as a peer
                }
                self.active_peers
                    .write()
                    .await
                    .insert(node_id.clone(), address.clone());
                let last_log_index = self.log.read().await.last_log_index();
                self.next_index
                    .write()
                    .await
                    .entry(node_id.clone())
                    .or_insert(last_log_index + 1);
                self.match_index
                    .write()
                    .await
                    .entry(node_id.clone())
                    .or_insert(0);
                tracing::info!(node = %node_id.0, "membership: added node");
            }
            LogOperation::RemoveNode { node_id } => {
                self.active_peers.write().await.remove(node_id);
                self.next_index.write().await.remove(node_id);
                self.match_index.write().await.remove(node_id);
                tracing::info!(node = %node_id.0, "membership: removed node");
            }
            _ => {}
        }
    }

    /// True if there is a membership-change entry in the log that is not yet committed
    /// (Raft requires at most one membership change in flight at a time).
    async fn membership_change_pending(&self) -> bool {
        let log = self.log.read().await;
        let commit = log.get_commit_index();
        let last = log.last_log_index();
        for idx in (commit + 1)..=last {
            if let Some(entry) = log.get_entry(idx) {
                if matches!(
                    entry.operation,
                    LogOperation::AddNode { .. } | LogOperation::RemoveNode { .. }
                ) {
                    return true;
                }
            }
        }
        false
    }

    /// Append a membership-change entry on the leader, apply it immediately
    /// (apply-on-append), replicate, and wait for it to commit.
    async fn propose_membership_change(&self, op: LogOperation) -> Result<(), BlockDBError> {
        {
            let state = self.state.read().await;
            if !matches!(state.state, RaftState::Leader) {
                return Err(BlockDBError::ConsensusError("not the leader".to_string()));
            }
        }
        if self.membership_change_pending().await {
            return Err(BlockDBError::ConsensusError(
                "a membership change is already in progress".to_string(),
            ));
        }

        let request_id = Uuid::new_v4();
        {
            let mut log = self.log.write().await;
            let state = self.state.read().await;
            let entry = LogEntry::new(
                log.last_log_index() + 1,
                state.current_term.clone(),
                op.clone(),
                request_id,
            );
            drop(state);
            log.append(entry)?;
        }

        let (tx, rx) = tokio::sync::oneshot::channel();
        {
            let mut client_requests = self.client_requests.lock().await;
            client_requests.insert(request_id, tx);
        }

        // The change commits under the CURRENT configuration (apply-on-commit): a
        // 1->2 add commits with the old majority of 1, so it can't deadlock waiting
        // for the not-yet-present node. apply_committed() adopts the new membership
        // once the entry commits, after which the leader starts replicating to it.
        self.replicate_to_followers().await;
        self.update_commit_index().await;

        let timeout_duration = self.config.election_timeout * 10;
        match timeout(timeout_duration, rx).await {
            Ok(Ok(_)) => Ok(()),
            _ => {
                self.client_requests.lock().await.remove(&request_id);
                Err(BlockDBError::ConsensusError(
                    "timed out committing membership change".to_string(),
                ))
            }
        }
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
            let peers = self.active_peers.read().await;

            for peer_id in peers.keys() {
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
            ConsensusMessage::InstallSnapshot {
                term,
                leader_id,
                last_included_index,
                last_included_term,
                membership,
                data,
            } => {
                self.handle_install_snapshot(
                    sender,
                    term,
                    leader_id,
                    last_included_index,
                    last_included_term,
                    membership,
                    data,
                )
                .await;
            }
            ConsensusMessage::InstallSnapshotResponse { term, last_index } => {
                self.handle_install_snapshot_response(sender, term, last_index).await;
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

        // Leader stickiness (Raft dissertation §4.2.3): if we currently have a leader
        // and have heard from it within the minimum election timeout, ignore this vote
        // request outright — don't grant it and don't advance our term. This stops a
        // partitioned or just-restarted node (whose term has drifted upward) from
        // forcing a healthy leader to step down and causing an availability blip.
        let heard_recently = state
            .last_heartbeat
            .elapsed()
            .map(|e| e < self.config.election_timeout)
            .unwrap_or(false);
        if state.leader_id.is_some()
            && state.leader_id.as_ref() != Some(&candidate_id)
            && heard_recently
            && !matches!(state.state, RaftState::Candidate)
        {
            let response = ConsensusMessage::RequestVoteResponse {
                term: state.current_term.clone(),
                vote_granted: false,
            };
            drop(state);
            drop(log);
            self.send_message(sender, response).await;
            return;
        }

        let mut vote_granted = false;
        let mut dirty = false;

        // Update term if newer
        if term.0 > state.current_term.0 {
            state.current_term = term.clone();
            state.voted_for = None;
            state.state = RaftState::Follower;
            dirty = true;
        }

        // Grant vote if conditions are met
        if term.0 == state.current_term.0 &&
           (state.voted_for.is_none() || state.voted_for.as_ref() == Some(&candidate_id)) &&
           log.is_up_to_date(last_log_index, &last_log_term) {
            vote_granted = true;
            state.voted_for = Some(candidate_id);
            state.last_heartbeat = SystemTime::now();
            dirty = true;
        }

        let response = ConsensusMessage::RequestVoteResponse {
            term: state.current_term.clone(),
            vote_granted,
        };
        let persist_term = state.current_term.0;
        let persist_vote = state.voted_for.as_ref().map(|n| n.0.clone());

        drop(state);
        drop(log);

        // Raft safety: a granted vote (or an advanced term) must be on durable storage
        // before we tell the candidate, so a crash+restart never double-votes in a term.
        if dirty {
            self.persist_hard_state(persist_term, persist_vote).await;
        }

        self.send_message(sender, response).await;
    }
    
    async fn handle_vote_response(&self, sender: NodeId, term: Term, vote_granted: bool) {
        let mut state = self.state.write().await;
        
        // Update term if newer
        if term.0 > state.current_term.0 {
            state.current_term = term;
            state.state = RaftState::Follower;
            state.voted_for = None;
            let persist_term = state.current_term.0;
            drop(state);
            self.persist_hard_state(persist_term, None).await;
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

            if votes_received >= self.cluster_majority().await {
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
        let mut dirty = false;

        // Update term if newer
        if term.0 > state.current_term.0 {
            state.current_term = term.clone();
            state.state = RaftState::Follower;
            state.voted_for = None;
            dirty = true;
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

                // Append entries — persisted durably inside append_entries before success.
                success = match log.append_entries(prev_log_index, entries) {
                    Ok(ok) => ok,
                    Err(e) => {
                        tracing::error!(error = %e, "follower failed to persist appended entries");
                        false
                    }
                };

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
        let persist_term = state.current_term.0;
        let persist_vote = state.voted_for.as_ref().map(|n| n.0.clone());

        drop(state);
        drop(log);

        // Persist an advanced term before acknowledging the leader.
        if dirty {
            self.persist_hard_state(persist_term, persist_vote).await;
        }

        // If the leader advanced our commit index, apply the newly-committed entries
        // to our local state machine so this follower stays consistent with the leader.
        // Membership changes are adopted inside apply_committed (apply-on-commit).
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
            let persist_term = state.current_term.0;
            drop(state);
            self.persist_hard_state(persist_term, None).await;
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

    /// Follower side of InstallSnapshot: a leader is catching us up with a state-machine
    /// snapshot because we fell below its compacted log.
    async fn handle_install_snapshot(
        &self,
        sender: NodeId,
        term: Term,
        leader_id: NodeId,
        last_included_index: u64,
        last_included_term: Term,
        membership: Vec<(String, String, u16)>,
        data: Vec<u8>,
    ) {
        let mut dirty = false;
        {
            let mut state = self.state.write().await;
            if term.0 < state.current_term.0 {
                // Stale leader: reject with our current term.
                let response = ConsensusMessage::InstallSnapshotResponse {
                    term: state.current_term.clone(),
                    last_index: self.log.read().await.last_log_index(),
                };
                drop(state);
                self.send_message(sender, response).await;
                return;
            }
            if term.0 > state.current_term.0 {
                state.current_term = term.clone();
                state.voted_for = None;
                dirty = true;
            }
            state.state = RaftState::Follower;
            state.leader_id = Some(leader_id);
            state.last_heartbeat = SystemTime::now();
        }

        // If we are already at or beyond the snapshot point, ignore the data but ack.
        let our_last = self.log.read().await.last_log_index();
        let our_base = self.log.read().await.base_index();
        if last_included_index <= our_base {
            let (persist_term, persist_vote) = self.term_and_vote().await;
            if dirty {
                self.persist_hard_state(persist_term, persist_vote).await;
            }
            let response = ConsensusMessage::InstallSnapshotResponse {
                term: Term(persist_term),
                last_index: our_last,
            };
            self.send_message(sender, response).await;
            return;
        }

        // Restore the state machine FIRST (so a crash before advancing the log never
        // claims state we don't have), then install the snapshot baseline in the log.
        if let Err(e) = self.state_machine.restore(&data).await {
            tracing::error!(error = %e, "failed to restore snapshot; aborting install");
            return;
        }

        {
            let mut log = self.log.write().await;
            if let Err(e) = log.install_snapshot(last_included_index, last_included_term.clone()) {
                tracing::error!(error = %e, "failed to install snapshot into log");
                return;
            }
        }

        // Adopt the snapshot's membership.
        {
            let mut peers = self.active_peers.write().await;
            peers.clear();
            for (id, host, port) in &membership {
                if *id != self.config.node_id.0 {
                    peers.insert(NodeId::from_string(id.clone()), NodeAddress::new(host.clone(), *port));
                }
            }
        }

        // Persist the snapshot baseline + membership so a restart recovers it.
        if let Some(store) = &self.snapshot_store {
            let meta = SnapshotMeta {
                base_index: last_included_index,
                base_term: last_included_term.0,
                membership,
            };
            let store = store.lock().await;
            if let Err(e) = store.save(&meta) {
                tracing::error!(error = %e, "failed to persist snapshot baseline");
            }
        }

        let (persist_term, persist_vote) = self.term_and_vote().await;
        if dirty {
            self.persist_hard_state(persist_term, persist_vote).await;
        }

        tracing::info!(last_included_index, "installed snapshot from leader");
        let response = ConsensusMessage::InstallSnapshotResponse {
            term: Term(persist_term),
            last_index: last_included_index,
        };
        self.send_message(sender, response).await;
    }

    /// Leader side of the InstallSnapshot ack: advance the follower's indices.
    async fn handle_install_snapshot_response(&self, sender: NodeId, term: Term, last_index: u64) {
        let mut state = self.state.write().await;
        if term.0 > state.current_term.0 {
            state.current_term = term;
            state.state = RaftState::Follower;
            state.voted_for = None;
            let persist_term = state.current_term.0;
            drop(state);
            self.persist_hard_state(persist_term, None).await;
            return;
        }
        if !matches!(state.state, RaftState::Leader) || term.0 != state.current_term.0 {
            return;
        }
        drop(state);
        {
            let mut next_index = self.next_index.write().await;
            let mut match_index = self.match_index.write().await;
            match_index.insert(sender.clone(), last_index);
            next_index.insert(sender, last_index + 1);
        }
        self.update_commit_index().await;
    }

    /// Current (term, voted_for) for persistence.
    async fn term_and_vote(&self) -> (u64, Option<String>) {
        let state = self.state.read().await;
        (state.current_term.0, state.voted_for.as_ref().map(|n| n.0.clone()))
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

        // Append the entry to the leader's log (persisted durably before it counts
        // toward commitment).
        {
            let mut log = self.log.write().await;
            let state = self.state.read().await;
            let entry = LogEntry::new(
                log.last_log_index() + 1,
                state.current_term.clone(),
                log_operation,
                request_id,
            );
            drop(state);
            log.append(entry)?;
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
    
    /// Linearizable-read barrier (Raft ReadIndex, implemented via a committed no-op).
    ///
    /// A leader cannot safely serve a read from local state without first confirming
    /// it is still the leader — a newer leader may have committed writes it hasn't
    /// seen. This appends a no-op entry and waits for it to commit: commitment
    /// requires a majority to acknowledge an entry in the leader's current term, which
    /// proves current leadership, and applying up to that entry guarantees every write
    /// committed before the read is reflected in local state. The caller then reads
    /// locally and gets a linearizable result. Returns an error if this node is not
    /// the leader (the caller should forward to the leader).
    pub async fn linearizable_barrier(&self) -> Result<(), BlockDBError> {
        {
            let state = self.state.read().await;
            if !matches!(state.state, RaftState::Leader) {
                return Err(BlockDBError::ConsensusError("not the leader".to_string()));
            }
        }

        let request_id = Uuid::new_v4();
        {
            let mut log = self.log.write().await;
            let state = self.state.read().await;
            let entry = LogEntry::new(
                log.last_log_index() + 1,
                state.current_term.clone(),
                LogOperation::NoOp,
                request_id,
            );
            drop(state);
            log.append(entry)?;
        }

        let (tx, rx) = tokio::sync::oneshot::channel();
        {
            let mut client_requests = self.client_requests.lock().await;
            client_requests.insert(request_id, tx);
        }

        self.replicate_to_followers().await;
        self.update_commit_index().await;

        let timeout_duration = self.config.election_timeout * 10;
        match timeout(timeout_duration, rx).await {
            Ok(Ok(_apply_result)) => Ok(()),
            _ => {
                self.client_requests.lock().await.remove(&request_id);
                Err(BlockDBError::ConsensusError(
                    "timed out confirming leadership for linearizable read".to_string(),
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

        let persist_term = state.current_term.0;
        let self_id = self.config.node_id.0.clone();

        // A single-node cluster wins its own election immediately.
        if self.cluster_majority().await <= 1 {
            state.become_leader(self.config.node_id.clone());
            drop(state);
            drop(log);
            // Durably record the new term + self-vote even in single-node mode.
            self.persist_hard_state(persist_term, Some(self_id)).await;
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

        // Raft safety: persist the incremented term and self-vote before soliciting
        // votes, so a crash+restart cannot vote again in this term.
        self.persist_hard_state(persist_term, Some(self_id)).await;

        // Send vote requests to all peers
        for peer_id in self.peers_snapshot().await.keys() {
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
        
        let (appends, snapshot_targets, base_index, base_term) = {
            let log = self.log.read().await;
            let next_index = self.next_index.read().await;
            let leader_commit = log.get_commit_index();
            let base_index = log.base_index();
            let base_term = log.base_term();

            let mut appends: Vec<(NodeId, ConsensusMessage)> = Vec::new();
            let mut snapshot_targets: Vec<NodeId> = Vec::new();

            for (peer_id, &next_idx) in next_index.iter() {
                // If the follower needs entries at or below our compaction baseline, the
                // log can no longer provide them: send a snapshot instead.
                if next_idx <= base_index {
                    snapshot_targets.push(peer_id.clone());
                    continue;
                }
                let prev_log_index = next_idx.saturating_sub(1);
                let prev_log_term = if prev_log_index == 0 {
                    Term::new()
                } else {
                    log.get_term(prev_log_index).unwrap_or_else(Term::new)
                };
                let entries = log.get_entries_from(next_idx);
                appends.push((
                    peer_id.clone(),
                    ConsensusMessage::AppendEntries {
                        term: current_term.clone(),
                        leader_id: leader_id.clone(),
                        prev_log_index,
                        prev_log_term,
                        entries,
                        leader_commit,
                    },
                ));
            }
            (appends, snapshot_targets, base_index, base_term)
        };

        for (peer_id, message) in appends {
            self.send_message(peer_id, message).await;
        }

        if !snapshot_targets.is_empty() {
            // Build the snapshot once for all lagging followers.
            let data = self.state_machine.snapshot().await.unwrap_or_default();
            let membership: Vec<(String, String, u16)> = self
                .active_peers
                .read()
                .await
                .iter()
                .map(|(id, addr)| (id.0.clone(), addr.host.clone(), addr.port))
                .collect();
            let msg = ConsensusMessage::InstallSnapshot {
                term: current_term.clone(),
                leader_id: leader_id.clone(),
                last_included_index: base_index,
                last_included_term: base_term,
                membership,
                data,
            };
            for peer_id in snapshot_targets {
                self.send_message(peer_id, msg.clone()).await;
            }
        }
    }
    
    async fn initialize_leader_state(&self) {
        // Snapshot peers first (separate lock) to avoid holding index locks across it.
        let peers: Vec<NodeId> = self.active_peers.read().await.keys().cloned().collect();
        let mut next_index = self.next_index.write().await;
        let mut match_index = self.match_index.write().await;
        let log = self.log.read().await;

        let last_log_index = log.last_log_index();

        next_index.clear();
        match_index.clear();
        for peer_id in peers {
            next_index.insert(peer_id.clone(), last_log_index + 1);
            match_index.insert(peer_id.clone(), 0);
        }
    }

    async fn update_commit_index(&self) {
        // Compute the quorum threshold up front, before taking the index/log/state
        // locks, so we never await on the peer-set lock while holding them (which
        // could deadlock against an in-flight membership change).
        let majority = self.cluster_majority().await;

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

            if count >= majority {
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

        let address = self.active_peers.read().await.get(&target).cloned();
        if let Some(address) = address {
            let transport = self.transport.clone();
            let from = self.config.node_id.clone();
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
            // Adopt committed membership changes (apply-on-commit) on every node, so
            // leader and followers converge on the same configuration and quorum math.
            if matches!(
                entry.operation,
                LogOperation::AddNode { .. } | LogOperation::RemoveNode { .. }
            ) {
                self.apply_membership_change(&entry.operation).await;
            }

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

        // Having applied more entries, consider compacting the log tail.
        self.maybe_compact().await;
    }

    /// Compact the applied prefix of the log when it has grown past the retention
    /// tail. Durability order matters: record the new snapshot baseline + membership
    /// first, then rewrite the log file, so a crash in between is reconciled on load.
    async fn maybe_compact(&self) {
        let snapshot_store = match &self.snapshot_store {
            Some(s) => s,
            None => return, // in-memory node: nothing to persist/compact
        };

        // Decide the compaction point and capture the boundary term.
        let (target, base_term) = {
            let log = self.log.read().await;
            match log.compaction_target() {
                Some(t) => match log.term_at(t) {
                    Some(term) => (t, term.0),
                    None => return,
                },
                None => return,
            }
        };

        // Snapshot the membership as of the baseline so a restart can rebuild the
        // configuration even after the config entries are compacted away.
        let membership: Vec<(String, String, u16)> = self
            .active_peers
            .read()
            .await
            .iter()
            .map(|(id, addr)| (id.0.clone(), addr.host.clone(), addr.port))
            .collect();

        let meta = SnapshotMeta {
            base_index: target,
            base_term,
            membership,
        };
        {
            let store = snapshot_store.lock().await;
            if let Err(e) = store.save(&meta) {
                tracing::error!(error = %e, "failed to persist raft snapshot baseline; skipping compaction");
                return;
            }
        }

        // Now truncate the log in memory + on disk.
        let mut log = self.log.write().await;
        if let Err(e) = log.compact(target) {
            tracing::error!(error = %e, "log compaction failed after snapshot write");
        } else {
            tracing::info!(base_index = target, "compacted raft log");
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
            active_peers: self.active_peers.clone(),
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
            hard_state_store: self.hard_state_store.clone(),
            snapshot_store: self.snapshot_store.clone(),
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
    
    async fn add_node(&self, node_id: NodeId, address: NodeAddress) -> Result<(), BlockDBError> {
        if node_id == self.config.node_id {
            return Ok(()); // adding self is a no-op
        }
        self.propose_membership_change(LogOperation::AddNode { node_id, address })
            .await
    }

    async fn remove_node(&self, node_id: &NodeId) -> Result<(), BlockDBError> {
        self.propose_membership_change(LogOperation::RemoveNode {
            node_id: node_id.clone(),
        })
        .await
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}