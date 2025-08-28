// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Graph entry operations for the Autonomi client.
//! This module provides graph entry upload, download, and cost estimation.
//! All operations delegate to autonomi_core::Client through the wrapper.

use crate::client::{
    Client, GetError, PutError,
    payment::{PayError, PaymentOption},
    quote::CostError,
};

use ant_evm::{AttoTokens, EvmWalletError};
use ant_protocol::{
    NetworkAddress,
    storage::{DataTypes, RecordKind},
};
use bls::PublicKey;

pub use crate::SecretKey;
pub use ant_protocol::storage::{GraphContent, GraphEntry, GraphEntryAddress};
use autonomi_core::DataContent;

#[derive(Debug, thiserror::Error)]
pub enum GraphError {
    #[error("Failed to put graph entry: {0}")]
    PutError(#[from] PutError),
    #[error("Cost error: {0}")]
    Cost(#[from] CostError),
    #[error(transparent)]
    GetError(#[from] GetError),
    #[error("Serialization error {0}")]
    Serialization(String),
    #[error("Verification failed (corrupt)")]
    FailedVerification,
    #[error("Payment failure occurred during creation.")]
    Pay(#[from] PayError),
    #[error("Failed to retrieve wallet payment")]
    Wallet(#[from] EvmWalletError),
    #[error(
        "Received invalid quote from node, this node is possibly malfunctioning, try another node by trying another transaction name"
    )]
    InvalidQuote,
    #[error("Entry already exists at this address: {0:?}")]
    AlreadyExists(GraphEntryAddress),
    #[error("Graph forked! Multiple entries found: {0:?}")]
    Fork(Vec<GraphEntry>),
}

impl Client {
    /// Get a graph entry from the network.
    pub async fn graph_entry_get(
        &self,
        address: &GraphEntryAddress,
    ) -> Result<GraphEntry, GraphError> {
        let network_addr = NetworkAddress::from(*address);
        match self.core_client.record_get(&network_addr).await {
            Ok(content) => match content {
                DataContent::GraphEntry(entry) => Ok(entry),
                _ => Err(GraphError::GetError(GetError::RecordKindMismatch(
                    RecordKind::DataOnly(DataTypes::GraphEntry),
                ))),
            },
            Err(e) => Err(GraphError::GetError(GetError::from_error(&e))),
        }
    }

    /// Check if a graph_entry exists on the network
    /// This method is much faster than [`Client::graph_entry_get`]
    /// This may fail if called immediately after creating the graph_entry,
    /// as nodes sometimes take longer to store the graph_entry than this request takes to execute!
    pub async fn graph_entry_check_existence(
        &self,
        address: &GraphEntryAddress,
    ) -> Result<bool, GraphError> {
        let network_addr = ant_protocol::NetworkAddress::from(*address);
        Ok(self
            .core_client
            .record_check_existence(&network_addr)
            .await
            .map_err(|e| GetError::from_error(&e))?)
    }

    /// Create and put a graph entry to the network.
    pub async fn graph_entry_put(
        &self,
        graph_entry: GraphEntry,
        payment_option: PaymentOption,
    ) -> Result<(AttoTokens, GraphEntryAddress), GraphError> {
        let graph_addr = graph_entry.address();

        if self.graph_entry_check_existence(&graph_addr).await? {
            error!("GraphEntry at address: {graph_addr:?} was already paid for");
            return Err(GraphError::AlreadyExists(graph_addr));
        }

        let data_content = DataContent::GraphEntry(graph_entry);

        Ok(self
            .core_client
            .record_put(data_content, Some(payment_option))
            .await
            .map(|(cost, _addr)| (cost, graph_addr))
            .map_err(|e| PutError::from_error(&e))?)
    }

    /// Get the cost for storing a graph entry.
    pub async fn graph_entry_cost(&self, key: &PublicKey) -> Result<AttoTokens, CostError> {
        // Create a graph entry address from the public key
        let graph_entry_addr = GraphEntryAddress::new(*key);
        let network_addr = NetworkAddress::from(graph_entry_addr);

        self.core_client
            .get_cost_estimation(vec![(network_addr, 0)]) // 0 size means use max size
            .await
            .map_err(|e| CostError::from_error(&e))
    }
}
