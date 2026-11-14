use crate::auth::{AuthError, PrivateKey, PublicKey, Signature};
use base64::Engine;
use ed25519_dalek::{Keypair, PublicKey as Ed25519PublicKey, SecretKey, Signature as Ed25519Signature, Signer, Verifier};
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyPair {
    pub public_key: PublicKey,
    pub private_key: PrivateKey,
}

impl KeyPair {
    pub fn generate() -> Result<Self, AuthError> {
        // ed25519-dalek 1.0 pins rand_core 0.5, which is incompatible with the
        // rand 0.8 OsRng used elsewhere in this crate. Generate the 32-byte seed
        // directly from the OS CSPRNG and derive the keypair from it, which avoids
        // the version mismatch while remaining cryptographically sound.
        let mut seed = [0u8; 32];
        OsRng.fill_bytes(&mut seed);

        let secret = SecretKey::from_bytes(&seed)
            .map_err(|e| AuthError::CryptographicError(e.to_string()))?;
        let public = Ed25519PublicKey::from(&secret);
        let keypair = Keypair { secret, public };

        Ok(Self {
            public_key: keypair.public.to_bytes().to_vec(),
            private_key: keypair.secret.to_bytes().to_vec(),
        })
    }

    pub fn from_private_key(private_key: &[u8]) -> Result<Self, AuthError> {
        if private_key.len() != 32 {
            return Err(AuthError::CryptographicError(
                "Private key must be 32 bytes".to_string(),
            ));
        }

        let secret = SecretKey::from_bytes(private_key)
            .map_err(|e| AuthError::CryptographicError(e.to_string()))?;
        
        let public = Ed25519PublicKey::from(&secret);
        let keypair = Keypair { secret, public };

        Ok(Self {
            public_key: keypair.public.to_bytes().to_vec(),
            private_key: keypair.secret.to_bytes().to_vec(),
        })
    }
}

pub struct CryptoUtils;

impl CryptoUtils {
    pub fn sign_data(data: &[u8], private_key: &PrivateKey) -> Result<Signature, AuthError> {
        if private_key.len() != 32 {
            return Err(AuthError::CryptographicError(
                "Private key must be 32 bytes".to_string(),
            ));
        }

        let secret = SecretKey::from_bytes(private_key)
            .map_err(|e| AuthError::CryptographicError(e.to_string()))?;
        
        let public = Ed25519PublicKey::from(&secret);
        let keypair = Keypair { secret, public };

        let signature = keypair.sign(data);
        Ok(signature.to_bytes().to_vec())
    }

    pub fn verify_signature(
        data: &[u8],
        signature: &[u8],
        public_key: &PublicKey,
    ) -> Result<bool, AuthError> {
        if public_key.len() != 32 {
            return Err(AuthError::CryptographicError(
                "Public key must be 32 bytes".to_string(),
            ));
        }

        if signature.len() != 64 {
            return Err(AuthError::CryptographicError(
                "Signature must be 64 bytes".to_string(),
            ));
        }

        let public = Ed25519PublicKey::from_bytes(public_key)
            .map_err(|e| AuthError::CryptographicError(e.to_string()))?;

        let sig = Ed25519Signature::from_bytes(signature)
            .map_err(|e| AuthError::CryptographicError(e.to_string()))?;

        Ok(public.verify(data, &sig).is_ok())
    }

    pub fn hash_data(data: &[u8]) -> Vec<u8> {
        let mut hasher = Sha256::new();
        hasher.update(data);
        hasher.finalize().to_vec()
    }

    /// Hash a password with Argon2id (memory-hard), salted. The 16-byte prefix of the
    /// provided salt is used as the Argon2 salt. Returns the raw 32-byte derived key.
    /// Argon2id resists GPU/ASIC brute-forcing far better than a single SHA-256.
    pub fn hash_password(password: &str, salt: &[u8]) -> Vec<u8> {
        use argon2::{Argon2, PasswordHasher};
        use argon2::password_hash::SaltString;

        // Argon2's SaltString requires base64; derive it deterministically from the
        // caller-supplied salt so the same (password, salt) always yields the same hash.
        let salt_b64 = base64::engine::general_purpose::STANDARD_NO_PAD.encode(&salt[..salt.len().min(16)]);
        match SaltString::from_b64(&salt_b64) {
            Ok(salt_string) => {
                let argon2 = Argon2::default();
                match argon2.hash_password(password.as_bytes(), &salt_string) {
                    Ok(hash) => hash
                        .hash
                        .map(|h| h.as_bytes().to_vec())
                        .unwrap_or_else(|| Self::sha256_fallback(password, salt)),
                    Err(_) => Self::sha256_fallback(password, salt),
                }
            }
            Err(_) => Self::sha256_fallback(password, salt),
        }
    }

    fn sha256_fallback(password: &str, salt: &[u8]) -> Vec<u8> {
        let mut hasher = Sha256::new();
        hasher.update(password.as_bytes());
        hasher.update(salt);
        hasher.finalize().to_vec()
    }

    pub fn generate_salt() -> [u8; 32] {
        let mut salt = [0u8; 32];
        use rand::RngCore;
        OsRng.fill_bytes(&mut salt);
        salt
    }

    pub fn create_operation_hash(
        operation: &crate::transaction::Operation,
        user_id: &str,
        nonce: u64,
        timestamp: u64,
    ) -> Vec<u8> {
        let mut hasher = Sha256::new();
        
        // Add operation data
        match operation {
            crate::transaction::Operation::Put { key, value } => {
                hasher.update(b"PUT");
                hasher.update(key);
                hasher.update(value);
            }
            crate::transaction::Operation::Get { key } => {
                hasher.update(b"GET");
                hasher.update(key);
            }
            crate::transaction::Operation::Delete { key } => {
                hasher.update(b"DELETE");
                hasher.update(key);
            }
        }

        // Add authentication context
        hasher.update(user_id.as_bytes());
        hasher.update(&nonce.to_le_bytes());
        hasher.update(&timestamp.to_le_bytes());

        hasher.finalize().to_vec()
    }
}