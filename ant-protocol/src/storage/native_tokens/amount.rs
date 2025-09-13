// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use serde::{Deserialize, Serialize};
use std::{
    fmt::{self, Display, Formatter},
    ops::{Add, Sub},
};

/// Native token amount type for the Autonomi Network.
/// 
/// Uses u96 to match the 12-byte amount field in GraphEntry descendants content.
/// This provides sufficient range (2^96 ≈ 7.9 × 10^28) while maintaining compact storage.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
pub struct NativeTokens(pub u128);

impl NativeTokens {
    /// Maximum value for a u96 integer (2^96 - 1)
    pub const MAX: u128 = (1u128 << 96) - 1;
    
    /// Zero tokens
    pub const ZERO: Self = Self(0);
    
    /// Creates a new NativeTokens instance from a u128 value.
    /// 
    /// # Arguments
    /// * `value` - The token amount, must be <= u96::MAX
    /// 
    /// # Panics
    /// Panics if the value exceeds u96::MAX
    pub fn new(value: u128) -> Self {
        assert!(value <= Self::MAX, "Value exceeds u96 maximum");
        Self(value)
    }
    
    /// Creates a new NativeTokens instance from a u64 value.
    pub fn from_u64(value: u64) -> Self {
        Self(value as u128)
    }
    
    /// Creates a new NativeTokens instance from a u96 represented as bytes.
    /// 
    /// # Arguments
    /// * `bytes` - Little-endian byte array of length 12
    /// 
    /// # Returns
    /// * `Ok(NativeTokens)` if successful
    /// * `Err(String)` if byte array length is invalid
    pub fn from_le_bytes(bytes: [u8; 12]) -> Result<Self, String> {
        // Convert 12 bytes to u128, padding with zeros
        let mut padded_bytes = [0u8; 16];
        padded_bytes[..12].copy_from_slice(&bytes);
        let value = u128::from_le_bytes(padded_bytes);
        
        // Verify it's within u96 range
        if value > Self::MAX {
            return Err("Value exceeds u96 maximum".to_string());
        }
        
        Ok(Self(value))
    }
    
    /// Converts the token amount to a 12-byte little-endian array (u96).
    pub fn to_le_bytes(&self) -> [u8; 12] {
        let bytes = self.0.to_le_bytes();
        let mut result = [0u8; 12];
        result.copy_from_slice(&bytes[..12]);
        result
    }
    
    /// Returns the raw u128 value (guaranteed to be <= u96::MAX).
    pub fn as_u128(&self) -> u128 {
        self.0
    }
    
    /// Returns true if the amount is zero.
    pub fn is_zero(&self) -> bool {
        self.0 == 0
    }
    
    /// Checked addition. Returns None if overflow occurs.
    pub fn checked_add(&self, other: NativeTokens) -> Option<NativeTokens> {
        let result = self.0.checked_add(other.0)?;
        if result > Self::MAX {
            None
        } else {
            Some(Self(result))
        }
    }
    
    /// Checked subtraction. Returns None if underflow occurs.
    pub fn checked_sub(&self, other: NativeTokens) -> Option<NativeTokens> {
        self.0.checked_sub(other.0).map(Self)
    }
}

impl Display for NativeTokens {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Add for NativeTokens {
    type Output = Self;
    
    fn add(self, other: Self) -> Self {
        self.checked_add(other)
            .expect("Addition overflow in NativeTokens")
    }
}

impl Sub for NativeTokens {
    type Output = Self;
    
    fn sub(self, other: Self) -> Self {
        self.checked_sub(other)
            .expect("Subtraction underflow in NativeTokens")
    }
}

impl From<u64> for NativeTokens {
    fn from(value: u64) -> Self {
        Self::from_u64(value)
    }
}

/// Trait for converting between different amount types.
/// 
/// This provides a common interface for converting between native tokens
/// and other amount representations like EVM amounts.
pub trait AmountConversion {
    /// Converts to a u128 value representing the amount.
    fn to_u128(&self) -> u128;
    
    /// Creates an instance from a u128 value.
    /// 
    /// # Arguments
    /// * `amount` - The amount as u128
    /// 
    /// # Returns
    /// * `Ok(Self)` if conversion successful
    /// * `Err(String)` if conversion fails (e.g., value out of range)
    fn from_u128(amount: u128) -> Result<Self, String>
    where
        Self: Sized;
    
    /// Converts to native tokens.
    fn to_native_tokens(&self) -> Result<NativeTokens, String> {
        NativeTokens::from_u128(self.to_u128())
    }
    
    /// Creates from native tokens.
    fn from_native_tokens(tokens: &NativeTokens) -> Result<Self, String>
    where
        Self: Sized,
    {
        Self::from_u128(tokens.as_u128())
    }
}

impl AmountConversion for NativeTokens {
    fn to_u128(&self) -> u128 {
        self.0
    }
    
    fn from_u128(amount: u128) -> Result<Self, String> {
        if amount > Self::MAX {
            Err("Value exceeds u96 maximum".to_string())
        } else {
            Ok(Self(amount))
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_native_tokens_creation() {
        let tokens = NativeTokens::new(1000);
        assert_eq!(tokens.as_u128(), 1000);
        
        let tokens_from_u64 = NativeTokens::from_u64(500);
        assert_eq!(tokens_from_u64.as_u128(), 500);
    }

    #[test]
    fn test_native_tokens_max_value() {
        let max_tokens = NativeTokens::new(NativeTokens::MAX);
        assert_eq!(max_tokens.as_u128(), NativeTokens::MAX);
    }

    #[test]
    #[should_panic(expected = "Value exceeds u96 maximum")]
    fn test_native_tokens_overflow() {
        NativeTokens::new(NativeTokens::MAX + 1);
    }

    #[test]
    fn test_bytes_conversion() {
        let tokens = NativeTokens::from_u64(0x123456789ABCDEF0);
        let bytes = tokens.to_le_bytes();
        let restored = NativeTokens::from_le_bytes(bytes).unwrap();
        assert_eq!(tokens, restored);
    }

    #[test]
    fn test_arithmetic_operations() {
        let a = NativeTokens::from_u64(100);
        let b = NativeTokens::from_u64(50);
        
        assert_eq!(a + b, NativeTokens::from_u64(150));
        assert_eq!(a - b, NativeTokens::from_u64(50));
        
        assert_eq!(a.checked_add(b), Some(NativeTokens::from_u64(150)));
        assert_eq!(a.checked_sub(b), Some(NativeTokens::from_u64(50)));
        assert_eq!(b.checked_sub(a), None); // underflow
    }

    #[test]
    fn test_amount_conversion_trait() {
        let tokens = NativeTokens::from_u64(1000);
        assert_eq!(tokens.to_u128(), 1000);
        
        let converted = NativeTokens::from_u128(2000).unwrap();
        assert_eq!(converted.as_u128(), 2000);
        
        assert!(NativeTokens::from_u128(NativeTokens::MAX + 1).is_err());
    }

    #[test]
    fn test_zero_and_default() {
        assert!(NativeTokens::ZERO.is_zero());
        assert!(NativeTokens::default().is_zero());
        assert_eq!(NativeTokens::ZERO, NativeTokens::default());
    }
}
