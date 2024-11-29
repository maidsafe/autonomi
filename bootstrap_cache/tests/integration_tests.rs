// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use bootstrap_cache::{BootstrapEndpoints, InitialPeerDiscovery};
use libp2p::Multiaddr;
use tracing_subscriber::{fmt, EnvFilter};
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

// Initialize logging for tests
fn init_logging() {
    let _ = fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();
}

#[tokio::test]
async fn test_fetch_from_amazon_s3() {
    init_logging();
    let discovery = InitialPeerDiscovery::new();
    let peers = discovery.fetch_peers().await.unwrap();

    // We should get some peers
    assert!(!peers.is_empty(), "Expected to find some peers from S3");

    // Verify that all peers have valid multiaddresses
    for peer in &peers {
        println!("Found peer: {}", peer.addr);
        let addr_str = peer.addr.to_string();
        assert!(addr_str.contains("/ip4/"), "Expected IPv4 address");
        assert!(addr_str.contains("/udp/"), "Expected UDP port");
        assert!(addr_str.contains("/quic-v1/"), "Expected QUIC protocol");
        assert!(addr_str.contains("/p2p/"), "Expected peer ID");
    }
}

#[tokio::test]
async fn test_individual_s3_endpoints() {
    init_logging();

    // Start a mock server
    let mock_server = MockServer::start().await;

    // Create mock responses
    let mock_response = r#"/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE
/ip4/127.0.0.2/udp/8081/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERF"#;

    // Mount the mock
    Mock::given(method("GET"))
        .and(path("/peers"))
        .respond_with(ResponseTemplate::new(200).set_body_string(mock_response))
        .mount(&mock_server)
        .await;

    let endpoint = format!("{}/peers", mock_server.uri());
    let discovery = InitialPeerDiscovery::with_endpoints(vec![endpoint.clone()]);

    match discovery.fetch_peers().await {
        Ok(peers) => {
            println!(
                "Successfully fetched {} peers from {}",
                peers.len(),
                endpoint
            );
            assert!(
                !peers.is_empty(),
                "Expected to find peers from {}",
                endpoint
            );

            // Verify first peer's multiaddr format
            if let Some(first_peer) = peers.first() {
                let addr_str = first_peer.addr.to_string();
                println!("First peer from {}: {}", endpoint, addr_str);
                assert!(addr_str.contains("/ip4/"), "Expected IPv4 address");
                assert!(addr_str.contains("/udp/"), "Expected UDP port");
                assert!(addr_str.contains("/quic-v1/"), "Expected QUIC protocol");
                assert!(addr_str.contains("/p2p/"), "Expected peer ID");

                // Try to parse it back to ensure it's valid
                assert!(
                    addr_str.parse::<Multiaddr>().is_ok(),
                    "Should be valid multiaddr"
                );
            }
        }
        Err(e) => {
            panic!("Failed to fetch peers from {}: {}", endpoint, e);
        }
    }
}

#[tokio::test]
async fn test_response_format() {
    init_logging();
    let discovery = InitialPeerDiscovery::new();
    let peers = discovery.fetch_peers().await.unwrap();

    // Get the first peer to check format
    let first_peer = peers.first().expect("Expected at least one peer");
    let addr_str = first_peer.addr.to_string();

    // Print the address for debugging
    println!("First peer address: {}", addr_str);

    // Verify address components
    let components: Vec<&str> = addr_str.split('/').collect();
    assert!(components.contains(&"ip4"), "Missing IP4 component");
    assert!(components.contains(&"udp"), "Missing UDP component");
    assert!(components.contains(&"quic-v1"), "Missing QUIC component");
    assert!(
        components.iter().any(|&c| c == "p2p"),
        "Missing P2P component"
    );

    // Ensure we can parse it back into a multiaddr
    let parsed: Multiaddr = addr_str.parse().expect("Should be valid multiaddr");
    assert_eq!(parsed.to_string(), addr_str, "Multiaddr should round-trip");
}

#[tokio::test]
async fn test_json_endpoint_format() {
    init_logging();
    let mock_server = MockServer::start().await;

    // Create a mock JSON response
    let json_response = r#"
    {
        "peers": [
            "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE",
            "/ip4/127.0.0.2/udp/8081/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERF"
        ],
        "metadata": {
            "description": "Test endpoints",
            "last_updated": "2024-01-01T00:00:00Z"
        }
    }
    "#;

    // Mount the mock
    Mock::given(method("GET"))
        .and(path("/")) // Use root path instead of /peers
        .respond_with(ResponseTemplate::new(200).set_body_string(json_response))
        .mount(&mock_server)
        .await;

    let endpoint = mock_server.uri().to_string();
    let discovery = InitialPeerDiscovery::with_endpoints(vec![endpoint.clone()]);

    let peers = discovery.fetch_peers().await.unwrap();
    assert_eq!(peers.len(), 2);

    // Verify peer addresses
    let addrs: Vec<String> = peers.iter().map(|p| p.addr.to_string()).collect();
    assert!(addrs.contains(
        &"/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .to_string()
    ));
    assert!(addrs.contains(
        &"/ip4/127.0.0.2/udp/8081/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERF"
            .to_string()
    ));
}

#[tokio::test]
async fn test_s3_json_format() {
    init_logging();

    // Fetch and parse the bootstrap cache JSON
    let response =
        reqwest::get("https://sn-testnet.s3.eu-west-2.amazonaws.com/bootstrap_cache.json")
            .await
            .unwrap();
    let json_str = response.text().await.unwrap();

    // Parse using our BootstrapEndpoints struct
    let endpoints: BootstrapEndpoints = serde_json::from_str(&json_str).unwrap();

    // Verify we got all the peers
    assert_eq!(endpoints.peers.len(), 24);

    // Verify we can parse each peer address
    for peer in endpoints.peers {
        peer.parse::<Multiaddr>().unwrap();
    }

    // Verify metadata
    assert_eq!(
        endpoints.metadata.description,
        "Safe Network testnet bootstrap cache"
    );
}
