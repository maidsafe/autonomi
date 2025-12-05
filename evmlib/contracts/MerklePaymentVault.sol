// SPDX-License-Identifier: GPL-3.0
pragma solidity ^0.8.20;

import "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import "./IMerklePaymentVault.sol";

/// Merkle Batch Payment Vault
///
/// Handles batch payments for Merkle tree storage where multiple data chunks
/// are paid for in a single transaction. Uses a fair median pricing mechanism
/// based on candidate node metrics.
contract MerklePaymentVault is IMerklePaymentVault {
    // ============ State ============

    /// ANT token contract
    IERC20 public immutable override antToken;

    /// Payment info indexed by winner pool hash
    mapping(bytes32 => PaymentInfo) public override payments;

    /// Maximum supported Merkle tree depth
    uint8 public constant override MAX_MERKLE_DEPTH = 12;

    /// Number of candidates per pool (fixed)
    uint8 public constant override CANDIDATES_PER_POOL = 16;

    // ============ Constructor ============

    constructor(address _antToken) {
        require(_antToken != address(0), "Invalid token address");
        antToken = IERC20(_antToken);
    }

    // ============ Main Functions ============

    /// Pay for Merkle tree batch
    ///
    /// @param depth Tree depth (determines number of nodes paid)
    /// @param poolCommitments Array of pool commitments (2^ceil(depth/2))
    /// @param merklePaymentTimestamp Client-provided timestamp
    /// @return winnerPoolHash Hash of selected winner pool
    /// @return totalAmount Total tokens paid to winners
    function payForMerkleTree(uint8 depth, PoolCommitment[] calldata poolCommitments, uint64 merklePaymentTimestamp)
    external
    override
    returns (bytes32 winnerPoolHash, uint256 totalAmount)
    {
        // Validate depth
        if (depth > MAX_MERKLE_DEPTH) {
            revert DepthTooLarge(depth, MAX_MERKLE_DEPTH);
        }

        // Validate pool count: 2^ceil(depth/2)
        uint256 expectedPools = _expectedRewardPools(depth);
        if (poolCommitments.length != expectedPools) {
            revert WrongPoolCount(expectedPools, poolCommitments.length);
        }

        // Validate each pool has exactly CANDIDATES_PER_POOL candidates
        for (uint256 i = 0; i < poolCommitments.length; i++) {
            if (poolCommitments[i].candidates.length != CANDIDATES_PER_POOL) {
                revert WrongCandidateCount(i, CANDIDATES_PER_POOL, poolCommitments[i].candidates.length);
            }
        }

        // Select winner pool deterministically
        uint256 winnerPoolIdx = _selectWinnerPool(poolCommitments.length, msg.sender, merklePaymentTimestamp);
        PoolCommitment calldata winnerPool = poolCommitments[winnerPoolIdx];
        winnerPoolHash = winnerPool.poolHash;

        // Check if payment already exists for this pool
        if (payments[winnerPoolHash].depth != 0) {
            revert PaymentAlreadyExists(winnerPoolHash);
        }

        // Calculate median price from all CANDIDATES_PER_POOL candidates
        uint256 medianPrice = _calculateMedianPrice(winnerPool.candidates);
        totalAmount = medianPrice * depth;

        // Select depth winner nodes from pool
        uint8[] memory winnerIndices = _selectWinnerNodes(depth, winnerPoolHash, merklePaymentTimestamp);

        // Initialize storage for payment info
        PaymentInfo storage info = payments[winnerPoolHash];
        info.depth = depth;
        info.merklePaymentTimestamp = merklePaymentTimestamp;

        // Transfer tokens to winners and store payment records
        uint256 amountPerNode = totalAmount / depth;
        for (uint256 i = 0; i < depth; i++) {
            uint8 nodeIdx = winnerIndices[i];
            address rewardsAddress = winnerPool.candidates[nodeIdx].rewardsAddress;

            // Transfer tokens to winner
            antToken.transferFrom(msg.sender, rewardsAddress, amountPerNode);

            // Store paid node info
            info.paidNodeAddresses.push(PaidNode({rewardsAddress: rewardsAddress, poolIndex: nodeIdx}));
        }

        emit MerklePaymentMade(winnerPoolHash, depth, totalAmount, merklePaymentTimestamp);

        return (winnerPoolHash, totalAmount);
    }

    /// Estimate the cost of a Merkle tree payment without executing it
    ///
    /// This is a view function (0 gas) that runs the same pricing logic as
    /// payForMerkleTree but returns only the estimated cost without executing payment.
    ///
    /// @param depth Tree depth (determines number of nodes paid)
    /// @param poolCommitments Array of pool commitments (2^ceil(depth/2))
    /// @param merklePaymentTimestamp Client-provided timestamp
    /// @return totalAmount Estimated total tokens that would be paid
    function estimateMerkleTreeCost(
        uint8 depth,
        PoolCommitment[] calldata poolCommitments,
        uint64 merklePaymentTimestamp
    ) external view override returns (uint256 totalAmount) {
        // Validate depth
        if (depth > MAX_MERKLE_DEPTH) {
            revert DepthTooLarge(depth, MAX_MERKLE_DEPTH);
        }

        // Validate pool count: 2^ceil(depth/2)
        uint256 expectedPools = _expectedRewardPools(depth);
        if (poolCommitments.length != expectedPools) {
            revert WrongPoolCount(expectedPools, poolCommitments.length);
        }

        // Validate each pool has exactly CANDIDATES_PER_POOL candidates
        for (uint256 i = 0; i < poolCommitments.length; i++) {
            if (poolCommitments[i].candidates.length != CANDIDATES_PER_POOL) {
                revert WrongCandidateCount(i, CANDIDATES_PER_POOL, poolCommitments[i].candidates.length);
            }
        }

        // Select winner pool deterministically (same logic as payForMerkleTree)
        uint256 winnerPoolIdx = _selectWinnerPool(poolCommitments.length, msg.sender, merklePaymentTimestamp);
        PoolCommitment calldata winnerPool = poolCommitments[winnerPoolIdx];

        // Calculate median price from all CANDIDATES_PER_POOL candidates
        uint256 medianPrice = _calculateMedianPrice(winnerPool.candidates);
        totalAmount = medianPrice * depth;

        return totalAmount;
    }

    /// Get payment info by winner pool hash
    ///
    /// @param winnerPoolHash Hash returned from payForMerkleTree
    /// @return info Payment information stored on-chain
    function getPaymentInfo(bytes32 winnerPoolHash) external view override returns (PaymentInfo memory info) {
        info = payments[winnerPoolHash];
        if (info.depth == 0) {
            revert PaymentNotFound(winnerPoolHash);
        }
        return info;
    }

    // ============ Internal Functions ============

    /// Calculate expected number of reward pools: 2^ceil(depth/2)
    function _expectedRewardPools(uint8 depth) internal pure returns (uint256) {
        uint8 halfDepth = (depth + 1) / 2; // ceil division
        return 1 << halfDepth; // 2^halfDepth
    }

    /// Select winner pool using deterministic pseudo-randomness
    function _selectWinnerPool(uint256 poolCount, address sender, uint64 timestamp) internal view returns (uint256) {
        bytes32 seed = keccak256(abi.encodePacked(block.prevrandao, block.timestamp, sender, timestamp));
        return uint256(seed) % poolCount;
    }

    /// Calculate median price from CANDIDATES_PER_POOL candidate quotes
    function _calculateMedianPrice(CandidateNode[16] calldata candidates) internal pure returns (uint256) {
        // Get quote for each candidate
        uint256[16] memory quotes;
        for (uint256 i = 0; i < 16; i++) {
            quotes[i] = _getQuote(candidates[i].metrics);
        }

        // Sort quotes
        _sortQuotes(quotes);

        // Return median (average of 8th and 9th elements, 0-indexed: [7] and [8])
        return (quotes[7] + quotes[8]) / 2;
    }

    /// Calculate quote for a single node based on metrics
    ///
    /// Pricing algorithm considers:
    /// - Data size (base cost)
    /// - Node capacity/saturation (higher when more full)
    /// - Network participation (rewards reliability)
    /// - Network density (adjusts for network conditions)
    function _getQuote(QuotingMetrics calldata metrics) internal pure returns (uint256) {
        // Temporary until we have the updated pricing formula
        return (1);
    }

    /// Sort array of CANDIDATES_PER_POOL quotes using insertion sort (efficient for small arrays)
    function _sortQuotes(uint256[16] memory quotes) internal pure {
        for (uint256 i = 1; i < 16; i++) {
            uint256 key = quotes[i];
            uint256 j = i;
            while (j > 0 && quotes[j - 1] > key) {
                quotes[j] = quotes[j - 1];
                j--;
            }
            quotes[j] = key;
        }
    }

    /// Select depth winner nodes from pool deterministically
    function _selectWinnerNodes(uint8 depth, bytes32 poolHash, uint64 timestamp)
    internal
    view
    returns (uint8[] memory)
    {
        uint8[] memory winners = new uint8[](depth);
        bool[16] memory selected;

        bytes32 seed = keccak256(abi.encodePacked(block.prevrandao, poolHash, timestamp));

        uint256 selectedCount = 0;
        uint256 attempts = 0;

        // Select unique random indices
        while (selectedCount < depth && attempts < 100) {
            seed = keccak256(abi.encodePacked(seed, attempts));
            uint8 idx = uint8(uint256(seed) % 16);

            if (!selected[idx]) {
                selected[idx] = true;
                winners[selectedCount] = idx;
                selectedCount++;
            }
            attempts++;
        }

        require(selectedCount == depth, "Failed to select enough winners");

        return winners;
    }
}
