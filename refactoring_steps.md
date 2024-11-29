# Refactoring Steps for Autonomi Network

## Phase 1: Client API Refactoring
1. **Remove Connection Management from API**
   - Remove `connect()` method from client API
   - Move connection handling into individual operations
   - Each operation should handle its own connection lifecycle
   - Have a bootstrap mechanism that reads a bootstrrp_cache.json file or passed in via command line or ENV_VAR 
   - Use the bootstrap cache to connect to the network
   - During network requests collect peers connection info
   - Every minute update the bootstrap cache (limit entries to last 1500 seen)
   - on startup read the bootstrap cache file to get peers to connect to
   - on shutdown write the bootstrap cache file
   - all internal connect commands will use the nodes we have in ram 
   - update wasm and python bindings to use all the above 
   - test before going any further


2. **Data Type Operations**
   - **Chunks** (Mostly Complete)
     - Existing: `chunk_get`, `chunk_upload_with_payment`
     - Add: Better error handling for size limits
     - Language Bindings:
       - Python:
         - Implement `chunk_get`, `chunk_upload_with_payment` methods
         - Add size validation
         - Add comprehensive tests
         - Document API usage
       - WASM:
         - Implement `chunk_get`, `chuunk_upload_with_paymentput` methods
         - Add JavaScript examples
         - Add integration tests
         - Document browser usage
   
   - **Registers** (Integration Needed)
     - Existing in sn_registers:
       - CRDT-based implementation
       - `merge` operations
       - User-managed conflict resolution
     - To Add:
       - Client API wrappers in autonomi
       - Simplified append/merge interface
       - Connection handling in operations
     - Language Bindings:
       - Python:
         - Implement register CRUD operations
         - Add conflict resolution examples
         - Add unit and integration tests
         - Document CRDT usage
       - WASM:
         - Implement register operations
         - Add browser-based examples
         - Add JavaScript tests
         - Document concurrent usage
   
   - **Scratchpad (Vault)** (Enhancement Needed)
     - Existing in sn_protocol:
       - Basic scratchpad implementation
       - `update_and_sign` functionality
     - To Add:
       - Client API wrappers in autonomi
       - Simplified update/replace interface
       - Connection handling in operations
     - Language Bindings:
       - Python:
         - Implement vault operations
         - Add encryption examples
         - Add comprehensive tests
         - Document security features
       - WASM:
         - Implement vault operations
         - Add browser storage examples
         - Add security tests
         - Document encryption usage

3. **Transaction System Refactoring** (Priority)
   - Make transaction types generic in sn_transfers
   - Update client API to support generic transactions
   - Implement owner-based validation
   - Add support for optional additional keys
   - Implement transaction history verification

## Phase 2: Payment System Integration
1. **EVM Integration**
   - Integrate existing EVM implementation
   - Add runtime configuration support
   - Connect with transaction system

2. **Payment Processing**
   - Integrate with data operations
   - Add payment verification
   - Implement tracking system

## Phase 3: Testing and Documentation
1. **Testing**
   - Add unit tests for new API methods
   - Integration tests for complete workflows
   - Payment system integration tests

2. **Documentation**
   - Update API documentation
   - Add usage examples
   - Document error conditions
   - Include best practices

## Safe Network Health Management

### Core Parameters

#### Timing Intervals
- Replication: 90-180 seconds (randomized)
- Bad Node Detection: 300-600 seconds (randomized)
- Uptime Metrics: 10 seconds
- Record Cleanup: 3600 seconds (1 hour)
- Chunk Proof Retry: 15 seconds between attempts

#### Network Parameters
- Close Group Size: Defined by CLOSE_GROUP_SIZE constant
- Replication Target: REPLICATION_PEERS_COUNT closest nodes
- Minimum Peers: 100 (for bad node detection)
- Bad Node Consensus: Requires close_group_majority()
- Max Chunk Proof Attempts: 3 before marking as bad node

### Health Management Algorithms

#### 1. Bad Node Detection
```rust
Process:
1. Triggered every 300-600s when peers > 100
2. Uses rolling index (0-511) to check different buckets
3. For each bucket:
   - Select subset of peers
   - Query their closest nodes
   - Mark as bad if majority report shunning
4. Records NodeIssue::CloseNodesShunning
```

#### 2. Network Replication
```rust
Process:
1. Triggered by:
   - Every 90-180s interval
   - New peer connection
   - Peer removal
   - Valid record storage
2. Execution:
   - Get closest K_VALUE peers
   - Sort by XOR distance
   - Verify local storage
   - Replicate to REPLICATION_PEERS_COUNT nodes
```

#### 3. Routing Table Management
```rust
Components:
1. K-bucket organization by XOR distance
2. Peer tracking and metrics
3. Connection state monitoring
4. Regular table cleanup
5. Dynamic peer replacement
```

### Protection Mechanisms

#### 1. Data Integrity
- Chunk proof verification
- Record validation
- Replication confirmation
- Storage verification

#### 2. Network Resilience
- Distributed consensus for bad nodes
- Rolling health checks
- Randomized intervals
- Subset checking for efficiency

#### 3. Resource Optimization
- Periodic cleanup of irrelevant records
- Limited retry attempts
- Targeted replication
- Load distribution through rolling checks

### Metrics Tracking
- Peer counts and stability
- Replication success rates
- Network connectivity
- Bad node detection events
- Resource usage and cleanup

### Key Improvements
1. Reduced resource usage in bad node detection
2. Optimized replication targeting
3. Better load distribution
4. Enhanced peer verification
5. Efficient cleanup mechanisms

This system creates a self-maintaining network capable of:
- Identifying and removing problematic nodes
- Maintaining data redundancy
- Optimizing resource usage
- Ensuring network stability
- Providing reliable peer connections
