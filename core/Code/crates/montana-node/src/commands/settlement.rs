//! Prepare every table change before checking the proposal's state commitments.

use mt_account::{apply_proposal, ProposalSettle};
use mt_consensus::ProposalHeader;
use mt_crypto::Hash32;
use mt_entry::{apply_candidate_expiry, apply_selection_event, is_selection_window};
use mt_genesis::ProtocolParams;
use mt_merkle::{empty_internal, TREE_DEPTH};
use mt_state::compute_state_root;

use crate::state::LocalState;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct StateRoots {
    pub control: Hash32,
    pub nodes: Hash32,
    pub candidates: Hash32,
    pub accounts: Hash32,
    pub state: Hash32,
}

impl StateRoots {
    pub fn of(state: &LocalState) -> Self {
        let nodes = state.nodes.root();
        let candidates = state.candidates.root();
        let accounts = state.accounts.root();
        Self {
            // This legacy runtime has no proposal control-set transition.
            control: empty_internal(TREE_DEPTH),
            nodes,
            candidates,
            accounts,
            state: compute_state_root(&nodes, &candidates, &accounts),
        }
    }

    pub fn from_header(header: &ProposalHeader) -> Self {
        Self {
            control: header.control_root,
            nodes: header.node_root,
            candidates: header.candidate_root,
            accounts: header.account_root,
            state: header.state_root,
        }
    }
}

pub(super) struct PreparedSettlement {
    pub state: LocalState,
    pub roots: StateRoots,
    pub activated: usize,
}

/// A rejected transition cannot change its parent: preparation borrows it
/// immutably and owns all three candidate tables. Histories and disk are outside
/// this operation and may be changed only after it returns a prepared result.
pub(super) fn prepare(
    parent: &LocalState,
    settle: &ProposalSettle,
    timechain_value: &Hash32,
    aggregate: &Hash32,
    params: &ProtocolParams,
    expected: Option<StateRoots>,
) -> Option<PreparedSettlement> {
    let mut state = LocalState {
        accounts: parent.accounts.clone(),
        nodes: parent.nodes.clone(),
        candidates: parent.candidates.clone(),
    };
    apply_proposal(
        &mut state.accounts,
        &mut state.nodes,
        &state.candidates,
        settle,
        params,
    );
    apply_candidate_expiry(&mut state.candidates, settle.window_w);
    let activated = if is_selection_window(settle.window_w, params) {
        let active = state.nodes.len() as u64;
        apply_selection_event(
            &mut state.candidates,
            &mut state.nodes,
            &mut state.accounts,
            timechain_value,
            aggregate,
            active,
            settle.window_w,
            params,
        )
        .len()
    } else {
        0
    };
    // The account primitive returns its partial root before entry transitions.
    // Only the root after expiry and selection commits the tables held here.
    let roots = StateRoots::of(&state);
    if expected.is_some_and(|expected| expected != roots) {
        return None;
    }
    Some(PreparedSettlement {
        state,
        roots,
        activated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mt_state::{
        AccountRecord, AccountTable, CandidatePool, CandidateRecord, NodeRecord, NodeTable,
    };

    fn parent(expires: u64) -> LocalState {
        let mut state = LocalState {
            accounts: AccountTable::new(),
            nodes: NodeTable::new(),
            candidates: CandidatePool::new(),
        };
        for id in [1, 4] {
            state.accounts.insert(AccountRecord {
                account_id: [id; 32],
                balance: 0,
                suite_id: 1,
                is_node_operator: id == 4,
                frontier_hash: [0; 32],
                op_height: 0,
                account_chain_length: 0,
                account_chain_length_snapshot: 0,
                current_pubkey: [0; mt_crypto::PUBLIC_KEY_SIZE],
                creation_window: 0,
                last_op_window: 0,
                last_activation_window: 0,
            });
        }
        state.nodes.insert(NodeRecord {
            node_id: [4; 32],
            node_pubkey: [0; mt_crypto::PUBLIC_KEY_SIZE],
            suite_id: 1,
            operator_account_id: [4; 32],
            start_window: 0,
            chain_length: 10,
            chain_length_snapshot: 10,
            chain_length_checkpoints: [10; 6],
            last_confirmation_window: 0,
        });
        state.candidates.insert(CandidateRecord {
            node_id: [2; 32],
            node_pubkey: [0; mt_crypto::PUBLIC_KEY_SIZE],
            suite_id: 1,
            operator_account_id: [1; 32],
            proof_endpoint: [3; 32],
            w_start: 0,
            ssha_chain_length: 0,
            registration_window: 0,
            expires,
        });
        state
    }

    fn settlement(window: u64) -> ProposalSettle {
        ProposalSettle {
            window_w: window,
            winner_id: [4; 32],
            cemented_confirmers: vec![],
        }
    }

    #[test]
    fn an_expired_candidate_is_absent_from_the_committed_state_root() {
        let params = mt_genesis::genesis_params();
        let window = params.selection_interval;
        let parent = parent(window);
        let before = StateRoots::of(&parent);
        let prepared = prepare(
            &parent,
            &settlement(window),
            &[4; 32],
            &[5; 32],
            params,
            None,
        )
        .unwrap();
        assert!(prepared.state.candidates.is_empty());
        assert_eq!(StateRoots::of(&parent), before);
        assert_eq!(prepared.roots, StateRoots::of(&prepared.state));
    }

    #[test]
    fn selection_commits_the_new_node_operator_and_candidate_tables_together() {
        let params = mt_genesis::genesis_params();
        let window = params.selection_interval;
        let parent = parent(window + 1);
        let before = StateRoots::of(&parent);
        let prepared = prepare(
            &parent,
            &settlement(window),
            &[4; 32],
            &[5; 32],
            params,
            None,
        )
        .unwrap();
        assert_eq!(prepared.activated, 1);
        assert!(prepared.state.nodes.contains(&[2; 32]));
        assert!(
            prepared
                .state
                .accounts
                .get(&[1; 32])
                .unwrap()
                .is_node_operator
        );
        assert!(prepared.state.candidates.is_empty());
        assert_eq!(StateRoots::of(&parent), before);
        assert_eq!(prepared.roots, StateRoots::of(&prepared.state));
    }
    #[test]
    fn a_follower_requires_every_signed_root_and_keeps_its_parent_on_refusal() {
        let params = mt_genesis::genesis_params();
        let window = params.selection_interval;
        let parent = parent(window + 1);
        let before = StateRoots::of(&parent);
        let input = settlement(window);
        let proposed = prepare(&parent, &input, &[4; 32], &[5; 32], params, None).unwrap();
        let roots = proposed.roots;
        let accepted = prepare(&parent, &input, &[4; 32], &[5; 32], params, Some(roots)).unwrap();
        assert_eq!(accepted.roots, StateRoots::of(&accepted.state));
        for field in 0..5 {
            let mut bad = roots;
            let changed = match field {
                0 => &mut bad.control,
                1 => &mut bad.nodes,
                2 => &mut bad.candidates,
                3 => &mut bad.accounts,
                _ => &mut bad.state,
            };
            changed[0] ^= 1;
            assert!(
                prepare(&parent, &input, &[4; 32], &[5; 32], params, Some(bad)).is_none(),
                "field {field}"
            );
            assert_eq!(StateRoots::of(&parent), before);
        }
    }

    #[test]
    fn expiry_precedes_selection_and_an_expired_candidate_cannot_activate() {
        let params = mt_genesis::genesis_params();
        let window = params.selection_interval;
        let parent = parent(window);
        let prepared = prepare(
            &parent,
            &settlement(window),
            &[4; 32],
            &[5; 32],
            params,
            None,
        )
        .unwrap();
        assert_eq!(prepared.activated, 0);
        assert!(!prepared.state.nodes.contains(&[2; 32]));
        assert!(
            !prepared
                .state
                .accounts
                .get(&[1; 32])
                .unwrap()
                .is_node_operator
        );
        assert_eq!(prepared.roots, StateRoots::of(&prepared.state));
    }
}
