// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::storage::{ChunkAddress, GraphEntryAddress, PointerAddress, ScratchpadAddress};
use bls::{PublicKey, SecretKey, Signature};
use serde::{Deserialize, Serialize};
use xor_name::XorName;

/// Pointer, a mutable address pointing to other data on the Network
/// It is stored at the owner's public key and can only be updated by the owner
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Ord, PartialOrd)]
pub struct Pointer {
    owner: PublicKey,
    counter: u32,
    target: PointerTarget,
    signature: Signature,
}

impl std::fmt::Debug for Pointer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pointer")
            .field("owner", &self.owner.to_hex())
            .field("counter", &self.counter)
            .field("target", &self.target)
            .field("signature", &hex::encode(self.signature.to_bytes()))
            .finish()
    }
}

/// The target of a pointer, the address it points to
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Ord, PartialOrd)]
pub enum PointerTarget {
    ChunkAddress(ChunkAddress),
    GraphEntryAddress(GraphEntryAddress),
    PointerAddress(PointerAddress),
    ScratchpadAddress(ScratchpadAddress),
}

impl PointerTarget {
    /// Returns the xorname of the target
    pub fn xorname(&self) -> XorName {
        match self {
            PointerTarget::ChunkAddress(addr) => *addr.xorname(),
            PointerTarget::GraphEntryAddress(addr) => addr.xorname(),
            PointerTarget::PointerAddress(addr) => addr.xorname(),
            PointerTarget::ScratchpadAddress(addr) => addr.xorname(),
        }
    }

    /// Returns the hex string representation of the target
    pub fn to_hex(&self) -> String {
        match self {
            PointerTarget::ChunkAddress(addr) => addr.to_hex(),
            PointerTarget::GraphEntryAddress(addr) => addr.to_hex(),
            PointerTarget::PointerAddress(addr) => addr.to_hex(),
            PointerTarget::ScratchpadAddress(addr) => addr.to_hex(),
        }
    }
}

impl Pointer {
    /// Create a new pointer, signing it with the provided secret key.
    /// This pointer would be stored on the network at the provided key's public key.
    /// There can only be one pointer at a time at the same address (one per key).
    pub fn new(owner: &SecretKey, counter: u32, target: PointerTarget) -> Self {
        let pubkey = owner.public_key();
        let bytes_to_sign = Self::bytes_to_sign(&pubkey, counter, &target);
        let signature = owner.sign(&bytes_to_sign);

        Self {
            owner: pubkey,
            counter,
            target,
            signature,
        }
    }

    /// Create a new pointer with an existing signature
    pub fn new_with_signature(
        owner: PublicKey,
        counter: u32,
        target: PointerTarget,
        signature: Signature,
    ) -> Self {
        Self {
            owner,
            counter,
            target,
            signature,
        }
    }

    /// Get the bytes that the signature is calculated from
    fn bytes_to_sign(owner: &PublicKey, counter: u32, target: &PointerTarget) -> Vec<u8> {
        let mut bytes = Vec::new();
        // Add owner public key bytes
        bytes.extend_from_slice(&owner.to_bytes());
        // Add counter
        bytes.extend_from_slice(&counter.to_le_bytes());
        // Add target bytes using MessagePack serialization
        if let Ok(target_bytes) = rmp_serde::to_vec(target) {
            bytes.extend_from_slice(&target_bytes);
        }
        bytes
    }

    /// Get the address of the pointer
    pub fn address(&self) -> PointerAddress {
        PointerAddress::new(self.owner)
    }

    /// Get the owner of the pointer
    pub fn owner(&self) -> &PublicKey {
        &self.owner
    }

    /// Get the target of the pointer
    pub fn target(&self) -> &PointerTarget {
        &self.target
    }

    /// Get the bytes that were signed for this pointer
    pub fn bytes_for_signature(&self) -> Vec<u8> {
        Self::bytes_to_sign(&self.owner, self.counter, &self.target)
    }

    pub fn xorname(&self) -> XorName {
        self.target.xorname()
    }

    /// Get the counter of the pointer, the higher the counter, the more recent the pointer is
    /// Similarly to counter CRDTs only the latest version (highest counter) of the pointer is kept on the network
    pub fn counter(&self) -> u32 {
        self.counter
    }

    /// Verifies if the pointer has a valid signature
    pub fn verify_signature(&self) -> bool {
        let bytes = self.bytes_for_signature();
        self.owner.verify(&self.signature, &bytes)
    }

    /// Size of the pointer
    pub fn size() -> usize {
        size_of::<Pointer>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pointer_creation_and_validation() {
        let owner_sk = SecretKey::random();
        let counter = 1;
        let pk = SecretKey::random().public_key();
        let target = PointerTarget::GraphEntryAddress(GraphEntryAddress::new(pk));

        // Create and sign pointer
        let pointer = Pointer::new(&owner_sk, counter, target.clone());
        assert!(pointer.verify_signature()); // Should be valid with correct signature

        // Create pointer with wrong signature
        let wrong_sk = SecretKey::random();
        let sig = wrong_sk.sign(pointer.bytes_for_signature());
        let wrong_pointer =
            Pointer::new_with_signature(owner_sk.public_key(), counter, target.clone(), sig);
        assert!(!wrong_pointer.verify_signature()); // Should be invalid with wrong signature
    }
}
