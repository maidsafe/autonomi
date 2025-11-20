// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

mod disk_contract;
mod merkle_payment;
mod merkle_tree;

pub use disk_contract::{DiskMerklePaymentContract, OnChainPaymentInfo, SmartContractError};
pub use merkle_payment::{
    CANDIDATES_PER_POOL, MAX_MERKLE_DEPTH, MerklePaymentCandidateNode, MerklePaymentCandidatePool,
    MerklePaymentProof, MerklePaymentVerificationError, PoolCommitment,
};
pub use merkle_tree::{
    BadMerkleProof, MERKLE_PAYMENT_EXPIRATION, MerkleBranch, MerkleTree, MerkleTreeError,
    MidpointProof, expected_reward_pools, verify_merkle_proof,
};
