// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Scratchpad operations for the Autonomi client.
//! This module provides scratchpad upload, download, and cost estimation.
//! All operations delegate to autonomi_core::Client through the wrapper.

use crate::{
    AttoTokens, Client, GetError, NetworkError,
    client::{PutError, payment::PaymentOption, quote::CostError},
};
use ant_protocol::{
    NetworkAddress,
    storage::{DataTypes, RecordKind, try_deserialize_record},
};
use autonomi_core::DataContent;

pub use crate::Bytes;
pub use ant_protocol::storage::{Scratchpad, ScratchpadAddress};
pub use bls::{PublicKey, SecretKey, Signature};

const SCRATCHPAD_MAX_SIZE: usize = Scratchpad::MAX_SIZE;

#[derive(Debug, thiserror::Error)]
pub enum ScratchpadError {
    #[error("Failed to put scratchpad: {0}")]
    PutError(#[from] PutError),
    #[error("Payment failure occurred during scratchpad creation.")]
    Pay(#[from] crate::client::payment::PayError),
    #[error(transparent)]
    GetError(#[from] GetError),
    #[error("Scratchpad found at {0:?} was not a valid record.")]
    Corrupt(ScratchpadAddress),
    #[error("Serialization error")]
    Serialization,
    #[error("Scratchpad already exists at this address: {0:?}")]
    ScratchpadAlreadyExists(ScratchpadAddress),
    #[error(
        "Scratchpad cannot be updated as it does not exist, please create it first or wait for it to be created"
    )]
    CannotUpdateNewScratchpad,
    #[error("Scratchpad size is too big: {0} > {SCRATCHPAD_MAX_SIZE}")]
    ScratchpadTooBig(usize),
    #[error("Scratchpad signature is not valid")]
    BadSignature,
    #[error(
        "Got multiple conflicting scratchpads with the latest version, the fork can be resolved by putting a new scratchpad with a higher counter"
    )]
    Fork(Vec<Scratchpad>),
}

impl Client {
    /// Get Scratchpad from the Network.
    /// A Scratchpad is stored at the owner's public key so we can derive the address from it.
    pub async fn scratchpad_get_from_public_key(
        &self,
        public_key: &PublicKey,
    ) -> Result<Scratchpad, ScratchpadError> {
        let address = ScratchpadAddress::new(*public_key);
        self.scratchpad_get(&address).await
    }

    /// Get a scratchpad from the network.
    pub async fn scratchpad_get(
        &self,
        address: &ScratchpadAddress,
    ) -> Result<Scratchpad, ScratchpadError> {
        let network_addr = NetworkAddress::from(*address);

        match self.core_client.record_get(&network_addr).await {
            Ok(content) => match content {
                DataContent::Scratchpad(scratchpad) => Ok(scratchpad),
                DataContent::ScratchpadSplit(scratchpads) => {
                    Err(ScratchpadError::Fork(scratchpads))
                }
                _ => Err(
                    GetError::RecordKindMismatch(RecordKind::DataOnly(DataTypes::Scratchpad))
                        .into(),
                ),
            },
            Err(e) => Err(ScratchpadError::GetError(GetError::from_error(&e))),
        }
    }

    /// Check if a scratchpad exists at the given address.
    pub async fn scratchpad_check_existence(
        &self,
        address: &ScratchpadAddress,
    ) -> Result<bool, ScratchpadError> {
        let network_addr = NetworkAddress::from(*address);
        self.core_client
            .record_check_existence(&network_addr)
            .await
            .map_err(|e| GetError::from_error(&e).into())
    }

    /// Verify a scratchpad
    pub fn scratchpad_verify(scratchpad: &Scratchpad) -> Result<(), ScratchpadError> {
        if !scratchpad.verify_signature() {
            return Err(ScratchpadError::BadSignature);
        }
        if scratchpad.is_too_big() {
            return Err(ScratchpadError::ScratchpadTooBig(scratchpad.size()));
        }
        Ok(())
    }

    /// Manually store a scratchpad on the network
    pub async fn scratchpad_put(
        &self,
        scratchpad: Scratchpad,
        payment_option: PaymentOption,
    ) -> Result<(AttoTokens, ScratchpadAddress), ScratchpadError> {
        // Only need to pay for the oracle scratchpad (scratchpad.counter() == 0)
        let pass_down = if scratchpad.counter() == 0 {
            Some(payment_option)
        } else {
            None
        };

        let scratchpad_addr = *scratchpad.address();
        let data_content = DataContent::Scratchpad(scratchpad);
        self.core_client
            .record_put(data_content, pass_down)
            .await
            .map(|(cost, _addr)| (cost, scratchpad_addr))
            .map_err(|e| PutError::from_error(&e).into())
    }

    /// Create a new scratchpad to the network.
    ///
    /// Make sure that the owner key is not already used for another scratchpad as each key is associated with one scratchpad.
    /// The data will be encrypted with the owner key before being stored on the network.
    /// The content type is used to identify the type of data stored in the scratchpad, the choice is up to the caller.
    ///
    /// Returns the cost and the address of the scratchpad.
    pub async fn scratchpad_create(
        &self,
        owner: &SecretKey,
        content_type: u64,
        initial_data: &Bytes,
        payment_option: PaymentOption,
    ) -> Result<(AttoTokens, ScratchpadAddress), ScratchpadError> {
        let address = ScratchpadAddress::new(owner.public_key());
        let already_exists = self.scratchpad_check_existence(&address).await?;
        if already_exists {
            return Err(ScratchpadError::ScratchpadAlreadyExists(address));
        }

        let counter = 0;
        let scratchpad = Scratchpad::new(owner, content_type, initial_data, counter);
        self.scratchpad_put(scratchpad, payment_option).await
    }

    /// Update an existing scratchpad to the network.
    /// The scratchpad needs to be created first with [`Client::scratchpad_create`].
    /// This operation is free as the scratchpad was already paid for at creation.
    /// Only the latest version of the scratchpad is kept on the Network, previous versions will be overwritten and unrecoverable.
    pub async fn scratchpad_update(
        &self,
        owner: &SecretKey,
        content_type: u64,
        data: &Bytes,
    ) -> Result<(), ScratchpadError> {
        let address = ScratchpadAddress::new(owner.public_key());
        let current = match self.scratchpad_get(&address).await {
            Ok(scratchpad) => Some(scratchpad),
            Err(ScratchpadError::GetError(GetError::RecordNotFound)) => None,
            Err(ScratchpadError::GetError(GetError::Network(NetworkError::SplitRecord(
                result_map,
            )))) => result_map
                .values()
                .filter_map(|record| try_deserialize_record::<Scratchpad>(record).ok())
                .max_by_key(|scratchpad: &Scratchpad| scratchpad.counter()),
            Err(err) => {
                return Err(err);
            }
        };

        if let Some(p) = current {
            let _new = self
                .scratchpad_update_from(&p, owner, content_type, data)
                .await?;
            Ok(())
        } else {
            warn!(
                "Scratchpad at address {address:?} cannot be updated as it does not exist, please create it first or wait for it to be created"
            );
            Err(ScratchpadError::CannotUpdateNewScratchpad)
        }
    }

    /// Update an existing scratchpad from a specific scratchpad
    ///
    /// This will increment the counter of the scratchpad and update the content
    /// This function is used internally by [`Client::scratchpad_update`] after the scratchpad has been retrieved from the network.
    /// To skip the retrieval step if you already have the scratchpad, use this function directly
    /// This function will return the new scratchpad after it has been updated
    pub async fn scratchpad_update_from(
        &self,
        current: &Scratchpad,
        owner: &SecretKey,
        content_type: u64,
        data: &Bytes,
    ) -> Result<Scratchpad, ScratchpadError> {
        // prepare the new scratchpad to be stored
        let address = ScratchpadAddress::new(owner.public_key());
        let new_counter = current.counter() + 1;
        info!("Updating scratchpad at address {address:?} to version {new_counter}");
        let scratchpad = Scratchpad::new(owner, content_type, data, new_counter);

        // make sure the scratchpad is valid
        Self::scratchpad_verify(&scratchpad)?;

        let data_content = DataContent::Scratchpad(scratchpad.clone());

        let _ = self
            .core_client
            .record_put(data_content, None)
            .await
            .map_err(|e| PutError::from_error(&e))?;

        Ok(scratchpad)
    }

    /// Get the cost for storing a scratchpad.
    /// Delegates to autonomi_core through the wrapper pattern.
    pub async fn scratchpad_cost(&self, owner: &PublicKey) -> Result<AttoTokens, CostError> {
        let scratchpad_addr = ScratchpadAddress::new(*owner);
        let network_addr = NetworkAddress::from(scratchpad_addr);

        self.core_client
            .get_cost_estimation(vec![(network_addr, 0)]) // 0 size means use max size
            .await
            .map_err(|e| CostError::from_error(&e))
    }
}
