// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_evm::PaymentQuote;
use ant_protocol::{NetworkAddress, PrettyPrintRecordKey};

use super::{Network, RetryStrategy};
use super::{NetworkError, PeerInfo, Record, Strategy};
use tokio::time::sleep;

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
        let verification_strategy = Strategy {
            get_quorum: strategy.verification_quorum,
            ..*strategy
        };
        let mut errors = vec![];
        for duration in strategy.put_retry.backoff() {
            match self
                .put_record(record.clone(), to.clone(), strategy.put_quorum)
                .await
            {
                // return success if verification succeeds
                Ok(()) => {
                    let network_address = NetworkAddress::from(&record.key);
                    let error = match self
                        .get_record_with_retries(network_address, &verification_strategy)
                        .await
                    {
                        Ok(Some(_)) => return Ok(()),
                        Ok(None) => NetworkError::PutRecordVerification("Not found".to_string()),
                        Err(err) => NetworkError::PutRecordVerification(err.to_string()),
                    };

                    // retry on verification errors
                    warn!("Put record failed at {addr}: {error:?}, retrying in {duration:?}");
                    errors.push(error.clone());
                    match duration {
                        Some(retry_delay) => sleep(retry_delay).await,
                        None => return Err(error),
                    }
                }
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

    /// Get a record from the network with retries
    pub async fn get_record_with_retries(
        &self,
        addr: NetworkAddress,
        strategy: &Strategy,
    ) -> Result<Option<Record>, NetworkError> {
        let mut errors = vec![];
        let quorum = strategy.get_quorum;
        for duration in strategy.get_retry.backoff() {
            match self.get_record(addr.clone(), quorum).await {
                // return success
                Ok(Some(record)) => return Ok(Some(record)),
                // don't retry on split
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
                        None => return Ok(None),
                    }
                }
                // retry on other errors
                Err(err) => {
                    warn!("Get record failed at {addr}: {err:?}, retrying in {duration:?}");
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

    /// Get closest peers to an address with retries
    pub async fn get_closest_peers_with_retries(
        &self,
        addr: NetworkAddress,
    ) -> Result<Vec<PeerInfo>, NetworkError> {
        let mut errors = vec![];
        for duration in RetryStrategy::Once.backoff() {
            match self.get_closest_peers(addr.clone()).await {
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
