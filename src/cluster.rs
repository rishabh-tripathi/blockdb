//! Multi-node cluster deployment for BlockDB.
//!
//! A `ClusterNode` is one member of a Raft cluster. It owns a local, durable BlockDB
//! (the replicated state machine) and a `RaftNode`, and runs an HTTP server that
//! serves two roles:
//!
//!  * **Raft RPC** (`POST /raft/message`) — peers exchange consensus messages here.
//!  * **Client KV** (`POST /kv`, `GET /kv`) — application reads and writes.
//!
//! Consistency model: all writes are linearized through the Raft leader. A write is
//! appended to the leader's log, replicated to a majority, then applied to every
//! node's state machine in the same order — so all nodes converge on an identical
//! view. Writes sent to a follower are transparently forwarded to the leader.

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Client, Method, Request, Response, Server, StatusCode};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::consensus::log_entry::{LogEntry, LogOperation};
use crate::consensus::raft::RaftNode;
use crate::consensus::transport::{HttpRaftTransport, RaftEnvelope};
use crate::consensus::{ClusterConfig, ConsensusEngine, NodeAddress, NodeId, StateMachine};
use crate::error::BlockDBError;
use crate::storage::BlockDBConfig;
use crate::transaction::Operation;
use crate::BlockDBHandle;

/// Applies committed Raft log entries to the local durable BlockDB.
struct BlockDBStateMachine {
    db: BlockDBHandle,
}

#[async_trait]
impl StateMachine for BlockDBStateMachine {
    async fn apply(&self, entry: &LogEntry) -> Result<(), BlockDBError> {
        match &entry.operation {
            LogOperation::Put { key, value } => self.db.put(key, value).await,
            // Non-Put operations (transaction markers, membership, no-ops) don't
            // mutate the key/value store.
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KvWriteRequest {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KvWriteResponse {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KvReadResponse {
    pub success: bool,
    pub found: bool,
    pub value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterStatus {
    pub node_id: String,
    pub is_leader: bool,
    pub leader_id: Option<String>,
    pub cluster_size: usize,
}

/// One node in a BlockDB Raft cluster.
pub struct ClusterNode {
    node_id: NodeId,
    address: NodeAddress,
    peers: HashMap<NodeId, NodeAddress>,
    storage: BlockDBHandle,
    raft: Arc<RaftNode>,
    forward_timeout: Duration,
}

impl ClusterNode {
    /// Create a cluster node. `cluster_config` carries this node's id/address and the
    /// addresses of its peers; `storage_config` configures the local durable store.
    pub fn new(
        cluster_config: ClusterConfig,
        storage_config: BlockDBConfig,
    ) -> Result<Arc<Self>, BlockDBError> {
        let storage = BlockDBHandle::new(storage_config)?;
        let state_machine = Arc::new(BlockDBStateMachine { db: storage.clone() });
        let transport = Arc::new(HttpRaftTransport::new());

        let node_id = cluster_config.node_id.clone();
        let address = cluster_config.address.clone();
        let peers = cluster_config.peers.clone();

        let raft = Arc::new(RaftNode::new(cluster_config, transport, state_machine));

        Ok(Arc::new(ClusterNode {
            node_id,
            address,
            peers,
            storage,
            raft,
            forward_timeout: Duration::from_secs(5),
        }))
    }

    /// Start consensus and the cluster HTTP server. Returns once the server is bound;
    /// the server runs until `shutdown` resolves.
    pub async fn start<F>(self: &Arc<Self>, shutdown: F) -> Result<(), BlockDBError>
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        self.raft.start().await?;

        let addr: SocketAddr = format!("{}:{}", self.address.host, self.address.port)
            .parse()
            .map_err(|e| BlockDBError::ApiError(format!("invalid cluster address: {}", e)))?;

        let node = self.clone();
        let make_svc = make_service_fn(move |_conn| {
            let node = node.clone();
            async move {
                Ok::<_, Infallible>(service_fn(move |req| {
                    let node = node.clone();
                    async move { Ok::<_, Infallible>(node.handle(req).await) }
                }))
            }
        });

        info!(node = %self.node_id.0, %addr, "cluster node listening");
        Server::bind(&addr)
            .serve(make_svc)
            .with_graceful_shutdown(shutdown)
            .await
            .map_err(|e| BlockDBError::ApiError(format!("cluster server error: {}", e)))?;
        Ok(())
    }

    /// Write a key/value pair with cluster-wide consistency. On the leader this drives
    /// the write through Raft; on a follower it forwards to the current leader.
    pub async fn put(&self, key: &[u8], value: &[u8]) -> Result<(), BlockDBError> {
        if self.raft.is_leader().await {
            let op = Operation::Put {
                key: key.to_vec(),
                value: value.to_vec(),
            };
            return self.raft.propose(op).await;
        }

        // Not the leader: forward to whoever is.
        let leader = self
            .raft
            .get_leader()
            .await
            .ok_or_else(|| BlockDBError::ConsensusError("no leader elected yet".to_string()))?;

        if leader == self.node_id {
            // We think we're the leader per get_leader but is_leader said no — racey
            // transition; ask the caller to retry.
            return Err(BlockDBError::ConsensusError(
                "leadership in transition, retry".to_string(),
            ));
        }

        let leader_addr = self.peers.get(&leader).ok_or_else(|| {
            BlockDBError::ConsensusError(format!("unknown address for leader {}", leader.0))
        })?;

        self.forward_write(leader_addr, key, value).await
    }

    /// Read a key from the local durable store. Reads are served locally and are
    /// eventually consistent; route through the leader if you need linearizable reads.
    pub async fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, BlockDBError> {
        self.storage.get(key).await
    }

    pub async fn is_leader(&self) -> bool {
        self.raft.is_leader().await
    }

    async fn forward_write(
        &self,
        leader_addr: &NodeAddress,
        key: &[u8],
        value: &[u8],
    ) -> Result<(), BlockDBError> {
        let body = serde_json::to_vec(&KvWriteRequest {
            key: String::from_utf8_lossy(key).to_string(),
            value: String::from_utf8_lossy(value).to_string(),
        })
        .map_err(|e| BlockDBError::InvalidData(e.to_string()))?;

        let uri = format!("{}/kv", leader_addr.to_url());
        let req = Request::post(&uri)
            .header("content-type", "application/json")
            .body(Body::from(body))
            .map_err(|e| BlockDBError::ApiError(e.to_string()))?;

        let client = Client::new();
        let resp = tokio::time::timeout(self.forward_timeout, client.request(req))
            .await
            .map_err(|_| BlockDBError::ConsensusError("forward to leader timed out".to_string()))?
            .map_err(|e| BlockDBError::ConsensusError(format!("forward to leader failed: {}", e)))?;

        if resp.status().is_success() {
            Ok(())
        } else {
            let status = resp.status();
            let bytes = hyper::body::to_bytes(resp.into_body())
                .await
                .unwrap_or_default();
            Err(BlockDBError::ConsensusError(format!(
                "leader rejected write ({}): {}",
                status,
                String::from_utf8_lossy(&bytes)
            )))
        }
    }

    // ---- HTTP handling ----

    async fn handle(self: Arc<Self>, req: Request<Body>) -> Response<Body> {
        let method = req.method().clone();
        let path = req.uri().path().to_string();
        let query = req.uri().query().unwrap_or("").to_string();

        let result = match (&method, path.as_str()) {
            (&Method::POST, "/raft/message") => self.handle_raft_message(req).await,
            (&Method::POST, "/kv") => self.handle_kv_write(req).await,
            (&Method::GET, "/kv") => self.handle_kv_read(&query).await,
            (&Method::GET, "/status") => self.handle_status().await,
            _ => Ok(json(StatusCode::NOT_FOUND, &serde_json::json!({"error": "not found"}))),
        };

        result.unwrap_or_else(|e| {
            let status = match e {
                BlockDBError::DuplicateKey(_) => StatusCode::CONFLICT,
                BlockDBError::InvalidData(_) => StatusCode::BAD_REQUEST,
                BlockDBError::ConsensusError(_) => StatusCode::SERVICE_UNAVAILABLE,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            json(status, &serde_json::json!({"success": false, "error": e.to_string()}))
        })
    }

    async fn handle_raft_message(&self, req: Request<Body>) -> Result<Response<Body>, BlockDBError> {
        let bytes = hyper::body::to_bytes(req.into_body())
            .await
            .map_err(|e| BlockDBError::ApiError(e.to_string()))?;
        let envelope: RaftEnvelope = serde_json::from_slice(&bytes)
            .map_err(|e| BlockDBError::InvalidData(format!("bad raft envelope: {}", e)))?;
        self.raft.inject(envelope.from, envelope.message);
        Ok(json(StatusCode::OK, &serde_json::json!({"ok": true})))
    }

    async fn handle_kv_write(&self, req: Request<Body>) -> Result<Response<Body>, BlockDBError> {
        let bytes = hyper::body::to_bytes(req.into_body())
            .await
            .map_err(|e| BlockDBError::ApiError(e.to_string()))?;
        let write: KvWriteRequest = serde_json::from_slice(&bytes)
            .map_err(|e| BlockDBError::InvalidData(format!("bad write request: {}", e)))?;

        self.put(write.key.as_bytes(), write.value.as_bytes()).await?;
        Ok(json(
            StatusCode::OK,
            &KvWriteResponse {
                success: true,
                message: "written".to_string(),
            },
        ))
    }

    async fn handle_kv_read(&self, query: &str) -> Result<Response<Body>, BlockDBError> {
        let key = query
            .split('&')
            .find_map(|p| p.strip_prefix("key="))
            .ok_or_else(|| BlockDBError::InvalidData("missing 'key' parameter".to_string()))?;

        let value = self.get(key.as_bytes()).await?;
        Ok(json(
            StatusCode::OK,
            &KvReadResponse {
                success: true,
                found: value.is_some(),
                value: value.map(|v| String::from_utf8_lossy(&v).to_string()),
            },
        ))
    }

    async fn handle_status(&self) -> Result<Response<Body>, BlockDBError> {
        let is_leader = self.raft.is_leader().await;
        let leader_id = self.raft.get_leader().await.map(|n| n.0);
        Ok(json(
            StatusCode::OK,
            &ClusterStatus {
                node_id: self.node_id.0.clone(),
                is_leader,
                leader_id,
                cluster_size: self.peers.len() + 1,
            },
        ))
    }
}

fn json<T: Serialize>(status: StatusCode, body: &T) -> Response<Body> {
    let payload = serde_json::to_vec(body).unwrap_or_else(|_| b"{}".to_vec());
    Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .body(Body::from(payload))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

/// Build a `ClusterConfig` from this node's identity and a peer list.
pub fn build_cluster_config(
    node_id: &str,
    host: &str,
    port: u16,
    peers: &[(String, String, u16)],
    heartbeat: Duration,
    election_timeout: Duration,
) -> ClusterConfig {
    let mut config = ClusterConfig::new(
        NodeId::from_string(node_id.to_string()),
        NodeAddress::new(host.to_string(), port),
    );
    config.heartbeat_interval = heartbeat;
    config.election_timeout = election_timeout;
    for (id, phost, pport) in peers {
        if id != node_id {
            config.add_peer(
                NodeId::from_string(id.clone()),
                NodeAddress::new(phost.clone(), *pport),
            );
        }
    }
    if config.peers.is_empty() {
        warn!("cluster configured with no peers; running as a single-node cluster");
    }
    config
}
