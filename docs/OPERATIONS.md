# BlockDB Operations Runbook

## Monitoring

- `GET /health` — `200` healthy, `503` degraded. Healthy means the local store
  passes its integrity check **and** a leader is known. Use for readiness.
- `GET /metrics` — Prometheus. Key series:
  - `blockdb_cluster_is_leader` (1 on exactly one node — alert if 0 or >1 for
    long), `blockdb_cluster_has_leader`.
  - `blockdb_cluster_kv_writes_total`, `_kv_reads_total`, `_forwarded_writes_total`,
    `_write_errors_total`, `_auth_failures_total`, `_raft_messages_total`.
  - `blockdb_cluster_uptime_seconds`, `blockdb_cluster_size`.

Suggested alerts: no leader for > 30s; `auth_failures_total` spike; a node's
`is_leader` flapping; `write_errors_total` rising.

## Backup

Backups use `blockdb-admin` and produce a directory with a SHA-256 integrity
manifest.

```bash
# Back up a node's data directory.
blockdb-admin backup --data-dir /var/lib/blockdb --dest /backups/blockdb-$(date +%F)

# Verify a backup against its manifest.
blockdb-admin verify-backup --dir /backups/blockdb-2026-01-01
```

BlockDB's files are crash-safe (fsync'd WAL, atomic renames), so even a hot copy
restores to a consistent point, losing at most un-flushed recent writes (same
guarantee as a crash). For a guaranteed point-in-time backup, stop the node
first (cold backup). Encrypt and access-control the backup target.

## Restore / re-seed a node

```bash
# Restore into an empty data dir, then start the node normally; it rejoins and
# catches up via replication (InstallSnapshot if it is far behind).
blockdb-admin restore --from /backups/blockdb-2026-01-01 --data-dir /var/lib/blockdb
```

`restore` verifies the backup before trusting it and refuses to overwrite a
non-empty directory without `--force`.

Re-seeding a hopelessly-behind follower: restore it from a recent leader backup,
then start it — normal replication reconciles the tail.

## Membership changes

Add or remove a node at runtime (sent to any node; it reaches the leader). The
change is replicated through the Raft log and audit-logged.

```bash
blockdb-admin add-node --addr http://10.0.0.1:8080 \
  --cluster-secret "$BLOCKDB_CLUSTER_SECRET" \
  --id node3 --host 10.0.0.4 --port 8080

blockdb-admin remove-node --addr http://10.0.0.1:8080 \
  --cluster-secret "$BLOCKDB_CLUSTER_SECRET" --id node3
```

Rules:
- One membership change at a time (a second is rejected until the first commits).
- Keep the cluster at an **odd** size where possible.
- When **adding**: start the new node first (configured with the full peer list)
  so it is reachable, then `add-node`. It catches up from the log or a snapshot.
- When **removing**: `remove-node`, then decommission the node. Give the node a
  long election timeout before removal if it might otherwise disrupt elections.

## Log compaction / retention

Each node retains a tail of applied entries (`compaction_retention`, default
10 000) and compacts older ones behind a persisted snapshot baseline, bounding
disk/replay growth. A follower that falls further behind than the tail is caught
up automatically via `InstallSnapshot`. Tune retention higher if you routinely
have long-offline followers and want to avoid snapshot transfers.

## Audit log

The hash-chained audit log lives under `<data-dir>/auth/audit.log` (auth events)
and `<data-dir>/audit/audit.log` (membership). Ship it to a separate,
access-controlled store and periodically verify its integrity (the chain breaks
on any tampering).

## Disaster recovery

- **Lost a minority of nodes**: replace them (restore from backup or start fresh
  with the same peer list) — no data loss; they re-sync.
- **Lost a majority**: committed data survives on any one surviving node's
  storage. Restore survivors, and if quorum cannot be formed, rebuild the cluster
  from the most up-to-date node's backup. (This is a manual, last-resort
  procedure; practice it.)
- **Format version mismatch**: a node refuses to open a data directory written by
  a newer BlockDB version (see `FORMAT` marker). Upgrade the binary.

## Upgrades

- Roll one node at a time; the cluster stays available with a majority.
- The on-disk `FORMAT` marker gates format changes and runs forward migrations;
  never downgrade a binary below the data's format version.
