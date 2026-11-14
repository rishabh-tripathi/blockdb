# BlockDB Consistency Model

BlockDB is a replicated, append-only (immutable) key/value store with a consent
layer. This document states precisely what guarantees it provides, so you can
reason about correctness in your application.

## Replication and consensus

All writes go through **Raft** consensus:

1. A write is proposed to the leader, appended to its log, and replicated.
2. It is **committed** once a majority of nodes have durably persisted it.
3. Committed entries are applied to every node's state machine in the same order.

Writes sent to a follower are transparently forwarded to the leader.

### Durability

Before a node acts on them, the following are `fsync`'d to disk:

- Raft **hard state** (`currentTerm`, `votedFor`) — so a crash never causes a
  double-vote in a term.
- The Raft **log** — CRC-framed, with torn-tail recovery.
- The storage **WAL** — every acknowledged write survives a crash (when
  `wal_sync_interval_ms = 0`, the default for the cluster path).

A committed write survives the permanent loss of any minority of nodes.

## Write consistency: **linearizable**

Committed writes are linearizable. There is a single leader per term, and the
leader only commits entries from its current term (older entries commit
indirectly), so there is a single total order of writes consistent with real
time.

Append-only semantics: a key can be written **once**. A second write to the same
key is rejected cluster-wide with a `DuplicateKey` conflict, deterministically on
every node. This is the immutability guarantee — records cannot be altered.

## Read consistency: two modes

| Mode | API | Guarantee |
|------|-----|-----------|
| **Local read** (default) | `get`, `GET /kv?key=` | Eventually consistent. Served from the local node; may lag the latest committed write on a follower. |
| **Linearizable read** | `get_linearizable`, `GET /kv?key=&linearizable=true` | Reflects the most recently committed write as of the call. |

Linearizable reads use a **ReadIndex barrier**: the leader appends a no-op and
waits for it to commit (proving it is still the leader and that its applied state
is current), then reads locally. On a follower the read is forwarded to the
leader.

Choose local reads for throughput when staleness is acceptable (e.g. analytics);
choose linearizable reads when you must not miss a just-committed write (e.g.
checking current consent before processing).

## Consent-specific semantics

Consent is modeled as an **event log**: each grant/withdrawal is a unique,
immutable, time-ordered event. The current state is the latest event. Therefore:

- The full history is always available for audit (nothing is overwritten).
- `is_granted` is **effective** consent: the latest event is a grant **and** it
  has not passed its `expires_at_ms` (consent TTL).

## Failure behavior

- **Leader crash**: the surviving majority elects a new leader (typically within
  a couple of election timeouts) and continues. In-flight uncommitted writes may
  need to be retried by the client.
- **Minority partition / node loss**: the majority side continues to accept
  writes; the minority side cannot commit and serves only stale local reads.
- **Node restart**: recovers term/vote/log from disk and rejoins.
- **Far-behind follower**: caught up via `InstallSnapshot` when it has fallen
  past the log retention tail.

## What BlockDB does **not** guarantee

- **No multi-key transactions** across the cluster. Each write is an independent
  linearizable operation. (A single-node 2PC scaffold exists but is not part of
  the replicated path.)
- **No causal/session guarantees across local reads.** If you read locally from
  different nodes you may observe different suffixes of the log. Use linearizable
  reads if you need read-your-writes across nodes.
- **Single Raft group.** All data is in one group; write throughput is bounded by
  one leader. Horizontal sharding is a roadmap item (see `ROADMAP.md`).

## Verification

The consensus implementation is exercised by:

- Fault-injection tests (`tests/fault_test.rs`): crash recovery, leader
  failover, follower catch-up, membership changes, snapshot catch-up.
- A partition/chaos test (`tests/chaos_test.rs`): asserts state-machine safety
  (no two nodes apply different values at the same index) across leader
  isolation, minority stalls, and partition healing.
- A TLA+ safety specification (`formal/Raft.tla`).

These are strong but **not** a substitute for an independent external Jepsen
run, which is recommended before adopting BlockDB as a system of record. See
`ROADMAP.md`.
