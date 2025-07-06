// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::AttoTokens;
use thiserror::Error;

/// Specialisation of `std::Result`.
pub type Result<T, E = EvmError> = std::result::Result<T, E>;

#[allow(clippy::large_enum_variant)]
#[derive(Error, Debug, Clone, PartialEq)]
#[non_exhaustive]
/// Transfer errors
pub enum EvmError {
    #[error("Lost precision on the number of coins during parsing.")]
    LossOfPrecision,
    #[error("The token amount would exceed the maximum value")]
    ExcessiveValue,
    #[error("Failed to parse: {0}")]
    FailedToParseAttoToken(String),
    #[error("Overflow occurred while adding values")]
    NumericOverflow,
    #[error("Not enough balance, {0} available, {1} required")]
    NotEnoughBalance(AttoTokens, AttoTokens),
    #[error("Invalid quote public key")]
    InvalidQuotePublicKey,
}
