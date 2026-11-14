# BlockDB Production-Readiness Roadmap

This is an honest account of what is **done** and what **remains** before BlockDB
should be adopted as an externally-sold system of record. It is deliberately
candid so adopters can make an informed decision.

## Done (implemented and tested)

- **Durability**: fsync'd Raft hard state + log (CRC-framed, torn-tail recovery);
  crash-safe WAL/SSTable/blockchain; restart recovery.
- **Consensus**: leader election, log replication, leader stickiness, dynamic
  membership (add/remove node), log compaction + `InstallSnapshot` catch-up,
  linearizable reads (ReadIndex).
- **Security**: no default credentials; Argon2id; persistent accounts; cluster
  secret (inter-node) + API token (client) auth; optional rustls TLS; hash-chained
  tamper-evident audit log.
- **Consent / GDPR**: replicated consent on the cluster; schema versioning;
  consent TTL; subject export; crypto-shredding erasure (single-node, tested).
- **Operability**: `/health` + `/metrics`; backup/restore with integrity manifest;
  admin CLI; on-disk format versioning + migration framework.
- **Verification**: fault-injection + partition/chaos tests; a TLA+ safety spec.
- Zero compiler warnings; full test suite green.

## Remaining before "sell it as the system of record"

### 1. Independent consensus verification — **highest priority**
The Raft implementation is hand-rolled. It passes in-repo fault and partition
tests and ships with a TLA+ safety spec, but it has **not** been run under an
external **Jepsen** suite, nor has the TLA+ spec been machine-checked with TLC in
CI. Before betting compliance data on it:
- Run Jepsen with nemeses (partitions, clock skew, pauses) asserting
  linearizability over the KV register.
- Model-check `formal/Raft.tla` with TLC in CI.
- Alternatively, adopt a proven Raft library (e.g. `openraft`, `tikv/raft-rs`).

### 2. Cluster-wide crypto-shredding automation
Single-node crypto-shredding is complete. Cluster-wide erasure needs automatic
per-subject key distribution and a replicated erase that destroys the key on
every node's keyring, plus guaranteed removal of residual wrapped keys via
compaction. See `GDPR.md` for the interim deployment model.

### 3. Horizontal sharding
All data lives in one Raft group, so write throughput is bounded by one leader.
Multi-group sharding (hash/range partitioning with a routing layer and
cross-shard operations) is a significant design effort, not yet started.

### 4. Performance validation at scale
Single-node criterion benchmarks exist; there are no cluster throughput/latency
benchmarks or long-running soak tests, and no published SLOs. The inter-node
transport is JSON over HTTP (now pooled) — a binary framed transport would cut
overhead.

### 5. Operational hardening
- Mutual-TLS client certificates between nodes (currently shared-secret + TLS).
- Hot secret rotation (currently restart/rolling update).
- Rate limiting beyond the connection cap.
- Chunked `InstallSnapshot` for very large state (currently single-message).

### 6. Breadth
- Multi-key transactions on the replicated path (single-node 2PC scaffold only).
- Secondary indexes / richer queries (scaffolding exists, not production-ready).

## Suggested sequencing

1. Jepsen + TLC in CI (or swap to a proven Raft lib).
2. Cluster crypto-shredding automation.
3. Performance/soak benchmarks + binary transport.
4. Sharding design + implementation.
5. mTLS, secret rotation, rate limiting, chunked snapshots.
