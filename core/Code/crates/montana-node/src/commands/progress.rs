//! Passive observations of the legacy cement accumulator. The local clock only
//! schedules log output; neither these values nor their age authorize a fold.

use std::collections::BTreeMap;
use std::io::Write;
use std::time::{Duration, Instant};

use mt_crypto::Hash32;
use mt_genesis::ProtocolParams;
use mt_lottery::{quorum, BundledConfirmation};
use mt_state::{NodeId, NodeTable};

use super::cement::{accumulated_standing_at, active_chain_length_at, CementError};

const REPORT_INTERVAL: Duration = Duration::from_secs(10);

pub(super) struct ProgressInput<'a> {
    pub head: u64,
    pub nodes: &'a NodeTable,
    pub params: &'a ProtocolParams,
    pub endpoints: &'a BTreeMap<u64, Hash32>,
    pub bundles: &'a BTreeMap<u64, BTreeMap<NodeId, BundledConfirmation>>,
}

#[derive(Debug, PartialEq, Eq)]
struct Weights {
    total: u64,
    required: u64,
    previous: u64,
    current: u64,
}

impl ProgressInput<'_> {
    fn weights(&self, next: u64) -> Result<Weights, CementError> {
        let total = active_chain_length_at(self.nodes, next, self.params)?;
        Ok(Weights {
            total,
            required: quorum(total),
            previous: accumulated_standing_at(
                self.bundles.get(&self.head),
                self.head,
                next,
                self.nodes,
                self.params,
            )?,
            current: accumulated_standing_at(
                self.bundles.get(&next),
                next,
                next,
                self.nodes,
                self.params,
            )?,
        })
    }
}

#[derive(Default)]
pub(super) struct CementProgress {
    // A hash always carries the window it belongs to. An archive gap or a
    // snapshot without an envelope must not label an older hash as the new head.
    envelope: Option<(u64, Hash32)>,
    observed_head: Option<u64>,
    observed_since: Option<Instant>,
    reported_at: Option<Instant>,
}

impl CementProgress {
    pub fn record_envelope(&mut self, window: u64, hash: Hash32) {
        self.envelope = Some((window, hash));
    }

    /// No return value enters consensus. An observation failure becomes a
    /// diagnostic field rather than permission to close or a node error.
    pub fn observe(&mut self, input: ProgressInput<'_>) {
        if let Some(line) = self.report(&input, Instant::now()) {
            // A closed diagnostic stream must not panic in the consensus loop.
            let _ = writeln!(std::io::stderr().lock(), "{line}");
        }
    }

    fn report(&mut self, input: &ProgressInput<'_>, now: Instant) -> Option<String> {
        if self.observed_head != Some(input.head) {
            self.observed_head = Some(input.head);
            self.observed_since = Some(now);
            self.reported_at = None;
        }
        if self
            .reported_at
            .is_some_and(|at| now.saturating_duration_since(at) < REPORT_INTERVAL)
        {
            return None;
        }
        self.reported_at = Some(now);
        let age = now
            .saturating_duration_since(self.observed_since.unwrap_or(now))
            .as_secs();
        let hash = self
            .envelope
            .filter(|(w, _)| *w == input.head)
            .map(|(_, hash)| format!("\"{}\"", hex(&hash)))
            .unwrap_or_else(|| "null".into());
        let next = input.head.checked_add(1);
        let weights = next.and_then(|next| input.weights(next).ok());
        let current_endpoint = next.is_some_and(|w| input.endpoints.contains_key(&w));
        let previous_endpoint = input.head == 0 || input.endpoints.contains_key(&input.head);
        let phase = match (&weights, next) {
            (_, None) => "window_exhausted",
            (None, _) => "invalid_accumulator",
            (Some(w), _) if w.total == 0 => "no_active_standing",
            _ if !previous_endpoint => "missing_previous_endpoint",
            _ if !current_endpoint => "awaiting_current_endpoint",
            (Some(w), _) if input.head != 0 && w.previous < w.required => "short_previous_quorum",
            (Some(w), _) if w.current < w.required => "short_current_quorum",
            _ => "awaiting_verified_proposal",
        };
        let short = weights.as_ref().is_some_and(|w| {
            w.total == 0 || (input.head != 0 && w.previous < w.required) || w.current < w.required
        });
        let next_json = next.map(|w| w.to_string()).unwrap_or_else(|| "null".into());
        let weights_json = match weights {
            Some(w) => format!(
                "\"active_standing\":{},\"required_standing\":{},\"previous_standing\":{},\"current_standing\":{}",
                w.total, w.required, w.previous, w.current,
            ),
            None => "\"active_standing\":null,\"required_standing\":null,\"previous_standing\":null,\"current_standing\":null".into(),
        };
        Some(format!(
            "{{\"event\":\"legacy_cement_progress\",\"version\":1,\"local_head_window\":{},\"local_head_hash\":{},\"awaited_window\":{},\"observed_head_age_seconds\":{},\"short_of_quorum_at_window\":{},\"previous_endpoint_known\":{},\"current_endpoint_known\":{},\"phase\":\"{}\",{}}}",
            input.head, hash, next_json, age, if short { &next_json } else { "null" },
            previous_endpoint, current_endpoint, phase, weights_json,
        ))
    }
}

fn hex(hash: &Hash32) -> String {
    hash.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mt_crypto::{Signature, SIGNATURE_SIZE};
    use mt_state::NodeRecord;

    const HEAD: u64 = 100_000;

    struct Fixture {
        nodes: NodeTable,
        endpoints: BTreeMap<u64, Hash32>,
        bundles: BTreeMap<u64, BTreeMap<NodeId, BundledConfirmation>>,
    }

    impl Fixture {
        fn new() -> Self {
            let mut nodes = NodeTable::new();
            for (id, standing, last) in [(1, 67, HEAD), (2, 33, HEAD), (3, 500, 0)] {
                nodes.insert(NodeRecord {
                    node_id: [id; 32],
                    node_pubkey: [0; mt_crypto::PUBLIC_KEY_SIZE],
                    suite_id: 1,
                    operator_account_id: [id; 32],
                    start_window: 0,
                    chain_length: standing,
                    chain_length_snapshot: standing,
                    chain_length_checkpoints: [standing; 6],
                    last_confirmation_window: last,
                });
            }
            Self {
                nodes,
                endpoints: [(HEAD, [4; 32]), (HEAD + 1, [5; 32])].into(),
                bundles: BTreeMap::new(),
            }
        }

        // Synthetic entries exercise observation only; this test does not claim
        // that they pass the cryptographic gate which owns the real accumulator.
        fn add(&mut self, window: u64, id: u8) {
            self.bundles.entry(window).or_default().insert(
                [id; 32],
                BundledConfirmation {
                    node_id: [id; 32],
                    window_index: window,
                    endpoint: [5; 32],
                    op_hashes: vec![],
                    reveal_hashes: vec![],
                    signature: Signature::from_array([0; SIGNATURE_SIZE]),
                },
            );
        }

        fn input(&self) -> ProgressInput<'_> {
            ProgressInput {
                head: HEAD,
                nodes: &self.nodes,
                params: mt_genesis::genesis_params(),
                endpoints: &self.endpoints,
                bundles: &self.bundles,
            }
        }
    }

    #[test]
    fn a_minority_cannot_hide_behind_duplicates_or_inactive_weight() {
        let mut f = Fixture::new();
        for _ in 0..3 {
            f.add(HEAD + 1, 2);
        }
        f.add(HEAD + 1, 3);
        f.add(HEAD, 1);
        assert_eq!(
            f.input().weights(HEAD + 1).unwrap(),
            Weights {
                total: 100,
                required: 67,
                previous: 67,
                current: 33
            }
        );
        let line = CementProgress::default()
            .report(&f.input(), Instant::now())
            .unwrap();
        assert!(line.contains("\"phase\":\"short_current_quorum\""));
        assert!(!line.contains(&hex(&[2; 32])));
        assert!(!line.contains(&hex(&[3; 32])));
    }

    #[test]
    fn both_quorums_are_observed_but_neither_claims_a_closed_window() {
        let mut f = Fixture::new();
        f.add(HEAD + 1, 1);
        let now = Instant::now();
        let mut monitor = CementProgress::default();
        assert!(monitor
            .report(&f.input(), now)
            .unwrap()
            .contains("short_previous_quorum"));
        f.add(HEAD, 1);
        let line = monitor.report(&f.input(), now + REPORT_INTERVAL).unwrap();
        assert!(line.contains("awaiting_verified_proposal"));
        assert!(line.contains("\"local_head_window\":100000"));
    }

    #[test]
    fn repeated_messages_do_not_reset_age_and_only_a_new_head_does() {
        let mut f = Fixture::new();
        let now = Instant::now();
        let mut monitor = CementProgress::default();
        monitor.record_envelope(HEAD, [7; 32]);
        assert!(monitor
            .report(&f.input(), now)
            .unwrap()
            .contains(&hex(&[7; 32])));
        for second in 1..=35 {
            f.add(HEAD + 1, 2);
            let report = monitor.report(&f.input(), now + Duration::from_secs(second));
            if second % 10 == 0 {
                assert!(report
                    .unwrap()
                    .contains(&format!("\"observed_head_age_seconds\":{second}")));
            } else {
                assert!(report.is_none());
            }
        }
        let mut input = f.input();
        input.head += 1;
        let line = monitor
            .report(&input, now + Duration::from_secs(36))
            .unwrap();
        assert!(line.contains("\"observed_head_age_seconds\":0"));
        assert!(line.contains("\"local_head_hash\":null"));
    }

    #[test]
    fn empty_population_and_inconsistent_accumulator_are_visible() {
        let mut f = Fixture::new();
        f.nodes = NodeTable::new();
        assert!(CementProgress::default()
            .report(&f.input(), Instant::now())
            .unwrap()
            .contains("no_active_standing"));
        f.add(HEAD + 1, 1);
        assert!(CementProgress::default()
            .report(&f.input(), Instant::now())
            .unwrap()
            .contains("invalid_accumulator"));
        f = Fixture::new();
        f.add(HEAD + 1, 1);
        f.bundles
            .get_mut(&(HEAD + 1))
            .unwrap()
            .get_mut(&[1; 32])
            .unwrap()
            .window_index = HEAD;
        assert!(CementProgress::default()
            .report(&f.input(), Instant::now())
            .unwrap()
            .contains("invalid_accumulator"));
        f.bundles
            .get_mut(&(HEAD + 1))
            .unwrap()
            .get_mut(&[1; 32])
            .unwrap()
            .window_index = HEAD + 1;
        f.bundles
            .get_mut(&(HEAD + 1))
            .unwrap()
            .get_mut(&[1; 32])
            .unwrap()
            .node_id = [2; 32];
        assert!(CementProgress::default()
            .report(&f.input(), Instant::now())
            .unwrap()
            .contains("invalid_accumulator"));
    }

    #[test]
    fn missing_endpoints_and_exhausted_window_are_not_reported_as_quorum() {
        let mut f = Fixture::new();
        f.add(HEAD, 1);
        f.add(HEAD + 1, 1);
        f.endpoints = BTreeMap::new();
        let now = Instant::now();
        assert!(CementProgress::default()
            .report(&f.input(), now)
            .unwrap()
            .contains("missing_previous_endpoint"));
        f.endpoints.insert(HEAD, [4; 32]);
        assert!(CementProgress::default()
            .report(&f.input(), now)
            .unwrap()
            .contains("awaiting_current_endpoint"));
        let mut input = f.input();
        input.head = u64::MAX;
        let line = CementProgress::default().report(&input, now).unwrap();
        assert!(line.contains("window_exhausted"));
        assert!(line.contains("\"awaited_window\":null"));
    }

    #[test]
    fn overflowing_standing_is_unknown_instead_of_a_wrapped_majority() {
        let mut f = Fixture::new();
        let mut node = f.nodes.get(&[1; 32]).unwrap().clone();
        node.chain_length = u64::MAX;
        f.nodes.insert(node);
        f.add(HEAD + 1, 1);
        f.add(HEAD + 1, 2);
        assert_eq!(
            accumulated_standing_at(
                f.bundles.get(&(HEAD + 1)),
                HEAD + 1,
                HEAD + 1,
                &f.nodes,
                mt_genesis::genesis_params()
            ),
            Err(CementError::WeightOverflow)
        );
        let line = CementProgress::default()
            .report(&f.input(), Instant::now())
            .unwrap();
        assert!(line.contains("invalid_accumulator"));
        assert!(line.contains("\"active_standing\":null"));
    }
}
