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
use crate::consensus::transport::{HttpRaftTransport, RaftEnvelope, CLUSTER_AUTH_HEADER};
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

/// Credentials that gate the cluster's HTTP surface.
///
/// Both are optional so a dev/single-node setup can run open, but in production
/// both should be set (e.g. from a secrets manager):
///   * `cluster_secret` — a shared secret every node presents on `/raft/message`,
///     so a network attacker can't inject forged Raft RPCs (fake votes, bogus
///     AppendEntries) and subvert consensus.
///   * `api_token` — a bearer token required on client `/kv` reads and writes.
///
/// When a field is `None`, the corresponding endpoint is unauthenticated.
#[derive(Debug, Clone, Default)]
pub struct ClusterSecurity {
    pub cluster_secret: Option<String>,
    pub api_token: Option<String>,
    /// When set, the HTTP server is served over TLS. Strongly recommended in
    /// production: the cluster secret and API token travel in request headers and
    /// would otherwise be sent in plaintext.
    pub tls: Option<TlsConfig>,
}

/// Paths to the PEM-encoded server certificate chain and private key for TLS.
#[derive(Debug, Clone)]
pub struct TlsConfig {
    pub cert_path: String,
    pub key_path: String,
}

impl ClusterSecurity {
    /// Build from environment: `BLOCKDB_CLUSTER_SECRET`, `BLOCKDB_API_TOKEN`.
    /// An empty or unset variable leaves that endpoint open.
    pub fn from_env() -> Self {
        fn non_empty(var: &str) -> Option<String> {
            std::env::var(var).ok().filter(|s| !s.is_empty())
        }
        let tls = match (non_empty("BLOCKDB_TLS_CERT"), non_empty("BLOCKDB_TLS_KEY")) {
            (Some(cert_path), Some(key_path)) => Some(TlsConfig { cert_path, key_path }),
            _ => None,
        };
        ClusterSecurity {
            cluster_secret: non_empty("BLOCKDB_CLUSTER_SECRET"),
            api_token: non_empty("BLOCKDB_API_TOKEN"),
            tls,
        }
    }
}

/// Constant-time byte comparison, so secret checks don't leak length/content via
/// timing. Returns true only when both slices are equal.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// One node in a BlockDB Raft cluster.
pub struct ClusterNode {
    node_id: NodeId,
    address: NodeAddress,
    peers: HashMap<NodeId, NodeAddress>,
    storage: BlockDBHandle,
    raft: Arc<RaftNode>,
    forward_timeout: Duration,
    security: ClusterSecurity,
}

impl ClusterNode {
    /// Create an unauthenticated cluster node (dev/single-node). Equivalent to
    /// `new_secure` with `ClusterSecurity::default()`.
    pub fn new(
        cluster_config: ClusterConfig,
        storage_config: BlockDBConfig,
    ) -> Result<Arc<Self>, BlockDBError> {
        Self::new_secure(cluster_config, storage_config, ClusterSecurity::default())
    }

    /// Create a cluster node. `cluster_config` carries this node's id/address and the
    /// addresses of its peers; `storage_config` configures the local durable store;
    /// `security` gates the Raft RPC and client KV endpoints.
    pub fn new_secure(
        cluster_config: ClusterConfig,
        storage_config: BlockDBConfig,
        security: ClusterSecurity,
    ) -> Result<Arc<Self>, BlockDBError> {
        let data_dir = std::path::PathBuf::from(storage_config.data_dir.clone());
        let storage = BlockDBHandle::new(storage_config)?;
        let state_machine = Arc::new(BlockDBStateMachine { db: storage.clone() });
        // The transport signs outbound Raft RPCs with the same shared secret the
        // server requires on inbound ones.
        let transport = Arc::new(HttpRaftTransport::with_secret(
            Duration::from_millis(500),
            security.cluster_secret.clone(),
        ));

        let node_id = cluster_config.node_id.clone();
        let address = cluster_config.address.clone();
        let peers = cluster_config.peers.clone();

        // Durably persist the Raft log + hard state under <data_dir>/raft so a
        // restarted node recovers its term, vote, and committed log.
        let raft = Arc::new(RaftNode::new_persistent(
            cluster_config,
            transport,
            state_machine,
            &data_dir,
        )?);

        Ok(Arc::new(ClusterNode {
            node_id,
            address,
            peers,
            storage,
            raft,
            forward_timeout: Duration::from_secs(5),
            security,
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

        if let Some(tls) = self.security.tls.clone() {
            return self.serve_tls(addr, tls, shutdown).await;
        }

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

        info!(node = %self.node_id.0, %addr, "cluster node listening (plaintext)");
        Server::bind(&addr)
            .serve(make_svc)
            .with_graceful_shutdown(shutdown)
            .await
            .map_err(|e| BlockDBError::ApiError(format!("cluster server error: {}", e)))?;
        Ok(())
    }

    /// Serve the cluster HTTP surface over TLS. Accepts connections until `shutdown`
    /// resolves; each accepted connection is handled on its own task.
    async fn serve_tls<F>(
        self: &Arc<Self>,
        addr: SocketAddr,
        tls: TlsConfig,
        shutdown: F,
    ) -> Result<(), BlockDBError>
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        let server_config = load_server_tls_config(&tls)?;
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(server_config));
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .map_err(|e| BlockDBError::ApiError(format!("cluster TLS bind failed: {}", e)))?;

        info!(node = %self.node_id.0, %addr, "cluster node listening (TLS)");
        tokio::pin!(shutdown);
        loop {
            tokio::select! {
                _ = &mut shutdown => break,
                accepted = listener.accept() => {
                    let (stream, _peer) = match accepted {
                        Ok(pair) => pair,
                        Err(e) => {
                            warn!("cluster TLS accept error: {}", e);
                            continue;
                        }
                    };
                    let acceptor = acceptor.clone();
                    let node = self.clone();
                    tokio::spawn(async move {
                        let tls_stream = match acceptor.accept(stream).await {
                            Ok(s) => s,
                            Err(e) => {
                                warn!("cluster TLS handshake failed: {}", e);
                                return;
                            }
                        };
                        let svc = service_fn(move |req| {
                            let node = node.clone();
                            async move { Ok::<_, Infallible>(node.handle(req).await) }
                        });
                        if let Err(e) = hyper::server::conn::Http::new()
                            .serve_connection(tls_stream, svc)
                            .await
                        {
                            warn!("cluster TLS connection error: {}", e);
                        }
                    });
                }
            }
        }
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
        let mut builder = Request::post(&uri).header("content-type", "application/json");
        if let Some(token) = &self.security.api_token {
            builder = builder.header(hyper::header::AUTHORIZATION, format!("Bearer {}", token));
        }
        let req = builder
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

        // Authenticate before doing any work. `/status` and `/health` are left open
        // so load balancers and operators can probe liveness without a credential.
        let auth = match (&method, path.as_str()) {
            (&Method::POST, "/raft/message") => self.check_cluster_auth(&req),
            (_, "/kv") => self.check_client_auth(&req),
            _ => Ok(()),
        };

        let result = match auth {
            Err(e) => Err(e),
            Ok(()) => match (&method, path.as_str()) {
                (&Method::POST, "/raft/message") => self.handle_raft_message(req).await,
                (&Method::POST, "/kv") => self.handle_kv_write(req).await,
                (&Method::GET, "/kv") => self.handle_kv_read(&query).await,
                (&Method::GET, "/status") => self.handle_status().await,
                _ => Ok(json(StatusCode::NOT_FOUND, &serde_json::json!({"error": "not found"}))),
            },
        };

        result.unwrap_or_else(|e| {
            let status = match e {
                BlockDBError::Unauthorized(_) => StatusCode::UNAUTHORIZED,
                BlockDBError::DuplicateKey(_) => StatusCode::CONFLICT,
                BlockDBError::InvalidData(_) => StatusCode::BAD_REQUEST,
                BlockDBError::ConsensusError(_) => StatusCode::SERVICE_UNAVAILABLE,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            json(status, &serde_json::json!({"success": false, "error": e.to_string()}))
        })
    }

    /// Reject inter-node Raft RPCs that don't carry the shared cluster secret.
    fn check_cluster_auth(&self, req: &Request<Body>) -> Result<(), BlockDBError> {
        let expected = match &self.security.cluster_secret {
            Some(s) => s,
            None => return Ok(()),
        };
        let presented = req
            .headers()
            .get(CLUSTER_AUTH_HEADER)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if constant_time_eq(presented.as_bytes(), expected.as_bytes()) {
            Ok(())
        } else {
            Err(BlockDBError::Unauthorized(
                "invalid or missing cluster secret".to_string(),
            ))
        }
    }

    /// Reject client KV requests that don't carry the `Authorization: Bearer` token.
    fn check_client_auth(&self, req: &Request<Body>) -> Result<(), BlockDBError> {
        let expected = match &self.security.api_token {
            Some(t) => t,
            None => return Ok(()),
        };
        let presented = req
            .headers()
            .get(hyper::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .unwrap_or("");
        if constant_time_eq(presented.as_bytes(), expected.as_bytes()) {
            Ok(())
        } else {
            Err(BlockDBError::Unauthorized(
                "invalid or missing API token".to_string(),
            ))
        }
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

/// Load a rustls `ServerConfig` from PEM cert-chain and private-key files.
/// Accepts PKCS#8, PKCS#1 (RSA), or SEC1 (EC) private keys.
fn load_server_tls_config(
    tls: &TlsConfig,
) -> Result<tokio_rustls::rustls::ServerConfig, BlockDBError> {
    use std::io::BufReader;
    use tokio_rustls::rustls::{Certificate, PrivateKey, ServerConfig};

    let cert_file = std::fs::File::open(&tls.cert_path).map_err(|e| {
        BlockDBError::ApiError(format!("cannot open TLS cert {}: {}", tls.cert_path, e))
    })?;
    let certs: Vec<Certificate> = rustls_pemfile::certs(&mut BufReader::new(cert_file))
        .map_err(|e| BlockDBError::ApiError(format!("bad TLS cert PEM: {}", e)))?
        .into_iter()
        .map(Certificate)
        .collect();
    if certs.is_empty() {
        return Err(BlockDBError::ApiError(format!(
            "no certificates found in {}",
            tls.cert_path
        )));
    }

    let key_file = std::fs::File::open(&tls.key_path).map_err(|e| {
        BlockDBError::ApiError(format!("cannot open TLS key {}: {}", tls.key_path, e))
    })?;
    let mut key_reader = BufReader::new(key_file);
    let key = rustls_pemfile::read_all(&mut key_reader)
        .map_err(|e| BlockDBError::ApiError(format!("bad TLS key PEM: {}", e)))?
        .into_iter()
        .find_map(|item| match item {
            rustls_pemfile::Item::PKCS8Key(k)
            | rustls_pemfile::Item::RSAKey(k)
            | rustls_pemfile::Item::ECKey(k) => Some(PrivateKey(k)),
            _ => None,
        })
        .ok_or_else(|| {
            BlockDBError::ApiError(format!("no private key found in {}", tls.key_path))
        })?;

    ServerConfig::builder()
        .with_safe_defaults()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| BlockDBError::ApiError(format!("invalid TLS cert/key: {}", e)))
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
