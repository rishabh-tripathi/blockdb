//! Network transport for Raft consensus messages.
//!
//! Raft is a message-passing protocol: nodes exchange RequestVote / AppendEntries
//! messages (and their responses). This module abstracts *how* those messages travel
//! so the core state machine in `raft.rs` stays transport-agnostic and testable.
//!
//! The production transport is HTTP: each message is POSTed as JSON to the target
//! node's `/raft/message` endpoint (served by the cluster server). Delivery is
//! best-effort and fire-and-forget with a short timeout — Raft is designed to
//! tolerate dropped, delayed, and reordered messages, so a failed send simply means
//! the sender will retry on the next heartbeat/election tick.

use std::time::Duration;

use hyper::{Body, Client, Request};
use serde::{Deserialize, Serialize};
use tokio::time::timeout;

use crate::consensus::{ConsensusMessage, NodeAddress, NodeId};

/// Wire envelope: identifies the sender so the receiver knows where to reply.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaftEnvelope {
    pub from: NodeId,
    pub message: ConsensusMessage,
}

/// Abstraction over the medium used to deliver Raft messages to peers.
#[async_trait::async_trait]
pub trait RaftTransport: Send + Sync {
    async fn send(&self, target: &NodeAddress, from: NodeId, message: ConsensusMessage);
}

/// A transport that drops everything. Used for single-node/local setups and tests
/// that don't exercise networking.
pub struct NoopTransport;

#[async_trait::async_trait]
impl RaftTransport for NoopTransport {
    async fn send(&self, _target: &NodeAddress, _from: NodeId, _message: ConsensusMessage) {}
}

/// HTTP transport: POSTs each message to `http://<host>:<port>/raft/message`.
pub struct HttpRaftTransport {
    timeout: Duration,
}

impl HttpRaftTransport {
    pub fn new() -> Self {
        HttpRaftTransport {
            timeout: Duration::from_millis(500),
        }
    }

    pub fn with_timeout(timeout: Duration) -> Self {
        HttpRaftTransport { timeout }
    }
}

impl Default for HttpRaftTransport {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl RaftTransport for HttpRaftTransport {
    async fn send(&self, target: &NodeAddress, from: NodeId, message: ConsensusMessage) {
        let envelope = RaftEnvelope { from, message };
        let body = match serde_json::to_vec(&envelope) {
            Ok(b) => b,
            Err(_) => return,
        };
        let uri = format!("{}/raft/message", target.to_url());
        let req = match Request::post(&uri)
            .header("content-type", "application/json")
            .body(Body::from(body))
        {
            Ok(r) => r,
            Err(_) => return,
        };

        let client = Client::new();
        // Best-effort: a timeout or connection error is fine — Raft retries.
        let _ = timeout(self.timeout, client.request(req)).await;
    }
}
