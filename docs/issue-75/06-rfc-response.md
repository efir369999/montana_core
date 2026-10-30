# 06. The RFC of issue #75, requirement by requirement

Each requirement is **taken**, **taken in a changed form**, or **refused**. A refusal names the
norm it rests on, and a requirement that is taken names the text and the proof that cover it.
The council's longer working answer is `core/docs/protocol-13/75-RFC-GPA.md`.

## Requirement 1: a formal proof of GPA resistance

*Asked:* a proof, for example in UC, that active routing is indistinguishable from an idle
background-noise distribution `D_idle`, under a rushing adversary that delays or drops packets.

**Taken in a changed form.**

- The `D_idle` game cannot be stated, because Montana has no synthetic traffic and so no idle
  distribution (Constitution:409-414).
- GPA resistance is not claimed (document 01, N1). Proposition 1 of document 03 shows that this
  design does not have it. That proposition is a small instance of a general result, the
  anonymity trilemma of Das, Meiser, Mohammadi and Kate (2018): without bandwidth overhead or
  latency overhead, strong anonymity is out of reach.
- What is taken is the [I-16] game: two worlds of real traffic, an adversary that observes,
  delays, drops and injects at any admissible set of points, and an explicit leakage function
  (document 03, sections 2-4).
- Delay and drop are handled by construction: a late cell is extinguished, and loss is repaired
  by the erasure code without retransmission (Constitution:563-568; Network:475-481).
- **The finding:** Proposition 2 of document 03 shows that the invariant's literal quantifier is
  too strong when the adversary chooses the workload. The Author adopted Claim C3′ in its place
  (decision 436 = (a), 2026-10-30). The KIND case is proved (`75-RFC-GPA.md`, Theorem 1).
- **Still open:** a bound on `ε` in C3′, by analysis or by the measurement of stage 22. Both wait
  on routing by tags (stage 18).

## Requirement 2: adaptive, stochastic traffic padding

*Asked:* Poisson or differentially private padding that adapts to congestion and to battery.

**Refused.** It contradicts [I-16] requirement 3: "Synthetic filler therefore does not exist, and
neither does a price for it" (Constitution:412-414), and "a unit carries a message or a piece of
one and never synthetic filler" (Canon:2530). Any padding schedule is filler, adaptive or not.
The Constitution gives the reason: an even or shaped stream becomes the identifying mark itself.

What Montana does instead:

- other people's transit fills a reachable device's line, by the duty that pays for carrying it
  (Network:622-638);
- the only random delay is uniform over the rest of the window, and only for mail to a sleeping
  device (Network:562-568; Canon:1328-1342);
- a constant rate exists only for a live voice or video stream (Network:619).

The price of this refusal is named in document 01 (N3 and N4). The padding frame of the legacy
client core is defined but never sent (document 07), and it goes with the removal of that core.

## Requirement 3: post-quantum per-hop handshake, forward secrecy, post-compromise security

*Asked:* a hybrid of ML-KEM over classical ECC per hop, and "immediate post-compromise security
at every discrete hop".

- **The hybrid is refused.** By the Author's decision 304, the handshake stays pure ML-KEM-768.
  [I-1] forbids classical Diffie-Hellman and elliptic curves, and [I-7] forbids duplicating one
  function through two primitives (Constitution:77-81, 183-185). The handshake already has no
  elliptic curve.
- **"Immediate" PCS is refused in that form.** Nothing heals before fresh secret material the
  attacker does not hold has been exchanged after the compromise.
- **Forward secrecy and healing per hop are taken in a changed form.** A link re-keys once per
  window in which it carries anything, and at a unit ceiling (document 04, Theorems 1 and 2). A
  pipe heals in band (document 05, Theorem 3). Both run on ML-KEM-768 alone, and their derivations
  and vectors are on `main` (document 07).

## Requirement 4: resynchronising a token queue with NIZKs or accumulators

*Asked:* zero-knowledge recovery for a single-use token chain that packet loss desynchronises.

**Refused: the problem does not arise in this design.**

- There is no token chain and no counter. A tag is `H_{mt-tag}(pipe_secret(W) || W)[0..16]`
  (Canon:1347), and "Neither takes an index of any kind" (Canon:1383-1384).
- The walk needs no exchange to resynchronise: "a device returning after an absence walks to the
  present rather than asking anybody" (Canon:1368-1369).
- Loss is repaired by the erasure code, never by asking again (Network:475-481). A false route
  costs a wait, and "the sender's queue keeps the letter" (Network:153).
- The only shared state that could diverge is the window height. [I-16] requirement 14 takes it
  from the chain (Constitution:555-562). Re-keying links keeps no shared counter for the same
  reason (Network:240-245).
- [I-7] admits a new primitive only to close a concrete mechanism (Constitution:183-185), and
  there is no mechanism here to close.

The one real divergence, the iOS client's wall-clock window, is a defect tracked in issue #22
(document 07).

## The RFC's action items

1. **A formal whitepaper.** It exists: `WHITEPAPER.md`, governed by the normative set. Token
   generation is frozen with vectors (Canon:1344-1389). This package adds the formal treatment of
   issue #75.
2. **A pluggable C/Rust library.** There are two C boundaries: the legacy client core's
   `core/Code/crates/mt-bindings` (`staticlib`, `cdylib`, `rlib`; `Cargo.toml:11`) and the new
   core's `core/Montana-Core/crates/mt-boundary`. The RFC is right that the iOS app still computes
   delivery derivations in Swift (`MTPipe.swift`). Moving them behind the boundary is stage 21
   (`core/Montana-Core/ROADMAP.md:1207`).
3. **A verifiable test suite.** The conformance gate compares the specification with the code on
   every build (`core/Montana-Core/crates/mt-conformance/build.rs`). An observer simulation is
   stage 22 (`ROADMAP.md:1217-1221`). It has not started, and no GPA testnet exists
   (`doc/external-audit/LAB.md:5-10`).
