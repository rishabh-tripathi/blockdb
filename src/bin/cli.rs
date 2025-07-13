use blockdb::{BlockDBConfig, BlockDBHandle};
use clap::{Parser, Subcommand};
use std::io::{self, Write};
use base64::Engine;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long, default_value = "./blockdb_data")]
    data_dir: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Put {
        key: String,
        value: String,
        #[arg(long)]
        base64: bool,
    },
    Get {
        key: String,
        #[arg(long)]
        base64: bool,
    },
    Stats,
    Verify,
    Interactive,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let config = BlockDBConfig {
        data_dir: args.data_dir,
        ..Default::default()
    };

    let db = BlockDBHandle::new(config.clone())?;

    match args.command {
        Commands::Put { key, value, base64 } => {
            let key_bytes = if base64 {
                base64::engine::general_purpose::STANDARD.decode(key)?
            } else {
                key.into_bytes()
            };

            let value_bytes = if base64 {
                base64::engine::general_purpose::STANDARD.decode(value)?
            } else {
                value.into_bytes()
            };

            db.put(&key_bytes, &value_bytes).await?;
            println!("Successfully stored key-value pair");
        }
        Commands::Get { key, base64 } => {
            let key_bytes = if base64 {
                base64::engine::general_purpose::STANDARD.decode(key)?
            } else {
                key.into_bytes()
            };

            match db.get(&key_bytes).await? {
                Some(value) => {
                    if base64 {
                        println!("{}", base64::engine::general_purpose::STANDARD.encode(value));
                    } else {
                        println!("{}", String::from_utf8_lossy(&value));
                    }
                }
                None => {
                    println!("Key not found");
                }
            }
        }
        Commands::Stats => {
            println!("BlockDB Statistics:");
            println!("  Data directory: {}", config.data_dir);
            println!("  Memtable size limit: {} MB", config.memtable_size_limit / 1024 / 1024);
            println!("  WAL sync interval: {} ms", config.wal_sync_interval);
            println!("  Compaction threshold: {}", config.compaction_threshold);
            println!("  Blockchain batch size: {}", config.blockchain_batch_size);
        }
        Commands::Verify => {
            println!("Verifying blockchain integrity...");
            let is_valid = db.verify_integrity().await?;
            if is_valid {
                println!("✓ Blockchain integrity verified successfully");
            } else {
                println!("✗ Blockchain integrity verification failed");
                std::process::exit(1);
            }
        }
        Commands::Interactive => {
            println!("BlockDB Interactive Mode");
            println!("Commands: put <key> <value>, get <key>, stats, verify, quit");
            
            loop {
                print!("blockdb> ");
                io::stdout().flush()?;
                
                let mut input = String::new();
                io::stdin().read_line(&mut input)?;
                
                let parts: Vec<&str> = input.trim().split_whitespace().collect();
                if parts.is_empty() {
                    continue;
                }
                
                match parts[0] {
                    "put" => {
                        if parts.len() >= 3 {
                            let key = parts[1].as_bytes();
                            let value = parts[2..].join(" ").as_bytes().to_vec();
                            
                            match db.put(key, &value).await {
                                Ok(_) => println!("OK"),
                                Err(e) => println!("Error: {}", e),
                            }
                        } else {
                            println!("Usage: put <key> <value>");
                        }
                    }
                    "get" => {
                        if parts.len() >= 2 {
                            let key = parts[1].as_bytes();
                            
                            match db.get(key).await {
                                Ok(Some(value)) => {
                                    println!("{}", String::from_utf8_lossy(&value));
                                }
                                Ok(None) => println!("(nil)"),
                                Err(e) => println!("Error: {}", e),
                            }
                        } else {
                            println!("Usage: get <key>");
                        }
                    }
                    "stats" => {
                        println!("Statistics not yet implemented in interactive mode");
                    }
                    "verify" => {
                        print!("Verifying...");
                        io::stdout().flush()?;
                        match db.verify_integrity().await {
                            Ok(true) => println!(" ✓ OK"),
                            Ok(false) => println!(" ✗ FAILED"),
                            Err(e) => println!(" Error: {}", e),
                        }
                    }
                    "quit" | "exit" => {
                        println!("Goodbye!");
                        break;
                    }
                    _ => {
                        println!("Unknown command. Available: put, get, stats, verify, quit");
                    }
                }
            }
        }
    }

    Ok(())
}