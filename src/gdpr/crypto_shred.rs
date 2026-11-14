//! Crypto-shredding keyring: per-subject AES-256-GCM data keys, wrapped under a master
//! key and held in a mutable, deletable keyring. Destroying a subject's key makes all
//! of that subject's ciphertext permanently unrecoverable — erasure on an immutable
//! store.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use rand::RngCore;
use serde::{Deserialize, Serialize};

use crate::error::BlockDBError;

/// A 256-bit master key used to wrap per-subject data keys. In production this comes
/// from a KMS/HSM or a secret manager; the same value must be shared by every node.
#[derive(Clone)]
pub struct MasterKey([u8; 32]);

impl MasterKey {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        MasterKey(bytes)
    }

    /// Parse a base64-encoded 32-byte key (e.g. from `BLOCKDB_MASTER_KEY`).
    pub fn from_base64(s: &str) -> Result<Self, BlockDBError> {
        use base64::{engine::general_purpose::STANDARD, Engine};
        let raw = STANDARD
            .decode(s.trim())
            .map_err(|e| BlockDBError::InvalidData(format!("master key not valid base64: {}", e)))?;
        let arr: [u8; 32] = raw
            .as_slice()
            .try_into()
            .map_err(|_| BlockDBError::InvalidData("master key must be 32 bytes".to_string()))?;
        Ok(MasterKey(arr))
    }

    /// Generate a fresh random master key (for tests / single-node bootstrap).
    pub fn generate() -> Self {
        let mut b = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut b);
        MasterKey(b)
    }

    pub fn to_base64(&self) -> String {
        use base64::{engine::general_purpose::STANDARD, Engine};
        STANDARD.encode(self.0)
    }

    fn cipher(&self) -> Aes256Gcm {
        Aes256Gcm::new_from_slice(&self.0).expect("32-byte key")
    }
}

/// A single wrapped (encrypted) subject data key: `nonce || ciphertext(data_key)`.
#[derive(Clone, Serialize, Deserialize)]
struct WrappedKey {
    nonce: [u8; 12],
    ct: Vec<u8>,
}

#[derive(Default, Serialize, Deserialize)]
struct KeyringFile {
    keys: HashMap<String, WrappedKey>,
}

/// Mutable keyring of per-subject data keys, persisted to a file outside the immutable
/// store so keys can be destroyed.
pub struct SubjectKeyring {
    master: MasterKey,
    path: PathBuf,
    keys: RwLock<HashMap<String, WrappedKey>>,
}

fn aead_encrypt(cipher: &Aes256Gcm, plaintext: &[u8]) -> ([u8; 12], Vec<u8>) {
    let mut nonce = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce);
    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce), plaintext)
        .expect("AES-GCM encryption does not fail for valid key/nonce");
    (nonce, ct)
}

fn aead_decrypt(cipher: &Aes256Gcm, nonce: &[u8; 12], ct: &[u8]) -> Result<Vec<u8>, BlockDBError> {
    cipher
        .decrypt(Nonce::from_slice(nonce), ct)
        .map_err(|_| BlockDBError::InvalidData("decryption failed (wrong key or tampered)".into()))
}

impl SubjectKeyring {
    /// Open (or create) a keyring file under `dir`, wrapping keys with `master`.
    pub fn open(dir: &Path, master: MasterKey) -> Result<Self, BlockDBError> {
        std::fs::create_dir_all(dir)
            .map_err(|e| BlockDBError::StorageError(format!("create keyring dir: {}", e)))?;
        let path = dir.join("subject_keys.bin");
        let keys = if path.exists() {
            let bytes = std::fs::read(&path)
                .map_err(|e| BlockDBError::StorageError(format!("read keyring: {}", e)))?;
            let kf: KeyringFile = bincode::deserialize(&bytes).unwrap_or_default();
            kf.keys
        } else {
            HashMap::new()
        };
        Ok(SubjectKeyring {
            master,
            path,
            keys: RwLock::new(keys),
        })
    }

    fn persist(&self) -> Result<(), BlockDBError> {
        let kf = KeyringFile {
            keys: self.keys.read().unwrap().clone(),
        };
        let bytes = bincode::serialize(&kf)
            .map_err(|e| BlockDBError::StorageError(format!("serialize keyring: {}", e)))?;
        let tmp = self.path.with_extension("tmp");
        std::fs::write(&tmp, &bytes)
            .and_then(|_| std::fs::rename(&tmp, &self.path))
            .map_err(|e| BlockDBError::StorageError(format!("persist keyring: {}", e)))?;
        Ok(())
    }

    /// Return the plaintext data key for `subject`, creating and persisting a fresh one
    /// if none exists. Returns `None` only if the subject was erased... but since we
    /// create on demand, this always returns a key unless the master key is wrong.
    fn data_key(&self, subject: &str) -> Result<[u8; 32], BlockDBError> {
        if let Some(wrapped) = self.keys.read().unwrap().get(subject).cloned() {
            let dk = aead_decrypt(&self.master.cipher(), &wrapped.nonce, &wrapped.ct)?;
            let arr: [u8; 32] = dk
                .as_slice()
                .try_into()
                .map_err(|_| BlockDBError::InvalidData("corrupt data key".into()))?;
            return Ok(arr);
        }
        // Create a new random data key, wrap it under the master key, persist.
        let mut dk = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut dk);
        let (nonce, ct) = aead_encrypt(&self.master.cipher(), &dk);
        self.keys
            .write()
            .unwrap()
            .insert(subject.to_string(), WrappedKey { nonce, ct });
        self.persist()?;
        Ok(dk)
    }

    /// The data key for `subject` only if it still exists (not erased, present on disk).
    fn existing_data_key(&self, subject: &str) -> Result<Option<[u8; 32]>, BlockDBError> {
        let Some(wrapped) = self.keys.read().unwrap().get(subject).cloned() else {
            return Ok(None);
        };
        let dk = aead_decrypt(&self.master.cipher(), &wrapped.nonce, &wrapped.ct)?;
        let arr: [u8; 32] = dk
            .as_slice()
            .try_into()
            .map_err(|_| BlockDBError::InvalidData("corrupt data key".into()))?;
        Ok(Some(arr))
    }

    /// Encrypt `plaintext` for `subject`, returning `nonce || ciphertext`.
    pub fn encrypt(&self, subject: &str, plaintext: &[u8]) -> Result<Vec<u8>, BlockDBError> {
        let dk = self.data_key(subject)?;
        let cipher = Aes256Gcm::new_from_slice(&dk).expect("32-byte key");
        let (nonce, ct) = aead_encrypt(&cipher, plaintext);
        let mut out = Vec::with_capacity(12 + ct.len());
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&ct);
        Ok(out)
    }

    /// Decrypt a `nonce || ciphertext` blob for `subject`. Returns
    /// `Ok(None)` if the subject's key has been erased (data is unrecoverable).
    pub fn decrypt(&self, subject: &str, blob: &[u8]) -> Result<Option<Vec<u8>>, BlockDBError> {
        let Some(dk) = self.existing_data_key(subject)? else {
            return Ok(None); // erased
        };
        if blob.len() < 12 {
            return Err(BlockDBError::InvalidData("ciphertext too short".into()));
        }
        let cipher = Aes256Gcm::new_from_slice(&dk).expect("32-byte key");
        let mut nonce = [0u8; 12];
        nonce.copy_from_slice(&blob[..12]);
        Ok(Some(aead_decrypt(&cipher, &nonce, &blob[12..])?))
    }

    /// Whether this subject still has a live key (false after erasure).
    pub fn has_key(&self, subject: &str) -> bool {
        self.keys.read().unwrap().contains_key(subject)
    }

    /// Crypto-shred: permanently destroy `subject`'s data key. All ciphertext for the
    /// subject becomes unrecoverable. Returns whether a key was present.
    pub fn erase(&self, subject: &str) -> Result<bool, BlockDBError> {
        let removed = self.keys.write().unwrap().remove(subject).is_some();
        if removed {
            self.persist()?;
        }
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let dir = TempDir::new().unwrap();
        let kr = SubjectKeyring::open(dir.path(), MasterKey::generate()).unwrap();
        let blob = kr.encrypt("alice", b"sensitive personal data").unwrap();
        assert_ne!(blob, b"sensitive personal data");
        let out = kr.decrypt("alice", &blob).unwrap().unwrap();
        assert_eq!(out, b"sensitive personal data");
    }

    #[test]
    fn erase_makes_data_unrecoverable() {
        let dir = TempDir::new().unwrap();
        let kr = SubjectKeyring::open(dir.path(), MasterKey::generate()).unwrap();
        let blob = kr.encrypt("bob", b"bob's data").unwrap();
        assert!(kr.has_key("bob"));

        assert!(kr.erase("bob").unwrap());
        assert!(!kr.has_key("bob"));
        // The ciphertext is still here, but it can never be decrypted again.
        assert!(kr.decrypt("bob", &blob).unwrap().is_none());
        // Erasing again is a no-op.
        assert!(!kr.erase("bob").unwrap());
    }

    #[test]
    fn keys_persist_across_reopen() {
        let dir = TempDir::new().unwrap();
        let master = MasterKey::generate();
        let blob = {
            let kr = SubjectKeyring::open(dir.path(), master.clone()).unwrap();
            kr.encrypt("carol", b"hello").unwrap()
        };
        // Reopen with the same master key: the subject key is still wrapped on disk.
        let kr2 = SubjectKeyring::open(dir.path(), master).unwrap();
        assert_eq!(kr2.decrypt("carol", &blob).unwrap().unwrap(), b"hello");
    }

    #[test]
    fn wrong_master_key_cannot_unwrap() {
        let dir = TempDir::new().unwrap();
        let blob = {
            let kr = SubjectKeyring::open(dir.path(), MasterKey::generate()).unwrap();
            kr.encrypt("dave", b"secret").unwrap()
        };
        // A different master key cannot unwrap the stored data key.
        let kr2 = SubjectKeyring::open(dir.path(), MasterKey::generate()).unwrap();
        assert!(kr2.decrypt("dave", &blob).is_err());
    }

    #[test]
    fn master_key_base64_roundtrip() {
        let m = MasterKey::generate();
        let b64 = m.to_base64();
        let m2 = MasterKey::from_base64(&b64).unwrap();
        assert_eq!(m.0, m2.0);
        assert!(MasterKey::from_base64("short").is_err());
    }
}
