use crate::auth::{AuthError, PrivateKey, PublicKey, Signature, UserId};
use ed25519_dalek::{Keypair, PublicKey as Ed25519PublicKey, SecretKey, Signature as Ed25519Signature, Signer, Verifier};
use rand::rngs::OsRng;
use rand::{CryptoRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyPair {
    pub public_key: PublicKey,
    pub private_key: PrivateKey,
}

impl KeyPair {
    pub fn generate() -> Result<Self, AuthError> {
        let mut csprng = OsRng {};
        let keypair = Keypair::generate(&mut csprng);
        
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

    pub fn hash_password(password: &str, salt: &[u8]) -> Vec<u8> {
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