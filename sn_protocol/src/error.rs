// Copyright 2023 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::storage::{
    registers::{EntryHash, User},
    ChunkAddress, DbcAddress, RegisterAddress,
};

use serde::{Deserialize, Serialize};
use sn_dbc::{Hash, SignedSpend};
use thiserror::Error;
use xor_name::XorName;

/// A specialised `Result` type for protocol crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Main error types for the SAFE protocol.
#[derive(Error, Clone, PartialEq, Eq, Serialize, Deserialize, custom_debug::Debug)]
#[non_exhaustive]
pub enum Error {
    /// Chunk not found.
    #[error("Chunk not found: {0:?}")]
    ChunkNotFound(ChunkAddress),
    /// We failed to store chunk
    #[error("Chunk was not stored w/ xorname {0:?}")]
    ChunkNotStored(XorName),
    /// Register not found.
    #[error("Register not found: {0:?}")]
    RegisterNotFound(RegisterAddress),
    /// Register operation was not stored.
    #[error("Register operation was not stored: {0:?}")]
    RegisterCmdNotStored(RegisterAddress),
    /// Register operation destination address mistmatch
    #[error(
        "The CRDT operation cannot be applied since the Register operation destination address ({dst_addr:?}) \
         doesn't match the targeted Register's address: {reg_addr:?}"
    )]
    RegisterAddrMismatch {
        /// Register operation destination address
        dst_addr: RegisterAddress,
        /// Targeted Register's address
        reg_addr: RegisterAddress,
    },
    /// Access denied for user
    #[error("Access denied for user: {0:?}")]
    AccessDenied(User),
    /// Entry is too big to fit inside a register
    #[error("Entry is too big to fit inside a register: {size}, max: {max}")]
    EntryTooBig {
        /// Size of the entry
        size: usize,
        /// Maximum entry size allowed
        max: usize,
    },
    /// Cannot add another entry since the register entry cap has been reached.
    #[error("Cannot add another entry since the register entry cap has been reached: {0}")]
    TooManyEntries(usize),
    /// Entry could not be found on the data
    #[error("Requested entry not found {0}")]
    NoSuchEntry(EntryHash),
    /// User entry could not be found on the data
    #[error("Requested user not found {0:?}")]
    NoSuchUser(User),
    /// Data authority provided is invalid.
    #[error("Provided PublicKey could not validate signature: {0:?}")]
    InvalidSignature(bls::PublicKey),
    /// Spend not found.
    #[error("Spend not found: {0:?}")]
    SpendNotFound(DbcAddress),
    /// Insufficient valid spends found to make it valid, less that majority of closest peers
    #[error("Insufficient valid spends found: {0:?}")]
    InsufficientValidSpendsFound(DbcAddress),
    /// Node failed to store spend
    #[error("Failed to store spend: {0:?}")]
    FailedToStoreSpend(DbcAddress),
    /// Node failed to get spend
    #[error("Failed to get spend: {0:?}")]
    FailedToGetSpend(DbcAddress),
    /// A double spend was detected.
    #[error("A double spend was detected. Two diverging signed spends: {0:?}, {1:?}")]
    DoubleSpendAttempt(Box<SignedSpend>, Box<SignedSpend>),
    /// Cannot verify a Spend signature.
    #[error("Spend signature is invalid: {0}")]
    InvalidSpendSignature(String),
    /// Cannot verify a Spend's parents.
    #[error("Spend parents are invalid: {0}")]
    InvalidSpendParents(String),

    /// One or more parent spends of a requested spend had a different dst tx hash than the signed spend src tx hash.
    #[error(
        "The signed spend src tx ({signed_src_tx_hash:?}) did not match a valid parent's dst tx hash: {parent_dst_tx_hash:?}. The trail is invalid."
    )]
    TxTrailMismatch {
        /// The signed spend src tx hash.
        signed_src_tx_hash: Hash,
        /// The dst hash of a parent signed spend.
        parent_dst_tx_hash: Hash,
    },
    /// The provided source tx did not check out when verified with all supposed inputs to it (i.e. our spends parents).
    #[error(
        "The provided source tx (with hash {provided_src_tx_hash:?}) when verified with all supposed inputs to it (i.e. our spends parents).."
    )]
    InvalidSourceTxProvided {
        /// The signed spend src tx hash.
        signed_src_tx_hash: Hash,
        /// The hash of the provided source tx.
        provided_src_tx_hash: Hash,
    },
}
