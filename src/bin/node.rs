//! `blockdb-node` — run a single member of a BlockDB Raft cluster.
//!
//! Example (3-node cluster, one process each):
//!   blockdb-node --node-id node0 --host 0.0.0.0 --port 8080 \
//!     --peers node0@10.0.0.1:8080,node1@10.0.0.2:8080,node2@10.0.0.3:8080 \
//!     --data-dir /var/lib/blockdb
//!
//! Writes are linearized through the Raft leader and replicated to a majority before
//! being applied on every node; a write sent to a follower is forwarded to the leader.

use blockdb::api::shutdown_signal;
use blockdb::cluster::{build_cluster_config, ClusterNode, ClusterSecurity};
use blockdb::storage::BlockDBConfig;
use clap::Parser;
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(author, version, about = "BlockDB cluster node", long_about = None)]
struct Args {
    /// Unique id for this node (must match its entry in --peers).
    #[arg(long, env = "BLOCKDB_NODE_ID")]
    node_id: String,

    /// Address this node binds for cluster + client traffic.
    #[arg(long, env = "BLOCKDB_HOST", default_value = "0.0.0.0")]
    host: String,

    #[arg(long, env = "BLOCKDB_PORT", default_value = "8080")]
    port: u16,

    /// Comma-separated peer list as id@host:port (include this node too).
    #[arg(long, env = "BLOCKDB_PEERS", default_value = "")]
    peers: String,

    #[arg(long, env = "BLOCKDB_DATA_DIR", default_value = "./blockdb_data")]
    data_dir: String,

    #[arg(long, default_value = "150")]
    heartbeat_ms: u64,

    #[arg(long, default_value = "300")]
    election_timeout_ms: u64,

    /// Shared secret required on inter-node /raft/message RPCs. When unset the
    /// Raft endpoint is unauthenticated — set it in any real deployment.
    #[arg(long, env = "BLOCKDB_CLUSTER_SECRET")]
    cluster_secret: Option<String>,

    /// Bearer token required on client /kv reads and writes. When unset the KV
    /// endpoint is unauthenticated.
    #[arg(long, env = "BLOCKDB_API_TOKEN")]
    api_token: Option<String>,

    /// Path to a PEM server certificate chain. Enables TLS when given together
    /// with --tls-key.
    #[arg(long, env = "BLOCKDB_TLS_CERT")]
    tls_cert: Option<String>,

    /// Path to the PEM private key matching --tls-cert.
    #[arg(long, env = "BLOCKDB_TLS_KEY")]
    tls_key: Option<String>,
}

fn parse_peers(spec: &str) -> Result<Vec<(String, String, u16)>, String> {
    let mut peers = Vec::new();
    for entry in spec.split(',').filter(|s| !s.trim().is_empty()) {
        let (id, addr) = entry
            .split_once('@')
            .ok_or_else(|| format!("peer '{}' must be id@host:port", entry))?;
        let (host, port) = addr
            .rsplit_once(':')
            .ok_or_else(|| format!("peer '{}' must be id@host:port", entry))?;
        let port: u16 = port
            .parse()
            .map_err(|_| format!("peer '{}' has an invalid port", entry))?;
        peers.push((id.trim().to_string(), host.trim().to_string(), port));
    }
    Ok(peers)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let filter = std::env::var("RUST_LOG")
        .unwrap_or_else(|_| std::env::var("BLOCKDB_LOG_LEVEL").unwrap_or_else(|_| "info".to_string()));
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(filter))
        .init();

    let args = Args::parse();
    let peers = parse_peers(&args.peers)?;

    let cluster_config = build_cluster_config(
        &args.node_id,
        &args.host,
        args.port,
        &peers,
        Duration::from_millis(args.heartbeat_ms),
        Duration::from_millis(args.election_timeout_ms),
    );

    let storage_config = BlockDBConfig {
        data_dir: args.data_dir.clone(),
        ..Default::default()
    };

    std::fs::create_dir_all(&args.data_dir)?;

    tracing::info!(
        node_id = %args.node_id,
        host = %args.host,
        port = args.port,
        peers = peers.len(),
        "starting BlockDB cluster node"
    );

    let tls = match (
        args.tls_cert.filter(|s| !s.is_empty()),
        args.tls_key.filter(|s| !s.is_empty()),
    ) {
        (Some(cert_path), Some(key_path)) => Some(blockdb::cluster::TlsConfig { cert_path, key_path }),
        (None, None) => None,
        _ => {
            return Err("both --tls-cert and --tls-key must be provided together".into());
        }
    };
    let security = ClusterSecurity {
        cluster_secret: args.cluster_secret.filter(|s| !s.is_empty()),
        api_token: args.api_token.filter(|s| !s.is_empty()),
        tls,
    };
    if security.cluster_secret.is_none() {
        tracing::warn!("no --cluster-secret set: /raft/message is UNAUTHENTICATED");
    }
    if security.api_token.is_none() {
        tracing::warn!("no --api-token set: /kv is UNAUTHENTICATED");
    }
    if security.tls.is_none() {
        tracing::warn!("no --tls-cert/--tls-key set: traffic (incl. secrets) is PLAINTEXT");
    }

    let node = ClusterNode::new_secure(cluster_config, storage_config, security)?;
    node.start(shutdown_signal()).await?;
    Ok(())
}
