// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

#[macro_use]
extern crate tracing;

pub use evmlib::common::Address as RewardsAddress;
pub use evmlib::common::Address as EvmAddress;
pub use evmlib::common::QuoteHash;
pub use evmlib::common::QuotePayment;
pub use evmlib::common::TxHash;
pub use evmlib::common::U256;
pub use evmlib::contract::payment_vault;
pub use evmlib::cryptography;
#[cfg(feature = "external-signer")]
pub use evmlib::external_signer;
pub use evmlib::transaction_config::MaxFeePerGas;
pub use evmlib::transaction_config::TransactionConfig;
pub use evmlib::utils;
pub use evmlib::utils::get_evm_network;
pub use evmlib::utils::DATA_PAYMENTS_ADDRESS;
pub use evmlib::utils::PAYMENT_TOKEN_ADDRESS;
pub use evmlib::utils::RPC_URL;
pub use evmlib::wallet::Error as EvmWalletError;
pub use evmlib::wallet::Wallet as EvmWallet;
pub use evmlib::CustomNetwork;
pub use evmlib::Network as EvmNetwork;

mod amount;
mod data_payments;
mod error;

pub use data_payments::ClientProofOfPayment;
pub use data_payments::EncodedPeerId;
pub use data_payments::PaymentQuote;
pub use data_payments::ProofOfPayment;
pub use evmlib::quoting_metrics::QuotingMetrics;

/// Types used in the public API
pub use amount::Amount;
/// Types used in the public API
pub use amount::AttoTokens;
pub use error::EvmError;
pub use error::Result;
