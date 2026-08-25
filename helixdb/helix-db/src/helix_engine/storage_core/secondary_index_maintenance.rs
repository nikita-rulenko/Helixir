//! Restart-safe maintenance for configured LMDB secondary indices.

use super::HelixGraphStorage;
use crate::{
    helix_engine::types::{GraphError, SecondaryIndex},
    utils::items::Node,
};
use bumpalo::Bump;
use heed3::PutFlags;

const STAMP_PREFIX: &[u8] = b"helixir_secondary_index_v1:";
const STAMP_VALUE: &[u8] = b"complete";

#[derive(Debug, Default, Eq, PartialEq)]
pub(crate) struct ReindexReport {
    pub rebuilt_indices: usize,
    pub scanned_nodes: usize,
    pub inserted_entries: usize,
}

struct IndexEntry {
    index_name: String,
    key: Vec<u8>,
    node_id: u128,
}

impl HelixGraphStorage {
    /// Rebuild configured indices that do not carry a completion stamp.
    ///
    /// Upstream v2 creates a new LMDB table when an index appears in the
    /// schema, but it does not populate that table from existing nodes. This
    /// pass scans before the gateway accepts requests and publishes every
    /// rebuilt table plus its stamp in one write transaction.
    pub(crate) fn ensure_secondary_indices(
        &self,
        force: bool,
    ) -> Result<ReindexReport, GraphError> {
        let read_txn = self.graph_env.read_txn()?;
        let mut targets = Vec::new();
        for (name, (_, kind)) in &self.secondary_indices {
            let stamp = stamp_key(name, kind);
            let complete = self.metadata_db.get(&read_txn, &stamp)? == Some(STAMP_VALUE);
            if force || !complete {
                targets.push((name.clone(), kind.clone()));
            }
        }

        if targets.is_empty() {
            return Ok(ReindexReport::default());
        }

        let mut arena = Bump::new();
        let mut entries = Vec::new();
        let mut scanned_nodes = 0usize;
        for item in self.nodes_db.iter(&read_txn)? {
            let (node_id, bytes) = item?;
            arena.reset();
            let node = Node::from_bincode_bytes(node_id, bytes, &arena)?;
            let node = self.version_info.upgrade_to_node_latest(node);
            scanned_nodes += 1;

            for (name, _) in &targets {
                let Some(value) = node.get_property(name) else {
                    continue;
                };
                entries.push(IndexEntry {
                    index_name: name.clone(),
                    key: bincode::serialize(value)?,
                    node_id,
                });
            }
        }
        drop(read_txn);

        let mut write_txn = self.graph_env.write_txn()?;
        for (name, _) in &targets {
            let (database, _) = self.secondary_indices.get(name).ok_or_else(|| {
                GraphError::New(format!("Secondary Index {name} disappeared during rebuild"))
            })?;
            database.clear(&mut write_txn)?;
        }

        for entry in &entries {
            let (database, kind) =
                self.secondary_indices
                    .get(&entry.index_name)
                    .ok_or_else(|| {
                        GraphError::New(format!(
                            "Secondary Index {} disappeared during rebuild",
                            entry.index_name
                        ))
                    })?;
            match kind {
                SecondaryIndex::Unique(_) => database.put_with_flags(
                    &mut write_txn,
                    PutFlags::NO_OVERWRITE,
                    &entry.key,
                    &entry.node_id,
                )?,
                SecondaryIndex::Index(_) => {
                    database.put(&mut write_txn, &entry.key, &entry.node_id)?
                }
                SecondaryIndex::None => unreachable!("None indices are never configured"),
            }
        }

        for (name, kind) in &targets {
            self.metadata_db
                .put(&mut write_txn, &stamp_key(name, kind), STAMP_VALUE)?;
        }
        write_txn.commit()?;

        Ok(ReindexReport {
            rebuilt_indices: targets.len(),
            scanned_nodes,
            inserted_entries: entries.len(),
        })
    }
}

fn stamp_key(name: &str, kind: &SecondaryIndex) -> Vec<u8> {
    let discriminator = match kind {
        SecondaryIndex::Unique(_) => b"unique:".as_slice(),
        SecondaryIndex::Index(_) => b"index:".as_slice(),
        SecondaryIndex::None => b"none:".as_slice(),
    };
    let mut key = Vec::with_capacity(STAMP_PREFIX.len() + discriminator.len() + name.len());
    key.extend_from_slice(STAMP_PREFIX);
    key.extend_from_slice(discriminator);
    key.extend_from_slice(name.as_bytes());
    key
}
