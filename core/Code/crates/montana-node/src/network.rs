// spec, section "Network layer → Cross-machine peering" (M8) — Noise_PQ XX
// is the production transport. PeerId is derived from each peer's ML-DSA-65
// identity public key via SHA-256 multihash (sha2-256 multihash code 0x12).
//
// Spec connection lifecycle:
//   TCP → Noise_PQ XX handshake (ML-KEM-768 + ML-DSA-65) → Yamux multiplex
//
// The libp2p Ed25519 keypair is still required by SwarmBuilder for its
// internal book-keeping, but the cross-network identity is the ML-DSA-65 pk —
// not the Ed25519 pk. Each node's GenesisManifest peer_id MUST be the
// ML-DSA-derived multihash to align with what the XX upgrade returns.

use std::str::FromStr;
use std::time::Duration;

use futures::StreamExt;
use libp2p::request_response::{Event as RrEvent, Message as RrMessage};
use libp2p::swarm::SwarmEvent;
use libp2p::{Multiaddr, PeerId, Swarm};
use mt_crypto::{PublicKey as MtPublicKey, SecretKey as MtSecretKey};
use mt_genesis::GenesisManifest;
use mt_net::{MsgType, ProtocolMessage};
use mt_net_transport::{
    build_swarm_with_keypair, MontanaBehaviour, MontanaBehaviourEvent, NetworkConfig,
};

use crate::identity::NodeError;

const HEARTBEAT_PERIOD: Duration = Duration::from_secs(5);

struct DialPeer {
    id: PeerId,
    address: Multiaddr,
}

// Parse every entry, including our own. An example manifest must fail startup,
// rather than leave the consensus loop running after its network thread exits.
fn prepare_peers(
    manifest: &GenesisManifest,
    local_peer_id: PeerId,
) -> Result<Vec<DialPeer>, NodeError> {
    manifest
        .validate()
        .map_err(|e| NodeError::Network(format!("manifest: {e}")))?;
    let mut peers = Vec::new();
    for (index, peer) in manifest.peers.iter().enumerate() {
        let id = PeerId::from_str(&peer.peer_id)
            .map_err(|_| NodeError::Network(format!("manifest peer {index}: invalid peer_id")))?;
        let mut address = Multiaddr::from_str(&peer.multiaddr)
            .map_err(|_| NodeError::Network(format!("manifest peer {index}: invalid multiaddr")))?;
        if address.is_empty()
            || address
                .iter()
                .any(|part| matches!(part, libp2p::multiaddr::Protocol::P2p(_)))
        {
            return Err(NodeError::Network(format!(
                "manifest peer {index}: expected a transport address without /p2p"
            )));
        }
        address.push(libp2p::multiaddr::Protocol::P2p(id));
        if id != local_peer_id {
            peers.push(DialPeer { id, address });
        }
    }
    Ok(peers)
}

#[allow(clippy::too_many_arguments)]
pub async fn run_network_loop(
    local_keypair: libp2p::identity::Keypair,
    local_peer_id: PeerId,
    mldsa_id_pk: MtPublicKey,
    mldsa_id_sk: MtSecretKey,
    manifest: GenesisManifest,
    listen_addr: Multiaddr,
    mut broadcast_rx: tokio::sync::mpsc::UnboundedReceiver<ProtocolMessage>,
    incoming_tx: tokio::sync::mpsc::UnboundedSender<ProtocolMessage>,
    ready_tx: std::sync::mpsc::Sender<()>,
) -> Result<(), NodeError> {
    let peers = prepare_peers(&manifest, local_peer_id)?;
    let cfg = NetworkConfig {
        listen_addrs: vec![listen_addr.clone()],
        max_inbound: 13,
        max_outbound: 24,
    };

    let mut swarm: Swarm<MontanaBehaviour> = build_swarm_with_keypair(
        local_keypair,
        MontanaBehaviour::new(),
        &cfg,
        mldsa_id_pk,
        mldsa_id_sk,
    )
    .map_err(|e| NodeError::Network(format!("build swarm: {e}")))?;

    eprintln!(
        "[network] starting transport; peers_to_dial={}",
        peers.len()
    );
    let mut redial_targets = Vec::new();
    for peer in peers {
        swarm
            .dial(peer.address.clone())
            .map_err(|_| NodeError::Network("initial peer dial refused".into()))?;
        redial_targets.push((peer.id, peer.address));
    }

    let mut heartbeat = tokio::time::interval(HEARTBEAT_PERIOD);
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut request_id_counter: u64 = 0;
    let mut connected_peers = std::collections::HashSet::new();
    let mut ready_tx = Some(ready_tx);

    loop {
        tokio::select! {
            ev = swarm.select_next_some() => {
                match ev {
                    SwarmEvent::NewListenAddr { .. } => {
                        if let Some(ready) = ready_tx.take() {
                            ready.send(()).map_err(|_| NodeError::Network("startup caller disconnected".into()))?;
                        }
                        eprintln!("[network] listener ready");
                    }
                    SwarmEvent::ListenerClosed { .. } => {
                        return Err(NodeError::Network("network listener closed".into()));
                    }
                    SwarmEvent::ListenerError { .. } => {
                        eprintln!("[network] recoverable listener error");
                    }
                    SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                        connected_peers.insert(peer_id);
                        eprintln!("[network] connection established; connected={}", connected_peers.len());
                        request_id_counter = request_id_counter.wrapping_add(1);
                        let initial_ping = ProtocolMessage::new(
                            MsgType::Ping,
                            request_id_counter,
                            Vec::new(),
                        );
                        swarm.behaviour_mut().request_response.send_request(&peer_id, initial_ping);
                    }
                    SwarmEvent::ConnectionClosed { peer_id, .. } => {
                        connected_peers.remove(&peer_id);
                        eprintln!("[network] connection closed; connected={}", connected_peers.len());
                    }
                    SwarmEvent::OutgoingConnectionError { .. } => {
                        eprintln!("[network] outgoing connection failed");
                    }
                    SwarmEvent::Behaviour(MontanaBehaviourEvent::RequestResponse(
                        RrEvent::Message {
                            peer: _,
                            message: RrMessage::Request { request, channel, .. },
                            ..
                        },
                    )) => {
                        if request.msg_type == MsgType::Ping {
                            let pong = ProtocolMessage::new(MsgType::Pong, request.request_id, Vec::new());
                            if swarm.behaviour_mut().request_response.send_response(channel, pong).is_err() {
                                eprintln!("[network] heartbeat response failed");
                            }
                        } else {
                            if incoming_tx.send(request.clone()).is_err() {
                                return Err(NodeError::Network("consensus receiver disconnected".into()));
                            }
                            let ack = ProtocolMessage::new(MsgType::Pong, request.request_id, Vec::new());
                            let _ = swarm.behaviour_mut().request_response.send_response(channel, ack);
                        }
                    }
                    SwarmEvent::Behaviour(MontanaBehaviourEvent::RequestResponse(
                        RrEvent::Message {
                            message: RrMessage::Response { response, .. },
                            ..
                        },
                    )) if response.msg_type == MsgType::Pong => {}
                    SwarmEvent::Behaviour(MontanaBehaviourEvent::RequestResponse(_)) => {
                        eprintln!("[network] request-response event");
                    }
                    _ => {}
                }
            }
            _ = heartbeat.tick() => {
                for (peer_id, addr) in &redial_targets {
                    if !connected_peers.contains(peer_id) {
                        let _ = swarm.dial(addr.clone());
                    }
                }
                let peers: Vec<PeerId> = connected_peers.iter().copied().collect();
                for peer_id in peers {
                    request_id_counter = request_id_counter.wrapping_add(1);
                    let ping = ProtocolMessage::new(
                        MsgType::Ping,
                        request_id_counter,
                        Vec::new(),
                    );
                    swarm.behaviour_mut().request_response.send_request(&peer_id, ping);
                }
            }
            Some(broadcast_msg) = broadcast_rx.recv() => {
                let peers: Vec<PeerId> = connected_peers.iter().copied().collect();
                for peer_id in peers {
                    swarm.behaviour_mut().request_response.send_request(&peer_id, broadcast_msg.clone());
                }

            }
            _ = tokio::signal::ctrl_c() => {
                eprintln!("[network] Ctrl-C, exiting");
                return Ok(());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mt_genesis::GenesisPeer;

    fn manifest() -> (GenesisManifest, PeerId, PeerId) {
        let local = libp2p::identity::Keypair::generate_ed25519()
            .public()
            .to_peer_id();
        let remote = libp2p::identity::Keypair::generate_ed25519()
            .public()
            .to_peer_id();
        let peers = [local, remote]
            .into_iter()
            .enumerate()
            .map(|(index, id)| GenesisPeer {
                label: format!("peer-{index}"),
                multiaddr: "/ip4/127.0.0.1/tcp/8444".into(),
                peer_id: id.to_string(),
                account_id_hex: if index == 0 { "1" } else { "2" }.repeat(64),
                node_id_hex: if index == 0 { "a" } else { "b" }.repeat(64),
                bootstrap: index == 0,
            })
            .collect();
        (
            GenesisManifest {
                network_name: "test".into(),
                peers,
            },
            local,
            remote,
        )
    }

    #[test]
    fn an_example_address_is_rejected_even_in_the_local_entry() {
        for index in [0, 1] {
            let (mut manifest, local, _) = manifest();
            manifest.peers[index].multiaddr = "/ip4/<front>/tcp/8444".into();
            let error = prepare_peers(&manifest, local).err().expect("must reject");
            assert!(
                !error.to_string().contains("<front>"),
                "do not echo operator endpoints"
            );
        }
    }

    #[test]
    fn invalid_and_embedded_peer_ids_are_rejected() {
        let (mut manifest, local, remote) = manifest();
        manifest.peers[1].multiaddr = format!("/ip4/127.0.0.1/tcp/8444/p2p/{remote}");
        assert!(prepare_peers(&manifest, local).is_err());
        manifest.peers[1].multiaddr = "/ip4/127.0.0.1/tcp/8444".into();
        manifest.peers[1].peer_id = "not-a-peer-id".into();
        assert!(prepare_peers(&manifest, local).is_err());
    }

    #[test]
    fn the_prepared_target_pins_the_remote_identity_once() {
        let (manifest, local, remote) = manifest();
        let peers = prepare_peers(&manifest, local).expect("valid manifest");
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].id, remote);
        assert_eq!(
            peers[0].address.to_string(),
            format!("/ip4/127.0.0.1/tcp/8444/p2p/{remote}")
        );
    }
}
