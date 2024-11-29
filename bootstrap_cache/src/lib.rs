//! Bootstrap Cache for Safe Network
//!
//! This crate provides a decentralized peer discovery and caching system for the Safe Network.
//! It implements a robust peer management system with the following features:
//!
//! - Decentralized Design: No dedicated bootstrap nodes required
//! - Cross-Platform Support: Works on Linux, macOS, and Windows
//! - Shared Cache: System-wide cache file accessible by both nodes and clients
//! - Concurrent Access: File locking for safe multi-process access
//! - Atomic Operations: Safe cache updates using atomic file operations
//! - Initial Peer Discovery: Fallback web endpoints for new/stale cache scenarios
//! - Comprehensive Error Handling: Detailed error types and logging
//! - Circuit Breaker Pattern: Intelligent failure handling
//!
//! # Example
//!
//! ```no_run
//! use bootstrap_cache::{CacheStore, BootstrapConfig, PeersArgs};
//! use url::Url;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let config = BootstrapConfig::default();
//! let args = PeersArgs {
//!     first: false,
//!     peers: vec![],
//!     network_contacts_url: Some(Url::parse("https://example.com/peers")?),
//!     local: false,
//!     test_network: false,
//! };
//!
//! let store = CacheStore::from_args(args, config).await?;
//! let peers = store.get_peers().await;
//! # Ok(())
//! # }
//! ```

mod cache_store;
mod circuit_breaker;
pub mod config;
mod error;
mod initial_peer_discovery;

use libp2p::{multiaddr::Protocol, Multiaddr};
use serde::{Deserialize, Serialize};
use std::{fmt, net::SocketAddrV4, time::SystemTime};
use thiserror::Error;
use std::env;
use url::Url;
use tracing::{info, warn};

pub use cache_store::CacheStore;
pub use config::BootstrapConfig;
pub use error::{Error, Result};
pub use initial_peer_discovery::InitialPeerDiscovery;

/// Parse strings like `1.2.3.4:1234` and `/ip4/1.2.3.4/tcp/1234` into a multiaddr.
/// This matches the behavior of sn_peers_acquisition.
pub fn parse_peer_addr(addr: &str) -> std::result::Result<Multiaddr, libp2p::multiaddr::Error> {
    // Parse valid IPv4 socket address, e.g. `1.2.3.4:1234`.
    if let Ok(addr) = addr.parse::<SocketAddrV4>() {
        let start_addr = Multiaddr::from(*addr.ip());
        // Always use UDP and QUIC-v1 for socket addresses
        let multiaddr = start_addr
            .with(Protocol::Udp(addr.port()))
            .with(Protocol::QuicV1);

        return Ok(multiaddr);
    }

    // Parse any valid multiaddr string
    addr.parse::<Multiaddr>()
}

/// Structure representing a list of bootstrap endpoints
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapEndpoints {
    /// List of peer multiaddresses
    pub peers: Vec<String>,
    /// Optional metadata about the endpoints
    #[serde(default)]
    pub metadata: EndpointMetadata,
}

/// Metadata about bootstrap endpoints
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointMetadata {
    /// When the endpoints were last updated
    #[serde(default = "default_last_updated")]
    pub last_updated: String,
    /// Optional description of the endpoints
    #[serde(default)]
    pub description: String,
}

fn default_last_updated() -> String {
    chrono::Utc::now().to_rfc3339()
}

impl Default for EndpointMetadata {
    fn default() -> Self {
        Self {
            last_updated: default_last_updated(),
            description: String::new(),
        }
    }
}

/// A peer that can be used for bootstrapping into the network
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapPeer {
    /// The multiaddress of the peer
    pub addr: Multiaddr,
    /// The number of successful connections to this peer
    pub success_count: u32,
    /// The number of failed connection attempts to this peer
    pub failure_count: u32,
    /// The last time this peer was successfully contacted
    pub last_seen: SystemTime,
}

impl BootstrapPeer {
    pub fn new(addr: Multiaddr) -> Self {
        Self {
            addr,
            success_count: 0,
            failure_count: 0,
            last_seen: SystemTime::now(),
        }
    }

    pub fn update_status(&mut self, success: bool) {
        if success {
            self.success_count += 1;
            self.last_seen = SystemTime::now();
        } else {
            self.failure_count += 1;
        }
    }

    pub fn is_reliable(&self) -> bool {
        // A peer is considered reliable if it has more successes than failures
        self.success_count > self.failure_count
    }
}

impl fmt::Display for BootstrapPeer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "BootstrapPeer {{ addr: {}, last_seen: {:?}, success: {}, failure: {} }}",
            self.addr, self.last_seen, self.success_count, self.failure_count
        )
    }
}

/// Command line arguments for peer configuration
#[derive(Debug, Clone)]
pub struct PeersArgs {
    /// First node in the network
    pub first: bool,
    /// List of peer addresses
    pub peers: Vec<Multiaddr>,
    /// URL to fetch network contacts from
    pub network_contacts_url: Option<Url>,
    /// Use only local discovery (mDNS)
    pub local: bool,
    /// Test network mode - only use provided peers
    pub test_network: bool,
}

impl Default for PeersArgs {
    fn default() -> Self {
        Self {
            first: false,
            peers: Vec::new(),
            network_contacts_url: None,
            local: false,
            test_network: false,
        }
    }
}

/// Validates that a multiaddr has all required components for a valid peer address
pub(crate) fn is_valid_peer_addr(addr: &Multiaddr) -> bool {
    let mut has_ip = false;
    let mut has_port = false;
    let mut has_protocol = false;

    for protocol in addr.iter() {
        match protocol {
            Protocol::Ip4(_) | Protocol::Ip6(_) => has_ip = true,
            Protocol::Tcp(_) | Protocol::Udp(_) => has_port = true,
            Protocol::QuicV1 => has_protocol = true,
            _ => {}
        }
    }

    has_ip && has_port && has_protocol
}

impl CacheStore {
    /// Create a new CacheStore from command line arguments
    pub async fn from_args(args: PeersArgs, config: BootstrapConfig) -> Result<Self> {
        // If this is the first node, return empty store with no fallback
        if args.first {
            info!("First node in network, returning empty store");
            let store = Self::new_without_init(config).await?;
            store.clear_peers().await?;
            return Ok(store);
        }

        // If local mode is enabled, return empty store (will use mDNS)
        if args.local {
            info!("Local mode enabled, using only local discovery");
            let store = Self::new_without_init(config).await?;
            store.clear_peers().await?;
            return Ok(store);
        }

        // If test network mode is enabled, use in-memory store only
        if args.test_network {
            info!("Test network mode enabled, using in-memory store only");
            let mut config = config;
            config.cache_file_path = "".into(); // Empty path to prevent file operations
            let store = Self::new_without_init(config).await?;

            // Add peers from arguments if present
            for peer in args.peers {
                if is_valid_peer_addr(&peer) {
                    info!("Adding peer from arguments: {}", peer);
                    store.add_peer(peer).await?;
                }
            }

            // If network contacts URL is provided, fetch peers from there
            if let Some(url) = args.network_contacts_url {
                info!("Attempting to fetch peers from network contacts URL: {}", url);
                let discovery = InitialPeerDiscovery::with_endpoints(vec![url.to_string()]);
                match discovery.fetch_peers().await {
                    Ok(peers) => {
                        info!("Successfully fetched {} peers from network contacts", peers.len());
                        for peer in peers {
                            if is_valid_peer_addr(&peer.addr) {
                                store.add_peer(peer.addr).await?;
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Failed to fetch peers from network contacts: {}", e);
                    }
                }
            }

            return Ok(store);
        }

        // Create a new store but don't load from cache or fetch from endpoints yet
        let mut store = Self::new_without_init(config).await?;

        // Add peers from environment variable if present
        let mut has_specific_peers = false;
        if let Ok(env_peers) = std::env::var("SAFE_PEERS") {
            for peer_str in env_peers.split(',') {
                if let Ok(peer) = peer_str.parse() {
                    if is_valid_peer_addr(&peer) {
                        info!("Adding peer from environment: {}", peer);
                        store.add_peer(peer).await?;
                        has_specific_peers = true;
                    } else {
                        warn!("Invalid peer address format from environment: {}", peer);
                    }
                }
            }
        }

        // Add peers from arguments if present
        for peer in args.peers {
            if is_valid_peer_addr(&peer) {
                info!("Adding peer from arguments: {}", peer);
                store.add_peer(peer).await?;
                has_specific_peers = true;
            } else {
                warn!("Invalid peer address format from arguments: {}", peer);
            }
        }

        // If we have peers, update cache and return
        if has_specific_peers {
            info!("Using provided peers and updating cache");
            store.save_cache().await?;
            return Ok(store);
        }

        // If no peers specified, try network contacts URL
        if let Some(url) = args.network_contacts_url {
            info!("Attempting to fetch peers from network contacts URL: {}", url);
            let discovery = InitialPeerDiscovery::with_endpoints(vec![url.to_string()]);
            match discovery.fetch_peers().await {
                Ok(peers) => {
                    info!("Successfully fetched {} peers from network contacts", peers.len());
                    for peer in peers {
                        if is_valid_peer_addr(&peer.addr) {
                            store.add_peer(peer.addr).await?;
                            has_specific_peers = true;
                        } else {
                            warn!("Invalid peer address format from network contacts: {}", peer.addr);
                        }
                    }
                    if has_specific_peers {
                        info!("Successfully fetched {} peers from network contacts", store.get_peers().await.len());
                    }
                }
                Err(e) => {
                    warn!("Failed to fetch peers from network contacts: {}", e);
                }
            }
        }

        // If no peers from any source, initialize from cache and default endpoints
        if !has_specific_peers {
            store.init().await?;
        }

        Ok(store)
    }
}

/// Creates a new bootstrap cache with default configuration
pub async fn new() -> Result<CacheStore> {
    CacheStore::new(Default::default()).await
}

/// Creates a new bootstrap cache with custom configuration
pub async fn with_config(config: BootstrapConfig) -> Result<CacheStore> {
    CacheStore::new(config).await
}
