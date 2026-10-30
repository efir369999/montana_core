# 08. Decisions taken, decisions open, and questions for reviewers

## Decided by the Author

| Decision | Content |
|---|---|
| 304 (record 307) | Issue #75 is its own task and is not merged into issue #22. The handshake stays pure ML-KEM-768, with no hybrid. |
| 333 = (b) (record 338) | This also settles Q6. The section "How a link renews its keys" and its Canon form are adopted now, and the heal of a pipe in band is taken with it (Q4 (a)). The wire code follows only after the full network run of issue #24. The adoption was carried into the text (Network status line, ledger rows, conformance item, "In scope", Registry NET-25, roadmap stage 18) on 2026-10-30 by PR #121, merge `605c843`. |
| 353 → record 356 | Q4 sub-question: a correspondent offers a heal in the first letter it sends in a window while no heal of its own is pending, option (i). |
| 387 → record 389 | The formal documentation is prepared in full before it is sent to cryptographers. This package is the result. |
| 390 → record 393 | Q7 is decided after the reviewers answer, option (c). [I-16] keeps its present wording until then, and question 3 below asks about it. |
| 435 → record 436 (link 13395) | Q7 = (a): the second sentence of [I-16] is restated as Claim C3′ (document 03, section 5). This replaces record 393; reviewers are asked about C3′ itself, not about whether to adopt it. |
| 391 → record 392 | Q9, option (a). A pipe has at most one heal in flight. A side offers only while it holds one walk and awaits no answer to its own offer. When two offers cross, the offer with the smaller encapsulation key in byte order stands. Carried into Network by PR #126 (not yet merged). |
| 388 → record 394 | Q8, option (a) now. That Montana traffic can be recognized is a named limit, stated under "not claimed". A transport that disguises itself would be (b), a separate decision later. Carried into Network by PR #126. |
| 397 → record 399 | The wire code of the renewal (PR #125) merges after K2 and its own ring. It no longer waits for the network run of #24. This narrows 333 (b) for the wire. |
| 398 → record 400 | The iOS client heals a pipe by the UTC day, the step its pipe walk already takes. The healed walk stands from the next UTC day on. Letters the answering side sends under the old walk stay readable until the end of that day, a named limit of the client. |
| 401 → record 408 | Q10, option (a). On iOS the offer (`pk_h`) and the answer ride in a ninth field of the letter, after the author. The offer goes in the first letter of the UTC day that has room for it, otherwise in the next letter of that day, and the answer the same way. The envelope stays 2 048 bytes and the node sees nothing new. Older builds read eight fields and do not see the ninth. A named limit: a letter with a long text, a quote or a card rides without a heal. |
| 402 → record 409 | Q11, option (a). The answer names its offer. Next to `ct_h` it carries the last 32 bytes of `pk_h` (ρ of the key, already inside the offer, so no new hash domain). An answer that names another ρ is dropped. An unanswered offer is replaced by the first letter of the next UTC day. This closes the gap "Unanswered offers" below. |
| 416 → record 417 | Q12, option (a). The seed's devices hold one pipe with a correspondent (a card's secret goes to the others, `MontanaFirstContact.swift:722-729`), while a heal is one device's. So iOS wires receiving and opening a heal now (slice C2b-2: the walk keys `bodyKeys`, the letter fields read and opened) and sends no offer and no answer (`healCarry` is called nowhere) until the seed's devices carry one heal between them. That is its own item below. |
| 438 → record 439 (link 13479) | The iOS let-go of an unopened answer loses a pipe when an offer of an earlier day arrives late (paper, Section 5.5; `check_lemma5.py`). Option (b): a named limit while sending is off. The correction "later" (an offer of a window not later than the one answered is dropped; only a later offer lets the answer go), in Network and in the client, is a condition of any decision that turns sending a heal on in iOS. |

## Open with the Author

| # | Question | Council's recommendation |
|---|---|---|
| Q1 | What triggers a renewal: (a) the window, with the `rekey_units` ceiling for a stalled chain; (b) the window alone; (c) a unit count alone | (a) |
| Q2 | Whether a renewal is signed: (a) no, 5 units; (b) both answering keys sign, 11 units | (a). The threat named in Network:803 is the taking of a device, and that yields the answering key |
| Q3 | How a link gets fresh keys: (a) the renewal drafted here; (b) a whole new handshake inside the channel, 14 units, unchained | (a) |
| Q5 | Whether the step label should stand on the renewed link secret | leave it to issue #22, where the random label of the code already stands |

## Gaps in the draft that the council will fix

- **Two offers for one heal** and **crossing heals.** Document 05, section 4. Fixed in the text
  by record 392 (Q9 (a)) in PR #126. A pipe has at most one heal in flight. Of two crossing offers,
  the one with the smaller encapsulation key in byte order stands, and the other side answers it
  and nothing else. The code of both clients follows.
- **Unanswered offers. Closed by record 409 (Q11 (a)).** Document 05, section 4 (`05-pipe-heal.md:26`).
  Record 392 lets a side offer only while it awaits no answer to its own offer. Read alone, that
  means one lost letter ends the heals of the pipe for good. The draft, and the line of Network
  that PR #126 leaves standing, replace an unanswered offer with the next one. But then a late
  answer to the old offer is opened with the new key. ML-KEM decapsulation (FIPS 203) does not
  fail on a ciphertext made for another key. It returns a pseudorandom secret. So the offering
  side moves to a walk the other side does not have, and its letters are lost. Record 409 settles
  it. The answer carries the last 32 bytes of the offer's `pk_h` (ρ), and the offering side opens
  an answer only when that ρ is its own pending offer's. An unanswered offer is replaced by the
  first letter of the next UTC day. A late answer to a replaced offer names the old ρ and is
  dropped, so the pipe keeps its walk and costs one heal at most.
- **Several devices of one seed. Decided by record 417 (416 = (a)), its own item of this package.** A correspondent's pipe
  is held by every device of one seed, and a heal by one device alone. If one device heals the pipe with Bob, Bob erases
  the old walk, and the other devices of the same person no longer open his letters and are no longer read. Until this item
  is closed the iOS client receives and opens a heal and sends none. The way the council will draft: the devices of a seed
  carry one heal between them over their own pipe `twin:history`, one heal per seed, so that every device takes the healed
  walk before the correspondent erases the old one.
  Sending is turned on only together with the correction "later" of the let-go rule (decision 439 = (b) on record 438).
- **Stripped offers.** Document 05, section 4. An attacker that holds the old walk and sits on the
  path of the pipe can remove every offer and keep reading, and the draft gives a side no rule for
  offers that go unanswered window after window. Theorem 3 assumes this away.
- **The KDF.** Document 04, "Remark on the standard model". If reviewers advise HKDF or HMAC in
  place of plain domain-separated SHA-256, the change touches every derivation of the set, not
  only these two. It then becomes a question for the Author under [I-7].
- **The ceiling in a stalled window.** Network:305 says a link renews at most once in a window,
  while Network:250-255 has the ceiling renew it whenever a side's count reaches `rekey_units`,
  which in a stalled window happens more than once. The code on branch
  `agent-724/issue-75-wire` renews at the ceiling every time, and the same branch restates the
  invariant as both rules. The text on `main` changes with that code, after the network run of
  issue #24.
- **The exposure bound at the ceiling. Closed in the merged code** by a hard stop at a second ceiling: a key that would seal
  more than 2 x `rekey_units` units of its callers' messages refuses the whole message and the link ends; only the three
  renewal messages are outside the count (PR #125, `mt-net/src/lib.rs:465-474` at `390441f`; test
  `a_key_whose_offer_goes_unanswered_seals_no_more_than_twice_its_ceiling`, `:1130-1145`). What follows records how the gap
  was found. `75-RFC-GPA.md:133` says at most 2^23 x 1 232 bytes per
  direction stay under one key. The code on `agent-724/issue-75-wire` (`mt-net/src/lib.rs:376`,
  `:382-391`, `:546-576`) checks the ceiling before a message, not before a unit, and the offering
  side keeps sealing under its old key until it reads the answer. So the true bound is
  `rekey_units` + (`pieces_max` - 1) + the units the offering side seals before it next reads.
  The node does not read after every send: `pulse::deposit_at` (`montana-node/src/pulse.rs:3376`)
  sends and reads nothing, and a publication deposits every cell of an object in a row
  (`montana-node/src/lib.rs:1841-1843`, `pulse.rs:3455-3456`). The node reads on that link only at
  its next query or collection (`lib.rs:1751-1754`, `pulse.rs:3413-3418`). So the excess is the
  units the node deposits on the link between its offer and its next read, and `Link` does not
  bound it. Closing the gap needs either a read before each deposit once an offer is out, or a hard
  stop at a second ceiling. Until then the text states the bound with this condition.
- **A proposed closure of the exposure bound (for reviewers, not on `main`).** Branch
  `agent-731/issue-75-ceiling-settle`, commits `c72f362` and `d56658e` (comments), over `71ec1b9`: `Link::settle`
  (`mt-net/src/lib.rs:412-443`). A side at the ceiling with a renewal under way reads before it
  seals again, waiting with the patience of a message begun; the messages of the renewal are taken
  in, and any other message of the far side is held for `receive` up to one largest message
  (`pieces_max` x 1 232 bytes). Read from the code, a sending key then carries at most
  `rekey_units` - 1 + 2 x `pieces_max` units plus the units of the offer and of the finish, whatever
  the node deposits. Test `a_side_that_only_sends_settles_its_renewal_before_it_seals_past_the_ceiling`;
  runs on a Linux host: fmt, clippy and tests of `mt-net`, `point_delivery` 3 of 3 (L2
  `a683-k731-i75-settle`), `mt-conformance` (L2 `a683-k732-settle-conf`). The cost is liveness, not
  safety: when the far side does not read in time (it also only sends, or it sends more than one
  largest message before its renewal message), the link ends and a new handshake replaces it; the
  same holds for an answering side at its ceiling whose finish is owed by a side that only sends.
  With `rekey_units` = 2^23 this needs about 10 GB on one key. Whether this cost or the hard stop at
  a second ceiling is preferred is for the reviewers.

## Questions for reviewers

1. **Theorems 1-3.** Do the proofs hold in the random-oracle model as written? Is IND-CPA really
   enough for Theorems 2 and 3, given that the adversary is passive on the renewal and holds the
   link keys that let it read `pk` and `ct`?
2. **Standard model.** Is a dual-PRF assumption on plain SHA-256 acceptable here, or should the
   folds move to HKDF?
3. **The game.** Is the admissibility condition of document 03 the right one? Is C3′ a
   meaningful target, and is there a known bound for `ε` in a no-cover-traffic, slot-batched,
   tag-routed design of this kind?
4. **Leakage.** Does the leakage function L1-L6 leave anything out? Example: the fixed burst of
   five renewal units once per active window, which makes "this link was active in this window"
   slightly more visible than a single unit would.
5. **Active attacks on renewal.** Is the stay-in-the-middle remark of Theorem 2 complete? Is
   there a cheaper authentication than Q2 (b) that heals against an attacker who read a link but
   did not take the machine?
6. **Claims.** Does any line of document 01 claim more than the construction gives?
