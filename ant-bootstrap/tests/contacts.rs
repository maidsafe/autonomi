// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_bootstrap::{
    ContactsFetcher,
    cache_store::{cache_data_v0, cache_data_v1},
    get_network_version,
};
use ant_logging::LogBuilder;
use color_eyre::Result;
use libp2p::{Multiaddr, PeerId};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::SystemTime;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

// Valid peer IDs for testing (base58-encoded Ed25519 public keys)
const PEER_ID_1: &str = "12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE";
const PEER_ID_2: &str = "12D3KooWD2aV1f3qkhggzEFaJ24CEFYkSdZF5RKoMLpU6CwExYV5";
const PEER_ID_3: &str = "12D3KooWCKCeqLPSgMnDjyFsJuWqREDtKNHx1JEBiwxME7Zdw68n";

fn valid_quic_addr(ip_suffix: u8, port: u16, peer_id: &str) -> String {
    format!("/ip4/127.0.0.{ip_suffix}/udp/{port}/quic-v1/p2p/{peer_id}")
}

// ============================================================================
// Basic fetching tests (3 tests)
// ============================================================================

#[tokio::test]
async fn test_fetch_from_single_endpoint() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();

    let mock_server = MockServer::start().await;

    let addr = valid_quic_addr(1, 8080, PEER_ID_1);
    Mock::given(method("GET"))
        .and(path("/bootstrap"))
        .respond_with(ResponseTemplate::new(200).set_body_string(&addr))
        .mount(&mock_server)
        .await;

    let url = format!("{}/bootstrap", mock_server.uri()).parse()?;
    let fetcher = ContactsFetcher::with_endpoints(vec![url])?;
    let addrs = fetcher.fetch_bootstrap_addresses().await?;

    assert_eq!(addrs.len(), 1, "Should fetch exactly one address");
    let expected_addr: Multiaddr = addr.parse()?;
    assert_eq!(
        addrs[0].to_string(),
        expected_addr.to_string(),
        "Fetched address should match the expected address"
    );

    Ok(())
}

#[tokio::test]
async fn test_fetch_from_multiple_endpoints() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();

    let mock_server = MockServer::start().await;

    let addr1 = valid_quic_addr(1, 8080, PEER_ID_1);
    let addr2 = valid_quic_addr(2, 8081, PEER_ID_2);

    Mock::given(method("GET"))
        .and(path("/endpoint1"))
        .respond_with(ResponseTemplate::new(200).set_body_string(&addr1))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/endpoint2"))
        .respond_with(ResponseTemplate::new(200).set_body_string(&addr2))
        .mount(&mock_server)
        .await;

    let url1 = format!("{}/endpoint1", mock_server.uri()).parse()?;
    let url2 = format!("{}/endpoint2", mock_server.uri()).parse()?;
    let fetcher = ContactsFetcher::with_endpoints(vec![url1, url2])?;
    let addrs = fetcher.fetch_bootstrap_addresses().await?;

    assert_eq!(addrs.len(), 2, "Should fetch addresses from both endpoints");

    // Both addresses should be present (order may vary due to concurrent fetches)
    let addr_strings: Vec<String> = addrs.iter().map(|a| a.to_string()).collect();
    let expected1: Multiaddr = addr1.parse()?;
    let expected2: Multiaddr = addr2.parse()?;
    assert!(
        addr_strings.contains(&expected1.to_string()),
        "Should contain first address"
    );
    assert!(
        addr_strings.contains(&expected2.to_string()),
        "Should contain second address"
    );

    Ok(())
}

#[tokio::test]
async fn test_fetch_respects_max_addrs() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();

    let mock_server = MockServer::start().await;

    // Return 3 addresses
    let addrs_text = format!(
        "{}\n{}\n{}",
        valid_quic_addr(1, 8080, PEER_ID_1),
        valid_quic_addr(2, 8081, PEER_ID_2),
        valid_quic_addr(3, 8082, PEER_ID_3)
    );

    Mock::given(method("GET"))
        .and(path("/many"))
        .respond_with(ResponseTemplate::new(200).set_body_string(&addrs_text))
        .mount(&mock_server)
        .await;

    let url = format!("{}/many", mock_server.uri()).parse()?;
    let mut fetcher = ContactsFetcher::with_endpoints(vec![url])?;
    fetcher.set_max_addrs(2);

    let addrs = fetcher.fetch_bootstrap_addresses().await?;

    assert_eq!(
        addrs.len(),
        2,
        "Should limit fetched addresses to max_addrs"
    );

    Ok(())
}

// ============================================================================
// Retry logic tests (3 tests)
// ============================================================================

#[tokio::test]
async fn test_fetch_succeeds_first_try() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();

    let mock_server = MockServer::start().await;
    let call_count = Arc::new(AtomicUsize::new(0));
    let call_count_clone = Arc::clone(&call_count);

    let addr = valid_quic_addr(1, 8080, PEER_ID_1);
    let addr_clone = addr.clone();
    Mock::given(method("GET"))
        .and(path("/success"))
        .respond_with(move |_: &wiremock::Request| {
            call_count_clone.fetch_add(1, Ordering::SeqCst);
            ResponseTemplate::new(200).set_body_string(&addr_clone)
        })
        .mount(&mock_server)
        .await;

    let url = format!("{}/success", mock_server.uri()).parse()?;
    let fetcher = ContactsFetcher::with_endpoints(vec![url])?;
    let addrs = fetcher.fetch_bootstrap_addresses().await?;

    assert!(!addrs.is_empty(), "Should fetch addresses successfully");
    assert_eq!(
        call_count.load(Ordering::SeqCst),
        1,
        "Should only need one request"
    );

    Ok(())
}

#[tokio::test]
async fn test_fetch_retries_on_failure() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();

    let mock_server = MockServer::start().await;
    let call_count = Arc::new(AtomicUsize::new(0));
    let call_count_clone = Arc::clone(&call_count);

    let addr = valid_quic_addr(1, 8080, PEER_ID_1);
    let addr_clone = addr.clone();
    Mock::given(method("GET"))
        .and(path("/retry"))
        .respond_with(move |_: &wiremock::Request| {
            let count = call_count_clone.fetch_add(1, Ordering::SeqCst);
            if count < 2 {
                // Fail first two attempts
                ResponseTemplate::new(500)
            } else {
                // Succeed on third attempt
                ResponseTemplate::new(200).set_body_string(&addr_clone)
            }
        })
        .mount(&mock_server)
        .await;

    let url = format!("{}/retry", mock_server.uri()).parse()?;
    let fetcher = ContactsFetcher::with_endpoints(vec![url])?;
    let addrs = fetcher.fetch_bootstrap_addresses().await?;

    assert!(!addrs.is_empty(), "Should succeed after retries");
    assert!(
        call_count.load(Ordering::SeqCst) >= 3,
        "Should have made multiple attempts"
    );

    Ok(())
}

#[tokio::test]
async fn test_fetch_fails_after_max_retries() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();

    let mock_server = MockServer::start().await;
    let call_count = Arc::new(AtomicUsize::new(0));
    let call_count_clone = Arc::clone(&call_count);

    Mock::given(method("GET"))
        .and(path("/always_fail"))
        .respond_with(move |_: &wiremock::Request| {
            call_count_clone.fetch_add(1, Ordering::SeqCst);
            ResponseTemplate::new(500)
        })
        .mount(&mock_server)
        .await;

    let url = format!("{}/always_fail", mock_server.uri()).parse()?;
    let fetcher = ContactsFetcher::with_endpoints(vec![url])?;
    let addrs = fetcher.fetch_bootstrap_addresses().await?;

    // After max retries, the fetcher returns an empty list (graceful degradation)
    assert!(
        addrs.is_empty(),
        "Should return empty list after all retries fail"
    );
    // Should have attempted at least the maximum number of retries (3)
    assert!(
        call_count.load(Ordering::SeqCst) >= 3,
        "Should have attempted max retries"
    );

    Ok(())
}

// ============================================================================
// Response format parsing tests (5 tests)
// ============================================================================

#[tokio::test]
async fn test_parse_json_v1_format() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();

    let mock_server = MockServer::start().await;

    // Create v1 cache data with current network version
    let peer_id: PeerId = PEER_ID_1.parse()?;
    let addr: Multiaddr = valid_quic_addr(1, 8080, PEER_ID_1).parse()?;

    let mut v1_data = cache_data_v1::CacheData::default();
    v1_data.add_peer(peer_id, [addr.clone()].iter(), 10, 100);

    let v1_json = serde_json::to_string(&v1_data)?;

    Mock::given(method("GET"))
        .and(path("/v1"))
        .respond_with(ResponseTemplate::new(200).set_body_string(&v1_json))
        .mount(&mock_server)
        .await;

    let url = format!("{}/v1", mock_server.uri()).parse()?;
    let fetcher = ContactsFetcher::with_endpoints(vec![url])?;
    let addrs = fetcher.fetch_bootstrap_addresses().await?;

    assert!(
        !addrs.is_empty(),
        "Should parse JSON v1 format successfully"
    );
    assert_eq!(
        addrs[0].to_string(),
        addr.to_string(),
        "Should parse address correctly from JSON v1"
    );

    Ok(())
}

#[tokio::test]
async fn test_parse_json_v1_wrong_network_version_returns_empty() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();

    let mock_server = MockServer::start().await;

    // Create v1 cache data with wrong network version
    let peer_id: PeerId = PEER_ID_1.parse()?;
    let addr: Multiaddr = valid_quic_addr(1, 8080, PEER_ID_1).parse()?;

    let mut v1_data = cache_data_v1::CacheData::default();
    v1_data.add_peer(peer_id, [addr.clone()].iter(), 10, 100);
    // Set an invalid network version
    v1_data.network_version = "wrong_version_123".to_string();

    let v1_json = serde_json::to_string(&v1_data)?;

    Mock::given(method("GET"))
        .and(path("/wrong_version"))
        .respond_with(ResponseTemplate::new(200).set_body_string(&v1_json))
        .mount(&mock_server)
        .await;

    let url = format!("{}/wrong_version", mock_server.uri()).parse()?;
    let fetcher = ContactsFetcher::with_endpoints(vec![url])?;
    let addrs = fetcher.fetch_bootstrap_addresses().await?;

    // Wrong network version should return empty list (not an error)
    assert!(
        addrs.is_empty(),
        "Should return empty list for mismatched network version"
    );

    Ok(())
}

#[tokio::test]
async fn test_parse_json_v0_format() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();

    let mock_server = MockServer::start().await;

    // Create v0 cache data (legacy format)
    let peer_id: PeerId = PEER_ID_1.parse()?;
    let addr: Multiaddr = valid_quic_addr(1, 8080, PEER_ID_1).parse()?;

    let mut v0_data = cache_data_v0::CacheData {
        peers: Default::default(),
        last_updated: SystemTime::now(),
        network_version: get_network_version(),
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

    Mock::given(method("GET"))
        .and(path("/v0"))
        .respond_with(ResponseTemplate::new(200).set_body_string(&v0_json))
        .mount(&mock_server)
        .await;

    let url = format!("{}/v0", mock_server.uri()).parse()?;
    let fetcher = ContactsFetcher::with_endpoints(vec![url])?;
    let addrs = fetcher.fetch_bootstrap_addresses().await?;

    assert!(
        !addrs.is_empty(),
        "Should parse JSON v0 format successfully"
    );
    assert_eq!(
        addrs[0].to_string(),
        addr.to_string(),
        "Should parse address correctly from JSON v0"
    );

    Ok(())
}

#[tokio::test]
async fn test_parse_plaintext_format() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();

    let mock_server = MockServer::start().await;

    let addr1 = valid_quic_addr(1, 8080, PEER_ID_1);
    let addr2 = valid_quic_addr(2, 8081, PEER_ID_2);
    let plaintext = format!("{addr1}\n{addr2}");

    Mock::given(method("GET"))
        .and(path("/plaintext"))
        .respond_with(ResponseTemplate::new(200).set_body_string(&plaintext))
        .mount(&mock_server)
        .await;

    let url = format!("{}/plaintext", mock_server.uri()).parse()?;
    let fetcher = ContactsFetcher::with_endpoints(vec![url])?;
    let addrs = fetcher.fetch_bootstrap_addresses().await?;

    assert_eq!(addrs.len(), 2, "Should parse two addresses from plaintext");

    let addr_strings: Vec<String> = addrs.iter().map(|a| a.to_string()).collect();
    let expected1: Multiaddr = addr1.parse()?;
    let expected2: Multiaddr = addr2.parse()?;
    assert!(
        addr_strings.contains(&expected1.to_string()),
        "Should contain first plaintext address"
    );
    assert!(
        addr_strings.contains(&expected2.to_string()),
        "Should contain second plaintext address"
    );

    Ok(())
}

#[tokio::test]
async fn test_parse_plaintext_with_whitespace() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();

    let mock_server = MockServer::start().await;

    let addr = valid_quic_addr(1, 8080, PEER_ID_1);
    // Include empty lines and trailing whitespace (not leading spaces before addr)
    let plaintext_with_whitespace = format!("\n\n{addr}\n\n   \n");

    Mock::given(method("GET"))
        .and(path("/whitespace"))
        .respond_with(ResponseTemplate::new(200).set_body_string(&plaintext_with_whitespace))
        .mount(&mock_server)
        .await;

    let url = format!("{}/whitespace", mock_server.uri()).parse()?;
    let fetcher = ContactsFetcher::with_endpoints(vec![url])?;
    let addrs = fetcher.fetch_bootstrap_addresses().await?;

    assert_eq!(
        addrs.len(),
        1,
        "Should parse one valid address ignoring empty lines and whitespace"
    );
    let expected: Multiaddr = addr.parse()?;
    assert_eq!(
        addrs[0].to_string(),
        expected.to_string(),
        "Should correctly parse address despite whitespace"
    );

    Ok(())
}

#[tokio::test]
async fn test_parse_invalid_json_falls_back_to_plaintext() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();

    let mock_server = MockServer::start().await;

    // Return invalid JSON that starts with '{' but isn't valid JSON
    // followed by valid plaintext multiaddr on next line
    let addr = valid_quic_addr(1, 8080, PEER_ID_1);
    let invalid_json_with_plaintext = format!("{{not valid json}}\n{addr}");

    Mock::given(method("GET"))
        .and(path("/invalid_json"))
        .respond_with(ResponseTemplate::new(200).set_body_string(&invalid_json_with_plaintext))
        .mount(&mock_server)
        .await;

    let url = format!("{}/invalid_json", mock_server.uri()).parse()?;
    let fetcher = ContactsFetcher::with_endpoints(vec![url])?;
    let addrs = fetcher.fetch_bootstrap_addresses().await?;

    // When JSON parsing fails, should fall back to plaintext parsing
    // The invalid JSON line should be skipped, valid multiaddr should be parsed
    assert_eq!(
        addrs.len(),
        1,
        "Should fall back to plaintext parsing when JSON is invalid"
    );
    let expected: Multiaddr = addr.parse()?;
    assert_eq!(
        addrs[0].to_string(),
        expected.to_string(),
        "Should parse valid multiaddr from plaintext fallback"
    );

    Ok(())
}

// ============================================================================
// Error handling tests (3 tests)
// ============================================================================

#[tokio::test]
async fn test_parse_empty_response() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();

    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/empty"))
        .respond_with(ResponseTemplate::new(200).set_body_string(""))
        .mount(&mock_server)
        .await;

    let url = format!("{}/empty", mock_server.uri()).parse()?;
    let fetcher = ContactsFetcher::with_endpoints(vec![url])?;
    let addrs = fetcher.fetch_bootstrap_addresses().await?;

    // Empty response should return empty list (graceful degradation)
    assert!(
        addrs.is_empty(),
        "Empty response should return empty list, not error"
    );

    Ok(())
}

#[tokio::test]
async fn test_network_failure_graceful_degradation() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();

    // Use a URL that will fail to connect (closed port or unreachable)
    // The mock server is not started, so this URL won't work
    let url = "http://127.0.0.1:59999/nonexistent".parse()?;
    let fetcher = ContactsFetcher::with_endpoints(vec![url])?;
    let addrs = fetcher.fetch_bootstrap_addresses().await?;

    // Network failure should return empty list (graceful degradation)
    assert!(
        addrs.is_empty(),
        "Network failure should return empty list, not propagate error"
    );

    Ok(())
}

#[tokio::test]
async fn test_invalid_url_returns_error() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();

    // Try to create a fetcher with an invalid URL
    // Note: url::Url::parse will reject truly invalid URLs, so we need to test
    // at the URL parsing level
    let invalid_url_str = "not_a_valid_url";
    let parse_result: std::result::Result<url::Url, _> = invalid_url_str.parse();

    assert!(parse_result.is_err(), "Parsing an invalid URL should fail");

    // Test with a URL that looks like it might be valid but isn't quite right
    let malformed_url_str = "http://";
    let parse_result2: std::result::Result<url::Url, _> = malformed_url_str.parse();

    assert!(
        parse_result2.is_err(),
        "Parsing a malformed URL should fail"
    );

    Ok(())
}

// ============================================================================
// Endpoint management tests (4 tests)
// ============================================================================

#[tokio::test]
async fn test_insert_endpoint() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();

    // Create an empty fetcher
    let mut fetcher = ContactsFetcher::new()?;

    // Initially should have no endpoints
    assert!(
        fetcher.endpoints.is_empty(),
        "New fetcher should have no endpoints"
    );

    // Insert first endpoint
    let url1: url::Url = "http://127.0.0.1:8080/bootstrap1".parse()?;
    fetcher.insert_endpoint(url1.clone());
    assert_eq!(
        fetcher.endpoints.len(),
        1,
        "Should have 1 endpoint after first insert"
    );
    assert_eq!(
        fetcher.endpoints[0].to_string(),
        url1.to_string(),
        "First endpoint should match"
    );

    // Insert second endpoint
    let url2: url::Url = "http://127.0.0.2:8081/bootstrap2".parse()?;
    fetcher.insert_endpoint(url2.clone());
    assert_eq!(
        fetcher.endpoints.len(),
        2,
        "Should have 2 endpoints after second insert"
    );
    assert_eq!(
        fetcher.endpoints[1].to_string(),
        url2.to_string(),
        "Second endpoint should match"
    );

    Ok(())
}

#[tokio::test]
async fn test_insert_endpoint_used_in_fetch() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();

    let mock_server = MockServer::start().await;

    let addr = valid_quic_addr(1, 8080, PEER_ID_1);
    Mock::given(method("GET"))
        .and(path("/dynamic"))
        .respond_with(ResponseTemplate::new(200).set_body_string(&addr))
        .mount(&mock_server)
        .await;

    // Create empty fetcher and dynamically add endpoint
    let mut fetcher = ContactsFetcher::new()?;
    let url: url::Url = format!("{}/dynamic", mock_server.uri()).parse()?;
    fetcher.insert_endpoint(url);

    // Fetch should use the dynamically added endpoint
    let addrs = fetcher.fetch_bootstrap_addresses().await?;

    assert!(
        !addrs.is_empty(),
        "Should fetch from dynamically added endpoint"
    );
    let expected: Multiaddr = addr.parse()?;
    assert_eq!(
        addrs[0].to_string(),
        expected.to_string(),
        "Should fetch correct address from dynamic endpoint"
    );

    Ok(())
}

#[tokio::test]
async fn test_with_mainnet_endpoints() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();

    let fetcher = ContactsFetcher::with_mainnet_endpoints()?;

    // MAINNET_CONTACTS has exactly 6 endpoints
    assert_eq!(
        fetcher.endpoints.len(),
        6,
        "Mainnet fetcher should have exactly 6 endpoints"
    );

    // Verify some expected URLs are present
    let endpoint_strings: Vec<String> = fetcher.endpoints.iter().map(|u| u.to_string()).collect();

    // Check for the S3 endpoint
    assert!(
        endpoint_strings
            .iter()
            .any(|s| s.contains("sn-testnet.s3.eu-west-2.amazonaws.com")),
        "Mainnet endpoints should include S3 URL"
    );

    // Check for at least one of the direct IP endpoints
    assert!(
        endpoint_strings
            .iter()
            .any(|s| s.contains("159.89.251.80") || s.contains("159.65.210.89")),
        "Mainnet endpoints should include direct IP URLs"
    );

    Ok(())
}

#[tokio::test]
async fn test_with_alphanet_endpoints() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();

    let fetcher = ContactsFetcher::with_alphanet_endpoints()?;

    // ALPHANET_CONTACTS has exactly 5 endpoints
    assert_eq!(
        fetcher.endpoints.len(),
        5,
        "Alphanet fetcher should have exactly 5 endpoints"
    );

    // Verify some expected URLs are present
    let endpoint_strings: Vec<String> = fetcher.endpoints.iter().map(|u| u.to_string()).collect();

    // Check for some of the alphanet IP endpoints
    assert!(
        endpoint_strings
            .iter()
            .any(|s| s.contains("188.166.133.208") || s.contains("188.166.133.125")),
        "Alphanet endpoints should include expected IP URLs"
    );

    // All URLs should end with bootstrap_cache.json
    for url in &endpoint_strings {
        assert!(
            url.ends_with("bootstrap_cache.json"),
            "Alphanet endpoint should end with bootstrap_cache.json: {url}"
        );
    }

    Ok(())
}
