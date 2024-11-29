# Bootstrap Cache

A robust peer caching system for the Safe Network that provides persistent storage and management of network peer addresses. This crate handles peer discovery, caching, and reliability tracking with support for concurrent access across multiple processes.

## Features

### Storage and Accessibility
- System-wide accessible cache location
- Configurable primary cache location
- Automatic fallback to user's home directory (`~/.safe/bootstrap_cache.json`)
- Cross-process safe with file locking
- Atomic write operations to prevent cache corruption

### Concurrent Access
- Thread-safe in-memory cache with `RwLock`
- File system level locking for cross-process synchronization
- Shared (read) and exclusive (write) lock support
- Exponential backoff retry mechanism for lock acquisition

### Data Management
- Automatic cleanup of stale and unreliable peers
- Configurable maximum peer limit
- Peer reliability tracking (success/failure counts)
- Atomic file operations for data integrity

## Configuration Options

The `BootstrapConfig` struct provides the following configuration options:

```rust
pub struct BootstrapConfig {
    /// List of endpoints to fetch initial peers from
    pub endpoints: Vec<String>,
    
    /// Maximum number of peers to maintain in the cache
    pub max_peers: usize,
    
    /// Path where the cache file will be stored
    pub cache_file_path: PathBuf,
    
    /// How long to wait for peer responses
    pub peer_response_timeout: Duration,
    
    /// Interval between connection attempts
    pub connection_interval: Duration,
    
    /// Maximum number of connection retries
    pub max_retries: u32,
}
```

### Option Details

#### `endpoints`
- List of URLs to fetch initial peers from when cache is empty
- Example: `["https://sn-node1.s3.amazonaws.com/peers", "https://sn-node2.s3.amazonaws.com/peers"]`
- Default: Empty vector (no endpoints)

#### `max_peers`
- Maximum number of peers to store in cache
- When exceeded, oldest peers are removed first
- Default: 1500 peers

#### `cache_file_path`
- Location where the cache file will be stored
- Falls back to `~/.safe/bootstrap_cache.json` if primary location is not writable
- Example: `/var/lib/safe/bootstrap_cache.json`

#### `peer_response_timeout`
- Maximum time to wait for a peer to respond
- Affects peer reliability scoring
- Default: 60 seconds

#### `connection_interval`
- Time to wait between connection attempts
- Helps prevent network flooding
- Default: 10 seconds

#### `max_retries`
- Maximum number of times to retry connecting to a peer
- Affects peer reliability scoring
- Default: 3 attempts

## Usage Modes

### Default Mode
```rust
let config = BootstrapConfig::default();
let store = CacheStore::new(config).await?;
```
- Uses default configuration
- Loads peers from cache if available
- Falls back to configured endpoints if cache is empty

### Test Network Mode
```rust
let args = PeersArgs {
    test_network: true,
    peers: vec![/* test peers */],
    ..Default::default()
};
let store = CacheStore::from_args(args, config).await?;
```
- Isolates from main network cache
- Only uses explicitly provided peers
- No cache persistence

### Local Mode
```rust
let args = PeersArgs {
    local: true,
    ..Default::default()
};
let store = CacheStore::from_args(args, config).await?;
```
- Returns empty store
- Suitable for local network testing
- Uses mDNS for peer discovery

### First Node Mode
```rust
let args = PeersArgs {
    first: true,
    ..Default::default()
};
let store = CacheStore::from_args(args, config).await?;
```
- Returns empty store
- No fallback to endpoints
- Used for network initialization

## Error Handling

The crate provides comprehensive error handling for:
- File system operations
- Network requests
- Concurrent access
- Data serialization/deserialization
- Lock acquisition

All errors are propagated through the `Result<T, Error>` type with detailed error variants.

## Thread Safety

The cache store is thread-safe and can be safely shared between threads:
- `Clone` implementation for `CacheStore`
- Internal `Arc<RwLock>` for thread-safe data access
- File system locks for cross-process synchronization

## Logging

Comprehensive logging using the `tracing` crate:
- Info level for normal operations
- Warn level for recoverable issues
- Error level for critical failures
- Debug level for detailed diagnostics

## License

This SAFE Network Software is licensed under the General Public License (GPL), version 3 ([LICENSE](LICENSE) http://www.gnu.org/licenses/gpl-3.0.en.html).
