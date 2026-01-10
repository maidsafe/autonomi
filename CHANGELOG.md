# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

*When editing this file, please respect a line length of 100.*

## 2026-01-09

### Network

#### Changed

- The binaries in the automatic upgrade process are now fetched from `autonomi.com` URLs, with the
  S3 location being used as a fallback.

#### Fixed

- The automatic upgrades process was being confused by the fact that our last release was a
  client-only release. It was detecting it as a new release and scheduling a restart of the node,
  although the binary wasn't being replaced. The process has now been changed to only check and
  compare the sha256 hash of the `antnode` binary rather than the commit hash of the release.

## 2025-12-23

### API

#### Added

- `Client::check_records_exist_batch` async method for checking the existence of multiple records
  in batch operations, improving efficiency when verifying data presence on the network.
- `Client::retry_failed_merkle_chunks` async method for retrying failed chunk uploads with
  configurable retry attempts and pause duration between attempts.
- `NetworkAddress::xorname` method to extract the XorName from a NetworkAddress, returning `None`
  for PeerId and RecordKey variants.
- `MerkleBatchUploadState` struct to track failed chunks from a merkle batch upload, including
  chunk data for retry operations.
- `MerkleBatchUploadResult` struct returned from merkle batch uploads containing remaining streams,
  completed files, and any failed chunks.
- `MerklePutError::Batch` variant for tracking batch upload state with failed chunks.

#### Changed

- `Client::upload_batch_with_merkle` method signature now requires an additional `dont_reupload:
  &mut HashSet<XorName>` parameter to track and skip chunks that already exist on the network. The
  return type has changed from `Result<(Vec<EncryptionStream>, Vec<(PathBuf, DataMapChunk,
  Metadata)>), MerklePutError>` to `Result<MerkleBatchUploadResult, MerklePutError>`. [BREAKING]
- Merkle payment uploads now skip redundant encryption when all chunks already exist on the
  network, significantly improving performance for re-uploads of existing data.
- Upload operations no longer pay for chunks that already exist on the network, reducing
  unnecessary costs when re-uploading or syncing existing data.
- Chunk existence checking now reports progress during Merkle payment operations, providing better
  visibility into upload status.
- File upload operations with merkle payments now provide high-level retry logic with exponential
  backoff, improving reliability for large file uploads.

#### Fixed

- Issue where one-chunk files would fail to create merkle trees correctly, preventing successful
  uploads of very small files using merkle payments.
- Incorrect chunk upload count display during merkle payment operations.
- Excessive use of XorName in merkle hashing flows, improving efficiency and correctness of merkle
  tree operations.

### Payments

#### Changed

- Gas limit buffer increased to 50% to provide more headroom for transaction execution and reduce
  the risk of out-of-gas errors.
- Transaction receipts are now validated after mining to ensure successful execution before
  considering the transaction complete.
- Pool creation for merkle payments is now processed in parallel, significantly reducing the time
  required to prepare batch payments.

#### Fixed

- Merkle tree maximum depth validation corrected to prevent tree construction errors with large
  datasets.
- Duplicate hash function removed from merkle payment implementation, simplifying the codebase.

### Ant Client

#### Added

- The `file cost` command now supports a `--merkle` flag to estimate costs using Merkle batch
  payment mode instead of traditional per-chunk payments.
- The `file upload` command now supports a `--merkle` flag to upload files using Merkle batch
  payment mode, enabling more efficient bulk uploads.
- Merkle payment upload operations now cache successful payments and support retry of failed chunks
  through `cached_merkle_payments` module.

#### Changed

- Upload operations display clearer progress information when checking for existing chunks on the
  network.

### General

#### Fixed

- Python bindings for ARM architectures now compile correctly by setting appropriate CFLAGS to
  define ARM architecture version for cross-compilation targets. This resolves the ring crate
  compilation failures on older GCC toolchains.

## 2025-12-18

### API

#### Changed

- `Client::get_storage_proofs_from_peer` method signature now requires two additional parameters:
  `data_type: DataTypes` and `data_size: usize`. The return type has also changed from
  `Vec<(NetworkAddress, Result<ChunkProof, Error>)>` to `PeerQuoteWithStorageProof`, which includes
  an optional `PaymentQuote` alongside the storage proofs. [BREAKING]

### Network

#### Added

- Retry logic with exponential backoff to closest peers lookup operations, improving robustness when
  querying the network.
- Support for querying a specific number of closest peers, providing more control over network
  queries.
- Bad peer detection system that verifies fetched record copies match expected values and blocks
  peers that fail validation checks.
- Metrics tracking for bad peers to monitor network health.

#### Changed

- Node and client now use the same network `get_closest` scheme for consistent peer resolution
  across the network.
- Blocklist behaviour is limited to prevent unlimited growth by using a circular buffer mechanism.
- Record store indexing cache is now pruned to remove out-of-sync entries and improve accuracy.
- Replication is skipped when a close-up peer restart pattern is detected to reduce unnecessary
  network traffic.
- Increased `KAD_QUERY_TIMEOUT` to 120 seconds from 10 seconds. Improves node-side KAD query
  reliability and should prevent `GetClosestTimeout` errors when doing get_closest_peers lookups.

#### Fixed

- Record copy verification now ensures fetched records match the expected content before accepting
  them.
- Peers that have been dropped from the routing table are no longer blocked during recheck
  operations, preventing false positives in bad peer detection.
- Client `get_closest` check now returns more consistent results across multiple queries.
- Client operations no longer miss existing copies when verifying data storage.
- Mutable data re-uploads now use the same quoting range consistently.

### Antctl

#### Fixed

- Services now stop correctly on macOS when using the `antctl stop` command. The underlying service
  manager crate was incorrectly specifying an 'on success' restart policy that prevented proper
  service termination.
- Services no longer start automatically when added with the `antctl add` command on macOS, restoring
  expected behaviour consistent with other service managers.
- Node registry manager is now properly cleared during reset operations, preventing phantom nodes
  from appearing after reset.

### Launchpad

#### Fixed

- Input mode now only switches when error popup is dismissed, preventing unintended mode changes.
- Node registry is properly synchronized between local storage and in-memory representation after
  reset operations.

## 2025-12-11

### API

#### Added

- Merkle payment support for batch payments through cryptographic proof verification.
- `MerklePaymentOption` enum to enable merkle payment mode in upload operations.
- `MerkleUploadError` and `MerkleUploadErrorWithReceipt` error types for merkle-specific upload
  failures.
- `MerklePaymentError` for merkle payment processing errors.
- `MerklePaymentReceipt` type to track successful merkle payment transactions.
- `MerklePutError` for merkle-specific put operation failures.
- `Client::upload_with_merkle_payments` method to upload data using merkle batch payment mode.
- `Client::put_with_merkle_payment` method for storing individual records with merkle payment
  proofs.

### Protocol

#### Added

- `Query::GetMerkleCandidateQuote` query variant for requesting merkle payment candidate quotes from
  nodes.
- Includes `key` (target address for topology verification), `data_type`, `data_size`, and
  `merkle_payment_timestamp` fields.
- Node signs its current state (metrics + reward address) with the provided timestamp, creating a
  cryptographic commitment binding PeerId to RewardsAddress.
- `RecordKind::DataWithMerklePayment` variant for records paid via merkle proofs.
- Uses index range starting at `RECORD_KIND_MERKLE_PAYMENT_STARTING_INDEX` (20) to differentiate
  from traditional payment records.
- Merkle payment timestamp verification to ensure payments are neither expired nor in the future.

#### Changed

- Record serialization now supports merkle payment proof headers alongside data payloads.
- `RecordHeader` serialization extended to handle merkle payment record types.

### Network

#### Added

- Merkle payment proof verification in `PutValidation` for all data types
- `PutValidationError::MerklePaymentVerificationFailed` error variant for failed merkle proof
  validations.
- Merkle payment quote generation signed with node metrics and reward address.
- Cryptographic verification of merkle proofs against on-chain merkle tree roots.
- Merkle payment expiration validation to reject expired payment proofs.
- Support for deserializing records containing `(MerklePaymentProof, T)` tuples where `T` is the
  data type.
- The `antnode` binary now supports automatic upgrades on macOS and Linux. Every 3 days the node
  will check for the availability of a new version. If available, the new version will be downloaded
  and the current binary will be replaced (this is permitted for running processes on Unix-based
  systems and it also works with symlinks). Each node is then assigned a restart time based on its
  peer ID and the network size. This ensures the restarts will be spread out quite evenly. When the
  node restarts it will retain its peer ID and data.

#### Changed

- Put record validation now branches on `RecordKind::DataWithMerklePayment` to verify merkle proofs
  before accepting records.
- Node quote responses now include merkle payment candidate signatures when requested via
  `GetMerkleCandidateQuote`.
- Payment proof structure extended to accommodate merkle tree proofs alongside traditional payment
  receipts.

### Payments

#### Added

- `ant-evm::merkle_payments` module providing core merkle payment infrastructure:
  - `MerkleTree` for constructing and managing merkle trees with up to `MAX_LEAVES` (1024) leaves.
  - `MerkleBranch` type representing the path from a leaf to the root in a merkle tree.
  - `MerklePaymentProof` containing the merkle branch, on-chain commitment info, and signatures.
  - `MerklePaymentCandidateNode` representing a node's signed commitment for merkle payment selection.
  - `MerklePaymentCandidatePool` for organizing candidates into payment pools.
  - `verify_merkle_proof` function for cryptographic verification of merkle proofs.
  - `MidpointProof` for proving the midpoint of merkle tree ranges.
- `MERKLE_PAYMENT_EXPIRATION` constant defining the validity period for merkle payment proofs.
- `MAX_MERKLE_DEPTH` constant defining the maximum depth of merkle trees.
- `CANDIDATES_PER_POOL` constant for pool organization.
- `MerklePaymentVerificationError` for merkle-specific verification failures.
- `PoolCommitment` type for on-chain merkle root commitments.
- `OnChainPaymentInfo` containing merkle root and payment metadata from smart contracts.
- `expected_reward_pools` function to calculate expected number of reward pools based on data size.

#### Changed

- EVM network configuration now includes optional `merkle_payments_address` for merkle payment vault
  contracts.
- Wallet and testnet setup extended to support merkle payment contract deployment and interaction.

### Client

#### Changed

- Upload strategy now supports both traditional per-chunk payments and merkle batch payments.
- The client will now carry out a check of its close group candidates to see whether each peer is
  known by the others in the group. This improves uploads because more robust nodes will be chosen,
  and in turn downloads will also be more stable.
- The quoting candidate range is extended from 7 to 10 peers to attempt to provide more stability.

### Antctl

#### Changed

- The service definitions emitted by the `add` command now use an 'on success' restart policy
  on Unix platforms. This is to allow the service manager to restart the node for use with automatic
  upgrades. Before upgrading to the new version of `antctl`, users should use `antctl reset` with
  their current version to clear any existing node deployments. The new node is not compatible with
  the old service definitions produced by `antctl`. New deployments can then be created with the new
  version of `antctl`. [BREAKING]
- The `add` command will assign a random port in the service definition rather than
  delegating that to the node. This is to ensure the node will restart with the same port when it is
  automatically upgraded.
- The `status` command now obtains the version number of each service. This makes the command work
  naturally with automatic upgrades.

### Launchpad

#### Changed

- As with the `antctl` change above, old service definitions are not compatible with the new version
  of the nodes to have automatic upgrades work correctly. Before upgrading to the new
  `node-launchpad`, users should use the reset command with `Ctrl+R` to clear out their current node
  deployments. New deployments can then be created with the new version of `node-launchpad`.
  [BREAKING]
- The `Status` panel will update with the new node versions when automatic upgrades take place in
  the background.
 
## 2025-11-27

### API

#### Added

- `Client::analyze_address_recursively` method for recursively analyzing addresses by following all
  discovered addresses, returning a map of analyses.
- `Client::get_record_from_peer` method to retrieve a record directly from a specific peer.
- `Client::get_record_and_holders` method that returns both the record and a list of peer holders,
  supporting custom quorum requirements.
- `Client::get_storage_proofs_from_peer` method to retrieve storage proofs from a specific peer.
- `encrypt_directory_files` function now publicly exported from the `self_encryption` module for
  external directory encryption operations.
- `EncryptionStream` type now publicly exported from the `self_encryption` module.
- `DataMap` field added to `GetError::TooLargeForMemory` variant to provide the data map when a file
  is too large for in-memory processing. [BREAKING]

#### Changed

- `Client::get_closest_to_address` now accepts an optional `count` parameter to specify the number
  of peers to retrieve; if `None`, returns `CLOSE_GROUP+2` peers. [BREAKING]
- `Analysis` enum now uses `custom_debug::Debug` for improved debug output.
- `Analysis::DataMap` and `Analysis::File` variants now skip the `data` field in debug output to
  reduce noise when debugging large data structures.
- `Analysis::Chunk` display no longer prints chunk content in hex format.
- Chunk retrieval strategy changed to use fallback approach with `closest_20` peers when normal DHT
  queries fail, and chunk get retry strategy set to `RetryStrategy::None` to rely on this fallback.
- Maximum stream data size reduced to 1 MB for both client and node connections to optimize network
  performance.

#### Removed

- Upload fallback approach removed to ensure proper replication with multiple copies instead of only
  a single copy being uploaded.

#### Fixed

- Symlink handling in self-encryption to skip encryption of both symlinks and directories.

### Network

#### Added

- Network-wide replication of all node keys to improve data availability.
- Dynamic replication range expansion when the network is under load.

#### Changed

- Replication range expanded to give holders more trust in data availability.
- Network-wide replication deadline decreased to 4 days to ensure fresher data distribution.
- `GetReplicatedRecord` request error responses made more accurate for better diagnostics.

#### Fixed

- Replication behaviour modified according to review feedback for improved efficiency.

### Ant Client

#### Added

- The `analyze` command now supports extensive new functionality for network health monitoring:
    + Use the `--recursive` flag to recursively analyze datamaps and other data structures to fetch
      all referenced data.
    + Use the `--holders` flag to see the peers holding that piece of data.
    + Use the `--closest-nodes` flag to query the closest peers to check if they're holding that
      piece of data.
    + Use the `--json` to display all analyzed information as json.
    + Use the `--addr-range` flag during blind scan operations to specify address ranges.

#### Changed

- Analyze functionality refactored into its own module for better code organization.

#### Fixed

- The `analyze` command properly obtains target addresses from all enum entry types.
- Public archive address extraction now returns correct addresses.
- Analysis output is printed to screen before storing to JSON file.
- KAD `get_record` queries now use `Quorum::N(20)` to fetch records in a best-effort manner.

### Launchpad

#### Added

- Automatic permission elevation on Windows; the user can now simply double click on the exe to run
  it, rather than running from an elevated cmd.exe or Powershell session.

#### Changed

- Removed custom panic handler in favor of using the `ant-logging` crate for consistent error
  handling and logging.

### General

#### Changed

- Logging path handling improved for tokio tests with dynamic file or directory-based logging.
- Metrics initialization now properly uses the metrics feature flag.

## 2025-11-06

### API

#### Fixed

- An inefficient mechanism was being used when various tasks were being processed in parallel. This
  has been replaced with a more optimised approach. As a result we can expect to see improved
  performance in various areas, including file downloads and uploads, and quotations.

### Bootstrapping

#### Fixed

- An issue caused the bootstrap process to halt before being connected to the network. It will now
  correctly stop either when we are connected to the network or have exhausted all the addresses.

## 2025-10-15

### Bootstrapping

#### Fixed

- Provide a timeout in the bootstrapping process that prevents indefinite waiting to fetch contacts,
  which in turn could result in a timeout error. We will also now complete bootstrapping when we've
  obtained five addresses. This fix will apply to both the `antnode` and `ant` binaries.

## 2025-10-06

### Language Bindings

#### Added

**Python:**
- `Client` class: `with_payment_mode` method for setting the payment mode.
- `PaymentMode` enum for defining the available payment modes.

**NodeJS:**
- `Client` class: `with_payment_mode` method for setting the payment mode.
- `PaymentMode` enum for defining the available payment modes.

### Network

### Changed

- Nodes now use the `Bootstrap` struct to drive the initial bootstrapping process and the bootstrap
  cache.
- Nodes now evict peers immediately if they notice their peer ID has changed. This allows the
  network to flush out old peers much quicker. This should resolve some performance issues we've seen
  on the production network that have been the result of node operators who are over-provisioning and
  pulling large numbers of nodes in short time periods.

### Bootstrapping

#### Added

- A new `Bootstrap` struct is introduced that provides a single interface to bootstrap a `Node` or
  `Client` to the network.
- The `BootstrapConfig` allows the user to modify various configurations for bootstrapping to the
  network. Some options include providing a manual address, setting a custom bootstrap cache
  directory, disabling bootstrap cache reading, setting custom network contacts url and so on.
- The `Bootstrap` struct dials peers from all provided sources before terminating: environment
  variables, CLI arguments, bootstrap cache, and network contact URLs. This solves the major issue of
  using an outdated bootstrap cache to dial the network.
- Implement file locking for bootstrap cache such that concurrent accesses do not error out or
  produce empty values.

#### Changed

- The old method of obtaining the bootstrap peers as `Vec<MultiAddress>` using
  `InitialPeersConfig::get_bootstrap_addrs()` has now been removed in favour of the automated
  bootstrapping process.

### API

#### Added

- Introduce a new payment mode: single node. This reduces gas fees by making a single transaction to
  the median-priced node with 3x the quote amount, rather than 3 separate transactions to 3
  highest nodes.
- `PaymentMode` enum for controlling upload payment strategy with `Standard` (pay 3 nodes) and
  `SingleNode` (pay 1 node with 3x amount) variants.
- `Client::with_payment_mode()` method for setting the payment mode on the client.
- `Client::get_raw_quote_from_peer()` method for obtaining quotes from specific peers without
  market prices. This is useful for testing and obtaining reward addresses.
- `Client::get_node_version()` async method for requesting the node version of a specific peer on
  the network.

#### Changed

- `self_encryption` dependency upgraded to version `0.34.1` for improved encryption performance.
- `ClientConfig::bootstrap_cache_config` and `ClientConfig::init_peers_config` has been
  deprecated in favour of `ClientConfig::bootstrap_config`. This new config combines all the options
  from the deprecated fields.

### Payments

#### Changed

- Payment vault smart contract upgraded from V2 to V6. This upgrade supports the new single-node
  payment verification logic while maintaining backward compatibility.

### Ant Client

#### Added

- The `file cost` command provides a `--disable-single-node-payment` flag to switch from the
  default single-node payment mode to the multi-node payment mode.
- The `file upload` command provides a `--disable-single-node-payment` flag to switch from the
  default single-node payment mode to the multi-node payment mode.
- The `analyze` command now has an `analyse` alias for British English spelling preference.
- The `analyze` command now supports a `--closest-nodes` flag argument that will display the closest 
  nodes to the address being analysed.

#### Changed

- Single-node payment is now enabled by default for both the `file cost` and `file upload` commands,
  reducing gas fees for users. The previous behaviour can be restored using the
  `--disable-single-node-payment` flag.
- The `NetworkDriver` now uses the `Boostrap` struct to drive the initial bootstrapping process and
  the bootstrap cache.

### General

#### Changed

- Various nightly CI workflows have been removed as they were not being actively used.
- GitHub Actions `setup-python` upgraded from v5 to v6.

## 2025-09-29

### API

#### Added

- `DataStream` struct with streaming data access methods:
  - `data_size()` returns the original data size
  - `get_range(start, len)` decrypts and returns a specific byte range
  - `range(range)` convenience method using Range syntax
  - `range_from(start)` gets range from starting position to end
  - `range_to(end)` gets range from beginning to end position
  - `range_full()` gets the entire file content
  - `range_inclusive(start, end)` gets an inclusive range
- `data_stream(&DataMapChunk)` async method on `Client` for creating streaming access to private
  data.
- `data_stream_public(&DataAddress)` async method on `Client` for creating streaming access to
  public data.
- `scratchpad_put_update` async method for wallet-free scratchpad updates with caller-controlled
  management.
- `print_fork_analysis` function for detailed scratchpad fork error analysis and display.
- `vault_expand_capacity` async method for expanding vault storage capacity.
- `vault_claimed_capacity` async method for checking claimed vault capacity.
- `vault_split_bytes` function for splitting bytes for vault storage.

#### Changed

- Client initialization now includes automatic network connectivity verification via
  `wait_for_connectivity()` during the `init` process, improving reliability and error diagnostics.
- Scratchpad error handling enhanced with fork resolution capabilities in update operations, solving
  code duplication issues.
- Vault function names updated for consistency:
  - `fetch_and_decrypt_vault` → `vault_get` (deprecated function retained for compatibility)
  - `write_bytes_to_vault` → `vault_put` (deprecated function retained for compatibility)
  - `app_name_to_vault_content_type` → `vault_content_type_from_app_name` (deprecated function
    retained for compatibility)
- Code organization improved by moving encryption and utility modules out of the client module to
  top-level `self_encryption` and `utils` modules.

#### Fixed

- Analyze functionality now properly handles old datamap format types for backward compatibility.
- Scratchpad fork display and resolution issues resolved across all API operations.
- Streaming operations now validate destination paths before processing to prevent errors.

### Language Bindings

#### Added

**Python:**
- `GraphEntry` class methods for member access:
  - `content()` returns the entry content
  - `parents()` returns parent entry references
  - `descendants()` returns descendant entry references
- Data streaming bindings providing Python access to the new streaming data APIs.
- Enhanced fork error display functionality for scratchpad operations with comprehensive error
  details.
- Comprehensive test coverage for all Python binding functionality including address format
  validation.

**Node.js:**
- Updated vault operation support with new function names matching the renamed API standards.

#### Fixed

- Python binding tests updated to handle 96-character address hex format and proper `from_hex`
  round-trip conversions.
- `GraphEntry` bindings now properly expose all member access methods with correct error handling.

### Ant Client

#### Added

- Enhanced `scratchpad` command functionality with improved fork error handling and resolution
  capabilities.
- Better error reporting for scratchpad operations with detailed fork analysis output.
- The `download` command has improved error handling to immediately fail if the chosen download path
  cannot be used.

#### Fixed

- Scratchpad fork display and resolution functionality now works correctly across all client command
  operations.
- Get record operations now only perform early returns when unique content is received from
  sufficient peers, improving data retrieval reliability.
- The `analyze` command now properly handles file references in the old datamap format.

### Launchpad

#### Fixed

- Node storage size handling corrected for ARM v7 architecture devices.
- Node addition process on Windows now functions properly without configuration conflicts.

## 2025-09-02

### API

#### Added

- `chunk_batch_upload` function is now public, allowing developers to upload multiple chunks in
  batches with custom receipt handling.
- `deserialize_data_map` function in `DataMapChunk` for backward compatibility with old data map
  schemes.
- `pointer_update_from` async method for updating pointers from specific sources.
- `scratchpad_update_from` async method for updating scratchpads from specific sources.
- `EncryptionStream` struct with methods:
  - `total_chunks()` to get the total number of chunks
  - `next_batch()` to retrieve the next batch of chunks for processing
  - `data_map_chunk()` to get the associated data map chunk
  - `data_address()` to retrieve the data address
  - `new_in_memory_with()`, `new_in_memory()`, and `new_stream_from_file()` constructors for
    different encryption modes


#### Changed

- `DataMapChunk` field visibility changed from `pub(crate)` to `pub`, making the inner `Chunk`
  publicly accessible.
- Enhanced error handling and retry mechanisms for chunk upload operations through improved helper
  functions.
- File upload workflow now uses the same approach as directory uploads for consistency.
- Improved streaming encryption support with updated self-encryption dependency integration.
- Enhanced language usage in user-facing messages for better clarity across client operations.
- Unified approach for `Pointer` and `Scratchpad` split resolution through `resolve_split_records`
  function.
- Reduced `IN_MEMORY_ENCRYPTION_MAX_SIZE` threshold to 50MB for improved memory management during
  encryption operations.
- Streaming download capability in high-level file operations for `file_download`,
  `file_download_public`, `dir_download_public`, `dir_download`. Allows downloading larger files
  without spikes in memory usage experienced previously.
- The streaming capability results in a new datamap format that requires four extra chunks. If there
  is an attempt to re-upload files uploaded before the streaming implementation, there will be a cost
  for these extra chunks.
- The new datamap format always returns a root datamap that points to three chunks. These three
  extra chunks will now be paid for in uploads.

#### Fixed

- Vault operations now properly support single file uploads and access.
- If there were failed chunks in the final batch of an upload they were not retried. This has now
  been fixed with improved error handling.
- Deduplication logic for fetched scratchpads with identical highest counter values.

### Language Bindings

#### Added

**Python:**
- `AttoTokens` class with methods: `zero()`, `is_zero()`, `from_atto()`, `from_u64()`, `from_u128()`, `as_atto()`, `checked_add()`, `checked_sub()`, `as_bytes()`, `from_str()`, `__str__()`, `__repr__()`
- `ClientOperatingStrategy` class with getters: `get_chunks()`, `get_graph_entry()`, `get_pointer()`, `get_scratchpad()`
- `BootstrapCacheConfig` class with configuration methods: `new()`, `empty()`, `with_addr_expiry_duration()`, `with_cache_dir()`, `with_max_peers()`, `with_addrs_per_peer()`, `with_disable_cache_writing()`
- `InitialPeersConfig` class with peer management methods: `new()`, getters/setters for `first`, `addrs`, `network_contacts_url`, `local`, `ignore_cache`, `bootstrap_cache_dir`, `get_bootstrap_addr()`, `read_bootstrap_addr_from_env()`
- `MainPubkey` class with methods: `new()`, `verify()`, `derive_key()`, `as_bytes()`, `as_hex()`, `from_hex()`, `__str__()`, `__repr__()`
- `MainSecretKey` class with methods: `new()`, `public_key()`, `sign()`, `derive_key()`, `to_bytes()`, `random()`, `random_derived_key()`, `__repr__()`
- `Signature` class with methods: `parity()`, `from_bytes()`, `to_bytes()`, `__str__()`, `__repr__()`
- `StoreQuote` class with methods: `price()`, `len()`, `is_empty()`, `payments()`
- `RetryStrategy` class with methods: `none()`, `quick()`, `balanced()`, `persistent()`, `default()`, `attempts()`, `backoff()`, `__str__()`, `__repr__()`
- `Quorum` class with string representation methods
- `Strategy` class with getters: `get_put_quorum()`, `get_put_retry()`, `get_verification_quorum()`, `get_get_quorum()`, `get_get_retry()`
- `QuoteForAddress` class with `price()` method
- `RegisterAddress` class with methods: `new()`, `owner()`, `as_underlying_graph_root()`, `as_underlying_head_pointer()`, `as_hex()`, `from_hex()`
- `DerivationIndex`, `DerivedPubkey`, `DerivedSecretKey` classes for key derivation functionality
- Enhanced `ChunkAddress` class with `xorname()` and `from_hex()` methods
- Enhanced `TransactionConfig` class with `new()` constructor and `max_fee_per_gas` getter

**Node.js:**
- Complete ant-node package with network spawning capabilities

#### Fixed

- Python `get_bootstrap_addr()` method updated to match original Rust API changes
- Python `cache_save_scaling_factor` return type corrected from u64 to u32 to match Rust API
- Python `PyTransactionConfig` class fixes for proper configuration handling

### Network

#### Added

- New metrics: `antnode_branch`
- Improved logging for query response types to aid in network debugging and monitoring. This will
  help us measure the success of the next node upgrade.

#### Fixed

- Race condition in local network startup where bootstrap cache where the bootstrap cache was never
  written with newer addresses.
- Issue with the bootstrap cache where newer nodes were not updated when the cache became full.
- Expected holder calculation now properly capped to majority of `CLOSE_GROUP_SIZE` for improved
  consensus reliability.
- Replication accept range expanded from 5 to 7 nodes for middle range records to improve data
  availability.

### Antctl

#### Changed

- The `local run` command will now automatically provide the EVM setup for a local testnet without
  having to run the `evm-testnet` binary separately.

#### Fixed

- The `local run` command has extra wait time before launching a second node to prevent startup
  conflicts and improve reliability.

### Payments

#### Added

- The `evm-testnet` binary will be included in each release. This will make it easier to work with
  local testnets and we will also use it in our CI processes.

### Ant Client

#### Changed

- Previously, the `file download` command required access to RAM in proportion to the size of the
  file being downloaded, making it prohibitive to download large files. The command has now been
  changed to utilise new streaming features that keep memory usage low and consistent, so larger
  files can be downloaded without issue.

#### Fixed

- Logging is restored for events in the `ant` binary after it was inadvertently disabled.
- In some cases the chunk cache was not correctly cleared after a download.

## 2025-07-31

### API

#### Added

- New `chunk_cache` module that provides a mechanism for caching downloaded chunks.
- Use the chunk cache for downloads to enable resuming failed downloads.

#### Fixed

- Public files without archives had their content downloaded twice

#### Changed

- For the client connection, nodes that do not identify as KAD will not be added to the routing
  table. The client's routing table included nodes that were not upgraded, and these nodes were not
  identifying themselves as KAD nodes. If any of those older, non-KAD nodes were returned in a query
  for the closest peers, this resulted in no close peers being obtained. These older nodes did not
  identify themselves as KAD due to the removal of the external address manager. Having them in the
  routing table then had cascading effects, resulting in failed downloads and uploads. Excluding them
  using a block list restores reliable uploads and downloads. These older nodes already constitute a
  small percentage of the network and will eventually be filtered out with more upgrades.

### Client

#### Added

- The `file download` command now supports resuming downloads. The command will attempt to fetch all
  the chunks for a file, and in doing so, they will be saved to a temporary location on the local
  disk. If there's a failure to retrieve some chunks, users can run the same command again and it
  will only attempt to download the missing chunks. When all the chunks have been retrieved and the
  file is reassembled, the cached chunks will be deleted.
- The `file download` command supports a `--disable-cache` argument, if for some reason users want
  to disable the caching behaviour that applies by default.
- When connecting to the network, the client will now use the local bootstrap cache if it exists. If
  it doesn't exist, the initial connection will use a set of pre-defined bootstrap servers to obtain
  a peer list, and the cache will then be written periodically. This improves decentralization.


## 2025-07-21

### API

#### Added

- The `ScratchpadError` type has a new `Fork` variant. When there was a forked scratchpad with two
  or more scratchpads at the same version, the API would only return one of them, meaning a merge
  couldn't be performed correctly. Now when this situation occurs, the `Fork` error type is
  returned, and along with it, all the scratchpad versions, which can then be used for merge and
  conflict resolution.

### Network

#### Fixed

- Reintroduce the external address manager. The removal of this component caused an issue with
  clients whereby they sometimes couldn't communicate with nodes, though node-to-node communication
  was fine. This resulted in problems such as randomly failing to retrieve chunks during downloads,
  and it also affected emissions payments, because the client in the emissions service wasn't
  communicating with certain types of nodes. It seemed that port-forwarded nodes were the most
  affected. The removal of the external address manager was based on the assumption that addresses
  could be obtained from the connection information, but we suspect the libp2p client doesn't have
  that part of the code. Reintroducing the component resolves emissions for nodes configured with
  port-forwarding and should also very significantly improve the situation with uploads and
  downloads.

## 2025-07-18

### API

#### Added

- `RetryStrategy::N(usize)`: New retry strategy for data operations that allows specification of a
  custom count

#### Changed

- Extend libp2p client substreams timeout to 30 seconds. This should allow a client with a poor
  connection to upload larger records with a higher success rate.
- Enhanced logging and progress tracking for chunk data operations.
- Improved error handling and retry mechanisms for chunk operations.
- Paths in archives now use forward slashes on all platforms for cross-platform compatibility.

### Network

#### Changed

- The bootstrap peer cache is changed to use a simple FIFO mechanism to maintain the cache, rather
  than attempting to track the reliability of a peer. There is a `--write-older-cache-files`
  argument provided for backwards compatibility. This enables the peer cache servers to still
  provide the bootstrap cache in the old format until everyone has upgraded.
-  The `libp2p` library was upgraded from `0.55.0` to `0.56.0`. The main benefit of this was to
  enable the request/response model for uploads on the client.
- The `ant-networking` code was moved to a module within the `ant-node` crate, and in turn
  `ant-networking` was removed. This enabled the refactor and simplification of network
  initialisation and it also opens the door for further refactors. The networking code is now much
  more maintainable.

#### Removed

- The node's external address manager was removed. This component was responsible for advertising a
  node's address to others, but we now favour obtaining the address from the connection information,
  which is more accurate and less error prone.

### Client

#### Added

- The `file download` command supports a `--retries` argument that allows the user to specify a
  custom retry count for pulling chunks. If you are on a slower connection, you can consider trying
  a value like `20`, and you should see better and more consistent downloads.
- Use a request/response model for storing records on the network. This was a feature enabled by the
  `libp2p` upgrade and in internal testing it significantly improved the speed of uploads. This is
  because KAD requests in `libp2p` are not as well optimised as request/response.

#### Changed

- The output of the `file download` command was enhanced to use text to show chunks being obtained.
  The purpose being to provide the user with more feedback that progress is being made on the
  download.

#### Removed

- The progress bar was removed from the `file download` command in favour of text output that
  provides more informative feedback. The bar was only useful for downloads with multiple files and
  did not make sense for single file downloads. Better progress indicators will be added as later
  enhancements.

## 2025-06-26

### API

Key changes are the new networking module, `Quorum` type replacement, pointer counter expansion to
`u64`.

#### Added

- Networking:
    + `pub mod networking`: new network module.
    + `networking::Quorum`: enum for consensus operations.
    + Network driver, retry strategies, and utilities.
- Enhanced transaction configuration with the new `MaxFeePerGas` type.

#### Changed

- Type updates:
    + `ResponseQuorum` → `networking::Quorum` (breaking change)
    + Pointer counter type: `u32` → `u64` (with backward compatibility)
- Error handling improvements:
    + Enhanced `PointerError` error variants, e.g., `PutError`, `GetError`.
    + More specific error handling in pointer operations.
- Internal infrastructure:
    + Enhanced networking layer with better retry mechanisms.
    + Improved put/get record operations with retries.
    + Better split record handling for pointers.

####  Fixed

- `Client::pointer_check_existance()` → `Client::pointer_check_existence()` (typo fix)

### Network

- A peer's address is obtained from its connection info rather than self-advertised addresses from
  identify requests. The earlier method was incorrect and error prone.
- Peers are now added to the routing table only if they can be dialled back after 180 seconds, giving
  enough time for the UDP mapping to expire. Dialling back immediately would always succeed even if the
  peer was not externally reachable. When the routing table consists more of these reachable peers,
  it improves network health and should subsequently lead to better performance and reliability.
- Introduce a `DoNotDisturb` behaviour to fix an edge case with the 180-second delayed dial-back
  queue, where a peer would get re-added if it sent constant requests. This would happen if the peer
  thinks we are close and spams periodic messages.

### Client

#### Added

- The `ant` binary now has a `scratchpad` subcommand for working with scratchpads. These are a
  mutable 4MB blob of memory on the network. An initial payment is made to create one, but all
  further mutations are free. Personal user vaults have been implemented with scratchpads and some
  people have been using them for chat applications.
- The `ant` binary now has a `pointer` subcommand for managing pointers. Pointers enable building
  mutable data structures by providing authenticated, updatable references that only the owner can
  modify. Here are the available subcommands:
      + `cost`: calculate payment required before creating pointers
      + `create`: point to different data types (chunks, graphs, scratchpads, other pointers) that
        can be updated over time
      + `edit`: update an existing pointer reference
      + `generate-key`: create cryptographic keys for owning and updating pointers
      + `get`: retrieve the target of a pointer from the network
      + `list`: view all pointers controlled by the current user
      + `share`: give others read/write access by sharing pointer secret keys

#### Changed

The client has fundamentally changed with a large refactor we call 'light client networking'. The
previous networking implementation was complex and also shared between both node and client, leading
to all kinds of exceptions and special cases in the code. After the refactor, the client now has its
own, simpler networking implementation, and now the client and node network can evolve
independently.

This refactor made it easier to make other changes that featured in the release:

* Retry strategies were adapted to limit retrying without reason.
* Connect to peers in advance when performing libp2p put queries.
* No periodic network discovery.
* Improve connection success rate by using libp2p's `add_address` rather than dialling.
* Use a batched upload flow to reduce payment rejection resulting from quote expiration.

All these changes led to the following observations in our testing:

* Improved performance and throughput for uploads.
* Improved reliability and elimination of errors in uploads that were not related to payments. We do
  still see some errors related to gas prices on the Ethereum network, but these would hopefully be
  mitigated with retrying.
* Improved reliability and reduction of errors in downloads.

The download performance had parity with the current stable release.

There should be further improvements coming for the next release. In particular we are waiting on a
new `libp2p` release which will have a feature contributed by us that should further improve
performance. We've been told by the `libp2p` team that this release is now forthcoming.

### Launchpad

#### Added

- Provide a 3-minute timeout for NAT detection. If not successful, relaying will be used.

## 2025-05-14

### Client

#### Fixed

- Correctly obtain the EVM network on the `wallet balance` command. The change for the `--alpha`
  flag unintentionally introduced a regression that resulted in not being able to obtain the balance
  unless the EVM network was explicitly set. It will now be correctly selected based on the network
  ID.

### Launchpad

#### Added

- Display a dialog to indicate NAT detection is running when a new node service is requested.
  Without this, the launchpad appeared to be unresponsive.
- Introduce a check for the latest version. To encourage upgrades, a dialog will now appear to
  indicate the availability of a new version.

#### Changed

- Improve the grammar of some text used on the `Options` panel.

## 2025-05-09

### Network

#### Added

- The `antnode` binary now provides an `--alpha` flag argument. When used, it will connect the node
  to the alpha network and use Arbitrum Sepolia as the EVM provider.

### API

#### Added

- Provide an `init_alpha` function on the `Client`. It will return a client that is initialised
  specifically for the alpha network.
- Provide a `set_register_key` function on `UserData`. It sets the register key and returns the old
  one if it was already set.
- Provide a `display_stats` function on `UserData` to print out the current user data.

#### Changed

- Use the same set of payees for verification of quotes. This improves upload success rate.

### Client

#### Added

- The `ant` binary now provides an `--alpha` flag argument. When used, it will connect the client to
  the alpha network and use Arbitrum Sepolia as the EVM provider.
- Synchronise the register signing key in the vault.

#### Fixed

- Correct the formatting of `AttoTokens` from 32 to 18 decimal places.

#### Changed

- Peers are dialled before a put record request. This improves upload success rate.

### Antctl

#### Added

- Provide backwards compatibility in the form of reading old node registries that do not have new
  fields.
- The `add` command now provides an `--alpha` flag argument. When used, the node services will
  connect to the alpha network.

### Language Bindings

#### Added

- Several new classes were added to the Python bindings:
    + `Chunk`
    + `ClientEvent`
    + `ClientEventReceiver`
    + `DataTypes`
    + `PaymentQuote`
    + `QuotingMetrics`
    + `Receipt`
    + `StoreQuote`
    + `UploadSummary`
- The Python `Client` class also has several methods added:
    + `init_alpha`
    + `enable_client_events`
    + `evm_network`
    + `file_content_upload`
    + `file_content_upload_public`
    + `get_raw_quotes`
    + `get_store_quotes`
    + `pointer_verify`
    + `scratchpad_verify`
    + `upload_chunks_with_retries`

## 2025-04-29

### Network

#### Changed

- Disable `libp2p` `disjoint_query_path`. This improves resolving the closest nodes in the network.
- Reduce logging by changing the level of some messages. These were generating a lot of traffic and
  making life difficult for our ELK setup.

### API / Client

These changes were implemented in the API but are also manifest in the `ant` client.

#### Added

- The "Paying for X chunks" output was moved and added to the payment process.

#### Changed

- The number of quotes we attempt to obtain in parallel is reduced to the value of
  `CHUNK_UPLOAD_BATCH_SIZE` multiplied by `8`, and capped at `128`. Recently the default value of
  `CHUNK_UPLOAD_BATCH_SIZE` was changed to `1`, so in in turn the new default for how many quotes we
  obtain in parallel is significantly reduced. This works much better for poorer connections. Users
  with better connections can experiment with slightly larger values for `CHUNK_UPLOAD_BATCH_SIZE`.
- The `FILE_UPLOAD_BATCH_SIZE` variable now defaults to `1` rather than being based on the number of
  available threads. This means when a directory is being uploaded, only a single file will be
  uploaded at a time. This proved to be much better for poorer connections. Users with better
  connections can experiment and adjust the value as they see fit; for easier control we will probably
  add it as an argument on the `file upload` command.
- The "Paying for X chunks" output is changed to "Quoting for X chunks". The previous message was
  misleading because the payment doesn't take place until the chunk is uploaded.

#### Fixed

- Obtaining quotes will now have retries when there is a failure resolving the closest nodes.

### Client

#### Changed

- Increase the default query timeout from 60 to 120 seconds. On the production network we need more
  time for queries.

## 2025-04-22

### Network

#### Added

- While refreshing the routing table the node adds itself as one of the closest targets. This
  improves network discovery.

#### Changed

- A strict condition based on the `identify` agent info has now been removed. This would allow us to
  use this field to transfer node/client version information.
- The node relay server is now only enabled if the node is detected as public and is not a relay
  client. Previously it was not disabled, but rather unused.
- Change logging output for `NetworkAddress` to assist in investigating issues using ELK.
- To reduce resource usage, peer versions are only filtered when metrics are updated. 
- Several improvements for reducing resource usage:
    + Peer versions are only filtered when metrics are updated. 
    + While refreshing the routing table we avoid unnecessary random generation of indexes for
      picking closest candidates.
    + Reduce the frequency of network discovery.
- While refreshing the routing table we fill up as many of the closest targets among empty buckets
  as possible. This improves network discovery.

#### Fixed

- For using `evm-testnet` in a remote setup, some code for instructing Anvil to read the binding
  address from the `ANVIL_IP_ADDR` was removed. It was restored again for this functionality.
- Added some margin for the max chunk size to account for encryption and compression overhead. In
  rare cases, valid chunks were considered too big.
- Improve network discovery by avoiding holes between full buckets.
- Eliminate the chance that network discovery `round_robin_index` got stalled due to edge case.
- Avoid division by zero errors in an edge case with empty buckets.

### API

#### Added

- We can now set `network_id` via the client API. This allows for programming against different
  networks rather than just mainnet.
- Public function `get_raw_quotes` to get quotes from nodes without the market prices from the smart
  contract.
- Public function `get_closest_to_address` to get the closest peers to an address.

#### Changed

- Various improvements in the client API for better usability.
- Documentation was moved from the autonomi repository to https://github.com/maidsafe/docs.

#### Fixed

- Archive serialization was made backwards compatible.
- Issue that prevented pointer mutation in some cases.
- The pointer `xorname` function now returns its own address rather than the target.

### Language Bindings

#### Added

- Documentation for behaviour of the Scratchpad in Python bindings.
- Documentation and examples for using wallets in Python bindings.
- Initial bindings for NodeJS have been provided using `napi-rs`.

### Client

#### Added

- Provide an `analyze` command, which queries the details of an address on the network. This
  should help developers debugging their apps and understanding the network.
- Provide a `register history` command, which is useful for showing how the register's content has
  changed over time.
- The `register` `create`, `edit` and `get` commands now provide a `--hex` value for working with
  hex addresses.
- The `file download` command now supports downloading from a data map or public address.
- The `file upload` command has extra output for indicating uploaded chunks.
- File uploads and downloads now emit unique error codes for different error scenarios.

### Launchpad

#### Added

- Provided documentation for connecting to a custom network.
- Individual node management.

## 2025-04-01

### Network

#### Added

- Improve logging for the addition and removal of peers from the routing table.

#### Changed

- Enhanced strategy for refreshing the node's routing table that aims to maintain an accurate
  picture of the network. It incorporates periodic liveness checks and will remove inactive nodes.
- Incorporate the use of distance range to verify payments. The payee could have been blocked or
  churned out, but we should still consider the payment valid if the payee is close enough.
- Stop logging too many faults on the node listeners. This produced a lot of spam in the logs.

#### Fixed

- Issue with version upgrades not being detected correctly during periodic version checks.
- Do not add client peers to the routing table.
- Only check the expiry date on quotes from the current node. Checking the date on a quote from
  another node can fail due to differences in the operating system's clock.
- During re-attempts for requests, when the address of the target is not provided, e.g., in 
  replication-related requests, the addresses will be provided from the local node.

## 2025-03-20

### Client

- Increase the timeout for the query that obtains the closest nodes, from 10 seconds to 60 seconds.
  This is currently necessary because our routing table refresh is not optimised to purge dead nodes,
  and those dead entries cause the query to take more time. With more time, the query result is
  better and thus the upload and download performance is improved.
- Decrease the time between some retries during uploads. In many cases, the larger interval will not
  help. This allows the uploading process to fail faster if need be.

## 2025-03-12

### Network

#### Changed

- Log the `evmlib` crate by default.
- When a node receives a payment from a client, failed payment verification will be retried after 5
  seconds. The client and node could have queried different EVM nodes that were not synchronised
  yet. This could have resulted in a chunk proof verification error on the client.

#### Fixed

- During payee verification, use closest peers to target, rather than self. In some edge cases, the
  latter could cause payment to be rejected and result in a chunk proof verification error on
  uploads.
- Improve the efficiency of network discovery by handling an edge case where there is a 'hole' in
  the routing table.
- A peer will be dialled before sending it a request. This helped in the elimination of 'not enough
  quotes' errors.
- When obtaining peers, use `get_closest_local_peers` rather than `find_closest_local_peers`. This
  helped eliminate chunk proof verification errors.

### Autonomi API

#### Fixed

- Add missing class exports for several Python bindings.

### Client

#### Changed

- Use a single error variant for 'not enough quotes' error. This facilitated easier internal testing
  when investigating the errors.
- Various changes improved the efficiency of obtaining quotes for uploads:
    + Use a cloned network to increase parallelism.
    + Use 10 seconds for query timeout, rather than the default 60 seconds.
    + Use redials only during reattempts to avoid unnecessary timeout.
    + All content addresses across files are merged into a single call for obtaining the quotes.

#### Fixed

- The client no longer dials back when it receives an identify request; it has to assume nodes are
  OK. This may help to reduce open connections.
- Do not fetch mainnet contacts when the `--testnet` argument is used.

### Launchpad

#### Fixed

- When `UPnP` was selected, nodes would be started using `Manual` mode. They will now start as
  expected when `UPnP` is used.

## 2025-02-28

### Network

#### Added

- The node outputs critical start up and runtime failures to a `critical_failure.log` file. This is
  to help `antctl` feedback failure information to the user, but it should hopefully be generally
  useful to more advanced users.
- New metrics:
    + `connected_relay_clients`
    + `relay_peers_in_routing_table`
    + `peers_in_non_full_buckets`
    + `relay_peers_in_non_full_buckets`
    + `percentage_of_relay_peers`
- We also add a `node_versions` metric. This will be used to help us gauge what versions of nodes
  are present in the network and how many nodes have upgraded to the latest releases. It will also
  assist us in ensuring backward compatibility.

#### Changed

- The network bootstrapping process is changed to dial three of the initial peer addresses rather
  than all of them concurrently. When the routing table reaches five peers, network discovery takes
  over the rest of the bootstrapping process, and no more peers are dialled. This mechanism is much
  more efficient and avoids overloading the peers in the bootstrap cache.
- Network discovery rate has been increased during the start up phase, but it should slow down
  exponentially as more peers are added to the routing table.
- Several items aim to address uploading issues:
    + Avoid deadlocks on record store cache access
    + Do not fetch from the network when a replication fetch failed
    + Lower the number of parallel replication fetches
    + Issues that come in too quick will not trigger an extra action
    + Disable the current black list (possibly to be re-enabled when we have more data)
  They may also help reduce open connections and `libp2p` identify attempts
- Remove relay clients from the swarm driver tracker if the reservation has been closed.
- The `peers_in_rt` metric is improved by calculating it directly from kbuckets rather than using
  `libp2p` events.

### Autonomi API

#### Added

- Support uploading files with empty metadata

#### Changed

- Several file-related functions were renamed [BREAKING]:
    + `dir_upload` to `dir_content_upload`
    + `dir_and_archive_upload` to `dir_upload`
    + `file_upload` to `file_content_upload`
    + `dir_upload_public` to `dir_content_upload_public`
    + `dir_and_archive_upload_public` to `dir_upload_public`
    + `file_upload_public` to `file_content_upload_public`
- Improved address management to make it easier to use [BREAKING]:
    + All address types have the same methods: `to_hex` and `from_hex`.
    + All public-key addressed data types have the public key in their address.
    + High level `DataAddress` shares the values above instead of the low-level `XorName` that can't
      be constructed from hex.
    + Python now uses accurate addresses instead of clunky hex strings, and addresses for other
      types.
    + Fix inaccurate/missing python bindings for addresses: now all have `to_hex` and `from_hex`.

### Client

#### Added

- Support merging one archive into another.
- Introduce a maximum limit of 0.2 Gwei on the gas price when uploading files or creating/editing
  registers. If the gas exceeds this value, operations will be aborted. The commands provide a
  `--max-fee-per-gas` argument to override the value. This measure has been taken to avoid
  involuntarily paying excessive fees when the gas price fluctuates.

#### Changed

- The `ant file download` command can download directly from a `XorName`.
- The `ant file download` command can download data directly from a `DataMapChunk` to a file.

### Antctl

#### Added

- A `--no-upnp` flag to disable launching nodes with UPnP.
- A failure column is added to the `status` command.

#### Changed

- The `add` command will create services that will launch the node with `--upnp` by default. For
  home networking we want to try encourage people to use UPnP rather than relaying.
- The `add` command does not apply the 'on failure' restart policy to services. This is to prevent
  the node from continually restarting if UPnP is not working.
- The `--home-network` argument has been renamed `--relay` [BREAKING].

#### Fixed

- A debug logging statement used during the upgrade process caused an error if there were no nodes
  in the node registry.

### Launchpad

#### Added

- New column in the nodes panel for node failure reason.
- New column in the nodes panel to indicate UPnP support.
- New column in the nodes panel to the connection mode chosen by `Automatic`.

#### Changed

- Remove `Home Network` from the connection modes. Relay can only be selected by using `Automatic`
  in the case where UPnP fails. We are trying to avoid the use of relays when UPnP is available.

## 2025-02-11

### Network

#### Changed

- Removed encrypt data compile time flag (now always on).
- Refactor of data types.
- Removed the default trait for `QuotingMetrics` and it is now initialized with the correct values
  everywhere.
- Compile UPnP support by default; will still require `--upnp` when launching the node to activate.
- Removed the old flawed `Register` native data type.
- Creating `DataTypes` as the sole place to show network natively supported data types. And use it to
  replace existing `RecordKind`.
- Rename `RecordType` to `ValidationType`.
- Remove MDNS. For local nodes will bootstrap via the peer cache mechanism.
- Upgrade `libp2p` to `0.55.0` and use some small configuration changes it makes available.

#### Added

- `GraphEntry` data native type as a generic graph for building collections.
- `Pointer` data native type that points to other data on the network.
- Relay client events to the metrics endpoint.
- Relay reservation score to the metrics endpoint. This measures the health of a relay server that
  we are connected to, by tracking all the recent connections that were routed through that server.
- Allow override QUIC max stream window with `ANT_MAX_STREAM_DATA`.
- Added an easy way to spawn nodes or an entire network from code, with `ant_node::spawn::node_spawner::NodeSpawner` and `ant_node::spawn::network_spawner::NetworkSpawner`.
- Added a `data_type` verification when receiving records with proof of  payment.
- Added extra logging around payment verification.
- Make `QuotingMetrics` support data type variant pricing.
- Avoid free upload via replication.

#### Fixed

- External Address Manager will not consider `IncomingConnectionError` that originates from multiple
  dial attempts as a serious issue.
- `MultiAddressNotSupported` error is not considered as a critical error if the error set contains
  at least one different error.
- The record count metrics is now set as soon as a node is restarted.
- Push our Identify info if we make a new reservation with a relay server. This reduces the number
  of `CircuitReqDenied` errors throughout the network.
- All connection errors are now more forgiving and does not result in a peer being evicted from the
  routing table immediately. These errors are tracked and the action is taken only if we go over a
  threshold.
- Only replicate fresh uploads to other payees.
- During quoting re-attempts, use non-blocking sleep instead.

### Client

#### Changed

- Update python bindings and docs. Added initial NodeJS typescript integration.
- Updated test suit and added comprehensive documentation.
- Deprecate storing registers references in user data.
- Correctly report on chunks that were already uploaded to the network when syncing or re-uploading
  the same data.
- Add version field to archive data structure for backwards compatibility. And add future
  compatibility serialization into file metadata.
- Changed default EVM network to `Arbitrum One`.
- Removed the deprecated `Client::connect` function! Please use `Client::init` instead.
- Removed the old `Register` native data type, although the new `Register` high level type does the
  same job but better.
- Removed the feature flags and the complexities around those, now everything is configurable at
  runtime (no need to recompile).

#### Added

- NodeJS/Typescript bindings.
- 36 different configurations for publish Python bindings.
- Client examples.
- Added `evm_network` field to client config.
- Added a better retry strategy for getting market prices and sending transactions. This reduces the
  frequency of RPC related upload errors significantly.
- Added a `data_type` verification when receiving quotes from nodes.
- Client API for all four data types: `Chunk`, `GraphEntry`, `Scratchpad`, `Pointer`.
- High level `Register` data type that works similarly to old registers but without the update limit
  they had: now infinitely mutable.
- key derivation tooling

#### Fixed

- Rust optimization: Use parallelised chunk cloning in self encryption.
- Deterministically serialize archives. This leads to de-duplication and less payments when syncing
  folders and files.
- Patched and refactored client Python bindings to reflect almost the whole Rust API.
- EVM network uses default if not supplied by ENV.
- Event receiver panic after completing client operations.

## 2025-01-21

### Client

#### Changed

- Use balanced retry strategy for downloading chunks. Sometimes it would be possible we wouldn't
  find a chunk if we tried to retrieve it on the first attempt, so as with uploads, we will use a
  balanced retry strategy for downloads. This should make the `ant file download` command more
  robust.

## 2025-01-20

### Client

#### Fixed

- Remove unallocated static IP from the bootstrap mechanism. We have five static IP addresses
  allocated to five hosts, each of which run nodes and a minimal web server. The web server makes a
  list of peers available to nodes and clients to enable them to join the network. These static IP
  addresses are hard-coded in the `antnode` and `ant` binaries. It was discovered we had accidentally
  added six IPs and one of those was unallocated. Removing the unallocated IP should reduce the time
  to connect to the network.

### Network

#### Changed

- Reduce the frequency of metrics collection in the node's metrics server, from fifteen to sixty
  seconds. This should reduce resource usage and improve performance.
- Do not refresh all CPU information in the metrics collection process in the node's metrics server.
  Again, this should reduce resource usage and improve performance.
- Remove the 50% CPU usage safety measure. We added a safety measure to the node to cause the
  process to terminate if the system's CPU usage exceeded 50% for five consecutive minutes. This was
  to prevent cascading failures resulting from too much churn when a large node operator pulled the
  plug on tens of thousands of nodes in a very short period of time. If other operators had
  provisioned to max capacity and not left some buffer room for their own nodes, many other node
  processes could die from the resulting churn. After an internal discussion, the decision was taken
  to remove the safety measure.

## 2025-01-14

### Client

#### Fixed

- Remove `uploaded` timestamp from archive metadata to prevent unnecessary re-uploads when archive
  contents remain unchanged. This ensures we do not charge when uploading the same file more than
  once on `ant file upload`.
- Switch from `HashMap` to `BTreeMap` for archive to ensure deterministic serialization, which also
  prevents unnecessary re-uploads. As above, this facilitates the fix for the duplicate payment
  issue.

## 2025-01-09

### Network

#### Changed

- Network discovery no longer queries the farthest full buckets. This significantly reduces the
  number of messages as the network grows, resulting in fewer open connections and reduced resource
  usage.

## 2025-01-06

### Network

#### Changed

- Memory and CPU metrics use more precise `f64` measurements

### Client

#### Fixed

- Apply a timeout for EVM transactions. This fixes an issue where some uploads would freeze indefinitely.
- The `ant` CLI was not selecting its network consistently from the environment variable.

## 2024-12-21

### Network

#### Fixed

- Do not dial back when a new peer is detected. This resulted in a large number of open connections,
  in turn causing increased CPU usage.

### Client

#### Changed

- Remove the 'dial error' output on the `file upload` command

## 2024-12-18

### General

#### Changed

- For a branding alignment that moves Safe Network to Autonomi, all crates in the workspace prefixed
  `sn-` were renamed with an `ant-` prefix. For example, `sn-node` was renamed `ant-node`.
- To further support this alignment, several binaries were renamed:
   + `autonomi` -> `ant`
   + `safenode` -> `antnode`
   + `safenode-manager` -> `antctl`
   + `safenode_rpc_client` -> `antnode_rpc_client`
- The location of data directories used by the binaries were changed from `~/.local/share/safe` to
  `~/.local/share/autonomi`. The same is true of the equivalent locations on macOS and Windows.
- The prefixes of metric names in the `safenode` binary (now `antnode`) were changed from `sn_` to
  `ant_`.

### Network

#### Added

- Provide Python bindings for `antnode`.
- Generic `Transaction` data type
- Upgraded quoting with smart-contract-based pricing. This makes pricing fairer, as more nodes
  are rewarded and there are less incentives to cheat.
- Upgraded data payments verification.
- New storage proof verification which attempts to avoid outsourcing attack
- RBS support, dynamic `responsible_range` based on `network_density` equation estimation.
- Node support for client’s RBS `get_closest` query.
- More quoting metrics for potential future quoting scheme.
- Implement bootstrap cache for local, decentralized network contacts.
- Increased the number of peers returned for the `get_closest` query result.

#### Changed

- The `SignedSpend` data type was replaced by `Transaction`.
- Removed `group_consensus` on `BadNode` to support RBS in the future.
- Removed node-side quoting history check as part of the new quoting scheme.
- Rename `continuous_bootstrap` to `network_discovery`.
- Convert `Distance` into `U256` via output string. This avoids the need to access the
  `libp2p::Distance` private field because the change for it has not been published yet.
- For node and protocol versioning we remove the use of various keys in favour of a simple 
  integer between `0` and `255`. We reserve the value `1` for the main production network.
- The `websockets` feature was removed from the node binary. We will no longer support the `ws`
  protocol for connections.

#### Fixed

- Populate `records_by_bucket` during restart so that proper quoting can be retained after restart.
- Scramble `libp2p` native bootstrap to avoid patterned spike of resource usage.
- Replicate fresh `ScratchPad`
- Accumulate and merge `ScratchPad` on record get. 
- Remove an external address if it is unreliable.
- Bootstrap nodes were being replaced too frequently in the routing table.

### Client

#### Added

- Provide Python bindings.
- Support for generic `Transaction` data type.
- Upgraded quoting with smart contract.
- Upgraded data payments with new quoting.
- Retry failed PUTs. This will retry when chunks failed to upload.
- WASM function to generate a vault key from a wallet signature.
- Use bootstrap cache mechanism to initialize `Client` object. 
- Exposed many types at top-level, for more ergonomic use of the API. Together with more examples on
  function usage.
- Deprecated registers for the client, planning on replacing them fully with transactions and
  pointers.
- Wait a short while for initial network discovery to settle before quoting or uploading tasks
  begin.
- Stress tests for the register features of the vault.
- Improved logging for vault end-to-end test cases.
- More debugging logging for the client API and `evmlib`.
- Added support for adding a wallet from an environment variable if no wallet files are present.
- Provide `wallet export` command to export a wallet’s private key

#### Changed

- Added and modified documentation in various places to improve developer experience.
- Renamed various methods to 'default' to private uploading, while public will have `_public`
  suffixed. Also has various changes to allow more granular uploading of archives and data maps.
- Archives now store relative paths to files instead of absolute paths.
- The `wallet create --private-key` command has been changed to `wallet import`.

#### Fixed

- Files now download to a specific destination path.
- Retry when the number of quotes obtained are not enough.
- Return the wallet from an environment variable rather than creating a file.
- Error when decrypting a wallet that was imported without the `0x` prefix.
- Issue when selecting a wallet that had multiple wallet files (unencrypted & encrypted).

### Launchpad

#### Added

- Added `--network-id` and `--antnode-path` args for testing

## 2024-11-25

### Network

#### Fixed

- Make native kad bootstrap interval more random. So that when running multiple nodes
  on one machine, there is no resource usage spike appears with fixed interval.

## 2024-11-13

### Network

#### Fixed

- During a restart, the node builds a cache of locally restored records,
  which is used to improve the speed of the relevant records calculation.
  The restored records were not being added to the cache.
  This has now been corrected.

## 2024-11-12

### Network

#### Added

- Enable the `websockets` connection feature, for compatibility with the webapp.

#### Fixed

- Reduce incorrect logging of connection errors.
- Fixed verification for crdt operations.
- Pick chunk-proof verification (for storage confirmation) candidates more equally.

### Launchpad

#### Added

- Display an error when Launchpad is not whitelisted on Windows devices.
- Ctrl+V can paste rewards address on pop up section.

#### Changed

- Help section copy changed after beta phase.
- Update ratatui and throbbber library versions.

#### Fixed

- We display starting status when not running nodes

### Client

#### Added

- Support pre-paid put operations.
- Add the necessary WASM bindings for the webapp to be able to upload private data to a vault
  and fetch it again.

#### Changed

- Chunks are now downloaded in parallel.
- Rename some WASM methods to be more conventional for web.

## 2024-11-07

### Launchpad

#### Added

- You can select a node. Pressing L will show its logs.
- The upgrade screen has an estimated time.

#### Changed

- Launchpad now uses multiple threads. This allows the UI to be functional while nodes are being
  started, upgraded, and so on.
- Mbps vs Mb units on status screen.

#### Fixed

- Spinners now move when updating.

## 2024-11-06

### Network

#### Added

- Remove outdated record copies that cannot be decrypted. This is used when a node is restarted.

#### Changed

- The node will only restart at the end of its process if it has explicitly been requested in the
  RPC restart command. This removes the potential for creation of undesired new processes.
- Range search optimization to reduce resource usage.
- Trigger record_store pruning earlier. The threshold lowered from 90% to 10% to improve the disk
  usage efficiency.

#### Fixed

- Derive node-side record encryption details from the node's keypair. This ensures data is retained
  in a restart.

### Client

#### Changed

- When paying for quotes through the API, the contract allowance will be set to ~infinite instead of
  the specific amount needed. This is to reduce the amount of approval transactions needed for doing
  quote payments.

### Node Manager

#### Fixed

- The `--rewards-address` argument is retained on an upgrade

### Launchpad

#### Added

- Support for upgrading nodes version
- Support for Ctrl+V on rewards address
- More error handling
- Use 5 minute interval between upgrades

#### Changed

- Help screen after beta
- New Ratatui version 0.29.0

## 2024-10-28

### Autonomi API/CLI

#### Added 

- Private data support.
- Local user data support.
- Network Vault containing user data encrypted.
- Archives with Metadata.
- Prepaid upload support for data_put using receipts.

#### Changed

- Contract token approval amount set to infinite before doing data payments.

### Client

#### Added

- Expose APIs in WASM (e.g. archives, vault and user data within vault).
- Uploads are not run in parallel.
- Support for local wallets.
- Provide `wallet create` command.
- Provide `wallet balance` command.

#### Changed

- Take metadata from file system and add `uploaded` field for time of upload.

#### Fixed

- Make sure we use the new client path throughout the codebase

### Network

#### Added

- Get range used for store cost and register queries.
- Re-enabled large_file_upload, memcheck, benchmark CI tests.

#### Changed

- Scratchpad modifications to support multiple data encodings.
- Registers are now merged at the network level, preventing failures during update and during
  replication.
- Libp2p config and get range tweaks reduce intensity of operations. Brings down CPU usage
  considerably.
- Libp2p’s native kad bootstrap interval introduced in 0.54.1 is intensive, and as we roll our own,
  we significantly reduce the kad period to lighten the CPU load.
- Wipe node’s storage dir when restarting for new network

#### Fixed

- Fixes in networking code for WASM compatibility (replacing `std::time` with compatible
  alternative).
- Event dropped errors should not happen if the event is not dropped.
- Reduce outdated connection pruning frequency.

### Node Manager

#### Fixed

- Local node register is cleaned up when --clean flag applied (prevents some errors when register
  changes).

### Launchpad

#### Fixed

- Status screen is updated after nodes have been reset.
- Rewards Address is required before starting nodes. User input is required.
- Spinner does not stop spinning after two minutes when nodes are running.

## 2024-10-24

### Network

#### Changed

- The `websockets` feature is removed because it was observed to cause instability.

### Client

#### Changed

- PR #2281 was reverted to restore prior behaviour.

### Launchpad

#### Changed

- The Discord username was replaced with the rewards address.
- Remove the reject terms and conditions pop-up screen.

## 2024-10-22

Unfortunately the entry for this release will not have fully detailed changes. This release is
special in that it's very large and moves us to a new, EVM-based payments system. The Github Release
description has a list of all the merged PRs. If you want more detail, consult the PR list. Normal
service will resume for subsequent releases.

Here is a brief summary of the changes:

- A new `autonomi` CLI that uses EVM payments and replaces the previous `safe` CLI.
- A new `autonomi` API that replaces `sn_client` with a simpler interface.
- The node has been changed to use EVM payments.
- The node runs without a wallet. This increases security and removes the need for forwarding.
- Data is paid for through an EVM smart contract. Payment proofs are not linked to the original
  data.
- Payment royalties have been removed, resulting in less centralization and fees.

## 2024-10-08

### Network

#### Changed

- Optimize auditor tracking by not to re-attempt fetched spend.
- Optimize auditor tracking function by using DashMap and stream.

## 2024-10-07

### Network

#### Changed

- Increase chunk size to 4MB with node size remaining at 32GB
- Bootstrap peer parsing in CI was changed to accommodate new log format in libp2p

### Node Manager

#### Added

- The `add` command has new `--max-log-files` and `--max-archived-log-files` arguments to support
  capping node log output

#### Fixed

- The Discord username on the `--owner` argument will always be converted to lower case

#### Launchpad

### Added

- Increased logging related to app configuration. This could help solving issues on launchpad start
  up.

## 2024-10-03

### Launchpad

### Changed

- Upgrade to `Ratatui` v0.28.1
- Styling and layout fixes

#### Added

- Drives that don't have enough space are being shown and flagged
- Error handling and generic error popup
- New metrics in the `Status` section
- Confirmation needed when changing connection mode

### Fixed

- NAT mode only on first start in `Automatic Connection Mode`
- Force Discord username to be in lowercase

## 2024-10-01

### Launchpad

#### Changed

- Disable node selection on status screen
- We change node size from 5GB to 35GB

## 2024-10-01

### Network

#### Changed

- Increase node storage size from 2GB to 32GB

## 2024-09-24

### Network

#### Fixed

- The auditor now uses width-first tracking, to bring it in alignment with the new wallet.

### Client

#### Added

- The client will perform quote validation to avoid invalid quotes.
- A new high-level client API, `autonomi`. The crate provides most of the features necessary to
  build apps for the Autonomi network.

### Node Manager

#### Fixed

- The node manager status command was not functioning correctly when used with a local network. The
  mechanism for determining whether a node was running was changed to use the path of the service
  process, but this did not work for a local network. The status command now differentiates between
  a local and a service-based network, and the command now behaves as expected when using a local
  network.

### Documentation

- In the main README for the repository, the four network keys were updated to reflect the keys
  being used  by the new stable network.

## 2024-09-12

### Network

#### Changed

- The circuit-bytes limit is increased. This enables `libp2p-relay` to forward large records, such
  as `ChunkWithPayment`, enabling home nodes to be notified that they have been paid.

## 2024-09-09

### Network

#### Added

- More logging for storage errors and setting the responsible range.

#### Changed

- The node's store cost calculation has had various updates:
    + The minimum and maximum were previously set to 10 and infinity. They've now been updated to 1
      and 1 million, respectively.
    + We are now using a sigmoid curve, rather than a linear curve, as the base curve. The previous
      curve only grew steep when the storage capacity was 40 to 60 percent.
    + The overall calculation is simplified.
- We expect the updates to the store cost calculation to prevent 'lottery' payments, where one node
  would have abnormally high earnings.
- The network version string, which is used when both nodes and clients connect to the network, now
  uses the version number from the `sn_protocol` crate rather than `sn_networking`. This is a
  breaking change in `sn_networking`.
- External address management is improved. Before, if anyone observed us at a certain public
  IP+port, we would trust that and add it if it matches our local port. Now, we’re keeping track and
  making sure we only have a single external address that we set when we’ve been observed as that
  address a certain amount of times (3 by default). It should even handle cases where our IP changes
  because of (mobile) roaming.
- The `Spend` network data type has been refactored to make it lighter and simpler.
- The entire transaction system has been redesigned; the code size and complexity have been reduced
  by an order of magnitude.
- In addition, almost 10 types were removed from the transaction code, further reducing the
  complexity.
- The internals of the `Transfer` and `CashNote` types have been reworked.
- The replication range has been reduced, which in turn reduces the base traffic for replication.

### Client

#### Fixed

- Registers are fetched and merged correctly. 

### Launchpad

#### Added

- A connection mode feature enables users to select whether they want their nodes to connect to the
  network using automatic NAT detection, upnp, home network, or custom port mappings in their
  connection. Previously, the launchpad used NAT detection on the user’s behalf. By providing the
  ability to explore more connection modes, hopefully this will get more users connected.

#### Changed

- On the drive selection dialog, drives to which the user does not have read or write access are
  marked as such.

### Documentation

#### Added

- A README was provided for the `sn_registers` crate. It intends to give a comprehensive
  understanding of the register data type and how it can be used by developers.

#### Changed

- Provided more information on connecting to the network using the four keys related to funds, fees
  and royalties.

## 2024-09-02

### Launchpad

#### Fixed

- Some users encountered an error when the launchpad started, related to the storage mountpoint not
  being set. We fix the error by providing default values for the mountpoint settings when the
  `app_data.json` file doesn't exist (fresh install). In the case where it does exist, we validate
  the contents.

## 2024-08-27

### Network

#### Added

- The node will now report its bandwidth usage through the metrics endpoint.
- The metrics server has a new `/metadata` path which will provide static information about the node,
  including peer ID and version.
- The metrics server exposes more metrics on store cost derivation. These include relevant record
  count and number of payments received.
- The metrics server exposes metrics related to bad node detection.
- Test to confirm main key can’t verify signature signed by child key.
- Avoid excessively high quotes by pruning records that are not relevant.

#### Changed

- Bad node detection and bootstrap intervals have been increased. This should reduce the number
  of messages being sent.
- The spend parent verification strategy was refactored to be more aligned with the public
  network.
- Nodes now prioritize local work over new work from the network, which reduces memory footprint.
- Multiple GET queries to the same address are now de-duplicated and will result in a single query
  being processed.
- Improve efficiency of command handling and the record store cache.
- A parent spend is now trusted with a majority of close group nodes, rather than all of them. This
  increases the chance of the spend being stored successfully when some percentage of nodes are slow
  to respond.

#### Fixed

- The amount of bytes a home node could send and receive per relay connection is increased. This
  solves a problem where transmission of data is interrupted, causing home nodes to malfunction.
- Fetching the network contacts now times out and retries. Previously we would wait for an excessive
  amount of time, which could cause the node to hang during start up.
- If a node has been shunned, we inform that node before blocking all communication to it.
- The current wallet balance metric is updated more frequently and will now reflect the correct
  state.
- Avoid burnt spend during forwarding by correctly handling repeated CashNotes and confirmed spends.
- Fix logging for CashNote and confirmed spend disk ops
- Check whether a CashNote has already been received to avoid duplicate CashNotes in the wallet.

### Node Manager

#### Added

- The `local run` command supports `--metrics-port`, `--node-port` and `--rpc-port` arguments.
- The `start` command waits for the node to connect to the network before attempting to start the
  next node. If it takes more than 300 seconds to connect, we consider that a failure and move to the
  next node. The `--connection-timeout` argument can be used to vary the timeout. If you prefer the
  old behaviour, you can use the `--interval` argument, which will continue to apply a static,
  time-based interval.

#### Changed

- On an upgrade, the node registry is saved after each node is processed, as opposed to waiting
  until the end. This means if there is an unexpected failure, the registry will have the
  information about which nodes have already been upgraded.

### Launchpad

#### Added

- The user can choose a different drive for the node's data directory.
- New sections in the UI: `Options` and `Help`.
- A navigation bar has been added with `Status`, `Options` and `Help` sections.
- The node's logs can be viewed from the `Options` section.

#### Changed

- Increased spacing for title and paragraphs.
- Increased spacing on footer.
- Increased spacing on box titles.
- Moved `Discord Username` from the top title into the `Device Status` section.
- Made the general layout of `Device Status` more compact.

### Client

#### Added

- The `safe files download` command now displays duration per file.

#### Changed

- Adjust the put and get configuration scheme to align the client with a more realistic network
  which would have some percentage of slow nodes.
- Improved spend logging to help debug the upload process.

#### Fixed

- Avoid a corrupt wallet by terminating the payment process during an unrecoverable error.

## 2024-07-25

### Network

#### Added

- Protection against an attack allowing bad nodes or clients to shadow a spend (make it disappear)
  through spamming.
- Nodes allow more relayed connections through them. Also, home nodes will relay through 4 nodes
  instead of 2. Without these changes, relays were denying new connections to home nodes, making them
  difficult to reach.
- Auditor tracks forwarded payments using the default key. 
- Auditor tracks burnt spend attempts and only credits them once.
- Auditor collects balance of UTXOs.
- Added different attack types to the spend simulation test to ensure spend validation is solid.
- Bad nodes and nodes with a mismatched protocol are now added to a block list. This reduces the
  chance of a network interference and the impact of a bad node in the network.
- The introduction of a record-store cache has significantly reduced the node's disk IO. As a side
  effect, the CPU does less work, and performance improves. RAM usage has increased by around 25MB per
  node, but we view this as a reasonable trade off.

#### Changed

- For the time being, hole punching has been removed. It was causing handshake time outs, resulting
  in home nodes being less stable. It will be re-enabled in the future.
- Force connection closure if a peer is using a different protocol.
- Reserve trace level logs for tracking event statistics. Now you can use `SN_LOG=v` to get more
  relevant logs without being overwhelmed by event handling stats.
- Chunk verification is now probabilistic, which should reduce messaging. In combination with
  replication messages also being reduced, this should result in a bandwidth usage reduction of
  ~20%.

#### Fixed

- During payment forwarding, CashNotes are removed from disk and confirmed spends are stored to
  disk. This is necessary for resolving burnt spend attempts for forwarded payments.
- Fix a bug where the auditor was not storing data to disk because of a missing directory.
- Bootstrap peers are not added as relay candidates as we do not want to overwhelm them.

### Client

#### Added

- Basic global documentation for the `sn_client` crate.
- Option to encrypt the wallet private key with a password, in a file called
  `main_secret_key.encrypted`, inside the wallet directory.
- Option to load a wallet from an encrypted secret-key file using a password.
- The `wallet create` command provides a `--password` argument to encrypt the wallet.
- The `wallet create` command provides a `--no-password` argument skip encryption.
- The `wallet create` command provides a `--no-replace` argument to suppress a prompt to replace an
  existing wallet.
- The `wallet create` command provides a `--key` argument to create a wallet from a hex-encoded
  private key.
- The `wallet create` command provides a `--derivation` argument to set a derivation passphrase to
  be used with the mnemonic to create a new private key.
- A new `wallet encrypt` command encrypts an existing wallet.

#### Changed

- The `wallet address` command no longer creates a new wallet if no wallet exists.
- The `wallet create` command creates a wallet using the account mnemonic instead of requiring a
  hex-encoded secret key.
- The `wallet create` `--key` and `--derivation` arguments are mutually exclusive.

### Launchpad

#### Fixed

- The `Total Nanos Earned` stat no longer resets on restart.

### RPC Client

#### Added

- A `--version` argument shows the binary version

### Other

#### Added

- Native Apple Silicon (M-series) binaries have been added to our releases, meaning M-series Mac
  users do not have to rely on running Intel binaries with Rosetta.

## 2024-07-10

### Network

#### Added

- The node exposes more metrics, including its uptime, number of connected peers, number of peers in
  the routing table, and the number of open connections. These will help us more effectively
  diagnose user issues.

#### Changed

- Communication between node and client is strictly limited through synchronised public keys. The
  current beta network allows the node and client to use different public keys, resulting in
  undefined behaviour and performance issues. This change mitigates some of those issues and we also
  expect it to prevent other double spend issues.
- Reduced base traffic for nodes, resulting in better upload performance. This will result in better
  distribution of nanos, meaning users with a smaller number of nodes will be expected to receive
  nanos more often.

#### Fixed

- In the case where a client retries a failed upload, they would re-send their payment. In a rare
  circumstance, the node would forward this reward for a second time too. This is fixed on the node.
- Nodes are prevented from double spending under rare circumstances.
- ARM builds are no longer prevented from connecting to the network.

### Node Manager

#### Added

- Global `--debug` and `--trace` arguments are provided. These will output debugging and trace-level
  logging, respectively, direct to stderr.

#### Changed

- The mechanism used by the node manager to refresh its state is significantly changed to address
  issues that caused commands to hang for long periods of time. Now, when using commands like
  `start`, `stop`, and `reset`, users should no longer experience the commands taking excessively
  long to complete.
- The `nat-detection run` command provides a default list of servers, meaning the `--servers`
  argument is now optional.

### Launchpad

#### Added

- Launchpad and node versions are displayed on the user interface.

#### Changed

- The node manager change for refreshing its state also applies to the launchpad. Users should
  experience improvements in operations that appeared to be hanging but were actually just taking
  an excessive amount of time to complete.

#### Fixed

- The correct primary storage will now be selected on Linux and macOS.
