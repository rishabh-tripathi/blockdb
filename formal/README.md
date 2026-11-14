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

## Status — important

This spec was authored alongside the implementation but **has not yet been
machine-checked with TLC in this environment** (the TLC download was blocked by
the sandbox). Treat it as a reviewed specification, not a completed proof, until
it is run under TLC in CI. Wiring `tlc2.TLC` into CI against this config — and an
independent Jepsen run against a live cluster — are the top items in
`../docs/ROADMAP.md` before BlockDB is adopted as a system of record.
