// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{
    circuit_breaker::{CircuitBreaker, CircuitBreakerConfig},
    BootstrapEndpoints, BootstrapPeer, Error, Result,
};
use libp2p::Multiaddr;
use reqwest::Client;
use tokio::time::timeout;
use tracing::{info, warn};

const DEFAULT_JSON_ENDPOINT: &str =
    "https://sn-testnet.s3.eu-west-2.amazonaws.com/network-contacts";

const DEFAULT_BOOTSTRAP_ENDPOINTS: &[&str] = &[
    DEFAULT_JSON_ENDPOINT,
];

const FETCH_TIMEOUT_SECS: u64 = 30;

/// Discovers initial peers from a list of endpoints
pub struct InitialPeerDiscovery {
    endpoints: Vec<String>,
    client: Client,
    circuit_breaker: CircuitBreaker,
}

impl Default for InitialPeerDiscovery {
    fn default() -> Self {
        Self::new()
    }
}

impl InitialPeerDiscovery {
    pub fn new() -> Self {
        Self {
            endpoints: DEFAULT_BOOTSTRAP_ENDPOINTS
                .iter()
                .map(|s| s.to_string())
                .collect(),
            client: Client::new(),
            circuit_breaker: CircuitBreaker::new(),
        }
    }

    pub fn with_endpoints(endpoints: Vec<String>) -> Self {
        Self {
            endpoints,
            client: Client::new(),
            circuit_breaker: CircuitBreaker::new(),
        }
    }

    pub fn with_config(
        endpoints: Vec<String>,
        circuit_breaker_config: CircuitBreakerConfig,
    ) -> Self {
        Self {
            endpoints,
            client: Client::new(),
            circuit_breaker: CircuitBreaker::with_config(circuit_breaker_config),
        }
    }

    /// Load endpoints from a JSON file
    pub async fn from_json(json_str: &str) -> Result<Self> {
        let endpoints: BootstrapEndpoints = serde_json::from_str(json_str)?;
        Ok(Self {
            endpoints: endpoints.peers,
            client: Client::new(),
            circuit_breaker: CircuitBreaker::new(),
        })
    }

    /// Fetch peers from all configured endpoints
    pub async fn fetch_peers(&self) -> Result<Vec<BootstrapPeer>> {
        info!("Starting peer discovery from {} endpoints: {:?}", self.endpoints.len(), self.endpoints);
        let mut peers = Vec::new();
        let mut last_error = None;

        for endpoint in &self.endpoints {
            info!("Attempting to fetch peers from endpoint: {}", endpoint);
            match self.fetch_from_endpoint(endpoint).await {
                Ok(mut endpoint_peers) => {
                    info!(
                        "Successfully fetched {} peers from {}. First few peers: {:?}",
                        endpoint_peers.len(),
                        endpoint,
                        endpoint_peers.iter().take(3).collect::<Vec<_>>()
                    );
                    peers.append(&mut endpoint_peers);
                }
                Err(e) => {
                    warn!("Failed to fetch peers from {}: {}", endpoint, e);
                    last_error = Some(e);
                }
            }
        }

        if peers.is_empty() {
            if let Some(e) = last_error {
                warn!("No peers found from any endpoint. Last error: {}", e);
                Err(Error::NoPeersFound(format!(
                    "No valid peers found from any endpoint: {}",
                    e
                )))
            } else {
                warn!("No peers found from any endpoint and no errors reported");
                Err(Error::NoPeersFound(
                    "No valid peers found from any endpoint".to_string(),
                ))
            }
        } else {
            info!(
                "Successfully discovered {} total peers. First few: {:?}",
                peers.len(),
                peers.iter().take(3).collect::<Vec<_>>()
            );
            Ok(peers)
        }
    }

    async fn fetch_from_endpoint(&self, endpoint: &str) -> Result<Vec<BootstrapPeer>> {
        // Check circuit breaker state
        if !self.circuit_breaker.check_endpoint(endpoint).await {
            warn!("Circuit breaker is open for endpoint: {}", endpoint);
            return Err(Error::CircuitBreakerOpen(endpoint.to_string()));
        }

        // Get backoff duration and wait if necessary
        let backoff = self.circuit_breaker.get_backoff_duration(endpoint).await;
        if !backoff.is_zero() {
            info!("Backing off for {:?} before trying endpoint: {}", backoff, endpoint);
        }
        tokio::time::sleep(backoff).await;

        info!("Fetching peers from endpoint: {}", endpoint);
        // Get backoff duration and wait if necessary
        let result = async {
            info!("Sending HTTP request to {}", endpoint);
            let response = match timeout(
                std::time::Duration::from_secs(FETCH_TIMEOUT_SECS),
                self.client.get(endpoint).send(),
            )
            .await {
                Ok(resp) => match resp {
                    Ok(r) => {
                        info!("Got response with status: {}", r.status());
                        r
                    }
                    Err(e) => {
                        warn!("HTTP request failed: {}", e);
                        return Err(Error::RequestFailed(e.to_string()));
                    }
                },
                Err(_) => {
                    warn!("Request timed out after {} seconds", FETCH_TIMEOUT_SECS);
                    return Err(Error::RequestTimeout);
                }
            };

            let content = match response.text().await {
                Ok(c) => {
                    info!("Received response content length: {}", c.len());
                    if c.len() < 1000 { // Only log if content is not too large
                        info!("Response content: {}", c);
                    }
                    c
                }
                Err(e) => {
                    warn!("Failed to get response text: {}", e);
                    return Err(Error::InvalidResponse(format!("Failed to get response text: {}", e)));
                }
            };

            // Try parsing as JSON first
            if content.trim().starts_with('{') {
                info!("Attempting to parse response as JSON");
                match serde_json::from_str::<BootstrapEndpoints>(&content) {
                    Ok(json_endpoints) => {
                        info!("Successfully parsed JSON response with {} peers", json_endpoints.peers.len());
                        let peers = json_endpoints
                            .peers
                            .into_iter()
                            .filter_map(|addr| match addr.parse::<Multiaddr>() {
                                Ok(addr) => Some(BootstrapPeer::new(addr)),
                                Err(e) => {
                                    warn!("Failed to parse multiaddr {}: {}", addr, e);
                                    None
                                }
                            })
                            .collect::<Vec<_>>();

                        if peers.is_empty() {
                            warn!("No valid peers found in JSON response");
                            Err(Error::NoPeersFound(
                                "No valid peers found in JSON response".to_string(),
                            ))
                        } else {
                            info!("Successfully parsed {} valid peers from JSON", peers.len());
                            Ok(peers)
                        }
                    }
                    Err(e) => {
                        warn!("Failed to parse JSON response: {}", e);
                        Err(Error::InvalidResponse(format!(
                            "Invalid JSON format: {}",
                            e
                        )))
                    }
                }
            } else {
                info!("Attempting to parse response as plain text");
                // Try parsing as plain text with one multiaddr per line
                let peers = content
                    .lines()
                    .filter(|line| !line.trim().is_empty())
                    .filter_map(|line| match line.trim().parse::<Multiaddr>() {
                        Ok(addr) => Some(BootstrapPeer::new(addr)),
                        Err(e) => {
                            warn!("Failed to parse multiaddr {}: {}", line, e);
                            None
                        }
                    })
                    .collect::<Vec<_>>();

                if peers.is_empty() {
                    warn!("No valid peers found in plain text response");
                    Err(Error::NoPeersFound(
                        "No valid peers found in plain text response".to_string(),
                    ))
                } else {
                    info!("Successfully parsed {} valid peers from plain text", peers.len());
                    Ok(peers)
                }
            }
        }
        .await;

        match result {
            Ok(peers) => {
                info!("Successfully fetched {} peers from {}", peers.len(), endpoint);
                self.circuit_breaker.record_success(endpoint).await;
                Ok(peers)
            }
            Err(e) => {
                warn!("Failed to fetch peers from {}: {}", endpoint, e);
                self.circuit_breaker.record_failure(endpoint).await;
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    #[tokio::test]
    async fn test_fetch_peers() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string("/ip4/127.0.0.1/tcp/8080\n/ip4/127.0.0.2/tcp/8080"),
            )
            .mount(&mock_server)
            .await;

        let mut discovery = InitialPeerDiscovery::new();
        discovery.endpoints = vec![mock_server.uri()];

        let peers = discovery.fetch_peers().await.unwrap();
        assert_eq!(peers.len(), 2);

        let addr1: Multiaddr = "/ip4/127.0.0.1/tcp/8080".parse().unwrap();
        let addr2: Multiaddr = "/ip4/127.0.0.2/tcp/8080".parse().unwrap();
        assert!(peers.iter().any(|p| p.addr == addr1));
        assert!(peers.iter().any(|p| p.addr == addr2));
    }

    #[tokio::test]
    async fn test_endpoint_failover() {
        let mock_server1 = MockServer::start().await;
        let mock_server2 = MockServer::start().await;

        // First endpoint fails
        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&mock_server1)
            .await;

        // Second endpoint succeeds
        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_string("/ip4/127.0.0.1/tcp/8080"))
            .mount(&mock_server2)
            .await;

        let mut discovery = InitialPeerDiscovery::new();
        discovery.endpoints = vec![mock_server1.uri(), mock_server2.uri()];

        let peers = discovery.fetch_peers().await.unwrap();
        assert_eq!(peers.len(), 1);

        let addr: Multiaddr = "/ip4/127.0.0.1/tcp/8080".parse().unwrap();
        assert_eq!(peers[0].addr, addr);
    }

    #[tokio::test]
    async fn test_invalid_multiaddr() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(
                    "/ip4/127.0.0.1/tcp/8080\ninvalid-addr\n/ip4/127.0.0.2/tcp/8080",
                ),
            )
            .mount(&mock_server)
            .await;

        let mut discovery = InitialPeerDiscovery::new();
        discovery.endpoints = vec![mock_server.uri()];

        let peers = discovery.fetch_peers().await.unwrap();
        let valid_addr: Multiaddr = "/ip4/127.0.0.1/tcp/8080".parse().unwrap();
        assert_eq!(peers[0].addr, valid_addr);
    }

    #[tokio::test]
    async fn test_empty_response() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_string(""))
            .mount(&mock_server)
            .await;

        let mut discovery = InitialPeerDiscovery::new();
        discovery.endpoints = vec![mock_server.uri()];

        let result = discovery.fetch_peers().await;
        assert!(matches!(result, Err(Error::NoPeersFound(_))));
    }

    #[tokio::test]
    async fn test_whitespace_and_empty_lines() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string("\n  \n/ip4/127.0.0.1/tcp/8080\n  \n"),
            )
            .mount(&mock_server)
            .await;

        let mut discovery = InitialPeerDiscovery::new();
        discovery.endpoints = vec![mock_server.uri()];

        let peers = discovery.fetch_peers().await.unwrap();
        assert_eq!(peers.len(), 1);

        let addr: Multiaddr = "/ip4/127.0.0.1/tcp/8080".parse().unwrap();
        assert_eq!(peers[0].addr, addr);
    }

    #[tokio::test]
    async fn test_default_endpoints() {
        let discovery = InitialPeerDiscovery::new();
        assert_eq!(discovery.endpoints.len(), 1);
        assert_eq!(
            discovery.endpoints[0],
            "https://sn-testnet.s3.eu-west-2.amazonaws.com/network-contacts"
        );
    }

    #[tokio::test]
    async fn test_custom_endpoints() {
        let endpoints = vec!["http://example.com".to_string()];
        let discovery = InitialPeerDiscovery::with_endpoints(endpoints.clone());
        assert_eq!(discovery.endpoints, endpoints);
    }

    #[tokio::test]
    async fn test_json_endpoints() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"peers": ["/ip4/127.0.0.1/tcp/8080", "/ip4/127.0.0.2/tcp/8080"]}"#,
            ))
            .mount(&mock_server)
            .await;

        let mut discovery = InitialPeerDiscovery::new();
        discovery.endpoints = vec![mock_server.uri()];

        let peers = discovery.fetch_peers().await.unwrap();
        assert_eq!(peers.len(), 2);

        let addr1: Multiaddr = "/ip4/127.0.0.1/tcp/8080".parse().unwrap();
        let addr2: Multiaddr = "/ip4/127.0.0.2/tcp/8080".parse().unwrap();
        assert!(peers.iter().any(|p| p.addr == addr1));
        assert!(peers.iter().any(|p| p.addr == addr2));
    }
}
