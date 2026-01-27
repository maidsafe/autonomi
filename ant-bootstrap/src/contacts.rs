// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{
    Error, Result, cache_store::CACHE_DATA_VERSION_LATEST, craft_valid_multiaddr_from_str,
};
use futures::stream::{self, StreamExt};
use libp2p::Multiaddr;
use reqwest::Client;
use std::time::Duration;
use url::Url;

const CONTACTS_CACHE_VERSION_HEADER: &str = "Cache-Version";

pub const MAINNET_CONTACTS: &[&str] = &[
    "https://sn-testnet.s3.eu-west-2.amazonaws.com/network-contacts",
    "http://159.89.251.80/bootstrap_cache.json",
    "http://159.65.210.89/bootstrap_cache.json",
    "http://159.223.246.45/bootstrap_cache.json",
    "http://139.59.201.153/bootstrap_cache.json",
    "http://139.59.200.27/bootstrap_cache.json",
];
pub const ALPHANET_CONTACTS: &[&str] = &[
    "http://188.166.133.208/bootstrap_cache.json",
    "http://188.166.133.125/bootstrap_cache.json",
    "http://178.128.137.64/bootstrap_cache.json",
    "http://159.223.242.7/bootstrap_cache.json",
    "http://143.244.197.147/bootstrap_cache.json",
];

/// The client fetch timeout
const FETCH_TIMEOUT_SECS: u64 = 30;
/// Maximum number of endpoints to fetch at a time
const MAX_CONCURRENT_FETCHES: usize = 3;
/// The max number of retries for an endpoint on failure.
const MAX_RETRIES_ON_FETCH_FAILURE: usize = 3;

/// Discovers initial peers from a list of endpoints
pub struct ContactsFetcher {
    /// The number of addrs to fetch
    max_addrs: usize,
    /// The list of endpoints
    pub endpoints: Vec<Url>,
    /// Reqwest Client
    request_client: Client,
}

impl ContactsFetcher {
    /// Create a new struct with the default endpoint
    pub fn new() -> Result<Self> {
        Self::with_endpoints(vec![])
    }

    /// Create a new struct with the provided endpoints
    pub fn with_endpoints(endpoints: Vec<Url>) -> Result<Self> {
        let request_client = Client::builder()
            .timeout(Duration::from_secs(FETCH_TIMEOUT_SECS))
            .build()?;

        Ok(Self {
            max_addrs: usize::MAX,
            endpoints,
            request_client,
        })
    }

    /// Set the number of addrs to fetch
    pub fn set_max_addrs(&mut self, max_addrs: usize) {
        self.max_addrs = max_addrs;
    }

    /// Create a new struct with the mainnet endpoints
    pub fn with_mainnet_endpoints() -> Result<Self> {
        let mut fetcher = Self::new()?;
        #[allow(clippy::expect_used)]
        let mainnet_contact = MAINNET_CONTACTS
            .iter()
            .map(|url| url.parse().expect("Failed to parse static URL"))
            .collect();
        fetcher.endpoints = mainnet_contact;
        Ok(fetcher)
    }

    /// Create a new struct with the alphanet endpoints
    pub fn with_alphanet_endpoints() -> Result<Self> {
        let mut fetcher = Self::new()?;
        #[allow(clippy::expect_used)]
        let alphanet_contact = ALPHANET_CONTACTS
            .iter()
            .map(|url| url.parse().expect("Failed to parse static URL"))
            .collect();
        fetcher.endpoints = alphanet_contact;
        Ok(fetcher)
    }

    pub fn insert_endpoint(&mut self, endpoint: Url) {
        self.endpoints.push(endpoint);
    }

    /// Fetch the list of bootstrap multiaddrs from all configured endpoints
    pub async fn fetch_bootstrap_addresses(&self) -> Result<Vec<Multiaddr>> {
        info!(
            "Starting peer fetcher from {} endpoints: {:?}",
            self.endpoints.len(),
            self.endpoints
        );
        let mut bootstrap_addresses = Vec::new();

        let mut fetches = stream::iter(self.endpoints.clone())
            .map(|endpoint| async move {
                info!(
                    "Attempting to fetch bootstrap addresses from endpoint: {}",
                    endpoint
                );
                (
                    Self::fetch_from_endpoint(self.request_client.clone(), &endpoint).await,
                    endpoint,
                )
            })
            .buffer_unordered(MAX_CONCURRENT_FETCHES);

        while let Some((result, endpoint)) = fetches.next().await {
            match result {
                Ok(mut endpoing_bootstrap_addresses) => {
                    info!(
                        "Successfully fetched {} bootstrap addrs from {}. First few addrs: {:?}",
                        endpoing_bootstrap_addresses.len(),
                        endpoint,
                        endpoing_bootstrap_addresses
                            .iter()
                            .take(3)
                            .collect::<Vec<_>>()
                    );
                    bootstrap_addresses.append(&mut endpoing_bootstrap_addresses);
                    if bootstrap_addresses.len() >= self.max_addrs {
                        info!(
                            "Fetched enough bootstrap addresses. Stopping. needed: {} Total fetched: {}",
                            self.max_addrs,
                            bootstrap_addresses.len()
                        );
                        break;
                    }
                }
                Err(e) => {
                    warn!("Failed to fetch bootstrap addrs from {}: {}", endpoint, e);
                }
            }
        }

        bootstrap_addresses.truncate(self.max_addrs);

        info!(
            "Successfully discovered {} total addresses. First few: {:?}",
            bootstrap_addresses.len(),
            bootstrap_addresses.iter().take(3).collect::<Vec<_>>()
        );
        Ok(bootstrap_addresses)
    }

    /// Fetch the list of multiaddrs from a single endpoint
    async fn fetch_from_endpoint(request_client: Client, endpoint: &Url) -> Result<Vec<Multiaddr>> {
        let mut retries = 0;

        let bootstrap_addresses = loop {
            let response = request_client
                .get(endpoint.clone())
                .header(CONTACTS_CACHE_VERSION_HEADER, CACHE_DATA_VERSION_LATEST)
                .send()
                .await;

            match response {
                Ok(response) => {
                    if response.status().is_success() {
                        let text = response.text().await?;

                        match Self::try_parse_response(&text) {
                            Ok(addrs) => break addrs,
                            Err(err) => {
                                warn!("Failed to parse response with err: {err:?}");
                                retries += 1;
                                if retries >= MAX_RETRIES_ON_FETCH_FAILURE {
                                    return Err(Error::FailedToObtainAddrsFromUrl(
                                        endpoint.to_string(),
                                        MAX_RETRIES_ON_FETCH_FAILURE,
                                    ));
                                }
                            }
                        }
                    } else {
                        retries += 1;
                        if retries >= MAX_RETRIES_ON_FETCH_FAILURE {
                            return Err(Error::FailedToObtainAddrsFromUrl(
                                endpoint.to_string(),
                                MAX_RETRIES_ON_FETCH_FAILURE,
                            ));
                        }
                    }
                }
                Err(err) => {
                    error!("Failed to get bootstrap addrs from URL {endpoint}: {err:?}");
                    retries += 1;
                    if retries >= MAX_RETRIES_ON_FETCH_FAILURE {
                        return Err(Error::FailedToObtainAddrsFromUrl(
                            endpoint.to_string(),
                            MAX_RETRIES_ON_FETCH_FAILURE,
                        ));
                    }
                }
            }
            debug!(
                "Failed to get bootstrap addrs from URL, retrying {retries}/{MAX_RETRIES_ON_FETCH_FAILURE}"
            );

            tokio::time::sleep(Duration::from_secs(1)).await;
        };

        Ok(bootstrap_addresses)
    }

    /// Try to parse a response from an endpoint
    fn try_parse_response(response: &str) -> Result<Vec<Multiaddr>> {
        let cache_data: Option<super::cache_store::cache_data_v1::CacheData> =
            serde_json::from_str(response).ok().or_else(|| {
                serde_json::from_str::<super::cache_store::cache_data_v0::CacheData>(response)
                    .ok()
                    .map(Into::into)
            });

        match cache_data {
            Some(cache_data) => {
                info!(
                    "Successfully parsed JSON response with {} peers",
                    cache_data.peers.len()
                );
                let our_network_version = crate::get_network_version();

                if cache_data.network_version != our_network_version {
                    warn!(
                        "Network version mismatch. Expected: {our_network_version}, got: {}. Skipping.",
                        cache_data.network_version
                    );
                    return Ok(vec![]);
                }
                let bootstrap_addresses = cache_data.get_all_addrs().cloned().collect::<Vec<_>>();

                info!(
                    "Successfully parsed {} valid peers from JSON",
                    bootstrap_addresses.len()
                );
                Ok(bootstrap_addresses)
            }
            None => {
                info!("Attempting to parse response as plain text");
                // Try parsing as plain text with one multiaddr per line
                // example of contacts file exists in resources/network-contacts-examples

                let bootstrap_addresses = response
                    .split('\n')
                    .filter_map(craft_valid_multiaddr_from_str)
                    .collect::<Vec<_>>();

                if bootstrap_addresses.is_empty() {
                    warn!("Failed to parse response as plain text");
                    return Err(Error::FailedToParseCacheData);
                }

                info!(
                    "Successfully parsed {} valid bootstrap addrs from plain text",
                    bootstrap_addresses.len()
                );
                Ok(bootstrap_addresses)
            }
        }
    }
}
