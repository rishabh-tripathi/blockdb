//! Consent management on top of the append-only, blockchain-verified store.
//!
//! Consent is inherently mutable state (a subject grants, then later withdraws),
//! but BlockDB is append-only and forbids key updates. We therefore model consent
//! as an **event log**: every grant or withdrawal is written under a unique,
//! time-ordered key. The current state for a (subject, purpose) pair is derived by
//! reading the latest event, and the full immutable history is available for audit —
//! which is exactly the property a consent/GDPR record needs.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::BlockDBError;
use crate::BlockDBHandle;

const CONSENT_PREFIX: &str = "consent/";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConsentStatus {
    Granted,
    Withdrawn,
}

impl std::fmt::Display for ConsentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConsentStatus::Granted => write!(f, "granted"),
            ConsentStatus::Withdrawn => write!(f, "withdrawn"),
        }
    }
}

/// Current schema version for a consent event, so the record format can evolve.
pub const CONSENT_SCHEMA_VERSION: u32 = 1;

fn default_schema_version() -> u32 {
    CONSENT_SCHEMA_VERSION
}

/// A single immutable consent event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsentEvent {
    /// Record schema version (for forward-compatible evolution of this struct).
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    /// Unique id for this event.
    pub event_id: String,
    /// The data subject (e.g. the user/person the consent is about).
    pub subject_id: String,
    /// The processing purpose this consent applies to.
    pub purpose: String,
    /// Whether this event grants or withdraws consent.
    pub status: ConsentStatus,
    /// Milliseconds since the Unix epoch when the event was recorded.
    pub timestamp_ms: u64,
    /// Optional expiry: a grant stops being effective after this time (consent TTL /
    /// retention of consent validity). `None` means it does not expire on its own.
    #[serde(default)]
    pub expires_at_ms: Option<u64>,
    /// Free-form context: policy version, source system, IP, legal basis, etc.
    pub metadata: BTreeMap<String, String>,
    /// SHA-256 over the canonical event content, giving each event a tamper-evident id
    /// independent of (and cross-checkable against) the blockchain record hash.
    pub proof_hash: String,
}

impl ConsentEvent {
    pub fn new(
        subject_id: String,
        purpose: String,
        status: ConsentStatus,
        metadata: BTreeMap<String, String>,
    ) -> Self {
        Self::with_expiry(subject_id, purpose, status, metadata, None)
    }

    pub fn with_expiry(
        subject_id: String,
        purpose: String,
        status: ConsentStatus,
        metadata: BTreeMap<String, String>,
        expires_at_ms: Option<u64>,
    ) -> Self {
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let event_id = uuid::Uuid::new_v4().to_string();

        let mut event = ConsentEvent {
            schema_version: CONSENT_SCHEMA_VERSION,
            event_id,
            subject_id,
            purpose,
            status,
            timestamp_ms,
            expires_at_ms,
            metadata,
            proof_hash: String::new(),
        };
        event.proof_hash = event.compute_proof_hash();
        event
    }

    /// Whether this event, if it is the latest, currently represents effective consent
    /// — i.e. it is a grant that has not expired as of `now_ms`.
    pub fn is_effective_grant(&self, now_ms: u64) -> bool {
        self.status == ConsentStatus::Granted && self.expires_at_ms.map_or(true, |e| now_ms < e)
    }

    /// Canonical hash over the semantically meaningful fields (excluding the hash itself).
    fn compute_proof_hash(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.schema_version.to_be_bytes());
        hasher.update(self.event_id.as_bytes());
        hasher.update(self.subject_id.as_bytes());
        hasher.update(self.purpose.as_bytes());
        hasher.update(self.status.to_string().as_bytes());
        hasher.update(self.timestamp_ms.to_be_bytes());
        if let Some(exp) = self.expires_at_ms {
            hasher.update(b"exp=");
            hasher.update(exp.to_be_bytes());
        }
        for (k, v) in &self.metadata {
            hasher.update(k.as_bytes());
            hasher.update(b"=");
            hasher.update(v.as_bytes());
            hasher.update(b";");
        }
        hex(&hasher.finalize())
    }

    /// Recompute the hash and confirm it matches the stored proof — detects tampering
    /// of any field even before consulting the blockchain layer.
    pub fn verify(&self) -> bool {
        self.proof_hash == self.compute_proof_hash()
    }
}

/// A subject's full exported consent record (for GDPR subject-access / portability).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubjectExport {
    pub subject_id: String,
    pub exported_at_ms: u64,
    pub events_by_purpose: BTreeMap<String, Vec<ConsentEvent>>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Current consent state for a (subject, purpose) pair, derived from the latest event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsentState {
    pub subject_id: String,
    pub purpose: String,
    pub status: ConsentStatus,
    pub last_updated_ms: u64,
    pub latest_event: ConsentEvent,
}

/// High-level consent API backed by a `BlockDBHandle`.
///
/// When a `keyring` is configured, each event's value is stored encrypted under the
/// subject's data key (crypto-shredding): erasing the subject destroys the key and
/// renders every one of their events permanently unreadable, satisfying GDPR
/// right-to-erasure on the append-only store. Without a keyring, events are stored in
/// plaintext (the historical behavior).
#[derive(Clone)]
pub struct ConsentStore {
    db: BlockDBHandle,
    keyring: Option<std::sync::Arc<crate::gdpr::SubjectKeyring>>,
}

impl ConsentStore {
    pub fn new(db: BlockDBHandle) -> Self {
        ConsentStore { db, keyring: None }
    }

    /// Consent store with crypto-shredding: event values are encrypted per subject and
    /// `erase_subject` can permanently destroy a subject's data.
    pub fn with_keyring(
        db: BlockDBHandle,
        keyring: std::sync::Arc<crate::gdpr::SubjectKeyring>,
    ) -> Self {
        ConsentStore { db, keyring: Some(keyring) }
    }

    /// Build the stored value for an event: plaintext JSON, or ciphertext when a
    /// keyring is configured (encrypted under the subject's data key).
    pub fn encode_value(&self, subject_id: &str, event: &ConsentEvent) -> Result<Vec<u8>, BlockDBError> {
        let plain = serde_json::to_vec(event)
            .map_err(|e| BlockDBError::InvalidData(format!("serialize consent event: {}", e)))?;
        match &self.keyring {
            Some(kr) => kr.encrypt(subject_id, &plain),
            None => Ok(plain),
        }
    }

    /// Decode a stored value back into an event. Returns `Ok(None)` when the subject's
    /// key has been erased (the event is permanently unrecoverable).
    fn decode_value(&self, subject_id: &str, value: &[u8]) -> Result<Option<ConsentEvent>, BlockDBError> {
        let plain = match &self.keyring {
            Some(kr) => match kr.decrypt(subject_id, value)? {
                Some(p) => p,
                None => return Ok(None), // erased
            },
            None => value.to_vec(),
        };
        let event = serde_json::from_slice(&plain)
            .map_err(|e| BlockDBError::InvalidData(format!("corrupt consent event: {}", e)))?;
        Ok(Some(event))
    }

    /// Crypto-shred a subject: destroy their data key so every encrypted consent event
    /// for them becomes permanently unreadable. Returns whether a key was present.
    /// Requires a keyring; on a plaintext store this is a no-op returning `false`.
    pub fn erase_subject(&self, subject_id: &str) -> Result<bool, BlockDBError> {
        match &self.keyring {
            Some(kr) => kr.erase(subject_id),
            None => Ok(false),
        }
    }

    /// Whether this store has crypto-shredding enabled.
    pub fn erasure_enabled(&self) -> bool {
        self.keyring.is_some()
    }

    /// Record a consent grant. Returns the stored event (including its proof hash).
    pub async fn grant(
        &self,
        subject_id: &str,
        purpose: &str,
        metadata: BTreeMap<String, String>,
    ) -> Result<ConsentEvent, BlockDBError> {
        self.append_event(subject_id, purpose, ConsentStatus::Granted, metadata, None)
            .await
    }

    /// Record a consent grant that expires at `expires_at_ms` (consent TTL).
    pub async fn grant_with_expiry(
        &self,
        subject_id: &str,
        purpose: &str,
        metadata: BTreeMap<String, String>,
        expires_at_ms: Option<u64>,
    ) -> Result<ConsentEvent, BlockDBError> {
        self.append_event(subject_id, purpose, ConsentStatus::Granted, metadata, expires_at_ms)
            .await
    }

    /// Record a consent withdrawal. Returns the stored event.
    pub async fn withdraw(
        &self,
        subject_id: &str,
        purpose: &str,
        metadata: BTreeMap<String, String>,
    ) -> Result<ConsentEvent, BlockDBError> {
        self.append_event(subject_id, purpose, ConsentStatus::Withdrawn, metadata, None)
            .await
    }

    /// Validate inputs and build the immutable `(key, value, event)` for a consent
    /// event WITHOUT writing it. The append-only key is unique and time-ordered. This
    /// is the shared core used both by the local writer and by the cluster, which must
    /// construct the event once and then replicate the resulting key/value through Raft
    /// (so every node applies identical bytes).
    pub fn prepare_event(
        subject_id: &str,
        purpose: &str,
        status: ConsentStatus,
        metadata: BTreeMap<String, String>,
        expires_at_ms: Option<u64>,
    ) -> Result<(String, Vec<u8>, ConsentEvent), BlockDBError> {
        if subject_id.is_empty() || purpose.is_empty() {
            return Err(BlockDBError::InvalidData(
                "subject_id and purpose must not be empty".to_string(),
            ));
        }
        if subject_id.contains('/') || purpose.contains('/') {
            return Err(BlockDBError::InvalidData(
                "subject_id and purpose must not contain '/'".to_string(),
            ));
        }

        let event = ConsentEvent::with_expiry(
            subject_id.to_string(),
            purpose.to_string(),
            status,
            metadata,
            expires_at_ms,
        );
        let key = event_key(subject_id, purpose, event.timestamp_ms, &event.event_id);
        let value = serde_json::to_vec(&event).map_err(|e| {
            BlockDBError::InvalidData(format!("failed to serialize consent event: {}", e))
        })?;
        Ok((key, value, event))
    }

    async fn append_event(
        &self,
        subject_id: &str,
        purpose: &str,
        status: ConsentStatus,
        metadata: BTreeMap<String, String>,
        expires_at_ms: Option<u64>,
    ) -> Result<ConsentEvent, BlockDBError> {
        let (key, _plain, event) =
            Self::prepare_event(subject_id, purpose, status, metadata, expires_at_ms)?;
        // Encrypt the value per subject when crypto-shredding is enabled.
        let value = self.encode_value(subject_id, &event)?;
        // Each event has a unique, time-ordered key, so the append-only store accepts it.
        self.db.put(key.as_bytes(), &value).await?;
        Ok(event)
    }

    /// Export everything recorded for a subject across all purposes — the full
    /// immutable event history grouped by purpose, for a GDPR subject-access /
    /// data-portability request.
    pub async fn export_subject(&self, subject_id: &str) -> Result<SubjectExport, BlockDBError> {
        let prefix = format!("{}{}/", CONSENT_PREFIX, subject_id);
        let entries = self.db.scan_prefix(prefix.as_bytes()).await?;
        let mut by_purpose: BTreeMap<String, Vec<ConsentEvent>> = BTreeMap::new();
        for (_key, value) in entries {
            let Some(event) = self.decode_value(subject_id, &value)? else {
                continue; // erased
            };
            by_purpose.entry(event.purpose.clone()).or_default().push(event);
        }
        for events in by_purpose.values_mut() {
            events.sort_by_key(|e| e.timestamp_ms);
        }
        Ok(SubjectExport {
            subject_id: subject_id.to_string(),
            exported_at_ms: now_ms(),
            events_by_purpose: by_purpose,
        })
    }

    /// Full immutable history of events for a (subject, purpose), oldest first.
    pub async fn history(
        &self,
        subject_id: &str,
        purpose: &str,
    ) -> Result<Vec<ConsentEvent>, BlockDBError> {
        let prefix = format!("{}{}/{}/", CONSENT_PREFIX, subject_id, purpose);
        let entries = self.db.scan_prefix(prefix.as_bytes()).await?;
        let mut events = Vec::with_capacity(entries.len());
        for (_key, value) in entries {
            // Skip events whose subject key has been erased (unrecoverable).
            if let Some(event) = self.decode_value(subject_id, &value)? {
                events.push(event);
            }
        }
        // Keys embed a zero-padded timestamp, so scan order is already chronological;
        // sort defensively in case of equal timestamps.
        events.sort_by_key(|e| e.timestamp_ms);
        Ok(events)
    }

    /// Current consent state for a (subject, purpose), or `None` if never recorded.
    pub async fn status(
        &self,
        subject_id: &str,
        purpose: &str,
    ) -> Result<Option<ConsentState>, BlockDBError> {
        let history = self.history(subject_id, purpose).await?;
        Ok(history.into_iter().last().map(|latest| ConsentState {
            subject_id: subject_id.to_string(),
            purpose: purpose.to_string(),
            status: latest.status,
            last_updated_ms: latest.timestamp_ms,
            latest_event: latest,
        }))
    }

    /// Convenience predicate: is consent currently *effective* for this purpose — the
    /// latest event is a grant and it has not expired?
    pub async fn is_granted(&self, subject_id: &str, purpose: &str) -> Result<bool, BlockDBError> {
        let now = now_ms();
        Ok(self
            .status(subject_id, purpose)
            .await?
            .map(|s| s.latest_event.is_effective_grant(now))
            .unwrap_or(false))
    }

    /// Current consent state for every purpose recorded for a subject.
    pub async fn list_for_subject(
        &self,
        subject_id: &str,
    ) -> Result<Vec<ConsentState>, BlockDBError> {
        let prefix = format!("{}{}/", CONSENT_PREFIX, subject_id);
        let entries = self.db.scan_prefix(prefix.as_bytes()).await?;

        // Group latest event per purpose.
        let mut latest_by_purpose: BTreeMap<String, ConsentEvent> = BTreeMap::new();
        for (_key, value) in entries {
            let Some(event) = self.decode_value(subject_id, &value)? else {
                continue; // erased
            };
            latest_by_purpose
                .entry(event.purpose.clone())
                .and_modify(|existing| {
                    if event.timestamp_ms >= existing.timestamp_ms {
                        *existing = event.clone();
                    }
                })
                .or_insert(event);
        }

        Ok(latest_by_purpose
            .into_iter()
            .map(|(purpose, latest)| ConsentState {
                subject_id: subject_id.to_string(),
                purpose,
                status: latest.status,
                last_updated_ms: latest.timestamp_ms,
                latest_event: latest,
            })
            .collect())
    }

    /// Verify the blockchain integrity of the underlying store.
    pub async fn verify_integrity(&self) -> Result<bool, BlockDBError> {
        self.db.verify_integrity().await
    }
}

/// Build the append-only key for an event. The timestamp is zero-padded so that a
/// lexical prefix scan yields events in chronological order; the event id makes the
/// key unique even for events recorded in the same millisecond.
fn event_key(subject_id: &str, purpose: &str, timestamp_ms: u64, event_id: &str) -> String {
    format!(
        "{}{}/{}/{:020}/{}",
        CONSENT_PREFIX, subject_id, purpose, timestamp_ms, event_id
    )
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BlockDBConfig;
    use tempfile::TempDir;

    async fn store() -> (ConsentStore, TempDir) {
        let dir = TempDir::new().unwrap();
        let config = BlockDBConfig {
            data_dir: dir.path().to_string_lossy().to_string(),
            ..Default::default()
        };
        let db = BlockDBHandle::new(config).unwrap();
        (ConsentStore::new(db), dir)
    }

    #[tokio::test]
    async fn grant_then_status_is_granted() {
        let (store, _dir) = store().await;
        store.grant("alice", "marketing", BTreeMap::new()).await.unwrap();

        let state = store.status("alice", "marketing").await.unwrap().unwrap();
        assert_eq!(state.status, ConsentStatus::Granted);
        assert!(store.is_granted("alice", "marketing").await.unwrap());
        assert!(state.latest_event.verify());
    }

    #[tokio::test]
    async fn withdraw_supersedes_grant() {
        let (store, _dir) = store().await;
        store.grant("bob", "analytics", BTreeMap::new()).await.unwrap();
        // Ensure a distinct timestamp/key.
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        store.withdraw("bob", "analytics", BTreeMap::new()).await.unwrap();

        assert!(!store.is_granted("bob", "analytics").await.unwrap());
        let history = store.history("bob", "analytics").await.unwrap();
        assert_eq!(history.len(), 2, "both events retained for audit");
        assert_eq!(history[0].status, ConsentStatus::Granted);
        assert_eq!(history[1].status, ConsentStatus::Withdrawn);
    }

    #[tokio::test]
    async fn unknown_subject_has_no_state() {
        let (store, _dir) = store().await;
        assert!(store.status("nobody", "x").await.unwrap().is_none());
        assert!(!store.is_granted("nobody", "x").await.unwrap());
    }

    #[tokio::test]
    async fn list_for_subject_returns_latest_per_purpose() {
        let (store, _dir) = store().await;
        store.grant("carol", "email", BTreeMap::new()).await.unwrap();
        store.grant("carol", "sms", BTreeMap::new()).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        store.withdraw("carol", "sms", BTreeMap::new()).await.unwrap();

        let states = store.list_for_subject("carol").await.unwrap();
        assert_eq!(states.len(), 2);
        let sms = states.iter().find(|s| s.purpose == "sms").unwrap();
        assert_eq!(sms.status, ConsentStatus::Withdrawn);
        let email = states.iter().find(|s| s.purpose == "email").unwrap();
        assert_eq!(email.status, ConsentStatus::Granted);
    }

    #[tokio::test]
    async fn expired_grant_is_not_effective() {
        let (store, _dir) = store().await;
        // Grant that already expired a second ago.
        let past = now_ms() - 1000;
        store
            .grant_with_expiry("dave", "newsletter", BTreeMap::new(), Some(past))
            .await
            .unwrap();
        // Latest event is a grant, but it is expired -> not effectively granted.
        assert!(!store.is_granted("dave", "newsletter").await.unwrap());
        let state = store.status("dave", "newsletter").await.unwrap().unwrap();
        assert_eq!(state.status, ConsentStatus::Granted);
        assert!(!state.latest_event.is_effective_grant(now_ms()));

        // A non-expiring grant is effective.
        store.grant("dave", "email", BTreeMap::new()).await.unwrap();
        assert!(store.is_granted("dave", "email").await.unwrap());
    }

    #[tokio::test]
    async fn export_subject_returns_all_purposes() {
        let (store, _dir) = store().await;
        store.grant("erin", "email", BTreeMap::new()).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        store.withdraw("erin", "email", BTreeMap::new()).await.unwrap();
        store.grant("erin", "sms", BTreeMap::new()).await.unwrap();

        let export = store.export_subject("erin").await.unwrap();
        assert_eq!(export.subject_id, "erin");
        assert_eq!(export.events_by_purpose.len(), 2);
        assert_eq!(export.events_by_purpose["email"].len(), 2);
        assert_eq!(export.events_by_purpose["sms"].len(), 1);
        // Events are chronological.
        let email = &export.events_by_purpose["email"];
        assert!(email[0].timestamp_ms <= email[1].timestamp_ms);
    }

    #[tokio::test]
    async fn crypto_shredding_erases_subject() {
        use crate::gdpr::{MasterKey, SubjectKeyring};
        let dir = TempDir::new().unwrap();
        let config = BlockDBConfig {
            data_dir: dir.path().to_string_lossy().to_string(),
            ..Default::default()
        };
        let db = BlockDBHandle::new(config).unwrap();
        let keyring = std::sync::Arc::new(
            SubjectKeyring::open(&dir.path().join("keys"), MasterKey::generate()).unwrap(),
        );
        let store = ConsentStore::with_keyring(db, keyring);
        assert!(store.erasure_enabled());

        let mut md = BTreeMap::new();
        md.insert("email".to_string(), "frank@example.com".to_string());
        store.grant("frank", "marketing", md).await.unwrap();

        // Readable before erasure.
        assert!(store.is_granted("frank", "marketing").await.unwrap());
        let hist = store.history("frank", "marketing").await.unwrap();
        assert_eq!(hist.len(), 1);
        assert!(hist[0].verify());

        // Erase the subject: the key is destroyed.
        assert!(store.erase_subject("frank").unwrap());

        // All of the subject's data is now unrecoverable — reads return nothing.
        assert!(store.history("frank", "marketing").await.unwrap().is_empty());
        assert!(store.status("frank", "marketing").await.unwrap().is_none());
        assert!(!store.is_granted("frank", "marketing").await.unwrap());
        assert!(store.list_for_subject("frank").await.unwrap().is_empty());
        assert!(store.export_subject("frank").await.unwrap().events_by_purpose.is_empty());

        // Erasing again reports nothing to erase.
        assert!(!store.erase_subject("frank").unwrap());
    }

    #[tokio::test]
    async fn rejects_invalid_identifiers() {
        let (store, _dir) = store().await;
        assert!(store.grant("", "p", BTreeMap::new()).await.is_err());
        assert!(store.grant("s", "", BTreeMap::new()).await.is_err());
        assert!(store.grant("a/b", "p", BTreeMap::new()).await.is_err());
    }
}
