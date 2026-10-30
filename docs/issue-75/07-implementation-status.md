# 07. What the code does today

Every path is relative to the repository root at commit `9f72fc07b90c` unless stated otherwise.
"New core" means `core/Montana-Core`, the reference implementation of the specification.
"Legacy client core" means `core/Code`, which is due for removal (issue #5).

## The transport

| Property | State in code | Reference |
|---|---|---|
| Post-quantum handshake (four flights, ML-KEM-768 both ways, ML-DSA-65 over the transcript) | **implemented** | `crates/mt-wire/src/handshake.rs:239-252`; `crates/mt-net/src/lib.rs:326-329` (a fresh KEM pair per handshake) |
| Link units of one size, sealed with ChaCha20-Poly1305 and a random nonce | **implemented** | `crates/mt-net/src/lib.rs:262-271`; `crates/mt-wire/src/link.rs` |
| Link keys fixed for the link's life | **yes**: nothing re-keys a link | `crates/mt-net/src/lib.rs:243-257` |
| Link over datagrams, as Network:179-181 specifies | **no**: a link runs over TCP | `crates/mt-net/src/lib.rs:210-214` (`stream: TcpStream`) |
| Step label derived per step (Canon:1348) | **no**: a random label is drawn per unit; this is issue #22 | `crates/mt-net/src/lib.rs:266` |
| Routing by tag: holder, path, canonical slot, right of carriage, deferred collection | **not implemented** (stage 18) | Registry:129-146 (rows NET-03, NET-04 and NET-06 to NET-20 await stage 18) |
| What a node does instead | deposits every cell of an object at the same point of **every** machine it holds a link to | `crates/montana-node/src/lib.rs:1817-1849`, loop at `:1833-1847` |
| Synthetic filler in the new core | **none**: the last piece of a message is padded to the unit width, and nothing else is sent | Canon:2464-2465, :2530 |
| Padding frame in the legacy client core | **defined, never sent**: `FLAG_PADDING`, `Frame::padding`, a 20 % padding-ratio check and a CSPRNG filler generator exist, and nothing in the repository constructs a padding frame outside that crate's own tests | `core/Code/crates/mt-net/src/framing.rs:21, 53-59, 193-202, 225-229` |

(Paths in the table under `crates/` are inside `core/Montana-Core/`.)

## The work of issue #75

| Slice | What | State | Evidence |
|---|---|---|---|
| 1 | The two folds: `renewed` and `pipe_heal`, the domains `mt-link-renew` and `mt-pipe-heal`, each recomputed by hand in a test with named wrong folds | **on main** | commit `f9281a3`; `mt-wire/src/handshake.rs:254-288`; `mt-derive/src/tag.rs:107-129`; Registry:151 (NET-25, proposed) |
| 2 | Frozen vectors, checked against the code by `mt-conformance` | **on main** | commit `1da3c57`; Canon:3807-3824, :3375-3389; merged by PR #112 (`2865ed6`) |
| 3a | Message kinds 15-17, their layouts in Canon, length checks in the gate | **committed, not on main** | commit `9bb7a7c` on branch `agent-724/issue-75-wire`, rebased onto `605c843`; tests of `mt-wire`, `mt-conformance` and `mt-derive` pass on it |
| 3b | The renewal state machine in `mt-net` `Link`: hold the secret, learn the window, offer, answer, finish, switch, erase | **committed, not on main** | commit `3d86a5b` on the same branch; five tests over a real socket in `mt-net/src/lib.rs`; a full run on a Linux host (rustfmt, clippy with warnings denied on the whole workspace, `mt-wire`, `mt-net`, `mt-conformance`, `montana-node` library tests) ends with 0 failures |
| 3c | `rekey_units` = 2^23 in the Decree of Canon (outside the genesis hash); a side of a link counts the units it sealed under its sending key and offers a renewal at the ceiling whatever the window; the node tells its links the window of every point it publishes at and collects from | **committed, not on main** | commit `124f209` (the ceiling) on the same branch, test `a_side_at_the_ceiling_renews_without_a_window_and_counts_again`; commit `71ec1b9` (the callers of the node) on the same branch, test `point_delivery::one_link_carries_three_windows_and_renews_between_them` (one link, three windows, two renewals); on a Linux host, on the tree of `71ec1b9`, `point_delivery` 3 of 3 and `a_round_on_two_machines` 1 of 1 pass; the full run of the workspace in release on that tree (rustfmt, clippy with warnings denied, `cargo test --release --workspace`) passes 111 of 112 suites (734 tests), and the one suite that failed there, `mt-suite` `differential` (5 of 6: the host had no OpenSSL 3.5 and no liboqs, `crates/mt-suite/tests/differential.rs:65`, `:120`, `:207`, `:389`), passes 6 of 6 on a second Linux host with OpenSSL 3.5.4 and liboqs |
| 3c′ | A proposed closure of the exposure bound at the ceiling: a side at the ceiling with a renewal under way reads before it seals again, and holds the far side's other messages for the caller | **committed, not on main; a proposal** | commits `c72f362` and `d56658e` (comments) on branch `agent-731/issue-75-ceiling-settle`, over `71ec1b9`; test `a_side_that_only_sends_settles_its_renewal_before_it_seals_past_the_ceiling`; on a Linux host fmt, clippy and tests of `mt-net`, `point_delivery` 3 of 3, `mt-conformance` 60 + 3 + 8 pass; the cost and the alternative are in 08 |
| C1 | The heal of a pipe in the iOS client (Swift): `MTPipeKey.healed` folds the old walk with `ss_h`, `pk_h` and `ct_h` under `mt-pipe-heal`; `offerStands` is the crossing rule of decision 391 = (a), the offer with the lower encapsulation key in byte order stands | **committed, not on the release branch** | iOS client repository, branch `agent-758/issue-75-ios-heal`, commit `42b323df`; `Code/Montana/MTPipeKey.swift:553-568`; `healAgreesWithCanon` (`:570`) holds Canon:3380, :3381 and :3384, the negative line included |
| C2a | The heal by the day (decision 398 = (a)): `heal(answeredOn:ss:pk:ct:)` folds the value the old walk reaches on the UTC day of the answering letter; the healed walk `MTPipeHeal` stands from the next UTC day; `MTPipeHealing` holds the two sides (`core/docs/Montana Network.md:291` on `a2baf1d`) | **committed, not on the release branch** | commit `65984b09`; `MTPipeKey.swift:600-683`; `healByDayAgreesWithCanon` (`:610`) holds the vectors of document 05, lines 45-47; a harness of the real file gives both checks true |
| C2b, common part | The ML-KEM-768 side of a heal, in the app target alone: `offer`, `answer(to:)`, the opening of an answer, and `agree`, which runs both sides of one heal | **committed, not on the release branch** | commit `69f9332d`; `Code/Montana/MTPipe.swift:207-251`; `swiftc -typecheck` for `arm64-apple-ios17.0`, exit 0 |
| C2b, the letters | Where the offer and the answer ride in a letter; what becomes of an offer left unanswered | **not written**: open with the Author (constitution records 401 and 402) | the letter envelope is 2 048 bytes (`Code/Montana/MTNodeWire.swift:107`) |
| — | The heal of a pipe in the Kotlin client | **outside this council's work** (the Author's decision 384) | — |
| — | Adoption of Network:193-316 into force | **carried into the text**: the text and Canon form are adopted, the wire code follows after the network run of #24 | decision 333 = (b), record 338; PR #121, merge `605c843` on main |

## Other findings that bear on the claims

- The iOS client computes the window from the wall clock and accepts the neighbouring windows
  (`apps/messenger/ios/Code/Montana/MTPipe.swift:29-35`). This breaks [I-16] requirement 14 and
  [I-15]. It is tracked in issue #22.
- No GPA test network exists. `doc/external-audit/LAB.md:5-10`: "There is no runnable
  production-equivalent Montana GPA testnet here." The observer measurement of stage 22
  (`core/Montana-Core/ROADMAP.md:1217`) has not started.
- The external-audit snapshot lists `'GPA resilience'` among the properties it does **not**
  attest (`doc/external-audit/snapshot.py:82`).

## What this means for a reviewer

Theorems 1-3 are about a specified construction whose derivations are implemented and
vector-checked. The protocol that runs them on a live link is not. Claim C3 (document 01) is
about a routing layer that is not implemented, so no test today can confirm it or refute it.
