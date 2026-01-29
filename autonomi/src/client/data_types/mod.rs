// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

pub mod chunk;
pub mod graph;
pub mod pointer;
pub mod scratchpad;

use crate::networking::Record;
use ant_protocol::NetworkAddress;
use std::collections::HashSet;
use tracing::{debug, error, warn};

/// Resolve records from a list of records for CRDT types,
/// which could be typically from fallback peer queries, or from a SplitRecord error.
///
/// It resolves conflicts by selecting the highest counter.
/// If multiple records share the highest counter but have different content,
/// return a fork error constructed by the provided closure.
/// If deserialization fails for any record, return the deserialization error.
/// If no valid records remain, return a corrupt error constructed by the provided closure.
///
/// # Arguments
/// * `records` - List of records fetched from peers
/// * `key` - The network address (for logging)
/// * `deserialize` - Function to deserialize a record into type T
/// * `counter_of` - Function to extract the counter value from type T
/// * `same_content` - Function to compare if two items have the same content
/// * `fork_error` - Function to construct a fork error
/// * `corrupt_error` - Function to construct a corrupt error
///
/// # Returns
/// * `Ok(T)` - The resolved item with the highest counter and sufficient consistent copies
/// * `Err(E)` - If deserialization fails, data is corrupt, forked, or insufficient copies exist
#[allow(clippy::too_many_arguments)]
pub(crate) fn resolve_records_from_peers<T, E, FDeser, FCounter, FEqual, FFork, FCorrupt>(
    records: Vec<Record>,
    key: NetworkAddress,
    deserialize: FDeser,
    counter_of: FCounter,
    same_content: FEqual,
    fork_error: FFork,
    corrupt_error: FCorrupt,
) -> Result<T, E>
where
    T: Clone + std::hash::Hash + Eq,
    FDeser: Fn(Record) -> Result<T, E>,
    FCounter: Fn(&T) -> u64,
    FEqual: Fn(&T, &T) -> bool,
    FFork: Fn(HashSet<T>) -> E,
    FCorrupt: Fn() -> E,
{
    debug!("Resolving {} records from peers for {key:?}", records.len());

    // Deserialize all records
    let mut items: Vec<T> = Vec::new();
    for record in records {
        match deserialize(record) {
            Ok(item) => items.push(item),
            Err(_) => {
                // Skip invalid records in fallback mode, we need to be resilient
                warn!("Skipping invalid record during fallback resolution at {key:?}");
            }
        }
    }

    if items.is_empty() {
        error!("No valid records found for {key:?}");
        return Err(corrupt_error());
    }

    // Sort by counter then pick the max counter value
    items.sort_by_key(|t| counter_of(t));
    let max_counter = match items.last().map(&counter_of) {
        Some(c) => c,
        None => {
            error!("No records left after sorting for {key:?}");
            return Err(corrupt_error());
        }
    };

    // Group items by content at the max counter
    let items_at_max: Vec<T> = items
        .into_iter()
        .filter(|item| counter_of(item) == max_counter)
        .collect();

    // Count consistent copies for each unique content
    let mut content_groups: Vec<(T, usize)> = Vec::new();
    for item in items_at_max.iter() {
        if let Some((_, count)) = content_groups
            .iter_mut()
            .find(|(existing, _)| same_content(existing, item))
        {
            *count += 1;
        } else {
            content_groups.push((item.clone(), 1));
        }
    }

    // Find the group(s) with the most copies
    let max_copies = content_groups.iter().map(|(_, c)| *c).max().unwrap_or(0);

    // Get all items with the max number of copies
    let best_items: HashSet<T> = content_groups
        .into_iter()
        .filter(|(_, count)| *count == max_copies)
        .map(|(item, _)| item)
        .collect();

    let best_vec: Vec<T> = best_items.iter().cloned().collect();
    match &best_vec[..] {
        [one] => {
            debug!("Successfully resolved {key:?} with {max_copies} consistent copies");
            Ok(one.clone())
        }
        [] => {
            error!("No valid content found for {key:?}");
            Err(corrupt_error())
        }
        [..] => {
            warn!("Multiple conflicting records with equal copy counts for {key:?}");
            Err(fork_error(best_items))
        }
    }
}
