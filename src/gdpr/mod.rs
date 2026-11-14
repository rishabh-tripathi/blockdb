//! GDPR support: crypto-shredding for right-to-erasure on an append-only store.
//!
//! An append-only, blockchain-verified store cannot delete records — which is exactly
//! what GDPR Article 17 (right to erasure) requires for personal data. Crypto-shredding
//! reconciles the two: personal data is stored **encrypted** under a per-subject key,
//! and "erasure" destroys the key rather than the record. The ciphertext remains in the
//! immutable log (so the chain stays intact and verifiable), but it is permanently
//! unrecoverable once the key is gone.
//!
//! Key management:
//!   * Each subject has a random 256-bit data key.
//!   * Data keys are stored **wrapped** (encrypted) under a master key in a mutable
//!     keyring file — NOT in the immutable store — so they can actually be deleted.
//!   * The master key is supplied by the operator (env/KMS). In a cluster every node
//!     shares the master key; erasing a subject deletes its wrapped data key on each
//!     node, after which no node can decrypt that subject's records.
//!
//! This module provides the keyring + encrypt/decrypt/erase primitive; `consent`
//! layers it over consent events.

pub mod crypto_shred;

pub use crypto_shred::{MasterKey, SubjectKeyring};
