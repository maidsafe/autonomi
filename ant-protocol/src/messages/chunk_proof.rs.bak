// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use serde::{Deserialize, Serialize};
use std::fmt;

/// The nonce provided by the verifier
pub type Nonce = u64;

/// The hash(record_value + nonce) that is used to prove the existence of a chunk
#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct ChunkProof([u8; 32]);

impl ChunkProof {
    pub fn new(record_value: &[u8], nonce: Nonce) -> Self {
        let nonce_bytes = nonce.to_be_bytes();
        let combined = [record_value, &nonce_bytes].concat();
        let hash = sha3_256(&combined);
        ChunkProof(hash)
    }

    pub fn verify(&self, other_proof: &ChunkProof) -> bool {
        self.0 == other_proof.0
    }

    /// Serialize this `ChunkProof` instance to a hex string.
    fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

fn sha3_256(input: &[u8]) -> [u8; 32] {
    use tiny_keccak::{Hasher, Sha3};

    let mut sha3 = Sha3::v256();
    let mut output = [0; 32];
    sha3.update(input);
    sha3.finalize(&mut output);
    output
}

impl fmt::Debug for ChunkProof {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ChunkProof").field(&self.to_hex()).finish()
    }
}
