use blockdb::{BlockDBConfig, BlockDBHandle};
use tokio;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = BlockDBConfig {
        data_dir: "./example_data".to_string(),
        memtable_size_limit: 1024 * 1024, // 1MB
        ..Default::default()
    };

    let db = BlockDBHandle::new(config)?;

    println!("Writing data to BlockDB...");
    
    db.put(b"user:1", b"Alice").await?;
    db.put(b"user:2", b"Bob").await?;
    db.put(b"user:3", b"Charlie").await?;
    db.put(b"config:timeout", b"30").await?;
    db.put(b"config:retries", b"3").await?;

    println!("Reading data from BlockDB...");
    
    if let Some(value) = db.get(b"user:1").await? {
        println!("user:1 = {}", String::from_utf8_lossy(&value));
    }
    
    if let Some(value) = db.get(b"config:timeout").await? {
        println!("config:timeout = {}", String::from_utf8_lossy(&value));
    }
    
    if let Some(value) = db.get(b"nonexistent").await? {
        println!("This shouldn't print");
    } else {
        println!("Key 'nonexistent' not found (expected)");
    }

    println!("Verifying blockchain integrity...");
    let is_valid = db.verify_integrity().await?;
    println!("Blockchain integrity: {}", if is_valid { "✓ Valid" } else { "✗ Invalid" });

    // Clean up
    std::fs::remove_dir_all("./example_data").ok();
    
    println!("Example completed successfully!");
    Ok(())
}