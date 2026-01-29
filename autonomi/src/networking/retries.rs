// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_evm::PaymentQuote;
use ant_protocol::{NetworkAddress, PrettyPrintRecordKey};
use futures::stream::{self, StreamExt};

use super::{Network, RetryStrategy};
use super::{NetworkError, PeerInfo, Record, Strategy};
use tokio::time::sleep;

/// Default number of closest peers to query in fallback fetch operations.
pub const FALLBACK_PEERS_COUNT: usize = 20;

impl Network {
    /// Put a record to the network with retries
    ///
    /// Will carry out network get after put success, to verify the existence of the record.
    pub async fn put_record_with_retries(
        &self,
        record: Record,
        to: Vec<PeerInfo>,
        strategy: &Strategy,
    ) -> Result<(), NetworkError> {
        let addr = PrettyPrintRecordKey::from(&record.key).into_owned();
        let mut errors = vec![];
        for duration in strategy.put_retry.backoff() {
            match self
                .put_record(record.clone(), to.clone(), strategy.put_quorum)
                .await
            {
                // Exitence verification is no longer mandatory as req/rsp upload allows client
                // collect storage result from nodes directly.
                Ok(()) => return Ok(()),
                // return fatal errors
                Err(err) if err.cannot_retry() => {
                    return Err(err);
                }
                // retry on other errors
                Err(err) => {
                    warn!("Put record failed at {addr}: {err:?}, retrying in {duration:?}");
                    errors.push(err.clone());
                    match duration {
                        Some(retry_delay) => sleep(retry_delay).await,
                        None => return Err(err),
                    }
                }
            }
        }
        Err(NetworkError::InvalidRetryStrategy)
    }

    /// Get a record from the network with retries and fallback to direct peer queries.
    ///
    /// This function first attempts standard DHT queries with the configured retry strategy.
    /// If all retries are exhausted without success, it falls back to querying the closest
    /// peers directly for the record.
    ///
    /// # Returns
    /// * `Ok(Some(Vec<Record>))` - Successfully retrieved records. For non-CRDT types (Chunk),
    ///   this will typically contain a single record. For CRDT types (Pointer, Scratchpad,
    ///   GraphEntry), this may contain multiple record versions that need resolution.
    /// * `Ok(None)` - No records found after all attempts including fallback.
    /// * `Err(NetworkError::SplitRecord(_))` - Multiple conflicting records detected during
    ///   DHT query (caller should handle split resolution).
    /// * `Err(_)` - Fatal network error that cannot be retried.
    pub async fn get_record_with_retries(
        &self,
        addr: NetworkAddress,
        strategy: &Strategy,
    ) -> Result<Option<Vec<Record>>, NetworkError> {
        let mut errors = vec![];
        let quorum = strategy.get_quorum;
        for duration in strategy.get_retry.backoff() {
            match self.get_record(addr.clone(), quorum).await {
                // return success as single-element vec
                Ok(Some(record)) => return Ok(Some(vec![record])),
                // don't retry on split - return for caller to resolve
                Err(err) if matches!(err, NetworkError::SplitRecord(_)) => {
                    return Err(err);
                }
                // return fatal errors
                Err(err) if err.cannot_retry() => {
                    return Err(err);
                }
                // retry on no record
                Ok(None) => {
                    warn!("Record not found at {addr}, retrying in {duration:?}");
                    match duration {
                        Some(retry_delay) => sleep(retry_delay).await,
                        None => break,
                    }
                }
                // retry on other errors
                Err(err) => {
                    warn!("Get record failed at {addr}: {err:?}, retrying in {duration:?}");
                    errors.push(err.clone());
                    match duration {
                        Some(retry_delay) => sleep(retry_delay).await,
                        None => break,
                    }
                }
            }
        }
        // All retries exhausted, try fallback
        debug!("All retries exhausted for {addr} after error, trying fallback to closest peers");
        self.fetch_records_from_closest_peers_fallback(addr).await
    }

    /// Fallback method to fetch records from the closest peers directly in parallel.
    ///
    /// This function queries the closest N peers for a given key and returns all
    /// successfully retrieved records. This is useful as a fallback mechanism when
    /// standard DHT queries fail or for CRDT types that may have different versions
    /// on different nodes.
    ///
    /// # Arguments
    /// * `addr` - The network address to query
    ///
    /// # Returns
    /// * `Ok(Some(Vec<Record>))` - All successfully retrieved records from the queried peers.
    /// * `Ok(None)` - If no peers could be found or all peer queries failed.
    async fn fetch_records_from_closest_peers_fallback(
        &self,
        addr: NetworkAddress,
    ) -> Result<Option<Vec<Record>>, NetworkError> {
        debug!("Querying closest {FALLBACK_PEERS_COUNT} nodes directly for {addr:?}");

        let closest_peers = match self
            .get_closest_peers(addr.clone(), Some(FALLBACK_PEERS_COUNT))
            .await
        {
            Ok(peers) => peers,
            Err(e) => {
                error!("Failed to get closest peers for {addr:?}: {e}");
                return Ok(None);
            }
        };

        debug!(
            "Querying {} closest peers in parallel for {addr:?}",
            closest_peers.len()
        );

        // Create query tasks for all closest peers
        let network = self.clone();
        let query_futures: Vec<_> = closest_peers
            .into_iter()
            .map(|peer| {
                let network = network.clone();
                let addr = addr.clone();
                async move { network.get_record_from_peer(addr, peer).await }
            })
            .collect();

        // Process all tasks concurrently
        let results: Vec<_> = stream::iter(query_futures)
            .buffer_unordered(FALLBACK_PEERS_COUNT)
            .collect()
            .await;

        // Collect all successful records
        let records: Vec<Record> = results
            .into_iter()
            .filter_map(|result| match result {
                Ok(Some(record)) => Some(record),
                _ => None,
            })
            .collect();

        if records.is_empty() {
            error!("❌ All closest peers failed to return records for {addr:?}");
            Ok(None)
        } else {
            debug!(
                "✅ Retrieved {} records from closest peers for {addr:?}",
                records.len()
            );
            Ok(Some(records))
        }
    }

    /// Get quotes from the network with retries
    pub async fn get_quotes_with_retries(
        &self,
        addr: NetworkAddress,
        data_type: u32,
        data_size: usize,
    ) -> Result<Option<Vec<(PeerInfo, PaymentQuote)>>, NetworkError> {
        let mut errors = vec![];
        for duration in RetryStrategy::Once.backoff() {
            match self.get_quotes(addr.clone(), data_type, data_size).await {
                // return success
                Ok(quotes) => return Ok(quotes),
                // return fatal errors
                Err(err) if err.cannot_retry() => {
                    return Err(err);
                }
                // retry on other errors
                Err(err) => {
                    warn!("Get quotes failed at {addr}: {err:?}, retrying in {duration:?}");
                    errors.push(err.clone());
                    match duration {
                        Some(retry_delay) => sleep(retry_delay).await,
                        None => return Err(err),
                    }
                }
            }
        }
        Err(NetworkError::InvalidRetryStrategy)
    }

    /// Get closest peers to an address with retries, optionally specifying count of peers to retrieve.
    pub async fn get_closest_peers_with_retries(
        &self,
        addr: NetworkAddress,
        count: Option<usize>,
    ) -> Result<Vec<PeerInfo>, NetworkError> {
        let mut errors = vec![];
        for duration in RetryStrategy::Once.backoff() {
            match self.get_closest_peers(addr.clone(), count).await {
                // return success
                Ok(peers) => return Ok(peers),
                // return fatal errors
                Err(err) if err.cannot_retry() => {
                    return Err(err);
                }
                // retry on other errors
                Err(err) => {
                    warn!("Get closest peers failed at {addr}: {err:?}, retrying in {duration:?}");
                    errors.push(err.clone());
                    match duration {
                        Some(retry_delay) => sleep(retry_delay).await,
                        None => return Err(err),
                    }
                }
            }
        }
        Err(NetworkError::InvalidRetryStrategy)
    }
}
