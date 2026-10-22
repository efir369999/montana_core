//! A cemented hash names a reveal; it cannot stand in for its missing contents.

use mt_crypto::Hash32;
use mt_lottery::{
    determine_winner, reveal_hash, weighted_ticket_node, Candidate, SshaReveal, WINNER_CLASS_NODE,
};
use mt_state::{NodeId, NodeTable};
use std::collections::{BTreeMap, BTreeSet};

use super::cement::meta_root;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum LotteryError {
    RepeatedHash,
    MissingReveal,
    InvalidContext,
    WrongRoot,
    WrongWinner,
}

pub(super) struct CompleteLottery {
    pub candidates: Vec<Candidate>,
    pub root: Hash32,
    winner: Option<(NodeId, Hash32)>,
}

impl CompleteLottery {
    /// The legacy empty-set fallback remains the proposer, with no endpoint.
    /// It is not an independently drawn winner or proof of canonical finality.
    pub fn winner_or(&self, proposer: NodeId) -> (NodeId, Hash32) {
        self.winner.unwrap_or((proposer, [0; 32]))
    }

    pub fn verify(
        &self,
        root: Hash32,
        winner: NodeId,
        endpoint: Hash32,
        proposer: NodeId,
    ) -> Result<(), LotteryError> {
        if root != self.root {
            return Err(LotteryError::WrongRoot);
        }
        if (winner, endpoint) != self.winner_or(proposer) {
            return Err(LotteryError::WrongWinner);
        }
        Ok(())
    }
}

/// The pool is owned by the runtime's signature/endpoint validator. Preparation
/// still checks its map key and window, and requires every cemented hash before
/// calculating a winner. A partial pool never yields a partial lottery result.
pub(super) fn complete(
    pool: Option<&BTreeMap<NodeId, SshaReveal>>,
    cemented: &[Hash32],
    window: u64,
    nodes: &NodeTable,
) -> Result<CompleteLottery, LotteryError> {
    let wanted: BTreeSet<_> = cemented.iter().copied().collect();
    if wanted.len() != cemented.len() {
        return Err(LotteryError::RepeatedHash);
    }
    let mut entries = Vec::new();
    let mut candidates = Vec::new();
    let mut endpoints = BTreeMap::new();
    if let Some(pool) = pool {
        for (id, reveal) in pool {
            let hash = reveal_hash(reveal);
            if !wanted.contains(&hash) {
                continue;
            }
            if *id != reveal.node_id || reveal.window_index != window {
                return Err(LotteryError::InvalidContext);
            }
            let node = nodes.get(id).ok_or(LotteryError::InvalidContext)?;
            entries.push((*id, hash));
            endpoints.insert(*id, reveal.endpoint);
            candidates.push(Candidate {
                ticket: weighted_ticket_node(
                    &reveal.endpoint,
                    node.chain_length,
                    node.chain_length_snapshot.max(1),
                ),
                class: WINNER_CLASS_NODE,
                id: *id,
            });
        }
    }
    if entries.len() != wanted.len() {
        return Err(LotteryError::MissingReveal);
    }
    candidates.sort_by(|a, b| a.ticket.cmp(&b.ticket).then(a.id.cmp(&b.id)));
    let winner = determine_winner(&candidates).map(|winner| (winner.id, endpoints[&winner.id]));
    Ok(CompleteLottery {
        candidates,
        root: meta_root(&entries),
        winner,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mt_crypto::{Signature, SIGNATURE_SIZE};
    use mt_state::NodeRecord;

    const WINDOW: u64 = 100;

    fn fixture() -> (NodeTable, BTreeMap<NodeId, SshaReveal>, Vec<Hash32>) {
        let mut nodes = NodeTable::new();
        let mut pool = BTreeMap::new();
        let mut hashes = Vec::new();
        for id in 1..=3 {
            nodes.insert(NodeRecord {
                node_id: [id; 32],
                node_pubkey: [0; mt_crypto::PUBLIC_KEY_SIZE],
                suite_id: 1,
                operator_account_id: [id; 32],
                start_window: 0,
                chain_length: 100,
                chain_length_snapshot: 100,
                chain_length_checkpoints: [100; 6],
                last_confirmation_window: WINDOW,
            });
            // This fixture exercises the already-validated pool's accounting.
            // It does not claim these synthetic signatures are authentic.
            let reveal = SshaReveal {
                node_id: [id; 32],
                window_index: WINDOW,
                endpoint: [id; 32],
                signature: Signature::from_array([0; SIGNATURE_SIZE]),
            };
            hashes.push(reveal_hash(&reveal));
            pool.insert(reveal.node_id, reveal);
        }
        (nodes, pool, hashes)
    }

    #[test]
    fn a_missing_reveal_prevents_acceptance_until_the_data_arrives() {
        let (nodes, pool, hashes) = fixture();
        let partial = pool
            .iter()
            .take(1)
            .map(|(id, r)| (*id, r.clone()))
            .collect();
        assert_eq!(
            complete(Some(&partial), &hashes[..2], WINDOW, &nodes).err(),
            Some(LotteryError::MissingReveal)
        );
        assert_eq!(
            complete(None, &hashes[..2], WINDOW, &nodes).err(),
            Some(LotteryError::MissingReveal)
        );
        let all = complete(Some(&pool), &hashes[..2], WINDOW, &nodes).unwrap();
        let (winner, endpoint) = all.winner_or([9; 32]);
        assert_eq!(all.verify(all.root, winner, endpoint, [9; 32]), Ok(()));
    }

    #[test]
    fn an_uncemented_ticket_cannot_change_the_root_or_winner() {
        let (nodes, pool, hashes) = fixture();
        let only = pool
            .iter()
            .take(2)
            .map(|(id, r)| (*id, r.clone()))
            .collect();
        let a = complete(Some(&only), &hashes[..2], WINDOW, &nodes).unwrap();
        let b = complete(Some(&pool), &hashes[..2], WINDOW, &nodes).unwrap();
        assert_eq!(a.root, b.root);
        assert_eq!(a.candidates, b.candidates);
        assert_eq!(a.winner_or([9; 32]), b.winner_or([9; 32]));
    }

    #[test]
    fn a_signed_winner_endpoint_and_root_must_match_the_complete_lottery() {
        let (nodes, pool, hashes) = fixture();
        let held = complete(Some(&pool), &hashes, WINDOW, &nodes).unwrap();
        let (winner, endpoint) = held.winner_or([9; 32]);
        assert_eq!(
            held.verify([0; 32], winner, endpoint, [9; 32]),
            Err(LotteryError::WrongRoot)
        );
        assert_eq!(
            held.verify(held.root, [9; 32], endpoint, [9; 32]),
            Err(LotteryError::WrongWinner)
        );
        assert_eq!(
            held.verify(held.root, winner, [0; 32], [9; 32]),
            Err(LotteryError::WrongWinner)
        );
    }

    #[test]
    fn duplicate_hashes_and_wrong_pool_context_are_refused() {
        let (nodes, mut pool, hashes) = fixture();
        assert_eq!(
            complete(Some(&pool), &[hashes[0]; 2], WINDOW, &nodes).err(),
            Some(LotteryError::RepeatedHash)
        );
        assert_eq!(
            complete(Some(&pool), &hashes, WINDOW + 1, &nodes).err(),
            Some(LotteryError::InvalidContext)
        );
        let reveal = pool.remove(&[1; 32]).unwrap();
        pool.insert([7; 32], reveal);
        assert_eq!(
            complete(Some(&pool), &hashes, WINDOW, &nodes).err(),
            Some(LotteryError::InvalidContext)
        );
    }

    #[test]
    fn an_empty_legacy_lottery_binds_its_fallback_to_the_proposer() {
        let empty = complete(None, &[], 0, &NodeTable::new()).unwrap();
        assert_eq!(empty.verify(empty.root, [9; 32], [0; 32], [9; 32]), Ok(()));
        assert_eq!(
            empty.verify(empty.root, [8; 32], [0; 32], [9; 32]),
            Err(LotteryError::WrongWinner)
        );
        assert_eq!(
            empty.verify(empty.root, [9; 32], [1; 32], [9; 32]),
            Err(LotteryError::WrongWinner)
        );
    }
}
