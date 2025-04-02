// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_evm::EvmNetwork;
use ant_networking::{GetRecordCfg, PutRecordCfg, VerificationKind};
use ant_protocol::messages::ChunkProof;
use libp2p::{kad::Record, PeerId};
use rand::{thread_rng, Rng};
use std::{collections::HashSet, num::NonZero};

pub use ant_bootstrap::{error::Error as BootstrapError, InitialPeersConfig};
pub use ant_networking::{ResponseQuorum, RetryStrategy};

/// Configuration for the [`crate::Client`] which can be provided through: [`crate::Client::init_with_config`].
#[derive(Debug, Clone, Default)]
pub struct ClientConfig {
    /// Configurations to fetch the initial peers which is used to bootstrap the network.
    /// Also contains the configurations to the bootstrap cache.
    pub init_peers_config: InitialPeersConfig,

    /// EVM network to use for quotations and payments.
    pub evm_network: EvmNetwork,

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
                put_quorum: ResponseQuorum::N(two),
                put_retry: RetryStrategy::Balanced,
                verification_quorum: ResponseQuorum::N(two),
                verification_retry: RetryStrategy::Balanced,
                get_quorum: ResponseQuorum::One, // chunks are content addressed so one is enough as there is no fork possible
                get_retry: RetryStrategy::Quick,
                verification_kind: VerificationKind::Network, // it is recommended to use [`Strategy::chunk_put_cfg`] for chunks to benefit from the chunk proof
            },
            graph_entry: Strategy {
                put_quorum: ResponseQuorum::Majority,
                put_retry: RetryStrategy::Balanced,
                verification_quorum: ResponseQuorum::Majority,
                verification_retry: RetryStrategy::Quick, // verification should be quick
                get_quorum: ResponseQuorum::N(two), // forks are rare but possible, balance between resilience and speed
                get_retry: RetryStrategy::Quick,
                verification_kind: VerificationKind::Crdt, // forks are possible
            },
            pointer: Strategy {
                put_quorum: ResponseQuorum::Majority,
                put_retry: RetryStrategy::Balanced,
                verification_quorum: ResponseQuorum::Majority,
                verification_retry: RetryStrategy::Quick, // verification should be quick
                get_quorum: ResponseQuorum::Majority, // majority to catch possible differences in versions
                get_retry: RetryStrategy::Quick,
                verification_kind: VerificationKind::Crdt, // forks are possible
            },
            scratchpad: Strategy {
                put_quorum: ResponseQuorum::Majority,
                put_retry: RetryStrategy::Balanced,
                verification_quorum: ResponseQuorum::Majority,
                verification_retry: RetryStrategy::Quick, // verification should be quick
                get_quorum: ResponseQuorum::Majority, // majority to catch possible differences in versions
                get_retry: RetryStrategy::Quick,
                verification_kind: VerificationKind::Crdt, // forks are possible
            },
        }
    }
}

/// The strategy to adopt when puting and getting data from the network
///
/// Puts are followed by a verification using get, to ensure the data is stored correctly. This verification can be configured separately from the regular gets.
#[derive(Debug, Clone)]
pub struct Strategy {
    /// The number of responses to wait for before considering the put operation successful
    pub put_quorum: ResponseQuorum,
    /// The retry strategy to use if we fail to store a piece of data
    pub put_retry: RetryStrategy,
    /// The number of responses to wait for before considering the verification to be successful
    pub verification_quorum: ResponseQuorum,
    /// The retry strategy for verification
    pub verification_retry: RetryStrategy,
    /// The number of responses to wait for before considering the get operation successful
    pub get_quorum: ResponseQuorum,
    /// The retry strategy to use if the get operation fails
    pub get_retry: RetryStrategy,
    /// Verification kind
    pub(crate) verification_kind: VerificationKind,
}

impl Strategy {
    /// Get config for getting a record
    pub(crate) fn get_cfg(&self) -> GetRecordCfg {
        GetRecordCfg {
            get_quorum: self.get_quorum,
            retry_strategy: self.get_retry,
            target_record: None,
            expected_holders: HashSet::new(),
        }
    }

    /// Get config for verifying the existance of a record
    pub(crate) fn verification_cfg(&self) -> GetRecordCfg {
        GetRecordCfg {
            get_quorum: self.verification_quorum,
            retry_strategy: self.verification_retry,
            target_record: None,
            expected_holders: HashSet::new(),
        }
    }

    /// Put config for storing a record
    pub(crate) fn put_cfg(&self, put_to: Option<Vec<PeerId>>) -> PutRecordCfg {
        PutRecordCfg {
            put_quorum: self.put_quorum,
            retry_strategy: self.put_retry,
            use_put_record_to: put_to,
            verification: Some((self.verification_kind.clone(), self.verification_cfg())),
        }
    }

    /// Put config for storing a Chunk, more strict and requires a chunk proof of storage
    pub(crate) fn chunk_put_cfg(&self, expected: Record, put_to: Vec<PeerId>) -> PutRecordCfg {
        let random_nonce = thread_rng().gen::<u64>();
        let expected_proof = ChunkProof::new(&expected.value, random_nonce);

        PutRecordCfg {
            put_quorum: self.put_quorum,
            retry_strategy: self.put_retry,
            use_put_record_to: Some(put_to),
            verification: Some((
                VerificationKind::ChunkProof {
                    expected_proof,
                    nonce: random_nonce,
                },
                self.verification_cfg_specific(expected),
            )),
        }
    }

    /// Put config for storing a record and making sure it matches the expected record
    pub(crate) fn put_cfg_specific(
        &self,
        put_to: Option<Vec<PeerId>>,
        expected: Record,
    ) -> PutRecordCfg {
        PutRecordCfg {
            put_quorum: self.put_quorum,
            retry_strategy: self.put_retry,
            use_put_record_to: put_to,
            verification: Some((
                self.verification_kind.clone(),
                self.verification_cfg_specific(expected),
            )),
        }
    }

    /// Get config for verifying the existance and value of a record
    pub(crate) fn verification_cfg_specific(&self, expected: Record) -> GetRecordCfg {
        GetRecordCfg {
            get_quorum: self.verification_quorum,
            retry_strategy: self.verification_retry,
            target_record: Some(expected),
            expected_holders: HashSet::new(),
        }
    }
}
