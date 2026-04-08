// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::resolve_records_from_peers;

use crate::{
    client::{
        Client, GetError, PutError,
        payment::{PayError, PaymentOption},
        quote::CostError,
    },
    networking::{NetworkError, PeerInfo, QUOTING_CANDIDATES, Record},
};
use futures::StreamExt;

/// How many peers to query when looking for the latest pointer version
const FALLBACK_PEERS_COUNT: usize = 20;

use ant_evm::{AttoTokens, EvmWalletError};
use ant_protocol::{
    NetworkAddress,
    storage::{DataTypes, RecordHeader, RecordKind, try_deserialize_record, try_serialize_record},
};
use std::collections::HashSet;
use tracing::{debug, error, info, trace, warn};

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
    Pay(#[from] PayError),
    #[error("Failed to retrieve wallet payment")]
    Wallet(#[from] EvmWalletError),
    #[error("Received invalid quote from node")]
    InvalidQuote,
    #[error("Pointer already exists at this address: {0:?}")]
    PointerAlreadyExists(PointerAddress),
    #[error("Pointer cannot be updated as it does not exist")]
    CannotUpdateNewPointer,
    #[error("Got multiple conflicting pointers with the latest version")]
    Fork(Vec<Pointer>),
}

impl Client {
    /// Get a pointer from the network.
    /// Always queries multiple nodes and picks the one with highest counter.
    pub async fn pointer_get(&self, address: &PointerAddress) -> Result<Pointer, PointerError> {
        let key = NetworkAddress::from(*address);
        debug!("Fetching pointer at: {key:?}");

        let records = self.fetch_all_pointer_records(&key).await?;
        if records.is_empty() {
            return Err(PointerError::GetError(GetError::RecordNotFound));
        }

        debug!("Got {} record(s) for pointer", records.len());

        // Pick the one with highest counter
        let pointer = resolve_records_from_peers(
            records,
            key.clone(),
            pointer_from_record,
            |p: &Pointer| p.counter(),
            |a: &Pointer, b: &Pointer| a == b,
            |multiples: HashSet<Pointer>| PointerError::Fork(multiples.into_iter().collect()),
            || PointerError::Corrupt(format!("Invalid pointer records at {key:?}")),
        )?;

        info!("Got pointer: {pointer:?}");
        Self::pointer_verify(&pointer)?;
        Ok(pointer)
    }

    /// Fetch pointer records from multiple nodes to find the latest version.
    async fn fetch_all_pointer_records(
        &self,
        key: &NetworkAddress,
    ) -> Result<Vec<Record>, PointerError> {
        // Start with normal DHT query
        let initial = match self
            .network
            .get_record_with_retries(key.clone(), &self.config.pointer)
            .await
        {
            Ok(Some(records)) => records,
            Ok(None) => return Err(PointerError::GetError(GetError::RecordNotFound)),
            Err(NetworkError::SplitRecord(map)) => map.into_values().collect(),
            Err(err) => {
                error!("Error fetching pointer: {err:?}");
                return Err(PointerError::GetError(err.into()));
            }
        };

        // Query more peers to make sure we have the latest
        self.query_additional_peers(key, initial).await
    }

    /// Ask more peers for their pointer records, merge with what we have.
    async fn query_additional_peers(
        &self,
        key: &NetworkAddress,
        initial: Vec<Record>,
    ) -> Result<Vec<Record>, PointerError> {
        let max_counter = initial
            .iter()
            .filter_map(|r| pointer_from_record(r.clone()).ok())
            .map(|p| p.counter())
            .max()
            .unwrap_or(0);

        let peers = match self
            .network
            .get_closest_peers(key.clone(), Some(FALLBACK_PEERS_COUNT))
            .await
        {
            Ok(p) => p,
            Err(e) => {
                warn!("Couldn't get peers for fallback query: {e}");
                return Ok(initial);
            }
        };

        debug!("Querying {} extra peers for pointer", peers.len());

        // Ask each peer in parallel
        let network = self.network.clone();
        let queries: Vec<_> = peers
            .into_iter()
            .map(|peer| {
                let network = network.clone();
                let key = key.clone();
                async move { network.get_record_from_peer(key, peer).await }
            })
            .collect();

        let results: Vec<_> = futures::stream::iter(queries)
            .buffer_unordered(FALLBACK_PEERS_COUNT)
            .collect()
            .await;

        // Merge all records
        let mut all = initial;
        for res in results {
            if let Ok(Some(rec)) = res {
                all.push(rec);
            }
        }

        // Keep only records with counter >= max_counter (or everything if nothing valid)
        let filtered: Vec<Record> = all
            .iter()
            .filter(|r| {
                pointer_from_record((*r).clone())
                    .map(|p| p.counter() >= max_counter || max_counter == 0)
                    .unwrap_or(false)
            })
            .cloned()
            .collect();

        if filtered.is_empty() {
            Ok(all)
        } else {
            Ok(filtered)
        }
    }

    pub async fn pointer_check_existence(
        &self,
        address: &PointerAddress,
    ) -> Result<bool, PointerError> {
        let key = NetworkAddress::from(*address);
        match self
            .network
            .get_record(key.clone(), self.config.pointer.verification_quorum)
            .await
        {
            Ok(None) => Ok(false),
            Ok(Some(_)) => Ok(true),
            Err(NetworkError::SplitRecord(..)) => Ok(true),
            Err(err) => Err(PointerError::GetError(GetError::Network(err))),
        }
    }

    pub fn pointer_verify(pointer: &Pointer) -> Result<(), PointerError> {
        if !pointer.verify_signature() {
            return Err(PointerError::BadSignature);
        }
        Ok(())
    }

    /// Store a pointer on the network.
    ///
    /// Don't use this for updates! Use `pointer_create()` first, then `pointer_update()`.
    /// Using `pointer_put()` on an existing pointer can cause problems.
    pub async fn pointer_put(
        &self,
        pointer: Pointer,
        payment_option: PaymentOption,
    ) -> Result<(AttoTokens, PointerAddress), PointerError> {
        let address = pointer.address();
        let xor_name = address.xorname();

        debug!("Paying for pointer at {address:?}");
        let (payment_proofs, _) = self
            .pay_for_content_addrs(
                DataTypes::Pointer,
                std::iter::once((xor_name, Pointer::size())),
                payment_option,
            )
            .await
            .map_err(|err| {
                error!("Payment failed for {address:?}: {err}");
                PointerError::Pay(err)
            })?;

        let (proof, price) = match payment_proofs.get(&xor_name) {
            Some((p, pr)) => (Some(p.clone()), *pr),
            None => {
                info!("Pointer already paid for at {address:?}");
                (None, AttoTokens::zero())
            }
        };

        let (record, targets) = if let Some(proof) = proof {
            let payees = proof
                .payees()
                .iter()
                .map(|(id, addrs)| PeerInfo {
                    peer_id: *id,
                    addrs: addrs.clone(),
                })
                .collect();
            let record = Record {
                key: NetworkAddress::from(address).to_record_key(),
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
            let net_addr = NetworkAddress::from(address);
            let record = Record {
                key: net_addr.to_record_key(),
                value: try_serialize_record(&pointer, RecordKind::DataOnly(DataTypes::Pointer))
                    .map_err(|_| PointerError::Serialization)?
                    .to_vec(),
                publisher: None,
                expires: None,
            };
            let targets = self
                .network
                .get_closest_peers_with_retries(net_addr.clone(), Some(QUOTING_CANDIDATES))
                .await
                .map_err(|e| PutError::Network {
                    address: Box::new(net_addr),
                    network_error: e,
                    payment: None,
                })?;
            (record, targets)
        };

        debug!("Storing pointer at {address:?}");
        self.network
            .put_record_with_retries(record, targets, &self.config.pointer)
            .await
            .map_err(|err| {
                PointerError::PutError(PutError::Network {
                    address: Box::new(NetworkAddress::from(address)),
                    network_error: err,
                    payment: Some(payment_proofs),
                })
            })?;

        Ok((price, address))
    }

    pub async fn pointer_create(
        &self,
        owner: &SecretKey,
        target: PointerTarget,
        payment_option: PaymentOption,
    ) -> Result<(AttoTokens, PointerAddress), PointerError> {
        let address = PointerAddress::new(owner.public_key());
        if self.pointer_check_existence(&address).await? {
            return Err(PointerError::PointerAlreadyExists(address));
        }
        let pointer = Pointer::new(owner, 0, target);
        self.pointer_put(pointer, payment_option).await
    }

    pub async fn pointer_update(
        &self,
        owner: &SecretKey,
        target: PointerTarget,
    ) -> Result<(), PointerError> {
        let address = PointerAddress::new(owner.public_key());
        info!("Updating pointer {address:?} to {target:?}");

        let current = match self.pointer_get(&address).await {
            Ok(p) => Some(p),
            Err(PointerError::GetError(GetError::Network(NetworkError::SplitRecord(map)))) => map
                .values()
                .filter_map(|r| try_deserialize_record::<Pointer>(r).ok())
                .max_by_key(|p| p.counter()),
            Err(err) => return Err(err),
        };

        match current {
            Some(p) => {
                self.pointer_update_from(&p, owner, target).await?;
                Ok(())
            }
            None => {
                warn!("Pointer {address:?} doesn't exist");
                Err(PointerError::CannotUpdateNewPointer)
            }
        }
    }

    pub async fn pointer_update_from(
        &self,
        current: &Pointer,
        owner: &SecretKey,
        target: PointerTarget,
    ) -> Result<Pointer, PointerError> {
        let address = PointerAddress::new(owner.public_key());
        let new_counter = current.counter() + 1;
        info!("Updating pointer {address:?} to v{new_counter}");

        let pointer = Pointer::new(owner, new_counter, target);
        let net_addr = NetworkAddress::from(address);
        let record = Record {
            key: net_addr.to_record_key(),
            value: try_serialize_record(&pointer, RecordKind::DataOnly(DataTypes::Pointer))
                .map_err(|_| PointerError::Serialization)?
                .to_vec(),
            publisher: None,
            expires: None,
        };

        let targets = self
            .network
            .get_closest_peers_with_retries(net_addr.clone(), Some(QUOTING_CANDIDATES))
            .await
            .map_err(|e| PutError::Network {
                address: Box::new(net_addr),
                network_error: e,
                payment: None,
            })?;

        debug!("Storing updated pointer");
        self.network
            .put_record_with_retries(record, targets, &self.config.pointer)
            .await
            .map_err(|err| {
                PointerError::PutError(PutError::Network {
                    address: Box::new(NetworkAddress::from(address)),
                    network_error: err,
                    payment: None,
                })
            })?;

        Ok(pointer)
    }

    pub async fn pointer_cost(&self, key: &PublicKey) -> Result<AttoTokens, CostError> {
        trace!("Pointer cost for {key:?}");
        let addr = PointerAddress::new(*key);
        let xor = addr.xorname();
        let quotes = self
            .get_store_quotes(DataTypes::Pointer, std::iter::once((xor, Pointer::size())))
            .await?;
        let cost = AttoTokens::from_atto(quotes.0.values().map(|q| q.price()).sum());
        debug!("Pointer cost: {cost}");
        Ok(cost)
    }
}

fn pointer_from_record(record: Record) -> Result<Pointer, PointerError> {
    let key = &record.key;
    let header = RecordHeader::from_record(&record)
        .map_err(|e| PointerError::Corrupt(format!("Bad header at {key:?}: {e:?}")))?;

    if !matches!(header.kind, RecordKind::DataOnly(DataTypes::Pointer)) {
        error!("Wrong record type: {:?}", header.kind);
        return Err(GetError::RecordKindMismatch(RecordKind::DataOnly(DataTypes::Pointer)).into());
    }

    try_deserialize_record(&record)
        .map_err(|e| PointerError::Corrupt(format!("Bad pointer at {key:?}: {e:?}")))
}
