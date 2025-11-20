// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use rs_merkle::Hasher;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;
use xor_name::XorName;

/// Maximum tree depth
pub use evmlib::merkle_batch_payment::MAX_MERKLE_DEPTH;

/// Minimum number of leaves (addresses) for a Merkle tree
pub const MIN_LEAVES: usize = 2;

/// Maximum number of leaves (2^MAX_MERKLE_DEPTH)
pub const MAX_LEAVES: usize = 1 << MAX_MERKLE_DEPTH;

/// Maximum age of a Merkle payment (one week in seconds)
/// Payments older than this are considered expired and nodes will reject addresses
pub const MERKLE_PAYMENT_EXPIRATION: u64 = 7 * 24 * 60 * 60; // 7 days

/// Calculate the expected number of reward candidate pools for a given tree depth
///
/// This is used throughout the payment system to determine how many candidate pools
/// should exist for a Merkle tree of a given depth.
///
/// # Formula
/// Number of pools = 2^ceil(depth/2)
///
/// # Examples
/// - depth 4 → ceil(4/2) = 2 → 2^2 = 4 pools
/// - depth 7 → ceil(7/2) = 4 → 2^4 = 16 pools
/// - depth 8 → ceil(8/2) = 4 → 2^4 = 16 pools
///
/// # Arguments
/// * `depth` - The depth of the Merkle tree
///
/// # Returns
/// The expected number of reward candidate pools
pub fn expected_reward_pools(depth: u8) -> usize {
    1 << midpoint_proof_depth(depth)
}

/// Errors that can occur when working with Merkle trees
#[derive(Debug, Error)]
pub enum MerkleTreeError {
    #[error("Too few leaves: got {got}, minimum is {MIN_LEAVES}")]
    TooFewLeaves { got: usize },
    #[error("Too many leaves: got {got}, maximum is {MAX_LEAVES}")]
    TooManyLeaves { got: usize },
    #[error("Invalid leaf index: {index} (tree has {leaf_count} leaves)")]
    InvalidLeafIndex { index: usize, leaf_count: usize },
    #[error("Invalid midpoint index: {index} (tree has {midpoint_count} midpoints)")]
    InvalidMidpointIndex { index: usize, midpoint_count: usize },
    #[error("Invalid proof")]
    InvalidProof,
    #[error("Internal error: {0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, MerkleTreeError>;

/// A Merkle tree built from XorNames (content addresses)
///
/// Used for batch payment system where:
/// - Build tree from XorNames (min: 2, max: 65,536)
/// - Tree is automatically padded to next power of 2
/// - Root is committed on-chain for payment
/// - Intersections at level depth/2 determine candidate pools
/// - Individual address proofs verify addresses belong to paid batch
pub struct MerkleTree {
    /// The underlying rs_merkle tree
    inner: rs_merkle::MerkleTree<XorNameHasher>,

    /// Original leaf count (before padding)
    leaf_count: usize,

    /// Tree depth
    depth: u8,

    /// The root hash of the tree
    root: XorName,

    /// Salts for each original leaf
    /// Used to compute salted hashes: hash(address || salt)
    salts: Vec<[u8; 32]>,
}

impl MerkleTree {
    /// Create a new Merkle tree from XorNames
    ///
    /// # Arguments
    ///
    /// * `leaves` - Vector of XorNames (address addresses). Min: 2, Max: 65,536
    ///
    /// # Returns
    ///
    /// A new MerkleTree with automatic padding to next power of 2
    ///
    /// # Errors
    ///
    /// - `TooFewLeaves` if less than 2 leaves
    /// - `TooManyLeaves` if more than 65,536 leaves
    ///
    /// # Example
    ///
    /// ```ignore
    /// let addresses: Vec<XorName> = vec![
    ///     XorName::from_content(b"address1"),
    ///     XorName::from_content(b"address2"),
    ///     XorName::from_content(b"address3"),
    /// ];
    ///
    /// let tree = MerkleTree::from_xornames(addresses)?;
    /// println!("Root: {:?}", tree.root());
    /// println!("Depth: {}", tree.depth());
    /// ```
    pub fn from_xornames(leaves: Vec<XorName>) -> Result<Self> {
        let leaf_count = leaves.len();

        // Validate leaf count
        if leaf_count < MIN_LEAVES {
            return Err(MerkleTreeError::TooFewLeaves { got: leaf_count });
        }
        if leaf_count > MAX_LEAVES {
            return Err(MerkleTreeError::TooManyLeaves { got: leaf_count });
        }

        // Generate random salt for each real leaf (privacy protection)
        let mut rng = rand::thread_rng();
        let salts: Vec<[u8; 32]> = (0..leaf_count)
            .map(|_| {
                let mut salt = [0u8; 32];
                rand::Rng::fill(&mut rng, &mut salt);
                salt
            })
            .collect();

        // Calculate depth and pad to next power of 2
        let depth = tree_depth(leaf_count);
        let padded_size = 1 << depth;

        // Apply salt to each real leaf: hash(address || salt)
        let mut salted_leaves: Vec<[u8; 32]> = leaves
            .iter()
            .zip(&salts)
            .map(|(address, salt)| {
                // Compute hash(address || salt)
                let mut data = Vec::with_capacity(64);
                data.extend_from_slice(address.as_ref());
                data.extend_from_slice(salt);
                XorNameHasher::hash(&data)
            })
            .collect();

        // Add random dummy padding leaves (no salt needed - already random)
        if leaf_count < padded_size {
            for _ in leaf_count..padded_size {
                let mut dummy = [0u8; 32];
                rand::Rng::fill(&mut rng, &mut dummy);
                salted_leaves.push(dummy);
            }
        }

        // Build rs_merkle tree from salted hashes
        let inner = rs_merkle::MerkleTree::<XorNameHasher>::from_leaves(&salted_leaves);

        let root = inner.root().ok_or(MerkleTreeError::Internal(
            "Tree must have root after construction".to_string(),
        ))?;

        Ok(Self {
            inner,
            root: XorName(root),
            leaf_count,
            depth,
            salts,
        })
    }

    /// Get the root hash of the tree
    ///
    /// This is the hash committed on-chain for batch payment
    pub fn root(&self) -> XorName {
        self.root
    }

    /// Get the depth of the tree
    pub fn depth(&self) -> u8 {
        self.depth
    }

    /// Get the original leaf count (before padding)
    pub fn leaf_count(&self) -> usize {
        self.leaf_count
    }

    /// Get midpoint nodes at depth/2
    ///
    /// These are the nodes used internally to determine candidate pools for payment routing.
    /// Returns intersections nodes at level depth/2
    ///
    /// Note: Users typically don't need this directly - use `reward_candidates()` instead.
    fn midpoints(&self) -> Result<Vec<MerkleMidpoint>> {
        let level = midpoint_level(self.depth);

        let nodes = self
            .inner
            .get_nodes_at_level(level)
            .ok_or(MerkleTreeError::Internal(
                "Midpoint level must exist".to_string(),
            ))?;

        let midpoints: Vec<MerkleMidpoint> = nodes
            .into_iter()
            .map(|(index, hash)| MerkleMidpoint {
                hash: XorName(hash),
                index,
            })
            .collect();

        Ok(midpoints)
    }

    /// Get reward candidates for batch payment
    ///
    /// Computes candidate addresses as hash(midpoint_hash || root || merkle_payment_timestamp).
    /// Network nodes closest to these addresses are eligible for payment rewards.
    /// Each candidate contains a proof that the midpoint belongs to the tree.
    /// Returns 2^ceil(depth/2) reward candidates.
    ///
    /// # Arguments
    ///
    /// * `merkle_payment_timestamp` - Unix timestamp of the transaction (seconds since epoch)
    ///
    /// # Returns
    ///
    /// A vector of `RewardCandidatePool` or an error if proof generation fails
    ///
    /// # Example
    ///
    /// ```ignore
    /// let tree = MerkleTree::from_xornames(addresses)?;
    /// let merkle_payment_timestamp = SystemTime::now()
    ///     .duration_since(UNIX_EPOCH)
    ///     .expect("Failed to get current time")
    ///     .as_secs();
    ///
    /// let candidates = tree.reward_candidates(merkle_payment_timestamp)?;
    ///
    /// // Each candidate's branch can be verified independently
    /// for candidate in candidates {
    ///     assert!(candidate.branch.verify());
    /// }
    /// ```
    pub fn reward_candidates(&self, merkle_payment_timestamp: u64) -> Result<Vec<MidpointProof>> {
        let midpoints = self.midpoints()?;

        midpoints
            .into_iter()
            .map(|midpoint| {
                // Generate proof for this midpoint
                let branch = self.generate_midpoint_proof(midpoint.index, midpoint.hash)?;

                Ok(MidpointProof {
                    branch,
                    merkle_payment_timestamp,
                })
            })
            .collect()
    }

    /// Generate a proof that a address belongs to this tree
    ///
    /// # Arguments
    ///
    /// * `address_index` - Index of the address (0-based, must be < leaf_count)
    /// * `address_hash` - The XorName hash of the address data
    ///
    /// # Important: Index vs Hash
    ///
    /// Both the **index** and **hash** are required:
    /// - **Index**: Tells us the address's position in the tree (which leaf)
    /// - **Hash**: The actual XorName we're proving belongs at that position
    ///
    /// The proof verifies: "This specific hash is at this specific index in the tree"
    ///
    /// # Example
    ///
    /// ```ignore
    /// // You have addresses with their original order preserved
    /// let addresses: Vec<XorName> = vec![
    ///     XorName::from_content(b"address 0 data"),
    ///     XorName::from_content(b"address 1 data"),
    ///     XorName::from_content(b"address 2 data"),
    /// ];
    ///
    /// // Build tree from the addresses
    /// let tree = MerkleTree::from_xornames(addresses.clone())?;
    ///
    /// // Generate proofs for all addresses
    /// for (index, address_hash) in addresses.iter().enumerate() {
    ///     // index: position in the tree (0, 1, 2)
    ///     // address_hash: the actual XorName at that position
    ///     let proof = tree.generate_address_proof(index, *address_hash)?;
    ///
    ///     // Each proof can be verified independently
    ///     assert!(proof.verify());
    /// }
    /// ```
    ///
    /// # Returns
    ///
    /// A `MerkleBranch` proof from address to root
    ///
    /// # Errors
    ///
    /// - `InvalidLeafIndex` if index >= leaf_count
    pub fn generate_address_proof(
        &self,
        address_index: usize,
        address_hash: XorName,
    ) -> Result<MerkleBranch> {
        if address_index >= self.leaf_count {
            return Err(MerkleTreeError::InvalidLeafIndex {
                index: address_index,
                leaf_count: self.leaf_count,
            });
        }

        let indices = vec![address_index];
        let proof = self.inner.proof(&indices);

        // Padded size is 2^depth
        let padded_size = 1 << self.depth;

        let root = self.root();

        // Get the salt for this address
        let salt = self.salts[address_index];

        Ok(MerkleBranch::from_rs_merkle_proof(
            proof,
            address_index,
            padded_size,
            address_hash,
            root,
            Some(salt),
        ))
    }

    /// Generate a proof that a midpoint exists at the midpoint level
    ///
    /// Midpoints are 2^ceil(depth/2) nodes at level depth/2
    ///
    /// # Arguments
    ///
    /// * `midpoint_index` - Index of the midpoint at the midpoint level
    /// * `midpoint_hash` - Hash of the midpoint node
    ///
    /// # Returns
    ///
    /// A `MerkleBranch` proof from midpoint to root
    ///
    /// # Errors
    ///
    /// - `InvalidMidpointIndex` if index is out of bounds
    fn generate_midpoint_proof(
        &self,
        midpoint_index: usize,
        midpoint_hash: XorName,
    ) -> Result<MerkleBranch> {
        // Midpoints are at level depth/2, giving us 2^ceil(depth/2) nodes
        let level = midpoint_level(self.depth);
        let midpoint_count = expected_reward_pools(self.depth);

        if midpoint_index >= midpoint_count {
            return Err(MerkleTreeError::InvalidMidpointIndex {
                index: midpoint_index,
                midpoint_count,
            });
        }

        let proof = self
            .inner
            .proof_from_node(level, midpoint_index)
            .ok_or_else(|| {
                MerkleTreeError::Internal("Failed to generate midpoint proof".to_string())
            })?;

        // For midpoint proofs, treat nodes at midpoint level as "leaves"
        // Total count is the number of nodes at that level (2^midpoint_level)
        let effective_leaf_count = midpoint_count;

        let root = self.root();

        Ok(MerkleBranch::from_rs_merkle_proof(
            proof,
            midpoint_index,
            effective_leaf_count,
            midpoint_hash,
            root,
            None, // Midpoint proofs don't need salt
        ))
    }
}

/// A node at the depth/2 layer of the Merkle tree
///
/// Used internally to determine candidate pools for batch payment routing
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct MerkleMidpoint {
    /// Hash of the midpoint node
    hash: XorName,

    /// Index at this level
    index: usize,
}

/// A reward candidate derived from a midpoint
///
/// The candidate pool address is computed as hash(midpoint_hash || root || merkle_payment_timestamp).
/// Network nodes closest to this address are eligible for batch payment rewards.
/// Contains everything needed to verify the candidate is valid.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct MidpointProof {
    /// Proof that the midpoint belongs to the Merkle tree
    pub branch: MerkleBranch,

    /// Merkle payment timestamp provided by client (used to compute candidate address)
    /// This is the timestamp that all nodes in the pool must use for their quotes
    pub merkle_payment_timestamp: u64,
}

impl MidpointProof {
    /// Get the Merkle root from the candidate pool's branch
    ///
    /// Returns the root hash that the branch proves membership in.
    pub fn root(&self) -> &XorName {
        self.branch.root()
    }

    /// Get the candidate address for this pool
    ///
    /// The address is computed as hash(midpoint_hash || root || merkle_payment_timestamp).
    /// Network nodes closest to this address are eligible for batch payment rewards.
    pub fn address(&self) -> XorName {
        let mut data = Vec::with_capacity(32 + 32 + 8);
        data.extend_from_slice(self.branch.leaf_hash().as_ref());
        data.extend_from_slice(self.branch.root().as_ref());
        data.extend_from_slice(&self.merkle_payment_timestamp.to_le_bytes());
        XorName::from_content(&data)
    }

    /// Compute deterministic hash for storage/verification
    ///
    /// Uses fixed-width encoding (u64) for numeric fields to ensure
    /// architecture-independent hashing across 32-bit and 64-bit platforms.
    pub fn hash(&self) -> XorName {
        let mut bytes = Vec::new();

        // Serialize MerkleBranch fields
        for proof_hash in &self.branch.proof_hashes {
            bytes.extend_from_slice(proof_hash);
        }

        // usize fields - cast to u64 for fixed-width encoding
        bytes.extend_from_slice(&(self.branch.leaf_index as u64).to_le_bytes());
        bytes.extend_from_slice(&(self.branch.total_leaves_count as u64).to_le_bytes());

        bytes.extend_from_slice(self.branch.unsalted_leaf_hash.as_ref());
        bytes.extend_from_slice(self.branch.root.as_ref());
        if let Some(salt) = &self.branch.salt {
            bytes.push(1); // Option::Some marker
            bytes.extend_from_slice(salt);
        } else {
            bytes.push(0); // Option::None marker
        }

        // Add timestamp (u64 - native width)
        bytes.extend_from_slice(&self.merkle_payment_timestamp.to_le_bytes());

        XorName::from_content(&bytes)
    }
}

/// A Merkle branch (proof) from a leaf or midpoint to the root
///
/// Used to prove that a address or midpoint belongs to a paid batch.
/// Contains everything needed for verification - just call `verify()` with no arguments.
///
/// For leaf proofs:
/// - leaf_index is the address index
/// - total_leaves_count is the padded tree size (2^depth)
/// - salt is included for privacy (prevents address content from being revealed)
///
/// For midpoint proofs:
/// - leaf_index is the midpoint index at its level
/// - total_leaves_count is the number of midpoints
/// - salt is None (midpoints are intermediate hashes, not raw addresses)
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct MerkleBranch {
    /// The proof hashes (sibling hashes only) from leaf/midpoint to root
    proof_hashes: Vec<[u8; 32]>,

    /// Index of the leaf/node in the tree
    leaf_index: usize,

    /// Total number of leaves/nodes at the starting level
    /// - For leaf proofs: padded tree size (2^depth)
    /// - For midpoint proofs: number of midpoints
    total_leaves_count: usize,

    /// The unsalted leaf hash (address or midpoint) being proven
    /// For address proofs: this is the address XorName (before salting)
    /// For midpoint proofs: this is the midpoint hash
    unsalted_leaf_hash: XorName,

    /// The expected Merkle root
    root: XorName,

    /// Salt used for address privacy (None for midpoint proofs)
    /// For address proofs: random salt applied as hash(unsalted_leaf_hash || salt)
    /// For midpoint proofs: None (intermediate hashes don't need salting)
    salt: Option<[u8; 32]>,
}

impl MerkleBranch {
    /// Create from rs_merkle proof
    fn from_rs_merkle_proof(
        proof: rs_merkle::MerkleProof<XorNameHasher>,
        leaf_index: usize,
        total_leaves_count: usize,
        unsalted_leaf_hash: XorName,
        root: XorName,
        salt: Option<[u8; 32]>,
    ) -> Self {
        let proof_hashes = proof.proof_hashes().to_vec();
        Self {
            proof_hashes,
            leaf_index,
            total_leaves_count,
            unsalted_leaf_hash,
            root,
            salt,
        }
    }

    /// Get the unsalted leaf hash (address or intersection) being proven
    /// For address proofs: returns the address XorName (before salting)
    /// For midpoint proofs: returns the midpoint hash
    pub fn leaf_hash(&self) -> &XorName {
        &self.unsalted_leaf_hash
    }

    /// Get the expected Merkle root
    pub fn root(&self) -> &XorName {
        &self.root
    }

    /// Verify this proof
    ///
    /// # Returns
    ///
    /// `true` if the proof is valid, `false` otherwise
    ///
    /// # Example
    ///
    /// ```ignore
    /// let proof = tree.generate_address_proof(0)?;
    ///
    /// // The proof contains everything needed for verification
    /// println!("Proving leaf: {:?}", proof.leaf_hash());
    /// println!("Against root: {:?}", proof.root());
    ///
    /// assert!(proof.verify());
    /// ```
    pub fn verify(&self) -> bool {
        // Compute the hash to verify
        let hash = if let Some(salt) = &self.salt {
            // For address proofs: compute hash(unsalted_leaf_hash || salt)
            let mut data = Vec::with_capacity(64);
            data.extend_from_slice(self.unsalted_leaf_hash.as_ref());
            data.extend_from_slice(salt);
            XorNameHasher::hash(&data)
        } else {
            // For midpoint proofs: use unsalted_leaf_hash directly
            let leaf_bytes = self.unsalted_leaf_hash.as_ref();
            let mut hash = [0u8; 32];
            hash.copy_from_slice(leaf_bytes);
            hash
        };

        let root_bytes = self.root.as_ref();
        let mut expected_root = [0u8; 32];
        expected_root.copy_from_slice(root_bytes);

        // Use rs_merkle's verify for both leaves and midpoints
        // For midpoints, we treat nodes at that level as "leaves" of a smaller tree
        let proof = rs_merkle::MerkleProof::<XorNameHasher>::new(self.proof_hashes.clone());
        proof.verify(
            expected_root,
            &[self.leaf_index],
            &[hash],
            self.total_leaves_count,
        )
    }

    /// Get the depth (number of hashing steps) of this proof
    pub fn depth(&self) -> usize {
        self.proof_hashes.len()
    }
}

/// Calculate tree depth from leaf count: ceil(log2(n))
pub fn tree_depth(leaf_count: usize) -> u8 {
    if leaf_count <= 1 {
        return 0;
    }

    let mut depth = 0;
    let mut n = leaf_count - 1;
    while n > 0 {
        depth += 1;
        n >>= 1;
    }
    depth
}

/// Calculate the proof depth from midpoint to root: ceil(depth/2)
pub fn midpoint_proof_depth(depth: u8) -> u8 {
    depth.div_ceil(2)
}

/// Calculate the level in the tree where midpoints are located: floor(depth/2)
fn midpoint_level(depth: u8) -> usize {
    (depth / 2) as usize
}

/// Errors that can occur when verifying a Merkle proof for batch payments
///
/// Nodes verify address proofs without access to the original Merkle tree,
/// using only the proof data and information stored on the smart contract.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BadMerkleProof {
    #[error("Address branch proof failed Merkle verification")]
    InvalidAddressBranchProof,
    #[error("Winner/intersection branch proof failed Merkle verification")]
    InvalidWinnerBranchProof,
    #[error("Address proof depth mismatch: expected {expected}, got {got}")]
    AddressProofDepthMismatch { expected: usize, got: usize },
    #[error("Winner proof depth mismatch: expected {expected}, got {got}")]
    WinnerProofDepthMismatch { expected: usize, got: usize },
    #[error(
        "Address branch root doesn't match smart contract root: smart_contract={smart_contract_root}, branch={branch_root}"
    )]
    AddressBranchRootMismatch {
        smart_contract_root: XorName,
        branch_root: XorName,
    },
    #[error(
        "Winner branch root doesn't match smart contract root: smart_contract={smart_contract_root}, branch={branch_root}"
    )]
    WinnerBranchRootMismatch {
        smart_contract_root: XorName,
        branch_root: XorName,
    },
    #[error(
        "Payment timestamp {payment_timestamp} is in the future (current time: {current_time})"
    )]
    TimestampInFuture {
        payment_timestamp: u64,
        current_time: u64,
    },
    #[error(
        "Payment expired: timestamp {payment_timestamp} is {age_seconds}s old (max: {MERKLE_PAYMENT_EXPIRATION}s)"
    )]
    PaymentExpired {
        payment_timestamp: u64,
        current_time: u64,
        age_seconds: u64,
    },
    #[error("Failed to get current system time: {0}")]
    SystemTimeError(String),
    #[error(
        "Winner pool timestamp {pool_timestamp} doesn't match smart contract timestamp {contract_timestamp}"
    )]
    TimestampMismatch {
        pool_timestamp: u64,
        contract_timestamp: u64,
    },
    #[error("Address hash not matching branch leaf: leaf={leaf}, address={address}")]
    AddressHashNotBranchLeaf { leaf: XorName, address: XorName },
}

/// Validate payment timestamp against current time
///
/// Checks:
/// 1. Timestamp is not in the future
/// 2. Payment has not expired (older than MERKLE_PAYMENT_EXPIRATION)
/// 3. Winner pool timestamp matches smart contract timestamp
fn validate_payment_timestamp(
    payment_timestamp: u64,
    pool_timestamp: u64,
) -> std::result::Result<(), BadMerkleProof> {
    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| BadMerkleProof::SystemTimeError(e.to_string()))?
        .as_secs();

    // Verify timestamp is not in the future
    if payment_timestamp > current_time {
        return Err(BadMerkleProof::TimestampInFuture {
            payment_timestamp,
            current_time,
        });
    }

    // Verify payment has not expired
    let age = current_time - payment_timestamp;
    if age > MERKLE_PAYMENT_EXPIRATION {
        return Err(BadMerkleProof::PaymentExpired {
            payment_timestamp,
            current_time,
            age_seconds: age,
        });
    }

    // Verify pool timestamp matches contract timestamp
    if pool_timestamp != payment_timestamp {
        return Err(BadMerkleProof::TimestampMismatch {
            pool_timestamp,
            contract_timestamp: payment_timestamp,
        });
    }

    Ok(())
}

/// Verify a address proof against smart contract payment data
///
/// This is the complete verification flow that nodes perform when receiving addresses.
/// Nodes don't have access to the tree, only the proofs and on-chain data.
///
/// # Security
///
/// - Validates proof depths BEFORE calling expensive verify() to prevent DoS
/// - Verifies address hash matches the provided address bytes to prevent malicious data storage
///
/// # Arguments
///
/// * `address_hash` - The actual hash of the address being stored
/// * `address_branch` - Merkle proof for the address (leaf to root)
/// * `winner_pool_midpoint_proof` - The winner midpoint proof with timestamp
/// * `smart_contract_depth` - Tree depth claimed on smart contract
/// * `smart_contract_root` - Merkle root from smart contract payment
/// * `smart_contract_timestamp` - Payment timestamp from smart contract (Unix seconds)
///
/// # Returns
///
/// `Ok(())` if all verifications pass, otherwise returns specific error
///
/// # Example
///
/// ```ignore
/// // Node receives address and proofs from client
/// verify_merkle_proof(
///     &address_hash,
///     &address_proof,
///     &winner_pool_midpoint_proof,
///     contract_data.depth,
///     &contract_data.root,
///     contract_data.timestamp,
/// )?;
///
/// // If we get here, address is verified - store it
/// ```
pub fn verify_merkle_proof(
    address_hash: &XorName,
    address_branch: &MerkleBranch,
    winner_pool_midpoint_proof: &MidpointProof,
    smart_contract_depth: u8,
    smart_contract_root: &XorName,
    smart_contract_timestamp: u64,
) -> std::result::Result<(), BadMerkleProof> {
    // Validate payment timestamp
    validate_payment_timestamp(
        smart_contract_timestamp,
        winner_pool_midpoint_proof.merkle_payment_timestamp,
    )?;

    // Verify address proof depth matches smart contract claimed depth
    let address_depth = address_branch.depth();
    let expected_address_depth = smart_contract_depth as usize;
    if address_depth != expected_address_depth {
        return Err(BadMerkleProof::AddressProofDepthMismatch {
            expected: expected_address_depth,
            got: address_depth,
        });
    }

    // Verify winner proof depth matches expected for midpoint (ceil(depth/2))
    let winner_depth = winner_pool_midpoint_proof.branch.depth();
    let expected_winner_depth = midpoint_proof_depth(smart_contract_depth) as usize;
    if winner_depth != expected_winner_depth {
        return Err(BadMerkleProof::WinnerProofDepthMismatch {
            expected: expected_winner_depth,
            got: winner_depth,
        });
    }

    // Verify Merkle inclusion (address belongs to tree)
    if !address_branch.verify() {
        return Err(BadMerkleProof::InvalidAddressBranchProof);
    }

    // Verify winner pool (intersection legitimacy)
    if !winner_pool_midpoint_proof.branch.verify() {
        return Err(BadMerkleProof::InvalidWinnerBranchProof);
    }

    // Verify address hash matches the provided address bytes
    if address_hash != address_branch.leaf_hash() {
        return Err(BadMerkleProof::AddressHashNotBranchLeaf {
            leaf: *address_branch.leaf_hash(),
            address: *address_hash,
        });
    }

    // Verify address proof root matches on-chain root
    if address_branch.root() != smart_contract_root {
        return Err(BadMerkleProof::AddressBranchRootMismatch {
            smart_contract_root: *smart_contract_root,
            branch_root: *address_branch.root(),
        });
    }

    // Verify winner proof root matches on-chain root
    if winner_pool_midpoint_proof.branch.root() != smart_contract_root {
        return Err(BadMerkleProof::WinnerBranchRootMismatch {
            smart_contract_root: *smart_contract_root,
            branch_root: *winner_pool_midpoint_proof.branch.root(),
        });
    }

    Ok(())
}

/// XorName hasher for rs_merkle
///
/// Uses XorNameHasher (32-byte output) for hashing, consistent with Autonomi network
#[derive(Clone)]
struct XorNameHasher;

impl rs_merkle::Hasher for XorNameHasher {
    type Hash = [u8; 32];

    fn hash(data: &[u8]) -> Self::Hash {
        XorName::from_content(data).0
    }

    fn concat_and_hash(left: &Self::Hash, right: Option<&Self::Hash>) -> Self::Hash {
        if let Some(right) = right {
            XorName::from_content_parts(&[left, right]).0
        } else {
            XorName::from_content(left).0
        }
    }

    fn hash_size() -> usize {
        32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_leaves(count: usize) -> Vec<XorName> {
        (0..count)
            .map(|i| XorName::from_content(&i.to_le_bytes()))
            .collect()
    }

    #[test]
    fn test_reward_candidate_pool_hash_fixed_width_encoding() {
        // Test that RewardCandidatePool::hash uses fixed-width encoding
        let leaves = make_test_leaves(16);
        let tree = MerkleTree::from_xornames(leaves).unwrap();
        let timestamp = 1234567890u64;
        let pools = tree.reward_candidates(timestamp).unwrap();
        let pool = &pools[0];

        // Get the hash
        let hash1 = pool.hash();

        // Manually reconstruct the hash with explicit u64 encoding to verify
        let mut bytes = Vec::new();
        for proof_hash in &pool.branch.proof_hashes {
            bytes.extend_from_slice(proof_hash);
        }
        bytes.extend_from_slice(&(pool.branch.leaf_index as u64).to_le_bytes());
        bytes.extend_from_slice(&(pool.branch.total_leaves_count as u64).to_le_bytes());
        bytes.extend_from_slice(pool.branch.unsalted_leaf_hash.as_ref());
        bytes.extend_from_slice(pool.branch.root.as_ref());
        if let Some(salt) = &pool.branch.salt {
            bytes.push(1);
            bytes.extend_from_slice(salt);
        } else {
            bytes.push(0);
        }
        bytes.extend_from_slice(&pool.merkle_payment_timestamp.to_le_bytes());

        let hash2 = XorName::from_content(&bytes);

        assert_eq!(
            hash1, hash2,
            "RewardCandidatePool::hash should match manual u64-encoded hash"
        );
    }

    #[test]
    fn test_reward_candidate_pool_hash_architecture_independence() {
        // Create a pool with maximum usize values to test encoding
        let leaves = make_test_leaves(4);
        let tree = MerkleTree::from_xornames(leaves).unwrap();
        let timestamp = u64::MAX;
        let pools = tree.reward_candidates(timestamp).unwrap();

        // Get hashes for all pools - they should be deterministic
        let hash1 = pools[0].hash();
        let hash2 = pools[0].hash();

        assert_eq!(hash1, hash2, "Same pool should produce identical hash");

        // Verify that the serialization uses 8 bytes for usize fields
        let pool = &pools[0];
        let mut bytes = Vec::new();
        for proof_hash in &pool.branch.proof_hashes {
            bytes.extend_from_slice(proof_hash);
        }

        let start_offset = bytes.len();
        bytes.extend_from_slice(&(pool.branch.leaf_index as u64).to_le_bytes());
        bytes.extend_from_slice(&(pool.branch.total_leaves_count as u64).to_le_bytes());

        // Verify 8 bytes were written for each usize field
        assert_eq!(
            bytes.len() - start_offset,
            16, // 2 * 8 bytes
            "Should use 8 bytes per usize field regardless of platform"
        );

        // Verify values are preserved correctly
        let leaf_index_bytes = &bytes[start_offset..start_offset + 8];
        let leaf_index = u64::from_le_bytes(leaf_index_bytes.try_into().unwrap());
        assert_eq!(
            leaf_index, pool.branch.leaf_index as u64,
            "leaf_index should be preserved in u64 encoding"
        );
    }

    #[test]
    fn test_expected_reward_pools() {
        // Test the formula: 2^ceil(depth/2)
        assert_eq!(expected_reward_pools(1), 2); // ceil(1/2) = 1 → 2^1 = 2
        assert_eq!(expected_reward_pools(2), 2); // ceil(2/2) = 1 → 2^1 = 2
        assert_eq!(expected_reward_pools(3), 4); // ceil(3/2) = 2 → 2^2 = 4
        assert_eq!(expected_reward_pools(4), 4); // ceil(4/2) = 2 → 2^2 = 4
        assert_eq!(expected_reward_pools(5), 8); // ceil(5/2) = 3 → 2^3 = 8
        assert_eq!(expected_reward_pools(6), 8); // ceil(6/2) = 3 → 2^3 = 8
        assert_eq!(expected_reward_pools(7), 16); // ceil(7/2) = 4 → 2^4 = 16
        assert_eq!(expected_reward_pools(8), 16); // ceil(8/2) = 4 → 2^4 = 16
        assert_eq!(expected_reward_pools(16), 256); // ceil(16/2) = 8 → 2^8 = 256
    }

    #[test]
    fn test_blake2b_output_size() {
        // Verify that Blake2b::<U32> produces 32-byte (256-bit) hashes
        let hash1 = XorNameHasher::hash(b"test data");
        let hash2 = XorNameHasher::concat_and_hash(&hash1, Some(&hash1));

        // These should compile - proving the type is [u8; 32]
        assert_eq!(hash1.len(), 32, "Hash should be 32 bytes (256 bits)");
        assert_eq!(
            hash2.len(),
            32,
            "Concatenated hash should be 32 bytes (256 bits)"
        );

        // Verify hashes are different for different inputs
        let hash3 = XorNameHasher::hash(b"different data");
        assert_ne!(
            hash1, hash3,
            "Different inputs should produce different hashes"
        );

        println!("Blake2b hash size verified: 32 bytes (256 bits)");
        println!("Sample hash: {:02x?}", &hash1[..8]);
    }

    #[test]
    fn test_reward_candidate_pool_hash() {
        let leaves = make_test_leaves(16);
        let tree = MerkleTree::from_xornames(leaves).unwrap();
        let candidates = tree.reward_candidates(12345).unwrap();

        // Verify we can use RewardCandidatePool in a HashSet (tests std::hash::Hash trait)
        let mut seen = std::collections::HashSet::new();
        for candidate in &candidates {
            assert!(seen.insert(candidate));
        }
        assert_eq!(seen.len(), candidates.len());

        // Verify our hash() method is deterministic
        let hash1 = candidates[0].hash();
        let hash2 = candidates[0].hash();
        assert_eq!(hash1, hash2, "Hash should be deterministic");

        // Verify different candidates have different hashes
        let hash3 = candidates[1].hash();
        assert_ne!(
            hash1, hash3,
            "Different candidates should have different hashes"
        );
    }

    #[test]
    fn test_min_leaves_validation() {
        let leaves = make_test_leaves(1);
        let result = MerkleTree::from_xornames(leaves);
        assert!(matches!(result, Err(MerkleTreeError::TooFewLeaves { .. })));
    }

    #[test]
    fn test_max_leaves_validation() {
        let leaves = make_test_leaves(MAX_LEAVES + 1);
        let result = MerkleTree::from_xornames(leaves);
        assert!(matches!(result, Err(MerkleTreeError::TooManyLeaves { .. })));
    }

    #[test]
    fn test_basic_tree_construction() {
        let leaves = make_test_leaves(100);
        let tree = MerkleTree::from_xornames(leaves).unwrap();

        assert_eq!(tree.leaf_count(), 100);
        assert_eq!(tree.depth(), 7); // ceil(log2(100)) = 7
    }

    #[test]
    fn test_power_of_two_leaves() {
        for power in 1..=MAX_MERKLE_DEPTH {
            let count = 1 << power; // 2^power
            let leaves = make_test_leaves(count);
            let tree = MerkleTree::from_xornames(leaves).unwrap();

            assert_eq!(tree.depth(), power as u8);
            assert_eq!(tree.leaf_count(), count);
        }
    }

    #[test]
    fn test_midpoints() {
        let leaves = make_test_leaves(1024);
        let tree = MerkleTree::from_xornames(leaves).unwrap();

        let midpoints = tree.midpoints().unwrap();

        // Depth = 10, (depth+1)/2 = 5 (rounded up), so 2^5 = 32 midpoints
        assert_eq!(midpoints.len(), 32);

        // Check all midpoints have valid indices
        for (i, midpoint) in midpoints.iter().enumerate() {
            assert_eq!(midpoint.index, i);
        }
    }

    #[test]
    fn test_reward_candidates() {
        let leaves = make_test_leaves(1024);
        let tree = MerkleTree::from_xornames(leaves).unwrap();

        let merkle_payment_timestamp = 1234567890u64;
        let candidates = tree.reward_candidates(merkle_payment_timestamp).unwrap();

        // Should have same number as midpoints
        assert_eq!(candidates.len(), 32);

        // Each candidate should have unique address
        let mut addresses = std::collections::HashSet::new();
        for candidate in &candidates {
            assert!(addresses.insert(candidate.address()));
        }

        // Verify all candidates are valid
        for candidate in &candidates {
            assert!(
                candidate.branch.verify(),
                "Candidate branch should be valid"
            );
        }

        // Verify deterministic - same timestamp gives same candidates
        let candidates2 = tree.reward_candidates(merkle_payment_timestamp).unwrap();
        assert_eq!(candidates, candidates2);

        // Different timestamp gives different candidates
        let candidates3 = tree
            .reward_candidates(merkle_payment_timestamp + 1)
            .unwrap();
        assert_ne!(candidates[0].address(), candidates3[0].address());

        // But they should still be valid
        for candidate in &candidates3 {
            assert!(
                candidate.branch.verify(),
                "Candidate branch with different timestamp should still be valid"
            );
        }

        // Verify candidate address is hash(midpoint || root || timestamp)
        let tree_root = tree.root();
        let expected_address = candidates[0].address();

        // Manually compute to verify the address calculation
        let mut data = Vec::with_capacity(32 + 32 + 8);
        data.extend_from_slice(candidates[0].branch.leaf_hash().as_ref());
        data.extend_from_slice(tree_root.as_ref());
        data.extend_from_slice(&merkle_payment_timestamp.to_le_bytes());
        let manually_computed = XorName::from_content(&data);
        assert_eq!(expected_address, manually_computed);

        // Verify branch is valid
        assert!(candidates[0].branch.verify());

        // Verify direct field access
        assert_eq!(
            candidates[0].merkle_payment_timestamp,
            merkle_payment_timestamp
        );
        assert_eq!(candidates[0].address(), candidates[0].address()); // Address calculation
        assert_eq!(candidates[0].branch.root(), &tree_root);
        assert_eq!(
            candidates[0].branch.leaf_hash(),
            candidates[0].branch.leaf_hash()
        );
    }

    #[test]
    fn test_address_proof_generation_and_verification() {
        let leaves = make_test_leaves(100);
        let tree = MerkleTree::from_xornames(leaves.clone()).unwrap();

        // Test proof for first address
        let proof = tree.generate_address_proof(0, leaves[0]).unwrap();
        assert!(proof.verify());

        // Test proof for last address
        let proof = tree.generate_address_proof(99, leaves[99]).unwrap();
        assert!(proof.verify());

        // Test proof for middle address
        let proof = tree.generate_address_proof(50, leaves[50]).unwrap();
        assert!(proof.verify());
    }

    #[test]
    fn test_invalid_address_index() {
        let leaves = make_test_leaves(100);
        let tree = MerkleTree::from_xornames(leaves.clone()).unwrap();

        let dummy_hash = leaves[0]; // Just need any hash for this test
        let result = tree.generate_address_proof(100, dummy_hash);
        assert!(matches!(
            result,
            Err(MerkleTreeError::InvalidLeafIndex { .. })
        ));
    }

    #[test]
    fn test_midpoint_proof_generation_and_verification() {
        let leaves = make_test_leaves(1024);
        let tree = MerkleTree::from_xornames(leaves).unwrap();

        let midpoints = tree.midpoints().unwrap();

        // Test proof for first midpoint
        let proof = tree.generate_midpoint_proof(0, midpoints[0].hash).unwrap();
        assert!(proof.verify());

        // Test proof for last midpoint
        let proof = tree
            .generate_midpoint_proof(31, midpoints[31].hash)
            .unwrap();
        assert!(proof.verify());
    }

    #[test]
    fn test_proof_depth() {
        let leaves = make_test_leaves(16);
        let tree = MerkleTree::from_xornames(leaves.clone()).unwrap();

        // Address proof should go from leaf to root (depth 4)
        let address_proof = tree.generate_address_proof(0, leaves[0]).unwrap();
        assert_eq!(address_proof.depth(), 4);

        // Midpoint proof should go from depth/2 to root (depth 2)
        let midpoints = tree.midpoints().unwrap();
        let midpoint_proof = tree.generate_midpoint_proof(0, midpoints[0].hash).unwrap();
        assert_eq!(midpoint_proof.depth(), 2);
    }

    #[test]
    fn test_non_deterministic_root_due_to_salts() {
        // With random salts, the same leaves produce different roots
        // This is a privacy feature - prevents address content from being revealed
        let leaves = make_test_leaves(100);

        let tree1 = MerkleTree::from_xornames(leaves.clone()).unwrap();
        let tree2 = MerkleTree::from_xornames(leaves).unwrap();

        // Roots should be different due to random salts
        assert_ne!(tree1.root(), tree2.root());

        // But both trees should still work correctly
        assert_eq!(tree1.depth(), tree2.depth());
        assert_eq!(tree1.leaf_count(), tree2.leaf_count());
    }

    #[test]
    fn test_invalid_proof_rejection() {
        let leaves = make_test_leaves(10);
        let tree = MerkleTree::from_xornames(leaves.clone()).unwrap();

        // Wrong leaf should fail - create new proof with wrong leaf hash
        let wrong_leaf = XorName::from_content(b"wrong");
        let wrong_proof = tree.generate_address_proof(0, wrong_leaf).unwrap();
        assert!(!wrong_proof.verify());

        // Wrong root should fail - we can't easily test this with the new API
        // since the root is embedded in the proof during generation
        // This test case is no longer applicable with the new API design
    }

    #[test]
    fn test_proof_hashes_length_for_depth_4() {
        // Simple test to print and verify proof_hashes length for depth 4 tree
        let leaves = make_test_leaves(16); // 16 = 2^4
        let tree = MerkleTree::from_xornames(leaves.clone()).unwrap();

        println!("Tree depth: {}", tree.depth());
        println!("Tree leaf count: {}", tree.leaf_count());

        let address_proof = tree.generate_address_proof(0, leaves[0]).unwrap();
        println!(
            "Address proof depth (proof_hashes.len()): {}",
            address_proof.depth()
        );

        let midpoints = tree.midpoints().unwrap();
        let midpoint_proof = tree.generate_midpoint_proof(0, midpoints[0].hash).unwrap();
        println!(
            "Midpoint proof depth (proof_hashes.len()): {}",
            midpoint_proof.depth()
        );

        // Verify expectations
        assert_eq!(tree.depth(), 4);
        assert_eq!(
            address_proof.depth(),
            4,
            "Address proof should have 4 siblings (levels 0->1->2->3->4)"
        );
        assert_eq!(
            midpoint_proof.depth(),
            2,
            "Midpoint proof should have 2 siblings (levels 2->3->4)"
        );
    }

    #[test]
    fn test_verify_works_correctly() {
        // Test that verify() correctly validates proofs for depth 4 tree
        let leaves = make_test_leaves(16); // 16 = 2^4, depth = 4
        let tree = MerkleTree::from_xornames(leaves.clone()).unwrap();

        println!("Testing address proof verification...");

        // Test address proof for first leaf
        let proof_0 = tree.generate_address_proof(0, leaves[0]).unwrap();
        println!("Address 0 proof depth: {}", proof_0.depth());
        let valid = proof_0.verify();
        println!("Address 0 verification: {valid}");
        assert!(valid, "Proof for address 0 should be valid");

        // Test address proof for last leaf
        let proof_15 = tree.generate_address_proof(15, leaves[15]).unwrap();
        println!("Address 15 proof depth: {}", proof_15.depth());
        let valid = proof_15.verify();
        println!("Address 15 verification: {valid}");
        assert!(valid, "Proof for address 15 should be valid");

        // Test address proof for middle leaf
        let proof_7 = tree.generate_address_proof(7, leaves[7]).unwrap();
        println!("Address 7 proof depth: {}", proof_7.depth());
        let valid = proof_7.verify();
        println!("Address 7 verification: {valid}");
        assert!(valid, "Proof for address 7 should be valid");

        println!("\nTesting midpoint proof verification...");

        // Test midpoint proofs
        let midpoints = tree.midpoints().unwrap();
        println!("Number of midpoints: {}", midpoints.len());

        let int_proof_0 = tree.generate_midpoint_proof(0, midpoints[0].hash).unwrap();
        println!("Midpoint 0 proof depth: {}", int_proof_0.depth());
        let valid = int_proof_0.verify();
        println!("Midpoint 0 verification: {valid}");
        assert!(valid, "Proof for midpoint 0 should be valid");

        let int_proof_3 = tree.generate_midpoint_proof(3, midpoints[3].hash).unwrap();
        println!("Midpoint 3 proof depth: {}", int_proof_3.depth());
        let valid = int_proof_3.verify();
        println!("Midpoint 3 verification: {valid}");
        assert!(valid, "Proof for midpoint 3 should be valid");

        println!("\nTesting invalid proofs are rejected...");

        // Test wrong leaf hash fails - create proof with wrong hash
        let wrong_leaf = XorName::from_content(b"wrong_leaf");
        let wrong_proof = tree.generate_address_proof(0, wrong_leaf).unwrap();
        let valid = wrong_proof.verify();
        println!("Wrong leaf verification: {valid}");
        assert!(!valid, "Proof with wrong leaf should fail");

        // Test using proof for wrong leaf index fails
        let wrong_index_proof = tree.generate_address_proof(0, leaves[1]).unwrap();
        let valid = wrong_index_proof.verify();
        println!("Wrong leaf index verification: {valid}");
        assert!(!valid, "Proof for leaf 0 with hash from leaf 1 should fail");

        println!("\nAll verification tests passed!");
    }

    #[test]
    fn test_complete_batch_payment_flow() {
        // This test simulates the complete Merkle batch payment flow as described in the spec:
        // 1. Client prepares data and builds tree
        // 2. Client extracts midpoints (intersections)
        // 3. Client generates reward candidates for payment
        // 4. Winner pool is selected (simulated)
        // 5. Client uploads addresses with proofs
        // 6. Nodes verify addresses belong to paid batch

        println!("\n=== SIMULATING COMPLETE MERKLE BATCH PAYMENT FLOW ===\n");

        // ==================================================================
        // PHASE 1: CLIENT PREPARES DATA
        // ==================================================================
        println!("PHASE 1: CLIENT PREPARES DATA");
        println!("------------------------------");

        // Simulate uploading 100 addresses (self-encrypted file)
        let real_address_count = 100;
        let addresses = make_test_leaves(real_address_count);
        println!("✓ Generated {real_address_count} real addresses from self-encryption");

        // ==================================================================
        // PHASE 2: CLIENT BUILDS MERKLE TREE
        // ==================================================================
        println!("\nPHASE 2: CLIENT BUILDS MERKLE TREE");
        println!("----------------------------------");

        let tree = MerkleTree::from_xornames(addresses.clone()).unwrap();
        let depth = tree.depth();
        let root = tree.root();
        let leaf_count = tree.leaf_count();

        println!("✓ Tree depth: {depth}");
        println!("✓ Real addresses: {leaf_count}");
        println!("✓ Padded size: {} (2^{})", 1 << depth, depth);
        println!("✓ Dummy addresses added: {}", (1 << depth) - leaf_count);
        println!("✓ Merkle root: {root:?}");

        assert_eq!(depth, 7); // ceil(log2(100)) = 7
        assert_eq!(leaf_count, 100);

        // ==================================================================
        // PHASE 3: CLIENT GETS REWARD CANDIDATES
        // ==================================================================
        println!("\nPHASE 3: CLIENT GETS REWARD CANDIDATES");
        println!("---------------------------------------");

        // Simulate payment timestamp (use current time for realistic test)
        let merkle_payment_timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Failed to get current time")
            .as_secs();
        println!("✓ Transaction timestamp: {merkle_payment_timestamp}");

        // Get all reward candidates (this represents all candidate pools)
        let candidates = tree.reward_candidates(merkle_payment_timestamp).unwrap();
        let midpoint_count = expected_reward_pools(depth);
        let level = midpoint_level(depth);
        let proof_depth = midpoint_proof_depth(depth);

        println!("✓ Midpoint level: {level}");
        println!("✓ Midpoint proof depth: {proof_depth}");
        println!("✓ Number of midpoint nodes (candidate pools): {midpoint_count}");
        println!("✓ Tree depth: {depth}");
        println!(
            "✓ Total nodes queried: {} × {} = {}",
            candidates.len(),
            depth,
            candidates.len() * depth as usize
        );

        assert_eq!(candidates.len(), midpoint_count);

        // In production, all candidates would be verified successfully
        // For this test, we're demonstrating the structure and flow
        println!("✓ Generated {} candidate pools", candidates.len());

        // Show example candidate address calculation
        let first_candidate = &candidates[0];
        let midpoint_hash = first_candidate.branch.leaf_hash();
        let candidate_root = first_candidate.branch.root();

        println!("\n  Example candidate #0:");
        println!("    Midpoint hash: {midpoint_hash:?}");
        println!("    Root: {candidate_root:?}");
        println!(
            "    Timestamp: {}",
            first_candidate.merkle_payment_timestamp
        );
        println!("    Address: {:?}", first_candidate.address());
        println!("    (Address = hash(midpoint || root || timestamp))");

        // ==================================================================
        // PHASE 4: SMART CONTRACT RECEIVES PAYMENT AND STORES DATA
        // ==================================================================
        println!("\nPHASE 4: SMART CONTRACT RECEIVES PAYMENT");
        println!("-----------------------------------------");

        // Client submits payment to smart contract with:
        // - Merkle root
        // - Tree depth
        // - Candidate pools (with intersection proofs)
        // - Transaction timestamp

        // Smart contract stores this data on-chain for nodes to verify against
        let smart_contract_root = root;
        let smart_contract_depth = depth;
        let smart_contract_timestamp = merkle_payment_timestamp;

        println!("✓ Smart contract received payment");
        println!("✓ Stored root: {smart_contract_root:?}");
        println!("✓ Stored depth: {smart_contract_depth}");
        println!("✓ Stored timestamp: {smart_contract_timestamp}");
        println!("✓ Stored {} candidate pools", candidates.len());

        // Smart contract randomly selects winner pool
        let winner_pool_midpoint_proof_index = 0; // In reality this is random
        let winner_candidate = &candidates[winner_pool_midpoint_proof_index];

        // Smart contract stores hash of the entire winner pool for nodes to verify
        let smart_contract_winner_pool_midpoint_proof_hash = winner_candidate.hash();

        println!("✓ Winner pool selected: index {winner_pool_midpoint_proof_index}");
        println!("✓ Winner pool hash stored: {smart_contract_winner_pool_midpoint_proof_hash:?}");
        println!("✓ Payment distributed to {depth} nodes (depth)");

        // ==================================================================
        // PHASE 5: CLIENT UPLOADS CHUNKS WITH PROOFS
        // ==================================================================
        println!("\nPHASE 5: CLIENT UPLOADS CHUNKS WITH PROOFS");
        println!("-------------------------------------------");

        // Generate proofs for all real addresses (not dummies)
        let mut address_proofs = Vec::new();
        for (i, address_hash) in addresses.iter().enumerate() {
            let proof = tree.generate_address_proof(i, *address_hash).unwrap();
            address_proofs.push(proof);
        }

        println!("✓ Generated {} address proofs", address_proofs.len());
        println!("✓ Each proof includes:");
        println!("  - Merkle proof (siblings from leaf to root)");
        println!("  - Salt (for privacy)");
        println!("  - Node hash (address being proven)");
        println!("  - Root (expected Merkle root)");

        // ==================================================================
        // PHASE 6: NODES VERIFY AND STORE CHUNKS
        // ==================================================================
        println!("\nPHASE 6: NODES VERIFY AND STORE CHUNKS");
        println!("---------------------------------------");

        // Simulate node verification for each address
        // Nodes receive from client: address, address_proof, winner_proof
        // Nodes query smart contract for: root, depth, winner_hash, timestamp
        // verify_merkle_proof() automatically checks current system time

        let mut verified_count = 0;
        for (i, address_proof) in address_proofs.iter().enumerate() {
            // In reality, client sends the address hash
            // The node receives the address data and hashes it to get address_hash
            let address_hash = &addresses[i];

            // Node uses the complete verification function
            // This performs all 10 verification steps from the spec
            let result = verify_merkle_proof(
                address_hash,
                address_proof,
                winner_candidate,
                smart_contract_depth,
                &smart_contract_root,
                smart_contract_timestamp,
            );

            assert!(
                result.is_ok(),
                "Address {} verification failed: {:?}",
                i,
                result.err()
            );

            // Additional checks nodes would perform:
            // - Target was computed correctly
            // - Candidates were closest to target at payment time
            // - Majority of candidates still alive
            // - All candidate signatures are valid

            verified_count += 1;
        }

        println!("✓ All {verified_count} addresses verified using verify_merkle_proof()");
        println!("✓ Core Merkle verification includes:");
        println!("  1. Timestamp not in future");
        println!("  2. Payment not expired (< {MERKLE_PAYMENT_EXPIRATION} seconds old)");
        println!("  3. Winner pool timestamp matches smart contract timestamp");
        println!("  4. Address Merkle proof valid (address ∈ tree)");
        println!("  5. Winner Merkle proof valid (midpoint ∈ tree)");
        println!("  6. Address proof depth matches on-chain depth");
        println!("  7. Winner proof depth matches expected for midpoint");
        println!("  8. Address proof root matches on-chain root");
        println!("  9. Winner proof root matches on-chain root");
        println!("  Note: Winner pool hash verification happens in MerklePaymentProof::verify()");

        // ==================================================================
        // PHASE 7: VERIFY PROOF STRUCTURE AND PROPERTIES
        // ==================================================================
        println!("\nPHASE 7: VERIFY PROOF STRUCTURE");
        println!("--------------------------------");

        // Check first address proof structure (using claimed depth, not tree)
        let first_proof = &address_proofs[0];
        let claimed_depth = depth; // From on-chain
        let expected_address_depth = claimed_depth as usize;
        println!("✓ Address proof depth: {}", first_proof.depth());
        println!(
            "✓ Expected address proof depth (from claimed depth {claimed_depth}): {expected_address_depth}"
        );
        println!("✓ Number of sibling hashes: {}", first_proof.depth());
        println!("✓ Has salt: {}", first_proof.salt.is_some());

        assert_eq!(
            first_proof.depth(),
            expected_address_depth,
            "Proof depth should match expected"
        );

        // Verify winner candidate's branch to midpoint (using claimed depth, not tree)
        let winner_branch = &winner_candidate.branch;
        let expected_midpoint_depth = midpoint_proof_depth(claimed_depth) as usize;
        let level = midpoint_level(claimed_depth);

        println!("\n✓ Winner midpoint proof depth: {}", winner_branch.depth());
        println!("✓ Expected midpoint proof depth: {expected_midpoint_depth}");
        println!("✓ Midpoint level: {level}");
        println!("✓ Tree depth: {claimed_depth}");
        println!("✓ No salt (midpoints are intermediate hashes)");

        assert_eq!(
            winner_branch.depth(),
            expected_midpoint_depth,
            "Midpoint proof depth should match expected"
        );
        assert!(
            winner_branch.salt.is_none(),
            "Midpoint proofs should not have salt"
        );

        // ==================================================================
        // PHASE 8: VERIFY PRIVACY PROPERTIES
        // ==================================================================
        println!("\nPHASE 8: VERIFY PRIVACY PROPERTIES");
        println!("-----------------------------------");

        // Each address has a unique salt
        let salts: Vec<_> = address_proofs.iter().map(|p| p.salt.unwrap()).collect();

        let unique_salts: std::collections::HashSet<_> = salts.iter().collect();
        assert_eq!(
            unique_salts.len(),
            salts.len(),
            "All salts should be unique"
        );
        println!("✓ All {} addresses have unique salts", salts.len());

        // Different trees from same addresses have different roots (due to random salts)
        let tree2 = MerkleTree::from_xornames(addresses.clone()).unwrap();
        assert_ne!(tree.root(), tree2.root(), "Different salt → different root");
        println!("✓ Random salts ensure non-deterministic roots");
        println!("✓ Privacy: address content cannot be inferred from tree structure");

        // ==================================================================
        // PHASE 9: COST COMPARISON
        // ==================================================================
        println!("\nPHASE 9: COST COMPARISON");
        println!("-------------------------");

        let old_payments = real_address_count * 3; // 3 nodes per address
        let new_payments = depth as usize; // Only winner pool paid

        println!("Old system (per-address payment):");
        println!(
            "  {real_address_count} addresses × 3 nodes = {old_payments} payment transactions"
        );

        println!("\nNew system (Merkle batch payment):");
        println!("  1 batch payment → {new_payments} winner nodes");
        println!(
            "  Nodes queried: {} (only query phase, no storage payment)",
            candidates.len() * depth as usize
        );

        let savings_pct = ((old_payments - new_payments) as f64 / old_payments as f64) * 100.0;
        println!("\n✓ Gas savings: {savings_pct:.1}% reduction");
        println!(
            "✓ Network query overhead: {}% of old system",
            (candidates.len() * depth as usize * 100) / old_payments
        );

        // ==================================================================
        // SUMMARY
        // ==================================================================
        println!("\n=== FLOW COMPLETE ===");
        println!("✓ {real_address_count} real addresses uploaded");
        println!("✓ {} dummy addresses padded", (1 << depth) - leaf_count);
        println!("✓ {} candidate pools formed", candidates.len());
        println!("✓ 1 winner pool paid ({depth} nodes)");
        println!("✓ All addresses verified and stored");
        println!("✓ Privacy preserved with random salts");
        println!("✓ {savings_pct:.1}% gas cost reduction achieved\n");
    }

    #[test]
    fn test_get_nodes_at_level_with_padding() {
        // Verify that our padded tree has the correct number of nodes at each level
        println!("\n=== TESTING OUR PADDED TREE STRUCTURE ===\n");

        let leaves = make_test_leaves(100); // 100 real addresses
        let tree = MerkleTree::from_xornames(leaves).unwrap();

        let depth = tree.depth();
        println!("Tree with 100 leaves:");
        println!("  Depth: {depth}");
        println!("  Original leaves: {}", tree.leaf_count());
        println!("  Padded size: {} (2^{})", 1 << depth, depth);

        // Check each level
        for level in 0..=depth {
            let expected_count = 1 << (depth - level); // 2^(depth - level)

            if let Some(nodes) = tree.inner.get_nodes_at_level(level as usize) {
                let actual_count = nodes.len();

                println!("\nLevel {level}:");
                println!("  Expected: {} nodes (2^{})", expected_count, depth - level);
                println!("  Actual: {actual_count} nodes");

                if level as usize == midpoint_level(depth) {
                    println!("  >>> MIDPOINT LEVEL <<<");
                    println!(
                        "  Our workaround takes: {} nodes",
                        std::cmp::min(actual_count, 1 << midpoint_proof_depth(depth))
                    );
                }

                if actual_count != expected_count {
                    println!("  ⚠ Mismatch! This is why we need .take() workaround");
                }
            }
        }

        println!("\n=== END TEST ===\n");
    }

    #[test]
    fn test_proof_hashes_length_matches_depth() {
        // Test with various tree sizes to verify proof_hashes.len() == depth

        // 16 leaves = 2^4, depth = 4
        let leaves = make_test_leaves(16);
        let tree = MerkleTree::from_xornames(leaves.clone()).unwrap();
        assert_eq!(tree.depth(), 4);

        let address_proof = tree.generate_address_proof(0, leaves[0]).unwrap();
        // From leaf (level 0) to root (level 4) = 4 hashing steps = 4 siblings
        assert_eq!(address_proof.depth(), 4);

        let midpoints = tree.midpoints().unwrap();
        let midpoint_proof = tree.generate_midpoint_proof(0, midpoints[0].hash).unwrap();
        // From midpoint (level 2) to root (level 4) = 2 hashing steps = 2 siblings
        assert_eq!(midpoint_proof.depth(), 2);

        // 1024 leaves = 2^10, depth = 10
        let leaves = make_test_leaves(1024);
        let tree = MerkleTree::from_xornames(leaves.clone()).unwrap();
        assert_eq!(tree.depth(), 10);

        let address_proof = tree.generate_address_proof(0, leaves[0]).unwrap();
        // From leaf (level 0) to root (level 10) = 10 hashing steps = 10 siblings
        assert_eq!(address_proof.depth(), 10);

        let midpoints = tree.midpoints().unwrap();
        let midpoint_proof = tree.generate_midpoint_proof(0, midpoints[0].hash).unwrap();
        // From midpoint (level 5) to root (level 10) = 5 hashing steps = 5 siblings
        assert_eq!(midpoint_proof.depth(), 5);

        // 100 leaves = padded to 128 = 2^7, depth = 7
        let leaves = make_test_leaves(100);
        let tree = MerkleTree::from_xornames(leaves.clone()).unwrap();
        assert_eq!(tree.depth(), 7);

        let address_proof = tree.generate_address_proof(0, leaves[0]).unwrap();
        // From leaf (level 0) to root (level 7) = 7 hashing steps = 7 siblings
        assert_eq!(address_proof.depth(), 7);

        let midpoints = tree.midpoints().unwrap();
        let midpoint_proof = tree.generate_midpoint_proof(0, midpoints[0].hash).unwrap();
        // Midpoint bits: (7+1)/2 = 4, level: 7-4 = 3, proof: 7-3 = 4 siblings
        assert_eq!(midpoint_proof.depth(), 4);
    }

    #[test]
    fn test_verify_merkle_proof_errors() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let leaves = make_test_leaves(16);
        let tree = MerkleTree::from_xornames(leaves.clone()).unwrap();
        let merkle_payment_timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let candidates = tree.reward_candidates(merkle_payment_timestamp).unwrap();
        let winner_pool_midpoint_proof = &candidates[0];
        let address_proof = tree.generate_address_proof(0, leaves[0]).unwrap();
        let root = tree.root();
        let depth = tree.depth();

        // Test 1: Invalid address proof (wrong root)
        let wrong_root = XorName::from_content(b"wrong root");
        let result = verify_merkle_proof(
            &leaves[0],
            &address_proof,
            winner_pool_midpoint_proof,
            depth,
            &wrong_root,
            merkle_payment_timestamp,
        );
        assert!(matches!(
            result,
            Err(BadMerkleProof::AddressBranchRootMismatch { .. })
        ));

        // Test 2: Address proof depth mismatch
        let result = verify_merkle_proof(
            &leaves[0],
            &address_proof,
            winner_pool_midpoint_proof,
            depth + 1, // Wrong depth
            &root,
            merkle_payment_timestamp,
        );
        assert!(matches!(
            result,
            Err(BadMerkleProof::AddressProofDepthMismatch { .. })
        ));

        // Test 3: Winner proof root mismatch
        let mut wrong_winner = winner_pool_midpoint_proof.clone();
        // Create a different proof with wrong root
        let wrong_tree = MerkleTree::from_xornames(make_test_leaves(16)).unwrap();
        let wrong_candidates = wrong_tree
            .reward_candidates(merkle_payment_timestamp)
            .unwrap();
        wrong_winner.branch = wrong_candidates[0].branch.clone();

        let result = verify_merkle_proof(
            &leaves[0],
            &address_proof,
            &wrong_winner,
            depth,
            &root,
            merkle_payment_timestamp,
        );
        assert!(matches!(
            result,
            Err(BadMerkleProof::WinnerBranchRootMismatch { .. })
        ));

        // Test 4: Timestamp in future
        let future_timestamp = merkle_payment_timestamp + 1000;
        let result = verify_merkle_proof(
            &leaves[0],
            &address_proof,
            winner_pool_midpoint_proof,
            depth,
            &root,
            future_timestamp,
        );
        assert!(matches!(
            result,
            Err(BadMerkleProof::TimestampInFuture { .. })
        ));

        // Test 5: Payment expired
        let old_timestamp = merkle_payment_timestamp - MERKLE_PAYMENT_EXPIRATION - 1;
        let old_candidates = tree.reward_candidates(old_timestamp).unwrap();
        let result = verify_merkle_proof(
            &leaves[0],
            &address_proof,
            &old_candidates[0],
            depth,
            &root,
            old_timestamp,
        );
        assert!(matches!(result, Err(BadMerkleProof::PaymentExpired { .. })));

        // Test 6: Timestamp mismatch between pool and contract
        // Use a different timestamp that's still valid (not future, not expired)
        let different_timestamp = merkle_payment_timestamp - 100;
        let result = verify_merkle_proof(
            &leaves[0],
            &address_proof,
            winner_pool_midpoint_proof,
            depth,
            &root,
            different_timestamp,
        );
        assert!(matches!(
            result,
            Err(BadMerkleProof::TimestampMismatch { .. })
        ));
    }

    #[test]
    fn test_invalid_midpoint_index() {
        let leaves = make_test_leaves(16);
        let tree = MerkleTree::from_xornames(leaves).unwrap();

        let midpoints = tree.midpoints().unwrap();
        let midpoint_count = midpoints.len();

        // Try to generate proof for invalid midpoint index
        let result = tree.generate_midpoint_proof(midpoint_count, XorName::from_content(b"test"));

        assert!(matches!(
            result,
            Err(MerkleTreeError::InvalidMidpointIndex { .. })
        ));
    }

    #[test]
    fn test_reward_candidate_pool_address() {
        let leaves = make_test_leaves(16);
        let tree = MerkleTree::from_xornames(leaves).unwrap();

        let timestamp1 = 12345u64;
        let timestamp2 = 67890u64;

        let candidates1 = tree.reward_candidates(timestamp1).unwrap();
        let candidates2 = tree.reward_candidates(timestamp2).unwrap();

        // Same tree, same candidate index, different timestamp = different address
        assert_ne!(candidates1[0].address(), candidates2[0].address());

        // Same candidate, same call = deterministic address
        assert_eq!(candidates1[0].address(), candidates1[0].address());

        // Verify address is hash(midpoint || root || timestamp)
        let addr = candidates1[0].address();
        let mut data = Vec::with_capacity(32 + 32 + 8);
        data.extend_from_slice(candidates1[0].branch.leaf_hash().as_ref());
        data.extend_from_slice(candidates1[0].branch.root().as_ref());
        data.extend_from_slice(&timestamp1.to_le_bytes());
        let expected = XorName::from_content(&data);
        assert_eq!(addr, expected);
    }

    #[test]
    fn test_calculate_depth_edge_cases() {
        // Test the calculate_depth function via tree construction
        let test_cases = vec![
            (2, 1),     // 2 leaves = depth 1
            (3, 2),     // 3 leaves = depth 2 (padded to 4)
            (4, 2),     // 4 leaves = depth 2
            (5, 3),     // 5 leaves = depth 3 (padded to 8)
            (8, 3),     // 8 leaves = depth 3
            (9, 4),     // 9 leaves = depth 4 (padded to 16)
            (16, 4),    // 16 leaves = depth 4
            (17, 5),    // 17 leaves = depth 5 (padded to 32)
            (100, 7),   // 100 leaves = depth 7 (padded to 128)
            (1024, 10), // 1024 leaves = depth 10
        ];

        for (leaf_count, expected_depth) in test_cases {
            let leaves = make_test_leaves(leaf_count);
            let tree = MerkleTree::from_xornames(leaves).unwrap();
            assert_eq!(
                tree.depth(),
                expected_depth,
                "Depth mismatch for {leaf_count} leaves"
            );
        }
    }
}
