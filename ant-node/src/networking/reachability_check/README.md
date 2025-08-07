# Reachability Check Module

## Overview

The reachability check determines whether a node in the Autonomi Network can be reached directly from the internet. This automated process ensures only nodes with reliable connectivity join the network, maintaining overall network health and performance.

### Why This Matters

In peer-to-peer networks, nodes behind NAT/firewalls often cannot receive direct connections. This module automatically detects such situations and determines if a node has the necessary connectivity to participate in the network.

**Key Benefits:**
- Automatic network configuration without manual intervention
- Ensures optimal performance by validating connectivity
- Maintains network stability by preventing unreachable nodes from joining
- Provides clear feedback about connectivity status

## How It Works

The reachability check determines connectivity by having peers dial back to this node after a 180-second delay, then correlating the external addresses they observe with local network adapters. The key insight is that consistent external address observation across multiple peers indicates reliable direct connectivity.

## Reachability Status

The module returns one of two statuses:

| Status | Description | Result |
|--------|-------------|---------|
| **Reachable** | Node can receive direct connections from the internet | Returns the local adapter address that is externally reachable. Node can fully participate in the network. |
| **NotRoutable** | Node cannot receive direct connections | Indicates NAT/firewall issues. Node should terminate as it cannot properly participate. |

Both statuses include UPnP support information for diagnostics.

## Key Technical Components

The module uses a sophisticated state machine tracking 7 concurrent dial attempts with 3-stage progression (Initiated → Connected → DialBackReceived), requiring a majority (4/7) of successful observations for validation.

### Critical Design: 180-Second Dial-Back Delay

The most crucial aspect is the 180-second delay before accepting identify responses:

```rust
fn transition_to_dial_back_received(&mut self, peer_id: &PeerId) {
    match self {
        DialState::Connected { at } => {
            if at.elapsed() > DIAL_BACK_DELAY {  // Must wait 180s
                *self = DialState::DialBackReceived { at: Instant::now() };
            }
        }
    }
}
```

This delay ensures peers have sufficient time to process our connection and respond with accurate external address observations, preventing false positives from immediate responses.

## Core Algorithms

### Bootstrap Peer Filtering

The module filters out circuit (relay) addresses since direct connectivity testing requires direct connections:

```rust
let initial_contacts: Vec<Multiaddr> = initial_contacts
    .into_iter()
    .filter(|addr| {
        !addr.iter().any(|protocol| matches!(protocol, Protocol::P2pCircuit))
    })
    .filter(|addr| {
        addr.iter().any(|protocol| matches!(protocol, Protocol::P2p(_)))
    })
    .collect();
```

### Address Consistency Validation

The core reachability logic enforces strict consistency - all peers must observe the same external address:

```rust
fn get_majority(value: usize) -> usize {
    if value == 0 { 0 } else { (value / 2) + 1 }  // Requires 4 out of 7
}

// Require consistent single external address across all observations
if external_addrs.len() != 1 {
    error!("Multiple external addresses observed. Terminating the node.");
    result.terminate = true;
}
```

**Why This Strictness Matters:**
- **Predictable NAT behavior**: Multiple addresses indicate problematic NAT configurations
- **Network stability**: Inconsistent connectivity patterns would cause DHT routing issues  
- **Simplified implementation**: Single address eliminates complex address selection logic

## Design Decisions

### Why Terminate Instead of Relay?

The implementation terminates unreachable nodes rather than using relay connections:

- **Network Health**: Only fully capable nodes participate, ensuring robust DHT performance
- **Performance**: Avoids relay overhead that would impact all network operations  
- **Simplicity**: Eliminates complex relay management and failure scenarios

### Fault Detection Strategy

The module distinguishes between network issues vs. node problems:

```rust
pub(crate) fn are_we_faulty(&self) -> bool {
    // Check if at least one dial attempt was successful
    let mut faulty = true;
    for dial_result in self.all_dial_attempts.values() {
        match dial_result {
            DialResult::TimedOutAfterConnecting => faulty = false,  // Connected but no dial-back
            DialResult::SuccessfulDialBack => faulty = false,       // Full success
            _ => {}
        }
    }
    faulty
}
```

If the node successfully connects to peers but they don't dial back, it indicates network connectivity issues rather than node faults. This distinction helps with retry vs. termination decisions.

## Summary

The reachability check provides essential connectivity assessment for the Autonomi Network. Through systematic testing and strict validation, it ensures only nodes with reliable, direct connectivity join the network. The module's design prioritizes network health and stability over accommodating problematic connectivity scenarios.

For additional support or questions, refer to the main project documentation or reach out to the development team.