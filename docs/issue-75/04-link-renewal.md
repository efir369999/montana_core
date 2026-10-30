# 04. Re-keying a link: protocol and proofs

**Status: adopted, not implemented (Network:193-316 at `9f72fc0`; marked adopted on `main` at
`605c843`, PR #121, decision 333 = (b)).** The two folds and their frozen vectors are on `main`
(document 07). The state machine that runs a renewal on a live link is not on `main`.

## 1. The problem

Today a link keeps the two directional keys of its handshake for its whole life
(`mt-net/src/lib.rs:243-271`): `send` and `receive` read them from the channel at every unit, and
nothing replaces them. Whoever reads those keys out of a machine at any moment reads every unit
of that link, past and future, until the link ends.

## 2. The protocol

Notation as in document 02. The link secret starts at the handshake's master, and epoch `n` has
secret `ls_n`:

```
ls_0      = master                                                    Canon:3089
ls_n      = H_{mt-link-renew}( ls_{n-1} || ss_i || ss_r || pk_i || pk_r || ct_i || ct_r )
k_i2r(n)  = H_{mt-noise-pq-v1-i2r}(ls_n)       k_r2i(n) = H_{mt-noise-pq-v1-r2i}(ls_n)
```

The field widths are 32, 32, 32, 1 184, 1 184, 1 088 and 1 088 bytes. `pk_i` and `pk_r` are the
fresh ML-KEM-768 encapsulation keys drawn for this renewal by the initiator and by the responder
of the *handshake*. `ct_i` is what the initiator encapsulated to `pk_r`, and `ct_r` what the
responder encapsulated to `pk_i`. `ss_i` and `ss_r` are the corresponding shared secrets. Whichever
side offers, the fold is written in handshake roles, so both sides compute one value. At `n = 0`
the keys are exactly the handshake's keys, so a link that never renews is the link of today.

**Messages.** All three are ordinary link messages, sealed under the keys of the current epoch,
with their kind inside the seal:

| Kind | Message | Body | Length | Units |
|---|---|---|---|---|
| 15 | `renew_offer` | `pk_o`, drawn for this renewal alone | 1 184 B | 2 |
| 16 | `renew_answer` | `ct_a` = Encaps(`pk_o`), then `pk_a`, drawn for this renewal alone | 2 272 B | 2 |
| 17 | `renew_finish` | `ct_o` = Encaps(`pk_a`) | 1 088 B | 1 |

No window, counter or index of a renewal crosses the wire.

**Steps.**

1. **Offer.** A side about to send its first message in a window later than the window in which
   `ls_n` was set sends `renew_offer` first, then its message. It also offers when the units it
   has sealed under its current sending key reach `rekey_units`. A link that carries nothing in a
   window renews nothing.
2. **Answer.** The other side encapsulates to `pk_o`, draws `(pk_a, sk_a)` and sends
   `renew_answer`.
3. **Finish.** The offerer decapsulates `ct_a`, encapsulates to `pk_a` and sends `renew_finish`
   under the *old* keys. Every later unit it seals under `k(n+1)`.
4. **Switch.** The answerer decapsulates `ct_o`, computes `ls_{n+1}`, and seals every later unit
   under `k(n+1)`.
5. **Erase.** Each side keeps its old *receiving* key only until the first unit opens under the
   new one. It then erases that key, `ls_n`, the decapsulation key it drew, and both shared
   secrets.
6. **Crossing offers.** If both sides offer in one window, the offer of the handshake initiator
   stands, and the other side erases its key and answers. The window renews a link at most once
   per window; in a window that stalls, the ceiling renews it again each time a side's count
   reaches `rekey_units`.
7. **Failure.** A unit that opens under neither key ends the link in silence, and a new handshake
   replaces it. So a renewal that fails to complete costs one handshake, and the two ends never
   go on holding different secrets.

**The ceiling.** `rekey_units = 2^23`. A commodity link of [I-5] is symmetric gigabit, and in a
nominal sixty-second window it carries `10^9 · 60 / 8 / 1 232 ≈ 6 087 662` units, which lies
between `2^22` and `2^23`. So in a chain that advances, the window renews the link first, and the
ceiling binds only when the chain stalls or the link is faster. With 96-bit random nonces, the
chance of a nonce repeating under one key after `q` seals is at most `q^2 / 2^97`, which is
`2^-51` at the ceiling. The ceiling is where a side *offers*, not where its old key stops: the
count is checked before a message, and the offering side keeps sealing under its old key until it
reads the answer, so `q` can pass `rekey_units`. Document 08 ("Gaps") states by how much, and
that the node does not bound it today.

**Cost.** One renewal is 5 units (6 160 bytes) per window in which the link carries anything:
2 key generations, 2 encapsulations, 2 decapsulations and 1 hash. It carries no signature. A new
handshake would be 14 units with 2 signatures (`75-RFC-GPA.md:134-146`).

## 3. Security

The adversary sees every unit of the link. With the link's keys it can open them and read
`pk`/`ct` of every renewal. `Corrupt(P, t)` is as in document 02. `q_H` is the number of
random-oracle queries.

### Theorem 1 (forward secrecy across renewals)

Let renewal `n` be complete at side `P`, and let `P` have erased its old receiving key, before
`Corrupt(P, t)`. Then for every epoch `m < n`, the units sealed under `k(m)` keep the security
they had for an adversary that never corrupted `P`. The extra advantage the corruption gives is at
most `(n + 1) · q_H / 2^256`.

*Proof.* In the random-oracle model, `ls_n` is the output of `H_{mt-link-renew}` on an input that
contains `ls_{n-1}`. `Corrupt` returns `ls_n` and the material of epoch `n`, and by (A4) nothing of
epoch `m < n`: those values were erased at step 5. Each `ls_m` is an oracle output on a fresh
input, and each `k(m)` is an oracle output on `ls_m`. So the adversary's view of `ls_m` and
`k(m)` changes only if it queries the oracle on an input that contains `ls_m`. Without such a
query the view is independent of `ls_m`: the outputs it holds, `ls_n` among them, are independent
uniform values. Each `ls_m` is uniform over `2^256` values in that view, so `q_H` queries hit it
with probability at most `q_H / 2^256`. A union bound over `m = 0..n` gives the stated term. If
the old receiving key has not yet been erased, the bound holds for `m < n - 1`, and epoch `n - 1`
is exposed in that one direction. ∎

The handshake itself provides the security of `ls_0` against a passive adversary: ML-KEM in both
directions, folded with the transcript (Canon:3089). Theorem 1 says that a later corruption adds
nothing to it.

### Theorem 2 (healing after a passive renewal)

Let `t` be the last corruption of either side. Let renewal `n + 1` begin after `t`, so both
renewal key pairs are drawn after `t`, and let there be no corruption of either side until that
renewal's erasures are done. Suppose the adversary does not modify, inject or reorder the units
of renewal `n + 1`, and that it injects no renewal message of its own under `k(n)` before that
renewal completes; dropping units is allowed, and it only ends the link. Then `ls_{n+1}`, and with
it the keys of epoch `n + 1` and of every later epoch up to the next corruption, are
indistinguishable from uniform. For every adversary `A` there is an adversary `B` such that, for
the keys of epoch `n + 1` and of the `e` epochs after it,

```
Adv(A) ≤ Adv^{cpa}_{ML-KEM-768}(B) + (e + 1) · q_H / 2^256
```

and `B` runs in about the time of `A` plus one simulation of the link.

*Proof.* Hybrid `G_0` is the real game. In hybrid `G_1`, the shared secret of the encapsulation
`ct_o = Encaps(pk_a)` is replaced by an independent uniform `ss*`. Whichever role the offerer has,
this secret is one of `ss_i` and `ss_r`.

*`G_0` to `G_1`.* `B` receives an IND-CPA challenge `(pk*, ct*, K*)`. It simulates both honest
sides, generates every other value itself (including `ls_n`, which it hands over at `t`), and
embeds `pk_a := pk*` and `ct_o := ct*`. It uses `K*` as that shared secret wherever the protocol
needs it. The answerer would normally get this secret by decapsulating `ct_o`, and it needs `sk_a`
for nothing else. Because no corruption falls inside the renewal, `B` never has to reveal `sk_a`
or the shared secret. If `K*` is real, `B` simulates `G_0`; if `K*` is random, it simulates `G_1`.
Because the adversary is passive on the renewal, `ct_o` is exactly the honest encapsulation, so
IND-CPA suffices and no decapsulation oracle is needed.

*In `G_1`.* `ls_{n+1} = H_{mt-link-renew}(… || ss* || …)` contains the uniform 32-byte `ss*`,
which the adversary never sees. As in Theorem 1, the output is independent of the view unless the
adversary queries the oracle on that exact input, which happens with probability at most
`q_H / 2^256`. The secret of each later epoch is an oracle output on an input that holds the
secret before it, which the view does not fix, so each of the `e` later secrets adds at most
`q_H / 2^256` by the same argument. A union bound over the `e + 1` secrets gives the second
term. ∎

*Remark: one fresh side is enough.* The proof uses only `pk_a`. Embedding `pk_o` and `ct_a`
instead gives the same bound. So the theorem still holds if only one of the two key pairs was
drawn after the last corruption and erased before the next. This is the property Canon states for
the handshake: "a channel is secure if either encapsulation was" (Canon:3111).

*Remark: what an active attacker gets.* An attacker that holds `ls_n` can seal units under
`k(n)`, so it can offer a key of its own and stay in the middle of renewal `n + 1` and of every
renewal after it. No unauthenticated renewal can exclude it, since authentication from the link's
own keys is exactly what it holds. It must re-seal every unit in both directions for as long as
the link lives. If it stops once, a unit fails to open, the link ends, and the next handshake is
signed by both machines' answering keys and heals the link. An attacker without the link's keys
cannot inject anything: the renewal travels under ChaCha20-Poly1305 (A3). Signing the renewal with
the answering keys (option Q2(b), 11 units) would also exclude an attacker that read the link but
not the machine. It would not help against an attacker that took the machine, because the
answering key is derived from the machine secret (`mt-net/src/lib.rs:194-201`). The draft does
not sign. Q2 is open (document 08).

*Remark: implicit rejection.* ML-KEM decapsulation never fails: a tampered ciphertext gives a
pseudorandom shared secret. A tampered renewal therefore leaves the two sides with different
`ls_{n+1}`. The first unit under the new key fails to open, and step 7 ends the link. No state
divergence outlives the link.

### Remark on the standard model

The proofs model SHA-256 as a random oracle (A2). A standard-model proof of Theorem 2 needs
`H_{mt-link-renew}` to be a PRF when keyed by `ss*`, which sits in the middle of the input with
`ls_n` known to the adversary. That is a dual-PRF assumption on plain SHA-256 used as a
Merkle-Damgård hash, and it is not HMAC or HKDF. Analyses of TLS 1.3 and HKDF make similar
assumptions on HMAC (Bellare and Lysyanskaya, *Symmetric and Dual PRFs from Standard
Assumptions*, 2015; Backendal, Bellare, Günther and Scarlata, *When Messages are Keys: Is HMAC a
dual-PRF?*, CRYPTO 2023). Montana uses domain-separated plain SHA-256 throughout. The outputs are
used only as keys, or published truncated (tags), and never as MACs over attacker-chosen
suffixes, so length extension gives an attacker nothing. **Whether plain SHA-256 is an acceptable
KDF here, or whether the fold should be HKDF/HMAC, is one of our questions for reviewers.**

## 4. What this does not do

- It gives no forward secrecy *inside* an epoch. Every unit of epoch `n` falls to a corruption
  in epoch `n`. A side offers to end its epoch at its first message in a later window or once it
  has sealed `rekey_units` units under its key, whichever comes first, and the epoch ends when
  that renewal completes.
- It does not touch pipes. The inner seal of a cell is under the pipe key, and no hop holds it.
  Document 05 covers pipes.
- It does not touch the step label. The step label is derived from the handshake secret
  (Canon:1348), and the draft leaves it there. The code currently draws a random label for every
  unit (`mt-net/src/lib.rs:266`). That divergence belongs to issue #22 (Q5).
- It adds an observable: an observer of a line sees that the link carried something in a window,
  and with the proposal also a fixed burst of renewal units. The ledger row proposed at
  Network:303-308 names it.
