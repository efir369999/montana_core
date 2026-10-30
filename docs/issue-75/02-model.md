# 02. System model, notation and primitives

## Objects

**Window.** The chain advances in windows. `W` always means the height of the last cemented
window, read from the chain and never from a clock ([I-15], [I-16] requirement 14;
Constitution:555-562). A window lasts nominally sixty seconds (`window_credit` = 60, "one for
every second of it", Canon:4792), but no protocol rule reads seconds.

**Machine, device.** A participant that runs the protocol. Two acquainted machines hold a
**link**.

**Link.** A channel between two neighbouring machines, opened by the four-flight post-quantum
handshake (Canon:3076-3095). The side that opened the connection is the **initiator** `i` and the
other is the **responder** `r`. The handshake produces `master` and two directional keys:

```
k_i2r = H_{mt-noise-pq-v1-i2r}(master)        k_r2i = H_{mt-noise-pq-v1-r2i}(master)
```

**Unit.** Everything a link carries travels in units of `cell_bytes` = 1 232 bytes (Canon:2152): a
16-byte label, a 12-byte nonce drawn afresh for each unit, and a ChaCha20-Poly1305 seal under the
sender's directional key, with the label as associated data (Canon:2453-2458). A message of the link is cut into pieces of 1 183 bytes,
so it takes `ceil(len / 1 183)` units (Canon:2464-2467). Its kind (one byte, values 1-14 today and
15-17 proposed) travels inside the seal.

**Pipe.** The end-to-end relation between two correspondents. They share a secret, and it walks
one way, once per window:

```
pipe_secret(W0)    = shared_secret
pipe_secret(W + 1) = H_{mt-pipe-ratchet}(pipe_secret(W))              Canon:1358-1361
tag(W)             = H_{mt-tag}(pipe_secret(W) || W_8B_LE)[0..16]      Canon:1347
```

The tag of a window decides which machine holds the letters of that window (Network, "Which
machine holds a tag"). The cell of a letter is sealed twice: an outer layer per hop under the link
key, and an inner layer under the pipe key of the window, which no hop holds.

**Delivery, and its ends.** A delivery `d` carries one letter from an origin device to a
destination device across at least the minimum number of distinct machines that Canon fixes.
`ends(d) = {line(origin(d)), line(destination(d))}`, where `line(x)` is the access line of device
`x`: the link or links by which it reaches its first machine. [I-16] protects every set of
observation points that does not contain both elements of `ends(d)` (document 03).

## Primitives

| Primitive | Use | Sizes |
|---|---|---|
| ML-KEM-768 (FIPS 203) | the handshake, and the renewal and heal proposed here | encapsulation key 1 184 B, ciphertext 1 088 B, shared secret 32 B |
| ML-DSA-65 (FIPS 204) | the two identity signatures of the handshake; nothing in this package signs | public key 1 952 B, signature 3 309 B |
| SHA-256 | every derivation, always domain-separated | 32 B |
| ChaCha20-Poly1305 (RFC 8439) | the seal of every unit and cell | 12 B nonce, 16 B tag |

**Domain separation.** `H_d(x) = SHA-256(d || 0x00 || x)`, where `d` is an ASCII domain string from
the registry of separators (Canon:3826 and on). No domain string contains `0x00`, so
`d || 0x00 || x` never equals `d' || 0x00 || x'` when `d` differs from `d'`, even when one string is
a prefix of the other. Every input field has a width fixed by Canon, so within one domain the
concatenation can be split back into its fields without length prefixes.

## Assumptions used by the proofs

- **(A1)** ML-KEM-768 is IND-CCA2 secure, the property FIPS 203 targets. The theorems for a
  passive renewal need only IND-CPA, which IND-CCA2 implies. We write
  `Adv^{cpa}_{KEM}(B)` for the advantage of an adversary `B`.
- **(A2)** SHA-256 under distinct domains is modelled as independent random oracles. This is
  the only place the proofs leave the standard model. Section "Remark on the standard model" of
  document 04 says what a standard-model proof would need.
- **(A3)** ChaCha20-Poly1305 with nonces drawn uniformly from 96 bits is IND-CPA and INT-CTXT
  for up to `q` seals under one key, apart from a nonce-collision term of at most `q^2 / 2^97`.
- **(A4)** Erasure works: a value the protocol says is erased cannot be recovered from the
  machine afterwards. This is an assumption about the implementation (`Zeroizing` buffers in the
  code), not a theorem.

## Corruption

`Corrupt(P, t)` gives the adversary everything machine `P` holds *for one link* at time `t`: the
link secret, both directional keys, the state of any renewal in flight (drawn decapsulation keys,
shared secrets, received public values), and the previous receiving key if it is still kept.
Corruption of a pipe is analogous. The compromise of a whole machine, including its long-term
machine secret and therefore its answering (signing) key, is a stronger event, and the theorems
say when it matters.
