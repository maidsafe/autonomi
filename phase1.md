# Phase 1: Create Proof-of-Concept Branch and Extract Kademlia Module

## Objective
Create a proof-of-concept branch that extracts the libp2p-kad module into a standalone, transport-agnostic module within the autonomi project, preparing for integration with iroh.

## Background
The autonomi project currently uses libp2p for networking, including its Kademlia DHT implementation. We need to replace libp2p with iroh for better NAT traversal and efficiency, while keeping the Kademlia functionality. This is the first phase of a multi-phase migration.

## Tasks

### 1. Create Feature Branch
```bash
git checkout -b feat/iroh-migration-poc
```

### 2. Extract libp2p-kad Source Code

Create the following directory structure:
```
ant-node/src/networking/kad/
├── mod.rs
├── behaviour.rs
├── kbucket.rs
├── query.rs
├── record.rs
├── protocol.rs
├── addresses.rs
├── jobs.rs
└── handlers.rs
```

Copy the essential components from libp2p-kad (v0.46.2 - matching your current version):
- Download libp2p-kad source from: https://github.com/libp2p/rust-libp2p/tree/master/protocols/kad/src
- Focus on core Kademlia logic, excluding libp2p-specific networking code

### 3. Create Transport Abstraction Layer

Create `ant-node/src/networking/kad/transport.rs`:
```rust
use async_trait::async_trait;
use bytes::Bytes;
use std::time::Duration;

/// Transport-agnostic peer identifier
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct KadPeerId(pub Vec<u8>);

/// Abstract transport for Kademlia operations
#[async_trait]
pub trait KademliaTransport: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;
    
    /// Send a Kademlia message to a peer
    async fn send_message(
        &self,
        peer: &KadPeerId,
        message: KadMessage,
    ) -> Result<KadResponse, Self::Error>;
    
    /// Check if we're connected to a peer
    async fn is_connected(&self, peer: &KadPeerId) -> bool;
    
    /// Get our local peer ID
    fn local_peer_id(&self) -> KadPeerId;
}

/// Kademlia protocol messages
#[derive(Debug, Clone)]
pub enum KadMessage {
    FindNode { key: Vec<u8> },
    FindValue { key: Vec<u8> },
    PutValue { key: Vec<u8>, value: Vec<u8> },
    GetProviders { key: Vec<u8> },
    AddProvider { key: Vec<u8>, provider: KadPeerId },
}

#[derive(Debug, Clone)]
pub enum KadResponse {
    Nodes(Vec<(KadPeerId, Vec<String>)>), // peer id + addresses
    Value(Vec<u8>),
    Providers(Vec<KadPeerId>),
    Ok,
}
```

### 4. Adapt Core Kademlia Components

Modify the extracted files to use the transport abstraction:

In `behaviour.rs`:
- Replace `libp2p::PeerId` with `KadPeerId`
- Replace `libp2p::swarm::NetworkBehaviour` trait with a custom trait
- Remove dependencies on `libp2p_swarm` and `libp2p_core`
- Keep the core Kademlia algorithm logic intact

In `kbucket.rs`:
- Replace `libp2p::PeerId` with `KadPeerId`
- Keep the k-bucket data structure and distance calculations
- Remove any libp2p-specific imports

### 5. Create Compatibility Layer

Create `ant-node/src/networking/kad/compat.rs`:
```rust
use libp2p::PeerId;
use crate::networking::kad::transport::KadPeerId;

/// Convert libp2p PeerId to KadPeerId
impl From<PeerId> for KadPeerId {
    fn from(peer_id: PeerId) -> Self {
        KadPeerId(peer_id.to_bytes())
    }
}

/// Convert KadPeerId to libp2p PeerId
impl TryFrom<KadPeerId> for PeerId {
    type Error = String;
    
    fn try_from(kad_peer: KadPeerId) -> Result<Self, Self::Error> {
        PeerId::from_bytes(&kad_peer.0)
            .map_err(|e| format!("Invalid peer id: {}", e))
    }
}
```

### 6. Create Temporary libp2p Transport Implementation

Create `ant-node/src/networking/kad/libp2p_transport.rs`:
```rust
/// Temporary implementation using existing libp2p infrastructure
pub struct Libp2pTransport {
    swarm: Arc<Mutex<libp2p::Swarm<...>>>,
}

#[async_trait]
impl KademliaTransport for Libp2pTransport {
    type Error = NetworkError;
    
    async fn send_message(
        &self,
        peer: &KadPeerId,
        message: KadMessage,
    ) -> Result<KadResponse, Self::Error> {
        // Convert and send using existing libp2p infrastructure
        // This maintains compatibility during migration
    }
    
    // ... implement other methods
}
```

### 7. Update Cargo.toml Dependencies

Add to `ant-node/Cargo.toml`:
```toml
# Keep existing libp2p dependencies for now
# Add new dependencies for extracted kad module
async-trait = "0.1"
bytes = "1.0"

# Prepare for iroh (don't use yet)
# iroh = { version = "0.28", optional = true }
```

### 8. Integration Points

Identify and document all places where the existing code interfaces with libp2p-kad:
- `ant-node/src/networking/driver/event/kad.rs`
- `ant-node/src/networking/driver/mod.rs`
- `ant-node/src/networking/network.rs`

Create a migration checklist in `ant-node/src/networking/kad/MIGRATION.md`.

### 9. Testing

Create test modules:
- `ant-node/src/networking/kad/tests/mod.rs`
- `ant-node/src/networking/kad/tests/kbucket_tests.rs`
- `ant-node/src/networking/kad/tests/routing_tests.rs`

Ensure all core Kademlia functionality works with the abstracted transport.

## Validation Criteria

1. The extracted Kademlia module compiles without libp2p-specific dependencies
2. Unit tests pass for k-bucket operations, distance calculations, and routing logic
3. The existing network functionality continues to work using the compatibility layer
4. No breaking changes to the external API
5. Clear separation between Kademlia logic and transport implementation

## Notes

- Keep all existing functionality working during this phase
- Document any assumptions or limitations discovered during extraction
- Create issues for any complex refactoring that should be deferred
- This is a proof-of-concept - optimize for clarity over performance

## Next Phase
Phase 2 will implement the iroh transport adapter and begin parallel testing.
