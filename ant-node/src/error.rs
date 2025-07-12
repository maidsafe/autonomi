// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_protocol::PrettyPrintRecordKey;
use libp2p::PeerId;
use thiserror::Error;

pub(super) type Result<T, E = Error> = std::result::Result<T, E>;

const SCRATCHPAD_MAX_SIZE: usize = ant_protocol::storage::Scratchpad::MAX_SIZE;

/// Put validation errors.
#[derive(Debug, Error)]
#[allow(missing_docs)]
pub enum PutValidationError {
    #[error("Error while requesting data from the local swarm")]
    LocalSwarmError,

    #[error("The record header cannot be deserialized")]
    InvalidRecordHeader,

    #[error("The record cannot be deserialized to the expected type")]
    InvalidRecord(PrettyPrintRecordKey<'static>),

    #[error("The Record::key does not match with the key derived from Record::value")]
    RecordKeyMismatch,

    #[error("Failed to serialize the record")]
    RecordSerializationFailed(PrettyPrintRecordKey<'static>),

    // ---------- Payment errors
    #[error("The record did not contain any payments: {0:?}")]
    NoPayment(PrettyPrintRecordKey<'static>),

    /// At this point in replication flows, payment is unimportant and should not be supplied
    #[error("Record should not be a `WithPayment` type: {0:?}")]
    UnexpectedRecordWithPayment(PrettyPrintRecordKey<'static>),

    #[error("Our node did not receive any payment for record: {0:?}")]
    PaymentNotMadeToOurNode(PrettyPrintRecordKey<'static>),

    #[error("The payment was made to an incorrect data type: {0:?}")]
    PaymentMadeToIncorrectDataType(PrettyPrintRecordKey<'static>),

    #[error(
        "The payment quote has out of range payees for record: {record_key:?}. Payees: {payees:?}"
    )]
    PaymentQuoteOutOfRange {
        record_key: PrettyPrintRecordKey<'static>,
        payees: Vec<PeerId>,
    },

    #[error(
        "Failed to verify payment with EVM network for record: {record_key:?}. Error: {error}"
    )]
    PaymentVerificationFailed {
        record_key: PrettyPrintRecordKey<'static>,
        error: ant_evm::payment_vault::error::Error,
    },

    // ---------- Chunk errors
    #[error("Chunk is too large: {0} bytes, when max size is {1} bytes")]
    OversizedChunk(usize, usize),

    // ------------ Mutable data errors
    #[error("Rejected outdated record: with counter {counter}, expected any above {expected}")]
    OutdatedRecordCounter { counter: u64, expected: u64 },

    #[error("Scratchpad signature is invalid")]
    InvalidScratchpadSignature,

    #[error("Scratchpad too big: {0}, max size is {SCRATCHPAD_MAX_SIZE}")]
    ScratchpadTooBig(usize),

    // ---------- GraphEntry errors
    #[error("There are no GraphEntries in the record: {0:?}")]
    EmptyGraphEntry(PrettyPrintRecordKey<'static>),

    // ---------- Pointer errors
    #[error("Pointer signature is invalid")]
    InvalidPointerSignature,
}

/// Internal node error.
#[derive(Debug, Error)]
#[allow(missing_docs)]
pub enum Error {
    #[error("Network error {0}")]
    Network(#[from] crate::networking::NetworkError),

    #[error("Failed to parse NodeEvent")]
    NodeEventParsingFailed,

    #[error("Failed to obtain node's current port")]
    FailedToGetNodePort,

    // ---------- Quote Errors
    #[error("The content of the payment quote is invalid")]
    InvalidQuoteContent,

    #[error("The payment quote's signature is invalid")]
    InvalidQuoteSignature,
    #[error("The node is not externally reachable by the network")]
    UnreachableNode,
}
