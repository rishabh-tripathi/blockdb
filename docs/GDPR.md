# BlockDB for Consent & GDPR

BlockDB models consent as an **immutable, tamper-evident event log**, which gives
you a defensible audit trail, and provides the features a consent/compliance
deployment needs on top of it.

## Consent model

Each grant or withdrawal is a unique, time-ordered `ConsentEvent` carrying:
`schema_version`, `subject_id`, `purpose`, `status`, `timestamp_ms`,
`expires_at_ms?`, free-form `metadata` (policy version, legal basis, source,
...), and a SHA-256 `proof_hash` over its content.

- **Current state** = the latest event for a `(subject, purpose)`.
- **History** = every event, retained forever (never overwritten) for audit.
- **Effective consent** (`is_granted`) = latest event is a grant **and** it has
  not passed `expires_at_ms`.

All consent writes are replicated through Raft (linearizable); reads are served
locally or, if needed, linearizably.

## Feature → GDPR mapping

| Need | BlockDB feature |
|------|-----------------|
| Record of consent & withdrawal (Art. 7) | Immutable event log; `grant` / `withdraw` |
| Demonstrate consent / audit trail (Art. 7(1)) | Full history + per-event proof hash + blockchain store |
| Consent expiry / re-consent | `expires_at_ms` (TTL); expired grants are not effective |
| Right of access / portability (Art. 15, 20) | `export_subject` / `GET /consent/export?subject=` — full record as JSON |
| Right to erasure (Art. 17) | **Crypto-shredding** (below) |
| Schema evolution | `schema_version` on every event |

## Right to erasure on an immutable store: crypto-shredding

An append-only store cannot delete records — which conflicts with the right to
erasure. BlockDB resolves this with **crypto-shredding**: personal data is stored
**encrypted** under a per-subject key; to erase, you destroy the key. The
ciphertext remains in the immutable chain (so integrity is preserved and the
chain still verifies), but it is permanently unrecoverable.

Mechanism (`gdpr::SubjectKeyring`, AES-256-GCM):

1. Each subject has a random 256-bit data key.
2. Data keys are stored **wrapped** under a master key (`BLOCKDB_MASTER_KEY`) in
   a mutable keyring **outside** the immutable store — so they can be deleted.
3. `ConsentStore::with_keyring` encrypts each event value under the subject's key.
4. `erase_subject(subject)` destroys the key; thereafter every one of that
   subject's events is undecryptable and reads return nothing for them.

This is implemented and tested for the co-located store. See
`gdpr::crypto_shred` and the `crypto_shredding_erases_subject` test.

### Deployment model for a cluster

For crypto-shredding across a multi-node cluster:

- Provision the **same `BLOCKDB_MASTER_KEY`** on every node (from a KMS/HSM).
- Per-subject keys must be available on each node to decrypt and deletable on
  each node to erase. Production erasure therefore destroys the subject key on
  **every** node's keyring.
- **Residual window**: a wrapped key referenced by an old, not-yet-compacted
  Raft log entry can linger until log compaction removes it; it is protected by
  the master key (held in KMS) in the meantime. Erasure is cryptographically
  complete once the key is gone from all live keyrings and the master key is
  protected.

> Status: single-node crypto-shredding is complete and tested. Automatic
> cluster-wide key distribution/erasure is a roadmap item (`ROADMAP.md`); until
> then, run crypto-shredding on a co-located store or follow the documented
> master-key + per-node erase procedure.

## Retention

Consent **validity** retention is expressed with `expires_at_ms`. **Storage**
retention of the immutable audit trail is intentionally unbounded (that is the
point of an audit log); use crypto-shredding to make specific subjects'
historical personal data unrecoverable when erasure is required.

## Operational notes

- Treat `subject_id` as a **pseudonymous** identifier; avoid putting direct
  identifiers (name, email) in the storage key. Put personal data in the
  encrypted event value/metadata so crypto-shredding covers it.
- Export a subject before erasing if you must fulfil an access request first.
