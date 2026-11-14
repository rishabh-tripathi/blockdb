//! `blockdb-admin` — operator tooling for a BlockDB cluster.
//!
//! Local, offline commands (no server needed):
//!   backup / verify-backup / restore — consistent data-directory backup with an
//!   integrity manifest.
//!
//! Cluster commands (talk to a node's HTTP API; require the cluster secret):
//!   status / add-node / remove-node.
//!
//! Cluster commands use plain HTTP. For a TLS-enabled cluster, run this from inside the
//! trusted network against the node's address, or use curl with the CA.

use std::path::PathBuf;

use blockdb::backup;
use clap::{Parser, Subcommand};
use hyper::{Body, Client, Method, Request};

#[derive(Parser)]
#[command(author, version, about = "BlockDB cluster admin tool", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show a node's status and health.
    Status {
        /// Node base URL, e.g. http://127.0.0.1:8080
        #[arg(long)]
        addr: String,
    },
    /// Add a node to the cluster (sent to the current leader).
    AddNode {
        #[arg(long)]
        addr: String,
        #[arg(long, env = "BLOCKDB_CLUSTER_SECRET")]
        cluster_secret: String,
        #[arg(long)]
        id: String,
        #[arg(long)]
        host: String,
        #[arg(long)]
        port: u16,
    },
    /// Remove a node from the cluster (sent to the current leader).
    RemoveNode {
        #[arg(long)]
        addr: String,
        #[arg(long, env = "BLOCKDB_CLUSTER_SECRET")]
        cluster_secret: String,
        #[arg(long)]
        id: String,
    },
    /// Back up a data directory to a destination directory.
    Backup {
        #[arg(long)]
        data_dir: PathBuf,
        #[arg(long)]
        dest: PathBuf,
    },
    /// Verify a backup directory against its manifest.
    VerifyBackup {
        #[arg(long)]
        dir: PathBuf,
    },
    /// Restore a backup into a data directory.
    Restore {
        #[arg(long)]
        from: PathBuf,
        #[arg(long)]
        data_dir: PathBuf,
        /// Overwrite a non-empty target directory.
        #[arg(long)]
        force: bool,
    },
}

async fn post_json(
    addr: &str,
    path: &str,
    secret: &str,
    body: serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let uri = format!("{}{}", addr.trim_end_matches('/'), path);
    let req = Request::builder()
        .method(Method::POST)
        .uri(&uri)
        .header("content-type", "application/json")
        .header("x-cluster-auth", secret)
        .body(Body::from(serde_json::to_vec(&body)?))?;
    let resp = Client::new().request(req).await?;
    let status = resp.status();
    let bytes = hyper::body::to_bytes(resp.into_body()).await?;
    let text = String::from_utf8_lossy(&bytes);
    if status.is_success() {
        println!("{}", text);
        Ok(())
    } else {
        Err(format!("request failed ({}): {}", status, text).into())
    }
}

async fn get(addr: &str, path: &str) -> Result<String, Box<dyn std::error::Error>> {
    let uri = format!("{}{}", addr.trim_end_matches('/'), path);
    let resp = Client::new().get(uri.parse()?).await?;
    let status = resp.status();
    let bytes = hyper::body::to_bytes(resp.into_body()).await?;
    let text = String::from_utf8_lossy(&bytes).to_string();
    if status.is_success() {
        Ok(text)
    } else {
        Err(format!("request failed ({}): {}", status, text).into())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.command {
        Command::Status { addr } => {
            println!("status:  {}", get(&addr, "/status").await?);
            println!("health:  {}", get(&addr, "/health").await?);
        }
        Command::AddNode { addr, cluster_secret, id, host, port } => {
            post_json(
                &addr,
                "/cluster/add-node",
                &cluster_secret,
                serde_json::json!({"node_id": id, "host": host, "port": port}),
            )
            .await?;
        }
        Command::RemoveNode { addr, cluster_secret, id } => {
            post_json(
                &addr,
                "/cluster/remove-node",
                &cluster_secret,
                serde_json::json!({"node_id": id}),
            )
            .await?;
        }
        Command::Backup { data_dir, dest } => {
            let m = backup::backup(&data_dir, &dest)?;
            println!(
                "backed up {} files from {} to {}",
                m.files.len(),
                data_dir.display(),
                dest.display()
            );
        }
        Command::VerifyBackup { dir } => {
            let n = backup::verify(&dir)?;
            println!("backup OK: {} files verified against manifest", n);
        }
        Command::Restore { from, data_dir, force } => {
            let n = backup::restore(&from, &data_dir, force)?;
            println!("restored {} files to {}", n, data_dir.display());
        }
    }
    Ok(())
}
