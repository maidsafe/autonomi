# Bootstrap Cache PRD

## Overview
This document outlines the design and implementation of a decentralized bootstrap cache system for the Safe Network. This system replaces the current centralized "bootstrap node" concept with a fully decentralized approach where all nodes are equal participants.

## Goals
- Remove the concept of dedicated "bootstrap nodes"
- Implement a shared local cache system for both nodes and clients
- Reduce infrastructure costs
- Improve network stability and decentralization
- Simplify the bootstrapping process

## Non-Goals
- Creating any form of centralized node discovery
- Implementing DNS-based discovery
- Maintaining long-term connections between nodes
- Running HTTP servers on nodes

## Technical Design

### Bootstrap Cache File
- Location: 
  - Unix/Linux: `/var/safe/bootstrap_cache.json`
  - macOS: `/Library/Application Support/Safe/bootstrap_cache.json`
  - Windows: `C:\ProgramData\Safe\bootstrap_cache.json`
- Format: JSON file containing:
  ```json
  {
    "last_updated": "ISO-8601-timestamp",
    "peers": [
      {
        "addr": "multiaddr-string", // e.g., "/ip4/1.2.3.4/udp/1234/quic-v1"
        "last_seen": "ISO-8601-timestamp",
        "success_count": "number",
        "failure_count": "number"
      }
    ]
  }
  ```

### Cache Management
1. **Writing Cache**
   - Write to cache when routing table changes occur
   - Write to cache on clean node/client shutdown
   - Keep track of successful/failed connection attempts
   - Limit cache size to prevent bloat (e.g., 1000 entries)
   - Handle file locking for concurrent access from multiple nodes/clients

2. **Reading Cache**
   - On startup, read shared local cache if available
   - If cache peers are unreachable:
     1. Try peers from `--peer` argument or `SAFE_PEERS` env var
     2. If none available, fetch from network contacts URL
     3. If local feature enabled, discover through mDNS
   - Sort peers by connection success rate

### Node Implementation
1. **Cache Updates**
   - Use Kademlia routing table as source of truth
   - Every period, copy nodes from routing table to cache
   - Track peer reliability through:
     - Successful/failed connection attempts
     - Response times
     - Data storage and retrieval success rates

2. **Startup Process**
   ```rust
   async fn startup() {
       // 1. Get initial peers
       let peers = PeersArgs::get_peers().await?;
       
       // 2. Initialize Kademlia with configuration
       let kad_cfg = KademliaConfig::new()
           .set_kbucket_inserts(Manual)
           .set_query_timeout(KAD_QUERY_TIMEOUT_S)
           .set_replication_factor(REPLICATION_FACTOR)
           .disjoint_query_paths(true);
       
       // 3. Begin continuous bootstrap process
       loop {
           bootstrap_with_peers(peers).await?;
           
           // If we have enough peers, slow down bootstrap attempts
           if connected_peers >= K_VALUE {
               increase_bootstrap_interval();
           }
           
           // Update cache with current routing table
           update_bootstrap_cache().await?;
           
           sleep(bootstrap_interval).await;
       }
   }
   ```

### Client Implementation
1. **Cache Management**
   - Maintain Kademlia routing table in outbound-only mode
   - Read from shared bootstrap cache
   - Update peer reliability metrics based on:
     - Connection success/failure
     - Data retrieval success rates
     - Response times

2. **Connection Process**
   ```rust
   async fn connect() {
       // 1. Get initial peers
       let peers = PeersArgs::get_peers().await?;
       
       // 2. Initialize client-mode Kademlia
       let kad_cfg = KademliaConfig::new()
           .set_kbucket_inserts(Manual)
           .set_protocol_support(Outbound) // Clients only make outbound connections
           .disjoint_query_paths(true);
       
       // 3. Connect to peers until we have enough
       while connected_peers < K_VALUE {
           bootstrap_with_peers(peers).await?;
           
           // Update peer reliability in cache
           update_peer_metrics().await?;
           
           // Break if we've tried all peers
           if all_peers_attempted() {
               break;
           }
       }
   }
   ```

### Peer Acquisition Process
1. **Order of Precedence**
   - Command line arguments (`--peer`)
   - Environment variables (`SAFE_PEERS`)
   - Local discovery (if enabled)
   - Network contacts URL

2. **Network Contacts**
   - URL: `https://sn-testnet.s3.eu-west-2.amazonaws.com/network-contacts`
   - Format: One multiaddr per line
   - Fallback mechanism when no local peers available
   - Retries with exponential backoff (max 7 attempts)

3. **Local Discovery**
   - Uses mDNS when `local` feature is enabled
   - Useful for development and testing
   - Not used in production environments

### Cache File Synchronization
1. **File Locking**
   - Use file-system level locks for synchronization
   - Read locks for cache queries
   - Write locks for cache updates
   - Exponential backoff for lock acquisition

2. **Update Process**
   ```rust
   async fn update_cache(peers: Vec<PeerInfo>) -> Result<()> {
       // 1. Check if file is read-only
       if is_readonly(cache_path) {
           warn!("Cache file is read-only");
           return Ok(());
       }
       
       // 2. Acquire write lock
       let file = acquire_exclusive_lock(cache_path)?;
       
       // 3. Perform atomic write
       atomic_write(file, peers).await?;
       
       Ok(())
   }
   ```

## Success Metrics
- Reduction in bootstrap time
- More evenly distributed network load
- Improved network resilience
- Higher peer connection success rates

## Security Considerations
- Validate peer multiaddresses before caching
- Protect against malicious cache entries
- Handle file permissions securely
- Prevent cache poisoning attacks
- Implement rate limiting for cache updates

## Future Enhancements
- Peer prioritization based on network metrics
- Geographic-based peer selection
- Advanced reputation system
- Automated peer list pruning
- Cross-client cache sharing mechanisms
