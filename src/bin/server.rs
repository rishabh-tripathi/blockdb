use blockdb::{BlockDBConfig, BlockDBHandle, BlockDBServer, ApiConfig};
use clap::Parser;
use std::path::Path;
use tokio;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    #[arg(short, long, default_value = "8080")]
    port: u16,

    #[arg(short, long, default_value = "./blockdb_data")]
    data_dir: String,

    #[arg(long, default_value = "67108864")]
    memtable_size: usize,

    #[arg(long, default_value = "1000")]
    wal_sync_interval: u64,

    #[arg(long, default_value = "4")]
    compaction_threshold: usize,

    #[arg(long, default_value = "1000")]
    blockchain_batch_size: usize,

    #[arg(long, default_value = "1000")]
    max_connections: usize,

    #[arg(long, default_value = "30")]
    request_timeout: u64,

    #[arg(long)]
    disable_cors: bool,

    #[arg(long)]
    disable_compression: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    let db_config = BlockDBConfig {
        data_dir: args.data_dir,
        memtable_size_limit: args.memtable_size,
        wal_sync_interval: args.wal_sync_interval,
        compaction_threshold: args.compaction_threshold,
        blockchain_batch_size: args.blockchain_batch_size,
    };

    let api_config = ApiConfig {
        host: args.host,
        port: args.port,
        max_connections: args.max_connections,
        request_timeout: args.request_timeout,
        enable_cors: !args.disable_cors,
        enable_compression: !args.disable_compression,
    };

    println!("Starting BlockDB server with config:");
    println!("  Data directory: {}", db_config.data_dir);
    println!("  Memtable size: {} MB", db_config.memtable_size_limit / 1024 / 1024);
    println!("  Server address: {}:{}", api_config.host, api_config.port);
    println!("  Max connections: {}", api_config.max_connections);

    if !Path::new(&db_config.data_dir).exists() {
        std::fs::create_dir_all(&db_config.data_dir)?;
        println!("Created data directory: {}", db_config.data_dir);
    }

    let db_handle = BlockDBHandle::new(db_config)?;
    let server = BlockDBServer::new(db_handle, api_config);

    println!("BlockDB server initialized successfully");
    println!("Starting HTTP server...");

    if let Err(e) = server.start().await {
        eprintln!("Server error: {}", e);
        std::process::exit(1);
    }

    Ok(())
}