// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::networking::{Quorum, RetryStrategy, Strategy};
pub use ant_bootstrap::{BootstrapCacheConfig, InitialPeersConfig, error::Error as BootstrapError};
use ant_evm::EvmNetwork;
use std::num::NonZero;

/// Configuration for the [`crate::Client`] which can be provided through: [`crate::Client::init_with_config`].
#[derive(Debug, Clone, Default)]
pub struct ClientConfig {
    /// Configuration for the Bootstrap Cache.
    pub bootstrap_cache_config: Option<BootstrapCacheConfig>,

    /// EVM network to use for quotations and payments.
    pub evm_network: EvmNetwork,

    /// Configurations to fetch the initial peers which is used to bootstrap the network.
    /// Also contains the configurations to the bootstrap cache.
    pub init_peers_config: InitialPeersConfig,

    /// The network ID to use for the client.
    /// This is used to differentiate between different networks.
    pub network_id: Option<u8>,

    /// Strategy for data operations by the client.
    pub strategy: ClientOperatingStrategy,
}

/// Strategy configuration for data operations by the client.
///
/// Default values are used for each type of data, but you can override them here.
#[derive(Debug, Clone)]
pub struct ClientOperatingStrategy {
    pub chunks: Strategy,
    pub graph_entry: Strategy,
    pub pointer: Strategy,
    pub scratchpad: Strategy,
    /// Enable chunk caching for faster retrieval
    pub chunk_cache_enabled: bool,
    /// Custom chunk cache directory (if None, uses default)
    pub chunk_cache_dir: Option<std::path::PathBuf>,
}

impl ClientOperatingStrategy {
    pub fn new() -> Self {
        Default::default()
    }
}

/// The default configuration for the client.
///
/// It is optimized for faster chunk put and get, benefiting from the chunk content addressed property.
/// Other data types are optimized for fast verification, and resilience in case of forks, which are impossible for chunks.
impl Default for ClientOperatingStrategy {
    fn default() -> Self {
        let two = NonZero::new(2).expect("2 is non 0");
        Self {
            chunks: Strategy {
                put_quorum: Quorum::N(two),
                put_retry: RetryStrategy::Balanced,
                verification_quorum: Quorum::N(two),
                get_quorum: Quorum::One, // chunks are content addressed so one is enough as there is no fork possible
                get_retry: RetryStrategy::Quick,
            },
            graph_entry: Strategy {
                put_quorum: Quorum::Majority,
                put_retry: RetryStrategy::Balanced,
                verification_quorum: Quorum::N(two),
                get_quorum: Quorum::N(two), // forks are rare but possible, balance between resilience and speed
                get_retry: RetryStrategy::Quick,
            },
            pointer: Strategy {
                put_quorum: Quorum::Majority,
                put_retry: RetryStrategy::Balanced,
                verification_quorum: Quorum::N(two),
                get_quorum: Quorum::Majority, // majority to catch possible differences in versions
                get_retry: RetryStrategy::Quick,
            },
            scratchpad: Strategy {
                put_quorum: Quorum::Majority,
                put_retry: RetryStrategy::Balanced,
                verification_quorum: Quorum::N(two),
                get_quorum: Quorum::Majority, // majority to catch possible differences in versions
                get_retry: RetryStrategy::Quick,
            },
            chunk_cache_enabled: true,
            chunk_cache_dir: None,
        }
    }
}
