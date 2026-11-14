# Product lens
Score: 3 | Vote: FUND IF

## What is strong
- The consent API is the right shape: grant, withdraw, status, history, each event with a proof hash. That is a product a compliance person can picture using. Everything else is plumbing.
- Craft in the parts nobody sees: fsync'd WAL, CRC recovery, fault-injection tests, TLS between nodes. For a store whose whole promise is "you can trust this record", that is the correct place to spend effort.
- A one-sentence pitch is within reach: "Prove, to a regulator, exactly what each person consented to and when."

## What would kill it (the top 3, most dangerous first)
1. **The core promise contradicts the buyer's core obligation.** The store forbids deletes; the buyer must honour erasure requests. The brief has no answer. Until there is one, the first five minutes with a privacy team ends at the first question, and no polish elsewhere matters.
2. **It is a database, not a product, and the buyer is not a database buyer.** Compliance teams do not want Raft, LSM-trees, collections, indexes and a transaction manager. The experience was built from the engine outward, not from the audit moment backward. There is no sign-up, dashboard, export, or "hand this to the auditor" artefact, which is the actual product. No customer has been spoken to, so nobody knows what that moment needs.
3. **Trust is the product and none is evidenced.** A tamper-evident claim from a single-operator chain, on v0.1.0 code written quickly with AI assistance, with no audit or certification, asks the buyer to trust the same party that could rewrite the chain. Owning the whole experience here includes independent verifiability; today that seam is wide open.

## The questions I would ask the founder
- What is the one thing: a general-purpose database, or a consent ledger? Which do you cut?
- Describe the moment a regulator asks for proof. What does the customer click, and what do they hand over?
- How does an erasure request work without breaking the chain?
- How does a customer verify the chain without trusting you?
- Who inside the customer integrates the API, and how long before the first consent event is recorded?
- What does the buyer use today, and what is worse about it? (Open: /founder-competitors.)
- Will a compliance team pay for this as a standalone service, and what? (Open: /founder-consumer, then /founder-cfo for hosting cost against price.)

## If FUND IF: the conditions
1. A written, defensible erasure design (for example, personal data held outside the chain with only hashes anchored) reviewed by someone qualified in data-protection law, before any further build.
2. Cut the pitch to the consent ledger only. Generic collections, indexes and transactions leave the story; the one-sentence pitch stands alone.
3. Five conversations with real privacy/compliance leads, working back from their audit moment, producing a spec for the proof export a regulator would accept.
4. A customer-run verification path: the buyer can check the chain independently of the operator.
5. A hosted first-five-minutes: sign up, record a consent, withdraw it, export proof. No Kubernetes in sight.

If condition 1 cannot be met, this is a PASS.
