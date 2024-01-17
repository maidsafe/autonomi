// Copyright 2023 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{error::Result, Entry, EntryHash, Error, RegisterAddress, RegisterOp};

use crdts::merkle_reg::Node as MerkleDagEntry;
use crdts::{merkle_reg::MerkleReg, CmRDT, CvRDT};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fmt::{self, Debug, Display, Formatter},
    hash::Hash,
};

/// Register data type as a CRDT with Access Control
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd)]
pub(crate) struct RegisterCrdt {
    /// Address on the network of this piece of data
    address: RegisterAddress,
    /// CRDT to store the actual data, i.e. the items of the Register.
    data: MerkleReg<Entry>,
}

impl Display for RegisterCrdt {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "(")?;
        for (i, entry) in self.data.read().values().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "<{entry:?}>")?;
        }
        write!(f, ")")
    }
}

impl RegisterCrdt {
    /// Constructs a new '`RegisterCrdtImpl`'.
    pub(crate) fn new(address: RegisterAddress) -> Self {
        Self {
            address,
            data: MerkleReg::new(),
        }
    }

    /// Returns the address.
    pub(crate) fn address(&self) -> &RegisterAddress {
        &self.address
    }

    /// Merge another register into this one.
    pub(crate) fn merge(&mut self, other: Self) {
        self.data.merge(other.data);
    }

    /// Returns total number of items in the register.
    pub(crate) fn size(&self) -> u64 {
        (self.data.num_nodes() + self.data.num_orphans()) as u64
    }

    /// Write a new entry to the `RegisterCrdt`, returning the hash
    /// of the entry and the CRDT operation without a signature
    pub(crate) fn write(
        &mut self,
        entry: Entry,
        children: BTreeSet<EntryHash>,
    ) -> Result<(EntryHash, RegisterAddress, MerkleDagEntry<Entry>)> {
        let address = *self.address();

        let children_array: BTreeSet<[u8; 32]> = children.iter().map(|itr| itr.0).collect();
        let crdt_op = self.data.write(entry, children_array);
        self.data.apply(crdt_op.clone());
        let hash = crdt_op.hash();

        Ok((EntryHash(hash), address, crdt_op))
    }

    /// Apply a remote data CRDT operation to this replica of the `RegisterCrdtImpl`.
    pub(crate) fn apply_op(&mut self, op: RegisterOp) -> Result<()> {
        // Let's first check the op is validly signed.
        // Note: Perms and valid sig for the op are checked at the upper Register layer.

        // Check the targeting address is correct
        if self.address != op.address {
            return Err(Error::RegisterAddrMismatch {
                dst_addr: Box::new(op.address),
                reg_addr: Box::new(self.address),
            });
        }

        // Apply the CRDT operation to the Register
        self.data.apply(op.crdt_op);

        Ok(())
    }

    /// Get the entry corresponding to the provided `hash` if it exists.
    pub(crate) fn get(&self, hash: EntryHash) -> Option<&Entry> {
        self.data.node(hash.0).map(|node| &node.value)
    }

    /// Read current entries (multiple entries occur on concurrent writes).
    pub(crate) fn read(&self) -> BTreeSet<(EntryHash, Entry)> {
        self.data
            .read()
            .hashes_and_nodes()
            .map(|(hash, node)| (EntryHash(hash), node.value.clone()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use bls::SecretKey;
    use xor_name::XorName;

    #[test]
    fn creating_entry_hash() -> Result<()> {
        let mut rng = rand::thread_rng();
        let address_1 = RegisterAddress {
            meta: XorName::random(&mut rng),
            owner: SecretKey::random().public_key(),
        };
        let address_2 = RegisterAddress {
            meta: XorName::random(&mut rng),
            owner: SecretKey::random().public_key(),
        };

        let mut crdt_1 = RegisterCrdt::new(address_1);
        let mut crdt_2 = RegisterCrdt::new(address_2);
        let mut parents = BTreeSet::new();

        let entry_1 = vec![0x1, 0x1];
        // Different RegisterCrdtImpl shall create same hashes for the same entry from root
        let (entry_hash_1, _, _) = crdt_1.write(entry_1.clone(), parents.clone())?;
        let (entry_hash_2, _, _) = crdt_2.write(entry_1, parents.clone())?;
        assert!(entry_hash_1 == entry_hash_2);

        let entry_2 = vec![0x2, 0x2];
        // RegisterCrdtImpl shall create different hashes for different entries from root
        let (entry_hash_1_2, _, _) = crdt_1.write(entry_2, parents.clone())?;
        assert!(entry_hash_1 != entry_hash_1_2);

        let entry_3 = vec![0x3, 0x3];
        // Different RegisterCrdtImpl shall create same hashes for the same entry from same parents
        let _ = parents.insert(entry_hash_1);
        let (entry_hash_1_3, _, _) = crdt_1.write(entry_3.clone(), parents.clone())?;
        let (entry_hash_2_3, _, _) = crdt_1.write(entry_3, parents)?;
        assert!(entry_hash_1_3 == entry_hash_2_3);

        Ok(())
    }
}
