// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{EvmError, Result};

pub use evmlib::common::Amount;
use serde::{Deserialize, Serialize};
use std::{
    fmt::{self, Display, Formatter},
    str::FromStr,
};

/// The conversion from AttoTokens to raw value
const TOKEN_TO_RAW_POWER_OF_10_CONVERSION: u64 = 18;
/// The conversion from AttoTokens to raw value
const TOKEN_TO_RAW_CONVERSION: u64 = 1_000_000_000_000_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
/// An amount in SNT Atto. 10^18 Nanos = 1 SNT.
pub struct AttoTokens(Amount);

impl AttoTokens {
    /// Type safe representation of zero AttoTokens.
    pub const fn zero() -> Self {
        Self(Amount::ZERO)
    }

    /// Returns whether it's a representation of zero AttoTokens.
    pub fn is_zero(&self) -> bool {
        self.0.is_zero()
    }

    /// New value from an amount
    pub fn from_atto(value: Amount) -> Self {
        Self(value)
    }

    /// New value from a number of atto tokens.
    pub fn from_u64(value: u64) -> Self {
        Self(Amount::from(value))
    }

    /// New value from a number of atto tokens.
    pub fn from_u128(value: u128) -> Self {
        Self(Amount::from(value))
    }

    /// Total AttoTokens expressed in number of nano tokens.
    pub fn as_atto(self) -> Amount {
        self.0
    }

    /// Computes `self + rhs`, returning `None` if overflow occurred.
    pub fn checked_add(self, rhs: AttoTokens) -> Option<AttoTokens> {
        self.0.checked_add(rhs.0).map(Self::from_atto)
    }

    /// Computes `self - rhs`, returning `None` if overflow occurred.
    pub fn checked_sub(self, rhs: AttoTokens) -> Option<AttoTokens> {
        self.0.checked_sub(rhs.0).map(Self::from_atto)
    }

    /// Converts the Nanos into bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        self.0.as_le_bytes().to_vec()
    }
}

impl From<u64> for AttoTokens {
    fn from(value: u64) -> Self {
        Self(Amount::from(value))
    }
}

impl From<Amount> for AttoTokens {
    fn from(value: Amount) -> Self {
        Self(value)
    }
}

impl FromStr for AttoTokens {
    type Err = EvmError;

    fn from_str(value_str: &str) -> Result<Self> {
        let mut itr = value_str.splitn(2, '.');
        let converted_units = {
            let units = itr
                .next()
                .and_then(|s| s.parse::<Amount>().ok())
                .ok_or_else(|| {
                    EvmError::FailedToParseAttoToken("Can't parse token units".to_string())
                })?;

            // Check if the units part is too large before multiplication
            if units > Amount::from(u64::MAX) {
                return Err(EvmError::ExcessiveValue);
            }

            units
                .checked_mul(Amount::from(TOKEN_TO_RAW_CONVERSION))
                .ok_or(EvmError::ExcessiveValue)?
        };

        let remainder = {
            let remainder_str = itr.next().unwrap_or_default().trim_end_matches('0');

            if remainder_str.is_empty() {
                Amount::ZERO
            } else {
                let parsed_remainder = remainder_str.parse::<Amount>().map_err(|_| {
                    EvmError::FailedToParseAttoToken("Can't parse token remainder".to_string())
                })?;

                let remainder_conversion = TOKEN_TO_RAW_POWER_OF_10_CONVERSION
                    .checked_sub(remainder_str.len() as u64)
                    .ok_or(EvmError::LossOfPrecision)?;
                if remainder_conversion > 32 {
                    return Err(EvmError::LossOfPrecision);
                }
                parsed_remainder * Amount::from(10).pow(Amount::from(remainder_conversion))
            }
        };

        Ok(Self(converted_units + remainder))
    }
}

impl Display for AttoTokens {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        let unit = self.0 / Amount::from(TOKEN_TO_RAW_CONVERSION);
        let remainder = self.0 % Amount::from(TOKEN_TO_RAW_CONVERSION);
        write!(formatter, "{unit}.{remainder:018}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_str() -> Result<()> {
        assert_eq!(AttoTokens::from_u64(0), AttoTokens::from_str("0")?);
        assert_eq!(AttoTokens::from_u64(0), AttoTokens::from_str("0.")?);
        assert_eq!(AttoTokens::from_u64(0), AttoTokens::from_str("0.0")?);
        assert_eq!(
            AttoTokens::from_u64(1),
            AttoTokens::from_str("0.000000000000000001")?
        );
        assert_eq!(
            AttoTokens::from_u64(1_000_000_000_000_000_000),
            AttoTokens::from_str("1")?
        );
        assert_eq!(
            AttoTokens::from_u64(1_000_000_000_000_000_000),
            AttoTokens::from_str("1.")?
        );
        assert_eq!(
            AttoTokens::from_u64(1_000_000_000_000_000_000),
            AttoTokens::from_str("1.0")?
        );
        assert_eq!(
            AttoTokens::from_u64(1_000_000_000_000_000_001),
            AttoTokens::from_str("1.000000000000000001")?
        );
        assert_eq!(
            AttoTokens::from_u64(1_100_000_000_000_000_000),
            AttoTokens::from_str("1.1")?
        );
        assert_eq!(
            AttoTokens::from_u64(1_100_000_000_000_000_001),
            AttoTokens::from_str("1.100000000000000001")?
        );
        assert_eq!(
            AttoTokens::from_u128(4_294_967_295_000_000_000_000_000u128),
            AttoTokens::from_str("4294967.295")?
        );
        assert_eq!(
            AttoTokens::from_u128(4_294_967_295_999_999_999_000_000u128),
            AttoTokens::from_str("4294967.295999999999")?,
        );
        assert_eq!(
            AttoTokens::from_u128(4_294_967_295_999_999_999_000_000u128),
            AttoTokens::from_str("4294967.2959999999990000")?,
        );
        assert_eq!(
            AttoTokens::from_u128(18_446_744_074_000_000_000_000_000_000u128),
            AttoTokens::from_str("18446744074")?
        );

        assert_eq!(
            Err(EvmError::FailedToParseAttoToken(
                "Can't parse token units".to_string()
            )),
            AttoTokens::from_str("a")
        );
        assert_eq!(
            Err(EvmError::FailedToParseAttoToken(
                "Can't parse token remainder".to_string()
            )),
            AttoTokens::from_str("0.a")
        );
        assert_eq!(
            Err(EvmError::FailedToParseAttoToken(
                "Can't parse token remainder".to_string()
            )),
            AttoTokens::from_str("0.0.0")
        );
        assert_eq!(
            Err(EvmError::LossOfPrecision),
            AttoTokens::from_str("0.0000000000000000001")
        );
        assert_eq!(
            Err(EvmError::ExcessiveValue),
            AttoTokens::from_str("340282366920938463463374607431768211455")
        );
        Ok(())
    }

    #[test]
    fn display() {
        assert_eq!(
            "0.000000000000000000",
            format!("{}", AttoTokens::from_u64(0))
        );
        assert_eq!(
            "0.000000000000000001",
            format!("{}", AttoTokens::from_u64(1))
        );
        assert_eq!(
            "0.000000000000000010",
            format!("{}", AttoTokens::from_u64(10))
        );
        assert_eq!(
            "1.000000000000000000",
            format!("{}", AttoTokens::from_u64(1_000_000_000_000_000_000))
        );
        assert_eq!(
            "1.000000000000000001",
            format!("{}", AttoTokens::from_u64(1_000_000_000_000_000_001))
        );
        assert_eq!(
            "4.294967295000000000",
            format!("{}", AttoTokens::from_u64(4_294_967_295_000_000_000))
        );
    }

    #[test]
    fn checked_add_sub() {
        assert_eq!(
            Some(AttoTokens::from_u64(3)),
            AttoTokens::from_u64(1).checked_add(AttoTokens::from_u64(2))
        );
        assert_eq!(
            Some(AttoTokens::from_u128(u64::MAX as u128 + 1)),
            AttoTokens::from_u64(u64::MAX).checked_add(AttoTokens::from_u64(1))
        );
        assert_eq!(
            Some(AttoTokens::from_u128(u64::MAX as u128 * 2)),
            AttoTokens::from_u64(u64::MAX).checked_add(AttoTokens::from_u64(u64::MAX))
        );

        assert_eq!(
            Some(AttoTokens::from_u64(0)),
            AttoTokens::from_u64(u64::MAX).checked_sub(AttoTokens::from_u64(u64::MAX))
        );
        assert_eq!(
            None,
            AttoTokens::from_u64(0).checked_sub(AttoTokens::from_u64(1))
        );
    }
}
