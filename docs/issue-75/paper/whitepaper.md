# Transport Indistinguishability, Link Re-keying and Pipe Healing in Montana

**A formal treatment, written in answer to issue #75 of `github.com/efir369999/montana_messenger`**

The Montana council, 2026-10-30 (UTC). Prepared on the Author's word of 17:50 UTC that day.

---

## Status of this paper

- **The proofs are the council's own work.** Nobody outside the council has reviewed them, and
  no independent party has audited them. No machine has checked them either, with one
  exception: Section 5.5 checks Lemma 5 (c) exhaustively within stated bounds. That check also
  found a fault in the iOS client. This paper exists so that someone outside the council can
  check the proofs.
- **Montana does not claim resistance to a global passive adversary.** Section 3.5 shows why no
  design of this kind can make that claim.
- **The routing layer that the anonymity claim is about is not running yet.** The node currently
  puts every cell on every link it holds. Routing by tags is stage 18 of the roadmap (Section 7).
  The re-keying wire code is merged, and the iOS client can receive and open a heal but does not
  start one. Section 7 gives the details.
- **What is normative.** The specification files are normative: Constitution, Network and Canon
  under `core/docs/`. This paper is not normative. Where the two disagree, the specification
  wins, and the disagreement is a defect in this paper.

### How references are pinned

| Mark | Meaning |
|---|---|
| `Constitution:n`, `Network:n` *(main)* | line `n` at commit **`390441f`** on `main` (2026-10-30 17:46 UTC) |
| `Canon:n`, `Network:n` *(9f72fc0)*, code paths in `core/` | line `n` at commit **`9f72fc07b90c`**, the pin of the review package (`../README.md`) |
| `MTPipeKey.swift:n`, `MTPipe.swift:n` | the iOS client at source **`d20b2d66`**, build 2324 |
| `RFC answer` | `core/docs/protocol-13/75-RFC-GPA.md`, the council's longer working answer |
| documents `01`-`08` | the review package in the folder above this one |

---

## Abstract

Montana is a messenger and payment network. Every cell on its wire has the same length and is
sealed again at every hop. The network sends no synthetic cover traffic. We give a formal
treatment of three of its properties.

First, we state the transport invariant [I-16] as an indistinguishability game in the AnoA style.
The game compares two worlds of real traffic and admits every set of observation points that
does not hold both ends of one delivery. We prove that an observer at **any** set of points
cannot tell a payment from a letter, except with the advantage of an attacker on
ChaCha20-Poly1305 (Theorem 1). We prove that a both-ends observer separates destinations with
advantage 1 (Proposition 1). We also show that the invariant's original wording cannot hold when
the adversary chooses the workload (Proposition 2). This finding led the Author to restate
[I-16] as a bound of the form `ε(D_bg, profile, ℓ) + negl(λ)` (Claim C3′, now the text of the
Constitution).

Second, we specify how a link renews its keys with ML-KEM-768 once in every window in which it
carries anything, and prove forward secrecy across renewals (Theorem 2). We also prove that the
link heals after any renewal the adversary does not tamper with (Theorem 3).

Third, we specify how an end-to-end pipe heals in band, inside two ordinary letters, and prove
that it heals (Theorem 4). We prove a new lemma for the case where both correspondents offer a
heal in the same window: the two sides agree on one healed walk, no pattern of dropped letters
makes them diverge, and security costs a factor of two (Lemma 5).

Every theorem relies on ML-KEM-768 being IND-CPA secure and on domain-separated SHA-256 being a
random oracle. Section 4.5 states what a proof in the standard model would need.

---

## 1. Introduction

The RFC of issue #75 asked Montana for four things:

1. a proof of resistance to a global passive adversary (GPA), against an idle-noise distribution
   `D_idle`;
2. adaptive stochastic padding;
3. a hybrid post-quantum handshake per hop, with "immediate" post-compromise security;
4. zero-knowledge resynchronisation of a token queue.

The council's position on each request is in document 06. This paper is the formal part of that
position.

**Contributions.**

| # | Result | Where | Standing |
|---|---|---|---|
| T1 | KIND indistinguishability for every set of points | Section 3.4 | proved |
| P1 | Both-ends observers win with advantage 1 | Section 3.5 | proved |
| P2 | The original wording of [I-16] fails under an adversary-chosen workload | Section 3.6 | sketch; a finding |
| C3′ | The bound that replaces the original wording | Section 3.7 | adopted (decision 436 = (a)); `ε` not yet measured |
| T2 | Forward secrecy across link renewals | Section 4.2 | proved |
| T3 | A link heals after a passive renewal | Section 4.3 | proved |
| T4 | A pipe heals after a passive heal | Section 5.2 | proved |
| L5 | Crossing heals: uniqueness, agreement, no divergence on loss, security | Section 5.3 | proved (new in this paper) |
| M5 | Lemma 5 (c) checked over every reachable state, and a fault found in the iOS client's rule | Section 5.5 | exhaustive within bounds; `check_lemma5.py` |

**What this paper does not do.** It does not treat consensus, coins or the handshake itself. The
handshake uses ML-KEM-768 in both directions and ML-DSA-65 over the transcript (Canon:3076-3095),
and nothing here changes it. The paper does not bound `ε` for the REL and ORIG challenge types,
because that bound is a measurement over routing code that does not exist yet.

---

## 2. Model

### 2.1 Objects

**Window.** The chain advances in windows. `W` always denotes the height of the last cemented
window. It is read from the chain and never from a clock ([I-15]; [I-16] requirement 14). A window
lasts nominally sixty seconds, but no protocol rule reads seconds.

**Link.** A link is a channel between two neighbouring machines, opened by a four-flight
post-quantum handshake. The side that opened the connection is the initiator `i`; the other side
is the responder `r`. The handshake yields a value `master` and two directional keys,
`k_i2r = H_{mt-noise-pq-v1-i2r}(master)` and `k_r2i = H_{mt-noise-pq-v1-r2i}(master)`.

**Unit.** A unit is `cell_bytes` = 1 232 bytes (Canon:2152): a 16-byte label, a 12-byte nonce
drawn afresh for each unit, and a ChaCha20-Poly1305 seal under the sender's directional key.
A message of a link is cut into pieces of 1 183 bytes. The message's kind travels inside the
seal (Canon:2453-2467).

**Pipe.** A pipe is the end-to-end relation between two correspondents. They share a secret
that walks one way, one step per window:

```
pipe_secret(W + 1) = H_{mt-pipe-ratchet}(pipe_secret(W))                 Canon:1358-1361
tag(W)             = H_{mt-tag}(pipe_secret(W) ‖ W_8B_LE)[0..16]          Canon:1347
```

A letter's cell is sealed twice: an outer layer per hop under the link key, and an inner layer
under the window's pipe key, which no hop holds.

**Delivery.** A delivery `d` carries one letter from an origin device to a destination device.
Its ends are `ends(d) = {line(origin(d)), line(destination(d))}`, where `line(x)` is the access
line of device `x`.

### 2.2 Primitives and notation

| Primitive | Use | Sizes |
|---|---|---|
| ML-KEM-768 (FIPS 203) | the handshake, link renewal, pipe heal | encapsulation key 1 184 B, ciphertext 1 088 B, shared secret 32 B |
| ML-DSA-65 (FIPS 204) | handshake signatures only | public key 1 952 B, signature 3 309 B |
| SHA-256 | every derivation, always domain-separated | 32 B |
| ChaCha20-Poly1305 (RFC 8439) | the seal of every unit and cell | 12 B nonce, 16 B tag |

We write `H_d(x) = SHA-256(d ‖ 0x00 ‖ x)`, where `d` is an ASCII domain string from Canon's
registry of separators. No domain string contains `0x00`, so inputs under distinct domains never
collide, even when one domain string is a prefix of another. Every field has a width fixed by
Canon, so inside one domain the concatenation splits back into its fields without length
prefixes.

### 2.3 Assumptions

- **(A1)** ML-KEM-768 is IND-CCA2 secure. Every theorem here needs only IND-CPA, which IND-CCA2
  implies. We write `Adv^{cpa}_{KEM}(B)` for the advantage of an adversary `B`, and
  `δ_KEM ≈ 2^-164.8` for the decapsulation failure rate that FIPS 203 states for ML-KEM-768.
- **(A2)** SHA-256 under distinct domains behaves as a family of independent random oracles.
  This is the only point where the proofs leave the standard model (Section 4.5).
- **(A3)** ChaCha20-Poly1305 with nonces drawn uniformly from 96 bits is IND$-CPA and INT-CTXT for
  `q` seals under one key, up to a nonce-collision term of at most `q^2 / 2^97`. We write
  `Adv^{ind$}` and `Adv^{int}`.
- **(A4)** Erasure works: once the protocol erases a value, the value cannot be recovered from
  the machine. This is an assumption about the implementation (`Zeroizing` buffers), not a
  theorem.
- **(A5)** Two honestly generated ML-KEM-768 encapsulation keys share their last 32 bytes, the
  seed `ρ`, with probability at most `γ`. Equal keys share `ρ`, so `γ` also bounds the
  probability that two keys are equal. In key generation `ρ` is an output of the hash `G` on a
  fresh 32-byte seed (FIPS 203), so `γ ≤ 2^-255`: either the two seeds collide or `G` does.

**Corruption.** `Corrupt(P, t)` gives the adversary everything machine `P` holds for one link (or
one pipe) at time `t`: the secret, both directional keys, the state of any renewal or heal in
flight, and any previous receiving key still kept. Compromising the whole machine, including the
long-term secret from which its signing key derives, is a stronger event. The theorems say where
that matters.

---

## 3. Transport indistinguishability

### 3.1 The invariant

The Constitution states [I-16] as follows (Constitution:386-396, *main*). The text below is the
form adopted by decision 436 = (a).

> For any object the network carries — a payment, a message, a chunk of media, a unit of
> consensus data — an observer of the wire at one point, and at any set of points that does not
> hold both ends of the same delivery, cannot determine what the object is; its advantage in
> telling who originated it, for whom it is destined, or whether a given machine is its origin
> or a relay is bounded by the honest volume on the points it observes, by the delay profile and
> by the length of the path, and falls toward nothing as that volume grows. An observer holding
> both ends is the one case the invariant does not close.

The RFC asked for a game against an idle-noise distribution `D_idle`. That game cannot be stated
for Montana, because Montana sends no synthetic traffic and so has no idle distribution: "Synthetic
filler therefore does not exist" (Constitution, [I-16] requirement 3; Canon:2530). The game below
compares two worlds of real traffic instead. It follows the AnoA framework [BKMMM13].

### 3.2 The game `G^X_Π(A, P)`

The game is parameterised by a protocol `Π`, a challenge type `X ∈ {KIND, REL, ORIG}`, and a set
`P` of observation points. Each point is a link or an access line.

1. **Setup.** The challenger starts `N` machines running `Π` over a chain that advances, with
   honest devices attached.
2. **Background.** The two variants differ in who chooses the traffic the adversary does not
   challenge on. In `G_adv` the adversary chooses the whole workload. In `G_bg` that traffic is
   drawn from a fixed distribution `D_bg`.
3. **Challenge.** The adversary names two scenarios, `σ_0` and `σ_1`, of type `X`. The challenger
   draws a bit `b` uniformly and runs `Π` on the background plus `σ_b`.
4. **Observation and action.** On every point in `P` the adversary sees every unit: its bytes,
   its canonical slot and its direction. It may delay, drop, reorder or inject units there. It
   corrupts no machine (Section 8 discusses corruption).
5. **Output.** The adversary outputs a guess `b'`. Its advantage is
   `Adv = |2·Pr[b' = b] − 1|`.

**Equal volume.** In both scenarios, every honest device sends and receives the same number of
letters, of the same length classes, in the same windows. The volume of a device's own activity
is therefore identical in the two worlds by construction.

**Admissibility.** `P` is admissible for `(σ_0, σ_1)` if `ends(d) ⊄ P` for every delivery
`d ∈ σ_0 ∪ σ_1`. This is the invariant's exclusion of both-ends observers, and nothing more.

### 3.3 Challenge types and leakage

- **KIND.** Each scenario holds one delivery, with the same ends, the same length and the same
  class of carriage. In one world it is a payment and in the other a letter ([I-16] requirement 1).
- **REL.** `σ_0 = {u_0 → v_0, u_1 → v_1}` and `σ_1 = {u_0 → v_1, u_1 → v_0}`.
- **ORIG.** A machine `m` originates a delivery in `σ_0`, and in `σ_1` relays a delivery of the
  same length that was originated elsewhere.

**Leakage function.** The observability ledger names what a wire observer may learn:

| # | Leaked | Source |
|---|---|---|
| L1 | the number of units per canonical slot on each observed line | Network:793 *(9f72fc0)* |
| L2 | for a device that carries nothing for others, the moments and volumes of its own activity | Constitution:530-534 *(9f72fc0)* |
| L3 | that a live stream is running, at its constant rate | Network:619, :819 *(9f72fc0)* |
| L4 | that a link carried anything in a window, and that it renewed in that window | Network:802 *(9f72fc0)*; Network:332-333 *(main)* |
| L5 | that a machine answers at an address | Network:824 *(9f72fc0)* |
| L6 | to the machine a device asks, that some device is joining | Network:796 *(9f72fc0)* |

The equal-volume condition of Section 3.2 makes L1-L6 equal in the two worlds, as functions of
the workload. Anything else an admissible adversary learns violates [I-16].

### 3.4 Theorem 1 (KIND)

**Theorem 1.** Let `P` be any set of observation points, admissible or not. Let `A` be an
adversary that makes `q` observations of units and `q'` injections. Then in the KIND game

```
Adv(A) ≤ 2q · Adv^{ind$}_{ChaCha20-Poly1305} + q' · Adv^{int}_{ChaCha20-Poly1305}.
```

**Scope condition.** The bound covers the delivery's own cells, and it assumes that what the
recipient does after opening the delivery is the same for both kinds. A reaction that depends on
the kind, for example a transfer that the recipient then announces to the chain, leaves a trace
of its own. That trace is counted against [I-16] requirement 1. The theorem does not hide it.

*Proof.* We take the parts of the adversary's view of a cell in turn.

*Length.* Every cell is `cell_bytes` = 1 232 bytes, whatever its content (Canon:2152). A message
is cut into 1 183-byte pieces, and its last piece is padded to the width of a unit (Canon:2464-2467).
Both scenarios have the same length by definition, so they produce the same number of units.

*Moment.* A cell is sent in its canonical slot. The slot is derived from the window and from
the sender's ephemeral identity, never from the content (Network *(9f72fc0)*, "The canonical
slot"). Both scenarios have the same ends and the same windows, so the slots are the same.

*Open fields.* A cell's only unsealed fields are its target commitment and its step label. The
target commitment is a function of the pipe and the window. The step label takes nothing from
the delivery (Canon:1347-1348). Both are the same in the two worlds.

*Path.* A payment has no transport of its own: it travels the path of the pair, as a letter does
(Constitution, [I-16]: "The path an object takes discloses nothing that its content conceals").

*Sealed bytes.* So the two views differ only in the plaintexts under the seals. There are two
seals, the routing layer and the delivery layer, and each is ChaCha20-Poly1305 with a nonce drawn
afresh for every cell. No unsealed field tells one kind of content from another, and the cell is
sealed again at every hop.

*Hybrids.* First apply the game hop of *Injections* below. After it, every honest receiver
rejects every cell that no honest sender sealed, and the adversary's success probability has
changed by at most the second term of the bound. Let `k_1, …, k_K` be the keys under which the
observed cells are sealed, at either layer, so `K ≤ 2q`. In hybrid `H_i`, every string sealed
under `k_1, …, k_i` is replaced by a uniform string of the same length. A distinguisher between
`H_{i−1}` and `H_i` is an IND$-CPA adversary against `k_i`. It simulates every other key itself
and asks its oracle for every seal under `k_i`. It never has to open anything under `k_i`: after
the hop above, a receiver accepts under `k_i` only strings that the oracle returned, and the
distinguisher answers those from its own table of what it asked to seal. A per-ciphertext hybrid
would not reduce in this way, because the distinguisher holds no key with which to make the other
real seals under `k_i`. So each of the at most `2q` steps changes the adversary's success
probability by at most `Adv^{ind$}`, taken over as many queries as there are cells under that
key. In the last hybrid no observed byte depends on `b`.

*Injections.* An injected or altered cell of the wrong length is discarded silently. A cell
whose step-header was already accepted in the window is a replay, and the digest set of the
window discards it without an answer (Network:686-691, *main*). A new cell of the right length
that does not open ends the link, and a new handshake replaces it. All three reactions are the
same in both worlds unless the injected cell forges a seal. Each of the `q'` injections forges
with probability at most `Adv^{int}`, and a union bound gives the second term. ∎

**Corollary.** For KIND, the `ε` of Claim C3′ is zero.

### 3.5 Proposition 1 (both ends)

**Proposition 1.** Suppose deliveries complete within a horizon of `h` windows, and no unit is
ever sent unless it carries a piece of a real message or of real transit. Then in `G_adv` there
is an adversary with `P = ends(d)` and advantage 1 against `σ_0 = {u → v}` and `σ_1 = {u → v'}`,
where the workload is otherwise empty.

*Proof.* In `σ_1`, no letter is addressed to `v`. With no other workload, `v` carries no transit,
and no filler exists. In `σ_0`, the delivery brings at least one unit onto `line(v)` within `h`
windows. Consensus objects also cross `line(v)`, but their volume does not depend on `b`, and in
the clean model the adversary knows it exactly. The adversary outputs 0 exactly when it sees
letter units on `line(v)` within the horizon. ∎

This is a small instance of the anonymity trilemma [DMMK18]: strong anonymity requires bandwidth
overhead or latency overhead. Montana adds latency only to mail for a sleeping device, at most
one window, drawn uniformly. It adds no bandwidth beyond real traffic. The specification
therefore names the both-ends case as a physical prohibition and does not claim to close it.
Proposition 1 is the formal reason the RFC's first request is refused.

### 3.6 Proposition 2 (interior observer; a finding)

**Proposition 2.** Consider `G_adv` with challenge type REL. Let `P` be every link except the
access lines of `v_0` and `v_1`, let the first machines of `v_0` and `v_1` differ and be known to
the adversary, and let the workload otherwise be empty. Then `P` is admissible, and an adversary
wins with advantage close to 1.

*Sketch.* `P` holds no recipient line, so it is admissible. The units of `u_0` are sealed again
at every hop, so no two observations match bit for bit. Their volume, however, moves hop by hop
across links in `P`, from `line(u_0)` to the holder `H` of the pipe's tag. With nothing else in
flight, `H` is the machine where that volume stops. The recipient then collects, which is a
second flow from the recipient's first machine toward `H`, again across links in `P`. The
adversary pairs each collection flow with the holder it reaches. Slot batching removes fine
timing but not volume, and nothing hides a flow when there are no other flows. ∎

**Consequence.** Read word for word, the original [I-16] claimed indistinguishability for every
set of points short of both ends. When the adversary chooses the workload, no design without
synthetic filler can give that. The Constitution already says as much in prose: mixing in other
people's traffic sets "the price of the correlation and not its impossibility".

### 3.7 Claim C3′ (adopted as the text of [I-16])

**Claim C3′.** In `G_bg`, for every PPT adversary and every admissible `P`,
`Adv ≤ ε(D_bg, profile, ℓ) + negl(λ)`. Here `ε` depends on the honest volume on the links of `P`,
on the delay profile and on the path length `ℓ`, and it tends to 0 as that volume grows. For
KIND, `ε = 0` (Theorem 1).

The Author adopted this claim on 2026-10-30 (decision 435, record 436, council link 13395,
commit `1e2a294`), and it is now the second sentence of [I-16]. Bounding `ε` for REL and ORIG is
open. There are two routes: an analytical bound in the style of the mix-network literature, or
the measurement of stage 22 of the roadmap, "what an observer sees". Both need routing by tags.
Long-term intersection attacks [Dan03] fall outside the single-challenge game and need a
multi-round variant.

---

## 4. Re-keying a link

### 4.1 Protocol

Today a link keeps its two handshake keys for its whole life (`mt-net/src/lib.rs:243-271`). If
an adversary reads them at any moment, it can read the link's past and future traffic. The
renewal fixes that. The text is adopted in Network, section "How a link renews its keys"
(decision 333 = (b); PR #121, `605c843`), and its wire code is merged (PR #125, `0f8e300`).

```
ls_0     = master
ls_n     = H_{mt-link-renew}( ls_{n-1} ‖ ss_i ‖ ss_r ‖ pk_i ‖ pk_r ‖ ct_i ‖ ct_r )
k_i2r(n) = H_{mt-noise-pq-v1-i2r}(ls_n)          k_r2i(n) = H_{mt-noise-pq-v1-r2i}(ls_n)
```

The field widths are 32, 32, 32, 1 184, 1 184, 1 088 and 1 088 bytes. The fields are always
written in the roles of the handshake, whichever side offers, so both sides compute the same
value. At `n = 0` the keys are exactly the handshake's keys.

| Kind | Message | Body | Units |
|---|---|---|---|
| 15 | `renew_offer` | `pk_o`, a fresh encapsulation key | 2 |
| 16 | `renew_answer` | `ct_a = Encaps(pk_o)`, then a fresh `pk_a` | 2 |
| 17 | `renew_finish` | `ct_o = Encaps(pk_a)` | 1 |

All three messages are ordinary link messages, sealed under the current keys, with their kind
inside the seal. No window number, counter or index crosses the wire.

1. **Offer.** A side sends `renew_offer` before its first message in a window later than the one
   in which `ls_n` was set. It also offers once it has sealed `rekey_units = 2^23` units under its
   current key. A link that carries nothing in a window renews nothing.
2. **Answer.** The other side encapsulates to `pk_o`, draws `(pk_a, sk_a)`, and replies.
3. **Finish.** The offering side decapsulates, encapsulates to `pk_a`, sends `renew_finish` under
   the old keys, and from then on seals under `k(n+1)`.
4. **Switch.** The answering side decapsulates `ct_o`, computes `ls_{n+1}`, and from then on
   seals under `k(n+1)`.
5. **Erase.** Each side keeps its old *receiving* key until the first unit opens under the new
   key. It then erases that key, `ls_n`, its decapsulation key, and both shared secrets.
6. **Crossing.** If both sides offer in the same window, the offer of the handshake initiator
   stands.
7. **Failure.** A unit that opens under neither key ends the link silently, and a new handshake
   replaces it.

**Cost.** Five units (6 160 bytes) per active window, and no signature. A new handshake costs
fourteen units and two signatures.

**Why `2^23`.** A gigabit link carries about 6 087 662 units in a nominal 60-second window, which
lies between `2^22` and `2^23`. So in a chain that keeps advancing, the window triggers the
renewal first, and the ceiling binds only when the chain stalls or the link is faster than
gigabit. With 96-bit random nonces, the probability of a nonce repeat at the ceiling is at most
`2^46 / 2^97 = 2^-51`.

### 4.2 Theorem 2 (forward secrecy across renewals)

**Theorem 2.** Suppose renewal `n` has completed at side `P`, and `P` has erased its old
receiving key, before `Corrupt(P, t)`. Then for every epoch `m < n`, the units sealed under
`k(m)` keep the security they had against an adversary that never corrupted `P`. The corruption
adds at most `(n + 1) · q_H / 2^256` to the adversary's advantage, where `q_H` is the number of
random-oracle queries it makes.

*Proof.* `Corrupt` returns `ls_n` and the material of epoch `n`. By (A4) it returns nothing from
an epoch `m < n`, because step 5 erased those values. Each `ls_m` is an oracle output on a fresh
input, and each `k(m)` is an oracle output on `ls_m`. The adversary's view of `ls_m` changes only
if it queries the oracle on an input that contains `ls_m`. The outputs it does hold, `ls_n`
among them, are independent uniform values, so in its view `ls_m` is uniform over `2^256`
values, and `q_H` queries hit it with probability at most `q_H / 2^256`. A union bound over
`m = 0, …, n` gives the term. If the old receiving key has not yet been erased, the same argument
covers `m < n − 1`, and epoch `n − 1` is exposed in that one direction. ∎

### 4.3 Theorem 3 (healing after a passive renewal)

**Theorem 3.** Let `t` be the last corruption of either side, and suppose:

- renewal `n + 1` begins after `t`;
- no corruption of either side occurs until that renewal's erasures are done;
- the adversary does not modify, inject or reorder the units of renewal `n + 1`, and injects no
  renewal message of its own under `k(n)` before that renewal completes. Dropping units is
  allowed; it only ends the link.

Then for every adversary `A` there is an adversary `B`, running in about the time of `A` plus
one simulation of the link, such that for the keys of epoch `n + 1` and of the `e` epochs after it

```
Adv(A) ≤ Adv^{cpa}_{ML-KEM-768}(B) + (e + 1) · q_H / 2^256.
```

*Proof.* `G_0` is the real game. In `G_1`, the shared secret of `ct_o = Encaps(pk_a)` is replaced
by an independent uniform value `ss*`.

*From `G_0` to `G_1`.* `B` receives an IND-CPA challenge `(pk*, ct*, K*)`. It simulates both
honest sides and generates every other value itself, including `ls_n`, which it hands to the
adversary at time `t`. It sets `pk_a := pk*` and `ct_o := ct*`, and uses `K*` wherever the
protocol needs that shared secret. The answering side would normally get the secret by
decapsulating `ct_o`, and it needs `sk_a` for nothing else. No corruption falls inside the
renewal, so `B` never has to reveal `sk_a`. The adversary is passive on the renewal, so `ct_o` is
exactly the honest encapsulation, and no decapsulation oracle is needed. If `K*` is real, `B`
simulates `G_0`; if it is random, `B` simulates `G_1`.

*In `G_1`.* `ls_{n+1}` is an oracle output on an input that contains the uniform `ss*`, which
the adversary never sees. As in Theorem 2, the output is independent of the adversary's view
unless it queries that exact input, which happens with probability at most `q_H / 2^256`. Each
of the `e` later secrets is an oracle output on the secret before it, and each adds at most
`q_H / 2^256`. ∎

**Remark (one fresh side is enough).** The proof uses only `pk_a`. Embedding the challenge in
`pk_o` and `ct_a` instead gives the same bound. So the theorem holds even if only one of the two
key pairs was drawn after the last corruption, which is the property Canon states for the
handshake (Canon:3111).

**Remark (active attacker).** An attacker that holds `ls_n` can seal units under `k(n)`, so it can
offer its own key and stay in the middle of every later renewal. No unauthenticated renewal can
exclude it. To do so it must re-seal every unit in both directions for as long as the link
lives. If it misses one unit, a unit fails to open and the link ends, and the next handshake,
signed by both machines, heals the link. Signing the renewal (option Q2(b), 11 units) would
exclude an attacker that read the link but did not take the machine. It would not help against
an attacker that took the machine, because the signing key derives from the machine secret. Q2
is open with the Author.

**Remark (implicit rejection).** ML-KEM decapsulation never fails; a tampered ciphertext yields a
pseudorandom shared secret. A tampered renewal therefore leaves the two sides with different
`ls_{n+1}`. The first unit under the new key fails to open, and step 7 ends the link. No
divergence outlives the link.

### 4.4 Exposure at the ceiling

The ceiling is where a side *offers*, not where its old key stops. The offering side keeps
sealing under the old key until it reads the answer. The exact excess, and a proposed closure
(`Link::settle`, branch `agent-731/issue-75-ceiling-settle`), are given in document 08, under
"The exposure bound at the ceiling". The code merged by PR #125 bounds the excess with a hard
stop at a second ceiling. A key that would seal more than `2 · rekey_units` units of its callers'
messages refuses the whole message (`NetError::KeyExhausted`), and the link ends. Only the three
renewal messages are exempt from that count (`mt-net/src/lib.rs:465-474` at `390441f`; test
`a_key_whose_offer_goes_unanswered_seals_no_more_than_twice_its_ceiling`, `:1130-1145`). So at
most `2^24 · 1 232` bytes, plus those three messages, travel under one key per direction, and
the nonce bound is about `2^-49`. The item in document 08 that says the node does not bound
this predates the merge.

### 4.5 Remark on the standard model

The proofs model SHA-256 as a random oracle (A2). A standard-model proof of Theorem 3 would need
`H_{mt-link-renew}` to be a PRF when keyed by `ss*`. But `ss*` sits in the middle of the input, and
the adversary knows `ls_n`, which comes before it. That is a dual-PRF assumption on plain SHA-256
used as a Merkle-Damgård hash, which is not HMAC or HKDF. Analyses of TLS 1.3 make comparable
assumptions on HMAC [BL15, BBGS23]. Montana's outputs are used only as keys, or are published
truncated as tags, and never as MACs over attacker-chosen suffixes, so length extension gives an
attacker nothing. Whether plain SHA-256 is an acceptable KDF here is question 2 for reviewers. If
it is not, the change touches every derivation of the specification under [I-7], not only these
two.

---

## 5. Healing a pipe

### 5.1 Protocol

A pipe's walk gives forward secrecy but does not heal. Anyone who takes a device, or a copy of
its storage, reads the pipe from then on. A heal fixes that (Network:302-319, *main*):

```
healed(W0 + 1) = H_{mt-pipe-heal}( pipe_secret(W0) ‖ ss_h ‖ pk_h ‖ ct_h )     widths 32, 32, 1 184, 1 088
healed(W + 1)  = H_{mt-pipe-ratchet}( healed(W) )
```

1. **Offer.** Correspondent `O` puts a fresh encapsulation key `pk_h` into an ordinary letter.
   It does so in the first letter it sends in a window while it holds one walk and awaits no
   answer to an offer of its own from that window (records 356, 392 and 409).
2. **Answer.** Correspondent `A` replies, in an ordinary letter, with `ct_h = Encaps(pk_h)` and
   `ρ(pk_h)`, the last 32 bytes of `pk_h`. `O` opens an answer only if its `ρ` names `O`'s pending
   offer, and drops any other answer. `W0` is the window of the tag the answering letter travels
   under, and both sides know it.
3. **Switch at `O`.** `O` sends under the healed walk from the first window it covers. It collects
   under both walks until it opens a letter of `A` under the healed walk, and then erases the old
   walk and `sk_h`.
4. **Switch at `A`.** `A` keeps sending under the old walk until it opens a letter of `O` under the
   healed walk. From then on it sends under the healed walk alone, and erases the old walk and
   `ss_h`.
5. **Replacement.** If an offer gets no answer in its window, the side's first letter in a later
   window replaces it, and the old key is erased. A late answer to the replaced offer names a `ρ`
   that is no longer held, so it is dropped.

The frozen vectors are Canon:3379-3384. The iOS client checks them in
`MTPipeKey.swift:596-621`, including the named wrong heal one window late.

### 5.2 Theorem 4 (a pipe heals after a passive heal)

**Theorem 4.** Let `t` be the last corruption of either correspondent's pipe state, and suppose:

- `O` draws `(pk_h, sk_h)` after `t`;
- no corruption occurs until both sides have erased the old walk;
- the adversary does not modify or replace the offer or the answer. It may read both, because
  the old walk opens their inner seal, and it may drop either.

Then for every adversary `𝒜` there is an adversary `B` such that, for `healed(W0 + 1)` and the
`k` values of the healed walk after it,

```
Adv(𝒜) ≤ Adv^{cpa}_{ML-KEM-768}(B) + (k + 1) · q_H / 2^256.
```

The same bound covers every tag, right to collect and inner key derived from those values,
because each is an oracle output on the value of its window.

*Proof.* This is the argument of Theorem 3 with one KEM instance. `B` embeds its challenge
`(pk*, ct*, K*)` as `(pk_h, ct_h, ss_h)`. It simulates both correspondents, generates the old walk
itself, and hands it to the adversary at time `t`. Then `healed(W0 + 1)` is an oracle output on an
input that contains a uniform 32-byte value the adversary never sees. Each later value of the
healed walk adds at most `q_H / 2^256` by the same argument. If the adversary drops the offer or
the answer, no heal happens, a later offer replaces this one, and the pipe survives. ∎

**Tags do not join.** Tags of the healed walk are oracle outputs on values the adversary does
not hold. They are no more linkable to tags of the old walk than any two tags are.

### 5.3 Lemma 5 (crossing heals)

When both correspondents write in the same window, each offers in its first letter, so their
offers cross. This is the ordinary case, not a race. The rule of decision 391 = (a) handles it
(Network:313-319 and :330, *main*): of two crossing offers, the one whose encapsulation key is
lower in byte order stands. The side whose offer did not stand erases its key and answers the
standing offer. The side whose offer stands answers nothing. The iOS client implements the
comparison as `mine != theirs && mine.lexicographicallyPrecedes(theirs)`
(`MTPipeKey.swift:587-589`). Equal keys therefore count as no crossing, so a side does not
mistake a reflection of its own offer for the other side's.

**Setting.** `X` and `Y` hold the same old walk `w`. In one window each sends an offer, `pk_X` and
`pk_Y`, before opening the other's. Let `S` be the side with the lower key and `T` the other, so
that `pk_S` precedes `pk_T` in byte order. The adversary may drop any letter, and may delay it
past the window, but does not alter or inject offers or answers. An offer delayed into a later
window, where it meets an offer drawn in that window, is outside this setting (Section 8,
item 10).

**Lemma 5.** Except with probability `γ + δ_KEM`:

- **(a) Uniqueness.** Every healed walk that `X` or `Y` derives from `w` is
  `H_{mt-pipe-heal}(w(W0) ‖ ss ‖ pk_S ‖ ct)`, for the one answer `ct` that `T` produced. So at most
  one healed walk exists, and it is folded over the lower key.
- **(b) Agreement.** If `S`'s offer reaches `T`, and `T`'s answer reaches `S` while `pk_S` is still
  pending, then `S` and `T` derive the same healed walk.
- **(c) No divergence under loss.** For every pattern of dropped or late letters, no side erases
  `w` while the other side lacks the healed walk, and the two sides never hold two different
  healed walks. A lost letter costs at most the heals of that window.
- **(d) Security.** If `pk_X` and `pk_Y` are both drawn after the last corruption, and there is no
  corruption until both sides have erased `w`, then the standing heal satisfies Theorem 4 with
  `Adv^{cpa}` replaced by `2 · Adv^{cpa} / (1 − γ)`.

*Proof of (a).* A side folds a heal in only two ways: (i) as the answering side, over an offer it
answered; (ii) as the offering side, on opening an answer that names the `ρ` of its own pending
offer. Take each side and each way in turn.

- `T` by (i): `T` compares the two keys on opening `S`'s offer and answers `pk_S` and nothing else.
  If it never opens `S`'s offer, it answers nothing.
- `T` by (ii): this would need an answer naming `ρ(pk_T)`. Only `S` could produce one, and `S`
  answers nothing to `pk_T`: if it opens `T`'s offer, its own offer stands; if it never opens it,
  it has nothing to answer. The adversary injects nothing. An answer to `pk_S` names `ρ(pk_T)` only
  if `ρ(pk_S) = ρ(pk_T)`, which happens with probability at most `γ` (A5). Moreover `T` erased
  `sk_T` when it compared the keys.
- `S` by (i): `S` answers nothing to `pk_T`.
- `S` by (ii): only on `T`'s answer, which names `ρ(pk_S)`, so the fold is over `pk_S` and that
  answer's `ct`.

An honest side answers an offer at most once (in the client, an offer already answered is not
answered again; document 05, section 4). So `T` produces at most one `ct`, carried in one letter
under one tag, which fixes one `W0`. Every fold therefore takes the same four inputs. ∎(a)

*Proof of (b).* `T` computes `(ct, ss) = Encaps(pk_S)`, and `S` computes `ss' = Decaps(sk_S, ct)`.
`ss' = ss` except with probability `δ_KEM` (A1). Both sides hold `w(W0)`: `T` because it deposited
under that tag, `S` because it collected there. Both hold the same bytes `pk_S` and `ct`, because
the adversary alters nothing. So the inputs agree, and so does the fold. Whether `T`'s offer ever
reached `S` makes no difference to the result. ∎(b)

*Proof of (c).* The safety comes from the erasure rule (Network:306 and :329, *main*): a side
erases `w` only on opening a letter of the other side under the healed walk. By (a) only one
healed walk exists, so opening such a letter proves that the other side holds it. We list the
cases by which letters arrive.

1. *`S`'s offer is lost.* `T` never compares, and its own offer stays pending. If `S` opens `T`'s
   offer, it answers nothing. No answer exists, nobody folds, and both offers are replaced in a
   later window.
2. *`S`'s offer arrives; `T`'s answer is lost or arrives after `S` replaced `pk_S`.* `T` has folded
   as the answering side, and it keeps sending under `w` until it opens a letter of `S` under the
   healed walk. `S` never moves to the healed walk: either it has no answer, or the late answer
   names a `ρ` it no longer holds and is dropped (step 5). Nobody erases `w`.
3. *Both arrive in time.* By (b) both sides hold one healed walk. `S` sends under it and collects
   under both walks. `T` erases `w` only on opening a healed letter of `S`, and `S` erases only on
   opening a healed letter of `T`. If such a letter is lost, the side that would have received it
   simply keeps `w` and keeps collecting under both walks.
4. *`T`'s offer, arriving or not*, changes nothing above, because `S` answers it in no case.

In no case does a side erase `w` without proof that the other side holds the healed walk, and by
(a) there is no second healed walk to diverge to. ∎(c)

*Proof of (d).* By (a), `pk_T` is folded nowhere, and `sk_T` was erased unused, so `T`'s instance
adds nothing to the adversary's view beyond a public key. The difficulty is that a reduction
does not know in advance which key will stand. `B` receives `(pk*, ct*, K*)`, draws one honest
key pair `(pk', sk')` itself, and gives `pk*` to `X` or to `Y` by a fair coin, with `pk'` going to
the other side. The pair `(pk_X, pk_Y)` is then distributed exactly as two independent honest
draws. Let `E` be the event that `pk*` is the lower key. The two keys are equal with some
probability `γ_0 ≤ γ` (A5), and otherwise each is the lower one with the same probability. So
`Pr[E] = (1 − γ_0) / 2 ≥ (1 − γ) / 2`.

- On `E`, `pk*` stands. `B` embeds `ct*` as `T`'s encapsulation and `K*` as `ss`, exactly as in
  Theorem 4, and outputs what the adversary outputs.
- Otherwise `B` knows `sk'` for the standing key and cannot use its challenge, so it outputs a
  uniform bit.

Once the two keys are fixed and distinct, `E` is decided by the coin alone. It holds exactly
when the coin put `pk*` on the side whose key is lower. The coin is uniform and independent of
the pair. Nothing the adversary sees depends on which of the two keys `B` drew itself, because
`pk*` and `pk'` are independent draws from one distribution. So, conditioned on `E`, the
adversary's view (both keys, which side holds which, and everything after) has the same
distribution as conditioned only on the keys being distinct. On `E` the adversary's advantage is
therefore its advantage `δ` in the hybrid step of Theorem 4. Off `E`, `B` answers with a uniform
bit, so `Adv^{cpa}(B) = Pr[E] · δ`. Hence `δ = Adv^{cpa}(B) / Pr[E] ≤ 2 · Adv^{cpa}(B) / (1 − γ)`.
The oracle terms are those of Theorem 4. ∎(d)

**Remark (what the rule gives an active attacker).** An attacker that holds `w` and sits on the
path can inject an offer whose key begins with a low byte, and so always win a crossing. But
such an attacker can already strip or substitute offers (Section 5.4), so the byte-order rule
gives it no capability it lacked. The rule is a tie-break between honest parties, and Lemma 5
claims nothing beyond that.

**Remark (why byte order).** The two sides need to agree on the winner without exchanging another
letter, and a link settles crossing renewals by handshake role. A pipe's two correspondents have
no such roles. Byte order on the two keys is a total order that both sides can compute from the
same two values. The client's vector check rejects any other comparison, such as little-endian
or length-first, because those give a different answer on the fourth test pair
(`MTPipeKey.swift:591-595`, `:604-605`, `:618`).

### 5.4 What a heal does not close

- **The residue.** Until it opens a healed letter of `O`, `A` keeps sending under the old walk, so
  a holder of the old walk can read those letters. Network:308 *(main)* names this residue and
  does not close it. The iOS client heals by the UTC day, so on iOS the residue lasts until the
  end of the answering day (record 400).
- **Stripped offers.** An attacker that holds the old walk and sits on the pipe's path can strip
  every offer and re-seal the rest of each letter. Then no heal happens, and offers that go
  unanswered tell a side nothing it can act on. Network:308 *(main)* names this case and does
  not close it. Theorem 4 and Lemma 5 exclude it by their hypotheses.
- **Several devices of one seed.** A pipe is held by every device of a seed, but a heal is kept by
  one device. If one device healed the pipe, the correspondent would erase the old walk, and the
  seed's other devices would lose the pipe. By decision 416 = (a), iOS receives and opens heals
  and starts none until a seed's devices can carry one heal between them.

### 5.5 A machine check of Lemma 5 (c)

`check_lemma5.py`, in this folder, explores every reachable state of two correspondents that
follow the rules of Network:304-319 and :329-330 *(main)*. The adversary delivers any letter in
flight, in any order and at any time, or drops it. It alters and injects nothing, as Lemma 5
assumes. Windows advance. Each side writes up to two letters of its own, plus every answer it
owes. As in the client, a letter carries one heal part, and an owed answer goes before an offer.
Four encapsulation keys are drawn, and every run is repeated for all 24 byte orders of them. The
checker tests two properties in every state:

- **SAFE.** The walk each side sends under is held by the other side. If this fails, one side
  can no longer read the other. That is the divergence that Lemma 5 (c) rules out.
- **ONE.** No side holds two healed walks from one old walk. Network:330 calls that case a
  defect.

The text leaves three points open, and the checker runs every combination of readings:

- **OFFER.** Strict: a side that has answered a heal, and not yet moved to it, does not offer.
  This is the iOS reading (`holdsOneWalk`, `MTPipeKey.swift:767`). Loose: such a side may offer.
- **STALE.** A side may open an offer of an earlier window while it has an offer of its own
  pending. Compare: it applies the byte-order rule anyway, as the client does
  (`MTPipeKey.swift:808`). Answer: it answers and keeps its own offer. Ignore: it drops the
  offer.
- **LETGO.** Off: the text as written. Ios: the client's rule (`MTPipeKey.swift:802-807`),
  which Network does not state. A side that answered a heal it has not yet moved to lets that
  heal go when the other side offers again. Later: the same rule, but only an offer of a later
  window than the one answered lets the heal go, and an offer of an earlier window is dropped.

| LETGO | Result over the six readings of OFFER and STALE |
|---|---|
| off | SAFE in all six (122 616 to 228 960 states each). ONE fails in all six. |
| ios | **UNSAFE in all six.** |
| later | SAFE in all six (67 704 to 157 128 states). ONE holds for OFFER strict with STALE compare or ignore, and fails otherwise. |

The checker finds faults when they exist. Two mutants each come out UNSAFE: the offering side
opening an answer without checking `ρ`, and the answering side moving to the healed walk without
proof.

**What this confirms.** Under the text as written, Lemma 5 (c) holds for every reading and every
pattern of loss within these bounds. Item 10 of Section 8 is a question of liveness and of ONE,
not of safety.

**The fault in the client.** The shortest run under LETGO ios has five steps:

1. `X` offers `p_1` in window 1.
2. In window 2, `X` replaces `p_1` with `p_2`.
3. `Y` opens `p_2` and answers it, so it folds `h(p_2)`.
4. The letter carrying `p_1` arrives late. `Y` takes it as a new offer and lets `h(p_2)` go.
5. `X` opens the answer to `p_2` and moves to `h(p_2)`, which `Y` no longer holds.

The rule assumes that a side offers again only if it never opened the answer. A late letter can
carry an offer older than the one answered, and then the assumption is false. In the client,
`received` gets the letter's day but does not compare it for an offer (`MTPipe.swift:640`,
`MTPipeKey.swift:799-811`). The fault is latent in build 2324: the client sends no heal part at
all, because `carry` is reached only through `healCarry`, which nothing calls (decision 416 =
(a)). The reading LETGO later is SAFE, and with OFFER strict and STALE compare it also keeps ONE.
That is the client's own reading with the let-go corrected. The Author decided (record 438 →
439 = (b), 2026-10-30): the fault stays a named limit while sending is off, and LETGO later, in
Network and in the client, is a condition of any decision that turns sending a heal on in iOS.

**Bounds of the check.** Two windows, two letters of its own per side, four keys. The adversary
is passive on content. A proof for all bounds is the argument of Lemma 5 (c), and the checker
does not replace it.

---

## 6. What is refused, and on which norm

| Request of the RFC | Answer | Norm or result |
|---|---|---|
| 1. Proof of GPA resistance against `D_idle` | taken in a changed form: the [I-16] game, Theorem 1, Propositions 1-2, C3′ | no `D_idle` exists (no filler); Proposition 1 and [DMMK18] show that GPA resistance is out of reach without overhead |
| 2. Adaptive stochastic padding | refused | [I-16] requirement 3: "Synthetic filler therefore does not exist"; Canon:2530. An even or shaped stream becomes the identifying mark itself. |
| 3a. Hybrid ML-KEM plus ECC per hop | refused | decision 304; [I-1] excludes classical Diffie-Hellman and elliptic curves; [I-7] excludes duplicating one function across two primitives |
| 3b. "Immediate" post-compromise security | refused in that form | nothing heals before fresh secret material the attacker lacks has been exchanged; Theorems 3 and 4 heal at the next renewal or heal |
| 3c. Forward secrecy and healing per hop | taken in a changed form | Theorems 2-4, Lemma 5, on ML-KEM-768 alone |
| 4. NIZK or accumulator resynchronisation of a token queue | refused: the problem does not arise | no counter or index exists (Canon:1383-1384); a returning device walks to the present without asking anyone (Canon:1368-1369); loss is repaired by erasure coding, never by asking again |

Document 06 gives each answer in full, with its citations.

---

## 7. Implementation status (2026-10-30)

| Part | State | Evidence |
|---|---|---|
| Handshake (ML-KEM-768 both ways, ML-DSA-65 over the transcript) | running | `mt-wire/src/handshake.rs:239-252` *(9f72fc0)* |
| The two folds `mt-link-renew` and `mt-pipe-heal`, and their frozen vectors | on `main` | commits `f9281a3` and `1da3c57`; checked by `mt-conformance` |
| Text of re-keying and healing | adopted | PR #121, `605c843` (decision 333 = (b)); crossing rule at Network:310-319 and :330 *(main)* |
| Renewal wire code: kinds 15-17, state machine, `rekey_units`, hard stop at twice the ceiling | merged | PR #125, merge `0f8e300` (2026-10-30); decision 397 → record 399 |
| [I-16] as Claim C3′ | in the Constitution | commit `1e2a294`; Constitution:386-396 *(main)* |
| iOS client: fold, heal by the day, crossing rule, keys, letter fields, wiring into the pipe (slices C1-C2b-2) | in build 2324 (source `d20b2d66`); receives and opens heals | `MTPipeKey.swift:579-589`, `:596-631`; `MTPipe.swift:543` |
| iOS client: sending a heal | **off** by decision 416 = (a) | `healCarry` is defined at `MTPipe.swift:605` and called nowhere |
| iOS client: letting go of an unopened answer when the other side offers again | **unsafe** when an offer arrives late; latent while sending is off | `MTPipeKey.swift:802-807`, `MTPipe.swift:640`; Section 5.5; record 438 → 439 = (b) |
| iOS client: window from the chain | **no**: taken from the wall clock, one window of tolerance either way | `MTPipe.swift`; a finding of issue #22, against [I-16] requirement 14 |
| Routing by tags (holder, path, canonical slot, right of carriage, deferred collection) | **not implemented** | stage 18; today the node deposits every cell on every link (`montana-node/src/lib.rs:1817-1849` *(9f72fc0)*) |
| Measurement of what an observer sees (`ε` of C3′) | **not started** | stage 22; no GPA test network exists (`doc/external-audit/LAB.md:5-10`) |
| Kotlin client | outside this council's work | decision 384 |

Theorems 2-4 and Lemma 5 are about a construction whose derivations are implemented and checked
against vectors, whose link protocol is merged, and whose pipe protocol runs on iOS in the
receiving direction only. Theorem 1 and C3′ are about a routing layer that does not run yet, so
no test today can confirm or refute them.

---

## 8. Limitations and open problems

1. **`ε` for REL and ORIG.** C3′ is a target without a bound. It waits on routing by tags.
2. **Corruption in the transport game.** The game corrupts no machine. A corrupted hop learns one
   step label and an address; the holder of a tag learns the size of one letter; a machine
   serving a tenant learns that it does. These rows of the threat model belong in the leakage
   function once the game admits corruption.
3. **Volume watermarks.** Delaying cells in a pattern produces loss, not a timing mark, because a
   late cell is extinguished. But dropping `k` cells upstream lowers the volume downstream by `k`.
   For an admissible `P`, this matters only together with interior observation, which is the
   setting of Proposition 2.
4. **Multi-round intersection.** Statistical disclosure [Dan03] is outside a single-challenge
   game.
5. **Standard model.** Section 4.5.
6. **Active attackers on renewal and heal.** Staying in the middle, and stripping offers, are
   named in the specification and not closed. Signing renewals (Q2(b)) helps only against an
   attacker that read the link but did not take the machine.
7. **An answered heal that never confirms.** If `T` answers and `S` never moves (Lemma 5 (c),
   case 2), `T` keeps the old walk safely. The text does not say how long `T` keeps the answered
   heal, or whether `T` may offer again while it is kept. This bears on liveness, not on safety,
   and we put it to reviewers.
8. **Recognisability.** Fixed unit and flight sizes are a signature a DPI classifier can learn.
   This is a named limit (record 394). A transport that disguises itself would be a separate
   decision.
9. **Decapsulation failure.** Lemma 5 (b) and Theorem 4 inherit `δ_KEM`. If decapsulation fails,
   the offering side moves to a walk the answering side does not hold. The erasure rule keeps the
   pipe alive, because the answering side never erases, but no heal completes. The probability
   is about `2^-164.8`.
10. **An offer opened after its window.** Lemma 5 covers offers of one window, as the crossing
    rule does (Network:313-314, *main*). The text does not say whether an offer drawn in an
    earlier window and opened late takes part in the byte-order comparison against an offer of
    the current window. If it does, a stale offer can beat a fresh one. The side that opened it
    erases its fresh key and answers an offer its sender may already have replaced, and that
    window's heals are lost. Safety does not depend on the answer. A side sends under a healed
    walk only once it holds that walk, so opening a letter under a healed walk proves that its
    sender holds that walk. The erasure rule (Network:329, *main*) then never leaves a side
    without the walk the other side sends on. Section 5.5 confirms this for every reading within
    its bounds. The same check shows a second gap: under the text as written, a side can answer
    a late offer and a fresh one and so fold two healed walks from one old walk, which Network:330
    calls a defect. The question bears on liveness and on that defect, not on safety. We put it
    to reviewers together with item 7.
11. **The iOS client's let-go rule.** It loses a pipe when an offer arrives late (Section 5.5).
    The fault is latent while the client sends no heal part. The correction LETGO later is
    checked; by decision 439 = (b) on record 438 it is a condition of turning sending on.

**Questions for reviewers** are listed in document 08. Two are specific to this paper. Does the
symmetry argument in the proof of Lemma 5 (d) hold as written? Is the scope condition of
Theorem 1 (reactions after opening) the right place to draw the line?

---

## 9. Reproducing

```
git clone https://github.com/efir369999/montana_messenger
git -C montana_messenger checkout 390441f
cargo test --manifest-path montana_messenger/core/Montana-Core/Cargo.toml \
    -p mt-wire -p mt-derive -p mt-codec -p mt-net -p mt-conformance
```

`python3 -B check_lemma5.py` in this folder repeats the check of Section 5.5 with the Python
standard library alone. It exits with 0 when the results are those of the table there.

`mt-conformance` reads Canon and compares its frozen vectors with what the code computes. The
iOS vectors of the heal, the crossing rule and the heal by the day are checked at app start by
`MTPipeKey.healAgreesWithCanon()` (`MTPipeKey.swift:596-621`). A reviewer should run these
checks again rather than rely on the council's runs.

---

## References

- [BKMMM13] M. Backes, A. Kate, P. Manoharan, S. Meiser, E. Mohammadi. *AnoA: A Framework for
  Analyzing Anonymous Communication Protocols.* IEEE CSF 2013.
- [DMMK18] D. Das, S. Meiser, E. Mohammadi, A. Kate. *Anonymity Trilemma: Strong Anonymity, Low
  Bandwidth Overhead, Low Latency — Choose Two.* IEEE S&P 2018.
- [Dan03] G. Danezis. *Statistical Disclosure Attacks.* IFIP SEC 2003.
- [MD05] S. J. Murdoch, G. Danezis. *Low-Cost Traffic Analysis of Tor.* IEEE S&P 2005.
- [JWJSS13] A. Johnson, C. Wacek, R. Jansen, M. Sherr, P. Syverson. *Users Get Routed: Traffic
  Correlation on Tor by Realistic Adversaries.* ACM CCS 2013.
- [NBH18] M. Nasr, A. Bahramali, A. Houmansadr. *DeepCorr: Strong Flow Correlation Attacks on Tor
  Using Deep Learning.* ACM CCS 2018.
- [BL15] M. Bellare, A. Lysyanskaya. *Symmetric and Dual PRFs from Standard Assumptions: A Generic
  Validation of an HMAC Assumption.* IACR ePrint 2015/1198.
- [BBGS23] M. Backendal, M. Bellare, F. Günther, M. Scarlata. *When Messages are Keys: Is HMAC a
  dual-PRF?* CRYPTO 2023.
- [FIPS203] NIST. *Module-Lattice-Based Key-Encapsulation Mechanism Standard.* FIPS 203, 2024.
- [FIPS204] NIST. *Module-Lattice-Based Digital Signature Standard.* FIPS 204, 2024.
- [RFC8439] Y. Nir, A. Langley. *ChaCha20 and Poly1305 for IETF Protocols.* RFC 8439, 2018.

---

## Appendix A. Numbering against the review package

| This paper | Review package | RFC answer |
|---|---|---|
| Theorem 1 (KIND) | document 03, status line | Theorem 1 |
| Propositions 1, 2; Claim C3′ | document 03, section 5 | Propositions 1, 2; C3′ |
| Theorem 2 (forward secrecy across renewals) | document 04, Theorem 1 | — |
| Theorem 3 (link heals) | document 04, Theorem 2 | — |
| Theorem 4 (pipe heals) | document 05, Theorem 3 | — |
| Lemma 5 (crossing heals) | new; the rule is in document 08 (record 392) | — |
