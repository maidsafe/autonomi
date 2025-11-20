// SPDX-License-Identifier: GPL-3.0
pragma solidity ^0.8.20;

import "@openzeppelin/contracts/token/ERC20/IERC20.sol";

/// Merkle Batch Payment Vault
///
/// Handles batch payments for Merkle tree storage where multiple data chunks
/// are paid for in a single transaction. Uses a fair median pricing mechanism
/// based on candidate node metrics.
contract MerklePaymentVault {

    // ============ Types ============

    /// Node storage and network metrics for pricing calculation
    struct QuotingMetrics {
        uint8 dataType;           // Type of data being stored
        uint256 dataSize;         // Size in bytes
        uint256 closeRecordsStored; // Records stored by this node
        Record[] recordsPerType;  // Breakdown by data type
        uint256 maxRecords;       // Node capacity
        uint256 receivedPaymentCount; // Reliability metric
        uint256 liveTime;         // Hours connected to network
        uint256 networkDensity;   // Network density metric
        uint256 networkSize;      // Estimated network size
    }

    /// Record count per data type
    struct Record {
        uint8 dataType;
        uint256 records;
    }

    /// One candidate node with metrics
    struct CandidateNode {
        address rewardsAddress;
        QuotingMetrics metrics;
    }

    /// Pool commitment with CANDIDATES_PER_POOL candidates (always)
    struct PoolCommitment {
        bytes32 poolHash;           // Cryptographic commitment to full pool data
        CandidateNode[16] candidates; // Fixed size: always CANDIDATES_PER_POOL
    }

    /// Payment information stored on-chain
    struct PaymentInfo {
        uint8 depth;                      // Merkle tree depth
        uint64 merklePaymentTimestamp;    // Payment timestamp
        PaidNode[] paidNodeAddresses;     // List of paid nodes
    }

    /// Individual paid node record
    struct PaidNode {
        address rewardsAddress;
        uint8 poolIndex;  // Index in winner pool (0-15)
    }

    // ============ State ============

    /// ANT token contract
    IERC20 public immutable antToken;

    /// Payment info indexed by winner pool hash
    mapping(bytes32 => PaymentInfo) public payments;

    /// Maximum supported Merkle tree depth
    uint8 public constant MAX_MERKLE_DEPTH = 12;

    /// Number of candidates per pool (fixed)
    uint8 public constant CANDIDATES_PER_POOL = 16;

    // ============ Events ============

    /// Emitted when a Merkle batch payment is made
    event MerklePaymentMade(
        bytes32 indexed winnerPoolHash,
        uint8 depth,
        uint256 totalAmount,
        uint64 merklePaymentTimestamp
    );

    // ============ Errors ============

    error DepthTooLarge(uint8 depth, uint8 max);
    error WrongPoolCount(uint256 expected, uint256 got);
    error WrongCandidateCount(uint256 poolIdx, uint256 expected, uint256 got);
    error PaymentNotFound(bytes32 poolHash);
    error InsufficientBalance(uint256 have, uint256 need);
    error InsufficientAllowance(uint256 have, uint256 need);
    error TransferFailed();
    error PaymentAlreadyExists(bytes32 poolHash);

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
    function payForMerkleTree(
        uint8 depth,
        PoolCommitment[] calldata poolCommitments,
        uint64 merklePaymentTimestamp
    ) external returns (bytes32 winnerPoolHash, uint256 totalAmount) {

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
                revert WrongCandidateCount(
                    i,
                    CANDIDATES_PER_POOL,
                    poolCommitments[i].candidates.length
                );
            }
        }

        // Select winner pool deterministically
        uint256 winnerPoolIdx = _selectWinnerPool(
            poolCommitments.length,
            msg.sender,
            merklePaymentTimestamp
        );
        PoolCommitment calldata winnerPool = poolCommitments[winnerPoolIdx];
        winnerPoolHash = winnerPool.poolHash;

        // Check if payment already exists for this pool
        if (payments[winnerPoolHash].depth != 0) {
            revert PaymentAlreadyExists(winnerPoolHash);
        }

        // Calculate median price from all CANDIDATES_PER_POOL candidates
        uint256 medianPrice = _calculateMedianPrice(winnerPool.candidates);
        totalAmount = medianPrice * depth;

        // Check balance and allowance
        uint256 balance = antToken.balanceOf(msg.sender);
        if (balance < totalAmount) {
            revert InsufficientBalance(balance, totalAmount);
        }

        uint256 allowance = antToken.allowance(msg.sender, address(this));
        if (allowance < totalAmount) {
            revert InsufficientAllowance(allowance, totalAmount);
        }

        // Select depth winner nodes from pool
        uint8[] memory winnerIndices = _selectWinnerNodes(
            depth,
            winnerPoolHash,
            merklePaymentTimestamp
        );

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
            bool success = antToken.transferFrom(msg.sender, rewardsAddress, amountPerNode);
            if (!success) {
                revert TransferFailed();
            }

            // Store paid node info
            info.paidNodeAddresses.push(PaidNode({
                rewardsAddress: rewardsAddress,
                poolIndex: nodeIdx
            }));
        }

        emit MerklePaymentMade(
            winnerPoolHash,
            depth,
            totalAmount,
            merklePaymentTimestamp
        );

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
    ) external view returns (uint256 totalAmount) {

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
                revert WrongCandidateCount(
                    i,
                    CANDIDATES_PER_POOL,
                    poolCommitments[i].candidates.length
                );
            }
        }

        // Select winner pool deterministically (same logic as payForMerkleTree)
        uint256 winnerPoolIdx = _selectWinnerPool(
            poolCommitments.length,
            msg.sender,
            merklePaymentTimestamp
        );
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
    function getPaymentInfo(bytes32 winnerPoolHash)
        external
        view
        returns (PaymentInfo memory info)
    {
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
    function _selectWinnerPool(
        uint256 poolCount,
        address sender,
        uint64 timestamp
    ) internal view returns (uint256) {
        bytes32 seed = keccak256(abi.encodePacked(
            block.prevrandao,
            block.timestamp,
            sender,
            timestamp
        ));
        return uint256(seed) % poolCount;
    }

    /// Calculate median price from CANDIDATES_PER_POOL candidate quotes
    function _calculateMedianPrice(CandidateNode[16] calldata candidates)
        internal
        pure
        returns (uint256)
    {
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
    function _getQuote(QuotingMetrics calldata metrics)
        internal
        pure
        returns (uint256)
    {
        // Base price per byte (in atto tokens)
        uint256 basePrice = 1000;

        // Calculate storage saturation: 0-100%
        uint256 saturation = 0;
        if (metrics.maxRecords > 0) {
            saturation = (metrics.closeRecordsStored * 100) / metrics.maxRecords;
            if (saturation > 100) saturation = 100;
        }

        // Capacity multiplier: 100% when empty, 200% when full
        // This incentivizes nodes with more free space
        uint256 capacityMultiplier = 100 + saturation;

        // Reliability bonus: small bonus for nodes with payment history
        // Caps at 50% bonus for very reliable nodes
        uint256 paymentBonus = metrics.receivedPaymentCount > 50 ? 50 : metrics.receivedPaymentCount;
        uint256 reliabilityMultiplier = 100 + paymentBonus;

        // Network density adjustment (if provided)
        // Higher density = more nodes available = slightly lower price
        uint256 densityAdjustment = 100;
        if (metrics.networkDensity > 0 && metrics.networkSize > 0) {
            // Simple heuristic: if network is dense, reduce price by up to 20%
            // This is a simplified calculation - real implementation may be more complex
            if (metrics.networkSize > 1000) {
                densityAdjustment = 90;
            } else if (metrics.networkSize > 100) {
                densityAdjustment = 95;
            }
        }

        // Calculate final quote
        uint256 quote = (
            metrics.dataSize *
            basePrice *
            capacityMultiplier *
            reliabilityMultiplier *
            densityAdjustment
        ) / 1_000_000; // Normalize from 100^3 = 1,000,000

        // Ensure minimum price
        if (quote == 0) {
            quote = metrics.dataSize; // At least 1 atto per byte
        }

        return quote;
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
    function _selectWinnerNodes(
        uint8 depth,
        bytes32 poolHash,
        uint64 timestamp
    ) internal view returns (uint8[] memory) {
        uint8[] memory winners = new uint8[](depth);
        bool[16] memory selected;

        bytes32 seed = keccak256(abi.encodePacked(
            block.prevrandao,
            poolHash,
            timestamp
        ));

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
