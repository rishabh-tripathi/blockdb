# Board brief: BlockDB

## The idea (from the founder)

- What it is: a distributed, append-only database with blockchain-style (SHA-256 hash chain) verification, offered as a tamper-evident consent and audit store.
- Who it is for: privacy/compliance teams at companies that must prove consent and keep audit trails under GDPR-style regulation.
- What it sells, at what price: a managed cloud service (hosted BlockDB). Price NOT decided yet.
- Where and how: online; delivered as a hosted service with an HTTP API. How customers find it is NOT decided yet.
- Budget and constraints: small team, bootstrapped, no outside funding. No budget figure given.

## What exists today (facts from the code repository)

- An open-source (MIT) Rust codebase, version 0.1.0, about 12,400 lines of source. Not a hosted service yet: no cloud offering, billing, dashboard, sign-up or website exists.
- Storage engine: LSM-tree (memtable + SSTables), write-ahead log with fsync and CRC-checked crash recovery, compaction. Data is append-only: no updates or deletes.
- Every write is anchored in an internal SHA-256 hash chain ("blockchain") so tampering is detectable. It is a private, single-operator chain, not a public or multi-party blockchain.
- Multi-node Raft consensus over HTTP: leader election, log replication, linearizable reads, dynamic membership, log compaction, optional TLS and authentication between nodes.
- Collections with optional schema validation and indexes; authentication/authorization; a transaction manager.
- A consent API: POST /consent/grant, POST /consent/withdraw, GET /consent/status, /consent/history, /consent/subject. Consent is an immutable event log per (subject, purpose) with a proof hash per event.
- Interfaces: HTTP API and a CLI. Docker and Kubernetes manifests, Prometheus metrics, health endpoints.
- The project's own stated benchmarks: about 190+ writes/second single node; reads under 1 ms cached, under 5 ms from disk; cluster replication about 85% of single-node throughput.
- Test suite covering storage, consensus, fault injection, auth and TLS.
- Much of the recent code was written with AI coding assistance in a short span of commits.

## What is not known (do not invent these)

- No customers, users, revenue, waitlist or customer interviews have been reported.
- No price, no market-size figure, no customer-acquisition cost, no named competitors from the founder.
- Team size, background and sales experience beyond "small team, bootstrapped" are not given.
- No security audit, compliance certification (e.g. SOC 2, ISO 27001) or independent verification of the consistency claims has been reported.
- How an append-only store that forbids deletes handles data-erasure requests (e.g. GDPR right to erasure) is not addressed in the materials.
