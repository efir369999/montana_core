// The history of a window, walked from an anchor through the one entry point. Consensus, "The join":
// a device that holds nothing but two consecutive final proposals its operator handed it verifies the
// certificate of the later one against the electorate and the aggregate the earlier one carries,
// judges the next proposal whole but for its roots, and takes as the head the newest proposal whose
// certificate verified. Every attestation here is the real node prover's, every proof is verified by
// the real verifier, and every proposal is signed by the key its ticket names.
//
// **The wrong implementations it refuses**, named before the assertions: a walker reading the
// electorate of a window off the proposal of the window before rather than two before — the trees of
// the two differ here, so the attestations of the first window stand against the tree only the
// right reading holds; a walker taking a proposal as final before its own certificate verified — the
// head of a walk handed one step stays at the anchor; a walker counting a certificate below the
// quorum; one judging a ticket against an aggregate the proposal two before does not carry; one
// trusting its anchor beyond the anchor; one taking as the next proposal a proposal the window
// after it does not attest; and one stopped by a runner publishing, under the honest name, bytes the
// door refuses — it judges every copy under the name and takes the first that stands.

use montana_node::attest::Standing;
use montana_node::certificate::{
    decided_of, empty_census, verify_certificate, walk, Anchor, Electorate, Predecessor, Roots,
    Round, Step,
};
use montana_node::pulse::Cohort;
use montana_node::runner::Runner;
use mt_state::layout::{Beacon, Confirmation, Proposal};

const ANCHOR: u64 = 2_000;

fn refused(what: &'static str) -> montana_node::NodeError {
    montana_node::NodeError::Refused { what }
}

// A final proposal the operator hands over: what the walk trusts, and nothing it verifies but the
// link between the two.
fn handed(window: u64, previous: [u8; 32], cohort: &Cohort, cement: [u8; 32]) -> Proposal {
    Proposal {
        window,
        protocol_version: mt_state::PROTOCOL_VERSION,
        previous,
        final_beacon: [0x22; 32],
        note_root: [0x33; 32],
        nullifier_root: [0x44; 32],
        record_root: [0x55; 32],
        machine_root: [0x66; 32],
        admitted_root: cohort.root().bytes(),
        operations_root: [0x88; 32],
        admitted_count: cohort.count(),
        cement_aggregate: cement,
        cement_count: cohort.count(),
        census: empty_census().expect("the Decree names the census"),
        suite_id: mt_net::SUITE,
        runner_pubkey: vec![0xA1; mt_codec::size::SIGNING_PUBLIC_KEY],
        ticket_proof: vec![0x00; mt_state::PRESENCE_PROOF_LEN],
        signature: vec![0x5E; mt_codec::size::SIGNATURE],
    }
}

// The opening beacon of a window, standing on the proposal of the window before.
fn opening(window: u64, previous: [u8; 32]) -> Beacon {
    Beacon {
        window,
        chain: 0,
        round: 0,
        previous,
        cement_aggregate: mt_derive::aggregate::aggregate(&[], window).expect("the empty set"),
        cement_count: 0,
        cement_votes: 0,
    }
}

// The beacon of round one, carrying the cement the heavies of round zero name.
fn closing(window: u64, first: &Beacon, heavies: &[Confirmation]) -> Beacon {
    let mut parts: Vec<[u8; 32]> = heavies.iter().map(|h| h.part_nullifier).collect();
    parts.sort_unstable();
    Beacon {
        window,
        chain: 0,
        round: 1,
        previous: first.id().expect("an identifier"),
        cement_aggregate: mt_derive::aggregate::aggregate(&parts, window).expect("distinct parts"),
        cement_count: parts.len() as u64,
        cement_votes: 0,
    }
}

// The heavy attestations of a window by the named machines of a tree, answering the opening beacon
// and attesting the proposal of the window before.
fn heavies(
    secrets: &[[u8; 32]],
    cohort: &Cohort,
    window: u64,
    beacon: &Beacon,
    previous: [u8; 32],
) -> Vec<Confirmation> {
    secrets
        .iter()
        .map(|secret| {
            let branch = cohort.branch_of(secret).expect("its own leaf");
            Standing::of_the_window(secret, window, &branch.siblings, branch.position)
                .expect("a machine of the tree proves its presence")
                .heavy(secret, beacon.id().expect("an identifier"), previous)
                .expect("a heavy attestation stands")
        })
        .collect()
}

#[test]
fn a_device_walks_from_its_anchor_to_the_head_and_no_further() {
    let (one, two, three) = ([0x11u8; 32], [0x22u8; 32], [0x33u8; 32]);
    let pair = Cohort::of(&[Cohort::half_of(&one), Cohort::half_of(&two)]).expect("a tree of two");
    let all = Cohort::of(&[
        Cohort::half_of(&one),
        Cohort::half_of(&two),
        Cohort::half_of(&three),
    ])
    .expect("a tree of three");
    assert_ne!(pair.root(), all.root());
    let draw = mt_pulse::retarget::draw_threshold().expect("the threshold of the draw");
    let major = mt_state::implemented_major();

    // The anchor: the earlier proposal admits the pair and the later one all three, so the
    // electorate of the window two on is the pair and the electorate of the window three on is the
    // three. The two carry two different cements, which the draws two windows on read.
    let earlier = handed(ANCHOR, [0x10; 32], &pair, [0x0Au8; 32]);
    let later = handed(
        ANCHOR + 1,
        earlier.id().expect("an identifier"),
        &all,
        [0x0Bu8; 32],
    );
    let anchor = Anchor {
        earlier: earlier.clone(),
        later: later.clone(),
    };
    let later_id = later.id().expect("an identifier");

    // The window after the anchor: the pair attests the later proposal against the tree the earlier
    // proposal carries, and the runner of the window proposes what it decided.
    let window = ANCHOR + 2;
    let first = opening(window, later_id);
    let answered = heavies(&[one, two], &pair, window, &first, later_id);
    let last = closing(window, &first, &answered);
    let rounds = [
        Round {
            beacon: &first,
            heavy: &answered,
            ballots: &[],
        },
        Round {
            beacon: &last,
            heavy: &[],
            ballots: &[],
        },
    ];
    let electorate = Electorate::of(&earlier).expect("the electorate of the window");
    let closure = verify_certificate(
        window,
        &electorate,
        Predecessor::Proposal(&later),
        &earlier.cement_aggregate,
        1,
        &rounds,
    )
    .expect("the certificate of the later proposal verifies against the tree two before");
    // The electorate of the window before is another tree, and the attestations of the pair stand
    // nowhere in it: the reading of one window too late refuses an honest certificate.
    assert_eq!(
        verify_certificate(
            window,
            &Electorate::of(&later).expect("an electorate"),
            Predecessor::Proposal(&later),
            &earlier.cement_aggregate,
            1,
            &rounds,
        ),
        Err(refused(
            "an attestation whose proof of presence does not verify"
        ))
    );
    let decided = decided_of(
        &last.id().expect("an identifier"),
        Predecessor::Proposal(&later),
        &rounds,
        &closure,
        &electorate,
    )
    .expect("the final beacon of the window");
    let branch = pair.branch_of(&one).expect("its own leaf");
    let runner = Runner::of_the_window(
        &one,
        window,
        &branch.siblings,
        branch.position,
        &earlier.cement_aggregate,
        &draw,
    )
    .expect("the ticket proves")
    .expect("the threshold of the draw clears every machine");
    let roots = Roots {
        note_root: [0x31; 32],
        nullifier_root: [0x41; 32],
        record_root: [0x51; 32],
        machine_root: [0x61; 32],
        admitted_root: all.root().bytes(),
        operations_root: [0x81; 32],
        admitted_count: all.count(),
        census: later.census.clone(),
    };
    let next = runner
        .propose(
            mt_state::PROTOCOL_VERSION,
            Predecessor::Proposal(&later),
            &roots,
            &decided,
        )
        .expect("the proposal stands");
    let next_id = next.id().expect("an identifier");

    // The window after that: all three attest the next proposal against the tree the later proposal
    // of the anchor carries.
    let after = ANCHOR + 3;
    let first_after = opening(after, next_id);
    let answered_after = heavies(&[one, two, three], &all, after, &first_after, next_id);
    let rounds_after = [Round {
        beacon: &first_after,
        heavy: &answered_after,
        ballots: &[],
    }];

    // Accepted: the walk verifies the certificate of the anchor's later proposal, judges the next
    // one, verifies its certificate, and stands on it.
    let steps = [
        Step {
            certificate: &rounds,
            next: std::slice::from_ref(&next),
        },
        Step {
            certificate: &rounds_after,
            next: &[],
        },
    ];
    let head = walk(&anchor, &steps, 1, &draw, major).expect("the walk reaches the head");
    assert_eq!(head.earlier, later);
    assert_eq!(head.later.id(), next.id());

    // A proposal awaiting its certificate is not final: handed the first step alone, the walk stands
    // where it began.
    let pending = walk(&anchor, &steps[..1], 1, &draw, major).expect("the anchor's certificate");
    assert_eq!(pending, anchor);

    // Refused: a certificate below the quorum — two of the three.
    let short = [Round {
        beacon: &first_after,
        heavy: &answered_after[..2],
        ballots: &[],
    }];
    assert_eq!(
        walk(
            &anchor,
            &[
                Step {
                    certificate: &rounds,
                    next: std::slice::from_ref(&next),
                },
                Step {
                    certificate: &short,
                    next: &[],
                },
            ],
            1,
            &draw,
            major,
        ),
        Err(refused("a cement below the quorum share of the electorate"))
    );

    // Refused: a next proposal whose ticket was drawn against the aggregate the later proposal
    // carries rather than the earlier one.
    let drawn_late = Runner::of_the_window(
        &one,
        window,
        &branch.siblings,
        branch.position,
        &later.cement_aggregate,
        &draw,
    )
    .expect("the ticket proves")
    .expect("drawn")
    .propose(
        mt_state::PROTOCOL_VERSION,
        Predecessor::Proposal(&later),
        &roots,
        &decided,
    )
    .expect("the proposal stands");
    assert_eq!(drawn_late.id(), next.id());
    assert_eq!(
        walk(
            &anchor,
            &[Step {
                certificate: &rounds,
                next: std::slice::from_ref(&drawn_late),
            }],
            1,
            &draw,
            major,
        ),
        Err(refused("a proposal whose ticket proof does not verify"))
    );

    // Refused: an anchor whose later proposal does not name the earlier one.
    let loose = Anchor {
        earlier: earlier.clone(),
        later: handed(ANCHOR + 1, [0x77; 32], &all, [0x0Bu8; 32]),
    };
    assert_eq!(
        walk(&loose, &steps, 1, &draw, major),
        Err(refused(
            "an anchor of two proposals that are not consecutive"
        ))
    );

    // Refused: an anchor that is not the chain's own — a forged pair, linked to itself, is refused at
    // the first certificate, which attests the chain's own proposal and not the forged one.
    let forged_earlier = handed(ANCHOR, [0x10; 32], &pair, [0x0Cu8; 32]);
    let forged = Anchor {
        earlier: forged_earlier.clone(),
        later: handed(
            ANCHOR + 1,
            forged_earlier.id().expect("an identifier"),
            &all,
            [0x0Bu8; 32],
        ),
    };
    assert_eq!(
        walk(&forged, &steps, 1, &draw, major),
        Err(refused(
            "an attestation outside this beacon and the proposal it finalizes"
        ))
    );

    // A next proposal the window after it does not attest is not taken. Its roots differ, so it
    // decided something else and carries another name; the attestations of the window after name the
    // true one, and the walk stands where it was until a proposal under that name is offered.
    let mut other_roots = roots.clone();
    other_roots.note_root[0] ^= 1;
    let other = runner
        .propose(
            mt_state::PROTOCOL_VERSION,
            Predecessor::Proposal(&later),
            &other_roots,
            &decided,
        )
        .expect("the proposal stands");
    assert_ne!(other.id(), next.id());
    assert_eq!(
        walk(
            &anchor,
            &[
                Step {
                    certificate: &rounds,
                    next: std::slice::from_ref(&other),
                },
                Step {
                    certificate: &rounds_after,
                    next: &[],
                },
            ],
            1,
            &draw,
            major,
        ),
        Ok(anchor.clone())
    );
    let both = [other.clone(), next.clone()];
    let chosen = walk(
        &anchor,
        &[
            Step {
                certificate: &rounds,
                next: &both,
            },
            Step {
                certificate: &rounds_after,
                next: &[],
            },
        ],
        1,
        &draw,
        major,
    )
    .expect("the attested proposal is taken among two");
    assert_eq!(chosen.later.id(), next.id());

    // A copy of the honest decision under the honest name, re-signed by its runner with a final
    // beacon outside the certificate: the name is one, since the beacon stands outside it, and the
    // door refuses the copy at the beacon. Offered alone it is refused by name; offered beside the
    // honest bytes it stops nothing.
    let mut misplaced = next.clone();
    misplaced.final_beacon = [0x5Au8; 32];
    let seed = mt_derive::pulse::runner_key_seed(&one, window);
    let mut held = [0u8; mt_suite::sign::SEED_BYTES];
    held.copy_from_slice(&seed[..mt_suite::sign::SEED_BYTES]);
    let (key, secret) = mt_suite::sign::keypair_from_seed(&held);
    assert_eq!(key.as_bytes().to_vec(), next.runner_pubkey);
    misplaced.signature = mt_suite::sign::sign(
        &secret,
        &mt_state::Layout::signed_scope(&misplaced).expect("a scope"),
    )
    .expect("the runner owns its key")
    .as_bytes()
    .to_vec();
    assert_eq!(misplaced.id(), next.id());
    assert_eq!(
        walk(
            &anchor,
            &[
                Step {
                    certificate: &rounds,
                    next: std::slice::from_ref(&misplaced),
                },
                Step {
                    certificate: &rounds_after,
                    next: &[],
                },
            ],
            1,
            &draw,
            major,
        ),
        Err(refused("a final beacon outside the certificate"))
    );
    let copies = [misplaced, next.clone()];
    let past_the_copy = walk(
        &anchor,
        &[
            Step {
                certificate: &rounds,
                next: &copies,
            },
            Step {
                certificate: &rounds_after,
                next: &[],
            },
        ],
        1,
        &draw,
        major,
    )
    .expect("the honest bytes stand beside a refused copy");
    assert_eq!(past_the_copy.later.signature, next.signature);
}
