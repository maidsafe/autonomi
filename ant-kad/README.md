# ant-kad

Kademlia Distributed Hash Table (DHT) implementation for the Autonomi Network.

This module is a fork of `libp2p-kad` that has been integrated into the Autonomi monorepo to provide complete control over the Kademlia implementation. It maintains API compatibility with the original libp2p-kad module while enabling Autonomi-specific customizations and optimizations.

## Overview

The Kademlia DHT is a peer-to-peer distributed hash table that enables:
- Decentralized storage and retrieval of key-value pairs
- Efficient peer discovery and routing
- Network resilience through redundant storage
- Logarithmic lookup performance

## Features

- **API Compatibility**: Drop-in replacement for `libp2p::kad`
- **Network Integration**: Seamless integration with libp2p networking stack
- **Customizable**: Configurable parameters for Autonomi-specific optimizations
- **Performance**: Optimized for the Autonomi Network's storage and retrieval patterns

## Usage

```rust
use ant_kad::{Behaviour, Config, Event, Record};
use libp2p::swarm::{SwarmBuilder, NetworkBehaviour};

#[derive(NetworkBehaviour)]
struct MyBehaviour {
    kademlia: Behaviour<MemoryStore>,
}

// Configure and initialize Kademlia
let mut config = Config::new(protocol_name);
let store = MemoryStore::new(local_peer_id);
let kademlia = Behaviour::with_config(local_peer_id, store, config);
```

## Migration from libp2p-kad

This module provides the same API as `libp2p-kad`. To migrate:

1. Update imports:
   ```rust
   // Before
   use libp2p::kad::{Behaviour, Event, Record};
   
   // After  
   use ant_kad::{Behaviour, Event, Record};
   ```

2. Update Cargo.toml dependencies:
   ```toml
   # Remove "kad" from libp2p features
   libp2p = { version = "0.55.0", features = ["identify", "request-response"] }
   
   # Add ant-kad
   ant-kad = { path = "../ant-kad" }
   ```

## Key Types

- `Behaviour` - Main Kademlia behavior for libp2p integration
- `Event` - Kademlia events (queries, routing updates, etc.)
- `Record` - DHT records with keys, values, and metadata
- `Config` - Configuration for Kademlia parameters
- `Query*` types - Query results and error handling

## Development

This module is designed to be evolved independently from upstream libp2p-kad, enabling:
- Autonomi-specific performance optimizations
- Custom routing strategies
- Enhanced error handling and diagnostics
- Integration with Autonomi's storage layer

## License

Licensed under the General Public License (GPL), version 3.