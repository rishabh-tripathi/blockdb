# Formal specification (TLA+)

`Raft.tla` is an abstract TLA+ specification of the safety-critical core of
BlockDB's Raft consensus layer — leader election and log replication — with the
four safety invariants that underpin linearizability:

- **ElectionSafety** — at most one leader per term.
- **LogMatching** — logs that agree at an index agree on all earlier entries.
- **LeaderCompleteness** / **StateMachineSafety** — no two servers ever hold
  different committed entries at the same index.

It models the *protocol design*, not the Rust code line-by-line. It abstracts
timers, message loss/reordering, and concrete state-machine values.

## Checking it

Install TLC (requires Java). Get `tla2tools.jar` from
<https://github.com/tlaplus/tlaplus/releases>, then:

```bash
java -cp tla2tools.jar tlc2.TLC -config MC_Raft.cfg Raft.tla
```

`MC_Raft.cfg` checks a small, exhaustively-tractable instance (3 servers,
2 values, terms and log length bounded at 3). Increase the bounds for more
coverage at the cost of state-space size.

## CI

`.github/workflows/formal.yml` runs exactly this check on every push that touches
`formal/` (TLC is downloaded there — it could not be fetched in the authoring
sandbox, but runs in CI). So the safety invariants are verified continuously on
the finite instance in `MC_Raft.cfg`, not merely asserted.

## Scope and honesty

This is an abstract model of the *protocol*, model-checked on a small finite
instance. It is strong evidence for the design's safety but is **not** equivalent
to verifying the Rust implementation, nor to an adversarial **Jepsen** run
against a live cluster. Both the broader TLC instance and a Jepsen run (see
`../jepsen/`) remain recommended before BlockDB is adopted as a system of record;
see `../docs/ROADMAP.md`.
