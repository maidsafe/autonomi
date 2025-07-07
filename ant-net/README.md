# ant-net

Network abstraction layer for the Autonomi Network.

## Overview

`ant-net` provides a clean abstraction layer over libp2p networking functionality, designed to:

- **Encapsulate libp2p complexity**: Hide libp2p implementation details behind clean, high-level interfaces
- **Enable network layer flexibility**: Allow for potential future networking backend changes without affecting upper layers
- **Provide consistent APIs**: Standardize networking patterns across the Autonomi codebase
- **Restrict libp2p imports**: Centralize libp2p dependencies to prevent scattered direct usage

## Architecture

The abstraction is organized into several key components:

- **Transport Layer**: Abstract transport configuration and connection management
- **Behavior System**: Composable network behaviors (Kademlia, Request/Response, etc.)
- **Event Processing**: Unified event handling with priority-based processing
- **Connection Management**: Abstract connection lifecycle and state tracking
- **Protocol Handling**: Generic protocol interfaces and message routing

## Usage

```rust
use ant_net::{AntNet, AntNetBuilder, Transport, NetworkBehaviour};

// Build a network instance
let network = AntNetBuilder::new()
    .with_transport(Transport::quic())
    .with_behaviour(KademliaBehaviour::new())
    .with_behaviour(RequestResponseBehaviour::new())
    .build()?;

// Use the network
network.start().await?;
```

## Design Principles

1. **Minimal Abstraction**: Only abstract what's necessary, avoid over-engineering
2. **Performance Focus**: Maintain libp2p performance characteristics  
3. **Compatibility**: Ensure seamless interoperability with existing libp2p networks
4. **Extensibility**: Support easy addition of new behaviors and protocols
5. **Error Transparency**: Provide clear error propagation and debugging information

## Integration

This crate is designed to replace direct libp2p usage throughout the Autonomi project:

- `ant-node`: Use ant-net for full node networking
- `autonomi`: Use ant-net for client networking
- `ant-protocol`: Continue to use libp2p types for protocol definitions (allowed)

All other crates should depend on `ant-net` rather than `libp2p` directly.