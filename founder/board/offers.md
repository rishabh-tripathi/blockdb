# Offers lens
Score: 3 | Vote: PASS

There is no offer yet. There is a database. No price, no named buyer, no channel, no proof, no guarantee. The Offers lens cannot fund an offer that has not been made, and the one implied here fails on likelihood.

## What is strong
- **The market passes on two of four tests.** Compliance teams under GDPR-style regulation have budget, and the pain is real when a regulator asks them to prove consent. Whether that pain is massive day to day, and whether the market is growing, is unverified.
- **The dream outcome is nameable:** "prove any consent decision, on demand, with evidence that cannot have been altered." That is a better pitch than "a blockchain database".
- **A real consent API already exists** (grant, withdraw, status, history, proof hash per event). Time-to-first-result could be short for a buyer's engineer.
- **Low cost to deliver the core** once hosted: it is software, not service hours.

## What would kill it (the top 3, most dangerous first)
1. **Perceived likelihood is near zero.** The buyer is a risk-averse compliance team being asked to put legal evidence in a v0.1.0 store with no customers, no security audit, no SOC 2 or ISO 27001, no independent verification, on a hosted service that does not exist. And the chain is single-operator: the vendor holding the data is the one vouching it was not altered. This buyer purchases proof, and there is none.
2. **The product contradicts the outcome it sells.** An append-only store with no deletes has no stated answer to erasure requests. To a privacy team that reads as a new compliance problem, not a solved one. Until answered, the offer is negative value to its own target buyer.
3. **Effort and sacrifice are high, and the stack is empty.** The buyer must integrate an HTTP API, migrate consent records, and pass vendor security review, alone. Nothing surrounds the database: no consent-capture front end, no regulator-ready export, no migration help, no guarantee. Without those it is a commodity database and will be compared on price, which is undecided.

## The questions I would ask the founder
- Who, by job title and company type, has lost money or sleep over failing to prove consent? Have you spoken to five of them? (/founder-consumer)
- What do they use today, what does it cost, and what do they complain about? (/founder-competitors)
- How does erasure work on an append-only chain?
- Why would a buyer believe your tamper evidence when you run the chain?
- What can you guarantee, and can you afford to honour it? (/founder-cfo)
- How many days from sign-up to a first regulator-ready proof?
- Could you double whatever price you pick and still be the obvious choice? Today, no.

## If FUND IF: the conditions
Not applicable; the vote is PASS. It becomes a FUND IF when the founder returns with: a documented erasure answer; ten buyer conversations with at least three written commitments to a paid pilot at a stated price; some independent verification of the tamper-evidence claim; and a named stack around the database (migration, audit export, guarantee).
