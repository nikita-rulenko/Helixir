use crate::{
    debug_println,
    helix_engine::{
        storage_core::HelixGraphStorage,
        types::GraphError,
        vector_core::{hnsw::HNSW, vector::HVector},
    },
    protocol::value::Value,
    utils::properties::ImmutablePropertiesMap,
};

use bumpalo::{
    Bump,
    collections::{String as BString, Vec as BVec},
};
use heed3::{Database, DatabaseFlags, Env, RoTxn, RwTxn, byteorder::BE, types::*};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::task;

const DB_BM25_INVERTED_INDEX: &str = "bm25_inverted_index"; // term -> list of (doc_id, tf)
const DB_BM25_REVERSE_INDEX: &str = "bm25_reverse_index"; // doc_id -> list of (term, tf)
const DB_BM25_DOC_LENGTHS: &str = "bm25_doc_lengths"; // doc_id -> document length
const DB_BM25_TERM_FREQUENCIES: &str = "bm25_term_frequencies"; // term -> document frequency
const DB_BM25_METADATA: &str = "bm25_metadata"; // stores total docs, avgdl, etc.
pub const METADATA_KEY: &[u8] = b"metadata";
pub const BM25_SCHEMA_VERSION_KEY: &[u8] = b"schema_version";
pub const BM25_SCHEMA_VERSION: u64 = 2;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BM25Metadata {
    pub total_docs: u64,
    pub avgdl: f64,
    pub k1: f32, // controls term frequency saturation
    pub b: f32,  // controls document length normalization
}

/// For inverted index
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PostingListEntry {
    pub doc_id: u128,
    pub term_frequency: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct ReversePostingEntry {
    pub term: String,
    pub term_frequency: u32,
}

enum DocPresence {
    Absent,
    Empty,
    Indexed(u32),
}

enum DocState {
    Absent,
    Empty,
    Indexed {
        doc_length: u32,
        reverse_entries: Vec<ReversePostingEntry>,
    },
}

pub trait BM25 {
    fn tokenize<const SHOULD_FILTER: bool>(&self, text: &str) -> Vec<String>;

    fn insert_doc(&self, txn: &mut RwTxn, doc_id: u128, doc: &str) -> Result<(), GraphError>;

    fn delete_doc(&self, txn: &mut RwTxn, doc_id: u128) -> Result<(), GraphError>;

    fn update_doc(&self, txn: &mut RwTxn, doc_id: u128, doc: &str) -> Result<(), GraphError>;

    /// Calculate the BM25 score for a single term of a query (no sum)
    fn calculate_bm25_score(
        &self,
        tf: u32,         // term frequency
        doc_len: u32,    // document length
        df: u32,         // document frequency
        total_docs: u64, // total documents
        avgdl: f64,      // average document length
    ) -> f32;

    fn search(
        &self,
        txn: &RoTxn,
        query: &str,
        limit: usize,
        arena: &Bump,
    ) -> Result<Vec<(u128, f32)>, GraphError>;
}

pub struct HBM25Config {
    pub graph_env: Env,
    pub inverted_index_db: Database<Bytes, Bytes>,
    pub reverse_index_db: Database<U128<BE>, Bytes>,
    pub doc_lengths_db: Database<U128<BE>, U32<BE>>,
    pub term_frequencies_db: Database<Bytes, U32<BE>>,
    pub metadata_db: Database<Bytes, Bytes>,
    k1: f64,
    b: f64,
}

impl HBM25Config {
    pub fn new(graph_env: &Env, wtxn: &mut RwTxn) -> Result<HBM25Config, GraphError> {
        let inverted_index_db: Database<Bytes, Bytes> = graph_env
            .database_options()
            .types::<Bytes, Bytes>()
            .flags(DatabaseFlags::DUP_SORT)
            .name(DB_BM25_INVERTED_INDEX)
            .create(wtxn)?;

        let reverse_index_db: Database<U128<BE>, Bytes> = graph_env
            .database_options()
            .types::<U128<BE>, Bytes>()
            .flags(DatabaseFlags::DUP_SORT)
            .name(DB_BM25_REVERSE_INDEX)
            .create(wtxn)?;

        let doc_lengths_db: Database<U128<BE>, U32<BE>> = graph_env
            .database_options()
            .types::<U128<BE>, U32<BE>>()
            .name(DB_BM25_DOC_LENGTHS)
            .create(wtxn)?;

        let term_frequencies_db: Database<Bytes, U32<BE>> = graph_env
            .database_options()
            .types::<Bytes, U32<BE>>()
            .name(DB_BM25_TERM_FREQUENCIES)
            .create(wtxn)?;

        let metadata_db: Database<Bytes, Bytes> = graph_env
            .database_options()
            .types::<Bytes, Bytes>()
            .name(DB_BM25_METADATA)
            .create(wtxn)?;

        Ok(HBM25Config {
            graph_env: graph_env.clone(),
            inverted_index_db,
            reverse_index_db,
            doc_lengths_db,
            term_frequencies_db,
            metadata_db,
            k1: 1.2,
            b: 0.75,
        })
    }

    pub fn new_temp(
        graph_env: &Env,
        wtxn: &mut RwTxn,
        uuid: &str,
    ) -> Result<HBM25Config, GraphError> {
        let inverted_index_db: Database<Bytes, Bytes> = graph_env
            .database_options()
            .types::<Bytes, Bytes>()
            .flags(DatabaseFlags::DUP_SORT)
            .name(format!("{DB_BM25_INVERTED_INDEX}_{uuid}").as_str())
            .create(wtxn)?;

        let reverse_index_db: Database<U128<BE>, Bytes> = graph_env
            .database_options()
            .types::<U128<BE>, Bytes>()
            .flags(DatabaseFlags::DUP_SORT)
            .name(format!("{DB_BM25_REVERSE_INDEX}_{uuid}").as_str())
            .create(wtxn)?;

        let doc_lengths_db: Database<U128<BE>, U32<BE>> = graph_env
            .database_options()
            .types::<U128<BE>, U32<BE>>()
            .name(format!("{DB_BM25_DOC_LENGTHS}_{uuid}").as_str())
            .create(wtxn)?;

        let term_frequencies_db: Database<Bytes, U32<BE>> = graph_env
            .database_options()
            .types::<Bytes, U32<BE>>()
            .name(format!("{DB_BM25_TERM_FREQUENCIES}_{uuid}").as_str())
            .create(wtxn)?;

        let metadata_db: Database<Bytes, Bytes> = graph_env
            .database_options()
            .types::<Bytes, Bytes>()
            .name(format!("{DB_BM25_METADATA}_{uuid}").as_str())
            .create(wtxn)?;

        Ok(HBM25Config {
            graph_env: graph_env.clone(),
            inverted_index_db,
            reverse_index_db,
            doc_lengths_db,
            term_frequencies_db,
            metadata_db,
            k1: 1.2,
            b: 0.75,
        })
    }

    fn default_metadata(&self) -> BM25Metadata {
        BM25Metadata {
            total_docs: 0,
            avgdl: 0.0,
            k1: self.k1 as f32,
            b: self.b as f32,
        }
    }

    pub fn clear_all(&self, txn: &mut RwTxn) -> Result<(), GraphError> {
        self.inverted_index_db.clear(txn)?;
        self.reverse_index_db.clear(txn)?;
        self.doc_lengths_db.clear(txn)?;
        self.term_frequencies_db.clear(txn)?;
        self.metadata_db.clear(txn)?;
        Ok(())
    }

    pub fn schema_version(&self, txn: &RoTxn) -> Result<Option<u64>, GraphError> {
        let Some(bytes) = self.metadata_db.get(txn, BM25_SCHEMA_VERSION_KEY)? else {
            return Ok(None);
        };

        let Ok(version_bytes) = <[u8; std::mem::size_of::<u64>()]>::try_from(bytes) else {
            return Ok(None);
        };

        Ok(Some(u64::from_le_bytes(version_bytes)))
    }

    pub fn write_schema_version(&self, txn: &mut RwTxn, version: u64) -> Result<(), GraphError> {
        self.metadata_db
            .put(txn, BM25_SCHEMA_VERSION_KEY, &version.to_le_bytes())?;
        Ok(())
    }

    pub fn reverse_entries(
        &self,
        txn: &RoTxn,
        doc_id: u128,
    ) -> Result<Vec<ReversePostingEntry>, GraphError> {
        let mut entries = Vec::new();
        if let Some(duplicates) = self.reverse_index_db.get_duplicates(txn, &doc_id)? {
            for result in duplicates {
                let (_, entry_bytes) = result?;
                entries.push(bincode::deserialize(entry_bytes)?);
            }
        }
        Ok(entries)
    }

    fn reverse_entries_rw(
        &self,
        txn: &RwTxn,
        doc_id: u128,
    ) -> Result<Vec<ReversePostingEntry>, GraphError> {
        let mut entries = Vec::new();
        if let Some(duplicates) = self.reverse_index_db.get_duplicates(txn, &doc_id)? {
            for result in duplicates {
                let (_, entry_bytes) = result?;
                entries.push(bincode::deserialize(entry_bytes)?);
            }
        }
        Ok(entries)
    }

    fn has_reverse_entries(&self, txn: &RoTxn, doc_id: u128) -> Result<bool, GraphError> {
        Ok(self
            .reverse_index_db
            .get_duplicates(txn, &doc_id)?
            .is_some())
    }

    fn classify_doc_presence(
        doc_id: u128,
        doc_length: Option<u32>,
        has_reverse_entries: bool,
    ) -> Result<DocPresence, GraphError> {
        match doc_length {
            None if !has_reverse_entries => Ok(DocPresence::Absent),
            None => Err(GraphError::New(format!(
                "BM25 reverse index exists without doc length for document {doc_id}"
            ))),
            Some(0) if !has_reverse_entries => Ok(DocPresence::Empty),
            Some(0) => Err(GraphError::New(format!(
                "BM25 zero-length document {doc_id} has reverse entries"
            ))),
            Some(doc_length) if has_reverse_entries => Ok(DocPresence::Indexed(doc_length)),
            Some(doc_length) => Err(GraphError::New(format!(
                "BM25 document {doc_id} has doc length {doc_length} but no reverse entries"
            ))),
        }
    }

    fn read_metadata(&self, txn: &RwTxn) -> Result<Option<BM25Metadata>, GraphError> {
        self.metadata_db
            .get(txn, METADATA_KEY)?
            .map(bincode::deserialize)
            .transpose()
            .map_err(GraphError::from)
    }

    fn write_metadata(&self, txn: &mut RwTxn, metadata: &BM25Metadata) -> Result<(), GraphError> {
        let metadata_bytes = bincode::serialize(metadata)?;
        self.metadata_db.put(txn, METADATA_KEY, &metadata_bytes)?;
        Ok(())
    }

    fn require_metadata(&self, txn: &RwTxn, doc_id: u128) -> Result<BM25Metadata, GraphError> {
        self.read_metadata(txn)?.ok_or_else(|| {
            GraphError::New(format!(
                "BM25 metadata missing for indexed document {doc_id}"
            ))
        })
    }

    fn flush_token<const SHOULD_FILTER: bool>(
        term_counts: &mut HashMap<String, u32>,
        token: &mut String,
    ) {
        if token.is_empty() {
            return;
        }

        if SHOULD_FILTER && token.len() <= 2 {
            token.clear();
            return;
        }

        if let Some(count) = term_counts.get_mut(token.as_str()) {
            *count += 1;
            token.clear();
        } else {
            term_counts.insert(std::mem::take(token), 1);
        }
    }

    fn add_text_term_counts<const SHOULD_FILTER: bool>(
        &self,
        term_counts: &mut HashMap<String, u32>,
        text: &str,
    ) {
        let mut token = String::new();

        for ch in text.chars() {
            for lower in ch.to_lowercase() {
                if lower.is_alphanumeric() {
                    token.push(lower);
                } else {
                    Self::flush_token::<SHOULD_FILTER>(term_counts, &mut token);
                }
            }
        }

        Self::flush_token::<SHOULD_FILTER>(term_counts, &mut token);
    }

    fn add_value_term_counts(
        &self,
        term_counts: &mut HashMap<String, u32>,
        value: &Value,
    ) -> Result<(), GraphError> {
        match value {
            Value::String(s) => {
                self.add_text_term_counts::<true>(term_counts, s);
                Ok(())
            }
            Value::F32(f) => {
                self.add_text_term_counts::<true>(term_counts, &f.to_string());
                Ok(())
            }
            Value::F64(f) => {
                self.add_text_term_counts::<true>(term_counts, &f.to_string());
                Ok(())
            }
            Value::I8(i) => {
                self.add_text_term_counts::<true>(term_counts, &i.to_string());
                Ok(())
            }
            Value::I16(i) => {
                self.add_text_term_counts::<true>(term_counts, &i.to_string());
                Ok(())
            }
            Value::I32(i) => {
                self.add_text_term_counts::<true>(term_counts, &i.to_string());
                Ok(())
            }
            Value::I64(i) => {
                self.add_text_term_counts::<true>(term_counts, &i.to_string());
                Ok(())
            }
            Value::U8(u) => {
                self.add_text_term_counts::<true>(term_counts, &u.to_string());
                Ok(())
            }
            Value::U16(u) => {
                self.add_text_term_counts::<true>(term_counts, &u.to_string());
                Ok(())
            }
            Value::U32(u) => {
                self.add_text_term_counts::<true>(term_counts, &u.to_string());
                Ok(())
            }
            Value::U64(u) => {
                self.add_text_term_counts::<true>(term_counts, &u.to_string());
                Ok(())
            }
            Value::U128(u) => {
                self.add_text_term_counts::<true>(term_counts, &u.to_string());
                Ok(())
            }
            Value::Date(d) => {
                self.add_text_term_counts::<true>(term_counts, &d.to_string());
                Ok(())
            }
            Value::Boolean(b) => {
                self.add_text_term_counts::<true>(term_counts, if *b { "true" } else { "false" });
                Ok(())
            }
            Value::Id(id) => {
                self.add_text_term_counts::<true>(term_counts, &id.stringify());
                Ok(())
            }
            Value::Array(values) => {
                for value in values {
                    self.add_value_term_counts(term_counts, value)?;
                }
                Ok(())
            }
            Value::Object(entries) => {
                for (key, value) in entries {
                    self.add_text_term_counts::<true>(term_counts, key);
                    self.add_value_term_counts(term_counts, value)?;
                }
                Ok(())
            }
            Value::Empty => Err(GraphError::New(
                "BM25: unexpected empty value in node properties".to_string(),
            )),
        }
    }

    pub(crate) fn term_counts_for_node(
        &self,
        properties: &ImmutablePropertiesMap<'_>,
        label: &str,
    ) -> Result<HashMap<String, u32>, GraphError> {
        let mut term_counts = HashMap::new();

        for (key, value) in properties.iter() {
            self.add_text_term_counts::<true>(&mut term_counts, key);
            self.add_value_term_counts(&mut term_counts, value)?;
        }

        self.add_text_term_counts::<true>(&mut term_counts, label);
        Ok(term_counts)
    }

    fn term_counts(&self, doc: &str) -> HashMap<String, u32> {
        let mut term_counts = HashMap::new();
        self.add_text_term_counts::<true>(&mut term_counts, doc);
        term_counts
    }

    fn reverse_entries_from_term_counts(
        term_counts: &HashMap<String, u32>,
    ) -> Vec<ReversePostingEntry> {
        let mut entries = term_counts
            .iter()
            .map(|(term, term_frequency)| ReversePostingEntry {
                term: term.clone(),
                term_frequency: *term_frequency,
            })
            .collect::<Vec<_>>();
        entries.sort_by(|a, b| a.term.cmp(&b.term));
        entries
    }

    fn replace_reverse_entries(
        &self,
        txn: &mut RwTxn,
        doc_id: u128,
        reverse_entries: &[ReversePostingEntry],
    ) -> Result<(), GraphError> {
        self.reverse_index_db.delete(txn, &doc_id)?;
        for entry in reverse_entries {
            let entry_bytes = bincode::serialize(entry)?;
            self.reverse_index_db.put(txn, &doc_id, &entry_bytes)?;
        }
        Ok(())
    }

    fn insert_posting(
        &self,
        txn: &mut RwTxn,
        doc_id: u128,
        term: &str,
        term_frequency: u32,
    ) -> Result<(), GraphError> {
        let term_bytes = term.as_bytes();
        let posting_entry = PostingListEntry {
            doc_id,
            term_frequency,
        };
        let posting_bytes = bincode::serialize(&posting_entry)?;
        self.inverted_index_db
            .put(txn, term_bytes, &posting_bytes)?;

        let current_df = self.term_frequencies_db.get(txn, term_bytes)?.unwrap_or(0);
        self.term_frequencies_db
            .put(txn, term_bytes, &(current_df + 1))?;
        Ok(())
    }

    fn delete_posting(
        &self,
        txn: &mut RwTxn,
        doc_id: u128,
        term: &str,
        term_frequency: u32,
    ) -> Result<(), GraphError> {
        let term_bytes = term.as_bytes();
        let posting_entry = PostingListEntry {
            doc_id,
            term_frequency,
        };
        let posting_bytes = bincode::serialize(&posting_entry)?;
        let deleted =
            self.inverted_index_db
                .delete_one_duplicate(txn, term_bytes, &posting_bytes)?;

        if !deleted {
            return Err(GraphError::New(format!(
                "BM25 posting missing while deleting term '{term}' for document {doc_id}"
            )));
        }

        let current_df = self
            .term_frequencies_db
            .get(txn, term_bytes)?
            .ok_or_else(|| {
                GraphError::New(format!(
                    "BM25 term frequency missing while deleting term '{term}'"
                ))
            })?;

        if current_df <= 1 {
            self.term_frequencies_db.delete(txn, term_bytes)?;
        } else {
            self.term_frequencies_db
                .put(txn, term_bytes, &(current_df - 1))?;
        }

        Ok(())
    }

    fn doc_state(&self, txn: &RwTxn, doc_id: u128) -> Result<DocState, GraphError> {
        let reverse_entries = self.reverse_entries_rw(txn, doc_id)?;
        match Self::classify_doc_presence(
            doc_id,
            self.doc_lengths_db.get(txn, &doc_id)?,
            !reverse_entries.is_empty(),
        )? {
            DocPresence::Absent => Ok(DocState::Absent),
            DocPresence::Empty => Ok(DocState::Empty),
            DocPresence::Indexed(doc_length) => Ok(DocState::Indexed {
                doc_length,
                reverse_entries,
            }),
        }
    }

    fn validate_search_doc(&self, txn: &RoTxn, doc_id: u128) -> Result<u32, GraphError> {
        match Self::classify_doc_presence(
            doc_id,
            self.doc_lengths_db.get(txn, &doc_id)?,
            self.has_reverse_entries(txn, doc_id)?,
        )? {
            DocPresence::Indexed(doc_length) => Ok(doc_length),
            DocPresence::Absent => Err(GraphError::New(format!(
                "BM25 posting exists for absent document {doc_id}"
            ))),
            DocPresence::Empty => Err(GraphError::New(format!(
                "BM25 posting exists for empty document {doc_id}"
            ))),
        }
    }

    fn record_insert(&self, txn: &mut RwTxn, doc_length: u32) -> Result<(), GraphError> {
        let mut metadata = self
            .read_metadata(txn)?
            .unwrap_or_else(|| self.default_metadata());
        let old_total_docs = metadata.total_docs;
        metadata.total_docs += 1;
        metadata.avgdl = (metadata.avgdl * old_total_docs as f64 + doc_length as f64)
            / metadata.total_docs as f64;
        self.write_metadata(txn, &metadata)
    }

    fn record_delete(
        &self,
        txn: &mut RwTxn,
        doc_id: u128,
        doc_length: u32,
    ) -> Result<(), GraphError> {
        let mut metadata = self.require_metadata(txn, doc_id)?;
        if metadata.total_docs == 0 {
            return Err(GraphError::New(format!(
                "BM25 metadata total_docs is zero while deleting document {doc_id}"
            )));
        }

        metadata.avgdl = if metadata.total_docs > 1 {
            (metadata.avgdl * metadata.total_docs as f64 - doc_length as f64)
                / (metadata.total_docs - 1) as f64
        } else {
            0.0
        };
        metadata.total_docs -= 1;
        self.write_metadata(txn, &metadata)
    }

    fn record_update(
        &self,
        txn: &mut RwTxn,
        doc_id: u128,
        old_doc_length: u32,
        new_doc_length: u32,
    ) -> Result<(), GraphError> {
        let mut metadata = self.require_metadata(txn, doc_id)?;
        if metadata.total_docs == 0 {
            return Err(GraphError::New(format!(
                "BM25 metadata total_docs is zero while updating document {doc_id}"
            )));
        }

        metadata.avgdl = (metadata.avgdl * metadata.total_docs as f64 - old_doc_length as f64
            + new_doc_length as f64)
            / metadata.total_docs as f64;
        self.write_metadata(txn, &metadata)
    }

    fn doc_length_from_reverse_entries(reverse_entries: &[ReversePostingEntry]) -> u32 {
        reverse_entries
            .iter()
            .map(|entry| entry.term_frequency)
            .sum()
    }

    fn insert_new_document_from_reverse_entries(
        &self,
        txn: &mut RwTxn,
        doc_id: u128,
        reverse_entries: &[ReversePostingEntry],
        doc_length: u32,
    ) -> Result<(), GraphError> {
        self.doc_lengths_db.put(txn, &doc_id, &doc_length)?;
        for entry in reverse_entries {
            self.insert_posting(txn, doc_id, &entry.term, entry.term_frequency)?;
        }
        self.replace_reverse_entries(txn, doc_id, reverse_entries)?;
        self.record_insert(txn, doc_length)
    }

    fn update_doc_with_reverse_entries(
        &self,
        txn: &mut RwTxn,
        doc_id: u128,
        mut new_reverse_entries: Vec<ReversePostingEntry>,
    ) -> Result<(), GraphError> {
        new_reverse_entries.sort_by(|a, b| a.term.cmp(&b.term));
        let new_doc_length = Self::doc_length_from_reverse_entries(&new_reverse_entries);

        let (old_doc_length, mut old_reverse_entries) = match self.doc_state(txn, doc_id)? {
            DocState::Absent => {
                return self.insert_new_document_from_reverse_entries(
                    txn,
                    doc_id,
                    &new_reverse_entries,
                    new_doc_length,
                );
            }
            DocState::Empty => (0, Vec::new()),
            DocState::Indexed {
                doc_length,
                reverse_entries,
            } => (doc_length, reverse_entries),
        };

        old_reverse_entries.sort_by(|a, b| a.term.cmp(&b.term));

        if old_reverse_entries == new_reverse_entries {
            return Ok(());
        }

        let mut old_index = 0;
        let mut new_index = 0;

        while old_index < old_reverse_entries.len() && new_index < new_reverse_entries.len() {
            let old_entry = &old_reverse_entries[old_index];
            let new_entry = &new_reverse_entries[new_index];

            match old_entry.term.cmp(&new_entry.term) {
                std::cmp::Ordering::Less => {
                    self.delete_posting(txn, doc_id, &old_entry.term, old_entry.term_frequency)?;
                    old_index += 1;
                }
                std::cmp::Ordering::Greater => {
                    self.insert_posting(txn, doc_id, &new_entry.term, new_entry.term_frequency)?;
                    new_index += 1;
                }
                std::cmp::Ordering::Equal => {
                    if old_entry.term_frequency != new_entry.term_frequency {
                        self.delete_posting(
                            txn,
                            doc_id,
                            &old_entry.term,
                            old_entry.term_frequency,
                        )?;
                        self.insert_posting(
                            txn,
                            doc_id,
                            &new_entry.term,
                            new_entry.term_frequency,
                        )?;
                    }
                    old_index += 1;
                    new_index += 1;
                }
            }
        }

        for old_entry in &old_reverse_entries[old_index..] {
            self.delete_posting(txn, doc_id, &old_entry.term, old_entry.term_frequency)?;
        }

        for new_entry in &new_reverse_entries[new_index..] {
            self.insert_posting(txn, doc_id, &new_entry.term, new_entry.term_frequency)?;
        }

        self.replace_reverse_entries(txn, doc_id, &new_reverse_entries)?;

        if old_doc_length != new_doc_length {
            self.doc_lengths_db.put(txn, &doc_id, &new_doc_length)?;
            self.record_update(txn, doc_id, old_doc_length, new_doc_length)?;
        }

        Ok(())
    }

    pub(crate) fn insert_doc_for_node(
        &self,
        txn: &mut RwTxn,
        doc_id: u128,
        properties: &ImmutablePropertiesMap<'_>,
        label: &str,
    ) -> Result<(), GraphError> {
        if !matches!(self.doc_state(txn, doc_id)?, DocState::Absent) {
            return Err(GraphError::New(format!(
                "BM25 document {doc_id} already exists"
            )));
        }

        let term_counts = self.term_counts_for_node(properties, label)?;
        let reverse_entries = Self::reverse_entries_from_term_counts(&term_counts);
        let doc_length = Self::doc_length_from_reverse_entries(&reverse_entries);

        self.insert_new_document_from_reverse_entries(txn, doc_id, &reverse_entries, doc_length)
    }

    pub(crate) fn update_doc_for_node(
        &self,
        txn: &mut RwTxn,
        doc_id: u128,
        properties: &ImmutablePropertiesMap<'_>,
        label: &str,
    ) -> Result<(), GraphError> {
        let term_counts = self.term_counts_for_node(properties, label)?;
        let reverse_entries = Self::reverse_entries_from_term_counts(&term_counts);
        self.update_doc_with_reverse_entries(txn, doc_id, reverse_entries)
    }
}

impl BM25 for HBM25Config {
    /// Converts text to lowercase, removes non-alphanumeric chars, splits into words
    fn tokenize<const SHOULD_FILTER: bool>(&self, text: &str) -> Vec<String> {
        text.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|s| !s.is_empty())
            .filter_map(|s| (!SHOULD_FILTER || s.len() > 2).then_some(s.to_string()))
            .collect()
    }

    fn insert_doc(&self, txn: &mut RwTxn, doc_id: u128, doc: &str) -> Result<(), GraphError> {
        if !matches!(self.doc_state(txn, doc_id)?, DocState::Absent) {
            return Err(GraphError::New(format!(
                "BM25 document {doc_id} already exists"
            )));
        }

        let term_counts = self.term_counts(doc);
        let reverse_entries = Self::reverse_entries_from_term_counts(&term_counts);
        let doc_length = Self::doc_length_from_reverse_entries(&reverse_entries);
        self.insert_new_document_from_reverse_entries(txn, doc_id, &reverse_entries, doc_length)
    }

    fn delete_doc(&self, txn: &mut RwTxn, doc_id: u128) -> Result<(), GraphError> {
        let (doc_length, reverse_entries) = match self.doc_state(txn, doc_id)? {
            DocState::Absent => return Ok(()),
            DocState::Empty => (0, Vec::new()),
            DocState::Indexed {
                doc_length,
                reverse_entries,
            } => (doc_length, reverse_entries),
        };

        for entry in &reverse_entries {
            self.delete_posting(txn, doc_id, &entry.term, entry.term_frequency)?;
        }

        self.reverse_index_db.delete(txn, &doc_id)?;
        self.doc_lengths_db.delete(txn, &doc_id)?;
        self.record_delete(txn, doc_id, doc_length)
    }

    fn update_doc(&self, txn: &mut RwTxn, doc_id: u128, doc: &str) -> Result<(), GraphError> {
        let term_counts = self.term_counts(doc);
        let reverse_entries = Self::reverse_entries_from_term_counts(&term_counts);
        self.update_doc_with_reverse_entries(txn, doc_id, reverse_entries)
    }

    fn calculate_bm25_score(
        &self,
        tf: u32,
        doc_len: u32,
        df: u32,
        total_docs: u64,
        avgdl: f64,
    ) -> f32 {
        // ensure we don't have division by zero
        let df = df.max(1) as f64;
        let total_docs = total_docs.max(1) as f64;

        // calculate IDF: ln((N - df + 0.5) / (df + 0.5) + 1)
        // this can be negative when df is high relative to N, which is mathematically correct
        let idf = (((total_docs - df + 0.5) / (df + 0.5)) + 1.0).ln();

        // ensure avgdl is not zero
        let avgdl = if avgdl > 0.0 { avgdl } else { doc_len as f64 };

        // calculate BM25 score
        let tf = tf as f64;
        let doc_len = doc_len as f64;
        let tf_component = (tf * (self.k1 + 1.0))
            / (tf + self.k1 * (1.0 - self.b + self.b * (doc_len.abs() / avgdl)));

        (idf * tf_component) as f32
    }

    fn search(
        &self,
        txn: &RoTxn,
        query: &str,
        limit: usize,
        arena: &Bump,
    ) -> Result<Vec<(u128, f32)>, GraphError> {
        let query_terms: BVec<BString> = BVec::from_iter_in(
            self.tokenize::<true>(query)
                .into_iter()
                .map(|s| BString::from_str_in(&s, arena)),
            arena,
        );

        let Some(metadata_bytes) = self.metadata_db.get(txn, METADATA_KEY)? else {
            return Ok(Vec::new());
        };
        let metadata: BM25Metadata = bincode::deserialize(metadata_bytes)?;
        if metadata.total_docs == 0 {
            return Ok(Vec::new());
        }

        // (node uuid, score)
        let estimated_capacity = (query_terms.len() * 50).min(limit * 4);
        let mut doc_scores: HashMap<u128, f32> = HashMap::with_capacity(estimated_capacity);

        // for each query term, calculate scores
        for term in query_terms {
            let term_bytes = term.as_bytes();

            let doc_frequency = self.term_frequencies_db.get(txn, term_bytes)?.unwrap_or(0);
            if doc_frequency == 0 {
                continue;
            }

            // Get all documents containing this term
            if let Some(duplicates) = self.inverted_index_db.get_duplicates(txn, term_bytes)? {
                for result in duplicates {
                    let (_, posting_bytes) = result?;
                    let posting: PostingListEntry = bincode::deserialize(posting_bytes)?;

                    // Get document length
                    let doc_length = self.validate_search_doc(txn, posting.doc_id)?;

                    // Calculate BM25 score for this term in this document
                    let score = self.calculate_bm25_score(
                        posting.term_frequency,
                        doc_length,
                        doc_frequency,
                        metadata.total_docs,
                        metadata.avgdl,
                    );

                    *doc_scores.entry(posting.doc_id).or_insert(0.0) += score;
                }
            }
        }

        // Sort by score and return top results
        let mut results: Vec<(u128, f32)> = Vec::with_capacity(doc_scores.len());
        results.extend(doc_scores);
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(limit);

        debug_println!("found {} results in bm25 search", results.len());

        Ok(results)
    }
}

pub trait HybridSearch {
    /// Search both hnsw index and bm25 docs
    fn hybrid_search(
        self,
        query: &str,
        query_vector: &[f64],
        alpha: f32,
        limit: usize,
    ) -> impl std::future::Future<Output = Result<Vec<(u128, f32)>, GraphError>> + Send;
}

impl HybridSearch for HelixGraphStorage {
    async fn hybrid_search(
        self,
        query: &str,
        query_vector: &[f64],
        alpha: f32,
        limit: usize,
    ) -> Result<Vec<(u128, f32)>, GraphError> {
        let query_owned = query.to_string();
        let query_vector_owned = query_vector.to_vec();

        let graph_env_bm25 = self.graph_env.clone();
        let graph_env_vector = self.graph_env.clone();

        let bm25_handle = task::spawn_blocking(move || -> Result<Vec<(u128, f32)>, GraphError> {
            let txn = graph_env_bm25.read_txn()?;
            let arena = Bump::new();
            match self.bm25.as_ref() {
                Some(s) => s.search(&txn, &query_owned, limit * 2, &arena),
                None => Err(GraphError::from("BM25 not enabled!")),
            }
        });

        let vector_handle =
            task::spawn_blocking(move || -> Result<Option<Vec<(u128, f64)>>, GraphError> {
                let txn = graph_env_vector.read_txn()?;
                let arena = Bump::new();
                let query_slice = arena.alloc_slice_copy(query_vector_owned.as_slice());
                let results = self.vectors.search::<fn(&HVector, &RoTxn) -> bool>(
                    &txn,
                    query_slice,
                    limit * 2,
                    "vector",
                    None,
                    false,
                    &arena,
                )?;
                let scores = results
                    .into_iter()
                    .map(|vec| (vec.id, vec.distance.unwrap_or(0.0)))
                    .collect::<Vec<(u128, f64)>>();
                Ok(Some(scores))
            });

        let (bm25_results, vector_results) = match tokio::try_join!(bm25_handle, vector_handle) {
            Ok((a, b)) => (a, b),
            Err(e) => return Err(GraphError::from(e.to_string())),
        };

        let mut combined_scores: HashMap<u128, f32> = HashMap::new();

        for (doc_id, score) in bm25_results? {
            combined_scores.insert(doc_id, alpha * score);
        }

        // correct_score = alpha * bm25_score + (1.0 - alpha) * vector_score
        if let Some(vector_results) = vector_results? {
            for (doc_id, score) in vector_results {
                let similarity = (1.0 / (1.0 + score)) as f32;
                combined_scores
                    .entry(doc_id)
                    .and_modify(|existing_score| *existing_score += (1.0 - alpha) * similarity)
                    .or_insert((1.0 - alpha) * similarity);
            }
        }

        let mut results = Vec::with_capacity(combined_scores.len());
        results.extend(combined_scores);
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(limit);

        Ok(results)
    }
}

pub fn build_bm25_payload(properties: &ImmutablePropertiesMap<'_>, label: &str) -> String {
    let mut data = properties.flatten_bm25();
    data.push_str(label);
    data
}

pub trait BM25Flatten {
    /// util func to flatten array of strings to a single string
    fn flatten_bm25(&self) -> String;
}

impl BM25Flatten for ImmutablePropertiesMap<'_> {
    fn flatten_bm25(&self) -> String {
        self.iter()
            .fold(String::with_capacity(self.len() * 4), |mut s, (k, v)| {
                s.push_str(k);
                s.push(' ');
                s.push_str(&v.inner_stringify());
                s.push(' ');
                s
            })
    }
}
