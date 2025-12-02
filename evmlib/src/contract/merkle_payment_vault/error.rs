// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::common::Amount;
use crate::retry;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Contract error: {0}")]
    Contract(#[from] alloy::contract::Error),
    #[error("RPC error: {0}")]
    Rpc(String),
    #[error("Payment not found for pool hash: {0}")]
    PaymentNotFound(String),
    #[error("Merkle payments address not configured for this network")]
    MerklePaymentsAddressNotConfigured,

    // Smart contract custom errors
    #[error("Merkle tree depth {depth} exceeds maximum allowed depth {max}")]
    DepthTooLarge { depth: u8, max: u8 },
    #[error("Wrong pool count: expected {expected}, got {got}")]
    WrongPoolCount { expected: u64, got: u64 },
    #[error("Wrong candidate count in pool {pool_idx}: expected {expected}, got {got}")]
    WrongCandidateCount {
        pool_idx: u64,
        expected: u64,
        got: u64,
    },
    #[error("Insufficient token balance: have {have}, need {need}")]
    InsufficientBalance { have: Amount, need: Amount },
    #[error(
        "Insufficient token allowance for MerklePaymentVault contract: have {have}, need {need}. Please approve more tokens using wallet.approve()"
    )]
    InsufficientAllowance { have: Amount, need: Amount },
    #[error("Token transfer failed")]
    TransferFailed,
    #[error("Payment already exists for pool hash: {0}")]
    PaymentAlreadyExists(String),
    #[error(transparent)]
    Transaction(#[from] retry::TransactionError),
}
