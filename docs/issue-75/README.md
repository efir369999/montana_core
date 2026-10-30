# Montana, issue #75: transport metadata, link re-keying and pipe healing

**A review package for cryptographers.**

Prepared by the Montana council on 2026-10-30 (UTC) in answer to issue #75 of
`github.com/efir369999/montana_messenger`, an RFC titled *"Formalizing GPA-Resistance: Provable
Security Models, Traffic Padding Determinism, and Post-Quantum Forward Secrecy"*.

## Status, stated first

- **The re-keying text is adopted. Its code is not running.** On 2026-10-29 the Author of
  Montana decided to adopt the re-keying section and its Canon form (decision 333 = (b), record
  338). On 2026-10-30 the adoption was written into the specification: on `main` at commit
  `605c843` (PR #121) the section is marked *adopted*, and the decision is carried into "In
  scope", the ceiling, the rule of record 356, the ledger rows, conformance item 23, Registry
  NET-25 and roadmap stage 18. The code that runs a renewal and a heal follows only after the
  full network run of issue #24, so the wire does not change under tests that are running.
  Every `File:line` reference below stays pinned to commit `9f72fc0`, where the same section
  still reads *proposed*. The change between the two is `git diff 9f72fc0 605c843`.
- **The proofs here are the council's own.** They have not been reviewed or machine-checked, and
  no independent party has audited them. We are sending them out so that someone outside the
  council can check them.
- **Montana does not claim resistance to a global passive adversary.** Document 01 says exactly
  what is claimed, and document 03 shows why the stronger claim cannot be made by any design like
  this one.
- **The transport the anonymity claim is about is not running.** The current node puts every cell
  on every link it holds. Routing by tags is scheduled for stage 18 of the roadmap. See document 07.

## What we ask of a reviewer

1. Check the proofs of Theorems 1-3 (documents 04 and 05). For each theorem, does it follow from
   the stated assumptions, and are the assumptions the right ones?
2. Check the game of document 03. Is the admissibility condition ("does not hold both ends of one
   delivery") stated tightly enough to mean something? Is the leakage function honest, meaning
   does it list everything the design leaks to an admissible observer?
3. Tell us where the claims of document 01 say more than the construction gives.

Answers can go on issue #75 or to the repository's maintainers. We would rather learn now that a
claim is wrong than defend it later.

## Contents

| File | What it holds |
|---|---|
| [01-claims.md](01-claims.md) | What Montana claims and does not claim, including a correction of an earlier public proposal (TokTok/c-toxcore #3112) and the recognisability of Montana traffic |
| [02-model.md](02-model.md) | System model, notation and primitives |
| [03-transport-game.md](03-transport-game.md) | The transport-indistinguishability game of invariant [I-16], its leakage function, an impossibility result for both-ends observers, and its status (KIND **proved**; C3′ adopted as [I-16], its bound `ε` not yet measured) |
| [04-link-renewal.md](04-link-renewal.md) | Re-keying a link by ML-KEM-768: the protocol, and Theorems 1 and 2 (forward secrecy across renewals; healing after a passive renewal) with proofs |
| [05-pipe-heal.md](05-pipe-heal.md) | Healing an end-to-end pipe in band: the protocol, and Theorem 3 with its proof |
| [06-rfc-response.md](06-rfc-response.md) | The four requirements of the RFC answered one by one: taken, taken in a changed form, or refused, with the norm each refusal rests on |
| [07-implementation-status.md](07-implementation-status.md) | What the code does today and what it does not, with file and line references |
| [08-open-questions.md](08-open-questions.md) | Decisions taken, decisions still open, and questions for reviewers |
| [paper/](paper/README.md) | The same work as one paper for cryptographers: every theorem with its full proof, and Lemma 5 on crossing heals (new) |

Suggested order: 01, 02, 03, then 04-05 for the proofs, and 07 before believing anything about
the code.

## Sources and how to check them

Every reference of the form `File:line` points into the repository at commit
**`9f72fc07b90c25095c4dbd2f4bea621bdead0cef`** (`main`, 2026-10-30 00:02 UTC). The specification
files are:

| Short name | Path |
|---|---|
| Constitution | `core/docs/Montana Constitution.md` |
| Network | `core/docs/Montana Network.md` |
| Canon | `core/docs/Montana Canon.md` (integer forms, layouts, frozen vectors) |
| Registry | `core/docs/Montana Registry.md` |
| RFC answer | `core/docs/protocol-13/75-RFC-GPA.md` (the council's working answer, in more detail) |

To recompute the frozen vectors these documents cite:

```
git clone https://github.com/efir369999/montana_messenger
git -C montana_messenger checkout 9f72fc07b90c25095c4dbd2f4bea621bdead0cef
cargo test --manifest-path montana_messenger/core/Montana-Core/Cargo.toml \
    -p mt-wire -p mt-derive -p mt-codec -p mt-conformance
```

`mt-conformance` reads Canon and compares its frozen vectors with what the code computes (see
document 07). The council ran these tests for slices 1 and 2 of this work, and they passed (exit
0; roads `a683-i75-renew3` and `a683-i75-vec`, 2026-10-29 12:3x UTC). A reviewer should run them
again rather than rely on our run.

## Scope

**In scope:** link re-keying, pipe healing, the [I-16] game, and the position on padding and
cover traffic. **Out of scope:** consensus, coins, and the handshake itself (ML-KEM-768 both
ways, ML-DSA-65 over the transcript, Canon:3076-3095). The handshake is unchanged by anything
here.
