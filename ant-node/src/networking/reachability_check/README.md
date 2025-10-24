# Reachability Check

This module tests whether a node can receive direct inbound connections from other peers on the network. If a node can't receive connections, it won't be able to participate in the DHT properly, so we prevent it from starting.

## How it works

The workflow tries one network listener at a time, with up to 3 attempts per listener:

1. Bind to a listener.
2. Dial out to 7 bootstrap peers concurrently
3. Each peer that connects dials us back after 180 seconds
4. If we get 4+ matching dial-backs, we're reachable
5. If it fails, retry up to 2 more times on the same listener
6. If all 3 attempts fail, move to the next listener and start over

The check runs for 3-10 minutes depending on network conditions and how many listeners need to be tried.

## State Machine

Each dial attempt goes through these states:

```rust
pub enum DialState {
    Initiated { at: Instant },      // We started dialing
    Connected { at: Instant },      // Connection established, waiting for dial-back
    DialBackReceived { at: Instant }, // Peer dialed us back successfully
}
```

We require peers to wait 180 seconds before dialing back. This is because NAT state tables need time to stabilize - if a peer dials back immediately, we might accept it through an existing connection hole rather than proving we can receive truly independent inbound connections.

## Why 7 dials and 4/7 majority?

`MAX_CONCURRENT_DIALS = 7` gives us fault tolerance - we can lose 3 peers to network issues or timeouts and still get a valid result. The majority check (`get_majority(7) = 4`) ensures we have statistical confidence in the external address we discovered.

```rust
pub fn get_majority(value: usize) -> usize {
    if value == 0 { 0 } else { (value / 2) + 1 }
}
```

## Address Validation

Once we collect dial-backs, we validate the observed addresses:

- All dial-backs must report the **same external address** - if peers see us at different IPs, something's wrong with our NAT setup
- All dial-backs must use the **same local adapter** - mixed adapters indicate routing issues
- The addresses can't be unspecified (0.0.0.0)
- The local port can't be zero

If any of these checks fail, we either retry the workflow or fail with a specific error.

## Module Structure

```
reachability_check/
├── mod.rs        # Main event loop and orchestration
├── dialer.rs     # Manages dial attempts and state transitions
├── listener.rs   # Discovers available network adapters and handles UPnP
└── progress.rs   # Calculates progress across workflow attempts
```

### Dialer (`dialer.rs`)

The `DialManager` handles:
- Fetching bootstrap addresses via `Bootstrap::next_addr()`
- Tracking up to 7 concurrent dial attempts
- Transitioning states based on connection events
- Cleaning up timed-out attempts
- Determining if the workflow is complete

It maintains two maps:
- `ongoing_dial_attempts`: Current state of active dials
- `all_dial_attempts`: Historical results across retry attempts

The `is_faulty()` check is used to distinguish between network issues (we connected but got no dial-backs) vs node issues (we couldn't connect at all).

### Listener (`listener.rs`)

The `ListenerManager`:
- Discovers all available network interfaces when created
- Attempts UPnP port mapping if not disabled
- Prioritizes UPnP-enabled adapters first (they're more likely to work)
- Binds to **one listener at a time** during the workflow
- Tracks which listeners failed and why

The workflow is sequential - we fully test one listener (up to 3 attempts) before moving to the next. This is better than testing all listeners simultaneously because:
- We can stop as soon as one works
- Each listener gets a fair chance with retries
- Error tracking is cleaner (we know exactly which adapter had which issue)

### Progress (`progress.rs`)

Provides time-based progress reporting for the UI. Progress is calculated by:
- Dividing 100% across the max workflow attempts (3)
- Within each workflow, tracking dial state progress
- Connected state progress is weighted by how long we've been waiting for dial-back

## Retry Logic

Each listener gets up to 3 attempts (`MAX_WORKFLOW_ATTEMPTS`). Some errors are retryable, others terminate immediately:

**Retryable errors** (will retry on same listener):
- No dial-backs received
- Not enough dial-backs (need 4, got fewer)

**Terminal errors** (skip to next listener or fail):
- No outbound connections possible
- Multiple external addresses observed (peers see us at different IPs)
- Multiple local adapter addresses observed (dial-backs on different interfaces)
- Unspecified external address (0.0.0.0)
- Unspecified local adapter address (0.0.0.0)
- Local adapter port is zero

### How retries work

**Same listener retry** (`increment_workflow()`):
- Increments attempt counter (1 → 2 → 3)
- Resets dial state but keeps `all_dial_attempts` history
- Re-initializes bootstrap peer list

**Next listener retry** (`reset_workflow_for_new_listener()`):
- Resets attempt counter back to 1
- Clears `all_dial_attempts` (fresh start for new listener)
- Re-initializes bootstrap peer list

If all listeners fail after 3 attempts each, the node terminates with a map of what went wrong on each listener.

## UPnP Integration

UPnP (Universal Plug and Play) automates port forwarding on routers, making nodes more likely to be reachable without manual configuration.

### How it works

When `ListenerManager` initializes (unless `--no-upnp` is specified):

1. **Discovery phase**: A temporary swarm with UPnP behavior enabled runs for up to 10 seconds
2. **UPnP events processed**:
   - `NewExternalAddr`: Router successfully created port mapping - we mark this listener as UPnP-supported
   - `GatewayNotFound`: No UPnP-capable router detected
   - `NonRoutableGateway`: Router found but it's not the internet gateway
   - `ExpiredExternalAddr`: Previous mapping expired (shouldn't happen during discovery)

3. **Prioritization**: Listeners are sorted - UPnP-enabled adapters are tried first since they're more likely to succeed

The `upnp_result_found` flag ensures we wait for the router to respond (or timeout) before proceeding. This prevents starting the reachability check before we know if UPnP worked.

### Why prioritize UPnP listeners?

Nodes behind NAT need port forwarding to receive inbound connections. UPnP listeners have already proven they can automatically set up port mappings, so they're much more likely to pass the reachability check. Non-UPnP listeners might still work if the user manually configured port forwarding, but they're lower priority.

### UPnP in the final result

The `ReachabilityStatus::Reachable` result includes a `upnp: bool` field indicating whether the working listener had UPnP support. This helps with debugging - if a node is reachable *without* UPnP, it means either:
- Manual port forwarding is configured
- The node has a public IP (no NAT)
- The firewall allows unsolicited inbound connections

### Disabling UPnP

Use `--no-upnp` flag to skip UPnP discovery entirely. Useful if:
- You have manual port forwarding configured
- UPnP is disabled/blocked on your router
- You want to test non-UPnP reachability specifically

## Bootstrap Address Management

Addresses come from the `ant-bootstrap` crate's `Bootstrap` struct, which handles:
- Reading from `ANT_PEERS` environment variable
- Command-line provided addresses
- Bootstrap cache on disk
- Network contacts fetched from URLs

The `Bootstrap` struct filters addresses internally - it excludes relay/circuit addresses and ensures peer IDs are present. The reachability check module just calls `next_addr()` and gets pre-validated addresses ready for direct dialing.

## Timeouts

```rust
TIMEOUT_ON_INITIATED_STATE = 30 seconds  // Connection establishment
TIMEOUT_ON_CONNECTED_STATE = 200 seconds // Dial-back wait (180s delay + 20s buffer)
```

If a peer doesn't dial back within 200 seconds, we mark it as `TimedOutAfterConnecting` and move on.

## Status Results

```rust
pub enum ReachabilityStatus {
    Reachable {
        local_addr: SocketAddr,   // Which adapter worked
        external_addr: SocketAddr, // Our public address
        upnp: bool,                // Whether UPnP was used
    },
    NotReachable {
        reasons: HashMap<(SocketAddr, bool), ReachabilityIssue>,
    },
}
```

`NotReachable` includes a map of all tested listeners and why each failed, which helps with debugging.

## Common Issues

### Retryable Issues (will retry up to 3 times)

**No dial-backs**: We connected to peers successfully but none of them could dial back. This indicates port forwarding issues, strict NAT, or firewall blocking inbound connections. The workflow will retry a few times in case it was a timing issue.

**Not enough dial-backs**: We got some dial-backs (say, 2 or 3) but need at least 4 for majority consensus. Could be transient network issues. The workflow retries automatically.

### Terminal Issues (move to next listener immediately)

**No outbound connections**: We couldn't connect to any bootstrap peers at all. This means either the node has no internet connectivity, the bootstrap peers are all down (unlikely), or there's a severe local firewall/network issue. No point retrying - we move to the next listener.

**Multiple external addresses**: Peers reported seeing us at different public IPs. This usually means complex NAT setup (multiple NAT layers, carrier-grade NAT, or VPN interference). We can't determine which address is "correct", so we fail. Fix: specify `--ip` to pick one specific adapter.

**Multiple local adapters**: Different dial-backs arrived on different network interfaces (e.g., some on eth0, some on eth1). Usually means routing issues or multiple active networks. Fix: use `--ip` to bind to a specific adapter.

**Unspecified addresses (0.0.0.0)**: Either our external address or local adapter came back as `0.0.0.0`. This shouldn't happen in normal operation - indicates a bug in address detection or extremely broken network configuration.

**Local adapter port is zero**: The local port came back as 0, which is invalid for binding. This is a configuration error or system issue.

## Why terminate unreachable nodes?

Nodes that can't receive direct connections degrade the DHT - they can't be used for routing, storage, or replication. Rather than letting them start up in a broken state, we fail fast with clear error messages so the user can fix their network config.
