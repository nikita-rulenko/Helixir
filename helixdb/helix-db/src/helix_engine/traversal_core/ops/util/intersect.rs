use std::collections::HashSet;

use crate::helix_engine::{
    storage_core::HelixGraphStorage,
    traversal_core::{traversal_iter::RoTraversalIterator, traversal_value::TraversalValue},
    types::GraphError,
};
use heed3::RoTxn;

pub trait IntersectAdapter<'db, 'arena, 'txn>: Iterator {
    /// Computes the intersection of sub-traversal results across all upstream items.
    ///
    /// For each upstream item, runs the closure (sub-traversal) and collects result IDs.
    /// Returns only items that appear in ALL result sets.
    fn intersect<F>(
        self,
        f: F,
    ) -> RoTraversalIterator<
        'db,
        'arena,
        'txn,
        impl Iterator<Item = Result<TraversalValue<'arena>, GraphError>>,
    >
    where
        F: Fn(
            TraversalValue<'arena>,
            &'db HelixGraphStorage,
            &'txn RoTxn<'db>,
            &'arena bumpalo::Bump,
        ) -> Result<Vec<TraversalValue<'arena>>, GraphError>;
}

impl<'db, 'arena, 'txn, I: Iterator<Item = Result<TraversalValue<'arena>, GraphError>>>
    IntersectAdapter<'db, 'arena, 'txn> for RoTraversalIterator<'db, 'arena, 'txn, I>
{
    fn intersect<F>(
        self,
        f: F,
    ) -> RoTraversalIterator<
        'db,
        'arena,
        'txn,
        impl Iterator<Item = Result<TraversalValue<'arena>, GraphError>>,
    >
    where
        F: Fn(
            TraversalValue<'arena>,
            &'db HelixGraphStorage,
            &'txn RoTxn<'db>,
            &'arena bumpalo::Bump,
        ) -> Result<Vec<TraversalValue<'arena>>, GraphError>,
    {
        let storage = self.storage;
        let txn = self.txn;
        let arena = self.arena;

        // Collect all upstream items, propagating errors
        let mut upstream: Vec<TraversalValue<'arena>> = Vec::new();
        for item in self.inner {
            match item {
                Ok(val) => upstream.push(val),
                Err(e) => {
                    return RoTraversalIterator {
                        storage,
                        arena,
                        txn,
                        inner: vec![Err(e)].into_iter(),
                    };
                }
            }
        }

        if upstream.is_empty() {
            return RoTraversalIterator {
                storage,
                arena,
                txn,
                inner: Vec::new().into_iter(),
            };
        }

        // Run all sub-traversals, propagating errors
        let mut all_results: Vec<Vec<TraversalValue<'arena>>> = Vec::new();
        for item in upstream {
            match f(item, storage, txn, arena) {
                Ok(results) => all_results.push(results),
                Err(e) => {
                    return RoTraversalIterator {
                        storage,
                        arena,
                        txn,
                        inner: vec![Err(e)].into_iter(),
                    };
                }
            }
        }

        // Sort by size — smallest first so intersection shrinks fastest
        all_results.sort_by_key(|r| r.len());

        // Seed intersection from smallest result set
        let first = all_results.remove(0);
        let mut intersection: HashSet<u128> = first.iter().map(|v| v.id()).collect();

        // Intersect with remaining sets (in-place retain, no allocation)
        for results in &all_results {
            let id_set: HashSet<u128> = results.iter().map(|v| v.id()).collect();
            intersection.retain(|id| id_set.contains(id));
            if intersection.is_empty() {
                return RoTraversalIterator {
                    storage,
                    arena,
                    txn,
                    inner: Vec::new().into_iter(),
                };
            }
        }

        // Return matching items from the smallest set
        let result: Vec<Result<TraversalValue<'arena>, GraphError>> = first
            .into_iter()
            .filter(|v| intersection.contains(&v.id()))
            .map(Ok)
            .collect();

        RoTraversalIterator {
            storage,
            arena,
            txn,
            inner: result.into_iter(),
        }
    }
}
