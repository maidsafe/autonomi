// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_evm::EvmNetwork;
use ant_networking::{ResponseQuorum, RetryStrategy};
use libp2p::Multiaddr;
use std::num::NonZero;

/// Configuration for [`crate::Client::init_with_config`].
///
/// Use [`ClientConfig::set_client_operation_config`] to set configure how the client performs operations
/// on the network.
#[derive(Debug, Clone, Default)]
pub struct ClientConfig {
    /// Whether we're expected to connect to a local network.
    pub local: bool,

    /// List of peers to connect to.
    ///
    /// If not provided, the client will use the default bootstrap peers.
    pub peers: Option<Vec<Multiaddr>>,

    /// EVM network to use for quotations and payments.
    pub evm_network: EvmNetwork,

    /// Configuration for operations on the client.
    ///
    /// This will be shared across all clones of the client and cannot be changed after initialization.
    pub operation_config: ClientOperationConfig,
}

/// Configurations for operations on the client.
///
/// Default values are used for each type of data, but you can override them here.
#[derive(Debug, Clone, Default)]
pub struct ClientOperationConfig {
    /// Configuration for chunk operations.
    pub(crate) chunk_operation_config: ChunkOperationConfig,
    /// Configuration for graph operations.
    pub(crate) graph_operation_config: GraphOperationConfig,
    /// Configuration for pointer operations.
    pub(crate) pointer_operation_config: PointerOperationConfig,
    /// Configuration for scratchpad operations.
    pub(crate) scratchpad_operation_config: ScratchpadOperationConfig,
}

#[derive(Debug, Clone)]
pub struct ChunkOperationConfig {
    /// The retry strategy to use if we fail to store a chunk. Every write will also verify that the chunk
    /// is stored on the network by fetching it back.
    ///
    /// Use the `verification_quorum` and `verification_retry_strategy` to configure the verification operation.
    pub(crate) write_retry_strategy: RetryStrategy,
    /// The number of chunks to wait for before considering the read after write operation successful.
    pub(crate) verification_quorum: ResponseQuorum,
    /// The retry strategy to use if the read after write operation fails.
    pub(crate) verification_retry_strategy: RetryStrategy,
    /// The number of chunks to wait for before considering the read operation successful.
    pub(crate) read_quorum: ResponseQuorum,
    /// The retry strategy to use if the read operation fails.
    pub(crate) read_retry_strategy: RetryStrategy,
}

#[derive(Debug, Clone)]
pub struct GraphOperationConfig {
    /// The retry strategy to use if we fail to store a graph entry. Every write will also verify that the entry
    /// is stored on the network by fetching it back.
    ///
    /// Use the `verification_quorum` and `verification_retry_strategy` to configure the verification operation.
    pub(crate) write_retry_strategy: RetryStrategy,
    /// The number of entries to wait for before considering the read after write operation successful.
    pub(crate) verification_quorum: ResponseQuorum,
    /// The retry strategy to use if the read after write operation fails.
    pub(crate) verification_retry_strategy: RetryStrategy,
    /// The number of entries to wait for before considering the read operation successful.
    pub(crate) read_quorum: ResponseQuorum,
    /// The retry strategy to use if the read operation fails.
    pub(crate) read_retry_strategy: RetryStrategy,
}

#[derive(Debug, Clone)]
pub struct PointerOperationConfig {
    /// The retry strategy to use if we fail to store a pointer. Every write will also verify that the pointer
    /// is stored on the network by fetching it back.
    ///
    /// Use the `verification_quorum` and `verification_retry_strategy` to configure the verification operation.
    pub(crate) write_retry_strategy: RetryStrategy,
    /// The number of pointers to wait for before considering the read after write operation successful.
    pub(crate) verification_quorum: ResponseQuorum,
    /// The retry strategy to use if the read after write operation fails.
    pub(crate) verification_retry_strategy: RetryStrategy,
    /// The number of pointers to wait for before considering the read operation successful.
    pub(crate) read_quorum: ResponseQuorum,
    /// The retry strategy to use if the read operation fails.
    pub(crate) read_retry_strategy: RetryStrategy,
}

#[derive(Debug, Clone)]
pub struct ScratchpadOperationConfig {
    /// The retry strategy to use if we fail to store a scratchpad. Every write will also verify that the scratchpad
    /// is stored on the network by fetching it back.
    ///
    /// Use the `verification_quorum` and `verification_retry_strategy` to configure the verification operation.
    pub(crate) write_retry_strategy: RetryStrategy,
    /// The number of scratchpads to wait for before considering the read after write operation successful.
    pub(crate) verification_quorum: ResponseQuorum,
    /// The retry strategy to use if the read after write operation fails.
    pub(crate) verification_retry_strategy: RetryStrategy,
    /// The number of scratchpads to wait for before considering the read operation successful.
    pub(crate) read_quorum: ResponseQuorum,
    /// The retry strategy to use if the read operation fails.
    pub(crate) read_retry_strategy: RetryStrategy,
}

impl Default for ChunkOperationConfig {
    fn default() -> Self {
        Self {
            write_retry_strategy: RetryStrategy::Balanced,
            verification_quorum: ResponseQuorum::N(NonZero::new(2).expect("2 is non-zero")),
            verification_retry_strategy: RetryStrategy::Balanced,
            read_quorum: ResponseQuorum::One,
            read_retry_strategy: RetryStrategy::Balanced,
        }
    }
}

impl Default for GraphOperationConfig {
    fn default() -> Self {
        Self {
            write_retry_strategy: RetryStrategy::Quick,
            verification_quorum: ResponseQuorum::Majority,
            verification_retry_strategy: RetryStrategy::Balanced,
            read_quorum: ResponseQuorum::All,
            read_retry_strategy: RetryStrategy::Quick,
        }
    }
}

impl Default for PointerOperationConfig {
    fn default() -> Self {
        Self {
            write_retry_strategy: RetryStrategy::Quick,
            verification_quorum: ResponseQuorum::Majority,
            verification_retry_strategy: RetryStrategy::Balanced,
            read_quorum: ResponseQuorum::Majority,
            read_retry_strategy: RetryStrategy::Quick,
        }
    }
}

impl Default for ScratchpadOperationConfig {
    fn default() -> Self {
        Self {
            write_retry_strategy: RetryStrategy::None,
            verification_quorum: ResponseQuorum::Majority,
            verification_retry_strategy: RetryStrategy::Quick,
            read_quorum: ResponseQuorum::Majority,
            read_retry_strategy: RetryStrategy::Quick,
        }
    }
}

impl ClientConfig {
    pub fn local(peers: Option<Vec<Multiaddr>>) -> Self {
        Self {
            local: true,
            peers,
            evm_network: EvmNetwork::new(true).unwrap_or_default(),
            operation_config: Default::default(),
        }
    }

    pub fn set_client_operation_config(&mut self, operation_config: ClientOperationConfig) {
        self.operation_config = operation_config;
    }
}

impl ClientOperationConfig {
    /// Set the retry strategy forthe  chunk write operations. Every write will also verify that the chunk
    /// is stored on the network by fetching it back.
    ///
    /// Use the `chunk_verification_quorum` and `chunk_verification_retry_strategy` to configure the verification
    /// operation.
    pub fn chunk_write_retry_strategy(&mut self, strategy: RetryStrategy) {
        self.chunk_operation_config.write_retry_strategy = strategy;
    }

    /// Set the quorum for the chunk verification operations. This is the number of chunks to wait for before
    /// considering the read after write operation successful.
    pub fn chunk_verification_quorum(&mut self, quorum: ResponseQuorum) {
        self.chunk_operation_config.verification_quorum = quorum;
    }

    /// Set the retry strategy for the chunk verification operation. This is the retry strategy to use if the read
    /// after write operation fails.
    pub fn chunk_verification_retry_strategy(&mut self, strategy: RetryStrategy) {
        self.chunk_operation_config.verification_retry_strategy = strategy;
    }

    /// Set the quorum for the chunk read operations. This is the number of chunks to wait for before considering
    /// the read operation successful.
    pub fn chunk_read_quorum(&mut self, quorum: ResponseQuorum) {
        self.chunk_operation_config.read_quorum = quorum;
    }

    /// Set the retry strategy for the chunk read operation. This is the retry strategy to use if the read
    /// operation fails.
    pub fn chunk_read_retry_strategy(&mut self, strategy: RetryStrategy) {
        self.chunk_operation_config.read_retry_strategy = strategy;
    }

    /// Set the retry strategy for the graph write operations. Every write will also verify that the entry
    /// is stored on the network by fetching it back.
    ///
    /// Use the `graph_verification_quorum` and `graph_verification_retry_strategy` to configure the verification
    /// operation.
    pub fn graph_write_retry_strategy(&mut self, strategy: RetryStrategy) {
        self.graph_operation_config.write_retry_strategy = strategy;
    }

    /// Set the quorum for the graph verification operations. This is the number of entries to wait for before
    /// considering the read after write operation successful.
    pub fn graph_verification_quorum(&mut self, quorum: ResponseQuorum) {
        self.graph_operation_config.verification_quorum = quorum;
    }

    /// Set the retry strategy for the graph verification operation. This is the retry strategy to use if the read
    /// after write operation fails.
    pub fn graph_verification_retry_strategy(&mut self, strategy: RetryStrategy) {
        self.graph_operation_config.verification_retry_strategy = strategy;
    }

    /// Set the quorum for the graph read operations. This is the number of entries to wait for before considering
    /// the read operation successful.
    pub fn graph_read_quorum(&mut self, quorum: ResponseQuorum) {
        self.graph_operation_config.read_quorum = quorum;
    }

    /// Set the retry strategy for the graph read operation. This is the retry strategy to use if the read
    /// operation fails.
    pub fn graph_read_retry_strategy(&mut self, strategy: RetryStrategy) {
        self.graph_operation_config.read_retry_strategy = strategy;
    }

    /// Set the retry strategy for the pointer write operations. Every write will also verify that the pointer
    /// is stored on the network by fetching it back.
    ///
    /// Use the `pointer_verification_quorum` and `pointer_verification_retry_strategy` to configure the verification
    /// operation.
    pub fn pointer_write_retry_strategy(&mut self, strategy: RetryStrategy) {
        self.pointer_operation_config.write_retry_strategy = strategy;
    }

    /// Set the quorum for the pointer verification operations. This is the number of pointers to wait for before
    /// considering the read after write operation successful.
    pub fn pointer_verification_quorum(&mut self, quorum: ResponseQuorum) {
        self.pointer_operation_config.verification_quorum = quorum;
    }

    /// Set the retry strategy for the pointer verification operation. This is the retry strategy to use if the read
    /// after write operation fails.
    pub fn pointer_verification_retry_strategy(&mut self, strategy: RetryStrategy) {
        self.pointer_operation_config.verification_retry_strategy = strategy;
    }

    /// Set the quorum for the pointer read operations. This is the number of pointers to wait for before considering
    /// the read operation successful.
    pub fn pointer_read_quorum(&mut self, quorum: ResponseQuorum) {
        self.pointer_operation_config.read_quorum = quorum;
    }

    /// Set the retry strategy for the pointer read operation. This is the retry strategy to use if the read
    /// operation fails.
    pub fn pointer_read_retry_strategy(&mut self, strategy: RetryStrategy) {
        self.pointer_operation_config.read_retry_strategy = strategy;
    }

    /// Set the retry strategy for the scratchpad write operations. Every write will also verify that the scratchpad
    /// is stored on the network by fetching it back.
    ///
    /// Use the `scratchpad_verification_quorum` and `scratchpad_verification_retry_strategy` to configure the
    /// verification operation.
    pub fn scratchpad_write_retry_strategy(&mut self, strategy: RetryStrategy) {
        self.scratchpad_operation_config.write_retry_strategy = strategy;
    }

    /// Set the quorum for the scratchpad verification operations. This is the number of scratchpads to wait for before
    /// considering the read after write operation successful.
    pub fn scratchpad_verification_quorum(&mut self, quorum: ResponseQuorum) {
        self.scratchpad_operation_config.verification_quorum = quorum;
    }

    /// Set the retry strategy for the scratchpad verification operation. This is the retry strategy to use if the read
    /// after write operation fails.
    pub fn scratchpad_verification_retry_strategy(&mut self, strategy: RetryStrategy) {
        self.scratchpad_operation_config.verification_retry_strategy = strategy;
    }

    /// Set the quorum for the scratchpad read operations. This is the number of scratchpads to wait for before
    /// considering the read operation successful.
    pub fn scratchpad_read_quorum(&mut self, quorum: ResponseQuorum) {
        self.scratchpad_operation_config.read_quorum = quorum;
    }

    /// Set the retry strategy for the scratchpad read operation. This is the retry strategy to use if the read
    /// operation fails.
    pub fn scratchpad_read_retry_strategy(&mut self, strategy: RetryStrategy) {
        self.scratchpad_operation_config.read_retry_strategy = strategy;
    }
}
