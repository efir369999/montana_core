use mt_crypto::Hash32;
use mt_net::{FastSyncResponseChunk, TableId};
use mt_sync::{FastSyncChunk, FastSyncTableId};
use std::collections::BTreeMap;

// Transport data cannot authenticate its own checkpoint. Only roots recorded
// after canonical application may enter this constructor's verified map.
pub(super) struct FastSyncSession {
    pub(super) client: mt_sync::FastSyncClient,
    pub(super) request_id: u64,
    pub(super) window: u64,
    root: Hash32,
    pub(super) timechain: Hash32,
}

impl FastSyncSession {
    pub(super) fn from_verified(
        request_id: u64,
        window: u64,
        advertised_root: Hash32,
        verified_roots: &BTreeMap<u64, Hash32>,
        verified_clocks: &BTreeMap<u64, Hash32>,
    ) -> Option<Self> {
        let root = *verified_roots.get(&window)?;
        if root != advertised_root {
            return None;
        }
        Some(Self {
            client: mt_sync::FastSyncClient::new(),
            request_id,
            window,
            root,
            timechain: *verified_clocks.get(&window)?,
        })
    }

    pub(super) fn matches(&self, request_id: u64, window: u64) -> bool {
        request_id == self.request_id && window == self.window
    }

    pub(super) fn finalize(
        self,
    ) -> Result<(u64, mt_sync::snapshot::TypedTables), mt_sync::FastSyncClientError> {
        self.client
            .finalize(&BTreeMap::from([(self.window, self.root)]))
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum WireChunkError {
    RecordCountZero,
    EmptyRecords,
    RecordsIndivisible { len: usize, count: usize },
}

// Conversion of a network fast-sync chunk into an mt-sync chunk: splits the flat
// concatenation of records into record_count equal records. The canonical record
// size per table is not duplicated here — Snapshot::add_record checks it
// itself (single source of truth for the size). The TableId mapping is total; the
// Proposals table reaches the client and is rejected there (ProposalsNotImplementedYet).
pub fn wire_chunk_to_sync(wire: FastSyncResponseChunk) -> Result<FastSyncChunk, WireChunkError> {
    let table_id = match wire.table_id {
        TableId::Account => FastSyncTableId::Account,
        TableId::Node => FastSyncTableId::Node,
        TableId::Candidate => FastSyncTableId::Candidate,
        TableId::Proposals => FastSyncTableId::Proposals,
    };
    let count = wire.record_count as usize;
    if count == 0 {
        return Err(WireChunkError::RecordCountZero);
    }
    if wire.records.is_empty() {
        return Err(WireChunkError::EmptyRecords);
    }
    if wire.records.len() % count != 0 {
        return Err(WireChunkError::RecordsIndivisible {
            len: wire.records.len(),
            count,
        });
    }
    let rec_size = wire.records.len() / count;
    let records: Vec<Vec<u8>> = wire
        .records
        .chunks_exact(rec_size)
        .map(<[u8]>::to_vec)
        .collect();
    Ok(FastSyncChunk {
        chunk_index: wire.chunk_index,
        total_chunks: wire.total_chunks,
        table_id,
        anchor_window: wire.anchor_window,
        records,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mt_codec::CanonicalEncode;
    use mt_crypto::PUBLIC_KEY_SIZE;
    use mt_state::{AccountRecord, ACCOUNT_RECORD_SIZE};

    #[test]
    fn a_remote_head_cannot_create_or_move_its_own_snapshot_anchor() {
        let mut roots = BTreeMap::new();
        let mut clocks = BTreeMap::new();
        assert!(FastSyncSession::from_verified(7, 42, [1; 32], &roots, &clocks).is_none());
        roots.insert(42, [1; 32]);
        assert!(FastSyncSession::from_verified(7, 42, [1; 32], &roots, &clocks).is_none());
        clocks.insert(42, [3; 32]);
        assert!(FastSyncSession::from_verified(7, 42, [2; 32], &roots, &clocks).is_none());
        assert!(FastSyncSession::from_verified(7, 43, [1; 32], &roots, &clocks).is_none());
        let session = FastSyncSession::from_verified(7, 42, [1; 32], &roots, &clocks).unwrap();
        assert!(session.matches(7, 42));
        assert!(!session.matches(8, 42));
        assert!(!session.matches(7, 43));
        roots.insert(42, [2; 32]);
        assert_eq!(
            session.root, [1; 32],
            "in-flight verification is pinned to its anchor"
        );
    }

    fn acct(seed: u8) -> Vec<u8> {
        let rec = AccountRecord {
            account_id: [seed; 32],
            balance: 1000,
            suite_id: 1,
            is_node_operator: false,
            frontier_hash: [seed; 32],
            op_height: 0,
            account_chain_length: 0,
            account_chain_length_snapshot: 0,
            current_pubkey: [seed; PUBLIC_KEY_SIZE],
            creation_window: 0,
            last_op_window: 0,
            last_activation_window: 0,
        };
        let mut buf = Vec::with_capacity(ACCOUNT_RECORD_SIZE);
        rec.encode(&mut buf);
        buf
    }

    fn flat(records: &[Vec<u8>]) -> Vec<u8> {
        let mut f = Vec::new();
        for r in records {
            f.extend_from_slice(r);
        }
        f
    }

    #[test]
    fn splits_flat_records_preserving_bytes() {
        let recs = vec![acct(0x11), acct(0x22), acct(0x33)];
        let wire = FastSyncResponseChunk {
            chunk_index: 2,
            total_chunks: 5,
            table_id: TableId::Account,
            record_count: 3,
            anchor_window: 0,
            records: flat(&recs),
        };
        let out = wire_chunk_to_sync(wire).expect("convert");
        assert_eq!(out.chunk_index, 2);
        assert_eq!(out.total_chunks, 5);
        assert_eq!(out.table_id, FastSyncTableId::Account);
        assert_eq!(out.records, recs);
    }

    #[test]
    fn maps_every_table_id() {
        for (net, sync) in [
            (TableId::Account, FastSyncTableId::Account),
            (TableId::Node, FastSyncTableId::Node),
            (TableId::Candidate, FastSyncTableId::Candidate),
            (TableId::Proposals, FastSyncTableId::Proposals),
        ] {
            let wire = FastSyncResponseChunk {
                chunk_index: 0,
                total_chunks: 1,
                table_id: net,
                record_count: 1,
                anchor_window: 0,
                records: vec![0u8; 8],
            };
            assert_eq!(wire_chunk_to_sync(wire).unwrap().table_id, sync);
        }
    }

    #[test]
    fn rejects_zero_record_count() {
        let wire = FastSyncResponseChunk {
            chunk_index: 0,
            total_chunks: 1,
            table_id: TableId::Account,
            record_count: 0,
            anchor_window: 0,
            records: vec![0u8; 10],
        };
        assert_eq!(
            wire_chunk_to_sync(wire),
            Err(WireChunkError::RecordCountZero)
        );
    }

    #[test]
    fn rejects_empty_records() {
        let wire = FastSyncResponseChunk {
            chunk_index: 0,
            total_chunks: 1,
            table_id: TableId::Account,
            record_count: 2,
            anchor_window: 0,
            records: Vec::new(),
        };
        assert_eq!(wire_chunk_to_sync(wire), Err(WireChunkError::EmptyRecords));
    }

    #[test]
    fn rejects_indivisible_records() {
        let wire = FastSyncResponseChunk {
            chunk_index: 0,
            total_chunks: 1,
            table_id: TableId::Account,
            record_count: 3,
            anchor_window: 0,
            records: vec![0u8; 10],
        };
        assert_eq!(
            wire_chunk_to_sync(wire),
            Err(WireChunkError::RecordsIndivisible { len: 10, count: 3 })
        );
    }
}
