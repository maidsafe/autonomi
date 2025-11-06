// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

mod merkle_payment;
mod merkle_tree;

// Re-export types from evmlib (disk contract and minimal types)
pub use evmlib::merkle_batch_payment::{
    DiskMerklePaymentContract, OnChainPaymentInfo, PoolCommitment, SmartContractError,
    CANDIDATES_PER_POOL, MAX_MERKLE_DEPTH, expected_reward_pools,
};

// Export ant-evm specific types (nodes, pools, proofs with signatures)
pub use merkle_payment::{
    MerklePaymentCandidateNode, MerklePaymentCandidatePool, MerklePaymentProof,
    MerklePaymentVerificationError,
};
pub use merkle_tree::{
    BadMerkleProof, MerkleBranch, MerkleTree, MerkleTreeError, MidpointProof,
    verify_merkle_proof, MERKLE_PAYMENT_EXPIRATION,
};
