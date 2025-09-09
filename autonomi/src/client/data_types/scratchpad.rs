// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::resolve_split_records;

use crate::{
    Amount, AttoTokens, Client,
    client::{
        GetError, PutError,
        payment::{PayError, PaymentOption},
        quote::CostError,
    },
    networking::{NetworkError, PeerInfo},
};

use ant_protocol::{
    NetworkAddress,
    storage::{DataTypes, RecordKind, try_deserialize_record, try_serialize_record},
};
use libp2p::kad::Record;

pub use crate::Bytes;
pub use ant_protocol::storage::{Scratchpad, ScratchpadAddress};
pub use bls::{PublicKey, SecretKey, Signature};

const SCRATCHPAD_MAX_SIZE: usize = Scratchpad::MAX_SIZE;

/// Errors that can occur when dealing with Scratchpads
#[derive(Debug, thiserror::Error)]
pub enum ScratchpadError {
    #[error("Failed to put scratchpad: {0}")]
    PutError(#[from] PutError),
    #[error("Payment failure occurred during scratchpad creation.")]
    Pay(#[from] PayError),
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

    /// Get Scratchpad from the Network
    pub async fn scratchpad_get(
        &self,
        address: &ScratchpadAddress,
    ) -> Result<Scratchpad, ScratchpadError> {
        let network_address = NetworkAddress::from(*address);
        info!("Fetching scratchpad from network at {network_address:?}",);
        let scratch_key = network_address.to_record_key();

        let pad = match self
            .network
            .get_record_with_retries(network_address.clone(), &self.config.scratchpad)
            .await
        {
            Ok(maybe_record) => {
                let record = maybe_record.ok_or(GetError::RecordNotFound)?;
                debug!("Got scratchpad for {scratch_key:?}");
                return try_deserialize_record::<Scratchpad>(&record)
                    .map_err(|_| ScratchpadError::Corrupt(*address));
            }
            Err(NetworkError::SplitRecord(result_map)) => {
                debug!("Got multiple scratchpads for {scratch_key:?}");
                resolve_split_records(
                    result_map,
                    network_address.clone(),
                    |r| {
                        try_deserialize_record::<Scratchpad>(&r)
                            .map_err(|_| ScratchpadError::Corrupt(*address))
                    },
                    |s: &Scratchpad| s.counter(),
                    |a: &Scratchpad, b: &Scratchpad| {
                        a.data_encoding() == b.data_encoding()
                            && a.encrypted_data() == b.encrypted_data()
                    },
                    |latest: Vec<Scratchpad>| ScratchpadError::Fork(latest),
                    || ScratchpadError::Corrupt(*address),
                )?
            }
            Err(e) => {
                warn!("Failed to fetch scratchpad {network_address:?} from network: {e}");
                return Err(ScratchpadError::GetError(e.into()));
            }
        };

        Self::scratchpad_verify(&pad)?;
        Ok(pad)
    }

    /// Check if a scratchpad exists on the network
    /// This method is much faster than [`Client::scratchpad_get`]
    /// This may fail if called immediately after creating the scratchpad, as nodes sometimes take longer to store the scratchpad than this request takes to execute!
    pub async fn scratchpad_check_existence(
        &self,
        address: &ScratchpadAddress,
    ) -> Result<bool, ScratchpadError> {
        let key = NetworkAddress::from(*address);
        debug!("Checking scratchpad existence at: {key:?}");

        match self
            .network
            .get_record(key, self.config.scratchpad.verification_quorum)
            .await
        {
            Ok(Some(_)) => Ok(true),
            Ok(None) => Ok(false),
            Err(NetworkError::SplitRecord(..)) => Ok(true),
            Err(err) => Err(ScratchpadError::GetError(err.into()))
                .inspect_err(|err| error!("Error checking scratchpad existence: {err:?}")),
        }
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
        let address = scratchpad.address();
        Self::scratchpad_verify(&scratchpad)?;

        // pay for the scratchpad
        let xor_name = address.xorname();
        debug!("Paying for scratchpad at address: {address:?}");
        let (payment_proofs, _skipped_payments) = self
            .pay_for_content_addrs(
                DataTypes::Scratchpad,
                std::iter::once((xor_name, scratchpad.size())),
                payment_option,
            )
            .await
            .inspect_err(|err| {
                error!("Failed to pay for scratchpad at address: {address:?} : {err}")
            })?;

        // verify payment was successful
        let (proof, price) = match payment_proofs.get(&xor_name) {
            Some((proof, price)) => (Some(proof), price),
            None => {
                info!("Scratchpad at address: {address:?} was already paid for, update is free");
                (None, &AttoTokens::zero())
            }
        };
        let total_cost = *price;

        let net_addr = NetworkAddress::from(*address);
        let (record, target_nodes) = if let Some(proof) = proof {
            let payees = proof
                .payees()
                .iter()
                .map(|(peer_id, addrs)| PeerInfo {
                    peer_id: *peer_id,
                    addrs: addrs.clone(),
                })
                .collect();
            let record = Record {
                key: net_addr.to_record_key(),
                value: try_serialize_record(
                    &(proof.to_proof_of_payment(), &scratchpad),
                    RecordKind::DataWithPayment(DataTypes::Scratchpad),
                )
                .map_err(|_| ScratchpadError::Serialization)?
                .to_vec(),
                publisher: None,
                expires: None,
            };
            (record, payees)
        } else {
            let record = Record {
                key: net_addr.to_record_key(),
                value: try_serialize_record(
                    &scratchpad,
                    RecordKind::DataOnly(DataTypes::Scratchpad),
                )
                .map_err(|_| ScratchpadError::Serialization)?
                .to_vec(),
                publisher: None,
                expires: None,
            };
            let target_nodes = self
                .network
                .get_closest_peers_with_retries(net_addr.clone())
                .await
                .map_err(|e| PutError::Network {
                    address: Box::new(net_addr),
                    network_error: e,
                    payment: None,
                })?;
            (record, target_nodes)
        };

        // store the scratchpad on the network
        debug!(
            "Storing scratchpad at address {address:?} to the network on nodes {target_nodes:?}"
        );

        self.network
            .put_record_with_retries(record, target_nodes, &self.config.scratchpad)
            .await
            .inspect_err(|err| {
                error!("Failed to put record - scratchpad {address:?} to the network: {err}")
            })
            .map_err(|err| {
                ScratchpadError::PutError(PutError::Network {
                    address: Box::new(NetworkAddress::from(*address)),
                    network_error: err.clone(),
                    payment: Some(payment_proofs),
                })
            })?;

        Ok((total_cost, *address))
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

        // prepare the record to be stored
        let net_addr = NetworkAddress::from(address);
        let record = Record {
            key: net_addr.to_record_key(),
            value: try_serialize_record(&scratchpad, RecordKind::DataOnly(DataTypes::Scratchpad))
                .map_err(|_| ScratchpadError::Serialization)?
                .to_vec(),
            publisher: None,
            expires: None,
        };

        // store the scratchpad on the network
        let target_nodes = self
            .network
            .get_closest_peers_with_retries(net_addr.clone())
            .await
            .map_err(|e| PutError::Network {
                address: Box::new(net_addr),
                network_error: e,
                payment: None,
            })?;
        debug!(
            "Updating scratchpad at address {address:?} (counter {}) to the network on nodes {target_nodes:?}",
            scratchpad.counter()
        );

        self.network
            .put_record_with_retries(record, target_nodes, &self.config.scratchpad)
            .await
            .inspect_err(|err| {
                error!("Failed to update scratchpad at address {address:?} to the network: {err}")
            })
            .map_err(|err| {
                ScratchpadError::PutError(PutError::Network {
                    address: Box::new(NetworkAddress::from(address)),
                    network_error: err,
                    payment: None,
                })
            })?;

        Ok(scratchpad)
    }

    /// Stores the provided, fully-formed `Scratchpad` as an update.
    /// Requires: (a) address exists, (b) owner signature is valid.
    /// No counter bump, encryption, or signing is performed; input is stored verbatim.
    /// Size/signature are checked via `scratchpad_verify`; network enforces versioning.
    /// Note: For caller-controlled scratchpads, scratchpad_put supports create and update; use scratchpad_put_update when you want an explicit wallet-free update (skips the payment path).
    /// The owner-key–managed path with enforced encryption/signing remains scratchpad_create / scratchpad_update.
    pub async fn scratchpad_put_update(
        &self,
        scratchpad: Scratchpad,
    ) -> Result<(), ScratchpadError> {
        // signature and size verified
        Self::scratchpad_verify(&scratchpad)?;

        let address = scratchpad.address();

        if !self.scratchpad_check_existence(address).await? {
            warn!(
                "Scratchpad at address {address:?} cannot be updated as it does not exist, please create it first or wait for it to be created"
            );
            return Err(ScratchpadError::CannotUpdateNewScratchpad);
        }

        // prepare the record to be stored
        let net_addr = scratchpad.network_address();
        let record = Record {
            key: net_addr.to_record_key(),
            value: try_serialize_record(&scratchpad, RecordKind::DataOnly(DataTypes::Scratchpad))
                .map_err(|_| ScratchpadError::Serialization)?
                .to_vec(),
            publisher: None,
            expires: None,
        };

        debug!("Getting target nodes for scratchpad at address {address:?}");

        // get the closest nodes to the data address
        let target_nodes = self
            .network
            .get_closest_peers_with_retries(net_addr.clone())
            .await
            .map_err(|e| PutError::Network {
                address: Box::new(net_addr.clone()),
                network_error: e,
                payment: None,
            })?;

        debug!(
            "Updating scratchpad at address {address:?} (counter {}) to the network on nodes {target_nodes:?}",
            scratchpad.counter()
        );

        // store the scratchpad to the target nodes
        self.network
            .put_record_with_retries(record, target_nodes, &self.config.scratchpad)
            .await
            .inspect_err(|err| {
                error!("Failed to update scratchpad at address {address:?} to the network: {err}")
            })
            .map_err(|err| {
                ScratchpadError::PutError(PutError::Network {
                    address: Box::new(net_addr),
                    network_error: err,
                    payment: None,
                })
            })?;

        Ok(())
    }

    /// Get the cost of creating a new Scratchpad
    pub async fn scratchpad_cost(&self, owner: &PublicKey) -> Result<AttoTokens, CostError> {
        info!("Getting cost for scratchpad");
        let scratch_xor = ScratchpadAddress::new(*owner).xorname();

        let store_quote = self
            .get_store_quotes(
                DataTypes::Scratchpad,
                std::iter::once((scratch_xor, SCRATCHPAD_MAX_SIZE)),
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
