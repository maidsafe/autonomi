// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

mod equation;
pub use equation::{compute, COMPLEX_EQUATION, PLUS_EQUATION};

use crate::client::payment::PaymentOption;
use crate::client::Client;
use ant_evm::AttoTokens;
use ant_protocol::storage::{Chunk, ChunkAddress, PointerAddress, PointerTarget};
use bls::SecretKey;
use bytes::Bytes;
use thiserror::Error;
use tracing::info;

#[derive(Debug, Error)]
pub enum SmartEquationError {
    #[error("Serialization error: {0}")]
    Serialization(String),
    #[error("Deserialization error: {0}")]
    Deserialization(String),
    #[error("Network error: {0}")]
    Network(String),
    #[error("Pointer not found")]
    PointerNotFound,
    #[error("Chunk not found")]
    ChunkNotFound,
    #[error("InvalidHeadPointer: {0:?}")]
    InvalidHeadPointer(PointerTarget),
}

impl Client {
    /// Publishes a new smart equation to the network.
    /// Takes a serializable JSON object, stores it as a Chunk, and creates a Pointer to it.
    pub async fn publish_smart_equation(
        &self,
        equation_data: String,
        owner: &SecretKey,
        payment_option: PaymentOption,
    ) -> Result<(AttoTokens, PointerAddress), SmartEquationError> {
        info!("Publishing new smart equation...");

        // Create and upload the chunk
        let chunk = Chunk::new(Bytes::from(equation_data));
        let chunk_address = ChunkAddress::new(*chunk.name());
        self.chunk_put(&chunk, payment_option.clone())
            .await
            .map_err(|e| SmartEquationError::Network(e.to_string()))?;

        // Create and upload the pointer
        let pointer_target = PointerTarget::ChunkAddress(chunk_address);
        let (cost, pointer_address) = self
            .pointer_create(owner, pointer_target, payment_option)
            .await
            .map_err(|e| SmartEquationError::Network(e.to_string()))?;

        info!(
            "Smart equation published at address: {:?} with cost of {cost}",
            pointer_address
        );
        Ok((cost, pointer_address))
    }

    /// Retrieves a smart equation from the network.
    /// Takes a PointerAddress and returns the deserialized JSON object.
    pub async fn get_smart_equation(
        &self,
        pointer_address: PointerAddress,
    ) -> Result<Bytes, SmartEquationError> {
        info!(
            "Retrieving smart equation at address: {:?}",
            pointer_address
        );

        // Get the pointer
        let pointer = self
            .pointer_get(&pointer_address)
            .await
            .map_err(|_| SmartEquationError::PointerNotFound)?;

        // Get the chunk
        let chunk_addr = match pointer.target() {
            PointerTarget::ChunkAddress(addr) => addr,
            other => return Err(SmartEquationError::InvalidHeadPointer(other.clone())),
        };
        let chunk = self
            .chunk_get(chunk_addr)
            .await
            .map_err(|_| SmartEquationError::ChunkNotFound)?;

        info!("Smart equation successfully retrieved");
        Ok(chunk.value)
    }

    /// Updates an existing smart equation on the network.
    /// Takes new equation data and updates the Pointer to point to the new Chunk.
    pub async fn update_smart_equation(
        &self,
        pointer_address: PointerAddress,
        new_equation_data: String,
        owner: &SecretKey,
        payment_option: PaymentOption,
    ) -> Result<(), SmartEquationError> {
        info!("Updating smart equation at address: {:?}", pointer_address);

        // Create and upload the new chunk
        let new_chunk = Chunk::new(Bytes::from(new_equation_data));
        let new_chunk_address = ChunkAddress::new(*new_chunk.name());
        self.chunk_put(&new_chunk, payment_option.clone())
            .await
            .map_err(|e| SmartEquationError::Network(e.to_string()))?;

        // Update the pointer to point to the new chunk
        let new_pointer_target = PointerTarget::ChunkAddress(new_chunk_address);
        self.pointer_update(owner, new_pointer_target)
            .await
            .map_err(|_| SmartEquationError::PointerNotFound)?;

        info!("Smart equation at {pointer_address:?} successfully updated");
        Ok(())
    }
}
