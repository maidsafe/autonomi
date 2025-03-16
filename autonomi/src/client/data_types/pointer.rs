// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::client::{
    payment::{PayError, PaymentOption},
    quote::CostError,
    Client,
};
use ant_evm::{Amount, AttoTokens, EvmWalletError};
use ant_networking::{Addresses, GetRecordError, NetworkError};
use ant_protocol::{
    storage::{try_deserialize_record, try_serialize_record, DataTypes, RecordHeader, RecordKind},
    NetworkAddress,
};
use bls::{PublicKey, SecretKey};
use libp2p::kad::Record;
use tracing::{debug, error, trace};

pub use ant_protocol::storage::{Pointer, PointerAddress, PointerTarget};

/// Errors that can occur when dealing with Pointers
#[derive(Debug, thiserror::Error)]
pub enum PointerError {
    #[error("Network error")]
    Network(#[from] NetworkError),
    #[error("Serialization error")]
    Serialization,
    #[error("Pointer record corrupt: {0}")]
    Corrupt(String),
    #[error("Pointer signature is invalid")]
    BadSignature,
    #[error("Payment failure occurred during pointer creation.")]
    Pay(#[from] PayError),
    #[error("Failed to retrieve wallet payment")]
    Wallet(#[from] EvmWalletError),
    #[error("Received invalid quote from node, this node is possibly malfunctioning, try another node by trying another pointer name")]
    InvalidQuote,
    #[error("Pointer already exists at this address: {0:?}")]
    PointerAlreadyExists(PointerAddress),
    #[error("Pointer cannot be updated as it does not exist, please create it first or wait for it to be created")]
    CannotUpdateNewPointer,
}

impl Client {
    /// Get a pointer from the network
    pub async fn pointer_get(&self, address: &PointerAddress) -> Result<Pointer, PointerError> {
        let key = NetworkAddress::from_pointer_address(*address).to_record_key();
        debug!("Fetching pointer from network at: {key:?}");

        let get_cfg = self.config.pointer.get_cfg();
        let record = self
            .network
            .get_record_from_network(key.clone(), &get_cfg)
            .await
            .inspect_err(|err| error!("Error fetching pointer: {err:?}"))?;
        let header = RecordHeader::from_record(&record).map_err(|err| {
            PointerError::Corrupt(format!(
                "Failed to parse record header for pointer at {key:?}: {err:?}"
            ))
        })?;

        let kind = header.kind;
        if !matches!(kind, RecordKind::DataOnly(DataTypes::Pointer)) {
            error!("Record kind mismatch: expected Pointer, got {kind:?}");
            return Err(
                NetworkError::RecordKindMismatch(RecordKind::DataOnly(DataTypes::Pointer)).into(),
            );
        };

        let pointer: Pointer = try_deserialize_record(&record).map_err(|err| {
            PointerError::Corrupt(format!(
                "Failed to parse record for pointer at {key:?}: {err:?}"
            ))
        })?;

        Self::pointer_verify(&pointer)?;
        Ok(pointer)
    }

    /// Check if a pointer exists on the network
    pub async fn pointer_check_existance(
        &self,
        address: &PointerAddress,
    ) -> Result<bool, PointerError> {
        let key = NetworkAddress::from_pointer_address(*address).to_record_key();
        debug!("Checking pointer existance at: {key:?}");
        let get_cfg = self.config.pointer.verification_cfg();
        match self
            .network
            .get_record_from_network(key.clone(), &get_cfg)
            .await
        {
            Ok(_) => Ok(true),
            Err(NetworkError::GetRecordError(GetRecordError::SplitRecord { .. })) => Ok(true),
            Err(NetworkError::GetRecordError(GetRecordError::RecordNotFound)) => Ok(false),
            Err(err) => Err(PointerError::Network(err))
                .inspect_err(|err| error!("Error checking pointer existance: {err:?}")),
        }
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
        let address = pointer.address();

        // pay for the pointer storage
        let xor_name = address.xorname();
        debug!("Paying for pointer at address: {address:?}");
        let (payment_proofs, _skipped_payments) = self
            .pay_for_content_addrs(
                DataTypes::Pointer,
                std::iter::once((xor_name, Pointer::size())),
                payment_option,
            )
            .await
            .inspect_err(|err| {
                error!("Failed to pay for pointer at address: {address:?} : {err}")
            })?;

        // verify payment was successful
        let (proof, price) = match payment_proofs.get(&xor_name) {
            Some((proof, price)) => (Some(proof), price),
            None => {
                info!("Pointer at address: {address:?} was already paid for, update is free");
                (None, &AttoTokens::zero())
            }
        };
        let total_cost = *price;

        let (record, payees) = if let Some(proof) = proof {
            let payees = Some(
                proof
                    .payees()
                    .iter()
                    .map(|(peer_id, addrs)| (*peer_id, Addresses(addrs.clone())))
                    .collect(),
            );
            let record = Record {
                key: NetworkAddress::from_pointer_address(address).to_record_key(),
                value: try_serialize_record(
                    &(proof.to_proof_of_payment(), &pointer),
                    RecordKind::DataWithPayment(DataTypes::Pointer),
                )
                .map_err(|_| PointerError::Serialization)?
                .to_vec(),
                publisher: None,
                expires: None,
            };
            (record, payees)
        } else {
            let record = Record {
                key: NetworkAddress::from_pointer_address(address).to_record_key(),
                value: try_serialize_record(&pointer, RecordKind::DataOnly(DataTypes::Pointer))
                    .map_err(|_| PointerError::Serialization)?
                    .to_vec(),
                publisher: None,
                expires: None,
            };
            (record, None)
        };

        // store the pointer on the network
        debug!("Storing pointer at address {address:?} to the network");
        let put_cfg = self.config.pointer.put_cfg(payees);
        self.network
            .put_record(record, &put_cfg)
            .await
            .inspect_err(|err| {
                error!("Failed to put record - pointer {address:?} to the network: {err}")
            })?;

        Ok((total_cost, address))
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
        let already_exists = self.pointer_check_existance(&address).await?;
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
        let current = match self.pointer_get(&address).await {
            Ok(pointer) => Some(pointer),
            Err(PointerError::Network(NetworkError::GetRecordError(
                GetRecordError::RecordNotFound,
            ))) => None,
            Err(PointerError::Network(NetworkError::GetRecordError(
                GetRecordError::SplitRecord { result_map },
            ))) => result_map
                .values()
                .filter_map(|(record, _)| try_deserialize_record::<Pointer>(record).ok())
                .max_by_key(|pointer: &Pointer| pointer.counter()),
            Err(err) => {
                return Err(err);
            }
        };

        let pointer = if let Some(p) = current {
            let version = p.counter() + 1;
            Pointer::new(owner, version, target)
        } else {
            warn!("Pointer at address {address:?} cannot be updated as it does not exist, please create it first or wait for it to be created");
            return Err(PointerError::CannotUpdateNewPointer);
        };

        // prepare the record to be stored
        let record = Record {
            key: NetworkAddress::from_pointer_address(address).to_record_key(),
            value: try_serialize_record(&pointer, RecordKind::DataOnly(DataTypes::Pointer))
                .map_err(|_| PointerError::Serialization)?
                .to_vec(),
            publisher: None,
            expires: None,
        };

        // store the pointer on the network
        debug!("Updating pointer at address {address:?} to the network");
        let put_cfg = self.config.pointer.put_cfg(None);
        self.network
            .put_record(record, &put_cfg)
            .await
            .inspect_err(|err| {
                error!("Failed to update pointer at address {address:?} to the network: {err}")
            })?;

        Ok(())
    }

    /// Calculate the cost of storing a pointer
    pub async fn pointer_cost(&self, key: &PublicKey) -> Result<AttoTokens, CostError> {
        trace!("Getting cost for pointer of {key:?}");

        let address = PointerAddress::new(*key);
        let xor = address.xorname();
        let store_quote = self
            .get_store_quotes(DataTypes::Pointer, std::iter::once((xor, Pointer::size())))
            .await?;
        let total_cost = AttoTokens::from_atto(
            store_quote
                .0
                .values()
                .map(|quote| quote.price())
                .sum::<Amount>(),
        );
        debug!("Calculated the cost to create pointer of {key:?} is {total_cost}");
        Ok(total_cost)
    }
}
