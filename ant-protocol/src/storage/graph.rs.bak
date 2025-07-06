// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::address::GraphEntryAddress;
use bls::SecretKey;
use serde::{Deserialize, Serialize};

// re-exports
pub use bls::{PublicKey, Signature};

/// Content of a graph, limited to 32 bytes
pub type GraphContent = [u8; 32];

/// A generic GraphEntry on the Network.
///
/// Graph entries are stored at the owner's public key. Note that there can only be one graph entry per owner.
/// Graph entries can be linked to other graph entries as parents or descendants.
/// Applications are free to define the meaning of these links, those are not enforced by the protocol.
/// The protocol only ensures that the graph entry is immutable once uploaded and that the signature is valid and matches the owner.
///
/// For convenience it is advised to make use of BLS key derivation to create multiple graph entries from a single key.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Hash, Ord, PartialOrd)]
pub struct GraphEntry {
    /// The owner of the graph. Note that graph entries are stored at the owner's public key
    pub owner: PublicKey,
    /// Other graph entries that this graph entry refers to as parents
    pub parents: Vec<PublicKey>,
    /// The content of the graph entry
    pub content: GraphContent,
    /// Other graph entries that this graph entry refers to as descendants/outputs along with some data associated to each one
    pub descendants: Vec<(PublicKey, GraphContent)>,
    /// signs the above 4 fields with the owners key
    pub signature: Signature,
}

impl std::fmt::Debug for GraphEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GraphEntry")
            .field("owner", &self.owner.to_hex())
            .field(
                "parents",
                &self.parents.iter().map(|p| p.to_hex()).collect::<Vec<_>>(),
            )
            .field("content", &hex::encode(self.content))
            .field(
                "descendants",
                &self
                    .descendants
                    .iter()
                    .map(|(p, c)| format!("{}: {}", p.to_hex(), hex::encode(c)))
                    .collect::<Vec<_>>(),
            )
            .field("signature", &hex::encode(self.signature.to_bytes()))
            .finish()
    }
}

impl GraphEntry {
    /// Maximum size of a graph entry: 100KB
    pub const MAX_SIZE: usize = 100 * 1024;

    /// Create a new graph entry, signing it with the provided secret key.
    pub fn new(
        owner: &SecretKey,
        parents: Vec<PublicKey>,
        content: GraphContent,
        descendants: Vec<(PublicKey, GraphContent)>,
    ) -> Self {
        let key = owner;
        let owner = key.public_key();
        let signature = key.sign(Self::bytes_to_sign(
            &owner,
            &parents,
            &content,
            &descendants,
        ));
        Self {
            owner,
            parents,
            content,
            descendants,
            signature,
        }
    }

    /// Create a new graph entry, with the signature already calculated.
    pub fn new_with_signature(
        owner: PublicKey,
        parents: Vec<PublicKey>,
        content: GraphContent,
        descendants: Vec<(PublicKey, GraphContent)>,
        signature: Signature,
    ) -> Self {
        Self {
            owner,
            parents,
            content,
            descendants,
            signature,
        }
    }

    /// Get the bytes that the signature is calculated from.
    pub fn bytes_to_sign(
        owner: &PublicKey,
        parents: &[PublicKey],
        content: &[u8],
        descendants: &[(PublicKey, GraphContent)],
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&owner.to_bytes());
        bytes.extend_from_slice("parent".as_bytes());
        bytes.extend_from_slice(
            &parents
                .iter()
                .map(|p| p.to_bytes())
                .collect::<Vec<_>>()
                .concat(),
        );
        bytes.extend_from_slice("content".as_bytes());
        bytes.extend_from_slice(content);
        bytes.extend_from_slice("descendants".as_bytes());
        bytes.extend_from_slice(
            &descendants
                .iter()
                .flat_map(|(p, c)| [&p.to_bytes(), c.as_slice()].concat())
                .collect::<Vec<_>>(),
        );
        bytes
    }

    pub fn address(&self) -> GraphEntryAddress {
        GraphEntryAddress::new(self.owner)
    }

    /// Get the bytes that the signature is calculated from.
    pub fn bytes_for_signature(&self) -> Vec<u8> {
        Self::bytes_to_sign(&self.owner, &self.parents, &self.content, &self.descendants)
    }

    /// Verify the signature of the graph entry
    pub fn verify_signature(&self) -> bool {
        self.owner
            .verify(&self.signature, self.bytes_for_signature())
    }

    /// Size of the graph entry
    pub fn size(&self) -> usize {
        size_of::<GraphEntry>()
            + self
                .descendants
                .iter()
                .map(|(p, c)| p.to_bytes().len() + c.len())
                .sum::<usize>()
            + self
                .parents
                .iter()
                .map(|p| p.to_bytes().len())
                .sum::<usize>()
    }

    /// Returns true if the graph entry is too big
    pub fn is_too_big(&self) -> bool {
        self.size() > Self::MAX_SIZE
    }
}
