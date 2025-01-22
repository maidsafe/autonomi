// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::client::payment::PayError;
use crate::client::quote::CostError;
use crate::client::Client;
use crate::client::ClientEvent;
use crate::client::UploadSummary;

use ant_evm::{Amount, AttoTokens, EvmWallet, EvmWalletError};
use ant_networking::{GetRecordCfg, NetworkError, PutRecordCfg, VerificationKind};
use ant_protocol::storage::GraphEntryAddress;
use ant_protocol::{
    storage::{try_serialize_record, DataTypes, RecordKind, RetryStrategy},
    NetworkAddress,
};
use libp2p::kad::{Quorum, Record};

pub use ant_protocol::storage::GraphEntry;
pub use bls::SecretKey;

#[derive(Debug, thiserror::Error)]
pub enum GraphError {
    #[error("Cost error: {0}")]
    Cost(#[from] CostError),
    #[error("Network error")]
    Network(#[from] NetworkError),
    #[error("Serialization error")]
    Serialization,
    #[error("Verification failed (corrupt)")]
    FailedVerification,
    #[error("Payment failure occurred during creation.")]
    Pay(#[from] PayError),
    #[error("Failed to retrieve wallet payment")]
    Wallet(#[from] EvmWalletError),
    #[error("Received invalid quote from node, this node is possibly malfunctioning, try another node by trying another transaction name")]
    InvalidQuote,
    #[error("Entry already exists at this address: {0:?}")]
    AlreadyExists(GraphEntryAddress),
}

impl Client {
    /// Fetches a GraphEntry from the network.
    pub async fn graph_entry_get(
        &self,
        address: GraphEntryAddress,
    ) -> Result<Vec<GraphEntry>, GraphError> {
        let graph_entries = self.network.get_graph_entry(address).await?;

        Ok(graph_entries)
    }

    /// Puts a GraphEntry to the network.
    pub async fn graph_entry_put(
        &self,
        entry: GraphEntry,
        wallet: &EvmWallet,
    ) -> Result<(), GraphError> {
        let address = entry.address();

        // pay for the graph entry
        let xor_name = address.xorname();
        debug!("Paying for graph entry at address: {address:?}");
        let (payment_proofs, skipped_payments) = self
            .pay(
                DataTypes::GraphEntry.get_index(),
                std::iter::once((*xor_name, entry.bytes_for_signature().len())),
                wallet,
            )
            .await
            .inspect_err(|err| {
                error!("Failed to pay for graph entry at address: {address:?} : {err}")
            })?;

        // make sure the graph entry was paid for
        let (proof, price) = match payment_proofs.get(xor_name) {
            Some((proof, price)) => (proof, price),
            None => {
                // graph entry was skipped, meaning it was already paid for
                error!("GraphEntry at address: {address:?} was already paid for");
                return Err(GraphError::AlreadyExists(address));
            }
        };

        // prepare the record for network storage
        let payees = proof.payees();
        let record = Record {
            key: NetworkAddress::from_graph_entry_address(address).to_record_key(),
            value: try_serialize_record(
                &(proof, &entry),
                RecordKind::DataWithPayment(DataTypes::GraphEntry),
            )
            .map_err(|_| GraphError::Serialization)?
            .to_vec(),
            publisher: None,
            expires: None,
        };
        let get_cfg = GetRecordCfg {
            get_quorum: Quorum::Majority,
            retry_strategy: Some(RetryStrategy::default()),
            target_record: None,
            expected_holders: Default::default(),
        };
        let put_cfg = PutRecordCfg {
            put_quorum: Quorum::All,
            retry_strategy: None,
            use_put_record_to: Some(payees),
            verification: Some((VerificationKind::Crdt, get_cfg)),
        };

        // put the record to the network
        debug!("Storing GraphEntry at address {address:?} to the network");
        self.network
            .put_record(record, &put_cfg)
            .await
            .inspect_err(|err| {
                error!("Failed to put record - GraphEntry {address:?} to the network: {err}")
            })?;

        // send client event
        if let Some(channel) = self.client_event_sender.as_ref() {
            let summary = UploadSummary {
                records_paid: 1usize.saturating_sub(skipped_payments),
                records_already_paid: skipped_payments,
                tokens_spent: price.as_atto(),
            };
            if let Err(err) = channel.send(ClientEvent::UploadComplete(summary)).await {
                error!("Failed to send client event: {err}");
            }
        }

        Ok(())
    }

    /// Get the cost to create a GraphEntry
    pub async fn graph_entry_cost(&self, key: SecretKey) -> Result<AttoTokens, GraphError> {
        let pk = key.public_key();
        trace!("Getting cost for GraphEntry of {pk:?}");

        let address = GraphEntryAddress::from_owner(pk);
        let xor = *address.xorname();
        // TODO: define default size of GraphEntry
        let store_quote = self
            .get_store_quotes(
                DataTypes::GraphEntry.get_index(),
                std::iter::once((xor, 512)),
            )
            .await?;
        let total_cost = AttoTokens::from_atto(
            store_quote
                .0
                .values()
                .map(|quote| quote.price())
                .sum::<Amount>(),
        );
        debug!("Calculated the cost to create GraphEntry of {pk:?} is {total_cost}");
        Ok(total_cost)
    }
}
