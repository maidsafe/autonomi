// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use std::collections::HashSet;

use super::merkle_tree::{BadMerkleProof, MerkleBranch, MidpointProof};
use crate::RewardsAddress;
use evmlib::merkle_batch_payment::{PoolCommitment, PoolHash};
use evmlib::quoting_metrics::QuotingMetrics;
use libp2p::{
    PeerId,
    identity::{Keypair, PublicKey},
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use xor_name::XorName;

pub use evmlib::merkle_batch_payment::CANDIDATES_PER_POOL;

/// Errors that can occur during Merkle payment verification
#[derive(Debug, Error)]
pub enum MerklePaymentVerificationError {
    #[error("Winner pool hash mismatch: expected {expected:?}, got {got:?}")]
    WinnerPoolHashMismatch { expected: PoolHash, got: PoolHash },
    #[error("Merkle proof verification failed: {0}")]
    MerkleProofFailed(#[from] BadMerkleProof),
    #[error(
        "Paid addresses not subset of candidate pool. Paid: {smart_contract_paid_node_addresses:?}, Candidates: {candidate_addresses:?}"
    )]
    PaidAddressesNotSubset {
        smart_contract_paid_node_addresses: Vec<RewardsAddress>,
        candidate_addresses: Vec<RewardsAddress>,
    },
    #[error("Wrong number of paid addresses: expected {expected}, got {got}")]
    WrongPaidAddressCount { expected: usize, got: usize },
    #[error("Wrong number of candidates: expected {expected}, got {got}")]
    WrongCandidateCount { expected: usize, got: usize },
    #[error("Invalid node signature for address {address}")]
    InvalidNodeSignature { address: RewardsAddress },
    #[error("Timestamp mismatch for node {address}: expected {expected}, got {got}")]
    TimestampMismatch {
        address: RewardsAddress,
        expected: u64,
        got: u64,
    },
    #[error("Pool commitment does not match the pool")]
    CommitmentDoesNotMatchPool,
    #[error("Paid node index {index} is out of bounds for pool size {pool_size}")]
    PaidNodeIndexOutOfBounds { index: usize, pool_size: usize },
    #[error("Address mismatch at index {index}: expected {expected}, got {actual}")]
    PaidAddressMismatch {
        index: usize,
        expected: RewardsAddress,
        actual: RewardsAddress,
    },
    #[error("Invalid node peer id for address {address} at index {index}")]
    InvalidNodePeerId {
        index: usize,
        address: RewardsAddress,
    },
    #[error("Data type mismatch: expected {expected}, got {got} at node {address}")]
    DataTypeMismatch {
        address: RewardsAddress,
        expected: u32,
        got: u32,
    },
    #[error("Data size mismatch: expected {expected}, got {got} at node {address}")]
    DataSizeMismatch {
        address: RewardsAddress,
        expected: usize,
        got: usize,
    },
}

/// A node's signed quote for potential reward eligibility
///
/// Nodes create this structure in response to a client's quote request. The client provides
/// a `merkle_payment_timestamp`, which nodes verify is not outdated (not in the future or expired).
/// Nodes then sign their quoting metrics and payment address with this timestamp, establishing
/// their candidacy to be selected for payment rewards. The client collects these from multiple
/// nodes to build a [`MerklePaymentCandidatePool`].
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MerklePaymentCandidateNode {
    /// Node's libp2p public key
    /// PeerId can be derived from this: PeerId::from(PublicKey::try_decode_protobuf(pub_key))
    pub pub_key: Vec<u8>,

    /// Node's storage metrics at quote time
    pub quoting_metrics: QuotingMetrics,

    /// Node's Ethereum address for payment
    pub reward_address: RewardsAddress,

    /// Quote timestamp (provided by the client)
    pub merkle_payment_timestamp: u64,

    /// Signature over hash(quoting_metrics || reward_address || timestamp)
    pub signature: Vec<u8>,
}

impl MerklePaymentCandidateNode {
    /// Create a new candidate node with signed commitment
    ///
    /// # Arguments
    /// * `keypair` - Node's libp2p keypair for signing
    /// * `quoting_metrics` - Node's storage metrics at quote time
    /// * `reward_address` - Node's Ethereum address for payment
    /// * `timestamp` - Quote timestamp
    ///
    /// # Returns
    /// * `Result<Self, MerklePaymentError>` - Signed candidate node or signing error
    pub fn new(
        keypair: &Keypair,
        quoting_metrics: QuotingMetrics,
        reward_address: RewardsAddress,
        merkle_payment_timestamp: u64,
    ) -> Result<Self, libp2p::identity::SigningError> {
        // Extract public key in protobuf format
        let pub_key = keypair.public().encode_protobuf();

        // Sign the content
        let msg = Self::bytes_to_sign(&quoting_metrics, &reward_address, merkle_payment_timestamp);
        let signature = keypair.sign(&msg)?;

        Ok(Self {
            pub_key,
            quoting_metrics,
            reward_address,
            merkle_payment_timestamp,
            signature,
        })
    }

    /// Get the bytes to sign
    pub fn bytes_to_sign(
        quoting_metrics: &QuotingMetrics,
        reward_address: &RewardsAddress,
        timestamp: u64,
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&quoting_metrics.to_bytes());
        bytes.extend_from_slice(reward_address.as_slice());
        bytes.extend_from_slice(&timestamp.to_le_bytes());
        bytes
    }

    /// Convert to deterministic byte representation for hashing
    fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&self.pub_key);
        bytes.extend_from_slice(&self.quoting_metrics.to_bytes());
        bytes.extend_from_slice(self.reward_address.as_slice());
        bytes.extend_from_slice(&self.merkle_payment_timestamp.to_le_bytes());
        bytes.extend_from_slice(&self.signature);
        bytes
    }

    /// Derive PeerId from public key
    pub fn peer_id(&self) -> Result<PeerId, libp2p::identity::DecodingError> {
        PublicKey::try_decode_protobuf(&self.pub_key).map(|pk| pk.to_peer_id())
    }

    /// Verify signature is valid for this node
    pub fn verify_signature(&self) -> bool {
        let pub_key = match PublicKey::try_decode_protobuf(&self.pub_key) {
            Ok(pk) => pk,
            Err(_) => return false,
        };

        let msg = Self::bytes_to_sign(
            &self.quoting_metrics,
            &self.reward_address,
            self.merkle_payment_timestamp,
        );
        pub_key.verify(&msg, &self.signature)
    }
}

/// One candidate pool: midpoint proof + nodes who could store addresses
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MerklePaymentCandidatePool {
    /// The midpoint proof from ant-evm's merkle_tree module
    pub midpoint_proof: MidpointProof,

    /// Candidate nodes for this pool (should be 20 nodes)
    /// Provides redundancy - only 'depth' of these will be selected as winners
    pub candidate_nodes: Vec<MerklePaymentCandidateNode>,
}

impl MerklePaymentCandidatePool {
    /// Compute deterministic hash for on-chain storage key
    pub fn hash(&self) -> PoolHash {
        let mut bytes = Vec::new();

        // Hash of the intersection proof
        bytes.extend_from_slice(self.midpoint_proof.hash().as_ref());

        // Number of candidate nodes
        bytes.extend_from_slice(&(self.candidate_nodes.len() as u32).to_le_bytes());

        // Each candidate node's data
        for node in &self.candidate_nodes {
            bytes.extend_from_slice(&node.to_bytes());
        }

        XorName::from_content(&bytes).0
    }

    /// Convert to minimal commitment for smart contract submission
    pub fn to_commitment(&self) -> PoolCommitment {
        PoolCommitment {
            pool_hash: self.hash(),
            candidate_addresses: self
                .candidate_nodes
                .iter()
                .map(|node| node.reward_address)
                .collect(),
        }
    }

    /// Helper function for PoolCommitment verification
    ///
    /// This verifies that a commitment matches a pool and that the pool signatures are valid.
    pub fn verify_commitment(
        &self,
        commitment: &PoolCommitment,
        merkle_payment_timestamp: u64,
    ) -> Result<(), MerklePaymentVerificationError> {
        self.verify_signatures(merkle_payment_timestamp)?;
        let expected_commitment = self.to_commitment();
        if commitment != &expected_commitment {
            return Err(MerklePaymentVerificationError::CommitmentDoesNotMatchPool);
        }
        Ok(())
    }

    /// Get the addresses of the candidate nodes
    pub fn candidate_nodes_addresses(&self) -> HashSet<RewardsAddress> {
        self.candidate_nodes
            .iter()
            .map(|node| node.reward_address)
            .collect()
    }

    /// Verify that the signatures in the candidate pool are valid
    ///
    /// Checks:
    /// 1. Correct number of candidate nodes [`CANDIDATES_PER_POOL`]
    /// 2. All node signatures are valid
    /// 3. All timestamps match the merkle payment timestamp
    /// 4. All nodes quote the same data_type and data_size
    ///
    /// It does not verify the pool branch proof.
    pub fn verify_signatures(
        &self,
        merkle_payment_timestamp: u64,
    ) -> Result<(), MerklePaymentVerificationError> {
        // Verify correct number of candidates
        if self.candidate_nodes.len() != CANDIDATES_PER_POOL {
            return Err(MerklePaymentVerificationError::WrongCandidateCount {
                expected: CANDIDATES_PER_POOL,
                got: self.candidate_nodes.len(),
            });
        }

        // Verify all node signatures
        for node in &self.candidate_nodes {
            if !node.verify_signature() {
                return Err(MerklePaymentVerificationError::InvalidNodeSignature {
                    address: node.reward_address,
                });
            }
        }

        // Verify all timestamps match the merkle payment timestamp
        for node in &self.candidate_nodes {
            if node.merkle_payment_timestamp != merkle_payment_timestamp {
                return Err(MerklePaymentVerificationError::TimestampMismatch {
                    address: node.reward_address,
                    expected: merkle_payment_timestamp,
                    got: node.merkle_payment_timestamp,
                });
            }
        }

        // Verify all nodes are quoting for the same data_type and data_size
        // All nodes in a pool should be providing quotes for the same data
        if let Some(first_node) = self.candidate_nodes.first() {
            let expected_data_type = first_node.quoting_metrics.data_type;
            let expected_data_size = first_node.quoting_metrics.data_size;

            for node in &self.candidate_nodes[..] {
                if node.quoting_metrics.data_type != expected_data_type {
                    return Err(MerklePaymentVerificationError::DataTypeMismatch {
                        address: node.reward_address,
                        expected: expected_data_type,
                        got: node.quoting_metrics.data_type,
                    });
                }

                if node.quoting_metrics.data_size != expected_data_size {
                    return Err(MerklePaymentVerificationError::DataSizeMismatch {
                        address: node.reward_address,
                        expected: expected_data_size,
                        got: node.quoting_metrics.data_size,
                    });
                }
            }
        }

        Ok(())
    }
}

/// Data package sent from client to node for data storage and payment verification
///
/// Contains everything a node needs to verify:
/// 1. The data belongs to a paid Merkle tree (via proof)
/// 2. Payment was made to the correct pool (via winner pool proof and smart contract query)
/// 3. The node is eligible to store this data (if they're in the winner pool)
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MerklePaymentProof {
    /// The data's XorName
    pub address: XorName,

    /// Merkle proof that this data belongs to the paid tree
    pub data_proof: MerkleBranch,

    /// The winner pool selected by the smart contract
    /// Contains the full candidate pool with signatures and intersection proof
    pub winner_pool: MerklePaymentCandidatePool,
}

impl MerklePaymentProof {
    /// Create a new Merkle payment proof
    pub fn new(
        address: XorName,
        data_proof: MerkleBranch,
        winner_pool: MerklePaymentCandidatePool,
    ) -> Self {
        Self {
            address,
            data_proof,
            winner_pool,
        }
    }

    /// Get the hash of the winner pool (used to query smart contract for payment info)
    pub fn winner_pool_hash(&self) -> PoolHash {
        self.winner_pool.hash()
    }

    /// Get the corresponding peer ids for the paid nodes using their indices in the winner pool
    ///
    /// This uses the indices from the smart contract to identify exactly which nodes were paid.
    /// This is necessary because multiple nodes can share the same reward address.
    ///
    /// # Arguments
    /// * `paid_nodes` - The (address, index) pairs from the smart contract
    ///
    /// # Returns
    /// * `Ok(Vec<PeerId>)` - PeerIds of the paid nodes, in the same order as the input
    /// * `Err(PaidNodeIndexOutOfBounds)` - If any index is out of bounds
    /// * `Err(InvalidNodePeerId)` - If any paid node has an invalid public key
    /// * `Err(PaidAddressMismatch)` - If address at index doesn't match expected address
    pub fn corresponding_peer_ids(
        &self,
        paid_nodes: &[(RewardsAddress, usize)],
    ) -> Result<Vec<PeerId>, MerklePaymentVerificationError> {
        let mut peer_ids = Vec::with_capacity(paid_nodes.len());

        for (expected_address, index) in paid_nodes {
            let node = self.winner_pool.candidate_nodes.get(*index).ok_or(
                MerklePaymentVerificationError::PaidNodeIndexOutOfBounds {
                    index: *index,
                    pool_size: self.winner_pool.candidate_nodes.len(),
                },
            )?;

            // Verify the address at this index matches what the smart contract says
            if node.reward_address != *expected_address {
                return Err(MerklePaymentVerificationError::PaidAddressMismatch {
                    index: *index,
                    expected: *expected_address,
                    actual: node.reward_address,
                });
            }

            // Derive the PeerId from the node's public key
            let peer_id =
                node.peer_id()
                    .map_err(|_| MerklePaymentVerificationError::InvalidNodePeerId {
                        address: node.reward_address,
                        index: *index,
                    })?;

            peer_ids.push(peer_id);
        }

        Ok(peer_ids)
    }

    /// Verify the payment proof against the smart contract payment info
    ///
    /// # Arguments
    /// * `smart_contract_depth` - The depth value stored in the smart contract
    /// * `smart_contract_timestamp` - The merkle payment timestamp stored in the smart contract
    /// * `smart_contract_pool_hash` - The hash of the winner pool stored in the smart contract
    /// * `smart_contract_paid_nodes` - The (address, index) pairs of paid nodes from the smart contract
    pub fn verify(
        &self,
        smart_contract_depth: u8,
        smart_contract_timestamp: u64,
        smart_contract_pool_hash: &PoolHash,
        smart_contract_paid_nodes: &[(RewardsAddress, usize)],
    ) -> Result<(), MerklePaymentVerificationError> {
        // Verify the winner pool signatures and timestamps first
        self.winner_pool
            .verify_signatures(smart_contract_timestamp)?;

        // Verify the winner pool hash matches the smart contract pool hash
        let actual_hash = self.winner_pool.hash();
        if actual_hash != *smart_contract_pool_hash {
            return Err(MerklePaymentVerificationError::WinnerPoolHashMismatch {
                expected: *smart_contract_pool_hash,
                got: actual_hash,
            });
        }
        let smart_contract_root = self.winner_pool.midpoint_proof.root();

        // Verify the core Merkle proof using the tree-level verification
        crate::merkle_payments::verify_merkle_proof(
            &self.address,
            &self.data_proof,
            &self.winner_pool.midpoint_proof,
            smart_contract_depth,
            smart_contract_root,
            smart_contract_timestamp,
        )?;

        // Verify the correct number of nodes were paid (should equal depth)
        if smart_contract_paid_nodes.len() != smart_contract_depth as usize {
            return Err(MerklePaymentVerificationError::WrongPaidAddressCount {
                expected: smart_contract_depth as usize,
                got: smart_contract_paid_nodes.len(),
            });
        }

        // Verify all paid node (address, index) pairs are valid
        // This also verifies addresses match what's at those indices
        self.corresponding_peer_ids(smart_contract_paid_nodes)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::merkle_payments::DiskMerklePaymentContract;
    use crate::merkle_payments::merkle_tree::MerkleTree;
    use evmlib::quoting_metrics::QuotingMetrics;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tempfile::TempDir;

    fn make_test_addresses(count: usize) -> Vec<XorName> {
        (0..count)
            .map(|i| XorName::from_content(&i.to_le_bytes()))
            .collect()
    }

    fn create_mock_quoting_metrics(node_id: usize) -> QuotingMetrics {
        QuotingMetrics {
            data_type: 0,
            data_size: 4 * 1024 * 1024, // 4MB
            close_records_stored: node_id * 100,
            records_per_type: vec![],
            max_records: 1000,
            received_payment_count: node_id * 10,
            live_time: 3600 + (node_id as u64),
            network_density: Some([node_id as u8; 32]),
            network_size: Some(1000),
        }
    }

    #[test]
    fn test_candidate_node_constructor_and_signature() {
        // Create a keypair for signing
        let keypair = Keypair::generate_ed25519();
        let peer_id = keypair.public().to_peer_id();

        // Create test data
        let quoting_metrics = create_mock_quoting_metrics(42);
        let reward_address = RewardsAddress::from([0x42; 20]);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Create candidate node using constructor
        let node = MerklePaymentCandidateNode::new(
            &keypair,
            quoting_metrics.clone(),
            reward_address,
            timestamp,
        )
        .expect("Failed to create candidate node");

        // Verify the peer_id matches
        assert_eq!(
            node.peer_id().expect("Failed to derive peer_id"),
            peer_id,
            "PeerId should match keypair"
        );

        // Verify the signature is valid
        assert!(
            node.verify_signature(),
            "Signature should be valid for the signed data"
        );

        // Verify all fields are correctly set
        assert_eq!(node.reward_address, reward_address);
        assert_eq!(node.merkle_payment_timestamp, timestamp);
        assert_eq!(
            node.quoting_metrics.close_records_stored,
            quoting_metrics.close_records_stored
        );
    }

    #[test]
    fn test_signature_verification_with_tampering() {
        // Create a valid signed node
        let keypair = Keypair::generate_ed25519();
        let quoting_metrics = create_mock_quoting_metrics(1);
        let reward_address = RewardsAddress::from([0x11; 20]);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut node = MerklePaymentCandidateNode::new(
            &keypair,
            quoting_metrics.clone(),
            reward_address,
            timestamp,
        )
        .expect("Failed to create candidate node");

        // Valid signature should verify
        assert!(
            node.verify_signature(),
            "Original signature should be valid"
        );

        // Tamper with reward address - signature should now fail
        node.reward_address = RewardsAddress::from([0x22; 20]);
        assert!(
            !node.verify_signature(),
            "Signature should fail after tampering with reward_address"
        );

        // Restore and tamper with quoting metrics
        node.reward_address = reward_address;
        node.quoting_metrics.close_records_stored = 999;
        assert!(
            !node.verify_signature(),
            "Signature should fail after tampering with quoting_metrics"
        );

        // Restore and tamper with timestamp (use a clearly different time)
        node.quoting_metrics = quoting_metrics;
        node.merkle_payment_timestamp = timestamp + 3600; // 1 hour later
        assert!(
            !node.verify_signature(),
            "Signature should fail after tampering with timestamp"
        );

        // Test with wrong keypair's signature
        let wrong_keypair = Keypair::generate_ed25519();
        let wrong_node = MerklePaymentCandidateNode::new(
            &wrong_keypair,
            create_mock_quoting_metrics(2),
            RewardsAddress::from([0x33; 20]),
            timestamp,
        )
        .expect("Failed to create node with wrong keypair");

        // Swap signatures between nodes
        let original_signature = node.signature.clone();
        node.signature = wrong_node.signature.clone();
        assert!(
            !node.verify_signature(),
            "Signature from different keypair should fail"
        );

        // Verify original signature still works when restored
        node.signature = original_signature;
        node.reward_address = reward_address;
        node.merkle_payment_timestamp = timestamp;
        assert!(
            node.verify_signature(),
            "Original signature should work after restoration"
        );
    }

    #[test]
    fn test_pool_commitment_verification() {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Create a simple merkle tree
        let addresses = make_test_addresses(10);
        let tree = MerkleTree::from_xornames(addresses).unwrap();

        // Get a reward candidate pool
        let reward_candidates = tree.reward_candidates(timestamp).unwrap();
        let reward_pool = &reward_candidates[0];

        // Create candidate nodes for this pool
        let mut candidate_nodes = Vec::new();
        for i in 0..CANDIDATES_PER_POOL {
            let keypair = Keypair::generate_ed25519();
            let node = MerklePaymentCandidateNode::new(
                &keypair,
                create_mock_quoting_metrics(i),
                RewardsAddress::from([i as u8; 20]),
                timestamp,
            )
            .expect("Failed to create candidate node");
            candidate_nodes.push(node);
        }

        let pool = MerklePaymentCandidatePool {
            midpoint_proof: reward_pool.clone(),
            candidate_nodes: candidate_nodes.clone(),
        };

        // Create commitment from pool
        let commitment = pool.to_commitment();

        // Verify commitment matches pool
        assert!(
            pool.verify_commitment(&commitment, timestamp).is_ok(),
            "Commitment should verify against original pool"
        );

        // Test 1: Verify pool_hash is correct
        assert_eq!(
            commitment.pool_hash,
            pool.hash(),
            "Commitment pool_hash should match pool.hash()"
        );

        // Test 2: Verify addresses match
        let expected_addresses: Vec<RewardsAddress> = pool
            .candidate_nodes
            .iter()
            .map(|node| node.reward_address)
            .collect();
        assert_eq!(
            commitment.candidate_addresses, expected_addresses,
            "Commitment addresses should match pool node addresses"
        );
        assert_eq!(
            commitment.candidate_addresses.len(),
            CANDIDATES_PER_POOL,
            "Should have exactly {CANDIDATES_PER_POOL} candidate addresses",
        );

        // Test 3: Tamper with pool - verification should fail
        let mut tampered_pool = pool.clone();
        tampered_pool.candidate_nodes[0].reward_address = RewardsAddress::from([0xFF; 20]);
        assert!(
            tampered_pool
                .verify_commitment(&commitment, timestamp)
                .is_err(),
            "Commitment should not verify against tampered pool"
        );

        // Test 4: Create commitment from tampered pool and verify it doesn't match original commitment
        let tampered_commitment = tampered_pool.to_commitment();
        assert_ne!(
            commitment.pool_hash, tampered_commitment.pool_hash,
            "Tampered pool should have different hash"
        );
        assert_ne!(
            commitment.candidate_addresses[0], tampered_commitment.candidate_addresses[0],
            "Tampered pool should have different addresses"
        );

        // Test 5: Verify wrong number of candidates fails
        let mut wrong_count_pool = pool.clone();
        wrong_count_pool.candidate_nodes.pop();
        assert!(
            wrong_count_pool
                .verify_commitment(&commitment, timestamp)
                .is_err(),
            "Commitment should not verify pool with wrong candidate count"
        );

        // Test 6: Verify determinism - same pool generates same commitment
        let commitment2 = pool.to_commitment();
        assert_eq!(
            commitment.pool_hash, commitment2.pool_hash,
            "Same pool should generate same commitment hash"
        );
        assert_eq!(
            commitment.candidate_addresses, commitment2.candidate_addresses,
            "Same pool should generate same addresses"
        );
    }

    #[test]
    fn test_pool_verify_method() {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Create a valid pool with properly signed nodes
        let addresses = make_test_addresses(10);
        let tree = MerkleTree::from_xornames(addresses).unwrap();
        let reward_candidates = tree.reward_candidates(timestamp).unwrap();
        let reward_pool = &reward_candidates[0];

        // Create valid candidate nodes
        let mut candidate_nodes = Vec::new();
        for i in 0..CANDIDATES_PER_POOL {
            let keypair = Keypair::generate_ed25519();
            let node = MerklePaymentCandidateNode::new(
                &keypair,
                create_mock_quoting_metrics(i),
                RewardsAddress::from([i as u8; 20]),
                timestamp,
            )
            .expect("Failed to create candidate node");
            candidate_nodes.push(node);
        }

        let pool = MerklePaymentCandidatePool {
            midpoint_proof: reward_pool.clone(),
            candidate_nodes: candidate_nodes.clone(),
        };

        // Test 1: Valid pool should verify
        assert!(
            pool.verify_signatures(timestamp).is_ok(),
            "Valid pool should verify successfully"
        );

        // Test 2: Pool with wrong number of candidates should fail
        let mut wrong_count_pool = pool.clone();
        wrong_count_pool.candidate_nodes.pop();
        assert!(
            wrong_count_pool.verify_signatures(timestamp).is_err(),
            "Pool with wrong candidate count should fail verification"
        );

        // Test 3: Pool with too many candidates should fail
        let mut too_many_pool = pool.clone();
        too_many_pool
            .candidate_nodes
            .push(candidate_nodes[0].clone());
        assert!(
            too_many_pool.verify_signatures(timestamp).is_err(),
            "Pool with too many candidates should fail verification"
        );

        // Test 4: Pool with invalid signature should fail
        let mut invalid_sig_pool = pool.clone();
        invalid_sig_pool.candidate_nodes[0].signature = vec![0xFF; 64]; // Corrupt signature
        assert!(
            invalid_sig_pool.verify_signatures(timestamp).is_err(),
            "Pool with invalid signature should fail verification"
        );

        // Test 5: Pool with tampered node data should fail
        let mut tampered_pool = pool.clone();
        tampered_pool.candidate_nodes[0].reward_address = RewardsAddress::from([0xFF; 20]);
        assert!(
            tampered_pool.verify_signatures(timestamp).is_err(),
            "Pool with tampered node data should fail verification (signature mismatch)"
        );

        // Test 6: Empty pool should fail
        let empty_pool = MerklePaymentCandidatePool {
            midpoint_proof: reward_pool.clone(),
            candidate_nodes: vec![],
        };
        assert!(
            empty_pool.verify_signatures(timestamp).is_err(),
            "Empty pool should fail verification"
        );
    }

    #[test]
    fn test_pool_verify_timestamp_consistency() {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Create a valid pool
        let addresses = make_test_addresses(10);
        let tree = MerkleTree::from_xornames(addresses).unwrap();
        let reward_candidates = tree.reward_candidates(timestamp).unwrap();
        let reward_pool = &reward_candidates[0];

        // Create candidate nodes with identical timestamps
        let mut candidate_nodes = Vec::new();
        for i in 0..CANDIDATES_PER_POOL {
            let keypair = Keypair::generate_ed25519();
            let node = MerklePaymentCandidateNode::new(
                &keypair,
                create_mock_quoting_metrics(i),
                RewardsAddress::from([i as u8; 20]),
                timestamp,
            )
            .expect("Failed to create candidate node");
            candidate_nodes.push(node);
        }

        let pool = MerklePaymentCandidatePool {
            midpoint_proof: reward_pool.clone(),
            candidate_nodes: candidate_nodes.clone(),
        };

        // Valid pool with identical timestamps should verify
        assert!(
            pool.verify_signatures(timestamp).is_ok(),
            "Pool with identical timestamps should verify"
        );

        // Create pool with mismatched timestamps
        let mut mismatched_pool = pool.clone();
        let different_keypair = Keypair::generate_ed25519();
        let different_timestamp = timestamp + 3600; // 1 hour later
        mismatched_pool.candidate_nodes[5] = MerklePaymentCandidateNode::new(
            &different_keypair,
            create_mock_quoting_metrics(5),
            RewardsAddress::from([5u8; 20]),
            different_timestamp,
        )
        .expect("Failed to create node with different timestamp");

        assert!(
            mismatched_pool.verify_signatures(timestamp).is_err(),
            "Pool with mismatched timestamps should fail verification"
        );
    }

    #[test]
    fn test_invalid_public_key_error() {
        // Create a node with invalid pub_key
        let keypair = Keypair::generate_ed25519();
        let mut node = MerklePaymentCandidateNode::new(
            &keypair,
            create_mock_quoting_metrics(1),
            RewardsAddress::from([0x11; 20]),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        )
        .expect("Failed to create candidate node");

        // Corrupt pub_key with invalid data
        node.pub_key = vec![0xFF; 10]; // Too short and invalid format

        // Should fail to derive peer_id
        assert!(
            node.peer_id().is_err(),
            "Should fail to derive peer_id from invalid pub_key"
        );

        // Signature verification should also fail
        assert!(
            !node.verify_signature(),
            "Signature verification should fail with invalid pub_key"
        );
    }

    #[test]
    fn test_node_hash_determinism() {
        let keypair = Keypair::generate_ed25519();
        let quoting_metrics = create_mock_quoting_metrics(42);
        let reward_address = RewardsAddress::from([0x42; 20]);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Create two identical nodes
        let node1 = MerklePaymentCandidateNode::new(
            &keypair,
            quoting_metrics.clone(),
            reward_address,
            timestamp,
        )
        .expect("Failed to create first node");

        let node2 =
            MerklePaymentCandidateNode::new(&keypair, quoting_metrics, reward_address, timestamp)
                .expect("Failed to create second node");

        // to_bytes() should be deterministic
        assert_eq!(
            node1.to_bytes(),
            node2.to_bytes(),
            "Same inputs should produce same byte representation"
        );

        // Hashes of pools containing these nodes should be deterministic
        let addresses = make_test_addresses(10);
        let tree = MerkleTree::from_xornames(addresses).unwrap();
        let reward_candidates = tree.reward_candidates(timestamp).unwrap();
        let reward_pool = &reward_candidates[0];

        let pool1 = MerklePaymentCandidatePool {
            midpoint_proof: reward_pool.clone(),
            candidate_nodes: vec![node1.clone(); CANDIDATES_PER_POOL],
        };

        let pool2 = MerklePaymentCandidatePool {
            midpoint_proof: reward_pool.clone(),
            candidate_nodes: vec![node2.clone(); CANDIDATES_PER_POOL],
        };

        assert_eq!(
            pool1.hash(),
            pool2.hash(),
            "Identical pools should produce identical hashes"
        );
    }

    #[test]
    fn test_corresponding_peer_ids_happy_path() {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Create a merkle tree and get a reward pool
        let addresses = make_test_addresses(10);
        let tree = MerkleTree::from_xornames(addresses.clone()).unwrap();
        let reward_candidates = tree.reward_candidates(timestamp).unwrap();
        let reward_pool = &reward_candidates[0];

        // Create 20 candidate nodes with different addresses and PeerIds
        let mut candidate_nodes = Vec::new();
        let mut expected_peer_ids = Vec::new();
        for i in 0..CANDIDATES_PER_POOL {
            let keypair = Keypair::generate_ed25519();
            let peer_id = keypair.public().to_peer_id();
            let node = MerklePaymentCandidateNode::new(
                &keypair,
                create_mock_quoting_metrics(i),
                RewardsAddress::from([i as u8; 20]),
                timestamp,
            )
            .expect("Failed to create candidate node");
            candidate_nodes.push(node);
            expected_peer_ids.push((RewardsAddress::from([i as u8; 20]), peer_id));
        }

        let proof = MerklePaymentProof::new(
            addresses[0],
            tree.generate_address_proof(0, addresses[0]).unwrap(),
            MerklePaymentCandidatePool {
                midpoint_proof: reward_pool.clone(),
                candidate_nodes,
            },
        );

        // Test: Get PeerIds using (address, index) pairs
        let paid_nodes = vec![
            (RewardsAddress::from([0; 20]), 0),
            (RewardsAddress::from([1; 20]), 1),
            (RewardsAddress::from([2; 20]), 2),
        ];

        let result = proof.corresponding_peer_ids(&paid_nodes);
        assert!(
            result.is_ok(),
            "Should succeed when indices and addresses match"
        );

        let peer_ids = result.unwrap();
        assert_eq!(peer_ids.len(), 3, "Should return 3 PeerIds");

        // Verify returned PeerIds match expected (in order)
        for (i, (_addr, expected_peer_id)) in expected_peer_ids.iter().take(3).enumerate() {
            assert_eq!(
                peer_ids[i], *expected_peer_id,
                "Should return PeerId at index {i} in correct order"
            );
        }
    }

    #[test]
    fn test_corresponding_peer_ids_index_out_of_bounds() {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Create a merkle tree and get a reward pool
        let addresses = make_test_addresses(10);
        let tree = MerkleTree::from_xornames(addresses.clone()).unwrap();
        let reward_candidates = tree.reward_candidates(timestamp).unwrap();
        let reward_pool = &reward_candidates[0];

        // Create 20 candidate nodes
        let mut candidate_nodes = Vec::new();
        for i in 0..CANDIDATES_PER_POOL {
            let keypair = Keypair::generate_ed25519();
            let node = MerklePaymentCandidateNode::new(
                &keypair,
                create_mock_quoting_metrics(i),
                RewardsAddress::from([i as u8; 20]),
                timestamp,
            )
            .expect("Failed to create candidate node");
            candidate_nodes.push(node);
        }

        let proof = MerklePaymentProof::new(
            addresses[0],
            tree.generate_address_proof(0, addresses[0]).unwrap(),
            MerklePaymentCandidatePool {
                midpoint_proof: reward_pool.clone(),
                candidate_nodes,
            },
        );

        // Test: Index out of bounds (attack scenario)
        let paid_nodes = vec![
            (RewardsAddress::from([0; 20]), 0),   // valid
            (RewardsAddress::from([99; 20]), 99), // index 99 is out of bounds!
        ];

        let result = proof.corresponding_peer_ids(&paid_nodes);
        assert!(result.is_err(), "Should fail when index is out of bounds");

        match result {
            Err(MerklePaymentVerificationError::PaidNodeIndexOutOfBounds { index, pool_size }) => {
                assert_eq!(index, 99);
                assert_eq!(pool_size, CANDIDATES_PER_POOL);
            }
            _ => panic!("Expected PaidNodeIndexOutOfBounds error"),
        }
    }

    #[test]
    fn test_corresponding_peer_ids_address_mismatch() {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Create a merkle tree and get a reward pool
        let addresses = make_test_addresses(10);
        let tree = MerkleTree::from_xornames(addresses.clone()).unwrap();
        let reward_candidates = tree.reward_candidates(timestamp).unwrap();
        let reward_pool = &reward_candidates[0];

        // Create 20 candidate nodes
        let mut candidate_nodes = Vec::new();
        for i in 0..CANDIDATES_PER_POOL {
            let keypair = Keypair::generate_ed25519();
            let node = MerklePaymentCandidateNode::new(
                &keypair,
                create_mock_quoting_metrics(i),
                RewardsAddress::from([i as u8; 20]),
                timestamp,
            )
            .expect("Failed to create candidate node");
            candidate_nodes.push(node);
        }

        let proof = MerklePaymentProof::new(
            addresses[0],
            tree.generate_address_proof(0, addresses[0]).unwrap(),
            MerklePaymentCandidatePool {
                midpoint_proof: reward_pool.clone(),
                candidate_nodes,
            },
        );

        // Test: Address doesn't match what's at the index (attack scenario)
        let paid_nodes = vec![
            (RewardsAddress::from([99; 20]), 0), // index 0 has address [0;20], not [99;20]!
        ];

        let result = proof.corresponding_peer_ids(&paid_nodes);
        assert!(
            result.is_err(),
            "Should fail when address doesn't match index"
        );

        match result {
            Err(MerklePaymentVerificationError::PaidAddressMismatch {
                index,
                expected,
                actual,
            }) => {
                assert_eq!(index, 0);
                assert_eq!(expected, RewardsAddress::from([99; 20]));
                assert_eq!(actual, RewardsAddress::from([0; 20]));
            }
            _ => panic!("Expected PaidAddressMismatch error"),
        }
    }

    #[test]
    fn test_corresponding_peer_ids_multiple_nodes_same_address() {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Create a merkle tree and get a reward pool
        let addresses = make_test_addresses(10);
        let tree = MerkleTree::from_xornames(addresses.clone()).unwrap();
        let reward_candidates = tree.reward_candidates(timestamp).unwrap();
        let reward_pool = &reward_candidates[0];

        // Create candidate nodes where MULTIPLE nodes share the SAME reward address
        let shared_address = RewardsAddress::from([42; 20]);
        let mut candidate_nodes = Vec::new();
        let mut peer_ids_for_shared_address = Vec::new();

        // First 3 nodes share the same address
        for i in 0..3 {
            let keypair = Keypair::generate_ed25519();
            let peer_id = keypair.public().to_peer_id();
            let node = MerklePaymentCandidateNode::new(
                &keypair,
                create_mock_quoting_metrics(i),
                shared_address, // Same address!
                timestamp,
            )
            .expect("Failed to create candidate node");
            candidate_nodes.push(node);
            peer_ids_for_shared_address.push(peer_id);
        }

        // Remaining nodes have unique addresses
        for i in 3..CANDIDATES_PER_POOL {
            let keypair = Keypair::generate_ed25519();
            let node = MerklePaymentCandidateNode::new(
                &keypair,
                create_mock_quoting_metrics(i),
                RewardsAddress::from([i as u8; 20]),
                timestamp,
            )
            .expect("Failed to create candidate node");
            candidate_nodes.push(node);
        }

        let proof = MerklePaymentProof::new(
            addresses[0],
            tree.generate_address_proof(0, addresses[0]).unwrap(),
            MerklePaymentCandidatePool {
                midpoint_proof: reward_pool.clone(),
                candidate_nodes,
            },
        );

        // Test: When nodes at specific indices are paid (even with shared address),
        // we can distinguish them by index
        let paid_nodes = vec![
            (shared_address, 0), // First node with shared address
            (shared_address, 1), // Second node with shared address
        ];

        let result = proof.corresponding_peer_ids(&paid_nodes);
        assert!(
            result.is_ok(),
            "Should succeed when indices and addresses match"
        );

        let peer_ids = result.unwrap();
        assert_eq!(
            peer_ids.len(),
            2,
            "Should return exactly 2 PeerIds for the 2 paid indices"
        );

        // Verify the specific PeerIds at indices 0 and 1 are returned (in order)
        assert_eq!(peer_ids[0], peer_ids_for_shared_address[0]);
        assert_eq!(peer_ids[1], peer_ids_for_shared_address[1]);

        // Test: Paying index 2 (third node with same address)
        let paid_nodes = vec![(shared_address, 2)];
        let result = proof.corresponding_peer_ids(&paid_nodes);
        assert!(result.is_ok());
        let peer_ids = result.unwrap();
        assert_eq!(peer_ids[0], peer_ids_for_shared_address[2]);
    }

    #[test]
    fn test_corresponding_peer_ids_invalid_public_key() {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Create a merkle tree and get a reward pool
        let addresses = make_test_addresses(10);
        let tree = MerkleTree::from_xornames(addresses.clone()).unwrap();
        let reward_candidates = tree.reward_candidates(timestamp).unwrap();
        let reward_pool = &reward_candidates[0];

        // Create candidate nodes
        let mut candidate_nodes = Vec::new();
        for i in 0..CANDIDATES_PER_POOL {
            let keypair = Keypair::generate_ed25519();
            let node = MerklePaymentCandidateNode::new(
                &keypair,
                create_mock_quoting_metrics(i),
                RewardsAddress::from([i as u8; 20]),
                timestamp,
            )
            .expect("Failed to create candidate node");
            candidate_nodes.push(node);
        }

        // Corrupt the pub_key of one node
        candidate_nodes[5].pub_key = vec![0xFF; 10];

        let proof = MerklePaymentProof::new(
            addresses[0],
            tree.generate_address_proof(0, addresses[0]).unwrap(),
            MerklePaymentCandidatePool {
                midpoint_proof: reward_pool.clone(),
                candidate_nodes,
            },
        );

        // Try to get PeerIds for the corrupted node's address
        let paid_nodes = vec![(RewardsAddress::from([5; 20]), 5)];

        let result = proof.corresponding_peer_ids(&paid_nodes);
        assert!(
            result.is_err(),
            "Should fail when candidate has invalid pub_key"
        );

        match result {
            Err(MerklePaymentVerificationError::InvalidNodePeerId { address, index }) => {
                assert_eq!(address, RewardsAddress::from([5; 20]));
                assert_eq!(index, 5);
            }
            _ => panic!("Expected InvalidNodePeerId error"),
        }
    }

    #[test]
    fn test_corresponding_peer_ids_empty_input() {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Create a merkle tree and get a reward pool
        let addresses = make_test_addresses(10);
        let tree = MerkleTree::from_xornames(addresses.clone()).unwrap();
        let reward_candidates = tree.reward_candidates(timestamp).unwrap();
        let reward_pool = &reward_candidates[0];

        // Create candidate nodes
        let mut candidate_nodes = Vec::new();
        for i in 0..CANDIDATES_PER_POOL {
            let keypair = Keypair::generate_ed25519();
            let node = MerklePaymentCandidateNode::new(
                &keypair,
                create_mock_quoting_metrics(i),
                RewardsAddress::from([i as u8; 20]),
                timestamp,
            )
            .expect("Failed to create candidate node");
            candidate_nodes.push(node);
        }

        let proof = MerklePaymentProof::new(
            addresses[0],
            tree.generate_address_proof(0, addresses[0]).unwrap(),
            MerklePaymentCandidatePool {
                midpoint_proof: reward_pool.clone(),
                candidate_nodes,
            },
        );

        // Test: Empty paid nodes
        let paid_nodes: Vec<(RewardsAddress, usize)> = vec![];

        let result = proof.corresponding_peer_ids(&paid_nodes);
        assert!(result.is_ok(), "Should succeed with empty input");

        let peer_ids = result.unwrap();
        assert_eq!(peer_ids.len(), 0, "Should return empty Vec for empty input");
    }

    #[test]
    fn test_corresponding_peer_ids_partial_payment() {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Create a merkle tree and get a reward pool
        let addresses = make_test_addresses(10);
        let tree = MerkleTree::from_xornames(addresses.clone()).unwrap();
        let reward_candidates = tree.reward_candidates(timestamp).unwrap();
        let reward_pool = &reward_candidates[0];

        // Create 20 candidate nodes
        let mut candidate_nodes = Vec::new();
        let mut all_peer_ids = Vec::new();
        for i in 0..CANDIDATES_PER_POOL {
            let keypair = Keypair::generate_ed25519();
            let peer_id = keypair.public().to_peer_id();
            let node = MerklePaymentCandidateNode::new(
                &keypair,
                create_mock_quoting_metrics(i),
                RewardsAddress::from([i as u8; 20]),
                timestamp,
            )
            .expect("Failed to create candidate node");
            candidate_nodes.push(node);
            all_peer_ids.push(peer_id);
        }

        let proof = MerklePaymentProof::new(
            addresses[0],
            tree.generate_address_proof(0, addresses[0]).unwrap(),
            MerklePaymentCandidatePool {
                midpoint_proof: reward_pool.clone(),
                candidate_nodes,
            },
        );

        // Test: Only subset of candidates were paid (depth=7, so only 7 out of 20 paid)
        let paid_nodes = vec![
            (RewardsAddress::from([0; 20]), 0),
            (RewardsAddress::from([1; 20]), 1),
            (RewardsAddress::from([2; 20]), 2),
            (RewardsAddress::from([3; 20]), 3),
            (RewardsAddress::from([4; 20]), 4),
            (RewardsAddress::from([5; 20]), 5),
            (RewardsAddress::from([6; 20]), 6),
        ];

        let result = proof.corresponding_peer_ids(&paid_nodes);
        assert!(
            result.is_ok(),
            "Should succeed when indices and addresses match"
        );

        let peer_ids = result.unwrap();
        assert_eq!(
            peer_ids.len(),
            7,
            "Should return only 7 PeerIds for the 7 paid nodes"
        );

        // Verify only the paid nodes' PeerIds are returned (in order)
        for i in 0..7 {
            assert_eq!(
                peer_ids[i], all_peer_ids[i],
                "Should return PeerId at index {i} in correct order"
            );
        }
    }

    #[test]
    fn test_complete_merkle_batch_payment_flow() {
        // Phase 1: Client prepares addresses and tree
        let address_count = 100;
        let addresses = make_test_addresses(address_count);
        let tree = MerkleTree::from_xornames(addresses.clone()).unwrap();
        let _root = tree.root();
        let depth = tree.depth();
        assert_eq!(depth, 7); // ceil(log2(100)) = 7

        // Phase 2: Client queries candidate pools
        let merkle_payment_timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let reward_candidates = tree.reward_candidates(merkle_payment_timestamp).unwrap();
        let expected_pools = crate::merkle_payments::expected_reward_pools(depth);
        assert_eq!(reward_candidates.len(), expected_pools);

        // Simulate querying 20 nodes for each pool with properly signed commitments
        let mut all_candidate_pools = Vec::new();
        for (pool_idx, reward_pool) in reward_candidates.iter().enumerate() {
            let mut candidate_nodes = Vec::new();
            for node_id in 0..CANDIDATES_PER_POOL {
                let keypair = Keypair::generate_ed25519();

                // Nodes verify the merkle payment timestamp is not too old (or in the future)
                let current_time = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                assert!(
                    merkle_payment_timestamp <= current_time,
                    "Timestamp should not be in the future"
                );
                assert!(
                    current_time - merkle_payment_timestamp
                        < crate::merkle_payments::merkle_tree::MERKLE_PAYMENT_EXPIRATION,
                    "Timestamp should not be expired"
                );

                // Nodes generate their MerklePaymentCandidateNode and return it to the client
                let node = MerklePaymentCandidateNode::new(
                    &keypair,
                    create_mock_quoting_metrics(node_id),
                    RewardsAddress::from([(pool_idx * CANDIDATES_PER_POOL + node_id) as u8; 20]),
                    merkle_payment_timestamp,
                )
                .expect("Failed to create candidate node");

                // client verifies the node's signature
                assert!(node.verify_signature());

                // client verifies the node's MerklePaymentCandidateNode is the same as the one provided
                assert_eq!(
                    node.merkle_payment_timestamp, merkle_payment_timestamp,
                    "Honest nodes should return the same merkle payment timestamp"
                );

                // client adds the node to the pool
                candidate_nodes.push(node);
            }

            let pool = MerklePaymentCandidatePool {
                midpoint_proof: reward_pool.clone(),
                candidate_nodes,
            };
            all_candidate_pools.push(pool);
        }

        // Phase 3: Client submits payment to contract
        let pool_commitments: Vec<PoolCommitment> = all_candidate_pools
            .iter()
            .map(|pool| pool.to_commitment())
            .collect();

        let temp_dir = TempDir::new().unwrap();
        let contract =
            DiskMerklePaymentContract::new_with_path(temp_dir.path().to_path_buf()).unwrap();

        let (winner_pool_hash, _amount) = contract
            .pay_for_merkle_tree(depth, pool_commitments.clone(), merkle_payment_timestamp)
            .unwrap();

        // Verify payment info stored correctly
        let payment_info = contract.get_payment_info(winner_pool_hash).unwrap();
        assert_eq!(payment_info.depth, depth);
        assert_eq!(
            payment_info.merkle_payment_timestamp,
            merkle_payment_timestamp
        );
        assert_eq!(payment_info.paid_node_addresses.len(), depth as usize);

        // Find winner pool and verify commitment
        let (winner_pool, winner_commitment) = all_candidate_pools
            .iter()
            .zip(pool_commitments.iter())
            .find(|(pool, _)| pool.hash() == winner_pool_hash)
            .expect("Winner pool should be found");

        assert!(
            winner_pool
                .verify_commitment(winner_commitment, merkle_payment_timestamp)
                .is_ok(),
            "Winner commitment should verify against full pool data"
        );

        // Phase 4: Generate payment proofs for upload
        // Client creates a MerklePaymentProof for each address to send to nodes
        let payment_proofs: Vec<MerklePaymentProof> = addresses
            .iter()
            .enumerate()
            .map(|(i, address_hash)| {
                let address_proof = tree.generate_address_proof(i, *address_hash).unwrap();
                MerklePaymentProof::new(*address_hash, address_proof, winner_pool.clone())
            })
            .collect();

        // Phase 5: Nodes verify payment proofs
        for payment_proof in &payment_proofs {
            // Node queries smart contract using the winner_pool_hash
            let winner_to_fetch = payment_proof.winner_pool_hash();
            let payment = contract
                .get_payment_info(winner_to_fetch)
                .expect("Payment should be found");

            // Node verifies the payment proof using the smart contract data
            let verification_result = payment_proof.verify(
                payment.depth,
                payment.merkle_payment_timestamp,
                &winner_to_fetch,
                &payment.paid_node_addresses,
            );
            if let Err(e) = &verification_result {
                eprintln!("Verification failed: {e:?}");
            }
            assert!(
                verification_result.is_ok(),
                "Payment proof should verify against smart contract data: {verification_result:?}"
            );

            // Node verifies the closest nodes to winner pool include majority of paid nodes (this is done node side)
        }
    }
}
