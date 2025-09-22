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
use std::collections::{HashMap, HashSet};
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
    T: Clone + std::hash::Hash + Eq,
    FDeser: Fn(Record) -> Result<T, E>,
    FCounter: Fn(&T) -> u64,
    FEqual: Fn(&T, &T) -> bool,
    FFork: Fn(HashSet<T>) -> E,
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

    // Collect unique entries with max counter
    let mut latest: HashSet<T> = HashSet::new();
    for item in items.into_iter() {
        if counter_of(&item) == max_counter
            && !latest.iter().any(|existing| same_content(existing, &item))
        {
            latest.insert(item);
        }
    }

    match latest.len() {
        1 => {
            let item = latest
                .into_iter()
                .next()
                .expect("HashSet with len() == 1 must contain exactly one item");
            Ok(item)
        }
        0 => {
            error!("No latest records found for {key:?}");
            Err(corrupt_error())
        }
        _ => {
            warn!("Multiple conflicting records found at latest version for {key:?}");
            Err(fork_error(latest))
        }
    }
}
