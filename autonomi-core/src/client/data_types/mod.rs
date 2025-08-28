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

use crate::{
    GetError,
    networking::{PeerId, Record},
};
use ant_protocol::NetworkAddress;
use std::collections::HashMap;
use tracing::{debug, error, warn};

/// Resolve split records by
/// * deserialize the records by calling the FDeser functor
/// * try to resolve the splits by following the rules defined by FCounter functor
/// * if still multiple different entries remains,
///   return the entire result_map in the error for the caller to resolve further.
pub(crate) fn resolve_split_records<T, FDeser, FCounter, FEqual>(
    result_map: HashMap<PeerId, Record>,
    key: NetworkAddress,
    deserialize: FDeser,
    counter_of: FCounter,
    same_content: FEqual,
) -> Result<T, GetError>
where
    T: Clone,
    FDeser: Fn(&Record) -> Result<T, GetError>,
    FCounter: Fn(&T) -> u64,
    FEqual: Fn(&T, &T) -> bool,
{
    debug!(
        "Resolving split records at {key:?} among {} entries.",
        result_map.len()
    );

    // Deserialize all records; if any fails, propagate the error upstream
    let mut items: Vec<T> = result_map
        .values()
        .map(deserialize)
        .collect::<Result<Vec<_>, _>>()?;

    if items.is_empty() {
        error!("Got empty records map for {key:?}");
        return Err(GetError::RecordNotFound);
    }

    // Sort by counter then pick the max counter value
    items.sort_by_key(|t| counter_of(t));
    let max_counter = match items.last().map(&counter_of) {
        Some(c) => c,
        None => {
            error!("No records left after sorting for {key:?}");
            return Err(GetError::RecordNotFound);
        }
    };

    // Collect all with max counter
    let latest: Vec<T> = items
        .into_iter()
        .filter(|t| counter_of(t) == max_counter)
        .collect();

    if latest.is_empty() {
        error!("No latest records found for {key:?}");
        return Err(GetError::RecordNotFound);
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
            Err(GetError::RecordNotFound)
        }
        _multi => {
            warn!("Still got multiple conflicting records after split resolvement for {key:?}");
            Err(GetError::SplitRecord(result_map.into_values().collect()))
        }
    }
}
