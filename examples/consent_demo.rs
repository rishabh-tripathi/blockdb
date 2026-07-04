//! Demonstrates the blockchain-backed consent store.
//!
//! Run with: `cargo run --example consent_demo`

use blockdb::{BlockDBConfig, BlockDBHandle, ConsentStore};
use std::collections::BTreeMap;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let config = BlockDBConfig {
        data_dir: dir.path().to_string_lossy().to_string(),
        ..Default::default()
    };
    let db = BlockDBHandle::new(config)?;
    let consent = ConsentStore::new(db);

    // Alice grants consent for marketing, with some contextual metadata.
    let mut meta = BTreeMap::new();
    meta.insert("policy_version".to_string(), "2024-05".to_string());
    meta.insert("source".to_string(), "signup_form".to_string());
    let event = consent.grant("alice", "marketing", meta).await?;
    println!("granted: {} (proof {})", event.event_id, &event.proof_hash[..16]);

    println!("is_granted(marketing) = {}", consent.is_granted("alice", "marketing").await?);

    // Later, Alice withdraws it.
    consent.withdraw("alice", "marketing", BTreeMap::new()).await?;
    println!("is_granted(marketing) = {}", consent.is_granted("alice", "marketing").await?);

    // The full, immutable history is available for audit.
    println!("\naudit trail for alice/marketing:");
    for e in consent.history("alice", "marketing").await? {
        println!("  {} at {}ms  (verified: {})", e.status, e.timestamp_ms, e.verify());
    }

    // Blockchain integrity of the whole store.
    println!("\nblockchain integrity verified: {}", consent.verify_integrity().await?);
    Ok(())
}
