// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{NetworkAddress, PrettyPrintRecordKey};
use libp2p::kad::store;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A specialised `Result` type for protocol crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Main error types for the SAFE protocol.
#[derive(Error, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Error {
    // ---------- Misc errors
    #[error("Could not obtain data dir")]
    CouldNotObtainDataDir,

    // ---------- Chunk Proof errors
    #[error("Chunk does not exist {0:?}")]
    ChunkDoesNotExist(NetworkAddress),

    // ---------- Scratchpad errors
    /// The provided SecretyKey failed to decrypt the data
    #[error("Failed to derive CipherText from encrypted_data")]
    ScratchpadCipherTextFailed,
    /// The provided cypher text is invalid
    #[error("Provided cypher text is invalid")]
    ScratchpadCipherTextInvalid,

    // ---------- Record Put errors
    #[error("Error handling record put: {0}")]
    PutRecordFailed(String),
    #[error("Outdated record: with counter {counter}, expected any above {expected}")]
    OutdatedRecordCounter { counter: u64, expected: u64 },

    // ---------- payment errors
    #[error("There was an error getting the storecost from kademlia store")]
    GetStoreQuoteFailed,
    #[error("There was an error generating the payment quote")]
    QuoteGenerationFailed,

    // ---------- replication errors
    /// Replication not found.
    #[error("Peer {holder:?} cannot find Record {key:?}")]
    ReplicatedRecordNotFound {
        /// Holder that being contacted
        holder: Box<NetworkAddress>,
        /// Key of the missing record
        key: Box<NetworkAddress>,
    },

    // ---------- record errors
    // Could not Serialize/Deserialize RecordHeader from Record
    #[error("Could not Serialize/Deserialize RecordHeader to/from Record")]
    RecordHeaderParsingFailed,
    // Could not Serialize/Deserialize Record
    #[error("Could not Serialize/Deserialize Record")]
    RecordParsingFailed,
    // The record already exists at this node
    #[error("The record already exists, so do not charge for it: {0:?}")]
    RecordExists(PrettyPrintRecordKey<'static>),
}

impl From<Error> for store::Error {
    fn from(_err: Error) -> Self {
        store::Error::ValueTooLarge
    }
}

impl From<store::Error> for Error {
    fn from(_err: store::Error) -> Self {
        Error::RecordParsingFailed
    }
}
