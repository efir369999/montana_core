# 05. Healing a pipe: protocol and proof

**Status: adopted, not implemented (Network:280-291 at `9f72fc0`; marked adopted on `main` at
`605c843`, PR #121, decision 333 = (b)).** The fold and its vectors are on `main`
(Canon:3375-3389; `mt-derive/src/tag.rs:114-129`). The Author has decided when a heal is offered
(Q4 (a)(i), below). The iOS client (Swift) carries the fold, the heal by the day, the keys of a
heal, its letters and its wiring into the pipe, not yet the sending of a heal (document 07, slices C1-C2b-2;
decision 416); the
Android client (Kotlin) is outside this council's work (decision 384).

## 1. The problem

The secret of a pipe walks one way, one step per window (Canon:1354-1371), which gives forward
secrecy. It does not heal. Whoever takes a device, or a copy of its storage, reads that pipe from
the window of the taking onward, until the correspondents set up a new secret. Network:803 names
that limit. Re-keying links (document 04) changes nothing here, because no hop holds the pipe key.

## 2. The protocol

```
healed(W0 + 1) = H_{mt-pipe-heal}( pipe_secret(W0) || ss_h || pk_h || ct_h )      widths 32, 32, 1 184, 1 088
healed(W + 1)  = H_{mt-pipe-ratchet}( healed(W) )                                  the ordinary walk
```

1. **Offer.** A correspondent `O` puts `pk_h`, a fresh ML-KEM-768 encapsulation key, into an
   ordinary letter of the pipe. *When:* in the first letter it sends in a window while no heal of
   its own is pending (Author's decision on Q4, sub-question (i); constitution record 356). `O`
   holds at most one pending offer. An offer that gets no answer is replaced by its next offer:
   on iOS, by the first letter of the next UTC day (Author's decision on Q11, record 409).
2. **Answer.** The other correspondent `A` replies, in an ordinary letter, with
   `ct_h = Encaps(pk_h)` and the last 32 bytes of `pk_h` (ρ), which name the offer it answers.
   `O` opens an answer only when that ρ is its own pending offer's and drops any other: ML-KEM
   decapsulation of a ciphertext made for another key returns noise, not a failure (record 409).
   `W0` is the window of the tag that answering letter travels under. Both
   sides know `W0`: `A` because it deposited there, `O` because it collected there.
3. **Switch at `O`.** Once `O` opens the answer, it computes `healed(W0 + 1)`. It sends under the
   healed walk from the first window the healed walk covers. It collects under both walks until
   it opens a letter of `A` under the healed walk, and then erases the old walk and `sk_h`.
4. **Switch at `A`.** `A` keeps sending under the old walk until it opens a letter of `O` under the
   healed walk. From then on it sends only under the healed walk, and it erases the old walk and
   `ss_h`.

**Cost.** One key generation, one encapsulation, one decapsulation and one hash. The offering
letter grows by 1 184 bytes and the answering letter by 1 088 bytes, cut into blocks of
`chunk_bytes` = 868 bytes (Canon:2242). The cost falls only on letters that are sent anyway.

**Frozen vectors** (Canon:3379-3384), with test inputs `ss_h = 0x22×32`, `pk_h = 0x23×1184`,
`ct_h = 0x24×1088`:

```
pipe_heal(pipe_secret(test_secret, W + 1))  = 112f58fbae5637bb976ac9c6a85ed9bf5d26669a511de8310387a62419d81adc
pipe_secret(healed, W + 3)                  = 43ee0cf3597a6d9450483d33820847866be77ad8cf53191102c0ee8dd401f283
negative, healing one window late:          = 85169530874db497c0d4f527900ac25eff7fc12ac2d132e7c9984b42836924b5
```

## 3. Theorem 3 (a pipe heals after a passive heal)

Let `t` be the last corruption of either correspondent's pipe state. Let `O` draw `(pk_h, sk_h)`
after `t`, and let there be no corruption until both sides have erased the old walk. Suppose the
adversary does not modify or replace the offer or the answer. It may read both, since the old
walk opens the inner seal of those letters, and it may drop either. Then `healed(W0 + 1)`, and so
every tag, right to collect and inner key derived from the healed walk, is indistinguishable from
uniform. (Here the adversary is written `𝒜`, because `A` names the answering correspondent.) For
every adversary `𝒜` there is an adversary `B` such that, for `healed(W0 + 1)` and the `k` values
of the healed walk after it,

```
Adv(𝒜) ≤ Adv^{cpa}_{ML-KEM-768}(B) + (k + 1) · q_H / 2^256
```

*Proof.* The argument is the one for Theorem 2 with one KEM instance. `B` embeds the challenge
`(pk*, ct*, K*)` as `(pk_h, ct_h, ss_h)`. It simulates both correspondents, and it generates the
old walk itself and hands it over at `t`. Then `healed(W0 + 1)` is an oracle output on an input
holding a uniform 32-byte `ss*` the adversary never sees, so it is independent of the view
except with probability `q_H / 2^256`. Every later value of the healed walk is an oracle output
on the value before it, so each of the `k` later values adds at most `q_H / 2^256`, and every tag
and key of a window is an oracle output on the value of its window. If the adversary drops the offer or the
answer, no heal happens; the next offer replaces it, and the pipe survives. ∎

**Tags do not join.** Tags of the healed walk are oracle outputs on values the adversary does not
hold, so they join to tags of the old walk no more than any two tags do. This is the ledger row
proposed at Network:308-311.

## 4. What remains

- **The residue.** Letters `A` sends before it opens a letter of `O` under the healed walk are
  sealed under the old walk, so a holder of the old walk can read them. Network:289 names this
  residue and does not close it. It lasts until `O` next writes.
- **An active attacker.** An attacker holding the old walk can open and re-seal letters of the
  pipe. If it also sits on their path (for instance at a correspondent's first machine), it does
  not need to substitute anything: it can strip the offer from every letter that carries one and
  re-seal the rest unchanged. No heal then happens, both sides stay on the old walk, and the
  attacker keeps reading. The only sign is that offers go unanswered, and the draft gives a side
  no rule for what to do about that. The attacker can also substitute its own `pk_h` and stay in
  the middle, as on a link (document 04, the remark on active attackers), but stripping is
  cheaper. Theorem 3 rules out both by its hypothesis and gives nothing against them. The draft
  also does not say what `A` does when it receives two offers for one pending heal in one
  window. **These are gaps in the draft**, listed in document 08.
- **Crossing heals.** Under the Author's rule for when to offer (Q4 (i)), each correspondent
  offers in the first letter it sends in a window. When both write in one window, two heals are
  in flight at once, and that is the ordinary case, not a race. The draft does not order them.
  A side that is the offering side of one heal and the answering side of the other is told, by
  the same line of Network (:287), both to send under its healed walk and to keep sending under
  the old walk, and the two heals would derive two different healed walks from one old walk.
  The fix needs a fixed order between the two heals, as the handshake roles order crossing
  renewals of a link (document 04, step 6). **This is a gap in the draft**, listed in document
  08.
- **The clients.** In Swift the fold, the heal by the day and the keys of a heal are written and
  checked against the vectors above (document 07, slices C1, C2a and the common part of C2b), and
  so are the letters that carry the offer and the answer (slice C2b-1, records 401 and 402).
  Slice C2b-2 wires the heal into the pipe: the book keeps each pipe's side of its heals beside the
  pipe, sealed as the pipe's secret is (`pipeHeal`, a key of the seat); the pipe derives the seal,
  the tag and the daily label of a letter under the walk the heal names, and opens under both
  walks, the healed one first, telling the heal which one opened; the roads of the app read the
  node box and register under both walks' labels; a heal done rebases the pipe on the healed walk
  as a floor on the day after the answer. One letter read again by another road is the same
  letter: an offer already answered is not answered again. **Not wired:** the sending of offers
  and answers. The devices of one seed hold one pipe (a card's secret goes to the seed's other
  devices, `MontanaFirstContact.swift:722-729`), while a heal is kept by one device: a heal on
  one would strand the others once the correspondent erases the old walk. By the Author's decision
  416 = (a) (record 417) iOS receives and opens a heal and starts none until the seed's devices
  carry one heal between them, its own item of document 08. Kotlin is outside this council's work (decision
  384). The iOS client also still takes the window from the wall clock, with a
  tolerance of one window either side (`MTPipe.swift:24-35`). That is a finding of issue #22, and
  it breaks [I-16] requirement 14.
