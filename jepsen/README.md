# Jepsen test for BlockDB

An adversarial consistency/durability test that runs a real BlockDB cluster and
cuts the network with a partition nemesis while clients write and read.

## What it checks

BlockDB is append-only, so this models a **grow-only set**: each `:add` writes a
unique key (`jset:<n>`), and `:read` scans the prefix and collects the values.
The `set-full` checker asserts, across random network partitions:

- every **acknowledged** add is present in the final reads — no lost committed
  write;
- no element that was never added ever appears — no phantom/dirty read.

That exercises the durability and linearizability of the replicated store under
the failure modes (partitions, leader loss) that break naive consensus.

## Status

This is a **scaffold**. It is structurally complete Jepsen 0.3.x code but has not
been executed in the authoring environment, which has no multi-node SSH cluster.
Run it in a real Jepsen setup as below. Expect to iterate on the `db` component
for your deployment (how the binary is delivered to nodes).

## Requirements

- A Jepsen **control node** (Debian/Ubuntu) with Leiningen + JDK.
- `n` **DB nodes** (3 or 5), SSH-reachable from the control node, resolvable by
  the node names you pass with `--node`.
- A prebuilt `blockdb-node` binary present at `/opt/blockdb/blockdb-node` on each
  DB node (bake it into the image, or extend `db/setup!` to upload/build it).
  Build it with `cargo build --release` on a matching platform.

The test cluster runs **without** auth/TLS for simplicity. To test the secured
path, add `--cluster-secret`/`--api-token` in `db/setup!` and send the bearer
token from the client.

## Running

```bash
cd jepsen
lein run test \
  --nodes n1,n2,n3,n4,n5 \
  --username root \
  --ssh-private-key ~/.ssh/id_rsa \
  --time-limit 300 \
  --concurrency 50
```

Results (history, latency plots, checker verdict) land in `store/`. A green
`set-full` verdict across many partition cycles is the evidence that matters.

## Extending

- **Register/linearizability test**: since keys are write-once, a per-key
  linearizable-read test (write `k=v`, then linearizable `GET /kv?key=k`) checks
  read-your-writes and staleness under partitions.
- **Nemeses**: add clock skew (`nemesis/clock-scrambler`) and process
  pauses/kills for broader coverage.
- **Membership**: drive `/cluster/add-node` / `remove-node` as a nemesis to test
  configuration changes under load.
