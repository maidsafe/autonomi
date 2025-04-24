// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use std::collections::HashMap;

use crate::client::{
    payment::{PayError, PaymentOption},
    quote::CostError,
    Client, GetError,
};
use crate::networking::{PeerId, Record};
use ant_evm::{Amount, AttoTokens, EvmWalletError};
use ant_protocol::{
    storage::{try_deserialize_record, try_serialize_record, DataTypes, RecordHeader, RecordKind},
    NetworkAddress,
};
use bls::{PublicKey, SecretKey};
use tracing::{debug, error, trace};

use crate::networking::NetworkError;
pub use ant_protocol::storage::{Pointer, PointerAddress, PointerTarget};

/// Errors that can occur when dealing with Pointers
#[derive(Debug, thiserror::Error)]
pub enum PointerError {
    #[error("Network error")]
    Network(#[from] NetworkError),
    #[error(transparent)]
    GetError(#[from] GetError),
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
        let key = NetworkAddress::from(*address);
        debug!("Fetching pointer from network at: {key:?}");

        let pointer = match self
            .network
            .get_record_with_retries(key.clone(), &self.config.pointer)
            .await
        {
            Ok(Some(r)) => pointer_from_record(r)?,
            Ok(None) => Err(GetError::RecordNotFound)?,
            Err(NetworkError::SplitRecord(result_map)) => {
                warn!("Pointer at {key:?} is split, trying resolution");
                select_highest_pointer_version(result_map, key)?
            }
            Err(err) => {
                error!("Error fetching pointer: {err:?}");
                Err(err)?
            }
        };

        info!("Got pointer at address {address:?}: {pointer:?}");
        Self::pointer_verify(&pointer)?;
        Ok(pointer)
    }

    /// Check if a pointer exists on the network
    /// This method is much faster than [`Client::pointer_get`]
    /// This may fail if called immediately after creating the pointer, as nodes sometimes take longer to store the pointer than this request takes to execute!
    pub async fn pointer_check_existence(
        &self,
        address: &PointerAddress,
    ) -> Result<bool, PointerError> {
        let key = NetworkAddress::from(*address);
        debug!("Checking pointer existence at: {key:?}");

        match self
            .network
            .get_record(key.clone(), self.config.pointer.verification_quorum)
            .await
        {
            Ok(None) => Ok(false),
            Ok(Some(_)) => Ok(true),
            Err(NetworkError::SplitRecord(..)) => Ok(true),
            Err(err) => Err(PointerError::Network(err))
                .inspect_err(|err| error!("Error checking pointer existence: {err:?}")),
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
            let payees = Some(proof.payees());
            let record = Record {
                key: NetworkAddress::from(address).to_record_key(),
                value: try_serialize_record(
                    &(proof, &pointer),
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
                key: NetworkAddress::from(address).to_record_key(),
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

        let target_nodes = payees.unwrap_or_default();

        self.network
            .put_record_with_retries(record, target_nodes, &self.config.pointer)
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
        let current = match self.pointer_get(&address).await {
            Ok(pointer) => Some(pointer),
            Err(PointerError::Network(NetworkError::SplitRecord(result_map))) => result_map
                .values()
                .filter_map(|record| try_deserialize_record::<Pointer>(record).ok())
                .max_by_key(|pointer: &Pointer| pointer.counter()),
            Err(err) => {
                return Err(err);
            }
        };

        let pointer = if let Some(p) = current {
            let version = p.counter() + 1;
            info!("Updating pointer at address {address:?} to version {version}");
            Pointer::new(owner, version, target)
        } else {
            warn!("Pointer at address {address:?} cannot be updated as it does not exist, please create it first or wait for it to be created");
            return Err(PointerError::CannotUpdateNewPointer);
        };

        // prepare the record to be stored
        let record = Record {
            key: NetworkAddress::from(address).to_record_key(),
            value: try_serialize_record(&pointer, RecordKind::DataOnly(DataTypes::Pointer))
                .map_err(|_| PointerError::Serialization)?
                .to_vec(),
            publisher: None,
            expires: None,
        };

        // store the pointer on the network
        debug!("Updating pointer at address {address:?} to the network");

        self.network
            .put_record_with_retries(record, Default::default(), &self.config.pointer)
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

/// Select the highest versioned pointer from a list of conflicting pointer records
///
/// If there are multiple conflicting pointers at the latest version, the first one is returned
/// If there are no valid pointers, an error is returned
fn select_highest_pointer_version(
    result_map: HashMap<PeerId, Record>,
    key: NetworkAddress,
) -> Result<Pointer, PointerError> {
    let highest_version = result_map
        .into_iter()
        .filter_map(|(peer, record)| match pointer_from_record(record) {
            Ok(pointer) => Some(pointer),
            Err(err) => {
                warn!("Peer {peer:?} returned invalid pointer at {key} with error: {err}");
                None
            }
        })
        .max_by_key(|pointer| pointer.counter());

    match highest_version {
        Some(pointer) => Ok(pointer),
        None => {
            let msg = format!("Found multiple conflicting invalid pointers at {key}");
            warn!("{msg}");
            Err(PointerError::Corrupt(msg))
        }
    }
}

/// Deserialize a pointer from a record
fn pointer_from_record(record: Record) -> Result<Pointer, PointerError> {
    let key = &record.key;
    let header = RecordHeader::from_record(&record).map_err(|err| {
        PointerError::Corrupt(format!(
            "Failed to parse record header for pointer at {key:?}: {err:?}"
        ))
    })?;

    let kind = header.kind;
    if !matches!(kind, RecordKind::DataOnly(DataTypes::Pointer)) {
        error!("Record kind mismatch: expected Pointer, got {kind:?}");
        return Err(GetError::RecordKindMismatch(RecordKind::DataOnly(DataTypes::Pointer)).into());
    };

    let pointer: Pointer = try_deserialize_record(&record).map_err(|err| {
        PointerError::Corrupt(format!(
            "Failed to parse record for pointer at {key:?}: {err:?}"
        ))
    })?;

    Ok(pointer)
}
