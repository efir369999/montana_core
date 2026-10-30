# 03. The transport game of invariant [I-16]

**Status: the KIND case is proved; the rest is bounded by Claim C3′, now the wording of [I-16].**
Theorem 1 of `core/docs/protocol-13/75-RFC-GPA.md` proves that an observer cannot tell a payment
from a letter, for any set of points. Section 5 shows that the old wording claimed more than any
design of this kind can deliver when the adversary chooses the workload. The Author therefore
restated the second sentence of [I-16] as Claim C3′ (decision 436 = (a), 2026-10-30, Constitution:387-392).
The bound `ε` of C3′ for REL and ORIG cannot be measured yet, because routing by tags is not
implemented (document 07).

## 1. What the invariant says

> For any object the network carries ... an observer of the wire at one point, and at any set of
> points that does not hold both ends of the same delivery, cannot determine what the object is,
> who originated it, for whom it is destined, or whether a given machine is its origin or a relay.
> An observer holding both ends is the one case the invariant does not close.
> (Constitution:386-394)

The RFC asked for a game in which the routing state is indistinguishable from an idle
background-noise distribution `D_idle`. Montana cannot state that game: it has no synthetic
traffic (N2 in document 01), so no `D_idle` exists. The game below compares two **worlds of real
traffic** instead. It follows the AnoA framework of Backes, Kate, Manoharan, Meiser and Mohammadi
(CSF 2013), in which an adversary picks two scenarios that differ only in the property being
protected.

## 2. The game `G^{X}_{Π}(A, P)`

The parameters are a protocol `Π`, a challenge type `X ∈ {REL, KIND, ORIG}` (section 3), and a set
`P` of observation points, each of them a link or an access line.

1. **Setup.** The challenger brings up a network of `N` machines running `Π` with a chain that
   advances, and honest devices attached to it.
2. **Background.** Traffic not chosen by the adversary is drawn from a distribution `D_bg`: the
   letters, payments, streams and consensus objects of honest parties. The two variants of the
   game differ here:
   - **`G_adv`**: the adversary chooses the whole workload, background included. This is the
     reading the invariant's wording invites.
   - **`G_bg`**: the background comes from a fixed `D_bg`, and the adversary chooses only the
     challenge deliveries.
3. **Challenge.** The adversary submits two scenarios `σ_0` and `σ_1` of type `X`. The challenger
   draws `b` uniformly and runs `Π` on the background plus `σ_b`.
4. **Observation and action.** On every point in `P` the adversary sees every unit: its bytes,
   its moment at the resolution of the canonical slot, and the direction it crosses. It may also
   delay, drop, reorder or inject units there. It corrupts no machine; Section 6 covers that
   extension.
5. **Output.** The adversary outputs `b'`. Its advantage is `Adv = |2·Pr[b' = b] − 1|`.

**Admissibility.** `P` is admissible for `(σ_0, σ_1)` when no delivery of either scenario has
both of its ends in `P`, that is `ends(d) ⊄ P` for every `d ∈ σ_0 ∪ σ_1`. The invariant's
exclusion is exactly this condition.

## 3. Challenge types

In every type, both scenarios give every honest device the same number of letters sent and
received, of the same length classes, in the same windows. So the volume of a device's own
activity is the same in both worlds by construction. The game asks what a fixed volume still
gives away.

- **REL (who with whom).** `σ_0 = {u_0 → v_0, u_1 → v_1}` and `σ_1 = {u_0 → v_1, u_1 → v_0}`.
  This is relationship anonymity. Admissibility then forces `P` to contain no sender line or no
  recipient line, because every sender is paired with every recipient in one of the two worlds.
- **KIND (what).** `σ_0` and `σ_1` hold one delivery with the same ends and the same length,
  a payment in one world and a letter in the other. This is [I-16] requirement 1.
- **ORIG (origin or relay).** A machine `m` originates a delivery in `σ_0` and relays a delivery
  of the same length originated elsewhere in `σ_1`. The origin, the destination and the relay
  are arranged so that admissibility holds.

## 4. The leakage function

The observability ledger (Network:805-832) lists quantities that are *deliberately observable*
or that it *names as limits*. For an observer of wire points, those are:

| # | Leaked to an observer of the line | Source |
|---|---|---|
| L1 | the count of units per canonical slot on every observed line; the volume "is not made even" | Network:793 |
| L2 | for a device that carries nothing for others: the moments and volumes of its own activity | Constitution:530-534 |
| L3 | that a live stream runs, at its constant rate | Network:619, :819 |
| L4 | that a link carried anything in a window; if the proposal is adopted, that it renewed in that window | Network:802, :303-308 |
| L5 | that a machine answers at an address | Network:824 |
| L6 | that some device is joining, to the machine it asks | Network:796 |

The challenge types of section 3 equalise L1-L6 for the honest devices *as functions of the
workload*. Anything else an admissible adversary learns is a violation of [I-16].

## 5. What can and cannot be claimed

**Proposition 1 (both ends; the excluded case).** Suppose deliveries complete within a horizon
of `h` windows and no unit is ever sent unless it carries a piece of a real message or real
transit. In `G_adv` there is an adversary with `P = ends(d)` and advantage 1 against the
scenarios `σ_0 = {u → v}` and `σ_1 = {u → v'}`, where the workload is otherwise empty.

*Proof.* In `σ_1` no letter is addressed to `v`. With no other workload `v` carries no transit,
and there is no filler (Constitution:412-414). In `σ_0` the delivery brings at least one unit
onto `line(v)` within `h` windows. The adversary outputs 0 exactly when it sees a letter's units
on `line(v)` within the horizon. Consensus objects also travel on `line(v)` ([I-16] requirement
16), but their volume does not depend on `b`. The adversary subtracts what it expects of them;
in the clean model it knows them exactly, so the advantage is 1. ∎

This is a small instance of the anonymity trilemma of Das, Meiser, Mohammadi and Kate (IEEE S&P
2018): strong anonymity needs either bandwidth overhead or latency overhead. Montana adds latency
only to mail for a sleeping device, bounded by one window (Network:562-568), and it adds no
bandwidth beyond real traffic. That is why the specification names the both-ends case as a
physical prohibition (Network:820) and does not claim to close it. Proposition 1 justifies
refusing the RFC's requirement for global passive adversary resistance.

**Proposition 2 (interior observer, adversary-chosen workload; finding).** Take `G_adv` with
challenge type REL, and let `P` be every link of the network except the access lines of `v_0`
and `v_1`. `P` is admissible, because it holds no recipient line. Let the workload be otherwise
empty, apart from consensus traffic that does not depend on `b`. Let `v_0` and `v_1` reach
different first machines, and let the adversary know which machine each reaches. In `G_adv` it can
learn this before the challenge from letters of its own workload, since it chooses that workload.
Under the routing of Network (holder by tag, collection by the recipient), an adversary can then
win with advantage close to 1.

*Sketch.* The units of `u_0` are re-sealed at every step, so no two observations match bit for
bit ([I-16] requirement 10). Their *volume* still moves hop by hop across links in `P`, from
`line(u_0)` to the holder `H` of the pipe's tag. With nothing else in flight, `H` is identified
as the machine where `u_0`'s volume ends. The recipient's collection is a second flow, from the
recipient's first machine toward `H`, also across links in `P`. The adversary pairs each
collection flow with the holder it reaches, and so pairs `u_0` with the first machine of `v_0` or
of `v_1`. Slot batching removes fine timing but not volume. Nothing hides a flow when there are
no other flows. If `v_0` and `v_1` shared a first machine, this pairing would fail, which is why the
adversary chooses recipients that do not. ∎

**Consequence.** Read literally ("at any set of points that does not hold both ends"), [I-16]
cannot hold with negligible advantage in `G_adv`. Proposition 2 is a short way to see this, and
it follows the same logic as the trilemma. The invariant's own text points the same way: what
the sum with others' traffic gives is "the price of the correlation and not its impossibility"
(Constitution:432), and "a flow that is the sum of one summand equals that summand"
(Constitution:516-518). We read the invariant as the following statement, and we ask reviewers
whether it is the right one:

> **Claim C3′ (the form the council proposes to prove or measure).** In `G_bg`, for every PPT
> adversary and every admissible `P`, `Adv ≤ ε(D_bg, profile, ℓ) + negl(λ)`, where `ε` depends on
> the volume of honest background on the links of `P`, on the delay profile, and on the path
> length `ℓ`, and goes to 0 as the background grows.

Proving C3′ means bounding `ε`. Two routes are open, and both wait on code that routes by tags:
an analytical bound in the style of the mix-network literature, or the measurement owed by stage
22 of the roadmap, "what an observer sees" (`core/Montana-Core/ROADMAP.md:1217`). Long-term
intersection attacks (statistical disclosure, Danezis 2003) are outside the single-challenge game
and need a multi-round variant.

**Adopted.** The Author decided to restate [I-16] as C3′ (decision 436 = (a), 2026-10-30, council
link 13395; document 08). In Montana only the Author amends the Constitution. The section 1 quote
above is the wording before that decision.

## 6. Extensions not yet formalised

- **Corrupted machines.** A neighbouring hop learns one step label and an address. The holder of
  a tag learns the size of one letter. A machine serving a tenant learns that it serves that
  tenant. These rows of the threat model (Network:791-803) belong in the leakage function once
  the game admits corruption.
- **Active dropping as a watermark.** A late cell is extinguished ([I-16] requirement 15) and
  loss is repaired by erasure coding without retransmission. So delaying cells in a pattern gives
  loss, not a timing watermark. But dropping `k` cells upstream lowers the volume downstream by
  `k`, which is a volume watermark. In an admissible `P` this matters only together with
  interior observation, which is Proposition 2's setting.

## References

- M. Backes, A. Kate, P. Manoharan, S. Meiser, E. Mohammadi. *AnoA: A Framework for Analyzing
  Anonymous Communication Protocols.* IEEE CSF 2013.
- D. Das, S. Meiser, E. Mohammadi, A. Kate. *Anonymity Trilemma: Strong Anonymity, Low Bandwidth
  Overhead, Low Latency — Choose Two.* IEEE S&P 2018.
- G. Danezis. *Statistical Disclosure Attacks.* IFIP SEC 2003.
- S. J. Murdoch, G. Danezis. *Low-Cost Traffic Analysis of Tor.* IEEE S&P 2005.
- A. Johnson, C. Wacek, R. Jansen, M. Sherr, P. Syverson. *Users Get Routed: Traffic Correlation
  on Tor by Realistic Adversaries.* ACM CCS 2013.
- M. Nasr, A. Bahramali, A. Houmansadr. *DeepCorr: Strong Flow Correlation Attacks on Tor Using
  Deep Learning.* ACM CCS 2018.
