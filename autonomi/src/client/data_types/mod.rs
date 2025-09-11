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

use crate::networking::{PeerId, Record};
use ant_protocol::NetworkAddress;
use std::collections::HashMap;
use tracing::{debug, error, warn};

/// Resolve split records by selecting the highest counter.
/// If multiple records share the highest counter but have different content,
/// return a fork error constructed by the provided closure.
/// If deserialization fails for any record, return the deserialization error.
/// If no valid records remain, return a corrupt error constructed by the provided closure.
pub(crate) fn resolve_split_records<T, E, FDeser, FCounter, FEqual, FFork, FCorrupt>(
    result_map: HashMap<PeerId, Record>,
    key: NetworkAddress,
    deserialize: FDeser,
    counter_of: FCounter,
    same_content: FEqual,
    fork_error: FFork,
    corrupt_error: FCorrupt,
) -> Result<T, E>
where
    T: Clone,
    FDeser: Fn(Record) -> Result<T, E>,
    FCounter: Fn(&T) -> u64,
    FEqual: Fn(&T, &T) -> bool,
    FFork: Fn(Vec<T>) -> E,
    FCorrupt: Fn() -> E,
{
    debug!("Resolving split records at {key:?}");

    // Deserialize all records; if any fails, propagate the error upstream
    let mut items: Vec<T> = result_map
        .into_values()
        .map(deserialize)
        .collect::<Result<Vec<_>, _>>()?;

    if items.is_empty() {
        error!("Got empty records map for {key:?}");
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

    // Collect all with max counter
    let latest: Vec<T> = items
        .into_iter()
        .filter(|t| counter_of(t) == max_counter)
        .collect();

    if latest.is_empty() {
        error!("No latest records found for {key:?}");
        return Err(corrupt_error());
    }

    // Deduplicate equal-content entries
    let mut dedup_latest: Vec<T> = Vec::with_capacity(latest.len());
    for item in latest.iter().cloned() {
        if !dedup_latest
            .iter()
            .any(|existing| same_content(existing, &item))
        {
            dedup_latest.push(item);
        }
    }

    match dedup_latest.as_slice() {
        [one] => Ok(one.clone()),
        [] => {
            error!("No valid records remain after deduplication for {key:?}");
            Err(corrupt_error())
        }
        _multi => {
            warn!("Multiple conflicting records found at latest version for {key:?}");
            Err(fork_error(dedup_latest))
        }
    }
}
