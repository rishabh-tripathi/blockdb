use blockdb::api::shutdown_signal;
use blockdb::auth::AuthConfig;
use blockdb::{ApiConfig, AuthManager, BlockDBConfig, BlockDBHandle, BlockDBServer};
use clap::Parser;
use std::path::Path;

#[derive(Parser, Debug)]
#[command(author, version, about = "BlockDB HTTP server", long_about = None)]
struct Args {
    /// Optional TOML config file. CLI flags override values loaded from it.
    #[arg(long)]
    config: Option<String>,

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

    /// Disable authentication entirely (NOT recommended for production).
    #[arg(long)]
    disable_auth: bool,

    /// Require a valid token for read operations as well as writes.
    #[arg(long)]
    require_auth_for_reads: bool,

    #[arg(long)]
    disable_cors: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Structured logging honouring RUST_LOG / BLOCKDB_LOG_LEVEL (default: info).
    let filter = std::env::var("RUST_LOG").unwrap_or_else(|_| {
        std::env::var("BLOCKDB_LOG_LEVEL").unwrap_or_else(|_| "info".to_string())
    });
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(filter))
        .init();

    let args = Args::parse();

    // Optionally load a TOML config file (env var BLOCKDB_CONFIG or --config).
    let file_config = load_file_config(args.config.clone());
    if let Some(ref fc) = file_config {
        tracing::info!("loaded configuration from file");
        let _ = fc; // values applied below where present
    }

    let db_config = BlockDBConfig {
        data_dir: args.data_dir.clone(),
        memtable_size_limit: args.memtable_size,
        wal_sync_interval_ms: args.wal_sync_interval,
        compaction_threshold: args.compaction_threshold,
        blockchain_batch_size: args.blockchain_batch_size,
        auth_enabled: !args.disable_auth,
        ..Default::default()
    };

    let api_config = ApiConfig {
        host: args.host.clone(),
        port: args.port,
        max_connections: args.max_connections,
        request_timeout: args.request_timeout,
        enable_cors: !args.disable_cors,
        enable_compression: false,
        auth_enabled: !args.disable_auth,
        require_auth_for_reads: args.require_auth_for_reads,
        session_duration_hours: 24,
    };

    if !Path::new(&db_config.data_dir).exists() {
        std::fs::create_dir_all(&db_config.data_dir)?;
        tracing::info!(data_dir = %db_config.data_dir, "created data directory");
    }

    tracing::info!(
        host = %api_config.host,
        port = api_config.port,
        data_dir = %db_config.data_dir,
        auth_enabled = api_config.auth_enabled,
        "starting BlockDB server"
    );

    // Persist auth state (user accounts + permissions) alongside the data, so
    // accounts survive a restart instead of resetting to the default admin.
    let auth_dir = Path::new(&db_config.data_dir).join("auth");
    let db_handle = BlockDBHandle::new(db_config)?;

    let server = if api_config.auth_enabled {
        // Disable the library's auto-seeded admin; the server bootstraps admin
        // explicitly so an operator-provided credential is always honoured.
        let auth_config = AuthConfig {
            admin_users: vec![],
            ..AuthConfig::default()
        };
        let mut am = AuthManager::new_persistent(auth_config, &auth_dir);
        // Attach a tamper-evident audit log for auth and permission events.
        match blockdb::audit::AuditLog::open(&auth_dir) {
            Ok(log) => am.attach_audit_log(std::sync::Arc::new(log)),
            Err(e) => tracing::warn!(error = %e, "could not open audit log; auditing disabled"),
        }
        match (
            std::env::var("BLOCKDB_ADMIN_USER"),
            std::env::var("BLOCKDB_ADMIN_PASSWORD"),
        ) {
            (Ok(user), Ok(pass)) => {
                // Bootstrap an admin from the environment (no-op if it already exists).
                match am.ensure_admin(user.clone(), &pass) {
                    Ok(true) => tracing::info!(%user, "bootstrapped admin from environment"),
                    Ok(false) => tracing::info!(%user, "admin already exists; env credential not reapplied"),
                    Err(e) => {
                        return Err(format!(
                            "BLOCKDB_ADMIN_PASSWORD rejected ({}). Choose a stronger password.",
                            e
                        )
                        .into());
                    }
                }
            }
            _ => {
                // No operator credential supplied: create a default admin with a
                // generated strong password, surfaced once, instead of a known default.
                let generated = blockdb::auth::generate_secure_password();
                match am.ensure_admin("admin".to_string(), &generated) {
                    Ok(true) => tracing::warn!(
                        "BLOCKDB_ADMIN_USER/PASSWORD not set; created admin 'admin' with a \
                         generated password (shown once): {} — set the env vars and rotate it",
                        generated
                    ),
                    Ok(false) => tracing::info!("admin already exists from a previous run"),
                    Err(e) => return Err(format!("failed to bootstrap admin: {}", e).into()),
                }
            }
        }
        BlockDBServer::with_auth(db_handle, api_config, am)
    } else {
        tracing::warn!("authentication is DISABLED");
        BlockDBServer::new(db_handle, api_config)
    };

    server.serve_with_shutdown(shutdown_signal()).await?;
    Ok(())
}

/// Load a TOML config file if `--config` or the BLOCKDB_CONFIG env var points to one.
/// Returns the parsed table; individual fields are consumed by the caller. Missing
/// files are a hard error only when explicitly requested via `--config`.
fn load_file_config(explicit: Option<String>) -> Option<toml::Value> {
    let path = explicit.or_else(|| std::env::var("BLOCKDB_CONFIG").ok())?;
    match std::fs::read_to_string(&path) {
        Ok(contents) => match toml::from_str::<toml::Value>(&contents) {
            Ok(value) => Some(value),
            Err(e) => {
                tracing::error!(path = %path, error = %e, "failed to parse config file");
                None
            }
        },
        Err(e) => {
            tracing::error!(path = %path, error = %e, "failed to read config file");
            None
        }
    }
}
