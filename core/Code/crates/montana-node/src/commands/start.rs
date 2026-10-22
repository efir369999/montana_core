use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use mt_account::ProposalSettle;
use mt_codec::CanonicalEncode;
use mt_consensus::{proposal_hash, ProposalHeader, PROPOSAL_HEADER_SIZE};
use mt_crypto::{sign, Hash32, Signature, SIGNATURE_SIZE};
use mt_entry::{
    apply_noderegistrations_batch, apply_selection_event, candidate_ssha_init, nodereg_hash,
    validate_noderegistration, NodeRegistration,
};
use mt_genesis::genesis_params;
use mt_lottery::{
    bundle_hash, compute_endpoint, quorum, reveal_hash, validate_bundle, validate_reveal,
    weighted_ticket_node, BundledConfirmation, SshaReveal,
};
use mt_merkle::{empty_internal, TREE_DEPTH};
use mt_net::{MsgType, ProtocolMessage};
use mt_state::compute_state_root;
use mt_store::FsStore;
use mt_timechain::{cemented_bundle_aggregate, next_d, ssha_step};

use crate::clock::{load_current_window, save_current_window};
use crate::commands::cement::{
    accumulated_standing_at, active_chain_length_at, meta_root, parse_envelope_bundles,
    CementContext,
};
use crate::commands::fastsync::FastSyncSession;
use crate::commands::lottery::{complete, LotteryError};
use crate::commands::progress::{CementProgress, ProgressInput};
use crate::commands::settlement::{prepare, StateRoots};
use crate::identity::{default_data_dir, load_identity, NodeError};
use crate::node_lifecycle::{load_or_init_lifecycle, save_lifecycle, NodeLifecycle, NodePhase};
use crate::state::LocalState;
use crate::timechain_state::{load_or_init_timechain, save_timechain, TimeChainState};

static STOP: AtomicBool = AtomicBool::new(false);

extern "C" fn shutdown_handler(_: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

// M7 fast-sync trigger threshold (network-layer implementation guidance per
// Network spec — not consensus-critical, may vary between implementations).
// Replay costs ~6 min / 1000 windows on 1 vCPU (mt-sync lib doc); beyond this
// lag snapshot delivery is bandwidth-bound and cheaper than apply_proposal loop.
const FAST_SYNC_LAG_THRESHOLD: u64 = 64;

// Operational override of the fast-sync lag threshold via env. The threshold is
// network-layer (not consensus); operators tune it for deployment/observation.
// Empty, unparsable, or zero values fall back to the production default.
fn resolve_fast_sync_lag_threshold(override_val: Option<String>) -> u64 {
    override_val
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|&t| t > 0)
        .unwrap_or(FAST_SYNC_LAG_THRESHOLD)
}

pub struct StartArgs {
    pub data_dir: Option<PathBuf>,
    pub max_windows: Option<u64>,
    pub d_test_override: Option<u64>,
    /// Multiaddr for libp2p listen (for example `/ip4/0.0.0.0/tcp/8444`).
    /// When present — the node starts the cross-machine peering thread.
    pub listen_multiaddr: Option<String>,
    /// Path to genesis-manifest.json with the peer list for bootstrap connectivity.
    /// Required when `--listen` is present.
    pub genesis_manifest: Option<PathBuf>,
    /// Enable the candidate flow (registration → Active via the lottery). Off
    /// by default: new nodes only sync and send a heartbeat
    /// (observers), consensus is fixed on the genesis operators. "for now".
    pub enable_candidate: bool,
    /// Enable the name layer. OFF by default: until the subsystem is verified live,
    /// the node behaves exactly as before it appeared — it assembles no batches, builds no index,
    /// answers no requests.
    pub enable_names: bool,
}

pub fn run(args: StartArgs) -> Result<(), NodeError> {
    if args.listen_multiaddr.is_some() != args.genesis_manifest.is_some() {
        return Err(NodeError::InvalidArguments(
            "listen and genesis manifest must be provided together".into(),
        ));
    }
    let data_dir = args.data_dir.unwrap_or_else(default_data_dir);
    let identity = load_identity(&data_dir)?;
    let params = genesis_params();

    // Cross-machine M8: spawn a network thread with its own tokio runtime.
    // The network event loop is separate from the consensus loop (SSHA compute) —
    // a separate OS thread prevents CPU-heavy SSHA computation
    // from blocking async tasks.
    // Name layer: created by the flag and, when off, entirely inert.
    let names = if args.enable_names {
        eprintln!("[names] name layer ENABLED");
        crate::names::NamesService::enabled()
    } else {
        crate::names::NamesService::disabled()
    };
    debug_assert!(args.enable_names || !names.is_enabled());
    let _names = names;

    let mut network_handle: Option<NetworkHandle> = None;
    if let (Some(listen_str), Some(manifest_path)) =
        (&args.listen_multiaddr, &args.genesis_manifest)
    {
        network_handle = Some(spawn_network_thread(&identity, listen_str, manifest_path)?);
    }

    // spec, Genesis Decree: Genesis = empty window 0. The genesis state is empty
    // (no baked active operators); a node self-admits via the existing admission
    // path. The manifest is discovery-only and is not hash-bound.
    let mut state = LocalState::load_or_bootstrap(&data_dir, &identity, params)?;
    // Persist the seeded genesis state immediately so a separate `status`
    // invocation reads the real bootstrapped tables instead of re-bootstrapping
    // an incomplete view.
    if !data_dir.join("accounts.bin").exists() {
        state.save(&data_dir)?;
    }
    let mut current = load_current_window(&data_dir)?;
    let mut timechain = load_or_init_timechain(&data_dir)?;
    let mut lifecycle = load_or_init_lifecycle(&data_dir, &identity, params)?;
    let mut effective_d = args.d_test_override.unwrap_or(timechain.current_d);

    if let Some(d) = args.d_test_override {
        eprintln!(
            "WARNING: --d-test-override={d} is active. This is a test-only mode. \
             In production D = params.d0 = {} iterations (Genesis Decree).",
            params.d0
        );
        timechain.current_d = d;
    }

    install_shutdown_handlers();

    let stop_at = args.max_windows.map(|n| current.saturating_add(n));

    let my_account = identity.account_id();
    let my_node = identity.node_id();
    let initial_balance = state
        .accounts
        .get(&my_account)
        .map(|a| a.balance)
        .unwrap_or(0);

    // Genesis = empty window 0: no baked bootstrap node. A node is considered already
    // active only if its node_id is present in NodeTable (it was admitted
    // in a previous session and the state is loaded from disk). A fresh node is a
    // candidate: self-admit via the standard admission path (selection_slots(0)=1
    // admits the first candidate when the Active set is empty).
    let already_active = state.nodes.contains(&my_node);

    // Phase Bootstrap = first boot. Already active (in NodeTable) → Active.
    // Others → CandidateSsha with target_chain_length = τ₂ and w_start = current
    // window + 1.
    if lifecycle.phase == NodePhase::Bootstrap {
        if already_active {
            lifecycle.phase = NodePhase::Active;
        } else if args.enable_candidate {
            lifecycle.phase = NodePhase::CandidateSsha;
            lifecycle.target_chain_length = params.ssha_entry_windows;
            lifecycle.w_start = current.saturating_add(1);
            lifecycle.candidate_progress = 0;
            // candidate_endpoint starts with T_r of the current timechain — this is the
            // canonical seed for the chain start; on each window it ticks via
            // ssha_step_chunked in the Active phase code path below.
            lifecycle.candidate_endpoint = timechain.t_r;
        } else {
            // Candidates disabled ("for now"): a non-genesis node is a pure observer —
            // Registered is a no-op phase in the consensus loop, so the node only
            // fast-syncs the canonical chain and emits heartbeats. No SSHA ticking,
            // no NodeRegistration. Flip with --enable-candidate to rejoin admission.
            lifecycle.phase = NodePhase::Registered;
        }
        save_lifecycle(&data_dir, &lifecycle)?;
    }

    println!("=== montana-node start — Montana node ===");
    println!();
    println!("data-dir         : {}", data_dir.display());
    println!("account_id       : {}", hex16(&my_account));
    println!("node_id          : {}", hex16(&my_node));
    println!("phase            : {:?}", lifecycle.phase);
    println!("current_window   : {current}");
    println!(
        "D                : {} SHA-256 iterations / window",
        effective_d
    );
    println!("T_r              : {}", hex16(&timechain.t_r));
    println!(
        "balance start    : {} nɈ ({} Ɉ)",
        initial_balance,
        initial_balance / 1_000_000_000
    );
    if let Some(stop) = stop_at {
        println!(
            "stop_at          : window {stop} (in {} windows)",
            stop - current
        );
    } else {
        println!("stop_at          : Ctrl-C");
    }
    if lifecycle.phase == NodePhase::CandidateSsha {
        let remaining = lifecycle
            .target_chain_length
            .saturating_sub(lifecycle.candidate_progress);
        println!(
            "candidate SSHA    : progress {}/{}, {} windows left to registration",
            lifecycle.candidate_progress, lifecycle.target_chain_length, remaining
        );
    }
    println!();
    println!("--- SSHA ticking ---");
    println!();

    let session_start = Instant::now();
    let mut session_emitted: u128 = 0;
    let mut session_windows: u64 = 0;
    let mut prev_proposal_hash: Hash32 = [0u8; 32];

    let store = FsStore::open(&data_dir)
        .map_err(|e| NodeError::InvalidArguments(format!("FsStore::open: {e:?}")))?;

    // DEV-012 T_r history: per-window T_r snapshot for BC endpoint validation
    // when BCs arrive after current has advanced.
    let mut t_r_history: BTreeMap<u64, Hash32> = BTreeMap::new();
    // DEV-023: track per-proposer last cemented window so each node can decide
    // when an elected proposer has gone silent (≥ K_FALLBACK_WINDOWS windows
    // without producing cement). A solo node falls back to itself.
    let mut last_proposer_cement: BTreeMap<mt_state::NodeId, u64> = BTreeMap::new();
    #[allow(dead_code)]
    const K_FALLBACK_WINDOWS: u64 = 3;
    // DEV-022 Lookback Leadership: track winner_id per cemented window so any
    // Active node can compute proposer_W = winner_{W-2} for its own window
    // decisions. Genesis = empty window 0: with empty W-2 there is no canonical
    // proposer — a lone node submits its own proposal (self-admit + self-cement).
    let mut winner_history: BTreeMap<u64, mt_state::NodeId> = BTreeMap::new();
    // DEV-020: per-window reveal pool, keyed by (window_index → (node_id → SshaReveal)).
    // All Active nodes publish their own Reveal every window via MsgType::SshaReveal.
    // The proposer at cement-time collects the cemented Reveals (those whose reveal_hash entered the
    // 67% chain_length of BC) and computes winner = argmin(weighted_ticket_node) per spec.
    let mut reveal_pool: BTreeMap<u64, BTreeMap<mt_state::NodeId, SshaReveal>> = BTreeMap::new();

    // DEV-012 multi-confirmer: per-window accumulator of BCs from Active peers.
    // Keyed by window then node_id so duplicates from same node deduplicate.
    let mut bc_accumulator: BTreeMap<u64, BTreeMap<mt_state::NodeId, BundledConfirmation>> =
        BTreeMap::new();

    // M7 fast-sync: held across loop iterations while a snapshot is in flight.
    let fast_sync_lag_threshold =
        resolve_fast_sync_lag_threshold(std::env::var("MONTANA_FASTSYNC_LAG_THRESHOLD").ok());
    // Networked mode only: a solo node has no peers to lag behind, so the
    // fast-sync threshold is meaningless noise in the singleton banner.
    if network_handle.is_some() {
        println!("fast-sync lag    : threshold {fast_sync_lag_threshold} windows");
    }
    let mut fast_sync: Option<FastSyncSession> = None;
    let mut fast_sync_deadline: Option<Instant> = None;
    // M7 fast-sync: recent cemented bootstrap state_roots (window -> root),
    // the trusted set a reconstructed snapshot root must match.
    let mut recent_roots: BTreeMap<u64, Hash32> = BTreeMap::new();

    // Per-window consensus histories (bounded by HISTORY_BOUND windows).
    let mut lottery_history: BTreeMap<u64, Vec<mt_lottery::Candidate>> = BTreeMap::new();
    let mut bc_set_history: BTreeMap<u64, Vec<mt_state::NodeId>> = BTreeMap::new();
    let mut own_bc_cache: BTreeMap<u64, BundledConfirmation> = BTreeMap::new();
    let mut pending_msgs: Vec<ProtocolMessage> = Vec::new();
    let mut last_cement_at = Instant::now();
    let mut cement_progress = CementProgress::default();
    // Re-broadcast of the last cemented envelope after a restart:
    // if the process died between cementing and delivery, followers will receive the
    // envelope now (idempotent: a redundant envelope is rejected by monotonicity).
    if current >= 1 {
        if let Ok(Some(env)) = store.load_proposal_envelope(current) {
            if let Some(ref handle) = network_handle {
                let _ =
                    handle
                        .broadcast_tx
                        .send(ProtocolMessage::new(MsgType::Proposal, current, env));
                eprintln!("[consensus] replaying archived envelope w={current} after restart");
            }
        }
    }
    // Own saved clock — the canonical value of its last
    // turn: without this record the node after a restart cannot republish
    // the artifacts of a not-yet-cemented window (the archive is empty in the first windows).
    if timechain.last_window >= 1 {
        t_r_history.insert(timechain.last_window, timechain.t_r);
    }
    // Restoring histories from the archive of cemented envelopes after a
    // restart: the set of confirmers, winners, clock values.
    for w in current.saturating_sub(8)..=current {
        if let Ok(Some(env)) = store.load_proposal_envelope(w) {
            if let Some(h) = parse_header(&env) {
                t_r_history.insert(w, h.timechain_value);
                if w >= 1 {
                    winner_history.insert(w - 1, h.winner_id);
                }
                if let Some((bundles_prev, _evidence)) = parse_envelope_bundles(&env) {
                    if w >= 1 {
                        let mut ids: Vec<mt_state::NodeId> =
                            bundles_prev.iter().map(|b| b.node_id).collect();
                        ids.sort();
                        bc_set_history.insert(w - 1, ids);
                    }
                }
                prev_proposal_hash = proposal_hash(&h);
                if w == current {
                    cement_progress.record_envelope(w, prev_proposal_hash);
                }
            }
        }
    }

    // Duration of the previous turn — the basis of the adaptive liveness grace.
    let mut last_tick_dur = Duration::from_secs(30);
    // Neighbour liveness by the fact of receiving their messages.
    loop {
        // Effective D is recomputed every turn (adaptation at the τ₂ boundary
        // happens in the single state transition settle_and_bookkeep).
        effective_d = args.d_test_override.unwrap_or(timechain.current_d);
        macro_rules! net_ctx {
            () => {
                NetCtx {
                    state: &mut state,
                    current: &mut current,
                    timechain: &mut timechain,
                    recent_roots: &mut recent_roots,
                    t_r_history: &mut t_r_history,
                    reveal_pool: &mut reveal_pool,
                    bc_accumulator: &mut bc_accumulator,
                    winner_history: &mut winner_history,
                    lottery_history: &mut lottery_history,
                    bc_set_history: &mut bc_set_history,
                    last_proposer_cement: &mut last_proposer_cement,
                    own_bc_cache: &mut own_bc_cache,
                    pending_msgs: &mut pending_msgs,
                    fast_sync: &mut fast_sync,
                    fast_sync_deadline: &mut fast_sync_deadline,
                    last_cement_at: &mut last_cement_at,
                    cement_progress: &mut cement_progress,
                    prev_proposal_hash: &mut prev_proposal_hash,
                    fast_sync_lag_threshold,
                    fallback_secs: (last_tick_dur.as_secs() * 2).max(3),
                    data_dir: &data_dir,
                    params,
                    store: &store,
                    identity: &identity,
                    my_node,
                }
            };
        }
        if let Some(ref mut handle) = network_handle {
            let mut ctx = net_ctx!();
            drain_network(&mut ctx, handle)?;
        }

        if STOP.load(Ordering::SeqCst) {
            println!();
            println!("[shutdown] received SIGINT/SIGTERM, saving state...");
            break;
        }
        if let Some(stop) = stop_at {
            if current >= stop {
                println!();
                println!("[stop_at] reached target window {stop}");
                break;
            }
        }

        // The window the network closes next.
        let next_window = current + 1;

        if timechain.last_window < next_window {
            // Spec "continuity of the sequential SHA-256 chain (SSHA)": the chain of the next window
            // is computed continuously; finalization and ticket intake run
            // in parallel — network draining between portions of the turn.
            let tick_seed = timechain.t_r;
            let tick_t0 = Instant::now();
            let next_t_r = ssha_step_chunked(
                &tick_seed,
                effective_d,
                "TimeChain SSHA",
                next_window,
                || {
                    if let Some(ref mut handle) = network_handle {
                        let mut ctx = net_ctx!();
                        drain_network(&mut ctx, handle)?;
                    }
                    Ok(())
                },
            )?;
            last_tick_dur = tick_t0.elapsed();
            // Reconcile with the network value if the window was already seen in an envelope
            // (or drain managed to apply it in the middle of our turn).
            if let Some(known) = t_r_history.get(&next_window) {
                if *known != next_t_r {
                    return Err(NodeError::InvalidArguments(format!(
                        "timechain divergence at window {next_window}: network != local"
                    )));
                }
            }
            if timechain.last_window < next_window {
                timechain.t_r = next_t_r;
                timechain.last_window = next_window;
            }
            t_r_history.insert(next_window, next_t_r);
            bound_map(&mut t_r_history);
        } else {
            // This window's turn is already computed; wait for cementing from the network.
            std::thread::sleep(Duration::from_millis(50));
        }

        // Every Active node publishes the window artifacts from the canonical clock:
        // the ticket (SSHA_Reveal) of window next_window + the confirmation
        // (BundledConfirmation) with the ticket hashes of the previous window. The ticket of window
        // next_window is cemented by the evidence of window next_window+1, so
        // publication is appropriate even when an envelope from the network advanced the clock.
        if lifecycle.phase == NodePhase::Active
            && state.nodes.get(&my_node).is_some()
            && !own_bc_cache.contains_key(&next_window)
        {
            if let Some(t_r_w) = t_r_history.get(&next_window).copied() {
                publish_window_artifacts(
                    &state,
                    &mut reveal_pool,
                    &mut bc_accumulator,
                    &mut own_bc_cache,
                    &bc_set_history,
                    &identity,
                    my_node,
                    next_window,
                    &t_r_w,
                    timechain.lottery_target,
                    network_handle.as_ref().map(|h| &h.broadcast_tx),
                )?;
            }
        }

        if current >= next_window {
            save_progress(&data_dir, &state, &timechain, &lifecycle, current)?;
            continue;
        }

        match lifecycle.phase {
            NodePhase::Bootstrap => unreachable!("Bootstrap → CandidateSsha transition above"),
            NodePhase::CandidateSsha => {
                lifecycle.candidate_endpoint = ssha_step_chunked(
                    &lifecycle.candidate_endpoint,
                    effective_d,
                    "Candidate SSHA",
                    next_window,
                    || {
                        if let Some(ref handle) = network_handle {
                            handle.ensure_alive()?;
                        }
                        Ok(())
                    },
                )?;
                lifecycle.candidate_progress = lifecycle
                    .candidate_progress
                    .checked_add(1)
                    .expect("candidate_progress overflow at u64::MAX");

                if lifecycle.candidate_progress >= lifecycle.target_chain_length {
                    let cba_w_start_minus_2 =
                        cba_from(&bc_set_history, lifecycle.w_start.saturating_sub(2));
                    let proof_endpoint =
                        candidate_ssha_init(&timechain.t_r, &cba_w_start_minus_2, &my_node);

                    let mut nr = NodeRegistration {
                        suite_id: identity.suite_id as u16,
                        node_pubkey: *identity.node_pk.as_bytes(),
                        operator_account_id: my_account,
                        proof_endpoint,
                        w_start: lifecycle.w_start,
                        ssha_chain_length: lifecycle.candidate_progress,
                        signature: Signature::from_array([0u8; SIGNATURE_SIZE]),
                    };
                    let mut scope = Vec::new();
                    nr.encode_signed_scope(&mut scope);
                    nr.signature = sign(&identity.node_sk, &scope).map_err(NodeError::Crypto)?;

                    validate_noderegistration(
                        &nr,
                        &state.nodes,
                        &state.candidates,
                        &state.accounts,
                    )
                    .map_err(|e| {
                        NodeError::InvalidArguments(format!("validate_noderegistration: {e:?}"))
                    })?;

                    let pending_baseline = state.candidates.len() as u64;
                    let active_nodes = state.nodes.len() as u64;
                    let cba_w_p_minus_2 = cba_from(&bc_set_history, next_window.saturating_sub(2));
                    let outcome = apply_noderegistrations_batch(
                        &mut state.candidates,
                        &[nr.clone()],
                        &timechain.t_r,
                        &cba_w_p_minus_2,
                        pending_baseline,
                        active_nodes,
                        next_window,
                        params,
                    );
                    if outcome.applied.len() != 1 {
                        return Err(NodeError::InvalidArguments(format!(
                            "apply_noderegistrations_batch: applied={}, rejected={}, expected applied=1",
                            outcome.applied.len(),
                            outcome.rejected.len()
                        )));
                    }
                    lifecycle.nodereg_hash = nodereg_hash(&nr);
                    lifecycle.registration_window = next_window;
                    lifecycle.phase = NodePhase::Registered;
                    println!(
                        "[register W={next_window}] nodereg_hash={} | ssha_chain_length={}",
                        hex16(&lifecycle.nodereg_hash),
                        lifecycle.candidate_progress
                    );
                }
            },
            NodePhase::Registered => {
                // Cold-start self-admission: with an empty Active set (genesis, no
                // Active operator at all) a registered node itself conducts the
                // selection_event at the selection window. selection_slots(0)=1
                // admits it (the only candidate) → Node Table; then the
                // Active arm drives the windows itself (its own BC gives quorum at active=0/1).
                // If the Active set is NOT empty — the node waits for ordinary admission
                // by the existing Active (a no-op here). This is the bootstrap of the first node
                // of a solo chain; admission-as-cemented-proposal for multi-node is
                // a separate path (see the critic's cold-start finding).
                if state.nodes.is_empty()
                    && !state.candidates.is_empty()
                    && next_window % params.selection_interval == 0
                {
                    let cba_w_minus_2 = cba_from(&bc_set_history, next_window.saturating_sub(2));
                    let t_r_w = *t_r_history.get(&next_window).unwrap_or(&timechain.t_r);
                    let admitted = apply_selection_event(
                        &mut state.candidates,
                        &mut state.nodes,
                        &mut state.accounts,
                        &t_r_w,
                        &cba_w_minus_2,
                        0,
                        next_window,
                        params,
                    );
                    if state.nodes.contains(&my_node) {
                        lifecycle.phase = NodePhase::Active;
                        last_cement_at = Instant::now();
                        println!(
                            "[cold-start W={next_window}] self-admit \u{2192} Active (admitted={})",
                            admitted.len()
                        );
                    }
                }
            },
            NodePhase::Active => 'active_arm: {
                // Legitimate proposer of window next_window: the winner of window
                // next_window-2; on silence — a cascade of fallbacks in
                // ascending weighted ticket (spec Lookback).
                let silence = last_cement_at.elapsed().as_secs();
                let (acting, my_depth, canonical_proposer) = {
                    let ctx = net_ctx!();
                    let (acting, depth) = expected_proposer(&ctx, next_window, silence);
                    let (canonical, _) = canonical_proposer_at(&ctx, next_window, silence);
                    (acting, depth, canonical)
                };
                if acting != my_node {
                    break 'active_arm;
                }
                let propose_w = next_window;
                if my_depth > 1 {
                    eprintln!(
                        "[lookback W={propose_w}] acting as fallback leader (depth {my_depth})"
                    );
                }

                // Candidate notification: the signal "window propose_w is being assembled"
                // + the canonical clock value for lagging nodes.
                let t_r_w = *t_r_history.get(&propose_w).unwrap_or(&timechain.t_r);
                let notify = {
                    let mut h = ProposalHeader {
                        prev_proposal_hash,
                        window_index: propose_w,
                        protocol_version: 1,
                        control_root: empty_internal(TREE_DEPTH),
                        node_root: [0u8; 32],
                        candidate_root: state.candidates.root(),
                        account_root: [0u8; 32],
                        state_root: [0u8; 32],
                        timechain_value: t_r_w,
                        included_bundles_root: [0u8; 32],
                        included_reveals_root: [0u8; 32],
                        winner_endpoint: [0u8; 32],
                        winner_id: my_node,
                        proposer_node_id: canonical_proposer,
                        target: timechain.lottery_target,
                        fallback_depth: my_depth,
                        signature: Signature::from_array([0u8; SIGNATURE_SIZE]),
                    };
                    let mut scope = Vec::new();
                    h.encode_signed_scope(&mut scope);
                    h.signature = sign(&identity.node_sk, &scope).map_err(NodeError::Crypto)?;
                    let mut bytes = Vec::with_capacity(PROPOSAL_HEADER_SIZE);
                    h.encode(&mut bytes);
                    bytes
                };
                if let Some(ref handle) = network_handle {
                    let _ = handle.broadcast_tx.send(ProtocolMessage::new(
                        MsgType::Proposal,
                        propose_w,
                        notify.clone(),
                    ));
                }

                // Waiting for the quorum of confirmations of window propose_w over the LIVE set.
                // Spec "Network pace property": the fastest node WAITS
                // until enough others catch up — pace = the median active node.
                // A live-but-slow neighbour is NOT dropped (it is waited for as long as needed);
                // only one that has truly gone silent drops out of the set (M4-INFO-10 failsafe).
                let mut last_notify = Instant::now();
                loop {
                    if let Some(ref mut handle) = network_handle {
                        let mut ctx = net_ctx!();
                        drain_network(&mut ctx, handle)?;
                    }
                    if current >= propose_w || STOP.load(Ordering::SeqCst) {
                        break;
                    }
                    let active =
                        active_chain_length_at(&state.nodes, propose_w, params).map_err(|e| {
                            NodeError::InvalidArguments(format!("active standing: {e:?}"))
                        })?;
                    let need = quorum(active);
                    let got = accumulated_standing_at(
                        bc_accumulator.get(&propose_w),
                        propose_w,
                        propose_w,
                        &state.nodes,
                        params,
                    )
                    .map_err(|e| {
                        NodeError::InvalidArguments(format!("accumulated standing: {e:?}"))
                    })?;
                    if active != 0 && got >= need {
                        break;
                    }
                    if last_notify.elapsed() > Duration::from_secs(10) {
                        if let Some(ref handle) = network_handle {
                            let _ = handle.broadcast_tx.send(ProtocolMessage::new(
                                MsgType::Proposal,
                                propose_w,
                                notify.clone(),
                            ));
                        }
                        last_notify = Instant::now();
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                if current >= propose_w {
                    // The window was cemented by another legitimate proposer.
                    break 'active_arm;
                }
                if STOP.load(Ordering::SeqCst) {
                    break 'active_arm;
                }

                // Cementing: evidence = confirmations of window propose_w,
                // included_bundles = confirmations of window propose_w-1.
                let evidence = bc_accumulator.get(&propose_w).cloned().unwrap_or_default();
                let prev_w = propose_w.saturating_sub(1);
                let bundles_prev: Vec<BundledConfirmation> = if propose_w >= 1 {
                    bc_accumulator
                        .get(&prev_w)
                        .map(|m| m.values().cloned().collect())
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };
                let included_bundles_root = meta_root(
                    &bundles_prev
                        .iter()
                        .map(|b| (b.node_id, bundle_hash(b)))
                        .collect::<Vec<_>>(),
                );
                let evidence_list: Vec<_> = evidence.values().cloned().collect();
                let context = CementContext {
                    window: propose_w,
                    previous_endpoint: t_r_history.get(&prev_w),
                    current_endpoint: &t_r_w,
                    nodes: &state.nodes,
                    params,
                };
                let verified =
                    match context.verify(&included_bundles_root, &bundles_prev, &evidence_list) {
                        Ok(verified) => verified,
                        Err(e) => {
                            eprintln!(
                                "[consensus] w={propose_w}: waiting for verifiable cement ({e:?})"
                            );
                            break 'active_arm;
                        },
                    };
                let confirmers = verified.confirmers;
                let cemented = verified.cemented_hashes;
                let lottery = match complete(
                    reveal_pool.get(&prev_w),
                    &cemented,
                    prev_w,
                    &state.nodes,
                ) {
                    Ok(lottery) => lottery,
                    Err(error) => {
                        eprintln!("[consensus] w={propose_w}: waiting for the complete lottery ({error:?})");
                        break 'active_arm;
                    },
                };
                let (winner_id, winner_endpoint) = lottery.winner_or(my_node);
                let candidates_prev = lottery.candidates;
                let included_reveals_root = lottery.root;

                eprintln!(
                    "[lottery W={prev_w}] candidates={} winner={}",
                    candidates_prev.len(),
                    hex16(&winner_id)
                );

                // Single state transition (the same as for followers).
                let post_root = {
                    let mut ctx = net_ctx!();
                    settle_and_bookkeep(
                        &mut ctx,
                        propose_w,
                        &t_r_w,
                        winner_id,
                        &confirmers,
                        candidates_prev.clone(),
                        cemented.len() as u64,
                        None,
                    )?
                    .expect("leader mode: expected_roots=None prepares the transition")
                };
                let recomputed = compute_state_root(
                    &state.nodes.root(),
                    &state.candidates.root(),
                    &state.accounts.root(),
                );
                if recomputed != post_root {
                    panic!(
                        "state_root self-verify failed: {:02x?}.. ≠ {:02x?}..",
                        &post_root[..4],
                        &recomputed[..4]
                    );
                }

                let mut header = ProposalHeader {
                    prev_proposal_hash,
                    window_index: propose_w,
                    protocol_version: 1,
                    control_root: empty_internal(TREE_DEPTH),
                    node_root: state.nodes.root(),
                    candidate_root: state.candidates.root(),
                    account_root: state.accounts.root(),
                    state_root: post_root,
                    timechain_value: t_r_w,
                    included_bundles_root,
                    included_reveals_root,
                    winner_endpoint,
                    winner_id,
                    proposer_node_id: canonical_proposer,
                    target: timechain.lottery_target,
                    fallback_depth: my_depth,
                    signature: Signature::from_array([0u8; SIGNATURE_SIZE]),
                };
                let mut header_scope = Vec::new();
                header.encode_signed_scope(&mut header_scope);
                header.signature =
                    sign(&identity.node_sk, &header_scope).map_err(NodeError::Crypto)?;

                // Envelope: [header 3722][u16 n1][BC of window w-1][u16 n2][BC of window w].
                let mut envelope_payload: Vec<u8> = Vec::with_capacity(
                    PROPOSAL_HEADER_SIZE + 4 + 3500 * (bundles_prev.len() + evidence.len()),
                );
                header.encode(&mut envelope_payload);
                envelope_payload.extend_from_slice(&(bundles_prev.len() as u16).to_le_bytes());
                for bc in &bundles_prev {
                    bc.encode(&mut envelope_payload);
                }
                envelope_payload.extend_from_slice(&(evidence.len() as u16).to_le_bytes());
                for bc in evidence.values() {
                    bc.encode(&mut envelope_payload);
                }
                // Archive before broadcast: a cemented envelope must survive a
                // process crash — after a restart it is re-broadcast from the archive.
                if let Err(e) = store.archive_proposal_envelope(propose_w, &envelope_payload) {
                    eprintln!(
                        "[archive] FAILED to write envelope w={propose_w} to {}/proposals: {e:?}",
                        data_dir.display()
                    );
                }
                if let Some(ref handle) = network_handle {
                    let envelope = ProtocolMessage::new(
                        MsgType::Proposal,
                        propose_w,
                        envelope_payload.clone(),
                    );
                    if let Err(e) = handle.broadcast_tx.send(envelope) {
                        eprintln!(
                            "[consensus] broadcast CEMENTED Proposal w={propose_w} failed: {e}"
                        );
                    } else {
                        eprintln!(
                            "[consensus] broadcast CEMENTED Proposal window={propose_w} → peers (bundles={}, evidence={})",
                            bundles_prev.len(),
                            evidence.len()
                        );
                    }
                }
                recent_roots.insert(propose_w, post_root);
                bound_map(&mut recent_roots);
                prev_proposal_hash = proposal_hash(&header);
                cement_progress.record_envelope(propose_w, prev_proposal_hash);
                last_proposer_cement.insert(my_node, propose_w);
                last_cement_at = Instant::now();
                session_emitted = session_emitted.saturating_add(params.emission_moneta);
                session_windows += 1;
            },
        }

        save_progress(&data_dir, &state, &timechain, &lifecycle, current)?;
    }

    let elapsed = session_start.elapsed();
    println!();
    println!("--- session ended ---");
    println!("phase            : {:?}", lifecycle.phase);
    println!(
        "candidate SSHA    : {}/{}",
        lifecycle.candidate_progress, lifecycle.target_chain_length
    );
    println!("windows processed : {session_windows} (Active phase only)");
    println!(
        "emitted          : {} nɈ ({} Ɉ)",
        session_emitted,
        session_emitted / 1_000_000_000
    );
    println!("session wall     : {:.1}s", elapsed.as_secs_f64());
    let final_balance = state
        .accounts
        .get(&my_account)
        .map(|a| a.balance)
        .unwrap_or(0);
    println!(
        "balance final    : {} nɈ ({} Ɉ)",
        final_balance,
        final_balance / 1_000_000_000
    );

    Ok(())
}

fn save_progress(
    data_dir: &std::path::Path,
    state: &LocalState,
    timechain: &TimeChainState,
    lifecycle: &NodeLifecycle,
    window: u64,
) -> Result<(), NodeError> {
    state.save(data_dir)?;
    save_current_window(data_dir, window)?;
    save_timechain(data_dir, timechain)?;
    save_lifecycle(data_dir, lifecycle)?;
    Ok(())
}

fn install_shutdown_handlers() {
    STOP.store(false, Ordering::SeqCst);
    unsafe {
        // SAFETY: shutdown_handler is an extern "C" function with the correct signature
        // (void (*)(int)). libc::signal takes a usize-cast pointer and
        // registers the handler. The atomic store to a global flag is the
        // handler's only side effect, signal-safe (POSIX async-signal-safe).
        // SIGINT — Ctrl-C from the terminal. SIGTERM — launchctl unload / systemd stop /
        // kill PID. Both are routed to the same handler for uniform graceful shutdown.
        libc::signal(libc::SIGINT, shutdown_handler as *const () as usize);
        libc::signal(libc::SIGTERM, shutdown_handler as *const () as usize);
    }
}

// Decomposition of ssha_step(prev, d) into chunks with progress to stdout.
// Byte-exact correctness: SHA-256^d is associative under composition,
// ssha_step(ssha_step(x, a), b) = ssha_step(x, a + b) for a + b = d.
//
// Boundaries are computed as (d × i) / N — divides D exactly into N equal
// parts even when D is not a multiple of N (the last chunk may be 1 iteration
// larger due to rounding, but the percentages in the output are always exactly 10, 20, …, 100).
const SSHA_PROGRESS_CHUNKS: u64 = 10;

/// Proposer silence timeout: after this many seconds without cementing
/// the fallback cascade shifts by one position (spec "Fallback cascade").
/// Retention depth of per-window histories (pools, winners, confirmer sets).
const HISTORY_BOUND: usize = 64;
const MAX_PENDING_MESSAGES: usize = 512;
// Operational memory budget: enough for eight maximum-size wire envelopes.
// Reaching it drops a deferred message; it never reduces a consensus quorum.
const MAX_PENDING_BYTES: usize = 8 * mt_net_transport::codec::MAX_PROTOCOL_PAYLOAD_BYTES;

fn defer_message(pending: &mut Vec<ProtocolMessage>, message: ProtocolMessage) {
    if pending.len() >= MAX_PENDING_MESSAGES || pending.contains(&message) {
        return;
    }
    let Some(used) = pending
        .iter()
        .try_fold(0usize, |sum, held| sum.checked_add(held.payload.len()))
    else {
        return;
    };
    if used
        .checked_add(message.payload.len())
        .is_some_and(|bytes| bytes <= MAX_PENDING_BYTES)
    {
        pending.push(message);
    }
}

fn bound_map<K: Ord + Copy, V>(m: &mut BTreeMap<K, V>) {
    while m.len() > HISTORY_BOUND {
        let k = *m.keys().next().unwrap();
        m.remove(&k);
    }
}

/// The node's whole consensus context as one bundle of references — shared by
/// the message handler, the proposer and the turn epilogue.
struct NetCtx<'a> {
    state: &'a mut LocalState,
    current: &'a mut u64,
    timechain: &'a mut TimeChainState,
    recent_roots: &'a mut BTreeMap<u64, Hash32>,
    t_r_history: &'a mut BTreeMap<u64, Hash32>,
    reveal_pool: &'a mut BTreeMap<u64, BTreeMap<mt_state::NodeId, SshaReveal>>,
    bc_accumulator: &'a mut BTreeMap<u64, BTreeMap<mt_state::NodeId, BundledConfirmation>>,
    winner_history: &'a mut BTreeMap<u64, mt_state::NodeId>,
    lottery_history: &'a mut BTreeMap<u64, Vec<mt_lottery::Candidate>>,
    bc_set_history: &'a mut BTreeMap<u64, Vec<mt_state::NodeId>>,
    last_proposer_cement: &'a mut BTreeMap<mt_state::NodeId, u64>,
    own_bc_cache: &'a mut BTreeMap<u64, BundledConfirmation>,
    pending_msgs: &'a mut Vec<ProtocolMessage>,
    fast_sync: &'a mut Option<FastSyncSession>,
    fast_sync_deadline: &'a mut Option<Instant>,
    last_cement_at: &'a mut Instant,
    cement_progress: &'a mut CementProgress,
    prev_proposal_hash: &'a mut Hash32,
    fast_sync_lag_threshold: u64,
    // Adaptive proposer-takeover timeout (× window duration): after each such
    // interval of silence from the legitimate proposer the role passes to the
    // next fallback, terminally to bootstrap (the network does not stall).
    fallback_secs: u64,
    data_dir: &'a std::path::Path,
    params: &'static mt_genesis::ProtocolParams,
    store: &'a FsStore,
    identity: &'a crate::identity::Identity,
    my_node: mt_state::NodeId,
}

/// Canonical aggregate of confirmations of window w: the confirmer set is taken from the
/// included_bundles of the proposal that closed window w (bc_set_history).
fn cba_from(history: &BTreeMap<u64, Vec<mt_state::NodeId>>, w: u64) -> Hash32 {
    let set = history.get(&w).map(|v| v.as_slice()).unwrap_or(&[]);
    cemented_bundle_aggregate(w, set)
}

fn cba_for(ctx: &NetCtx, w: u64) -> Hash32 {
    cba_from(ctx.bc_set_history, w)
}

/// Depth of the fallback cascade on silence of the legitimate proposer: each interval of
/// fallback_secs without cementing shifts the role to the next fallback.
fn fallback_depth_at(silence_secs: u64, fallback_secs: u64) -> u8 {
    let depth_extra = (silence_secs / fallback_secs.max(1)).min(254) as u8;
    1u8.saturating_add(depth_extra)
}

/// The CANONICAL proposer of window w per spec "Lookback Leadership":
///   proposer_W = the first node-candidate (class=Node) in ascending
///   weighted_ticket among the sorted candidates of window W-2; on silence — the N-th
///   in the cascade (fallback_depth). Returns the CANONICAL value
///   (NO_PROPOSER for an empty set W-2 or one without node-candidates — Genesis
///   cold-start), which is written into the header field proposer_node_id and
///   is CHECKED by validate_proposer_is_canonical against the same W-2 candidates.
///
/// The source is exactly the same (sorted W-2 candidates via fallback_proposer)
/// as the validator's: header and check agree by construction. winner_history
/// is NOT used here (its value is the common lottery winner, including class=Account,
/// while the canonical proposer is the first node-candidate, which need not coincide).
fn canonical_proposer_at(ctx: &NetCtx, w: u64, silence_secs: u64) -> (mt_state::NodeId, u8) {
    canonical_proposer_lookback(
        w,
        fallback_depth_at(silence_secs, ctx.fallback_secs),
        ctx.lottery_history
            .get(&w.wrapping_sub(2))
            .map(|v| v.as_slice())
            .unwrap_or(&[]),
    )
}

/// Pure lookback logic of the canonical proposer, separated from NetCtx for
/// unit testing. sorted_w_minus_2 — the sorted candidates of window w-2.
/// Returns (canonical_proposer, depth): NO_PROPOSER for an empty set W-2 or one without
/// node-candidates (Genesis cold-start), otherwise the node_id of the fallback
/// slot fallback_depth.
fn canonical_proposer_lookback(
    w: u64,
    fallback_depth: u8,
    sorted_w_minus_2: &[mt_lottery::Candidate],
) -> (mt_state::NodeId, u8) {
    if w < 2 {
        return (mt_consensus::NO_PROPOSER, 1);
    }
    (
        mt_consensus::fallback_proposer(sorted_w_minus_2, fallback_depth),
        fallback_depth,
    )
}

/// The legitimate proposer the node ACTS AS in window w. The decision "propose myself":
/// when there is no canonical proposer (NO_PROPOSER — Genesis cold-start)
/// a lone node proposes itself — self-admit (selection_slots(0)=1) + self-cement
/// (quorum(1)=1) make its own proposal a chain. For the fast path
/// (depth=1) winner_history[W-2] is permitted as an operational hint about
/// whom to try as proposer; the on-chain field proposer_node_id is taken SEPARATELY from
/// canonical_proposer_at (only the sorted W-2 candidates), so a
/// winner_history ↔ lottery_history divergence does not reach the header.
/// Returns (acting, depth).
fn expected_proposer(ctx: &NetCtx, w: u64, silence_secs: u64) -> (mt_state::NodeId, u8) {
    if w < 2 {
        return (ctx.my_node, 1);
    }
    let depth = fallback_depth_at(silence_secs, ctx.fallback_secs);
    if depth == 1 {
        if let Some(p) = ctx.winner_history.get(&(w - 2)) {
            return (*p, 1);
        }
    }
    let (canonical, depth) = canonical_proposer_at(ctx, w, silence_secs);
    let acting = if canonical == mt_consensus::NO_PROPOSER {
        ctx.my_node
    } else {
        canonical
    };
    (acting, depth)
}

/// Parsing the 3722-byte proposal header from the canonical layout.
fn parse_header(payload: &[u8]) -> Option<ProposalHeader> {
    if payload.len() < PROPOSAL_HEADER_SIZE {
        return None;
    }
    let h32 = |a: usize| -> Hash32 {
        let mut b = [0u8; 32];
        b.copy_from_slice(&payload[a..a + 32]);
        b
    };
    let mut w8 = [0u8; 8];
    w8.copy_from_slice(&payload[32..40]);
    let mut v4 = [0u8; 4];
    v4.copy_from_slice(&payload[40..44]);
    let mut t16 = [0u8; 16];
    t16.copy_from_slice(&payload[396..412]);
    let mut sig = [0u8; SIGNATURE_SIZE];
    sig.copy_from_slice(&payload[413..PROPOSAL_HEADER_SIZE]);
    Some(ProposalHeader {
        prev_proposal_hash: h32(0),
        window_index: u64::from_le_bytes(w8),
        protocol_version: u32::from_le_bytes(v4),
        control_root: h32(44),
        node_root: h32(76),
        candidate_root: h32(108),
        account_root: h32(140),
        state_root: h32(172),
        timechain_value: h32(204),
        included_bundles_root: h32(236),
        included_reveals_root: h32(268),
        winner_endpoint: h32(300),
        winner_id: h32(332),
        proposer_node_id: h32(364),
        target: u128::from_le_bytes(t16),
        fallback_depth: payload[412],
        signature: Signature::from_array(sig),
    })
}

/// Single state transition when closing window settled_w — shared by the proposer
/// and followers (determinism): apply_proposal + candidate expiry + selection
/// event + D adaptation at the τ₂ boundary + per-window histories.
#[allow(clippy::too_many_arguments)]
fn settle_and_bookkeep(
    ctx: &mut NetCtx,
    settled_w: u64,
    timechain_value: &Hash32,
    winner_id: Hash32,
    confirmers: &[mt_state::NodeId],
    candidates_w_minus_1: Vec<mt_lottery::Candidate>,
    cemented_reveal_count: u64,
    expected_roots: Option<StateRoots>,
) -> Result<Option<Hash32>, NodeError> {
    // DEV-042: prepare every table change before accepting any signed root.
    let settle = ProposalSettle {
        window_w: settled_w,
        winner_id,
        cemented_confirmers: confirmers.to_vec(),
    };
    let aggregate = cba_for(ctx, settled_w.saturating_sub(2));
    let Some(prepared) = prepare(
        ctx.state,
        &settle,
        timechain_value,
        &aggregate,
        ctx.params,
        expected_roots,
    ) else {
        return Ok(None);
    };
    let post_root = prepared.roots.state;
    let activated_count = prepared.activated;
    *ctx.state = prepared.state;
    if activated_count > 0 {
        println!("[selection W={settled_w}] activated {activated_count} node(s)");
    }
    ctx.timechain.tau2_reveal_count = ctx
        .timechain
        .tau2_reveal_count
        .saturating_add(cemented_reveal_count);
    if settled_w > 0 && settled_w % ctx.params.tau2_windows == 0 {
        let median_permille = 1000u32;
        let new_d = next_d(ctx.timechain.current_d, median_permille, ctx.params);
        if new_d != ctx.timechain.current_d {
            println!(
                "[next_d W={settled_w}] D: {} → {}",
                ctx.timechain.current_d, new_d
            );
            ctx.timechain.current_d = new_d;
        }
        // Spec "Target calibration": at the τ₂ boundary the threshold target
        // is recomputed to ~13 candidates per window (binding vectors TA1-TA5).
        let new_target = mt_lottery::calibrate_target(
            ctx.timechain.lottery_target,
            ctx.timechain.tau2_reveal_count,
            ctx.params.tau2_windows,
        );
        if new_target != ctx.timechain.lottery_target {
            println!(
                "[target W={settled_w}] {:#x} → {:#x} (tickets in τ₂: {})",
                ctx.timechain.lottery_target, new_target, ctx.timechain.tau2_reveal_count
            );
            ctx.timechain.lottery_target = new_target;
        }
        ctx.timechain.tau2_reveal_count = 0;
    }
    if settled_w >= 1 {
        ctx.bc_set_history
            .insert(settled_w - 1, confirmers.to_vec());
        ctx.winner_history.insert(settled_w - 1, winner_id);
        ctx.lottery_history
            .insert(settled_w - 1, candidates_w_minus_1);
    }
    bound_map(ctx.bc_set_history);
    bound_map(ctx.winner_history);
    bound_map(ctx.lottery_history);
    ctx.bc_accumulator.remove(&settled_w.saturating_sub(1));
    bound_map(ctx.bc_accumulator);
    *ctx.current = settled_w;
    save_current_window(ctx.data_dir, settled_w)?;
    ctx.store
        .save_meta_last_cemented(settled_w)
        .map_err(|e| NodeError::InvalidArguments(format!("save_meta_last_cemented: {e:?}")))?;
    Ok(Some(post_root))
}

/// Handling one incoming protocol message. Called from
/// drain_network at the start of a turn, between portions of the sequential SHA-256
/// chain, and from the proposer's quorum-wait loop — one logic everywhere.
fn handle_protocol_message(
    ctx: &mut NetCtx,
    broadcast_tx: &tokio::sync::mpsc::UnboundedSender<mt_net::ProtocolMessage>,
    msg: ProtocolMessage,
) -> Result<(), NodeError> {
    match msg.msg_type {
        MsgType::Proposal => {
            let Some(header) = parse_header(&msg.payload) else {
                eprintln!(
                    "[consensus] Proposal envelope wrong size {} — skip",
                    msg.payload.len()
                );
                return Ok(());
            };
            let w = header.window_index;
            let is_cemented = msg.payload.len() > PROPOSAL_HEADER_SIZE;
            // Canonical clock check: one window — one value of the time chain.
            if let Some(known) = ctx.t_r_history.get(&w) {
                if *known != header.timechain_value {
                    eprintln!("[consensus] TIMECHAIN DIVERGENCE w={w}: envelope != local — skip");
                    return Ok(());
                }
            }
            if !is_cemented {
                // Candidate notification: the signal "window w is being assembled". If our BC
                // of this window is already published — repeat it (proposer recovery
                // after a restart). We must not recompute BC from a foreign t_r ourselves:
                // a window confirmation is permissible only from the own clock.
                if header.proposer_node_id != ctx.my_node {
                    if let Some(bc) = ctx.own_bc_cache.get(&w) {
                        let mut bc_payload = Vec::new();
                        bc.encode(&mut bc_payload);
                        let _ = broadcast_tx.send(ProtocolMessage::new(
                            MsgType::BundledConfirmation,
                            w,
                            bc_payload,
                        ));
                    }
                    // The network is assembling a window beyond our head — we have fallen behind.
                    // Signal with our last cemented envelope:
                    // the neighbour will reply by serving the missing archive envelopes.
                    if w > *ctx.current + 1 {
                        if let Ok(Some(env)) = ctx.store.load_proposal_envelope(*ctx.current) {
                            let _ = broadcast_tx.send(ProtocolMessage::new(
                                MsgType::Proposal,
                                *ctx.current,
                                env,
                            ));
                            eprintln!(
                                "[consensus] behind (head {}, network at {w}) — signaling with own envelope",
                                *ctx.current
                            );
                        }
                    }
                }
                return Ok(());
            }
            // --- cemented envelope ---
            // DEV-018c: reset a hung fast-sync client by deadline.
            if let Some(deadline) = *ctx.fast_sync_deadline {
                if Instant::now() > deadline {
                    eprintln!("[m7] fast-sync deadline exceeded — drop client, retry");
                    *ctx.fast_sync = None;
                    *ctx.fast_sync_deadline = None;
                }
            }
            if ctx.fast_sync.is_some() {
                return Ok(());
            }
            // A received proposal is a discovery hint until its canonical
            // transition has verified. It must not add its own root to the
            // snapshot trust set, even when the local node table is empty.
            if ctx.state.nodes.is_empty()
                || w.saturating_sub(*ctx.current) > ctx.fast_sync_lag_threshold
            {
                let Some(session) = FastSyncSession::from_verified(
                    msg.request_id,
                    w,
                    header.state_root,
                    ctx.recent_roots,
                    ctx.t_r_history,
                ) else {
                    eprintln!(
                        "[m7] unverified anchor at window {w}; waiting for authenticated history"
                    );
                    // Ask through the existing sequential-replay path. A fresh
                    // node without a trusted origin cannot safely adopt a peer's
                    // account table merely because that peer calls it genesis.
                    if let Ok(Some(envelope)) = ctx.store.load_proposal_envelope(*ctx.current) {
                        let _ = broadcast_tx.send(ProtocolMessage::new(
                            MsgType::Proposal,
                            *ctx.current,
                            envelope,
                        ));
                    } else {
                        eprintln!("[m7] local replay anchor unavailable");
                    }
                    return Ok(());
                };
                let mut fs_payload = Vec::new();
                mt_net::FastSyncRequest {
                    anchor_window: w,
                    resume_offset: 0,
                }
                .encode(&mut fs_payload);
                match broadcast_tx.send(ProtocolMessage::new(
                    MsgType::FastSyncRequest,
                    msg.request_id,
                    fs_payload,
                )) {
                    Ok(()) => {
                        eprintln!(
                            "[m7] {} windows behind (> {}) → fast-sync anchored at window {w}",
                            w.saturating_sub(*ctx.current),
                            ctx.fast_sync_lag_threshold
                        );
                        *ctx.fast_sync = Some(session);
                        *ctx.fast_sync_deadline =
                            Some(Instant::now() + std::time::Duration::from_secs(10));
                    },
                    Err(e) => eprintln!("[m7] FastSyncRequest broadcast failed: {e}"),
                }
                return Ok(());
            }
            if w != *ctx.current + 1 {
                if w > *ctx.current {
                    eprintln!(
                        "[consensus] cemented w={w} gap (current={}) — awaiting sequential cementing or fast-sync",
                        *ctx.current
                    );
                } else if header.proposer_node_id != ctx.my_node {
                    // The neighbour broadcasts an already closed window = it has fallen behind. Serve it
                    // the missing archive envelopes sequentially.
                    let from = w + 1;
                    let upto = (*ctx.current).min(w + 8);
                    let mut served = 0u32;
                    for w_re in from..=upto {
                        if let Ok(Some(env)) = ctx.store.load_proposal_envelope(w_re) {
                            let _ = broadcast_tx.send(ProtocolMessage::new(
                                MsgType::Proposal,
                                w_re,
                                env,
                            ));
                            served += 1;
                        }
                    }
                    if served > 0 {
                        eprintln!(
                            "[consensus] peer on stale window {w} — re-served {served} envelope(s) up to {upto}"
                        );
                    }
                }
                return Ok(());
            }
            if header.prev_proposal_hash != *ctx.prev_proposal_hash {
                eprintln!("[consensus] w={w}: predecessor mismatch — skip");
                return Ok(());
            }
            // Spec "Proposal finality": the proposer's signature is checked
            // against the node table (NOT only the first settler), the window is monotonic.
            if let Err(e) =
                mt_consensus::validate_header(&header, &ctx.state.nodes, *ctx.current, 1, 1)
            {
                eprintln!("[consensus] header w={w} rejected: {e:?}");
                return Ok(());
            }
            // Proposer legitimacy is EXCLUSIVELY a deterministic check
            // (DEV-043 closure, [I-3]): the proposer must equal
            // fallback_proposer(W-2, header.fallback_depth). The cascade depth
            // is declared in the signed header, not computed from local
            // wall-clock time — so acceptance is a pure function of
            // (header, cemented W-2) and is identical on all honest nodes. The local
            // clock takes part ONLY in the decision "should I propose myself" (self-action,
            // liveness) on the proposer side and does not enter consensus state.
            // Proposer canonicity (spec "Canonical acceptance" (a)):
            // proposer_node_id must equal fallback_proposer(W-2, depth) at the
            // fallback_depth declared by the header. With an empty set W-2
            // (Genesis cold-start) the canonical value is NO_PROPOSER; a forged
            // real node_id with an empty W-2 is rejected here.
            let sorted_w_minus_2 = ctx
                .lottery_history
                .get(&w.saturating_sub(2))
                .cloned()
                .unwrap_or_default();
            if let Err(e) = mt_consensus::validate_proposer_is_canonical(&header, &sorted_w_minus_2)
            {
                eprintln!(
                    "[consensus] w={w}: leader {} not canonical ({e:?}) — skip",
                    hex16(&header.proposer_node_id)
                );
                return Ok(());
            }
            if header.target != ctx.timechain.lottery_target {
                eprintln!(
                    "[consensus] w={w}: envelope target != local — calibration divergence, skip"
                );
                return Ok(());
            }
            let Some((bundles_prev, evidence)) = parse_envelope_bundles(&msg.payload) else {
                eprintln!("[consensus] cemented w={w}: envelope tail not parsed — skip");
                return Ok(());
            };
            let context = CementContext {
                window: w,
                previous_endpoint: w
                    .checked_sub(1)
                    .and_then(|previous| ctx.t_r_history.get(&previous)),
                current_endpoint: &header.timechain_value,
                nodes: &ctx.state.nodes,
                params: ctx.params,
            };
            let verified =
                match context.verify(&header.included_bundles_root, &bundles_prev, &evidence) {
                    Ok(verified) => verified,
                    Err(e) => {
                        eprintln!("[consensus] w={w}: cement rejected ({e:?})");
                        return Ok(());
                    },
                };
            let confirmers = verified.confirmers;
            let cemented = verified.cemented_hashes;
            let previous_window = w.saturating_sub(1);
            let lottery = match complete(
                ctx.reveal_pool.get(&previous_window),
                &cemented,
                previous_window,
                &ctx.state.nodes,
            ) {
                Ok(lottery) => lottery,
                Err(error) => {
                    eprintln!(
                        "[consensus] w={w}: lottery not complete ({error:?}) — window deferred"
                    );
                    if error == LotteryError::MissingReveal {
                        defer_message(ctx.pending_msgs, msg);
                    }
                    return Ok(());
                },
            };
            if let Err(error) = lottery.verify(
                header.included_reveals_root,
                header.winner_id,
                header.winner_endpoint,
                header.proposer_node_id,
            ) {
                eprintln!("[consensus] w={w}: lottery rejected ({error:?})");
                return Ok(());
            }
            let candidates_prev = lottery.candidates;
            // Application by a single state transition (transactional, DEV-042).
            if settle_and_bookkeep(
                ctx,
                w,
                &header.timechain_value,
                header.winner_id,
                &confirmers,
                candidates_prev,
                cemented.len() as u64,
                Some(StateRoots::from_header(&header)),
            )?
            .is_none()
            {
                eprintln!(
                    "[consensus] w={w}: state commitment mismatch — window rejected, recovery via fast-sync"
                );
                return Ok(());
            }
            ctx.recent_roots.insert(w, header.state_root);
            bound_map(ctx.recent_roots);
            ctx.t_r_history.insert(w, header.timechain_value);
            bound_map(ctx.t_r_history);
            if ctx.timechain.last_window < w {
                // Canonical clock value from the quorum envelope — catch up.
                ctx.timechain.t_r = header.timechain_value;
                ctx.timechain.last_window = w;
            }
            // The archive is observability, not consensus: a write failure is logged
            // loudly (with the path), the node continues working.
            if let Err(e) = ctx.store.archive_proposal_envelope(w, &msg.payload) {
                eprintln!(
                    "[archive] FAILED to write envelope w={w} to {}/proposals: {e:?}",
                    ctx.data_dir.display()
                );
            }
            *ctx.prev_proposal_hash = proposal_hash(&header);
            ctx.cement_progress
                .record_envelope(w, *ctx.prev_proposal_hash);
            ctx.last_proposer_cement.insert(header.proposer_node_id, w);
            *ctx.last_cement_at = Instant::now();
            eprintln!(
                "[consensus] applied cemented Proposal w={w} (confirmers={}, winner={})",
                confirmers.len(),
                hex16(&header.winner_id)
            );
        },
        MsgType::FastSyncRequest => match mt_net::FastSyncRequest::decode(&msg.payload) {
            Ok(req) => {
                if req.anchor_window != *ctx.current {
                    return Ok(());
                }
                let snap = mt_sync::Snapshot::from_tables(
                    *ctx.current,
                    &ctx.state.accounts,
                    &ctx.state.nodes,
                    &ctx.state.candidates,
                );
                let chunks = snap.to_wire_chunks(32);
                let total = chunks.len();
                for chunk in chunks {
                    let table_id_byte = match chunk.table_id {
                        mt_sync::FastSyncTableId::Account => mt_net::TableId::Account,
                        mt_sync::FastSyncTableId::Node => mt_net::TableId::Node,
                        mt_sync::FastSyncTableId::Candidate => mt_net::TableId::Candidate,
                        mt_sync::FastSyncTableId::Proposals => mt_net::TableId::Proposals,
                    };
                    let mut flat: Vec<u8> = Vec::new();
                    for r in &chunk.records {
                        flat.extend_from_slice(r);
                    }
                    let wire_chunk = mt_net::FastSyncResponseChunk {
                        chunk_index: chunk.chunk_index,
                        total_chunks: chunk.total_chunks,
                        table_id: table_id_byte,
                        record_count: chunk.records.len() as u32,
                        anchor_window: *ctx.current,
                        records: flat,
                    };
                    let mut payload = Vec::new();
                    wire_chunk.encode(&mut payload);
                    let envelope =
                        ProtocolMessage::new(MsgType::FastSyncResponse, msg.request_id, payload);
                    if broadcast_tx.send(envelope).is_err() {
                        break;
                    }
                }
                eprintln!(
                    "[m7] served FastSync snapshot: anchor_window={} req={} chunks={total}",
                    *ctx.current, req.anchor_window
                );
            },
            Err(e) => {
                eprintln!("[m7] FastSyncRequest decode failed: {e:?}");
            },
        },
        MsgType::FastSyncResponse => {
            let Ok(wire) = mt_net::FastSyncResponseChunk::decode(&msg.payload) else {
                return Ok(());
            };
            // Check before taking or changing the session. Unsolicited traffic
            // must not reset a valid transfer or choose its anchor.
            if !ctx
                .fast_sync
                .as_ref()
                .is_some_and(|session| session.matches(msg.request_id, wire.anchor_window))
            {
                return Ok(());
            }
            if let Some(mut session) = ctx.fast_sync.take() {
                if session.window <= *ctx.current {
                    *ctx.fast_sync_deadline = None;
                    return Ok(());
                }
                let parsed = crate::commands::fastsync::wire_chunk_to_sync(wire)
                    .map_err(|error| format!("wire: {error:?}"));
                match parsed {
                    Ok(chunk) => match session.client.accept_chunk(chunk) {
                        Ok(mt_sync::AcceptOutcome::Complete) => {
                            let verified_timechain = session.timechain;
                            match session.finalize() {
                                Ok((window, tables)) => {
                                    ctx.state.apply_fast_sync(tables, ctx.data_dir, window)?;
                                    *ctx.current = window;
                                    save_current_window(ctx.data_dir, window)?;
                                    ctx.timechain.t_r = verified_timechain;
                                    ctx.timechain.last_window = window;
                                    save_timechain(ctx.data_dir, ctx.timechain)?;
                                    *ctx.fast_sync_deadline = None;
                                    eprintln!(
                                    "[m7] fast-sync complete → state replaced, current_window={window}"
                                );
                                },
                                Err(e) => eprintln!(
                                    "[m7] fast-sync finalize rejected: {e:?} — retry on next lag"
                                ),
                            }
                        },
                        Ok(mt_sync::AcceptOutcome::Progress { received, total }) => {
                            eprintln!("[m7] fast-sync chunk {received}/{total}");
                            *ctx.fast_sync = Some(session);
                        },
                        Err(e) => eprintln!(
                            "[m7] fast-sync chunk rejected: {e:?} — discard, retry on next lag"
                        ),
                    },
                    Err(reason) => {
                        eprintln!("[m7] FastSyncResponse {reason}");
                        *ctx.fast_sync = Some(session);
                    },
                }
            }
        },
        MsgType::SshaReveal => {
            if let Ok((rec_reveal, used)) = SshaReveal::decode(&msg.payload) {
                if used != msg.payload.len() {
                    return Ok(());
                }
                let rw = rec_reveal.window_index;
                if !ctx.t_r_history.contains_key(&rw) && rw > ctx.timechain.last_window {
                    // The clock of this window is not yet known to us — defer until our own turn.
                    defer_message(ctx.pending_msgs, msg);
                    return Ok(());
                }
                let Some(exp_t_r) = ctx.t_r_history.get(&rw).copied() else {
                    return Ok(());
                };
                let cba = cba_for(ctx, rw.saturating_sub(2));
                if validate_reveal(&rec_reveal, &ctx.state.nodes, &exp_t_r, &cba, rw).is_ok() {
                    let nid = rec_reveal.node_id;
                    ctx.reveal_pool
                        .entry(rw)
                        .or_default()
                        .insert(nid, rec_reveal);
                    bound_map(ctx.reveal_pool);
                    eprintln!(
                        "[lottery] accepted ticket from {} for window {rw}",
                        hex16(&nid)
                    );
                    // Confirmation maturation: our BC of window rw+1 carries the hashes
                    // of window rw tickets. A late ticket extends the set —
                    // re-issue the BC so that all confirmers converge on a
                    // unanimous (at equal weights) cementable set.
                    let bw_own = rw + 1;
                    if ctx.own_bc_cache.contains_key(&bw_own) {
                        if let Some(t_r_bw) = ctx.t_r_history.get(&bw_own).copied() {
                            let mut hashes: Vec<Hash32> = ctx
                                .reveal_pool
                                .get(&rw)
                                .map(|m| m.values().map(reveal_hash).collect())
                                .unwrap_or_default();
                            hashes.sort();
                            let stale = ctx
                                .own_bc_cache
                                .get(&bw_own)
                                .map(|b| b.reveal_hashes != hashes)
                                .unwrap_or(true);
                            if stale {
                                let mut bc = BundledConfirmation {
                                    node_id: ctx.my_node,
                                    endpoint: t_r_bw,
                                    window_index: bw_own,
                                    op_hashes: Vec::new(),
                                    reveal_hashes: hashes,
                                    signature: Signature::from_array([0u8; SIGNATURE_SIZE]),
                                };
                                let mut scope = Vec::new();
                                bc.encode_signed_scope(&mut scope);
                                if let Ok(sig) = sign(&ctx.identity.node_sk, &scope) {
                                    bc.signature = sig;
                                    ctx.own_bc_cache.insert(bw_own, bc.clone());
                                    ctx.bc_accumulator
                                        .entry(bw_own)
                                        .or_default()
                                        .insert(ctx.my_node, bc.clone());
                                    let mut payload = Vec::new();
                                    bc.encode(&mut payload);
                                    let _ = broadcast_tx.send(ProtocolMessage::new(
                                        MsgType::BundledConfirmation,
                                        bw_own,
                                        payload,
                                    ));
                                }
                            }
                        }
                    }
                } else {
                    eprintln!(
                        "[lottery] ticket {} w={rw} failed validation",
                        hex16(&rec_reveal.node_id)
                    );
                }
            }
        },
        MsgType::BundledConfirmation => match BundledConfirmation::decode(&msg.payload) {
            Ok((bc, used)) => {
                if used != msg.payload.len() {
                    return Ok(());
                }
                let bw = bc.window_index;
                if !ctx.t_r_history.contains_key(&bw) && bw > ctx.timechain.last_window {
                    defer_message(ctx.pending_msgs, msg);
                    return Ok(());
                }
                let Some(expected_t_r) = ctx.t_r_history.get(&bw).copied() else {
                    return Ok(());
                };
                if validate_bundle(&bc, &ctx.state.nodes, &expected_t_r).is_ok() {
                    let node_id = bc.node_id;
                    ctx.bc_accumulator
                        .entry(bw)
                        .or_default()
                        .insert(node_id, bc);
                    eprintln!(
                        "[bc] accepted confirmation from {} for window {bw}",
                        hex16(&node_id)
                    );
                    // A confirmation for an already closed window = the neighbour is behind by
                    // several windows (less than the fast-sync threshold).
                    // Serve the archive cemented envelopes again
                    // so that it catches up by sequential application.
                    if bw < *ctx.current && node_id != ctx.my_node {
                        let upto = (*ctx.current).min(bw + 8);
                        for w_re in bw..=upto {
                            if let Ok(Some(env)) = ctx.store.load_proposal_envelope(w_re) {
                                let _ = broadcast_tx.send(ProtocolMessage::new(
                                    MsgType::Proposal,
                                    w_re,
                                    env,
                                ));
                            }
                        }
                        eprintln!(
                            "[consensus] peer {} behind (window {bw} <= {}), re-served envelopes",
                            hex16(&node_id),
                            *ctx.current
                        );
                    }
                } else {
                    eprintln!(
                        "[bc] confirmation {} w={bw} failed validation",
                        hex16(&bc.node_id)
                    );
                }
            },
            Err(e) => eprintln!("[bc] decode failed: {e:?}"),
        },
        _ => {},
    }
    Ok(())
}

/// Publication of window w artifacts from the own canonical clock:
/// the lottery ticket (SSHA_Reveal) of window w and the confirmation (BundledConfirmation)
/// of window w with the ticket hashes of window w-1 — spec "Confirmations":
/// "A Bundle contains the operations of the current window W and the SSHA_Reveals of the previous window W-1".
#[allow(clippy::too_many_arguments)]
fn publish_window_artifacts(
    state: &LocalState,
    reveal_pool: &mut BTreeMap<u64, BTreeMap<mt_state::NodeId, SshaReveal>>,
    bc_accumulator: &mut BTreeMap<u64, BTreeMap<mt_state::NodeId, BundledConfirmation>>,
    own_bc_cache: &mut BTreeMap<u64, BundledConfirmation>,
    bc_set_history: &BTreeMap<u64, Vec<mt_state::NodeId>>,
    identity: &crate::identity::Identity,
    my_node: mt_state::NodeId,
    w: u64,
    t_r_w: &Hash32,
    lottery_target: u128,
    broadcast_tx: Option<&tokio::sync::mpsc::UnboundedSender<mt_net::ProtocolMessage>>,
) -> Result<(), NodeError> {
    let cba = cba_from(bc_set_history, w.saturating_sub(2));
    let endpoint = compute_endpoint(t_r_w, &cba, &my_node, w);
    // Spec: "If weighted_ticket_node < target — the node is a candidate and
    // publishes SSHA_Reveal". The window confirmation is always published.
    let is_candidate = state
        .nodes
        .get(&my_node)
        .map(|n| {
            let snapshot = n.chain_length_snapshot.max(1);
            weighted_ticket_node(&endpoint, n.chain_length, snapshot) < lottery_target
        })
        .unwrap_or(false);
    let mut reveal = SshaReveal {
        node_id: my_node,
        window_index: w,
        endpoint,
        signature: Signature::from_array([0u8; SIGNATURE_SIZE]),
    };
    let mut scope = Vec::new();
    reveal.encode_signed_scope(&mut scope);
    reveal.signature = sign(&identity.node_sk, &scope).map_err(NodeError::Crypto)?;
    if is_candidate {
        validate_reveal(&reveal, &state.nodes, t_r_w, &cba, w)
            .map_err(|e| NodeError::InvalidArguments(format!("validate_reveal: {e:?}")))?;
        reveal_pool
            .entry(w)
            .or_default()
            .insert(my_node, reveal.clone());
        bound_map(reveal_pool);
        if let Some(tx) = broadcast_tx {
            let mut payload = Vec::new();
            reveal.encode(&mut payload);
            let _ = tx.send(ProtocolMessage::new(MsgType::SshaReveal, w, payload));
        }
    } else {
        eprintln!("[lottery] window {w}: weighted ticket above target — not a candidate");
    }
    // Window w confirmation: hashes of the previous window tickets (two-window pipeline).
    let mut bc_reveal_hashes: Vec<Hash32> = if w >= 1 {
        reveal_pool
            .get(&(w - 1))
            .map(|m| m.values().map(reveal_hash).collect())
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    bc_reveal_hashes.sort();
    let mut bc = BundledConfirmation {
        node_id: my_node,
        endpoint: *t_r_w,
        window_index: w,
        op_hashes: Vec::new(),
        reveal_hashes: bc_reveal_hashes,
        signature: Signature::from_array([0u8; SIGNATURE_SIZE]),
    };
    let mut bc_scope = Vec::new();
    bc.encode_signed_scope(&mut bc_scope);
    bc.signature = sign(&identity.node_sk, &bc_scope).map_err(NodeError::Crypto)?;
    validate_bundle(&bc, &state.nodes, t_r_w)
        .map_err(|e| NodeError::InvalidArguments(format!("validate_bundle: {e:?}")))?;
    bc_accumulator
        .entry(w)
        .or_default()
        .insert(my_node, bc.clone());
    own_bc_cache.insert(w, bc.clone());
    bound_map(own_bc_cache);
    if let Some(tx) = broadcast_tx {
        let mut payload = Vec::new();
        bc.encode(&mut payload);
        let _ = tx.send(ProtocolMessage::new(
            MsgType::BundledConfirmation,
            w,
            payload,
        ));
    }
    Ok(())
}

/// Draining the incoming queue: first deferred messages whose windows have already
/// received the canonical clock value, then fresh ones from the channel.
fn drain_network(ctx: &mut NetCtx, handle: &mut NetworkHandle) -> Result<(), NodeError> {
    handle.ensure_alive()?;
    observe_cement(ctx);
    if !ctx.pending_msgs.is_empty() {
        let pend = std::mem::take(ctx.pending_msgs);
        let tx = handle.broadcast_tx.clone();
        for msg in pend {
            handle_protocol_message(ctx, &tx, msg)?;
            observe_cement(ctx);
        }
    }
    let tx = handle.broadcast_tx.clone();
    while let Some(msg) = handle.try_recv()? {
        handle_protocol_message(ctx, &tx, msg)?;
        observe_cement(ctx);
    }
    observe_cement(ctx);
    Ok(())
}

fn observe_cement(ctx: &mut NetCtx) {
    ctx.cement_progress.observe(ProgressInput {
        head: *ctx.current,
        nodes: &ctx.state.nodes,
        params: ctx.params,
        endpoints: ctx.t_r_history,
        bundles: ctx.bc_accumulator,
    });
}

fn ssha_step_chunked<F: FnMut() -> Result<(), NodeError>>(
    prev: &Hash32,
    d: u64,
    label: &str,
    window: u64,
    mut on_chunk: F,
) -> Result<Hash32, NodeError> {
    on_chunk()?;
    if !(1..=mt_timechain::MAX_D).contains(&d) {
        return Err(NodeError::InvalidArguments(
            "SSHA iteration count is outside the protocol band".into(),
        ));
    }
    let mut current = *prev;
    let chunk_start = Instant::now();
    let mut prev_boundary: u64 = 0;
    use std::io::Write;
    for i in 1..=SSHA_PROGRESS_CHUNKS {
        // Boundary distributes D exactly: (d × i) / N (overflow is safe:
        // d ≤ 2^32 typical, × N=10 ≤ 2^36).
        let boundary = d.saturating_mul(i) / SSHA_PROGRESS_CHUNKS;
        let this_chunk = boundary - prev_boundary;
        if this_chunk != 0 {
            current = ssha_step(&current, this_chunk);
        }
        prev_boundary = boundary;
        on_chunk()?;
        let percent = (i * 100) / SSHA_PROGRESS_CHUNKS;
        let bar = progress_bar(boundary, d, 30);
        let elapsed = chunk_start.elapsed().as_secs_f64();
        let line = format!(
            "window {window:>5} {label:<14} {} {:>3}% | {:>4}/{:>4} M | {:>5.1}s",
            bar,
            percent,
            boundary / 1_000_000,
            d / 1_000_000,
            elapsed
        );
        // `\r` at the start — carriage return; the bar is updated in one logical
        // line. The final chunk at 100% ends with `\n`. Works identically
        // in a TTY (cursor return) and when viewed via `tail -F` (the terminal
        // renders `\r` as a cursor return → animated bar).
        if i == SSHA_PROGRESS_CHUNKS {
            println!("\r{line}");
        } else {
            print!("\r{line}");
            let _ = std::io::stdout().flush();
        }
    }
    Ok(current)
}

fn progress_bar(done: u64, total: u64, width: usize) -> String {
    if total == 0 {
        return "[".to_string() + &"░".repeat(width) + "]";
    }
    let filled = ((done as u128) * (width as u128) / (total as u128)) as usize;
    let filled = filled.min(width);
    let empty = width - filled;
    let mut s = String::with_capacity(width + 2);
    s.push('[');
    for _ in 0..filled {
        s.push('▓');
    }
    for _ in 0..empty {
        s.push('░');
    }
    s.push(']');
    s
}

fn hex16(bytes: &[u8]) -> String {
    let take = bytes.len().min(8);
    let mut s = String::with_capacity(take * 2);
    for b in &bytes[..take] {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn spawn_network_thread(
    identity: &crate::identity::Identity,
    listen_str: &str,
    manifest_path: &std::path::Path,
) -> Result<NetworkHandle, NodeError> {
    use std::str::FromStr;

    let manifest_text = std::fs::read_to_string(manifest_path).map_err(|e| {
        NodeError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("genesis-manifest {manifest_path:?}: {e}"),
        ))
    })?;
    let manifest = mt_genesis::GenesisManifest::parse(&manifest_text)
        .map_err(|e| NodeError::Network(format!("parse manifest: {e}")))?;

    let listen_addr = libp2p::Multiaddr::from_str(listen_str)
        .map_err(|e| NodeError::Network(format!("parse --listen {listen_str}: {e}")))?;

    let local_keypair = identity.libp2p_keypair();
    let mldsa_id_pk = identity.node_pk.clone();
    let mldsa_id_sk_bytes: [u8; mt_crypto::SECRET_KEY_SIZE] = *identity.node_sk.as_bytes();
    let mldsa_id_sk = mt_crypto::SecretKey::from_array(mldsa_id_sk_bytes);
    let local_peer_id = mt_net_transport::derive_peer_id(&identity.node_pk)
        .map_err(|e| NodeError::Network(format!("derive XX peer_id: {e}")))?;
    let manifest_clone = manifest.clone();

    let (broadcast_tx, broadcast_rx) =
        tokio::sync::mpsc::unbounded_channel::<mt_net::ProtocolMessage>();
    let (incoming_tx, incoming_rx) =
        tokio::sync::mpsc::unbounded_channel::<mt_net::ProtocolMessage>();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();

    eprintln!(
        "[main] spawning network thread; configured_peers={n}",
        n = manifest.peers.len()
    );

    std::thread::Builder::new()
        .name("montana-network".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    eprintln!("[network] failed to start tokio runtime: {e}");
                    return;
                },
            };
            if let Err(e) = runtime.block_on(crate::network::run_network_loop(
                local_keypair,
                local_peer_id,
                mldsa_id_pk,
                mldsa_id_sk,
                manifest_clone,
                listen_addr,
                broadcast_rx,
                incoming_tx,
                ready_tx,
            )) {
                eprintln!("[network] event loop exited with error: {e}");
            }
        })
        .map_err(|e| NodeError::Network(format!("spawn network thread: {e}")))?;

    // No local consensus state may be bootstrapped until parsing, transport
    // construction and binding the listener have actually succeeded.
    ready_rx
        .recv()
        .map_err(|_| NodeError::Network("network failed before startup completed".into()))?;
    let handle = NetworkHandle {
        broadcast_tx,
        incoming_rx,
    };
    handle.ensure_alive()?;
    Ok(handle)
}

/// Handle to the network thread. Through broadcast_tx the consensus loop sends
/// envelopes to all connected peers. Through incoming_rx the consensus loop
/// receives incoming envelopes (Proposal, BundledConfirmation, NodeRegistration,
/// ...) from peers for apply.
pub struct NetworkHandle {
    pub broadcast_tx: tokio::sync::mpsc::UnboundedSender<mt_net::ProtocolMessage>,
    pub incoming_rx: tokio::sync::mpsc::UnboundedReceiver<mt_net::ProtocolMessage>,
}

impl NetworkHandle {
    fn ensure_alive(&self) -> Result<(), NodeError> {
        if self.broadcast_tx.is_closed() || self.incoming_rx.is_closed() {
            return Err(NodeError::Network(
                "network worker stopped; refusing to continue consensus".into(),
            ));
        }
        Ok(())
    }

    fn try_recv(&mut self) -> Result<Option<mt_net::ProtocolMessage>, NodeError> {
        self.ensure_alive()?;
        match self.incoming_rx.try_recv() {
            Ok(message) => Ok(Some(message)),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => Ok(None),
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                Err(NodeError::Network("network worker disconnected".into()))
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::canonical_proposer_lookback;
    use super::resolve_fast_sync_lag_threshold as resolve;
    use super::FAST_SYNC_LAG_THRESHOLD as DEFAULT;
    use mt_consensus::{validate_proposer_is_canonical, NO_PROPOSER};
    use mt_lottery::{Candidate, WINNER_CLASS_NODE};

    #[test]
    fn deferred_replays_cannot_exceed_the_count_or_byte_budget() {
        use super::{defer_message, MAX_PENDING_BYTES, MAX_PENDING_MESSAGES};
        use mt_net::{MsgType, ProtocolMessage};
        let message = ProtocolMessage::new(MsgType::Proposal, 7, vec![3; 1024]);
        let mut pending = Vec::new();
        for _ in 0..MAX_PENDING_MESSAGES * 2 {
            defer_message(&mut pending, message.clone());
        }
        assert_eq!(pending, vec![message]);
        let mut pending = Vec::new();
        for id in 0..MAX_PENDING_MESSAGES * 2 {
            defer_message(
                &mut pending,
                ProtocolMessage::new(MsgType::Proposal, id as u64, vec![3]),
            );
        }
        assert_eq!(pending.len(), MAX_PENDING_MESSAGES);
        let mut pending = Vec::new();
        let size = mt_net_transport::codec::MAX_PROTOCOL_PAYLOAD_BYTES;
        for id in 0..16 {
            defer_message(
                &mut pending,
                ProtocolMessage::new(MsgType::Proposal, id, vec![3; size]),
            );
        }
        assert_eq!(
            pending.iter().map(|m| m.payload.len()).sum::<usize>(),
            MAX_PENDING_BYTES
        );
        let length = pending.len();
        defer_message(
            &mut pending,
            ProtocolMessage::new(MsgType::Proposal, 100, vec![4]),
        );
        assert_eq!(pending.len(), length);
    }

    #[test]
    fn a_network_failure_interrupts_hash_work_without_an_endpoint() {
        for iterations in [0, 1, 10] {
            let result = super::ssha_step_chunked(&[0; 32], iterations, "test", 1, || {
                Err(super::NodeError::Network("worker stopped".into()))
            });
            assert!(matches!(result, Err(super::NodeError::Network(_))));
        }
    }

    #[test]
    fn short_hash_work_matches_the_canonical_step_without_zero_sized_panics() {
        let previous = [0x37; 32];
        for iterations in 1..=21 {
            assert_eq!(
                super::ssha_step_chunked(&previous, iterations, "test", 1, || Ok(())).unwrap(),
                mt_timechain::ssha_step(&previous, iterations),
            );
        }
        for iterations in [0, mt_timechain::MAX_D + 1] {
            assert!(matches!(
                super::ssha_step_chunked(&previous, iterations, "test", 1, || Ok(())),
                Err(super::NodeError::InvalidArguments(_)),
            ));
        }
    }

    #[test]
    fn an_idle_network_is_distinct_from_a_dead_network() {
        for incoming_dies in [true, false] {
            let (broadcast_tx, broadcast_rx) = tokio::sync::mpsc::unbounded_channel();
            let (incoming_tx, incoming_rx) = tokio::sync::mpsc::unbounded_channel();
            let mut handle = super::NetworkHandle {
                broadcast_tx,
                incoming_rx,
            };
            assert!(matches!(handle.try_recv(), Ok(None)));
            if incoming_dies {
                drop(incoming_tx);
            } else {
                drop(broadcast_rx);
            }
            assert!(
                handle.try_recv().is_err(),
                "worker death must stop consensus"
            );
        }
    }

    #[test]
    fn buffered_message_does_not_hide_a_dead_network() {
        let (broadcast_tx, _broadcast_rx) = tokio::sync::mpsc::unbounded_channel();
        let (incoming_tx, incoming_rx) = tokio::sync::mpsc::unbounded_channel();
        incoming_tx
            .send(mt_net::ProtocolMessage::new(
                mt_net::MsgType::Ping,
                1,
                Vec::new(),
            ))
            .unwrap();
        let mut handle = super::NetworkHandle {
            broadcast_tx,
            incoming_rx,
        };
        drop(incoming_tx);
        assert!(handle.try_recv().is_err());
    }

    #[test]
    fn lag_threshold_override_resolution() {
        assert_eq!(resolve(None), DEFAULT);
        assert_eq!(resolve(Some("5".to_string())), 5);
        assert_eq!(resolve(Some("  7 ".to_string())), 7);
        assert_eq!(resolve(Some("0".to_string())), DEFAULT);
        assert_eq!(resolve(Some("abc".to_string())), DEFAULT);
        assert_eq!(resolve(Some(String::new())), DEFAULT);
    }

    fn node_cand(ticket: u128, id_byte: u8) -> Candidate {
        Candidate {
            ticket,
            class: WINNER_CLASS_NODE,
            id: [id_byte; 32],
        }
    }

    // Genesis cold-start: with an empty set W-2 (or in the first windows w<2)
    // the canonical proposer = NO_PROPOSER — exactly this value must go into the
    // header field proposer_node_id. The orchestration in that case proposes itself
    // (acting = my_node), but the HEADER carries NO_PROPOSER, to pass
    // validate_proposer_is_canonical against an empty W-2.
    #[test]
    fn cold_start_canonical_proposer_is_no_proposer() {
        // w < 2: before any windows have accumulated.
        assert_eq!(canonical_proposer_lookback(0, 1, &[]), (NO_PROPOSER, 1));
        assert_eq!(canonical_proposer_lookback(1, 1, &[]), (NO_PROPOSER, 1));
        // w >= 2 but W-2 is empty: no node-candidates → NO_PROPOSER.
        assert_eq!(canonical_proposer_lookback(2, 1, &[]), (NO_PROPOSER, 1));
    }

    // The orchestration (canonical header value) and the validator agree for
    // cold-start: a header with NO_PROPOSER passes against an empty W-2; a real
    // node_id against an empty W-2 is rejected.
    #[test]
    fn cold_start_header_value_agrees_with_validator() {
        let (canonical, _depth) = canonical_proposer_lookback(1, 1, &[]);
        assert_eq!(canonical, NO_PROPOSER);
        // A header with this canonical value passes validation.
        let mut h = stub_proposer_header(canonical);
        h.fallback_depth = 1;
        assert_eq!(validate_proposer_is_canonical(&h, &[]), Ok(()));
        // A forged real node_id with an empty W-2 is rejected.
        let mut forged = stub_proposer_header([0x42; 32]);
        forged.fallback_depth = 1;
        assert!(validate_proposer_is_canonical(&forged, &[]).is_err());
    }

    // Steady-state: the canonical proposer = the first node-candidate of W-2; a header
    // carrying this value passes validate_proposer_is_canonical against the same
    // W-2 candidates (header and validator agree by construction — one
    // source, the sorted W-2 candidates).
    #[test]
    fn steady_state_canonical_proposer_is_first_node_candidate() {
        let cands = vec![node_cand(100, 0x11), node_cand(200, 0x22)];
        let (canonical, depth) = canonical_proposer_lookback(5, 1, &cands);
        assert_eq!(canonical, [0x11u8; 32]);
        assert_eq!(depth, 1);
        let mut h = stub_proposer_header(canonical);
        h.fallback_depth = depth;
        assert_eq!(validate_proposer_is_canonical(&h, &cands), Ok(()));
    }

    fn stub_proposer_header(proposer: mt_state::NodeId) -> mt_consensus::ProposalHeader {
        use mt_crypto::{Signature, SIGNATURE_SIZE};
        mt_consensus::ProposalHeader {
            prev_proposal_hash: [0u8; 32],
            window_index: 1,
            protocol_version: 1,
            control_root: [0u8; 32],
            node_root: [0u8; 32],
            candidate_root: [0u8; 32],
            account_root: [0u8; 32],
            state_root: [0u8; 32],
            timechain_value: [0u8; 32],
            included_bundles_root: [0u8; 32],
            included_reveals_root: [0u8; 32],
            winner_endpoint: [0u8; 32],
            winner_id: [0u8; 32],
            proposer_node_id: proposer,
            target: 0,
            fallback_depth: 1,
            signature: Signature::from_array([0u8; SIGNATURE_SIZE]),
        }
    }
}
