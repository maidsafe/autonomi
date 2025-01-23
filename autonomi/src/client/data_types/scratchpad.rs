// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::client::payment::PaymentOption;
use crate::client::PutError;
use crate::{client::quote::CostError, Client};
use ant_evm::{Amount, AttoTokens};
use ant_networking::{
    GetRecordCfg, GetRecordError, NetworkError, PutRecordCfg, ResponseQuorum, VerificationKind,
};
use ant_protocol::storage::{try_serialize_record, RecordKind};
use ant_protocol::{
    storage::{try_deserialize_record, DataTypes, ScratchpadAddress},
    NetworkAddress,
};
use libp2p::kad::Record;
use std::collections::HashSet;

pub use ant_protocol::storage::Scratchpad;
pub use bls::{PublicKey, SecretKey};

#[derive(Debug, thiserror::Error)]
pub enum ScratchpadError {
    #[error("Scratchpad found at {0:?} was not a valid record.")]
    CouldNotDeserializeScratchPad(ScratchpadAddress),
    #[error("Network: {0}")]
    Network(#[from] NetworkError),
    #[error("Scratchpad not found")]
    Missing,
}

impl Client {
    /// Get Scratchpad from the Network
    /// It is stored at the owner's public key
    pub async fn scratchpad_get(
        &self,
        public_key: &PublicKey,
    ) -> Result<Scratchpad, ScratchpadError> {
        let scratch_address = ScratchpadAddress::new(*public_key);
        let network_address = NetworkAddress::from_scratchpad_address(scratch_address);
        info!("Fetching scratchpad from network at {network_address:?}",);
        let scratch_key = network_address.to_record_key();

        let get_cfg = GetRecordCfg {
            get_quorum: self
                .operation_config
                .scratchpad_operation_config
                .read_quorum,
            retry_strategy: self
                .operation_config
                .scratchpad_operation_config
                .read_retry_strategy,
            target_record: None,
            expected_holders: HashSet::new(),
        };

        let pad = match self
            .network
            .get_record_from_network(scratch_key.clone(), &get_cfg)
            .await
        {
            Ok(record) => {
                debug!("Got scratchpad for {scratch_key:?}");
                try_deserialize_record::<Scratchpad>(&record)
                    .map_err(|_| ScratchpadError::CouldNotDeserializeScratchPad(scratch_address))?
            }
            Err(NetworkError::GetRecordError(GetRecordError::SplitRecord { result_map })) => {
                debug!("Got multiple scratchpads for {scratch_key:?}");
                let mut pads = result_map
                    .values()
                    .map(|(record, _)| try_deserialize_record::<Scratchpad>(record))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| ScratchpadError::CouldNotDeserializeScratchPad(scratch_address))?;

                // take the latest versions
                pads.sort_by_key(|s| s.count());
                let max_version = pads.last().map(|p| p.count()).unwrap_or_else(|| {
                    error!("Got empty scratchpad vector for {scratch_key:?}");
                    u64::MAX
                });
                let latest_pads: Vec<_> = pads
                    .into_iter()
                    .filter(|s| s.count() == max_version)
                    .collect();

                // make sure we only have one of latest version
                let pad = match &latest_pads[..] {
                    [one] => one,
                    [multi, ..] => {
                        error!("Got multiple conflicting scratchpads for {scratch_key:?} with the latest version, returning the first one");
                        multi
                    }
                    [] => {
                        error!("Got empty scratchpad vector for {scratch_key:?}");
                        return Err(ScratchpadError::Missing);
                    }
                };
                pad.to_owned()
            }
            Err(e) => {
                warn!("Failed to fetch scratchpad {network_address:?} from network: {e}");
                return Err(e)?;
            }
        };

        Ok(pad)
    }

    /// Returns the latest found version of the scratchpad for that secret key
    /// If none is found, it creates a new one locally
    /// Note that is does not upload that new scratchpad to the network, one would need to call [`Self::scratchpad_create`] to do so
    /// Returns the scratchpad along with a boolean indicating if that scratchpad is new or not
    pub async fn get_or_create_scratchpad(
        &self,
        public_key: &PublicKey,
        content_type: u64,
    ) -> Result<(Scratchpad, bool), PutError> {
        let pad_res = self.scratchpad_get(public_key).await;
        let mut is_new = true;

        let scratch = if let Ok(existing_data) = pad_res {
            info!("Scratchpad already exists, returning existing data");

            info!(
                "scratch already exists, is version {:?}",
                existing_data.count()
            );

            is_new = false;

            if existing_data.owner() != public_key {
                return Err(PutError::ScratchpadBadOwner);
            }

            existing_data
        } else {
            trace!("new scratchpad creation");
            Scratchpad::new(*public_key, content_type)
        };

        Ok((scratch, is_new))
    }

    /// Create a new scratchpad to the network
    pub async fn scratchpad_create(
        &self,
        scratchpad: Scratchpad,
        payment_option: PaymentOption,
    ) -> Result<AttoTokens, PutError> {
        let scratch_address = scratchpad.network_address();
        let scratch_key = scratch_address.to_record_key();

        // pay for the scratchpad
        let (receipt, _skipped_payments) = self
            .pay_for_content_addrs(
                DataTypes::Scratchpad.get_index(),
                std::iter::once((scratchpad.xorname(), scratchpad.payload_size())),
                payment_option,
            )
            .await
            .inspect_err(|err| {
                error!("Failed to pay for new scratchpad at addr: {scratch_address:?} : {err}");
            })?;

        let (proof, price) = match receipt.values().next() {
            Some(proof) => proof,
            None => return Err(PutError::PaymentUnexpectedlyInvalid(scratch_address)),
        };
        let total_cost = *price;

        let record = Record {
            key: scratch_key,
            value: try_serialize_record(
                &(proof, scratchpad),
                RecordKind::DataWithPayment(DataTypes::Scratchpad),
            )
            .map_err(|_| {
                PutError::Serialization("Failed to serialize scratchpad with payment".to_string())
            })?
            .to_vec(),
            publisher: None,
            expires: None,
        };

        let put_cfg = PutRecordCfg {
            put_quorum: ResponseQuorum::Majority,
            retry_strategy: self
                .operation_config
                .scratchpad_operation_config
                .write_retry_strategy,
            use_put_record_to: None,
            verification: Some((
                VerificationKind::Crdt,
                GetRecordCfg {
                    get_quorum: self
                        .operation_config
                        .scratchpad_operation_config
                        .verification_quorum,
                    retry_strategy: self
                        .operation_config
                        .scratchpad_operation_config
                        .verification_retry_strategy,
                    target_record: None,
                    expected_holders: HashSet::new(),
                },
            )),
        };

        debug!("Put record - scratchpad at {scratch_address:?} to the network");
        self.network
            .put_record(record, &put_cfg)
            .await
            .inspect_err(|err| {
                error!(
                    "Failed to put scratchpad {scratch_address:?} to the network with err: {err:?}"
                )
            })?;

        Ok(total_cost)
    }

    /// Update an existing scratchpad to the network
    /// This operation is free but requires the scratchpad to be already created on the network
    /// Only the latest version of the scratchpad is kept, make sure to update the scratchpad counter before calling this function
    /// The method [`Scratchpad::update_and_sign`] should be used before calling this function to send the scratchpad to the network
    pub async fn scratchpad_update(&self, scratchpad: Scratchpad) -> Result<(), PutError> {
        let scratch_address = scratchpad.network_address();
        let scratch_key = scratch_address.to_record_key();

        let put_cfg = PutRecordCfg {
            put_quorum: ResponseQuorum::Majority,
            retry_strategy: self
                .operation_config
                .scratchpad_operation_config
                .write_retry_strategy,
            use_put_record_to: None,
            verification: Some((
                VerificationKind::Crdt,
                GetRecordCfg {
                    get_quorum: self
                        .operation_config
                        .scratchpad_operation_config
                        .verification_quorum,
                    retry_strategy: self
                        .operation_config
                        .scratchpad_operation_config
                        .verification_retry_strategy,
                    target_record: None,
                    expected_holders: HashSet::new(),
                },
            )),
        };

        let record = Record {
            key: scratch_key,
            value: try_serialize_record(&scratchpad, RecordKind::DataOnly(DataTypes::Scratchpad))
                .map_err(|_| PutError::Serialization("Failed to serialize scratchpad".to_string()))?
                .to_vec(),
            publisher: None,
            expires: None,
        };

        debug!("Put record - scratchpad at {scratch_address:?} to the network");
        self.network
            .put_record(record, &put_cfg)
            .await
            .inspect_err(|err| {
                error!(
                    "Failed to put scratchpad {scratch_address:?} to the network with err: {err:?}"
                )
            })?;

        Ok(())
    }

    /// Get the cost of creating a new Scratchpad
    pub async fn scratchpad_cost(&self, owner: &SecretKey) -> Result<AttoTokens, CostError> {
        info!("Getting cost for scratchpad");
        let client_pk = owner.public_key();
        let content_type = Default::default();
        let scratch = Scratchpad::new(client_pk, content_type);
        let scratch_xor = scratch.address().xorname();

        // TODO: define default size of Scratchpad
        let store_quote = self
            .get_store_quotes(
                DataTypes::Scratchpad.get_index(),
                std::iter::once((scratch_xor, 256)),
            )
            .await?;

        let total_cost = AttoTokens::from_atto(
            store_quote
                .0
                .values()
                .map(|quote| quote.price())
                .sum::<Amount>(),
        );

        Ok(total_cost)
    }
}
