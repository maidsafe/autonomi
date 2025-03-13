use bls::PublicKey;
use serde::{Deserialize, Serialize};
use xor_name::XorName;

use super::AddressParseError;

/// Address of a [`crate::storage::pointer::Pointer`]
/// It is derived from the owner's public key
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct PointerAddress(PublicKey);

impl PointerAddress {
    /// Create a new [`PointerAddress`]
    pub fn new(owner: PublicKey) -> Self {
        Self(owner)
    }

    /// Return the network name of the scratchpad.
    /// This is used to locate the scratchpad on the network.
    pub fn xorname(&self) -> XorName {
        XorName::from_content(&self.0.to_bytes())
    }

    /// Return the owner.
    pub fn owner(&self) -> &PublicKey {
        &self.0
    }

    /// Serialize this [`PointerAddress`] into a hex-encoded string.
    pub fn to_hex(&self) -> String {
        hex::encode(self.0.to_bytes())
    }

    /// Parse a hex-encoded string into a [`PointerAddress`].
    pub fn from_hex(hex: &str) -> Result<Self, AddressParseError> {
        let owner = PublicKey::from_hex(hex)?;
        Ok(Self(owner))
    }
}

impl std::fmt::Display for PointerAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", &self.to_hex())
    }
}

impl std::fmt::Debug for PointerAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", &self.to_hex())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pointer_serialization() {
        let key = bls::SecretKey::random();
        let pointer_address = PointerAddress::new(key.public_key());
        let serialized = pointer_address.to_hex();
        let deserialized = PointerAddress::from_hex(&serialized).unwrap();
        assert_eq!(pointer_address, deserialized);
    }
}
