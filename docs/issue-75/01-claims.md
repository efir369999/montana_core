# 01. What Montana claims, and what it does not

This document comes first because the rest of the package should be judged against it. Each
claim is labelled with its standing:

- **in force**: normative text of the specification, and implemented;
- **specified**: normative text, but not yet implemented, or implemented only in part;
- **proposed**: text still marked *proposed (issue #75)*. Since PR #121 (`605c843`) no claim
  below has this standing: the re-keying section is adopted (decision 333 = (b)) and is now
  **specified**;
- **not claimed**: Montana makes no such claim, and the specification says so.

## Claims

| # | Claim | Standing | Where |
|---|---|---|---|
| C1 | The content of every link unit is confidential and authenticated under a per-direction key from a post-quantum handshake: ML-KEM-768 encapsulation in both directions, ML-DSA-65 signatures over the transcript, SHA-256 key derivation, ChaCha20-Poly1305 sealing, no elliptic curve. | in force | Canon:3076-3095; `mt-net/src/lib.rs:262-320`; `mt-wire/src/handshake.rs:239-252` |
| C2 | A pipe (the end-to-end secret of two correspondents) has forward secrecy at window granularity: its secret walks one way, one hash per window, and earlier values are erased. | specified (derivation and vectors in code; routing that uses it is stage 18) | Canon:1354-1377; `mt-derive/src/tag.rs:103` |
| C3 | **Transport indistinguishability [I-16]**, restated as Claim C3′ by decision 436 = (a). An observer of the wire at any set of points *that does not hold both ends of the same delivery* cannot tell what an object is; its advantage on who originated it, for whom it is destined, or whether a given machine is its origin or a relay is bounded by the honest volume, the delay profile and the path length. | KIND **proved** (`75-RFC-GPA.md`, Theorem 1); the bound for origin, destination and relay **not established, not implemented** | Constitution:386-398; Network:787-832; `core/Montana-Core/protocol-13/11-ANONYMITY.md:3` |
| C4 | A link re-keys once in every window it carries anything, plus at a unit ceiling. A key read at one moment opens nothing sealed before the previous renewal (forward secrecy), and nothing sealed after the next renewal, if that renewal was not tampered with (healing). | specified (adopted at `605c843`; code at stage 18, after #24) | Network:193-316; documents 04 here |
| C5 | A pipe heals in band, by an ML-KEM-768 offer and an answer carried inside two ordinary letters. | specified (adopted at `605c843`; no client implements it) | Network:280-291; document 05 here |

C3 is the claim people usually mean by "anonymity", and it is the weakest of the five today.
Document 03 states it as a game. Document 07 shows that the routing it depends on does not run yet.

## Not claimed

| # | Not claimed | Why, in the specification's own words |
|---|---|---|
| N1 | **Resistance to a global passive adversary**, or to any observer holding both ends of one delivery. | "An observer holding both ends is the one case the invariant does not close" (Constitution:391-394); "a named physical prohibition: delivery moves bytes at both ends, and no construction supplies volume nobody carries" (Network:820). Document 03, Proposition 1, shows why. |
| N2 | **Cover traffic or padding of any kind**: constant-rate, Poisson, adaptive or differentially private. | "A constant rate would be the worst solution ... Synthetic filler therefore does not exist" (Constitution:409-414; Network:640-646). "A unit carries a message or a piece of one and never synthetic filler" (Canon:2530). What a line carries is the user's own traffic plus other people's transit. |
| N3 | **That superimposed flows cannot be separated.** | Mixing other people's traffic in "does **not** make the summands inseparable ... The claim that the summands are inseparable is stronger than the truth and is not made here" (Constitution:416-426). |
| N4 | **Hiding the timing and volume of a device that carries nothing for others.** | For a device that cannot be reached from outside (behind NAT, asleep), "the fact that a source is here, and the moments and volumes of its own activity" are exposed to the owner of its line and to the neighbouring hop. Content, recipient, path and correspondent addresses stay closed (Constitution:530-534). Most phones are in this case. |
| N5 | **That Montana traffic cannot be recognised as Montana.** | Every unit on the wire is 1 232 bytes (Canon:2152), and the handshake flights have fixed lengths (3 138, 4 226, 4 397 and 3 309 bytes; Canon:2267-2283, checked by `mt-wire/src/handshake.rs:468-473`). A DPI classifier can learn this signature. The specification hands censorship circumvention to external tunnels, which "are **not** a source of privacy" (Network:781-785). Whether Montana should gain a pluggable transport that disguises itself is an open question with the Author (document 08). |
| N6 | **Immediate post-compromise security.** | Nothing can heal before fresh secret material that the attacker does not hold has been exchanged. The proposal heals at the next renewal, and only against an attacker who stays passive during it (document 04, Theorem 2 and its remark). |
| N7 | **A hybrid post-quantum/classical key exchange.** | Refused by the Author's decision 304. Invariant [I-1] excludes classical Diffie-Hellman and elliptic curves, and [I-7] excludes duplicating one function across two primitives (Constitution:77-81, 183-185). |
| N8 | **"Absolute" or "total" anonymity as an achieved fact.** | "Unsupported absolute anonymity claims must not be published as achieved facts" (`core/Montana-Core/protocol-13/11-ANONYMITY.md:21-22`). |

## A correction of an earlier public proposal

On 2026-10-21 an issue was opened from the project's account on TokTok/c-toxcore
(`TokTok/c-toxcore#3112`, *"Proposal: Introducing timing-attack obfuscation and metadata
decoupling layer inspired by Montana Protocol"*). It described Montana as using:

1. "a strict **Constant-Rate Padding** framework", and
2. "**Poisson Delay Routing Injection**: introducing artificial mathematical noise and
   pseudo-random delay intervals",

with the goal of "absolute metadata anonymity".

**That description is wrong on all three counts**, and this package replaces it.

- Montana forbids constant-rate padding and synthetic filler (N2 above). The fixed *size* of a
  unit is real. A fixed *rate* is not, and the specification calls a fixed rate "the worst
  solution" because an even round-the-clock stream is itself an identifying mark.
- Montana injects no noise. Its only random delay applies to mail for a sleeping device. That
  delay is drawn **uniformly** over the remainder of the current window, not from a Poisson
  profile (Network:562-568; Canon:1328-1342 explains why a shaped profile would leak more), and it
  is never applied to live traffic.
- Montana does not claim absolute anonymity (N8), and it does not claim GPA resistance (N1).

The maintainer's reply (Green-Sky, 2026-10-21) objected that constant-rate padding "is
prohibitively expensive for a system as dynamic as Tox". **That objection is correct.** It is one
of the two reasons Montana's own specification forbids such padding; the other is that an even
stream becomes the mark itself. The reply also said that Montana "does not really seem to even
exist". The specification and the core are public at the commit in the README, but the anonymous
routing layer is not implemented (document 07), so the remark was fair about the part the
proposal advertised.
