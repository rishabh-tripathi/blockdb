# BlockDB Security Guide

## Threat model

BlockDB stores records that must be authentic, tamper-evident and, for consent,
subject to erasure. The main threats are: forged consensus messages,
unauthorized reads/writes, credential theft, and tampering with stored history.

## Authentication & authorization

- **Accounts**: password-based identities with **Argon2id** hashing, persisted
  across restarts. Permissions are role/permission sets checked per operation.
- **No default credentials.** Admins are bootstrapped from
  `BLOCKDB_ADMIN_USER`/`BLOCKDB_ADMIN_PASSWORD`, or a strong random password is
  generated and logged once. Rotate it immediately.
- **Client API**: `/kv` and `/consent/*` require `Authorization: Bearer <api-token>`
  when an API token is configured.
- **Inter-node**: `/raft/message` requires the shared `x-cluster-auth` secret, so
  a network attacker cannot inject votes/AppendEntries and subvert consensus.
  Membership administration (`/cluster/*`) is gated by the same secret.
- Secret comparisons are constant-time.

## Transport security (TLS)

Enable TLS (`--tls-cert`/`--tls-key`, rustls) on every node, or terminate TLS at
a trusted sidecar/mesh. **The cluster secret and API token are sent in request
headers**, so plaintext HTTP exposes them on the wire. `/status`, `/health`,
`/metrics` stay open for probes.

## Tamper-evidence

- **Blockchain store**: records are chained with hashes; `verify_integrity`
  detects modification of stored data.
- **Audit log** (`audit::AuditLog`): append-only, fsync'd, and **hash-chained** —
  each record links to its predecessor, so insertion/deletion/edit of any past
  record breaks the chain and is caught by `verify()`. Audited events: logins
  (success/failure), permission grant/revoke, cluster add/remove node.

## Secrets management

- Supply `BLOCKDB_CLUSTER_SECRET`, `BLOCKDB_API_TOKEN`, `BLOCKDB_MASTER_KEY` from
  a secrets manager / KMS, not from committed files.
- These are read from env/CLI and held in memory. **Rotation** currently requires
  a coordinated restart with the new value (and, for the cluster secret, a rolling
  update). Automated hot rotation is a roadmap item.
- `BLOCKDB_MASTER_KEY` (GDPR crypto-shredding) must be the **same on every node**
  and protected like any root key — its loss makes encrypted data unrecoverable;
  its leak weakens erasure guarantees.

## GDPR / data protection

See `GDPR.md`. Summary: personal data in consent events can be stored encrypted
per subject; erasure destroys the subject key (crypto-shredding), leaving the
immutable chain intact but the data unrecoverable.

## Hardening checklist

- [ ] TLS enabled (or TLS-terminating trusted mesh) on all nodes.
- [ ] `BLOCKDB_CLUSTER_SECRET` set (inter-node auth).
- [ ] `BLOCKDB_API_TOKEN` set (client auth).
- [ ] Admin bootstrapped from a secret manager; generated password rotated.
- [ ] `BLOCKDB_MASTER_KEY` provisioned from KMS if using crypto-shredding.
- [ ] Node port reachable only from cluster peers and authorized clients
      (network policy / security groups).
- [ ] Audit log shipped to a separate, access-controlled store and periodically
      `verify()`-checked.
- [ ] Backups encrypted at rest and access-controlled (see `OPERATIONS.md`).
- [ ] Monitoring on `/health` and `/metrics`.

## Known limitations (see `ROADMAP.md`)

- No mutual-TLS client-certificate auth between nodes (the shared secret is the
  inter-node authenticator; pair it with TLS).
- No built-in rate limiting beyond the connection cap (`max_connections`).
- Secret rotation requires a restart/rolling update.
