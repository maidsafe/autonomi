# Reachability Check Module

## Overview

The reachability check module is a critical networking component that determines whether an Autonomi Network node can receive direct inbound connections from the internet. This automated assessment ensures network health by allowing only fully capable nodes to participate in the distributed hash table (DHT), preventing performance degradation and routing instability.

### Why Reachability Matters

In peer-to-peer networks, nodes behind NAT/firewall configurations often cannot receive direct connections, severely limiting their ability to participate as full network members. The reachability check provides:

- **Network Performance**: Eliminates unreachable or faulty nodes form the network
- **DHT Stability**: Ensures predictable routing behavior with consistent addressing  
- **Zero Configuration**: Automatically detects and configures optimal network settings
- **Quality Assurance**: Maintains network health by filtering out problematic connectivity

## Core Algorithm

### Three-Stage State Machine

The reachability check operates through a sophisticated state machine tracking each dial attempt:

```rust
pub enum DialState {
    /// Initial dial attempt initiated
    Initiated { at: Instant },
    /// Connection established, waiting for dial-back
    Connected { at: Instant },
    /// Dial-back received after mandatory delay
    DialBackReceived { at: Instant },
}
```

**State Transitions:**
1. **Initiated**: Outbound connection attempt starts
2. **Connected**: Successful connection established with remote peer
3. **DialBackReceived**: Remote peer successfully dials back after 180-second delay

### The Critical 180-Second Delay

The most important technical aspect is the mandatory `DIAL_BACK_DELAY` of 180 seconds:

```rust
const DIAL_BACK_DELAY: Duration = Duration::from_secs(180);

fn transition_to_dial_back_received(&mut self, peer_id: &PeerId) {
    match self {
        DialState::Connected { at } => {
            if at.elapsed() > DIAL_BACK_DELAY {  // Must wait full 180s
                *self = DialState::DialBackReceived { at: Instant::now() };
            }
        }
    }
}
```

**Why 180 Seconds?**
- **NAT Binding Establishment**: Allows sufficient time for NAT/firewall state tables to establish stable bindings
- **False Positive Prevention**: Prevents immediate responses that don't represent true external reachability
- **Network Propagation**: Ensures address observations represent stable, routable endpoints

### Concurrent Dial Management

The system executes exactly **7 concurrent dial attempts** requiring **4/7 majority consensus**:

```rust
pub const MAX_CONCURRENT_DIALS: usize = 7;

pub fn get_majority(value: usize) -> usize {
    if value == 0 { 0 } else { (value / 2) + 1 }  // Returns 4 for value 7
}
```

**Mathematical Rationale:**
- **Fault Tolerance**: Can handle up to 3 failed/corrupted dial attempts
- **Statistical Confidence**: 57% majority provides robust consensus
- **Network Efficiency**: Optimal balance between reliability and resource usage

### Strict Address Consistency Validation

The algorithm enforces **single external address consistency** across all successful dial-backs:

```rust
// All peers must observe identical external address
if external_addrs.len() != 1 {
    error!("Multiple external addresses observed. Terminating the node.");
    result.terminate = true;
}
```

**Why This Strictness?**
- **NAT Predictability**: Multiple addresses indicate problematic NAT behavior
- **DHT Routing Stability**: Inconsistent addressing breaks distributed hash table assumptions
- **Implementation Simplicity**: Single address eliminates complex selection logic

## Technical Architecture

### Module Structure

```
reachability_check/
├── mod.rs           # Main orchestration and swarm event handling
├── dialer.rs        # Dial attempt management and state tracking  
├── listener.rs      # Address discovery and UPnP integration
└── progress.rs      # Time-based progress calculation
```

### Key Components

#### DialManager (`dialer.rs`)
- **State Tracking**: Manages concurrent dial attempts across multiple peers
- **Contact Management**: Handles bootstrap peer filtering and random selection  
- **Retry Logic**: Implements 3-attempt workflow with intelligent error handling
- **Timeout Management**: Enforces timeouts for both connection and dial-back phases

#### Listener Discovery (`listener.rs`)
- **UPnP Detection**: Automatically discovers and configures UPnP port mappings
- **Address Collection**: Gathers all potential listening addresses from network interfaces
- **Protocol Configuration**: Sets up QUIC transport with proper addressing

#### Progress Tracking (`progress.rs`)
- **Time-Based Calculation**: Provides real-time progress feedback across workflow attempts
- **State-Aware Metrics**: Tracks progress based on actual dial state transitions
- **Workflow Management**: Handles progress across multiple retry attempts

## Reachability Status Results

The module returns one of two definitive status results:

### Reachable
```rust
ReachabilityStatus::Reachable {
    local_addr: SocketAddr,    // Local adapter address for binding
    external_addr: SocketAddr,  // External address observed by peers
    upnp: bool,                // UPnP support availability
}
```
**Outcome**: Node can fully participate in the network with direct connectivity

### NotReachable  
```rust
ReachabilityStatus::NotReachable {
    upnp: bool,                    // UPnP support status for diagnostics
    reason: ReachabilityIssue,     // Specific failure reason
}
```
**Outcome**: Node terminates to prevent network degradation

## Critical Design Decisions

### Termination

The system **terminates unreachable nodes**.

**Benefits of Termination:**
- **Network Quality**: Only fully capable nodes participate in DHT operations
- **Predictability**: Ensures consistent node capabilities across the network

### Bootstrap Peer Filtering

The system filters bootstrap contacts to ensure direct connectivity testing:

```rust
let initial_contacts: Vec<Multiaddr> = initial_contacts
    .into_iter()
    .filter(|addr| {
        // Exclude circuit (relay) addresses
        !addr.iter().any(|protocol| matches!(protocol, Protocol::P2pCircuit))
    })
    .filter(|addr| {
        // Require peer ID in address
        addr.iter().any(|protocol| matches!(protocol, Protocol::P2p(_)))
    })
    .collect();
```

### Fault Detection Logic

The system distinguishes between node faults vs network issues:

```rust
pub fn is_faulty(&self) -> bool {
    // Not faulty if at least one connection was established
    for dial_result in self.all_dial_attempts.values() {
        match dial_result {
            DialResult::TimedOutAfterConnecting => return false,  // Connected but no dial-back
            DialResult::SuccessfulDialBack => return false,       // Full success
            _ => {}
        }
    }
    true  // No successful connections = node is faulty
}
```

**Logic**: If the node can establish outbound connections but peers cannot dial back, it indicates network reachability issues rather than node problems.

## Benefits for Antnode

### 1. Network Health Assurance
- **Quality Control**: Prevents nodes with problematic connectivity from degrading network performance
- **DHT Stability**: Ensures all participating nodes can receive direct connections for reliable routing
- **Consistent Behavior**: Eliminates NAT traversal complexity from core network operations

### 2. Performance Optimization  
- **Predictable Latency**: Direct connections provide consistent performance characteristics
- **Resource Efficiency**: Eliminates redundant msg resending

### 3. Operational Simplicity
- **Auto-Configuration**: Automatically detects and configures optimal network settings
- **Clear Feedback**: Provides definitive reachability status with specific failure reasons
- **UPnP Integration**: Seamlessly handles automatic port mapping when available

### 4. Developer Benefits
- **Debugging Clarity**: Clear distinction between node faults and network configuration issues
- **Metrics Integration**: Comprehensive progress tracking and status reporting
- **Retry Logic**: Intelligent retry mechanisms for transient network conditions

## Implementation Details

### Timeout Configuration
- **Connection Timeout**: 30 seconds for initial connection establishment
- **Dial-Back Timeout**: 200 seconds (180s delay + 20s buffer) for complete dial-back cycle
- **Workflow Retry**: Maximum 3 attempts with exponential backoff

### Error Handling Hierarchy
1. **Retryable Errors**: No dial-backs received, insufficient dial-backs, multiple local adapters
2. **Terminal Errors**: No outbound connections, multiple external addresses, unspecified addresses
3. **Configuration Errors**: Invalid local adapter ports, missing peer IDs

### Progress Reporting
The module provides real-time progress updates across workflow attempts:
- **Per-Dial Progress**: Individual dial attempt state tracking
- **Workflow Progress**: Overall completion across retry attempts
- **Time-Based Calculation**: Progress based on actual state transition timing

## Summary

The reachability check module provides essential connectivity validation for the Autonomi Network. Through systematic testing with strict validation criteria, it ensures only nodes with reliable, direct internet connectivity can participate as full network members. This design prioritizes network health and performance over accommodating problematic connectivity scenarios, resulting in a more robust and efficient distributed network.

The module's sophisticated state machine, concurrent dial management, and strict consensus requirements work together to provide definitive reachability assessment, enabling antnode to make informed decisions about network participation and configuration.