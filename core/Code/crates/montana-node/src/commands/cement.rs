//! Verification of the legacy proposal's two confirmation sets before mutation.
//! Wire duplicates, an unrelated window and inactive standing cannot buy quorum.

use std::collections::{BTreeMap, BTreeSet};

use mt_consensus::PROPOSAL_HEADER_SIZE;
use mt_crypto::Hash32;
use mt_genesis::ProtocolParams;
use mt_lottery::{
    bundle_hash, quorum, validate_bundle, BundledConfirmation, BUNDLE_FIXED_OVERHEAD,
};
use mt_merkle::{empty_internal, SparseMerkleTree, TREE_DEPTH};
use mt_state::{is_active, NodeId, NodeTable};

pub(super) fn parse_envelope_bundles(
    payload: &[u8],
) -> Option<(Vec<BundledConfirmation>, Vec<BundledConfirmation>)> {
    fn list(input: &mut &[u8]) -> Option<Vec<BundledConfirmation>> {
        let count = u16::from_le_bytes(input.get(..2)?.try_into().ok()?) as usize;
        *input = input.get(2..)?;
        // Bound allocation by bytes actually received, before trusting a count.
        if count > input.len() / BUNDLE_FIXED_OVERHEAD {
            return None;
        }
        let mut seen = BTreeSet::new();
        let mut bundles = Vec::with_capacity(count);
        for _ in 0..count {
            let (bundle, used) = BundledConfirmation::decode(input).ok()?;
            if !seen.insert(bundle.node_id) {
                return None;
            }
            *input = input.get(used..)?;
            bundles.push(bundle);
        }
        Some(bundles)
    }
    let mut input = payload.get(PROPOSAL_HEADER_SIZE..)?;
    let previous = list(&mut input)?;
    let current = list(&mut input)?;
    input.is_empty().then_some((previous, current))
}

pub(super) fn meta_root(entries: &[(NodeId, Hash32)]) -> Hash32 {
    if entries.is_empty() {
        return empty_internal(TREE_DEPTH);
    }
    let mut tree = SparseMerkleTree::new();
    for (id, h) in entries {
        let mut meta = Vec::with_capacity(64);
        meta.extend_from_slice(id);
        meta.extend_from_slice(h);
        tree.insert(*id, &meta);
    }
    tree.root()
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum CementError {
    EmptyActiveSet,
    WeightOverflow,
    MissingPredecessor,
    WrongWindow,
    Duplicate,
    InvalidBundle,
    BundleRoot,
    ShortOfQuorum,
}

pub(super) fn active_chain_length_at(
    nodes: &NodeTable,
    window: u64,
    params: &ProtocolParams,
) -> Result<u64, CementError> {
    nodes.iter().try_fold(0u64, |sum, node| {
        sum.checked_add(active_standing(node, window, params))
            .ok_or(CementError::WeightOverflow)
    })
}

fn active_standing(node: &mt_state::NodeRecord, window: u64, params: &ProtocolParams) -> u64 {
    if is_active(node, window, params.tau2_windows) {
        node.chain_length
    } else {
        0
    }
}

/// Read only the accumulator populated after bundle validation. This does not
/// authenticate a wire bundle and must never replace `CementContext::verify`.
pub(super) fn accumulated_standing_at(
    bundles: Option<&BTreeMap<NodeId, BundledConfirmation>>,
    bundle_window: u64,
    quorum_window: u64,
    nodes: &NodeTable,
    params: &ProtocolParams,
) -> Result<u64, CementError> {
    let mut sum = 0u64;
    if let Some(bundles) = bundles {
        for (id, bc) in bundles {
            if bc.window_index != bundle_window {
                return Err(CementError::WrongWindow);
            }
            if *id != bc.node_id {
                return Err(CementError::InvalidBundle);
            }
            let node = nodes.get(id).ok_or(CementError::InvalidBundle)?;
            sum = sum
                .checked_add(active_standing(node, quorum_window, params))
                .ok_or(CementError::WeightOverflow)?;
        }
    }
    Ok(sum)
}

pub(super) struct CementContext<'a> {
    pub window: u64,
    pub previous_endpoint: Option<&'a Hash32>,
    pub current_endpoint: &'a Hash32,
    pub nodes: &'a NodeTable,
    pub params: &'a ProtocolParams,
}

pub(super) struct VerifiedCement {
    pub confirmers: Vec<NodeId>,
    pub cemented_hashes: Vec<Hash32>,
}

impl CementContext<'_> {
    fn set<'a>(
        &self,
        bundles: &'a [BundledConfirmation],
        window: u64,
        endpoint: &Hash32,
    ) -> Result<(BTreeMap<NodeId, &'a BundledConfirmation>, u64), CementError> {
        let mut set = BTreeMap::new();
        let mut standing = 0u64;
        for bc in bundles {
            if bc.window_index != window {
                return Err(CementError::WrongWindow);
            }
            if set.insert(bc.node_id, bc).is_some() {
                return Err(CementError::Duplicate);
            }
            validate_bundle(bc, self.nodes, endpoint).map_err(|_| CementError::InvalidBundle)?;
            // A valid returning node may be included and earn presence. It
            // contributes to this quorum only if it was active before the fold.
            let node = self
                .nodes
                .get(&bc.node_id)
                .ok_or(CementError::InvalidBundle)?;
            standing = standing
                .checked_add(active_standing(node, self.window, self.params))
                .ok_or(CementError::WeightOverflow)?;
        }
        Ok((set, standing))
    }

    pub fn verify(
        &self,
        bundles_root: &Hash32,
        previous: &[BundledConfirmation],
        current: &[BundledConfirmation],
    ) -> Result<VerifiedCement, CementError> {
        let previous_window = self.window.checked_sub(1).ok_or(CementError::WrongWindow)?;
        let total = active_chain_length_at(self.nodes, self.window, self.params)?;
        if total == 0 {
            return Err(CementError::EmptyActiveSet);
        }
        let need = quorum(total);
        let root = meta_root(
            &previous
                .iter()
                .map(|bc| (bc.node_id, bundle_hash(bc)))
                .collect::<Vec<_>>(),
        );
        if root != *bundles_root {
            return Err(CementError::BundleRoot);
        }
        // Legacy genesis has no confirmations of window zero. This exception
        // belongs to that first transition; an empty set later is not a quorum.
        let confirmers = if self.window == 1 && previous.is_empty() {
            Vec::new()
        } else {
            let endpoint = self
                .previous_endpoint
                .ok_or(CementError::MissingPredecessor)?;
            let (set, standing) = self.set(previous, previous_window, endpoint)?;
            if standing < need {
                return Err(CementError::ShortOfQuorum);
            }
            set.into_keys().collect()
        };
        let (set, standing) = self.set(current, self.window, self.current_endpoint)?;
        if standing < need {
            return Err(CementError::ShortOfQuorum);
        }
        let mut weight = BTreeMap::<Hash32, u64>::new();
        for (id, bc) in set {
            let node = self.nodes.get(&id).ok_or(CementError::InvalidBundle)?;
            if !is_active(node, self.window, self.params.tau2_windows) {
                continue;
            }
            // validate_bundle has already rejected repeated hashes within a BC.
            for hash in &bc.reveal_hashes {
                let sum = weight.entry(*hash).or_default();
                *sum = sum
                    .checked_add(node.chain_length)
                    .ok_or(CementError::WeightOverflow)?;
            }
        }
        Ok(VerifiedCement {
            confirmers,
            cemented_hashes: weight
                .into_iter()
                .filter(|(_, sum)| *sum >= need)
                .map(|(h, _)| h)
                .collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mt_codec::CanonicalEncode;
    use mt_crypto::{keypair_from_seed, sign, SecretKey, Signature, SuiteId, SIGNATURE_SIZE};
    use mt_state::{derive_node_id, NodeRecord};

    const WINDOW: u64 = 100_000;
    const PREVIOUS: Hash32 = [0x11; 32];
    const CURRENT: Hash32 = [0x22; 32];
    const REVEAL: Hash32 = [0x33; 32];

    struct Fixture {
        nodes: NodeTable,
        keys: Vec<(NodeId, SecretKey)>,
        previous: Vec<BundledConfirmation>,
        current: Vec<BundledConfirmation>,
    }

    fn signed(id: NodeId, sk: &SecretKey, window: u64, endpoint: Hash32) -> BundledConfirmation {
        let mut bc = BundledConfirmation {
            node_id: id,
            window_index: window,
            endpoint,
            op_hashes: Vec::new(),
            reveal_hashes: vec![REVEAL],
            signature: Signature::from_array([0; SIGNATURE_SIZE]),
        };
        let mut scope = Vec::new();
        bc.encode_signed_scope(&mut scope);
        bc.signature = sign(sk, &scope).unwrap();
        bc
    }

    impl Fixture {
        fn new() -> Self {
            let mut nodes = NodeTable::new();
            let mut keys = Vec::new();
            let mut previous = Vec::new();
            let mut current = Vec::new();
            for (i, standing) in [67, 33].into_iter().enumerate() {
                let (pk, sk) = keypair_from_seed(&[i as u8 + 1; 32]).unwrap();
                let node_id = derive_node_id(pk.as_bytes());
                nodes.insert(NodeRecord {
                    node_id,
                    node_pubkey: *pk.as_bytes(),
                    suite_id: SuiteId::Mldsa65 as u16,
                    operator_account_id: [0; 32],
                    start_window: 0,
                    chain_length: standing,
                    chain_length_snapshot: standing,
                    chain_length_checkpoints: [standing; 6],
                    last_confirmation_window: WINDOW - 1,
                });
                previous.push(signed(node_id, &sk, WINDOW - 1, PREVIOUS));
                current.push(signed(node_id, &sk, WINDOW, CURRENT));
                keys.push((node_id, sk));
            }
            Self {
                nodes,
                keys,
                previous,
                current,
            }
        }

        fn context(&self) -> CementContext<'_> {
            CementContext {
                window: WINDOW,
                previous_endpoint: Some(&PREVIOUS),
                current_endpoint: &CURRENT,
                nodes: &self.nodes,
                params: mt_genesis::genesis_params(),
            }
        }
    }

    fn root(bundles: &[BundledConfirmation]) -> Hash32 {
        meta_root(
            &bundles
                .iter()
                .map(|bc| (bc.node_id, bundle_hash(bc)))
                .collect::<Vec<_>>(),
        )
    }

    fn envelope(previous: &[BundledConfirmation], current: &[BundledConfirmation]) -> Vec<u8> {
        let mut bytes = vec![0; PROPOSAL_HEADER_SIZE];
        for list in [previous, current] {
            bytes.extend_from_slice(&(list.len() as u16).to_le_bytes());
            for bc in list {
                bc.encode(&mut bytes);
            }
        }
        bytes
    }

    #[test]
    fn one_signature_cannot_be_counted_repeatedly() {
        let f = Fixture::new();
        let minority = vec![f.current[1].clone(); 3];
        assert_eq!(
            f.context()
                .verify(&root(&f.previous), &f.previous, &minority)
                .err(),
            Some(CementError::Duplicate)
        );
        assert!(parse_envelope_bundles(&envelope(&f.previous, &minority)).is_none());
        let minority_previous = vec![f.previous[1].clone(); 3];
        assert_eq!(
            f.context()
                .verify(&root(&minority_previous), &minority_previous, &f.current)
                .err(),
            Some(CementError::Duplicate)
        );
        assert!(parse_envelope_bundles(&envelope(&minority_previous, &f.current)).is_none());
    }

    #[test]
    fn honest_quorum_verifies_and_a_minority_does_not() {
        let f = Fixture::new();
        let verified = f
            .context()
            .verify(&root(&f.previous[..1]), &f.previous[..1], &f.current[..1])
            .unwrap();
        assert_eq!(verified.confirmers, vec![f.keys[0].0]);
        assert_eq!(verified.cemented_hashes, vec![REVEAL]);
        assert_eq!(
            f.context()
                .verify(&root(&f.previous), &f.previous, &f.current[1..])
                .err(),
            Some(CementError::ShortOfQuorum)
        );
    }

    #[test]
    fn signed_wrong_window_and_wrong_root_are_rejected() {
        let mut f = Fixture::new();
        f.current[0] = signed(f.keys[0].0, &f.keys[0].1, WINDOW - 1, CURRENT);
        assert_eq!(
            f.context()
                .verify(&root(&f.previous), &f.previous, &f.current)
                .err(),
            Some(CementError::WrongWindow)
        );
        let f = Fixture::new();
        assert_eq!(
            f.context()
                .verify(&[0x99; 32], &f.previous, &f.current)
                .err(),
            Some(CementError::BundleRoot)
        );
    }

    #[test]
    fn missing_predecessor_cannot_be_learned_from_an_unverified_bundle() {
        let f = Fixture::new();
        let mut context = f.context();
        context.previous_endpoint = None;
        assert_eq!(
            context
                .verify(&root(&f.previous), &f.previous, &f.current)
                .err(),
            Some(CementError::MissingPredecessor)
        );
        assert_eq!(
            f.context().verify(&root(&[]), &[], &f.current).err(),
            Some(CementError::ShortOfQuorum)
        );
    }

    #[test]
    fn inactive_standing_does_not_buy_cement() {
        let mut f = Fixture::new();
        let mut inactive = f.nodes.get(&f.keys[0].0).unwrap().clone();
        inactive.last_confirmation_window = 0;
        inactive.chain_length = u64::MAX;
        f.nodes.insert(inactive);
        assert_eq!(
            f.context()
                .verify(&root(&f.previous), &f.previous, &f.current[..1])
                .err(),
            Some(CementError::ShortOfQuorum)
        );
        let verified = f
            .context()
            .verify(&root(&f.previous), &f.previous, &f.current)
            .unwrap();
        assert_eq!(
            verified.confirmers.len(),
            2,
            "returning presence can be included"
        );
        assert_eq!(verified.cemented_hashes, vec![REVEAL]);
    }

    #[test]
    fn malformed_and_extended_envelopes_are_not_accepted() {
        let f = Fixture::new();
        let bytes = envelope(&f.previous[..1], &f.current[..1]);
        assert!(parse_envelope_bundles(&bytes).is_some());
        for size in 0..bytes.len() {
            assert!(
                parse_envelope_bundles(&bytes[..size]).is_none(),
                "prefix {size}"
            );
        }
        let mut extended = bytes;
        extended.push(0);
        assert!(parse_envelope_bundles(&extended).is_none());
        let mut huge_count = vec![0; PROPOSAL_HEADER_SIZE];
        huge_count.extend_from_slice(&u16::MAX.to_le_bytes());
        assert!(parse_envelope_bundles(&huge_count).is_none());
    }

    #[test]
    fn empty_or_overflowing_total_does_not_close() {
        let mut f = Fixture::new();
        for (id, _) in &f.keys {
            let mut node = f.nodes.get(id).unwrap().clone();
            node.chain_length = u64::MAX;
            f.nodes.insert(node);
        }
        assert_eq!(
            f.context()
                .verify(&root(&f.previous), &f.previous, &f.current)
                .err(),
            Some(CementError::WeightOverflow)
        );
        f.nodes = NodeTable::new();
        assert_eq!(
            f.context().verify(&root(&[]), &[], &[]).err(),
            Some(CementError::EmptyActiveSet)
        );
    }

    #[test]
    fn genesis_exception_does_not_waive_current_quorum() {
        let mut f = Fixture::new();
        f.current[0] = signed(f.keys[0].0, &f.keys[0].1, 1, CURRENT);
        let mut context = f.context();
        context.window = 1;
        context.previous_endpoint = None;
        assert!(context.verify(&root(&[]), &[], &f.current[..1]).is_ok());
        assert_eq!(
            context.verify(&root(&[]), &[], &[]).err(),
            Some(CementError::ShortOfQuorum)
        );
    }
}
