// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::networking::{Quorum, RetryStrategy, Strategy};
pub use ant_bootstrap::{
    Bootstrap, BootstrapConfig, InitialPeersConfig, error::Error as BootstrapError,
};
use ant_evm::EvmNetwork;
use evmlib::contract::payment_vault::MAX_TRANSFERS_PER_TRANSACTION;
use std::{num::NonZero, sync::LazyLock};

/// Number of chunks to upload in parallel.
///
/// Can be overridden by the `CHUNK_UPLOAD_BATCH_SIZE` environment variable.
pub(crate) static CHUNK_UPLOAD_BATCH_SIZE: LazyLock<usize> = LazyLock::new(|| {
    let batch_size = std::env::var("CHUNK_UPLOAD_BATCH_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    info!("Chunk upload batch size: {}", batch_size);
    batch_size
});

/// Number of chunks to download in parallel.
///
/// Can be overridden by the `CHUNK_DOWNLOAD_BATCH_SIZE` environment variable.
pub static CHUNK_DOWNLOAD_BATCH_SIZE: LazyLock<usize> = LazyLock::new(|| {
    let batch_size = std::env::var("CHUNK_DOWNLOAD_BATCH_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    info!("Chunk download batch size: {}", batch_size);
    batch_size
});

/// Maximum number of chunks that we allow to download from a datamap in memory.
/// This affects the maximum size of data downloaded with APIs such as [`crate::Client::data_get`]
///
/// Can be overridden by the `MAX_IN_MEMORY_DOWNLOAD_SIZE ` environment variable.
pub static MAX_IN_MEMORY_DOWNLOAD_SIZE: LazyLock<usize> = LazyLock::new(|| {
    let size = std::env::var("MAX_IN_MEMORY_DOWNLOAD_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(20);
    info!("Max in memory download size: {}", size);
    size
});

/// Number of files to upload in parallel.
///
/// Can be overridden by the `FILE_UPLOAD_BATCH_SIZE` environment variable.
pub static FILE_UPLOAD_BATCH_SIZE: LazyLock<usize> = LazyLock::new(|| {
    let batch_size = std::env::var("FILE_UPLOAD_BATCH_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    info!("File upload batch size: {}", batch_size);
    batch_size
});

/// Number of files to encrypt in parallel.
///
/// Can be overridden by the `FILE_ENCRYPT_BATCH_SIZE` environment variable.
pub static FILE_ENCRYPT_BATCH_SIZE: LazyLock<usize> = LazyLock::new(|| {
    let batch_size = std::env::var("FILE_ENCRYPT_BATCH_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1)
                * 8,
        );
    info!("File encryption batch size: {}", batch_size);
    batch_size
});

/// Maximum size of a file to be encrypted in memory.
///
/// Can be overridden by the [`IN_MEMORY_ENCRYPTION_MAX_SIZE`] environment variable.
/// The default is 100MB.
pub static IN_MEMORY_ENCRYPTION_MAX_SIZE: LazyLock<usize> = LazyLock::new(|| {
    let max_size = std::env::var("IN_MEMORY_ENCRYPTION_MAX_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(50_000_000);
    info!(
        "IN_MEMORY_ENCRYPTION_MAX_SIZE (from that threshold, the file will be encrypted in a stream): {}",
        max_size
    );
    max_size
});

/// Number of batch size of an entire quote-pay-upload flow to process.
/// Suggested to be multiples of `MAX_TRANSFERS_PER_TRANSACTION  / 3` (records-payouts-per-transaction).
///
/// Can be overridden by the `UPLOAD_FLOW_BATCH_SIZE` environment variable.
pub(crate) static UPLOAD_FLOW_BATCH_SIZE: LazyLock<usize> = LazyLock::new(|| {
    let batch_size = std::env::var("UPLOAD_FLOW_BATCH_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(MAX_TRANSFERS_PER_TRANSACTION / 3);
    info!("Upload flow batch size: {}", batch_size);
    batch_size
});

/// Configuration for the [`crate::Client`] which can be provided through: [`crate::Client::init_with_config`].
#[derive(Debug, Clone, Default)]
pub struct ClientConfig {
    /// Configuration for bootstrapping into the network and caching peers.
    pub bootstrap_config: BootstrapConfig,

    /// EVM network to use for quotations and payments.
    pub evm_network: EvmNetwork,

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
