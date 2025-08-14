// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Pointer operations for the Autonomi client.
//! This module provides pointer upload, download, and cost estimation.
//! All operations delegate to autonomi_core::Client through the wrapper.

use crate::client::{Client, GetError, PutError, payment::PaymentOption, quote::CostError};

use ant_evm::{AttoTokens, EvmWalletError};
use ant_protocol::{
    NetworkAddress,
    storage::{DataTypes, RecordKind},
};
use autonomi_core::DataContent;
use tracing::{error, trace};

pub use ant_protocol::storage::{Pointer, PointerAddress, PointerTarget};
pub use bls::{PublicKey, SecretKey};

#[derive(Debug, thiserror::Error)]
pub enum PointerError {
    #[error("Failed to put pointer: {0}")]
    PutError(#[from] PutError),
    #[error(transparent)]
    GetError(#[from] GetError),
    #[error("Serialization error")]
    Serialization,
    #[error("Pointer record corrupt: {0}")]
    Corrupt(String),
    #[error("Pointer signature is invalid")]
    BadSignature,
    #[error("Payment failure occurred during pointer creation.")]
    Pay(#[from] crate::client::payment::PayError),
    #[error("Failed to retrieve wallet payment")]
    Wallet(#[from] EvmWalletError),
    #[error(
        "Received invalid quote from node, this node is possibly malfunctioning, try another node by trying another pointer name"
    )]
    InvalidQuote,
    #[error("Pointer already exists at this address: {0:?}")]
    PointerAlreadyExists(PointerAddress),
    #[error(
        "Pointer cannot be updated as it does not exist, please create it first or wait for it to be created"
    )]
    CannotUpdateNewPointer,
}

impl Client {
    /// Get a pointer from the network.
    pub async fn pointer_get(&self, address: &PointerAddress) -> Result<Pointer, PointerError> {
        let network_addr = NetworkAddress::from(*address);

        match self.core_client.record_get(&network_addr).await {
            Ok(content) => match content {
                DataContent::Pointer(pointer) => Ok(pointer),
                _ => Err(
                    GetError::RecordKindMismatch(RecordKind::DataOnly(DataTypes::Pointer)).into(),
                ),
            },
            Err(e) => Err(GetError::from_error(&e).into()),
        }
    }

    /// Check if a pointer exists on the network
    /// This method is much faster than [`Client::pointer_get`]
    /// This may fail if called immediately after creating the pointer, as nodes sometimes take longer to store the pointer than this request takes to execute!
    pub async fn pointer_check_existence(
        &self,
        address: &PointerAddress,
    ) -> Result<bool, PointerError> {
        let network_addr = NetworkAddress::from(*address);
        self.core_client
            .record_check_existence(&network_addr)
            .await
            .map_err(|e| GetError::from_error(&e).into())
    }

    /// Verify a pointer
    pub fn pointer_verify(pointer: &Pointer) -> Result<(), PointerError> {
        if !pointer.verify_signature() {
            return Err(PointerError::BadSignature);
        }
        Ok(())
    }

    /// Manually store a pointer on the network
    pub async fn pointer_put(
        &self,
        pointer: Pointer,
        payment_option: PaymentOption,
    ) -> Result<(AttoTokens, PointerAddress), PointerError> {
        let pointer_addr = pointer.address();

        if self.pointer_check_existence(&pointer_addr).await? {
            return Err(PointerError::PointerAlreadyExists(pointer_addr));
        }

        let data_content = DataContent::Pointer(pointer);

        self.core_client
            .record_put(data_content, Some(payment_option))
            .await
            .map(|(cost, _addr)| (cost, pointer_addr))
            .map_err(|e| PutError::from_error(&e).into())
    }

    /// Create a new pointer on the network.
    ///
    /// Make sure that the owner key is not already used for another pointer as each key is associated with one pointer
    pub async fn pointer_create(
        &self,
        owner: &SecretKey,
        target: PointerTarget,
        payment_option: PaymentOption,
    ) -> Result<(AttoTokens, PointerAddress), PointerError> {
        let address = PointerAddress::new(owner.public_key());
        let already_exists = self.pointer_check_existence(&address).await?;
        if already_exists {
            return Err(PointerError::PointerAlreadyExists(address));
        }

        let pointer = Pointer::new(owner, 0, target);
        self.pointer_put(pointer, payment_option).await
    }

    /// Update an existing pointer to point to a new target on the network.
    ///
    /// The pointer needs to be created first with [`Client::pointer_put`].
    /// This operation is free as the pointer was already paid for at creation.
    /// Only the latest version of the pointer is kept on the Network, previous versions will be overwritten and unrecoverable.
    pub async fn pointer_update(
        &self,
        owner: &SecretKey,
        target: PointerTarget,
    ) -> Result<(), PointerError> {
        let address = PointerAddress::new(owner.public_key());
        info!("Updating pointer at address {address:?} to {target:?}");

        if self.pointer_check_existence(&address).await? {
            return Err(PointerError::CannotUpdateNewPointer);
        }

        // Will always return the highest pointer
        let current = self.pointer_get(&address).await?;

        let _ = self.pointer_update_from(&current, owner, target).await?;
        Ok(())
    }

    /// Update an existing pointer from a specific pointer
    ///
    /// This will increment the counter of the pointer and update the target
    /// This function is used internally by [`Client::pointer_update`] after the pointer has been retrieved from the network.
    /// To skip the retrieval step if you already have the pointer, use this function directly
    /// This function will return the new pointer after it has been updated
    pub async fn pointer_update_from(
        &self,
        current: &Pointer,
        owner: &SecretKey,
        new_target: PointerTarget,
    ) -> Result<Pointer, PointerError> {
        // prepare the new pointer to be stored
        let address = PointerAddress::new(owner.public_key());
        let new_counter = current.counter() + 1;
        info!("Updating pointer at address {address:?} to version {new_counter}");
        let pointer = Pointer::new(owner, new_counter, new_target);

        let data_content = DataContent::Pointer(pointer.clone());
        let _ = self
            .core_client
            .record_put(data_content, None)
            .await
            .map(|(cost, _addr)| (cost, address))
            .map_err(|e| PutError::from_error(&e))?;

        Ok(pointer)
    }

    /// Calculate the cost of storing a pointer
    pub async fn pointer_cost(&self, key: &PublicKey) -> Result<AttoTokens, CostError> {
        trace!("Getting cost for pointer of {key:?}");
        let pointer_addr = PointerAddress::new(*key);
        let network_addr = NetworkAddress::from(pointer_addr);

        self.core_client
            .get_cost_estimation(vec![(network_addr, 0)]) // 0 size means use max size
            .await
            .map_err(|e| CostError::from_error(&e))
    }
}
