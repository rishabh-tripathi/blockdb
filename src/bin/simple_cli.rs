use blockdb::{BlockDBConfig, BlockDBHandle};
use blockdb::storage::collection::{CollectionManager, IndexDefinition};
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
    Flush {
        #[arg(long)]
        force: bool,
    },
    Collection {
        #[command(subcommand)]
        action: CollectionAction,
    },
    Interactive,
}

#[derive(Subcommand, Debug)]
enum CollectionAction {
    Create {
        name: String,
        #[arg(long)]
        description: Option<String>,
    },
    List,
    Drop {
        collection_id: String,
        #[arg(long)]
        force: bool,
    },
    Put {
        collection_id: String,
        key: String,
        value: String,
        #[arg(long)]
        base64: bool,
    },
    Get {
        collection_id: String,
        key: String,
        #[arg(long)]
        base64: bool,
    },
    Stats {
        collection_id: String,
    },
    Verify {
        collection_id: String,
    },
    Flush {
        collection_id: String,
        #[arg(long)]
        force: bool,
    },
    CreateIndex {
        collection_id: String,
        index_name: String,
        #[arg(long)]
        fields: String,
        #[arg(long)]
        unique: bool,
    },
    DropIndex {
        collection_id: String,
        index_name: String,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let config = BlockDBConfig {
        data_dir: args.data_dir,
        ..Default::default()
    };

    let db = BlockDBHandle::new(config.clone())?;
    let collection_manager = CollectionManager::new(config.clone())?;

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
            println!("  WAL sync interval: {} ms", config.wal_sync_interval_ms);
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
        Commands::Flush { force } => {
            if !force {
                println!("⚠️  WARNING: This will delete ALL data in the database!");
                print!("Are you sure you want to continue? (y/N): ");
                io::stdout().flush()?;
                
                let mut confirmation = String::new();
                io::stdin().read_line(&mut confirmation)?;
                
                if !confirmation.trim().to_lowercase().starts_with('y') {
                    println!("Operation cancelled.");
                    return Ok(());
                }
            }
            
            println!("Flushing all database data...");
            db.flush_all().await?;
            println!("✅ Database flushed successfully");
        }
        Commands::Collection { action } => {
            handle_collection_action(action, &collection_manager).await?;
        }
        Commands::Interactive => {
            println!("BlockDB Interactive Mode");
            println!("Commands: put <key> <value>, get <key>, stats, verify, flush, collection <action>, quit");
            
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
                        println!("BlockDB Statistics:");
                        println!("  Data directory: {}", config.data_dir);
                        println!("  Memtable size limit: {} MB", config.memtable_size_limit / 1024 / 1024);
                        println!("  WAL sync interval: {} ms", config.wal_sync_interval_ms);
                        println!("  Compaction threshold: {}", config.compaction_threshold);
                        println!("  Blockchain batch size: {}", config.blockchain_batch_size);
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
                    "flush" => {
                        print!("Are you sure you want to flush all data? (y/N): ");
                        io::stdout().flush()?;
                        let mut confirmation = String::new();
                        io::stdin().read_line(&mut confirmation)?;
                        
                        if confirmation.trim().to_lowercase().starts_with('y') {
                            match db.flush_all().await {
                                Ok(_) => println!("✅ Database flushed successfully"),
                                Err(e) => println!("Error: {}", e),
                            }
                        } else {
                            println!("Flush cancelled");
                        }
                    }
                    "collection" => {
                        if parts.len() >= 2 {
                            handle_interactive_collection_command(&parts[1..], &collection_manager).await?;
                        } else {
                            println!("Usage: collection <create|list|drop|put|get|stats|verify|flush> ...");
                        }
                    }
                    "quit" | "exit" => {
                        println!("Goodbye!");
                        break;
                    }
                    _ => {
                        println!("Unknown command. Available: put, get, stats, verify, flush, collection, quit");
                    }
                }
            }
        }
    }

    Ok(())
}

async fn handle_collection_action(action: CollectionAction, collection_manager: &CollectionManager) -> Result<(), Box<dyn std::error::Error>> {
    match action {
        CollectionAction::Create { name, description } => {
            let collection_id = collection_manager.create_collection(
                name.clone(),
                None, // No schema for now
                None, // Default settings
                None, // No created_by
            )?;
            println!("✅ Collection '{}' created with ID: {}", name, collection_id);
        }
        CollectionAction::List => {
            let collections = collection_manager.list_collections()?;
            if collections.is_empty() {
                println!("No collections found.");
            } else {
                println!("Collections:");
                for collection in collections {
                    println!("  • {} ({}) - {} documents, {} bytes", 
                        collection.name, 
                        collection.id, 
                        collection.stats.document_count,
                        collection.stats.total_size_bytes
                    );
                }
            }
        }
        CollectionAction::Drop { collection_id, force } => {
            if !force {
                println!("⚠️  WARNING: This will delete ALL data in collection '{}'!", collection_id);
                print!("Are you sure you want to continue? (y/N): ");
                io::stdout().flush()?;
                
                let mut confirmation = String::new();
                io::stdin().read_line(&mut confirmation)?;
                
                if !confirmation.trim().to_lowercase().starts_with('y') {
                    println!("Operation cancelled.");
                    return Ok(());
                }
            }
            
            collection_manager.drop_collection(&collection_id)?;
            println!("✅ Collection '{}' dropped successfully", collection_id);
        }
        CollectionAction::Put { collection_id, key, value, base64 } => {
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

            collection_manager.put(&collection_id, &key_bytes, &value_bytes)?;
            println!("✅ Successfully stored document in collection");
        }
        CollectionAction::Get { collection_id, key, base64 } => {
            let key_bytes = if base64 {
                base64::engine::general_purpose::STANDARD.decode(key)?
            } else {
                key.into_bytes()
            };

            match collection_manager.get(&collection_id, &key_bytes)? {
                Some(value) => {
                    if base64 {
                        println!("{}", base64::engine::general_purpose::STANDARD.encode(value));
                    } else {
                        println!("{}", String::from_utf8_lossy(&value));
                    }
                }
                None => {
                    println!("Document not found in collection");
                }
            }
        }
        CollectionAction::Stats { collection_id } => {
            let stats = collection_manager.get_collection_stats(&collection_id)?;
            println!("Collection '{}' Statistics:", collection_id);
            println!("  Document count: {}", stats.document_count);
            println!("  Total size: {} bytes", stats.total_size_bytes);
            println!("  Index size: {} bytes", stats.index_size_bytes);
            println!("  Operations count: {}", stats.operations_count);
            println!("  Last updated: {}", stats.last_updated);
        }
        CollectionAction::Verify { collection_id } => {
            let collections = collection_manager.list_collections()?;
            if let Some(collection_meta) = collections.iter().find(|c| c.id == collection_id) {
                println!("Verifying collection '{}' integrity...", collection_meta.name);
                let collection = collection_manager.get_collection(&collection_id)?;
                let is_valid = collection.verify_integrity()?;
                if is_valid {
                    println!("✓ Collection integrity verified successfully");
                } else {
                    println!("✗ Collection integrity verification failed");
                    std::process::exit(1);
                }
            } else {
                println!("❌ Collection '{}' not found", collection_id);
                std::process::exit(1);
            }
        }
        CollectionAction::Flush { collection_id, force } => {
            if !force {
                println!("⚠️  WARNING: This will delete ALL data in collection '{}'!", collection_id);
                print!("Are you sure you want to continue? (y/N): ");
                io::stdout().flush()?;
                
                let mut confirmation = String::new();
                io::stdin().read_line(&mut confirmation)?;
                
                if !confirmation.trim().to_lowercase().starts_with('y') {
                    println!("Operation cancelled.");
                    return Ok(());
                }
            }
            
            collection_manager.flush_collection(&collection_id)?;
            println!("✅ Collection '{}' flushed successfully", collection_id);
        }
        CollectionAction::CreateIndex { collection_id, index_name, fields, unique } => {
            let field_list: Vec<String> = fields.split(',').map(|s| s.trim().to_string()).collect();
            let index_def = IndexDefinition {
                name: index_name.clone(),
                fields: field_list,
                unique,
                sparse: false,
            };
            
            collection_manager.create_index(&collection_id, index_def)?;
            println!("✅ Index '{}' created successfully", index_name);
        }
        CollectionAction::DropIndex { collection_id, index_name } => {
            collection_manager.drop_index(&collection_id, &index_name)?;
            println!("✅ Index '{}' dropped successfully", index_name);
        }
    }
    Ok(())
}

async fn handle_interactive_collection_command(parts: &[&str], collection_manager: &CollectionManager) -> Result<(), Box<dyn std::error::Error>> {
    match parts[0] {
        "create" => {
            if parts.len() >= 2 {
                let name = parts[1].to_string();
                let collection_id = collection_manager.create_collection(name.clone(), None, None, None)?;
                println!("✅ Collection '{}' created with ID: {}", name, collection_id);
            } else {
                println!("Usage: collection create <name>");
            }
        }
        "list" => {
            let collections = collection_manager.list_collections()?;
            if collections.is_empty() {
                println!("No collections found.");
            } else {
                println!("Collections:");
                for collection in collections {
                    println!("  • {} ({}) - {} documents, {} bytes", 
                        collection.name, collection.id, collection.stats.document_count, collection.stats.total_size_bytes);
                }
            }
        }
        "drop" => {
            if parts.len() >= 2 {
                let collection_id = parts[1];
                print!("Are you sure you want to drop collection '{}'? (y/N): ", collection_id);
                io::stdout().flush()?;
                let mut confirmation = String::new();
                io::stdin().read_line(&mut confirmation)?;
                
                if confirmation.trim().to_lowercase().starts_with('y') {
                    match collection_manager.drop_collection(collection_id) {
                        Ok(_) => println!("✅ Collection dropped successfully"),
                        Err(e) => println!("Error: {}", e),
                    }
                } else {
                    println!("Drop cancelled");
                }
            } else {
                println!("Usage: collection drop <collection_id>");
            }
        }
        "put" => {
            if parts.len() >= 4 {
                let collection_id = parts[1];
                let key = parts[2].as_bytes();
                let value = parts[3..].join(" ").as_bytes().to_vec();
                
                match collection_manager.put(collection_id, key, &value) {
                    Ok(_) => println!("OK"),
                    Err(e) => println!("Error: {}", e),
                }
            } else {
                println!("Usage: collection put <collection_id> <key> <value>");
            }
        }
        "get" => {
            if parts.len() >= 3 {
                let collection_id = parts[1];
                let key = parts[2].as_bytes();
                
                match collection_manager.get(collection_id, key) {
                    Ok(Some(value)) => println!("{}", String::from_utf8_lossy(&value)),
                    Ok(None) => println!("(nil)"),
                    Err(e) => println!("Error: {}", e),
                }
            } else {
                println!("Usage: collection get <collection_id> <key>");
            }
        }
        "stats" => {
            if parts.len() >= 2 {
                let collection_id = parts[1];
                match collection_manager.get_collection_stats(collection_id) {
                    Ok(stats) => {
                        println!("Collection '{}' Statistics:", collection_id);
                        println!("  Document count: {}", stats.document_count);
                        println!("  Total size: {} bytes", stats.total_size_bytes);
                        println!("  Operations count: {}", stats.operations_count);
                    }
                    Err(e) => println!("Error: {}", e),
                }
            } else {
                println!("Usage: collection stats <collection_id>");
            }
        }
        "verify" => {
            if parts.len() >= 2 {
                let collection_id = parts[1];
                match collection_manager.get_collection(collection_id) {
                    Ok(collection) => {
                        print!("Verifying collection...");
                        io::stdout().flush()?;
                        match collection.verify_integrity() {
                            Ok(true) => println!(" ✓ OK"),
                            Ok(false) => println!(" ✗ FAILED"),
                            Err(e) => println!(" Error: {}", e),
                        }
                    }
                    Err(e) => println!("Error: {}", e),
                }
            } else {
                println!("Usage: collection verify <collection_id>");
            }
        }
        "flush" => {
            if parts.len() >= 2 {
                let collection_id = parts[1];
                print!("Are you sure you want to flush collection '{}'? (y/N): ", collection_id);
                io::stdout().flush()?;
                let mut confirmation = String::new();
                io::stdin().read_line(&mut confirmation)?;
                
                if confirmation.trim().to_lowercase().starts_with('y') {
                    match collection_manager.flush_collection(collection_id) {
                        Ok(_) => println!("✅ Collection flushed successfully"),
                        Err(e) => println!("Error: {}", e),
                    }
                } else {
                    println!("Flush cancelled");
                }
            } else {
                println!("Usage: collection flush <collection_id>");
            }
        }
        _ => {
            println!("Unknown collection command. Available: create, list, drop, put, get, stats, verify, flush");
        }
    }
    Ok(())
}