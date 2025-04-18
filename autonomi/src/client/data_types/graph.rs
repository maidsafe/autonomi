// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::client::payment::PayError;
use crate::client::payment::PaymentOption;
use crate::client::quote::CostError;
use crate::client::ClientEvent;
use crate::client::UploadSummary;
use crate::client::{Client, GetError};

use ant_evm::{Amount, AttoTokens, EvmWalletError};
use ant_protocol::storage::try_deserialize_record;
use ant_protocol::storage::RecordHeader;
use ant_protocol::PrettyPrintRecordKey;
use ant_protocol::{
    storage::{try_serialize_record, DataTypes, RecordKind},
    NetworkAddress,
};
use bls::PublicKey;
use libp2p::kad::Record;

use crate::networking::NetworkError;
pub use crate::SecretKey;
pub use ant_protocol::storage::{GraphContent, GraphEntry, GraphEntryAddress};

#[derive(Debug, thiserror::Error)]
pub enum GraphError {
    #[error("Cost error: {0}")]
    Cost(#[from] CostError),
    #[error("Network error")]
    Network(#[from] NetworkError),
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
    #[error("Received invalid quote from node, this node is possibly malfunctioning, try another node by trying another transaction name")]
    InvalidQuote,
    #[error("Entry already exists at this address: {0:?}")]
    AlreadyExists(GraphEntryAddress),
    #[error("Graph forked! Multiple entries found: {0:?}")]
    Fork(Vec<GraphEntry>),
}

impl Client {
    /// Fetches a GraphEntry from the network.
    pub async fn graph_entry_get(
        &self,
        address: &GraphEntryAddress,
    ) -> Result<GraphEntry, GraphError> {
        let key = NetworkAddress::from(*address);

        let record = self
            .network
            .get_record_with_retries(key.clone(), &self.config.graph_entry)
            .await?
            .ok_or(GetError::RecordNotFound)?;

        debug!(
            "Got record from the network, {:?}",
            PrettyPrintRecordKey::from(&record.key)
        );

        let graph_entries = get_graph_entry_from_record(&record)?;
        match &graph_entries[..] {
            [entry] => Ok(entry.clone()),
            multiple => Err(GraphError::Fork(multiple.to_vec())),
        }
    }

    /// Check if a graph_entry exists on the network
    /// This method is much faster than [`Client::graph_entry_get`]
    /// This may fail if called immediately after creating the graph_entry, as nodes sometimes take longer to store the graph_entry than this request takes to execute!
    pub async fn graph_entry_check_existence(
        &self,
        address: &GraphEntryAddress,
    ) -> Result<bool, GraphError> {
        let key = NetworkAddress::from(*address);
        debug!("Checking graph_entry existence at: {key:?}");

        match self
            .network
            .get_record(key.clone(), self.config.graph_entry.verification_quorum)
            .await
        {
            Ok(None) => Ok(false),
            Ok(Some(_)) => Ok(true),
            Err(NetworkError::SplitRecord(..)) => Ok(true),
            Err(err) => Err(GraphError::Network(err))
                .inspect_err(|err| error!("Error checking graph_entry existence: {err:?}")),
        }
    }

    /// Manually puts a GraphEntry to the network.
    pub async fn graph_entry_put(
        &self,
        entry: GraphEntry,
        payment_option: PaymentOption,
    ) -> Result<(AttoTokens, GraphEntryAddress), GraphError> {
        let address = entry.address();

        // pay for the graph entry
        let xor_name = address.xorname();
        debug!("Paying for graph entry at address: {address:?}");
        let (payment_proofs, skipped_payments) = self
            .pay_for_content_addrs(
                DataTypes::GraphEntry,
                std::iter::once((xor_name, entry.size())),
                payment_option,
            )
            .await
            .inspect_err(|err| {
                error!("Failed to pay for graph entry at address: {address:?} : {err}")
            })?;

        // make sure the graph entry was paid for
        let (proof, price) = match payment_proofs.get(&xor_name) {
            Some((proof, price)) => (proof, price),
            None => {
                // graph entry was skipped, meaning it was already paid for
                error!("GraphEntry at address: {address:?} was already paid for");
                return Err(GraphError::AlreadyExists(address));
            }
        };
        let total_cost = *price;

        // prepare the record for network storage
        let payees = proof.payees();
        let record = Record {
            key: NetworkAddress::from(address).to_record_key(),
            value: try_serialize_record(
                &(proof, &entry),
                RecordKind::DataWithPayment(DataTypes::GraphEntry),
            )
            .map_err(|err| GraphError::Serialization(err.to_string()))?
            .to_vec(),
            publisher: None,
            expires: None,
        };

        // put the record to the network
        debug!("Storing GraphEntry at address {address:?} to the network");
        self.network
            .put_record_with_retries(record, payees, &self.config.graph_entry)
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

        Ok((total_cost, address))
    }

    /// Get the cost to create a GraphEntry
    pub async fn graph_entry_cost(&self, key: &PublicKey) -> Result<AttoTokens, CostError> {
        trace!("Getting cost for GraphEntry of {key:?}");
        let address = GraphEntryAddress::new(*key);
        let xor = address.xorname();
        let store_quote = self
            .get_store_quotes(
                DataTypes::GraphEntry,
                std::iter::once((xor, GraphEntry::MAX_SIZE)),
            )
            .await?;
        let total_cost = AttoTokens::from_atto(
            store_quote
                .0
                .values()
                .map(|quote| quote.price())
                .sum::<Amount>(),
        );
        debug!("Calculated the cost to create GraphEntry of {key:?} is {total_cost}");
        Ok(total_cost)
    }
}

pub fn get_graph_entry_from_record(record: &Record) -> Result<Vec<GraphEntry>, GraphError> {
    let header = RecordHeader::from_record(record).map_err(|_| {
        GraphError::Serialization(format!(
            "Failed to deserialize record header {:?}",
            PrettyPrintRecordKey::from(&record.key)
        ))
    })?;
    if let RecordKind::DataOnly(DataTypes::GraphEntry) = header.kind {
        let entry = try_deserialize_record::<Vec<GraphEntry>>(record).map_err(|_| {
            GraphError::Serialization(format!(
                "Failed to deserialize record value {:?}",
                PrettyPrintRecordKey::from(&record.key)
            ))
        })?;
        Ok(entry)
    } else {
        warn!(
            "RecordKind mismatch while trying to retrieve graph_entry from record {:?}",
            PrettyPrintRecordKey::from(&record.key)
        );
        Err(GraphError::Serialization(format!(
            "RecordKind mismatch while trying to retrieve graph_entry from record {:?}",
            PrettyPrintRecordKey::from(&record.key)
        )))
    }
}
