# Issue #75: the paper

This folder holds the academic, formal part of the council's answer to issue #75. The review
package in the folder above has eight documents that each cover one subject. Here the same work
is one paper, with every theorem stated and proved in full. The paper also adds one new result,
Lemma 5 on crossing heals.

| File | What it is |
|---|---|
| [whitepaper.md](whitepaper.md) | *Transport Indistinguishability, Link Re-keying and Pipe Healing in Montana*: model, the [I-16] game, Theorem 1 (KIND), Propositions 1-2, Claim C3′, Theorems 2-4, Lemma 5, its machine check, refusals, implementation status, limitations, references |
| [check_lemma5.py](check_lemma5.py) | the exhaustive check of Lemma 5 (c) over every reachable state of two correspondents, with two mutants that show it finds faults (whitepaper, Section 5.5); Python standard library only |

## Read this first

- **The proofs are the council's own.** No one outside the council has reviewed them, and no
  independent party has audited them. No machine has checked them, except Lemma 5 (c), which
  is checked exhaustively within stated bounds (whitepaper, Section 5.5). That check found a
  latent fault in the iOS client's heal rule; by decision 439 = (b) on record 438 it stays a
  named limit, and its correction is a condition of turning sending a heal on in iOS.
- **Montana does not claim resistance to a global passive adversary.** See Proposition 1.
- **The routing layer that the anonymity claim is about is not running yet** (stage 18). The
  link renewal wire code is merged (PR #125). The iOS client, build 2324, receives and opens a
  pipe heal but does not send one (decision 416 = (a)).
- **The specification is normative and this paper is not.** If they disagree, the specification
  is right and the paper has a defect.

## Results at a glance

| Result | Statement | Bound or standing |
|---|---|---|
| Theorem 1 | For any set of observation points, an observer cannot tell a payment from a letter | `2q·Adv^{ind$} + q'·Adv^{int}` |
| Proposition 1 | An observer at both ends of a delivery tells destinations apart | advantage 1 |
| Proposition 2 | With an adversary-chosen workload, the original [I-16] wording fails | a finding; it led to C3′ |
| Claim C3′ | The current text of [I-16]: advantage bounded by honest volume, delay profile and path length | adopted; `ε` not yet measured |
| Theorem 2 | Forward secrecy across link renewals | `(n+1)·q_H/2^256` |
| Theorem 3 | A link heals after a passive renewal | `Adv^{cpa} + (e+1)·q_H/2^256` |
| Theorem 4 | A pipe heals after a passive heal | `Adv^{cpa} + (k+1)·q_H/2^256` |
| Lemma 5 | Crossing heals: one healed walk, agreement, no divergence under loss | `2·Adv^{cpa}/(1−γ) + (k+1)·q_H/2^256` |
| Section 5.5 | Lemma 5 (c) over every reachable state; the iOS let-go rule fails on a late offer | exhaustive within bounds |

## What we ask of a reviewer

Check the proofs. Two points need the most scrutiny: the symmetry argument in Lemma 5 (d), and
the scope condition of Theorem 1. The general questions are in
[../08-open-questions.md](../08-open-questions.md). Answers can go on issue #75.
