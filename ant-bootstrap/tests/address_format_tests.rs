// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_bootstrap::{craft_valid_multiaddr, craft_valid_multiaddr_from_str, multiaddr_get_peer_id};
use ant_logging::LogBuilder;
use color_eyre::Result;
use libp2p::Multiaddr;
use tracing::info;

#[tokio::test]
async fn test_transport_protocol_variants() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();

    // Test different valid and invalid multiaddr variants
    let variants = vec![
        // QUIC format (valid)
        ("/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE", true),
        // WebSocket format (valid)
        ("/ip4/127.0.0.1/tcp/8080/ws/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE", true),
        // TCP format (valid)
        ("/ip4/127.0.0.1/tcp/8080/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE", true),
        // Missing peer ID (valid)
        ("/ip4/127.0.0.1/tcp/8080", true),
        // No transport protocol (invalid)
        ("/ip4/127.0.0.1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE", false),
        // Invalid protocol chain (invalid)
        ("/ip4/127.0.0.1/wss/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE", false),
    ];

    for (addr_str, should_be_valid) in variants {
        let addr = addr_str.parse::<Multiaddr>()?;
        info!("Testing multiaddr: {}", addr_str);
        let result = craft_valid_multiaddr(&addr);

        if should_be_valid {
            assert!(
                result.is_some(),
                "Should accept valid multiaddr: {addr_str}"
            );
        } else {
            assert!(
                result.is_none(),
                "Should reject invalid multiaddr: {addr_str}"
            );
        }
    }

    Ok(())
}

#[tokio::test]
async fn test_craft_valid_multiaddr_from_str() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();

    // Test valid multiaddr
    let valid_addr =
        "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE";
    let result = craft_valid_multiaddr_from_str(valid_addr);
    assert!(result.is_some(), "Should accept valid multiaddr string");

    // Test invalid multiaddr
    let invalid_addr = "not a multiaddr";
    let result = craft_valid_multiaddr_from_str(invalid_addr);
    assert!(result.is_none(), "Should reject invalid multiaddr string");

    Ok(())
}

#[tokio::test]
async fn test_multiaddr_get_peer_id() -> Result<()> {
    let _guard = LogBuilder::init_single_threaded_tokio_test();

    // Test with peer ID
    let addr_with_peer: Multiaddr =
        "/ip4/127.0.0.1/udp/8080/quic-v1/p2p/12D3KooWRBhwfeP2Y4TCx1SM6s9rUoHhR5STiGwxBhgFRcw3UERE"
            .parse()?;
    let peer_id = multiaddr_get_peer_id(&addr_with_peer);
    assert!(peer_id.is_some(), "Should extract peer ID when present");

    // Test without peer ID
    let addr_without_peer: Multiaddr = "/ip4/127.0.0.1/udp/8080/quic-v1".parse()?;
    let peer_id = multiaddr_get_peer_id(&addr_without_peer);
    assert!(
        peer_id.is_none(),
        "Should return None when peer ID is missing"
    );

    Ok(())
}
