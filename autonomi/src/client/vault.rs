// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

pub mod key;
pub mod user_data;

pub use key::{derive_vault_key, VaultSecretKey};
pub use user_data::UserData;

use super::data::CostError;
use crate::client::data::PutError;
use crate::client::payment::PaymentOption;
use crate::client::Client;
use libp2p::kad::{Quorum, Record};
use sn_evm::{Amount, AttoTokens};
use sn_networking::{GetRecordCfg, GetRecordError, NetworkError, PutRecordCfg, VerificationKind};
use sn_protocol::storage::{
    try_serialize_record, RecordKind, RetryStrategy, Scratchpad, ScratchpadAddress,
};
use sn_protocol::Bytes;
use sn_protocol::{storage::try_deserialize_record, NetworkAddress};
use std::collections::HashSet;
use std::hash::{DefaultHasher, Hash, Hasher};
use tracing::info;

#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("Could not generate Vault secret key from entropy: {0:?}")]
    Bls(#[from] bls::Error),
    #[error("Scratchpad found at {0:?} was not a valid record.")]
    CouldNotDeserializeVaultScratchPad(ScratchpadAddress),
    #[error("Protocol: {0}")]
    Protocol(#[from] sn_protocol::Error),
    #[error("Network: {0}")]
    Network(#[from] NetworkError),
    #[error("Vault not found")]
    Missing,
}

/// The content type of the vault data
/// The number is used to determine the type of the contents of the bytes contained in a vault
/// Custom apps can use this to store their own custom types of data in vaults
/// It is recommended to use the hash of the app name or an unique identifier as the content type using [`app_name_to_vault_content_type`]
/// The value 0 is reserved for tests
pub type VaultContentType = u64;

/// For custom apps using Scratchpad, this function converts an app identifier or name to a [`VaultContentType`]
pub fn app_name_to_vault_content_type<T: Hash>(s: T) -> VaultContentType {
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

impl Client {
    /// Retrieves and returns a decrypted vault if one exists.
    /// Returns the content type of the bytes in the vault
    pub async fn fetch_and_decrypt_vault(
        &self,
        secret_key: &VaultSecretKey,
    ) -> Result<(Bytes, VaultContentType), VaultError> {
        info!("Fetching and decrypting vault");
        let pad = self.get_vault_from_network(secret_key).await?;

        let data = pad.decrypt_data(secret_key)?;
        debug!("Data is successfully fetched and decrypted");
        Ok((data, pad.data_encoding()))
    }

    /// Gets the vault Scratchpad from a provided client public key
    async fn get_vault_from_network(
        &self,
        secret_key: &VaultSecretKey,
    ) -> Result<Scratchpad, VaultError> {
        let client_pk = secret_key.public_key();

        let scratch_address = ScratchpadAddress::new(client_pk);
        let network_address = NetworkAddress::from_scratchpad_address(scratch_address);
        info!("Fetching vault from network at {network_address:?}",);
        let scratch_key = network_address.to_record_key();

        let get_cfg = GetRecordCfg {
            get_quorum: Quorum::Majority,
            retry_strategy: None,
            target_record: None,
            expected_holders: HashSet::new(),
            is_register: false,
        };

        let pad = match self
            .network
            .get_record_from_network(scratch_key.clone(), &get_cfg)
            .await
        {
            Ok(record) => {
                debug!("Got scratchpad for {scratch_key:?}");
                try_deserialize_record::<Scratchpad>(&record)
                    .map_err(|_| VaultError::CouldNotDeserializeVaultScratchPad(scratch_address))?
            }
            Err(NetworkError::GetRecordError(GetRecordError::SplitRecord { result_map })) => {
                debug!("Got multiple scratchpads for {scratch_key:?}");
                let mut pads = result_map
                    .values()
                    .map(|(record, _)| try_deserialize_record::<Scratchpad>(record))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| VaultError::CouldNotDeserializeVaultScratchPad(scratch_address))?;

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
                        return Err(VaultError::Missing);
                    }
                };
                pad.to_owned()
            }
            Err(e) => {
                warn!("Failed to fetch vault {network_address:?} from network: {e}");
                return Err(e)?;
            }
        };
        debug!("Fetched vault scratchpad successfully");

        Ok(pad)
    }

    /// Get the cost of creating a new vault
    pub async fn vault_cost(&self, owner: &VaultSecretKey) -> Result<AttoTokens, CostError> {
        info!("Getting cost for vault");
        let client_pk = owner.public_key();
        let content_type = Default::default();
        let scratch = Scratchpad::new(client_pk, content_type);
        let vault_xor = scratch.network_address().as_xorname().unwrap_or_default();

        // NB TODO: vault should be priced differently from other data
        let cost_map = self.get_store_quotes(std::iter::once(vault_xor)).await?;
        let total_cost = AttoTokens::from_atto(
            cost_map
                .values()
                .map(|quote| quote.2.cost.as_atto())
                .sum::<Amount>(),
        );
        debug!("Cost for the new vault is successfully computed");
        Ok(total_cost)
    }

    /// Put data into the client's VaultPacket
    ///
    /// Pays for a new VaultPacket if none yet created for the client.
    /// Provide the bytes to be written to the vault and the content type of those bytes.
    /// It is recommended to use the hash of the app name or unique identifier as the content type.
    pub async fn write_bytes_to_vault(
        &self,
        data: Bytes,
        payment_option: PaymentOption,
        secret_key: &VaultSecretKey,
        content_type: VaultContentType,
    ) -> Result<AttoTokens, PutError> {
        let mut total_cost = AttoTokens::zero();

        let (mut scratch, is_new) = self
            .get_or_create_scratchpad(secret_key, content_type)
            .await?;

        let _ = scratch.update_and_sign(data, secret_key);
        debug_assert!(scratch.is_valid(), "Must be valid after being signed. This is a bug, please report it by opening an issue on our github");

        let scratch_address = scratch.network_address();
        let scratch_key = scratch_address.to_record_key();

        info!("Writing to vault at {scratch_address:?}",);

        let record = if is_new {
            let receipt = self
                .pay_for_content_addrs(scratch.to_xor_name_vec().into_iter(), payment_option)
                .await
                .inspect_err(|err| {
                    error!("Failed to pay for new vault at addr: {scratch_address:?} : {err}");
                })?;

            let proof = match receipt.values().next() {
                Some(proof) => proof,
                None => return Err(PutError::PaymentUnexpectedlyInvalid(scratch_address)),
            };

            total_cost = proof.quote.cost;

            Record {
                key: scratch_key,
                value: try_serialize_record(&(proof, scratch), RecordKind::ScratchpadWithPayment)
                    .map_err(|_| {
                        PutError::Serialization(
                            "Failed to serialize scratchpad with payment".to_string(),
                        )
                    })?
                    .to_vec(),
                publisher: None,
                expires: None,
            }
        } else {
            Record {
                key: scratch_key,
                value: try_serialize_record(&scratch, RecordKind::Scratchpad)
                    .map_err(|_| {
                        PutError::Serialization("Failed to serialize scratchpad".to_string())
                    })?
                    .to_vec(),
                publisher: None,
                expires: None,
            }
        };

        let put_cfg = PutRecordCfg {
            put_quorum: Quorum::Majority,
            retry_strategy: Some(RetryStrategy::Balanced),
            use_put_record_to: None,
            verification: Some((
                VerificationKind::Crdt,
                GetRecordCfg {
                    get_quorum: Quorum::Majority,
                    retry_strategy: None,
                    target_record: None,
                    expected_holders: HashSet::new(),
                    is_register: false,
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

    /// Returns an existing scratchpad or creates a new one if it does not exist.
    pub async fn get_or_create_scratchpad(
        &self,
        secret_key: &VaultSecretKey,
        content_type: VaultContentType,
    ) -> Result<(Scratchpad, bool), PutError> {
        let client_pk = secret_key.public_key();

        let pad_res = self.get_vault_from_network(secret_key).await;
        let mut is_new = true;

        let scratch = if let Ok(existing_data) = pad_res {
            info!("Scratchpad already exists, returning existing data");

            info!(
                "scratch already exists, is version {:?}",
                existing_data.count()
            );

            is_new = false;

            if existing_data.owner() != &client_pk {
                return Err(PutError::VaultBadOwner);
            }

            existing_data
        } else {
            trace!("new scratchpad creation");
            Scratchpad::new(client_pk, content_type)
        };

        Ok((scratch, is_new))
    }
}
