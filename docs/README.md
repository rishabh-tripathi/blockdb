# BlockDB Documentation

BlockDB is a replicated, append-only (immutable), blockchain-verified key/value
store with a consent/GDPR layer, built on a Raft consensus core.

- **[DEPLOYMENT.md](DEPLOYMENT.md)** — binaries, single-node and cluster setup,
  env vars, endpoints, security flags.
- **[CONSISTENCY.md](CONSISTENCY.md)** — the consistency model: linearizable
  writes, read modes, durability, failure behavior, and what is *not* guaranteed.
- **[SECURITY.md](SECURITY.md)** — auth, TLS, inter-node secret, tamper-evident
  audit, secrets management, hardening checklist.
- **[GDPR.md](GDPR.md)** — consent model, crypto-shredding erasure, retention/TTL,
  export, and the cluster deployment model.
- **[OPERATIONS.md](OPERATIONS.md)** — runbook: monitoring, backup/restore,
  membership, compaction, disaster recovery, upgrades.
- **[ROADMAP.md](ROADMAP.md)** — an honest account of what is done and what
  remains before adoption as a system of record.
- **[../formal/](../formal/)** — TLA+ safety specification of the consensus core.
