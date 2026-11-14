# BlockDB Deployment Guide

## Binaries

- `blockdb-server` — single-node server with the full REST API (KV, auth,
  consent, health, metrics).
- `blockdb-node` — one member of a Raft cluster (KV + consent, replicated).
- `blockdb-admin` — operator CLI (status, membership, backup/restore).
- `blockdb-cli` / `simple_cli` — local client utilities.

Build: `cargo build --release`.

## Single-node server

```bash
BLOCKDB_ADMIN_USER=admin \
BLOCKDB_ADMIN_PASSWORD='<strong-password>' \
blockdb-server --data-dir /var/lib/blockdb --host 0.0.0.0 --port 8080
```

If `BLOCKDB_ADMIN_USER`/`BLOCKDB_ADMIN_PASSWORD` are not set, the server creates
an `admin` with a **generated** strong password printed once in the logs —
capture it and rotate it. There is no default password.

## Multi-node cluster (3 or 5 nodes)

Run one `blockdb-node` per member. Use an **odd** cluster size so a majority is
well-defined (3 tolerates 1 failure, 5 tolerates 2).

```bash
blockdb-node \
  --node-id node0 --host 0.0.0.0 --port 8080 \
  --peers node0@10.0.0.1:8080,node1@10.0.0.2:8080,node2@10.0.0.3:8080 \
  --data-dir /var/lib/blockdb \
  --cluster-secret "$BLOCKDB_CLUSTER_SECRET" \
  --api-token "$BLOCKDB_API_TOKEN" \
  --tls-cert /etc/blockdb/tls/cert.pem \
  --tls-key  /etc/blockdb/tls/key.pem
```

Repeat on each host with its own `--node-id`/`--host`/`--port` (the `--peers`
list is the same everywhere and includes this node).

### Required security settings (production)

| Flag / env | Purpose |
|------------|---------|
| `--cluster-secret` / `BLOCKDB_CLUSTER_SECRET` | Shared secret authenticating inter-node Raft RPCs. Without it, anyone who can reach a node can forge consensus messages. **Set it.** |
| `--api-token` / `BLOCKDB_API_TOKEN` | Bearer token required on client `/kv` and `/consent` requests. |
| `--tls-cert` / `--tls-key` | Serve over TLS. The cluster secret and API token travel in request headers, so **enable TLS** (or terminate TLS at a trusted mesh/proxy) in any untrusted network. |

The node logs a loud warning for each of these left unset.

### Network

Each node listens on one port for both Raft RPC (`/raft/message`) and client
traffic. Open that port between all cluster members and from clients to the
cluster. `/status`, `/health`, `/metrics` are unauthenticated for probes.

## Endpoints

Client (require `Authorization: Bearer <api-token>` when set):

- `POST /kv` `{ "key": ..., "value": ... }` — write (forwarded to leader).
- `GET  /kv?key=K[&linearizable=true]` — read.
- `GET  /kv/scan?prefix=P[&limit=N]` — prefix scan.
- `POST /consent/grant|withdraw` `{ "subject_id", "purpose", "metadata"?, "expires_at_ms"? }`.
- `GET  /consent/status|history|list|export?subject=...[&purpose=...]`.

Admin (require `x-cluster-auth: <cluster-secret>`):

- `POST /cluster/add-node` `{ "node_id", "host", "port" }`.
- `POST /cluster/remove-node` `{ "node_id" }`.

Operational (open):

- `GET /status`, `GET /health`, `GET /metrics` (Prometheus).

## Environment variables

| Variable | Meaning |
|----------|---------|
| `BLOCKDB_NODE_ID`, `BLOCKDB_HOST`, `BLOCKDB_PORT`, `BLOCKDB_PEERS`, `BLOCKDB_DATA_DIR` | Node identity/config |
| `BLOCKDB_CLUSTER_SECRET`, `BLOCKDB_API_TOKEN` | Cluster/client auth |
| `BLOCKDB_TLS_CERT`, `BLOCKDB_TLS_KEY` | TLS |
| `BLOCKDB_ADMIN_USER`, `BLOCKDB_ADMIN_PASSWORD` | Single-node admin bootstrap |
| `BLOCKDB_MASTER_KEY` | Base64 32-byte master key for GDPR crypto-shredding (see `GDPR.md`) |
| `RUST_LOG` / `BLOCKDB_LOG_LEVEL` | Log verbosity |

## Data directory layout

```
<data-dir>/
  FORMAT              on-disk format version marker
  wal.log             write-ahead log
  *.sst               SSTables
  blockchain.dat      tamper-evident block chain
  raft/               Raft log, hard state, snapshot baseline
  auth/               persisted accounts + audit log
  audit/              cluster audit log
```

Back up the whole directory (see `OPERATIONS.md`).

## Scaling guidance

- Reads scale horizontally (add followers; use local reads).
- Writes are bounded by the single leader. For higher write throughput than one
  group sustains, you currently run multiple independent clusters and shard at
  the application layer. Built-in sharding is on the roadmap.
