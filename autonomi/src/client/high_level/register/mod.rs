// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::client::data_types::graph::{GraphContent, GraphEntry, GraphEntryAddress, GraphError};
use crate::client::data_types::pointer::{PointerAddress, PointerError, PointerTarget};
use crate::client::key_derivation::{DerivationIndex, MainPubkey, MainSecretKey};
use crate::client::payment::PaymentOption;
use crate::client::quote::CostError;
use crate::client::Client;
use crate::AttoTokens;
use ant_networking::{GetRecordError, NetworkError};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use xor_name::XorName;

mod history;

pub use crate::{PublicKey, SecretKey};
pub use history::RegisterHistory;

/// A Register is addressed at a [`RegisterAddress`] which is in fact the owner's [`PublicKey`].
/// There can only be one register stored at [`PublicKey`].
/// Any data stored in the register is stored as is, without encryption or modifications.
/// Since the data is publicly accessible by anyone knowing the [`RegisterAddress`],
/// it is up to the owner to encrypt the data uploaded to the register, if wanted.
/// Only the owner can update the register with its [`SecretKey`].
/// The [`SecretKey`] is the only piece of information an owner should keep to access to the register.
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, Debug)]
pub struct RegisterAddress(PublicKey);

impl RegisterAddress {
    /// Create a new register address
    pub fn new(owner: PublicKey) -> Self {
        Self(owner)
    }

    /// Get the owner of the register
    pub fn owner(&self) -> PublicKey {
        self.0
    }

    /// To underlying graph representation
    pub fn to_underlying_graph_root(&self) -> GraphEntryAddress {
        GraphEntryAddress::new(self.0)
    }

    /// To underlying head pointer
    pub fn to_underlying_head_pointer(&self) -> PointerAddress {
        register_head_pointer_address(self)
    }

    /// Convert a register address to a hex string
    pub fn to_hex(&self) -> String {
        self.0.to_hex()
    }

    /// Convert a hex string to a register address
    pub fn from_hex(hex: &str) -> Result<Self, bls::Error> {
        let owner = PublicKey::from_hex(hex)?;
        Ok(Self(owner))
    }
}

impl std::fmt::Display for RegisterAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

/// The value of a register: a 32 bytes array (same as [`GraphContent`])
pub type RegisterValue = GraphContent;

/// The size of a register value: 32 bytes
pub const REGISTER_VALUE_SIZE: usize = size_of::<RegisterValue>();

#[derive(Error, Debug)]
pub enum RegisterError {
    #[error("Underlying GraphError: {0}")]
    GraphError(#[from] GraphError),
    #[error("Underlying PointerError: {0}")]
    PointerError(#[from] PointerError),
    #[error("Invalid cost")]
    InvalidCost,
    #[error("Invalid head pointer, was expecting a GraphEntryAddress but got: {0:?}")]
    InvalidHeadPointer(PointerTarget),
    #[error("Forked register, this can happen if the register has been updated concurrently, you can solve this by updating the register again with a new value. Concurrent entries: {0:?}")]
    Fork(Vec<[u8; 32]>),
    #[error("Corrupt register: {0}")]
    Corrupt(String),
    #[error("Register cannot be updated as it does not exist, please create it first or wait for it to be created")]
    CannotUpdateNewRegister,
    #[error(
        "Invalid register value length: {0}, expected something within {REGISTER_VALUE_SIZE} bytes"
    )]
    InvalidRegisterValueLength(usize),
}

/// Hard coded derivation index for the register head pointer
/// Derive the register's main public key by it to get the pointer owner/address
const REGISTER_HEAD_DERIVATION_INDEX: [u8; 32] = [0; 32];

impl Client {
    /// Create a new register key from a SecretKey and a name.
    ///
    /// This derives a new [`SecretKey`] from the owner's [`SecretKey`] using the name.
    /// Note that you will need to keep track of the names you used to create the register key.
    pub fn register_key_from_name(owner: &SecretKey, name: &str) -> SecretKey {
        let main_key = MainSecretKey::new(owner.clone());
        let derivation_index =
            DerivationIndex::from_bytes(XorName::from_content(name.as_bytes()).0);
        main_key.derive_key(&derivation_index).into()
    }

    /// Create a new [`RegisterValue`] from bytes, make sure the bytes are not longer than [`REGISTER_VALUE_SIZE`].
    pub fn register_value_from_bytes(bytes: &[u8]) -> Result<RegisterValue, RegisterError> {
        if bytes.len() > REGISTER_VALUE_SIZE {
            return Err(RegisterError::InvalidRegisterValueLength(bytes.len()));
        }
        let mut content: RegisterValue = [0; REGISTER_VALUE_SIZE];
        content[..bytes.len()].copy_from_slice(bytes);
        Ok(content)
    }

    /// Create a new register with an initial value.
    ///
    /// Note that two payments are required, one for the underlying [`GraphEntry`] and one for the [`crate::Pointer`].
    pub async fn register_create(
        &self,
        owner: &SecretKey,
        initial_value: RegisterValue,
        payment_option: PaymentOption,
    ) -> Result<(AttoTokens, RegisterAddress), RegisterError> {
        let main_key = MainSecretKey::new(owner.clone());
        let public_key = main_key.public_key();

        // create the first entry and decide on the next key
        let index = DerivationIndex::random(&mut rand::thread_rng());
        let next_key = main_key.public_key().derive_key(&index);
        let parents = vec![];
        let descendants = vec![(next_key.into(), index.into_bytes())];
        let root_entry = GraphEntry::new(
            &main_key.clone().into(),
            parents,
            initial_value,
            descendants,
        );

        // put the first entry in the graph
        let (graph_cost, addr) = self
            .graph_entry_put(root_entry, payment_option.clone())
            .await?;

        // create a Pointer to the last entry
        let target = PointerTarget::GraphEntryAddress(addr);
        let pointer_key = register_head_pointer_sk(&main_key.into());
        let (pointer_cost, _pointer_addr) = self
            .pointer_create(&pointer_key, target, payment_option.clone())
            .await?;
        let total_cost = graph_cost
            .checked_add(pointer_cost)
            .ok_or(RegisterError::InvalidCost)?;
        Ok((total_cost, RegisterAddress(public_key.into())))
    }

    /// Update the value of a register.
    ///
    /// The register needs to be created first with [`Client::register_create`].
    pub async fn register_update(
        &self,
        owner: &SecretKey,
        new_value: RegisterValue,
        payment_option: PaymentOption,
    ) -> Result<AttoTokens, RegisterError> {
        // get the pointer of the register head
        let addr = RegisterAddress(owner.public_key());
        let pointer_addr = register_head_pointer_address(&addr);
        debug!("Getting pointer of register head at {pointer_addr:?}");
        let pointer = match self.pointer_get(&pointer_addr).await {
            Ok(pointer) => pointer,
            Err(PointerError::Network(NetworkError::GetRecordError(
                GetRecordError::RecordNotFound,
            ))) => return Err(RegisterError::CannotUpdateNewRegister),
            Err(err) => return Err(err.into()),
        };
        let graph_entry_addr = match pointer.target() {
            PointerTarget::GraphEntryAddress(addr) => addr,
            other => return Err(RegisterError::InvalidHeadPointer(other.clone())),
        };

        // get the next derivation index from the current head entry
        debug!("Getting register head graph entry at {graph_entry_addr:?}");
        let (parent_entry, new_derivation) = self
            .register_get_graph_entry_and_next_derivation_index(graph_entry_addr)
            .await?;

        // create a new entry with the new value
        let main_key = MainSecretKey::new(owner.clone());
        let new_key = main_key.derive_key(&new_derivation);
        let parents = vec![parent_entry.owner];
        let next_derivation = DerivationIndex::random(&mut rand::thread_rng());
        let next_pk = main_key.public_key().derive_key(&next_derivation);
        let descendants = vec![(next_pk.into(), next_derivation.into_bytes())];
        let new_entry = GraphEntry::new(&new_key.into(), parents, new_value, descendants);

        // put the new entry in the graph
        let (cost, new_graph_entry_addr) = match self
            .graph_entry_put(new_entry, payment_option)
            .await
        {
            Ok(res) => res,
            Err(GraphError::AlreadyExists(address)) => {
                // pointer is apparently not at head, update it
                let target = PointerTarget::GraphEntryAddress(address);
                let pointer_key = register_head_pointer_sk(&main_key.into());
                self.pointer_update(&pointer_key, target).await?;
                return Err(RegisterError::Corrupt(format!(
                    "Pointer doesn't point to the register latest value, attempting to heal the register by updating it to point to the next entry at {address:?}, please retry the operation"
                )));
            }
            Err(err) => return Err(err.into()),
        };

        // update the pointer to point to the new entry
        let target = PointerTarget::GraphEntryAddress(new_graph_entry_addr);
        let pointer_key = register_head_pointer_sk(&main_key.into());
        self.pointer_update(&pointer_key, target).await?;

        Ok(cost)
    }

    /// Get the current value of the register.
    pub async fn register_get(
        &self,
        addr: &RegisterAddress,
    ) -> Result<RegisterValue, RegisterError> {
        // get the pointer of the register head
        let pointer_addr = register_head_pointer_address(addr);
        debug!("Getting pointer of register head at {pointer_addr:?}");
        let pointer = self.pointer_get(&pointer_addr).await?;
        let graph_entry_addr = match pointer.target() {
            PointerTarget::GraphEntryAddress(addr) => addr,
            other => return Err(RegisterError::InvalidHeadPointer(other.clone())),
        };

        // get the entry from the graph
        debug!("Getting register head graph entry at {graph_entry_addr:?}");
        let entry = match self.graph_entry_get(graph_entry_addr).await {
            Ok(entry) => entry,
            Err(GraphError::Fork(entries)) => {
                let values = entries.iter().map(|e| e.content).collect::<Vec<_>>();
                return Err(RegisterError::Fork(values));
            }
            Err(err) => return Err(err.into()),
        };

        // get the content of the entry
        let content = entry.content;
        Ok(content)
    }

    /// Get the cost of a register operation.
    /// Returns the cost of creation if it doesn't exist, else returns the cost of an update.
    pub async fn register_cost(&self, owner: &PublicKey) -> Result<AttoTokens, CostError> {
        let pointer_pk = register_head_pointer_pk(&RegisterAddress(*owner));
        let graph_entry_cost = self.graph_entry_cost(owner);
        let pointer_cost = self.pointer_cost(&pointer_pk);
        let (graph_entry_cost, pointer_cost) =
            futures::future::join(graph_entry_cost, pointer_cost).await;
        graph_entry_cost?
            .checked_add(pointer_cost?)
            .ok_or(CostError::InvalidCost)
    }

    /// Get underlying register graph entry and next derivation index
    /// In normal circumstances, there is only one entry with one descendant, yielding ONE entry and ONE derivation index
    /// In the case of a fork or a corrupt register, the smallest derivation index among all the entries descendants is chosen
    /// We chose here to deal with the errors instead of erroring out to allow users to solve Fork and Corrupt issues by updating the register
    async fn register_get_graph_entry_and_next_derivation_index(
        &self,
        graph_entry_addr: &GraphEntryAddress,
    ) -> Result<(GraphEntry, DerivationIndex), RegisterError> {
        let entry = match self.graph_entry_get(graph_entry_addr).await {
            Ok(e) => e,
            Err(GraphError::Fork(entries)) => {
                warn!("Forked register, multiple entries found: {entries:?}, choosing the one with the smallest derivation index for the next entry");
                let (entry_by_smallest_derivation, _) = entries
                    .into_iter()
                    .filter_map(|e| {
                        get_derivation_from_graph_entry(&e)
                            .ok()
                            .map(|derivation| (e, derivation))
                    })
                    .min_by(|a, b| a.1.cmp(&b.1))
                    .ok_or(RegisterError::Corrupt(format!(
                        "No valid descendants found for FORKED entry at {graph_entry_addr:?}"
                    )))?;
                entry_by_smallest_derivation
            }
            Err(err) => return Err(err.into()),
        };
        let new_derivation = get_derivation_from_graph_entry(&entry)?;
        Ok((entry, new_derivation))
    }
}

/// Get the address of the register's head pointer
fn register_head_pointer_address(addr: &RegisterAddress) -> PointerAddress {
    let pk: MainPubkey = addr.0.into();
    let pointer_pk = pk.derive_key(&DerivationIndex::from_bytes(REGISTER_HEAD_DERIVATION_INDEX));
    PointerAddress::new(pointer_pk.into())
}

/// Get the secret key of the register's head pointer
fn register_head_pointer_sk(register_owner: &SecretKey) -> SecretKey {
    let pointer_sk = MainSecretKey::new(register_owner.clone())
        .derive_key(&DerivationIndex::from_bytes(REGISTER_HEAD_DERIVATION_INDEX));
    pointer_sk.into()
}

/// Get the public key of the register's head pointer
fn register_head_pointer_pk(addr: &RegisterAddress) -> PublicKey {
    let pk: MainPubkey = addr.0.into();
    let pointer_pk = pk.derive_key(&DerivationIndex::from_bytes(REGISTER_HEAD_DERIVATION_INDEX));
    pointer_pk.into()
}

fn get_derivation_from_graph_entry(entry: &GraphEntry) -> Result<DerivationIndex, RegisterError> {
    let graph_entry_addr = GraphEntryAddress::new(entry.owner);
    let d = match entry.descendants.as_slice() {
        [d] => d.1,
        _ => return Err(RegisterError::Corrupt(format!(
            "Underlying Register GraphEntry at {graph_entry_addr:?} is corrupted, expected one descendant but got {}: {:?}",
            entry.descendants.len(),
            entry.descendants
        ))),
    };
    Ok(DerivationIndex::from_bytes(d))
}

mod tests {
    #[tokio::test]
    async fn test_register_by_name() {
        let main_key = bls::SecretKey::random();
        let register_key = super::Client::register_key_from_name(&main_key, "register1");
        assert_ne!(register_key.public_key(), main_key.public_key());
        let same_name = super::Client::register_key_from_name(&main_key, "register1");
        assert_eq!(same_name.public_key(), register_key.public_key());
    }

    #[tokio::test]
    async fn test_register_value_from_bytes() {
        let value = super::Client::register_value_from_bytes(&[1, 2, 3]).unwrap();
        assert_eq!(
            value,
            [
                1, 2, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0
            ]
        );
        let value = super::Client::register_value_from_bytes(&[
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
            25, 26, 27, 28, 29, 30, 31, 32,
        ])
        .unwrap();
        assert_eq!(
            value,
            [
                1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
                24, 25, 26, 27, 28, 29, 30, 31, 32
            ]
        );
        let err = super::Client::register_value_from_bytes(&[
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
            25, 26, 27, 28, 29, 30, 31, 32, 33,
        ])
        .unwrap_err();
        assert!(matches!(err, super::RegisterError::InvalidRegisterValueLength(v) if v == 33));
    }
}
