# Board summary: BlockDB

## 1. The vote

**2 pass, 1 fund if. Average score 3.0 / 10.**

| Lens | Score | Vote |
|---|---|---|
| Offers | 3 | PASS |
| Monopoly | 3 | PASS |
| Product | 3 | FUND IF |

As pitched (a managed cloud database for privacy/compliance teams), the board does not fund this. The scores are for the business, not the engineering: all three members called the engineering real.

## 2. Risks more than one member raised

1. **Append-only versus the right to erasure (all three).** The store forbids deletes; the buyer must honour erasure requests. There is no stated answer. Offers calls it negative value to the target buyer, Monopoly calls it disqualifying, and Product makes it the one condition that turns its vote into a PASS if unmet.
2. **The tamper evidence depends on trusting the operator (all three).** A private, single-operator hash chain proves nothing to an auditor who does not already trust the party running it. No security audit, SOC 2 / ISO 27001, or independent verification exists, and the code is v0.1.0.
3. **No evidence of demand, and no way to reach buyers (all three).** No customers, interviews, waitlist, price or channel. Compliance buyers purchase slowly, through procurement and security review, which is hard on a bootstrapped small team.
4. **It is a database, and the buyer does not buy databases (Offers, Product).** Nothing surrounds the engine: no sign-up, dashboard, migration help, or regulator-ready export. Without those it is compared on price.

Raised by one member only:

- **No secret and no 10x (Monopoly).** Hash-chained logs are a known technique; cloud ledger databases, immutable tables, write-once object storage and existing consent platforms reach the same buyer. The code is MIT, so anyone can host it.

## 3. Where the board disagreed

- **Is it fundable at all?** Offers and Monopoly say no until evidence exists. Product says yes, conditionally, because the consent API is the right shape and a one-sentence pitch is within reach.
- **What is the fix?** Product thinks the problem is focus: cut to the consent ledger and build the audit moment. Monopoly thinks focus does not help, because a focused consent ledger still has no moat against incumbents and a copycat. Offers sits between: the outcome is sellable, but only with proof and a stack around it.
- **Is the engineering an asset?** Product sees care in the right place (durability, fault tests). Monopoly sees it as incremental and easy to copy.

## 4. Conditions (merged checklist)

- [ ] A written erasure design (for example, personal data kept outside the chain, only hashes anchored), reviewed by someone qualified in data-protection law. **Do this before any further build.**
- [ ] A way for the customer to verify the chain without trusting the operator (customer-run verification or hashes anchored with an independent third party).
- [ ] Ten conversations with real privacy/compliance leads, working back from the moment a regulator asks for proof. First read: `/founder-consumer`.
- [ ] At least three written commitments to a paid pilot at a stated price. Price: `/founder-pricing`.
- [ ] A named gap: one regulated niche where existing tools demonstrably fail, and who sells to that buyer today. `/founder-competitors`.
- [ ] Cut the pitch to the consent ledger. Generic collections, indexes and transactions leave the story.
- [ ] A hosted first five minutes: sign up, record a consent, withdraw it, export proof.
- [ ] A spec for the proof export a regulator or auditor would accept, plus migration help and a guarantee the business can afford.
- [ ] The cost of certification, hosting and a long sales cycle against bootstrapped cash. `/founder-cfo`.

## 5. The strongest version the board can see

This differs from what was pitched. The strongest version is not "a managed blockchain database"; it is a **consent-proof service**: "prove to a regulator exactly what each person consented to and when." The database becomes an invisible implementation detail. Personal data stays out of the chain so erasure works, the customer can verify the record independently of the vendor, and the thing actually sold is the auditor-ready export plus the hosted API, in one regulated niche where current tools fall short. Even in that form, Monopoly's objection stands: nothing yet stops an incumbent or a copycat, so the defensibility would have to come from the niche, the trust built there and the proof format, not from the engine. None of this is worth building further until the erasure answer and the first buyer conversations exist.
