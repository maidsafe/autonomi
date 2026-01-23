// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_bootstrap::{
    ANT_PEERS_ENV, ContactsFetcher, InitialPeersConfig,
    cache_store::{cache_data_v0, cache_data_v1},
};
use ant_logging::LogBuilder;
use color_eyre::Result;
use libp2p::{Multiaddr, PeerId};
use std::{sync::atomic::Ordering, time::SystemTime};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

#[tokio::test]
async fn test_network_contacts_formats() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    // Start mock server
    let mock_server = MockServer::start().await;

    // Test v0 cache data format
    let peer_id = PeerId::random();
    let addr: Multiaddr =
        "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()?;
    let mut v0_data = cache_data_v0::CacheData {
        peers: Default::default(),
        last_updated: SystemTime::now(),
        network_version: ant_bootstrap::get_network_version(),
    };
    let boot_addr = cache_data_v0::BootstrapAddr {
        addr: addr.clone(),
        success_count: 1,
        failure_count: 0,
        last_seen: SystemTime::now(),
    };
    let v0_addrs = cache_data_v0::BootstrapAddresses(vec![boot_addr]);
    v0_data.peers.insert(peer_id, v0_addrs);
    let v0_json = serde_json::to_string(&v0_data)?;

    // Test v1 cache data format
    let mut v1_data = cache_data_v1::CacheData::default();
    v1_data.add_peer(peer_id, [addr.clone()].iter(), 10, 100);
    let v1_json = serde_json::to_string(&v1_data)?;

    // Set up mock endpoints
    Mock::given(method("GET"))
        .and(path("/v0"))
        .respond_with(ResponseTemplate::new(200).set_body_string(v0_json))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_string(v1_json))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/text"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE\n"
        ))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/malformed"))
        .respond_with(ResponseTemplate::new(200).set_body_string("this is not valid data"))
        .mount(&mock_server)
        .await;

    // Test each format
    for endpoint in &["/v0", "/v1", "/text"] {
        let url = format!("{}{}", mock_server.uri(), endpoint).parse()?;
        let fetcher = ContactsFetcher::with_endpoints(vec![url])?;
        let addrs = fetcher.fetch_bootstrap_addresses().await?;

        assert!(!addrs.is_empty(), "Should fetch addresses from {endpoint}");
        assert_eq!(
            addrs[0].to_string(),
            addr.to_string(),
            "Should parse address correctly from {endpoint}"
        );
    }

    // Test malformed response
    let bad_url = format!("{}/malformed", mock_server.uri()).parse()?;
    let fetcher = ContactsFetcher::with_endpoints(vec![bad_url])?;
    let addrs = fetcher.fetch_bootstrap_addresses().await?;
    assert_eq!(addrs.len(), 0, "Should handle malformed data");

    Ok(())
}

#[tokio::test]
async fn test_network_contacts_retries() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    // Start mock server
    let mock_server = MockServer::start().await;

    // Set up endpoint that fails twice then succeeds
    let call_count = std::sync::atomic::AtomicUsize::new(0);

    Mock::given(method("GET"))
        .and(path("/retry"))
        .respond_with(move |_: &wiremock::Request| {
            let count = call_count.fetch_add(1, Ordering::SeqCst);
            if count < 2 {
                ResponseTemplate::new(500)
            } else {
                ResponseTemplate::new(200).set_body_string(
                    "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
                )
            }
        })
        .mount(&mock_server)
        .await;

    // Test retry logic
    let url = format!("{}/retry", mock_server.uri()).parse()?;
    let fetcher = ContactsFetcher::with_endpoints(vec![url])?;
    let addrs = fetcher.fetch_bootstrap_addresses().await?;

    assert!(!addrs.is_empty(), "Should succeed after retries");

    Ok(())
}

#[tokio::test]
async fn test_env_variable_parsing() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();

    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var(
            ANT_PEERS_ENV,
            "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE,/ip4/127.0.0.2/udp/8081/quic-v1/p2p/12D3KooWD2aV1f3qkhggzEFaJ24CEFYkSdZF5RKoMLpU6CwExYV5",
        );
    }

    let addrs = InitialPeersConfig::read_bootstrap_addr_from_env();

    #[allow(unsafe_code)]
    unsafe {
        std::env::remove_var(ANT_PEERS_ENV);
    }

    assert_eq!(addrs.len(), 2, "Should parse both addresses from env var");

    Ok(())
}

#[tokio::test]
async fn test_fetch_max_addresses() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();
    // Start mock server
    let mock_server = MockServer::start().await;

    // Set up endpoint with multiple peers
    Mock::given(method("GET"))
        .and(path("/multiple"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE\n\
             /ip4/127.0.0.2/udp/8081/quic-v1/p2p/12D3KooWD2aV1f3qkhggzEFaJ24CEFYkSdZF5RKoMLpU6CwExYV5\n\
             /ip4/127.0.0.3/udp/8082/quic-v1/p2p/12D3KooWCKCeqLPSgMnDjyFsJuWqREDtKNHx1JEBiwxME7Zdw68n"
        ))
        .mount(&mock_server)
        .await;

    // Get max 2 addresses
    let url = format!("{}/multiple", mock_server.uri()).parse()?;
    let mut fetcher = ContactsFetcher::with_endpoints(vec![url])?;
    fetcher.set_max_addrs(2);

    let addrs = fetcher.fetch_bootstrap_addresses().await?;

    assert_eq!(addrs.len(), 2, "Should limit to max_addrs");

    Ok(())
}
