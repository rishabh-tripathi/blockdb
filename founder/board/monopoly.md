# Monopoly lens
Score: 3 | Vote: PASS

## What is strong
- The target is narrow: consent proof for privacy teams is a small, nameable market, which is the right shape to dominate first. The consent API (grant, withdraw, status, history, a proof hash per event) is the only part of the product pointed at it.
- Real engineering exists: Raft replication, WAL with crash recovery, fault-injection tests. It is a working base, not a slide.
- Regulation gives a "why now" that does not depend on fashion.

## What would kill it (the top 3, most dangerous first)
1. **No secret and no 10x.** A hash-chained append-only log is a well-known technique. Managed ledger databases from large cloud providers, immutable-table features in mainstream databases, object storage with write-once locks, and consent-management platforms that already sit in the buyer's stack all reach the same buyer. The code is MIT-licensed and was written quickly with AI assistance, so a copycat in month six needs neither permission nor much time. Against these, the stated throughput is not an advantage.
2. **The proof is weak where it matters.** A private, single-operator chain proves the operator did not tamper only if you trust the operator. An auditor may not accept that, and without a security audit or SOC 2 / ISO 27001 a compliance team cannot buy a hosted store for regulated data at all. Separately, no deletes versus the right to erasure is unanswered; if it stays unanswered the product is disqualified in its own market.
3. **No distribution.** No customers, interviews, waitlist, channel or price. Compliance buyers purchase slowly, through procurement and security review, from vendors they trust. A bootstrapped small team selling infrastructure into that cycle has no stated way to reach the first 100 customers.

On the seven questions: engineering incremental, timing plausible, monopoly unproven, people unknown, distribution absent, durability none identified, secret none stated.

## The questions I would ask the founder
- What do you know about consent proof that the incumbents get wrong? Name the specific failed audit or fine it prevents.
- Who verifies the chain besides you? Would you anchor hashes with an independent third party?
- How is an erasure request honoured without breaking the chain?
- Which five companies have you asked, and what do they use today?
- Why would a buyer choose a version 0.1.0 database over a feature of the cloud they already pay for?
- What stops someone hosting your MIT code more cheaply?

## If FUND IF: the conditions
Not applicable; the vote is PASS. What would reopen it:
- A defensible wedge: one regulated niche where existing tools demonstrably fail, confirmed by /founder-competitors (who sells this within reach of the same buyer, and the gap they leave).
- Evidence of demand and willingness to pay from real compliance buyers; /founder-consumer for a first read, then real interviews.
- A designed answer to erasure and to independent verification.
- The cost of certification and of a long sales cycle against bootstrapped cash: /founder-cfo.
