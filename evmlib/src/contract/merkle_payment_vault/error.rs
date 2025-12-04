// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::contract::merkle_payment_vault::interface::IMerklePaymentVault;
use crate::retry;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Contract error: {0}")]
    Contract(#[from] alloy::contract::Error),
    #[error("RPC error: {0}")]
    Rpc(String),
    #[error("Merkle payments address not configured for this network")]
    MerklePaymentsAddressNotConfigured,
    #[error(transparent)]
    Transaction(#[from] retry::TransactionError),

    // Smart contract custom errors (from IMerklePaymentVault.json)
    #[error("ANT token address is null")]
    AntTokenNull,
    #[error("Batch limit exceeded")]
    BatchLimitExceeded,
    #[error("Merkle tree depth {depth} exceeds maximum allowed depth {max}")]
    DepthTooLarge { depth: u8, max: u8 },
    #[error("Grace period not over")]
    GracePeriodNotOver,
    #[error("Invalid amount")]
    InvalidAmount,
    #[error("Invalid Chainlink price")]
    InvalidChainlinkPrice,
    #[error("Invalid input length")]
    InvalidInputLength,
    #[error("Invalid quote hash")]
    InvalidQuoteHash,
    #[error("Invalid recipients count")]
    InvalidRecipientsCount,
    #[error("Invalid root")]
    InvalidRoot,
    #[error("Invalid tree depth")]
    InvalidTreeDepth,
    #[error("Payment already exists for pool hash: {0}")]
    PaymentAlreadyExists(String),
    #[error("Payment not found for pool hash: {0}")]
    PaymentNotFound(String),
    #[error("Price feed address is null")]
    PriceFeedNull,
    #[error("Root already paid")]
    RootAlreadyPaid,
    #[error("Sequencer is down")]
    SequencerDown,
    #[error("Wrong candidate count in pool {pool_idx}: expected {expected}, got {got}")]
    WrongCandidateCount {
        pool_idx: u64,
        expected: u64,
        got: u64,
    },
    #[error("Wrong pool count: expected {expected}, got {got}")]
    WrongPoolCount { expected: u64, got: u64 },
}

impl Error {
    /// Try to decode a contract error from revert data
    pub(crate) fn try_decode_revert(data: &[u8]) -> Option<Self> {
        use alloy::sol_types::SolInterface;

        // The revert data should start with the 4-byte selector followed by the error data
        if data.len() < 4 {
            return None;
        }

        let selector: [u8; 4] = data[..4].try_into().ok()?;
        let error_data = &data[4..];

        // Try to decode as IMerklePaymentVaultErrors
        if let Ok(contract_error) =
            IMerklePaymentVault::IMerklePaymentVaultErrors::abi_decode_raw(selector, error_data)
        {
            return Some(Self::from_contract_error(contract_error));
        }

        None
    }

    /// Convert a decoded contract error to our Error type
    fn from_contract_error(error: IMerklePaymentVault::IMerklePaymentVaultErrors) -> Self {
        use IMerklePaymentVault::IMerklePaymentVaultErrors;

        match error {
            IMerklePaymentVaultErrors::AntTokenNull(_) => Self::AntTokenNull,
            IMerklePaymentVaultErrors::BatchLimitExceeded(_) => Self::BatchLimitExceeded,
            IMerklePaymentVaultErrors::DepthTooLarge(e) => Self::DepthTooLarge {
                depth: e.depth,
                max: e.max,
            },
            IMerklePaymentVaultErrors::GracePeriodNotOver(_) => Self::GracePeriodNotOver,
            IMerklePaymentVaultErrors::InvalidAmount(_) => Self::InvalidAmount,
            IMerklePaymentVaultErrors::InvalidChainlinkPrice(_) => Self::InvalidChainlinkPrice,
            IMerklePaymentVaultErrors::InvalidInputLength(_) => Self::InvalidInputLength,
            IMerklePaymentVaultErrors::InvalidQuoteHash(_) => Self::InvalidQuoteHash,
            IMerklePaymentVaultErrors::InvalidRecipientsCount(_) => Self::InvalidRecipientsCount,
            IMerklePaymentVaultErrors::InvalidRoot(_) => Self::InvalidRoot,
            IMerklePaymentVaultErrors::InvalidTreeDepth(_) => Self::InvalidTreeDepth,
            IMerklePaymentVaultErrors::PaymentAlreadyExists(e) => {
                Self::PaymentAlreadyExists(hex::encode(e.poolHash))
            }
            IMerklePaymentVaultErrors::PaymentNotFound(e) => {
                Self::PaymentNotFound(hex::encode(e.poolHash))
            }
            IMerklePaymentVaultErrors::PriceFeedNull(_) => Self::PriceFeedNull,
            IMerklePaymentVaultErrors::RootAlreadyPaid(_) => Self::RootAlreadyPaid,
            IMerklePaymentVaultErrors::SequencerDown(_) => Self::SequencerDown,
            IMerklePaymentVaultErrors::WrongCandidateCount(e) => Self::WrongCandidateCount {
                pool_idx: e.poolIdx.try_into().unwrap_or(u64::MAX),
                expected: e.expected.try_into().unwrap_or(u64::MAX),
                got: e.got.try_into().unwrap_or(u64::MAX),
            },
            IMerklePaymentVaultErrors::WrongPoolCount(e) => Self::WrongPoolCount {
                expected: e.expected.try_into().unwrap_or(u64::MAX),
                got: e.got.try_into().unwrap_or(u64::MAX),
            },
        }
    }
}
