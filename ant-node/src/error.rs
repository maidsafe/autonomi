// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_protocol::{NetworkAddress, PrettyPrintRecordKey};
use thiserror::Error;

pub(super) type Result<T, E = Error> = std::result::Result<T, E>;

/// Internal error.
#[derive(Debug, Error)]
#[allow(missing_docs)]
pub enum Error {
    #[error("Network error {0}")]
    Network(#[from] ant_networking::NetworkError),

    #[error("Protocol error {0}")]
    Protocol(#[from] ant_protocol::Error),

    #[error("Register error {0}")]
    Register(#[from] ant_registers::Error),

    #[error("Transfers Error {0}")]
    Transfers(#[from] ant_evm::EvmError),

    #[error("Failed to parse NodeEvent")]
    NodeEventParsingFailed,

    // ---------- Record Errors
    #[error("Record was not stored as no payment supplied: {0:?}")]
    InvalidPutWithoutPayment(PrettyPrintRecordKey<'static>),
    /// At this point in replication flows, payment is unimportant and should not be supplied
    #[error("Record should not be a `WithPayment` type: {0:?}")]
    UnexpectedRecordWithPayment(PrettyPrintRecordKey<'static>),
    // The Record::key must match with the one that is derived from the Record::value
    #[error("The Record::key does not match with the key derived from Record::value")]
    RecordKeyMismatch,

    // Scratchpad is old version
    #[error("A newer version of this Scratchpad already exists")]
    IgnoringOutdatedScratchpadPut,
    // Scratchpad is invalid
    #[error("Scratchpad signature is invalid over the counter + content hash")]
    InvalidScratchpadSignature,

    // ---------- Payment Errors
    #[error("The content of the payment quote is invalid")]
    InvalidQuoteContent,
    #[error("The payment quote's signature is invalid")]
    InvalidQuoteSignature,
    #[error("The payment quote expired for {0:?}")]
    QuoteExpired(NetworkAddress),

    // ---------- Miscellaneous Errors
    #[error("Failed to obtain node's current port")]
    FailedToGetNodePort,
    /// The request is invalid or the arguments of the function are invalid
    #[error("Invalid request: {0}")]
    InvalidRequest(String),
    #[error("EVM Network error: {0}")]
    EvmNetwork(String),
}
