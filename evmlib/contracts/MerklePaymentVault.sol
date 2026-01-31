// SPDX-License-Identifier: GPL-3.0
pragma solidity ^0.8.20;

import "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import "@prb/math/src/SD59x18.sol";
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

    // ============ Pricing Constants ============

    /// Precision for fixed-point calculations (1e18)
    uint256 public constant PRECISION = 1e18;

    /// Constant ANT price for local testnet (1e18)
    /// In production, this would be fetched from Chainlink
    uint256 public constant ANT_PRICE = 1e18;

    /// Scaling factor for the pricing formula
    uint256 public scalingFactor = 1e18;

    /// Minimum price floor (3 to match regular payment baseline)
    uint256 public minPrice = 3;

    /// Maximum cost unit for capacity calculation
    uint256 public maxCostUnit = 1e24;

    /// Default cost unit assigned to each data type
    uint256 public constant DEFAULT_COST_UNIT = 1e18;

    /// Cost unit per data type
    mapping(DataType => uint256) public costUnitPerDataType;

    // ============ Constructor ============

    constructor(address _antToken) {
        require(_antToken != address(0), "Invalid token address");
        antToken = IERC20(_antToken);

        // Initialize cost units per data type
        costUnitPerDataType[DataType.Chunk] = DEFAULT_COST_UNIT;
        costUnitPerDataType[DataType.GraphEntry] = DEFAULT_COST_UNIT;
        costUnitPerDataType[DataType.Scratchpad] = DEFAULT_COST_UNIT;
        costUnitPerDataType[DataType.Pointer] = DEFAULT_COST_UNIT;
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
        uint256 numChunks = 1 << depth; // 2^depth chunks in the merkle tree
        totalAmount = medianPrice * numChunks;

        // Select depth winner nodes from pool
        uint8[] memory winnerIndices = _selectWinnerNodes(depth, winnerPoolHash, merklePaymentTimestamp);

        // Initialize storage for payment info
        PaymentInfo storage info = payments[winnerPoolHash];
        info.depth = depth;
        info.merklePaymentTimestamp = merklePaymentTimestamp;

        // Transfer tokens to winners and store payment records
        // depth winners share the total amount (each gets totalAmount/depth)
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

    /// Pay for Merkle tree batch using packed calldata (v2)
    ///
    /// Uses packed data structures where each candidate's data type and total cost unit
    /// are encoded into a single uint256 for smaller calldata and lower gas costs.
    ///
    /// @param depth Tree depth (determines number of nodes paid)
    /// @param poolCommitments Array of packed pool commitments (2^ceil(depth/2))
    /// @param merklePaymentTimestamp Client-provided timestamp
    /// @return winnerPoolHash Hash of selected winner pool
    /// @return totalAmount Total tokens paid to winners
    function payForMerkleTree2(uint8 depth, PoolCommitmentPacked[] calldata poolCommitments, uint64 merklePaymentTimestamp)
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

        // Select winner pool deterministically
        uint256 winnerPoolIdx = _selectWinnerPool(poolCommitments.length, msg.sender, merklePaymentTimestamp);
        PoolCommitmentPacked calldata winnerPool = poolCommitments[winnerPoolIdx];
        winnerPoolHash = winnerPool.poolHash;

        // Check if payment already exists for this pool
        if (payments[winnerPoolHash].depth != 0) {
            revert PaymentAlreadyExists(winnerPoolHash);
        }

        // Calculate median price from all CANDIDATES_PER_POOL packed candidates
        uint256 medianPrice = _calculateMedianPricePacked(winnerPool.candidates);
        uint256 numChunks = 1 << depth; // 2^depth chunks in the merkle tree
        totalAmount = medianPrice * numChunks;

        // Select depth winner nodes from pool
        uint8[] memory winnerIndices = _selectWinnerNodes(depth, winnerPoolHash, merklePaymentTimestamp);

        // Initialize storage for payment info
        PaymentInfo storage info = payments[winnerPoolHash];
        info.depth = depth;
        info.merklePaymentTimestamp = merklePaymentTimestamp;

        // Transfer tokens to winners and store payment records
        // depth winners share the total amount (each gets totalAmount/depth)
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
        uint256 numChunks = 1 << depth; // 2^depth chunks in the merkle tree
        totalAmount = medianPrice * numChunks;

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

    /// Calculate median price from CANDIDATES_PER_POOL packed candidate quotes (v2)
    function _calculateMedianPricePacked(CandidateNodePacked[16] calldata candidates) internal view returns (uint256) {
        // Get quote for each candidate
        uint256[16] memory quotes;
        for (uint256 i = 0; i < 16; i++) {
            quotes[i] = _getQuotePacked(candidates[i].dataTypeAndTotalCostUnit);
        }

        // Sort quotes
        _sortQuotes(quotes);

        // Return median (average of 8th and 9th elements, 0-indexed: [7] and [8])
        return (quotes[7] + quotes[8]) / 2;
    }

    /// Calculate quote for a single node from packed data (v2)
    ///
    /// Unpacks the dataTypeAndTotalCostUnit field and applies the same pricing formula as _getQuote.
    /// Format: packed = (totalCostUnit << 8) | dataType
    function _getQuotePacked(uint256 dataTypeAndTotalCostUnit) internal view returns (uint256) {
        // Unpack: dataType is lower 8 bits, totalCostUnit is upper bits
        DataType dataType = DataType(uint8(dataTypeAndTotalCostUnit & 0xFF));
        uint256 totalCostUnit = dataTypeAndTotalCostUnit >> 8;

        uint256 lowerBound = _getBound(totalCostUnit);
        uint256 upperBound = _getBound(totalCostUnit + costUnitPerDataType[dataType]);

        // Edge cases: if bounds are equal or at precision, return minPrice
        if (lowerBound == upperBound || lowerBound == PRECISION || upperBound == PRECISION) {
            return minPrice;
        }

        // Calculate |rUpper - 1| and |rLower - 1| for logarithm
        uint256 upperDiff = _absDiff(upperBound, PRECISION);
        uint256 lowerDiff = _absDiff(lowerBound, PRECISION);

        // Avoid log(0) - return minPrice if either diff is 0
        if (upperDiff == 0 || lowerDiff == 0) {
            return minPrice;
        }

        // Calculate ln|rUpper - 1| - ln|rLower - 1| using PRB Math
        int256 logUpper = _calculateLn(upperDiff);
        int256 logLower = _calculateLn(lowerDiff);
        int256 logDiff = logUpper - logLower;

        // Calculate linear part: rUpper - rLower
        uint256 linearPart = _absDiff(upperBound, lowerBound);

        // Formula components:
        // partOne = (-s/ANT) * logDiff
        // partTwo = pMin * linearPart / PRECISION
        // partThree = linearPart / ANT
        int256 partOne = (-int256(scalingFactor) * logDiff) / int256(ANT_PRICE * PRECISION);
        uint256 partTwo = (linearPart * minPrice) / PRECISION;
        uint256 partThree = linearPart / ANT_PRICE;

        // Combine: price = partOne + partTwo - partThree
        int256 price = partOne + int256(partTwo) - int256(partThree);

        // Return price, with minPrice as floor
        if (price <= 0) {
            return minPrice;
        }
        return uint256(price);
    }

    /// Calculate median price from CANDIDATES_PER_POOL candidate quotes
    function _calculateMedianPrice(CandidateNode[16] calldata candidates) internal view returns (uint256) {
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
    /// Uses the production pricing formula:
    /// price = (-s/ANT) * (ln|rUpper - 1| - ln|rLower - 1|) + pMin*(rUpper - rLower) - (rUpper - rLower)/ANT
    ///
    /// Where:
    /// - s = scalingFactor
    /// - ANT = antPrice
    /// - pMin = minPrice
    /// - rLower = totalCostUnit / maxCostUnit
    /// - rUpper = (totalCostUnit + costUnitPerDataType[dataType]) / maxCostUnit
    function _getQuote(QuotingMetrics calldata metrics) internal view returns (uint256) {
        uint256 totalCostUnit = _getTotalCostUnit(metrics);
        uint256 lowerBound = _getBound(totalCostUnit);
        uint256 upperBound = _getBound(totalCostUnit + costUnitPerDataType[metrics.dataType]);

        // Edge cases: if bounds are equal or at precision, return minPrice
        if (lowerBound == upperBound || lowerBound == PRECISION || upperBound == PRECISION) {
            return minPrice;
        }

        // Calculate |rUpper - 1| and |rLower - 1| for logarithm
        uint256 upperDiff = _absDiff(upperBound, PRECISION);
        uint256 lowerDiff = _absDiff(lowerBound, PRECISION);

        // Avoid log(0) - return minPrice if either diff is 0
        if (upperDiff == 0 || lowerDiff == 0) {
            return minPrice;
        }

        // Calculate ln|rUpper - 1| - ln|rLower - 1| using PRB Math
        int256 logUpper = _calculateLn(upperDiff);
        int256 logLower = _calculateLn(lowerDiff);
        int256 logDiff = logUpper - logLower;

        // Calculate linear part: rUpper - rLower
        uint256 linearPart = _absDiff(upperBound, lowerBound);

        // Formula components:
        // partOne = (-s/ANT) * logDiff
        // partTwo = pMin * linearPart / PRECISION
        // partThree = linearPart / ANT
        int256 partOne = (-int256(scalingFactor) * logDiff) / int256(ANT_PRICE * PRECISION);
        uint256 partTwo = (linearPart * minPrice) / PRECISION;
        uint256 partThree = linearPart / ANT_PRICE;

        // Combine: price = partOne + partTwo - partThree
        int256 price = partOne + int256(partTwo) - int256(partThree);

        // Return price, with minPrice as floor
        if (price <= 0) {
            return minPrice;
        }
        return uint256(price);
    }

    /// Calculate total cost unit from metrics
    function _getTotalCostUnit(QuotingMetrics calldata metrics) internal view returns (uint256) {
        uint256 total = 0;
        for (uint256 i = 0; i < metrics.recordsPerType.length; i++) {
            Record calldata record = metrics.recordsPerType[i];
            total += costUnitPerDataType[record.dataType] * record.records;
        }
        return total;
    }

    /// Calculate bound ratio: value / maxCostUnit
    function _getBound(uint256 value) internal view returns (uint256) {
        if (maxCostUnit == 0) {
            return 0;
        }
        return (value * PRECISION) / maxCostUnit;
    }

    /// Calculate absolute difference between two values
    function _absDiff(uint256 a, uint256 b) internal pure returns (uint256) {
        if (a >= b) {
            return a - b;
        }
        return b - a;
    }

    /// Calculate natural logarithm using PRB Math
    function _calculateLn(uint256 x) internal pure returns (int256) {
        if (x == 0) {
            revert("ln(0) undefined");
        }
        // Convert to SD59x18 (scaled by 1e18)
        SD59x18 value = sd(int256(x));
        SD59x18 result = ln(value);
        return result.unwrap();
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
