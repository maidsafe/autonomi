// SPDX-License-Identifier: GPL-3.0
pragma solidity ^0.8.20;

import "@openzeppelin/contracts/token/ERC20/IERC20.sol";

/// @title IMerklePaymentVault
/// @notice Interface for the Merkle Batch Payment Vault
/// @dev Handles batch payments for Merkle tree storage where multiple data chunks
///      are paid for in a single transaction using a fair median pricing mechanism
interface IMerklePaymentVault {
    // ============ Types ============

    struct MerklePayment {
        uint256 treeDepth;
        uint256 amount;
        uint256 paymentTstamp;
        address[] recipients;
    }

    struct DataPayment {
        address rewardsAddress;
        uint256 amount;
        bytes32 quoteHash;
    }

    struct Payment {
        bytes16 rewardsAddress;
        uint128 amount;
    }

    /// @notice Types of data that can be stored
    enum DataType {
        GraphEntry,
        Scratchpad,
        Chunk,
        Pointer
    }

    /// @notice Record count per data type
    struct Record {
        DataType dataType;
        uint256 records;
    }

    /// @notice Quoting metrics provided by candidate nodes
    struct QuotingMetrics {
        DataType dataType;
        uint256 closeRecordsStored;
        Record[] recordsPerType;
    }

    /// @notice One candidate node with its metrics
    struct CandidateNode {
        address rewardsAddress;
        QuotingMetrics metrics;
    }

    /// @notice Pool commitment with 16 candidates
    struct PoolCommitment {
        bytes32 poolHash; // Cryptographic commitment to full pool data
        CandidateNode[16] candidates; // Fixed size: always 16
    }

    /// @notice Individual paid node record
    struct PaidNode {
        address rewardsAddress;
        uint8 poolIndex; // Index in winner pool (0-15)
    }

    /// @notice Payment information stored on-chain
    struct PaymentInfo {
        uint8 depth; // Merkle tree depth
        uint64 merklePaymentTimestamp; // Payment timestamp
        PaidNode[] paidNodeAddresses; // List of paid nodes
    }

    struct CostUnit {
        uint256 costUnit;
        uint256 costUnitMax;
    }

    struct PaymentVerification {
        QuotingMetrics metrics;
        DataPayment dataPayment;
    }

    struct PaymentVerificationResult {
        bytes32 quoteHash;
        uint256 amountPaid;
        bool isValid;
    }

    // ============ Events ============

    /// @notice Emitted when a Merkle batch payment is made
    /// @param winnerPoolHash Hash of the selected winner pool
    /// @param depth Tree depth
    /// @param totalAmount Total tokens paid to winners
    /// @param merklePaymentTimestamp Client-provided timestamp
    event MerklePaymentMade(
        bytes32 indexed winnerPoolHash, uint8 depth, uint256 totalAmount, uint64 merklePaymentTimestamp
    );

    /// @notice Emitted when a data payment is made (legacy event)
    /// @param root Merkle root
    /// @param treeDepth Tree depth
    /// @param amount Payment amount
    event DataPaymentMade(bytes32 indexed root, uint256 indexed treeDepth, uint256 indexed amount);

    // ============ Errors ============

    error InvalidRoot();

    error RootAlreadyPaid();

    error InvalidAmount();

    error InvalidTreeDepth();

    error InvalidRecipientsCount();

    error AntTokenNull();
    error BatchLimitExceeded();

    error InvalidInputLength();

    error PriceFeedNull();

    error InvalidChainlinkPrice();

    error SequencerDown();
    error GracePeriodNotOver();

    error InvalidQuoteHash();

    error DepthTooLarge(uint8 depth, uint8 max);

    error WrongPoolCount(uint256 expected, uint256 got);

    error PaymentAlreadyExists(bytes32 poolHash);

    error PaymentNotFound(bytes32 poolHash);

    error WrongCandidateCount(uint256 poolIdx, uint256 expected, uint256 got);

    // ============ View Functions ============

    /// @notice Returns the ANT token contract address
    /// @return The IERC20 token contract
    function antToken() external view returns (IERC20);

    /// @notice Returns payment info for a winner pool hash
    /// @param winnerPoolHash Hash of the winner pool
    /// @return depth Tree depth
    /// @return merklePaymentTimestamp Payment timestamp
    function payments(bytes32 winnerPoolHash) external view returns (uint8 depth, uint64 merklePaymentTimestamp);

    /// @notice Maximum supported Merkle tree depth
    /// @return Maximum depth value
    function MAX_MERKLE_DEPTH() external view returns (uint8);

    /// @notice Number of candidates per pool
    /// @return Number of candidates (always 16)
    function CANDIDATES_PER_POOL() external view returns (uint8);

    /// @notice Get payment info by winner pool hash
    /// @param winnerPoolHash Hash returned from payForMerkleTree
    /// @return info Payment information stored on-chain
    function getPaymentInfo(bytes32 winnerPoolHash) external view returns (PaymentInfo memory info);

    /// @notice Estimate the cost of a Merkle tree payment without executing it
    /// @dev This is a view function (0 gas) that runs the same pricing logic as
    ///      payForMerkleTree but returns only the estimated cost
    /// @param depth Tree depth (determines number of nodes paid)
    /// @param poolCommitments Array of pool commitments (2^ceil(depth/2))
    /// @param merklePaymentTimestamp Client-provided timestamp
    /// @return totalAmount Estimated total tokens that would be paid
    function estimateMerkleTreeCost(
        uint8 depth,
        PoolCommitment[] calldata poolCommitments,
        uint64 merklePaymentTimestamp
    ) external view returns (uint256 totalAmount);

    // ============ State-Changing Functions ============

    /// @notice Pay for Merkle tree batch
    /// @param depth Tree depth (determines number of nodes paid)
    /// @param poolCommitments Array of pool commitments (2^ceil(depth/2))
    /// @param merklePaymentTimestamp Client-provided timestamp
    /// @return winnerPoolHash Hash of selected winner pool
    /// @return totalAmount Total tokens paid to winners
    function payForMerkleTree(uint8 depth, PoolCommitment[] calldata poolCommitments, uint64 merklePaymentTimestamp)
    external
    returns (bytes32 winnerPoolHash, uint256 totalAmount);
}
